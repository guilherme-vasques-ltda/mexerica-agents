mod model;
pub(crate) mod store;
#[cfg(test)]
mod tests;

use crate::{
    api::devices::{self, Caller, Forbidden},
    fork::{
        autoridade::{Autoridade, OrigemSaida, com_saida_validada},
        control_tower, spaces,
    },
    hub::Hub,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) use store::migrate;

pub(crate) const METHODS: &[&str] = &["donnaGovernance", "donnaGovernanceSave"];
const READ: &str = METHODS[0];
const SAVE: &str = METHODS[1];
pub(crate) const EVENT: &str = "donnaGovernanceChanged";

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    call_with_commit_boundary(hub, caller, method, body, || {}).await
}

async fn call_with_commit_boundary(
    hub: &Arc<Hub>,
    caller: &Caller,
    method: &str,
    body: Value,
    after_commit: impl FnOnce() + Send + 'static,
) -> Result<Value> {
    devices::permit(caller, method)?;
    ensure!(METHODS.contains(&method), "invalid_field: método de governança inválido.");
    let authority = Autoridade::capturar(hub, caller)?;
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let hub = hub.clone();
    let caller = caller.clone();
    let save = method == SAVE;
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        control_tower::require(&hub, &caller)?;
        let request = model::Request::parse(body, save)?;
        let (hub_id, space) = request.coords();
        if hub_id != hub.host_id {
            return Err(Forbidden.into());
        }
        if save {
            crate::fork::donna_fixa::pedido::antes_de_gravar(&hub.store);
        }
        let (answer, changed) = authority.with_db(&hub, None, |db| match &request {
            model::Request::Read(_) => Ok((serde_json::to_value(store::snapshot(db, hub_id, space)?)?, false)),
            model::Request::Save(save) => store::save(db, hub_id, space, save),
        })?;
        if changed {
            after_commit();
            let _ = hub.events.send(invalidation(&hub, space));
        }
        com_saida_validada(
            &hub,
            OrigemSaida::Controle(&authority, None),
            |db, _, _, _, _| spaces::require_space(db, space),
            || answer,
        )
    })
    .await?
}

fn invalidation(hub: &Hub, space: &str) -> Value {
    json!({"type":EVENT,"hubId":hub.host_id,"spaceId":space})
}

pub(crate) fn event(hub: &Hub, caller: &Caller, space: &str, input: &Value) -> Option<Value> {
    if input["type"] != EVENT || input["hubId"] != hub.host_id || input["spaceId"] != space {
        return None;
    }
    devices::permit(caller, READ).ok()?;
    control_tower::require(hub, caller).ok()?;
    let authority = Autoridade::capturar(hub, caller).ok()?;
    com_saida_validada(
        hub,
        OrigemSaida::Controle(&authority, None),
        |db, _, _, _, _| spaces::require_space(db, space),
        || invalidation(hub, space),
    )
    .ok()
}
