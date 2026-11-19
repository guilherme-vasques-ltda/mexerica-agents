//! Fonte inicial 19.1a: contratos e `StoreTx`; sem API, fila ou executor ativos.
pub(crate) mod escopo_migracao;
pub(crate) mod escopo_store;
pub mod modelo;
pub(crate) mod startup_store;
pub(crate) mod store;
#[cfg(test)]
mod tests;

pub mod erro_operacao;
pub mod operacao;
pub mod revisao;
#[cfg(test)]
mod testes_escopo_m1a;
#[cfg(test)]
mod testes_operacao;
