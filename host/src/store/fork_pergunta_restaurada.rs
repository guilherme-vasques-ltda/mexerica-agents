//! Pergunta de permissão restaurada uma vez depois de um reinício. O cartão antigo expira (sua resposta nunca mais
//! vale) e, na mesma transação, nasce um cartão novo, com outro ID, que pede aprovação nova. O cartão restaurado é do
//! Hub: não expira em reinícios seguintes e nunca é restaurado de novo, então não se duplica. Só a pergunta volta:
//! nenhuma permissão, execução ou escopo persistente é herdado.
use super::entries::{entry_on, insert_visible_on};
use super::model::{Entry, EntryKind, Lane};
use super::{Store, now_ms};
use crate::LockExt;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

/// Campos do cartão original que pertencem à execução que morreu, ou que o Hub recalcula ao gravar.
const DROPPED: &[&str] = &["selected", "executionBinding", "spaceContext", "restoredAs"];

impl Store {
    /// `true` quando o cartão foi restaurado; `false` deixa o chamador expirá-lo como antes.
    pub(crate) fn restore_permission_question(&self, id: &str) -> Result<bool> {
        let mut db = self.db.locked();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let Some(card) = entry_on(&tx, id)? else { return Ok(false) };
        let Some(question) = restorable_on(&tx, &card)? else {
            tracing::warn!(entry = %id, "cartão não elegível para restauração; expira");
            return Ok(false);
        };
        let lane = Lane {
            chat: card.bot_id.clone(),
            session: (card.session_id != card.bot_id).then(|| card.session_id.clone()),
        };
        let restored = format!("{}-restaurada", card.id);
        if entry_on(&tx, &restored)?.is_none() {
            insert_visible_on(&tx, &restored, &lane, EntryKind::Permission, card.turn, &question)?;
        }
        let mut old = card.data.clone();
        old["status"] = json!("expired");
        old["restoredAs"] = json!(restored);
        Self::update_entry_on(&tx, &card.id, &old, None)?;
        tx.commit()?;
        Ok(true)
    }
}

/// A pergunta nova, ou `None` quando o cartão não pode ser restaurado com segurança (fica só expirado).
fn restorable_on(c: &Connection, card: &Entry) -> Result<Option<Value>> {
    if card.kind != EntryKind::Permission.as_str()
        || card.data["status"] != "pending"
        || card.data.get("restoredFrom").is_some()
        || card.data["toolKind"] == "screen_access"
    {
        return Ok(None);
    }
    let Some(author) = card.data["author"].as_str() else { return Ok(None) };
    match Store::bot_on(c, author)? {
        Some(row) if !row.deleted && !row.config.is_group() => {}
        _ => return Ok(None),
    }
    // Pedido de outro bot tem a própria recuperação (fica interrompido); o cartão dele não volta.
    let delegated: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM donna_delegated_permission_checkpoints WHERE card_id=?1)",
            [&card.id],
            |r| r.get(0),
        )
        .unwrap_or(true);
    if delegated || !same_conversation_on(c, author, &card.bot_id)? {
        return Ok(None);
    }
    // Escopo reduzido: a pergunta restaurada só oferece «uma vez» ou recusar uma vez, nunca «sempre». Sem recusa
    // única, a primeira «recusar sempre» volta como recusa única: o cartão restaurado só encerra, não grava regra.
    let offered = card.data["options"].as_array().map_or(&[][..], Vec::as_slice);
    let mut options: Vec<Value> = offered
        .iter()
        .filter(|option| matches!(option["kind"].as_str(), Some("allow_once" | "reject_once")))
        .cloned()
        .collect();
    if !options.iter().any(|option| option["kind"] == "reject_once")
        && let Some(mut reject) = offered.iter().find(|option| option["kind"] == "reject_always").cloned()
    {
        reject["kind"] = json!("reject_once");
        // O rótulo «sempre» não acompanha; os clientes exigem um `name`, então fica um neutro.
        reject["name"] = json!("Recusar");
        options.push(reject);
    }
    if !options.iter().any(|option| option["kind"] == "allow_once") {
        return Ok(None);
    }
    let mut question = card.data.clone();
    if let Some(fields) = question.as_object_mut() {
        for key in DROPPED {
            fields.remove(*key);
        }
    }
    question["status"] = json!("pending");
    question["options"] = Value::Array(options);
    question["restoredFrom"] = json!(card.id);
    question["restoredAt"] = json!(now_ms());
    Ok(Some(question))
}

/// O autor ainda pertence a esta conversa: a principal dele, uma conversa de Space aberta dele, ou uma sala da qual
/// ainda é membro. Conversa fechada ou revogada não recebe pergunta nova.
fn same_conversation_on(c: &Connection, author: &str, chat: &str) -> Result<bool> {
    if chat == author {
        return Ok(true);
    }
    let open: Option<i64> = c
        .query_row(
            "SELECT 1 FROM donna_space_conversations WHERE id=?1 AND bot_id=?2 AND closed=0",
            params![chat, author],
            |r| r.get(0),
        )
        .optional()?;
    if open.is_some() {
        return Ok(true);
    }
    Ok(Store::bot_on(c, chat)?
        .is_some_and(|room| !room.deleted && room.config.is_group() && room.config.members.iter().any(|m| m == author)))
}
