//! Consumidor PL1 fechado: preparo/posse próprios; produtor root e uso são separados.
mod ceremony;
pub(crate) mod channel;
mod clock;
mod enrollment;
mod epoch;
mod fences;
mod helper;
pub(crate) mod live;
mod manifest;
pub(crate) mod prepare;
mod prepare_wire;
pub(crate) mod probe;
pub(crate) mod provisioning;
pub(crate) mod relay;
mod runtime;
mod secure_fs;
mod sync;
mod transport;
pub(crate) use runtime::Runtime;
#[cfg(test)]
mod possession_tests;
mod protocol;
mod shapes;
#[cfg(test)]
mod tests;
mod wire;

use crate::api::devices::{self, Caller};
use anyhow::{Result, ensure};
use serde_json::{Value, json};

pub(crate) const STATUS_METHOD: &str = "donnaPersonalBootstrapStatus";

pub(crate) fn status(caller: &Caller, body: &Value) -> Result<Value> {
    devices::permit(caller, STATUS_METHOD)?;
    ensure!(matches!(caller, Caller::Local), "Este pedido exige o acesso local do app.");
    ensure!(
        body.as_object().is_some_and(serde_json::Map::is_empty),
        "Não foi possível conferir este pedido de vínculo."
    );
    let admitted = manifest::admitted().ok().flatten();
    let (profile, wire, reason) = if let Some(context) = admitted {
        (context.0[0].clone(), context.0[1].clone(), "helperRequired")
    } else {
        (json!(protocol::PROFILE), json!(1), "updateRequired")
    };
    // Admissão de manifesto não comprova helper, origem, consentimentos ou canal.
    Ok(json!({"profile":profile,"wire":wire,"available":false,"reason":reason}))
}

/// Até C4, PL1 não admite uso genérico. A origem durável não some com V/lease/DeviceSource.
pub(crate) fn guard_generic(hub: &crate::hub::Hub, caller: &Caller) -> Result<()> {
    if let Caller::Device { key, .. } = caller
        && hub.store.personal_origin(key)?.is_some()
    {
        return Err(devices::Forbidden.into());
    }
    Ok(())
}
