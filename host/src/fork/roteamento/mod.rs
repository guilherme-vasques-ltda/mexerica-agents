//! D210/D212: configuração e gravador rota-03a; o consumidor de execução é `fork::sessoes::admissao` (PR4).
pub(crate) mod admission;
pub(crate) mod api;
mod config;
pub(crate) mod receipt;
mod selection;
#[cfg(test)]
mod tests;
