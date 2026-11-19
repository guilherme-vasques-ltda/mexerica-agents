//! Observação causal por instância, compilada somente nos testes. Não altera guardas nem desbloqueia workers.
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Evento {
    ProntaParaPedido,
    CapturaIniciada,
    CapturaConcluida,
    TrabalhoIniciado,
    TrabalhoConcluido,
    SessaoInvalidada(bool),
    DrainPendente,
    WorkerDrainPendente,
    ServidorConcluido,
}

#[derive(Clone, Copy)]
pub(super) enum Etapa {
    Captura,
    Trabalho,
}

#[derive(Default)]
pub(super) struct Sincronia(Mutex<Option<UnboundedSender<Evento>>>);

impl Sincronia {
    pub(super) fn observar(&self) -> UnboundedReceiver<Evento> {
        let (tx, rx) = unbounded_channel();
        *self.0.lock().expect("observador") = Some(tx);
        rx
    }

    pub(super) fn emitir(&self, evento: Evento) {
        if let Some(tx) = self.0.lock().expect("observador").as_ref() {
            let _ = tx.send(evento);
        }
    }

    pub(super) fn invalidada(&self, viva: &AtomicBool) {
        // Observa o resultado da guarda real: o hook nunca desliga a sessão por conta própria.
        self.emitir(Evento::SessaoInvalidada(viva.load(Ordering::Acquire)));
    }

    /// Só sinaliza se o await real do worker for sondado e permanecer pendente.
    pub(super) async fn aguardar<T>(
        &self,
        mut trabalho: tokio::task::JoinHandle<T>,
    ) -> Result<T, tokio::task::JoinError> {
        use std::{future::Future, pin::Pin};
        let mut observado = false;
        std::future::poll_fn(|cx| {
            let estado = Pin::new(&mut trabalho).poll(cx);
            if estado.is_pending() && !observado {
                observado = true;
                self.emitir(Evento::WorkerDrainPendente);
            }
            estado
        })
        .await
    }

    pub(super) fn iniciar(&self, tipo: &str, etapa: Etapa) -> Conclusao<'_> {
        let eventos = (tipo == "root-accept").then_some(match etapa {
            Etapa::Captura => (Evento::CapturaIniciada, Evento::CapturaConcluida),
            Etapa::Trabalho => (Evento::TrabalhoIniciado, Evento::TrabalhoConcluido),
        });
        if let Some((inicio, _)) = eventos {
            self.emitir(inicio);
        }
        Conclusao { sincronia: self, evento: eventos.map(|(_, fim)| fim) }
    }
}

pub(super) struct Conclusao<'a> {
    sincronia: &'a Sincronia,
    evento: Option<Evento>,
}

impl Drop for Conclusao<'_> {
    fn drop(&mut self) {
        if let Some(evento) = self.evento {
            self.sincronia.emitir(evento);
        }
    }
}
