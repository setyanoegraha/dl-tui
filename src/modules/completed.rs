//! Self-marked machine completion (`POST /toggle_completed_machine`,
//! requires login). This is the platform's "solved" mechanic — there are no
//! flags or score submission on DockerLabs.

use anyhow::{bail, Result};

use crate::modules::session::DlSession;

pub struct CompletedManager {
    session: DlSession,
}

impl CompletedManager {
    pub fn new(session: DlSession) -> Self {
        Self { session }
    }

    /// Toggles the completed flag of a machine; returns the new state.
    pub async fn toggle(&self, machine: &str) -> Result<bool> {
        let body = self
            .session
            .post_json(
                "/toggle_completed_machine",
                &serde_json::json!({"machine_name": machine}),
            )
            .await?;
        let value: serde_json::Value = serde_json::from_str(&body)
            .unwrap_or(serde_json::Value::String(body.trim().to_string()));
        if value.get("success").and_then(serde_json::Value::as_bool) == Some(true) {
            Ok(value
                .get("completed")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false))
        } else {
            let message = value
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("El servidor rechazó la operación.");
            bail!("{message}")
        }
    }
}
