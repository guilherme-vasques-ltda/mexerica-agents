//! Mexerica: regras e aprovações. O porteiro (`gate`) avalia os pedidos de
//! permissão do motor; os demais executores ainda não consultam este núcleo.
pub mod api;
#[allow(dead_code)]
pub mod avaliacao;
#[allow(dead_code)]
#[path = "chao.rs"]
pub mod floor;
pub mod gate;
#[allow(dead_code)]
pub mod inventario;
mod levels;
pub(crate) mod mcp;
pub mod migrate;
#[cfg(test)]
mod provas;
#[path = "contrato.rs"]
pub mod rule;
pub mod store;
