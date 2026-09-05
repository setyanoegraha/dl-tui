//! Authenticated session against dockerlabs.es. Login is a JSON POST that
//! sets the `session` cookie; state-changing POSTs additionally need the
//! CSRF token embedded in the site's HTML (`<meta name="csrf-token">`).

use anyhow::{Context, Result};
use reqwest::Client;
use scraper::Selector;

use crate::config::ConfigManager;

pub const BASE_URL: &str = "https://dockerlabs.es";
const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) ",
    "AppleWebKit/537.36 (KHTML, like Gecko) ",
    "Chrome/120.0.0.0 Safari/537.36 dl-tui/",
    env!("CARGO_PKG_VERSION")
);

/// A logged-in session. Cloning is cheap: reqwest::Client shares one
/// connection pool and cookie jar internally.
#[derive(Clone)]
pub struct DlSession {
    client: Client,
    csrf: Option<String>,
}

impl DlSession {
    /// GET a path and return the response body text.
    pub async fn get(&self, path: &str) -> Result<String> {
        let resp = self
            .client
            .get(format!("{BASE_URL}{path}"))
            .send()
            .await
            .context("Error de conexión")?;
        Ok(resp.text().await?)
    }

    /// POST a JSON body and return the response body text. Adds the CSRF
    /// token header when one was captured.
    pub async fn post_json(&self, path: &str, body: &serde_json::Value) -> Result<String> {
        let mut req = self.client.post(format!("{BASE_URL}{path}")).json(body);
        if let Some(csrf) = &self.csrf {
            req = req.header("X-CSRFToken", csrf);
        }
        let resp = req
            .send()
            .await
            .context("Error de conexión")?;
        Ok(resp.text().await?)
    }

    async fn fetch_csrf(&self) -> Option<String> {
        let html = self.get("/").await.ok()?;
        let doc = scraper::Html::parse_document(&html);
        let sel = Selector::parse("meta[name='csrf-token']").ok()?;
        doc.select(&sel)
            .next()?
            .value()
            .attr("content")
            .map(str::to_string)
    }
}

/// Logs in with explicit credentials and returns the authenticated session.
/// `POST /auth/login` answers `{"success":true,...}` and sets the session
/// cookie; afterwards the CSRF token is captured from the homepage.
pub async fn login_with(username: &str, password: &str) -> Result<DlSession> {
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(15))
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("No se pudo crear el cliente HTTP")?;

    let resp = client
        .post(format!("{BASE_URL}/auth/login"))
        .json(&serde_json::json!({"username": username, "password": password}))
        .send()
        .await
        .context("Error de conexión")?;

    let body: serde_json::Value = resp.json().await.context("Respuesta inválida del servidor")?;
    if body.get("success").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err(crate::modules::DlError::AuthFailed.into());
    }

    let session = DlSession { client, csrf: None };
    let csrf = session.fetch_csrf().await;
    Ok(DlSession {
        client: session.client,
        csrf,
    })
}

/// Logs in using stored credentials.
pub async fn login(cfg: &ConfigManager) -> Result<DlSession> {
    let (username, password) = cfg.load_credentials()?;
    login_with(&username, &password).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_csrf_meta_token() {
        let html = r#"<html><head><meta name="csrf-token" content="abc123"></head></html>"#;
        let doc = scraper::Html::parse_document(html);
        let sel = Selector::parse("meta[name='csrf-token']").unwrap();
        let token = doc
            .select(&sel)
            .next()
            .and_then(|m| m.value().attr("content"))
            .unwrap();
        assert_eq!(token, "abc123");
    }
}
