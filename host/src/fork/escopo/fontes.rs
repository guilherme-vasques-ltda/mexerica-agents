//! Entradas descritivas e recusas da fonte pessoal; nenhum tipo é credencial.
use super::{ContextRef, Revision};
use crate::fork::sessoes::ExecutionRef;

pub(crate) struct PersonalScopeExpectation {
    pub(crate) context: ContextRef,
    pub(crate) space_revision: Revision,
    pub(crate) bot_revision: Revision,
}

/// De onde vem o pedido pessoal. Nenhuma variante é fabricada aqui.
pub(crate) enum PersonalOrigin {
    /// Turno ACP já capturado pelo Hub (`sessoes::bindings::capture`).
    Execution(ExecutionRef),
    /// Leitura pedida pelo app; o id é só expectativa, conferida no catálogo de Sessões.
    App { session_id: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScopeSourceError {
    IngressDenied,
    ContextUnavailable,
    ContextChanged,
    SourcesUninitialized,
    SourcesInvalidated,
    StateConflict,
    OutcomeUnknown,
    UseDenied,
}
impl std::fmt::Display for ScopeSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::IngressDenied => "scope_ingress_denied",
            Self::ContextUnavailable => "scope_context_unavailable",
            Self::ContextChanged => "scope_context_changed",
            Self::SourcesUninitialized => "scope_sources_uninitialized",
            Self::SourcesInvalidated => "scope_sources_invalidated",
            Self::StateConflict => "scope_source_state_conflict",
            Self::OutcomeUnknown => "scope_source_outcome_unknown",
            Self::UseDenied => "scope_use_denied",
        })
    }
}
impl std::error::Error for ScopeSourceError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceInvalidationReason {
    PrincipalChanged,
    PolicyChanged,
    BindingChanged,
    RecoveryRequired,
}
impl SourceInvalidationReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::PrincipalChanged => "principal_changed",
            Self::PolicyChanged => "policy_changed",
            Self::BindingChanged => "binding_changed",
            Self::RecoveryRequired => "recovery_required",
        }
    }
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "principal_changed" => Some(Self::PrincipalChanged),
            "policy_changed" => Some(Self::PolicyChanged),
            "binding_changed" => Some(Self::BindingChanged),
            "recovery_required" => Some(Self::RecoveryRequired),
            _ => None,
        }
    }
}
