// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Shared host state: store, event fan-out, bot actors, push, usage.

use crate::LockExt;
use crate::agent::bot::{self, BotHandle, Cmd};
use crate::api::devices::Pairing;
use crate::remote::identity::Identity;
use crate::remote::push;
use crate::screen::Screen;
use crate::service;
use crate::store::{BotConfig, Entry, EntryKind, Lane, Store};
use crate::usage::Usage;
use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard};
use std::time::Duration;
use tokio::sync::{broadcast, watch};

/// MX-280: teto da espera pelas tarefas de memória no desligamento. O caso
/// comum é instantâneo; estourado o prazo, registra o atraso e continua
/// aguardando para preservar a drenagem e a segurança das escritas.
#[cfg(not(test))]
const MEMORY_SHUTDOWN_WAIT: Duration = Duration::from_secs(10);
/// Nos testes o prazo é curto para exercitar o caminho de estouro sem espera real.
#[cfg(test)]
const MEMORY_SHUTDOWN_WAIT: Duration = Duration::from_millis(500);

#[path = "fork/bot_deletion/hub.rs"]
mod bot_deletion;
pub(crate) mod bot_groups;
mod order;
mod reactions;
mod sync;

/// What a bot is doing right now (wire values: `idle` / `working` / `needsInput` / `error`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BotStatus {
    #[default]
    Idle,
    Working,
    NeedsInput,
    Waiting,
    Error,
}

#[derive(Clone, Default, Debug)]
pub struct Runtime {
    pub status: BotStatus,
    pub activity: String,
    /// MX-147: código estável opcional da atividade; texto e código mudam juntos.
    pub activity_code: Option<crate::fork::activity_code::ActivityCode>,
    pub started_at: Option<i64>,
    pub resume_at: Option<i64>,
    /// Where the running turn talks: its own chat or a group.
    pub lane: Option<Lane>,
    /// The user's message that started the running turn; `None` for a turn a routine, a room or another bot
    /// started. Memory cites it as where a fact was learned.
    pub message: Option<String>,
    /// The ACP session the running turn prompts. A memory write made there is one that session already knows.
    pub session: Option<String>,
    /// Uma falha ao marcar a sessão fecha a escrita até o próximo turno.
    pub memory_exposure_error: bool,
    /// Identidade única da execução que recebeu o pedido de memória.
    pub(crate) memory_turn_token: Option<String>,
    /// Mesma identidade, ainda sem autorização de execução ou escrita de memória.
    pub(crate) preparing_turn_token: Option<String>,
    /// Contexto da chamada capturado para um worker, sem adotar a sessão de outro turno.
    pub(crate) execution: Option<crate::fork::sessoes::ExecutionRef>,
    pub fast_mode: crate::agent::config::FastModeSupport,
    /// MB-1b: perguntas `ask_owner` ainda pendentes (no máximo 5, as mais novas). Com alguma e sem turno,
    /// o estado mostrado é Precisa de você (`shown_status`); o estado interno continua o do turno.
    pub(crate) pending_questions: Vec<crate::fork::owner_question::PendingQuestion>,
    /// Revisão do banco da última lista aplicada; leituras mais antigas são descartadas (MB-1b S1).
    pub(crate) pending_questions_rev: i64,
}

impl Runtime {
    /// O estado que o dono vê: Livre com pergunta pendente é Precisa de você, como a permissão.
    /// Verificações de "turno em andamento" continuam usando `status`.
    pub(crate) fn shown_status(&self, has_questions: bool) -> BotStatus {
        if self.status == BotStatus::Idle && has_questions { BotStatus::NeedsInput } else { self.status }
    }

    /// MX-147: texto legado e código estável mudam na mesma mutação, inclusive nas transições.
    pub(crate) fn set_activity(
        &mut self,
        text: impl Into<String>,
        kind: Option<crate::fork::activity_code::ActivityKind>,
    ) {
        self.activity = text.into();
        self.activity_code = kind.map(crate::fork::activity_code::ActivityCode::of);
    }

    pub(crate) fn clear_activity(&mut self) {
        self.activity.clear();
        self.activity_code = None;
    }
}

pub struct Hub {
    pub analytics: crate::analytics::Analytics,
    pub routines: crate::routines::Routines,
    pub computers: crate::computers::State,
    pub team: crate::chat::team::Requests,
    pub groups: crate::chat::group::Rooms,
    pub store: Store,
    pub(crate) conectores: crate::fork::conectores::State,
    pub(crate) whatsapp: crate::fork::whatsapp::State,
    /// Fase 2b: jobs do gerador de conjuntos 3D (um ativo por Hub).
    pub gerador: crate::fork_avatars::gerador::State,
    pub host_id: String,
    pub identity: Identity,
    /// Loopback-only bearer token for this computer's own apps and helpers.
    pub token: String,
    pub(crate) pontes: crate::fork::ponte_credencial::Credenciais,
    pub port: u16,
    pub events: broadcast::Sender<Value>,
    /// Held across "mutate store + broadcast" so events leave in rev order.
    pub(crate) emit_lock: Mutex<()>,
    bots: Mutex<HashMap<String, BotHandle>>,
    /// MX-280: tarefas de memória do fork (sonhador, nomeação e varredura de
    /// arquivos) vivas neste Hub, com rótulo para diagnóstico; o desligamento
    /// as aguarda para nenhuma escrita sobreviver ao fechamento.
    memory_tasks: Mutex<Vec<(String, tokio::task::JoinHandle<()>)>>,
    memory_writes: Arc<RwLock<()>>,
    pub(crate) memory_stop: watch::Sender<bool>,
    #[cfg(test)]
    pub(crate) memory_wait_started: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    runtime: Mutex<HashMap<String, Runtime>>,
    #[cfg(test)]
    before_bot_deletion: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    pub phone_clients: AtomicUsize,
    pub usage: Mutex<Usage>,
    keep_awake: Mutex<service::KeepAwake>,
    /// Generation of the authorized-device table and pairing offers: bumped on every
    /// change so open channels re-check their device (and the ACL is republished).
    pub auth: watch::Sender<u64>,
    pub pairing: Mutex<Pairing>,
    /// Device key -> open channels that have exchanged at least one frame.
    pub connected: Mutex<HashMap<String, usize>>,
    /// The cloud's `/v1/host/state` was applied on the current relay connection;
    /// account devices are refused until then (§4.3).
    pub cloud_synced: crate::fork::autoridade::sincronizacao::EstadoSincronizado,
    pub cloud: crate::remote::cloud::Cloud,
    pub(crate) personal: crate::fork::personal_bootstrap::Runtime,
    pub screen: Arc<Screen>,
    /// Setup terminals (installs, sign-ins).
    pub terms: Arc<crate::agent::term::Terms>,
    /// Códigos de login de dois fatores: só memória, somem com o Hub.
    pub(crate) login_codes: crate::market::login_codes::Codes,
}

impl Hub {
    pub fn busy(&self) -> bool {
        self.runtime.locked().values().any(|r| matches!(r.status, BotStatus::Working | BotStatus::NeedsInput))
            || self.terms.busy()
    }

    pub fn new(store: Store, host_id: String, identity: Identity, token: String, port: u16) -> Arc<Self> {
        let (events, _) = broadcast::channel(1024);
        let screen = Arc::new(Screen::new(Screen::load_enabled(&store), events.clone()));
        Arc::new(Self {
            analytics: crate::analytics::Analytics::load(&store),
            routines: crate::routines::Routines::default(),
            computers: crate::computers::State::default(),
            team: crate::chat::team::Requests::default(),
            groups: crate::chat::group::Rooms::default(),
            store,
            conectores: crate::fork::conectores::State::default(),
            whatsapp: crate::fork::whatsapp::State::default(),
            gerador: crate::fork_avatars::gerador::State::default(),
            host_id,
            identity,
            token,
            pontes: crate::fork::ponte_credencial::Credenciais::default(),
            port,
            events,
            emit_lock: Mutex::new(()),
            bots: Mutex::default(),
            memory_tasks: Mutex::default(),
            memory_writes: Arc::new(RwLock::new(())),
            memory_stop: watch::channel(false).0,
            #[cfg(test)]
            memory_wait_started: Mutex::default(),
            runtime: Mutex::default(),
            #[cfg(test)]
            before_bot_deletion: Mutex::default(),
            phone_clients: AtomicUsize::new(0),
            usage: Mutex::default(),
            keep_awake: Mutex::default(),
            auth: watch::Sender::new(0),
            pairing: Mutex::default(),
            connected: Mutex::default(),
            cloud_synced: crate::fork::autoridade::sincronizacao::EstadoSincronizado::new(false),
            cloud: crate::remote::cloud::Cloud::default(),
            personal: crate::fork::personal_bootstrap::Runtime::default(),
            screen,
            terms: Arc::default(),
            login_codes: crate::market::login_codes::Codes::default(),
        })
    }

    pub fn start(self: &Arc<Self>) -> Result<()> {
        // D305: classifica, antes de qualquer ator, o que foi gravado antes do gravador de Roteamento ligado.
        crate::fork::bot_admission::mark_pre_routing(&self.store)?;
        crate::fork::sessoes::bindings::reset_runtime(&self.store)?;
        self.store.initialize_default_donna()?;
        crate::fork::spaces::notice::backfill(self)?;
        // Migração única de dados do Composio: se falhar, o Hub inicia mesmo assim e tenta de novo na próxima partida.
        if let Err(error) = crate::fork::composio_retirada::run(&self.store) {
            tracing::error!(%error, "migração do Composio não concluída; tenta de novo na próxima partida");
        }
        // Space das contas antigas: roda uma vez, quando o cofre já está aberto (senão, logo depois de abri-lo).
        if let Err(error) = crate::market::contas_migracao::run(self) {
            tracing::error!(%error, "migração do Space das contas não concluída; tenta de novo na próxima abertura do cofre");
        }
        // Aprovações e canais transitórios expiram; humanos queued continuam elegíveis.
        self.store.conector_recover()?;
        crate::fork::conectores::whatsapp_human::recover(self)?;
        self.store.expire_pending()?;
        if !crate::environment::Environment::is_open() {
            crate::screen::fork_permission::restore_pending(self)?;
        }
        // MB-1b D1: perguntas de bot/conversa excluídos, fechados ou limpos antes de uma queda.
        crate::fork::owner_question::sweep(self);
        // R1: pedido aberto vira interrompido (antes do envio ou incerto); nada é reenviado.
        crate::fork::retornos::recuperar(&self.store)?;
        // Trabalho do gerador não salvo não sobrevive ao recomeço; manifestos ficam.
        crate::fork_avatars::gerador::conjuntos::limpar_orfaos(&self.store);
        if !crate::environment::Environment::is_open() {
            crate::chat::memory::import_old_memory(self);
        }
        self.routines.restore(&self.store)?;
        crate::fork::tarefas_runtime::restore(self)?;
        crate::market::turn_on_installed_once(&self.store)?;
        for row in self.store.bots()? {
            if !row.deleted {
                crate::fork::pasta::avisar(self, &row.config);
            }
            if row.deleted {
                self.routines.disable_bot(&self.store, &row.config.id)?;
            } else if !row.config.is_group() {
                self.spawn_bot(row.config);
            }
        }
        if crate::environment::Environment::is_open() {
            return Ok(());
        }
        crate::routines::start(self);
        crate::fork::tarefas_runtime::start(self);
        crate::fork::resumo_diario::start(self);
        crate::fork::sessoes::encerrar::start(self);
        crate::analytics::start(self);
        crate::fork::whatsapp::start(self);
        Ok(())
    }

    fn spawn_bot(self: &Arc<Self>, cfg: BotConfig) {
        if crate::environment::Environment::is_open() {
            return;
        }
        let id = cfg.id.clone();
        let (pending_questions, pending_questions_rev) = crate::fork::owner_question::load(&self.store, &id);
        self.runtime
            .locked()
            .insert(id.clone(), Runtime { pending_questions, pending_questions_rev, ..Runtime::default() });
        let handle = bot::spawn(self.clone(), cfg);
        self.bots.locked().insert(id, handle);
    }

    pub(crate) fn track_memory_task(&self, label: &str, task: tokio::task::JoinHandle<()>) {
        let mut tasks = self.memory_tasks.locked();
        tasks.retain(|(_, t)| !t.is_finished());
        tasks.push((label.to_owned(), task));
    }

    /// Holds admission through the entire write, including projections to disk.
    pub(crate) fn memory_write(&self) -> Result<RwLockReadGuard<'_, ()>> {
        let guard = self.memory_writes.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        anyhow::ensure!(!self.routines.stopping(), "hub_stopping: o Hub está desligando.");
        Ok(guard)
    }

    /// Stops every bot actor and waits for memory writes and queues to finish.
    pub async fn shutdown(&self) {
        self.pontes.revogar_todas();
        self.routines.stop().await;
        self.memory_stop.send_replace(true);
        self.whatsapp.shutdown().await;
        let handles: Vec<BotHandle> = self.bots.locked().drain().map(|(_, h)| h).collect();
        let tasks: Vec<_> = handles
            .into_iter()
            .map(|h| {
                let _ = h.tx.send(Cmd::Shutdown);
                h.task
            })
            .collect();
        let bots = futures::future::join_all(tasks);
        tokio::pin!(bots);
        if tokio::time::timeout(Duration::from_secs(5), &mut bots).await.is_err() {
            tracing::warn!("some bots didn't stop within 5 s; waiting for their final turns");
            bots.await;
        }
        // Keep polling the same join future after the deadline: completed handles
        // must not be polled twice, and cancelling a worker would lose its queue.
        let memory = std::mem::take(&mut *self.memory_tasks.locked());
        let labels: Vec<_> = memory.iter().map(|(label, _)| label.clone()).collect();
        let wait = futures::future::join_all(memory.into_iter().map(|(_, task)| task));
        tokio::pin!(wait);
        #[cfg(test)]
        if let Some(started) = self.memory_wait_started.locked().take() {
            let _ = started.send(());
        }
        if tokio::time::timeout(MEMORY_SHUTDOWN_WAIT, &mut wait).await.is_err() {
            tracing::warn!(tasks = ?labels, "memory tasks exceeded shutdown deadline; waiting for safe completion");
            wait.await;
        }
        // The signal precedes the exclusive lock. Existing writers finish; any
        // later reader sees stopping and cannot write. Do not block a Tokio worker.
        let barrier = self.memory_writes.clone();
        if let Err(error) = tokio::task::spawn_blocking(move || {
            drop(barrier.write().unwrap_or_else(std::sync::PoisonError::into_inner));
        })
        .await
        {
            panic!("memory shutdown barrier failed: {error}");
        }
        self.keep_awake.locked().set(false);
    }

    /// Canal preparado antes da cerca de autoridade da conclusão de login.
    pub(crate) fn login_command_sender(&self, bot: &str) -> Result<tokio::sync::mpsc::UnboundedSender<Cmd>> {
        self.bots.locked().get(bot).map(|h| h.tx.clone()).ok_or_else(|| anyhow!("unknown bot"))
    }

    pub fn send_cmd(&self, bot_id: &str, cmd: Cmd) -> Result<()> {
        let bots = self.bots.locked();
        let h = bots.get(bot_id).ok_or_else(|| anyhow!("unknown bot"))?;
        h.tx.send(cmd).map_err(|_| anyhow!("bot stopped"))
    }

    /// D543: ausência de ator é distinta de ator parado/canal quebrado.
    pub(crate) fn has_bot_actor(&self, id: &str) -> bool {
        self.bots.locked().contains_key(id)
    }

    #[cfg(test)]
    pub(crate) fn test_bot_commands(&self, bot: &str) -> tokio::sync::mpsc::UnboundedReceiver<Cmd> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.bots.locked().insert(bot.into(), BotHandle { tx, task: tokio::spawn(async {}) });
        rx
    }

    #[cfg(test)]
    pub(crate) fn test_before_bot_deletion(&self, hook: impl FnOnce() + Send + 'static) {
        *self.before_bot_deletion.locked() = Some(Box::new(hook));
    }

    // MARK: bots

    pub fn bot_name(&self, id: &str) -> String {
        self.store.bot(id).ok().flatten().map_or_else(|| "A deleted bot".into(), |b| b.config.name)
    }

    /// A projeção distingue o fallback interno de um agregado com membros observados.
    fn observed_group_runtime(&self, group: &BotConfig) -> Option<Runtime> {
        let (busy, observed) = {
            let runtime = self.runtime.locked();
            let busy = group.members.iter().find_map(|m| {
                let r = runtime.get(m)?;
                (r.status != BotStatus::Idle && r.lane.as_ref().is_some_and(|l| l.chat == group.id))
                    .then(|| (m.clone(), r.clone()))
            });
            (busy, crate::fork::runtime_audiencia::group_idle_observed(group, &runtime))
        };
        let Some((member, mut r)) = busy else { return observed.then(Runtime::default) };
        let name = self.bot_name(&member);
        r.activity = if r.activity.is_empty() { name.clone() } else { format!("{name}: {}", r.activity) };
        // O código do membro sobe identificado, como o prefixo do texto.
        if let Some(code) = &mut r.activity_code {
            code.member = Some(name);
        }
        Some(r)
    }

    pub(crate) fn prepare_new_bot(&self, cfg: BotConfig) -> Result<BotConfig> {
        self.prepare_new_bot_with_workspace(cfg, crate::agent::workspace::prepare)
    }

    pub(crate) fn prepare_new_bot_with_workspace(
        &self,
        mut cfg: BotConfig,
        prepare_workspace: impl FnOnce(&mut BotConfig) -> Result<()>,
    ) -> Result<BotConfig> {
        cfg.id = uuid::Uuid::new_v4().to_string();
        cfg.created_at = crate::store::now_ms();
        cfg.position = self.new_bot_position_in_space(&cfg.space_id)?;
        // No name yet (Grok Bot's flow): it gets one after its first few conversations.
        cfg.auto_name = !cfg.is_group() && cfg.name.trim().is_empty();
        if cfg.auto_name {
            crate::chat::naming::PLACEHOLDER.clone_into(&mut cfg.name);
        }
        if cfg.is_group() {
            cfg.members = crate::chat::group::valid_members(&self.store, &cfg.id, &cfg.members)?;
        }
        crate::fork::lider_canal::validate(&mut cfg, None)?;
        validate(&mut cfg)?;
        crate::market::contas::validar_vinculo(&self.store, None, &mut cfg)?;
        crate::fork::engine_accounts::managed::validate_selection(&self.store, &cfg)?;
        prepare_workspace(&mut cfg)?;
        Ok(cfg)
    }

    #[cfg(test)]
    pub fn create_bot(self: &Arc<Self>, cfg: BotConfig) -> Result<Value> {
        self.create_bot_authorized(cfg, None, None, None)
    }
    pub(crate) fn create_bot_authorized(
        self: &Arc<Self>,
        cfg: BotConfig,
        authority: Option<&crate::fork::engine_setup::access::Authority>,
        skills: Option<&crate::fork::skill_selection::CreationSelection>,
        owner: Option<&crate::fork::autoridade::Autoridade>,
    ) -> Result<Value> {
        // c1: seleção positiva só pela condição capturada no formulário.
        anyhow::ensure!(skills.is_some() || cfg.skills.is_empty(), crate::fork::skill_selection::UNSUPPORTED);
        if let Some(authority) = authority {
            authority.require_current(self)?;
        }
        let cfg = self.prepare_new_bot(cfg)?;
        crate::fork_avatars::gerador::conjuntos::conferir_vinculo(&self.store, &cfg)?;
        if cfg.is_group()
            && cfg.space_id == crate::fork::spaces::PERSONAL
            && let Some(existing) = crate::chat::group::with_members(&self.store, &cfg.members)?
        {
            return Ok(self.bot_value(&existing).unwrap_or(Value::Null));
        }
        {
            let _g = self.emit_lock.locked();
            if let Some(skills) = skills {
                skills.save(self, &cfg, authority)?;
            } else if let Some(owner) = owner {
                crate::fork::donna_fixa::pedido::antes_de_gravar(&self.store);
                owner.with_db(self, authority, |db| {
                    crate::fork::donna_fixa::como_dono(db, |db| crate::store::bots::save_bot(db, &cfg))
                })?;
                self.store.guard_cleanup()?;
            } else if let Some(authority) = authority {
                authority.save_bot(self, &cfg)?;
            } else {
                self.store.save_bot(&cfg)?;
            }
        }
        let cfg = self.store.bot(&cfg.id)?.ok_or_else(|| anyhow!("saved bot missing"))?.config;
        self.start_created_bot(&cfg);
        Ok(self.bot_value(&cfg.id).unwrap_or(Value::Null))
    }

    pub(crate) fn start_created_bot(self: &Arc<Self>, cfg: &BotConfig) {
        if !cfg.is_group() {
            self.spawn_bot(cfg.clone());
        }
        self.emit_bot(&cfg.id);
    }

    pub fn update_bot(&self, patch: &Value) -> Result<Value> {
        crate::fork_avatars::user_only(patch)?;
        anyhow::ensure!(
            patch.get("browserPreference").is_none(),
            "not_allowed: só o dono escolhe o navegador padrão do bot"
        );
        anyhow::ensure!(patch.get("forkEditor").is_none(), "not_allowed: só o usuário seleciona a conta de motor.");
        if ["backend", "model", "command", "modelOptions"].iter().any(|field| patch.get(*field).is_some()) {
            let id = patch["id"].as_str().ok_or_else(|| anyhow!("id required"))?;
            let key = format!("turn.waiting.{id}");
            let active = if let Some(state) = bot::read_waiting_checkpoint(&self.store, id)? {
                if state["phase"] == "finished" {
                    // Só remove o tombstone: o próximo turno pode já estar em voo.
                    let deleted = if matches!(self.runtime(id).status, BotStatus::Working | BotStatus::NeedsInput) {
                        self.store.kv_delete(&key)
                    } else {
                        self.store.clear_waiting_inflight(id)
                    };
                    if let Err(error) = deleted {
                        tracing::warn!(%error, "tombstone de espera ainda não pôde ser limpo");
                    }
                    false
                } else {
                    true
                }
            } else {
                false
            };
            if self.runtime(id).status == BotStatus::Waiting || active {
                bail!("Só o usuário pode escolher ou trocar o modelo e o harness do bot em espera.");
            }
        }
        // D368 (rodada 5): caminho de bot (`chat::profile`, nomeador). Sem autoridade do dono, sem marca:
        // no Berga, os gatilhos recusam na transação o que é travado ou só do dono.
        self.update_bot_by_user_authorized(patch, None, None)
    }

    /// Fixture de testes: o app do dono no próprio computador do Hub.
    #[cfg(test)]
    pub(crate) fn update_bot_by_user(&self, patch: &Value) -> Result<Value> {
        self.update_bot_by_user_authorized(patch, None, Some(&crate::fork::autoridade::Autoridade::local()))
    }
    /// D368: `owner` é a autoridade ORIGINAL do pedido do dono, capturada antes de qualquer fila e
    /// revalidada dentro da transação de escrita (Cloud → DB); só com ela a escrita leva a marca do dono.
    /// `None`: caminho sem dono (bot, Hub interno), que não muda campos só do dono nem travados da Donna.
    pub(crate) fn update_bot_by_user_authorized(
        &self,
        patch: &Value,
        authority: Option<&crate::fork::engine_setup::access::Authority>,
        owner: Option<&crate::fork::autoridade::Autoridade>,
    ) -> Result<Value> {
        if let Some(authority) = authority {
            authority.require_current(self)?;
        }
        crate::fork_avatars::validate_patch(patch)?;
        anyhow::ensure!(
            owner.is_some() || patch.get("browserPreference").is_none(),
            "not_allowed: só o dono escolhe o navegador padrão do bot"
        );
        let permission_chosen = patch.get("permission").is_some() && patch["permissionChosenByUser"] == true;
        if patch.get("permission").is_some() && !permission_chosen {
            bail!("Atualize o app para mudar a autonomia.");
        }
        let id = patch["id"].as_str().ok_or_else(|| anyhow!("id required"))?.to_owned();
        let row = self.store.bot(&id)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("unknown bot"))?;
        crate::fork::skill_selection::refuse_unprotected(patch, &row.config.skills)?;
        let mut merged = serde_json::to_value(&row.config)?;
        for (k, v) in patch.as_object().into_iter().flatten() {
            if !matches!(k.as_str(), "id" | "createdAt" | "createdBy" | "permissionChosenByUser")
                && (k != "permission" || permission_chosen)
            {
                merged[k] = v.clone();
            }
        }
        crate::fork_avatars::merge_patch(&mut merged, patch)?;
        let mut cfg: BotConfig = serde_json::from_value(merged)?;
        crate::fork_avatars::gerador::conjuntos::conferir_vinculo(&self.store, &cfg)?;
        if patch.get("forkEditor").is_some()
            && cfg.fork_editor.engine_account_id.is_some()
            && patch.get("command").is_none()
        {
            cfg.command = None;
        }
        // Perfis completos, inclusive atrasados, não são uma escolha de autonomia.
        if permission_chosen {
            cfg.permission_chosen_by_user = true;
        }
        if cfg.name.trim().is_empty() {
            cfg.name.clone_from(&row.config.name);
        }
        // A name the user picked ends automatic naming; the namer passes `autoName` itself.
        if cfg.name != row.config.name && patch.get("autoName").is_none() {
            cfg.auto_name = false;
        }
        // Options belong to one model; an app that doesn't know them can't clear them itself.
        if (cfg.model != row.config.model || cfg.backend != row.config.backend) && patch.get("modelOptions").is_none() {
            cfg.model_options.clear();
        }
        if cfg.kind != row.config.kind {
            bail!("a bot can't become a group, or a group a bot");
        }
        if cfg.is_group() {
            cfg.members = crate::chat::group::valid_members(&self.store, &cfg.id, &cfg.members)?;
        }
        crate::fork::lider_canal::validate(&mut cfg, Some(&row.config))?;
        validate_update(&mut cfg, &row.config)?;
        // Marketplace: conta só liga em bot do Space dela; padrão precisa ser conta ligada, do serviço e do Space.
        crate::market::contas::validar_vinculo(&self.store, Some(&row.config), &mut cfg)?;
        if cfg.fork_editor != row.config.fork_editor
            || cfg.backend != row.config.backend
            || cfg.model != row.config.model
            || cfg.model_options != row.config.model_options
            || cfg.command != row.config.command
        {
            crate::fork::engine_accounts::managed::validate_selection(&self.store, &cfg)?;
        }
        if cfg.cwd != row.config.cwd {
            crate::agent::workspace::prepare(&mut cfg)?;
        }
        crate::agent::own_memory::settle(&mut cfg, &row.config, patch);
        {
            // Mesmo commit de autoridade de memória, team contextual e resposta ACP.
            let _boundary = self.store.memory_boundary.locked();
            let _g = self.emit_lock.locked();
            // D368: único caminho de configuração do dono pelo app; a marca libera só os campos do dono
            // e vale só dentro da transação que reconfere a autoridade original (e a de contas, se houver).
            match (owner, authority) {
                (Some(owner), _) => {
                    crate::fork::donna_fixa::pedido::antes_de_gravar(&self.store);
                    owner.with_db(self, authority, |db| {
                        // Omissão preserva a escolha canônica também se outra escrita ocorreu antes do commit.
                        if patch.get("browserPreference").is_none() {
                            cfg.browser_preference = Store::bot_on(db, &id)?
                                .ok_or_else(|| anyhow!("unknown bot"))?
                                .config
                                .browser_preference;
                        }
                        crate::fork::donna_fixa::como_dono(db, |db| crate::store::bots::save_bot(db, &cfg))
                    })?
                }
                (None, Some(authority)) => authority.save_bot(self, &cfg)?,
                (None, None) => {
                    crate::fork::donna_fixa::pedido::gravar(&self.store, |db| crate::store::bots::save_bot(db, &cfg))?
                }
            };
            self.store.guard_cleanup()?;
        }
        let cfg = self.store.bot(&cfg.id)?.ok_or_else(|| anyhow!("saved bot missing"))?.config;
        if !cfg.is_group() {
            if crate::chat::memory::shared(&cfg) != crate::chat::memory::shared(&row.config) {
                crate::chat::memory::changed(self, Some(&id));
            }
            if !crate::environment::Environment::is_open() {
                self.send_cmd(&id, Cmd::Reconfigure(Box::new(cfg)))?;
            }
        }
        self.emit_bot(&id);
        Ok(self.bot_value(&id).unwrap_or(Value::Null))
    }

    pub fn delete_bot(&self, id: &str) -> Result<()> {
        crate::fork::donna_fixa::check_delete(&self.store, id)?;
        crate::fork::times::check_bot_deletion(&self.store, id)?;
        let tower_change = crate::fork::control_tower::deletion(self, id)?;
        let led = crate::fork::lider_canal::led_by(&self.store, id)?;
        let committed = {
            let _boundary = self.store.memory_boundary.locked();
            #[cfg(test)]
            {
                let before = self.before_bot_deletion.locked().take();
                if let Some(before) = before {
                    before();
                }
            }
            self.commit_bot_deletion(id)?
        };
        tower_change.publish(self);
        // O líder saiu na transação: os canais chegam ao app mesmo se a limpeza de `members` abaixo falhar.
        for group in &led {
            self.publish_bot(group);
        }
        crate::fork::owner_question::deleted(self, id);
        crate::fork::runtime_audiencia::screen::changed(self, id);
        crate::fork::whatsapp::bot_deleted(self, id);
        // Its shared files are unreachable once the bot is deleted.
        let files = crate::chat::files::root(id);
        let bot = id.to_owned();
        tokio::task::spawn_blocking(move || {
            crate::chat::memory::files::remove_bot(&bot);
            if let Err(error) = std::fs::remove_dir_all(&files)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(error = %error, "couldn't remove a deleted bot's shared files");
            }
        });
        // A deleted bot leaves its groups; what it said there stays.
        for row in self.store.bots().with_context(|| {
            format!("bot_delete_committed:{committed}: exclusão confirmada; limpeza de canais pendente")
        })? {
            let mut group = row.config;
            if !row.deleted && group.is_group() && group.members.iter().any(|m| m == id) {
                group.members.retain(|m| m != id);
                {
                    let _g = self.emit_lock.locked();
                    self.store.save_bot(&group).with_context(|| {
                        format!("bot_delete_committed:{committed}: exclusão confirmada; limpeza de canal pendente")
                    })?;
                }
                self.emit_bot(&group.id);
            }
        }
        Ok(())
    }

    /// Marks what the user has seen as read. Nothing unread there is a no-op, so clients
    /// may call it whenever a chat is on screen.
    pub fn mark_read(&self, id: &str) -> Result<()> {
        let Some(row) = self.store.bot(id)? else { bail!("unknown bot") };
        if self.store.unread(id, row.read_rev) == 0 {
            return Ok(());
        }
        let _g = self.emit_lock.locked();
        self.store.mark_read(id)?;
        if let Some(v) = self.bot_value(id) {
            let _ = self.events.send(json!({"type": "bot", "rev": v["rev"], "bot": v}));
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn bot_value_for_test(&self, id: &str) -> Value {
        self.bot_value(id).unwrap_or(Value::Null)
    }

    fn bot_value(&self, id: &str) -> Option<Value> {
        self.store.bot(id).ok().flatten().map(|r| self.bot_json(&r))
    }

    /// Publica o estado já gravado, com a revisão que ele tem, sem gravar de novo.
    pub(crate) fn publish_bot(&self, id: &str) {
        let _g = self.emit_lock.locked();
        if let Some(v) = self.bot_value(id) {
            let _ = self.events.send(json!({"type": "bot", "rev": v["rev"], "bot": v}));
        }
    }

    /// Bumps the bot's rev and broadcasts its current state.
    pub fn emit_bot(&self, id: &str) {
        let _g = self.emit_lock.locked();
        if self.store.touch_bot(id).is_ok()
            && let Some(v) = self.bot_value(id)
        {
            let _ = self.events.send(json!({"type": "bot", "rev": v["rev"], "bot": v}));
        }
        crate::fork::runtime_audiencia::screen::changed(self, id);
    }

    pub fn set_runtime(&self, id: &str, f: impl FnOnce(&mut Runtime)) {
        self.change_runtime(id, f);
    }

    /// O ator admite somente sua preparação de sessão; conserva o token já emitido.
    pub(crate) fn prepare_session_binding(&self, binding: &crate::fork::sessoes::bindings::BindingRef) -> Result<()> {
        let runtime = self.runtime.locked();
        let current = runtime.get(&binding.bot).ok_or_else(|| anyhow!("chamada vencida"))?;
        crate::fork::sessoes::gateway::prepare(&self.store, binding, current)
    }

    /// Atalho só de teste: confere a conta do padrão SALVO do bot. Diferente da produção, lê esse padrão ANTES de
    /// `memory_boundary` e do runtime e depois segue `prepare_captured_turn` (mesmos locks e mesma guarda de setup).
    #[cfg(test)]
    pub(crate) fn prepare_bot_turn(
        &self,
        id: &str,
        restored: Option<&str>,
        f: impl FnOnce(&mut Runtime),
    ) -> Result<BotConfig> {
        let saved = self.store.bot(id)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("unknown bot"))?.config;
        self.prepare_captured_turn(id, restored, saved.fork_editor.engine_account_id.as_deref(), f)
    }

    /// Lê o perfil sob a mesma reserva de estado do Desfazer, antes do prompt ACP.
    /// D372: `captured` = conta da execução já capturada para a lane (seleção efetiva da conversa, ou o padrão em
    /// rotina/canal; `None` = sem conta). A guarda de setup confere essa conta, não o padrão atual nem a próxima.
    pub(crate) fn prepare_captured_turn(
        &self,
        id: &str,
        restored: Option<&str>,
        captured: Option<&str>,
        f: impl FnOnce(&mut Runtime),
    ) -> Result<BotConfig> {
        let _boundary = self.store.memory_boundary.locked();
        self.change_runtime(id, |r| {
            let cfg = self.store.bot(id)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("unknown bot"))?.config;
            if let Some(id) = captured {
                anyhow::ensure!(
                    !crate::fork::engine_setup::in_setup(&self.store, id),
                    "setup_in_progress: aguarde ou cancele o setup da conta."
                );
            }
            f(r);
            let token = restored.map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_owned);
            anyhow::ensure!(!token.is_empty(), "admission_execution_missing");
            r.preparing_turn_token = Some(token);
            r.memory_turn_token = None;
            r.status = BotStatus::Waiting;
            r.started_at = None;
            Ok(cfg)
        })
    }

    #[cfg(test)]
    /// Turno já liberado, como `start_bot_turn` da main: mesma identidade preparada e ativada.
    /// Testes do preparo (VG03) usam `prepare_bot_turn` diretamente.
    pub(crate) fn start_bot_turn(&self, id: &str, f: impl FnOnce(&mut Runtime)) -> Result<BotConfig> {
        let cfg = self.prepare_bot_turn(id, None, f)?;
        let token = self.runtime(id).preparing_turn_token.ok_or_else(|| anyhow!("admission_execution_missing"))?;
        self.activate_bot_turn(id, &token)?;
        Ok(cfg)
    }

    pub(crate) fn activate_bot_turn(&self, id: &str, token: &str) -> Result<()> {
        let _boundary = self.store.memory_boundary.locked();
        self.change_runtime(id, |r| {
            anyhow::ensure!(r.preparing_turn_token.as_deref() == Some(token), "admission_execution_conflict");
            r.memory_turn_token = Some(token.to_owned());
            r.preparing_turn_token = None;
            r.status = BotStatus::Working;
            r.set_activity("Pensando…", Some(crate::fork::activity_code::ActivityKind::Thinking));
            r.started_at = Some(crate::store::now_ms());
            Ok(())
        })
    }

    /// Serializa encerramento ou suspensão com o worker de memória.
    pub(crate) fn set_memory_runtime(&self, id: &str, f: impl FnOnce(&mut Runtime)) {
        let _boundary = self.store.memory_boundary.locked();
        self.set_runtime(id, |r| {
            r.memory_turn_token = None;
            f(r);
        });
    }

    fn change_runtime<T>(&self, id: &str, f: impl FnOnce(&mut Runtime) -> T) -> T {
        let (before, after, result, observed) = {
            let mut map = self.runtime.locked();
            // Um callback que aguardava a exclusão não recria runtime do tombstone.
            if !map.contains_key(id) && self.store.bot(id).ok().flatten().is_some_and(|row| row.deleted) {
                return f(&mut Runtime::default());
            }
            let observed = map.contains_key(id);
            let rt = map.entry(id.to_owned()).or_default();
            let before = rt.clone();
            let result = f(rt);
            // VG03: a identidade preparada (antes do permit) é a mesma do turno ativo.
            let identity = crate::fork::sessoes::gateway::turn_identity(rt);
            if (crate::fork::sessoes::gateway::turn_identity(&before) != identity
                || before.lane != rt.lane
                || before.session != rt.session
                || identity.is_none())
                && let Err(error) = crate::fork::sessoes::gateway::clear(&self.store.db.locked(), id)
            {
                tracing::error!(%error, "falha ao retirar preparação de sessão");
            }
            if before.session.is_some()
                && (rt.session.is_none() || identity.is_none())
                && let Err(error) = crate::fork::sessoes::bindings::deactivate(&self.store, id)
            {
                tracing::error!(%error,"falha ao desativar vínculo de execução");
            }
            self.screen.permission_runtime_changed(id, rt);
            (before, rt.clone(), result, observed)
        };
        crate::fork::control_tower::runtime_changed(self, id, observed.then_some(&before), &after);
        if before.status != after.status
            || before.resume_at != after.resume_at
            || before.activity != after.activity
            || before.activity_code != after.activity_code
            || before.lane != after.lane
            || before.fast_mode != after.fast_mode
            || before.pending_questions != after.pending_questions
        {
            self.emit_bot(id);
            // A group shows what its busy member is doing.
            for lane in [&before.lane, &after.lane].into_iter().flatten() {
                if lane.chat != id {
                    self.emit_bot(&lane.chat);
                }
            }
            self.update_keep_awake();
        }
        if before.status != after.status || before.pending_questions.is_empty() != after.pending_questions.is_empty() {
            push::live_activity_update(self, id);
            // A group's Live Activity follows its busy member.
            for lane in [&before.lane, &after.lane].into_iter().flatten().filter(|l| l.chat != id) {
                push::live_activity_update(self, &lane.chat);
            }
        }
        result
    }

    /// Observação sem fallback; não cria entrada e não altera os cinco estados internos.
    pub(crate) fn observed_runtime(&self, id: &str) -> Option<Runtime> {
        self.runtime.locked().get(id).cloned()
    }

    /// Cerca curta runtime → fonte → DB. Se houver emissão, o chamador adquire emit antes.
    /// Callback sem await/reentrada Hub; `start_bot_turn` já usa runtime → DB.
    pub(crate) fn conector_fence<T>(&self, bot: &str, action: impl FnOnce(&Runtime) -> Result<T>) -> Result<T> {
        let runtime = self.runtime.locked();
        action(runtime.get(bot).ok_or_else(|| anyhow!("Turno indisponível"))?)
    }

    /// Mesmo mutex canônico; o cartão tardio pode existir sem ator/turno.
    pub(crate) fn permission_runtime<T>(
        &self,
        bot: &str,
        action: impl FnOnce(Option<&Runtime>) -> Result<T>,
    ) -> Result<T> {
        let runtime = self.runtime.locked();
        action(runtime.get(bot))
    }

    /// Publica exatamente as projeções da transação, inclusive marcador, na ordem de rev.
    pub(crate) fn conector_publish<T>(&self, action: impl FnOnce() -> Result<(T, Vec<Entry>)>) -> Result<T> {
        let _emit = self.emit_lock.locked();
        let (value, entries) = action()?;
        for entry in entries {
            let _ = self.events.send(json!({"type":"entry","rev":entry.rev,"entry":entry}));
        }
        Ok(value)
    }

    pub fn runtime(&self, id: &str) -> Runtime {
        self.runtime.locked().get(id).cloned().unwrap_or_default()
    }

    /// Exclui preparo e transições para Working/NeedsInput durante restauração síncrona.
    /// O callback não pode emitir eventos nem acessar runtime novamente.
    pub(crate) fn with_inactive_bot<T>(&self, id: &str, change: impl FnOnce() -> Result<T>) -> Result<T> {
        let runtime = self.runtime.locked();
        if runtime.get(id).is_some_and(|r| {
            r.preparing_turn_token.is_some() || matches!(r.status, BotStatus::Working | BotStatus::NeedsInput)
        }) {
            bail!("wait for the bot to finish before undoing");
        }
        let result = change();
        drop(runtime);
        result
    }

    /// D372 (R1): cerca de Contas no mesmo mutex de runtime de todo preparo/ativação. Confere a ociosidade da conta
    /// sobre o mapa já bloqueado e roda `effect` (a escrita da conta) sem soltá-lo. O efeito não emite, não lê runtime,
    /// não chama Hub/`idle` e não pega `memory_boundary`/emit/`team_lock`.
    pub(crate) fn with_idle_engine_account<T>(&self, account: &str, effect: impl FnOnce() -> Result<T>) -> Result<T> {
        let runtime = self.runtime.locked();
        crate::fork::engine_accounts::managed::idle_with(&self.store, account, |bot| {
            runtime.get(bot).map_or(BotStatus::default(), |r| r.status)
        })?;
        let result = crate::fork::open_chat::with_idle_account(account, effect);
        drop(runtime);
        result
    }

    /// Testemunho de teste: o mutex de runtime está retido (por esta thread dentro de uma cerca, ou por outra).
    #[cfg(test)]
    pub(crate) fn runtime_held(&self) -> bool {
        self.runtime.try_lock().is_err()
    }

    pub(crate) fn update_keep_awake(&self) {
        let busy = self.runtime.locked().values().any(|r| r.status == BotStatus::Working);
        self.keep_awake.locked().set(busy || self.routines.keeps_awake());
    }

    // MARK: entries

    /// Persists and broadcasts a new transcript entry. A failed write is logged here
    /// (once) and yields `None`: the conversation goes on without that line.
    pub fn add_entry(&self, lane: &Lane, kind: EntryKind, turn: i64, data: &Value) -> Option<Entry> {
        self.add_entry_in_execution(lane, kind, turn, data, None)
    }
    pub(crate) fn add_entry_in_execution(
        &self,
        lane: &Lane,
        kind: EntryKind,
        turn: i64,
        data: &Value,
        execution: Option<&crate::fork::sessoes::ExecutionRef>,
    ) -> Option<Entry> {
        let _g = self.emit_lock.locked();
        match self.store.insert_visible_in_execution(lane, kind, turn, data, execution) {
            Ok((marker, e)) => {
                // O marcador de sessão sai antes da entrada, sob a mesma trava: o app antigo conectado vê a ordem certa.
                for e in marker.iter().chain([&e]) {
                    let _ = self.events.send(json!({"type": "entry", "rev": e.rev, "entry": e}));
                }
                Some(e)
            }
            Err(error) => {
                tracing::error!(
                    error = format!("{error:#}"),
                    chat = lane.chat,
                    kind = kind.as_str(),
                    "couldn't save transcript entry"
                );
                None
            }
        }
    }
    /// Publica o par somente após a admissão atômica; falha não deixa metade no stream.
    pub(crate) fn add_team_notices(
        &self,
        source: &Lane,
        turn: Option<i64>,
        notices: &[Value; 2],
        execution: Option<&crate::fork::sessoes::ExecutionRef>,
        chain: &crate::fork::cadeias::equipe::Entrada,
    ) -> Result<([Entry; 2], Option<crate::fork::cadeias::modelo::Recibo>)> {
        #[cfg(test)]
        crate::fork::sessoes::call_tests::before_effect(&self.store, "team_notices_commit");
        let _guard = self.emit_lock.locked();
        let (pair, receipt) = self.store.insert_team_notices(source, turn, notices, execution, chain)?;
        for (marker, entry) in &pair {
            for entry in marker.iter().chain([entry]) {
                let _ = self.events.send(json!({"type":"entry","rev":entry.rev,"entry":entry}));
            }
        }
        Ok((pair.map(|(_, entry)| entry), receipt))
    }

    /// Cartão e selo contextual são confirmados antes de qualquer publicação.
    pub(crate) fn add_permission_card(
        &self,
        cfg: &BotConfig,
        lane: &Lane,
        turn: i64,
        data: &Value,
        execution: &crate::fork::sessoes::ExecutionRef,
        pedido: Option<&crate::fork::delegacao::pausa::Pedido<'_>>,
    ) -> Result<Entry> {
        #[cfg(test)]
        crate::fork::sessoes::call_tests::before_effect(&self.store, "permission_card_commit");
        let _guard = self.emit_lock.locked();
        let (marker, entry) = self.store.insert_permission_card(cfg, lane, turn, data, execution, pedido)?;
        for entry in marker.iter().chain([&entry]) {
            let _ = self.events.send(json!({"type":"entry","rev":entry.rev,"entry":entry}));
        }
        Ok(entry)
    }

    pub fn add_entry_once(
        &self,
        id: &str,
        lane: &Lane,
        kind: EntryKind,
        turn: i64,
        data: &Value,
    ) -> Result<(Entry, bool)> {
        let _guard = self.emit_lock.locked();
        let (marker, entry, added) = self.store.insert_entry_once(id, lane, kind, turn, data)?;
        if added {
            for e in marker.iter().chain([&entry]) {
                let _ = self.events.send(json!({"type":"entry","rev":e.rev,"entry":e}));
            }
        }
        Ok((entry, added))
    }

    /// Grava e republica sob a trava do stream, na ordem devolvida: o marcador de sessão sai antes do aviso que ele
    /// precede. Se a gravação falha, nada é publicado e o erro volta ao chamador.
    pub(crate) fn republish(&self, action: impl FnOnce() -> Result<Vec<String>>) -> Result<()> {
        let _guard = self.emit_lock.locked();
        for id in action()? {
            let Some(entry) = self.store.entry(&id) else { continue };
            match self.store.update_entry(&id, &entry.data) {
                Ok(Some(e)) => {
                    let _ = self.events.send(json!({"type": "entry", "rev": e.rev, "entry": e}));
                }
                Ok(None) => {}
                Err(error) => tracing::error!(error = format!("{error:#}"), entry = id, "couldn't republish entry"),
            }
        }
        Ok(())
    }

    /// Updates and broadcasts an entry; failures are logged here, like [`Self::add_entry`].
    pub fn set_entry(&self, id: &str, data: &Value) -> Option<Entry> {
        self.set_entry_in_execution(id, data, None)
    }
    pub(crate) fn set_entry_in_execution(
        &self,
        id: &str,
        data: &Value,
        execution: Option<&crate::fork::sessoes::ExecutionRef>,
    ) -> Option<Entry> {
        let _g = self.emit_lock.locked();
        match self.store.update_entry_in_execution(id, data, execution) {
            Ok(e) => {
                if let Some(e) = &e {
                    let _ = self.events.send(json!({"type": "entry", "rev": e.rev, "entry": e}));
                }
                e
            }
            Err(error) => {
                tracing::error!(error = format!("{error:#}"), entry = id, "couldn't update transcript entry");
                None
            }
        }
    }

    /// Hook do produtor interno: commit da permissão e eventos ficam ordenados.
    pub(crate) fn permit_bot_dispatch(
        &self,
        cfg: &BotConfig,
        ticket: &crate::fork::bot_admission::Ticket,
        checkpoint: Option<&str>,
        ids: &[String],
        expected_session: Option<&str>,
        hooks: crate::fork::bot_admission::ClaimHooks<'_>,
    ) -> Result<bool> {
        let _guard = self.emit_lock.locked();
        let Some(entries) = crate::fork::bot_admission::permit_with(
            &self.store,
            cfg,
            ticket,
            checkpoint,
            ids,
            expected_session,
            hooks,
        )?
        else {
            return Ok(false);
        };
        for entry in entries {
            let _ = self.events.send(json!({"type":"entry", "rev":entry.rev, "entry":entry}));
        }
        Ok(true)
    }

    /// Registra todas as entradas do lote depois de determinar a sessão que as receberá.
    pub(crate) fn observe_memory_session_inputs(&self, bot: &str, session: &str, ids: &[String]) -> Result<()> {
        for id in ids {
            let entry = self.store.entry(id).ok_or_else(|| anyhow::anyhow!("entrada não encontrada"))?;
            self.store.memory_session_turn(bot, &entry.bot_id, session, entry.turn)?;
            crate::chat::memory::observe_session_input(self, bot, session, &entry.data)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn set_usage(&self, usage: Usage) {
        let _ = self.events.send(json!({"type": "usage", "usage": usage}));
        *self.usage.locked() = usage;
    }

    // MARK: devices

    /// Tells open channels (and the ACL publisher) that devices or pairing offers changed.
    pub fn auth_changed(&self) {
        self.auth.send_modify(|g| *g += 1);
    }

    /// Removes an authorized device: its tickets go with it and its open channels close.
    pub fn revoke_device(&self, key: &str) -> Result<bool> {
        let removed = self.store.remove_device(key)?;
        if removed {
            tracing::info!(device = key, "device revoked");
            self.auth_changed();
        }
        Ok(removed)
    }

    pub fn phone_connected(&self) -> bool {
        self.phone_clients.load(Ordering::Relaxed) > 0
    }
}

pub(crate) fn validate(cfg: &mut BotConfig) -> Result<()> {
    crate::fork_avatars::validate_config(cfg)?;
    crate::fork::pasta::normalizar_bot(cfg)?;
    crate::fork::funcao::normalizar(cfg)?;
    validate_fields(cfg)
}

/// Um patch de outro campo não revalida nem reescreve a pasta salva, inclusive legada.
pub(crate) fn validate_update(cfg: &mut BotConfig, before: &BotConfig) -> Result<()> {
    crate::fork_avatars::validate_config(cfg)?;
    if cfg.cwd != before.cwd {
        crate::fork::pasta::normalizar_bot(cfg)?;
    }
    crate::fork::funcao::normalizar(cfg)?;
    validate_fields(cfg)
}

fn validate_fields(cfg: &BotConfig) -> Result<()> {
    if cfg.name.trim().is_empty() {
        bail!("name is required");
    }
    if cfg.is_group() {
        return Ok(());
    }
    if cfg.command.as_deref().map(str::trim).unwrap_or_default().is_empty()
        && !crate::agent::backends::is_known(&cfg.backend)
    {
        bail!("unknown backend {}", cfg.backend);
    }
    crate::agent::launcher::check(cfg)?;
    Ok(())
}
