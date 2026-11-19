//! Controles capturados pelo Hub. Nunca trocam de conversa por estado do ator.
use super::{Actor, Queued};
use crate::{LockExt, store::EntryKind};
use anyhow::{Result, anyhow, ensure};
use serde_json::{Value, json};
impl Actor {
    /// Sessões PR5: o `newSession` legado recomeça só a principal (`primary_restart`), sem tocar extras.
    pub(super) async fn primary_new_session(
        &mut self,
        chat: &str,
        revision: i64,
        autoridade: &crate::fork::autoridade::Autoridade,
    ) -> Result<Value> {
        self.primary_restart(chat, revision, autoridade).await
    }

    pub(super) async fn context_control(
        &mut self,
        chat: &str,
        revision: i64,
        entry_id: Option<&str>,
        reset: bool,
    ) -> Result<Value> {
        let space = crate::fork::spaces::chat_space(&self.hub, chat)?;
        let c = self.hub.store.space_conversation(&space, chat)?;
        ensure!(c.bot_id == self.cfg.id && c.revision == revision, "conversation_revision_conflict");
        if entry_id.is_none() && !reset {
            let slot = (chat != self.cfg.id).then(|| format!("conversation:{chat}"));
            self.hub.pontes.revogar_slot(&self.cfg.id, slot.as_deref());
        }
        if reset {
            ensure!(self.turn.is_none() && self.waiting.is_none() && self.finishing.is_none(), "conversation_busy");
            ensure!(
                !self.hub.store.queued_user_entries_for_identity(&self.cfg.id)?.iter().any(|e| e.bot_id == chat),
                "conversation_queued"
            );
            let cutoff = self.hub.store.last_seq(chat) + 1;
            let cancelled = {
                let mut db = self.hub.store.db.locked();
                let tx = db.transaction()?;
                let rev = crate::store::next_rev(&tx)?;
                tx.execute("UPDATE donna_space_conversations SET context_start_seq=?2,revision=?3,continuity=NULL,session_id=NULL WHERE id=?1 AND revision=?4",rusqlite::params![chat,cutoff,rev,revision])?;
                // G1b: o corte e o cancelamento dos cartões de escolha commitam juntos.
                let cancelled = crate::fork::genui::flow::cancel_on(&tx, chat, None)?;
                tx.commit()?;
                cancelled
            };
            crate::fork::genui::flow::publish_committed(&self.hub, cancelled);
            // MB-1b (ajuste A): limpar a conversa retira as perguntas pendentes dela.
            crate::fork::owner_question::withdraw_chat(&self.hub, chat, None);
            if chat == self.cfg.id {
                self.set_session(None, None);
            } else {
                self.set_session(Some(&format!("conversation:{chat}")), None);
            }
            if self.lane.chat == chat {
                if let Some(conn) = self.conn.take() {
                    conn.kill().await;
                }
                self.handoff = None;
                self.announce = None;
                self.turn_session = None;
                self.turn_binding = None;
            }
            let c = self.hub.store.space_conversation(&space, chat)?;
            crate::fork::spaces::emit_conversation(&self.hub, &c);
            return Ok(json!({"context":crate::fork::spaces::conversation_context(&c),"conversation":c,"reset":true}));
        }
        let mut ids = Vec::new();
        if let Some(id) = entry_id {
            let entry = self.hub.store.entry(id).ok_or_else(|| anyhow!("message_not_found"))?;
            ensure!(entry.bot_id == chat && entry.kind == EntryKind::User.as_str(), "cross_conversation_reference");
            ids.push(id.to_owned());
        } else {
            ids.extend(
                self.hub
                    .store
                    .queued_user_entries_for_identity(&self.cfg.id)?
                    .into_iter()
                    .filter(|e| e.bot_id == chat)
                    .map(|e| e.id),
            );
            if let Some(input) = &self.input
                && input.chat == chat
            {
                ids.extend(input.entries.clone());
            }
        }
        let active = self.lane.chat == chat
            && (entry_id.is_none() || self.input.as_ref().is_some_and(|i| i.entries.iter().any(|id| ids.contains(id))));
        let ceiling = if entry_id.is_none() && chat == self.cfg.id {
            self.hub.team.cancel_from(&self.cfg.id);
            self.hub.routines.cancel_queued(&self.hub.store, &self.cfg.id)?;
            self.queue.retain(|q| !matches!(q, Queued::Routine(_) | Queued::BotRequest(_)));
            // MX-145: rotina e pedido parados pelo teto também são cancelados.
            self.drop_ceiling_holds(&super::fork_teto::CeilingDrop::StopMain { entries: &ids })
        } else {
            self.drop_ceiling_holds(&super::fork_teto::CeilingDrop::Entries(&ids))
        };
        self.queue.retain(
            |q| !matches!(q,Queued::User{entry_id,..} | Queued::RestoreUser{entry_id,..} if ids.contains(entry_id)),
        );
        for id in &ids {
            if let Some(mut e) = self.hub.store.entry(id)
                && e.bot_id == chat
                && e.data["status"] == "queued"
            {
                e.data["status"] = "cancelled".into();
                self.hub.set_entry(id, &e.data);
            }
        }
        if active {
            self.cancel_turn().await;
        }
        // MX-145: o resto do Parar valeu; cancelamento não durável de algo parado
        // pelo teto volta como erro explícito (o item segue parado, sem execução).
        ceiling?;
        Ok(json!({"context":crate::fork::spaces::conversation_context(&c),"stopRequested":active,"entryIds":ids}))
    }
}
