//! Barreira terminal do canal autenticado: tudo o que sai para o aparelho passa pelo epoch de
//! quando o canal abriu e pelo prazo da sessão Clerk (Hub com vínculo). Com um aumento pendente de
//! commit, a saída espera o desfecho; depois que o número avança, só sai o terminal.
//! O terminal confere a autoridade que admitiu o canal, não a captura de uma sessão nova. No fim do prazo da
//! sessão, o mesmo corte com `auth_invalid` e 4001. Vale no escritor do canal e, pela `Porta`, no
//! último ponto antes do socket direto ou do relay.
use super::{Caller, Hub, Out, RejectCode, close, reject};
use crate::LockExt;
use crate::fork::autoridade;
use crate::fork::sessao_clerk::{self, Limite};
use crate::fork::workspace::{self, Epoca};
use futures::{Stream, stream};
use rusqlite::OptionalExtension;
use serde_json::Value;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

/// O último recado do canal e o código de fechamento que vem depois dele.
#[derive(Clone, Debug)]
pub(crate) struct Terminal {
    pub(crate) reject: Option<Value>,
    pub(crate) close: u16,
    pub(crate) reason: &'static str,
}

#[derive(Clone)]
pub(crate) struct Barreira {
    hub: Arc<Hub>,
    caller: Caller,
    anuncia_corte: Arc<AtomicBool>,
    epoch: u64,
    geracao: Option<i64>,
    aviso: watch::Receiver<Epoca>,
    /// Fim da sessão (`None`: sem prazo, Hub sem vínculo).
    prazo: watch::Receiver<Option<Limite>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Admissao {
    pub(super) epoch: u64,
    pub(super) geracao: i64,
}

#[cfg(test)]
impl Admissao {
    pub(super) fn do_caller(hub: &Hub, caller: &Caller) -> Option<Self> {
        let Caller::Device { autoridade: Some(a), .. } = caller else { return None };
        Some(Self { epoch: hub.workspace.atual(), geracao: a.geracao_capturada()? })
    }
}

impl Barreira {
    /// `epoch` é lido antes do handshake: um bump durante a autenticação também fecha.
    pub(crate) fn nova(
        hub: Arc<Hub>,
        caller: Caller,
        aviso: watch::Receiver<Epoca>,
        epoch: u64,
        prazo: watch::Receiver<Option<Limite>>,
        geracao: Option<i64>,
    ) -> Self {
        Self { hub, caller, anuncia_corte: Arc::default(), epoch, geracao, aviso, prazo }
    }

    /// O terminal da sessão: prazo vencido (4001).
    fn sessao(&self) -> Option<Terminal> {
        let limite = (*self.prazo.borrow())?;
        limite.vencido().then(|| Terminal {
            reject: Some(sessao_clerk::reject_sessao()),
            close: RejectCode::Unauthorized.close_code(),
            reason: "rejected",
        })
    }

    pub(super) fn admissao(&self) -> Option<Admissao> {
        Some(Admissao { epoch: self.epoch, geracao: self.geracao? })
    }

    /// `Some` quando o número publicado avançou desde a abertura (o próprio aparelho revogado
    /// recebe `device_revoked`; a leitura espera o commit da revogação) ou quando a sessão acabou.
    pub(super) fn terminal(&self) -> Option<Terminal> {
        let atual = self.aviso.borrow().clone();
        if atual.avancou(self.epoch) {
            let terminal = self.terminal_do_corte(&atual);
            tracing::warn!(
                reason = terminal.reason,
                origin = "aparelho",
                epoch = atual.valor,
                opened = self.epoch,
                "device channel cut"
            );
            return Some(terminal);
        }
        self.sessao()
    }

    fn terminal_do_corte(&self, atual: &Epoca) -> Terminal {
        let incerto = Terminal { reject: None, close: close::TIMEOUT, reason: "tryAgain" };
        let Caller::Device { key, .. } = &self.caller else { return incerto };
        let revogado = (|| -> anyhow::Result<bool> {
            let db = self.hub.store.db.locked();
            let grant: Option<Option<String>> =
                db.query_row("SELECT grant_id FROM devices WHERE key=?1", [key], |r| r.get(0)).optional()?;
            match grant {
                None => Ok(true),
                Some(grant) => workspace::bloqueado_on(&db, key, grant.as_deref()),
            }
        })();
        match revogado {
            Ok(true) => {
                return Terminal {
                    reject: Some(reject(RejectCode::Revoked)),
                    close: RejectCode::Revoked.close_code(),
                    reason: "rejected",
                };
            }
            Ok(false) => {}
            Err(_) => return incerto,
        }
        if !atual.pendente()
            && self.prazo.borrow().is_none()
            && !self.anuncia_corte.load(Ordering::Acquire)
            && autoridade::reconferir_terminal(&self.hub, &self.caller).is_ok()
            && !self.hub.workspace.retrato().pendente()
        {
            Terminal { reject: None, close: close::REKEY, reason: "rekey" }
        } else {
            incerto
        }
    }

    /// Com um commit de aumento em curso, espera o desfecho: publicado, sai o terminal; desfeito
    /// (rollback, erro), o canal segue aberto. Com o número já avançado, o terminal sai sem
    /// esperar as outras marcas (a retenção do dono, por exemplo).
    pub(super) async fn resolver(&mut self) -> Option<Terminal> {
        loop {
            let atual = self.aviso.borrow_and_update().clone();
            if !atual.pendente() || atual.avancou(self.epoch) {
                break;
            }
            if self.aviso.changed().await.is_err() {
                break;
            }
        }
        self.terminal()
    }

    /// Termina quando o número avança ou a sessão acaba; sem barreira (pareamento, bootstrap), nunca.
    pub(super) async fn mudou(barreira: Option<&mut Self>) {
        let Some(b) = barreira else { return std::future::pending().await };
        loop {
            if b.aviso.borrow_and_update().avancou(b.epoch) || b.sessao().is_some() {
                return;
            }
            let espera = b.prazo.borrow_and_update().as_ref().map(Limite::resta);
            let ate_vencer = async {
                match espera {
                    Some(resta) => tokio::time::sleep(resta.max(Duration::from_millis(1))).await,
                    None => std::future::pending().await,
                }
            };
            // Sem quem mude o prazo, vale o último valor; o epoch segue vigiado.
            let prazo_vivo = b.prazo.has_changed().is_ok();
            tokio::select! {
                mudou = b.aviso.changed() => if mudou.is_err() { return std::future::pending().await },
                _ = b.prazo.changed(), if prazo_vivo => {}
                () = ate_vencer => {}
            }
        }
    }
}

/// Porta do transporte: armada pelo canal depois do handshake do aparelho, conferida pelo
/// consumidor da fila de saída (socket direto ou relay).
#[derive(Clone, Default)]
pub(crate) struct Porta(Arc<OnceLock<Barreira>>);

enum Fase {
    Aberta,
    Fechar(u16, &'static str),
    Fim,
}

impl Porta {
    pub(super) fn anunciar_corte(&self) {
        if let Some(barreira) = self.0.get() {
            barreira.anuncia_corte.store(true, Ordering::Release);
        }
    }
    pub(super) fn armar(&self, barreira: Barreira) {
        let _ = self.0.set(barreira);
    }

    /// Porta já armada para o aparelho `key` no epoch atual.
    #[cfg(test)]
    pub(crate) fn armada(hub: &Arc<Hub>, key: &str) -> Self {
        let porta = Self::default();
        let (_, prazo) = watch::channel(None);
        porta.armar(Barreira::nova(
            hub.clone(),
            autoridade::autenticar(hub, key).map_or_else(
                |_| Caller::Device { key: key.to_owned(), scopes: vec![], autoridade: None },
                |(_, caller)| caller,
            ),
            hub.workspace.assinar(),
            hub.workspace.atual(),
            prazo,
            None,
        ));
        porta
    }

    /// O terminal devido agora, para quem fecha o transporte fora da fila (o relay).
    pub(crate) fn terminal(&self) -> Option<Terminal> {
        self.0.get().and_then(Barreira::terminal)
    }

    /// A fila de saída vista pelo transporte. Depois do bump, o que ainda estiver na fila
    /// (quadros, um 4011) é descartado e o terminal passa à frente: `reject`, depois o fechamento.
    pub(crate) fn filtrar(self, rx: mpsc::Receiver<Out>) -> impl Stream<Item = Out> + Send + 'static {
        stream::unfold((self, rx, None::<Barreira>, Fase::Aberta), |(porta, mut rx, mut barreira, fase)| async move {
            match fase {
                Fase::Fim => return None,
                Fase::Fechar(close, reason) => {
                    let fim = Out::Close(close, reason);
                    return Some((fim, (porta, rx, barreira, Fase::Fim)));
                }
                Fase::Aberta => {}
            }
            loop {
                if barreira.is_none() {
                    barreira = porta.0.get().cloned();
                }
                let out = tokio::select! {
                    biased;
                    () = Barreira::mudou(barreira.as_mut()) => None,
                    out = rx.recv() => Some(out?),
                };
                if barreira.is_none() {
                    barreira = porta.0.get().cloned();
                }
                let terminal = match barreira.as_mut() {
                    Some(b) => b.resolver().await,
                    None => None,
                };
                if let Some(t) = terminal {
                    return Some(match t.reject {
                        Some(reject) => (Out::Msg(reject), (porta, rx, barreira, Fase::Fechar(t.close, t.reason))),
                        None => (Out::Close(t.close, t.reason), (porta, rx, barreira, Fase::Fim)),
                    });
                }
                if let Some(out) = out {
                    let fase = if matches!(out, Out::Close(..)) { Fase::Fim } else { Fase::Aberta };
                    return Some((out, (porta, rx, barreira, fase)));
                }
            }
        })
    }
}
