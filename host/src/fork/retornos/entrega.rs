use crate::{
    hub::Hub,
    store::{Entry, EntryKind, Lane, Store, insert_visible_on},
};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;

pub(super) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_request_deliveries(
            request_id TEXT PRIMARY KEY REFERENCES donna_session_requests(id),
            space_id TEXT NOT NULL, source_turn INTEGER NOT NULL,
            sealed INTEGER NOT NULL DEFAULT 0, body TEXT
        );
        CREATE INDEX IF NOT EXISTS donna_request_deliveries_turn
            ON donna_request_deliveries(source_turn);",
    )?;
    Ok(())
}

pub(crate) fn add_on(db: &Connection, id: &str, space: &str, turn: i64) -> Result<()> {
    db.execute(
        "INSERT INTO donna_request_deliveries(request_id,space_id,source_turn) VALUES(?1,?2,?3)",
        params![id, space, turn],
    )?;
    Ok(())
}

pub(super) fn stage_on(db: &Connection, id: &str, body: Option<&str>) -> Result<()> {
    db.execute(
        "UPDATE donna_request_deliveries SET body=COALESCE(?2,'') WHERE request_id=?1 AND body IS NULL
           AND EXISTS (SELECT 1 FROM donna_session_requests r WHERE r.id=?1 AND r.state NOT IN ('accepted','running'))",
        params![id, body.map(|text| crate::agent::acp::truncate(text.trim(), 8000))],
    )?;
    Ok(())
}

pub(crate) fn complete_on(db: &Connection, id: &str, body: Option<&str>) -> Result<Vec<Entry>> {
    stage_on(db, id, body)?;
    let group = db
        .query_row("SELECT requester_execution_ref FROM donna_session_requests WHERE id=?1", [id], |r| {
            r.get::<_, String>(0)
        })
        .optional()?;
    group.map_or_else(|| Ok(Vec::new()), |group| publish_on(db, &group))
}

pub(crate) fn complete(hub: &Hub, id: &str) -> Result<()> {
    hub.conector_publish(|| super::com_tx(&hub.store, |db| Ok(((), complete_on(db, id, None)?))))
}

pub(crate) fn seal(hub: &Hub, lane: &Lane, turn: i64) {
    let result = hub.conector_publish(|| {
        super::com_tx(&hub.store, |db| {
            db.execute(
                "UPDATE donna_request_deliveries SET sealed=1 WHERE source_turn=?1 AND request_id IN (
              SELECT id FROM donna_session_requests WHERE requester_chat_id=?2 AND requester_session_id=?3)",
                params![turn, lane.chat, lane.session_id()],
            )?;
            let mut query = db.prepare(
                "SELECT DISTINCT r.requester_execution_ref FROM donna_request_deliveries d
             JOIN donna_session_requests r ON r.id=d.request_id
             WHERE d.source_turn=?1 AND r.requester_chat_id=?2 AND r.requester_session_id=?3",
            )?;
            let groups = query
                .query_map(params![turn, lane.chat, lane.session_id()], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut entries = Vec::new();
            for group in groups {
                entries.extend(publish_on(db, &group)?);
            }
            Ok(((), entries))
        })
    });
    if result.is_err() {
        tracing::warn!("não foi possível fechar o lote de retornos");
    }
}

pub(super) fn recover_on(db: &Connection) -> Result<()> {
    db.execute("UPDATE donna_request_deliveries SET sealed=1,body=COALESCE(body,'')", [])?;
    let mut query = db.prepare(
        "SELECT DISTINCT r.requester_execution_ref FROM donna_request_deliveries d
         JOIN donna_session_requests r ON r.id=d.request_id",
    )?;
    let groups = query.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for group in groups {
        publish_on(db, &group)?;
    }
    Ok(())
}

struct Return {
    chat: String,
    session: String,
    space: String,
    turn: i64,
    sealed: bool,
    bot: String,
    outcome: String,
    body: Option<String>,
}

fn publish_on(db: &Connection, group: &str) -> Result<Vec<Entry>> {
    let mut query = super::privacidade::grouped_peer_returns(db)?;
    let rows = query
        .query_map([group], |r| {
            Ok(Return {
                chat: r.get(0)?,
                session: r.get(1)?,
                space: r.get(2)?,
                turn: r.get(3)?,
                sealed: r.get(4)?,
                bot: r.get(5)?,
                outcome: r.get(6)?,
                body: r.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let Some(first) = rows.first() else { return Ok(Vec::new()) };
    if rows.iter().any(|r| !r.sealed || r.body.is_none()) {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    let live = |id: &str| -> Result<_> {
        Ok(Store::bot_on(db, id)?
            .filter(|r| !r.deleted && !r.config.hidden && !r.config.is_group() && r.config.space_id == first.space))
    };
    if live(&first.chat)?.is_some()
        && crate::fork::sessoes::store::get(db, &first.session)?
            .is_some_and(|s| s.chat_id == first.chat && s.state == "open" && s.runs_on.is_none())
    {
        let lane =
            Lane { chat: first.chat.clone(), session: (first.session != first.chat).then(|| first.session.clone()) };
        let mut blocks = Vec::new();
        for r in &rows {
            anyhow::ensure!(
                r.chat == first.chat && r.session == first.session && r.space == first.space && r.turn == first.turn,
                "retorno_lote_invalido"
            );
            let Some(bot) = live(&r.bot)? else {
                blocks.push("Destino indisponível. Confira o pedido antes de reenviar.".to_owned());
                continue;
            };
            let body = r.body.as_deref().context("retorno_lote_invalido")?;
            let detail = match (r.outcome.as_str(), body) {
                ("completed", "") => "Terminou, mas não deixou texto de retorno.",
                ("completed", text) => text,
                ("restart_uncertain", _) => {
                    "Interrompido. O trabalho pode ter começado; confira o destino antes de reenviar."
                }
                ("restart_before_start", _) => "Interrompido antes do envio. Não foi reenviado.",
                ("timeout", _) => "O prazo terminou. Confira se houve trabalho parcial no destino.",
                ("cancelled", _) => "Pedido cancelado.",
                _ => "Não concluiu. Confira o estado e a conta do bot antes de reenviar.",
            };
            if let Some(question) = crate::fork::berga_delegacao::question(detail) {
                blocks.push(format!("Pergunta de {}:\n{question}", bot.config.name));
            } else {
                blocks.push(format!("{}:\n{detail}", bot.config.name));
            }
        }
        let (marker, entry) = insert_visible_on(
            db,
            &uuid::Uuid::new_v4().to_string(),
            &lane,
            EntryKind::Agent,
            first.turn,
            &json!({"text": format!("Retorno dos pedidos\n\n{}", blocks.join("\n\n")), "author":first.chat,"final":true}),
        )?;
        entries.extend(marker);
        entries.push(entry);
    }
    db.execute(
        "DELETE FROM donna_request_deliveries WHERE request_id IN (
           SELECT id FROM donna_session_requests WHERE requester_execution_ref=?1)",
        [group],
    )?;
    Ok(entries)
}
