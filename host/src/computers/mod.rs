//! D83: Hub side only. Local approval and execution belong to the future helper.
pub mod context;
pub mod executor_api;
pub mod helper;
pub mod jobs;
pub mod pairing;
pub mod registry;
#[cfg(test)]
mod test_executor;
pub mod tools;
pub mod xlsx;

use crate::LockExt;
use crate::hub::Hub;
use crate::store::{BotConfig, Store, now_ms};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

pub const APPROVAL_BUDGET: i64 = 120_000;
pub const LONG_APPROVAL: i64 = 600_000;
pub const EXEC_MAX: i64 = 120_000;
pub const GRACE: i64 = 10_000;
pub const LEASE: i64 = 15_000;
pub const POLL_WAIT: u64 = 25_000;
pub const CANCEL_RETRY: u64 = 5_000;
pub const ONLINE: i64 = 40_000;
pub const MAX_RESULT: usize = 40_960;
pub const MAX_STDIO: usize = 24_576;
pub const TOOL_WAIT: u64 = 240_000;
#[cfg(test)]
pub(crate) use crate::fork::acesso_rpc::EXECUTOR as EXECUTOR_METHODS;

/// Serializes transitions, including cancellation guards, without holding a lock across await.
pub struct State {
    pub(crate) gate: Mutex<()>,
    pub(crate) pairing: Mutex<pairing::Codes>,
    pub(crate) sessions: Mutex<HashMap<String, Session>>,
    external: Mutex<HashMap<String, i64>>,
    changed: watch::Sender<u64>,
}
#[derive(Clone)]
pub(crate) struct Session {
    pub boot: String,
    pub poll: u64,
    pub cancel_sent_at: HashMap<String, tokio::time::Instant>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            gate: Mutex::default(),
            pairing: Mutex::default(),
            sessions: Mutex::default(),
            external: Mutex::default(),
            changed: watch::Sender::new(0),
        }
    }
}
impl State {
    pub fn mark_external(&self, bot: &str) {
        self.external.locked().insert(bot.to_owned(), now_ms());
    }
    pub fn external(&self, bot: &str) -> bool {
        self.external.locked().get(bot).is_some_and(|at| now_ms() - at < 2 * 60 * 60 * 1000)
    }
    pub(crate) fn wake(&self) {
        self.changed.send_modify(|n| *n = n.wrapping_add(1));
    }
    pub(crate) fn watch(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }
}

pub fn offered(store: &Store, cfg: &BotConfig) -> bool {
    !cfg.is_group() && registry::bots(store).is_ok_and(|bots| bots.contains(&cfg.id))
}

pub async fn run(hub: Arc<Hub>) {
    if let Err(error) = jobs::recover(&hub) {
        tracing::error!(%error, "computer recovery failed");
        return;
    }
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        tick.tick().await;
        if let Err(error) = jobs::sweep(&hub, now_ms()) {
            tracing::warn!(%error, "computer sweep failed");
        }
    }
}
