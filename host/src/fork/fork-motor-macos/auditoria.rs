//! Auditoria fail-closed por caminho da porta de permissões do Motor no
//! macOS. Cada recusa — pedir, expirar, negar, fechar — é gravada uma única
//! vez no registro durável antes de o método retornar; se a gravação falha,
//! o efeito negado permanece negado, nada é refeito e a falha fica visível.
//! A identidade da instância acompanha cada linha: séries que recomeçam entre
//! instâncias nunca se confundem, e o registro é append-only, sem dedupe.
//!
//! Contrato de cada caminho, com o fechamento exato de cada falha:
//! - `pedir`: a expiração de uma pendência anterior é gravada antes de o
//!   pedido novo nascer; sem gravação, a porta fecha e o pedido não nasce.
//! - `resolver`: negar grava `negada`; vencer grava `expirada`; recusa de
//!   pedido desconhecido não grava — a série é da porta que a emitiu. Na
//!   falha da gravação, a recusa vale e a porta PERMANECE ABERTA: a falha
//!   é devolvida e logada, sem reabrir o pedido.
//! - `efetuar`: vencer na entrada grava `expirada` com o instante medido
//!   pela porta; vencer depois da auditoria grava `expirada` pelo gancho —
//!   nunca duas vezes. Gravação falha no gancho fecha a porta e nega o
//!   efeito; gravação falha na entrada nega o efeito com a porta
//!   PERMANECENDO ABERTA, falha devolvida e logada.
//! - `parar`: o fechamento nunca é bloqueado; havendo pendência, grava
//!   `fechada` — gravação falha deixa o fechamento valendo.
//!
//! Lacunas declaradas, condições antes da ativação:
//! - O gancho de intenção de `efetuar` não grava registro nesta fatia. A
//!   gravação durável da intenção, ordenada com a revogação antes do
//!   efeito, é condição explícita da fatia de integração.
//! - Fechar a porta quando a gravação falha nos caminhos de `resolver` e
//!   na expiração de entrada de `efetuar` é decisão registrada para antes
//!   da ativação: nesta fatia a recusa já vale e a porta segue aberta.
//!
//! Enquanto essas condições não existirem, nenhum caminho de produção
//! alcança este código e a recusa `motor_macos_indisponivel` segue na
//! frente de tudo.

use std::num::NonZeroU64;

use super::porta::{Acao, Decisao, Escopo, Pedido, Porta, Recusa};
use super::recusas::{Caminho, Motivo, Registro};

/// Falha do caminho auditado. `Recusa` é a recusa da porta, já gravada
/// quando cabível; `Auditoria` é a gravação durável que falhou — a recusa ou
/// o fechamento seguem valendo, nada é executado e nada é refeito, e o
/// motivo exato segue no log.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Falha {
    Recusa(Recusa),
    Auditoria,
}

/// Porta de permissões com registro durável fail-closed por caminho. A
/// instância identifica quem gravou cada recusa; o registro é a conexão já
/// aberta em WAL com `synchronous=FULL` e `fullfsync` — conexão do Store
/// legado (WAL+NORMAL) não serve e é recusada na gravação.
pub(super) struct PortaAuditada<'a> {
    porta: Porta,
    instancia: NonZeroU64,
    registro: &'a Registro,
}

impl<'a> PortaAuditada<'a> {
    pub(super) fn nova(escopo: Escopo, instancia: NonZeroU64, registro: &'a Registro) -> Self {
        Self { porta: Porta::nova(escopo), instancia, registro }
    }

    pub(super) fn aceitar_aviso(&mut self, escopo: Escopo) -> Result<(), Falha> {
        self.porta.aceitar_aviso(escopo).map_err(Falha::Recusa)
    }

    pub(super) fn pedir(&mut self, acao: Acao, agora_ms: u64) -> Result<Pedido, Falha> {
        let registro = self.registro;
        let instancia = self.instancia;
        self.porta
            .pedir_auditando(acao, agora_ms, |vencida| {
                if let Err(erro) = registro.registrar(vencida, Caminho::Pedir, Motivo::Expirada, instancia, agora_ms) {
                    tracing::warn!(
                        codigo = "motor_macos_auditoria",
                        caminho = "pedir",
                        motivo = "expirada",
                        erro = %format_args!("{erro:#}"),
                        "gravação durável da recusa falhou; o pedido novo não nasce"
                    );
                    return false;
                }
                true
            })
            .map_err(Falha::Recusa)
    }

    pub(super) fn resolver(
        &mut self,
        pedido: Pedido,
        respondente: Escopo,
        decisao: Decisao,
        agora_ms: u64,
    ) -> Result<(), Falha> {
        match self.porta.resolver(pedido, respondente, decisao, agora_ms) {
            Ok(()) if matches!(decisao, Decisao::Negar) => {
                if let Err(erro) =
                    self.registro.registrar(pedido, Caminho::Resolver, Motivo::Negada, self.instancia, agora_ms)
                {
                    tracing::warn!(
                        codigo = "motor_macos_auditoria",
                        caminho = "resolver",
                        motivo = "negada",
                        erro = %format_args!("{erro:#}"),
                        "gravação durável da recusa falhou; a negação segue valendo"
                    );
                    return Err(Falha::Auditoria);
                }
                Ok(())
            }
            Err(Recusa::Expirada) => {
                if let Err(erro) =
                    self.registro.registrar(pedido, Caminho::Resolver, Motivo::Expirada, self.instancia, agora_ms)
                {
                    tracing::warn!(
                        codigo = "motor_macos_auditoria",
                        caminho = "resolver",
                        motivo = "expirada",
                        erro = %format_args!("{erro:#}"),
                        "gravação durável da recusa falhou; a expiração segue valendo"
                    );
                    return Err(Falha::Auditoria);
                }
                Err(Falha::Recusa(Recusa::Expirada))
            }
            restante => restante.map_err(Falha::Recusa),
        }
    }

    /// Caminho de `efetuar`: vencer na ENTRADA (antes de qualquer efeito)
    /// grava `expirada` com o instante medido pela porta; vencer DEPOIS da
    /// auditoria grava `expirada` pelo próprio gancho — as duas expirações
    /// nunca gravam duas vezes a mesma recusa. Gravação falha no gancho
    /// fecha a porta e nega o efeito; gravação falha na entrada nega o
    /// efeito com a porta permanecendo aberta — falha devolvida e logada,
    /// sem retry em ambos.
    ///
    /// Lacuna declarada: o gancho de intenção não grava registro nesta
    /// fatia; a gravação durável da intenção é condição da fatia de
    /// integração. Fechar a porta quando a gravação falha na entrada é
    /// decisão registrada para antes da ativação: nesta fatia a expiração
    /// já vale e a porta segue aberta. Nenhum caminho de produção alcança
    /// este código enquanto a recusa `motor_macos_indisponivel` estiver na
    /// frente de tudo.
    pub(super) fn efetuar(
        &mut self,
        pedido: Pedido,
        acao: Acao,
        agora_ms: impl FnMut() -> u64,
        efeito_imediato: impl FnOnce(Acao),
    ) -> Result<(), Falha> {
        let registro = self.registro;
        let instancia = self.instancia;
        let gravada_pela_porta = std::cell::Cell::new(false);
        let ultimo_instante = std::cell::Cell::new(0);
        let mut relogio = agora_ms;
        let resultado = self.porta.efetuar(
            pedido,
            acao,
            || {
                let agora = relogio();
                ultimo_instante.set(agora);
                agora
            },
            |pedido, _acao, evento, instante| match evento {
                super::porta::EventoAuditoria::Intencao => true,
                super::porta::EventoAuditoria::ExpiradaNaoExecutada => {
                    gravada_pela_porta.set(true);
                    if let Err(erro) =
                        registro.registrar(pedido, Caminho::Efetuar, Motivo::Expirada, instancia, instante)
                    {
                        tracing::warn!(
                            codigo = "motor_macos_auditoria",
                            caminho = "efetuar",
                            motivo = "expirada",
                            erro = %format_args!("{erro:#}"),
                            "gravação durável da recusa falhou; o efeito segue negado"
                        );
                        return false;
                    }
                    true
                }
            },
            efeito_imediato,
        );
        match resultado {
            Err(Recusa::Expirada) if !gravada_pela_porta.get() => {
                if let Err(erro) =
                    registro.registrar(pedido, Caminho::Efetuar, Motivo::Expirada, instancia, ultimo_instante.get())
                {
                    tracing::warn!(
                        codigo = "motor_macos_auditoria",
                        caminho = "efetuar",
                        motivo = "expirada",
                        erro = %format_args!("{erro:#}"),
                        "gravação durável da recusa falhou; a expiração segue valendo"
                    );
                    return Err(Falha::Auditoria);
                }
                Err(Falha::Recusa(Recusa::Expirada))
            }
            restante => restante.map_err(Falha::Recusa),
        }
    }

    pub(super) fn parar(&mut self, agora_ms: u64) -> Result<(), Falha> {
        let pendente = self.porta.pendente();
        self.porta.parar(agora_ms);
        let Some(pedido) = pendente else {
            return Ok(());
        };
        self.registro.registrar(pedido, Caminho::Parar, Motivo::Fechada, self.instancia, agora_ms).map_err(|erro| {
            tracing::warn!(
                codigo = "motor_macos_auditoria",
                caminho = "parar",
                motivo = "fechada",
                erro = %format_args!("{erro:#}"),
                "gravação durável da recusa falhou; o fechamento segue valendo"
            );
            Falha::Auditoria
        })
    }
}
