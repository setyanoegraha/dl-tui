//! Interactive dashboard: application state, input handling and the event
//! loop. Rendering lives in `render.rs`; all state transitions here are pure
//! and unit-tested. UI text is in Spanish, like the original platform.

pub mod downloads;
pub mod render;

use std::time::Duration;

use anyhow::Result;
use std::path::PathBuf;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::modules::machines::Machine;
use crate::modules::profile::Profile;
use crate::modules::writeups::WriteupEntry;

/// What a popup asks the user for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupKind {
    /// Download destination input (Máquinas).
    Descarga,
    /// Certificate PDF destination input (`c`/`C`).
    CertDest,
    /// Writeup URL + tipo input (inside the writeups popup).
    WriteupSubmit,
    /// Four 1-5 score fields.
    RatingSubmit,
    /// Login credentials popup (first run, stale password, account switch).
    Config,
    /// Account overview (`a`): switch (`Enter`) or logout (`l`).
    Account,
    /// Read-only machine description (`i` / Enter on Máquinas).
    Descripcion,
    /// Read-only rating averages (`v`); `Enter` opens RatingSubmit.
    Valoracion,
}

/// Why the login popup was opened — drives its yellow notice line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigContext {
    FirstRun,
    LoginFailed,
    Switch,
    LoggedOut,
}

impl ConfigContext {
    fn notice(self) -> &'static str {
        match self {
            ConfigContext::FirstRun => "Primera ejecución: introduce tu cuenta de DockerLabs.",
            ConfigContext::LoginFailed => "Inicio de sesión fallido: vuelve a intentarlo.",
            ConfigContext::Switch => "Cambiar cuenta: introduce las nuevas credenciales.",
            ConfigContext::LoggedOut => "Sesión cerrada: inicia sesión de nuevo.",
        }
    }
}

/// A popup bound to one machine. Input popups carry `buffers`; info popups
/// (Account/Descripcion/Valoracion) are read-only and render `text`.
#[derive(Debug, Clone)]
pub struct Popup {
    pub kind: PopupKind,
    pub machine: String,
    /// Machine id for the download popup (0 when not applicable).
    pub machine_id: u32,
    pub buffers: Vec<String>,
    pub field: usize,
    pub notice: Option<String>,
    pub readonly: bool,
    /// Body text for read-only popups (descripción / valoración).
    pub text: Option<String>,
    /// Path-completion candidates for the Descarga popup (Tab).
    pub completions: Vec<String>,
    /// (cert_id, pdf_url) for the certificate destination popup.
    pub cert: Option<(String, String)>,
}

impl Popup {
    pub fn push(&mut self, c: char) {
        if let Some(buffer) = self.buffers.get_mut(self.field) {
            buffer.push(c);
        }
        // Typing invalidates a previous completion listing.
        self.completions.clear();
    }

    pub fn pop(&mut self) {
        if let Some(buffer) = self.buffers.get_mut(self.field) {
            buffer.pop();
        }
        self.completions.clear();
    }

    pub fn next_field(&mut self) {
        if self.buffers.len() > 1 {
            self.field = (self.field + 1) % self.buffers.len();
        }
    }

    pub fn previous_field(&mut self) {
        if self.buffers.len() > 1 {
            self.field = (self.field + self.buffers.len() - 1) % self.buffers.len();
        }
    }

    /// zsh-style destination completion for the Descarga popup: `Tab`
    /// expands `~`, completes the last path component against the parent
    /// directory's subdirectories (common prefix first) and stores the
    /// candidate list so the popup can display it.
    pub fn complete_destination(&mut self) {
        let Some(buffer) = self.buffers.get_mut(0) else {
            return;
        };
        let raw = buffer.clone();
        let expanded = expand_tilde(&raw);
        let ends_with_sep = raw.ends_with('/');
        // A trailing separator means "complete inside this directory": the
        // partial component is empty even though Path::file_name would
        // still report one.
        let (parent, partial) = if ends_with_sep {
            (expanded.clone(), String::new())
        } else {
            split_parent_partial(&expanded)
        };

        let mut matches: Vec<String> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&parent) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !partial.is_empty() && !name.starts_with(partial.as_str()) {
                    continue;
                }
                // Skip hidden dirs unless the user typed the dot herself.
                if name.starts_with('.') && (partial.is_empty() || !partial.starts_with('.')) {
                    continue;
                }
                if entry.path().is_dir() {
                    matches.push(name);
                }
            }
        }
        matches.sort();
        if matches.is_empty() {
            self.completions.clear();
            return;
        }

        let completed = common_prefix(&matches);
        // Replace the partial component with the completed prefix, keeping
        // the original `~` spelling the user typed.
        let mut new_raw = raw[..raw.len().saturating_sub(partial.chars().count())].to_string();
        new_raw.push_str(&completed);
        if matches.len() == 1 && completed == matches[0] && !ends_with_sep {
            new_raw.push(std::path::MAIN_SEPARATOR);
        }
        *buffer = new_raw;
        self.completions = matches;
    }
}

/// `~` and `~/...` expand to the user's home directory.
fn expand_tilde(raw: &str) -> PathBuf {
    if raw == "~" {
        return home::home_dir().unwrap_or_else(|| PathBuf::from(raw));
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = home::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(raw)
}

/// Splits an expanded path into (parent directory, last component); paths
/// ending in a separator (or the filesystem root) complete with no partial.
fn split_parent_partial(expanded: &std::path::Path) -> (PathBuf, String) {
    if expanded.as_os_str().is_empty() {
        return (PathBuf::from("."), String::new());
    }
    match (expanded.parent(), expanded.file_name()) {
        (Some(parent), Some(name)) => (parent.to_path_buf(), name.to_string_lossy().to_string()),
        _ => (expanded.to_path_buf(), String::new()),
    }
}

/// Longest prefix shared by every candidate.
fn common_prefix(items: &[String]) -> String {
    let mut prefix = items[0].clone();
    for item in &items[1..] {
        while !item.starts_with(&prefix) {
            prefix.pop();
            if prefix.is_empty() {
                return prefix;
            }
        }
    }
    prefix
}

/// A user action queued from a popup, executed by the host application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    ToggleCompleted,
    SubmitWriteup,
    SubmitRating,
    DownloadCert,
    DownloadAllCerts,
}

#[derive(Debug, Clone)]
pub struct TuiAction {
    pub kind: ActionKind,
    pub machine: String,
    pub values: Vec<(usize, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct TuiData {
    pub profile: Profile,
    pub catalog: Vec<Machine>,
    /// Position of our username in /api/ranking_autores (creators).
    pub ranking_creador: Option<u64>,
}

impl TuiData {
    /// Lowercased names of the machines the user marked as completed.
    pub fn completed_names(&self) -> std::collections::HashSet<String> {
        self.profile
            .maquinas_hechas
            .iter()
            .map(|m| m.nombre.trim().to_lowercase())
            .collect()
    }

    /// Lowercased names of the machines with a writeup published by the user.
    pub fn writeup_published_names(&self) -> std::collections::HashSet<String> {
        self.profile
            .writeups
            .iter()
            .map(|w| w.maquina.trim().to_lowercase())
            .collect()
    }

    /// What stands between the user and a machine's certificate. Certificates
    /// are issued manually by the DockerLabs admins once the writeup is
    /// validated and the machine is marked completed.
    pub fn cert_state(&self, machine: &str) -> CertState {
        let name = machine.trim().to_lowercase();
        if let Some(ficha) = self
            .profile
            .maquinas_hechas
            .iter()
            .find(|m| m.nombre.trim().to_lowercase() == name)
        {
            if let Some(cert) = &ficha.certificado {
                return CertState::Ready {
                    cert_id: cert.cert_id.clone(),
                    pdf_url: cert.pdf_url.clone(),
                };
            }
        }
        let completed = self.completed_names().contains(&name);
        let writeup = self.writeup_published_names().contains(&name);
        match (completed, writeup) {
            (true, true) => CertState::PendingAdmin,
            (true, false) => CertState::MissingWriteup,
            (false, true) => CertState::MissingCompleted,
            (false, false) => CertState::MissingBoth,
        }
    }
}

/// How far the selected machine is from a certificate.
#[derive(Debug, Clone, PartialEq)]
pub enum CertState {
    /// Admin-issued: the PDF can be downloaded.
    Ready { cert_id: String, pdf_url: String },
    /// Machine completed, but no writeup published yet.
    MissingWriteup,
    /// Writeup published, but the machine is not marked completed.
    MissingCompleted,
    /// Neither step done.
    MissingBoth,
    /// Both steps done — waiting for an admin to issue the certificate.
    PendingAdmin,
}

/// One line of an action result popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportKind {
    Success,
    Failure,
    Info,
}

/// Shown after an action; persists until dismissed. If `changed` is true, a
/// data refresh is queued for when the user closes it.
#[derive(Debug, Clone)]
pub struct ActionReport {
    pub title: String,
    pub entries: Vec<(ReportKind, String)>,
    pub changed: bool,
    pub status: String,
}

/// Community writeups of one machine (`w`), rendered as a table popup.
#[derive(Debug, Clone)]
pub struct WriteupsPopup {
    pub machine: String,
    pub entries: Vec<WriteupEntry>,
    pub selected: usize,
}

impl WriteupsPopup {
    pub fn move_selection(&mut self, delta: isize) {
        let last = self.entries.len().saturating_sub(1);
        self.selected = self.selected.saturating_add_signed(delta).min(last);
    }

    pub fn selected_url(&self) -> Option<&str> {
        self.entries.get(self.selected).map(|w| w.url.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Maquinas,
    Progreso,
    Writeups,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Maquinas, Tab::Progreso, Tab::Writeups];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Maquinas => "Máquinas",
            Tab::Progreso => "Progreso",
            Tab::Writeups => "Writeups",
        }
    }

    fn next(self) -> Self {
        let index = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    fn previous(self) -> Self {
        let index = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        let last = Self::ALL.len() - 1;
        Self::ALL[(index + last) % Self::ALL.len()]
    }
}

/// Sort order of the Máquinas tab (`s` cycles: sitio -> nombre -> fecha ->
/// dificultad).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MachineSort {
    #[default]
    Sitio,
    Nombre,
    Fecha,
    Dificultad,
}

impl MachineSort {
    fn next(self) -> Self {
        match self {
            MachineSort::Sitio => MachineSort::Nombre,
            MachineSort::Nombre => MachineSort::Fecha,
            MachineSort::Fecha => MachineSort::Dificultad,
            MachineSort::Dificultad => MachineSort::Sitio,
        }
    }

    pub fn indicator(self) -> &'static str {
        match self {
            MachineSort::Sitio => "",
            MachineSort::Nombre => " · orden: nombre",
            MachineSort::Fecha => " · orden: fecha",
            MachineSort::Dificultad => " · orden: dificultad",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Filter,
}

/// Overlay listing background download jobs (`o` toggles it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Normal,
    Downloads,
}

pub struct AppState {
    pub tab: Tab,
    pub input_mode: InputMode,
    pub view: ViewMode,
    pub needs_config: bool,
    pub pending_logout: bool,
    pub filter: String,
    pub selected: usize,
    pub scroll: usize,
    pub machine_sort: MachineSort,
    /// Progreso toggle (`p`): show only completed machines missing a writeup.
    pub only_pending_writeup: bool,
    pub quit_warned: bool,
    pub quit: bool,
    pub refresh_requested: bool,
    pub fetching: Option<String>,
    pub status: Option<String>,
    pub status_expiry: Option<std::time::Instant>,
    pub popup: Option<Popup>,
    pub pending_action: Option<TuiAction>,
    pub pending_download: Option<(String, u32, PathBuf)>,
    pub pending_config: Option<(String, String)>,
    pub pending_writeups: Option<String>,
    pub pending_rating: Option<String>,
    pub download_queue: std::collections::VecDeque<(String, u32, PathBuf)>,
    pub download_jobs: Vec<std::sync::Arc<downloads::DownloadJob>>,
    pub report: Option<ActionReport>,
    pub writeups_popup: Option<WriteupsPopup>,
    pub pending_refresh_after_close: bool,
    pub data: TuiData,
    pub last_visible_rows: Option<usize>,
}

/// How long a status message stays visible in the footer.
const STATUS_LIFETIME: Duration = Duration::from_secs(5);

impl AppState {
    pub fn new(data: TuiData) -> Self {
        Self {
            tab: Tab::Maquinas,
            input_mode: InputMode::Normal,
            view: ViewMode::Normal,
            needs_config: false,
            pending_logout: false,
            filter: String::new(),
            selected: 0,
            scroll: 0,
            machine_sort: MachineSort::default(),
            only_pending_writeup: false,
            quit_warned: false,
            quit: false,
            refresh_requested: false,
            fetching: None,
            status: None,
            status_expiry: None,
            popup: None,
            pending_action: None,
            pending_download: None,
            pending_config: None,
            pending_writeups: None,
            pending_rating: None,
            download_queue: std::collections::VecDeque::new(),
            download_jobs: Vec::new(),
            report: None,
            writeups_popup: None,
            pending_refresh_after_close: false,
            data,
            last_visible_rows: None,
        }
    }

    /// Entry state for `dl`: draws immediately, then loads all data.
    pub fn loading() -> Self {
        let mut state = Self::new(TuiData::default());
        state.fetching = Some("Cargando datos...".to_string());
        state
    }

    /// Entry state for bare `dl` with no usable stored credentials: opens
    /// straight into the login popup.
    pub fn unconfigured(stored_username: Option<&str>) -> Self {
        let mut state = Self::new(TuiData::default());
        state.needs_config = true;
        let context = if stored_username.is_some() {
            ConfigContext::LoginFailed
        } else {
            ConfigContext::FirstRun
        };
        state.open_config_popup(context, stored_username);
        state
    }

    /// Number of background downloads still running.
    pub fn active_downloads(&self) -> usize {
        self.download_jobs.iter().filter(|job| job.is_active()).count()
    }

    /// Toggles the downloads overlay; harmless while popups are open.
    pub fn toggle_downloads_view(&mut self) {
        if self.popup.is_none() && self.report.is_none() {
            self.view = match self.view {
                ViewMode::Normal => ViewMode::Downloads,
                ViewMode::Downloads => ViewMode::Normal,
            };
        }
    }

    /// Index of the newest active download (cancel target in the overlay).
    pub fn download_selected(&self) -> usize {
        self.download_jobs
            .iter()
            .rposition(|job| job.is_active())
            .unwrap_or(self.download_jobs.len().saturating_sub(1))
    }

    /// First `q` with active downloads warns instead of quitting.
    pub fn request_quit(&mut self) {
        let active = self.active_downloads();
        if active > 0 && !self.quit_warned {
            self.quit_warned = true;
            let jobs: Vec<String> = self
                .download_jobs
                .iter()
                .filter(|job| job.is_active())
                .map(|job| {
                    let state = job.state.lock().unwrap();
                    let pct = state
                        .downloaded
                        .checked_mul(100)
                        .and_then(|pct| pct.checked_div(state.total))
                        .map(|pct| format!(" {}%", pct))
                        .unwrap_or_default();
                    format!("↓ {}{pct}", job.machine)
                })
                .collect();
            self.set_status(format!(
                "{active} descarga(s) activa(s) — pulsa q otra vez para abortar: {}",
                jobs.join(" · ")
            ));
            return;
        }
        self.quit = true;
    }

    /// Shows a status message in the footer, auto-expiring after 5 seconds.
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status = Some(message.into());
        self.status_expiry = Some(std::time::Instant::now() + STATUS_LIFETIME);
    }

    /// Clears expired status messages; called once per event-loop iteration.
    pub fn tick(&mut self) {
        if let Some(expiry) = self.status_expiry {
            if std::time::Instant::now() >= expiry {
                self.status = None;
                self.status_expiry = None;
            }
        }
    }

    pub fn set_data(&mut self, data: TuiData) {
        self.data = data;
        self.selected = 0;
        self.scroll = 0;
    }

    pub fn next_tab(&mut self) {
        self.tab = self.tab.next();
        self.reset_list_position();
    }

    pub fn previous_tab(&mut self) {
        self.tab = self.tab.previous();
        self.reset_list_position();
    }

    fn reset_list_position(&mut self) {
        self.selected = 0;
        self.scroll = 0;
    }

    /// Filtered catalog for the Máquinas tab; filter matches name,
    /// difficulty, category or author; `machine_sort` orders the result.
    pub fn visible_machines(&self) -> Vec<&Machine> {
        let needle = self.filter.to_lowercase();
        let mut machines: Vec<&Machine> = self
            .data
            .catalog
            .iter()
            .filter(|m| {
                needle.is_empty()
                    || m.name.to_lowercase().contains(&needle)
                    || m.dificultad.to_lowercase().contains(&needle)
                    || m.categoria.to_lowercase().contains(&needle)
                    || m.autor.to_lowercase().contains(&needle)
            })
            .collect();
        match self.machine_sort {
            MachineSort::Sitio => {}
            MachineSort::Nombre => machines.sort_by_key(|m| m.name.to_lowercase()),
            MachineSort::Fecha => machines.sort_by_key(|m| {
                std::cmp::Reverse(crate::modules::machines::fecha_sort_key(&m.fecha))
            }),
            MachineSort::Dificultad => machines.sort_by_key(|m| {
                crate::modules::machines::dificultad_rank(&m.dificultad)
            }),
        }
        machines
    }

    /// Filtered "machines done" list for the Progreso tab. When
    /// `only_pending_writeup` is on, only completed machines whose writeup
    /// has not been published yet are shown.
    pub fn visible_hechas(&self) -> Vec<&crate::modules::profile::MaquinaFicha> {
        let needle = self.filter.to_lowercase();
        let published = self.data.writeup_published_names();
        self.data
            .profile
            .maquinas_hechas
            .iter()
            .filter(|m| {
                (needle.is_empty() || m.nombre.to_lowercase().contains(&needle))
                    && (!self.only_pending_writeup
                        || !published.contains(&m.nombre.trim().to_lowercase()))
            })
            .collect()
    }

    /// How many completed machines still lack a published writeup.
    pub fn pending_writeup_count(&self) -> usize {
        let published = self.data.writeup_published_names();
        self.data
            .profile
            .maquinas_hechas
            .iter()
            .filter(|m| !published.contains(&m.nombre.trim().to_lowercase()))
            .count()
    }

    /// Toggles the "only pending writeups" view (`p`, Progreso).
    pub fn toggle_pending_writeup(&mut self) {
        if self.tab != Tab::Progreso {
            self.set_status("El filtro de writeups pendientes está en Progreso.");
            return;
        }
        self.only_pending_writeup = !self.only_pending_writeup;
        self.reset_list_position();
    }

    /// Filtered list of the user's published writeups (Writeups tab).
    pub fn visible_own_writeups(&self) -> Vec<&crate::modules::profile::WriteupRef> {
        let needle = self.filter.to_lowercase();
        self.data
            .profile
            .writeups
            .iter()
            .filter(|w| {
                needle.is_empty()
                    || w.maquina.to_lowercase().contains(&needle)
                    || w.url.to_lowercase().contains(&needle)
                    || w.tipo.to_lowercase().contains(&needle)
            })
            .collect()
    }

    /// URL of the selected own writeup (Writeups tab).
    pub fn selected_own_writeup_url(&self) -> Option<&str> {
        if self.tab != Tab::Writeups {
            return None;
        }
        self.visible_own_writeups()
            .get(self.selected)
            .map(|w| w.url.as_str())
    }

    pub fn open_selected_own_writeup(&mut self) {
        if let Some(url) = self.selected_own_writeup_url() {
            let opened = std::process::Command::new("xdg-open")
                .arg(url)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            self.set_status(match opened {
                Ok(_) => format!("Abierto en el navegador: {url}"),
                Err(error) => format!("xdg-open falló: {error}"),
            });
        }
    }

    /// Dismisses the result popup; returns true if a refresh was queued.
    pub fn close_report(&mut self) -> bool {
        self.report = None;
        std::mem::take(&mut self.pending_refresh_after_close)
    }

    /// Name of the machine under the selection on machine-centric tabs.
    pub fn selected_machine_name(&self) -> Option<String> {
        match self.tab {
            Tab::Maquinas => self
                .visible_machines()
                .get(self.selected)
                .map(|m| m.name.clone()),
            Tab::Progreso => self
                .visible_hechas()
                .get(self.selected)
                .map(|m| m.nombre.clone()),
            Tab::Writeups => None,
        }
    }

    /// The full machine under the selection (Máquinas tab only).
    pub fn selected_machine(&self) -> Option<&Machine> {
        if self.tab != Tab::Maquinas {
            return None;
        }
        self.visible_machines().get(self.selected).copied()
    }

    /// Opens the login popup for the given reason, optionally prefilled
    /// with a username.
    pub fn open_config_popup(&mut self, context: ConfigContext, username: Option<&str>) {
        if self.popup.is_some() {
            return;
        }
        self.popup = Some(Popup {
            kind: PopupKind::Config,
            machine: String::new(),
            machine_id: 0,
            buffers: vec![username.unwrap_or_default().to_string(), String::new()],
            field: 0,
            notice: Some(context.notice().to_string()),
            readonly: false,
            text: None,
            completions: Vec::new(),
            cert: None,
        });
    }

    /// Opens the account popup for the active session (`a`).
    pub fn open_account_popup(&mut self) {
        if self.needs_config
            || self.popup.is_some()
            || self.report.is_some()
            || self.writeups_popup.is_some()
        {
            return;
        }
        self.popup = Some(Popup {
            kind: PopupKind::Account,
            machine: self.data.profile.username.clone(),
            machine_id: 0,
            buffers: Vec::new(),
            field: 0,
            notice: None,
            readonly: true,
            text: None,
            completions: Vec::new(),
            cert: None,
        });
    }

    /// Enter on the account popup: close it and open the login popup
    /// prefilled with the current username for an account switch.
    pub fn begin_account_switch(&mut self) {
        let username = self.data.profile.username.clone();
        self.popup = None;
        self.open_config_popup(ConfigContext::Switch, Some(&username));
    }

    /// Read-only description popup for the selected machine (Máquinas).
    pub fn open_descripcion_popup(&mut self) {
        if self.popup.is_some() || self.report.is_some() || self.writeups_popup.is_some() {
            return;
        }
        let Some(machine) = self.selected_machine() else {
            self.set_status("Nada seleccionado.");
            return;
        };
        let meta = format!(
            "Dificultad: {} · Categoría: {}\nAutor: {} · Fecha: {} · ID: {}",
            machine.dificultad, machine.categoria, machine.autor, machine.fecha, machine.id
        );
        let body = if machine.descripcion.is_empty() {
            "Sin descripción.".to_string()
        } else {
            machine.descripcion.clone()
        };
        self.popup = Some(Popup {
            kind: PopupKind::Descripcion,
            machine: machine.name.clone(),
            machine_id: machine.id,
            buffers: Vec::new(),
            field: 0,
            notice: None,
            readonly: true,
            text: Some(format!("{meta}\n\n{body}")),
            completions: Vec::new(),
            cert: None,
        });
    }

    /// Queues a writeups fetch for the selected machine (`w`, Máquinas).
    pub fn open_writeups_popup(&mut self) {
        if self.writeups_popup.is_some() || self.popup.is_some() || self.report.is_some() {
            return;
        }
        if self.tab != Tab::Maquinas {
            self.set_status("Los writeups están disponibles en la pestaña Máquinas.");
            return;
        }
        let Some(machine) = self.selected_machine_name() else {
            self.set_status("Nada seleccionado.");
            return;
        };
        self.pending_writeups = Some(machine);
    }

    /// Queues a rating fetch for the selected machine (`v`, Máquinas); the
    /// event loop opens the Valoración popup on success.
    pub fn open_rating_popup(&mut self) {
        if self.popup.is_some() || self.report.is_some() || self.writeups_popup.is_some() {
            return;
        }
        if self.tab != Tab::Maquinas {
            self.set_status("Las valoraciones están disponibles en la pestaña Máquinas.");
            return;
        }
        let Some(machine) = self.selected_machine_name() else {
            self.set_status("Nada seleccionado.");
            return;
        };
        self.pending_rating = Some(machine);
    }

    /// Opens the writeup submission input popup from the writeups popup.
    pub fn open_writeup_submit_popup(&mut self) {
        let Some(popup) = self.writeups_popup.as_ref() else {
            return;
        };
        let machine = popup.machine.clone();
        self.writeups_popup = None;
        self.popup = Some(Popup {
            kind: PopupKind::WriteupSubmit,
            machine,
            machine_id: 0,
            buffers: vec![String::new(), "📝".to_string()],
            field: 0,
            notice: None,
            readonly: false,
            text: None,
            completions: Vec::new(),
            cert: None,
        });
    }

    /// Queues the completed-toggle of the selected machine (`m`, Máquinas).
    pub fn toggle_completed_selected(&mut self) {
        if self.pending_action.is_some() || self.popup.is_some() || self.report.is_some() {
            return;
        }
        if self.tab != Tab::Maquinas {
            self.set_status("Marcar completada solo está disponible en Máquinas.");
            return;
        }
        let Some(machine) = self.selected_machine_name() else {
            self.set_status("Nada seleccionado.");
            return;
        };
        self.pending_action = Some(TuiAction {
            kind: ActionKind::ToggleCompleted,
            values: vec![(0, machine.clone())],
            machine,
        });
    }

    /// Certificate action for the selected machine (`c`, Máquinas y
    /// Progreso). Certificates are issued manually by the DockerLabs admins
    /// once the writeup is validated and the machine is marked completed:
    /// when the profile already carries it, queue the PDF download;
    /// otherwise explain the exact missing step.
    pub fn certificate_action_selected(&mut self) {
        if self.pending_action.is_some() || self.popup.is_some() || self.report.is_some() {
            return;
        }
        if !matches!(self.tab, Tab::Maquinas | Tab::Progreso) {
            self.set_status("Los certificados están en las pestañas Máquinas y Progreso.");
            return;
        }
        let Some(machine) = self.selected_machine_name() else {
            self.set_status("Nada seleccionado.");
            return;
        };
        match self.data.cert_state(&machine) {
            CertState::Ready { cert_id, pdf_url } => {
                let prefill = crate::config::ConfigManager::new()
                    .download_dir()
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
                    .join("certificados");
                self.popup = Some(Popup {
                    kind: PopupKind::CertDest,
                    machine,
                    machine_id: 0,
                    buffers: vec![prefill.display().to_string()],
                    field: 0,
                    notice: None,
                    readonly: false,
                    text: None,
                    completions: Vec::new(),
                    cert: Some((cert_id, pdf_url)),
                });
            }
            CertState::MissingWriteup => {
                self.report = Some(ActionReport {
                    title: format!(" Certificado — {machine} "),
                    entries: vec![
                        (ReportKind::Failure, "Certificado: ✗ FALTA PUBLICAR TU WRITEUP".to_string()),
                        (
                            ReportKind::Info,
                            "Envíalo desde Máquinas: pulsa w sobre la máquina y luego u.".to_string(),
                        ),
                    ],
                    changed: false,
                    status: format!("[!] {machine}: falta publicar el writeup."),
                });
            }
            CertState::MissingCompleted => {
                self.report = Some(ActionReport {
                    title: format!(" Certificado — {machine} "),
                    entries: vec![
                        (ReportKind::Failure, "Certificado: ✗ FALTA MARCARLA COMO COMPLETADA".to_string()),
                        (ReportKind::Info, "Pulsa m sobre la máquina en Máquinas.".to_string()),
                    ],
                    changed: false,
                    status: format!("[!] {machine}: falta marcarla como completada."),
                });
            }
            CertState::MissingBoth => {
                self.report = Some(ActionReport {
                    title: format!(" Certificado — {machine} "),
                    entries: vec![
                        (ReportKind::Failure, "Certificado: ✗ FALTAN LOS DOS PASOS".to_string()),
                        (
                            ReportKind::Info,
                            "1) Publica tu writeup (w → u) · 2) Marca la máquina completada (m).".to_string(),
                        ),
                    ],
                    changed: false,
                    status: format!("[!] {machine}: falta el writeup y marcarla completada."),
                });
            }
            CertState::PendingAdmin => {
                self.report = Some(ActionReport {
                    title: format!(" Certificado — {machine} "),
                    entries: vec![
                        (ReportKind::Success, "Writeup publicado ✓ · Máquina completada ✓".to_string()),
                        (
                            ReportKind::Info,
                            "El certificado lo emite un admin tras validar el writeup — vuelve a comprobarlo más tarde (r).".to_string(),
                        ),
                    ],
                    changed: false,
                    status: format!("[⏳] {machine}: certificado pendiente de emisión por el admin."),
                });
            }
        }
    }

    /// Queues the batch download of every admin-issued certificate (`C`,
    /// Progreso).
    pub fn download_all_certs_selected(&mut self) {
        if self.pending_action.is_some() || self.popup.is_some() || self.report.is_some() {
            return;
        }
        if self.tab != Tab::Progreso {
            self.set_status("Los certificados están disponibles en la pestaña Progreso.");
            return;
        }
        let prefill = crate::config::ConfigManager::new()
            .download_dir()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
            .join("certificados");
        self.popup = Some(Popup {
            kind: PopupKind::CertDest,
            machine: "todos".to_string(),
            machine_id: 0,
            buffers: vec![prefill.display().to_string()],
            field: 0,
            notice: None,
            readonly: false,
            text: None,
            completions: Vec::new(),
            cert: None,
        });
    }

    /// Opens the selected writeup of the writeups popup in a browser.
    pub fn open_selected_writeup_link(&mut self) {
        let Some(popup) = self.writeups_popup.as_ref() else {
            return;
        };
        let Some(url) = popup.selected_url() else {
            return;
        };
        let opened = std::process::Command::new("xdg-open")
            .arg(url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        self.set_status(match opened {
            Ok(_) => format!("Abierto en el navegador: {url}"),
            Err(error) => format!("xdg-open falló: {error}"),
        });
    }

    /// URL of the selected completed machine's writeup (Progreso).
    pub fn selected_hecha_writeup_url(&self) -> Option<&str> {
        if self.tab != Tab::Progreso {
            return None;
        }
        self.visible_hechas()
            .get(self.selected)
            .and_then(|m| m.writeup_url.as_deref())
    }

    pub fn open_selected_hecha_writeup(&mut self) {
        if let Some(url) = self.selected_hecha_writeup_url() {
            let opened = std::process::Command::new("xdg-open")
                .arg(url)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            self.set_status(match opened {
                Ok(_) => format!("Abierto en el navegador: {url}"),
                Err(error) => format!("xdg-open falló: {error}"),
            });
        } else {
            self.set_status("Esta máquina no tiene writeup enlazado.");
        }
    }

    /// Confirms the popup: validates input, queues the corresponding action.
    pub fn confirm_popup(&mut self) {
        let Some(popup) = self.popup.take() else {
            return;
        };
        let values: Vec<(usize, String)> = popup
            .buffers
            .iter()
            .enumerate()
            .map(|(index, b)| (index, b.trim().to_string()))
            .collect();

        match popup.kind {
            PopupKind::Config => {
                if values.iter().any(|(_, v)| v.is_empty()) {
                    self.popup = Some(popup);
                    self.set_status("Usuario y contraseña son obligatorios.");
                    return;
                }
                self.set_status("Conectando...");
                self.pending_config = Some((values[0].1.clone(), values[1].1.clone()));
            }
            PopupKind::Descarga => {
                let dir = values.first().map(|(_, v)| v.clone()).unwrap_or_default();
                if dir.is_empty() {
                    self.popup = Some(popup);
                    self.set_status("Indica el directorio de destino.");
                    return;
                }
                let entry = (popup.machine.clone(), popup.machine_id, PathBuf::from(dir));
                let machine = entry.0.clone();
                // Two concurrent downloads of the same machine into the same
                // folder would corrupt the shared staging file — refuse.
                let duplicate = self.download_jobs.iter().any(|job| {
                    job.is_active() && job.machine == machine && job.dest_dir == entry.2
                });
                if duplicate {
                    self.set_status(format!(
                        "[↓] {machine} ya se está descargando en esa carpeta."
                    ));
                    return;
                }
                if self.active_downloads() >= downloads::PARALLEL_DOWNLOADS {
                    self.download_queue.push_back(entry);
                    self.set_status(format!(
                        "[↓] {machine} en cola — {} descargas activas.",
                        downloads::PARALLEL_DOWNLOADS
                    ));
                } else {
                    self.pending_download = Some(entry);
                    self.set_status(format!("[↓] Descarga de {machine} iniciada."));
                }
            }
            PopupKind::WriteupSubmit => {
                let url = values.first().map(|(_, v)| v.clone()).unwrap_or_default();
                if url.is_empty() {
                    self.popup = Some(popup);
                    self.set_status("Indica la URL del writeup.");
                    return;
                }
                let tipo = values
                    .get(1)
                    .map(|(_, v)| v.clone())
                    .filter(|t| t == "📝" || t == "🎥")
                    .unwrap_or_else(|| "📝".to_string());
                self.pending_action = Some(TuiAction {
                    kind: ActionKind::SubmitWriteup,
                    machine: popup.machine.clone(),
                    values: vec![(0, url), (1, tipo)],
                });
            }
            PopupKind::RatingSubmit => {
                let mut scores = Vec::new();
                for (_, value) in &values {
                    match value.parse::<u64>() {
                        Ok(n) if (1..=5).contains(&n) => scores.push(n),
                        _ => {
                            self.popup = Some(popup);
                            self.set_status("Cada puntuación debe ser un número del 1 al 5.");
                            return;
                        }
                    }
                }
                if scores.len() < 4 {
                    self.popup = Some(popup);
                    self.set_status("Faltan puntuaciones por rellenar.");
                    return;
                }
                self.pending_action = Some(TuiAction {
                    kind: ActionKind::SubmitRating,
                    machine: popup.machine.clone(),
                    values: scores
                        .into_iter()
                        .enumerate()
                        .map(|(i, s)| (i, s.to_string()))
                        .collect(),
                });
            }
            PopupKind::CertDest => {
                let dir = values.first().map(|(_, v)| v.clone()).unwrap_or_default();
                if dir.is_empty() {
                    self.popup = Some(popup);
                    self.set_status("Indica el directorio de destino.");
                    return;
                }
                match popup.cert.clone() {
                    Some((cert_id, pdf_url)) => {
                        self.pending_action = Some(TuiAction {
                            kind: ActionKind::DownloadCert,
                            machine: popup.machine.clone(),
                            values: vec![(0, cert_id), (1, pdf_url), (2, dir)],
                        });
                    }
                    None => {
                        self.pending_action = Some(TuiAction {
                            kind: ActionKind::DownloadAllCerts,
                            machine: "todos".to_string(),
                            values: vec![(0, dir)],
                        });
                    }
                }
            }
            _ => {}
        }
    }

    fn row_count(&self) -> usize {
        match self.tab {
            Tab::Maquinas => self.visible_machines().len(),
            Tab::Progreso => self.visible_hechas().len(),
            Tab::Writeups => self.visible_own_writeups().len(),
        }
    }

    pub fn move_down(&mut self) {
        let last = self.row_count().saturating_sub(1);
        self.selected = (self.selected + 1).min(last);
        self.ensure_selected_visible();
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.ensure_selected_visible();
    }

    pub fn move_start(&mut self) {
        self.selected = 0;
        self.scroll = 0;
    }

    /// Keeps `selected` inside the `[scroll, scroll + visible)` window.
    pub fn ensure_selected_visible(&mut self) {
        let visible = self.last_visible_rows.unwrap_or(10).max(1);
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + visible {
            self.scroll = self.selected + 1 - visible;
        }
    }

    /// Row budget reported by the renderer after layout.
    pub fn set_visible_rows(&mut self, rows: usize) {
        self.last_visible_rows = Some(rows.max(1));
        self.ensure_selected_visible();
    }

    pub fn enter_filter_mode(&mut self) {
        self.input_mode = InputMode::Filter;
    }

    pub fn filter_push(&mut self, c: char) {
        self.filter.push(c);
        self.reset_list_position();
    }

    pub fn filter_pop(&mut self) {
        self.filter.pop();
        self.reset_list_position();
    }

    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.reset_list_position();
    }

    /// Manual refresh: only allowed while idle to keep the state machine
    /// sane. The event loop owns the `fetching` label.
    pub fn request_refresh(&mut self) {
        if self.fetching.is_none() {
            self.refresh_requested = true;
        }
    }

    /// Whether the event loop should run a (re)fetch right now.
    pub fn should_fetch(&self, pending_fetch: bool) -> bool {
        pending_fetch || self.refresh_requested
    }
}

/// Host-provided callbacks the event loop calls synchronously (blocking the
/// render thread for the duration of each network call).
pub struct Host<'a> {
    pub refetch: &'a dyn Fn() -> Result<TuiData>,
    pub run_action: &'a dyn Fn(TuiAction) -> Result<ActionReport>,
    pub run_writeups_fetch: &'a dyn Fn(&str) -> Result<Vec<WriteupEntry>>,
    pub run_rating_fetch: &'a dyn Fn(&str) -> Result<crate::modules::ratings::MachineRating>,
    pub run_config: &'a dyn Fn(&str, &str) -> Result<()>,
    pub logout: &'a dyn Fn() -> Result<()>,
    /// Set when the next loop iteration must (re)fetch all data; `run()`
    /// seeds it from the entry state's `fetching` label.
    pub pending_fetch: bool,
}

pub fn run(mut app: AppState, host: &mut Host<'_>) -> Result<()> {
    let mut terminal = ratatui::init();
    // Kick off the first load (and any pending request) before looping.
    host.pending_fetch = app.fetching.is_some();
    let result = event_loop(&mut terminal, &mut app, host);
    ratatui::restore();
    result
}

fn event_loop(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    app: &mut AppState,
    host: &mut Host<'_>,
) -> Result<()> {
    loop {
        terminal.draw(|frame| crate::tui::render::draw(frame, app))?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(app, key);
                }
            }
        }

        app.tick();

        // Logout from the account popup (`l`).
        if app.pending_logout {
            app.pending_logout = false;
            app.fetching = Some("Cerrando sesión...".to_string());
            terminal.draw(|frame| crate::tui::render::draw(frame, app))?;

            match (host.logout)() {
                Ok(()) => {
                    app.fetching = None;
                    app.needs_config = true;
                    app.tab = Tab::Maquinas;
                    app.input_mode = InputMode::Normal;
                    app.view = ViewMode::Normal;
                    app.filter.clear();
                    app.set_data(TuiData::default());
                    app.open_config_popup(ConfigContext::LoggedOut, None);
                    app.set_status("[✓] Sesión cerrada — inicia otra cuenta o pulsa Esc para salir.");
                }
                Err(error) => {
                    app.fetching = None;
                    app.set_status(format!("Error al cerrar sesión: {error:#}"));
                }
            }
        }

        // Login credentials from the config popup.
        if let Some((username, password)) = app.pending_config.take() {
            app.fetching = Some(format!("Conectando como {username}..."));
            terminal.draw(|frame| crate::tui::render::draw(frame, app))?;

            match (host.run_config)(&username, &password) {
                Ok(()) => {
                    app.fetching = None;
                    app.needs_config = false;
                    app.set_status(format!("[✓] Conectado como {username} — cargando datos..."));
                    host.pending_fetch = true;
                }
                Err(error) => {
                    app.fetching = None;
                    app.set_status(format!("Error de configuración: {error:#}"));
                    app.open_config_popup(ConfigContext::LoginFailed, Some(&username));
                }
            }
        }

        // Start queued/pending downloads as slots free up.
        if let Some((machine, id, dir)) = app.pending_download.take() {
            match downloads::start_download(machine.clone(), id, dir.clone()) {
                Ok(job) => {
                    app.download_jobs.push(std::sync::Arc::new(job));
                    app.set_status(format!(
                        "[↓] Descarga de {machine} iniciada → {}",
                        downloads::shorten_path(&dir)
                    ));
                }
                Err(error) => app.set_status(format!("Error al descargar: {error:#}")),
            }
        }
        if app.active_downloads() < downloads::PARALLEL_DOWNLOADS {
            while let Some((machine, id, dir)) = app.download_queue.pop_front() {
                let duplicate = app.download_jobs.iter().any(|job| {
                    job.is_active() && job.machine == machine && job.dest_dir == dir
                });
                if duplicate {
                    app.set_status(format!("[↓] {machine} ya se está descargando ahí."));
                    continue;
                }
                match downloads::start_download(machine.clone(), id, dir.clone()) {
                    Ok(job) => {
                        app.download_jobs.push(std::sync::Arc::new(job));
                        app.set_status(format!(
                            "[↓] Descarga de {machine} iniciada → {}",
                            downloads::shorten_path(&dir)
                        ));
                    }
                    Err(error) => app.set_status(format!("Error al descargar: {error:#}")),
                }
                if app.active_downloads() >= downloads::PARALLEL_DOWNLOADS {
                    break;
                }
            }
        }

        // User actions from popups (toggle completed, submit writeup/rating,
        // generate certificate).
        if let Some(action) = app.pending_action.take() {
            let label = match action.kind {
                ActionKind::ToggleCompleted => format!("Actualizando {}...", action.machine),
                ActionKind::SubmitWriteup => format!("Enviando writeup de {}...", action.machine),
                ActionKind::SubmitRating => format!("Enviando valoración de {}...", action.machine),
                ActionKind::DownloadCert => format!("Descargando certificado de {}...", action.machine),
                ActionKind::DownloadAllCerts => "Descargando todos los certificados...".to_string(),
            };
            app.fetching = Some(label);
            terminal.draw(|frame| crate::tui::render::draw(frame, app))?;

            match (host.run_action)(action) {
                Ok(report) => {
                    app.set_status(report.status.clone());
                    app.pending_refresh_after_close = report.changed;
                    app.report = Some(report);
                }
                Err(error) => app.set_status(format!("Acción fallida: {error:#}")),
            }
            app.fetching = None;
        }

        // Blocking writeups fetch for the `w` key.
        if let Some(machine) = app.pending_writeups.take() {
            app.fetching = Some(format!("Cargando writeups de {machine}..."));
            terminal.draw(|frame| crate::tui::render::draw(frame, app))?;

            match (host.run_writeups_fetch)(&machine) {
                Ok(entries) => {
                    app.fetching = None;
                    if entries.is_empty() {
                        app.set_status(format!("Sin writeups de la comunidad para {machine}."));
                    } else {
                        app.writeups_popup = Some(WriteupsPopup {
                            machine,
                            entries,
                            selected: 0,
                        });
                    }
                }
                Err(error) => {
                    app.fetching = None;
                    app.set_status(format!("Error al cargar writeups: {error:#}"));
                }
            }
        }

        // Rating averages fetch for the `v` key; opens the Valoración popup.
        if let Some(machine) = app.pending_rating.take() {
            app.fetching = Some(format!("Cargando valoración de {machine}..."));
            terminal.draw(|frame| crate::tui::render::draw(frame, app))?;

            match (host.run_rating_fetch)(&machine) {
                Ok(rating) => {
                    app.fetching = None;
                    app.popup = Some(Popup {
                        kind: PopupKind::Valoracion,
                        machine: machine.clone(),
                        machine_id: 0,
                        buffers: Vec::new(),
                        field: 0,
                        notice: None,
                        readonly: true,
                        text: Some(crate::tui::render::format_rating(&rating)),
                        completions: Vec::new(),
            cert: None,
                    });
                }
                Err(error) => {
                    app.fetching = None;
                    app.set_status(format!("Error al cargar valoración: {error:#}"));
                }
            }
        }

        if app.should_fetch(host.pending_fetch) {
            host.pending_fetch = false;
            app.refresh_requested = false;
            app.fetching = Some("Actualizando datos...".to_string());
            // Draw immediately so the `⟳ <label>` shows while the blocking
            // fetch runs, instead of freezing silently.
            terminal.draw(|frame| crate::tui::render::draw(frame, app))?;

            let result = (host.refetch)();
            app.fetching = None;
            match result {
                Ok(data) => {
                    app.set_data(data);
                    app.set_status("Datos actualizados.");
                }
                Err(error) => app.set_status(format!("Error al obtener datos: {error:#}")),
            }
        }

        if app.quit {
            // Abort active tasks and clean their staged `.part` files.
            for job in &app.download_jobs {
                if job.is_active() {
                    job.request_cancel();
                    job.remove_part();
                }
            }
            return Ok(());
        }
    }
}

fn handle_key(app: &mut AppState, key: crossterm::event::KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.request_quit();
        return;
    }

    // Writeups popup captures everything until dismissed.
    if app.writeups_popup.is_some() {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => app.writeups_popup = None,
            KeyCode::Enter => app.open_selected_writeup_link(),
            KeyCode::Char('u') => app.open_writeup_submit_popup(),
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(popup) = app.writeups_popup.as_mut() {
                    popup.move_selection(-1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(popup) = app.writeups_popup.as_mut() {
                    popup.move_selection(1);
                }
            }
            _ => {}
        }
        return;
    }

    // Result report popup captures everything until dismissed. Closing it
    // with `changed` set queues the deferred refresh.
    if app.report.is_some() {
        match key.code {
            KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => {
                app.refresh_requested |= app.close_report();
            }
            _ => {}
        }
        return;
    }

    // Account popup captures everything until dismissed.
    if app.popup.as_ref().map(|p| p.kind) == Some(PopupKind::Account) {
        match key.code {
            KeyCode::Enter => app.begin_account_switch(),
            KeyCode::Char('l') => {
                app.popup = None;
                app.pending_logout = true;
            }
            KeyCode::Esc | KeyCode::Char('q') => app.popup = None,
            _ => {}
        }
        return;
    }

    // Valoración popup: Enter opens the rating submission form.
    if app.popup.as_ref().map(|p| p.kind) == Some(PopupKind::Valoracion) {
        match key.code {
            KeyCode::Enter => {
                let machine = app.popup.as_ref().map(|p| p.machine.clone()).unwrap_or_default();
                app.popup = Some(Popup {
                    kind: PopupKind::RatingSubmit,
                    machine,
                    machine_id: 0,
                    buffers: vec![String::new(); 4],
                    field: 0,
                    notice: None,
                    readonly: false,
                    text: None,
                    completions: Vec::new(),
            cert: None,
                });
            }
            KeyCode::Esc | KeyCode::Char('q') => app.popup = None,
            _ => {}
        }
        return;
    }

    // Generic popup input mode captures everything first.
    if app.popup.is_some() {
        if app.popup.as_ref().map(|p| p.readonly) == Some(true) {
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => app.popup = None,
                _ => {}
            }
            return;
        }
        let is_config = app.popup.as_ref().map(|p| p.kind) == Some(PopupKind::Config);
        match key.code {
            KeyCode::Esc => {
                app.popup = None;
                if is_config {
                    // Nothing else to do without an account — leave.
                    app.quit = true;
                } else {
                    app.set_status("Cancelado.");
                }
            }
            KeyCode::Enter => app.confirm_popup(),
            KeyCode::Backspace => {
                if let Some(popup) = app.popup.as_mut() {
                    popup.pop();
                }
            }
            KeyCode::Up => {
                if let Some(popup) = app.popup.as_mut() {
                    popup.previous_field();
                }
            }
            KeyCode::Tab => {
                let is_path_popup = matches!(
                    app.popup.as_ref().map(|p| p.kind),
                    Some(PopupKind::Descarga) | Some(PopupKind::CertDest)
                );
                if let Some(popup) = app.popup.as_mut() {
                    if is_path_popup {
                        popup.complete_destination();
                    } else {
                        popup.next_field();
                    }
                }
            }
            KeyCode::Down => {
                if let Some(popup) = app.popup.as_mut() {
                    popup.next_field();
                }
            }
            KeyCode::Char(c) => {
                if let Some(popup) = app.popup.as_mut() {
                    popup.push(c);
                }
            }
            _ => {}
        }
        return;
    }

    match app.input_mode {
        InputMode::Filter => match key.code {
            KeyCode::Esc => {
                app.clear_filter();
                app.input_mode = InputMode::Normal;
            }
            KeyCode::Enter => app.input_mode = InputMode::Normal,
            KeyCode::Backspace => app.filter_pop(),
            KeyCode::Char(c) => app.filter_push(c),
            _ => {}
        },
        InputMode::Normal => match key.code {
            KeyCode::Char('q') | KeyCode::Esc => app.request_quit(),
            KeyCode::Char('r') => app.request_refresh(),
            KeyCode::Tab | KeyCode::Right => app.next_tab(),
            KeyCode::Left | KeyCode::BackTab => app.previous_tab(),
            KeyCode::Down | KeyCode::Char('j') => app.move_down(),
            KeyCode::Up | KeyCode::Char('k') => app.move_up(),
            KeyCode::Home | KeyCode::Char('g') => app.move_start(),
            KeyCode::Char('/') => app.enter_filter_mode(),
            KeyCode::Char('a') => app.open_account_popup(),
            KeyCode::Char('o') => app.toggle_downloads_view(),
            KeyCode::Char('p') => app.toggle_pending_writeup(),
            KeyCode::Char('s') => match app.tab {
                Tab::Maquinas => {
                    app.machine_sort = app.machine_sort.next();
                    app.reset_list_position();
                }
                Tab::Progreso | Tab::Writeups => {
                    app.set_status("La ordenación no está disponible en esta pestaña.");
                }
            },
            KeyCode::Char('d') => {
                if app.tab != Tab::Maquinas {
                    app.set_status("Las descargas están disponibles en la pestaña Máquinas.");
                    return;
                }
                if app.popup.is_some() || app.report.is_some() {
                    return;
                }
                let Some(machine) = app.selected_machine() else {
                    app.set_status("Nada seleccionado.");
                    return;
                };
                let prefill = crate::config::ConfigManager::new()
                    .download_dir()
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                app.popup = Some(Popup {
                    kind: PopupKind::Descarga,
                    machine: machine.name.clone(),
                    machine_id: machine.id,
                    buffers: vec![prefill.display().to_string()],
                    field: 0,
                    notice: None,
                    readonly: false,
                    text: None,
                    completions: Vec::new(),
            cert: None,
                });
            }
            KeyCode::Char('w') => app.open_writeups_popup(),
            KeyCode::Char('v') => app.open_rating_popup(),
            KeyCode::Char('m') => app.toggle_completed_selected(),
            KeyCode::Char('i') => app.open_descripcion_popup(),
            KeyCode::Char('c') if app.view == ViewMode::Downloads => {
                if let Some(job) = app.download_jobs.iter().rev().find(|job| job.is_active()) {
                    job.request_cancel();
                    app.set_status(format!("Cancelando {}...", job.machine));
                }
            }
            KeyCode::Char('c') => app.certificate_action_selected(),
            KeyCode::Char('C') => app.download_all_certs_selected(),
            KeyCode::Enter => match app.tab {
                Tab::Maquinas => app.open_descripcion_popup(),
                Tab::Progreso => app.open_selected_hecha_writeup(),
                Tab::Writeups => app.open_selected_own_writeup(),
            },
            _ => {}
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    fn sample_data() -> TuiData {
        let profile_json = r#"{
            "username": "noneofyour",
            "progreso": {
                "maquinas_totales": 201, "maquinas_hechas": 1,
                "porcentaje": 0.5,
                "por_dificultad": {"Muy Fácil": {"hechas": 1, "totales": 8}}
            },
            "estadisticas": {"puntos_writeups": 10, "ranking_writeups": 33, "ranking_creadores": 0},
            "maquinas_hechas": [
                {"id": 1, "nombre": "Intranet", "dificultad": "Fácil", "writeup_url": "https://example.com/w"}
            ],
            "writeups": [
                {"maquina": "Intranet", "url": "https://example.com/w", "tipo": "texto"}
            ]
        }"#;
        TuiData {
            ranking_creador: Some(40),
            profile: serde_json::from_str(profile_json).unwrap(),
            catalog: vec![
                Machine {
                    id: 1,
                    name: "Intranet".into(),
                    dificultad: "Fácil".into(),
                    clase: "facil".into(),
                    color: "#8bc34a".into(),
                    categoria: "Hacking Web".into(),
                    autor: "d1se0".into(),
                    autor_url: String::new(),
                    fecha: "15/03/2025".into(),
                    descripcion: "Una máquina fácil, con web".into(),
                },
                Machine {
                    id: 2,
                    name: "Dance Samba".into(),
                    dificultad: "Medio".into(),
                    clase: "medio".into(),
                    color: "#e0a553".into(),
                    categoria: "Pivoting".into(),
                    autor: "El Pingüino".into(),
                    autor_url: String::new(),
                    fecha: "01/12/2024".into(),
                    descripcion: String::new(),
                },
            ],
        }
    }

    fn app() -> AppState {
        AppState::new(sample_data())
    }

    #[test]
    fn tab_navigation_resets_selection() {
        let mut state = app();
        assert_eq!(state.visible_machines().len(), 2);
        state.next_tab(); // Progreso (1 máquina hecha)
        assert_eq!(state.tab, Tab::Progreso);
        assert_eq!(state.visible_hechas().len(), 1);
        state.next_tab(); // vuelta a Máquinas (ciclo de 2 pestañas)
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll, 0);
    }

    #[test]
    fn filter_narrows_catalog_and_completed() {
        let mut state = app();
        state.filter_push('d');
        state.filter_push('a');
        assert_eq!(state.visible_machines().len(), 1);
        assert_eq!(state.visible_machines()[0].name, "Dance Samba");
        state.clear_filter();
        assert_eq!(state.visible_machines().len(), 2);
        assert_eq!(state.visible_hechas().len(), 1);
    }

    #[test]
    fn machine_sort_cycles_and_orders() {
        let mut state = app();
        assert_eq!(state.machine_sort, MachineSort::Sitio);
        let names = |state: &AppState| -> Vec<String> {
            state.visible_machines().iter().map(|m| m.name.clone()).collect()
        };
        assert_eq!(names(&state), ["Intranet", "Dance Samba"]);

        state.cycle_sort_for_test(); // Nombre
        assert_eq!(state.machine_sort, MachineSort::Nombre);
        assert_eq!(names(&state), ["Dance Samba", "Intranet"]);

        state.cycle_sort_for_test(); // Fecha (más reciente primero)
        assert_eq!(state.machine_sort, MachineSort::Fecha);
        assert_eq!(names(&state), ["Intranet", "Dance Samba"]);

        state.cycle_sort_for_test(); // Dificultad (Fácil < Medio)
        assert_eq!(state.machine_sort, MachineSort::Dificultad);
        assert_eq!(names(&state), ["Intranet", "Dance Samba"]);

        state.cycle_sort_for_test(); // Sitio
        assert_eq!(state.machine_sort, MachineSort::Sitio);
        assert_eq!(names(&state), ["Intranet", "Dance Samba"]);
    }

    impl AppState {
        pub fn cycle_sort_for_test(&mut self) {
            self.machine_sort = self.machine_sort.next();
            self.reset_list_position();
        }
    }

    #[test]
    fn download_popup_flow_and_gate() {
        let mut state = app();
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::empty()),
        );
        let popup = state.popup.as_ref().unwrap();
        assert_eq!(popup.kind, PopupKind::Descarga);
        assert_eq!(popup.machine, "Intranet");
        assert_eq!(popup.machine_id, 1);
        assert!(!popup.buffers[0].is_empty(), "destino prellenado");

        state.popup.as_mut().unwrap().buffers[0] = "/tmp/vm-lab".to_string();
        state.confirm_popup();
        let (machine, id, dir) = state.pending_download.take().unwrap();
        assert_eq!(machine, "Intranet");
        assert_eq!(id, 1);
        assert_eq!(dir, PathBuf::from("/tmp/vm-lab"));
    }

    #[test]
    fn destination_completion_lists_and_completes() {
        let base = std::env::temp_dir().join(format!("dl-tui-compl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("alpha")).unwrap();
        std::fs::create_dir_all(base.join("alphabet")).unwrap();
        std::fs::create_dir_all(base.join("beta")).unwrap();
        std::fs::write(base.join("file.txt"), "x").unwrap(); // files never complete

        let mut state = app();
        state.popup = Some(Popup {
            kind: PopupKind::Descarga,
            machine: "X".into(),
            machine_id: 1,
            buffers: vec![format!("{}/", base.display())],
            field: 0,
            notice: None,
            readonly: false,
            text: None,
            completions: Vec::new(),
            cert: None,
        });

        // Tab with a trailing separator: lists every directory, input kept.
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()),
        );
        let popup = state.popup.as_ref().unwrap();
        assert_eq!(popup.completions, ["alpha", "alphabet", "beta"]);
        assert_eq!(popup.buffers[0], format!("{}/", base.display()));

        // Partial input completes the shared prefix of the candidates.
        state.popup.as_mut().unwrap().buffers[0] = format!("{}/al", base.display());
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()),
        );
        let popup = state.popup.as_ref().unwrap();
        assert_eq!(popup.buffers[0], format!("{}/alpha", base.display()));
        assert_eq!(popup.completions, ["alpha", "alphabet"]);

        // Single match: completes fully with the trailing separator.
        state.popup.as_mut().unwrap().buffers[0] = format!("{}/b", base.display());
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()),
        );
        let popup = state.popup.as_ref().unwrap();
        assert_eq!(popup.buffers[0], format!("{}/beta/", base.display()));

        // Typing clears the stale listing.
        state.popup.as_mut().unwrap().push('x');
        assert!(state.popup.as_ref().unwrap().completions.is_empty());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn config_popup_requires_both_fields_and_esc_quits() {
        let mut state = AppState::unconfigured(Some("someuser"));
        assert!(state.needs_config);
        let popup = state.popup.as_ref().unwrap();
        assert_eq!(popup.kind, PopupKind::Config);
        assert_eq!(popup.buffers[0], "someuser");

        state.popup.as_mut().unwrap().buffers[0] = "someuser".into();
        state.confirm_popup();
        assert!(state.pending_config.is_none(), "falta la contraseña");
        assert_eq!(state.popup.as_ref().unwrap().kind, PopupKind::Config);

        state.popup.as_mut().unwrap().buffers[1] = "hunter2".into();
        state.confirm_popup();
        assert_eq!(
            state.pending_config.take().unwrap(),
            ("someuser".to_string(), "hunter2".to_string())
        );

        let mut state = AppState::unconfigured(None);
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()),
        );
        assert!(state.quit);
    }

    #[test]
    fn writeup_submit_popup_queues_action() {
        let mut state = app();
        state.writeups_popup = Some(WriteupsPopup {
            machine: "Intranet".into(),
            entries: vec![],
            selected: 0,
        });
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::empty()),
        );
        assert!(state.writeups_popup.is_none());
        let popup = state.popup.as_ref().unwrap();
        assert_eq!(popup.kind, PopupKind::WriteupSubmit);
        assert_eq!(popup.machine, "Intranet");
        assert_eq!(popup.buffers[1], "📝");

        state.popup.as_mut().unwrap().buffers[0] = "https://example.com/w".into();
        state.confirm_popup();
        let action = state.pending_action.take().unwrap();
        assert_eq!(action.kind, ActionKind::SubmitWriteup);
        assert_eq!(action.machine, "Intranet");
        assert_eq!(
            action.values,
            vec![(0, "https://example.com/w".to_string()), (1, "📝".to_string())]
        );
    }

    #[test]
    fn rating_submit_validates_scores() {
        let mut state = app();
        state.popup = Some(Popup {
            kind: PopupKind::RatingSubmit,
            machine: "Intranet".into(),
            machine_id: 0,
            buffers: vec!["5".into(), "4".into(), "0".into(), "3".into()],
            field: 0,
            notice: None,
            readonly: false,
            text: None,
            completions: Vec::new(),
            cert: None,
        });
        state.confirm_popup();
        assert!(state.pending_action.is_none(), "0 no es válido");
        assert!(state.popup.is_some(), "el popup se restaura");

        state.popup.as_mut().unwrap().buffers[2] = "4".into();
        state.confirm_popup();
        let action = state.pending_action.take().unwrap();
        assert_eq!(action.kind, ActionKind::SubmitRating);
        assert_eq!(action.machine, "Intranet");
        assert_eq!(
            action.values,
            vec![
                (0, "5".to_string()),
                (1, "4".to_string()),
                (2, "4".to_string()),
                (3, "3".to_string())
            ]
        );
    }

    #[test]
    fn account_popup_flow_and_gating() {
        let mut state = app();
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::empty()),
        );
        let popup = state.popup.as_ref().unwrap();
        assert_eq!(popup.kind, PopupKind::Account);
        assert_eq!(popup.machine, "noneofyour");

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );
        let popup = state.popup.as_ref().unwrap();
        assert_eq!(popup.kind, PopupKind::Config);
        assert_eq!(popup.buffers[0], "noneofyour");

        // While unconfigured the account popup is gated out.
        let mut state = AppState::unconfigured(None);
        state.open_account_popup();
        assert_eq!(state.popup.as_ref().unwrap().kind, PopupKind::Config);
    }

    #[test]
    fn toggle_completed_queues_action() {
        let mut state = app();
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('m'), KeyModifiers::empty()),
        );
        let action = state.pending_action.take().unwrap();
        assert_eq!(action.kind, ActionKind::ToggleCompleted);
        assert_eq!(action.machine, "Intranet");
    }

    #[test]
    fn description_popup_shows_machine_text() {
        let mut state = app();
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );
        let popup = state.popup.as_ref().unwrap();
        assert_eq!(popup.kind, PopupKind::Descripcion);
        assert_eq!(popup.machine, "Intranet");
        let text = popup.text.as_ref().unwrap();
        assert!(text.contains("Una máquina fácil, con web"));
        assert!(text.contains("Dificultad: Fácil"));
    }

    #[test]
    fn status_expires_after_lifetime() {
        let mut state = app();
        state.set_status("Datos actualizados.");
        assert!(state.status.is_some());
        state.tick();
        assert!(state.status.is_some());
        state.status_expiry = Some(std::time::Instant::now() - Duration::from_secs(1));
        state.tick();
        assert!(state.status.is_none());
    }

    #[test]
    fn quit_warns_once_while_downloads_are_active() {
        let mut state = app();
        state.download_jobs = vec![std::sync::Arc::new(crate::tui::downloads::DownloadJob {
            machine: "Intranet".into(),
            dest_dir: "/tmp".into(),
            state: std::sync::Arc::new(std::sync::Mutex::new(
                crate::tui::downloads::DownloadState::default(),
            )),
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })];

        state.request_quit();
        assert!(!state.quit);
        assert!(state.status.as_deref().unwrap().contains("q otra vez"));

        state.request_quit();
        assert!(state.quit);
    }

    #[test]
    fn cert_state_matrix() {
        let mut state = app();
        // Intranet: completed + writeup published + certificate issued -> Ready.
        assert!(matches!(
            state.data.cert_state("Intranet"),
            CertState::PendingAdmin
        ));

        // Dance Samba: neither completed nor writeup -> MissingBoth.
        assert_eq!(state.data.cert_state("Dance Samba"), CertState::MissingBoth);

        // Completed but without writeup -> MissingWriteup. Add such a machine.
        state.data.profile.maquinas_hechas[0].certificado = None;
        assert_eq!(state.data.cert_state("Intranet"), CertState::PendingAdmin);
        state.data.profile.maquinas_hechas.push(
            serde_json::from_value(serde_json::json!({
                "id": 9, "nombre": "Second", "dificultad": "Fácil"
            }))
            .unwrap(),
        );
        assert_eq!(state.data.cert_state("Second"), CertState::MissingWriteup);

        // Writeup published but not completed -> MissingCompleted.
        state.data.profile.writeups.push(
            serde_json::from_value(serde_json::json!({
                "maquina": "Dance Samba", "url": "https://example.com/s", "tipo": "texto"
            }))
            .unwrap(),
        );
        assert_eq!(
            state.data.cert_state("Dance Samba"),
            CertState::MissingCompleted
        );

        // Admin-issued -> Ready with the id.
        state.data.profile.maquinas_hechas[0].certificado = Some(
            serde_json::from_value(serde_json::json!({
                "cert_id": "DL-ABC123", "pdf_url": "https://dockerlabs.es/api/certificado/pdf/DL-ABC123"
            }))
            .unwrap(),
        );
        match state.data.cert_state("Intranet") {
            CertState::Ready { cert_id, .. } => assert_eq!(cert_id, "DL-ABC123"),
            other => panic!("esperaba Ready, obtuve {other:?}"),
        }
    }

    #[test]
    fn loading_state_shows_placeholder() {
        let state = AppState::loading();
        assert_eq!(state.fetching, Some("Cargando datos...".to_string()));
        assert!(state.data.catalog.is_empty());
        assert!(state.visible_machines().is_empty());
        assert!(state.visible_hechas().is_empty());
    }
}
