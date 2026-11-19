//! D106: a origem registrada não é um `UserIntentGrant`. A autorização fica para 22.4.
use serde::Serialize;

/// Único modo disponível em 22.1: sem emitir/consumir grants ou alterar prompts.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Observe,
}
