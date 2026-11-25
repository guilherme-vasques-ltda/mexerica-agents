//! DON-908/C8: contabilidade observada de turnos, privada do dono.
//! MX-159: tabela versionada de preços de API estima turnos sem custo do motor.
use crate::{api::devices::Caller, hub::Hub};
use anyhow::{Result, anyhow};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

mod ledger;
mod precos;
mod report;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use ledger::record_background;
pub(crate) use ledger::{
    BACKGROUND_SESSION_PREFIX, Pending, UsoFundo, begin_background, begin_background_on, complete_background, migrate,
    turn_usage,
};
pub(crate) use ledger::{Cost, Tokens};
pub(crate) use precos::{PoliticaCache, estimar};
/// MX-145 E3: o teto usa o mesmo mês civil UTC do relatório.
pub(crate) use report::month;
pub(crate) const METHOD: &str = "donnaCostReport";

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Read {
    month: Option<String>,
    bot_id: Option<String>,
    session_id: Option<String>,
    conversation_id: Option<String>,
    space_id: Option<String>,
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    super::bot_editor::authorization::require_owner(caller)?;
    let read: Read = serde_json::from_value(body).map_err(|_| anyhow!("invalid_cost_request"))?;
    let hub = hub.clone();
    tokio::task::spawn_blocking(move || report::read(&hub, &read)).await?
}
