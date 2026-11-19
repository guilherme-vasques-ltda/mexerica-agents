//! S2a: primitivas tipadas sobre a mesma conexão/`StoreTx` P1, sem `Entry` ou execução.
use super::{
    modelo::CoreError,
    store::{StoreTx, with_store_tx},
};
use crate::{
    fork::{
        escopo::{Id, Revision, Version},
        spaces::{
            PERSONAL, Snapshot,
            startup::{
                self, Conversation, StartupConversationResult, StartupError, StartupExpectedContext, StartupLaunchRef,
                StartupLookupInput, StartupOpenInput,
            },
        },
    },
    store::{BotConfig, Store, TeamSettings},
};
use rusqlite::params;
use serde::de::DeserializeOwned;

type Opened = (StartupConversationResult, Option<Conversation>);
fn decode<T: DeserializeOwned>(raw: &str) -> Result<T, CoreError> {
    serde_json::from_str(raw).map_err(|_| CoreError::Storage)
}
fn domain<T>(value: Result<T, StartupError>, rejected: &mut Option<StartupError>) -> Result<T, CoreError> {
    value.map_err(|error| {
        *rejected = Some(error);
        CoreError::ScopeUnavailable
    })
}
fn revision(value: i64) -> Result<Revision, CoreError> {
    u64::try_from(value).ok().and_then(|v| Revision::try_from(v).ok()).ok_or(CoreError::Storage)
}
/// Resolve somente os vínculos nativos necessários à criação de metadados.
fn revalidate(
    tx: &mut StoreTx<'_>,
    context: &StartupExpectedContext,
    rejected: &mut Option<StartupError>,
) -> Result<(), CoreError> {
    let raw: Option<String> = tx.scalar("SELECT (SELECT v FROM kv WHERE k='donna.spaces.v1')", [])?;
    let snapshot: Snapshot = decode(&raw.ok_or(CoreError::Storage)?)?;
    if snapshot.version != 1
        || snapshot.personal_space_id != PERSONAL
        || snapshot.spaces.iter().filter(|s| s.id == PERSONAL).count() != 1
    {
        return Err(CoreError::Storage);
    }
    revision(snapshot.revision)?;
    let space = snapshot.spaces.iter().find(|s| s.id == PERSONAL).ok_or(CoreError::Storage)?;
    if space.kind != "personal" {
        return Err(CoreError::Storage);
    }
    if revision(space.revision)? != context.space_revision {
        return domain(Err(StartupError::ContextChanged), rejected);
    }
    let raw: Option<String> = tx.scalar("SELECT (SELECT v FROM kv WHERE k='team.settings.v1')", [])?;
    let settings: TeamSettings = match raw {
        Some(raw) => decode(&raw)?,
        None => return domain(Err(StartupError::ScopeUnavailable), rejected),
    };
    match settings.primary_bot_id.as_deref() {
        Some(id) if id == context.principal_bot_id => {}
        Some(_) => return domain(Err(StartupError::ContextChanged), rejected),
        None => return domain(Err(StartupError::ScopeUnavailable), rejected),
    }
    let raw: Option<String> =
        tx.scalar("SELECT (SELECT config FROM bots WHERE id=?1 AND deleted=0)", [&context.principal_bot_id])?;
    let Some(raw) = raw else { return domain(Err(StartupError::ScopeUnavailable), rejected) };
    let bot: BotConfig = decode(&raw)?;
    if bot.id != context.principal_bot_id || bot.space_id != PERSONAL || bot.is_group() {
        return domain(Err(StartupError::ScopeUnavailable), rejected);
    }
    let raw: Option<String> = tx.scalar(
        "SELECT (SELECT json_array(revision,is_primary,closed) FROM donna_space_conversations WHERE id=?1 AND bot_id=?1 AND space_id=?2)",
        params![context.principal_bot_id, PERSONAL],
    )?;
    let Some(raw) = raw else { return domain(Err(StartupError::ScopeUnavailable), rejected) };
    let (rev, primary, closed): (i64, i64, i64) = decode(&raw)?;
    if !matches!(primary, 0 | 1) || !matches!(closed, 0 | 1) {
        return Err(CoreError::Storage);
    }
    if primary != 1 || closed != 0 {
        return domain(Err(StartupError::ScopeUnavailable), rejected);
    }
    if revision(rev)? != context.primary_conversation_revision {
        return domain(Err(StartupError::ContextChanged), rejected);
    }
    Ok(())
}
fn found(
    tx: &mut StoreTx<'_>,
    launch: &StartupLaunchRef,
    context: &StartupExpectedContext,
    rejected: &mut Option<StartupError>,
) -> Result<Option<StartupConversationResult>, CoreError> {
    let raw: Option<String> = tx.scalar(
        "SELECT (SELECT json_array(id,revision,closed,startup_request_digest) FROM donna_space_conversations WHERE startup_hub_id=?1 AND startup_launch_id=?2 AND space_id=?3 AND bot_id=?4)",
        params![context.hub_id, String::from(launch.id.clone()), PERSONAL, context.principal_bot_id],
    )?;
    let Some(raw) = raw else {
        // Os dois entrypoints já revalidaram o contexto nesta mesma Tx.
        // Só existência da chave exata; não carrega dados do outro vínculo.
        let occupied: bool = tx.scalar(
            "SELECT EXISTS(SELECT 1 FROM donna_space_conversations WHERE startup_hub_id=?1 AND startup_launch_id=?2)",
            params![context.hub_id, String::from(launch.id.clone())],
        )?;
        if occupied {
            return domain(Err(StartupError::IntentConflict), rejected);
        }
        return Ok(None);
    };
    let (id, rev, closed, digest): (String, i64, i64, String) = decode(&raw)?;
    if id == context.principal_bot_id || !matches!(closed, 0 | 1) {
        return Err(CoreError::Storage);
    }
    let id = Id::try_from(id).map_err(|_| CoreError::Storage)?;
    let digest = super::modelo::PlanDigest::try_from(digest).map_err(|_| CoreError::Storage)?;
    if digest != launch.request_digest {
        return domain(Err(StartupError::IntentConflict), rejected);
    }
    Ok(Some(result(launch, context, id, revision(rev)?, closed == 1)))
}
fn result(
    launch: &StartupLaunchRef,
    context: &StartupExpectedContext,
    id: Id,
    revision: Revision,
    closed: bool,
) -> StartupConversationResult {
    StartupConversationResult::Found {
        version: Version,
        hub_id: context.hub_id.clone(),
        launch_ref: launch.clone(),
        expected_context: context.clone(),
        conversation_id: id,
        conversation_revision: revision,
        closed,
    }
}
pub(crate) fn open_startup_conversation(
    store: &Store,
    host_id: &str,
    input: &StartupOpenInput,
) -> Result<Opened, StartupError> {
    let mut rejected = None;
    let outcome = with_store_tx(store, |tx| {
        domain(startup::validate(host_id, &input.space_id, &input.launch_ref, &input.expected_context), &mut rejected)?;
        let context = &input.expected_context;
        revalidate(tx, context, &mut rejected)?;
        if let Some(existing) = found(tx, &input.launch_ref, context, &mut rejected)? {
            return Ok((existing, None));
        }
        let count: i64 = tx.scalar("SELECT COUNT(*) FROM donna_space_conversations WHERE space_id=?1", [PERSONAL])?;
        if count >= 1024 {
            return domain(Err(StartupError::ConversationLimit), &mut rejected);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let typed_id = Id::try_from(id.clone()).map_err(|_| CoreError::Storage)?;
        let rev = tx.next_revision()?;
        let typed_rev = revision(rev)?;
        let changed = tx.execute(
            "INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary,startup_hub_id,startup_launch_id,startup_request_digest) VALUES(?1,?2,?3,'Nova conversa',?4,0,?5,?6,?7)",
            params![id, PERSONAL, context.principal_bot_id, rev, host_id, String::from(input.launch_ref.id.clone()), String::from(input.launch_ref.request_digest.clone())],
        )?;
        if changed != 1 {
            return Err(CoreError::Storage);
        }
        tx.ensure_session_principal(&id)?;
        Ok((
            result(&input.launch_ref, context, typed_id, typed_rev, false),
            Some(startup::created_conversation(id, context.principal_bot_id.clone(), rev)),
        ))
    });
    outcome.map_err(|_| rejected.unwrap_or(StartupError::OutcomeUnknown))
}
pub(crate) fn lookup_startup_conversation(
    store: &Store,
    host_id: &str,
    input: &StartupLookupInput,
) -> Result<StartupConversationResult, StartupError> {
    let mut rejected = None;
    let outcome = with_store_tx(store, |tx| {
        domain(startup::validate(host_id, &input.space_id, &input.launch_ref, &input.expected_context), &mut rejected)?;
        revalidate(tx, &input.expected_context, &mut rejected)?;
        Ok(found(tx, &input.launch_ref, &input.expected_context, &mut rejected)?.unwrap_or_else(|| {
            StartupConversationResult::NotFound {
                version: Version,
                hub_id: host_id.into(),
                launch_ref: input.launch_ref.clone(),
                expected_context: input.expected_context.clone(),
            }
        }))
    });
    outcome.map_err(|_| rejected.unwrap_or(StartupError::OutcomeUnknown))
}
