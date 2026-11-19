//! Exposição do gateway pertence ao vínculo de origem, nunca ao turno que chegou depois.
use crate::{LockExt, hub::Hub};
use anyhow::Result;
#[cfg(test)]
use rusqlite::TransactionBehavior;
use rusqlite::{Connection, params};

#[cfg(test)]
pub(crate) fn observe(hub: &Hub, bot: &str, binding: &str) -> Result<()> {
    let _boundary = hub.store.memory_boundary.locked();
    let mut c = hub.store.db.locked();
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    observe_conn(&tx, bot, binding)?;
    tx.commit()?;
    Ok(())
}

/// A pendência já foi absorvida por `bindings::activate`, na transação da ACP exata.
pub(crate) fn bind(hub: &Hub, bot: &str, session: Option<&str>) -> Result<()> {
    let _boundary = hub.store.memory_boundary.locked();
    if let Some(session) = session {
        crate::fork::sessoes::bindings::for_acp(&hub.store, bot, session)?;
    }
    hub.set_runtime(bot, |r| r.session = session.map(str::to_owned));
    Ok(())
}

/// Exposição e recibo na mesma transação de entrega; usa apenas a Connection recebida.
pub(crate) fn observe_conn(c: &Connection, bot: &str, binding: &str) -> Result<()> {
    let origin = crate::fork::sessoes::bindings::resolve(c, binding, bot)?;
    if let Some(acp) = origin.acp_session {
        crate::fork::sessoes::bindings::mark(c, bot, &acp, "gateway MCP")?;
    } else {
        c.execute(
            "INSERT INTO donna_session_gateway_pending(bot,binding,at) VALUES(?1,?2,?3)
            ON CONFLICT(bot,binding) DO UPDATE SET at=excluded.at",
            params![bot, binding, crate::store::now_ms()],
        )?;
    }
    Ok(())
}
