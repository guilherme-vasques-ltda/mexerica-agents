//! Número de turno atômico por chat (D229-6, PR 1).
//!
//! Antes, cada ponto calculava `max_turn(chat) + 1` em duas etapas, e dois turnos que começavam juntos no mesmo chat
//! podiam receber o mesmo número. O contador fica no `kv` e nunca fica abaixo do maior turno gravado no chat, então
//! continua coerente com entradas inseridas por caminhos que só anexam ao turno corrente.
use super::Store;
use crate::LockExt;
use anyhow::Result;

fn key(chat: &str) -> String {
    format!("turn.seq.{chat}")
}

impl Store {
    /// Reserva o próximo número de turno de `chat` numa única instrução SQL, sob a trava do banco.
    pub fn alloc_turn(&self, chat: &str) -> Result<i64> {
        let c = self.db.locked();
        alloc_on(&c, chat)
    }
}

/// Usa a Connection do commit da admissão, sem reservar turno fora da transação.
pub(crate) fn alloc_on(c: &rusqlite::Connection, chat: &str) -> Result<i64> {
    Ok(c.query_row(
        "INSERT INTO kv(k, v)
               VALUES(?1, CAST((SELECT COALESCE(MAX(turn), 0) FROM entries WHERE bot_id = ?2) + 1 AS TEXT))
             ON CONFLICT(k) DO UPDATE SET v = CAST(MAX(CAST(kv.v AS INTEGER),
               (SELECT COALESCE(MAX(turn), 0) FROM entries WHERE bot_id = ?2)) + 1 AS TEXT)
             RETURNING CAST(v AS INTEGER)",
        rusqlite::params![key(chat), chat],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use crate::store::{EntryKind, Lane, Store};
    use std::collections::HashSet;
    use std::path::Path;
    use std::sync::Arc;

    #[test]
    fn turnos_concorrentes_do_mesmo_chat_nunca_repetem() {
        let store = Arc::new(Store::open(Path::new(":memory:")).unwrap());
        store.insert_entry(&Lane::main("grupo"), EntryKind::User, 7, &serde_json::json!({"text": "antes"})).unwrap();
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let store = store.clone();
                std::thread::spawn(move || (0..25).map(|_| store.alloc_turn("grupo").unwrap()).collect::<Vec<_>>())
            })
            .collect();
        let all: Vec<i64> = threads.into_iter().flat_map(|t| t.join().unwrap()).collect();
        let unique: HashSet<i64> = all.iter().copied().collect();
        assert_eq!(unique.len(), all.len(), "nenhum número repetido");
        assert_eq!(*all.iter().min().unwrap(), 8, "começa depois do maior turno já gravado");
        assert_eq!(store.alloc_turn("outro").unwrap(), 1, "cada chat tem sua sequência");
        store.insert_entry(&Lane::main("grupo"), EntryKind::Notice, 500, &serde_json::json!({"text": "x"})).unwrap();
        assert_eq!(store.alloc_turn("grupo").unwrap(), 501, "nunca fica abaixo do que já foi gravado");
    }
}
