//! Ligação ao caminho de partida do Motor no macOS no perfil rc.2 —
//! DORMENTE enquanto a recusa #117 estiver ativa. O caminho de produção
//! vigente continua negando antes de qualquer Motor/MCP (`ensure_runnable`,
//! recusa `motor_macos_indisponivel`); esta ligação não tem chamador de
//! produção e não existe fonte de entrada rc.2 no Hub hoje. Sem a retirada
//! da recusa e sem entrada verificada do runtime rc.2, nada inicia.
//!
//! Contrato da ligação: o Motor e seus descendentes só iniciam por admissão
//! verificada — identidade do artefato e do par em ARM64, partida sob perfil
//! Seatbelt comprovado cobrindo o motor e cada descendente, capacidades só
//! mediadas (sudo sempre negado, shell desligado por padrão, tela
//! desligada). Toda recusa do caminho de admissão vira o código estável
//! `motor_macos_indisponivel` antes de Motor/MCP. A unidade admitida carrega
//! a porta auditada e o cofre da geração: "Parar tudo" fecha os dois juntos,
//! sem bloquear, e a parada confirmada exige evidência dentro do prazo de
//! 2 s — sobrevivente é visível, nunca sucesso falso.

use std::num::NonZeroU64;

use super::auditoria::{Falha, PortaAuditada};
use super::cofre;
use super::identidade;
use super::partida;
use super::porta::{Decisao, Escopo, Pedido, Recusa};
use super::recusas::Registro;

/// Entrada de identidade de uma tentativa de partida rc.2: manifesto
/// esperado, geração, e as observações do artefato e do par validados.
pub(super) struct IdentidadeEntrada<'a> {
    pub(super) manifesto: identidade::Artefato<'a>,
    pub(super) geracao: NonZeroU64,
    pub(super) artefato: identidade::Observacao<identidade::Artefato<'a>>,
    pub(super) par: identidade::Observacao<identidade::ParValidado<'a>>,
}

/// Entrada de partida: perfil Seatbelt esperado, observações de perfil e
/// censo, e as capacidades pedidas — só a mediada existe nesta candidata.
pub(super) struct PartidaEntrada<'a> {
    pub(super) esperado: partida::Perfil<'a>,
    pub(super) perfil: partida::ObservacaoPerfil<'a>,
    pub(super) censo: partida::ObservacaoCenso<'a>,
    pub(super) capacidades: Vec<partida::Capacidade>,
}

/// Unidade rc.2 admitida: partida move-only, porta auditada e cofre da
/// geração. Só existe com identidade e partida verificadas e coerentes com o
/// escopo; o Motor e seus descendentes não iniciam sem ela. A unidade segue
/// viva como supervisora depois de entregar a partida: porta, cofre e censo
/// continuam sob seu controle, e a partida é recusada após a parada.
pub(super) struct UnidadeRc2<'a> {
    partida: Option<partida::Partida<'a>>,
    porta: PortaAuditada<'a>,
    cofre: cofre::Cofre<'a>,
    parada: bool,
}

/// Regra permanente da ligação: "Sempre permitir" não existe para shell —
/// proibição categórica, distinta da indisponibilidade das outras.
pub(super) fn regra_permanente(capacidade: partida::Capacidade) -> Result<(), cofre::RecusaRegra> {
    cofre::regra_permanente(capacidade)
}

/// Recusa estável da ligação: sempre o mesmo código, antes de Motor/MCP.
pub(super) fn recusa_estavel_de_identidade(recusa: &identidade::Recusa) -> anyhow::Error {
    let motivo = match recusa {
        identidade::Recusa::Manifesto => "manifesto inválido",
        identidade::Recusa::SemArtefatoArm64 => "artefato arm64 ausente",
        identidade::Recusa::ArtefatoDivergente => "artefato divergente do manifesto",
        identidade::Recusa::SemParArm64 => "par arm64 ausente",
        identidade::Recusa::ParDivergente => "par divergente do manifesto",
        identidade::Recusa::Geracao => "geração divergente",
    };
    anyhow::anyhow!("{}: {motivo}", partida::CODIGO)
}

impl std::fmt::Debug for UnidadeRc2<'_> {
    /// Projeção fixa: a unidade nunca vaza conteúdo de porta, cofre ou
    /// partida em mensagens de erro.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UnidadeRc2 (partida, porta e cofre da geração; sem projeção de conteúdo)")
    }
}

fn recusa_estavel_de_incoerencia() -> anyhow::Error {
    anyhow::anyhow!("{}: escopo incoerente com a admissão", partida::CODIGO)
}

/// Abre a tentativa de partida rc.2. Falha-closed: identidade primeiro,
/// partida depois, coerência de escopo por último — toda recusa vira o
/// código estável antes de qualquer porta, cofre ou efeito existir.
pub(super) fn iniciar<'a>(
    identidade_entrada: &IdentidadeEntrada<'a>,
    partida_entrada: PartidaEntrada<'a>,
    escopo: Escopo,
    instancia: NonZeroU64,
    registro: &'a Registro,
    vinculo: cofre::Vinculo<'a>,
) -> Result<UnidadeRc2<'a>, anyhow::Error> {
    identidade::admitir(
        &identidade_entrada.manifesto,
        identidade_entrada.geracao,
        identidade_entrada.artefato,
        identidade_entrada.par,
    )
    .map_err(|recusa| recusa_estavel_de_identidade(&recusa))?;
    let partida_verificada = partida::partir(
        partida_entrada.esperado,
        partida_entrada.perfil,
        partida_entrada.censo,
        &partida_entrada.capacidades,
    )
    .map_err(partida::recusa_estavel)?;
    if escopo.geracao != identidade_entrada.geracao || vinculo.escopo() != escopo {
        return Err(recusa_estavel_de_incoerencia());
    }
    let porta = PortaAuditada::nova(escopo, instancia, registro);
    let cofre = cofre::Cofre::abrir(vinculo);
    Ok(UnidadeRc2 { partida: Some(partida_verificada), porta, cofre, parada: false })
}

impl<'a> UnidadeRc2<'a> {
    /// Aceite do aviso versionado daquele bot: sem aceite, a porta não abre
    /// pedido.
    pub(super) fn aceitar_aviso(&mut self, escopo: Escopo) -> Result<(), Falha> {
        self.porta.aceitar_aviso(escopo)
    }

    /// Partida verificada da unidade: move-only, sai UMA única vez no ponto
    /// do efeito. A unidade NÃO deixa de existir: segue supervisora com
    /// porta, cofre e censo, e a partida é recusada após a parada ou se já
    /// foi entregue — nada parte duas vezes, e nada parte depois de
    /// "Parar tudo".
    pub(super) fn partir_para_o_motor(&mut self) -> Result<partida::Partida<'a>, Falha> {
        if self.parada {
            return Err(Falha::Recusa(Recusa::Fechada));
        }
        self.partida.take().ok_or(Falha::Recusa(Recusa::Fechada))
    }

    /// Registra instância observada (motor ou descendente): o censo é o que
    /// torna sobrevivente visível na parada.
    pub(super) fn registrar(&mut self, instancia: super::porta::Instancia) -> bool {
        self.porta.registrar(instancia)
    }

    /// Confirma a saída de uma instância registrada.
    pub(super) fn confirmar_saida(&mut self, instancia: super::porta::Instancia) -> bool {
        self.porta.confirmar_saida(instancia)
    }

    /// Caminho de `pedir` da porta auditada.
    pub(super) fn pedir(&mut self, acao: super::porta::Acao, agora_ms: u64) -> Result<Pedido, Falha> {
        self.porta.pedir(acao, agora_ms)
    }

    /// Caminho de `resolver` da porta auditada.
    pub(super) fn resolver(
        &mut self,
        pedido: Pedido,
        respondente: Escopo,
        decisao: Decisao,
        agora_ms: u64,
    ) -> Result<(), Falha> {
        self.porta.resolver(pedido, respondente, decisao, agora_ms)
    }

    /// Caminho de `efetuar` da porta auditada: a intenção é gravada antes do
    /// efeito; gravação falha fecha a porta, sem efeito e sem retry.
    pub(super) fn efetuar(
        &mut self,
        pedido: Pedido,
        acao: super::porta::Acao,
        agora_ms: impl FnMut() -> u64,
        efeito_imediato: impl FnOnce(super::porta::Acao),
    ) -> Result<(), Falha> {
        self.porta.efetuar(pedido, acao, agora_ms, efeito_imediato)
    }

    /// Autoriza inferência pelo cofre da geração: item de outro app negado;
    /// identidade exata do item; nenhuma autorização atravessa gerações.
    pub(super) fn autorizar_inferencia(&self, pedido: cofre::Pedido<'_>) -> Result<cofre::Autorizacao, cofre::Recusa> {
        self.cofre.autorizar(pedido)
    }

    /// Redenção da autorização: SÓ pelo cofre da geração ainda aberto — a
    /// emitida antes da parada morre com ela, nunca por estado do portador.
    pub(super) fn redimir(&self, autorizacao: cofre::Autorizacao) -> Result<NonZeroU64, cofre::Recusa> {
        self.cofre.redimir(autorizacao)
    }

    /// "Parar tudo" da unidade: o cofre da geração fecha PRIMEIRO,
    /// incondicional — nenhuma gravação de auditoria, que pode bloquear ou
    /// falhar, precede o fechamento do cofre, e o prazo de 2 s não depende
    /// dela. Depois a porta fecha (gravando a pendência como `fechada`, sem
    /// bloquear: a gravação falha deixa o fechamento valendo). A partida é
    /// recusada dali em diante.
    pub(super) fn parar_tudo(&mut self, agora_ms: u64) -> Result<(), Falha> {
        self.cofre.parar();
        self.parada = true;
        self.porta.parar(agora_ms)
    }

    /// Estado da parada com evidência: confirmada no prazo de 2 s só com
    /// unidade fechada e saídas confirmadas; sobrevivente é visível com
    /// identidade; sem evidência após o prazo é incerteza, nunca sucesso
    /// falso.
    pub(super) fn estado_da_parada(&self, agora_ms: u64, evidencia: super::porta::Evidencia) -> super::porta::Parada {
        self.porta.estado(agora_ms, evidencia)
    }
}
