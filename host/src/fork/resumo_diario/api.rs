//! RPC do dono para o horário e o liga/desliga do resumo do dia; mesma guarda do Mission Board.
use crate::{api::devices::Caller, fork::spaces::PERSONAL, hub::Hub};
use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) const METHOD: &str = "donnaResumoDiarioSave";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Save {
    enabled: bool,
    time: String,
    time_zone: String,
}

pub(crate) async fn call(hub: &Hub, caller: &Caller, body: Value) -> Result<Value> {
    let request: Save = serde_json::from_value(body)
        .map_err(|_| anyhow::anyhow!("invalid_field: solicitação do resumo do dia inválida ou campo desconhecido."))?;
    let _guard = hub.store.team_lock.lock().await;
    crate::fork::control_tower::require(hub, caller)?;
    let saved = super::store::salvar(&hub.store, request.enabled, &request.time, &request.time_zone)?;
    let _ =
        hub.events.send(json!({"type": crate::fork::control_tower::EVENT, "spaceId": PERSONAL, "rosterChanged": true}));
    Ok(serde_json::to_value(saved)?)
}
