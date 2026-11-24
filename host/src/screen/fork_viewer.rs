//! Identidade original e resultado restrito de close individual Linux.
use super::{Screen, ViewerRecords, ViewerRef, ViewerState, helper::Link};
use crate::LockExt;
use anyhow::{Result, anyhow, ensure};
use serde_json::{Value, json};
use std::sync::Arc;

const CHANGED: &str = "A origem da sessão de tela mudou";

pub(super) struct Original {
    id: String,
    row: ViewerRef,
    pub(super) link: Arc<Link>,
}
/// Fato efêmero, construído somente após validar o retorno no Original.
/// Não é estado global, autorização ou resultado recuperável por retry.
pub(super) enum CloseOutcome<'a> {
    ControlOnly,
    PipelineNull(PipelineNull<'a>),
}
pub(super) struct PipelineNull<'a> {
    original: &'a Original,
    answer: i64,
}

#[derive(Clone, Copy)]
enum CloseMode {
    ControlOnly,
    Individual,
}

impl CloseOutcome<'_> {
    pub(super) fn discard(self) {
        if let Self::PipelineNull(fact) = self {
            let _ = (fact.original, fact.answer);
        }
    }
}

impl Original {
    pub(super) fn capture(screen: &Screen, id: String) -> Option<Self> {
        let row = screen.sessions.locked().rows.get(&id)?.clone();
        let link = {
            let viewer = row.locked();
            if viewer.state == ViewerState::Uncertain {
                return None;
            }
            viewer.link.clone()
        };
        Some(Self { id, row, link })
    }
    pub(super) fn owned_by(&self, owner: &str) -> bool {
        self.row.locked().owner == owner
    }
    pub(super) fn current(&self, screen: &Screen) -> bool {
        let sessions = screen.sessions.locked();
        self.matches(screen, &sessions) && self.row.locked().state != ViewerState::Uncertain
    }
    // sessions impede troca/retirada do slot; não conservar slot junto de row/control.
    fn matches(&self, screen: &Screen, sessions: &ViewerRecords) -> bool {
        if !sessions.rows.get(&self.id).is_some_and(|row| Arc::ptr_eq(row, &self.row)) {
            return false;
        }
        let slot = screen.link.locked();
        slot.as_ref().is_some_and(|link| Arc::ptr_eq(link, &self.link))
    }
    pub(super) fn closing(&self) -> bool {
        matches!(self.row.locked().state, ViewerState::ClosingPrepared | ViewerState::ClosingStarted)
    }
    fn mark_closing(&self) {
        let mut row = self.row.locked();
        row.state = match row.state {
            ViewerState::Prepared => ViewerState::ClosingPrepared,
            ViewerState::Started => ViewerState::ClosingStarted,
            state => state,
        };
    }
    pub(super) fn enqueue_answer<'a>(
        &'a self,
        screen: &Screen,
        params: &Value,
    ) -> Result<impl Future<Output = Result<Value>> + Send + 'a + use<'a>> {
        let sessions = screen.sessions.locked();
        ensure!(self.matches(screen, &sessions), CHANGED);
        ensure!(screen.enabled(), "A tela remota está desligada");
        let mut row = self.row.locked();
        ensure!(matches!(row.state, ViewerState::Prepared | ViewerState::Started), CHANGED);
        let now = crate::store::now_ms();
        ensure!(
            row.ice.as_ref().is_some_and(|ice| ice.expires_at > now)
                && (row.state != ViewerState::Prepared || row.pending_until > now),
            "A conexão de tela expirou"
        );
        let mut params = params.clone();
        if cfg!(target_os = "linux")
            && let Some(answer) = row.origin_answer
        {
            params["answerId"] = json!(answer);
        }
        let (id, wait) = self.link.enqueue_identified("answer", &params)?;
        // Bind after tx.send, while the canonical row/sessions section is held.
        row.origin_answer.get_or_insert(id);
        Ok(wait)
    }
    fn enqueue_close_mode<'a>(
        &'a self,
        screen: &Screen,
        mode: CloseMode,
    ) -> Result<(Option<i64>, impl Future<Output = Result<Value>> + Send + 'a + use<'a>)> {
        let sessions = screen.sessions.locked();
        ensure!(self.matches(screen, &sessions), CHANGED);
        let mut row = self.row.locked();
        ensure!(row.state != ViewerState::Uncertain, CHANGED);
        let answer = if cfg!(target_os = "linux")
            && matches!(mode, CloseMode::Individual)
            && row.state == ViewerState::Started
        {
            row.origin_answer
        } else {
            None
        };
        row.state = match row.state {
            ViewerState::Prepared => ViewerState::ClosingPrepared,
            ViewerState::Started => ViewerState::ClosingStarted,
            state => state,
        };
        let mut params = json!({"session":self.id});
        if let Some(answer) = answer {
            params["answerId"] = json!(answer);
        }
        Ok((answer, self.link.enqueue("close", &params)?))
    }
    #[cfg(test)]
    fn enqueue_close<'a>(
        &'a self,
        screen: &Screen,
    ) -> Result<impl Future<Output = Result<Value>> + Send + 'a + use<'a>> {
        Ok(self.enqueue_close_mode(screen, CloseMode::ControlOnly)?.1)
    }
    pub(super) fn expired(&self) -> bool {
        let row = self.row.locked();
        let now = crate::store::now_ms();
        row.ice.as_ref().is_none_or(|ice| ice.expires_at <= now)
            || (row.state == ViewerState::Prepared && row.pending_until <= now)
    }
    pub(super) fn ice(&self) -> Result<super::viewer::IceConfig> {
        self.row.locked().ice.clone().ok_or_else(|| anyhow!(CHANGED))
    }
    pub(super) fn require_current(&self, screen: &Screen) -> Result<()> {
        ensure!(self.current(screen), CHANGED);
        Ok(())
    }
    pub(super) fn start(&self, screen: &Screen) -> Result<()> {
        let sessions = screen.sessions.locked();
        ensure!(self.matches(screen, &sessions), CHANGED);
        let mut row = self.row.locked();
        ensure!(matches!(row.state, ViewerState::Prepared | ViewerState::Started), CHANGED);
        row.state = ViewerState::Started;
        Ok(())
    }
    pub(super) fn expire(&self, screen: &Screen) {
        let sessions = screen.sessions.locked();
        if sessions.rows.get(&self.id).is_some_and(|row| Arc::ptr_eq(row, &self.row)) {
            let mut row = self.row.locked();
            if row.state != ViewerState::Uncertain
                && let Some(ice) = row.ice.as_mut()
            {
                ice.expires_at = 0;
            }
        }
    }
    /// Vigia e cleanup continuam controle legado, sem opt-in nativo.
    pub(super) async fn close(&self, screen: &Screen) -> Result<()> {
        self.close_with(screen, CloseMode::ControlOnly).await?.discard();
        Ok(())
    }
    pub(super) async fn close_individual<'a>(&'a self, screen: &Screen) -> Result<CloseOutcome<'a>> {
        self.close_with(screen, CloseMode::Individual).await
    }
    async fn close_with<'a>(&'a self, screen: &Screen, mode: CloseMode) -> Result<CloseOutcome<'a>> {
        let (answer, wait) = self.enqueue_close_mode(screen, mode)?;
        let result = wait.await?;
        let native_answer = if result == json!({}) {
            None
        } else {
            let expected = answer.ok_or_else(|| anyhow!("Confirmação nativa não solicitada"))?;
            let native_stop = result
                .as_object()
                .filter(|result| result.len() == 1)
                .and_then(|result| result.get("nativeStop"))
                .and_then(Value::as_object)
                .filter(|native_stop| native_stop.len() == 2)
                .ok_or_else(|| anyhow!("Confirmação nativa divergente"))?;
            ensure!(
                native_stop.get("answerId").and_then(Value::as_i64) == Some(expected)
                    && native_stop.get("state").and_then(Value::as_str) == Some("null"),
                "Confirmação nativa divergente"
            );
            Some(expected)
        };
        let outcome = {
            let mut sessions = screen.sessions.locked();
            ensure!(self.matches(screen, &sessions), CHANGED);
            ensure!(self.remove_locked(screen, &mut sessions), CHANGED);
            match native_answer {
                Some(answer) => CloseOutcome::PipelineNull(PipelineNull { original: self, answer }),
                None => CloseOutcome::ControlOnly,
            }
        };
        screen.emit();
        Ok(outcome)
    }
    fn remove(&self, screen: &Screen) -> bool {
        let mut sessions = screen.sessions.locked();
        self.remove_locked(screen, &mut sessions)
    }
    fn remove_locked(&self, screen: &Screen, sessions: &mut ViewerRecords) -> bool {
        if !sessions.rows.get(&self.id).is_some_and(|row| Arc::ptr_eq(row, &self.row))
            || self.row.locked().state == ViewerState::Uncertain
        {
            return false;
        }
        sessions.rows.remove(&self.id);
        if !sessions.rows.values().any(|v| v.locked().state != ViewerState::Uncertain) {
            screen.control.locked().user = false;
        }
        true
    }
}

impl Screen {
    /// Publicação local e closeAll na mesma fronteira das ofertas; espera fora.
    pub(super) async fn set_viewers_enabled(&self, on: bool) {
        let source;
        let originals;
        let wait = {
            let sessions = self.sessions.locked();
            *self.enabled.locked() = Some(on);
            if on {
                return;
            }
            source = self.link.locked().clone();
            originals = sessions
                .rows
                .iter()
                .filter_map(|(id, row)| {
                    let viewer = row.locked();
                    source
                        .as_ref()
                        .filter(|link| Arc::ptr_eq(link, &viewer.link))
                        .filter(|_| viewer.state != ViewerState::Uncertain)
                        .map(|link| Original { id: id.clone(), row: row.clone(), link: link.clone() })
                })
                .collect::<Vec<_>>();
            for original in &originals {
                original.mark_closing();
            }
            #[cfg(target_os = "linux")]
            let wait = source.as_ref().map(|link| {
                let mut notice = link.restore_notice.locked();
                let request = link.enqueue_identified("closeAll", &json!({}));
                notice.suspend(request.as_ref().ok().map(|(id, _)| *id));
                request.map(|(_, wait)| wait)
            });
            #[cfg(not(target_os = "linux"))]
            let wait = source.as_ref().map(|link| link.enqueue("closeAll", &json!({})));
            // Sem slot/row/enabled/pending guardados antes de pedir control.
            *self.control.locked() = super::Control::default();
            wait
        };
        match wait {
            Some(Ok(wait)) => {
                if wait.await.is_ok() {
                    for original in originals {
                        original.remove(self);
                    }
                } else {
                    tracing::warn!("couldn't confirm control response for disabled screen sessions");
                }
            }
            Some(Err(_)) => tracing::warn!("couldn't enqueue control request for disabled screen sessions"),
            None => {}
        }
    }

    /// Retira só a origem anterior do conjunto ativo; não atesta fim do observador.
    pub(super) fn attach_viewer_link(&self, next: Arc<Link>) {
        let mut sessions = self.sessions.locked();
        let mut slot = self.link.locked();
        if let Some(previous) = slot.replace(next) {
            retire(&mut sessions, &previous);
        }
        drop(slot);
        #[cfg(test)]
        link_retired_cut();
        if !sessions.rows.values().any(|v| v.locked().state != ViewerState::Uncertain) {
            self.control.locked().user = false;
        }
    }

    pub(super) fn detach_viewer_link(&self, source: &Arc<Link>) {
        let current = {
            let mut sessions = self.sessions.locked();
            let mut slot = self.link.locked();
            retire(&mut sessions, source);
            let current = slot.as_ref().is_some_and(|link| Arc::ptr_eq(link, source));
            if current {
                *slot = None;
            }
            drop(slot);
            #[cfg(test)]
            link_retired_cut();
            if !sessions.rows.values().any(|v| v.locked().state != ViewerState::Uncertain) {
                self.control.locked().user = false;
            }
            current
        };
        if current {
            *self.status.locked() = super::HelperStatus::default();
        }
        self.emit();
    }

    /// A notificação conserva a conexão emissora. Não atesta término nativo.
    pub(super) fn session_closed(&self, session: &str, source: &Arc<Link>) {
        let Some(original) = Original::capture(self, session.into()) else { return };
        if Arc::ptr_eq(&original.link, source) && original.remove(self) {
            self.emit();
        }
    }
}

const UNCERTAIN_LIMIT: usize = 256;

fn retire(sessions: &mut ViewerRecords, source: &Arc<Link>) {
    for row in sessions.rows.values() {
        let mut viewer = row.locked();
        if Arc::ptr_eq(&viewer.link, source) {
            viewer.state = ViewerState::Uncertain;
            viewer.ice = None;
            viewer.pending_until = 0;
        }
    }
    let mut uncertain: Vec<_> = sessions
        .rows
        .iter()
        .filter_map(|(id, row)| {
            let row = row.locked();
            if row.state == ViewerState::Uncertain { Some((row.link.id, id.clone())) } else { None }
        })
        .collect();
    let excess = uncertain.len().saturating_sub(UNCERTAIN_LIMIT);
    sessions.uncertainty_summarized |= excess > 0;
    if excess > 0 {
        // Ordem do Link original já existente; ids existentes desempatam a mesma retirada.
        uncertain.sort_unstable();
        for (_, id) in uncertain.into_iter().take(excess) {
            sessions.rows.remove(&id);
        }
    }
}

pub(super) fn missing() -> anyhow::Error {
    anyhow!("Sessão de tela desconhecida")
}

#[cfg(all(test, unix))]
mod tests;

#[cfg(test)]
thread_local! {
    static LINK_RETIRED_CUT: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn on_link_retired_for_test(cut: impl FnOnce() + 'static) {
    LINK_RETIRED_CUT.with_borrow_mut(|slot| {
        assert!(slot.is_none());
        *slot = Some(Box::new(cut));
    });
}

#[cfg(test)]
fn link_retired_cut() {
    let cut = LINK_RETIRED_CUT.with_borrow_mut(Option::take);
    if let Some(cut) = cut {
        cut();
    }
}

#[cfg(test)]
thread_local! {
    static OFFER_ADMISSION_CUT: std::cell::RefCell<Option<tokio::sync::oneshot::Receiver<()>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn on_offer_admission_for_test(cut: tokio::sync::oneshot::Receiver<()>) {
    OFFER_ADMISSION_CUT.with_borrow_mut(|slot| {
        assert!(slot.is_none());
        *slot = Some(cut);
    });
}

#[cfg(test)]
pub(super) async fn offer_admission_cut() {
    let cut = OFFER_ADMISSION_CUT.with_borrow_mut(Option::take);
    if let Some(cut) = cut {
        cut.await.expect("continuação do corte de offer");
    }
}
