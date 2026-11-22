//! Lifecycle canônico do helper: origem capturada antes de spawn/await.
use crate::native_stop::{NativeStopError, stop_pipeline};
use crate::stream::Session;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{Mutex as AsyncMutex, watch};

type StopResult = Result<(), NativeStopError>;

#[derive(Default)]
struct Registry {
    rows: HashMap<String, Arc<Operation>>,
    disconnected: bool,
}

#[derive(Default)]
pub(crate) struct Sessions {
    state: Mutex<Registry>,
}

impl Sessions {
    #[cfg(test)]
    pub(crate) fn admit(&self, id: &str) -> Result<Arc<Operation>, NativeStopError> {
        self.admit_answer(id, None, None)
    }

    pub(crate) fn admit_answer(
        &self,
        id: &str,
        request_id: Option<i64>,
        expected: Option<i64>,
    ) -> Result<Arc<Operation>, NativeStopError> {
        let mut registry = self.state.lock().map_err(|_| NativeStopError::Lifecycle)?;
        if registry.disconnected {
            return Err(NativeStopError::Closed);
        }
        if let Some(expected) = expected {
            return registry
                .rows
                .get(id)
                .filter(|operation| operation.origin_answer == Some(expected))
                .cloned()
                .ok_or(NativeStopError::Lifecycle);
        }
        Ok(registry
            .rows
            .entry(id.to_owned())
            .or_insert_with(|| Arc::new(Operation::identified(request_id)))
            .clone())
    }

    /// Legacy control only: no entry is a no-op, never a native stop fact.
    /// An admitted operation still requires its original pipeline to stop.
    pub(crate) fn close(&self, id: &str) -> Result<(), NativeStopError> {
        let operation = {
            let registry = self.state.lock().map_err(|_| NativeStopError::Lifecycle)?;
            let Some(operation) = registry.rows.get(id).cloned() else {
                return Ok(());
            };
            operation.mark_closed()?;
            operation
        };
        self.finish(id, &operation, || {})
    }

    /// Capture eligibility before mark_closed clears registered. NULL never
    /// holds Registry/State; the final identity check and removal are atomic.
    pub(crate) fn close_correlated(
        &self,
        id: &str,
        answer: i64,
    ) -> Result<Option<i64>, NativeStopError> {
        let (operation, pipeline, eligible) = {
            let registry = self.state.lock().map_err(|_| NativeStopError::Lifecycle)?;
            let Some(operation) = registry.rows.get(id).cloned() else {
                return Ok(None);
            };
            if registry.disconnected || operation.origin_answer != Some(answer) {
                return Err(NativeStopError::Lifecycle);
            }
            let mut state = operation
                .state
                .lock()
                .map_err(|_| NativeStopError::Lifecycle)?;
            let eligible = state.registered && !state.closed;
            let pipeline = state.pipeline.clone();
            state.closed = true;
            state.registered = false;
            operation.cancelled.send_replace(true);
            drop(state);
            (operation, pipeline, eligible)
        };
        stop_pipeline(&pipeline.ok_or(NativeStopError::Absent)?)?;
        let mut registry = self.state.lock().map_err(|_| NativeStopError::Lifecycle)?;
        if registry.disconnected
            || !registry
                .rows
                .get(id)
                .is_some_and(|current| Arc::ptr_eq(current, &operation))
        {
            return Err(NativeStopError::Lifecycle);
        }
        registry.rows.remove(id);
        Ok(eligible.then_some(answer))
    }

    pub(crate) fn register(&self, id: &str, operation: &Arc<Operation>) -> StopResult {
        let registry = self.state.lock().map_err(|_| NativeStopError::Lifecycle)?;
        if registry.disconnected
            || !registry
                .rows
                .get(id)
                .is_some_and(|current| Arc::ptr_eq(current, operation))
        {
            return Err(NativeStopError::Closed);
        }
        operation.register()
    }

    pub(crate) fn finish(
        &self,
        id: &str,
        original: &Arc<Operation>,
        on_current: impl FnOnce(),
    ) -> StopResult {
        // Failure leaves the original closed/unknown in the canonical map.
        // Native calls do not hold the registry lock.
        original.stop()?;
        let mut registry = self.state.lock().map_err(|_| NativeStopError::Lifecycle)?;
        if registry
            .rows
            .get(id)
            .is_some_and(|current| Arc::ptr_eq(current, original))
        {
            registry.rows.remove(id);
            // Enqueue legacy notification before any same-id successor admission.
            on_current();
        }
        Ok(())
    }

    /// Legacy aggregate control, including an empty map; not a native receipt.
    /// Existing operations are all closed before any original native stop.
    pub(crate) fn close_all(&self, disconnect: bool) -> Result<(), NativeStopError> {
        let operations = {
            let mut registry = self.state.lock().map_err(|_| NativeStopError::Lifecycle)?;
            registry.disconnected |= disconnect;
            // Linearize all closures against registration before native stops.
            for operation in registry.rows.values() {
                operation.mark_closed()?;
            }
            registry
                .rows
                .iter()
                .map(|(id, operation)| (id.clone(), operation.clone()))
                .collect::<Vec<_>>()
        };
        let mut result = Ok(());
        for (id, operation) in operations {
            if let Err(error) = self.finish(&id, &operation, || {}) {
                result = Err(error);
            }
        }
        if disconnect {
            // Link teardown releases records, but this is never confirmation.
            self.state
                .lock()
                .map_err(|_| NativeStopError::Lifecycle)?
                .rows
                .clear();
        }
        result
    }
}

#[derive(Default)]
struct State {
    closed: bool,
    registered: bool,
    pipeline: Option<gst::Pipeline>,
    session: Option<Arc<Session>>,
}

pub(crate) struct Operation {
    origin_answer: Option<i64>,
    state: Mutex<State>,
    cancelled: watch::Sender<bool>,
    negotiation: AsyncMutex<()>,
}

impl Operation {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::identified(None)
    }

    fn identified(origin_answer: Option<i64>) -> Self {
        Self {
            origin_answer,
            state: Mutex::default(),
            cancelled: watch::channel(false).0,
            negotiation: AsyncMutex::new(()),
        }
    }

    pub(crate) fn attach_pipeline(&self, pipeline: &gst::Pipeline) -> StopResult {
        let mut state = self.state.lock().map_err(|_| NativeStopError::Lifecycle)?;
        if state.closed {
            drop(state);
            stop_pipeline(pipeline)?;
            return Err(NativeStopError::Closed);
        }
        if state.pipeline.is_some() {
            return Err(NativeStopError::Lifecycle);
        }
        state.pipeline = Some(pipeline.clone());
        Ok(())
    }

    pub(crate) fn session(&self) -> Result<Option<Arc<Session>>, NativeStopError> {
        let state = self.state.lock().map_err(|_| NativeStopError::Lifecycle)?;
        if state.closed {
            return Err(NativeStopError::Closed);
        }
        Ok(state.session.clone())
    }

    pub(crate) fn install(&self, session: Arc<Session>) -> StopResult {
        let mut state = self.state.lock().map_err(|_| NativeStopError::Lifecycle)?;
        if state.closed {
            return Err(NativeStopError::Closed);
        }
        state.session = Some(session);
        Ok(())
    }

    fn register(&self) -> StopResult {
        let mut state = self.state.lock().map_err(|_| NativeStopError::Lifecycle)?;
        if state.closed {
            return Err(NativeStopError::Closed);
        }
        state.registered = true;
        Ok(())
    }

    // Short native state changes only; no await, portal or input under this lock.
    pub(crate) fn with_pipeline<T>(
        &self,
        pipeline: &gst::Pipeline,
        action: impl FnOnce() -> Result<T, NativeStopError>,
    ) -> Result<T, NativeStopError> {
        let state = self.state.lock().map_err(|_| NativeStopError::Lifecycle)?;
        if state.closed {
            return Err(NativeStopError::Closed);
        }
        let original = state.pipeline.as_ref().ok_or(NativeStopError::Absent)?;
        if original != pipeline {
            return Err(NativeStopError::Lifecycle);
        }
        action()
    }

    pub(crate) async fn negotiate<T>(
        self: &Arc<Self>,
        sessions: &Sessions,
        id: &str,
        answer: impl Future<Output = anyhow::Result<T>>,
    ) -> anyhow::Result<T> {
        tokio::select! {
            biased;
            () = self.cancelled() => Err(NativeStopError::Closed.into()),
            result = async {
                let _negotiation = self.negotiation.lock().await;
                let _ = self.session()?; // Reject closed operations before polling work.
                let result = answer.await?;
                sessions.register(id, self)?;
                Ok(result)
            } => result,
        }
    }

    pub(crate) async fn cancelled(&self) {
        let mut receiver = self.cancelled.subscribe();
        let _ = receiver.wait_for(|closed| *closed).await;
    }

    fn mark_closed(&self) -> StopResult {
        let mut state = self.state.lock().map_err(|_| NativeStopError::Lifecycle)?;
        state.closed = true;
        state.registered = false;
        self.cancelled.send_replace(true);
        Ok(())
    }

    pub(crate) fn stop(&self) -> StopResult {
        self.mark_closed()?;
        let pipeline = self
            .state
            .lock()
            .map_err(|_| NativeStopError::Lifecycle)?
            .pipeline
            .clone()
            .ok_or(NativeStopError::Absent)?;
        // Closed is monotonic: no future Ready/Playing is possible. Native NULL
        // runs without lifecycle locks, including synchronous Gst callbacks.
        // Repeated calls recheck this same object; absence is never confirmation.
        stop_pipeline(&pipeline)
    }

    #[cfg(test)]
    #[allow(clippy::unwrap_used)]
    fn is_registered(&self) -> bool {
        self.state.lock().unwrap().registered
    }
}

/// Cleanup is tied to the original request, including dropped futures.
pub(crate) struct Attempt<'a> {
    pub(crate) sessions: &'a Sessions,
    pub(crate) id: &'a str,
    pub(crate) operation: &'a Arc<Operation>,
    pub(crate) completed: bool,
}

impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        if !self.completed {
            let _ = self.sessions.finish(self.id, self.operation, || {});
        }
    }
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
