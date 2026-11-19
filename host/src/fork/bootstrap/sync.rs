//! Bounded relay wakeups. One read in flight and one dirty bit per connection.
use crate::hub::Hub;
use crate::remote::cloud;
use crate::store::now_ms;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::{Instant, sleep_until};

const MIN_WAIT: Duration = Duration::from_secs(1);
const MAX_WAIT: Duration = Duration::from_secs(16);

pub(crate) struct Pulls {
    tasks: JoinSet<bool>,
    pending: bool,
    after: Instant,
    backoff: Duration,
}

impl Default for Pulls {
    fn default() -> Self {
        Self { tasks: JoinSet::new(), pending: false, after: Instant::now(), backoff: MIN_WAIT }
    }
}

impl Pulls {
    pub(crate) fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.tasks.len()
    }

    pub(crate) fn request(&mut self, hub: &Arc<Hub>, base: &str) {
        if !self.tasks.is_empty() {
            self.pending = true;
            return;
        }
        let (hub, base, after) = (hub.clone(), base.to_owned(), self.after);
        self.tasks.spawn(async move {
            sleep_until(after).await;
            pull_state(hub, base).await
        });
    }

    pub(crate) fn retry_initial(&mut self, hub: &Arc<Hub>, base: &str) {
        self.after = Instant::now() + MIN_WAIT;
        self.backoff = MIN_WAIT * 2;
        self.request(hub, base);
    }

    pub(crate) async fn join_next(&mut self, hub: &Arc<Hub>, base: &str) -> Option<()> {
        let success = self.tasks.join_next().await?.unwrap_or(false);
        let wait = if success { MIN_WAIT } else { self.backoff };
        self.backoff = if success { MIN_WAIT } else { (self.backoff * 2).min(MAX_WAIT) };
        self.after = Instant::now() + wait;
        // Retry only within the persisted issuer deadline. An expired receipt is
        // retained for GET reconciliation; neither a timer nor an event renews it.
        let retry = hub.store.bootstrap_pending_deadline().ok().flatten().is_some_and(|deadline| {
            deadline.saturating_sub(now_ms()) > i64::try_from(wait.as_millis()).unwrap_or(i64::MAX)
        });
        if std::mem::take(&mut self.pending) || retry {
            self.request(hub, base);
        }
        Some(())
    }
}

/// State synchronization and receipt confirmation must both succeed for admission.
pub(crate) async fn pull_state(hub: Arc<Hub>, base: String) -> bool {
    match cloud::pull(&hub, &base).await {
        Ok(()) => {
            hub.cloud_synced.store(true, Ordering::Release);
            true
        }
        Err(e) => {
            if hub.store.bootstrap_needs_publication().unwrap_or(true) {
                hub.cloud_synced.store(false, Ordering::Release);
            }
            tracing::warn!(error = format!("{e:#}"), "couldn't get the cloud state");
            cloud::update_status(&hub, |s| s.last_error = Some(format!("{e:#}")));
            false
        }
    }
}
