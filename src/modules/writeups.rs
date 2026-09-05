//! Per-machine community writeups (`GET /api/writeups/<machine>`) and
//! writeup submission (`POST /api/submit_writeup`, requires login).

use anyhow::{bail, Result};
use serde::Deserialize;

use crate::modules::session::DlSession;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WriteupEntry {
    pub name: String,
    pub url: String,
    #[serde(rename = "type")]
    pub tipo: String, // "📝" (artículo) o "🎥" (vídeo)
    pub id: u64,
}

pub struct WriteupManager {
    session: DlSession,
}

impl WriteupManager {
    pub fn new(session: DlSession) -> Self {
        Self { session }
    }

    /// Lists the accepted community writeups of one machine.
    pub async fn fetch(&self, machine: &str) -> Result<Vec<WriteupEntry>> {
        let body = self
            .session
            .get(&format!("/api/writeups/{machine}"))
            .await?;
        let entries: Vec<WriteupEntry> = serde_json::from_str(&body)?;
        Ok(entries)
    }

    /// Submits a writeup URL for a machine. `tipo` is "📝" or "🎥".
    /// Returns Ok(()) when the server reports success.
    pub async fn submit(&self, machine: &str, url: &str, tipo: &str) -> Result<()> {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            bail!("La URL del writeup debe empezar por http:// o https://.");
        }
        let body = self
            .session
            .post_json(
                "/api/submit_writeup",
                &serde_json::json!({"maquina": machine, "url": url.trim(), "tipo": tipo}),
            )
            .await?;
        let value: serde_json::Value = serde_json::from_str(&body)
            .unwrap_or(serde_json::Value::String(body.trim().to_string()));
        if value.get("success").and_then(serde_json::Value::as_bool) == Some(true) {
            Ok(())
        } else {
            let message = value
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("El servidor rechazó el writeup.");
            bail!("{message}")
        }
    }
}
