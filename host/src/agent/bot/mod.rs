//! One actor per bot. It owns the bot's ACP process and session, runs turns one
//! at a time, and maps ACP `session/update`s onto transcript entries.
//!
//! Chat model (from Grok Bot): the chat only shows user messages, the *final*
//! agent message of each turn, permission cards and notices. Narration between
//! tool calls, thoughts, tool calls and plans are trace entries shown in the
//! "full conversation" sheet, and surface live as the roster activity line.
//!
//! Context (from Grok Bot too): the bot's instructions and memory are a frozen
//! snapshot per session and compaction epoch (`context`), finished turns feed
//! its dreaming (`memory`), messages sent while it works are folded into
//! its next turn, and a turn cut off by a host restart is resumed.
//!
//! Lanes: besides its own chat, a bot talks in group chats (turns in a session of its own for
//! each group, written to the group's transcript; see `group`). A routine run talks in its chat
//! from a session of its own (see `session`).

#[cfg(test)]
mod approval_origin_tests;
mod compaction;
mod fast_mode;
mod files;
mod fork_admission;
#[path = "../../fork/compressao/actor.rs"]
mod fork_compressao;
#[path = "../../fork/contexto/actor.rs"]
mod fork_contexto;
#[path = "../../fork/custo/actor.rs"]
mod fork_custo;
mod fork_fallback;
mod fork_fila;
mod fork_genui;
mod fork_owner_question;
mod fork_profile_skills;
mod fork_queue;
mod fork_session_delete;
mod fork_sessoes;
mod fork_sobre_voce;
mod fork_space_delete;
#[path = "../../fork/teto/ator.rs"]
mod fork_teto;
mod fork_troca_motor;
mod launch;
#[cfg(test)]
pub(crate) use launch::claude_meta;
mod lifecycle;
mod queue;
#[path = "../../fork/rotina_resultado/ator.rs"]
mod rotina_resultado;
mod session;
mod space_control;
mod turn;
mod updates;
mod voice_approval;
mod waiting;

pub(crate) use launch::{
    launch_commands, select_model, start_agent_protected, start_background_agent, system_prompt,
    verify_codex_background_mode,
};
pub(crate) use voice_approval::allowed_answer as voice_answer_allowed;
pub(crate) use waiting::{checkpoint_waiting_active, read_waiting_checkpoint};

use crate::agent::acp::{self, Acp, Incoming};
use crate::agent::config::FastModeSupport;
use crate::chat::context::Snapshot;
use crate::chat::group::GroupTurn;
use crate::chat::memory;
use crate::hub::{BotStatus, Hub};
use crate::store::{BotConfig, Entry, EntryKind, Lane};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// How a notice renders in the chat (wire values: `info` / `error`).
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NoticeStyle {
    Info,
    Error,
}

pub enum Cmd {
    ForkContext {
        request: crate::fork::contexto::Request,
        reply: tokio::sync::oneshot::Sender<Result<Value>>,
    },
    ContextTeam {
        context: Box<crate::fork::spaces::tool_context::ToolContext>,
        name: String,
        args: Value,
        reply: tokio::sync::oneshot::Sender<Result<crate::chat::team::Admission>>,
    },
    ContextTool {
        context: Box<crate::fork::spaces::tool_context::ToolContext>,
        command: Box<Cmd>,
    },
    RefreshTools,
    Routine(String),
    /// The user wrote in the bot's chat.
    Send {
        lane: Lane,
        entry_id: String,
        text: String,
    },
    BotRequest(crate::chat::team::BotRequest),
    /// `send_message` from the bot's `chat` MCP server: a message for the user, now.
    SendToUser {
        execution: Option<crate::fork::sessoes::ExecutionRef>,
        text: String,
        reply: tokio::sync::oneshot::Sender<Result<()>>,
    },
    /// G1b: `mostrar_componente` (escolha v1). Só chega dentro de `ContextTool`, da conversa capturada.
    ShowChoice {
        invocation: String,
        payload: Box<crate::fork::genui::contract::ValidatedChoice>,
        reply: tokio::sync::oneshot::Sender<Result<Value>>,
    },
    /// I-023: `report_routine_result` do servidor `routines`, com o vínculo original capturado em E1.
    RoutineResult {
        execution: crate::fork::sessoes::ExecutionRef,
        declaration: crate::fork::rotina_resultado::Declaration,
        reply: tokio::sync::oneshot::Sender<Result<()>>,
    },
    /// MB-1b: `ask_owner` do servidor `chat`: um cartão de pergunta ao dono na conversa do turno.
    AskOwner {
        execution: Option<crate::fork::sessoes::ExecutionRef>,
        text: String,
        reply: tokio::sync::oneshot::Sender<Result<()>>,
    },
    /// `react` from the bot's `chat` MCP server: an emoji on the message the turn answers.
    React {
        execution: Option<crate::fork::sessoes::ExecutionRef>,
        emoji: String,
        /// In a room: whose message (see `chat::reactions::in_room`).
        to: Option<String>,
        reply: tokio::sync::oneshot::Sender<Result<()>>,
    },
    SendFile {
        execution: Option<crate::fork::sessoes::ExecutionRef>,
        path: String,
        name: Option<String>,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
        reply: tokio::sync::oneshot::Sender<Result<()>>,
    },
    CancelAsk {
        id: String,
    },
    /// Its turn in a group's room turn.
    Group(GroupTurn),
    /// The group's room turns stopped: drop its queued turns, stop a running one.
    CancelGroup {
        chat: String,
    },
    #[cfg(test)]
    Stop,
    UsageWaitChanged {
        wait_id: String,
    },
    UsageWaitResume {
        target: crate::fork::usage_limits::Target,
        reply: tokio::sync::oneshot::Sender<Result<Value>>,
        /// D368: a origem do pedido do dono, reconferida pelo ator antes de retomar.
        origem: Option<crate::fork::donna_fixa::pedido::Origem>,
    },
    /// MX-145 E3: uma liberação do dono foi gravada; reavalia o envio segurado.
    CeilingChanged,
    RetrySubscription {
        entry_id: String,
    },
    ContextRetry {
        chat: String,
        revision: i64,
        entry_id: String,
        reply: tokio::sync::oneshot::Sender<Result<Value>>,
    },
    Permission {
        entry_id: String,
        option_id: Option<String>,
        reply: tokio::sync::oneshot::Sender<Result<()>>,
        /// CF-14: a origem ORIGINAL de quem respondeu, reconferida na Tx do cartão; nunca recapturada.
        origem: Option<crate::fork::donna_fixa::pedido::Origem>,
    },
    /// A Codync tool asks the user before it acts (`chat::manage`), on the agent's kind of approval card.
    Approve {
        execution: Option<crate::fork::sessoes::ExecutionRef>,
        title: String,
        detail: String,
        reply: tokio::sync::oneshot::Sender<bool>,
    },
    /// `newSession` legado na principal (Sessões PR5: recomeça só a principal). A autoridade ORIGINAL do pedido vai
    /// explícita, obrigatória; o ator grava com ela na Tx e nunca recaptura nem supõe Local.
    PrimaryNewSession {
        chat: String,
        revision: i64,
        autoridade: Box<crate::fork::autoridade::Autoridade>,
        reply: tokio::sync::oneshot::Sender<Result<Value>>,
    },
    /// Sessões PR5: `sessionClose` de uma extra deste bot, com a autoridade ORIGINAL capturada na RPC.
    SessionClose {
        session: String,
        expected: i64,
        autoridade: Box<crate::fork::autoridade::Autoridade>,
        reply: tokio::sync::oneshot::Sender<Result<Value>>,
    },
    /// Reserva reversível: o ator confere trabalho em memória e fica parado até
    /// o chamador concluir/abortar a Tx. Não cancela trabalho nem altera filas.
    SpaceDeleteGuard {
        chats: Vec<String>,
        all: bool,
        reply: tokio::sync::oneshot::Sender<bool>,
        release: tokio::sync::oneshot::Receiver<()>,
    },
    SessionDelete {
        request: Box<crate::fork::sessoes::excluir::Request>,
        autoridade: Box<crate::fork::autoridade::Autoridade>,
        reply: tokio::sync::oneshot::Sender<Result<Value>>,
    },
    /// Sessões PR5 (P4): candidata da varredura do Hub; o ator confere o próprio trabalho e a Tx reconfere tudo.
    SessionIdleClose {
        session: String,
    },
    ContextControl {
        chat: String,
        revision: i64,
        entry_id: Option<String>,
        reset: bool,
        reply: tokio::sync::oneshot::Sender<Result<Value>>,
    },
    /// DON-907 (D431): remover, editar, mover ou mandar agora uma mensagem da fila, com a autoridade ORIGINAL.
    QueuedControl(Box<fork_fila::Pedido>),
    Reconfigure(Box<BotConfig>),
    Shutdown,
}

enum Queued {
    Routine(String),
    /// Entrada durável ainda sem contexto autorizado para reconstruir o prompt.
    RestoreUser {
        entry_id: String,
        retry_at: Option<Instant>,
    },
    User {
        lane: Lane,
        entry_id: String,
        text: String,
    },
    BotRequest(crate::chat::team::BotRequest),
    /// A compaction the host decided on (see `compaction`); `idle` while the user is away.
    Compact {
        idle: bool,
        manual: Option<crate::fork::contexto::Manual>,
    },
    Group(GroupTurn),
}

pub struct BotHandle {
    pub tx: mpsc::UnboundedSender<Cmd>,
    /// The actor task; it kills its agent process on the way out.
    pub task: tokio::task::JoinHandle<()>,
}

pub(crate) use fork_fila::rpc as queued_rpc;

pub fn spawn(hub: Arc<Hub>, cfg: BotConfig) -> BotHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    // A turn the previous host process never finished (crash, update, restart), unless
    // it's stale: an hour-old task is more likely unwanted than awaited (gawkbot's rule).
    let interrupted = hub.store.kv_get(&inflight_key(&cfg.id)).and_then(|v| crate::fork::spaces::inflight(&v, &cfg.id));
    let actor = new_actor(hub, cfg);
    let task = tokio::spawn(actor.run(rx, interrupted));
    BotHandle { tx, task }
}

fn new_actor(hub: Arc<Hub>, cfg: BotConfig) -> Actor {
    let session_id = hub.store.bot(&cfg.id).ok().flatten().and_then(|b| b.session_id);
    let dreamer = memory::spawn_dreamer(hub.clone(), cfg.id.clone());
    let lane = Lane::main(&cfg.id);
    let fast_support = FastModeSupport::from_stored(hub.store.kv_get(&fast_mode::stored_key(&cfg.id)).as_deref());
    let support = fast_support.clone();
    hub.set_runtime(&cfg.id, |r| r.fast_mode = support);
    Actor {
        hub,
        cfg,
        conn: None,
        account_revision: None,
        session_id,
        run_sessions: HashMap::new(),
        lane,
        turn_session: None,
        turn_binding: None,
        cost_turn: None,
        active_group: None,
        active_routine: None,
        routine_attempt: None,
        routine_deadline: None,
        routine_timed_out: false,
        routine_terminal: false,
        routine_completion: None,
        session_fresh: false,
        profile_resend: HashSet::new(),
        compacting: None,
        context_compaction: None,
        raw_prompt: false,
        handoff: None,
        handoff_chat: None,
        handoff_card: None,
        session_card: None,
        compaction_notice: None,
        dreamer,
        turn_text: None,
        turn_entries: Vec::new(),
        turn_memory_revision: String::new(),
        announce: None,
        turn: None,
        input: None,
        waiting: None,
        turn_limit: None,
        fallback: fork_fallback::Turno::default(),
        finishing: None,
        queue: VecDeque::new(),
        queue_retry_at: None,
        queue_restore_retry_at: Some(Instant::now()),
        #[cfg(test)]
        before_permit: None,
        active_request: None,
        dispatch: None,
        profile_skills: None,
        dispatch_finishing: None,
        pending_admission: None,
        setup_reason: None,
        ceiling_holds: VecDeque::new(),
        ceiling_next_check: None,
        ceiling_probe: None,
        ceiling_unrestored: None,
        restart_admission: true,
        seg: Seg::None,
        tools: HashMap::new(),
        hub_tool_servers: HashMap::new(),
        plan_entry: None,
        last_text: None,
        sent: Vec::new(),
        reacted: Reacted::No,
        files: files::FileShares::default(),
        perms: HashMap::new(),
        stop_requested: false,
        cancel_queued: Vec::new(),
        send_now: Vec::new(),
        cancel_deadline: None,
        exit_tail: None,
        tools_changed: false,
        restart_after_turn: false,
        fast_support,
        idle_checked: Instant::now(),
    }
}

#[cfg(test)]
#[path = "../../fork/conectores/actor_fixture.rs"]
pub(crate) mod conectores_fixture;

/// The working line for a tool call, with its stable code: composing a message reads as typing.
fn activity(title: &str) -> (&str, crate::fork::activity_code::ActivityKind) {
    use crate::fork::activity_code::ActivityKind;
    if title.contains("send_message") {
        ("Typing…", ActivityKind::Typing)
    } else if ["memory__", "memory.", "bot-memory", "shared-memory"].iter().any(|m| title.contains(m)) {
        // Memory stays out of the chat, its working line and code included: the memory settings show it.
        ("Thinking…", ActivityKind::Thinking)
    } else {
        (title, ActivityKind::Tool { title: title.to_owned() })
    }
}

/// What the agent can do with sessions besides creating them.
// Each flag is one capability the agent advertised at `initialize`, independent of the others.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy)]
struct SessionCaps {
    /// `session/load`: resume a session after a restart.
    load: bool,
    /// `session/close`: an idle session can be released while the agent keeps running.
    close: bool,
    /// ACP `systemPrompt`: instructions on `session/new` and `load` that the agent keeps at system level.
    system_prompt: bool,
}

/// How the bot's instructions (identity and memory) reach the agent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Instructions {
    /// Claude: manual estável em `_meta.systemPrompt`; perfil/memória no envelope dinâmico.
    ClaudeMeta,
    /// Codex: identidade estável via `CODEX_CONFIG`; memória e audiência ainda seguem no envelope de usuário.
    CodexConfig,
    /// ACP `systemPrompt` (append), which the agent keeps across compaction.
    SystemPrompt,
    /// A `<bot-profile>` block before the first message, sent again after a compaction.
    FirstMessage,
}

/// Which adapter answered `initialize`, where Codync treats it apart from the others.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Adapter {
    /// claude-agent-acp: takes `_meta.systemPrompt` and `_meta.claudeCode.options`, and `/compact <instructions>`.
    Claude,
    /// codex-acp: `/compact` (its keep-list is in `CODEX_CONFIG`).
    Codex,
    /// Motor Mexerica (adaptador em processo do Hub): a identidade da ferramenta vem de
    /// `_meta.mexericaTool`, escrito pelo Hub a partir dos servidores MCP que ELE abriu.
    Mexerica,
    Other,
}

/// What the agent reported about a session's context while its own compaction ran: Codex measures the summarized
/// conversation before `completed` and not after (2.7.0: the previous measure again, the summary request, then the
/// summary). ACP carries no turn on `usage_update`, so a repeat of the measure accepted before the compaction
/// (`before`) is the old epoch's and is skipped; the newest other sample is the summary.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum CompactionUsage {
    #[default]
    Idle,
    Running {
        session: String,
        compaction: String,
        before: Option<u64>,
        latest: Option<(u64, u64)>,
    },
}

pub(crate) struct Conn {
    ponte: Option<crate::fork::ponte_credencial::Revogador>,
    curated: crate::fork::conectores::Pool,
    pub(crate) acp: Arc<Acp>,
    pub(crate) rx: mpsc::UnboundedReceiver<Incoming>,
    skill_reads: tokio::task::JoinSet<()>,
    sessions: SessionCaps,
    pub(crate) adapter: Adapter,
    /// `agentInfo.version` do handshake: o canal de identidade Codex só vale nas versões verificadas.
    pub(crate) adapter_version: Option<String>,
    /// Whether `CODEX_CONFIG` (Codex's manual channel) reached the agent at spawn.
    codex_config: bool,
    /// Avisos sobre arquivos persistentes, publicados no chat ao iniciar o processo.
    pub(crate) config_warnings: Vec<super::codex_audit::Warning>,
    /// Sessions already created or loaded in this process, and when each was last used.
    loaded: HashMap<String, Instant>,
    started: Instant,
    /// Each live session's Fast mode option, as the agent last listed it.
    fast_options: HashMap<String, Value>,
    /// The agent reports compactions (`compaction_update`); until it does, a fall in usage stands in for them.
    reports_compaction: bool,
    /// Usage reported while the agent's own compaction runs; it belongs to the epoch that compaction opens.
    compaction_usage: CompactionUsage,
    /// Each session's context use and window size at its last `usage_update`.
    usage: HashMap<String, (u64, u64)>,
}

impl Conn {
    pub(crate) fn instructions(&self) -> Instructions {
        // Codex só recebe o manual por CODEX_CONFIG: sem ela, o manual segue inline como nos outros motores.
        let adapter = if self.adapter == Adapter::Codex && !self.codex_config { Adapter::Other } else { self.adapter };
        Instructions::for_adapter(adapter, self.sessions.system_prompt)
    }
}

impl Instructions {
    fn for_adapter(adapter: Adapter, system_prompt: bool) -> Self {
        // Claude's adapter rebuilds the standard field without the `_meta` object's options, so Claude stays on it.
        if adapter == Adapter::Claude {
            Instructions::ClaudeMeta
        } else if adapter == Adapter::Codex {
            Instructions::CodexConfig
        } else if system_prompt {
            Instructions::SystemPrompt
        } else {
            Instructions::FirstMessage
        }
    }

    fn inline_context(self) -> bool {
        matches!(self, Self::ClaudeMeta | Self::CodexConfig | Self::FirstMessage)
    }

    pub(crate) fn native_manual(self) -> bool {
        matches!(self, Self::ClaudeMeta | Self::CodexConfig)
    }
}

#[cfg(test)]
mod harness_tests {
    use super::*;

    #[test]
    fn claude_e_codex_usam_canais_nativos_sem_depender_do_perfil_de_usuario() {
        for advertised in [false, true] {
            assert_eq!(Instructions::for_adapter(Adapter::Claude, advertised), Instructions::ClaudeMeta);
            assert_eq!(Instructions::for_adapter(Adapter::Codex, advertised), Instructions::CodexConfig);
        }
        assert_eq!(Instructions::for_adapter(Adapter::Other, true), Instructions::SystemPrompt);
        assert!(Instructions::for_adapter(Adapter::Other, false).inline_context());
    }
}

/// What came of the bot's `react` on the user's message this turn.
#[derive(Clone, Default, Serialize, Deserialize)]
enum Reacted {
    #[default]
    No,
    /// The emoji is on the message.
    With(String),
    /// Refused to keep reactions rare: the bot meant to answer with one, so it says nothing.
    Refused,
}

#[derive(Clone, Copy, PartialEq)]
enum SegKind {
    Text,
    Thought,
}

enum Seg {
    None,
    Open { kind: SegKind, entry_id: String, buf: String, flushed: Instant, dirty: bool },
}

// These flags describe independent actor concerns, not interchangeable states.
#[allow(clippy::struct_excessive_bools)]
struct Actor {
    #[cfg(test)]
    before_permit: Option<BeforePermit>,
    hub: Arc<Hub>,
    cfg: BotConfig,
    conn: Option<Conn>,
    account_revision: Option<u64>,
    /// The main session: the bot's own chat and its group turns.
    session_id: Option<String>,
    /// Routine run slot -> the run's session (loaded from kv on first use).
    run_sessions: HashMap<String, String>,
    /// Where the current (or last) turn talks; entries go there.
    lane: Lane,
    /// The session the current turn prompts.
    turn_session: Option<String>,
    /// Vínculo de sessão original (#237) do produtor canônico do setup (`ensure_session`).
    /// Atravessa os awaits até o permit, onde o encaixe do PR4 o recebe; nunca é recapturado.
    turn_binding: Option<crate::fork::sessoes::bindings::BindingRef>,
    cost_turn: Option<crate::fork::custo::Pending>,
    active_group: Option<GroupTurn>,
    active_routine: Option<String>,
    /// Recibo canônico original, persistido em Run.chain; nunca uma nova tentativa de retry SQL.
    routine_attempt: Option<crate::fork::cadeias::modelo::Recibo>,
    routine_deadline: Option<Instant>,
    routine_timed_out: bool,
    /// MX-263: a espera pausada encerra a run de forma terminal — sem reenfileirar
    /// catch-up (MX-110). Consumido e zerado pelo `finish_turn` da rotina.
    routine_terminal: bool,
    routine_completion: Option<RoutineCompletion>,
    /// True until the first prompt of a new ACP session has been sent.
    session_fresh: bool,
    /// Sessions that compacted away the `<bot-profile>` their first message carried; their next prompt carries
    /// a fresh one.
    profile_resend: HashSet<String>,
    /// The running turn is a compaction the host started (`Some(true)` while the user is away).
    compacting: Option<bool>,
    context_compaction: Option<crate::fork::contexto::Target>,
    /// The next prompt goes as it is: a command such as `/compact`, with nothing put before or after it.
    raw_prompt: bool,
    /// What the next prompt carries from the conversation it starts over from.
    handoff: Option<String>,
    /// The chat whose conversation `handoff` was taken for: only a turn of that conversation carries it, never a group
    /// turn, a request or a routine run that comes first (D229-6).
    /// A conversa e a sessão visível para as quais o resumo pendente foi tirado (PR4: nunca outra sessão).
    handoff_chat: Option<Lane>,
    /// DON-907 C9a × D372: o ID exato do cartão que o último `take_handoff` enviou (a troca promovida o marca).
    handoff_card: Option<String>,
    /// Sessões PR5: o cartão próprio de uma extra tirado para a mensagem dela, consumido só na Tx do permit.
    session_card: Option<fork_sessoes::SessionCard>,
    /// The chat line of the last compaction, which a late summary fills in.
    compaction_notice: Option<String>,
    dreamer: mpsc::UnboundedSender<memory::DreamEvent>,
    /// The user's words for this turn (None for a hidden turn or one not from the user); feeds memory.
    turn_text: Option<String>,
    /// The user messages this turn answers (none when it isn't the user's).
    turn_entries: Vec<String>,
    /// The memory change this turn's notice told the agent about (see `memory::change_notice`).
    turn_memory_revision: String,
    /// A profile update this turn carried, recorded once the agent got it.
    announce: Option<(Snapshot, String)>,
    turn: Option<i64>,
    input: Option<waiting::TurnInput>,
    waiting: Option<waiting::WaitingTurn>,
    turn_limit: Option<(crate::agent::subscription::Limit, i64)>,
    /// F6-B: o que o fallback entre contas decidiu para este turno.
    fallback: fork_fallback::Turno,
    finishing: Option<Done>,
    queue: VecDeque<Queued>,
    /// Falha de persistência da fila não cria uma espera de assinatura.
    queue_retry_at: Option<Instant>,
    /// Consumidor da enumeração inicial, mesmo antes de existir item na fila.
    queue_restore_retry_at: Option<Instant>,
    active_request: Option<crate::chat::team::BotRequest>,
    profile_skills: Option<crate::fork::bot_skills::Prepared>,
    dispatch: Option<crate::fork::bot_admission::Ticket>,
    dispatch_finishing: Option<crate::fork::bot_admission::Ticket>,
    pending_admission: Option<fork_admission::Pending>,
    /// Motivo (código fechado) da última falha de preparo humano; só em memória, um restart vale `setup_failed`.
    setup_reason: Option<fork_admission::Reason>,
    /// MX-145: trabalho travado pelo teto, fora da admissão e da fila; espelho das
    /// linhas duráveis de `donna_ceiling_holds` (uma por trabalho), fonte do restore.
    ceiling_holds: fork_teto::CeilingHolds,
    /// Próxima reavaliação da espera (sonda de geração, não agregação).
    ceiling_next_check: Option<Instant>,
    /// Cache da última sonda (geração do ledger+config+liberações) e quando
    /// a avaliação completa rodou — sonda igual e fresca não reagrega.
    ceiling_probe: Option<(String, Instant)>,
    /// MX-145: as esperas duráveis não puderam ser lidas no boot. Fail-closed: o
    /// ator não admite trabalho (nem re-drive) até a restauração valer.
    ceiling_unrestored: Option<fork_teto::PendingRestore>,
    restart_admission: bool,
    seg: Seg,
    tools: HashMap<String, String>,
    /// Identidades internas verificadas nas definições entregues a cada sessão ACP.
    hub_tool_servers: HashMap<String, HashSet<String>>,
    plan_entry: Option<String>,
    last_text: Option<String>,
    /// Messages the bot sent the user this turn (`send_message`); none: its last text is the reply.
    sent: Vec<String>,
    /// The bot's `react` this turn; with no message sent, it is the reply.
    reacted: Reacted,
    files: files::FileShares,
    /// permission entry id -> who waits for the answer
    perms: HashMap<String, updates::Pending>,
    stop_requested: bool,
    /// IDs enfileirados no momento do Parar; mensagens posteriores continuam válidas.
    cancel_queued: Vec<String>,
    /// DON-907: as mensagens marcadas por "Mandar agora"; cada uma sai sozinha no seu lote.
    send_now: Vec<String>,
    /// A cancelled turn still running past this stops its agent process.
    cancel_deadline: Option<Instant>,
    /// Last stderr lines of an agent process that just exited.
    exit_tail: Option<String>,
    /// Connectors changed: restart the agent before the next turn so the
    /// session is resumed with the new MCP servers.
    tools_changed: bool,
    /// A change that restarts the agent (`restart_agent`) came during a turn: it restarts once the turn ends.
    restart_after_turn: bool,
    /// What the agent says about Fast mode; clients see it in the bot's runtime.
    fast_support: FastModeSupport,
    /// When idle sessions and agents were last looked for.
    idle_checked: Instant,
}

#[cfg(test)]
type BeforePermit = Box<dyn FnOnce(&Hub) + Send + Sync>;

struct RoutineCompletion {
    id: String,
    /// I-023: o que o Hub observou; `Status` e desfecho saem dele e da declaração gravada.
    observed: crate::fork::rotina_resultado::Observed,
    detail: Option<String>,
    text: Option<String>,
}

type Done = Result<Value>;

/// Resposta ACP é vinculada à mesma execução Hub e tentativa admitida.
struct PromptDone {
    ticket: crate::fork::bot_admission::Ticket,
    result: Done,
}

impl Actor {
    /// `interrupted`: legado sem D72 é reconciliado localmente, sem novo prompt.
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Cmd>, interrupted: Option<Lane>) {
        let (done_tx, mut done_rx) = mpsc::unbounded_channel::<PromptDone>();
        let mut flush_tick = tokio::time::interval(Duration::from_millis(300));
        if let Err(error) = self.restore_dispatch() {
            tracing::error!(%error, "recuperação de despacho bloqueada; evidência preservada");
        }
        if let Err(error) = self.restore_ceiling_hold() {
            tracing::error!(%error, "espera do teto não restaurada; ator fechado até reler");
            self.ceiling_unrestored = Some(fork_teto::PendingRestore::default());
        }
        if self.restore_waiting() {
            self.retry_waiting(&done_tx).await;
        } else if let Some(run) = self.hub.routines.recovery(&self.cfg.id) {
            self.active_routine = Some(run.id.clone());
            self.routine_deadline = Some(Instant::now() + self.hub.routines.timeout(&run.id));
            self.recover_routine().await;
        } else if let Some(lane) = interrupted {
            // Sem as esperas lidas não dá para saber se a recuperação já está parada pelo teto.
            if let Some(pending) = self.ceiling_unrestored.as_mut() {
                pending.interrupted = Some(lane);
            } else if !self.ceiling_holds_recovery(&lane) {
                self.resume_interrupted(lane);
            }
        }
        if let Err(error) = self.restore_waiting_queue() {
            tracing::error!(%error, "não foi possível restaurar a fila da assinatura");
        }
        loop {
            tokio::select! {
                cmd = rx.recv() => {
                    let Some(cmd) = cmd else { break };
                    if !self.on_cmd(cmd, &done_tx).await { break }
                }
                Some(file) = self.files.rx.recv() => self.finish_file(file),
                inc = recv_incoming(&mut self.conn) => self.on_incoming(inc).await,
                Some(completion) = done_rx.recv() => {
                    if !self.accepts_completion(&completion.ticket) { continue; }
                    let done = completion.result;
                    // Updates sent before the prompt response are already queued; apply them first.
                    while let Some(inc) = self.conn.as_mut().and_then(|c| c.rx.try_recv().ok()) {
                        self.on_incoming(Some(inc)).await;
                    }
                    let (session, routine) = (self.turn_session.clone(), self.active_routine.is_some());
                    let failed = done.as_ref().is_err_and(acp::may_end_session);
                    self.finish_turn(&done);
                    match session {
                        _ if self.waiting.is_some() => {},
                        Some(sid) if routine => self.release_spent(&sid).await,
                        Some(sid) if failed => self.close_reloadable(&sid).await,
                        _ => {}
                    }
                    self.next_in_queue(&done_tx).await;
                }
                _ = flush_tick.tick() => {
                    self.flush(false);
                    self.retry_waiting_queue_restore();
                    if let Some(done) = self.finishing.take() { self.finish_turn(&done); }
                    self.retry_admission(&done_tx).await;
                    self.retry_waiting(&done_tx).await;
                    self.expire_abandoned().await;
                    if self.routine_completion.is_some() && self.retry_routine_completion() {
                        self.finish_dispatch();
                        self.next_in_queue(&done_tx).await;
                    }
                    // MX-263: o relógio da rotina não corre durante a espera da assinatura.
                    if self.active_routine.is_some() && self.turn.is_some() && !self.waiting_for_reset() && self.routine_deadline.is_some_and(|deadline| Instant::now() >= deadline) && !self.routine_timed_out {
                        self.routine_timed_out = true;
                        self.stop_requested = true;
                        if let Some(c) = self.conn.take() { c.kill().await; }
                    }
                    // The agent ignored the cancel: stop its process so the bot is free again.
                    if self.turn.is_some() && self.cancel_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                        self.cancel_deadline = None;
                        if let Some(c) = self.conn.take() { c.kill().await; }
                    }
                    self.release_idle().await;
                    // The idle check may have queued a compaction.
                    if self.turn.is_none() && !self.queue.is_empty() {
                        self.next_in_queue(&done_tx).await;
                    }
                },
            }
        }
        self.files.cancel();
        self.hub.team.cancel_from(&self.cfg.id);
        self.complete_request(Err(anyhow!("recipient shut down")), true, crate::fork::retornos::Fim::Desligado);
        self.complete_group(Err(anyhow!("bot shut down")));
        if let Some(c) = self.conn.take() {
            c.kill().await;
        }
    }

    fn id(&self) -> String {
        self.cfg.id.clone()
    }

    fn set_activity(&self, a: &str, kind: crate::fork::activity_code::ActivityKind) {
        let target = crate::fork::activity_code::ActivityCode::of(kind.clone());
        let current = self.hub.runtime(&self.cfg.id);
        if current.activity == a && current.activity_code.as_ref() == Some(&target) {
            return;
        }
        let a = a.to_owned();
        self.hub.set_runtime(&self.cfg.id, |r| {
            if r.status == BotStatus::Working {
                r.set_activity(a, Some(kind));
            }
        });
    }

    /// Adds an entry to the current lane, signed by this bot (a group shows who said it).
    fn add(&self, kind: EntryKind, turn: i64, mut data: Value) -> Option<Entry> {
        data["author"] = self.cfg.id.clone().into();
        self.hub.add_entry(&self.lane, kind, turn, &data)
    }

    /// A notice in the running turn's lane (or the bot's own chat between turns).
    fn notice(&self, text: &str, style: NoticeStyle) {
        if let Err(error) = self.try_notice(text, style) {
            tracing::error!(%error, "não foi possível publicar o aviso");
        }
    }

    fn try_notice(&self, text: &str, style: NoticeStyle) -> Result<()> {
        let lane = if self.turn.is_some() { self.lane.clone() } else { Lane::main(&self.cfg.id) };
        let turn = self.turn.unwrap_or_else(|| self.hub.store.max_turn(&self.cfg.id));
        let data = json!({"text": text, "style": style, "author": self.cfg.id});
        if let Some(id) = self.completion_notice_id() {
            self.hub.add_entry_once(&id, &lane, EntryKind::Notice, turn, &data)?;
        } else {
            self.hub
                .add_entry(&lane, EntryKind::Notice, turn, &data)
                .ok_or_else(|| anyhow!("não foi possível salvar o aviso"))?;
        }
        Ok(())
    }
}

/// How long a cancelled turn may take to wind down before its agent process is stopped.
const CANCEL_GRACE: Duration = Duration::from_secs(15);

fn inflight_key(bot_id: &str) -> String {
    format!("turn.inflight.{bot_id}")
}

async fn recv_incoming(conn: &mut Option<Conn>) -> Option<Incoming> {
    match conn {
        Some(c) => {
            // Conserva a resposta original antes do próximo Incoming. Os outros
            // ramos do select do ator continuam atendendo lifecycle e comandos.
            while !c.skill_reads.is_empty() {
                let _ = c.skill_reads.join_next().await;
            }
            c.rx.recv().await
        }
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn memory_work_reads_as_thinking() {
        use crate::fork::activity_code::ActivityKind;
        assert_eq!(super::activity("mcp__chat__send_message").0, "Typing…");
        assert_eq!(super::activity("mcp__chat__send_message").1, ActivityKind::Typing);
        assert_eq!(super::activity("mcp__memory__remember").1, ActivityKind::Thinking);
        assert_eq!(super::activity("Write bot-memory/trip.md").0, "Thinking…");
        assert_eq!(super::activity("Terminal"), ("Terminal", ActivityKind::Tool { title: "Terminal".into() }));
    }
}

#[cfg(test)]
mod fork_owner_question_tests;

#[cfg(test)]
mod fork_genui_tests;

#[cfg(test)]
mod fork_fila_tests;
#[cfg(test)]
mod fork_sessoes_tests;
#[cfg(test)]
mod waiting_tests;

#[cfg(test)]
#[path = "context_effects_tests.rs"]
mod context_effects_tests;

#[cfg(test)]
#[path = "../../fork/cadeias/ator_testes.rs"]
mod cadeias_tests;

#[cfg(test)]
#[path = "../../fork/whatsapp/passos_tests.rs"]
mod whatsapp_passos_tests;

#[cfg(test)]
mod skills_tests;
