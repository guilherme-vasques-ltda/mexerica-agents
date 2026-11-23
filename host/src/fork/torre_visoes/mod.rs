mod model;
mod projection;

use crate::{
    api::devices::{self, Caller, Forbidden},
    fork::{
        autoridade::{Autoridade, OrigemSaida, com_saida_validada},
        control_tower, spaces,
    },
    hub::Hub,
};
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;

pub(crate) const METHOD: &str = "donnaTowerView";

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    devices::permit(caller, METHOD)?;
    let authority = Autoridade::capturar(hub, caller)?;
    let read: model::Read = serde_json::from_value(body)
        .map_err(|_| anyhow::anyhow!("invalid_field: solicitação de visão da Torre inválida."))?;
    if read.hub_id.as_ref().is_some_and(|id| id != &hub.host_id) {
        return Err(Forbidden.into());
    }
    read.validate()?;
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let hub = hub.clone();
    let caller = caller.clone();
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        control_tower::require(&hub, &caller)?;
        let space = read.space_id.clone();
        let answer = authority.with_db(&hub, None, |db| {
            spaces::require_space(db, &space)?;
            let snapshot = projection::snapshot(db, &hub.host_id, read)?;
            Ok(serde_json::to_value(snapshot)?)
        })?;
        control_tower::require(&hub, &caller)?;
        com_saida_validada(
            &hub,
            OrigemSaida::Controle(&authority, None),
            |db, _, _, _, _| spaces::require_space(db, &space),
            || answer,
        )
    })
    .await?
}

#[cfg(test)]
mod tests;
