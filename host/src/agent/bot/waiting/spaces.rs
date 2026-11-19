//! D169: retomada da intenção original no mesmo Hub/Space/conversa/binding.
use super::{Actor, BotConfig, EntryKind, Phase, Result, TurnInput, Value, json, now_ms};
use anyhow::{anyhow, ensure};

impl Actor {
    fn space_waiting_config(&self, input: &TurnInput) -> Result<BotConfig> {
        let original =
            self.hub.store.bot(&self.cfg.id)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("bot_not_found"))?.config;
        let cfg = crate::fork::spaces::execution_config(&self.hub.store, &original, &input.chat)?;
        ensure!(cfg.context.is_some() && input.waitable(&cfg), "conversation_not_waiting");
        ensure!(
            input.context_binding.as_deref()
                == Some(crate::fork::spaces::execution_stamp(&self.hub.store, &cfg)?.as_str()),
            "conversation_binding_changed"
        );
        ensure!(
            input.entries.iter().all(|id| self
                .hub
                .store
                .entry(id)
                .is_some_and(|e| e.bot_id == input.chat && e.kind == EntryKind::User.as_str())),
            "invalid_waiting_intent"
        );
        crate::fork::spaces::validate_entries(&self.hub, &cfg, &input.entries)?;
        Ok(cfg)
    }

    pub(in crate::agent::bot) fn restore_space_waiting_config(
        &mut self,
        input: &TurnInput,
        completing: bool,
    ) -> Result<()> {
        // Mesmo um checkpoint cujo binding expirou conserva o namespace para publicar
        // seu estado. Nunca volta a cfg.id/memória pessoais para tentar executá-lo.
        let original = self.hub.store.bot(&self.cfg.id)?.ok_or_else(|| anyhow!("bot_not_found"))?.config;
        self.cfg = crate::fork::spaces::resource_config(&self.hub.store, &original, &input.chat)?;
        if !completing {
            self.cfg = self.space_waiting_config(input)?;
        }
        Ok(())
    }

    pub(in crate::agent::bot) fn revalidate_space_waiting(&mut self) -> Result<()> {
        if let Some(input) = self.waiting.as_ref().map(|w| &w.input).filter(|i| i.context_binding.is_some()) {
            self.cfg = self.space_waiting_config(input)?;
        }
        Ok(())
    }

    pub(in crate::agent::bot) fn context_retry(&mut self, chat: &str, revision: i64, entry_id: &str) -> Result<Value> {
        let space = crate::fork::spaces::chat_space(&self.hub, chat)?;
        let c = self.hub.store.space_conversation(&space, chat)?;
        ensure!(c.bot_id == self.cfg.id && c.revision == revision, "conversation_revision_conflict");
        let w = self.waiting.as_ref().ok_or_else(|| anyhow!("conversation_not_waiting"))?;
        ensure!(
            matches!(w.phase, Phase::Waiting | Phase::Paused)
                && !w.dirty
                && self.finishing.is_none()
                && !self.stop_requested
                && self.lane.chat == chat
                && w.input.chat == chat
                && w.input.context_binding.is_some(),
            "conversation_not_waiting"
        );
        ensure!(w.input.entries.iter().any(|id| id == entry_id), "waiting_intent_mismatch");
        let notice = w
            .notice
            .as_ref()
            .and_then(|id| self.hub.store.entry(id))
            .ok_or_else(|| anyhow!("waiting_notice_required"))?;
        ensure!(
            notice.bot_id == chat
                && notice.kind == EntryKind::Notice.as_str()
                && Some(notice.turn) == self.turn
                && notice.data["subscriptionWait"].is_object(),
            "waiting_notice_mismatch"
        );
        self.revalidate_space_waiting()?;
        let before = self.waiting.clone();
        if let Some(w) = self.waiting.as_mut() {
            w.phase = Phase::Waiting;
            w.retry_at = now_ms();
            w.attempt_count = 0;
            w.switch_error = None;
        }
        if let Err(error) = self.save_waiting() {
            self.waiting = before;
            return Err(error);
        }
        self.publish_waiting();
        Ok(json!({"context":crate::fork::spaces::conversation_context(&c),"retryRequested":true}))
    }
}
