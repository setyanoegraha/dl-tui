//! Secure credential storage: username in ~/.dl-tui/config.json, password in
//! the OS credential vault (Secret Service on Linux, Credential Manager on
//! Windows, Keychain on macOS). Also persists the last download directory.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

const SERVICE_NAME: &str = "dl-tui";
const CONFIG_DIR_NAME: &str = ".dl-tui";
const CONFIG_FILE_NAME: &str = "config.json";

#[derive(Serialize, Deserialize)]
struct ConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    download_dir: Option<String>,
}

pub struct ConfigManager {
    config_file: PathBuf,
}

impl ConfigManager {
    pub fn new() -> Self {
        let dir = home::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(CONFIG_DIR_NAME);
        let _ = fs::create_dir_all(&dir);
        Self {
            config_file: dir.join(CONFIG_FILE_NAME),
        }
    }

    fn read_config(&self) -> Option<ConfigFile> {
        let raw = fs::read_to_string(&self.config_file).ok()?;
        serde_json::from_str(&raw).ok()
    }

    fn read_username(&self) -> Result<String> {
        self.stored_username().ok_or_else(|| {
            anyhow!("No hay cuenta configurada. Ejecuta 'dl' y configura tu cuenta en el dashboard.")
        })
    }

    fn keyring_entry(&self, username: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE_NAME, username)
            .map_err(|e| anyhow!("Sistema de almacenamiento de contraseñas no disponible: {e}"))
    }

    /// Last download directory the user chose (survives restarts).
    pub fn download_dir(&self) -> Option<PathBuf> {
        self.read_config()
            .and_then(|cfg| cfg.download_dir)
            .map(PathBuf::from)
    }

    /// Persists the chosen download directory, preserving other fields.
    pub fn save_download_dir(&self, dir: &Path) -> Result<()> {
        let cfg = ConfigFile {
            username: self.read_username().ok(),
            download_dir: Some(dir.display().to_string()),
        };
        fs::write(&self.config_file, serde_json::to_string(&cfg)?)
            .with_context(|| "No se pudo escribir el archivo de configuración")?;
        Ok(())
    }

    /// Persists the username on disk and the password in the OS vault,
    /// preserving the saved download directory. Called only after a
    /// successful login so the vault never holds invalid credentials.
    pub fn save_credentials(&self, username: &str, password: &str) -> Result<()> {
        let download_dir = self.read_config().and_then(|cfg| cfg.download_dir);
        let json = serde_json::to_string(&ConfigFile {
            username: Some(username.to_string()),
            download_dir,
        })?;
        fs::write(&self.config_file, json)
            .with_context(|| "No se pudo escribir el archivo de configuración")?;

        let entry = self.keyring_entry(username)?;
        entry
            .set_password(password)
            .map_err(|e| anyhow!("No se pudo guardar la contraseña en la bóveda del sistema: {e}"))
    }

    /// Removes the stored account: deletes the vault password and blanks the
    /// username. The download-directory preference is preserved.
    pub fn clear_credentials(&self) -> Result<()> {
        if let Some(username) = self.stored_username() {
            if let Ok(entry) = self.keyring_entry(&username) {
                match entry.delete_credential() {
                    Ok(()) | Err(keyring::Error::NoEntry) => {}
                    Err(e) => {
                        return Err(anyhow!(
                            "No se pudo borrar la contraseña de la bóveda del sistema: {e}"
                        ))
                    }
                }
            }
        }

        let download_dir = self.read_config().and_then(|cfg| cfg.download_dir);
        let json = serde_json::to_string(&ConfigFile {
            username: None,
            download_dir,
        })?;
        fs::write(&self.config_file, json)
            .with_context(|| "No se pudo escribir el archivo de configuración")?;
        Ok(())
    }

    /// Stored username, if any (used to prefill the config popup).
    pub fn stored_username(&self) -> Option<String> {
        self.read_config().and_then(|cfg| cfg.username)
    }

    /// Loads the username from disk and the password from the OS vault.
    pub fn load_credentials(&self) -> Result<(String, String)> {
        let username = self.read_username()?;
        let entry = self.keyring_entry(&username)?;
        match entry.get_password() {
            Ok(password) => Ok((username, password)),
            Err(keyring::Error::NoEntry) => Err(anyhow!(
                "Contraseña no encontrada. Inicia sesión de nuevo en el dashboard."
            )),
            Err(e) => Err(anyhow!("Error al acceder a la bóveda del sistema: {e}")),
        }
    }
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self::new()
    }
}
