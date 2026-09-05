//! Machine ratings (`GET /api/get_machine_rating/<machine>`) and rating
//! submission (`POST /rate_machine`, requires login). Four criteria, 1–5.

use anyhow::{bail, Result};
use serde::Deserialize;

use crate::modules::session::DlSession;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MachineRating {
    pub average: f64,
    pub count: u64,
    pub details: RatingDetails,
    pub user_rating: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RatingDetails {
    pub dificultad: f64,
    pub aprendizaje: f64,
    pub recomendaria: f64,
    pub diversion: f64,
}

pub struct RatingManager {
    session: DlSession,
}

impl RatingManager {
    pub fn new(session: DlSession) -> Self {
        Self { session }
    }

    pub async fn fetch(&self, machine: &str) -> Result<MachineRating> {
        let body = self
            .session
            .get(&format!("/api/get_machine_rating/{machine}"))
            .await?;
        Ok(serde_json::from_str(&body)?)
    }

    /// Submits a personal rating; every score must be in 1..=5.
    pub async fn submit(
        &self,
        machine: &str,
        dificultad: u64,
        aprendizaje: u64,
        recomendaria: u64,
        diversion: u64,
    ) -> Result<()> {
        for score in [dificultad, aprendizaje, recomendaria, diversion] {
            if !(1..=5).contains(&score) {
                bail!("Cada puntuación debe estar entre 1 y 5.");
            }
        }
        let body = self
            .session
            .post_json(
                "/rate_machine",
                &serde_json::json!({
                    "maquina_nombre": machine,
                    "dificultad_score": dificultad,
                    "aprendizaje_score": aprendizaje,
                    "recomendaria_score": recomendaria,
                    "diversion_score": diversion,
                }),
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
                .unwrap_or("El servidor rechazó la valoración.");
            bail!("{message}")
        }
    }
}
