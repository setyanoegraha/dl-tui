//! Dashboard orchestration: session lifecycle, data fetching and the host
//! closures handed to the TUI event loop.

use anyhow::{Context, Result};

use crate::config::ConfigManager;
use crate::modules::completed::CompletedManager;
use crate::modules::profile::ProfileFetcher;
use crate::modules::ratings::RatingManager;
use crate::modules::session::{login, login_with, DlSession};
use crate::modules::writeups::WriteupManager;
use crate::tui::{ActionKind, ActionReport, ReportKind, TuiAction, TuiData};

/// Reusable authenticated session for the TUI's lifetime. Cloning a
/// `DlSession` is cheap (shared connection pool), so one login serves
/// every background action.
#[derive(Clone)]
pub struct SessionCache {
    session: DlSession,
    username: String,
}

impl SessionCache {
    pub async fn new() -> Result<Self> {
        let cfg = ConfigManager::new();
        let (username, _) = cfg.load_credentials()?;
        let session = login(&cfg).await?;
        Ok(Self { session, username })
    }

    pub fn session(&self) -> DlSession {
        self.session.clone()
    }
}

/// Session slot shared by all TUI closures; `None` until the login popup
/// succeeds and after a logout.
type SharedSession = std::sync::Arc<std::sync::Mutex<Option<SessionCache>>>;

fn take_session(shared: &SharedSession) -> Result<SessionCache> {
    shared
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Sin sesión: no hay cuenta de DockerLabs configurada."))
}

pub async fn tui_cmd() -> Result<()> {
    // Bare `dl`: without usable stored credentials the TUI starts directly
    // in the login popup; otherwise log in now and enter the dashboard with
    // an empty state (the first fetch runs inside the event loop).
    let cfg = ConfigManager::new();
    let stored_username = cfg.stored_username();
    let sessions = if stored_username.is_some() {
        match SessionCache::new().await {
            Ok(sessions) => Some(sessions),
            Err(error)
                if error.chain().any(|cause| {
                    cause
                        .downcast_ref::<crate::modules::DlError>()
                        .is_some_and(|e| matches!(e, crate::modules::DlError::AuthFailed))
                }) =>
            {
                // Stale password: offer re-configuration inside the TUI.
                None
            }
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    let unconfigured = sessions.is_none();

    let shared: SharedSession = std::sync::Arc::new(std::sync::Mutex::new(sessions));
    let fetch_sessions = shared.clone();
    let action_sessions = shared.clone();
    let writeups_sessions = shared.clone();
    let ratings_sessions = shared.clone();
    let config_sessions = shared.clone();
    let logout_sessions = shared.clone();

    let initial = if unconfigured {
        crate::tui::AppState::unconfigured(stored_username.as_deref())
    } else {
        crate::tui::AppState::loading()
    };

    let refetch = move || {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let sessions = take_session(&fetch_sessions)?;
                fetch_tui_data(&sessions).await
            })
        })
    };
    let run_action = move |action: TuiAction| {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let sessions = take_session(&action_sessions)?;
                run_tui_action(&sessions, action).await
            })
        })
    };
    let run_writeups_fetch = move |machine: &str| {
        let machine = machine.to_string();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let sessions = take_session(&writeups_sessions)?;
                WriteupManager::new(sessions.session())
                    .fetch(&machine)
                    .await
            })
        })
    };
    let run_rating_fetch = move |machine: &str| {
        let machine = machine.to_string();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let sessions = take_session(&ratings_sessions)?;
                RatingManager::new(sessions.session()).fetch(&machine).await
            })
        })
    };
    let run_config = move |username: &str, password: &str| {
        let username = username.to_string();
        let password = password.to_string();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(configure_account(&config_sessions, &username, &password))
        })
    };
    let logout = move || {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(logout_account(&logout_sessions))
        })
    };

    let mut host = crate::tui::Host {
        refetch: &refetch,
        run_action: &run_action,
        run_writeups_fetch: &run_writeups_fetch,
        run_rating_fetch: &run_rating_fetch,
        run_config: &run_config,
        logout: &logout,
        pending_fetch: false,
    };

    crate::tui::run(initial, &mut host)
}

/// Validates the entered credentials by logging in, stores them, and
/// installs the session for the rest of the dashboard lifetime.
async fn configure_account(shared: &SharedSession, username: &str, password: &str) -> Result<()> {
    let session = login_with(username, password).await?;
    ConfigManager::new().save_credentials(username, password)?;
    *shared.lock().unwrap() = Some(SessionCache {
        session,
        username: username.to_string(),
    });
    Ok(())
}

/// Removes the stored account and drops the in-memory session. Called from
/// the account popup (`l`). Running downloads are unaffected — the download
/// pages are public.
async fn logout_account(shared: &SharedSession) -> Result<()> {
    ConfigManager::new().clear_credentials()?;
    *shared.lock().unwrap() = None;
    Ok(())
}

/// Executes a user action from a popup; returns the result popup content.
async fn run_tui_action(sessions: &SessionCache, action: TuiAction) -> Result<ActionReport> {
    match action.kind {
        ActionKind::ToggleCompleted => {
            let machine = action.machine.clone();
            let completed = CompletedManager::new(sessions.session())
                .toggle(&machine)
                .await?;
            let (kind, entry, status) = if completed {
                (
                    ReportKind::Success,
                    format!("{machine}: ✓ marcada como completada"),
                    format!("[✓] {machine} marcada como completada."),
                )
            } else {
                (
                    ReportKind::Info,
                    format!("{machine}: desmarcada"),
                    format!("[·] {machine} desmarcada."),
                )
            };
            Ok(ActionReport {
                title: format!(" Estado — {machine} "),
                entries: vec![(kind, entry)],
                changed: true,
                status,
            })
        }
        ActionKind::SubmitWriteup => {
            let machine = action.machine.clone();
            let url = action.values[0].1.clone();
            let tipo = action.values[1].1.clone();
            WriteupManager::new(sessions.session())
                .submit(&machine, &url, &tipo)
                .await?;
            Ok(ActionReport {
                title: format!(" Writeup enviado — {machine} "),
                entries: vec![(
                    ReportKind::Success,
                    format!("Writeup: ✓ ENVIADO — {url}"),
                )],
                changed: true,
                status: format!("[✓] Writeup enviado para {machine}!"),
            })
        }
        ActionKind::SubmitRating => {
            let machine = action.machine.clone();
            let mut scores = [0u64; 4];
            for (index, (_, value)) in action.values.iter().enumerate() {
                scores[index] = value.parse().unwrap_or(0);
            }
            RatingManager::new(sessions.session())
                .submit(&machine, scores[0], scores[1], scores[2], scores[3])
                .await?;
            Ok(ActionReport {
                title: format!(" Valoración enviada — {machine} "),
                entries: vec![(
                    ReportKind::Success,
                    format!(
                        "Valoración: ✓ ENVIADA ({}-{}-{}-{})",
                        scores[0], scores[1], scores[2], scores[3]
                    ),
                )],
                changed: false,
                status: format!("[✓] ¡Valoración enviada para {machine}!"),
            })
        }
        ActionKind::DownloadAllCerts => {
            // Batch: fetch the profile, then download every issued
            // certificate into <download_dir>/certificados.
            let profile = ProfileFetcher::new(sessions.session())
                .fetch(&sessions.username)
                .await?;
            let certs: Vec<(String, String, String)> = profile
                .maquinas_hechas
                .iter()
                .filter_map(|f| {
                    f.certificado
                        .as_ref()
                        .map(|c| (f.nombre.clone(), c.cert_id.clone(), c.pdf_url.clone()))
                })
                .collect();
            if certs.is_empty() {
                anyhow::bail!("Todavía no hay certificados emitidos.");
            }

            let dest_dir = ConfigManager::new()
                .download_dir()
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
                .join("certificados");
            std::fs::create_dir_all(&dest_dir)
                .with_context(|| format!("No se pudo crear {}", dest_dir.display()))?;

            let client = reqwest::Client::builder()
                .user_agent(concat!("dl-tui/", env!("CARGO_PKG_VERSION")))
                .timeout(std::time::Duration::from_secs(120))
                .build()?;
            let mut downloaded = 0u64;
            let mut skipped = 0u64;
            let mut failed: Vec<String> = Vec::new();
            for (machine, cert_id, pdf_url) in &certs {
                let safe_machine: String = machine
                    .chars()
                    .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
                    .collect();
                let path = dest_dir.join(format!("certificado-{safe_machine}-{cert_id}.pdf"));
                if path.exists() {
                    skipped += 1;
                    continue;
                }
                let absolute = if pdf_url.starts_with("http") {
                    pdf_url.clone()
                } else {
                    format!("https://dockerlabs.es{pdf_url}")
                };
                match client
                    .get(&absolute)
                    .send()
                    .await
                    .and_then(|r| r.error_for_status())
                {
                    Ok(resp) => match resp.bytes().await {
                        Ok(bytes) => {
                            if std::fs::write(&path, &bytes).is_ok() {
                                downloaded += 1;
                                continue;
                            }
                            failed.push(format!("{machine}: no se pudo escribir"));
                        }
                        Err(e) => failed.push(format!("{machine}: {e}")),
                    },
                    Err(e) => failed.push(format!("{machine}: {e}")),
                }
            }
            open_in_browser(&dest_dir);
            let mut entries = vec![(
                ReportKind::Success,
                format!(
                    "✓ {downloaded} descargados · {skipped} ya existían · {} total",
                    certs.len()
                ),
            )];
            for failure in failed.iter().take(3) {
                entries.push((ReportKind::Failure, failure.clone()));
            }
            let status = if failed.is_empty() {
                format!(
                    "[✓] {} certificados en {} (descargados: {downloaded}, ya había: {skipped}).",
                    certs.len(),
                    crate::tui::downloads::shorten_path(&dest_dir)
                )
            } else {
                format!("[!] {downloaded} descargados, {} fallos — revisa el popup.", failed.len())
            };
            Ok(ActionReport {
                title: " Certificados ".to_string(),
                entries,
                changed: false,
                status,
            })
        }
        ActionKind::DownloadCert => {
            let machine = action.machine.clone();
            let cert_id = action.values[0].1.clone();
            let pdf_url = action.values[1].1.clone();

            // Certificates are admin-issued; here we only fetch the issued
            // PDF into the download folder and open the local copy.
            let cfg = ConfigManager::new();
            let dest_dir = cfg
                .download_dir()
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
                .join("certificados");
            std::fs::create_dir_all(&dest_dir)
                .with_context(|| format!("No se pudo crear {}", dest_dir.display()))?;
            let safe_machine: String = machine
                .chars()
                .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
                .collect();
            let filename = format!("certificado-{safe_machine}-{cert_id}.pdf");
            let path = dest_dir.join(&filename);
            if path.exists() {
                open_in_browser(&path);
                return Ok(ActionReport {
                    title: format!(" Certificado — {machine} "),
                    entries: vec![(
                        ReportKind::Success,
                        format!("Certificado {cert_id}: ya descargado — {}", path.display()),
                    )],
                    changed: false,
                    status: format!("[✓] Certificado de {machine} ya estaba descargado — reabierto."),
                });
            }

            let client = reqwest::Client::builder()
                .user_agent(concat!("dl-tui/", env!("CARGO_PKG_VERSION")))
                .timeout(std::time::Duration::from_secs(120))
                .build()?;
            // The profile may carry the PDF as a relative path.
            let absolute = if pdf_url.starts_with("http") {
                pdf_url.clone()
            } else if pdf_url.starts_with('/') {
                format!("https://dockerlabs.es{pdf_url}")
            } else {
                format!("https://dockerlabs.es/{pdf_url}")
            };
            let bytes = client
                .get(&absolute)
                .send()
                .await
                .context("Error de conexión al bajar el certificado")?
                .error_for_status()
                .context("El servidor devolvió un error al bajar el certificado")?
                .bytes()
                .await?;
            std::fs::write(&path, &bytes)
                .with_context(|| format!("No se pudo escribir {}", path.display()))?;
            open_in_browser(&path);
            Ok(ActionReport {
                title: format!(" Certificado — {machine} "),
                entries: vec![
                    (
                        ReportKind::Success,
                        format!("Certificado {cert_id}: ✓ DESCARGADO"),
                    ),
                    (ReportKind::Info, path.display().to_string()),
                ],
                changed: false,
                status: format!(
                    "[✓] Certificado de {machine} descargado ({}).",
                    crate::tui::downloads::fmt_bytes(bytes.len() as u64)
                ),
            })
        }
    }
}

/// Opens a local file or folder with the system opener.
fn open_in_browser(path: &std::path::Path) {
    let _ = std::process::Command::new("xdg-open")
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Fetches every dataset the dashboard shows: the user's profile, the full
/// machine catalog and both rankings. Rankings are nice-to-have: failures
/// degrade to empty tables instead of blanking the dashboard.
async fn fetch_tui_data(sessions: &SessionCache) -> Result<TuiData> {
    let session = sessions.session();

    let profile = ProfileFetcher::new(session.clone())
        .fetch(&sessions.username)
        .await?;
    let catalog = crate::modules::machines::MachineScraper::new(session.clone())
        .get_catalog()
        .await?;

    // Author ranking position: fetch the full list once and find our
    // username instead of displaying every entry.
    let ranking_creador: Option<u64> = async {
        let body = session.get("/api/ranking_autores").await.ok()?;
        let list: serde_json::Value = serde_json::from_str(&body).ok()?;
        let arr = list.as_array()?;
        let position = arr.iter().position(|author| {
            author
                .get("nombre")
                .and_then(serde_json::Value::as_str)
                .map(|name| name.trim().eq_ignore_ascii_case(&sessions.username))
                .unwrap_or(false)
        })?;
        Some(position as u64 + 1)
    }
    .await;

    Ok(TuiData {
        profile,
        catalog,
        ranking_creador,
    })
}
