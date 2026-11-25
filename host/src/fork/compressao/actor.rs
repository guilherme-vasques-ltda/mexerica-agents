//! Ganchos curtos do ator para a política própria do fork.
use super::{Actor, Done};
use crate::{fork::compressao, store::Lane};
use codync_mem::compaction::{self as rules, CacheLife, Conversation, Decision};

impl Actor {
    pub(super) fn compaction_policy(&self, c: &Conversation) -> anyhow::Result<Conversation> {
        let mut c = c.clone();
        c.trigger_tokens = compressao::settings(&self.hub.store)?.trigger_tokens;
        Ok(c)
    }

    pub(super) fn automatic_compaction_allowed(&self, lane: &Lane) -> bool {
        match compressao::failures(&self.hub.store, &self.cfg.id, lane) {
            Ok(state) => !state.paused(),
            Err(error) => {
                tracing::warn!(%error, "compaction failure state unavailable");
                false
            }
        }
    }

    pub(super) fn compaction_at_turn_end(&self, c: &Conversation, now: i64) -> Decision {
        if !self.automatic_compaction_allowed(&self.lane) {
            return Decision::Keep;
        }
        self.compaction_policy(c).map_or(Decision::Keep, |c| rules::at_turn_end(&c, now))
    }

    pub(super) fn compaction_while_idle(&self, c: &Conversation, life: CacheLife, now: i64) -> bool {
        if !self.automatic_compaction_allowed(&Lane::main(&self.cfg.id)) {
            return false;
        }
        let Ok(settings) = compressao::settings(&self.hub.store) else { return false };
        settings.idle == compressao::Idle::BeforeExpiry
            && self.compaction_policy(c).is_ok_and(|c| rules::while_idle(&c, life, now) == Decision::Compact)
    }

    pub(super) fn compaction_failed(&self, lane: &Lane) {
        if let Err(error) = compressao::failed(&self.hub.store, &self.cfg.id, lane) {
            tracing::warn!(%error, "couldn't persist compaction failure");
        }
    }

    pub(super) fn compaction_finished(&self, done: &Done) {
        if self.compacting.is_some() && self.context_compaction.is_none() && done.is_err() {
            self.compaction_failed(&self.lane);
        }
    }

    pub(super) fn compaction_reset(&self, lane: &Lane) {
        if let Err(error) = compressao::reset(&self.hub.store, &self.cfg.id, lane) {
            tracing::warn!(%error, "couldn't reset compaction failures");
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::{
        agent::bot::Cmd,
        hub::Hub,
        remote::identity::Identity,
        store::{BotConfig, EntryKind, Store},
    };
    use serde_json::json;

    #[tokio::test]
    async fn two_start_failures_block_third_attempt_new_user_and_success_resume() {
        let dir = std::env::temp_dir().join(format!("compaction-actor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg: BotConfig = serde_json::from_value(json!({"id":"a","name":"A","backend":"claude"})).unwrap();
        let hub = Hub::new(
            Store::open(std::path::Path::new(":memory:")).unwrap(),
            "hub".into(),
            Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        // Sem linha do bot: start_turn falha antes de iniciar qualquer processo.
        let mut actor = super::super::new_actor(hub.clone(), cfg.clone());
        actor.session_id = Some("sid".into());
        actor.restart_admission = false;
        let lane = Lane::main("a");
        let (done_tx, _) = tokio::sync::mpsc::unbounded_channel();
        actor.start_compaction(true, None, false, &done_tx).await;
        assert_eq!(compressao::failures(&hub.store, "a", &lane).unwrap().consecutive, 1);
        actor.start_compaction(true, None, false, &done_tx).await;
        assert_eq!(compressao::failures(&hub.store, "a", &lane).unwrap().consecutive, 2);
        actor.start_compaction(true, None, false, &done_tx).await;
        assert_eq!(
            compressao::failures(&hub.store, "a", &lane).unwrap().consecutive,
            2,
            "a terceira não chega ao start_turn"
        );
        let full = Conversation { used: 210_000, size: 1_000_000, last_turn: 1, ..Default::default() };
        assert_eq!(actor.compaction_at_turn_end(&full, 3_300_001), Decision::Keep);
        assert!(!actor.compaction_while_idle(&full, CacheLife::OneHour, 3_300_001));
        hub.store.save_bot(&cfg).unwrap();
        actor.turn = Some(99); // A mensagem nova entra na fila, sem começar outro processo no teste.
        let entry = hub.store.insert_entry(&lane, EntryKind::User, 0, &json!({"text":"oi","status":"queued"})).unwrap();
        actor.on_cmd(Cmd::Send { lane: lane.clone(), entry_id: entry.id, text: "oi".into() }, &done_tx).await;
        assert!(!compressao::failures(&hub.store, "a", &lane).unwrap().paused(), "mensagem nova despausa");
        assert_eq!(actor.compaction_at_turn_end(&full, 3_300_001), Decision::Compact);
        // Falhas automáticas que chegam pelo término do turno também contam.
        actor.turn_session = Some("sid".into());
        for _ in 0..2 {
            actor.compacting = Some(false);
            actor.after_turn(&Err(anyhow::anyhow!("compact failed")));
        }
        assert!(compressao::failures(&hub.store, "a", &lane).unwrap().paused());
        // O mesmo callback real da compactação manual bem-sucedida zera a pausa.
        actor.compacting = Some(false);
        actor.turn_session = Some("sid".into());
        actor.compacted("manual-success", None);
        assert!(!compressao::failures(&hub.store, "a", &lane).unwrap().paused());
        assert!(!compressao::failures(&hub.store, "b", &Lane::main("b")).unwrap().paused());
        let settings = compressao::Settings { trigger_tokens: Some(50_000), idle: compressao::Idle::Never };
        hub.store.kv_set("donna.compaction.settings", &serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(actor.compaction_policy(&full).unwrap().trigger(), 50_000);
        assert!(!actor.compaction_while_idle(&full, CacheLife::OneHour, 3_300_001));
        drop(actor);
        drop(hub);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
