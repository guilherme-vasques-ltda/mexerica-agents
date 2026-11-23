//! Recuperação por entrada na fila existente; não cria fila/ledger no banco.
use super::{Actor, Queued};
use crate::store::{Entry, EntryKind, Lane};
use anyhow::{Result, anyhow};
use std::time::{Duration, Instant};

impl Actor {
    pub(super) fn stop_subscription_retry(&self, ids: &[String], error: &anyhow::Error) {
        if error.downcast_ref::<crate::fork::assinatura_pessoal::Refusal>().is_none() {
            return;
        }
        for id in ids {
            if let Some(mut entry) = self.hub.store.entry(id)
                && entry.bot_id == self.lane.chat
                && entry.kind == EntryKind::User.as_str()
                && entry.data["status"] == "queued"
            {
                entry.data["status"] = serde_json::json!("failed");
                entry.data["error"] = serde_json::json!(error.to_string());
                if self.hub.set_entry(id, &entry.data).is_none() {
                    tracing::warn!("não foi possível salvar a recusa da assinatura");
                }
            }
        }
    }
    pub(super) fn retry_waiting_queue_restore(&mut self) {
        if self.queue_restore_retry_at.is_some_and(|at| Instant::now() >= at)
            && let Err(error) = self.restore_waiting_queue()
        {
            tracing::warn!(%error, "enumeração da fila será repetida no mesmo ator");
        }
    }

    pub(super) fn queue_has_user(&self, id: &str) -> bool {
        self.queue.iter().any(
            |q| matches!(q, Queued::User { entry_id, .. } | Queued::RestoreUser { entry_id, .. } if entry_id == id),
        )
    }

    fn restored_user(&self, entry: &Entry) -> Result<Queued> {
        let original = self
            .hub
            .store
            .bot(&self.cfg.id)?
            .filter(|bot| !bot.deleted)
            .ok_or_else(|| anyhow!("bot_not_found"))?
            .config;
        let cfg = crate::fork::spaces::execution_config(&self.hub.store, &original, &entry.bot_id)?;
        let text = crate::chat::prompt::of_entry(&self.hub, &cfg, &entry.data);
        let lane = Lane {
            chat: entry.bot_id.clone(),
            session: (entry.session_id != entry.bot_id).then(|| entry.session_id.clone()),
        };
        Ok(Queued::User { lane, entry_id: entry.id.clone(), text })
    }

    pub(super) fn retry_queue_restore(&mut self) {
        if !self.queue.iter().any(|q| {
            matches!(q, Queued::RestoreUser { retry_at, .. }
            if retry_at.is_none_or(|at| Instant::now() >= at))
        }) {
            return;
        }
        // Um snapshot por passagem; o SQLite só fornece as entradas duráveis,
        // nunca escolhe/despacha o próximo item em paralelo ao ator.
        let entries = match self.hub.store.queued_user_entries_for_identity(&self.cfg.id) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(%error, "fila durável indisponível; posições preservadas para retry");
                let at = Instant::now() + Duration::from_secs(5);
                self.queue_restore_retry_at = Some(at);
                for queued in &mut self.queue {
                    if let Queued::RestoreUser { retry_at, .. } = queued {
                        *retry_at = Some(at);
                    }
                }
                return;
            }
        };
        let mut index = 0;
        while index < self.queue.len() {
            let Some(Queued::RestoreUser { entry_id, retry_at }) = self.queue.get(index) else {
                index += 1;
                continue;
            };
            if retry_at.is_some_and(|at| Instant::now() < at) {
                index += 1;
                continue;
            }
            // A consulta também filtra conversa fechada/entrada cancelada.
            let Some(entry) = entries.iter().find(|entry| entry.id == *entry_id) else {
                self.queue.remove(index);
                continue;
            };
            match self.restored_user(entry) {
                Ok(queued) => self.queue[index] = queued,
                Err(error) => {
                    tracing::warn!(%error, "entrada aguarda reconstrução no contexto original");
                    if let Some(Queued::RestoreUser { retry_at, .. }) = self.queue.get_mut(index) {
                        *retry_at = Some(Instant::now() + Duration::from_secs(5));
                    }
                }
            }
            index += 1;
        }
    }

    /// Se nem houve reserva, devolver o lote à sua posição na mesma fila.
    /// Entrada sent/terminal nunca volta a disputar envio por esse caminho.
    pub(super) fn requeue_unreserved_humans(&mut self, ids: &[String]) {
        if self.dispatch.is_some() || self.pending_admission.is_some() {
            return;
        }
        match self.hub.store.queued_user_entries_for_identity(&self.cfg.id) {
            Ok(entries) => {
                self.requeue_humans_from_snapshot(ids, &entries, Some(Instant::now() + Duration::from_secs(5)));
            }
            Err(error) => {
                // A enumeração também é consumidor de segurança se a consulta
                // de elegibilidade falhar antes de devolver o lote à fila.
                self.queue_restore_retry_at = Some(Instant::now() + Duration::from_secs(5));
                tracing::warn!(%error, "reenfileiramento aguarda enumeração durável");
            }
        }
    }

    pub(super) fn requeue_humans_from_snapshot(
        &mut self,
        ids: &[String],
        entries: &[Entry],
        retry_at: Option<Instant>,
    ) {
        for id in ids.iter().rev() {
            let Some(entry) = entries.iter().find(|entry| &entry.id == id) else { continue };
            if !self.queue_has_user(id) && entry.kind == EntryKind::User.as_str() && entry.data["status"] == "queued" {
                // Não mover o lote para antes de uma origem anterior ainda em
                // reconstrução. Os demais itens estavam atrás do lote retirado.
                let at = self
                    .queue
                    .iter()
                    .position(|queued| match queued {
                        Queued::RestoreUser { entry_id, .. } | Queued::User { entry_id, .. } => entries
                            .iter()
                            .find(|before| &before.id == entry_id)
                            .is_none_or(|before| crate::store::queue_key(before) > crate::store::queue_key(entry)),
                        _ => true,
                    })
                    .unwrap_or(self.queue.len());
                self.queue.insert(at, Queued::RestoreUser { entry_id: id.clone(), retry_at });
            }
        }
    }
}
