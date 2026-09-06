//! Completion certificates: availability + generation for machines the user
//! marked as done.

use anyhow::Result;

use crate::modules::session::DlSession;

pub struct CertificateManager {
    session: DlSession,
}

impl CertificateManager {
    pub fn new(session: DlSession) -> Self {
        Self { session }
    }

    /// Whether the certificate can be generated: DockerLabs only issues it
    /// once the writeup is published AND the machine is marked completed.
    /// `GET /api/certificado/<machine>/disponible` -> {"disponible": bool}.
    pub async fn available(&self, machine: &str) -> Result<bool> {
        let body = self
            .session
            .get(&format!("/api/certificado/{machine}/disponible"))
            .await?;
        let value: serde_json::Value = serde_json::from_str(&body)?;
        Ok(value
            .get("disponible")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false))
    }

    /// Generates (or regenerates) the certificate of a completed machine and
    /// returns the PDF URL. Server errors surface via the `error` field.
    pub async fn generate(&self, machine: &str) -> Result<String> {
        let body = self
            .session
            .get(&format!("/api/certificado/{machine}"))
            .await?;
        let value: serde_json::Value = serde_json::from_str(&body)
            .unwrap_or(serde_json::Value::String(body.trim().to_string()));
        let pdf_url = value
            .get("pdf_url")
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                value
                    .get("certificado")
                    .and_then(|c| c.get("pdf_url"))
                    .and_then(serde_json::Value::as_str)
            });
        match pdf_url {
            Some(url) => Ok(url.to_string()),
            None => {
                let message = value
                    .get("error")
                    .or_else(|| value.get("message"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("No se pudo generar el certificado.");
                anyhow::bail!("{message}")
            }
        }
    }
}
