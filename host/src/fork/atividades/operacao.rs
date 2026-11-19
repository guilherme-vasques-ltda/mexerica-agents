//! Identidade do core e seletores de entrada são tipos distintos. Nenhum é grant.
use super::erro_operacao::OperationError;
use crate::fork::escopo::{Id, ResolvedScope, Version};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    PrepareReview,
    DecideReview,
}

/// Identidade do gesto no cliente; não há conversão para referência do core.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClientNonce(Id);

/// Entrada não confiável. M1 deverá resolver sua posse/contexto pelo ledger.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationSelector {
    pub version: Version,
    pub operation_id: Id,
    pub kind: OperationKind,
}

/// Emitida somente pelo core. Sem construtor público ou Deserialize.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct OperationRef(OperationSelector);
impl OperationRef {
    /// Alocação pré-persistência, NÃO receipt/reserva durável. Scope ainda não emitido.
    pub(super) fn issue(_scope: &ResolvedScope, kind: OperationKind) -> Result<Self, OperationError> {
        let operation_id =
            Id::try_from(uuid::Uuid::new_v4().to_string()).map_err(|_| OperationError::InvalidContract)?;
        Ok(Self(OperationSelector { version: Version, operation_id, kind }))
    }
    pub(super) fn descriptor(&self) -> OperationSelector {
        self.0.clone()
    }
}
