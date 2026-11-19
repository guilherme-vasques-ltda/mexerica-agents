//! Descoberta de configuração separada do contexto de execução de um bot.
use crate::{LockExt, store::Store};
use anyhow::{Result, anyhow, bail};
use rusqlite::OptionalExtension;

pub(crate) struct Setup<'a> {
    store: &'a Store,
    pub id: String,
}

impl<'a> Setup<'a> {
    pub fn issue(store: &'a Store, connector: &str) -> Result<Self> {
        let id = uuid::Uuid::new_v4().to_string();
        let db = store.db.locked();
        let expired = db
            .prepare("DELETE FROM donna_guard_setup WHERE expires_at<=?1 RETURNING id")?
            .query_map([crate::store::now_ms()], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        // O namespace é obtido fora da trava DB, respeitando a ordem do Store.
        drop(db);
        let namespace = store.guard_runtime_namespace()?;
        super::runtime::forget_contexts(&namespace, &expired);
        let db = store.db.locked();
        let count: i64 = db.query_row("SELECT COUNT(*) FROM donna_guard_setup", [], |row| row.get(0))?;
        if count >= 16 {
            bail!("Limite de verificações MCP simultâneas");
        }
        db.execute(
            "INSERT INTO donna_guard_setup(id,connector,expires_at) VALUES(?1,?2,?3)",
            rusqlite::params![id, connector, crate::store::now_ms() + 120_000],
        )?;
        Ok(Self { store, id })
    }
}

impl Drop for Setup<'_> {
    fn drop(&mut self) {
        let _ = self.store.guard_context_remove(&self.id);
    }
}

pub(crate) fn snapshot(db: &rusqlite::Connection, id: &str, connector: &str, method: &str) -> Result<Option<i64>> {
    let saved: Option<(String, i64)> = db
        .query_row("SELECT connector,expires_at FROM donna_guard_setup WHERE id=?1", [id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()?;
    let Some((saved, expires)) = saved else { return Ok(None) };
    if saved != connector {
        bail!("Conector não pertence ao setup");
    }
    if expires <= crate::store::now_ms() {
        return Err(anyhow!("Contexto de setup indisponível"));
    }
    if !matches!(
        method,
        "initialize"
            | "ping"
            | "tools/list"
            | "resources/list"
            | "resources/templates/list"
            | "prompts/list"
            | "notification"
            | "server-request"
            | "invalid"
            | "notifications/initialized"
    ) {
        bail!("Setup MCP admite somente handshake e descoberta, sem chamada de ferramenta");
    }
    Ok(Some(expires))
}
