//! D406: o trabalho da pergunta nunca extrai memória. A resposta segue pelo próprio turno.
//! Só usa a Connection recebida; não adquire MB nem reentra no Store.
use super::queue::Context;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

/// Chamado no checkpoint, depois de capturar o turno de fechamento, na mesma transação.
pub(super) fn checkpoint_on(db: &Connection, job: &str) -> Result<Option<&'static str>> {
    let raw: String = db.query_row("SELECT context FROM donna_memory_auto_jobs WHERE id=?1", [job], |r| r.get(0))?;
    let mut context: Context = serde_json::from_str(&raw)?;
    let card: Option<(String, String)> = db
        .query_row(
            "SELECT id,json_extract(data,'$.ownerQuestion.status') FROM entries
             WHERE bot_id=?1 AND COALESCE(session,bot_id)=?2 AND turn=?3 AND kind='notice'
             AND json_extract(data,'$.card')='ownerQuestion' AND json_extract(data,'$.author')=?4
             ORDER BY seq DESC LIMIT 1",
            params![
                context.chat_id,
                context.visible_session.as_deref().unwrap_or(&context.chat_id),
                context.closing_turn,
                context.bot_id
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((id, status)) = card else { return Ok(None) };
    context.owner_question = Some(id);
    db.execute(
        "UPDATE donna_memory_auto_jobs SET context=?2 WHERE id=?1",
        params![job, serde_json::to_string(&context)?],
    )?;
    Ok(Some(if status == "pending" { "owner" } else { "ineligible" }))
}

/// A única porta de fechamento do cartão chama aqui dentro da própria transação.
/// `running` não muda: seu checkpoint ainda vai reconciliar o cartão.
pub(crate) fn close_on(db: &Connection, card: &str) -> Result<()> {
    db.execute(
        "UPDATE donna_memory_auto_jobs SET status='ineligible',reason='ineligible'
         WHERE status='owner' AND outcome='success' AND json_extract(context,'$.ownerQuestion')=?1",
        [card],
    )?;
    Ok(())
}

/// A exclusão real pode apagar o cartão antes da retirada. Recupera só trabalhos não reserváveis.
pub(crate) fn sweep_on(db: &Connection) -> Result<()> {
    db.execute(
        "UPDATE donna_memory_auto_jobs SET status='ineligible',reason='ineligible'
         WHERE status='owner' AND NOT EXISTS(SELECT 1 FROM entries e
             WHERE e.id=json_extract(donna_memory_auto_jobs.context,'$.ownerQuestion')
             AND json_extract(e.data,'$.ownerQuestion.status')='pending')",
        [],
    )?;
    Ok(())
}
