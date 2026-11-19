//! G1b C6: selo de ferramenta da extra aberta de um bot agente do Pessoal (inclusive o Berga), emitido pelo Hub.
//!
//! O selo não é Authority, grant nem permissão: só liga o processo `chat` desta definição à `SessionRef`, ao
//! `BindingId` original, à geração do perfil e à ACP exata. O prefixo `session:`, a presença de um token ou o
//! `spaceContext` nunca provam domínio. Só `mostrar_componente` e o catálogo interno (`chatCall`/`__tools`) o usam;
//! as demais ferramentas da extra seguem o percurso que já têm. Principal, grupos, canais, Times e rotinas ficam
//! fora. O token vive só no `env` do builtin `chat`; o KV guarda o SHA256 e nada disso vai a log ou ao modelo.
use super::bindings::{self, BindingRef, ExecutionRef};
use super::store::{self as sessions, SessionRef};
use crate::{
    LockExt,
    api::devices::{Caller, Forbidden},
    fork::ponte_credencial::Origem,
    hub::{BotStatus, Hub, Runtime},
    remote::crypto,
    store::{BotConfig, Lane},
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

/// Variável do builtin `chat` da extra selada; a ponte a envia em [`BODY`], fora de `arguments`.
pub(crate) const SEAL_ENV: &str = "DONNABOT_SESSION_TOOL_SEAL";
pub(crate) const BODY: &str = "sessionToolSeal";
/// Ação reservada do catálogo da ponte `chat`: só descoberta, nunca ferramenta anunciada ao modelo.
pub(crate) const CATALOG: &str = "__tools";
const EXTRA: &str = "session:";
const REQUIRED: &str = "tool_binding_required";
const MISMATCH: &str = "tool_binding_mismatch";
const CHANGED: &str = "tool_session_changed";
const TURN: &str = "tool_turn_changed";
const HUMAN: &str = "human_turn_required";

/// Registro privado no KV existente, por bot e extra: o selo de A nunca sobrescreve nem vale em B.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    hash: String,
    process: String,
    bot: String,
    session: String,
    slot: String,
    binding: String,
    generation: i64,
    acp: Option<String>,
}

fn key(bot: &str, session: &str) -> String {
    format!("donna.sessions.tool-seal.{bot}.{session}")
}

fn digest(token: &str) -> String {
    crate::agent::registry::sha256_hex(token.as_bytes())
}

fn read(c: &Connection, bot: &str, session: &str) -> Result<Option<Record>> {
    let raw: Option<String> =
        c.query_row("SELECT v FROM kv WHERE k=?1", [key(bot, session)], |r| r.get(0)).optional()?;
    raw.map(|raw| serde_json::from_str(&raw).map_err(|_| anyhow!(REQUIRED))).transpose()
}

fn write(c: &Connection, record: &Record) -> Result<()> {
    c.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        params![key(&record.bot, &record.session), serde_json::to_string(record)?],
    )?;
    Ok(())
}

/// Extra aberta (não principal) do próprio chat, de bot agente vivo do Pessoal. Só a Connection recebida.
fn eligible_on(c: &Connection, bot: &str, session: &str) -> Result<bool> {
    if session == bot || !sessions::personal_on(c, bot)? {
        return Ok(false);
    }
    Ok(sessions::get(c, session)?.is_some_and(|row| row.chat_id == bot && !row.principal && row.state == "open"))
}

fn expected_ref(bot: &str, session: &str) -> SessionRef {
    SessionRef { chat_id: bot.into(), id: session.into() }
}

fn is_chat_definition(server: &Value) -> bool {
    server["name"] == "chat" && server["args"][0] == "mcp" && server["args"][1] == "chat"
}

fn seal_in(servers: &Value) -> Option<&str> {
    servers
        .as_array()?
        .iter()
        .filter(|s| is_chat_definition(s))
        .flat_map(|s| s["env"].as_array().into_iter().flatten())
        .find(|e| e["name"] == SEAL_ENV)?["value"]
        .as_str()
}

/// Emissão ao preparar as definições MCP do turno. Fora da extra elegível não emite nada (a escolha fica
/// indisponível) e não altera as demais ferramentas; só erro técnico do banco interrompe.
pub(crate) fn issue(
    hub: &Hub,
    cfg: &BotConfig,
    slot: Option<&str>,
    binding: &BindingRef,
    servers: &mut [Value],
) -> Result<()> {
    let Some(session) = slot.and_then(|s| s.strip_prefix(EXTRA)) else { return Ok(()) };
    if cfg.context.is_some()
        || cfg.is_group()
        || cfg.space_id != crate::fork::spaces::PERSONAL
        || binding.bot != cfg.id
        || binding.slot.as_deref() != slot
        || binding.session_ref != expected_ref(&cfg.id, session)
    {
        return Ok(());
    }
    let Some(chat) = servers.iter_mut().find(|s| is_chat_definition(s)) else { return Ok(()) };
    let mut c = hub.store.db.locked();
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let live = bindings::resolve(&tx, binding.id.as_str(), &binding.bot).ok();
    if !eligible_on(&tx, &cfg.id, session)?
        || live.is_none_or(|live| live.session_ref != binding.session_ref || live.slot != binding.slot)
    {
        return Ok(());
    }
    let token = zeroize::Zeroizing::new(format!("selo_{}", crypto::b64(&crypto::random::<32>())));
    write(
        &tx,
        &Record {
            hash: digest(&token),
            process: crate::fork::spaces::tool_context::process().into(),
            bot: cfg.id.clone(),
            session: session.into(),
            slot: format!("{EXTRA}{session}"),
            binding: binding.id.as_str().into(),
            generation: crate::fork::spaces::config_generation_on(&tx, &cfg.id)?,
            acp: None,
        },
    )?;
    tx.commit()?;
    chat["env"]
        .as_array_mut()
        .ok_or_else(|| anyhow!("invalid_mcp_environment"))?
        .push(json!({"name": SEAL_ENV, "value": token.as_str()}));
    Ok(())
}

/// Depois de session/new ou session/load: liga o selo desta definição à ACP exata. Selo substituído, de outro
/// processo, binding ou geração recusa; a ACP só é gravada uma vez.
pub(crate) fn activate(hub: &Hub, servers: &Value, slot: Option<&str>, binding: &BindingRef, acp: &str) -> Result<()> {
    let Some(token) = seal_in(servers) else { return Ok(()) };
    let session = slot.and_then(|s| s.strip_prefix(EXTRA)).ok_or_else(|| anyhow!(MISMATCH))?;
    let mut c = hub.store.db.locked();
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut record = read(&tx, &binding.bot, session)?.ok_or_else(|| anyhow!(REQUIRED))?;
    ensure!(
        record.hash == digest(token)
            && record.process == crate::fork::spaces::tool_context::process()
            && record.binding == binding.id.as_str()
            && record.generation == crate::fork::spaces::config_generation_on(&tx, &binding.bot)?
            && record.acp.as_deref().is_none_or(|current| current == acp)
            && eligible_on(&tx, &binding.bot, session)?,
        MISMATCH
    );
    record.acp = Some(acp.into());
    write(&tx, &record)?;
    tx.commit()?;
    Ok(())
}

/// Para reconhecer os servidores internos: a definição do `chat` sem o selo, que é conferido à parte (KV).
pub(crate) fn without_seal(servers: &Value) -> Value {
    let mut servers = servers.clone();
    for server in servers.as_array_mut().into_iter().flatten().filter(|s| is_chat_definition(s)) {
        if let Some(env) = server["env"].as_array_mut() {
            env.retain(|e| e["name"] != SEAL_ENV);
        }
    }
    servers
}

/// Selo apresentado pela ponte, conferido contra o registro, o binding e a extra (só a Connection recebida).
fn sealed(c: &Connection, body: &Value, binding: &BindingRef) -> Result<Record> {
    let bot = body["botId"].as_str().ok_or_else(|| anyhow!(MISMATCH))?;
    let slot = body["sessionSlot"].as_str().ok_or_else(|| anyhow!(MISMATCH))?;
    let session = slot.strip_prefix(EXTRA).ok_or_else(|| anyhow!(MISMATCH))?;
    let token = body[BODY].as_str().ok_or_else(|| anyhow!(REQUIRED))?;
    ensure!(body.get(crate::fork::spaces::tool_context::BODY).is_none(), MISMATCH);
    let record = read(c, bot, session)?.ok_or_else(|| anyhow!(REQUIRED))?;
    ensure!(
        record.hash == digest(token)
            && record.process == crate::fork::spaces::tool_context::process()
            && record.bot == bot
            && record.session == session
            && record.slot == slot
            && record.binding == binding.id.as_str()
            && binding.bot == bot
            && binding.session_ref == expected_ref(bot, session)
            && binding.slot.as_deref() == Some(slot)
            && record.acp.as_deref().is_none_or(|acp| binding.acp_session.as_deref() == Some(acp)),
        MISMATCH
    );
    ensure!(
        record.generation == crate::fork::spaces::config_generation_on(c, bot)? && eligible_on(c, bot, session)?,
        CHANGED
    );
    Ok(record)
}

pub(crate) fn is_catalog(method: &str, body: &Value) -> bool {
    method == "chatCall" && body["name"] == CATALOG
}

fn is_choice(method: &str, body: &Value) -> bool {
    method == "chatCall"
        && body["name"] == crate::fork::genui::flow::TOOL
        && body["sessionSlot"].as_str().is_some_and(|s| s.starts_with(EXTRA))
}

/// O ramo pessoal de `mostrar_componente` só aceita a Ponte autenticada ORIGINAL (já conferida por
/// `ponte_credencial::conferir`: a credencial emitida, o bot, o `BindingId` e o slot). `Local` ou outro caller com
/// execução e selo válidos recusa: env, prefixo e selo não substituem a Ponte. Outros caminhos não mudam.
pub(crate) fn require_ponte(caller: &Caller, method: &str, body: &Value) -> Result<()> {
    if is_choice(method, body) {
        ensure!(matches!(caller, Caller::Ponte { .. }), Forbidden);
    }
    Ok(())
}

/// Fase do catálogo, capturada uma única vez antes de responder.
enum Phase {
    /// Abertura: Preparation original de session/new/load, antes da ACP.
    Discovery(super::gateway::Preparation),
    /// ACP já ativa: a execução original do turno.
    Active(ExecutionRef),
}

/// Catálogo `chat` da extra selada (exceção fechada `chatCall`/`__tools`). Só diz se `mostrar_componente` pode ser
/// anunciado: não abre turno, não toca outbox/ContextTool/ShowChoice, pergunta, conta ou grant. A mesma fase capturada
/// é rechecada antes de responder; não adota a fase ativa nem outra preparação.
pub(crate) fn catalog(hub: &Hub, caller: &Caller, body: &Value) -> Result<Value> {
    let Caller::Ponte { origem, .. } = caller else { return Err(Forbidden.into()) };
    let bot = body["botId"].as_str().ok_or_else(|| anyhow!(MISMATCH))?;
    let binding =
        body["sessionBinding"].as_str().ok_or_else(|| anyhow!("chamada vencida: vínculo de sessão ausente"))?;
    let runtime = hub.runtime(bot);
    let phase = if runtime.session.is_none() {
        let preparation = super::gateway::capture(&hub.store.db.locked(), binding, bot, "tools/list")?;
        preparation.check_runtime(&runtime)?;
        Phase::Discovery(preparation)
    } else {
        Phase::Active(bindings::capture(hub, bot, binding)?)
    };
    let record = {
        let c = hub.store.db.locked();
        let live = bindings::resolve(&c, binding, bot)?;
        let record = sealed(&c, body, &live)?;
        if matches!(phase, Phase::Active(_)) {
            ensure!(record.acp.is_some() && record.acp == runtime.session, MISMATCH);
        }
        record
    };
    match &phase {
        Phase::Discovery(preparation) => {
            preparation.check_runtime(&hub.runtime(bot))?;
            preparation.check(&hub.store.db.locked())?;
        }
        Phase::Active(execution) => {
            let current = hub.runtime(bot);
            ensure!(
                matches!(current.status, BotStatus::Working | BotStatus::NeedsInput)
                    && current.session == runtime.session
                    && current.memory_turn_token.as_deref() == Some(execution.turn_token())
                    && current.lane == Some(Lane { chat: bot.into(), session: Some(record.session.clone()) }),
                TURN
            );
            bindings::assert_execution_live(&hub.store.db.locked(), execution)?;
        }
    }
    // SEG-C6-01: a conferência final segue com a MESMA Ponte original (coleção antes do DB, nunca dentro dele).
    origem.com_original(hub, || {
        let c = hub.store.db.locked();
        origem.conferir_db(&c)?;
        let live = bindings::resolve(&c, binding, bot)?;
        ensure!(sealed(&c, body, &live)? == record, CHANGED);
        Ok(json!({"available": true}))
    })
}

/// Cerca da variante pessoal, presa à `ExecutionRef` original. Não emite identidade, token ou permissão.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Fence {
    record: Record,
    acp: String,
    message: String,
    started_at: Option<i64>,
    turn_token: String,
}

impl Fence {
    /// Na Tx do efeito: só a Connection recebida (nunca Store, Cloud, runtime, rede ou await).
    pub(crate) fn check(&self, c: &Connection) -> Result<()> {
        let record = &self.record;
        ensure!(read(c, &record.bot, &record.session)?.as_ref() == Some(record), CHANGED);
        ensure!(
            record.process == crate::fork::spaces::tool_context::process()
                && record.acp.as_deref() == Some(self.acp.as_str())
                && record.generation == crate::fork::spaces::config_generation_on(c, &record.bot)?
                && eligible_on(c, &record.bot, &record.session)?,
            CHANGED
        );
        let entry = crate::store::entry_on(c, &self.message)?.ok_or_else(|| anyhow!("message_not_found"))?;
        let at = sessions::session_ref_of_entry(c, &record.bot, &self.message)?;
        ensure!(entry.bot_id == record.bot && at.id == record.session && entry.is_user_authored(), HUMAN);
        Ok(())
    }

    pub(crate) fn lane(&self) -> Lane {
        Lane { chat: self.record.bot.clone(), session: Some(self.record.session.clone()) }
    }

    pub(crate) fn acp(&self) -> &str {
        &self.acp
    }
}

/// Captura única no `tools/call` de `mostrar_componente`, sobre a `ExecutionRef` que o `dispatch` já capturou
/// antes dos awaits. Nada aqui recaptura ou procura sucessora por ACP/bot.
pub(crate) fn capture(
    hub: &Hub,
    body: &Value,
    execution: Option<&ExecutionRef>,
    origem: &Origem,
) -> Result<crate::fork::spaces::tool_context::ToolContext> {
    let mut execution = execution.ok_or_else(|| anyhow!("chamada vencida"))?.clone();
    let bot = body["botId"].as_str().ok_or_else(|| anyhow!(MISMATCH))?;
    let binding = execution.acp_binding().ok_or_else(|| anyhow!("chamada vencida"))?.to_owned();
    ensure!(origem.corresponde(bot, &binding), Forbidden);
    let mut turn: Runtime = hub.runtime(bot);
    let record = {
        let c = hub.store.db.locked();
        let live = bindings::resolve(&c, &binding, bot)?;
        let record = sealed(&c, body, &live)?;
        ensure!(record.acp.is_some() && live.acp_session == record.acp, MISMATCH);
        record
    };
    let acp = record.acp.clone().ok_or_else(|| anyhow!(MISMATCH))?;
    let cfg = hub.store.bot(bot)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("bot_not_found"))?.config;
    ensure!(
        cfg.context.is_none() && !cfg.is_group() && cfg.space_id == crate::fork::spaces::PERSONAL,
        "cross_conversation_identity"
    );
    let message = turn.message.clone().ok_or_else(|| anyhow!(HUMAN))?;
    let message_turn = hub.store.entry(&message).ok_or_else(|| anyhow!(HUMAN))?.turn;
    let fence =
        Fence { record, acp, message, started_at: turn.started_at, turn_token: execution.turn_token().to_owned() };
    execution.bind_tool_fence(crate::fork::spaces::tool_context::ToolFence::PersonalSession(Box::new(fence.clone())));
    turn.execution = Some(execution);
    let context =
        crate::fork::spaces::tool_context::ToolContext::personal(cfg, turn, fence, origem.clone(), message_turn);
    context.check(hub)?;
    Ok(context)
}

/// Conferência fora da Tx (API e ator): compara EXPLICITAMENTE o `BindingId`, o token do turno, a mensagem, a ACP,
/// a lane e o selo capturados com o estado atual; `bindings::recheck` sozinho não basta (recaptura e descarta).
pub(crate) fn check(hub: &Hub, captured: &Runtime, fence: &Fence) -> Result<()> {
    let execution = captured.execution.as_ref().ok_or_else(|| anyhow!("chamada vencida"))?;
    ensure!(
        execution.acp_binding() == Some(fence.record.binding.as_str())
            && execution.turn_token() == fence.turn_token
            && captured.message.as_deref() == Some(fence.message.as_str()),
        TURN
    );
    let current = hub.runtime(&fence.record.bot);
    ensure!(
        matches!(current.status, BotStatus::Working | BotStatus::NeedsInput)
            && current.lane == Some(fence.lane())
            && current.session.as_deref() == Some(fence.acp.as_str())
            && current.message.as_deref() == Some(fence.message.as_str())
            && current.started_at == fence.started_at
            && current.memory_turn_token.as_deref() == Some(fence.turn_token.as_str()),
        TURN
    );
    let session = bindings::assert_execution_live(&hub.store.db.locked(), execution)?;
    ensure!(session == expected_ref(&fence.record.bot, &fence.record.session), TURN);
    Ok(())
}

/// No ator: além de [`check`], o perfil e a lane do ator têm de ser os da captura.
pub(crate) fn check_actor(
    hub: &Hub,
    captured: &Runtime,
    fence: &Fence,
    cfg: &BotConfig,
    lane: &Lane,
    session: Option<&str>,
) -> Result<()> {
    check(hub, captured, fence)?;
    ensure!(
        cfg.id == fence.record.bot
            && cfg.context.is_none()
            && cfg.space_id == crate::fork::spaces::PERSONAL
            && *lane == fence.lane()
            && session == Some(fence.acp.as_str()),
        "tool_actor_changed"
    );
    Ok(())
}

/// Ramo restrito de `mostrar_componente` na extra (`session:`), com a execução capturada pelo `dispatch`.
/// As outras ferramentas e os outros slots seguem o percurso que já têm. A `Origem` ORIGINAL da Ponte acompanha o
/// contexto até a escrita (SEG-C6-01); nenhuma sucessora é procurada.
pub(crate) async fn route(
    hub: &Arc<Hub>,
    caller: &Caller,
    method: &str,
    body: &Value,
    execution: Option<&ExecutionRef>,
) -> Result<Option<Value>> {
    use crate::fork::genui::flow;
    if !is_choice(method, body) {
        return Ok(None);
    }
    let Caller::Ponte { origem, .. } = caller else { return Err(Forbidden.into()) };
    let invocation = flow::invocation(body)?;
    let ctx = capture(hub, body, execution, origem)?;
    #[cfg(test)]
    tests::after_capture(hub);
    let result =
        crate::chat::outbox::call_in(hub, &ctx.cfg.id, flow::TOOL, &body["arguments"], Some(&ctx), Some(&invocation))
            .await;
    ctx.check(hub)?;
    result.map(Some)
}

#[cfg(test)]
#[path = "contexto_ferramenta_tests.rs"]
pub(crate) mod tests;
