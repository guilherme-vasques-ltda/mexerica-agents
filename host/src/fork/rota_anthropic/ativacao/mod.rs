use std::{future::Future, sync::Mutex, task::Poll, time::Duration};
use tokio::{sync::watch, time::Instant};

#[cfg(test)]
mod testes;

pub(super) struct Porta<'a> {
    estado: Mutex<Estado<'a>>,
    fechou: watch::Sender<bool>,
}

enum Estado<'a> {
    Fechada,
    Ativa { bot: &'a str, concessao: &'a str, modelo: &'a str, vence: Instant, ocupada: bool },
}

pub(super) struct Vaga<'p, 'a> {
    porta: &'p Porta<'a>,
    vence: Instant,
    fase: Fase,
}

#[derive(PartialEq, Eq)]
enum Fase {
    Retida,
    Iniciada,
    Concluida,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Recusa {
    Fechada,
    Expirada,
    Escopo,
    Ocupada,
    JaIniciada,
    NaoIniciada,
}

impl<'a> Porta<'a> {
    pub(super) fn fechada() -> Self {
        Self { estado: Mutex::new(Estado::Fechada), fechou: watch::channel(true).0 }
    }

    #[cfg(test)]
    pub(super) fn fixture(bot: &'a str, concessao: &'a str, modelo: &'a str, validade: Duration) -> Self {
        let Some(vence) = Instant::now().checked_add(validade) else { return Self::fechada() };
        Self {
            estado: Mutex::new(Estado::Ativa { bot, concessao, modelo, vence, ocupada: false }),
            fechou: watch::channel(false).0,
        }
    }

    pub(super) fn admitir(
        &self,
        bot_do_turno: &str,
        concessao_do_turno: &str,
        modelo_do_turno: &str,
    ) -> Result<Vaga<'_, 'a>, Recusa> {
        let mut estado = self.estado.lock().map_err(|_| Recusa::Fechada)?;
        self.conferir(&mut estado)?;
        let Estado::Ativa { bot, concessao, modelo, vence, ocupada } = &mut *estado else {
            return Err(Recusa::Fechada);
        };
        if (*bot, *concessao, *modelo) != (bot_do_turno, concessao_do_turno, modelo_do_turno) {
            return Err(Recusa::Escopo);
        }
        if *ocupada {
            return Err(Recusa::Ocupada);
        }
        *ocupada = true;
        Ok(Vaga { porta: self, vence: *vence, fase: Fase::Retida })
    }

    pub(super) fn revogar(&self) -> Result<(), Recusa> {
        let mut estado = self.estado.lock().map_err(|_| Recusa::Fechada)?;
        *estado = Estado::Fechada;
        self.fechou.send_replace(true);
        Ok(())
    }

    fn conferir(&self, estado: &mut Estado<'_>) -> Result<(), Recusa> {
        match estado {
            Estado::Fechada => Err(Recusa::Fechada),
            Estado::Ativa { vence, .. } if Instant::now() >= *vence => {
                *estado = Estado::Fechada;
                self.fechou.send_replace(true);
                Err(Recusa::Expirada)
            }
            Estado::Ativa { .. } => Ok(()),
        }
    }
}

impl Vaga<'_, '_> {
    pub(super) fn conferir(&self) -> Result<(), Recusa> {
        let mut estado = self.porta.estado.lock().map_err(|_| Recusa::Fechada)?;
        self.porta.conferir(&mut estado)
    }

    pub(super) fn iniciada(&self) -> bool {
        self.fase == Fase::Iniciada
    }

    pub(super) async fn enviar<F: Future>(&mut self, iniciar: impl FnOnce() -> F) -> Result<F::Output, Recusa> {
        if self.fase != Fase::Retida {
            return Err(Recusa::JaIniciada);
        }
        let mut fechou = self.porta.fechou.subscribe();
        let mut iniciar = Some(iniciar);
        let mut envio = None;
        let tentativa = std::future::poll_fn(|cx| {
            let mut estado = self.porta.estado.lock().map_err(|_| Recusa::Fechada)?;
            self.porta.conferir(&mut estado)?;
            if let Some(envio) = &mut envio {
                return Future::poll(std::pin::Pin::as_mut(envio), cx).map(Ok);
            }
            let Some(iniciar) = iniciar.take() else { return Poll::Ready(Err(Recusa::JaIniciada)) };
            self.fase = Fase::Iniciada;
            Future::poll(envio.insert(Box::pin(iniciar())).as_mut(), cx).map(Ok)
        });
        tokio::select! {
            biased;
            () = tokio::time::sleep_until(self.vence) => {
                self.porta.revogar()?;
                Err(Recusa::Expirada)
            }
            _ = fechou.wait_for(|fechou| *fechou) => Err(Recusa::Fechada),
            resultado = tentativa => resultado,
        }
    }

    pub(super) fn concluir(mut self) -> Result<(), Recusa> {
        if !self.iniciada() {
            return Err(Recusa::NaoIniciada);
        }
        let mut estado = self.porta.estado.lock().map_err(|_| Recusa::Fechada)?;
        self.porta.conferir(&mut estado)?;
        self.fase = Fase::Concluida;
        Ok(())
    }
}

impl Drop for Vaga<'_, '_> {
    fn drop(&mut self) {
        if self.iniciada() {
            let _ = self.porta.revogar();
        } else if let Ok(mut estado) = self.porta.estado.lock()
            && let Estado::Ativa { ocupada, .. } = &mut *estado
        {
            *ocupada = false;
        }
    }
}
