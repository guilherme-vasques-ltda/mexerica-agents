//! Barreira terminal do canal autenticado: tudo o que sai para o aparelho passa pelo epoch de
//! quando o canal abriu e pelo prazo da sessão Clerk (Hub com vínculo). Com um aumento pendente de
//! commit, a saída espera o desfecho; depois que o número avança, nada mais sai além do `reject`
//! terminal e do 4003. A troca de papel registrada sobe o epoch: o canal confere a geração que o
//! admitiu, não o papel atual (`role_changed` ou `workspace_revoked`, com 4003). No fim do prazo da
//! sessão, o mesmo corte com `auth_invalid` e 4001. Vale no escritor do canal e, pela `Porta`, no
//! último ponto antes do socket direto ou do relay.
use super::{Hub, Out, RejectCode, reject};
use crate::fork::sessao_clerk::{self, Limite};
use crate::fork::workspace::{self, Epoca};
use futures::{Stream, stream};
use serde_json::Value;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

/// O último recado do canal e o código de fechamento que vem depois dele.
#[derive(Clone, Debug)]
pub(crate) struct Terminal {
    pub(crate) reject: Value,
    pub(crate) close: u16,
}

#[derive(Clone)]
pub(crate) struct Barreira {
    hub: Arc<Hub>,
    key: String,
    epoch: u64,
    aviso: watch::Receiver<Epoca>,
    /// Fim da sessão (`None`: sem prazo, Hub sem vínculo).
    prazo: watch::Receiver<Option<Limite>>,
}

impl Barreira {
    /// `epoch` é lido antes do handshake: um bump durante a autenticação também fecha.
    pub(crate) fn nova(
        hub: Arc<Hub>,
        key: String,
        aviso: watch::Receiver<Epoca>,
        epoch: u64,
        prazo: watch::Receiver<Option<Limite>>,
    ) -> Self {
        Self { hub, key, epoch, aviso, prazo }
    }

    /// O terminal da sessão: prazo vencido (4001).
    fn sessao(&self) -> Option<Terminal> {
        let limite = (*self.prazo.borrow())?;
        limite
            .vencido()
            .then(|| Terminal { reject: sessao_clerk::reject_sessao(), close: RejectCode::Unauthorized.close_code() })
    }

    /// `Some` quando o número publicado avançou desde a abertura (o próprio aparelho revogado
    /// recebe `device_revoked`; a leitura espera o commit da revogação) ou quando a sessão acabou.
    pub(super) fn terminal(&self) -> Option<Terminal> {
        let atual = self.aviso.borrow().clone();
        if atual.avancou(self.epoch) {
            let revogado = self.hub.store.device(&self.key).is_none_or(|d| workspace::bloqueado(&self.hub, &d));
            let reason = if revogado { "device_revoked" } else { "workspace_epoch_advanced" };
            tracing::warn!(reason, origin = "aparelho", epoch = atual.valor, opened = self.epoch, "device channel cut");
            let reject = if revogado { reject(RejectCode::Revoked) } else { workspace::reject_canal(&atual) };
            return Some(Terminal { reject, close: RejectCode::Revoked.close_code() });
        }
        self.sessao()
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
    Fechar(u16),
    Fim,
}

impl Porta {
    pub(super) fn armar(&self, barreira: Barreira) {
        let _ = self.0.set(barreira);
    }

    /// Porta já armada para o aparelho `key` no epoch atual.
    #[cfg(test)]
    pub(crate) fn armada(hub: &Arc<Hub>, key: &str) -> Self {
        let porta = Self::default();
        let (_, prazo) = watch::channel(None);
        porta.armar(Barreira::nova(hub.clone(), key.to_owned(), hub.workspace.assinar(), hub.workspace.atual(), prazo));
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
                Fase::Fechar(close) => {
                    let fim = Out::Close(close, "rejected");
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
                    return Some((Out::Msg(t.reject), (porta, rx, barreira, Fase::Fechar(t.close))));
                }
                if let Some(out) = out {
                    let fase = if matches!(out, Out::Close(..)) { Fase::Fim } else { Fase::Aberta };
                    return Some((out, (porta, rx, barreira, fase)));
                }
            }
        })
    }
}
