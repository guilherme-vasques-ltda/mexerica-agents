//! Resposta cortada (GUI-203): sem ator vivo, no início do Hub ou no fim do desligamento, o turno que ficou no meio é
//! fechado como interrompido. Nada é reenviado ao motor.
use super::Store;
use super::entries::{ENTRY_COLS, row_entry};
use super::model::{Entry, EntryKind};
use crate::LockExt;
use anyhow::Result;
use rusqlite::{OptionalExtension, params};

impl Store {
    /// Sem ator vivo (início do Hub ou fim do desligamento) ninguém continua a escrever. Num turno cortado (trecho de
    /// resposta ainda aberto, ferramenta sem desfecho ou aprovação sem resposta), a última resposta vira interrompida,
    /// nunca final, e a ferramenta sem desfecho vira falha interrompida. O turno cuja conclusão a espera da assinatura
    /// ainda vai publicar fica com ela. Nada é reenviado ao motor.
    pub fn interrupt_open_work(&self) -> Result<usize> {
        let mut completing = std::collections::BTreeSet::new();
        for key in self.kv_prefix("turn.waiting.") {
            let Some(bot) = key.strip_prefix("turn.waiting.") else { continue };
            if let Some(waiting) = crate::agent::bot::read_waiting_checkpoint(self, bot)?
                && waiting["phase"] == "completing"
                && let (Some(chat), Some(turn)) =
                    (waiting["input"]["chat"].as_str(), waiting["completion"]["turn"].as_i64())
            {
                completing.insert((chat.to_owned(), turn));
            }
        }
        let mut db = self.db.locked();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let open: Vec<Entry> = tx
            .prepare(&format!(
                "SELECT {ENTRY_COLS} FROM entries WHERE kind IN ('agent', 'tool', 'permission') AND json_valid(data)
                  AND ((kind = 'agent' AND json_extract(data, '$.streaming') = 1)
                    OR (kind = 'tool' AND json_extract(data, '$.status') IN ('pending', 'in_progress'))
                    OR (kind = 'permission' AND json_extract(data, '$.status') = 'pending'))"
            ))?
            .query_map([], row_entry)?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|e| !completing.contains(&(e.bot_id.clone(), e.turn)))
            .collect();
        let turns: std::collections::BTreeSet<_> =
            open.iter().map(|e| (e.bot_id.clone(), e.session_id.clone(), e.turn)).collect();
        let mut cut = std::collections::BTreeMap::new();
        for (chat, session, turn) in turns {
            let last = tx
                .query_row(
                    &format!(
                        "SELECT {ENTRY_COLS} FROM entries WHERE bot_id = ?1 AND COALESCE(session, bot_id) = ?2
                          AND turn = ?3 AND kind = 'agent' AND json_valid(data) ORDER BY seq DESC LIMIT 1"
                    ),
                    params![chat, session, turn],
                    row_entry,
                )
                .optional()?;
            if let Some(last) =
                last.filter(|e| e.data["final"] != true && e.data["interrupted"] != true && e.data["delegated"] != true)
            {
                cut.insert(last.id.clone(), last);
            }
        }
        let mut changed = 0;
        for mut e in open {
            if e.kind == "permission" || cut.contains_key(&e.id) {
                continue;
            }
            let Some(fields) = e.data.as_object_mut() else { continue };
            if e.kind == EntryKind::Tool.as_str() {
                fields.insert("status".into(), "failed".into());
                fields.insert("interrupted".into(), true.into());
            } else {
                fields.remove("streaming");
            }
            Self::update_entry_on(&tx, &e.id, &e.data, None)?;
            changed += 1;
        }
        for mut last in cut.into_values() {
            let Some(fields) = last.data.as_object_mut() else { continue };
            fields.remove("streaming");
            fields.insert("interrupted".into(), true.into());
            Self::update_entry_on(&tx, &last.id, &last.data, None)?;
            changed += 1;
        }
        tx.commit()?;
        Ok(changed)
    }
}
