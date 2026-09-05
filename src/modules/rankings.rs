//! Community rankings: machine creators and writeup authors.

use anyhow::Result;
use serde::Deserialize;

use crate::modules::session::DlSession;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AuthorRank {
    pub id: u64,
    pub nombre: String,
    pub maquinas: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WriteupRank {
    pub id: u64,
    pub nombre: String,
    pub puntos: u64,
}

pub struct RankingFetcher {
    session: DlSession,
}

impl RankingFetcher {
    pub fn new(session: DlSession) -> Self {
        Self { session }
    }

    pub async fn autores(&self) -> Result<Vec<AuthorRank>> {
        let body = self.session.get("/api/ranking_autores").await?;
        Ok(serde_json::from_str(&body)?)
    }

    pub async fn writeups(&self) -> Result<Vec<WriteupRank>> {
        let body = self.session.get("/api/ranking_writeups").await?;
        Ok(serde_json::from_str(&body)?)
    }
}
