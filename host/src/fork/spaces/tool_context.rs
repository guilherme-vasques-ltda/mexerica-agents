//! Origem emitida pelo Hub: slot não é autoridade nem destino substituível.
use crate::{
    agent::bot::Cmd,
    hub::{BotStatus, Hub, Runtime},
    store::{BotConfig, Entry, Lane},
};
use anyhow::{Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, OnceLock};

pub(crate) const BINDING_ENV: &str = "DONNABOT_TOOL_BINDING";
/// Campo do corpo em que a ponte envia [`BINDING_ENV`].
pub(crate) const BODY: &str = "toolBinding";
pub(crate) fn process() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| uuid::Uuid::new_v4().to_string())
}
fn key(chat: &str) -> String {
    format!("donna.tools.binding.{chat}")
}
#[derive(Clone, Serialize, Deserialize)]
struct Binding {
    token: String,
    process: String,
    bot: String,
    space: String,
    chat: String,
    stamp: String,
    session: String,
}

pub(crate) fn definition_binding(servers: &Value) -> Option<&str> {
    servers
        .as_array()?
        .iter()
        .flat_map(|s| s["env"].as_array().into_iter().flatten())
        .find(|e| e["name"] == BINDING_ENV)?["value"]
        .as_str()
}
pub(crate) fn prepare(hub: &Hub, cfg: &BotConfig, servers: &Value) -> Result<()> {
    activate(hub, cfg, servers, "")
}
pub(crate) fn activate(hub: &Hub, cfg: &BotConfig, servers: &Value, session: &str) -> Result<()> {
    let Some(context) = &cfg.context else { return Ok(()) };
    if context.foreign_identity {
        return Ok(());
    }
    let token = definition_binding(servers).ok_or_else(|| anyhow!("tool_binding_required"))?;
    let binding = Binding {
        token: token.into(),
        process: process().into(),
        bot: cfg.id.clone(),
        space: cfg.space_id.clone(),
        chat: context.conversation_id.clone(),
        stamp: super::execution_stamp(&hub.store, cfg)?,
        session: session.into(),
    };
    if !session.is_empty() {
        let raw = hub.store.kv_read(&key(&binding.chat))?.ok_or_else(|| anyhow!("tool_binding_required"))?;
        let original: Binding = serde_json::from_str(&raw)?;
        ensure!(
            original.token == binding.token && original.process == binding.process && original.stamp == binding.stamp,
            "tool_binding_mismatch"
        );
    }
    hub.store.kv_set(&key(&binding.chat), &serde_json::to_string(&binding)?)
}

// Nova configuração pede novas definições MCP; capturas antigas nunca recebem o selo novo.
pub(crate) fn needs_rebind(hub: &Hub, cfg: &BotConfig) -> Result<bool> {
    let Some(context) = cfg.context.as_ref().filter(|c| !c.foreign_identity) else { return Ok(false) };
    let Some(raw) = hub.store.kv_read(&key(&context.conversation_id))? else { return Ok(true) };
    let binding: Binding = serde_json::from_str(&raw)?;
    Ok(binding.process != process()
        || binding.bot != cfg.id
        || binding.space != cfg.space_id
        || binding.stamp != super::execution_stamp(&hub.store, cfg)?)
}

fn configuration(hub: &Hub, binding: &Binding) -> Result<BotConfig> {
    let original = hub.store.bot(&binding.bot)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("bot_not_found"))?.config;
    let c = hub.store.space_conversation(&binding.space, &binding.chat)?;
    ensure!(
        !original.is_group()
            && c.bot_id == original.id
            && original.space_id == c.space_id
            && !c.closed
            && c.id != c.bot_id,
        "cross_conversation_identity"
    );
    let cfg = super::execution_config(&hub.store, &original, &c.id)?;
    ensure!(super::execution_stamp(&hub.store, &cfg)? == binding.stamp, "conversation_binding_changed");
    Ok(cfg)
}

/// Alvo fechado da captura: a conversa de Space (caminho existente) ou a extra do Pessoal (C6). Escolhido pela
/// captura, nunca por prefixo, carimbo ou `cfg.context` ausente.
#[derive(Clone)]
enum Target {
    Conversation(Binding),
    /// A cerca e a `Origem` ORIGINAL da Ponte que fez a chamada (SEG-C6-01), conferida no ponto da escrita.
    PersonalSession(Box<crate::fork::sessoes::contexto_ferramenta::Fence>, crate::fork::ponte_credencial::Origem),
}

#[derive(Clone)]
pub(crate) struct ToolContext {
    pub cfg: BotConfig,
    pub turn: Runtime,
    target: Target,
    message_turn: i64,
}
impl ToolContext {
    /// Só `sessoes::contexto_ferramenta::capture` monta a variante pessoal, já com a cerca presa à execução.
    pub(crate) fn personal(
        cfg: BotConfig,
        turn: Runtime,
        fence: crate::fork::sessoes::contexto_ferramenta::Fence,
        origem: crate::fork::ponte_credencial::Origem,
        message_turn: i64,
    ) -> Self {
        Self { cfg, turn, target: Target::PersonalSession(Box::new(fence), origem), message_turn }
    }
    /// SEG-C6-01: a escrita da variante pessoal só roda com a MESMA credencial da Ponte ainda emitida (coleção tomada
    /// ANTES do DB, nunca dentro dele); a conversa de Space segue como antes.
    pub(crate) fn com_origem<T>(&self, hub: &Hub, action: impl FnOnce() -> Result<T>) -> Result<T> {
        match &self.target {
            Target::Conversation(_) => action(),
            Target::PersonalSession(_, origem) => origem.com_original(hub, action),
        }
    }
    /// Na Tx da escrita (só a Connection recebida): o binding da Ponte original continua resolvível.
    pub(crate) fn conferir_origem_on(&self, c: &rusqlite::Connection) -> Result<()> {
        match &self.target {
            Target::Conversation(_) => Ok(()),
            Target::PersonalSession(_, origem) => origem.conferir_db(c),
        }
    }
    /// Domínio da pergunta pela variante capturada.
    pub(crate) fn domain(&self) -> crate::fork::genui::storage::Domain {
        match &self.target {
            Target::Conversation(_) => crate::fork::genui::storage::Domain::Conversation,
            Target::PersonalSession(..) => crate::fork::genui::storage::Domain::PersonalSession,
        }
    }
    #[cfg(test)]
    fn capture(hub: &Hub, body: &Value, chat: &str) -> Result<Self> {
        let execution = crate::fork::sessoes::bindings::capture(
            hub,
            body["botId"].as_str().ok_or_else(|| anyhow!("bot_required"))?,
            body["sessionBinding"].as_str().ok_or_else(|| anyhow!("chamada vencida"))?,
        )?;
        Self::capture_in_execution(hub, body, chat, Some(&execution))
    }
    pub(super) fn capture_in_execution(
        hub: &Hub,
        body: &Value,
        chat: &str,
        execution: Option<&crate::fork::sessoes::ExecutionRef>,
    ) -> Result<Self> {
        let raw = hub.store.kv_read(&key(chat))?.ok_or_else(|| anyhow!("tool_binding_required"))?;
        let binding: Binding = serde_json::from_str(&raw)?;
        ensure!(
            body["botId"] == binding.bot
                && body["spaceId"] == binding.space
                && body["toolBinding"] == binding.token
                && binding.chat == chat
                && binding.process == process(),
            "tool_binding_mismatch"
        );
        let cfg = configuration(hub, &binding)?;
        let mut turn = hub.runtime(&binding.bot);
        turn.execution = Some(execution.ok_or_else(|| anyhow!("chamada vencida"))?.clone());
        let message_turn = turn
            .message
            .as_deref()
            .and_then(|id| hub.store.entry(id))
            .ok_or_else(|| anyhow!("human_turn_required"))?
            .turn;
        let fence = ToolFence::Conversation {
            cfg: serde_json::to_string(&cfg)?,
            context: cfg.context.clone(),
            binding: serde_json::to_string(&binding)?,
            message: turn.message.clone().ok_or_else(|| anyhow!("human_turn_required"))?,
        };
        if let Some(execution) = &mut turn.execution {
            execution.bind_tool_fence(fence);
        }
        let context = Self { cfg, turn, target: Target::Conversation(binding), message_turn };
        context.check(hub)?;
        Ok(context)
    }
    /// A ACP do turno (nunca o ID visível da extra).
    pub(crate) fn session(&self) -> &str {
        match &self.target {
            Target::Conversation(binding) => &binding.session,
            Target::PersonalSession(fence, _) => fence.acp(),
        }
    }
    pub(crate) fn number(&self) -> i64 {
        self.message_turn
    }
    pub(crate) fn lane(&self) -> Lane {
        match &self.target {
            Target::Conversation(binding) => Lane::main(&binding.chat),
            Target::PersonalSession(fence, _) => fence.lane(),
        }
    }
    pub(crate) fn check(&self, hub: &Hub) -> Result<()> {
        let binding = match &self.target {
            Target::Conversation(binding) => binding,
            Target::PersonalSession(fence, _) => {
                return crate::fork::sessoes::contexto_ferramenta::check(hub, &self.turn, fence);
            }
        };
        crate::fork::sessoes::bindings::recheck_optional(hub, self.turn.execution.as_ref())?;
        let cfg = configuration(hub, binding)?;
        let active: Binding = serde_json::from_str(
            &hub.store.kv_read(&key(&binding.chat))?.ok_or_else(|| anyhow!("tool_binding_required"))?,
        )?;
        ensure!(active.token == binding.token && active.process == process(), "tool_session_changed");
        let current = hub.runtime(&self.cfg.id);
        ensure!(
            matches!(current.status, BotStatus::Working | BotStatus::NeedsInput)
                && current.lane == Some(self.lane())
                && current.session.as_deref() == Some(binding.session.as_str())
                && current.message == self.turn.message
                && current.started_at == self.turn.started_at
                && current.memory_turn_token == self.turn.memory_turn_token
                && current.memory_turn_token.is_some(),
            "tool_turn_changed"
        );
        let id = self.turn.message.as_ref().ok_or_else(|| anyhow!("human_turn_required"))?;
        let entry = hub.store.entry(id).ok_or_else(|| anyhow!("message_not_found"))?;
        let human = super::connector_cards::source(hub, &cfg, &entry)?;
        ensure!(entry.bot_id == binding.chat && human.is_user_authored(), "human_turn_required");
        super::validate_entries(hub, &cfg, &[id.clone(), human.id])
    }
    pub(crate) fn check_actor(&self, hub: &Hub, cfg: &BotConfig, lane: &Lane, session: Option<&str>) -> Result<()> {
        let binding = match &self.target {
            Target::Conversation(binding) => binding,
            Target::PersonalSession(fence, _) => {
                return crate::fork::sessoes::contexto_ferramenta::check_actor(
                    hub, &self.turn, fence, cfg, lane, session,
                );
            }
        };
        self.check(hub)?;
        ensure!(
            cfg.id == self.cfg.id
                && cfg.space_id == self.cfg.space_id
                && cfg.session_owner() == binding.chat
                && *lane == self.lane()
                && session == Some(binding.session.as_str()),
            "tool_actor_changed"
        );
        Ok(())
    }
    pub(crate) fn command(&self, cmd: Cmd) -> Cmd {
        Cmd::ContextTool { context: Box::new(self.clone()), command: Box::new(cmd) }
    }
}

pub(crate) async fn route_in_execution(
    hub: &Arc<Hub>,
    method: &str,
    body: &Value,
    execution: Option<&crate::fork::sessoes::ExecutionRef>,
) -> Result<Option<Value>> {
    if !matches!(
        method,
        "memoryCall"
            | "chatCall"
            | "teamCall"
            | "manageCall"
            | "routineCall"
            | "connectorCall"
            | "taskCall"
            | "computerCall"
            | "computersCall"
    ) {
        return Ok(None);
    }
    let slot = body["sessionSlot"].as_str();
    let Some(chat) = slot.and_then(|s| s.strip_prefix("conversation:")) else {
        ensure!(body.get("toolBinding").is_none(), "tool_slot_required");
        if let Some(bot) = body["botId"].as_str() {
            let lane = hub.runtime(bot).lane;
            if let Some(lane) = lane {
                ensure!(
                    lane.chat == bot
                        || hub.store.bot(&lane.chat)?.is_some_and(|row| !row.deleted && row.config.is_group()),
                    "tool_context_required"
                );
            }
        }
        return Ok(None);
    };
    let ctx = ToolContext::capture_in_execution(hub, body, chat, execution)?;
    let server = match method {
        "memoryCall" => "memory",
        "chatCall" => "chat",
        "teamCall" => "team",
        "taskCall" => "tasks",
        "connectorCall" => "connectors",
        _ => bail!("space_tool_not_bound"),
    };
    ensure!(
        super::tool_servers::decide(&hub.store, &ctx.cfg, false, false).builtin.contains(&server),
        "space_tool_not_bound"
    );
    let name = body["name"].as_str().ok_or_else(|| anyhow!("tool_name_required"))?;
    let args = &body["arguments"];
    let result = match server {
        "connectors" => {
            let execution = ctx.turn.execution.as_ref().ok_or_else(|| anyhow!("chamada vencida"))?;
            crate::market::requests::call_in_context(hub, &ctx.cfg.id, name, args, execution, Some(&ctx))
        }
        "memory" if matches!(name, "search_history" | "open_history") => Ok(crate::fork::historico::call_in_execution(
            hub,
            &ctx.cfg.id,
            name,
            args,
            slot,
            ctx.turn.execution.as_ref(),
        )
        .await),
        "memory" => crate::chat::memory::tool_call_in(hub, &ctx.cfg.id, name, args, Some(&ctx)).await,
        "chat" if crate::chat::profile::TOOLS.contains(&name) => {
            crate::chat::profile::call(hub, &ctx.cfg.id, name, args).await
        }
        "chat" => {
            // G1b: invocação só do envelope da ponte, nunca de `arguments`; ausente recusa.
            use crate::fork::genui::flow;
            let invocation = (name == flow::TOOL).then(|| flow::invocation(body)).transpose()?;
            crate::chat::outbox::call_in(hub, &ctx.cfg.id, name, args, Some(&ctx), invocation.as_deref()).await
        }
        "tasks" => crate::fork::tarefas::call_in(hub, &ctx.cfg.id, name, args, Some(&ctx)).await,
        _ => crate::chat::team::call_in(hub, &ctx.cfg.id, name, args, Some(&ctx)).await,
    };
    ctx.check(hub)?;
    result.map(Some)
}

// Cartões duráveis usam um selo privado, separado do cache do app e da sessão viva.
#[derive(Serialize, Deserialize)]
struct CardBinding {
    bot: String,
    space: String,
    chat: String,
    stamp: String,
}
#[cfg(test)]
pub(crate) fn record_card(hub: &Hub, cfg: &BotConfig, entry: &Entry) -> Result<()> {
    use crate::LockExt;
    record_card_on(&hub.store.db.locked(), cfg, entry)
}
pub(crate) fn record_card_on(db: &rusqlite::Connection, cfg: &BotConfig, entry: &Entry) -> Result<()> {
    if let Some(c) = cfg.context.as_ref().filter(|c| !c.foreign_identity) {
        ensure!(entry.bot_id == c.conversation_id && !c.foreign_identity, "card_context_mismatch");
        let binding = CardBinding {
            bot: cfg.id.clone(),
            space: cfg.space_id.clone(),
            chat: c.conversation_id.clone(),
            stamp: super::binding::execution_stamp_on(db, cfg)?,
        };
        db.execute(
            "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            rusqlite::params![format!("donna.tools.card.{}", entry.id), serde_json::to_string(&binding)?],
        )?;
    }
    Ok(())
}
pub(crate) fn card_config(hub: &Hub, entry: &Entry, original: BotConfig) -> Result<BotConfig> {
    if entry.bot_id == original.id {
        return Ok(original);
    }
    if hub.store.bot(&entry.bot_id)?.is_some_and(|row| {
        !row.deleted
            && row.config.is_group()
            && row.config.space_id == original.space_id
            && row.config.members.contains(&original.id)
    }) {
        return Ok(original);
    }
    let binding: CardBinding = serde_json::from_str(
        &hub.store
            .kv_read(&format!("donna.tools.card.{}", entry.id))?
            .ok_or_else(|| anyhow!("card_binding_required"))?,
    )?;
    ensure!(
        binding.bot == original.id && binding.space == original.space_id && binding.chat == entry.bot_id,
        "card_context_mismatch"
    );
    let c = hub.store.space_conversation(&binding.space, &binding.chat)?;
    ensure!(c.bot_id == original.id && c.space_id == original.space_id && !c.closed, "card_context_mismatch");
    let cfg = super::execution_config(&hub.store, &original, &binding.chat)?;
    ensure!(super::execution_stamp(&hub.store, &cfg)? == binding.stamp, "card_binding_changed");
    Ok(cfg)
}

#[cfg(test)]
#[path = "tool_context_tests.rs"]
pub(crate) mod tests;

/// Conserva somente a captura original; não emite identidade, token ou permissão.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum ToolFence {
    Conversation {
        cfg: String,
        context: Option<super::ExecutionContext>,
        binding: String,
        message: String,
    },
    /// C6: extra do Pessoal; cerca própria em `sessoes::contexto_ferramenta`, só Connection.
    PersonalSession(Box<crate::fork::sessoes::contexto_ferramenta::Fence>),
}
impl ToolFence {
    pub(crate) fn check(&self, db: &rusqlite::Connection) -> Result<()> {
        let (cfg, context, binding, message) = match self {
            Self::Conversation { cfg, context, binding, message } => (cfg, context, binding, message),
            Self::PersonalSession(fence) => return fence.check(db),
        };
        let mut cfg: BotConfig = serde_json::from_str(cfg)?;
        cfg.context.clone_from(context);
        let binding: Binding = serde_json::from_str(binding)?;
        let raw: String = db.query_row("SELECT v FROM kv WHERE k=?1", [key(&binding.chat)], |r| r.get(0))?;
        let active: Binding = serde_json::from_str(&raw)?;
        ensure!(
            active.token == binding.token
                && active.process == binding.process
                && binding.process == process()
                && active.bot == binding.bot
                && active.space == binding.space
                && active.chat == binding.chat
                && active.session == binding.session
                && active.stamp == binding.stamp,
            "tool_session_changed"
        );
        ensure!(super::binding::execution_stamp_on(db, &cfg)? == binding.stamp, "conversation_binding_changed");
        let entry = crate::store::entry_on(db, message)?.ok_or_else(|| anyhow!("message_not_found"))?;
        ensure!(entry.bot_id == binding.chat && entry.data["executionBinding"] == binding.stamp, "human_turn_required");
        super::connector_cards::human_on(db, &entry)
    }
}

impl std::fmt::Debug for ToolFence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ToolFence(private)")
    }
}

/// Abertura permite somente catálogo, junto da Preparation original do PR3.
pub(crate) struct CatalogContext {
    binding: Binding,
}
impl CatalogContext {
    pub(crate) fn check(&self, hub: &Hub) -> Result<()> {
        configuration(hub, &self.binding)?;
        let raw = hub.store.kv_read(&key(&self.binding.chat))?.ok_or_else(|| anyhow!("tool_binding_required"))?;
        let current: Binding = serde_json::from_str(&raw)?;
        ensure!(
            current.token == self.binding.token
                && current.process == self.binding.process
                && current.stamp == self.binding.stamp,
            "tool_session_changed"
        );
        Ok(())
    }
}
pub(crate) fn capture_catalog(hub: &Hub, body: &Value) -> Result<Option<CatalogContext>> {
    let Some(chat) = body["sessionSlot"].as_str().and_then(|s| s.strip_prefix("conversation:")) else {
        ensure!(body.get("toolBinding").is_none(), "tool_slot_required");
        return Ok(None);
    };
    ensure!(body["name"] == "__tools", "space_tool_not_bound");
    let raw = hub.store.kv_read(&key(chat))?.ok_or_else(|| anyhow!("tool_binding_required"))?;
    let binding: Binding = serde_json::from_str(&raw)?;
    ensure!(
        binding.bot == body["botId"]
            && binding.space == body["spaceId"]
            && binding.token == body["toolBinding"]
            && binding.process == process(),
        "tool_binding_mismatch"
    );
    let cfg = configuration(hub, &binding)?;
    ensure!(
        super::tool_servers::decide(&hub.store, &cfg, false, false).builtin.contains(&"memory"),
        "space_tool_not_bound"
    );
    let catalog = CatalogContext { binding };
    catalog.check(hub)?;
    Ok(Some(catalog))
}

#[cfg(test)]
pub(crate) async fn route(hub: &Arc<Hub>, method: &str, body: &Value) -> Result<Option<Value>> {
    let execution = crate::fork::sessoes::bindings::capture(
        hub,
        body["botId"].as_str().ok_or_else(|| anyhow!("bot_required"))?,
        body["sessionBinding"].as_str().ok_or_else(|| anyhow!("chamada vencida"))?,
    )?;
    route_in_execution(hub, method, body, Some(&execution)).await
}
