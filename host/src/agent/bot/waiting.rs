//! D72: espera da assinatura pertence ao host, nunca a um fallback de provedor.
use super::{Actor, Adapter, Done, Seg};
use crate::LockExt;
use crate::agent::{
    acp::RpcError,
    subscription::{self, Limit},
};
use crate::fork::activity_code::ActivityKind;
use crate::hub::BotStatus;
use crate::store::{BotConfig, EntryKind, Lane, Store, now_ms};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::mpsc;
mod spaces;
#[path = "../../fork/usage_limits/ator.rs"]
mod usage_controls;
use usage_controls::UsageControl;

const RETRY_MS: i64 = 5 * 60 * 1000;
const MAX_RETRY_MS: i64 = 60 * 60 * 1000;
const MAX_BLOCKS: u8 = 6;
const HINT_TTL_MS: i64 = 60_000;
const USAGE_CHECK_MS: i64 = 5_000;
const CONTINUE: &str = "[O limite da assinatura interrompeu este pedido. Continue de onde parou, usando o contexto da sessão. Não repita ações já concluídas. O pedido pendente está abaixo.]";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TurnInput {
    pub chat: String,
    pub entries: Vec<String>,
    pub text: String,
    pub hidden: bool,
    pub handoff: Option<String>,
    pub routine: Option<String>,
    /// Canais de grupo/delegação não sobrevivem ao host; não os promover a pedidos do usuário.
    pub transient: bool,
    pub backend: String,
    pub model: Option<String>,
    pub command: Option<String>,
    pub session: Option<String>,
    /// Selo de Hub/contexto/conta/modelo capturado na admissão, nunca resolvido pelo Retry.
    #[serde(default)]
    pub context_binding: Option<String>,
    /// Sessão visível do turno (`None` = principal); a retomada volta nela, nunca na principal por engano.
    #[serde(default)]
    pub visible_session: Option<String>,
}

impl TurnInput {
    pub(super) fn lane(&self) -> Lane {
        Lane { chat: self.chat.clone(), session: self.visible_session.clone() }
    }

    pub(super) fn direct_chat(&self, cfg: &BotConfig) -> bool {
        self.chat == cfg.session_owner()
            && (cfg.context.is_none() || self.context_binding.is_some())
            && !self.hidden
            && !self.transient
            && self.routine.is_none()
            && !self.entries.is_empty()
    }

    /// MX-263: além da conversa direta, o turno autônomo de rotina também entra em
    /// Aguardando — a mesma espera D72, um consumidor só por bot. Turno oculto ou
    /// transitório (grupo, delegação, compactação, pedido de outro bot) segue fora.
    fn waitable(&self, cfg: &BotConfig) -> bool {
        self.direct_chat(cfg) || (self.routine.is_some() && !self.hidden && !self.transient)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Phase {
    Waiting,
    Retrying,
    Finished,
    Paused,
    Completing,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WaitingTurn {
    #[serde(default)]
    pub execution_token: Option<String>,
    #[serde(default)]
    memory_policy: Option<crate::fork::bot_admission::MemoryPolicy>,
    /// Classificação de roteamento do permit original (D305), copiada como o token; ausente pausa a espera.
    #[serde(default)]
    routing: Option<crate::fork::bot_admission::RoutingClass>,
    input: TurnInput,
    #[serde(default)]
    usage_control: UsageControl,
    phase: Phase,
    pub resume_at: Option<i64>,
    retry_at: i64,
    backend: String,
    model: Option<String>,
    command: Option<String>,
    notice: Option<String>,
    #[serde(default)]
    fresh_session: bool,
    #[serde(default)]
    completion: Option<Completion>,
    observed_at: i64,
    #[serde(default, alias = "noResetCount")]
    attempt_count: u8,
    #[serde(default)]
    switch_error: Option<String>,
    #[serde(skip)]
    usage_checked_at: i64,
    #[serde(skip)]
    save_retry_at: i64,
    #[serde(skip)]
    dirty: bool,
}

/// Resultado pronto para publicação; sua recuperação nunca chama session/prompt.
#[derive(Clone, Serialize, Deserialize)]
struct Completion {
    #[serde(default)]
    memory_job: Option<String>,
    #[serde(default)]
    memory_blocked: bool,
    answer: Option<Value>,
    error: Option<String>,
    turn: Option<i64>,
    last_text: Option<String>,
    sent: Vec<String>,
    reacted: super::Reacted,
    file_summary: Option<String>,
    turn_text: Option<String>,
    turn_entries: Vec<String>,
    stopped: bool,
    #[serde(default)]
    routine_terminal: bool,
    stream: Option<(String, Value)>,
    tools: std::collections::HashMap<String, String>,
    permissions: Vec<String>,
    #[serde(default)]
    cancelled_entries: Vec<String>,
}

impl Completion {
    fn done(&self) -> Done {
        self.error.as_ref().map_or_else(
            || Ok(self.answer.clone().unwrap_or(Value::Null)),
            |e| {
                if e == crate::fork::antigravity::MODE_UNCONFIRMED {
                    Err(crate::fork::antigravity::ModeUnconfirmed.into())
                } else if let Some(reason) = crate::fork::anthropic_automation::Refusal::from_saved_error(e) {
                    Err(reason.into())
                } else {
                    Err(anyhow::anyhow!("{e}"))
                }
            },
        )
    }
}

fn key(bot: &str) -> String {
    format!("turn.waiting.{bot}")
}

/// Checkpoints inválidos não podem bloquear as operações normais do bot.
impl Actor {
    /// A conclusão do turno já está salva para publicar: o texto dela não muda mais.
    pub(super) fn completing(&self) -> bool {
        self.waiting.as_ref().is_some_and(|w| w.phase == Phase::Completing)
    }
}

pub(crate) fn read_waiting_checkpoint(store: &Store, bot: &str) -> Result<Option<Value>> {
    let key = key(bot);
    let Some(raw) = store.kv_read(&key)? else { return Ok(None) };
    let parsed = serde_json::from_str::<Value>(&raw).and_then(|value| {
        if value["phase"] != "finished" {
            serde_json::from_value::<WaitingTurn>(value.clone())?;
        }
        Ok(value)
    });
    match parsed {
        Ok(value) => Ok(Some(value)),
        Err(error) => {
            tracing::warn!(bot, %error, "espera persistida inválida; ignorando checkpoint");
            if let Err(error) = store.kv_delete(&key) {
                tracing::warn!(bot, %error, "não foi possível remover a espera inválida");
            }
            Ok(None)
        }
    }
}

pub(crate) fn checkpoint_waiting_active(value: &Value) -> bool {
    serde_json::from_value::<WaitingTurn>(value.clone()).is_ok_and(|waiting| waiting.active())
}

impl WaitingTurn {
    pub(super) fn active(&self) -> bool {
        matches!(self.phase, Phase::Waiting | Phase::Paused | Phase::Retrying)
    }

    /// MX-263: a execução de rotina dona desta espera (o gatilho normal não a reenfileira).
    pub(super) fn routine_id(&self) -> Option<&str> {
        self.input.routine.as_deref()
    }

    fn new(input: TurnInput, now: i64) -> Self {
        Self {
            execution_token: None,
            usage_control: UsageControl::default(),
            memory_policy: None,
            routing: None,
            backend: input.backend.clone(),
            model: input.model.clone(),
            command: input.command.clone(),
            input,
            phase: Phase::Waiting,
            resume_at: None,
            retry_at: now,
            notice: None,
            fresh_session: false,
            completion: None,
            observed_at: now,
            attempt_count: 0,
            switch_error: None,
            usage_checked_at: now,
            save_retry_at: 0,
            dirty: true,
        }
    }

    fn selected(&self, cfg: &BotConfig) -> bool {
        self.backend == cfg.backend && self.model == cfg.model && self.command == cfg.command
    }

    fn due(&self, now: i64) -> bool {
        self.phase == Phase::Waiting && now >= self.retry_at
    }

    fn block(&mut self, reset: Option<i64>, now: i64) {
        self.phase = Phase::Waiting;
        self.resume_at = reset.filter(|at| *at > now);
        // O bloqueio inicial conta uma vez; retomadas já contam antes do envio.
        self.attempt_count = self.attempt_count.max(1);
        if self.attempt_count >= MAX_BLOCKS {
            self.phase = Phase::Paused;
        }
        self.schedule(now);
    }

    fn begin_retry(&mut self, now: i64) {
        self.phase = Phase::Retrying;
        self.attempt_count = self.attempt_count.saturating_add(1);
        self.schedule(now);
    }

    fn schedule(&mut self, now: i64) {
        let backoff = RETRY_MS.saturating_mul(1_i64 << self.attempt_count.saturating_sub(1).min(8));
        self.retry_at = self.resume_at.map_or_else(
            || now.saturating_add(backoff.min(MAX_RETRY_MS)),
            |at| at.saturating_add(1000).max(now.saturating_add(RETRY_MS)),
        );
    }
}

impl Actor {
    pub(super) fn restored_execution_token(&self, cfg: &BotConfig) -> Result<Option<String>> {
        let Some(waiting) = &self.waiting else { return Ok(None) };
        let token = waiting
            .execution_token
            .as_deref()
            .filter(|token| !token.is_empty())
            .ok_or_else(|| anyhow::anyhow!("admission_legacy_execution_missing"))?;
        anyhow::ensure!(
            waiting.memory_policy.as_ref() == Some(&crate::fork::bot_admission::MemoryPolicy::of_config(cfg)),
            "admission_memory_policy_changed"
        );
        Ok(Some(token.to_owned()))
    }
    fn limit_backend(&self) -> &str {
        self.conn.as_ref().map_or(self.cfg.backend.as_str(), |c| match c.adapter {
            Adapter::Codex => "codex",
            Adapter::Claude => "claude",
            Adapter::Other | Adapter::Mexerica => self.cfg.backend.as_str(),
        })
    }

    /// Sessões PR5: a conversa e a sessão visível da espera em andamento (para encerrar só a dela).
    pub(super) fn waiting_lane(&self) -> Option<Lane> {
        self.waiting.as_ref().map(|waiting| waiting.input.lane())
    }

    /// Sessões PR5: a espera em andamento é uma conclusão durável (resposta já produzida, só falta publicar).
    pub(super) fn waiting_completing(&self) -> bool {
        self.waiting.as_ref().is_some_and(|waiting| waiting.phase == Phase::Completing)
    }

    pub(super) fn waiting_for_reset(&self) -> bool {
        self.waiting.as_ref().is_some_and(|w| matches!(w.phase, Phase::Waiting | Phase::Paused))
    }

    pub(super) fn waiting_active(&self) -> bool {
        self.waiting.as_ref().is_some_and(WaitingTurn::active)
    }

    /// MX-145: o envio em preparo é a retomada D72 (o teto devolve a ela, nunca a um segundo consumidor).
    pub(super) fn waiting_retrying(&self) -> bool {
        self.waiting.as_ref().is_some_and(|w| w.phase == Phase::Retrying)
    }

    /// MX-145: a retomada D72 parou no teto ANTES do envio — volta a `Waiting`
    /// sem contar a tentativa; turno e entrada continuam da espera, como no boot.
    pub(super) fn ceiling_hold_waiting(&mut self) {
        if let Some(waiting) = self.waiting.as_mut()
            && waiting.phase == Phase::Retrying
        {
            waiting.phase = Phase::Waiting;
            waiting.attempt_count = waiting.attempt_count.saturating_sub(1);
            self.save_waiting_logged();
        }
    }

    /// MX-145: teto liberado — a espera D72 fica devida agora e o próprio
    /// `retry_waiting` retoma. Pausada continua pausada (decisão do dono).
    pub(super) fn ceiling_resume_waiting(&mut self) -> Result<()> {
        if let Some(waiting) = self.waiting.as_mut()
            && waiting.phase == Phase::Waiting
        {
            waiting.retry_at = now_ms();
            self.save_waiting()?;
        }
        if self.waiting.is_some() {
            // Sai o "parado pelo teto": vale de novo o estado da espera D72.
            self.publish_waiting();
        }
        Ok(())
    }

    pub(super) fn note_limit(&mut self, update: &Value) {
        if let Some(limit) = subscription::detect(self.limit_backend(), None, "", update, now_ms()) {
            self.turn_limit = Some((limit, now_ms()));
        }
        let info = &update["_meta"]["_claude/rateLimit"];
        if matches!(info["status"].as_str(), Some("allowed" | "allowed_warning")) {
            self.turn_limit = None;
        }
    }

    /// Eventos de uso só completam o reset de um resultado que já prova o limite.
    pub(super) fn result_limit(&self, done: &Done) -> Option<Limit> {
        let backend = self.limit_backend();
        let now = now_ms();
        let hint = self.turn_limit.filter(|(_, at)| now.saturating_sub(*at) <= HINT_TTL_MS).map(|(limit, _)| limit);
        let mut limit = match done {
            Err(error) => error
                .chain()
                .find_map(|e| e.downcast_ref::<RpcError>())
                .and_then(|rpc| subscription::detect(backend, rpc.kind.as_deref(), &rpc.message, &rpc.data, now)),
            Ok(value) if value["stopReason"].as_str().is_some_and(subscription::limit_code) => {
                subscription::detect(backend, value["stopReason"].as_str(), "", value, now)
            }
            Ok(_) => None,
        }?;
        limit.resets_at = limit.resets_at.or_else(|| hint.and_then(|hint| hint.resets_at));
        Some(limit)
    }

    pub(super) fn wait_on_limit(&mut self, Limit { resets_at }: Limit) -> bool {
        if self.stop_requested
            || !self.input.as_ref().is_some_and(|input| input.waitable(&self.cfg))
            || self.compacting.is_some()
            || self.waiting.as_ref().is_some_and(|w| w.phase == Phase::Completing)
        {
            return false;
        }
        let backend = self.limit_backend().to_owned();
        let now = now_ms();
        self.turn_limit = None;
        let reset = resets_at
            .or_else(|| subscription::usage_reset(&self.hub.usage.locked(), &backend, self.cfg.model.as_deref(), now));
        // A tentativa cortada pelo limite fica na conversa como interrompida, como no fim de turno sem espera; só a marca
        // tira o `streaming`, e se ela falhar a varredura marca depois.
        let shown = self.sent.is_empty()
            && matches!(self.reacted, super::Reacted::No)
            && !self.active_request.as_ref().is_some_and(crate::chat::team::BotRequest::expects_reply);
        match self.last_text.clone().filter(|_| shown) {
            Some(id) => {
                self.flush(true);
                self.mark_interrupted(&id);
                self.seg = Seg::None;
            }
            None => {
                self.close_seg();
            }
        }
        self.files.cancel();
        self.close_turn_work(true);
        let had_waiting = self.waiting.is_some();
        let mut waiting = self
            .waiting
            .take()
            .unwrap_or_else(|| WaitingTurn::new(self.input.clone().expect("turn input checked"), now));
        waiting.input = self.input.clone().expect("turn input checked");
        let identity_proven = if let Ok(Some(record)) =
            crate::fork::bot_admission::current(&self.hub.store, &self.cfg.id)
            && self.dispatch.as_ref() == Some(&record.ticket())
        {
            if waiting.execution_token.is_none() {
                waiting.execution_token = Some(record.execution_token.clone());
                waiting.memory_policy.clone_from(&record.memory_policy);
                waiting.routing.clone_from(&record.routing);
            }
            waiting.execution_token.as_deref() == Some(record.execution_token.as_str())
                && waiting.memory_policy.is_some()
                && waiting.memory_policy == record.memory_policy
                // A classe nasce no permit: uma nova reserva (retry) ainda não a tem e não a contradiz.
                && waiting.routing.is_some()
                && (record.routing.is_none() || waiting.routing == record.routing)
        } else {
            false
        };
        // MX-263: sem identidade de admissão comprovada a espera D72 pausaria
        // para sempre — a rotina falha fechado, como antes, em vez de estacionar.
        // A espera anterior retirada pelo `take()` volta e é persistida: largá-la
        // deixaria o checkpoint gravado (`retrying`) sem consumidor vivo.
        if !identity_proven && waiting.input.routine.is_some() {
            if had_waiting {
                self.waiting = Some(waiting);
                self.save_waiting_logged();
            }
            return false;
        }
        waiting.fresh_session = false;
        waiting.observed_at = now;
        waiting.usage_checked_at = now;
        waiting.usage_control.manual = false;
        waiting.block(reset, now);
        if !identity_proven {
            waiting.phase = Phase::Paused;
            waiting.switch_error = Some("A espera não tem identidade/política de execução comprovada. Confira os efeitos e envie um pedido novo; nenhuma identidade foi criada para retomar.".into());
        }
        if let Err(error) = crate::fork::memory_auto::queue::abandon(&self.hub, &self.cfg.id) {
            tracing::warn!(%error,"não foi possível registrar interrupção das propostas de memória");
        }
        if self.input.as_ref().is_some_and(|input| input.routine.is_some()) {
            // MX-263: a run volta a Recovering ANTES de a espera existir — a
            // retomada prepara uma continuação da cadeia, como no reinício, e nunca
            // um segundo `started` sobre o recibo já despachado. Se a transição não
            // persistir, falha fechado: uma espera sem ela recusaria a preparação
            // da continuação e ficaria `retrying` para sempre com a fila presa.
            // A janela inversa (run `Recovering` sem espera, por crash aqui no
            // meio) é benigna: o boot trata run `Recovering` órfã pela recuperação
            // normal de runs — a ordem escolhida prioriza "nunca espera sem
            // transição persistida".
            // Em qualquer saída abortada a espera retirada pelo `take()` volta e
            // é persistida — nunca um checkpoint órfão sem consumidor vivo.
            let Some(run_id) = self.active_routine.clone() else {
                if had_waiting {
                    self.waiting = Some(waiting);
                    self.save_waiting_logged();
                }
                return false;
            };
            if self.hub.routines.mark_waiting(&self.hub.store, &self.cfg.id, &run_id).is_err() {
                if had_waiting {
                    self.waiting = Some(waiting);
                    self.save_waiting_logged();
                }
                return false;
            }
            self.routine_attempt = None;
        }
        self.waiting = Some(waiting);
        if let Err(error) = self.apply_usage_policy() {
            self.pause_waiting_switch(&error);
        }
        self.save_waiting_logged();
        self.publish_waiting();
        self.end_paused_routine_wait();
        true
    }

    /// Remove do checkpoint da lane a passagem que não pode atravessar uma troca de pessoa.
    pub(super) fn discard_waiting_handoff(&mut self, lane: &Lane) -> Result<()> {
        if let Some(waiting) = self.waiting.as_mut().filter(|waiting| waiting.input.lane() == *lane) {
            waiting.input.handoff = None;
            self.save_waiting()?;
        }
        Ok(())
    }

    fn save_waiting(&mut self) -> Result<()> {
        let result = self.waiting.as_ref().map_or(Ok(()), |waiting| {
            let raw = serde_json::to_string(waiting)?;
            if let Some(completion) = &waiting.completion {
                let success = completion.error.is_none()
                    && completion.answer.as_ref().is_some_and(|v| v["stopReason"] == "end_turn");
                let stopped =
                    completion.stopped || completion.answer.as_ref().is_some_and(|v| v["stopReason"] == "cancelled");
                let audit_context = crate::fork::memory_auto::checkpoint(
                    &self.hub.store,
                    &key(&self.cfg.id),
                    &raw,
                    completion.memory_job.as_deref(),
                    success,
                    stopped,
                )?;
                if let Some(context) = audit_context {
                    crate::fork::memory_auto::queue::notify_checkpoint(&self.hub, &context);
                }
                Ok(())
            } else {
                self.hub.store.kv_set(&key(&self.cfg.id), &raw)
            }
        });
        if let Some(waiting) = self.waiting.as_mut() {
            waiting.dirty = result.is_err();
            waiting.save_retry_at = now_ms().saturating_add(USAGE_CHECK_MS);
        }
        result
    }

    /// MX-263: uma preparação transitória que falhou dentro da retomada devolve
    /// o turno à espera dona da execução, que tenta de novo pelo próprio ciclo
    /// em vez de reenfileirar a run (a fila a trataria como recuperação legada).
    pub(super) fn rollback_routine_wait_retry(&mut self) -> bool {
        let Some(waiting) = self.waiting.as_mut().filter(|w| w.active() && w.input.routine.is_some()) else {
            return false;
        };
        // O `retry_at` com backoff de `schedule()` (calculado em `begin_retry`) é
        // preservado: fixar +1s aqui contornava o backoff e o `attempt_count`
        // nunca alcançava o teto — falha persistente de preparo retomaria para
        // sempre segurando a vaga do bot. Esgotado o teto, a espera pausa e a
        // política de rotina pausada encerra a execução liberando a fila.
        waiting.phase = if waiting.attempt_count >= MAX_BLOCKS { Phase::Paused } else { Phase::Waiting };
        waiting.dirty = true;
        // O turno e o input ficam: sem eles, uma rotina desativada até a próxima
        // tentativa não encontraria `finish_turn` para cancelar a espera. Se a
        // tentativa abortada já tinha zerado o turno, aluga um só para encerrar.
        self.turn = self.turn.or_else(|| Some(self.hub.store.max_turn(&self.lane.chat)));
        self.save_waiting_logged();
        self.publish_waiting();
        self.end_paused_routine_wait();
        true
    }

    fn save_waiting_logged(&mut self) {
        match self.save_waiting() {
            Ok(()) if self.waiting_for_reset() && self.waiting_owns_dispatch() => self.finish_dispatch(),
            Ok(()) => {}
            Err(error) => {
                tracing::error!(bot = %self.cfg.id, %error, "não foi possível persistir a espera; nova tentativa em cinco segundos");
            }
        }
    }

    fn waiting_owns_dispatch(&self) -> bool {
        let (Some(waiting), Some(ticket)) = (&self.waiting, &self.dispatch) else { return false };
        crate::fork::bot_admission::current(&self.hub.store, &self.cfg.id).is_ok_and(|record| {
            record.is_some_and(|record| {
                record.ticket() == *ticket
                    && waiting.execution_token.as_deref() == Some(record.execution_token.as_str())
                    && waiting.memory_policy.is_some()
                    && waiting.memory_policy == record.memory_policy
                    && waiting.routing.is_some()
                    && (record.routing.is_none() || waiting.routing == record.routing)
            })
        })
    }

    pub(super) fn checkpoint_completion(&mut self, done: &Done) -> Result<()> {
        let memory_job = crate::fork::memory_auto::queue::active_job(&self.hub, &self.cfg.id)?;
        if self.waiting.is_none() && memory_job.is_some() {
            // Turno normal também publica sucesso/falha atomicamente. Reutiliza o input original,
            // sem criar espera de assinatura, nova autoridade ou um pedido que possa ser reenviado.
            let input = self
                .input
                .clone()
                .filter(|input| input.direct_chat(&self.cfg))
                .ok_or_else(|| anyhow::anyhow!("memory_completion_input_unavailable"))?;
            let mut waiting = WaitingTurn::new(input, now_ms());
            if let Some(ticket) = &self.dispatch {
                let record = crate::fork::bot_admission::current(&self.hub.store, &self.cfg.id)?
                    .ok_or_else(|| anyhow::anyhow!("admission_dispatch_missing"))?;
                anyhow::ensure!(record.ticket() == *ticket, "admission_dispatch_conflict");
                waiting.execution_token = Some(record.execution_token);
                waiting.memory_policy = record.memory_policy;
                waiting.routing = record.routing;
            }
            // Nasce Completing abaixo, nunca uma Waiting inicial que autorize replay.
            self.waiting = Some(waiting);
        }
        let Some(waiting) = self.waiting.as_mut() else { return Ok(()) };
        if waiting.completion.is_none() {
            let stream = match &self.seg {
                Seg::Open { kind, entry_id, buf, .. } => Some((
                    entry_id.clone(),
                    match kind {
                        super::SegKind::Text => {
                            json!({"text":crate::fork::planos_codigo::mascarar_saida(buf, true),"final":false,"author":self.cfg.id})
                        }
                        super::SegKind::Thought => {
                            json!({"text":crate::fork::planos_codigo::mascarar_saida(buf, true),"author":self.cfg.id})
                        }
                    },
                )),
                Seg::None => None,
            };
            waiting.completion = Some(Completion {
                memory_job,
                memory_blocked: self.hub.runtime(&self.cfg.id).memory_exposure_error,
                answer: done.as_ref().ok().cloned(),
                error: done.as_ref().err().map(|e| format!("{e:#}")),
                turn: self.turn,
                last_text: self.last_text.clone(),
                sent: self.sent.clone(),
                reacted: self.reacted.clone(),
                file_summary: self.files.summary.clone(),
                turn_text: self.turn_text.clone(),
                turn_entries: self.turn_entries.clone(),
                stopped: self.stop_requested,
                routine_terminal: self.routine_terminal,
                stream,
                tools: self.tools.clone(),
                permissions: self.perms.keys().cloned().collect(),
                cancelled_entries: self.cancel_queued.clone(),
            });
        }
        waiting.phase = Phase::Completing;
        self.save_waiting()?;
        if let Some(job) =
            self.waiting.as_ref().and_then(|w| w.completion.as_ref()).and_then(|c| c.memory_job.as_deref())
        {
            crate::fork::memory_auto::apply::finish_prepared(&self.hub, job)?;
        }
        if self.waiting.as_ref().and_then(|w| w.completion.as_ref()).is_some_and(|c| {
            c.memory_job.is_some()
                && c.error.is_none()
                && !c.stopped
                && !c.memory_blocked
                && c.answer.as_ref().is_some_and(|v| v["stopReason"] == "end_turn")
        }) {
            let _ = self.dreamer.send(crate::chat::memory::DreamEvent::Wake);
        }
        self.apply_completion()
    }

    fn apply_completion(&mut self) -> Result<()> {
        let Some(completion) = self.waiting.as_ref().and_then(|w| w.completion.clone()) else { return Ok(()) };
        if let Some((id, data)) = &completion.stream {
            self.hub
                .set_entry(id, data)
                .ok_or_else(|| anyhow::anyhow!("não foi possível salvar a resposta completa"))?;
        }
        for id in &completion.permissions {
            if let Some(mut entry) = self.hub.store.entry(id) {
                entry.data["status"] = json!("expired");
                self.hub
                    .set_entry(id, &entry.data)
                    .ok_or_else(|| anyhow::anyhow!("não foi possível expirar permissão"))?;
            }
        }
        for id in &completion.cancelled_entries {
            if let Some(mut entry) = self.hub.store.entry(id) {
                entry.data["status"] = json!("cancelled");
                self.hub
                    .set_entry(id, &entry.data)
                    .ok_or_else(|| anyhow::anyhow!("não foi possível cancelar mensagem pendente"))?;
            }
        }
        self.turn = completion.turn;
        self.last_text = completion.last_text;
        self.sent = completion.sent;
        self.reacted = completion.reacted;
        self.files.summary = completion.file_summary;
        self.turn_text = completion.turn_text;
        self.turn_entries = completion.turn_entries;
        self.stop_requested = completion.stopped;
        self.routine_terminal = completion.routine_terminal;
        self.tools = completion.tools;
        self.seg = Seg::None;
        Ok(())
    }

    pub(super) fn defer_completion(&mut self, done: &Done) {
        // O motor já respondeu; o que ficou para depois é só a publicação do fim.
        self.mudo_desarmar();
        self.finishing = Some(match done {
            Ok(value) => Ok(value.clone()),
            Err(error) if error.is::<crate::fork::antigravity::ModeUnconfirmed>() => {
                Err(crate::fork::antigravity::ModeUnconfirmed.into())
            }
            Err(error) if let Some(reason) = error.downcast_ref::<crate::fork::anthropic_automation::Refusal>() => {
                Err((*reason).into())
            }
            Err(error) => Err(anyhow::anyhow!("{error:#}")),
        });
    }

    pub(super) fn completion_notice_id(&self) -> Option<String> {
        self.waiting.as_ref().filter(|w| w.completion.is_some()).map(|w| {
            w.notice.as_ref().or_else(|| w.input.entries.first()).map_or_else(
                || format!("{}-{}-completion", self.cfg.id, w.observed_at),
                |id| format!("{id}-completion"),
            )
        })
    }

    /// A nova sessão e o consumo do handoff tornam-se duráveis antes de session/prompt.
    pub(super) fn checkpoint_waiting_session(&mut self) -> Result<()> {
        let lane = self.input.as_ref().map(TurnInput::lane);
        let handoff = lane.as_ref().and_then(|lane| self.turn_handoff(lane));
        if let Some(input) = self.input.as_mut() {
            input.handoff = handoff;
        }
        if let Some(waiting) = self.waiting.as_mut() {
            if let Some(input) = self.input.as_ref() {
                waiting.input = input.clone();
            }
            waiting.fresh_session = false;
            self.save_waiting()?;
        }
        Ok(())
    }

    pub(super) fn wait_for_checkpoint(&mut self, error: &anyhow::Error) {
        tracing::warn!(%error, "checkpoint indisponível; mantendo o pedido em espera");
        if let Some(waiting) = self.waiting.as_mut() {
            waiting.phase = if waiting.attempt_count >= MAX_BLOCKS { Phase::Paused } else { Phase::Waiting };
            waiting.retry_at = waiting.retry_at.max(now_ms().saturating_add(1000));
            waiting.dirty = true;
        }
        self.save_waiting_logged();
        self.publish_waiting();
        self.end_paused_routine_wait();
    }

    /// Um tombstone que não pôde ser apagado não pode mascarar o próximo turno em voo.
    pub(super) fn clear_finished_before_turn(&self) -> Result<()> {
        if self.waiting.is_none()
            && let Some(value) = read_waiting_checkpoint(&self.hub.store, &self.cfg.id)?
            && value["phase"] == "finished"
        {
            self.hub.store.clear_waiting_inflight(&self.cfg.id)?;
        }
        Ok(())
    }

    fn publish_waiting(&mut self) {
        let Some(waiting) = &self.waiting else { return };
        let at = waiting.resume_at;
        let paused = waiting.phase == Phase::Paused;
        let (activity, kind) = if paused {
            ("Limite da assinatura · tentativa automática suspensa".into(), ActivityKind::SubscriptionPaused)
        } else {
            (waiting_activity(at), ActivityKind::SubscriptionWait)
        };
        self.hub.set_memory_runtime(&self.cfg.id, |r| {
            if !waiting.dirty {
                r.preparing_turn_token = None;
            }
            r.status = if paused { BotStatus::Error } else { BotStatus::Waiting };
            r.set_activity(activity, Some(kind));
            r.resume_at = at;
            r.started_at = None;
            r.lane = Some(self.lane.clone());
        });
        let routine = waiting.routine_id().and_then(|id| self.hub.routines.run_label(&self.cfg.id, id));
        let text = if paused {
            waiting.switch_error.clone().unwrap_or_else(|| {
                "Pausado após cinco retomadas sem conclusão. Use Tentar de novo para continuar.".into()
            })
        } else {
            at.map_or_else(
                || "Limite da assinatura atingido; volto assim que liberar.".to_owned(),
                |at| format!("Limite da assinatura atingido; volto às {}.", reset_label(at)),
            )
        };
        let text = match &routine {
            Some((_, name)) if !paused => at.map_or_else(
                || format!("Rotina «{name}» aguardando o limite da assinatura; volto assim que liberar."),
                |at| format!("Rotina «{name}» aguardando o limite da assinatura; volto às {}.", reset_label(at)),
            ),
            _ => text,
        };
        let mut data = json!({"text": text, "style": if paused { "error" } else { "info" }, "author": self.cfg.id,
            "subscriptionWait": {"resumeAt": at, "retry": paused}});
        if let Some((routine_id, _)) = &routine {
            // Não usar o campo `routineId` no topo: o desktop o roteia para o
            // botão "abrir a rotina" e esconderia os controles da espera (D72).
            data["routineWait"] = json!({ "routineId": routine_id, "runId": waiting.input.routine });
        }
        self.usage_notice(&mut data);
        if let Some(id) = waiting.notice.as_ref() {
            self.hub.set_entry(id, &data);
        } else if let Some(entry) = self.add(EntryKind::Notice, self.turn.unwrap_or(0), data) {
            if let Some(waiting) = self.waiting.as_mut() {
                waiting.notice = Some(entry.id);
            }
            self.save_waiting_logged();
        }
    }

    /// Executada antes do resume de crashes: não tem o limite de idade de uma hora.
    pub(super) fn restore_waiting(&mut self) -> bool {
        let value = match read_waiting_checkpoint(&self.hub.store, &self.cfg.id) {
            Ok(Some(value)) => value,
            Ok(None) => return false,
            Err(error) => {
                tracing::error!(%error, "não foi possível ler a espera persistida");
                return false;
            }
        };
        if value["phase"] == "finished" {
            if let Err(error) = self.hub.store.kv_delete(&key(&self.cfg.id)) {
                tracing::warn!(%error, "tombstone de espera ainda não pôde ser limpo");
            }
            return false;
        }
        let Ok(waiting) = serde_json::from_value::<WaitingTurn>(value) else { return false };
        let identity_error = match crate::fork::bot_admission::current(&self.hub.store, &self.cfg.id) {
            Ok(Some(record))
                if waiting.execution_token.as_deref() != Some(record.execution_token.as_str())
                    || waiting.memory_policy.is_none()
                    || waiting.memory_policy != record.memory_policy
                    || (record.routing.is_some() && waiting.routing != record.routing) =>
            {
                Some(anyhow::anyhow!("admission_checkpoint_execution_conflict"))
            }
            Err(error) => Some(error),
            _ => None,
        };
        let context_error = if waiting.input.context_binding.is_some() {
            self.restore_space_waiting_config(&waiting.input, waiting.phase == Phase::Completing).err()
        } else {
            None
        };
        let lane = waiting.input.lane();
        // D372: retoma na seleção ADMITIDA no despacho original, não na atual da conversa nem no padrão do bot;
        // ilegível, recibo ausente ou divergência pausam a espera (falha fechado).
        // Rotina e canal seguem o padrão do bot (domínio deles), sem a seleção da principal. Conclusão durável
        // (`Completing`) só publica o resultado salvo, sem executar a IA: não depende de a seleção atual ainda poder
        // executar o turno (inclusive checkpoint anterior à D372 cuja conversa foi migrada depois).
        let own_domain =
            waiting.input.routine.is_none() && !waiting.input.transient && waiting.phase != Phase::Completing;
        let context_error = context_error.or_else(|| {
            if !own_domain {
                return None;
            }
            crate::fork::sessoes::admissao::apply_resume(
                &self.hub.store.db.locked(),
                &lane,
                &mut self.cfg,
                waiting.execution_token.as_deref(),
                matches!(waiting.routing, Some(crate::fork::bot_admission::RoutingClass::Routed {})),
                (waiting.backend.as_str(), waiting.model.as_deref()),
            )
            .err()
        });
        self.lane = lane;
        self.turn = Some(
            waiting
                .input
                .entries
                .last()
                .and_then(|id| self.hub.store.entry(id))
                .map_or_else(|| self.hub.store.max_turn(&self.lane.chat), |e| e.turn),
        );
        self.active_routine.clone_from(&waiting.input.routine);
        if let Some(error) = identity_error {
            self.input = Some(waiting.input.clone());
            self.waiting = Some(waiting);
            self.pause_waiting_switch(&error);
            return true;
        }
        if let Some(error) = context_error {
            self.input = Some(waiting.input.clone());
            self.waiting = Some(waiting);
            self.pause_waiting_switch(&error);
            return true;
        }
        if waiting.phase == Phase::Completing {
            let Some(completion) = waiting.completion.as_ref() else { return false };
            self.active_routine.clone_from(&waiting.input.routine);
            self.finishing = Some(completion.done());
            self.turn = completion.turn;
            self.lane = waiting.input.lane();
            self.turn_session.clone_from(&waiting.input.session);
            self.input = Some(waiting.input.clone());
            self.waiting = Some(waiting);
            return true;
        }
        if !waiting.input.waitable(&self.cfg) {
            // Compatibilidade com checkpoints antigos: encerra localmente, sem session/prompt.
            self.input = Some(waiting.input.clone());
            self.waiting = Some(waiting);
            self.finish_turn(&Ok(json!({"stopReason":"subscription_limit_reached"})));
            return true;
        }
        if !waiting.fresh_session {
            let slot = self.slot();
            self.set_session(slot.as_deref(), waiting.input.session.as_deref());
        }
        self.input = Some(waiting.input.clone());
        self.waiting = Some(waiting);
        if self.waiting.as_ref().is_some_and(|w| w.input.routine.is_some())
            && let Some(run_id) = self.active_routine.clone()
            && let Err(error) = self.hub.routines.mark_waiting(&self.hub.store, &self.cfg.id, &run_id)
        {
            // Sem a transição a retomada é recusada; pausa encerra a run de forma coerente.
            self.pause_waiting_switch(&error.context("routine_wait_run_transition_failed"));
            return true;
        }
        if self
            .waiting
            .as_ref()
            .is_some_and(|w| w.execution_token.as_deref().is_none_or(str::is_empty) || w.memory_policy.is_none())
        {
            self.pause_waiting_switch(&anyhow::anyhow!("admission_legacy_execution_missing"));
            return true;
        }
        // D305: depois da migração de boot, espera sem classificação de roteamento pausa; nunca vira legado.
        if self.waiting.as_ref().is_some_and(|w| w.routing.is_none()) {
            self.pause_waiting_switch(&anyhow::anyhow!("admission_routing_missing"));
            return true;
        }
        // Se caiu durante uma tentativa, continua o pedido e avisa para não repetir efeitos externos.
        let uncertain_dispatch = crate::fork::bot_admission::current(&self.hub.store, &self.cfg.id)
            .is_ok_and(|record| record.is_some_and(|d| d.phase == crate::fork::bot_admission::Phase::Permitted));
        self.capture_usage_uncertainty(uncertain_dispatch);
        if let Some(waiting) = self.waiting.as_mut()
            && waiting.phase == Phase::Retrying
        {
            waiting.phase =
                if uncertain_dispatch || waiting.input.context_binding.is_some() || waiting.attempt_count >= MAX_BLOCKS
                {
                    Phase::Paused
                } else {
                    Phase::Waiting
                };
            if uncertain_dispatch || waiting.input.context_binding.is_some() {
                waiting.switch_error = Some("A retomada foi interrompida. Confira os efeitos antes de tentar continuar; nenhuma ação foi repetida automaticamente.".into());
            }
            self.save_waiting_logged();
        }
        if self.waiting.as_ref().is_some_and(|w| w.fresh_session) {
            self.forget_session();
        }
        self.restore_usage_control();
        self.publish_waiting();
        self.end_paused_routine_wait();
        if self.waiting_for_reset() && self.waiting.as_ref().is_some_and(|w| !w.dirty) && self.waiting_owns_dispatch() {
            self.finish_dispatch();
        }
        true
    }

    pub(super) async fn retry_waiting(&mut self, done_tx: &mpsc::UnboundedSender<super::PromptDone>) {
        // Sessões PR5: a extra foi encerrada (inclusive antes de reiniciar o Hub): a retomada não roda.
        if self.drop_closed_waiting() {
            return;
        }
        // MX-263: desativar, excluir ou cancelar a rotina encerra a espera dela.
        if self.cancel_stale_routine_wait() {
            return;
        }
        // MX-263: uma pausa de rotina (identidade, roteamento, teto de tentativas,
        // política) encerra a execução — não há dono para destravar a espera.
        if self.end_paused_routine_wait() {
            return;
        }
        self.reconcile_usage_notices();
        if self.sync_usage_control().await {
            self.end_paused_routine_wait();
            return;
        }
        // MX-145: a retomada parada pelo teto só volta quando ele liberar.
        if self.admission_blocked() || self.finishing.is_some() || self.stop_requested || self.ceiling_holds_waiting() {
            return;
        }
        if self.waiting.as_ref().is_some_and(|w| w.dirty) {
            if self.waiting.as_ref().is_some_and(|w| now_ms() < w.save_retry_at) {
                return;
            }
            self.save_waiting_logged();
            if self.waiting.as_ref().is_some_and(|w| w.dirty) {
                return;
            }
        }
        if self.dispatch.is_some() || !self.waiting_for_reset() {
            return;
        }
        if self.waiting.as_ref().is_some_and(|waiting| waiting.phase == Phase::Paused) {
            return;
        }
        if self.waiting.as_ref().is_some_and(|waiting| {
            crate::fork::assinatura_pessoal::claude(&waiting.backend)
                && (waiting.input.transient || waiting.input.routine.is_some() || waiting.input.hidden)
        }) {
            self.pause_waiting_switch(&crate::fork::assinatura_pessoal::Refusal::Automation.into());
            return;
        }
        let current_policy = crate::fork::spaces::current_memory_policy_on(&self.hub.store.db.locked(), &self.cfg);
        let identity_valid = self.waiting.as_ref().is_some_and(|w| {
            w.execution_token.as_deref().is_some_and(|token| !token.is_empty())
                && w.routing.is_some()
                && w.memory_policy.as_ref() == Some(&crate::fork::bot_admission::MemoryPolicy::of_config(&self.cfg))
                && current_policy.as_ref().is_ok_and(|policy| w.memory_policy.as_ref() == Some(policy))
        });
        if !identity_valid {
            self.pause_waiting_switch(&anyhow::anyhow!("admission_execution_or_memory_policy_changed"));
            return;
        }
        let now = now_ms();
        let Some(waiting) = self.waiting.as_mut() else { return };
        if now.saturating_sub(waiting.usage_checked_at) >= USAGE_CHECK_MS {
            waiting.usage_checked_at = now;
            let reset = {
                let usage = self.hub.usage.locked();
                usage
                    .providers
                    .iter()
                    .find(|p| p.id == waiting.backend)
                    .filter(|p| p.updated_at > waiting.observed_at)
                    .and_then(|_| subscription::usage_reset(&usage, &waiting.backend, waiting.model.as_deref(), now))
            };
            // Nunca antecipa o reset informado pelo erro do próprio turno.
            if reset.is_some() && reset > waiting.resume_at && waiting.selected(&self.cfg) {
                waiting.resume_at = reset;
                waiting.schedule(now);
                self.save_waiting_logged();
                self.publish_waiting();
            }
        }
        let Some(waiting) = self.waiting.as_ref() else { return };
        if !waiting.due(now) && waiting.selected(&self.cfg) {
            return;
        }
        if let Err(error) = self.revalidate_space_waiting() {
            self.pause_waiting_switch(&error);
            return;
        }
        if self.cfg.context.is_some() && self.waiting.as_ref().is_some_and(|w| !w.selected(&self.cfg)) {
            self.pause_waiting_switch(&anyhow::anyhow!("conversation_binding_changed"));
            return;
        }
        if self.waiting.as_ref().is_some_and(|w| !w.selected(&self.cfg))
            && let Err(error) = self.switch_waiting_model()
        {
            self.pause_waiting_switch(&error);
            return;
        }
        let Some(waiting) = self.waiting.as_mut() else { return };
        let before_retry = waiting.clone();
        waiting.begin_retry(now);
        let input = waiting.input.clone();
        let fresh = waiting.fresh_session;
        if let Err(error) = self.save_waiting() {
            self.waiting =
                Some(WaitingTurn { dirty: true, save_retry_at: now.saturating_add(USAGE_CHECK_MS), ..before_retry });
            tracing::error!(%error, "não foi possível salvar a tentativa de retomada");
            return;
        }
        if fresh {
            if let Some(slot) = self.slot() {
                self.set_session(Some(&slot), None);
            }
            self.forget_session();
        }
        // Claude pode deixar uma query encerrada após rate_limit: relança, carregando o mesmo ID.
        if let Some(conn) = self.conn.take() {
            conn.kill().await;
        }
        self.turn = None;
        // MX-263: o prazo da rotina não corre durante a espera; a nova tentativa
        // recebe o orçamento dela no `mark_started` (não o resto vencido da anterior).
        self.routine_deadline = None;
        self.input = Some(input.clone());
        self.handoff.clone_from(&input.handoff);
        self.handoff_chat = Some(input.lane());
        let text = format!("{CONTINUE}\n\n{}", input.text);
        if let Err(error) = self.start_turn(input.lane(), &input.entries, &text, input.hidden, done_tx).await {
            self.start_failed(&error);
        }
    }

    /// Só chamada para uma configuração já escolhida pelo usuário. Sem catálogo/fallback de API.
    pub(super) fn switch_waiting_model(&mut self) -> Result<()> {
        anyhow::ensure!(self.cfg.context.is_none(), "context_binding_is_immutable_during_wait");
        // A escolha de uma extra é fixada na admissão; a espera nunca a troca (PR4).
        anyhow::ensure!(self.lane.session.is_none(), "session_choice_is_immutable_during_wait");
        let Some(waiting) = self.waiting.as_ref() else { return Ok(()) };
        anyhow::ensure!(
            waiting.backend == self.cfg.backend
                || (!crate::fork::assinatura_pessoal::claude(&waiting.backend)
                    && !crate::fork::assinatura_pessoal::claude(&self.cfg.backend)),
            crate::fork::assinatura_pessoal::Refusal::Fallback
        );
        let (context, person) = crate::chat::handoff::build_in_with_person(
            &self.hub.store,
            &self.cfg.id,
            &self.cfg.id,
            self.turn_session.as_deref().or(self.session_id.as_deref()).unwrap_or_default(),
        )?;
        let mut waiting = waiting.clone();
        waiting.input.handoff = (person.is_some() && person == self.cfg.person).then_some(context);
        waiting.input.backend.clone_from(&self.cfg.backend);
        waiting.input.model.clone_from(&self.cfg.model);
        waiting.input.command.clone_from(&self.cfg.command);
        waiting.backend.clone_from(&self.cfg.backend);
        waiting.model.clone_from(&self.cfg.model);
        waiting.command.clone_from(&self.cfg.command);
        waiting.retry_at = now_ms();
        waiting.fresh_session = true;
        waiting.phase = Phase::Waiting;
        waiting.resume_at = None;
        waiting.attempt_count = 0;
        waiting.switch_error = None;
        // Não publica uma troca parcial se o checkpoint falhar.
        self.hub.store.kv_set(&key(&self.cfg.id), &serde_json::to_string(&waiting)?)?;
        waiting.dirty = false;
        self.waiting = Some(waiting);
        self.forget_session();
        Ok(())
    }

    pub(super) fn pause_waiting_switch(&mut self, error: &anyhow::Error) {
        tracing::error!(%error, "não foi possível preparar o handoff; aguardando ação do usuário");
        if let Some(waiting) = self.waiting.as_mut() {
            waiting.phase = Phase::Paused;
            waiting.switch_error = Some(if error.to_string().starts_with("admission_routing_") {
                "Esta espera não tem a classificação de roteamento do envio original. Confira os efeitos e envie um pedido novo; nada foi repetido nem reclassificado.".into()
            } else if error.to_string().starts_with("admission_") {
                "A identidade ou política de memória original não pôde ser comprovada. Confira os efeitos e envie um pedido novo; nenhuma ação ou identidade foi repetida/fabricada.".into()
            } else if waiting.input.context_binding.is_some() {
                "Não foi possível validar o contexto e o binding originais. Confira o estado e pare esta espera antes de escolher outra conta/modelo.".into()
            } else {
                "Não foi possível preparar a troca de modelo. Tente de novo para preservar o pedido e seu contexto."
                    .into()
            });
        }
        self.save_waiting_logged();
        self.publish_waiting();
        self.end_paused_routine_wait();
    }

    /// MX-263: a pausa da espera de rotina não tem dono para destravar — é trabalho
    /// autônomo — e seguraria o slot do bot até intervenção (no D72 da conversa o
    /// dono decide). Ela encerra a execução como falha em vez de esperar para sempre.
    fn end_paused_routine_wait(&mut self) -> bool {
        if !self.waiting.as_ref().is_some_and(|w| w.phase == Phase::Paused && w.input.routine.is_some()) {
            return false;
        }
        // `finish_turn` precisa do turno da tentativa; se ele não sobrou, aluga
        // um só para encerrar a espera e a run de forma durável. O encerramento
        // é terminal: uma run catch-up não pode ser reenfileirada pela pausa.
        self.routine_terminal = true;
        self.turn = self.turn.or_else(|| Some(self.hub.store.max_turn(&self.lane.chat)));
        self.finish_turn(&Err(anyhow::anyhow!("subscription_wait_paused")));
        true
    }

    pub(super) fn retry_subscription(&mut self, entry_id: &str) {
        if self.waiting.as_ref().is_some_and(|w| {
            crate::fork::usage_limits::cancellation(&self.hub.store, &w.usage_control.id).map_or(true, |c| c.is_some())
        }) {
            return;
        }
        if let Some(waiting) = self.waiting.as_mut()
            && waiting.phase == Phase::Paused
            && waiting.notice.as_deref() == Some(entry_id)
        {
            waiting.usage_control.manual = true;
            waiting.usage_control.policy_paused = false;
            waiting.phase = Phase::Waiting;
            waiting.retry_at = now_ms();
            waiting.attempt_count = 0;
            waiting.switch_error = None;
            self.save_waiting_logged();
            self.publish_waiting();
        }
    }

    /// MX-263: a espera de uma rotina morre com ela — desativada/excluída, ou a
    /// execução cancelada, a espera encerra pelo caminho durável do Parar (a run
    /// sai como parada, o tombstone impede replay e a vaga do bot é devolvida).
    /// A tentativa em voo (`Retrying`) segue até o fim do turno, como no Parar.
    fn cancel_stale_routine_wait(&mut self) -> bool {
        let stale = self.waiting.as_ref().is_some_and(|waiting| {
            matches!(waiting.phase, Phase::Waiting | Phase::Paused)
                && waiting.routine_id().is_some_and(|id| !self.hub.routines.waiting_run_live(&self.cfg.id, id))
        });
        if !stale {
            return false;
        }
        // `finish_turn` precisa do turno; uma tentativa abortada pode tê-lo zerado.
        self.turn = self.turn.or_else(|| Some(self.hub.store.max_turn(&self.lane.chat)));
        self.stop_requested = true;
        self.finish_turn(&Ok(json!({"stopReason":"cancelled"})));
        true
    }

    /// Um tombstone durável impede replay mesmo se a exclusão falhar.
    pub(super) fn clear_waiting(&mut self) -> bool {
        if self.waiting.is_some() {
            if let Some(waiting) = self.waiting.as_mut() {
                if let Some(id) = waiting.notice.as_ref()
                    && let Some(mut entry) = self.hub.store.entry(id)
                {
                    entry.data["subscriptionWait"]["retry"] = json!(false);
                    if let Some(wait) = entry.data["subscriptionWait"].as_object_mut() {
                        wait.remove("waitId");
                    }
                    self.hub.set_entry(id, &entry.data);
                }
                waiting.phase = Phase::Finished;
            }
            let tombstone = self.save_waiting();
            let deleted = self.hub.store.clear_waiting_inflight(&self.cfg.id);
            if tombstone.is_err() && deleted.is_err() {
                tracing::error!(
                    "não foi possível concluir a espera no SQLite; conclusão será repetida no próximo tick"
                );
                return false;
            }
        }
        // Uma troca cancelada/falha não deixa o ID do motor anterior no novo backend.
        if self.waiting.as_ref().is_some_and(|waiting| !waiting.selected(&self.cfg)) {
            self.forget_session();
        }
        self.waiting = None;
        self.input = None;
        self.turn_limit = None;
        self.finishing = None;
        true
    }
}

fn reset_label(at: i64) -> String {
    chrono::DateTime::from_timestamp_millis(at).map_or_else(
        || "assim que liberar".into(),
        |at| at.with_timezone(&chrono::Local).format("%H:%M (%Z)").to_string(),
    )
}

pub(super) fn waiting_activity(at: Option<i64>) -> String {
    at.map_or_else(
        || "Aguardando · assim que liberar".into(),
        |at| format!("Aguardando · retomada às {}", reset_label(at)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reset_e_sondagem_controlam_quando_o_pedido_pode_retomar() {
        let input = TurnInput {
            chat: "b".into(),
            entries: vec!["u".into()],
            text: "pedido".into(),
            hidden: false,
            handoff: None,
            routine: None,
            transient: false,
            backend: "codex".into(),
            model: None,
            command: None,
            session: None,
            context_binding: None,
            visible_session: None,
        };
        let mut w = WaitingTurn {
            execution_token: None,
            usage_control: UsageControl::default(),
            memory_policy: None,
            routing: None,
            input,
            phase: Phase::Waiting,
            resume_at: None,
            retry_at: 0,
            backend: "codex".into(),
            model: None,
            command: None,
            notice: None,
            fresh_session: false,
            completion: None,
            observed_at: 0,
            attempt_count: 0,
            switch_error: None,
            usage_checked_at: 0,
            save_retry_at: 0,
            dirty: false,
        };
        w.block(Some(1_800_000_100_000), 1_800_000_000_000);
        assert!(!w.due(1_800_000_099_999));
        assert!(w.due(1_800_000_300_000));
        w.phase = Phase::Retrying;
        assert!(!w.due(i64::MAX), "uma tentativa em voo não pode ser duplicada");
        w.attempt_count = 0;
        w.block(None, 1_800_000_200_000);
        assert!(!w.due(1_800_000_499_999));
        assert!(w.due(1_800_000_500_000));
        for delay in [10, 20, 40, 60] {
            w.begin_retry(0);
            w.block(None, 0);
            assert_eq!(w.retry_at, delay * 60 * 1000);
            assert!(w.due(w.retry_at));
        }
        w.begin_retry(0);
        w.block(None, 0);
        assert!(w.phase == Phase::Paused);
        assert!(!w.due(i64::MAX), "backoff tem teto de cinco retomadas automáticas");
        let restored: WaitingTurn = serde_json::from_str(&serde_json::to_string(&w).unwrap()).unwrap();
        assert!(restored.phase == Phase::Paused && restored.attempt_count == MAX_BLOCKS);
    }
}
