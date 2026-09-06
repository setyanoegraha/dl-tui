//! Machine download flow: the catalog links to
//! `GET /maquinas/<id>/descargar`, an HTML page whose `a.descargar-btn`
//! points at the public zip on gestion-maquinas.dockerlabs.es. The zip
//! streams straight to disk (no decryption needed) as `<name>.zip`, staged
//! through a `.part` file so partial downloads never look complete.

use anyhow::{bail, Context, Result};
use scraper::Selector;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// Callbacks handed to [`download_zip`]: terminal rendering and in-TUI
/// progress both live behind these hooks.
pub struct DownloadHooks<'a> {
    /// Set to true to abort the transfer; the `.part` file is cleaned up.
    pub cancel: &'a AtomicBool,
    /// Called once after metadata: (total bytes, original file name).
    pub on_metadata: &'a (dyn Fn(u64, &str) + Send + Sync),
    /// Called on every chunk with the absolute number of bytes written.
    pub on_progress: &'a (dyn Fn(u64) + Send + Sync),
    /// Called once when the `.part` staging file is created.
    pub on_part: &'a (dyn Fn(&Path) + Send + Sync),
}

/// Resolves the direct zip URL of a machine from its download page. The
/// page is public (no session needed) and answers GET only — HEAD returns
/// 405.
pub async fn resolve_download_url(machine_id: u32) -> Result<String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("dl-tui/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let resp = client
        .get(format!("https://dockerlabs.es/maquinas/{machine_id}/descargar"))
        .send()
        .await
        .context("Error de conexión al resolver la descarga")?
        .error_for_status()
        .context("La página de descarga devolvió un error")?;
    let html = resp.text().await?;
    let doc = scraper::Html::parse_document(&html);
    let btn_sel = Selector::parse("a.descargar-btn").unwrap();
    let href = doc
        .select(&btn_sel)
        .next()
        .and_then(|a| a.value().attr("href"))
        .map(str::to_string);

    match href {
        Some(url) if url.starts_with("http") => Ok(url),
        _ => bail!("No se encontró el enlace de descarga de la máquina."),
    }
}

/// Downloads a zip with progress reporting and verifies the byte count
/// against `Content-Length` when the server sends it. If the target file
/// already exists the download is skipped (same source, same archive) and
/// `Ok((path, true))` is returned.
pub async fn download_zip(
    url: &str,
    destination: &Path,
    hooks: &DownloadHooks<'_>,
) -> Result<(PathBuf, bool)> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("dl-tui/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(600))
        .build()?;

    let resp = client
        .get(url)
        .send()
        .await
        .context("Error de conexión durante la descarga")?
        .error_for_status()
        .context("El servidor de descargas devolvió un error")?;

    let total = resp.content_length().unwrap_or(0);
    let filename = filename_from(&resp, url);
    let output = destination.join(&filename);
    if output.exists() {
        return Ok((output, true));
    }

    let part = destination.join(format!("{filename}.part"));
    (hooks.on_part)(&part);
    (hooks.on_metadata)(total, &filename);

    let result = stream_to_file(resp, &part, hooks).await;
    match result {
        Ok(()) => {
            tokio::fs::rename(&part, &output)
                .await
                .context("No se pudo finalizar la descarga")?;
            Ok((output, false))
        }
        Err(error) => {
            let _ = tokio::fs::remove_file(&part).await;
            Err(error)
        }
    }
}

async fn stream_to_file(
    resp: reqwest::Response,
    part: &Path,
    hooks: &DownloadHooks<'_>,
) -> Result<()> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let total = resp.content_length().unwrap_or(0);
    let mut stream = resp.bytes_stream();
    let mut file = tokio::fs::File::create(part)
        .await
        .context("No se pudo crear el archivo temporal de descarga")?;
    let mut written = 0u64;

    while let Some(chunk) = stream.next().await {
        if hooks.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            bail!("Descarga cancelada.");
        }
        let chunk = chunk.context("Error de red durante la descarga")?;
        file.write_all(&chunk).await?;
        written += chunk.len() as u64;
        (hooks.on_progress)(written);
    }

    if total > 0 && written != total {
        bail!("Descarga incompleta: {written} de {total} bytes");
    }
    file.flush().await?;
    Ok(())
}

/// Filename from Content-Disposition, else from the URL path, sanitized.
fn filename_from(resp: &reqwest::Response, url: &str) -> String {
    let from_header = resp
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';').find_map(|part| {
                let part = part.trim();
                part.strip_prefix("filename=").map(|f| {
                    f.trim_matches(|c| c == '"' || c == '\'').to_string()
                })
            })
        })
        .or_else(|| {
            url.split('/')
                .next_back()
                .map(|s| s.split('?').next().unwrap_or(s).to_string())
        })
        .unwrap_or_else(|| "maquina.zip".to_string());

    let cleaned: String = from_header
        .chars()
        .filter(|c| *c != '/' && *c != '\\' && !c.is_control())
        .collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "maquina.zip".to_string()
    } else {
        cleaned
    }
}
