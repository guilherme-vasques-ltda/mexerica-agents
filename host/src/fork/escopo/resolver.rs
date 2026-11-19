//! M1a1/05a: confere vínculos reais, barreira e política; emite o único `ResolvedScope` pessoal.
use super::{
    Audience, ResolvedScope, Revision,
    fontes::{PersonalOrigin, PersonalScopeExpectation, ScopeSourceError, SourceInvalidationReason},
};
use crate::{
    api::devices::Caller,
    fork::{atividades::escopo_store, sessoes::bindings},
    hub::Hub,
};

pub(super) fn binding(input: &PersonalScopeExpectation) -> Result<escopo_store::PersonalBinding, ScopeSourceError> {
    let context = &input.context.0;
    let primary = match &context.audience {
        Audience::Principal {} => true,
        Audience::Additional {} => false,
        _ => return Err(ScopeSourceError::ContextUnavailable),
    };
    if context.team_id.is_some() {
        return Err(ScopeSourceError::ContextUnavailable);
    }
    Ok(escopo_store::PersonalBinding {
        hub_id: context.hub_id.clone().into(),
        space_id: context.space_id.clone().into(),
        bot_id: context.bot_id.clone().into(),
        conversation_id: context.conversation_id.clone().into(),
        space_revision: input.space_revision,
        bot_revision: input.bot_revision,
        conversation_revision: context.conversation_revision,
        privacy_domain_id: context.privacy_domain_id.clone().into(),
        domain_revision: context.domain_revision,
        primary,
    })
}

/// Emite o escopo pessoal positivo; revalida o turno ACP depois da Tx, antes de devolver.
/// O primeiro pedido validado faz, na mesma Tx, o bootstrap único da política fechada.
pub(crate) fn resolve_personal(
    hub: &Hub,
    caller: Caller,
    input: PersonalScopeExpectation,
    origin: PersonalOrigin,
) -> Result<ResolvedScope, ScopeSourceError> {
    let sources = escopo_store::personal_positive(hub, &caller, &binding(&input)?, &origin)?;
    turn_live(hub, &input, &origin)?;
    Ok(ResolvedScope { expectation: input, caller, origin, sources })
}

/// Turno ACP: o token capturado tem de ser o atual do runtime (fonte canônica, não só não vazio).
/// Chamar fora de qualquer Tx/lock do Store ou do runtime: `capture` toma runtime e depois DB.
pub(super) fn turn_live(
    hub: &Hub,
    input: &PersonalScopeExpectation,
    origin: &PersonalOrigin,
) -> Result<(), ScopeSourceError> {
    let PersonalOrigin::Execution(execution) = origin else { return Ok(()) };
    let id = execution.acp_binding().ok_or(ScopeSourceError::ContextUnavailable)?;
    let current = bindings::capture(hub, input.context.bot_id().as_str(), id)
        .map_err(|_| ScopeSourceError::ContextUnavailable)?;
    if current.turn_token() != execution.turn_token() {
        return Err(ScopeSourceError::ContextChanged);
    }
    Ok(())
}

/// Restrição interna com CAS. A revisão retornada não é receipt ou policyRevision.
pub(crate) fn invalidate_personal_sources(
    hub: &Hub,
    caller: &Caller,
    input: &PersonalScopeExpectation,
    expected_barrier_revision: Revision,
    reason: SourceInvalidationReason,
) -> Result<Revision, ScopeSourceError> {
    escopo_store::invalidate_personal_sources(hub, caller, &binding(input)?, expected_barrier_revision, reason)
}
