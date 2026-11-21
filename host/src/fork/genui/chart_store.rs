//! Registro durável do gráfico de leitura. Como o cartão de escolha, só `super::flow` o chama, depois de
//! autenticar a execução; aqui o vínculo de conversa e sessão é conferido de novo na mesma transação.
//! O gráfico não tem estado nem resposta: a entrada visível (`data.genuiChart`) é imutável e é ela que o
//! app lê ao reabrir a conversa, sem nenhuma chamada a fornecedor.
use super::{
    charts::ValidatedChart,
    storage::{self, Context},
};
use crate::store::{EntryKind, insert_visible_on};
use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::json;

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_genui_charts(
            id TEXT PRIMARY KEY, invocation TEXT NOT NULL UNIQUE, payload_hash TEXT NOT NULL,
            origin_entry TEXT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
            chat TEXT NOT NULL, session TEXT NOT NULL, space TEXT NOT NULL,
            entry_id TEXT NOT NULL UNIQUE REFERENCES entries(id) ON DELETE CASCADE,
            marker_entry TEXT
        );",
    )?;
    Ok(())
}

/// Limpa só os nossos registros na transação de exclusão do bot, sem depender de cascata.
pub(crate) fn on_bot_deleted(c: &Connection, bot: &str) -> Result<()> {
    c.execute(
        "DELETE FROM donna_genui_charts WHERE chat=?1
            OR chat IN (SELECT id FROM donna_space_conversations WHERE bot_id=?1)",
        [bot],
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Receipt {
    pub chart_id: String,
    pub entry_id: String,
    pub marker_entry: Option<String>,
}

/// Grava o gráfico (ou devolve o recibo no replay da mesma invocação). Mesma invocação com outro conteúdo ou
/// outra origem conflita; chamada nova com o mesmo JSON é outro gráfico.
pub(crate) fn create(
    tx: &Transaction<'_>,
    ctx: &Context<'_>,
    invocation: &str,
    chart: &ValidatedChart,
) -> Result<Receipt> {
    let turn = storage::context(tx, ctx)?;
    if invocation.is_empty() || invocation.len() > 256 {
        tracing::warn!("gráfico recusado: identificador de invocação vazio ou acima do limite");
        bail!("genui_invalid_invocation");
    }
    let hash = crate::agent::registry::sha256_hex(chart.snapshot()?.as_bytes());
    let chart = chart.redacted()?;
    let snapshot = chart.snapshot()?;
    let used_by_question: bool =
        tx.query_row("SELECT EXISTS(SELECT 1 FROM donna_genui_questions WHERE invocation=?1)", [invocation], |r| {
            r.get(0)
        })?;
    if used_by_question {
        tracing::warn!("gráfico recusado: a invocação já pertence a uma pergunta de escolha");
        bail!("genui_invocation_conflict");
    }
    let existing = tx
        .query_row(
            "SELECT id,payload_hash,origin_entry,chat,session,space,entry_id,marker_entry
                FROM donna_genui_charts WHERE invocation=?1",
            [invocation],
            |r| {
                Ok((
                    Receipt { chart_id: r.get(0)?, entry_id: r.get(6)?, marker_entry: r.get(7)? },
                    (r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?),
                    (r.get::<_, String>(4)?, r.get::<_, String>(5)?),
                ))
            },
        )
        .optional()?;
    if let Some((receipt, (stored_hash, origin, chat), (session, space))) = existing {
        let same = stored_hash == hash
            && origin == ctx.origin_entry
            && chat == ctx.lane.chat
            && session == ctx.lane.session_id()
            && space == ctx.space;
        if !same {
            // Sem conteúdo nem hash: só o fato, para auditoria.
            tracing::warn!("gráfico recusado: a invocação já pertence a outro conteúdo ou vínculo");
            bail!("genui_invocation_conflict");
        }
        storage::check_projection(tx, ctx, &receipt.entry_id)?;
        return Ok(receipt);
    }
    let id = uuid::Uuid::new_v4().to_string();
    // Texto literal para app antigo; `genuiChart` é a projeção server-owned para o gráfico.
    let (marker, entry) = insert_visible_on(
        tx,
        &uuid::Uuid::new_v4().to_string(),
        ctx.lane,
        EntryKind::Notice,
        turn,
        &json!({"text":chart.fallback(),"genuiChart":{"version":1,"chartId":id,"snapshot":snapshot}}),
    )?;
    let marker_entry = marker.map(|e| e.id);
    tx.execute(
        "INSERT INTO donna_genui_charts(id,invocation,payload_hash,origin_entry,chat,session,space,entry_id,marker_entry)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            id,
            invocation,
            hash,
            ctx.origin_entry,
            ctx.lane.chat,
            ctx.lane.session_id(),
            ctx.space,
            entry.id,
            marker_entry
        ],
    )?;
    storage::check_projection(tx, ctx, &entry.id)?;
    Ok(Receipt { chart_id: id, entry_id: entry.id, marker_entry })
}
