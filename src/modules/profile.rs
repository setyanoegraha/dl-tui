//! Public profile data for the logged-in user (`GET /u/<username>` JSON):
//! progress gauges, statistics, machines done/created and writeups.

use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;

use crate::modules::session::DlSession;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub username: String,
    pub progreso: Progreso,
    pub estadisticas: Estadisticas,
    pub maquinas_hechas: Vec<MaquinaFicha>,
    pub maquinas_creadas: Vec<MaquinaFicha>,
    pub writeups: Vec<WriteupRef>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Progreso {
    pub catalogo: u64,
    pub maquinas_totales: u64,
    pub maquinas_hechas: u64,
    pub porcentaje: f64,
    pub por_dificultad: HashMap<String, DificultadProgreso>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DificultadProgreso {
    pub hechas: u64,
    pub totales: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Estadisticas {
    pub puntos_writeups: u64,
    pub ranking_writeups: u64,
    pub ranking_creadores: u64,
}

/// A machine ficha as embedded in the profile lists; extra fields only
/// appear on `maquinas_hechas` (completion date, writeup, certificate).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MaquinaFicha {
    pub id: u64,
    pub nombre: String,
    pub dificultad: String,
    pub clase: String,
    pub color: String,
    pub categoria: String,
    pub autor: String,
    pub fecha: String,
    pub completada_el: Option<String>,
    pub writeup_url: Option<String>,
    pub certificado: Option<CertificadoInfo>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CertificadoInfo {
    pub cert_id: String,
    pub pdf_url: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WriteupRef {
    pub nombre: String,
    pub url: String,
    pub puntos: Option<u64>,
}

pub struct ProfileFetcher {
    session: DlSession,
}

impl ProfileFetcher {
    pub fn new(session: DlSession) -> Self {
        Self { session }
    }

    pub async fn fetch(&self, username: &str) -> Result<Profile> {
        let body = self.session.get(&format!("/u/{username}")).await?;
        let profile: Profile = serde_json::from_str(&body)?;
        Ok(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_profile_fixture() {
        let json = r#"{
            "slug": "noneofyour",
            "username": "noneofyour",
            "perfil": {"id": 7, "rol": "jugador"},
            "progreso": {
                "catalogo": 201,
                "maquinas_totales": 201,
                "maquinas_hechas": 42,
                "porcentaje": 20.9,
                "por_dificultad": {
                    "Muy Fácil": {"hechas": 5, "totales": 8},
                    "Fácil": {"hechas": 30, "totales": 68}
                }
            },
            "estadisticas": {"puntos_writeups": 120, "ranking_writeups": 9, "ranking_creadores": 0},
            "maquinas_hechas": [
                {
                    "id": 123, "nombre": "Intranet", "dificultad": "Fácil",
                    "completada_el": "2025-04-01T10:00:00Z",
                    "writeup_url": "https://example.com/w.md",
                    "certificado": {"cert_id": "DL-ABC123", "pdf_url": "https://dockerlabs.es/api/certificado/pdf/DL-ABC123"}
                }
            ],
            "maquinas_creadas": [],
            "writeups": [{"nombre": "Intranet writeup", "url": "https://example.com/w.md", "puntos": 10}]
        }"#;
        let profile: Profile = serde_json::from_str(json).unwrap();
        assert_eq!(profile.username, "noneofyour");
        assert_eq!(profile.progreso.maquinas_hechas, 42);
        assert_eq!(profile.progreso.por_dificultad["Fácil"].totales, 68);
        assert_eq!(profile.estadisticas.puntos_writeups, 120);
        let hecha = &profile.maquinas_hechas[0];
        assert_eq!(hecha.nombre, "Intranet");
        assert_eq!(
            hecha.certificado.as_ref().unwrap().cert_id,
            "DL-ABC123"
        );
        assert_eq!(profile.writeups[0].puntos, Some(10));
    }
}
