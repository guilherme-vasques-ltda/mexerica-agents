//! Marketplace 1a: a escolha da conta em cada pedido, feita pelo Hub e não pelo modelo.
//! Função pronta e testada; o encaixe no turno do bot (montar a lista de ferramentas do turno com `ContasDoTurno`
//! e recusar `tool_request` de outra conta) depende do `set_tools` do Motor Mexerica e fica para a próxima fatia.
#![allow(
    dead_code,
    reason = "escolha de conta pronta e testada; o encaixe no turno do bot vem com o set_tools do motor"
)]
use super::{Connector, contas::dobrar};
use crate::store::BotConfig;
use anyhow::{Result, ensure};
use std::collections::BTreeSet;

/// O que a pessoa escreveu neste pedido. Só se constrói a partir da mensagem humana do turno: resposta de
/// ferramenta, e-mail, ticket, página ou nota de memória nunca são convertidos neste tipo.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MensagemDaPessoa<'a>(&'a str);

impl<'a> MensagemDaPessoa<'a> {
    pub(crate) fn nova(texto: &'a str) -> Self {
        Self(texto)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Escolha {
    /// Contas autorizadas neste pedido (uma, ou as que a pessoa citou).
    Autorizadas(Vec<String>),
    /// O Hub pergunta "Qual conta?" e só segue com a resposta da pessoa.
    Perguntar(Vec<String>),
    /// Nenhuma conta ligada deste serviço.
    SemConta,
}

/// Posições (início, fim) de cada ocorrência do nome inteiro (limites de palavra) no texto já normalizado.
fn ocorrencias(texto: &str, nome: &str) -> Vec<(usize, usize)> {
    let padrao = format!(" {nome} ");
    (0..texto.len())
        .filter(|i| texto.is_char_boundary(*i) && texto[*i..].starts_with(&padrao))
        .map(|i| (i + 1, i + 1 + nome.len()))
        .collect()
}

/// Contas ligadas cujo nome (inteiro, com limites de palavra) aparece na mensagem. Um nome contido em outro mais longo
/// ("Casa" em "Casa Nova") só some quando TODAS as suas ocorrências estão dentro do nome longo; se a pessoa também
/// escreveu o nome curto sozinho ("Casa e Casa Nova"), as duas contas contam.
fn citadas(ligadas: &[&Connector], texto: &str) -> Vec<String> {
    type Achada<'a> = (&'a Connector, String, Vec<(usize, usize)>);
    let achadas: Vec<Achada<'_>> = ligadas
        .iter()
        .map(|c| (*c, dobrar(&c.name)))
        .filter(|(_, nome)| !nome.is_empty())
        .map(|(c, nome)| {
            let onde = ocorrencias(texto, &nome);
            (c, nome, onde)
        })
        .filter(|(_, _, onde)| !onde.is_empty())
        .collect();
    achadas
        .iter()
        .filter(|(_, nome, onde)| {
            let engolida = |(ini, fim): &(usize, usize)| {
                achadas.iter().any(|(_, outro, onde_outro)| {
                    outro.len() > nome.len()
                        && outro.contains(nome.as_str())
                        && onde_outro.iter().any(|(oi, of)| oi <= ini && fim <= of)
                })
            };
            !onde.iter().all(engolida)
        })
        .map(|(c, _, _)| c.id.clone())
        .collect()
}

/// A pessoa citou o nome de uma conta do serviço que existe mas não pode ser usada neste bot (desligada ou de outro
/// Space)? Nome igual ao de uma conta disponível não conta: ele resolve para a disponível.
fn cita_conta_indisponivel(
    servico: &str,
    texto: &str,
    ligadas: &[&Connector],
    cfg: &BotConfig,
    todas: &[Connector],
) -> bool {
    let nomes_ligadas: BTreeSet<String> = ligadas.iter().map(|c| dobrar(&c.name)).collect();
    todas
        .iter()
        .filter(|c| c.servico.as_deref() == Some(servico))
        .filter(|c| !(c.space == cfg.space_id && cfg.connectors.contains(&c.id)))
        .map(|c| dobrar(&c.name))
        .filter(|nome| !nome.is_empty() && !nomes_ligadas.contains(nome))
        .any(|nome| !ocorrencias(texto, &nome).is_empty())
}

/// A escolha do Hub, nesta ordem: (0) nome citado de conta que não pode ser usada → pergunta; (1) conta citada pelo nome na mensagem da pessoa; (2) conta padrão do bot;
/// (3) única conta ligada; (4) senão perguntar. Com "perguntar qual conta" ligado e mais de uma conta, o padrão não vale.
pub(crate) fn escolher_conta(
    servico: &str,
    mensagem: MensagemDaPessoa<'_>,
    cfg: &BotConfig,
    todas: &[Connector],
) -> Escolha {
    let ligadas: Vec<&Connector> = todas
        .iter()
        .filter(|c| c.servico.as_deref() == Some(servico) && c.space == cfg.space_id && cfg.connectors.contains(&c.id))
        .collect();
    let ids = || ligadas.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
    if ligadas.is_empty() {
        return Escolha::SemConta;
    }
    let texto = format!(" {} ", dobrar(mensagem.0));
    // Nome citado que existe mas está desligado ou é de outro Space: nunca cai no padrão em silêncio.
    if cita_conta_indisponivel(servico, &texto, &ligadas, cfg, todas) {
        return Escolha::Perguntar(ids());
    }
    let citadas = citadas(&ligadas, &texto);
    if !citadas.is_empty() {
        return Escolha::Autorizadas(citadas);
    }
    if ligadas.len() == 1 {
        return Escolha::Autorizadas(ids());
    }
    if cfg.perguntar_conta.get(servico) != Some(&true)
        && let Some(padrao) = cfg.conta_padrao.get(servico)
        && ligadas.iter().any(|c| &c.id == padrao)
    {
        return Escolha::Autorizadas(vec![padrao.clone()]);
    }
    Escolha::Perguntar(ids())
}

/// O que o Hub liberou para UM turno: só estas contas têm ferramentas no turno e só elas podem ser chamadas.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ContasDoTurno {
    autorizadas: BTreeSet<String>,
    pub perguntas: Vec<(String, Vec<String>)>,
}

impl ContasDoTurno {
    pub(crate) fn ids(&self) -> impl Iterator<Item = &str> {
        self.autorizadas.iter().map(String::as_str)
    }

    /// `tool_request` de conta que não foi autorizada neste pedido é recusado, mesmo ligada no bot.
    pub(crate) fn exigir(&self, conta: &str) -> Result<()> {
        ensure!(
            self.autorizadas.contains(conta),
            "conta_nao_autorizada: essa conta não foi liberada para este pedido; pergunte à pessoa qual usar."
        );
        Ok(())
    }
}

/// Calcula as contas do turno para todos os serviços ligados no bot. Contas sem serviço (MCP do próprio usuário) e
/// contas de outro Space nunca entram.
pub(crate) fn contas_do_turno(cfg: &BotConfig, mensagem: MensagemDaPessoa<'_>, todas: &[Connector]) -> ContasDoTurno {
    let servicos: BTreeSet<&str> = todas
        .iter()
        .filter(|c| c.space == cfg.space_id && cfg.connectors.contains(&c.id))
        .filter_map(|c| c.servico.as_deref())
        .collect();
    let mut turno = ContasDoTurno::default();
    for servico in servicos {
        match escolher_conta(servico, mensagem, cfg, todas) {
            Escolha::Autorizadas(ids) => turno.autorizadas.extend(ids),
            Escolha::Perguntar(opcoes) => turno.perguntas.push((servico.to_owned(), opcoes)),
            Escolha::SemConta => {}
        }
    }
    turno
}
