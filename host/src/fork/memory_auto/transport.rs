//! Vínculo de transporte do MCP memory. Não é `Authority` nem `ExecutionRef`.
//! D62: impede replay sem furtar credencial nova; não isola processo com leitura livre do DB/ambiente Unix.
use crate::{LockExt, hub::Hub, store::BotConfig};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

pub(crate) const ENV: &str = "DONNABOT_MEMORY_TRANSPORT";
pub(crate) const ARG: &str = "__memoryTransport";

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS donna_memory_transport_generations(bot TEXT PRIMARY KEY,generation TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS donna_memory_transports(capability TEXT PRIMARY KEY,bot TEXT NOT NULL,
        chat TEXT NOT NULL,slot TEXT NOT NULL,backend TEXT NOT NULL,generation TEXT NOT NULL,session TEXT,active INTEGER NOT NULL DEFAULT 1);
        CREATE INDEX IF NOT EXISTS donna_memory_transport_slot ON donna_memory_transports(bot,slot,active);
        UPDATE donna_memory_transports SET active=0;")?;
    Ok(())
}

/// Cada Conn efetivamente aberto invalida todos os servidores de sua geração anterior.
pub(crate) fn new_generation(hub: &Hub, cfg: &BotConfig, chat: &str, backend: &str) -> Result<()> {
    let runtime = hub.runtime(&cfg.id);
    let turn = runtime
        .memory_turn_token
        .or(runtime.preparing_turn_token)
        .ok_or_else(|| anyhow::anyhow!("memory_transport_unavailable"))?;
    let _boundary = hub.store.memory_boundary.locked();
    let mut db = hub.store.db.locked();
    let tx = db.transaction()?;
    let generation = uuid::Uuid::new_v4().to_string();
    tx.execute("UPDATE donna_memory_transports SET active=0 WHERE bot=?1", [&cfg.id])?;
    tx.execute(
        "INSERT INTO donna_memory_transport_generations(bot,generation) VALUES(?1,?2)
        ON CONFLICT(bot) DO UPDATE SET generation=excluded.generation",
        params![cfg.id, generation],
    )?;
    tx.execute(
        "INSERT INTO donna_memory_auto_engines(bot,token,chat,backend) VALUES(?1,?2,?3,?4)
        ON CONFLICT(bot) DO UPDATE SET token=excluded.token,chat=excluded.chat,backend=excluded.backend",
        params![cfg.id, turn, chat, backend],
    )?;
    tx.commit()?;
    Ok(())
}

/// O segredo só sai para a definição env do processo MCP, nunca para RPC de modelo, recibo ou log.
pub(crate) fn issue(
    hub: &Hub,
    cfg: &BotConfig,
    chat: &str,
    slot: Option<&str>,
    session: Option<&str>,
) -> Result<String> {
    let backend =
        super::queue::prepared_engine(hub, &cfg.id)?.ok_or_else(|| anyhow::anyhow!("memory_transport_unavailable"))?;
    let slot = crate::guard_hub::tool_origin::slot_value(slot);
    let mut db = hub.store.db.locked();
    let tx = db.transaction()?;
    let generation: String =
        tx.query_row("SELECT generation FROM donna_memory_transport_generations WHERE bot=?1", [&cfg.id], |r| {
            r.get(0)
        })?;
    let existing: Option<String> = tx
        .query_row(
            "SELECT capability FROM donna_memory_transports WHERE bot=?1 AND chat=?2
        AND slot=?3 AND backend=?4 AND generation=?5 AND active=1 AND session=?6",
            params![cfg.id, chat, slot, backend, generation, session],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(existing) = existing {
        return Ok(existing);
    }
    tx.execute("UPDATE donna_memory_transports SET active=0 WHERE bot=?1 AND slot=?2", params![cfg.id, slot])?;
    let capability = uuid::Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO donna_memory_transports(capability,bot,chat,slot,backend,generation) VALUES(?1,?2,?3,?4,?5,?6)",
        params![capability, cfg.id, chat, slot, backend, generation],
    )?;
    tx.commit()?;
    Ok(capability)
}

fn from_definition(servers: &Value) -> Option<&str> {
    servers.as_array()?.iter().find(|s|s["name"]=="memory")?["env"].as_array()?.iter()
        .find(|e|e["name"]==ENV)?["value"].as_str()
}

pub(crate) fn bind(
    hub: &Hub,
    cfg: &BotConfig,
    chat: &str,
    slot: Option<&str>,
    servers: &Value,
    session: &str,
) -> Result<()> {
    // Foreign não recebe MCPs; adicional própria vincula o transporte à sua sessão real.
    if cfg.context.as_ref().is_some_and(|context| context.foreign_identity) {
        return Ok(());
    }
    let capability = from_definition(servers).ok_or_else(|| anyhow::anyhow!("memory_transport_unavailable"))?;
    let backend = super::queue::prepared_engine(hub, &cfg.id)?;
    let changed=hub.store.db.locked().execute("UPDATE donna_memory_transports SET session=?6 WHERE capability=?1
        AND bot=?2 AND chat=?3 AND slot=?4 AND active=1 AND generation=(SELECT generation FROM donna_memory_transport_generations WHERE bot=?2)
        AND (session IS NULL OR session=?6) AND backend=?5",
        params![capability,cfg.id,chat,crate::guard_hub::tool_origin::slot_value(slot),backend,session])?;
    ensure!(changed == 1, "memory_transport_unavailable");
    Ok(())
}

/// Reuso de sessão só confere o vínculo existente; não emite nonce ou novas definições MCP.
pub(crate) fn assert_bound(hub: &Hub, cfg: &BotConfig, chat: &str, slot: Option<&str>, session: &str) -> Result<()> {
    if cfg.context.as_ref().is_some_and(|context| context.foreign_identity) {
        return Ok(());
    }
    let backend = super::queue::prepared_engine(hub, &cfg.id)?;
    let bound: bool = hub.store.db.locked().query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_memory_transports t
        JOIN donna_memory_transport_generations g ON g.bot=t.bot AND g.generation=t.generation
        WHERE t.bot=?1 AND t.chat=?2 AND t.slot=?3 AND t.session=?4 AND t.backend=?5 AND t.active=1)",
        params![cfg.id, chat, crate::guard_hub::tool_origin::slot_value(slot), session, backend],
        |row| row.get(0),
    )?;
    ensure!(bound, "memory_transport_unavailable");
    Ok(())
}

/// Valores ausentes/alheios/antigos nunca habilitam mutantes. Não propaga o valor em erro.
pub(crate) fn backend(hub: &Hub, bot: &str, capability: Option<&str>, mutation: bool) -> Result<Option<String>> {
    let Some(capability) = capability.filter(|s| uuid::Uuid::parse_str(s).is_ok()) else { return Ok(None) };
    let runtime = hub.runtime(bot);
    let Some(lane) = runtime.lane else { return Ok(None) };
    let Some(engine) = super::queue::engine(hub, bot)? else { return Ok(None) };
    let db = hub.store.db.locked();
    let row: Option<(String, Option<String>)> = db
        .query_row(
            "SELECT t.backend,t.session FROM donna_memory_transports t
        JOIN donna_memory_transport_generations g ON g.bot=t.bot AND g.generation=t.generation
        WHERE t.capability=?1 AND t.bot=?2 AND t.chat=?3 AND t.active=1 AND t.backend=?4",
            params![capability, bot, lane.chat, engine],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row.and_then(|(backend, session)| {
        let bound = session.as_deref() == runtime.session.as_deref() && session.is_some();
        if bound || (!mutation && session.is_none()) { Some(backend) } else { None }
    }))
}

/// D298: só para o anúncio (`__tools`). Durante o preparo o motor vê nome, descrição e
/// schema das ações pelo transporte vigente do motor preparado, sem conteúdo de memória.
/// Não é permissão: toda chamada continua exigindo `backend` com o token ativo do Hub.
pub(crate) fn listing_backend(hub: &Hub, bot: &str, capability: Option<&str>) -> Result<Option<String>> {
    let Some(capability) = capability.filter(|s| uuid::Uuid::parse_str(s).is_ok()) else { return Ok(None) };
    let Some(prepared) = super::queue::prepared_engine(hub, bot)? else { return Ok(None) };
    // Como em `backend` (#237): o preparo não adota transporte já vinculado a outra sessão.
    let session = hub.runtime(bot).session;
    let current: bool = hub.store.db.locked().query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_memory_transports t
        JOIN donna_memory_transport_generations g ON g.bot=t.bot AND g.generation=t.generation
        WHERE t.capability=?1 AND t.bot=?2 AND t.backend=?3 AND t.active=1
        AND (t.session IS NULL OR t.session=?4))",
        params![capability, bot, prepared, session],
        |row| row.get(0),
    )?;
    Ok(current.then_some(prepared))
}

pub(crate) fn revoke_slot(hub: &Hub, bot: &str, slot: Option<&str>) -> Result<()> {
    hub.store.db.locked().execute(
        "UPDATE donna_memory_transports SET active=0 WHERE bot=?1 AND slot=?2",
        params![bot, crate::guard_hub::tool_origin::slot_value(slot)],
    )?;
    Ok(())
}
