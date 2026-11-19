use serde::{Deserialize, Serialize};

/// Erros de contrato, sem corpo, IDs privados ou diagnóstico de SQL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationError {
    InvalidContract,
    NoncesMustDiffer,
    OperationMismatch,
    BindingMismatch,
    DigestMismatch,
    ReviewMismatch,
    InvalidWindow,
    NotYetIssued,
    Expired,
}
impl std::fmt::Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "operation_{self:?}")
    }
}
impl std::error::Error for OperationError {}
