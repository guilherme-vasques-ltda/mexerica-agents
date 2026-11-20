// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! The `entries` table: the transcript of every chat.

use super::model::{Entry, EntryKind, Lane};
use super::{Store, logged, next_rev, now_ms};
use crate::LockExt;
use anyhow::{Result, ensure};
use rusqlite::{OptionalExtension, params};
use serde_json::Value;
use std::fmt::Write as _;

pub(crate) fn row_entry(r: &rusqlite::Row) -> rusqlite::Result<Entry> {
    let data: String = r.get(6)?;
    Ok(Entry {
        seq: r.get(0)?,
        id: r.get(1)?,
        bot_id: r.get(2)?,
        rev: r.get(3)?,
        kind: r.get(4)?,
        turn: r.get(5)?,
        data: serde_json::from_str(&data).unwrap_or(Value::Null),
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
        session_id: r.get(9)?,
    })
}

/// DON-907 (D431): ordem da fila guardada pela reordenação (`data.queueOrder`); sem ela, a ordem de escrita.
pub(crate) const QUEUE_ORDER: &str = "COALESCE(json_extract(data,'$.queueOrder'),seq), seq";

/// A mesma chave de [`QUEUE_ORDER`] para uma entrada já lida.
pub(crate) fn queue_key(entry: &Entry) -> (i64, i64) {
    (entry.data["queueOrder"].as_i64().unwrap_or(entry.seq), entry.seq)
}

pub(crate) const ENTRY_COLS: &str =
    "seq, id, bot_id, rev, kind, turn, data, created_at, updated_at, COALESCE(session, bot_id)";
/// A bot still winding down after its deletion can't leave lines behind (`?2` is the chat).
const LIVE_CHAT: &str = "WHERE NOT EXISTS (SELECT 1 FROM bots WHERE id = ?2 AND deleted = 1)";

/// Consulta pura na transação do chamador; mantém o parser e a autoria canônicos.
pub(crate) fn entry_on(c: &rusqlite::Connection, id: &str) -> Result<Option<Entry>> {
    Ok(c.query_row(&format!("SELECT {ENTRY_COLS} FROM entries WHERE id=?1"), [id], row_entry).optional()?)
}

/// Histórico lido sob a conexão já adquirida pelo chamador.
pub(crate) fn history_in_session_on(
    c: &rusqlite::Connection,
    bot_id: &str,
    session: Option<&str>,
    before_seq: i64,
    limit: i64,
) -> Result<Vec<Entry>> {
    let mut st = c.prepare(&format!(
        "SELECT {ENTRY_COLS} FROM entries WHERE bot_id = ?1 AND seq < ?2
           AND (?4 IS NULL OR CASE WHEN ?4 = bot_id THEN session IS NULL ELSE session = ?4 END)
         ORDER BY seq DESC LIMIT ?3"
    ))?;
    let mut rows: Vec<Entry> =
        st.query_map(params![bot_id, before_seq, limit, session], row_entry)?.collect::<rusqlite::Result<_>>()?;
    rows.reverse();
    Ok(rows)
}

pub(crate) fn insert_row(
    c: &rusqlite::Connection,
    id: &str,
    lane: &Lane,
    kind: &str,
    turn: i64,
    data: &Value,
) -> Result<Entry> {
    let data = crate::fork::spaces::stamp_entry(c, &lane.chat, data)?;
    let added = c.execute(
        &format!(
            "INSERT INTO entries(id, bot_id, rev, kind, turn, data, created_at, updated_at, session)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8 {LIVE_CHAT}"
        ),
        params![id, lane.chat, next_rev(c)?, kind, turn, data.to_string(), now_ms(), lane.session],
    )?;
    ensure!(added == 1, "the chat was deleted");
    Ok(c.query_row(&format!("SELECT {ENTRY_COLS} FROM entries WHERE id = ?"), [id], row_entry)?)
}

/// O marcador de sessão para apps antigos, quando a entrada seguinte muda de sessão (`fork::sessoes::marker`).
pub(crate) fn insert_marker(c: &rusqlite::Connection, lane: &Lane, turn: i64) -> Result<Option<Entry>> {
    let Some(data) = crate::fork::sessoes::marker::needed(c, lane)? else { return Ok(None) };
    insert_row(c, &uuid::Uuid::new_v4().to_string(), lane, EntryKind::Notice.as_str(), turn, &data).map(Some)
}

pub(crate) fn read_on(c: &rusqlite::Connection, id: &str) -> Result<Option<Entry>> {
    Ok(c.query_row(&format!("SELECT {ENTRY_COLS} FROM entries WHERE id = ?1"), [id], row_entry).optional()?)
}

/// Insere na transação do chamador; não abre transação nem adquire o lock do Store.
/// Quem publica precisa publicar o marcador antes da entrada e só após o commit.
pub(crate) fn insert_visible_on(
    c: &rusqlite::Connection,
    id: &str,
    lane: &Lane,
    kind: EntryKind,
    turn: i64,
    data: &Value,
) -> Result<(Option<Entry>, Entry)> {
    let marker = insert_marker(c, lane, turn)?;
    let entry = insert_row(c, id, lane, kind.as_str(), turn, data)?;
    Ok((marker, entry))
}

/// Guarda comum na Connection do efeito; nenhuma exceção de destino para escrita normal.
pub(crate) fn check_execution_on(
    db: &rusqlite::Connection,
    lane: &Lane,
    execution: Option<&crate::fork::sessoes::ExecutionRef>,
) -> Result<()> {
    if let Some(execution) = execution {
        let session = crate::fork::sessoes::bindings::assert_execution_live(db, execution)?;
        ensure!(
            session.chat_id == lane.chat && lane.session.as_deref().unwrap_or(&lane.chat) == session.id,
            "chamada vencida"
        );
    }
    Ok(())
}

/// O que o reenvio de uma mensagem do dono com a mesma chave precisa repetir: texto, sessão, mensagem citada e anexos
/// por nome e tamanho (cada tentativa do app sobe o arquivo de novo, com outro id).
pub(crate) fn send_intent(text: &str, attachments: &[Value], reply_to: Option<&str>, session: Option<&str>) -> Value {
    let files: Vec<Value> =
        attachments.iter().map(|a| serde_json::json!({"name": a["name"], "size": a["size"]})).collect();
    serde_json::json!({"text": text, "attachments": files, "replyTo": reply_to, "session": session})
}

impl Store {
    #[cfg(test)]
    pub fn insert_entry(&self, lane: &Lane, kind: EntryKind, turn: i64, data: &Value) -> Result<Entry> {
        Ok(self.insert_visible(lane, kind, turn, data)?.1)
    }

    /// Grava a entrada e, antes dela, o marcador de sessão quando a sessão muda (D229-5), na mesma transação.
    /// Quem publica precisa publicar os dois, nessa ordem.
    #[cfg(test)]
    pub fn insert_visible(
        &self,
        lane: &Lane,
        kind: EntryKind,
        turn: i64,
        data: &Value,
    ) -> Result<(Option<Entry>, Entry)> {
        self.insert_visible_in_execution(lane, kind, turn, data, None)
    }

    pub(crate) fn insert_visible_in_execution(
        &self,
        lane: &Lane,
        kind: EntryKind,
        turn: i64,
        data: &Value,
        execution: Option<&crate::fork::sessoes::ExecutionRef>,
    ) -> Result<(Option<Entry>, Entry)> {
        let mut c = self.db.locked();
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        check_execution_on(&tx, lane, execution)?;
        let (marker, entry) = insert_visible_on(&tx, &uuid::Uuid::new_v4().to_string(), lane, kind, turn, data)?;
        tx.commit()?;
        Ok((marker, entry))
    }

    /// Deterministic IDs make routine transcript publication retryable after a crash.
    pub fn insert_entry_once(
        &self,
        id: &str,
        lane: &Lane,
        kind: EntryKind,
        turn: i64,
        data: &Value,
    ) -> Result<(Option<Entry>, Entry, bool)> {
        let mut c = self.db.locked();
        let tx = c.transaction()?;
        if let Some(entry) =
            tx.query_row(&format!("SELECT {ENTRY_COLS} FROM entries WHERE id = ?"), [id], row_entry).optional()?
        {
            ensure!(entry.bot_id == lane.chat, "entry_id_belongs_to_other_conversation");
            return Ok((None, entry, false));
        }
        let (marker, entry) = insert_visible_on(&tx, id, lane, kind, turn, data)?;
        tx.commit()?;
        Ok((marker, entry, true))
    }

    pub fn update_entry(&self, id: &str, data: &Value) -> Result<Option<Entry>> {
        self.update_entry_in_execution(id, data, None)
    }
    pub(crate) fn update_entry_in_execution(
        &self,
        id: &str,
        data: &Value,
        execution: Option<&crate::fork::sessoes::ExecutionRef>,
    ) -> Result<Option<Entry>> {
        let mut db = self.db.locked();
        let c = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let entry = Self::update_entry_on(&c, id, data, execution)?;
        c.commit()?;
        Ok(entry)
    }

    /// Mesma atualização na `Connection` recebida (CF-14: Tx da guarda da autoridade original); sem commit.
    pub(crate) fn update_entry_on(
        c: &rusqlite::Connection,
        id: &str,
        data: &Value,
        execution: Option<&crate::fork::sessoes::ExecutionRef>,
    ) -> Result<Option<Entry>> {
        crate::fork::sessoes::bindings::check_optional(c, execution)?;
        let before: Option<String> =
            c.query_row("SELECT data FROM entries WHERE id = ?1", [id], |r| r.get(0)).optional()?;
        let before: Value = before.as_deref().map(serde_json::from_str).transpose()?.unwrap_or(Value::Null);
        let data = super::guard_hub::preserve(&before, data);
        let data = crate::fork::genui::preserve_response(&before, data);
        let chat: Option<String> =
            c.query_row("SELECT bot_id FROM entries WHERE id=?1", [id], |r| r.get(0)).optional()?;
        let mut data = if let Some(chat) = chat { crate::fork::spaces::stamp_entry(c, &chat, &data)? } else { data };
        for key in ["executionBinding", "intent", "spaceContext"] {
            if let Some(value) = before.get(key) {
                data[key] = value.clone();
            }
        }
        let rev = next_rev(c)?;
        c.execute(
            "UPDATE entries SET rev = ?2, data = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, rev, data.to_string(), now_ms()],
        )?;
        let entry =
            c.query_row(&format!("SELECT {ENTRY_COLS} FROM entries WHERE id = ?1"), [id], row_entry).optional()?;
        Ok(entry)
    }

    pub fn entry(&self, id: &str) -> Option<Entry> {
        logged("entry", Self::entry_on(&self.db.locked(), id)).flatten()
    }

    pub(crate) fn entry_on(c: &rusqlite::Connection, id: &str) -> rusqlite::Result<Option<Entry>> {
        c.query_row(&format!("SELECT {ENTRY_COLS} FROM entries WHERE id = ?1"), [id], row_entry).optional()
    }

    /// Reenvio com a mesma chave na mesma conversa: devolve a mensagem original quando a intenção é a mesma e recusa
    /// com `nonce_intent_conflict` quando mudou. Mensagem gravada antes da intenção existir tem a sua reconstruída.
    pub(crate) fn replay_on(c: &rusqlite::Connection, chat: &str, data: &Value) -> Result<Option<Entry>> {
        let nonce = data["clientNonce"].as_str().unwrap_or_default();
        if nonce.is_empty() {
            return Ok(None);
        }
        let Some(existing) = c
            .query_row(
                &format!(
                    "SELECT {ENTRY_COLS} FROM entries WHERE bot_id = ?1 AND kind = 'user' AND json_valid(data)
                       AND json_extract(data, '$.clientNonce') = ?2"
                ),
                params![chat, nonce],
                row_entry,
            )
            .optional()?
        else {
            return Ok(None);
        };
        let stored = existing.data.get("intent").cloned().unwrap_or_else(|| {
            let attachments = existing.data["attachments"].as_array().cloned().unwrap_or_default();
            let session = (existing.session_id != existing.bot_id).then_some(existing.session_id.as_str());
            send_intent(
                existing.data["text"].as_str().unwrap_or_default(),
                &attachments,
                existing.data["replyTo"]["id"].as_str(),
                session,
            )
        });
        ensure!(stored == data["intent"], "nonce_intent_conflict");
        Ok(Some(existing))
    }

    pub fn find_by_nonce(&self, bot_id: &str, nonce: &str) -> Option<Entry> {
        let c = self.db.locked();
        let row = c.query_row(
            &format!(
                "SELECT {ENTRY_COLS} FROM entries WHERE bot_id = ?1 AND kind = 'user' AND json_extract(data, '$.clientNonce') = ?2"
            ),
            params![bot_id, nonce],
            row_entry,
        );
        logged("entry by nonce", row.optional()).flatten()
    }

    pub fn max_turn(&self, bot_id: &str) -> i64 {
        let c = self.db.locked();
        let n = c.query_row("SELECT COALESCE(MAX(turn), 0) FROM entries WHERE bot_id = ?1", [bot_id], |r| r.get(0));
        logged("max turn", n).unwrap_or(0)
    }

    /// Entries changed after `since`. A fresh client (`since == 0`) only gets the
    /// newest `cap` entries per bot and pages older ones in with `history`.
    #[cfg(test)]
    pub fn entries_since(&self, since: i64, cap: i64) -> Result<Vec<Entry>> {
        let c = self.db.locked();
        let sql = if since == 0 {
            format!(
                "SELECT {ENTRY_COLS} FROM (SELECT *, ROW_NUMBER() OVER (PARTITION BY bot_id ORDER BY seq DESC) AS rn FROM entries)
                 WHERE rn <= ?2 AND rev > ?1 ORDER BY seq"
            )
        } else {
            format!("SELECT {ENTRY_COLS} FROM entries WHERE rev > ?1 AND ?2 > 0 ORDER BY seq")
        };
        let mut st = c.prepare(&sql)?;
        Ok(st.query_map(params![since, cap], row_entry)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn entries_since_in_space(&self, space: &str, since: i64, cap: i64) -> Result<Vec<Entry>> {
        let c = self.db.locked();
        let sql=format!("SELECT {ENTRY_COLS} FROM (SELECT e.*,ROW_NUMBER() OVER (PARTITION BY bot_id ORDER BY seq DESC) AS rn
            FROM entries e WHERE EXISTS(SELECT 1 FROM donna_space_conversations c WHERE c.id=e.bot_id AND c.space_id=?1))
            WHERE rev>?2 AND (?2>0 OR rn<=?3) ORDER BY seq");
        Ok(c.prepare(&sql)?.query_map(params![space, since, cap], row_entry)?.collect::<rusqlite::Result<_>>()?)
    }

    /// Older entries of a chat.
    pub fn history(&self, bot_id: &str, before_seq: i64, limit: i64) -> Result<Vec<Entry>> {
        self.history_in_session(bot_id, None, before_seq, limit)
    }

    /// `session`: only that session's entries; the main one (the chat's id) reads `session IS NULL`, before `LIMIT`.
    pub fn history_in_session(
        &self,
        bot_id: &str,
        session: Option<&str>,
        before_seq: i64,
        limit: i64,
    ) -> Result<Vec<Entry>> {
        history_in_session_on(&self.db.locked(), bot_id, session, before_seq, limit)
    }

    /// Cards still waiting for the user to sign in to agent `backend`, oldest first.
    // ponytail: scans notices without an index; sign-ins are rare, add one if it shows up in profiles.
    pub fn pending_sign_ins(&self, backend: &str) -> Result<Vec<Entry>> {
        let c = self.db.locked();
        let mut st = c.prepare(&format!(
            "SELECT {ENTRY_COLS} FROM entries WHERE kind = 'notice'
               AND json_extract(data, '$.connectionRequest.kind') = 'agent'
               AND json_extract(data, '$.connectionRequest.status') = 'pending'
               AND json_extract(data, '$.connectionRequest.backend') = ?1
             ORDER BY rev"
        ))?;
        Ok(st.query_map([backend], row_entry)?.collect::<rusqlite::Result<_>>()?)
    }

    /// Chat-visible messages (the user's, final replies) in `bot_id`'s chat that
    /// contain every term (case-insensitive for ASCII), newest first, from `from` (a `seq`) back.
    // ponytail: substring scan, no index (CJK needs no tokenizer this way): 0.6 s over 180k messages of one bot.
    // An FTS5 trigram table once a bot's search passes a second.
    pub fn search_messages(&self, bot_id: &str, terms: &[String], from: Option<i64>, limit: i64) -> Result<Vec<Entry>> {
        let mut sql = format!(
            "SELECT {ENTRY_COLS} FROM entries WHERE bot_id = ?1 AND seq <= ?3 AND
               (kind = 'user' OR (kind = 'agent' AND json_extract(data, '$.final') = 1))"
        );
        for i in 0..terms.len() {
            let _ = write!(sql, " AND instr(lower(json_extract(data, '$.text')), ?{}) > 0", i + 4);
        }
        sql.push_str(" ORDER BY seq DESC LIMIT ?2");
        let mut args: Vec<rusqlite::types::Value> =
            vec![bot_id.to_owned().into(), limit.into(), from.unwrap_or(i64::MAX).into()];
        args.extend(terms.iter().map(|t| t.to_lowercase().into()));
        let c = self.db.locked();
        let mut st = c.prepare(&sql)?;
        Ok(st.query_map(rusqlite::params_from_iter(args), row_entry)?.collect::<rusqlite::Result<_>>()?)
    }

    /// The newest `limit` bot-to-bot notices in `bot_id`'s main chat that involve `peer_id`, oldest first.
    pub fn bot_conversation(&self, bot_id: &str, peer_id: &str, limit: i64) -> Result<Vec<Entry>> {
        let c = self.db.locked();
        let mut st = c.prepare(&format!(
            "SELECT {ENTRY_COLS} FROM entries WHERE bot_id = ?1 AND kind = 'notice'
               AND json_type(data, '$.botMessage') = 'object'
               AND ?2 IN (json_extract(data, '$.botMessage.sourceBotId'), json_extract(data, '$.botMessage.targetBotId'))
             ORDER BY seq DESC LIMIT ?3"
        ))?;
        let mut rows: Vec<Entry> =
            st.query_map(params![bot_id, peer_id, limit], row_entry)?.collect::<rusqlite::Result<_>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// The newest entry of a bot's chat.
    pub fn last_seq(&self, bot_id: &str) -> i64 {
        let c = self.db.locked();
        logged(
            "last seq",
            c.query_row("SELECT COALESCE(MAX(seq), 0) FROM entries WHERE bot_id = ?1", [bot_id], |r| r.get(0)),
        )
        .unwrap_or(0)
    }

    /// Chat-visible messages (the user's, final replies) in `lane` after `after_seq`, oldest first.
    pub fn messages_after(&self, lane: &Lane, after_seq: i64, limit: i64) -> Result<Vec<Entry>> {
        let c = self.db.locked();
        let mut st = c.prepare(&format!(
            "SELECT {ENTRY_COLS} FROM entries WHERE bot_id = ?1 AND seq > ?2 AND
               (kind = 'user' OR (kind = 'agent' AND json_extract(data, '$.final') = 1))
             ORDER BY seq DESC LIMIT ?3"
        ))?;
        let mut rows: Vec<Entry> =
            st.query_map(params![lane.chat, after_seq, limit], row_entry)?.collect::<rusqlite::Result<_>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// The `seq` of `bot`'s last reply in `lane` (0 if it hasn't spoken there).
    pub fn last_spoke(&self, lane: &Lane, bot: &str) -> i64 {
        let c = self.db.locked();
        let n = c.query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM entries WHERE bot_id = ?1 AND kind = 'agent'
               AND json_extract(data, '$.final') = 1 AND json_extract(data, '$.author') = ?2",
            params![lane.chat, bot],
            |r| r.get(0),
        );
        logged("last spoke", n).unwrap_or(0)
    }

    /// Transfere a fila ao turno em voo e só grava espera quando há uma retomada D72 ativa.
    #[cfg(test)]
    pub fn claim_user_turn(&self, bot: &str, checkpoint: Option<&str>, ids: &[String]) -> Result<Vec<Entry>> {
        self.claim_user_turn_in_conversation(bot, bot, checkpoint, ids)
    }
    #[cfg(test)]
    pub fn claim_user_turn_in_conversation(
        &self,
        bot: &str,
        chat: &str,
        checkpoint: Option<&str>,
        ids: &[String],
    ) -> Result<Vec<Entry>> {
        let mut db = self.db.locked();
        let tx = db.transaction()?;
        let token = checkpoint
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .and_then(|value| value["executionToken"].as_str().map(str::to_owned))
            .unwrap_or_else(|| if checkpoint.is_none() { uuid::Uuid::new_v4().to_string() } else { String::new() });
        tx.execute(
            "INSERT INTO kv(k, v) VALUES(?1, ?2) ON CONFLICT(k) DO UPDATE SET v = ?2",
            params![
                format!("turn.inflight.{bot}"),
                serde_json::json!({"started":now_ms(),"conversationId":chat,"executionToken":token}).to_string()
            ],
        )?;
        let entries = Self::claim_user_turn_on(&tx, bot, chat, checkpoint, ids, None, &token)?;
        tx.commit()?;
        Ok(entries)
    }

    /// Consumidor transacional da permissão de despacho, sem outro inflight.
    /// Confere entries.session esperado sem resolver ou abrir outra sessão.
    pub(crate) fn claim_user_turn_on(
        c: &rusqlite::Connection,
        bot: &str,
        chat: &str,
        checkpoint: Option<&str>,
        ids: &[String],
        expected_session: Option<&str>,
        execution_token: &str,
    ) -> Result<Vec<Entry>> {
        ensure!(!execution_token.is_empty(), "admission_execution_missing");
        ensure!(
            chat == bot
                || c.query_row(
                    "SELECT EXISTS(SELECT 1 FROM donna_space_conversations WHERE id=?1 AND bot_id=?2 AND closed=0)",
                    params![chat, bot],
                    |r| r.get::<_, bool>(0)
                )?,
            "cross_conversation_identity"
        );
        // Sessões PR5: extra encerrada não recebe Dispatch nem Resume, na mesma Tx do permit.
        crate::fork::sessoes::encerrar::assert_open_on(c, chat, expected_session)?;
        let saved_token: Option<String> = if checkpoint.is_some() {
            let saved: Option<String> =
                c.query_row("SELECT v FROM kv WHERE k=?1", [format!("turn.waiting.{bot}")], |r| r.get(0)).optional()?;
            saved
                .map(|raw| serde_json::from_str::<Value>(&raw))
                .transpose()?
                .and_then(|value| value["executionToken"].as_str().map(str::to_owned))
        } else {
            None
        };
        if let Some(input) = checkpoint {
            let value: Value = serde_json::from_str(input)?;
            ensure!(
                value["executionToken"].as_str() == Some(execution_token),
                "admission_checkpoint_execution_conflict"
            );
            c.execute(
                "INSERT INTO kv(k, v) VALUES(?1, ?2) ON CONFLICT(k) DO UPDATE SET v = ?2",
                params![format!("turn.waiting.{bot}"), input],
            )?;
        }
        let mut entries = Vec::new();
        for id in ids {
            let mut entry = c.query_row(&format!("SELECT {ENTRY_COLS} FROM entries WHERE id = ?1"), [id], row_entry)?;
            ensure!(entry.bot_id == chat, "cross_conversation_entry");
            // None é a principal (SQL NULL); Some é a sessão extra capturada.
            let expected_session = expected_session.unwrap_or(chat);
            ensure!(entry.session_id == expected_session, "admission_session_conflict");
            if entry.kind != EntryKind::User.as_str() {
                continue;
            }
            let retry = checkpoint.and_then(|raw| serde_json::from_str::<Value>(raw).ok()).is_some_and(|w| {
                w["phase"] == "retrying"
                    && w["executionToken"].as_str() == Some(execution_token)
                    && w["input"]["chat"] == chat
                    && w["input"]["entries"]
                        .as_array()
                        .is_some_and(|entries| entries.iter().any(|entry| entry.as_str() == Some(id.as_str())))
            });
            ensure!(
                entry.data["status"] == "queued"
                    || (entry.data["status"] == "sent" && retry && saved_token.as_deref() == Some(execution_token)),
                "admission_entry_no_longer_eligible"
            );
            entry.data["status"] = Value::String("sent".into());
            if let Some(data) = entry.data.as_object_mut() {
                data.remove("subscriptionQueued");
            }
            entry.rev = next_rev(c)?;
            entry.updated_at = now_ms();
            let changed = c.execute(
                "UPDATE entries SET data = ?2, rev = ?3, updated_at = ?4
                 WHERE id = ?1 AND bot_id = ?5 AND COALESCE(session, bot_id) = ?6",
                params![id, entry.data.to_string(), entry.rev, entry.updated_at, chat, expected_session],
            )?;
            ensure!(changed == 1, "admission_session_conflict");
            entries.push(entry);
        }
        Ok(entries)
    }

    pub fn queued_user_entries_for_identity(&self, bot: &str) -> Result<Vec<Entry>> {
        let c = self.db.locked();
        let sql = format!(
            "SELECT {ENTRY_COLS} FROM entries e WHERE (bot_id=?1 OR EXISTS(SELECT 1 FROM donna_space_conversations d WHERE d.id=e.bot_id AND d.bot_id=?1 AND d.closed=0)) AND kind='user' AND json_extract(data,'$.status')='queued' ORDER BY {QUEUE_ORDER}"
        );
        Ok(c.prepare(&sql)?.query_map([bot], row_entry)?.collect::<rusqlite::Result<_>>()?)
    }

    /// Mensagens na fila D72 continuam duráveis mesmo se o turno em espera acabou
    /// imediatamente antes do crash e seu checkpoint já foi removido.
    #[cfg(test)]
    pub fn queued_user_entries(&self, bot: &str) -> Result<Vec<Entry>> {
        let c = self.db.locked();
        let mut st = c.prepare(&format!(
            "SELECT {ENTRY_COLS} FROM entries WHERE bot_id = ?1
            AND kind = 'user' AND json_extract(data, '$.status') = 'queued' ORDER BY seq"
        ))?;
        Ok(st.query_map([bot], row_entry)?.collect::<rusqlite::Result<_>>()?)
    }

    /// Expira cards/canais transitórios, sem consumir humanos queued ainda não
    /// despachados. Conserva também o marcador legado de fila de assinatura.
    pub fn expire_pending(&self) -> Result<usize> {
        for key in self.kv_prefix("turn.waiting.") {
            let Some(bot) = key.strip_prefix("turn.waiting.") else { continue };
            if !crate::agent::bot::read_waiting_checkpoint(self, bot)?
                .as_ref()
                .is_some_and(crate::agent::bot::checkpoint_waiting_active)
            {
                continue;
            }
            self.db.locked().execute(
                "UPDATE entries SET data = json_set(data, '$.subscriptionQueued', json('true'))
                WHERE kind = 'user' AND json_extract(data, '$.status') = 'queued' AND bot_id = ?1",
                [bot],
            )?;
        }
        self.expire_unrecoverable()
    }

    fn expire_unrecoverable(&self) -> Result<usize> {
        let ids: Vec<String> = {
            let c = self.db.locked();
            let mut st = c.prepare(
                "SELECT id FROM entries WHERE (kind = 'permission' AND json_extract(data, '$.status') = 'pending'
                    AND NOT COALESCE(json_extract(data,'$.toolKind')='screen_access'
                      AND json_extract(data,'$.screenAccess.expiresAt')>?1
                      AND json_extract(data,'$.screenAccess.expiresAt')<=created_at+600000,0))
                  OR (kind = 'notice' AND json_extract(data, '$.delegationId') IS NOT NULL
                      AND json_extract(data, '$.status') IN ('queued', 'sent'))",
            )?;
            st.query_map([now_ms()], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        for id in &ids {
            if let Some(mut e) = self.entry(id) {
                let status = if e.kind == EntryKind::Notice.as_str() {
                    let heading = e.data["heading"].as_str().unwrap_or("Bot request");
                    e.data["text"] = format!(
                        "{heading}\nInterrupted by host restart. Partial work may have happened; check before retrying."
                    )
                    .into();
                    if let Some(message) = e.data.get_mut("botMessage").and_then(Value::as_object_mut) {
                        message.insert(
                            "detail".into(),
                            "Interrupted by host restart. Partial work may have happened; check before retrying."
                                .into(),
                        );
                    }
                    e.data["style"] = "error".into();
                    "failed"
                } else {
                    "expired"
                };
                e.data["status"] = status.into();
                self.update_entry(id, &e.data)?;
            }
        }
        Ok(ids.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::temp_store;
    use serde_json::json;

    #[test]
    fn reenvio_com_a_mesma_chave_compara_texto_sessao_citacao_e_anexos() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let file = json!({"id": "u1", "name": "a.txt", "size": 3});
        let intent = send_intent("oi", std::slice::from_ref(&file), None, None);
        let data = json!({"text": "oi", "clientNonce": "n", "attachments": [file.clone()], "intent": intent});
        let original = store.insert_entry(&Lane::main("b"), EntryKind::User, 1, &data).unwrap();
        let legacy = json!({"text": "velho", "clientNonce": "v", "attachments": [file.clone()], "replyTo": {"id": "m1", "text": "x"}});
        let old = store.insert_entry(&Lane::main("b"), EntryKind::User, 2, &legacy).unwrap();
        let c = store.db.locked();
        let ask =
            |nonce: &str, intent: Value| Store::replay_on(&c, "b", &json!({"clientNonce": nonce, "intent": intent}));
        let reuploaded = json!({"id": "u2", "name": "a.txt", "size": 3});
        assert_eq!(
            ask("n", send_intent("oi", std::slice::from_ref(&reuploaded), None, None)).unwrap().unwrap().id,
            original.id
        );
        for changed in [
            send_intent("tchau", std::slice::from_ref(&file), None, None),
            send_intent("oi", &[], None, None),
            send_intent("oi", &[json!({"id": "u1", "name": "b.txt", "size": 3})], None, None),
            send_intent("oi", std::slice::from_ref(&file), Some("m1"), None),
            send_intent("oi", std::slice::from_ref(&file), None, Some("extra")),
        ] {
            let error = ask("n", changed).unwrap_err();
            assert!(error.to_string().contains("nonce_intent_conflict"), "{error}");
        }
        assert_eq!(
            ask("v", send_intent("velho", std::slice::from_ref(&file), Some("m1"), None)).unwrap().unwrap().id,
            old.id
        );
        assert!(ask("v", send_intent("velho", std::slice::from_ref(&file), None, None)).is_err(), "citação diferente");
        assert!(ask("outra", send_intent("oi", &[], None, None)).unwrap().is_none());
        assert!(Store::replay_on(&c, "b", &json!({"clientNonce": "", "intent": intent})).unwrap().is_none());
    }

    #[test]
    fn transferencia_da_fila_e_checkpoint_sao_uma_transacao() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let entry = store
            .insert_entry(
                &Lane::main("bot"),
                EntryKind::User,
                2,
                &serde_json::json!({"text":"pedido", "status":"queued", "subscriptionQueued":true}),
            )
            .unwrap();
        let checkpoint = r#"{"executionToken":"fixture-token","input":{"chat":"bot"}}"#;
        store.db.locked().execute_batch("CREATE TRIGGER reject_sent BEFORE UPDATE ON entries WHEN json_extract(NEW.data, '$.status') = 'sent' BEGIN SELECT RAISE(FAIL, 'sent indisponível'); END;").unwrap();
        assert!(store.claim_user_turn("bot", Some(checkpoint), std::slice::from_ref(&entry.id)).is_err());
        assert!(store.kv_get("turn.waiting.bot").is_none());
        assert_eq!(store.entry(&entry.id).unwrap().data["status"], "queued");
        store.db.locked().execute_batch("DROP TRIGGER reject_sent").unwrap();
        store.claim_user_turn("bot", Some(checkpoint), std::slice::from_ref(&entry.id)).unwrap();
        assert_eq!(store.kv_get("turn.waiting.bot").as_deref(), Some(checkpoint));
        assert_eq!(store.entry(&entry.id).unwrap().data["status"], "sent");
        assert!(store.entry(&entry.id).unwrap().data.get("subscriptionQueued").is_none());
    }

    #[test]
    fn tombstone_nao_promove_nova_fila_a_espera() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let entry = store
            .insert_entry(&Lane::main("bot"), EntryKind::User, 2, &json!({"text":"turno posterior", "status":"queued"}))
            .unwrap();
        store.kv_set("turn.waiting.bot", r#"{"phase":"finished"}"#).unwrap();
        assert_eq!(store.expire_pending().unwrap(), 0);
        let entry = store.entry(&entry.id).unwrap();
        assert_eq!(entry.data["status"], "queued");
        assert_ne!(entry.data["subscriptionQueued"], true);
    }

    #[test]
    fn espera_corrompida_e_removida_sem_consumir_fila_humana() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        for raw in ["{quebrado", r#"{"phase":"waiting"}"#] {
            let entry = store
                .insert_entry(
                    &Lane::main("bot"),
                    EntryKind::User,
                    1,
                    &serde_json::json!({"text":"pedido", "status":"queued"}),
                )
                .unwrap();
            store.kv_set("turn.waiting.bot", raw).unwrap();
            assert_eq!(store.expire_pending().unwrap(), 0);
            assert_eq!(store.entry(&entry.id).unwrap().data["status"], "queued");
            assert!(store.kv_get("turn.waiting.bot").is_none());
        }
    }

    #[test]
    fn fila_da_assinatura_sobrevive_ao_fim_do_checkpoint() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let entry = store
            .insert_entry(
                &Lane::main("bot"),
                EntryKind::User,
                2,
                &serde_json::json!({"text":"depois", "status":"queued"}),
            )
            .unwrap();
        store
            .kv_set(
                "turn.waiting.bot",
                &serde_json::json!({
                    "phase":"waiting", "retryAt":0, "observedAt":0, "backend":"codex",
                    "input":{"chat":"bot", "entries":["pedido"], "text":"pedido", "hidden":false,
                        "transient":false, "backend":"codex"}
                })
                .to_string(),
            )
            .unwrap();
        assert_eq!(store.expire_pending().unwrap(), 0);
        store.kv_delete("turn.waiting.bot").unwrap();
        assert_eq!(store.expire_pending().unwrap(), 0);
        let queue = store.queued_user_entries("bot").unwrap();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].id, entry.id);
    }

    #[test]
    fn history_search_matches_every_term_in_chat_messages() {
        let s = temp_store();
        let main = Lane::main("b1");
        s.insert_entry(&main, EntryKind::User, 1, &json!({"text": "我們用 Swift 6 寫 Codync"})).unwrap();
        s.insert_entry(&main, EntryKind::Agent, 1, &json!({"text": "swift narration", "final": false})).unwrap();
        s.insert_entry(&main, EntryKind::Agent, 1, &json!({"text": "Swift 6 strict mode is on", "final": true}))
            .unwrap();
        s.insert_entry(&Lane::main("b2"), EntryKind::User, 1, &json!({"text": "Swift 6 elsewhere"})).unwrap();
        let texts = |terms: &[&str]| -> Vec<String> {
            let terms: Vec<String> = terms.iter().map(|t| (*t).to_owned()).collect();
            s.search_messages("b1", &terms, None, 10)
                .unwrap()
                .into_iter()
                .map(|e| e.data["text"].as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(texts(&["SWIFT", "6"]), ["Swift 6 strict mode is on", "我們用 Swift 6 寫 Codync"]);
        assert_eq!(texts(&["codync", "寫"]), ["我們用 Swift 6 寫 Codync"]);
        assert_eq!(texts(&["narration"]).len(), 0);
    }

    #[test]
    fn history_search_reads_back_from_a_message() {
        let s = temp_store();
        let main = Lane::main("b1");
        let seqs: Vec<i64> = (1..=4)
            .map(|i| s.insert_entry(&main, EntryKind::User, 1, &json!({"text": format!("note {i}")})).unwrap().seq)
            .collect();
        let texts = |terms: &[String], from| -> Vec<String> {
            let rows = s.search_messages("b1", terms, from, 2).unwrap();
            rows.into_iter().map(|e| e.data["text"].as_str().unwrap().to_owned()).collect()
        };
        assert_eq!(texts(&[], Some(seqs[2])), ["note 3", "note 2"], "starts at the message itself");
        assert_eq!(texts(&["1".to_owned()], Some(seqs[2])), ["note 1"]);
        assert_eq!(texts(&[], None), ["note 4", "note 3"]);
    }

    #[test]
    fn reinicio_expira_aprovacoes_e_preserva_humanos_ainda_nao_enviados() {
        let s = temp_store();
        s.insert_entry(&Lane::main("b1"), EntryKind::Permission, 1, &serde_json::json!({"status": "pending"})).unwrap();
        let q =
            s.insert_entry(&Lane::main("b1"), EntryKind::User, 2, &serde_json::json!({"status": "queued"})).unwrap();
        s.insert_entry(&Lane::main("b1"), EntryKind::User, 1, &serde_json::json!({"status": "sent"})).unwrap();
        assert_eq!(s.expire_pending().unwrap(), 1);
        assert_eq!(s.entry(&q.id).unwrap().data["status"], "queued");
        assert_eq!(s.expire_pending().unwrap(), 0, "idempotent");
    }

    #[test]
    fn bot_conversation_lists_one_peers_notices_oldest_first() {
        let s = temp_store();
        let notice = |bot: &str, source: &str, target: &str, text: &str| {
            let data = json!({"text": text, "sourceBotId": source, "targetBotId": target,
                "botMessage": {"sourceBotId": source, "targetBotId": target, "text": text}});
            s.insert_entry(&Lane::main(bot), EntryKind::Notice, 1, &data).unwrap();
        };
        notice("a", "a", "b", "first");
        notice("a", "a", "c", "other peer");
        notice("a", "b", "a", "second");
        notice("b", "a", "b", "other chat");
        s.insert_entry(&Lane::main("a"), EntryKind::Notice, 1, &json!({"text": "plain"})).unwrap();
        let old = s
            .insert_entry(
                &Lane::main("a"),
                EntryKind::Notice,
                1,
                &json!({"text": "Messaged b: old", "heading": "Messaged b: old", "delegationId": "old",
                "sourceBotId": "a", "targetBotId": "b", "status": "completed"}),
            )
            .unwrap();
        let old_data = old.data.clone();
        let texts: Vec<_> = s
            .bot_conversation("a", "b", 10)
            .unwrap()
            .into_iter()
            .map(|e| e.data["text"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(texts, ["first", "second"]);
        assert_eq!(s.entry(&old.id).unwrap().data, old_data, "existing notice is untouched");
        assert_eq!(s.bot_conversation("a", "b", 1).unwrap().len(), 1, "limit keeps the newest");
    }
}
