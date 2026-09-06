//! Shared application state and error types.

pub mod completed;
pub mod machines;
pub mod profile;
pub mod ratings;
pub mod session;
pub mod writeups;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DlError {
    #[error("Inicio de sesión fallido. Revisa tu usuario y contraseña.")]
    AuthFailed,
}
