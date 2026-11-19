//! Contrato exclusivamente negativo para materialização privada, sem consumidor produtivo.
//! Operação e classe são metadados, não autoridade ou prova de origem.

use std::convert::Infallible;

pub(crate) use super::manifest::Capability as Operation;
pub(crate) use super::payload::PayloadClass as Class;

/// Pedido interno, não uma atestação de qualificação do destino.
#[derive(Clone, Copy)]
pub(crate) enum Destination {
    Entries,
    Checkpoint,
    RendererCache,
    HarnessTranscript,
    AutomaticMemory,
    Publication,
    PrivateStore,
    PrivateView,
    ModelInput,
}

enum Requirement {
    Forbidden,
    QualifiedPathRequired,
}

fn requirement(requested: Destination) -> Requirement {
    match requested {
        Destination::Entries
        | Destination::Checkpoint
        | Destination::RendererCache
        | Destination::HarnessTranscript
        | Destination::AutomaticMemory
        | Destination::Publication => Requirement::Forbidden,
        Destination::PrivateStore | Destination::PrivateView | Destination::ModelInput => {
            Requirement::QualifiedPathRequired
        }
    }
}

/// Recusa estática, sem metadados ou conteúdo do pedido.
#[derive(Debug)]
pub(crate) struct Unavailable;

impl Unavailable {
    pub(crate) fn code() -> &'static str {
        "private_content_unavailable"
    }
}

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Conteúdo privado indisponível.")
    }
}

impl std::error::Error for Unavailable {}

/// Nenhuma combinação admite materialização; não recebe conteúdo nem emite autoridade.
pub(crate) fn probe(_operation: Operation, _class: Class, requested: Destination) -> Result<Infallible, Unavailable> {
    match requirement(requested) {
        Requirement::Forbidden | Requirement::QualifiedPathRequired => Err(Unavailable),
    }
}

#[cfg(test)]
#[path = "private_content_tests.rs"]
mod tests;
