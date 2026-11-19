//! Estado público da tela por Space; broker e permissões continuam em Screen.
use crate::hub::Hub;
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum AgentUse {
    Idle,
    Using,
    Unavailable,
}

/// Fonte única para respostas e eventos. A cópia interna nunca é entregue sem projeção.
pub(crate) fn state(hub: &Hub, space: &str) -> Value {
    let mut value = hub.screen.state();
    let usage = match value.get("agentBot") {
        _ if hub.store.require_space(space).is_err() => AgentUse::Unavailable,
        Some(Value::Null) => AgentUse::Idle,
        Some(Value::String(id)) if visible(hub, space, id) => AgentUse::Using,
        _ => AgentUse::Unavailable,
    };
    if !matches!(usage, AgentUse::Using) {
        value["agentBot"] = Value::Null;
    }
    value["agentUse"] = json!(usage);
    value
}

fn visible(hub: &Hub, space: &str, id: &str) -> bool {
    let Some(row) = hub.store.bot(id).ok().flatten().filter(|row| !row.deleted && !row.config.is_group()) else {
        return false;
    };
    let Some(rt) = hub.observed_runtime(id) else { return false };
    let Some(lane) = rt.lane else { return false };
    super::audience_in_space(hub, &row.config, space, Some(&lane.chat), false).unwrap_or(false)
}

/// A posse pode continuar ativa após mudar a audiência. Atualiza o evento existente;
/// grupos também afetam a lane do agente, sem expor o destino na projeção privada.
pub(crate) fn changed(hub: &Hub, id: &str) {
    let raw = hub.screen.state();
    let Some(agent) = raw["agentBot"].as_str() else { return };
    if agent == id || hub.observed_runtime(agent).and_then(|rt| rt.lane).is_some_and(|lane| lane.chat == id) {
        hub.screen.emit();
    }
}
