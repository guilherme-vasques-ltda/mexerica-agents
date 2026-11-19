//! S2a: apenas metadados. Launch não é operação/grant; Local não prova humano.
use super::PERSONAL;
pub(crate) use super::conversations::Conversation;
use crate::{
    api::devices::Caller,
    fork::{
        atividades::modelo::PlanDigest,
        escopo::{Id, Revision, Version},
    },
    hub::Hub,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartupLaunchRef {
    pub id: Id,
    pub request_digest: PlanDigest,
}
/// Captura descritiva de produtores existentes; não é `ResolvedScope`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartupExpectedContext {
    pub hub_id: String,
    pub space_id: Id,
    pub space_revision: Revision,
    pub principal_bot_id: String,
    pub primary_conversation_revision: Revision,
}
impl StartupExpectedContext {
    pub fn digest(&self) -> Result<PlanDigest, StartupError> {
        let mut hash = Sha256::new();
        hash.update(b"donnabot.startup-metadata.v1\0");
        hash.update(serde_json::to_vec(self).map_err(|_| StartupError::InvalidContract)?);
        let mut hex = String::with_capacity(64);
        for byte in hash.finalize() {
            let _ = write!(hex, "{byte:02x}");
        }
        PlanDigest::try_from(hex).map_err(|_| StartupError::InvalidContract)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartupOpenInput {
    pub version: Version,
    pub space_id: Id,
    pub launch_ref: StartupLaunchRef,
    pub expected_context: StartupExpectedContext,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartupLookupInput {
    pub version: Version,
    pub space_id: Id,
    pub launch_ref: StartupLaunchRef,
    pub expected_context: StartupExpectedContext,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum StartupConversationResult {
    Found {
        version: Version,
        hub_id: String,
        launch_ref: StartupLaunchRef,
        expected_context: StartupExpectedContext,
        conversation_id: Id,
        conversation_revision: Revision,
        closed: bool,
    },
    NotFound {
        version: Version,
        hub_id: String,
        launch_ref: StartupLaunchRef,
        expected_context: StartupExpectedContext,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupError {
    #[serde(rename = "startup_invalid_contract")]
    InvalidContract,
    #[serde(rename = "startup_context_changed")]
    ContextChanged,
    #[serde(rename = "startup_scope_unavailable")]
    ScopeUnavailable,
    #[serde(rename = "startup_intent_conflict")]
    IntentConflict,
    #[serde(rename = "startup_conversation_limit")]
    ConversationLimit,
    #[serde(rename = "startup_outcome_unknown")]
    OutcomeUnknown,
}
impl std::fmt::Display for StartupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidContract => "startup_invalid_contract",
            Self::ContextChanged => "startup_context_changed",
            Self::ScopeUnavailable => "startup_scope_unavailable",
            Self::IntentConflict => "startup_intent_conflict",
            Self::ConversationLimit => "startup_conversation_limit",
            Self::OutcomeUnknown => "startup_outcome_unknown",
        })
    }
}
impl std::error::Error for StartupError {}

pub(crate) fn validate(
    host_id: &str,
    space_id: &Id,
    launch: &StartupLaunchRef,
    context: &StartupExpectedContext,
) -> Result<(), StartupError> {
    let valid_string = |s: &str| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control);
    if !valid_string(&context.hub_id) || !valid_string(&context.principal_bot_id) {
        return Err(StartupError::InvalidContract);
    }
    if context.hub_id != host_id || space_id != &context.space_id || String::from(space_id.clone()) != PERSONAL {
        return Err(StartupError::ContextChanged);
    }
    if context.digest()? != launch.request_digest {
        return Err(StartupError::IntentConflict);
    }
    Ok(())
}
fn parse<T: serde::de::DeserializeOwned>(body: Value) -> Result<T, StartupError> {
    serde_json::from_value(body).map_err(|_| StartupError::InvalidContract)
}
pub(super) fn open(hub: &Hub, caller: &Caller, body: Value) -> anyhow::Result<Value> {
    super::super::engine_setup::require_local(caller)?;
    let input: StartupOpenInput = parse(body)?;
    let (result, created) =
        crate::fork::atividades::startup_store::open_startup_conversation(&hub.store, &hub.host_id, &input)?;
    if let Some(conversation) = created {
        super::conversations::emit(hub, &conversation);
    }
    Ok(serde_json::to_value(result)?)
}
pub(super) fn lookup(hub: &Hub, caller: &Caller, body: Value) -> anyhow::Result<Value> {
    super::super::engine_setup::require_local(caller)?;
    let input: StartupLookupInput = parse(body)?;
    Ok(serde_json::to_value(crate::fork::atividades::startup_store::lookup_startup_conversation(
        &hub.store,
        &hub.host_id,
        &input,
    )?)?)
}
/// Valores padrões conhecidos do INSERT novo; nenhum segundo read depois do commit.
pub(crate) fn created_conversation(id: String, bot: String, revision: i64) -> Conversation {
    Conversation {
        id,
        space_id: PERSONAL.into(),
        bot_id: bot,
        title: "Nova conversa".into(),
        revision,
        primary: false,
        closed: false,
        state_revision: revision,
        read_revision: 0,
        engine_account_id: None,
        engine_selection: None,
        continuity: None,
        account_revision: None,
        account_stamp: None,
    }
}

#[cfg(test)]
mod startup_tests;
