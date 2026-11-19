//! Sessões por bot (Marco 2a): o catálogo `donna_bot_sessions`, o marcador de sessão para apps antigos e a exposição
//! externa por sessão visível. A principal de cada conversa (bot, grupo ou conversa de Space) tem o id da conversa.
pub(crate) mod admissao;
pub(crate) mod api;
pub(crate) mod bindings;
pub(crate) mod contexto_ferramenta;
pub(crate) mod encerrar;
pub(crate) mod excluir;
pub(crate) use bindings::ExecutionRef;
pub(crate) mod engine_choice;
pub(crate) mod escolha;
pub(crate) mod exposure;
pub(crate) mod gateway;
pub(crate) mod marker;
pub(crate) mod origin;
pub(crate) mod runs_on;
pub(crate) mod store;
pub(crate) mod titulo;

pub(crate) use store::{ensure_principal, migrate};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod bindings_tests;

#[cfg(test)]
pub(crate) mod call_tests;

#[cfg(test)]
mod catalog_tests;

#[cfg(test)]
mod admissao_tests;

#[cfg(test)]
mod pr4_tests;

#[cfg(test)]
mod pr5_tests;

#[cfg(test)]
mod d372_tests;

#[cfg(test)]
pub(crate) mod gateway_actor_tests;

#[cfg(test)]
mod gateway_tests;
