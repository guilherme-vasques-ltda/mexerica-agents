//! Auditoria fail-closed por caminho da porta de permissões do Motor no
//! macOS. Cada recusa — pedir, expirar, negar, fechar — é gravada uma única
//! vez no registro durável antes de o método retornar, e a intenção aprovada
//! é registrada antes do efeito, na mesma sequência append-only: qualquer
//! revogação seguinte fica depois dela, ordenada. Se a gravação falha, o
//! efeito negado permanece negado, a porta FECHA em todos os caminhos, nada
//! é refeito e a falha fica visível. A identidade da instância acompanha
//! cada linha: séries que recomeçam entre instâncias nunca se confundem, e
//! o registro é append-only, sem dedupe.
//!
//! Contrato de cada caminho:
//! - `pedir`: a expiração de uma pendência anterior é gravada antes de o
//!   pedido novo nascer; sem gravação, a porta fecha e o pedido não nasce.
//! - `resolver`: negar grava `negada`; vencer grava `expirada`; recusa de
//!   pedido desconhecido não grava — a série é da porta que a emitiu. Na
//!   falha da gravação, a recusa vale e a porta fecha.
//! - `efetuar`: vencer na entrada grava `expirada` com o instante medido
//!   pela porta; vencer depois da auditoria grava `expirada` pelo gancho —
//!   nunca duas vezes. A intenção aprovada é gravada ANTES do efeito pelo
//!   gancho de intenção. Gravação falha em qualquer desses pontos nega o
//!   efeito e fecha a porta.
//! - `parar`: o fechamento nunca é bloqueado; havendo pendência, grava
//!   `fechada` — gravação falha deixa o fechamento valendo.
//!
//! Projeção de log: só campos fixos (código, caminho, motivo) — nenhuma
//! mensagem dinâmica de erro entra no log, porque a análise atual não
//! garante ausência de caminho do banco em todas as mensagens do SQLite; a
//! causa permanece no retorno do registro, nunca em texto de log.
//!
//! Enquanto este contrato for inerte, nenhum caminho de produção alcança
//! este código e a recusa `motor_macos_indisponivel` segue na frente de
//! tudo.

use std::num::NonZeroU64;

use super::porta::{Acao, Decisao, Escopo, Pedido, Porta, Recusa};
use super::recusas::{Caminho, Motivo, Registro};

/// Falha do caminho auditado. `Recusa` é a recusa da porta, já gravada
/// quando cabível; `Auditoria` é a gravação durável que falhou — a recusa ou
/// o fechamento seguem valendo, a porta está fechada, nada é executado e
/// nada é refeito.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Falha {
    Recusa(Recusa),
    Auditoria,
}

/// Porta de permissões com registro durável fail-closed por caminho. A
/// instância identifica quem gravou cada linha; o registro é a conexão já
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

    /// Caminho de `pedir`: a expiração de uma pendência anterior é gravada
    /// antes de o pedido novo nascer. Sem gravação, a porta fecha e o pedido
    /// não nasce.
    pub(super) fn pedir(&mut self, acao: Acao, agora_ms: u64) -> Result<Pedido, Falha> {
        let registro = self.registro;
        let instancia = self.instancia;
        self.porta
            .pedir_auditando(acao, agora_ms, |vencida| {
                if let Err(_erro) = registro.registrar(vencida, Caminho::Pedir, Motivo::Expirada, instancia, agora_ms) {
                    tracing::warn!(
                        codigo = "motor_macos_auditoria",
                        caminho = "pedir",
                        motivo = "expirada",
                        "gravação durável da recusa falhou; o pedido novo não nasce"
                    );
                    return false;
                }
                true
            })
            .map_err(Falha::Recusa)
    }

    /// Caminho de `resolver`: negar grava `negada`; vencer grava `expirada`;
    /// recusa de pedido desconhecido não grava — a série é da porta que a
    /// emitiu. Na falha da gravação, a recusa vale e a porta fecha.
    pub(super) fn resolver(
        &mut self,
        pedido: Pedido,
        respondente: Escopo,
        decisao: Decisao,
        agora_ms: u64,
    ) -> Result<(), Falha> {
        match self.porta.resolver(pedido, respondente, decisao, agora_ms) {
            Ok(()) if matches!(decisao, Decisao::Negar) => {
                if let Err(_erro) =
                    self.registro.registrar(pedido, Caminho::Resolver, Motivo::Negada, self.instancia, agora_ms)
                {
                    self.porta.parar(agora_ms);
                    tracing::warn!(
                        codigo = "motor_macos_auditoria",
                        caminho = "resolver",
                        motivo = "negada",
                        "gravação durável da recusa falhou; a negação vale e a porta fecha"
                    );
                    return Err(Falha::Auditoria);
                }
                Ok(())
            }
            Err(Recusa::Expirada) => {
                if let Err(_erro) =
                    self.registro.registrar(pedido, Caminho::Resolver, Motivo::Expirada, self.instancia, agora_ms)
                {
                    self.porta.parar(agora_ms);
                    tracing::warn!(
                        codigo = "motor_macos_auditoria",
                        caminho = "resolver",
                        motivo = "expirada",
                        "gravação durável da recusa falhou; a expiração vale e a porta fecha"
                    );
                    return Err(Falha::Auditoria);
                }
                Err(Falha::Recusa(Recusa::Expirada))
            }
            restante => restante.map_err(Falha::Recusa),
        }
    }

    /// Caminho de `efetuar`: a intenção aprovada é gravada ANTES do efeito,
    /// na mesma sequência das recusas; vencer na entrada grava `expirada`
    /// com o instante medido pela porta; vencer depois da auditoria grava
    /// `expirada` pelo próprio gancho — as duas expirações nunca gravam duas
    /// vezes a mesma recusa. Gravação falha em qualquer ponto nega o efeito
    /// e fecha a porta, sem retry.
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
                super::porta::EventoAuditoria::Intencao => {
                    if let Err(_erro) =
                        registro.registrar(pedido, Caminho::Efetuar, Motivo::Intencao, instancia, instante)
                    {
                        tracing::warn!(
                            codigo = "motor_macos_auditoria",
                            caminho = "efetuar",
                            motivo = "intencao",
                            "gravação durável da intenção falhou; o efeito não roda e a porta fecha"
                        );
                        return false;
                    }
                    true
                }
                super::porta::EventoAuditoria::ExpiradaNaoExecutada => {
                    gravada_pela_porta.set(true);
                    if let Err(_erro) =
                        registro.registrar(pedido, Caminho::Efetuar, Motivo::Expirada, instancia, instante)
                    {
                        tracing::warn!(
                            codigo = "motor_macos_auditoria",
                            caminho = "efetuar",
                            motivo = "expirada",
                            "gravação durável da recusa falhou; o efeito segue negado e a porta fecha"
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
                if let Err(_erro) =
                    registro.registrar(pedido, Caminho::Efetuar, Motivo::Expirada, instancia, ultimo_instante.get())
                {
                    self.porta.parar(ultimo_instante.get());
                    tracing::warn!(
                        codigo = "motor_macos_auditoria",
                        caminho = "efetuar",
                        motivo = "expirada",
                        "gravação durável da recusa falhou; a expiração vale e a porta fecha"
                    );
                    return Err(Falha::Auditoria);
                }
                Err(Falha::Recusa(Recusa::Expirada))
            }
            restante => restante.map_err(Falha::Recusa),
        }
    }

    /// Estado da parada com evidência: só a porta aberta expõe; sem
    /// evidência nunca é sucesso falso.
    pub(super) fn estado(&self, agora_ms: u64, evidencia: super::porta::Evidencia) -> super::porta::Parada {
        self.porta.estado(agora_ms, evidencia)
    }

    /// Registra descendente observado da unidade: entra no censo que torna
    /// sobrevivente visível na parada — não abre partida.
    pub(super) fn registrar(&mut self, instancia: super::porta::Instancia) -> bool {
        self.porta.registrar(instancia)
    }

    /// Registra a instância principal observada (o Motor): única marca que
    /// abre a partida; descendente sozinho não basta.
    pub(super) fn registrar_motor(&mut self, instancia: super::porta::Instancia) -> bool {
        self.porta.registrar_motor(instancia)
    }

    /// Confirma a saída de uma instância registrada; a saída confirmada do
    /// motor retira a marca que abre a partida.
    pub(super) fn confirmar_saida(&mut self, instancia: super::porta::Instancia) -> bool {
        self.porta.confirmar_saida(instancia)
    }

    /// A porta está fechada por qualquer caminho (parar ou falha de
    /// auditoria): quem coordena fecha o que depende dela.
    pub(super) fn fechada(&self) -> bool {
        self.porta.fechada()
    }

    /// O aceite do aviso desta geração aconteceu?
    pub(super) fn aceita(&self) -> bool {
        self.porta.aceita()
    }

    /// A instância principal (o Motor) está no censo, sem saída confirmada?
    pub(super) fn motor_no_censo(&self) -> bool {
        self.porta.motor_no_censo()
    }

    /// Caminho de `parar` ("Parar tudo" na porta): fecha sem bloquear;
    /// havendo pendência, grava `fechada`. Gravação falha deixa o
    /// fechamento valendo, com a falha devolvida.
    pub(super) fn parar(&mut self, agora_ms: u64) -> Result<(), Falha> {
        let pendente = self.porta.pendente();
        self.porta.parar(agora_ms);
        let Some(pedido) = pendente else {
            return Ok(());
        };
        self.registro.registrar(pedido, Caminho::Parar, Motivo::Fechada, self.instancia, agora_ms).map_err(|_erro| {
            tracing::warn!(
                codigo = "motor_macos_auditoria",
                caminho = "parar",
                motivo = "fechada",
                "gravação durável da recusa falhou; o fechamento segue valendo"
            );
            Falha::Auditoria
        })
    }
}
