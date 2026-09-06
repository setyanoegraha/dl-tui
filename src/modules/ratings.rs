//! Machine ratings (`GET /api/get_machine_rating/<machine>`) and rating
//! submission (`POST /rate_machine`, requires login). Four criteria, 1–5.

use anyhow::{bail, Result};
use serde::Deserialize;

use crate::modules::session::DlSession;

/// The user's own rating, returned by the API as an object once they have
/// rated the machine (null before that).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UserRating {
    pub dificultad: u64,
    pub aprendizaje: u64,
    pub recomendaria: u64,
    pub diversion: u64,
}

impl UserRating {
    /// Compact "3/3/4/4" style rendering of the user's own scores.
    pub fn summary(&self) -> String {
        format!(
            "{}/{}/{}/{}",
            self.dificultad, self.aprendizaje, self.recomendaria, self.diversion
        )
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MachineRating {
    pub average: f64,
    pub count: u64,
    pub details: RatingDetails,
    pub user_rating: Option<UserRating>,
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
                "/api/rate_machine",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rating_with_object_user_rating() {
        let json = r#"{"average":3.5,"count":2,"details":{"dificultad":3.0,"aprendizaje":3.0,
            "recomendaria":4.0,"diversion":4.0},
            "user_rating":{"dificultad":3,"aprendizaje":3,"recomendaria":4,"diversion":4}}"#;
        let rating: MachineRating = serde_json::from_str(json).unwrap();
        assert_eq!(rating.count, 2);
        assert_eq!(
            rating.user_rating.unwrap().summary(),
            "3/3/4/4"
        );
    }

    #[test]
    fn parses_rating_with_null_user_rating() {
        let json = r#"{"average":3.5,"count":1,"details":{"dificultad":3.0,"aprendizaje":3.0,
            "recomendaria":4.0,"diversion":4.0},"user_rating":null}"#;
        let rating: MachineRating = serde_json::from_str(json).unwrap();
        assert!(rating.user_rating.is_none());
    }
}