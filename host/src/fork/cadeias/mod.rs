//! D220: admissão interna única; sem fila, `SessionRef` ou autoridade recebida do modelo.
pub(crate) mod admissao;
pub(crate) mod descendencia_rotina;
pub(crate) mod equipe;
pub(crate) mod modelo;
pub(crate) mod store;

#[cfg(test)]
mod equipe_testes;
#[cfg(test)]
mod testes;
