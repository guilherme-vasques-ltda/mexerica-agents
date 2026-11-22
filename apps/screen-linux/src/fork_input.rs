//! Consumidor usado pelo canal WebRTC; separado para observar seu efeito em fixture.
use crate::lifecycle::Operation;
use crate::portal::Display;
use crate::stream::{View, error_msg, send, set_clipboard};
use crate::{Helper, input};
use serde_json::Value;
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::mpsc;

struct Consumer {
    /// Operation original da Session; fraca para não prender o ciclo de vida.
    original: Weak<Operation>,
    helper: Arc<Helper>,
    channels: Arc<Mutex<Vec<gst_webrtc::WebRTCDataChannel>>>,
    display: Display,
    view: View,
    #[cfg(test)]
    cut: Option<Cut>,
}

pub(crate) async fn run(
    rx: mpsc::UnboundedReceiver<Value>,
    original: Weak<Operation>,
    helper: Arc<Helper>,
    channels: Arc<Mutex<Vec<gst_webrtc::WebRTCDataChannel>>>,
    display: Display,
    view: View,
) {
    consume(
        Consumer {
            original,
            helper,
            channels,
            display,
            view,
            #[cfg(test)]
            cut: None,
        },
        rx,
    )
    .await;
}

async fn consume(consumer: Consumer, mut rx: mpsc::UnboundedReceiver<Value>) {
    #[cfg(test)]
    let mut cut = consumer.cut;
    while let Some(msg) = rx.recv().await {
        #[cfg(test)]
        if let Some(cut) = cut.take() {
            // Corte depois do recebimento, antes de portal/perform. Só o teste
            // detém e retoma o MESMO Value; nenhuma política é simulada aqui.
            let _ = cut.received.send(());
            if cut.resume.await.is_err() {
                return;
            }
        }
        // Value aceito antes do close não produz efeito depois dele.
        // Close é monotônico; encerrar descarta este e os pendentes.
        if !open(&consumer.original) {
            return;
        }
        let reply = match msg["type"].as_str() {
            Some("view") => Some(consumer.view.apply(&msg)),
            Some("clipboard") => match consumer.helper.portal().await {
                // Requer portal ativo: revogação do compartilhamento pela
                // interface do sistema nega também a escrita na área de
                // transferência, como já negava os eventos de entrada.
                // Reconfere depois do await do portal, imediatamente antes do efeito.
                Ok(_) if !open(&consumer.original) => return,
                Ok(_) => set_clipboard(msg["text"].as_str().unwrap_or_default())
                    .await
                    .err()
                    .as_ref()
                    .map(error_msg),
                // Portal revogado: nenhum efeito e nenhuma resposta ao par revogado.
                Err(_) => None,
            },
            _ => match consumer.helper.portal().await {
                // Reconfere depois do await do portal, imediatamente antes do efeito.
                Ok(_) if !open(&consumer.original) => return,
                Ok(portal) => {
                    // Cancelamento percorre perform: sessão fechada a meio
                    // interrompe os efeitos restantes e libera o que segurou.
                    let original = consumer.original.clone();
                    let alive = move || open(&original);
                    input::perform(&portal, &consumer.display, &msg, &alive)
                        .await
                        .err()
                        .as_ref()
                        .map(error_msg)
                }
                Err(error) => Some(error_msg(&error)),
            },
        };
        if let Some(reply) = reply {
            send(&consumer.channels, &reply);
        }
    }
}

/// Operation original ainda aberta. Trava curta e síncrona em `session()`,
/// liberada antes de qualquer await; descartada (Weak vazio) conta como fechada.
fn open(original: &Weak<Operation>) -> bool {
    original
        .upgrade()
        .is_some_and(|operation| operation.session().is_ok())
}

#[cfg(test)]
struct Cut {
    received: tokio::sync::oneshot::Sender<()>,
    resume: tokio::sync::oneshot::Receiver<()>,
}

#[cfg(test)]
#[path = "fork_input_tests.rs"]
mod tests;
