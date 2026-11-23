//! Barreira terminal do canal autenticado: tudo o que sai para o aparelho passa pelo epoch de
//! quando o canal abriu. Depois de um bump, nada mais sai além do `reject` terminal e do 4003.
use super::{Hub, RejectCode, reject};
use crate::fork::workspace::{self, Epoca};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::watch;

pub(super) struct Barreira {
    hub: Arc<Hub>,
    key: String,
    epoch: u64,
    aviso: watch::Receiver<Epoca>,
}

impl Barreira {
    /// `aviso` é assinado antes do handshake: um bump durante a autenticação também fecha.
    pub(super) fn nova(hub: Arc<Hub>, key: String, aviso: watch::Receiver<Epoca>) -> Self {
        let epoch = aviso.borrow().valor;
        Self { hub, key, epoch, aviso }
    }

    /// `Some(reject)` quando o epoch mudou. O próprio aparelho revogado recebe `device_revoked`.
    pub(super) fn terminal(&self) -> Option<Value> {
        let atual = self.aviso.borrow().clone();
        if atual.valor == self.epoch {
            return None;
        }
        let revogado = self.hub.store.device(&self.key).is_none_or(|d| workspace::bloqueado(&self.hub, &d));
        Some(if revogado { reject(RejectCode::Revoked) } else { workspace::reject_canal(&atual) })
    }

    /// Termina quando o epoch muda; sem barreira (pareamento, bootstrap), nunca.
    pub(super) async fn mudou(barreira: Option<&mut Self>) {
        let Some(b) = barreira else { return std::future::pending().await };
        while b.aviso.borrow_and_update().valor == b.epoch {
            if b.aviso.changed().await.is_err() {
                return std::future::pending().await;
            }
        }
    }
}
