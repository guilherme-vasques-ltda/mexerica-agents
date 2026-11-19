use super::{
    rule::{ApprovalLevel, Effect, Rule, SendScope},
    store,
};
use crate::{api::devices::Caller, hub::Hub};
use anyhow::{Result, bail};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{ops::ControlFlow, sync::Arc};

pub const METHODS: &[&str] =
    &["donnaRules", "donnaApprovalSet", "donnaRuleSave", "donnaRuleSetEnabled", "donnaRuleDelete"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Save {
    id: Option<String>,
    effect: Effect,
    field: String,
    op: String,
    value: String,
    bot_id: Option<String>,
    tool: Option<String>,
    scope: Option<SendScope>,
    #[serde(default = "enabled")]
    enabled: bool,
    expected_revision: i64,
}
fn enabled() -> bool {
    true
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetEnabled {
    id: String,
    enabled: bool,
    expected_revision: i64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Delete {
    id: String,
    expected_revision: i64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetApproval {
    bot_id: Option<String>,
    level: ApprovalLevel,
    #[serde(default)]
    dangerous_categories: Vec<String>,
    expected_revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {}
fn parse<T: DeserializeOwned>(body: Value) -> Result<T> {
    serde_json::from_value(body).map_err(|_| anyhow::anyhow!("invalid_field: solicitação de regras inválida."))
}

pub async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<ControlFlow<Value, Value>> {
    if !METHODS.contains(&method) {
        return Ok(ControlFlow::Continue(body));
    }
    if !matches!(caller, Caller::Local) {
        return Err(crate::api::devices::Forbidden.into());
    }
    // Não há método MCP, nem flag JSON capaz de conceder autoria. D62 continua valendo.
    let hub = hub.clone();
    let method = method.to_owned();
    let response=tokio::task::spawn_blocking(move||->Result<Value>{
      Ok(match method.as_str(){
        "donnaRules"=>{
            let _:Read=parse(body)?;
            let snapshot=hub.store.donna_rules()?;
            let mut response=super::levels::contract();
            response["rules"]=snapshot.rules.iter().map(store::rule_value).collect::<Result<Vec<_>>>()?.into();
            response["general"]=serde_json::to_value(snapshot.approvals.iter().find(|s|s.bot_id.is_none()).ok_or_else(||anyhow::anyhow!("policy_unavailable: estado geral indisponível."))?.level)?;
            response["approvals"]=serde_json::to_value(snapshot.approvals)?;
            response["revision"]=snapshot.revision.into();response
        },
        "donnaRuleSave"=>{
            let s:Save=parse(body)?; let creating=s.id.is_none();
            let rule=Rule{id:s.id.unwrap_or_default(),effect:s.effect,field:s.field,op:s.op,value:s.value.trim().to_owned(),bot_id:s.bot_id,tool:s.tool,scope:s.scope,
                revision:0,enabled:s.enabled,created_at:0,updated_at:0};
            json!({"rule":store::rule_value(&hub.store.donna_rule_save(rule,s.expected_revision,creating)?)?,"runtimeActive":true})
        },
        "donnaRuleSetEnabled"=>{let s:SetEnabled=parse(body)?;json!({"rule":store::rule_value(&hub.store.donna_rule_enabled(&s.id,s.enabled,s.expected_revision)?)?,"runtimeActive":true})},
        "donnaRuleDelete"=>{let s:Delete=parse(body)?;hub.store.donna_rule_delete(&s.id,s.expected_revision)?;json!({"runtimeActive":true})},
        "donnaApprovalSet"=>{
            let s:SetApproval=parse(body)?;
            if !s.dangerous_categories.is_empty(){bail!("not_available: seletores perigosos dependem de prova do chão por motor.");}
            json!({"state":hub.store.donna_approval_set(s.bot_id.as_deref(),s.level,s.expected_revision)?,"runtimeActive":true,"application":"permission_requests"})
        },
        _=>unreachable!("métodos filtrados acima"),
      })
    }).await??;
    Ok(ControlFlow::Break(response))
}
