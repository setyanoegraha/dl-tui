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
        csrf_from(&html)
    }
}

/// Extracts the CSRF token from a page's `<meta name="csrf-token">`.
fn csrf_from(html: &str) -> Option<String> {
    let doc = scraper::Html::parse_document(html);
    let sel = Selector::parse("meta[name='csrf-token']").ok()?;
    doc.select(&sel)
        .next()?
        .value()
        .attr("content")
        .map(str::to_string)
}

/// Logs in with explicit credentials and returns the authenticated session.
/// The production contract (as called by the site itself):
///   1. GET /login -> grab the CSRF token from the meta tag
///   2. POST /api/auth/login with `X-CSRFToken` + JSON {username, password}
///   3. `{"success":true,...}` sets the session cookie
pub async fn login_with(username: &str, password: &str) -> Result<DlSession> {
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(15))
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("No se pudo crear el cliente HTTP")?;

    let login_page = client
        .get(format!("{BASE_URL}/login"))
        .send()
        .await
        .context("Error de conexión")?
        .text()
        .await?;
    let csrf = csrf_from(&login_page);

    let mut req = client
        .post(format!("{BASE_URL}/api/auth/login"))
        .json(&serde_json::json!({"username": username, "password": password}));
    if let Some(token) = &csrf {
        req = req.header("X-CSRFToken", token);
    }
    let resp = req
        .send()
        .await
        .context("Error de conexión")?;

    let body: serde_json::Value = resp.json().await.context("Respuesta inválida del servidor")?;
    if body.get("success").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err(crate::modules::DlError::AuthFailed.into());
    }

    // Re-capture the CSRF token for authenticated POSTs (it may rotate on
    // login); fall back to the pre-login one.
    let probe = DlSession {
        client: client.clone(),
        csrf,
    };
    let csrf = probe.fetch_csrf().await.or(probe.csrf);
    Ok(DlSession { client, csrf })
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

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::modules::machines::MachineScraper;
    use crate::modules::profile::ProfileFetcher;

    #[tokio::test(flavor = "multi_thread")]
    #[ignore] // live: DL_USER=... DL_PASS=... cargo test live_login -- --ignored --nocapture
    async fn live_login_and_fetch() {
        let username = std::env::var("DL_USER").unwrap();
        let password = std::env::var("DL_PASS").unwrap();
        let session = match login_with(&username, &password).await {
            Ok(s) => s,
            Err(e) => {
                println!("LOGIN ERROR: {e:#}");
                panic!("login failed");
            }
        };
        println!("login OK");
        match ProfileFetcher::new(session.clone()).fetch(&username).await {
            Ok(p) => println!("profile OK: {} — {}/{} máquinas", p.username, p.progreso.maquinas_hechas, p.progreso.maquinas_totales),
            Err(e) => println!("PROFILE ERROR: {e:#}"),
        }
        match MachineScraper::new(session).get_catalog().await {
            Ok(c) => println!("catalog OK: {} máquinas", c.len()),
            Err(e) => println!("CATALOG ERROR: {e:#}"),
        }
    }
}
