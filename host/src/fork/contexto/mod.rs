//! DON-906/C2: uso observado do motor, por conversa visível. Não faz parte do mirror.
use crate::{
    LockExt,
    api::devices::{self, Caller},
    hub::Hub,
    store::{BotConfig, Lane},
};
use anyhow::{Result, anyhow, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) const METHODS: &[&str] = &["donnaContext", "donnaContextCompact"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[expect(clippy::struct_field_names, reason = "DTO conserva os IDs explícitos do protocolo por conversa.")]
pub struct Read {
    pub bot_id: String,
    pub session_id: Option<String>,
    pub conversation_id: Option<String>,
    pub space_id: Option<String>,
    pub context_id: Option<String>,
    /// Cliente ligado a Hub (`hubId` injetado pelo cliente de Space): quando presente, precisa ser este Hub.
    pub hub_id: Option<String>,
}

pub struct Request {
    pub read: Read,
    pub compact: bool,
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    // A mesma autoridade de enviar naquela conversa, sem ferramenta nova para bots.
    devices::permit(caller, "send")?;
    let read: Read = serde_json::from_value(body).map_err(|_| anyhow!("invalid_context_request"))?;
    ensure!(read.hub_id.as_deref().is_none_or(|id| id == hub.host_id), "context_engine_changed");
    let target = resolve(hub, &read)?;
    let (reply, answer) = tokio::sync::oneshot::channel();
    hub.send_cmd(
        &target.bot,
        crate::agent::bot::Cmd::ForkContext {
            request: Request { read, compact: method == "donnaContextCompact" },
            reply,
        },
    )?;
    tokio::time::timeout(std::time::Duration::from_secs(30), answer).await??
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct Target {
    pub bot: String,
    pub hub_id: String,
    /// Space da conversa resolvida (não o `spaceId` do pedido: o da linha real).
    pub space_id: String,
    pub epoch: u32,
    pub lane: Lane,
    pub acp: Option<String>,
    pub engine: Value,
}

impl Target {
    // A época pertence à ACP: compactar muda contextId, trocar ACP muda scopeId.
    pub fn scope_id(&self) -> Result<String> {
        let scope = (&self.bot, &self.hub_id, &self.space_id, &self.lane, &self.acp, &self.engine);
        Ok(crate::agent::registry::sha256_hex(&serde_json::to_vec(&scope)?))
    }

    pub fn context_id(&self) -> Result<String> {
        Ok(crate::agent::registry::sha256_hex(&serde_json::to_vec(self)?))
    }
}

pub(crate) fn engine(cfg: &BotConfig) -> Value {
    json!({"backend":cfg.backend,"account":cfg.fork_editor.engine_account_id,
        "model":cfg.model,"options":cfg.model_options,"command":cfg.command})
}

/// Resolve o destino como `send`, mas sem materializar sessão ou iniciar motor.
pub(crate) fn resolve(hub: &Hub, read: &Read) -> Result<Target> {
    let chat = read.conversation_id.as_deref().unwrap_or(&read.bot_id);
    let space = read.space_id.as_deref().unwrap_or(crate::fork::spaces::PERSONAL);
    let conversation = hub.store.space_conversation(space, chat)?;
    ensure!(read.bot_id == chat || read.bot_id == conversation.bot_id, "cross_conversation_reference");
    let row = hub.store.bot(&conversation.bot_id)?.filter(|r| !r.deleted).ok_or_else(|| anyhow!("bot_not_found"))?;
    ensure!(!row.config.is_group(), "context_group_unsupported");
    let visible = read.session_id.as_deref().unwrap_or(chat);
    let lane = if visible == chat {
        Lane::main(chat)
    } else {
        let session = super::sessoes::store::get(&hub.store.db.locked(), visible)?
            .filter(|s| s.chat_id == chat)
            .ok_or_else(|| anyhow!("unknown_session"))?;
        Lane { chat: chat.into(), session: Some(session.id) }
    };
    let mut cfg = row.config;
    if chat != cfg.id {
        cfg.command = None;
        cfg.fork_editor.engine_account_id = conversation.engine_account_id;
        if let Some(selection) = conversation.engine_selection {
            cfg.backend = selection.backend;
            cfg.model = selection.model;
            cfg.model_options = selection.model_options;
        }
    }
    super::sessoes::admissao::apply_choice(&hub.store.db.locked(), &lane, &mut cfg)?;
    let slot = lane
        .session
        .as_ref()
        .map(|s| format!("session:{s}"))
        .or_else(|| (chat != cfg.id).then(|| format!("conversation:{chat}")));
    let acp = if let Some(slot) = slot {
        hub.store.kv_read(&crate::store::run_session_key(&cfg.id, &slot))?
    } else {
        row.session_id
    };
    let engine = engine(&cfg);
    let acp = acp.filter(|s| !s.is_empty());
    let epoch = acp.as_deref().map(|sid| epoch(hub, &lane.chat, sid)).transpose()?.unwrap_or(0);
    Ok(Target { bot: cfg.id, hub_id: hub.host_id.clone(), space_id: conversation.space_id, epoch, lane, acp, engine })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Usage {
    pub epoch: u32,
    pub used: u64,
    pub size: u64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Compaction {
    pub at: i64,
    pub by_host: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Observation {
    pub hub_id: String,
    pub lane: Lane,
    pub engine: Value,
    pub can_compact: bool,
    pub usage: Option<Usage>,
    pub last_compaction: Option<Compaction>,
}

fn key(hub: &Hub, bot: &str, acp: &str) -> String {
    format!("donna.context.{}.{bot}.{acp}", hub.host_id)
}

fn epoch(hub: &Hub, chat: &str, acp: &str) -> Result<u32> {
    let Some(raw) = hub.store.kv_read(&crate::chat::context::epoch_key(chat, acp))? else { return Ok(0) };
    let mut fields = raw.splitn(3, '\t');
    ensure!(fields.next() == Some(acp), "context_epoch_unavailable");
    fields
        .next()
        .ok_or_else(|| anyhow!("context_epoch_unavailable"))?
        .parse()
        .map_err(|_| anyhow!("context_epoch_unavailable"))
}

pub(crate) fn load(hub: &Hub, bot: &str, acp: &str) -> Result<Option<Observation>> {
    hub.store.kv_read(&key(hub, bot, acp))?.map(|s| serde_json::from_str(&s).map_err(Into::into)).transpose()
}

pub(crate) fn observe(
    hub: &Hub,
    cfg: &BotConfig,
    lane: &Lane,
    acp: &str,
    can_compact: bool,
    usage: Option<(u64, u64)>,
    compacted: Option<Compaction>,
) -> Result<()> {
    let mut observation = load(hub, &cfg.id, acp)?
        .filter(|o| o.hub_id == hub.host_id && o.lane == *lane && o.engine == engine(cfg))
        .unwrap_or_else(|| Observation {
            hub_id: hub.host_id.clone(),
            lane: lane.clone(),
            engine: engine(cfg),
            can_compact,
            usage: None,
            last_compaction: None,
        });
    observation.can_compact = can_compact;
    if let Some((used, size)) = usage {
        // Zero conserva a ausência de size no protocolo; nunca reaproveita a janela de outra observação.
        observation.usage =
            Some(Usage { epoch: epoch(hub, &lane.chat, acp)?, used, size, updated_at: crate::store::now_ms() });
    }
    if compacted.is_some() {
        observation.last_compaction = compacted;
    }
    hub.store.kv_set(&key(hub, &cfg.id, acp), &serde_json::to_string(&observation)?)
}

pub(crate) fn projection(target: &Target, observation: Option<Observation>, busy: bool) -> Result<Value> {
    let mut answer = json!({"hubId":target.hub_id,"botId":target.lane.chat,"sessionId":target.lane.session_id(),"spaceId":target.space_id,"epoch":target.epoch,"scopeId":target.scope_id()?});
    let Some(observation) =
        observation.filter(|o| o.hub_id == target.hub_id && o.engine == target.engine && o.lane == target.lane)
    else {
        return Ok(answer);
    };
    let Some(usage) = observation.usage.filter(|u| u.size > 0 && u.epoch == target.epoch) else { return Ok(answer) };
    let rule = codync_mem::compaction::Conversation { size: usage.size, ..Default::default() };
    let mut context = json!({"used":usage.used,"size":usage.size,"updatedAt":usage.updated_at,
        "trigger":rule.trigger(),"canCompact":observation.can_compact,"busy":busy,"contextId":target.context_id()?});
    if let Some(compaction) = observation.last_compaction {
        context["lastCompaction"] = serde_json::to_value(compaction)?;
    }
    answer["context"] = context;
    Ok(answer)
}

pub(crate) struct Manual {
    pub target: Target,
    pub read: Read,
    pub reply: tokio::sync::oneshot::Sender<Result<Value>>,
}

#[cfg(test)]
mod tests;
