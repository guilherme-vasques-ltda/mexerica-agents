//! D510 parte 1: chave adicional das ferramentas de tela. Não isola terminal nem oferece tela D83.
use crate::{LockExt, api::devices::Caller, fork::autoridade::Autoridade, hub::Hub, store::Store};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, params};
use serde_json::{Value, json};

mod store;
pub(crate) use store::{clear_target_on, migrate};

pub(crate) const METHOD: &str = "donnaScreenAccess";
pub(crate) const DENIED: &str =
    "o dono ainda não liberou a tela deste computador para você; ele pode ligar em Configurações";
pub(crate) const WARNING: &str = "Esta opção bloqueia as ferramentas de tela; não garante isolamento do terminal.";

pub(crate) fn host_target(hub: &Hub) -> String {
    format!("host:{}", hub.host_id)
}

/// Único predicado, sem Store/Cloud aninhados; erros nunca liberam observação.
pub(crate) fn allowed_on(
    db: &Connection,
    bot: &str,
    target: &str,
    temporary: Option<&crate::screen::fork_permission::Grant>,
) -> Result<()> {
    crate::fork::atividades::revisao::estado::work_allowed_on(db)?;
    let allowed: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_screen_access a JOIN bots b ON b.id=a.bot_id
         WHERE a.bot_id=?1 AND a.target=?2 AND b.deleted=0)",
        params![bot, target],
        |r| r.get(0),
    )?;
    if !allowed {
        let row = Store::bot_on(db, bot)?.ok_or_else(|| anyhow!(DENIED))?;
        ensure!(!row.deleted, DENIED);
        let grant = temporary.ok_or_else(|| anyhow!(DENIED))?;
        grant.check_on(db, bot, target)?;
    }
    Ok(())
}

fn target_on(db: &Connection, hub: &Hub, target: &str) -> Result<()> {
    if target == host_target(hub) {
        return Ok(());
    }
    let active: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_computers c JOIN devices d ON d.key=c.device_key
         WHERE c.id=?1 AND c.removed_at IS NULL AND d.source='local' AND d.scopes='[\"executor\"]')",
        [target],
        |r| r.get(0),
    )?;
    ensure!(active, "Computador não encontrado ou revogado.");
    Ok(())
}

/// Intenção exclusiva do dono, fora de engine.accounts. A mesma origem chega até o commit.
pub(crate) fn call(hub: &Hub, caller: &Caller, body: &Value) -> Result<Value> {
    let original = Autoridade::capturar(hub, caller)?;
    let bot = body["botId"].as_str().ok_or_else(|| anyhow!("Informe o bot."))?;
    let target = body["target"].as_str().ok_or_else(|| anyhow!("Informe o computador."))?;
    let enabled = body["enabled"].as_bool().ok_or_else(|| anyhow!("Informe se a chave está ligada."))?;
    crate::fork::donna_fixa::pedido::antes_de_gravar(&hub.store);
    {
        let _boundary = hub.store.memory_boundary.locked();
        let _emit = hub.emit_lock.locked();
        let (mut control, entries) = original.with_db(hub, None, |db| {
            let row = Store::bot_on(db, bot)?.ok_or_else(|| anyhow!("Bot não encontrado."))?;
            ensure!(!row.deleted && !row.config.is_group(), "Bot não disponível.");
            ensure!(!enabled || target == host_target(hub), "tela ainda indisponível neste computador");
            target_on(db, hub, target)?;
            let control = hub.screen.screen_access_change(enabled)?;
            let mut entries = vec![];
            if enabled {
                db.execute(
                    "INSERT OR IGNORE INTO donna_screen_access(bot_id,target) VALUES(?1,?2)",
                    params![bot, target],
                )?;
            } else {
                let removed = db.execute("DELETE FROM donna_screen_access WHERE bot_id=?1 AND target=?2", params![bot, target])?;
                if removed == 0 && target == host_target(hub) {
                    db.execute("UPDATE kv SET v=CAST(CAST(v AS INTEGER)+1 AS TEXT) WHERE k='donna.bot.config-generation.' || ?1", [bot])?;
                    if control.has_temporary(bot) {
                        db.execute("INSERT INTO donna_screen_access_audit(bot_id,target,enabled,at,choice,effect) VALUES(?1,?2,0,?3,'off',0)",params![bot,target,crate::store::now_ms()])?;
                    }
                }
            }
            if !enabled {
                let mut stmt = db.prepare("SELECT id FROM entries WHERE kind='permission' AND json_extract(data,'$.toolKind')='screen_access' AND json_extract(data,'$.author')=?1 AND json_extract(data,'$.screenAccess.target')=?2 AND json_extract(data,'$.status')='pending'")?;
                let ids = stmt.query_map(params![bot,target],|r| r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
                for id in ids {
                    if let Some(mut card) = Store::entry_on(db,&id)? {
                        card.data["status"] = json!("cancelled");
                        entries.extend(Store::update_entry_on(db,&id,&card.data,None)?);
                    }
                }
            }
            Ok((control,entries))
        })?;
        if target == host_target(hub) {
            control.revoke_temporary(bot);
        }
        for entry in entries {
            let _ = hub.events.send(json!({"type":"entry","rev":entry.rev,"entry":entry}));
        }
    }
    hub.emit_bot(bot);
    let row = hub.store.bot(bot)?.ok_or_else(|| anyhow!("Bot não encontrado."))?;
    Ok(hub.bot_json(&row))
}

/// Campo projetado pelo Hub, nunca aceito em updateBot ou `manage_team`.
pub(crate) fn projection(hub: &Hub, bot: &str) -> Result<Value> {
    let db = hub.store.db.locked();
    let target = host_target(hub);
    let mut rows = vec![json!({"target":target,"name":crate::service::host_name(),"available":true,
        "enabled":allowed_on(&db,bot,&target,None).is_ok(),"notice":WARNING})];
    let mut stmt = db.prepare(
        "SELECT c.id,c.label FROM donna_computers c JOIN devices d ON d.key=c.device_key
         WHERE c.removed_at IS NULL AND d.source='local' AND d.scopes='[\"executor\"]' ORDER BY c.enrolled_at,c.id",
    )?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (id, name) = row?;
        rows.push(json!({"target":id,"name":name,"available":false,"enabled":false,
            "notice":"tela ainda indisponível neste computador"}));
    }
    drop(stmt);
    drop(db);
    if crate::screen::fork_permission::can_end(hub, bot) {
        rows[0]["canEndTemporary"] = json!(true);
    }
    Ok(json!(rows))
}

#[cfg(test)]
mod tests;
