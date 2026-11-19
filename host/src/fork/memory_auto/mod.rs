//! Memória automática: prova durável comum, fila de referências e commit com recibo.
//! O executor real é fornecido pelo coordenador; ausência dele nunca equivale a sucesso.
pub(crate) mod admission;
pub(crate) mod apply;
pub(crate) mod audit;
pub(crate) mod origin;
pub(crate) mod owner;
pub(crate) mod queue;
pub(crate) mod recibo_injecao;
pub(crate) mod sleep;
pub(crate) mod sleep_checks;
pub(crate) mod sleep_model;
pub(crate) mod suggestions;
#[cfg(test)]
mod tests;
pub(crate) mod transport;
pub(crate) mod worker;
pub(crate) use queue::{begin, checkpoint, migrate};
