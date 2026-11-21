// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! SQLite persistence. Every mutation stamps a global, monotonically increasing
//! `rev`; clients sync with `since: rev` and never need a separate event log.
//!
//! Calls are synchronous. They're single-row or indexed queries measured in
//! microseconds, so async callers use them directly; the one bulk read (a fresh
//! client's catch-up) is capped per bot, and the history search runs through `spawn_blocking`.

pub mod bootstrap;
pub(crate) mod bot_groups;
pub(crate) mod bots;
mod computers;
pub(crate) mod conectores;
mod devices;
mod entries;
pub(crate) use entries::{
    ENTRY_COLS, QUEUE_ORDER, check_execution_on, entry_on, history_in_session_on, insert_visible_on, queue_key,
    row_entry, send_intent,
};
mod fork_donna;
mod fork_reinicio;
mod fork_settings;
mod guard_hub;
mod management_guard;
pub(crate) use management_guard::ManagementOrigin;
mod memory;
mod memory_session;
mod model;
mod mutex_observado;
pub(crate) mod personal_bootstrap;
pub(crate) mod personal_database;
mod personal_link;
pub(crate) mod personal_prepare;
pub(crate) mod personal_session;
mod schema;
mod team;
mod team_admission;
mod turn_alloc;
pub(crate) use team::read as team_settings_on;
pub use team::{TeamSettings, restrictive};
pub(crate) use team::{read as read_team_settings, restore_team_profile, same_team_profile};
pub(crate) use turn_alloc::alloc_on as alloc_turn_on;

pub use devices::{Device, DeviceSource, Scope};
pub use model::{
    AVATAR_COLORS, AVATAR_SHAPES, BotConfig, BotKind, BotRow, Entry, EntryKind, Lane, Permission, message_text,
    run_session_key,
};

use crate::LockExt;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::sync::{Arc, Mutex};

pub struct Store {
    pub(crate) skills_root: std::path::PathBuf,
    pub(crate) engine_setup_root: std::path::PathBuf,
    /// Conjuntos 3D gerados pelo usuário (fase 2b), fora do repo e do bundle.
    pub(crate) gerados_root: std::path::PathBuf,
    pub(crate) db: mutex_observado::ObservedMutex,
    /// Serializa intenções do usuário e mudanças do MCP manage, sem segurar locks SQLite durante aprovação.
    pub(crate) team_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) connector_lock: Mutex<()>,
    /// Barreira de autoridade do turno: exposição/memória, Stop, revogação do perfil,
    /// admissão contextual de team e commit no pipe de permissões. Nunca atravessa await.
    pub(crate) memory_boundary: Mutex<()>,
    #[cfg(test)]
    before_team_settings_write: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// MX-280: gancho de teste na entrada da manutenção de memória, já dentro
    /// do `spawn_blocking` — prova determinística do trabalho bloqueante.
    #[cfg(test)]
    pub(crate) before_memory_maintain: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// MX-280: gancho de teste na entrada da varredura de arquivos de memória.
    #[cfg(test)]
    pub(crate) before_memory_scan: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// D368: gancho de teste antes de toda gravação do app pareado (`Autoridade::with_db`).
    #[cfg(test)]
    pub(crate) before_owner_write: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// Pertence à única leitura de chaveiro em andamento, inclusive após timeout externo.
    pub(crate) vault_unlock: Arc<tokio::sync::Mutex<()>>,
    #[cfg(test)]
    pub(crate) vault_unlock_attempts: std::sync::atomic::AtomicUsize,
    pub(crate) secret_key: Mutex<Option<zeroize::Zeroizing<Vec<u8>>>>,
    /// MX-260: dedupe em memória e horário da última limpeza do histórico de uso.
    pub(crate) uso_historico: Mutex<crate::fork::uso_historico::Cache>,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Logs a failed read and treats it as "nothing there" — for the read paths that feed
/// best-effort details (previews, unread badges), where one bad query mustn't fail a request.
fn logged<T>(what: &str, r: rusqlite::Result<T>) -> Option<T> {
    r.map_err(|error| tracing::warn!(%error, what, "database read failed")).ok()
}

pub(crate) fn next_rev(c: &Connection) -> Result<i64> {
    Ok(c.query_row(
        "INSERT INTO kv(k, v) VALUES('rev', '1') ON CONFLICT(k) DO UPDATE SET v = CAST(v AS INTEGER) + 1 RETURNING CAST(v AS INTEGER)",
        [],
        |r| r.get(0),
    )?)
}

impl Store {
    /// Durable control state must distinguish a missing key from a failed read.
    pub fn kv_read(&self, k: &str) -> Result<Option<String>> {
        Ok(self.db.locked().query_row("SELECT v FROM kv WHERE k = ?", [k], |r| r.get(0)).optional()?)
    }

    pub fn kv_get(&self, k: &str) -> Option<String> {
        let c = self.db.locked();
        logged("kv", c.query_row("SELECT v FROM kv WHERE k = ?", [k], |r| r.get(0)).optional()).flatten()
    }

    pub fn kv_set(&self, k: &str, v: &str) -> Result<()> {
        let c = self.db.locked();
        c.execute("INSERT INTO kv(k, v) VALUES(?1, ?2) ON CONFLICT(k) DO UPDATE SET v = ?2", params![k, v])?;
        Ok(())
    }

    /// Grava só se o bot ainda existe, na mesma trava da exclusão: um efeito atrasado não recria dado de bot apagado.
    pub(crate) fn kv_set_if_bot(&self, bot: &str, k: &str, v: &str) -> Result<bool> {
        let c = self.db.locked();
        if !matches!(Self::bot_on(&c, bot)?, Some(row) if !row.deleted) {
            return Ok(false);
        }
        c.execute("INSERT INTO kv(k, v) VALUES(?1, ?2) ON CONFLICT(k) DO UPDATE SET v = ?2", params![k, v])?;
        Ok(true)
    }

    pub fn kv_delete(&self, k: &str) -> Result<()> {
        self.db.locked().execute("DELETE FROM kv WHERE k = ?1", [k])?;
        Ok(())
    }

    /// Tombstone D72 e inflight legado são limpos juntos; uma cerca nova só
    /// pode ser concluída pelo ticket capturado do ator.
    pub(crate) fn clear_waiting_inflight(&self, bot: &str) -> Result<()> {
        let mut db = self.db.locked();
        let tx = db.transaction()?;
        tx.execute("DELETE FROM kv WHERE k=?1", [format!("turn.waiting.{bot}")])?;
        crate::fork::bot_admission::clear_legacy_on(&tx, bot)?;
        tx.commit()?;
        Ok(())
    }

    /// Keys starting with `prefix`.
    pub fn kv_prefix(&self, prefix: &str) -> Vec<String> {
        let c = self.db.locked();
        let rows = c
            .prepare("SELECT k FROM kv WHERE substr(k, 1, length(?1)) = ?1")
            .and_then(|mut st| st.query_map([prefix], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>());
        logged("kv prefix", rows).unwrap_or_default()
    }

    pub fn current_rev(&self) -> i64 {
        self.kv_get("rev").and_then(|v| v.parse().ok()).unwrap_or(0)
    }
}

#[cfg(test)]
pub(crate) fn private_fixture_dir(prefix: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
    #[cfg(unix)]
    crate::fork::private_fs::Directory::open(&dir, true).expect("diretório privado da fixture");
    #[cfg(not(unix))]
    std::fs::create_dir_all(&dir).expect("diretório da fixture");
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn temp_store() -> Store {
        let dir = std::env::temp_dir().join(format!("codync-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Store::open(&dir.join("t.db")).unwrap()
    }
}
