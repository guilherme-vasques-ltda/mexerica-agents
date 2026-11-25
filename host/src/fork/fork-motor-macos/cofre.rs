//! Cofre do Motor no macOS: única fronteira com o Chaveiro, no lado confiável
//! do Hub. O vínculo é COMPOSTO pelo Hub — escopo, provedor e o item exato
//! daquele provedor — e nunca sai de texto de pedido: os campos são privados
//! e a composição recusa grupo ou identificador vazios. Só existe operação de
//! inferência vinculada a esse par, com identidade exata do item; item de
//! outro app nunca é atendido; chamada genérica de leitura/exportação não
//! existe; e "Sempre permitir" não existe para shell — nem nesta candidata
//! nem quando regras de escopo existirem para outras capacidades.
//!
//! O cofre é da geração: "Parar tudo" fecha o acesso do cofre daquela
//! geração, sem reabrir, e nenhuma autorização atravessa gerações — o escopo
//! do vínculo carrega a geração e pedido de outra geração recusa. A resposta
//! é só uma autorização consumida por valor no ponto do efeito: segredo e
//! descritor nunca saem, a autorização não implementa `Clone`/`Copy` e cada
//! uma sai de uma consulta única ao cofre.

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
    Vinculo,
    OutroApp,
    ItemDivergente,
    Fechada,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum RecusaComposicao {
    GrupoVazio,
    IdentificadorVazio,
}

/// Vínculo do cofre, composto pelo Hub no lado confiável: escopo, provedor e
/// o item exato daquele provedor, com o grupo do próprio app. Campos
/// privados: nada que vem do pedido constrói um vínculo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Vinculo<'a> {
    escopo: Escopo,
    provedor: NonZeroU64,
    item: Item<'a>,
}

impl<'a> Vinculo<'a> {
    /// Composição do vínculo — origem confiável, nunca texto do pedido.
    /// O item fica ligado ao provedor com identidade exata; grupo vazio ou
    /// identificador vazio não constroem vínculo.
    pub(super) fn compor(
        grupo_do_app: &'a str,
        escopo: Escopo,
        provedor: NonZeroU64,
        identificador: &'a str,
    ) -> Result<Self, RecusaComposicao> {
        if grupo_do_app.is_empty() {
            return Err(RecusaComposicao::GrupoVazio);
        }
        if identificador.is_empty() {
            return Err(RecusaComposicao::IdentificadorVazio);
        }
        Ok(Self { escopo, provedor, item: Item { grupo: grupo_do_app, identificador } })
    }
}

/// Autorização da inferência: move-only e ligada à geração do vínculo.
/// Consumida por valor no ponto do efeito — reuso não compila; cada
/// autorização sai de uma consulta única ao cofre.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Autorizacao {
    geracao: NonZeroU64,
}

impl Autorizacao {
    /// Consumo único no ponto do efeito: devolve a geração do vínculo que
    /// autorizou e destrói a autorização — depois disso ela não existe.
    pub(super) fn consumir(self) -> NonZeroU64 {
        self.geracao
    }
}

/// Cofre da geração: aberto sobre o vínculo composto pelo Hub, fechado por
/// "Parar tudo" e nunca reaberto. A autorização não atravessa gerações — o
/// escopo do vínculo carrega a geração e pedido de outra geração recusa
/// antes de olhar o item.
pub(super) struct Cofre<'a> {
    vinculo: Vinculo<'a>,
    fechado: bool,
}

impl<'a> Cofre<'a> {
    pub(super) fn abrir(vinculo: Vinculo<'a>) -> Self {
        Self { vinculo, fechado: false }
    }

    /// "Parar tudo" aplicado ao cofre: fecha o acesso da geração, sem
    /// reabrir. Idempotente e nunca bloqueado por auditoria.
    pub(super) fn parar(&mut self) {
        self.fechado = true;
    }

    /// Autoriza só a inferência vinculada a escopo e provedor conferidos,
    /// sobre o item exato do próprio app. A resposta é só a autorização:
    /// segredo e descritor do cofre nunca saem.
    pub(super) fn autorizar(&self, pedido: Pedido<'_>) -> Result<Autorizacao, Recusa> {
        let Pedido::Inferencia(inferencia) = pedido else {
            return Err(Recusa::OperacaoGenerica);
        };
        if self.fechado {
            return Err(Recusa::Fechada);
        }
        if inferencia.escopo != self.vinculo.escopo || inferencia.provedor != self.vinculo.provedor {
            return Err(Recusa::Vinculo);
        }
        if inferencia.item.grupo != self.vinculo.item.grupo {
            return Err(Recusa::OutroApp);
        }
        if inferencia.item != self.vinculo.item {
            return Err(Recusa::ItemDivergente);
        }
        Ok(Autorizacao { geracao: self.vinculo.escopo.geracao })
    }
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
