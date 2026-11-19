//! Persistent, automatically allocated bot workspaces. A cwd is an execution
//! default, not an access boundary; explicit project folders remain opt-in.

use crate::service;
use crate::store::BotConfig;
use anyhow::{Context as _, Result};
use std::path::PathBuf;

pub fn path(id: &str) -> PathBuf {
    if crate::fork::motor_exec::configured() {
        crate::fork::motor_roots::workspace(id)
    } else {
        service::data_dir().join("bots").join(id).join("workspace")
    }
}

pub fn is_managed(cfg: &BotConfig) -> bool {
    !cfg.is_group() && (cfg.context.is_some() || crate::fork::pasta::workspace_proprio(cfg))
}

/// Called on a blocking thread before persisting a bot configuration.
/// Empty cwd opts into automatic allocation, including when editing a bot.
pub fn prepare(cfg: &mut BotConfig) -> Result<()> {
    if crate::fork::motor_exec::configured() && !cfg.is_group() && !cfg.cwd.is_empty() {
        return crate::fork::motor_roots::prepare_workspace(std::path::Path::new(&cfg.cwd));
    }
    if cfg.is_group() || !cfg.cwd.is_empty() {
        return Ok(());
    }
    // Only host-issued IDs may become directory components.
    uuid::Uuid::parse_str(&cfg.id).context("invalid bot workspace id")?;
    let dir = path(&cfg.id);
    if crate::fork::motor_exec::configured() {
        crate::fork::motor_roots::prepare_workspace(&dir)?;
    } else {
        std::fs::create_dir_all(&dir).context("creating bot workspace")?;
    }
    cfg.cwd = dir.to_string_lossy().into_owned();
    crate::fork::pasta::normalizar_bot(cfg)
}
