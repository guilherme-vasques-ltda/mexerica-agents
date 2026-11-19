//! M1a1/05a: adaptador tipado. Metadados técnicos, barreira e política pessoal; SQL não sai daqui.
use super::{
    modelo::CoreError,
    store::{StoreTx, with_store_tx},
};
use crate::{
    api::devices::Caller,
    fork::{
        escopo::{
            Id, Revision,
            fontes::{PersonalOrigin, ScopeSourceError, SourceInvalidationReason},
        },
        sessoes::store::SessionRef,
        spaces::PERSONAL,
    },
    hub::Hub,
    store::{BotKind, Scope, now_ms},
};
use rusqlite::params;
use serde::{Deserialize, de::DeserializeOwned};
use std::sync::atomic::Ordering;

/// Selector descritivo, não é ACL, `ResolvedScope` ou principal humano.
pub(crate) struct PersonalBinding {
    pub(crate) hub_id: String,
    pub(crate) space_id: String,
    pub(crate) bot_id: String,
    pub(crate) conversation_id: String,
    pub(crate) space_revision: Revision,
    pub(crate) bot_revision: Revision,
    pub(crate) conversation_revision: Revision,
    /// Vinculação pessoal fechada: o domínio de privacidade é o Space pessoal e a sua revisão.
    pub(crate) privacy_domain_id: String,
    pub(crate) domain_revision: Revision,
    pub(crate) primary: bool,
}
enum BarrierState {
    Uninitialized,
    Invalidated(SourceInvalidationReason),
}
pub(crate) struct SourceBarrier {
    revision: Revision,
    state: BarrierState,
}
impl SourceBarrier {
    pub(crate) fn denial(&self) -> ScopeSourceError {
        match &self.state {
            BarrierState::Uninitialized => ScopeSourceError::SourcesUninitialized,
            BarrierState::Invalidated(_) => ScopeSourceError::SourcesInvalidated,
        }
    }
}

// Projeções somente dos vínculos. Nome, aparência e permission legado não são autoridade.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpacesBinding {
    version: u32,
    personal_space_id: String,
    revision: i64,
    spaces: Vec<SpaceBinding>,
}
#[derive(Deserialize)]
struct SpaceBinding {
    id: String,
    kind: String,
    revision: i64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrimaryBot {
    primary_bot_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BotBinding {
    id: String,
    space_id: String,
    kind: BotKind,
}

fn decode<T: DeserializeOwned>(raw: &str) -> Result<T, CoreError> {
    serde_json::from_str(raw).map_err(|_| CoreError::Storage)
}
fn revision(value: i64) -> Result<Revision, CoreError> {
    u64::try_from(value).ok().and_then(|v| Revision::try_from(v).ok()).ok_or(CoreError::Storage)
}
fn reject<T>(error: ScopeSourceError, rejected: &mut Option<ScopeSourceError>) -> Result<T, CoreError> {
    *rejected = Some(error);
    Err(CoreError::ScopeUnavailable)
}
fn ingress(
    tx: &mut StoreTx<'_>,
    hub: &Hub,
    caller: &Caller,
    rejected: &mut Option<ScopeSourceError>,
) -> Result<(), CoreError> {
    let key = match caller {
        Caller::Local => return Ok(()), // Só ingresso técnico; nenhuma fonte positiva abaixo.
        Caller::Device { key, scopes, .. } if scopes.contains(&Scope::Control) => key,
        _ => return reject(ScopeSourceError::IngressDenied, rejected),
    };
    let raw: Option<String> =
        tx.scalar("SELECT (SELECT json_array(source,scopes,lease_until) FROM devices WHERE key=?1)", [key])?;
    let Some(raw) = raw else { return reject(ScopeSourceError::IngressDenied, rejected) };
    let (source, scopes, lease): (String, String, Option<i64>) = decode(&raw)?;
    let scopes: Vec<Scope> = decode(&scopes)?;
    let eligible = match source.as_str() {
        "local" => true,
        "account" => {
            let now = now_ms();
            now > 0 && hub.cloud_synced.load(Ordering::Acquire) && lease.is_some_and(|until| until > now)
        }
        _ => return Err(CoreError::Storage),
    };
    if !eligible || !scopes.contains(&Scope::Control) {
        return reject(ScopeSourceError::IngressDenied, rejected);
    }
    Ok(())
}
fn revalidate(
    tx: &mut StoreTx<'_>,
    hub: &Hub,
    caller: &Caller,
    binding: &PersonalBinding,
    rejected: &mut Option<ScopeSourceError>,
) -> Result<(), CoreError> {
    ingress(tx, hub, caller, rejected)?;
    Id::try_from(hub.host_id.clone()).map_err(|_| CoreError::Storage)?;
    if binding.hub_id != hub.host_id || binding.space_id != PERSONAL {
        return reject(ScopeSourceError::ContextChanged, rejected);
    }
    let raw: Option<String> = tx.scalar("SELECT (SELECT v FROM kv WHERE k='host_id')", [])?;
    let Some(host_id) = raw else { return reject(ScopeSourceError::ContextUnavailable, rejected) };
    Id::try_from(host_id.clone()).map_err(|_| CoreError::Storage)?;
    if host_id != hub.host_id {
        return reject(ScopeSourceError::ContextChanged, rejected);
    }
    let raw: Option<String> = tx.scalar("SELECT (SELECT v FROM kv WHERE k='donna.spaces.v1')", [])?;
    let Some(raw) = raw else { return reject(ScopeSourceError::ContextUnavailable, rejected) };
    let snapshot: SpacesBinding = decode(&raw)?;
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
    if revision(space.revision)? != binding.space_revision {
        return reject(ScopeSourceError::ContextChanged, rejected);
    }
    // Domínio escolhido pelo chamador não sai embrulhado: só o Space pessoal na revisão atual.
    if binding.privacy_domain_id != PERSONAL || binding.domain_revision != binding.space_revision {
        return reject(ScopeSourceError::ContextChanged, rejected);
    }
    let raw: Option<String> = tx.scalar("SELECT (SELECT v FROM kv WHERE k='team.settings.v1')", [])?;
    let Some(raw) = raw else { return reject(ScopeSourceError::ContextUnavailable, rejected) };
    let settings: PrimaryBot = decode(&raw)?;
    match settings.primary_bot_id.as_deref() {
        Some(id) if id == binding.bot_id => {}
        Some(_) => return reject(ScopeSourceError::ContextChanged, rejected),
        None => return reject(ScopeSourceError::ContextUnavailable, rejected),
    }
    let raw: Option<String> =
        tx.scalar("SELECT (SELECT json_array(config,rev) FROM bots WHERE id=?1 AND deleted=0)", [&binding.bot_id])?;
    let Some(raw) = raw else { return reject(ScopeSourceError::ContextUnavailable, rejected) };
    let (config, rev): (String, i64) = decode(&raw)?;
    let bot: BotBinding = decode(&config)?;
    if bot.id != binding.bot_id || bot.space_id != PERSONAL || bot.kind != BotKind::Agent {
        return reject(ScopeSourceError::ContextUnavailable, rejected);
    }
    if revision(rev)? != binding.bot_revision {
        return reject(ScopeSourceError::ContextChanged, rejected);
    }
    let raw: Option<String> = tx.scalar(
        "SELECT (SELECT json_array(revision,is_primary,closed) FROM donna_space_conversations WHERE id=?1 AND space_id=?2 AND bot_id=?3)",
        params![binding.conversation_id, PERSONAL, binding.bot_id],
    )?;
    let Some(raw) = raw else { return reject(ScopeSourceError::ContextUnavailable, rejected) };
    let (rev, primary, closed): (i64, i64, i64) = decode(&raw)?;
    if !matches!(primary, 0 | 1) || !matches!(closed, 0 | 1) {
        return Err(CoreError::Storage);
    }
    if closed != 0
        || (primary == 1) != binding.primary
        || (binding.conversation_id == binding.bot_id) != binding.primary
    {
        return reject(ScopeSourceError::ContextUnavailable, rejected);
    }
    if revision(rev)? != binding.conversation_revision {
        return reject(ScopeSourceError::ContextChanged, rejected);
    }
    Ok(())
}
fn barrier(tx: &mut StoreTx<'_>) -> Result<SourceBarrier, CoreError> {
    let raw: Option<String> = tx.scalar(
        "SELECT (SELECT json_array(version,revision,state,reason) FROM donna_scope_authority_state WHERE singleton=1 AND (SELECT COUNT(*) FROM donna_scope_authority_state)=1)",
        [],
    )?;
    let (version, rev, state, reason): (u32, i64, String, Option<String>) = decode(&raw.ok_or(CoreError::Storage)?)?;
    if version != 1 {
        return Err(CoreError::Storage);
    }
    let state = match (state.as_str(), reason.as_deref()) {
        ("uninitialized", None) => BarrierState::Uninitialized,
        ("invalidated", Some(reason)) => {
            BarrierState::Invalidated(SourceInvalidationReason::parse(reason).ok_or(CoreError::Storage)?)
        }
        _ => return Err(CoreError::Storage),
    };
    Ok(SourceBarrier { revision: revision(rev)?, state })
}
pub(crate) fn personal_sources(
    hub: &Hub,
    caller: &Caller,
    binding: &PersonalBinding,
) -> Result<SourceBarrier, ScopeSourceError> {
    let mut rejected = None;
    with_store_tx(&hub.store, |tx| {
        revalidate(tx, hub, caller, binding, &mut rejected)?;
        barrier(tx)
    })
    .map_err(|_| rejected.unwrap_or(ScopeSourceError::OutcomeUnknown))
}
pub(crate) fn invalidate_personal_sources(
    hub: &Hub,
    caller: &Caller,
    binding: &PersonalBinding,
    expected: Revision,
    reason: SourceInvalidationReason,
) -> Result<Revision, ScopeSourceError> {
    let mut rejected = None;
    with_store_tx(&hub.store, |tx| {
        revalidate(tx, hub, caller, binding, &mut rejected)?;
        if barrier(tx)?.revision != expected {
            return reject(ScopeSourceError::StateConflict, &mut rejected);
        }
        let expected_sql = i64::try_from(u64::from(expected)).map_err(|_| CoreError::Storage)?;
        let next = tx.next_revision()?;
        let typed = revision(next)?;
        if u64::from(typed) <= u64::from(expected) {
            return Err(CoreError::Storage);
        }
        let changed = tx.execute(
            "UPDATE donna_scope_authority_state SET revision=?1,state='invalidated',reason=?2 WHERE singleton=1 AND version=1 AND revision=?3",
            params![next, reason.as_str(), expected_sql],
        )?;
        if changed != 1 {
            return reject(ScopeSourceError::StateConflict, &mut rejected);
        }
        Ok(typed)
    })
    .map_err(|_| rejected.unwrap_or(ScopeSourceError::OutcomeUnknown))
}

// 05a: fonte positiva mínima. Política fechada e única; a revisão vem da sequência do Store.
const POLICY: &str = "personal_private_read_v1";

/// Fontes positivas atuais. Dado interno, não credencial: só `fork::escopo` o embrulha.
/// `principal_bot_id` é o bot principal (`team.settings.v1`), nunca pessoa ou prova humana.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PersonalSources {
    principal_bot_id: Id,
    policy_revision: Revision,
    recovery_epoch: Id,
    barrier_revision: Revision,
    session: SessionRef,
}

/// Leitura privada na Tx já revalidada: só leituras tipadas (SELECT fixo), nunca SQL do
/// cliente/modelo nem escrita. O vínculo é o mesmo que acabou de ser revalidado.
pub(crate) struct PrivateReadTx<'a, 'db> {
    tx: &'a mut StoreTx<'db>,
    binding: &'a PersonalBinding,
}

struct Activation {
    barrier_revision: Revision,
    principal_bot_id: Id,
    policy_revision: Revision,
    recovery_epoch: Id,
}
fn activation(tx: &mut StoreTx<'_>) -> Result<Option<Activation>, CoreError> {
    let raw: Option<String> = tx.scalar(
        "SELECT (SELECT json_array(version,barrier_revision,principal_bot_id,policy,policy_revision,recovery_epoch) FROM donna_scope_personal_activation WHERE singleton=1)",
        [],
    )?;
    let Some(raw) = raw else { return Ok(None) };
    let (version, barrier, principal, policy, policy_revision, epoch): (u32, i64, String, String, i64, String) =
        decode(&raw)?;
    if version != 1 || policy != POLICY {
        return Err(CoreError::Storage);
    }
    Ok(Some(Activation {
        barrier_revision: revision(barrier)?,
        principal_bot_id: Id::try_from(principal).map_err(|_| CoreError::Storage)?,
        policy_revision: revision(policy_revision)?,
        recovery_epoch: Id::try_from(epoch).map_err(|_| CoreError::Storage)?,
    }))
}
/// Identidade canônica de Sessões; o id do app é só expectativa conferida no catálogo.
fn session(
    tx: &mut StoreTx<'_>,
    binding: &PersonalBinding,
    origin: &PersonalOrigin,
    rejected: &mut Option<ScopeSourceError>,
) -> Result<SessionRef, CoreError> {
    let session = match origin {
        PersonalOrigin::Execution(execution) => match tx.execution_session(execution) {
            Ok(session) => session,
            Err(CoreError::ScopeUnavailable) => return reject(ScopeSourceError::ContextUnavailable, rejected),
            Err(error) => return Err(error),
        },
        PersonalOrigin::App { session_id } => {
            let raw: Option<String> = tx.scalar(
                "SELECT (SELECT json_array(chat_id,state) FROM donna_bot_sessions WHERE id=?1)",
                [session_id],
            )?;
            let Some(raw) = raw else { return reject(ScopeSourceError::ContextUnavailable, rejected) };
            let (chat_id, state): (String, String) = decode(&raw)?;
            if state != "open" {
                return reject(ScopeSourceError::ContextUnavailable, rejected);
            }
            SessionRef { id: session_id.clone(), chat_id }
        }
    };
    if session.chat_id != binding.conversation_id {
        return reject(ScopeSourceError::ContextUnavailable, rejected);
    }
    Ok(session)
}
/// Bootstrap único da política fechada, na Tx do primeiro pedido já validado. CAS: só com a
/// barreira ainda `uninitialized` na revisão lida e sem linha; nunca reabre invalidação.
fn bootstrap(
    tx: &mut StoreTx<'_>,
    binding: &PersonalBinding,
    barrier: &SourceBarrier,
) -> Result<Activation, CoreError> {
    let barrier_sql = i64::try_from(u64::from(barrier.revision)).map_err(|_| CoreError::Storage)?;
    let next = tx.next_revision()?;
    let policy_revision = revision(next)?;
    let epoch = uuid::Uuid::new_v4().to_string();
    let changed = tx.execute(
        "INSERT INTO donna_scope_personal_activation(singleton,version,barrier_revision,principal_bot_id,policy,policy_revision,recovery_epoch) SELECT 1,1,?1,?2,'personal_private_read_v1',?3,?4 WHERE EXISTS(SELECT 1 FROM donna_scope_authority_state WHERE singleton=1 AND version=1 AND state='uninitialized' AND reason IS NULL AND revision=?1)",
        params![barrier_sql, binding.bot_id, next, epoch],
    )?;
    if changed != 1 {
        return Err(CoreError::Storage);
    }
    Ok(Activation {
        barrier_revision: barrier.revision,
        principal_bot_id: Id::try_from(binding.bot_id.clone()).map_err(|_| CoreError::Storage)?,
        policy_revision,
        recovery_epoch: Id::try_from(epoch).map_err(|_| CoreError::Storage)?,
    })
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bootstrap {
    Allowed,
    Denied,
}
fn current(
    tx: &mut StoreTx<'_>,
    hub: &Hub,
    caller: &Caller,
    binding: &PersonalBinding,
    origin: &PersonalOrigin,
    mode: Bootstrap,
    rejected: &mut Option<ScopeSourceError>,
) -> Result<PersonalSources, CoreError> {
    revalidate(tx, hub, caller, binding, rejected)?;
    let session = session(tx, binding, origin, rejected)?;
    let barrier = barrier(tx)?;
    let active = match activation(tx)? {
        Some(active) => active,
        None if mode == Bootstrap::Allowed && matches!(barrier.state, BarrierState::Uninitialized) => {
            bootstrap(tx, binding, &barrier)?
        }
        None => return reject(barrier.denial(), rejected),
    };
    // Qualquer invalidação muda a revisão da barreira; a política antiga não sobrevive.
    if active.barrier_revision != barrier.revision || active.principal_bot_id.as_str() != binding.bot_id {
        return reject(ScopeSourceError::SourcesInvalidated, rejected);
    }
    Ok(PersonalSources {
        principal_bot_id: active.principal_bot_id,
        policy_revision: active.policy_revision,
        recovery_epoch: active.recovery_epoch,
        barrier_revision: barrier.revision,
        session,
    })
}
/// Grava somente o bootstrap único da política; não emite evento nem re-resolve pedido antigo.
pub(crate) fn personal_positive(
    hub: &Hub,
    caller: &Caller,
    binding: &PersonalBinding,
    origin: &PersonalOrigin,
) -> Result<PersonalSources, ScopeSourceError> {
    let mut rejected = None;
    with_store_tx(&hub.store, |tx| current(tx, hub, caller, binding, origin, Bootstrap::Allowed, &mut rejected))
        .map_err(|_| rejected.unwrap_or(ScopeSourceError::OutcomeUnknown))
}
/// Revalida a captura original na MESMA Tx da leitura; qualquer diferença nega antes de ler.
pub(crate) fn personal_read<T: 'static>(
    hub: &Hub,
    caller: &Caller,
    binding: &PersonalBinding,
    origin: &PersonalOrigin,
    captured: &PersonalSources,
    read: impl FnOnce(&mut PrivateReadTx<'_, '_>) -> Result<T, CoreError>,
) -> Result<T, ScopeSourceError> {
    let mut rejected = None;
    with_store_tx(&hub.store, |tx| {
        let now = current(tx, hub, caller, binding, origin, Bootstrap::Denied, &mut rejected)?;
        if now.session != captured.session {
            return reject(ScopeSourceError::ContextChanged, &mut rejected);
        }
        if now != *captured {
            return reject(ScopeSourceError::SourcesInvalidated, &mut rejected);
        }
        read(&mut PrivateReadTx { tx, binding })
    })
    .map_err(|_| rejected.unwrap_or(ScopeSourceError::OutcomeUnknown))
}
