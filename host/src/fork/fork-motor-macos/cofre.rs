//! Cofre do Motor no macOS: única fronteira com o Chaveiro, no lado confiável
//! do Hub. Só existe operação de inferência vinculada a escopo e provedor;
//! item de outro app nunca é atendido; chamada genérica de leitura/exportação
//! não existe; e "Sempre permitir" não existe para shell — nem nesta candidata
//! nem quando regras de escopo existirem para outras capacidades.

use std::num::NonZeroU64;

use super::{partida::Capacidade, porta::Escopo};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Item<'a> {
    pub(super) grupo: &'a str,
    pub(super) identificador: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Inferencia<'a> {
    pub(super) escopo: Escopo,
    pub(super) provedor: NonZeroU64,
    pub(super) item: Item<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pedido<'a> {
    Inferencia(Inferencia<'a>),
    Leitura(Item<'a>),
    Exportacao(Item<'a>),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Recusa {
    OperacaoGenerica,
    OutroApp,
}

/// Autoriza só a inferência vinculada sobre item do próprio app. A resposta é
/// só `Ok`: segredo e descritor do cofre nunca saem, quem pede só recebe o
/// resultado funcional pelo canal do broker.
pub(super) fn autorizar(grupo_do_app: &str, pedido: Pedido<'_>) -> Result<(), Recusa> {
    let Pedido::Inferencia(inferencia) = pedido else {
        return Err(Recusa::OperacaoGenerica);
    };
    if inferencia.item.grupo != grupo_do_app {
        return Err(Recusa::OutroApp);
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum RecusaRegra {
    SempreShell,
    SempreIndisponivel,
}

/// Regra permanente: "Sempre permitir" não existe nesta candidata; para shell
/// é proibição categórica, distinta da indisponibilidade das outras.
pub(super) fn regra_permanente(capacidade: Capacidade) -> Result<(), RecusaRegra> {
    match capacidade {
        Capacidade::Shell => Err(RecusaRegra::SempreShell),
        Capacidade::Mediada | Capacidade::Sudo | Capacidade::Tela => Err(RecusaRegra::SempreIndisponivel),
    }
}
