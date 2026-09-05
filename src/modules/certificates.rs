//! Completion certificates: availability + generation for machines the user
//! marked as done, and public verification of `DL-XXXXXX` ids.

use anyhow::Result;
use serde::Deserialize;

use crate::modules::session::DlSession;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct VerifiedCertificate {
    pub valid: bool,
    pub username: String,
    pub machine: String,
    pub dificultad: String,
    pub pdf_url: String,
    pub message: String,
}

pub struct CertificateManager {
    session: DlSession,
}

impl CertificateManager {
    pub fn new(session: DlSession) -> Self {
        Self { session }
    }

    /// Generates (or regenerates) the certificate of a completed machine and
    /// returns the PDF URL. The server may take a moment; unknown shapes are
    /// searched for any `pdf_url`-like field.
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
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("No se pudo generar el certificado.");
                anyhow::bail!("{message}")
            }
        }
    }

    /// Publicly verifies a `DL-XXXXXX` certificate id.
    pub async fn verify(&self, cert_id: &str) -> Result<VerifiedCertificate> {
        let body = self
            .session
            .get(&format!("/api/certificado/verificar/{cert_id}"))
            .await?;
        Ok(serde_json::from_str(&body)?)
    }
}
