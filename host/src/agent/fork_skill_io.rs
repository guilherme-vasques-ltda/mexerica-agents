//! Entrega da API candidata; cancelamento de I/O não concede autoridade.
use super::{Acp, Failure, FailureKind, Writer};
use anyhow::Result;
use serde_json::{Value, json};

/// Uma resposta incompleta nunca deixa o próximo quadro continuar seu prefixo.
struct ScopedFrame<'a> {
    input: tokio::sync::MutexGuard<'a, Option<Writer>>,
    started: bool,
    committed: bool,
}
impl Drop for ScopedFrame<'_> {
    fn drop(&mut self) {
        if self.started && !self.committed {
            self.input.take();
        }
    }
}

impl Acp {
    /// Entrega candidata sob o escopo síncrono do produtor, após aguardar stdin.
    pub(crate) async fn respond_scoped(
        &self,
        id: Value,
        result: Value,
        scope: impl Fn(&mut dyn FnMut() -> std::task::Poll<Result<()>>) -> std::task::Poll<Result<()>>,
    ) -> Result<()> {
        let mut line = serde_json::to_vec(&json!({"jsonrpc":"2.0", "id":id, "result":result}))?;
        line.push(b'\n');
        let mut cancel = self.scoped_cancel.subscribe();
        #[cfg(test)]
        self.scoped_entered.notify_one();
        // A guarda existe antes do primeiro poll_write. Drop por cancelamento também
        // fecha um prefixo real; o bloco de erro após await não bastaria.
        let delivery = async {
            let input = self.stdin.lock().await;
            let mut frame = ScopedFrame { input, started: false, committed: false };
            let ScopedFrame { input, started, .. } = &mut frame;
            let writer = input.as_mut().ok_or_else(|| Failure::error(FailureKind::Exited, "agent stopped"))?;
            let result = crate::fork::spaces::tool_effects::write_scoped_progress(
                writer,
                &line,
                |effect| {
                    let polled = scope(effect);
                    #[cfg(test)]
                    if polled.is_pending() {
                        self.scoped_pending.notify_one();
                    }
                    polled
                },
                started,
            )
            .await;
            frame.committed = result.is_ok();
            result
        };
        tokio::select! {
            biased;
            _ = cancel.changed() => anyhow::bail!("skills_read_unavailable"),
            result = delivery => result,
        }
    }

    /// Sinal de lifecycle do transporte, nunca fonte de autoridade. O Ticket
    /// canônico continua sendo revalidado pelo produtor em cada escrita.
    pub(crate) fn cancel_scoped(&self) {
        self.scoped_cancel.send_replace(());
    }

    #[cfg(test)]
    pub(crate) async fn scoped_entered(&self) {
        self.scoped_entered.notified().await;
    }
    #[cfg(test)]
    pub(crate) async fn scoped_pending(&self) {
        self.scoped_pending.notified().await;
    }

    #[cfg(test)]
    pub(crate) async fn stopped(&self) {
        self.stopped.notified().await;
    }
}
