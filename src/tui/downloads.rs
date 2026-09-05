//! Background download jobs for the TUI. Each job runs on the tokio
//! runtime and reports through shared state the renderer reads directly —
//! no terminal output, no channel loss. The event loop owns starting,
//! queueing (max 2 parallel) and cancelling.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};

use crate::download::{download_zip, resolve_download_url, DownloadHooks};

/// At most two machine archives are pulled in parallel.
pub const PARALLEL_DOWNLOADS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Resolving,
    Downloading,
    Done,
    Failed,
    Cancelled,
}

/// Live progress, shared between the download task and the renderer.
#[derive(Debug)]
pub struct DownloadState {
    pub phase: Phase,
    pub downloaded: u64,
    pub total: u64,
    pub speed_bps: u64,
    pub message: String,
    pub part_path: Option<PathBuf>,
    last_bytes: u64,
    last_time: Option<Instant>,
}

impl Default for DownloadState {
    fn default() -> Self {
        Self {
            phase: Phase::Resolving,
            downloaded: 0,
            total: 0,
            speed_bps: 0,
            message: String::new(),
            part_path: None,
            last_bytes: 0,
            last_time: None,
        }
    }
}

impl DownloadState {
    fn note_progress(&mut self, bytes: u64) {
        let now = Instant::now();
        if let Some(last) = self.last_time {
            let dt = now.duration_since(last).as_secs_f64();
            if dt >= 0.25 {
                self.speed_bps = (bytes.saturating_sub(self.last_bytes) as f64 / dt) as u64;
                self.last_bytes = bytes;
                self.last_time = Some(now);
            }
        } else {
            self.last_time = Some(now);
        }
        self.downloaded = bytes;
    }
}

#[derive(Debug)]
pub struct DownloadJob {
    pub machine: String,
    pub state: Arc<Mutex<DownloadState>>,
    pub cancel: Arc<AtomicBool>,
}

impl DownloadJob {
    pub fn is_active(&self) -> bool {
        matches!(
            self.state.lock().unwrap().phase,
            Phase::Resolving | Phase::Downloading
        )
    }

    /// Requests cancellation; the task cleans up its `.part` file.
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Best-effort `.part` cleanup used when quitting with active jobs.
    pub fn remove_part(&self) {
        if let Some(part) = self.state.lock().unwrap().part_path.clone() {
            let _ = std::fs::remove_file(part);
        }
    }
}

/// Persists the destination choice and spawns the download task. The
/// download page is public, so the resolver uses its own client and no
/// session is needed here.
pub fn start_download(machine: String, machine_id: u32, dest_dir: PathBuf) -> Result<DownloadJob> {
    crate::config::ConfigManager::new().save_download_dir(&dest_dir)?;
    std::fs::create_dir_all(&dest_dir)
        .with_context(|| format!("No se pudo crear el directorio {}", dest_dir.display()))?;

    let state = Arc::new(Mutex::new(DownloadState::default()));
    let cancel = Arc::new(AtomicBool::new(false));

    let task_state = state.clone();
    let task_cancel = cancel.clone();
    // The task runs detached: cancellation goes through the shared flag and
    // `.part` cleanup through DownloadJob::remove_part.
    tokio::spawn(async move {
        let hooks = DownloadHooks {
            cancel: &task_cancel,
            on_metadata: &|total, _name| {
                let mut s = task_state.lock().unwrap();
                s.total = total;
                s.phase = Phase::Downloading;
            },
            on_progress: &|bytes| {
                task_state.lock().unwrap().note_progress(bytes);
            },
            on_part: &|part| {
                task_state.lock().unwrap().part_path = Some(part.to_path_buf());
            },
        };

        let outcome = async {
            let url = resolve_download_url(machine_id).await?;
            download_zip(&url, &dest_dir, &hooks).await
        }
        .await;

        let mut s = task_state.lock().unwrap();
        match outcome {
            Ok(path) => {
                s.phase = Phase::Done;
                s.message = path.display().to_string();
            }
            Err(error) => {
                if task_cancel.load(Ordering::Relaxed) {
                    s.phase = Phase::Cancelled;
                } else {
                    s.phase = Phase::Failed;
                    s.message = format!("{error:#}");
                }
            }
        }
    });

    Ok(DownloadJob {
        machine,
        state,
        cancel,
    })
}

/// Human-readable byte size: "0 B", "842.1 KB", "1.9 GB", ...
pub fn fmt_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_bytes_picks_units() {
        assert_eq!(fmt_bytes(0), "0 B");
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1024), "1.0 KB");
        assert_eq!(fmt_bytes(1_966_080), "1.9 MB");
        assert_eq!(fmt_bytes(2_038_433_792), "1.9 GB");
    }

    #[test]
    fn job_defaults_to_resolving_and_can_be_cancelled() {
        let job = DownloadJob {
            machine: "Intranet".into(),
            state: Arc::new(Mutex::new(DownloadState::default())),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        assert!(job.is_active());
        job.request_cancel();
        // Flag is set even though no task consumed it yet.
        assert!(job.is_active());
    }
}
