//! Gancho do C2 no ator: a intenção não vira mensagem e conserva sessão/motor até ao envio.
use super::{Actor, Adapter, Queued};
use crate::LockExt;
use crate::fork::contexto::{self, Manual, Request, Target};
use crate::store::now_ms;
use anyhow::{Result, anyhow, ensure};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

impl Actor {
    fn context_busy(&self) -> bool {
        self.turn.is_some()
            || self.input.is_some()
            || self.waiting.is_some()
            || self.finishing.is_some()
            || self.pending_admission.is_some()
            || self.dispatch.is_some()
            || self.dispatch_finishing.is_some()
            || self.compacting.is_some()
            || !self.queue.is_empty()
            || self.restart_admission
    }

    fn context_snapshot(&self, target: &Target) -> Result<Value> {
        let observation =
            target.acp.as_ref().map(|acp| contexto::load(&self.hub, &target.bot, acp)).transpose()?.flatten();
        let mut report = contexto::projection(target, observation, self.context_busy())?;
        if let Some(context) = report.get_mut("context") {
            let failures = crate::fork::compressao::failures(&self.hub.store, &target.bot, &target.lane)?;
            context["automaticPaused"] = json!(failures.paused());
            context["automaticFailures"] = json!(failures.consecutive);
            let size = context["size"].as_u64().ok_or_else(|| anyhow!("context_unavailable"))?;
            let parts = target
                .acp
                .as_deref()
                .and_then(|acp| crate::chat::context::parts(&self.hub.store, &target.lane.chat, acp, target.epoch));
            context["parts"] = contexto::breakdown(context["used"].as_u64().unwrap_or(0), parts);
            context["trigger"] = json!(
                self.compaction_policy(&codync_mem::compaction::Conversation { size, ..Default::default() })?.trigger()
            );
        }
        Ok(report)
    }

    pub(super) fn context_observe(&self, usage: Option<(u64, u64)>, compacted: Option<contexto::Compaction>) {
        let Some(acp) = self.turn_session.as_deref() else { return };
        // Rotinas/pedidos/canais não viram a medição da principal por compartilhar um bot.
        if self.active_routine.is_some() || self.active_request.is_some() || self.active_group.is_some() {
            return;
        }
        let can_compact = self.conn.as_ref().is_some_and(|c| matches!(c.adapter, Adapter::Claude | Adapter::Codex));
        if let Err(error) = contexto::observe(&self.hub, &self.cfg, &self.lane, acp, can_compact, usage, compacted) {
            tracing::warn!(%error, "context observation unavailable");
        }
    }

    pub(super) fn context_extra_after_turn(&self, compacting: Option<bool>) {
        if self.lane.session.is_none()
            || self.active_routine.is_some()
            || self.active_request.is_some()
            || self.active_group.is_some()
        {
            return;
        }
        let Some(sid) = self.turn_session.as_deref() else { return };
        let mut conversation = self.conversation(sid);
        if let Some(&(used, size)) = self.conn.as_ref().and_then(|conn| conn.usage.get(sid)) {
            conversation.usage(used, size);
        }
        if compacting.is_none() {
            conversation.turn_ended(now_ms());
        }
        self.save_conversation(sid, &conversation);
    }

    fn context_validate(&self, manual: &Manual, idle: bool) -> Result<()> {
        ensure!(!manual.reply.is_closed(), "context_request_cancelled");
        if idle {
            ensure!(!self.context_busy() && !self.admission_blocked(), "context_busy");
        }
        let target = contexto::resolve(&self.hub, &manual.read)?;
        ensure!(target == manual.target && target.bot == self.cfg.id, "context_engine_changed");
        ensure!(manual.read.context_id.as_deref() == Some(target.context_id()?.as_str()), "context_engine_changed");
        // O motor medido já está preso por context_id, observation.engine/lane, conn.loaded e send_lane. Não
        // comparar com self.cfg: ela é a da ÚLTIMA lane/domínio do ator (ou a original após reinício), não a da
        // conversa do pedido — numa conversa ligada (chat != bot, seleção própria) esse era um "mudou" falso.

        let acp = target.acp.as_deref().ok_or_else(|| anyhow!("context_unavailable"))?;
        let conn = self.conn.as_ref().ok_or_else(|| anyhow!("context_engine_unavailable"))?;
        ensure!(matches!(conn.adapter, Adapter::Claude | Adapter::Codex), "context_cannot_compact");
        ensure!(conn.loaded.contains_key(acp), "context_engine_changed");
        let observation = contexto::load(&self.hub, &target.bot, acp)?.ok_or_else(|| anyhow!("context_unavailable"))?;
        ensure!(
            observation.lane == target.lane && observation.engine == target.engine && observation.can_compact,
            "context_engine_changed"
        );
        ensure!(observation.usage.is_some_and(|u| u.size > 0 && u.epoch == target.epoch), "context_unavailable");
        ensure!(!self.compaction_policy(&self.conversation(acp))?.thrashing(now_ms()), "context_repeat_guard");
        let body = json!({"sessionId":target.lane.session_id()});
        let lane = crate::fork::sessoes::api::send_lane(&self.hub.store.db.locked(), &target.lane.chat, &body)?;
        ensure!(lane == target.lane, "context_engine_changed");
        Ok(())
    }

    pub(super) async fn context_call(
        &mut self,
        request: Request,
        reply: oneshot::Sender<Result<Value>>,
        done_tx: &mpsc::UnboundedSender<super::PromptDone>,
    ) {
        let target = contexto::resolve(&self.hub, &request.read);
        let target = match target {
            Ok(target) if target.bot == self.cfg.id => target,
            Ok(_) => {
                let _ = reply.send(Err(anyhow!("context_engine_changed")));
                return;
            }
            Err(error) => {
                let _ = reply.send(Err(error));
                return;
            }
        };
        if !request.compact {
            let _ = reply.send(self.context_snapshot(&target));
            return;
        }
        let manual = Manual { target, read: request.read, reply };
        if let Err(error) = self.context_validate(&manual, true) {
            let _ = manual.reply.send(Err(error));
            return;
        }
        self.queue.push_back(Queued::Compact { idle: false, manual: Some(manual) });
        self.next_in_queue(done_tx).await;
    }

    pub(super) async fn start_requested_compaction(
        &mut self,
        idle: bool,
        manual: Option<Manual>,
        done_tx: &mpsc::UnboundedSender<super::PromptDone>,
    ) {
        let Some(manual) = manual else {
            self.start_compaction(idle, None, done_tx).await;
            return;
        };
        if let Err(error) = self.context_validate(&manual, true) {
            let _ = manual.reply.send(Err(error));
            return;
        }
        self.context_compaction = Some(manual.target.clone());
        self.start_compaction(false, Some(manual.target.lane.clone()), done_tx).await;
        let result = if self.compacting.is_some() && self.turn_session == manual.target.acp {
            Ok(json!({"accepted":true}))
        } else {
            self.context_compaction = None;
            Err(anyhow!("context_compaction_not_started"))
        };
        let _ = manual.reply.send(result);
    }

    /// Também no retry de admissão e depois dos awaits de setup: não compacta uma ACP substituta.
    pub(super) fn context_compaction_guard(&self) -> Result<()> {
        let Some(target) = &self.context_compaction else { return Ok(()) };
        ensure!(
            self.compacting.is_some() && self.lane == target.lane && self.turn_session == target.acp,
            "context_engine_changed"
        );
        ensure!(contexto::engine(&self.cfg) == target.engine, "context_engine_changed");
        let space = self.hub.store.space_conversation(&self.cfg.space_id, &target.lane.chat)?;
        let read = contexto::Read {
            bot_id: target.lane.chat.clone(),
            session_id: Some(target.lane.session_id().into()),
            conversation_id: None,
            space_id: Some(space.space_id),
            context_id: None,
            hub_id: None,
        };
        ensure!(contexto::resolve(&self.hub, &read)? == *target, "context_engine_changed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::store::Store;
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };

    fn bot_cfg(dir: &std::path::Path) -> super::super::BotConfig {
        serde_json::from_value(json!({"id":"bot","name":"Bot","backend":"claude","cwd":dir})).unwrap()
    }

    #[tokio::test]
    async fn rejects_busy_wrong_engine_session_and_unsupported_adapter_before_queue() {
        let dir = std::env::temp_dir().join(format!("context-unit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let cfg = bot_cfg(dir.as_path());
        store.save_bot(&cfg).unwrap();
        store.set_session("bot", Some("sid")).unwrap();
        let hub = crate::hub::Hub::new(
            store,
            "fixture".into(),
            crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        let mut actor = super::super::new_actor(hub.clone(), bot_cfg(dir.as_path()));
        actor.restart_admission = false;
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/instructions_agent.py");
        let command = format!("python3 -u '{}' claude", script.display());
        actor.conn = Some(
            super::super::launch::start_agent_protected(&command, dir.to_str().unwrap(), &[], Duration::from_secs(5))
                .await
                .unwrap(),
        );
        actor.conn.as_mut().unwrap().loaded.insert("sid".into(), Instant::now());
        actor.turn_session = Some("sid".into());
        actor.track_usage(84_000, 200_000);
        let read: contexto::Read = serde_json::from_value(json!({"botId":"bot"})).unwrap();
        let target = contexto::resolve(&hub, &read).unwrap();
        let (reply, _answer) = oneshot::channel();
        let mut manual =
            Manual { read: contexto::Read { context_id: Some(target.context_id().unwrap()), ..read }, target, reply };
        assert!(actor.context_validate(&manual, true).is_ok());
        // Stop já concluído conserva esse sinal até o próximo turno; não é ocupação.
        actor.stop_requested = true;
        assert!(!actor.context_busy());
        assert!(actor.context_validate(&manual, true).is_ok());
        actor.stop_requested = false;
        actor.turn = Some(1);
        assert!(actor.context_validate(&manual, true).unwrap_err().to_string().contains("context_busy"));
        actor.turn = None;
        actor.conn.as_mut().unwrap().adapter = Adapter::Other;
        assert!(actor.context_validate(&manual, true).unwrap_err().to_string().contains("cannot_compact"));
        actor.conn.as_mut().unwrap().adapter = Adapter::Claude;
        // A observação de outro motor não pode autorizar o pedido medido.
        let mut changed = bot_cfg(dir.as_path());
        changed.model = Some("another-engine-model".into());
        contexto::observe(&hub, &changed, &manual.target.lane, "sid", true, Some((84_000, 200_000)), None).unwrap();
        assert!(actor.context_validate(&manual, true).unwrap_err().to_string().contains("engine_changed"));
        contexto::observe(
            &hub,
            &bot_cfg(dir.as_path()),
            &manual.target.lane,
            "sid",
            true,
            Some((84_000, 200_000)),
            None,
        )
        .unwrap();
        manual.target.acp = Some("another-session".into());
        assert!(actor.context_validate(&manual, true).unwrap_err().to_string().contains("engine_changed"));
        assert!(actor.queue.is_empty());
        if let Some(conn) = actor.conn.take() {
            conn.acp.kill_and_wait().await;
        }
        drop(actor);
        drop(hub);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Regressão do falso "mudou": a conversa ligada (chat != bot, seleção de motor própria) é medida na sua lane,
    /// mas o ator conserva a config da última lane/domínio — a original após reinício. O pedido manual não pode
    /// depender do cfg corrente do ator: o motor medido é preso por `context_id`, observation e conn.loaded.
    #[tokio::test]
    async fn accepts_bound_conversation_whatever_the_actor_last_lane_was() {
        let dir = std::env::temp_dir().join(format!("context-bound-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let cfg = bot_cfg(dir.as_path());
        store.save_bot(&cfg).unwrap();
        let hub = crate::hub::Hub::new(
            store,
            "fixture".into(),
            crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        // Conversa adicional do bot no Pessoal com motor próprio ("6.1 Terra"), como a do Berga.
        let chat = uuid::Uuid::new_v4().to_string();
        hub.store
            .db
            .locked()
            .execute(
                r#"INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary,engine_selection)
                 VALUES(?1,?2,'bot','Conversa',1,0,'{"backend":"claude","model":"terra","modelOptions":{}}')"#,
                rusqlite::params![chat, crate::fork::spaces::PERSONAL],
            )
            .unwrap();
        hub.store.kv_set(&crate::store::run_session_key("bot", &format!("conversation:{chat}")), "acp-c").unwrap();
        let lane = crate::store::Lane::main(&chat);
        let mut cfg_c = cfg.clone();
        cfg_c.model = Some("terra".into());
        contexto::observe(&hub, &cfg_c, &lane, "acp-c", true, Some((84_000, 200_000)), None).unwrap();
        // Ator recém-criado (reinício do Hub): config original, sem contexto ligado nem motor da conversa.
        let mut actor = super::super::new_actor(hub.clone(), cfg.clone());
        actor.restart_admission = false;
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/instructions_agent.py");
        let command = format!("python3 -u '{}' claude", script.display());
        actor.conn = Some(
            super::super::launch::start_agent_protected(&command, dir.to_str().unwrap(), &[], Duration::from_secs(5))
                .await
                .unwrap(),
        );
        actor.conn.as_mut().unwrap().loaded.insert("acp-c".into(), Instant::now());
        let read: contexto::Read =
            serde_json::from_value(json!({"botId":chat,"spaceId":crate::fork::spaces::PERSONAL})).unwrap();
        let target = contexto::resolve(&hub, &read).unwrap();
        assert_eq!(target.lane, lane);
        let (reply, _answer) = oneshot::channel();
        let manual =
            Manual { read: contexto::Read { context_id: Some(target.context_id().unwrap()), ..read }, target, reply };
        assert!(actor.context_validate(&manual, true).is_ok());
        // Mudança real de motor continua recusada: a observação medida já não é a do alvo resolvido.
        let mut cfg_d = cfg.clone();
        cfg_d.model = Some("outro".into());
        contexto::observe(&hub, &cfg_d, &lane, "acp-c", true, Some((84_000, 200_000)), None).unwrap();
        let (reply, _answer) = oneshot::channel();
        let manual = Manual {
            read: contexto::Read { context_id: Some(manual.target.context_id().unwrap()), ..manual.read },
            target: manual.target,
            reply,
        };
        assert!(actor.context_validate(&manual, true).unwrap_err().to_string().contains("engine_changed"));
        if let Some(conn) = actor.conn.take() {
            conn.acp.kill_and_wait().await;
        }
        drop(actor);
        drop(hub);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
