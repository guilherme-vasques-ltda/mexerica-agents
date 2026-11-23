//! Conversa pausada depois de um reinício com efeito incerto. A pausa é por bot e conversa: as outras conversas do
//! mesmo bot e os outros bots seguem. Só o dono libera; liberar não repete nada, só devolve a fila à admissão.
use crate::store::{Store, now_ms};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::LockExt;

const PREFIX: &str = "donna.conversation.paused.";

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Paused {
    bots: BTreeMap<String, Mark>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Mark {
    attempt: String,
    at: i64,
}

/// Recusa do permit para a conversa pausada. A reserva já saiu na mesma transação, sem despacho: a admissão é uma
/// por bot e não pode segurar as outras conversas.
#[derive(Debug)]
pub(crate) struct ConversationPaused;

impl std::fmt::Display for ConversationPaused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "esta conversa está pausada depois de um reinício com efeito incerto; nada foi enviado. Ela volta quando \
             você liberar a conversa.",
        )
    }
}

impl std::error::Error for ConversationPaused {}

fn key(chat: &str) -> String {
    format!("{PREFIX}{chat}")
}

/// Registro ilegível é erro: quem lê trata como pausado.
fn read(c: &Connection, chat: &str) -> Result<Paused> {
    let raw: Option<String> = c.query_row("SELECT v FROM kv WHERE k=?1", [key(chat)], |r| r.get(0)).optional()?;
    raw.map(|raw| serde_json::from_str(&raw).context("conversation_pause_invalid"))
        .transpose()
        .map(Option::unwrap_or_default)
}

/// Idempotente: a mesma tentativa repetida depois de outra queda não muda o registro.
pub(crate) fn pause_on(c: &Connection, bot: &str, chat: &str, attempt: &str) -> Result<()> {
    let mut state = read(c, chat)?;
    if state.bots.get(bot).is_some_and(|mark| mark.attempt == attempt) {
        return Ok(());
    }
    state.bots.insert(bot.to_owned(), Mark { attempt: attempt.to_owned(), at: now_ms() });
    c.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=?2",
        params![key(chat), serde_json::to_string(&state)?],
    )?;
    Ok(())
}

pub(crate) fn pause(store: &Store, bot: &str, chat: &str, attempt: &str) -> Result<()> {
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    pause_on(&tx, bot, chat, attempt)?;
    tx.commit()?;
    Ok(())
}

pub(crate) fn paused_on(c: &Connection, bot: &str, chat: &str) -> Result<bool> {
    Ok(read(c, chat)?.bots.contains_key(bot))
}

pub(crate) fn paused(store: &Store, bot: &str, chat: &str) -> Result<bool> {
    paused_on(&store.db.locked(), bot, chat)
}

/// Liberação explícita do dono para a conversa inteira. Devolve os bots que estavam pausados nela.
pub(crate) fn release(store: &Store, chat: &str) -> Result<Vec<String>> {
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let raw: Option<String> = tx.query_row("SELECT v FROM kv WHERE k=?1", [key(chat)], |r| r.get(0)).optional()?;
    let bots = match raw.as_deref().map(serde_json::from_str::<Paused>) {
        Some(Ok(state)) => state.bots.into_keys().collect(),
        // Registro ilegível também é apagado: o dono pediu para liberar esta conversa.
        Some(Err(_)) | None => Vec::new(),
    };
    tx.execute("DELETE FROM kv WHERE k=?1", [key(chat)])?;
    tx.commit()?;
    Ok(bots)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn pausa_por_bot_e_conversa_persiste_e_so_sai_na_liberacao() {
        let dir = std::env::temp_dir().join(format!("pausa-conversa-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("test.db")).unwrap();
        pause(&store, "b1", "chat-a", "t1").unwrap();
        pause(&store, "b1", "chat-a", "t1").unwrap();
        assert!(paused(&store, "b1", "chat-a").unwrap());
        assert!(!paused(&store, "b1", "chat-b").unwrap(), "outra conversa do mesmo bot segue");
        assert!(!paused(&store, "b2", "chat-a").unwrap(), "outro bot na mesma conversa segue");
        drop(store);
        let store = Store::open(&dir.join("test.db")).unwrap();
        assert!(paused(&store, "b1", "chat-a").unwrap(), "a pausa sobrevive ao reinício");
        store.kv_set(&key("chat-c"), "{quebrado").unwrap();
        assert!(paused(&store, "b1", "chat-c").is_err(), "registro ilegível não vira liberado");
        assert_eq!(release(&store, "chat-a").unwrap(), ["b1"]);
        assert!(!paused(&store, "b1", "chat-a").unwrap());
        assert_eq!(release(&store, "chat-c").unwrap(), [] as [String; 0]);
        assert!(!paused(&store, "b1", "chat-c").unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }
}
