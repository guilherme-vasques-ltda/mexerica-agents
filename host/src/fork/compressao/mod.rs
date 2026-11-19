//! DON-906/C3 e DON-910: política do Hub e pausa durável por conversa visível.
use crate::{
    api::devices::Caller,
    hub::Hub,
    store::{Lane, Store},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const METHODS: &[&str] = &["donnaContextSettings", "donnaContextSettingsSave"];
const SETTINGS_KEY: &str = "donna.compaction.settings";

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Idle {
    #[default]
    BeforeExpiry,
    Never,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Settings {
    pub trigger_tokens: Option<u64>,
    pub idle: Idle,
}

impl Settings {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.trigger_tokens.is_none_or(|n| (50_000..=9_007_199_254_740_991).contains(&n) && n % 1000 == 0),
            "invalid_context_threshold"
        );
        Ok(())
    }
}

pub(crate) fn settings(store: &Store) -> Result<Settings> {
    let value: Settings =
        store.kv_read(SETTINGS_KEY)?.map(|s| serde_json::from_str(&s)).transpose()?.unwrap_or_default();
    value.validate()?;
    Ok(value)
}

pub(crate) fn call(hub: &Hub, caller: &Caller, method: &str, mut body: Value) -> Result<Value> {
    crate::fork::bot_editor::authorization::require_owner(caller)?;
    if let Some(fields) = body.as_object_mut() {
        fields.remove("spaceId");
    }
    if method == "donnaContextSettingsSave" {
        let value: Settings = serde_json::from_value(body)?;
        value.validate()?;
        let raw = serde_json::to_string(&value)?;
        // D368: política que o Berga usa; do app pareado, só com a autoridade original na transação.
        crate::fork::donna_fixa::pedido::gravar(&hub.store, |tx| {
            tx.execute(
                "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
                [SETTINGS_KEY, raw.as_str()],
            )?;
            Ok(())
        })?;
    } else {
        ensure!(body.as_object().is_some_and(serde_json::Map::is_empty), "invalid_context_settings_request");
    }
    Ok(serde_json::to_value(settings(&hub.store)?)?)
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Failures {
    pub consecutive: u32,
}

impl Failures {
    pub fn paused(&self) -> bool {
        self.consecutive >= 2
    }
}

fn key(bot: &str, lane: &Lane) -> Result<String> {
    let id = crate::agent::registry::sha256_hex(&serde_json::to_vec(&(bot, lane))?);
    Ok(format!("donna.compaction.failures.{id}"))
}

pub(crate) fn failures(store: &Store, bot: &str, lane: &Lane) -> Result<Failures> {
    Ok(store.kv_read(&key(bot, lane)?)?.map(|s| serde_json::from_str(&s)).transpose()?.unwrap_or_default())
}

pub(crate) fn failed(store: &Store, bot: &str, lane: &Lane) -> Result<()> {
    let mut state = failures(store, bot, lane)?;
    state.consecutive = state.consecutive.saturating_add(1);
    store.kv_set(&key(bot, lane)?, &serde_json::to_string(&state)?)
}

pub(crate) fn reset(store: &Store, bot: &str, lane: &Lane) -> Result<()> {
    store.kv_set(&key(bot, lane)?, &serde_json::to_string(&Failures::default())?)
}

#[cfg(test)]
mod tests;
