//! Barreira terminal do canal autenticado: tudo o que sai para o aparelho passa pelo epoch de
//! quando o canal abriu. Depois de um bump (ou com um aumento pendente de commit), nada mais sai
//! além do `reject` terminal e do 4003. Vale no escritor do canal e, pela `Porta`, no último
//! ponto antes do socket direto ou do relay.
use super::{Hub, Out, RejectCode, reject};
use crate::fork::workspace::{self, Epoca};
use futures::{Stream, stream};
use serde_json::Value;
use std::sync::{Arc, OnceLock};
use tokio::sync::{mpsc, watch};

#[derive(Clone)]
pub(crate) struct Barreira {
    hub: Arc<Hub>,
    key: String,
    epoch: u64,
    aviso: watch::Receiver<Epoca>,
}

impl Barreira {
    /// `aviso` é assinado antes do handshake: um bump durante a autenticação também fecha.
    pub(crate) fn nova(hub: Arc<Hub>, key: String, aviso: watch::Receiver<Epoca>) -> Self {
        let epoch = aviso.borrow().valor;
        Self { hub, key, epoch, aviso }
    }

    /// `Some(reject)` quando o epoch mudou ou um aumento está pendente de commit. O próprio
    /// aparelho revogado recebe `device_revoked` (a leitura espera o commit da revogação).
    pub(super) fn terminal(&self) -> Option<Value> {
        let atual = self.aviso.borrow().clone();
        if !atual.corta(self.epoch) {
            return None;
        }
        let revogado = self.hub.store.device(&self.key).is_none_or(|d| workspace::bloqueado(&self.hub, &d));
        Some(if revogado { reject(RejectCode::Revoked) } else { workspace::reject_canal(&atual) })
    }

    /// Termina quando o epoch muda; sem barreira (pareamento, bootstrap), nunca.
    pub(super) async fn mudou(barreira: Option<&mut Self>) {
        let Some(b) = barreira else { return std::future::pending().await };
        while !b.aviso.borrow_and_update().corta(b.epoch) {
            if b.aviso.changed().await.is_err() {
                return std::future::pending().await;
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
    Fechar,
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
        porta.armar(Barreira::nova(hub.clone(), key.to_owned(), hub.workspace.assinar()));
        porta
    }

    /// A fila de saída vista pelo transporte. Depois do bump, o que ainda estiver na fila
    /// (quadros, um 4011) é descartado e o terminal passa à frente: `reject`, depois 4003.
    pub(crate) fn filtrar(self, rx: mpsc::Receiver<Out>) -> impl Stream<Item = Out> + Send + 'static {
        stream::unfold((self, rx, None::<Barreira>, Fase::Aberta), |(porta, mut rx, mut barreira, fase)| async move {
            match fase {
                Fase::Fim => return None,
                Fase::Fechar => {
                    let fim = Out::Close(RejectCode::Revoked.close_code(), "rejected");
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
                if let Some(reject) = barreira.as_ref().and_then(Barreira::terminal) {
                    return Some((Out::Msg(reject), (porta, rx, barreira, Fase::Fechar)));
                }
                if let Some(out) = out {
                    let fase = if matches!(out, Out::Close(..)) { Fase::Fim } else { Fase::Aberta };
                    return Some((out, (porta, rx, barreira, fase)));
                }
            }
        })
    }
}
