use super::{
    config::{self, Document, Policy},
    selection,
};
use crate::{api::devices::Caller, fork::engine_setup::access, hub::Hub, store::BotConfig};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) const METHODS: [&str; 4] = ["donnaRouting", "donnaRoutingSave", "donnaRoutingCheck", "donnaRoutingReceipt"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Read {
    bot_id: String,
    space_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptRead {
    #[serde(rename = "botId")]
    bot: String,
    #[serde(rename = "spaceId")]
    space: String,
    #[serde(rename = "conversationId")]
    conversation: String,
    #[serde(rename = "sessionId")]
    session: String,
    #[serde(rename = "entryId")]
    entry: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Save {
    bot_id: String,
    space_id: String,
    expected_revision: u64,
    expected_general_revision: u64,
    policy: Policy,
    general_guide: String,
}
fn parse<T: serde::de::DeserializeOwned>(body: Value) -> Result<T> {
    ensure!(body.to_string().len() <= 32_000, "invalid_field: configuração de roteamento muito grande.");
    serde_json::from_value(body).map_err(|_| anyhow!("invalid_field: solicitação de roteamento inválida."))
}
fn bot(db: &Connection, space: &str, id: &str) -> Result<BotConfig> {
    config::id(space)?;
    config::id(id)?;
    let raw: Option<String> =
        db.query_row("SELECT config FROM bots WHERE id=?1 AND deleted=0", [id], |r| r.get(0)).optional()?;
    let cfg: BotConfig =
        serde_json::from_str(&raw.ok_or_else(|| anyhow!("routing_unavailable: bot indisponível neste Space."))?)
            .map_err(|_| anyhow!("routing_unavailable: bot indisponível neste Space."))?;
    ensure!(cfg.space_id == space && !cfg.is_group(), "routing_unavailable: bot indisponível neste Space.");
    Ok(cfg)
}
fn response(db: &Connection, space: &str, id: &str) -> Result<Value> {
    let cfg = bot(db, space, id)?;
    let (key, general_key) = config::keys(space, id);
    let policy: Document<Policy> = config::load(db, &key)?;
    let general: Document<String> = config::load(db, &general_key)?;
    let accounts = selection::accounts(db, &cfg)?;
    let effective_guide = if policy.value.guide.trim().is_empty() { &general.value } else { &policy.value.guide };
    Ok(json!({"botId":id,"spaceId":space,"policy":policy,"generalGuide":general,
        "effectiveGuide":effective_guide,
        "auto":{"available":false,"reason":"executor_and_scope_unproven"},
        "reservesExecution":"unavailable","execution":"existing_fixed",
        "fixedCheck":selection::fixed(db,&cfg), "accounts":accounts}))
}
pub(super) fn save(
    db: &Connection,
    space: &str,
    id: &str,
    expected: u64,
    expected_general: u64,
    policy: Policy,
    general_guide: String,
) -> Result<()> {
    let cfg = bot(db, space, id)?;
    let (key, general_key) = config::keys(space, id);
    let before: Document<Policy> = config::load(db, &key)?;
    let general: Document<String> = config::load(db, &general_key)?;
    ensure!(
        before.revision == expected && general.revision == expected_general,
        "settings_conflict: as preferências mudaram; releia antes de salvar. Seu rascunho foi preservado."
    );
    config::guide(&general_guide)?;
    selection::policy(db, &cfg, &before.value, &policy)?;
    if !config::same(&before.value, &policy)? {
        config::save(db, &key, before, policy)?;
    }
    if general.value != general_guide {
        config::save(db, &general_key, general, general_guide)?;
    }
    Ok(())
}
pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    ensure!(METHODS.contains(&method), "invalid_field: operação de roteamento inválida.");
    // engine.accounts em todas as intenções, nunca só quando patch.forkEditor existe.
    let authority = access::resolve(hub, caller)?;
    let method = method.to_owned();
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let hub = hub.clone();
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        let mut value = authority.with_db(&hub, |db| {
            if method == "donnaRoutingSave" {
                let r: Save = parse(body)?;
                save(
                    db,
                    &r.space_id,
                    &r.bot_id,
                    r.expected_revision,
                    r.expected_general_revision,
                    r.policy,
                    r.general_guide,
                )?;
                response(db, &r.space_id, &r.bot_id)
            } else if method == "donnaRoutingReceipt" {
                let r: ReceiptRead = parse(body)?;
                super::receipt::entry_receipt(
                    db,
                    &super::receipt::ReceiptQuery {
                        bot: &r.bot,
                        space: &r.space,
                        conversation: &r.conversation,
                        session: &r.session,
                        entry: &r.entry,
                    },
                )
            } else {
                let r: Read = parse(body)?;
                response(db, &r.space_id, &r.bot_id)
            }
        })?;
        authority.require_current(&hub)?;
        value["hubId"] = hub.host_id.clone().into();
        Ok(value)
    })
    .await?
}
