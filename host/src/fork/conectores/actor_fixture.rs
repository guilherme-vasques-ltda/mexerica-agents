//! Fixture exclusiva do consumidor 04; usa o ator e o ACP reais com processo sintético.
use super::{Actor, Adapter, PromptDone, launch, new_actor};
use crate::{
    fork::bot_admission,
    hub::Hub,
    store::{BotConfig, Lane},
};
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

pub(crate) struct Fixture {
    actor: Actor,
    done_tx: mpsc::UnboundedSender<PromptDone>,
    done_rx: mpsc::UnboundedReceiver<PromptDone>,
}
impl Fixture {
    pub(crate) async fn new(hub: Arc<Hub>, cfg: BotConfig, command: &str) -> Result<Self> {
        let mut actor = new_actor(hub, cfg);
        actor.conn = Some(
            launch::start_agent_protected(&command.to_owned().into(), &actor.cfg.cwd, &[], Duration::from_secs(5))
                .await?,
        );
        let backend = match actor.conn.as_ref().unwrap().adapter {
            Adapter::Codex => "codex",
            Adapter::Claude => "claude",
            Adapter::Other | Adapter::Mexerica => actor.cfg.backend.as_str(),
        };
        crate::fork::memory_auto::transport::new_generation(&actor.hub, &actor.cfg, &actor.lane.chat, backend)?;
        let (done_tx, done_rx) = mpsc::unbounded_channel();
        Ok(Self { actor, done_tx, done_rx })
    }
    pub(crate) async fn ensure(&mut self) -> Result<()> {
        // Fora de turno, sessão só nasce com identidade preparada (sem token ativo), como no ator real.
        let runtime = self.actor.hub.runtime(&self.actor.cfg.id);
        if runtime.memory_turn_token.is_none() && runtime.preparing_turn_token.is_none() {
            let lane = self.actor.lane.clone();
            self.actor.hub.prepare_bot_turn(&self.actor.cfg.id, None, |r| r.lane = Some(lane))?;
        }
        let backend = self.actor.conn.as_ref().map_or(self.actor.cfg.backend.as_str(), |conn| match conn.adapter {
            Adapter::Codex => "codex",
            Adapter::Claude => "claude",
            Adapter::Other | Adapter::Mexerica => self.actor.cfg.backend.as_str(),
        });
        // start_turn também limpa a ACP anterior antes de preparar new/load/reuso.
        // Conserva o token do turno já emitido pelo Hub; não cria outra execução.
        self.actor.hub.set_runtime(&self.actor.cfg.id, |r| r.session = None);
        crate::fork::memory_auto::queue::capture_engine(&self.actor.hub, &self.actor.cfg, &self.actor.lane, backend)?;
        self.actor.ensure_session(None).await?;
        crate::fork::memory::gateway::bind(&self.actor.hub, &self.actor.cfg.id, self.actor.turn_session.as_deref())
    }
    /// Usa o produtor real: setup sozinho não autoriza frames do turno.
    pub(crate) async fn prompt(&mut self, entry_id: &str) -> Result<()> {
        let hub = self.actor.hub.clone();
        let bot = self.actor.cfg.id.clone();
        let entry = hub.store.entry(entry_id).expect("Entrada humana da fixture");
        assert!(entry.is_user_authored());
        assert_eq!(entry.data["status"], "queued");
        assert!(self.actor.dispatch.is_none());
        self.actor
            .start_turn(
                Lane::main(&bot),
                std::slice::from_ref(&entry.id),
                entry.data["text"].as_str().expect("Texto humano da fixture"),
                false,
                &self.done_tx,
            )
            .await?;
        let dispatch = bot_admission::current(&hub.store, &bot)?.expect("Admissão real da fixture");
        assert_eq!(dispatch.phase, bot_admission::Phase::Permitted);
        assert_eq!(self.actor.dispatch.as_ref(), Some(&dispatch.ticket()));
        let runtime = hub.runtime(&bot);
        assert_eq!(runtime.status, crate::hub::BotStatus::Working);
        assert!(runtime.preparing_turn_token.is_none());
        assert_eq!(runtime.memory_turn_token.as_deref(), Some(dispatch.execution_token.as_str()));
        assert_eq!(runtime.message.as_deref(), Some(entry_id));
        assert_eq!(runtime.lane.as_ref(), Some(&Lane::main(&bot)));
        assert!(self.actor.turn_session.is_some());
        assert_eq!(runtime.session.as_ref(), self.actor.turn_session.as_ref());
        assert_eq!(hub.store.entry(entry_id).expect("Entrada reclamada pelo produtor").data["status"], "sent");
        Ok(())
    }
    /// Consome a conclusão ACP da mesma captura antes de admitir outro turno.
    pub(crate) async fn finish(&mut self) -> Result<()> {
        let done = tokio::time::timeout(Duration::from_secs(2), self.done_rx.recv())
            .await?
            .ok_or_else(|| anyhow::anyhow!("Conclusão ACP da fixture ausente"))?;
        assert!(self.actor.accepts_completion(&done.ticket));
        assert!(done.result.is_ok());
        self.actor.finish_turn(&done.result);
        assert!(self.actor.dispatch.is_none());
        assert!(bot_admission::current(&self.actor.hub.store, &self.actor.cfg.id)?.is_none());
        Ok(())
    }
    pub(crate) fn connected(&self) -> bool {
        self.actor.conn.is_some()
    }
    pub(crate) fn loaded(&self) -> bool {
        self.actor
            .conn
            .as_ref()
            .is_some_and(|conn| self.actor.turn_session.as_ref().is_some_and(|sid| conn.loaded.contains_key(sid)))
    }
    pub(crate) async fn close(&mut self) {
        if let Some(sid) = self.actor.turn_session.clone() {
            self.actor.close_session(&sid).await;
        }
    }
    pub(crate) async fn replace(&mut self) {
        if let Some(conn) = self.actor.conn.take() {
            conn.acp.kill_and_wait().await;
        }
    }
}

/// Isola os helpers existentes de memória: sem `set_var` global ou dados do operador.
struct Directory(std::path::PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) async fn isolated(name: &str) -> Result<bool> {
    const CHILD: &str = "DONNABOT_CONECTORES_ACTOR_FIXTURE";
    if std::env::var(CHILD).is_ok_and(|value| value == name) {
        return Ok(false);
    }
    let home = Directory(std::env::temp_dir().join(format!("donna-conectores04-actor-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir_all(&home.0)?;
    let mut child = tokio::process::Command::new(std::env::current_exe()?);
    let (_, test) = name.split_once("::").ok_or_else(|| anyhow::anyhow!("Nome qualificado da fixture ausente"))?;
    child.args(["--exact", test, "--nocapture"]).env("MEXERICA_HOME", &home.0).env(CHILD, name).kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(60), child.output()).await??;
    anyhow::ensure!(
        output.status.success(),
        "fixture ACP sintética: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout[..output.stdout.len().min(8192)]),
        String::from_utf8_lossy(&output.stderr[..output.stderr.len().min(8192)])
    );
    anyhow::ensure!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"),
        "A fixture do ator precisa executar exatamente um teste"
    );
    Ok(true)
}
