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

/// Treats an explicit JSON `null` like a missing field for `String` props.
fn string_or_null<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = serde::Deserialize::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

/// A machine ficha as embedded in the profile lists; extra fields only
/// appear on `maquinas_hechas` (completion date, writeup, certificate).
/// Several string fields can be `null` for some machines.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MaquinaFicha {
    pub id: u64,
    #[serde(deserialize_with = "string_or_null")]
    pub nombre: String,
    #[serde(deserialize_with = "string_or_null")]
    pub dificultad: String,
    #[serde(deserialize_with = "string_or_null")]
    pub clase: String,
    #[serde(deserialize_with = "string_or_null")]
    pub color: String,
    #[serde(deserialize_with = "string_or_null")]
    pub categoria: String,
    #[serde(deserialize_with = "string_or_null")]
    pub autor: String,
    #[serde(deserialize_with = "string_or_null")]
    pub enlace_autor: String,
    #[serde(deserialize_with = "string_or_null")]
    pub fecha: String,
    #[serde(deserialize_with = "string_or_null")]
    pub descripcion: String,
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

/// One of the user's published writeups (`writeups[]` in the profile).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WriteupRef {
    pub maquina: String,
    pub url: String,
    pub tipo: String, // "texto" | "video"
    pub publicado_el: Option<String>,
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
                "catalogo": "docker",
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
            "writeups": [{"maquina": "Whoiam", "url": "https://example.com/whoiam.md", "tipo": "texto", "publicado_el": "2026-08-08T07:32:31Z"}]
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
        assert_eq!(profile.writeups[0].maquina, "Whoiam");
        assert_eq!(profile.writeups[0].tipo, "texto");
    }
}
