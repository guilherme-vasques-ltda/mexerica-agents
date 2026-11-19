//! Fonte administrativa same-Space. Não usa nem ativa o resolvedor preparatório de audiência.
use super::model::Bot;
use crate::fork::spaces;
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

pub(super) struct SourceScope {
    pub bots: Vec<Bot>,
    pub primary_bot_id: Option<String>,
    pub catalog_version: String,
}

pub(super) fn digest(value: &impl Serialize) -> Result<String> {
    let mut value = serde_json::to_value(value)?;
    value.sort_all_objects();
    Ok(Sha256::digest(serde_json::to_vec(&value)?).iter().fold(String::from("v1:"), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    }))
}

/// Chamado com a Connection da transação; não chama Store, Cloud ou I/O externo.
pub(super) fn read(c: &Connection, space: &str) -> Result<SourceScope> {
    super::validation::id(space)?;
    let raw: String = c.query_row("SELECT v FROM kv WHERE k='donna.spaces.v1'", [], |r| r.get(0))?;
    let spaces: spaces::Snapshot = serde_json::from_str(&raw)?;
    ensure!(spaces.spaces.iter().any(|s| s.id == space), "space_not_found: este Space não existe.");
    let mut st = c.prepare("SELECT id,config,avatar_config FROM bots WHERE deleted=0 ORDER BY id")?;
    let rows =
        st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?)))?;
    let mut bots = Vec::new();
    for row in rows {
        let (id, profile, avatar) = row?;
        let cfg = crate::fork_avatars::restored(&profile, avatar.as_deref()).map_err(|_| {
            anyhow::anyhow!("catalog_unavailable: há um bot ilegível; repare o cadastro antes de editar Times.")
        })?;
        ensure!(id == cfg.id, "catalog_unavailable: identidade do bot inconsistente.");
        if cfg.space_id == space && !cfg.is_group() {
            bots.push(Bot {
                id: cfg.id,
                name: cfg.name,
                space_id: cfg.space_id,
                title: cfg.title,
                avatar_color: cfg.avatar_color,
                avatar_shape: cfg.avatar_shape,
                avatar_config: cfg.avatar_config,
                mexeriquinha: cfg.mexeriquinha,
                hidden: cfg.hidden,
            });
        }
    }
    ensure!(bots.len() <= super::validation::MAX_MEMBERS, "times_limit: há bots demais neste Space.");
    let settings: Option<String> =
        c.query_row("SELECT v FROM kv WHERE k='team.settings.v1'", [], |r| r.get(0)).optional()?;
    let settings: crate::store::TeamSettings =
        settings.map(|s| serde_json::from_str(&s)).transpose()?.unwrap_or_default();
    let primary_bot_id = settings.primary_bot_id.filter(|id| bots.iter().any(|b| &b.id == id));
    let catalog_version = digest(&serde_json::json!({"bots":bots,"primaryBotId":primary_bot_id}))?;
    Ok(SourceScope { bots, primary_bot_id, catalog_version })
}
