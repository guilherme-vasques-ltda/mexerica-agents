//! L2: conserva origem no transporte comum; não identifica nem libera superfície privada.
use crate::{
    LockExt,
    api::devices::Caller,
    fork::{
        autoridade::{self, Autoridade, OrigemSaida},
        sessoes::bindings::{self, ExecutionRef},
    },
    hub::{BotStatus, Hub, Runtime},
    store::Store,
};
use anyhow::{Result, anyhow, ensure};
use std::{
    future::poll_fn,
    pin::Pin,
    sync::{Arc, Weak},
    task::{Poll, ready},
};
use tokio::io::AsyncWrite;

const DENIED: &str = "Observação indisponível: a origem da tentativa mudou";

pub(crate) struct Attempt {
    pub(super) hub: Weak<Hub>,
    origin: Origin,
    pub(super) bot: String,
    pub(super) runtime: Runtime,
    pub(super) execution: ExecutionRef,
    pub(super) generation: i64,
    read_only: bool,
    refused: std::sync::atomic::AtomicBool,
    helper: Option<Arc<super::helper::Link>>,
    pub(super) grant: Option<(String, i64)>,
}
enum Origin {
    Controle(Autoridade),
    Ponte(crate::fork::ponte_credencial::Origem),
}
impl Attempt {
    pub(crate) fn capture_original(
        hub: &Arc<Hub>,
        caller: &Caller,
        bot: &str,
        execution: &ExecutionRef,
        read_only: bool,
    ) -> Result<Self> {
        Ok(Self {
            hub: Arc::downgrade(hub),
            origin: match caller {
                Caller::Ponte { bot: original_bot, sessao, origem } => {
                    ensure!(
                        original_bot == bot
                            && execution.acp_binding() == Some(sessao.as_str())
                            && origem.corresponde(bot, sessao),
                        DENIED
                    );
                    Origin::Ponte(origem.clone())
                }
                _ => Origin::Controle(Autoridade::capturar(hub, caller)?),
            },
            bot: bot.into(),
            runtime: hub.runtime(bot),
            execution: execution.clone(),
            generation: crate::fork::spaces::config_generation_on(&hub.store.db.locked(), bot)?,
            read_only,
            refused: std::sync::atomic::AtomicBool::new(false),
            helper: hub.screen.link.locked().clone(),
            grant: None,
        })
    }
    #[cfg(test)]
    pub(crate) fn capture(
        hub: &Arc<Hub>,
        caller: &Caller,
        bot: &str,
        execution: &ExecutionRef,
        read_only: bool,
    ) -> Result<Arc<Self>> {
        let mut original = Self::capture_original(hub, caller, bot, execution, read_only)?;
        original.take_grant(hub);
        original.verify()?;
        Ok(Arc::new(original))
    }
    pub(super) fn take_grant(&mut self, hub: &Hub) {
        self.grant = hub
            .screen
            .control
            .locked()
            .permissions
            .grant(&self.bot, &self.execution, self.generation, &self.runtime)
            .map(|g| (g.card.clone(), g.revision));
    }
    pub(crate) async fn authorize(self, reason: Option<&str>) -> Result<Arc<Self>> {
        super::fork_permission::authorize(self, reason).await
    }
    pub(super) fn check_waiting(&self) -> Result<()> {
        self.checked(None, false, |_| ()).map_err(|_| anyhow!(DENIED))
    }
    /// O claim nasce sob a MESMA Tx/predicado da observação, antes de qualquer efeito.
    pub(super) fn claim(&self) -> Result<bool> {
        self.checked(None, true, |control| {
            let fresh = control.active_agent().is_none();
            control.permissions.claimed(&self.bot);
            control.agent = Some((self.bot.clone(), std::time::Instant::now()));
            fresh
        })
    }

    /// Validação Connection-only seguida do poll/envio síncrono. Não chama `with_db`,
    /// Store nem Cloud no callback; libera todas as guardas antes de Pending/await.
    pub(super) fn with_send<T>(&self, link: Option<u64>, send: impl FnOnce() -> T) -> Result<T> {
        self.checked(link, true, |_| send()).map_err(|_| anyhow!(DENIED))
    }
    fn checked<T>(&self, link: Option<u64>, admitted: bool, send: impl FnOnce(&mut super::Control) -> T) -> Result<T> {
        ensure!(!self.refused.load(std::sync::atomic::Ordering::Acquire), DENIED);
        let hub = self.hub.upgrade().ok_or_else(|| anyhow!(DENIED))?;
        let default_enabled = hub.screen.enabled();
        let _boundary = hub.store.memory_boundary.locked();
        let _emit = hub.emit_lock.locked();
        hub.conector_fence(&self.bot, |current| {
            ensure!(
                matches!(current.status, BotStatus::Working | BotStatus::NeedsInput)
                    && current.memory_turn_token.as_deref() == Some(self.execution.turn_token())
                    && current.memory_turn_token == self.runtime.memory_turn_token
                    && current.lane == self.runtime.lane
                    && current.session == self.runtime.session
                    && current.message == self.runtime.message
                    && current.started_at == self.runtime.started_at,
                DENIED
            );
            let origin = match &self.origin {
                Origin::Controle(original) => OrigemSaida::Controle(original, None),
                Origin::Ponte(original) => OrigemSaida::Ponte(original),
            };
            let held = std::cell::RefCell::new(None);
            autoridade::com_saida_validada(
                &hub,
                origin,
                |db, owner, owner_generation, epoch, synced| {
                    ensure!(!self.refused.load(std::sync::atomic::Ordering::Acquire), DENIED);
                    // D502 também em check_waiting (admitted=false), antes de DB → control.
                    if crate::fork::atividades::revisao::estado::work_allowed_on(db).is_err() {
                        self.refused.store(true, std::sync::atomic::Ordering::Release);
                        return Err(anyhow!(DENIED));
                    }
                    let control = hub.screen.control.locked();
                    if admitted {
                        let grant = control
                            .permissions
                            .grant(&self.bot, &self.execution, self.generation, &self.runtime)
                            .filter(|g| {
                                self.grant.as_ref().is_some_and(|(id, rev)| *id == g.card && *rev == g.revision)
                            });
                        if let Some(original) = grant.and_then(|g| g.owner()) {
                            original.conferir(db, owner, owner_generation, epoch, synced)?;
                        }
                        crate::fork::tela_por_computador::allowed_on(
                            db,
                            &self.bot,
                            &crate::fork::tela_por_computador::host_target(&hub),
                            grant,
                        )?;
                    }
                    *held.borrow_mut() = Some(control);
                    bindings::assert_execution_live(db, &self.execution)?;
                    ensure!(crate::fork::spaces::config_generation_on(db, &self.bot)? == self.generation, DENIED);
                    let row = Store::bot_on(db, &self.bot)?.ok_or_else(|| anyhow!(DENIED))?;
                    ensure!(!row.deleted && row.config.computer, DENIED);
                    Ok(())
                },
                || {
                    let mut held = held.borrow_mut();
                    let control = held.as_mut().ok_or_else(|| anyhow!(DENIED))?;
                    let enabled = hub.screen.enabled.locked();
                    if admitted {
                        ensure!(cfg!(windows) || enabled.unwrap_or(default_enabled), DENIED);
                    }
                    ensure!(
                        !admitted
                            || ((!control.user || self.read_only)
                                && control.active_agent().is_none_or(|bot| bot == self.bot)),
                        DENIED
                    );
                    let current_link = hub.screen.link.locked();
                    if self.helper.is_some() || link.is_some() {
                        ensure!(
                            self.helper.as_ref().is_some_and(|l| link.is_none_or(|id| l.id == id))
                                && current_link
                                    .as_ref()
                                    .zip(self.helper.as_ref())
                                    .is_some_and(|(live, original)| Arc::ptr_eq(live, original)),
                            DENIED
                        );
                    }
                    Ok(send(control))
                },
            )
            .and_then(|result| result)
        })
    }
    pub(super) fn verify(&self) -> Result<()> {
        self.with_send(None, || ())
    }
}

#[derive(Debug)]
pub(super) struct WriteFailure {
    pub(super) sent: bool,
}
impl std::fmt::Display for WriteFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.sent { "Observação incerta: envio parcial ou retorno indisponível" } else { DENIED })
    }
}
impl std::error::Error for WriteFailure {}

/// Socket/pipe nomeado não herdam atomicidade POSIX de `ChildStdin`. Qualquer byte
/// enviado antes de recusa torna o resultado incerto; o chamador encerra o writer,
/// nunca continua o frame ou o concatena com um sucessor. Sem retry automático.
pub(super) async fn write<W: AsyncWrite + Unpin>(
    writer: &mut W,
    bytes: &[u8],
    attempt: &Attempt,
    job: Option<(&super::helper::Link, i64)>,
) -> std::result::Result<(), WriteFailure> {
    let mut offset = 0;
    poll_fn(|cx| {
        while offset < bytes.len() {
            let polled = attempt
                .with_send(job.map(|(link, _)| link.id), || {
                    let pending = job.map(|(link, id)| (link.pending.locked(), id));
                    if pending.as_ref().is_some_and(|(waiters, id)| !waiters.contains_key(id)) {
                        return Err(anyhow!(DENIED));
                    }
                    Ok(Pin::new(&mut *writer).poll_write(cx, &bytes[offset..]))
                })
                .and_then(|polled| polled);
            let Ok(polled) = polled else {
                return Poll::Ready(Err(WriteFailure { sent: offset > 0 }));
            };
            match ready!(polled) {
                Ok(0) => return Poll::Ready(Err(WriteFailure { sent: offset > 0 })),
                Err(_) => return Poll::Ready(Err(WriteFailure { sent: true })),
                Ok(n) => offset += n,
            }
        }
        Poll::Ready(Ok(()))
    })
    .await
}

#[cfg(unix)]
pub(super) async fn helper_writer<W: AsyncWrite + Unpin>(
    mut writer: W,
    mut queue: tokio::sync::mpsc::UnboundedReceiver<super::helper::Outgoing>,
    link: Arc<super::helper::Link>,
) {
    use tokio::io::AsyncWriteExt;
    while let Some(mut outgoing) = queue.recv().await {
        outgoing.line.push('\n');
        if let Some(attempt) = outgoing.attempt {
            if let Err(error) = write(&mut writer, outgoing.line.as_bytes(), &attempt, Some((&link, outgoing.id))).await
            {
                if let Some(waiter) = link.pending.locked().remove(&outgoing.id) {
                    let _ = waiter.send(Err(error.to_string()));
                }
                if error.sent {
                    break;
                }
            }
        } else if writer.write_all(outgoing.line.as_bytes()).await.is_err() {
            break;
        }
    }
}

/// Orientação comum das ferramentas; não anuncia isolamento privado.
pub(super) const INSTRUCTIONS: &str = "Ver a tela do host exige liberação do dono para o par bot/computador, além de Uso do computador e Tela remota. Sem liberação, o Hub cria um cartão com Permitir agora, Permitir sempre e Não permitir; aguarda até 270 s sem reservar nem capturar a tela. O cartão dura 10 min. Não peça de novo no mesmo turno após recusa nem durante a hora de espera. Agora vale só para esta execução/sessão/lane, até 20 s sem tela ou 15 min totais, e termina ao encerrar o turno, outro bot assumir, reiniciar, revogar ou desligar. Permitir sempre é recusado enquanto qualquer bot usa a tela. Permitir agora com tela em uso exige aparelho Account com Control: abra o app conectado à sua conta. Não permitir é aceito de qualquer origem válida, inclusive Local e QR. Nenhuma opção aceita voz. O cartão tem só texto fixo do Hub, sem motivo do bot. Computadores D83 têm tela ainda indisponível. Esta opção bloqueia as ferramentas de tela; não garante isolamento do terminal. Desligar impede novos envios/capturas; ação já admitida ou input em voo não é desfeito. A origem original é reconferida no envio e no retorno; se mudar, não repita automaticamente. A pausa global D502 durante revisão protegida depende da futura integração 05b/Q1–Q5. Essa integração ainda não está disponível nesta versão. Caixa protegida e revisão humana privada continuam indisponíveis.";

#[cfg(test)]
mod tests;
