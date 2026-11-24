use std::{collections::BTreeSet, num::NonZeroU64};

const PRAZO_PEDIDO_MS: u64 = 300_000;
const PRAZO_PARADA_MS: u64 = 2_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Escopo {
    pub(super) titular: NonZeroU64,
    pub(super) dispositivo: NonZeroU64,
    pub(super) bot: NonZeroU64,
    pub(super) sessao: NonZeroU64,
    pub(super) geracao: NonZeroU64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Alvo {
    Autorizavel(NonZeroU64),
    Proibido,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Acao {
    Gravar { alvo: Alvo, conteudo: NonZeroU64 },
    Apagar(Alvo),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Decisao {
    UmaVez,
    Negar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EventoAuditoria {
    Intencao,
    ExpiradaNaoExecutada,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Pedido {
    escopo: Escopo,
    serie: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Recusa {
    Fechada,
    SemAceite,
    Escopo,
    AlvoProibido,
    Ocupada,
    PedidoInvalido,
    Expirada,
    JaDecidida,
    SemDecisao,
    AcaoDiferente,
    Auditoria,
}

#[derive(Debug)]
enum EstadoPorta {
    SemAceite,
    Aberta,
    Fechada,
}

#[derive(Debug)]
enum EstadoPedido {
    Pendente,
    Aprovado,
}

struct Solicitacao {
    pedido: Pedido,
    acao: Acao,
    criada_ms: u64,
    estado: EstadoPedido,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Instancia {
    pub(super) pid: NonZeroU64,
    pub(super) inicio: NonZeroU64,
}

#[derive(Clone, Copy)]
pub(super) enum Evidencia {
    SemCenso,
    Viva(Instancia),
    UnidadeFechada(Escopo),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Parada {
    NaoSolicitada,
    EmCurso,
    Sobrevivente(Instancia),
    NaoConfirmada,
    ConfirmadaNoPrazo,
    ConfirmadaAtrasada,
}

pub(super) struct Porta {
    escopo: Escopo,
    estado: EstadoPorta,
    ultima_serie: u64,
    solicitacao: Option<Solicitacao>,
    processos: BTreeSet<Instancia>,
    parada_ms: Option<u64>,
}

impl Porta {
    pub(super) fn nova(escopo: Escopo) -> Self {
        Self {
            escopo,
            estado: EstadoPorta::SemAceite,
            ultima_serie: 0,
            solicitacao: None,
            processos: BTreeSet::new(),
            parada_ms: None,
        }
    }

    pub(super) fn aceitar_aviso(&mut self, escopo: Escopo) -> Result<(), Recusa> {
        if matches!(self.estado, EstadoPorta::Fechada) {
            return Err(Recusa::Fechada);
        }
        if self.escopo != escopo {
            return Err(Recusa::Escopo);
        }
        self.estado = EstadoPorta::Aberta;
        Ok(())
    }

    pub(super) fn pedir(&mut self, acao: Acao, agora_ms: u64) -> Result<Pedido, Recusa> {
        self.exigir_aberta()?;
        let (Acao::Gravar { alvo, .. } | Acao::Apagar(alvo)) = acao;
        if alvo == Alvo::Proibido {
            return Err(Recusa::AlvoProibido);
        }
        self.expirar(agora_ms);
        if self.solicitacao.is_some() {
            return Err(Recusa::Ocupada);
        }
        let Some(serie) = self.ultima_serie.checked_add(1) else {
            self.parar(agora_ms);
            return Err(Recusa::Fechada);
        };
        self.ultima_serie = serie;
        let pedido = Pedido { escopo: self.escopo, serie };
        self.solicitacao = Some(Solicitacao { pedido, acao, criada_ms: agora_ms, estado: EstadoPedido::Pendente });
        Ok(pedido)
    }

    pub(super) fn resolver(
        &mut self,
        pedido: Pedido,
        respondente: Escopo,
        decisao: Decisao,
        agora_ms: u64,
    ) -> Result<(), Recusa> {
        if self.escopo != respondente {
            return Err(Recusa::Escopo);
        }
        let solicitacao = self.solicitacao(pedido, agora_ms)?;
        if matches!(solicitacao.estado, EstadoPedido::Aprovado) {
            return Err(Recusa::JaDecidida);
        }
        match decisao {
            Decisao::UmaVez => solicitacao.estado = EstadoPedido::Aprovado,
            Decisao::Negar => self.solicitacao = None,
        }
        Ok(())
    }

    pub(super) fn efetuar(
        &mut self,
        pedido: Pedido,
        acao: Acao,
        mut agora_ms: impl FnMut() -> u64,
        mut auditar: impl FnMut(Pedido, Acao, EventoAuditoria) -> bool,
        efeito_imediato: impl FnOnce(Acao),
    ) -> Result<(), Recusa> {
        let antes_ms = agora_ms();
        let solicitacao = self.solicitacao(pedido, antes_ms)?;
        if solicitacao.acao != acao {
            return Err(Recusa::AcaoDiferente);
        }
        if matches!(solicitacao.estado, EstadoPedido::Pendente) {
            return Err(Recusa::SemDecisao);
        }
        let criada_ms = solicitacao.criada_ms;
        self.solicitacao = None;
        if !auditar(pedido, acao, EventoAuditoria::Intencao) {
            self.parar(agora_ms().max(antes_ms));
            return Err(Recusa::Auditoria);
        }
        let depois_ms = agora_ms();
        if depois_ms < antes_ms || depois_ms - criada_ms >= PRAZO_PEDIDO_MS {
            if !auditar(pedido, acao, EventoAuditoria::ExpiradaNaoExecutada) {
                self.parar(agora_ms().max(depois_ms).max(antes_ms));
                return Err(Recusa::Auditoria);
            }
            return Err(Recusa::Expirada);
        }
        efeito_imediato(acao);
        Ok(())
    }

    pub(super) fn parar(&mut self, agora_ms: u64) {
        self.estado = EstadoPorta::Fechada;
        self.solicitacao = None;
        self.parada_ms.get_or_insert(agora_ms);
    }

    pub(super) fn registrar(&mut self, instancia: Instancia) -> bool {
        self.exigir_aberta().is_ok() && self.processos.insert(instancia)
    }

    pub(super) fn confirmar_saida(&mut self, instancia: Instancia) -> bool {
        self.processos.remove(&instancia)
    }

    pub(super) fn alvos_da_parada(&self) -> Option<&BTreeSet<Instancia>> {
        self.parada_ms.map(|_| &self.processos)
    }

    pub(super) fn estado(&self, agora_ms: u64, evidencia: Evidencia) -> Parada {
        let Some(parada_ms) = self.parada_ms else {
            return Parada::NaoSolicitada;
        };
        let decorrido = agora_ms.checked_sub(parada_ms);
        match evidencia {
            Evidencia::Viva(instancia) => return Parada::Sobrevivente(instancia),
            Evidencia::UnidadeFechada(escopo) if escopo == self.escopo && self.processos.is_empty() => {
                return match decorrido {
                    Some(ms) if ms <= PRAZO_PARADA_MS => Parada::ConfirmadaNoPrazo,
                    Some(_) => Parada::ConfirmadaAtrasada,
                    None => Parada::NaoConfirmada,
                };
            }
            Evidencia::UnidadeFechada(_) | Evidencia::SemCenso => {}
        }
        match decorrido {
            Some(decorrido) if decorrido < PRAZO_PARADA_MS => Parada::EmCurso,
            _ => Parada::NaoConfirmada,
        }
    }

    fn exigir_aberta(&self) -> Result<(), Recusa> {
        match self.estado {
            EstadoPorta::SemAceite => Err(Recusa::SemAceite),
            EstadoPorta::Aberta => Ok(()),
            EstadoPorta::Fechada => Err(Recusa::Fechada),
        }
    }

    fn expirar(&mut self, agora_ms: u64) -> bool {
        let venceu = self
            .solicitacao
            .as_ref()
            .is_some_and(|s| agora_ms.checked_sub(s.criada_ms).is_none_or(|idade| idade >= PRAZO_PEDIDO_MS));
        if venceu {
            self.solicitacao = None;
        }
        venceu
    }

    fn solicitacao(&mut self, pedido: Pedido, agora_ms: u64) -> Result<&mut Solicitacao, Recusa> {
        self.exigir_aberta()?;
        if self.escopo != pedido.escopo {
            return Err(Recusa::Escopo);
        }
        if self.solicitacao.as_ref().is_none_or(|s| s.pedido != pedido) {
            return Err(Recusa::PedidoInvalido);
        }
        if self.expirar(agora_ms) {
            return Err(Recusa::Expirada);
        }
        self.solicitacao.as_mut().ok_or(Recusa::PedidoInvalido)
    }
}
