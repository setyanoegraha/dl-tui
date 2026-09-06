//! Dashboard orchestration: session lifecycle, data fetching and the host
//! closures handed to the TUI event loop.

use anyhow::Result;

use crate::config::ConfigManager;
use crate::modules::certificates::CertificateManager;
use crate::modules::completed::CompletedManager;
use crate::modules::profile::ProfileFetcher;
use crate::modules::ratings::RatingManager;
use crate::modules::rankings::RankingFetcher;
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
        ActionKind::GenerateCert => {
            let machine = action.machine.clone();
            let manager = CertificateManager::new(sessions.session());
            // The platform only issues the certificate once the writeup is
            // published AND the machine is marked completed — explain that
            // instead of surfacing a raw server error.
            if !manager.available(&machine).await? {
                return Ok(ActionReport {
                    title: format!(" Certificado — {machine} "),
                    entries: vec![
                        (ReportKind::Failure, "Certificado: ✗ AÚN NO DISPONIBLE".to_string()),
                        (
                            ReportKind::Info,
                            "Requiere: 1) publicar tu writeup (w) y 2) marcar la máquina como completada (m).".to_string(),
                        ),
                    ],
                    changed: false,
                    status: format!(
                        "[!] Certificado de {machine} aún no disponible — falta el writeup o marcarla completada."
                    ),
                });
            }
            let pdf_url = manager.generate(&machine).await?;
            open_in_browser(&pdf_url);
            Ok(ActionReport {
                title: format!(" Certificado — {machine} "),
                entries: vec![
                    (ReportKind::Success, "Certificado: ✓ GENERADO".to_string()),
                    (ReportKind::Info, pdf_url),
                ],
                changed: false,
                status: format!("[✓] Certificado de {machine} generado — PDF abierto en el navegador."),
            })
        }
    }
}

fn open_in_browser(url: &str) {
    let _ = std::process::Command::new("xdg-open")
        .arg(url)
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

    let rankings = RankingFetcher::new(session.clone());
    let ranking_autores = rankings.autores().await.unwrap_or_default();
    let ranking_writeups = rankings.writeups().await.unwrap_or_default();

    Ok(TuiData {
        profile,
        catalog,
        ranking_autores,
        ranking_writeups,
    })
}
