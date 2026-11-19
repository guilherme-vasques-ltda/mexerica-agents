//! Contrato puro do PR2A. Retornos não são permissões de runtime nem cartões emitidos.
use super::{
    floor::{Categoria, Efeito, categoria},
    rule::{ApprovalLevel, Campo, Effect, Rule},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decisao {
    Recusar,
    Perguntar,
    Permitir,
    ForaDaGarantia,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mediacao {
    Comprovada,
    IncertaComPontoSeguro,
    IncertaSemPontoSeguro,
    ForaDoHub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origem {
    DonoDigitadoAutenticado,
    Derivada,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Validade {
    Atual,
    Invalida,
}

pub struct Pedido<'a> {
    pub bot: &'a str,
    pub ferramenta: &'a str,
    pub acao: &'a str,
    pub alvo: &'a str,
    pub efeitos: &'a [Efeito],
    pub origem: Origem,
    pub mediacao: Mediacao,
    pub politica_e_autoridade: Validade,
    pub nivel: ApprovalLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Correspondencia {
    Sim,
    Nao,
    Incerta,
}

fn corresponde(regra: &Rule, pedido: &Pedido<'_>) -> Correspondencia {
    if !regra.enabled
        || regra.bot_id.as_deref().is_some_and(|bot| bot != pedido.bot)
        || regra.tool.as_deref().is_some_and(|ferramenta| ferramenta != pedido.ferramenta)
    {
        return Correspondencia::Nao;
    }
    let Some((campo, _operador)) = regra.interpretacao() else {
        // Não provar relação permite negar; nunca permite conceder por inferência.
        return Correspondencia::Incerta;
    };
    let valor = match campo {
        Campo::Acao => pedido.acao,
        Campo::Alvo => pedido.alvo,
        Campo::Ferramenta => pedido.ferramenta,
    };
    if valor.is_empty() {
        Correspondencia::Incerta
    } else if valor == regra.value {
        Correspondencia::Sim
    } else {
        Correspondencia::Nao
    }
}

pub fn avaliar(pedido: &Pedido<'_>, regras: &[Rule]) -> Decisao {
    if pedido.politica_e_autoridade == Validade::Invalida {
        return Decisao::Recusar;
    }
    if regras.iter().any(|regra| regra.effect == Effect::Deny && corresponde(regra, pedido) != Correspondencia::Nao) {
        return Decisao::Recusar;
    }
    if pedido.mediacao == Mediacao::ForaDoHub {
        return Decisao::ForaDaGarantia;
    }
    if pedido.mediacao == Mediacao::IncertaSemPontoSeguro
        || pedido.acao.is_empty()
        || pedido.alvo.is_empty()
        || pedido.bot.is_empty()
        || pedido.ferramenta.is_empty()
    {
        return Decisao::Recusar;
    }
    if pedido.mediacao == Mediacao::IncertaComPontoSeguro {
        return Decisao::Perguntar;
    }
    let categorias: Vec<_> = pedido.efeitos.iter().copied().filter_map(categoria).collect();
    if pedido.efeitos.is_empty() || categorias.contains(&Categoria::Desconhecida) {
        return Decisao::Perguntar;
    }
    if !categorias.is_empty() || pedido.nivel == ApprovalLevel::Ask {
        return Decisao::Perguntar;
    }
    // Origem não concede poderes. Uma política previamente escolhida pode
    // autorizar ações comuns também para origens derivadas, se a autoridade
    // e a mediação daquele pedido já foram comprovadas pelo adaptador.
    if pedido.nivel == ApprovalLevel::FullAccess
        || regras
            .iter()
            .any(|regra| regra.effect == Effect::Allow && corresponde(regra, pedido) == Correspondencia::Sim)
    {
        Decisao::Permitir
    } else {
        // Origem autenticada não é autorização ampla: regra ou cartão específico.
        Decisao::Perguntar
    }
}

/// I-341: ausência/erro de política fecha antes de compor. Nunca reduz restrição.
pub const fn compor(hub: Decisao, ferramenta: Decisao) -> Decisao {
    match (hub, ferramenta) {
        (Decisao::Recusar | Decisao::ForaDaGarantia, _) | (_, Decisao::Recusar | Decisao::ForaDaGarantia) => {
            Decisao::Recusar
        }
        (Decisao::Perguntar, _) | (_, Decisao::Perguntar) => Decisao::Perguntar,
        (Decisao::Permitir, Decisao::Permitir) => Decisao::Permitir,
    }
}

pub fn avaliar_fechado(pedido: Result<&Pedido<'_>, ()>, regras: &[Rule]) -> Decisao {
    pedido.map_or(Decisao::Recusar, |pedido| avaliar(pedido, regras))
}

/// Regra que sustentou o veredito automático, para a auditoria do porteiro.
/// Não altera a decisão: `Recusar` sem regra indica vínculo/política,
/// não proibição; `Permitir` por Acesso total não tem regra.
pub(crate) fn decisao_regra<'a>(
    pedido: &Pedido<'_>,
    regras: &'a [Rule],
    decisao: Decisao,
) -> Option<(&'a Rule, Correspondencia)> {
    match decisao {
        Decisao::Recusar => regras.iter().find_map(|regra| {
            let c = corresponde(regra, pedido);
            (regra.effect == Effect::Deny && c != Correspondencia::Nao).then_some((regra, c))
        }),
        Decisao::Permitir if pedido.nivel != ApprovalLevel::FullAccess => regras
            .iter()
            .find(|regra| regra.effect == Effect::Allow && corresponde(regra, pedido) == Correspondencia::Sim)
            .map(|regra| (regra, Correspondencia::Sim)),
        _ => None,
    }
}
