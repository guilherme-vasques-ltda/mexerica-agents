use super::open_chat::{RUNNING, Running};
use crate::{
    LockExt,
    hub::{BotStatus, Hub},
    store::{BotConfig, Lane, Store},
};
use anyhow::{Context, Result, ensure};
use rusqlite::Connection;
use std::sync::{Arc, Weak};
use tokio::sync::watch;

#[derive(Clone)]
pub(crate) struct LocalTurn {
    hub: Weak<Hub>,
    lane: Lane,
    entry_id: String,
    turn: i64,
    space: String,
    cancel: watch::Receiver<bool>,
}

impl LocalTurn {
    pub(super) fn capture(
        hub: &Arc<Hub>,
        lane: &Lane,
        entry_id: &str,
        turn: i64,
        cancel: watch::Receiver<bool>,
    ) -> Result<Self> {
        let space = hub.store.bot(&lane.chat)?.context("open_tools_local_turn_required")?.config.space_id;
        let proof =
            Self { hub: Arc::downgrade(hub), lane: lane.clone(), entry_id: entry_id.into(), turn, space, cancel };
        proof.check(hub)?;
        Ok(proof)
    }

    pub(crate) fn lane(&self) -> &Lane {
        &self.lane
    }

    pub(crate) fn turn(&self) -> i64 {
        self.turn
    }

    pub(super) fn delegation_parts(&self) -> (&str, &str, &watch::Receiver<bool>) {
        (&self.entry_id, &self.space, &self.cancel)
    }

    pub(crate) fn check(&self, hub: &Hub) -> Result<BotConfig> {
        self.check_live(hub)?;
        self.check_db(&hub.store.db.locked())
    }

    pub(crate) fn check_live(&self, hub: &Hub) -> Result<()> {
        let running = RUNNING.locked();
        self.check_reserved(hub, running.as_ref().and_then(|m| m.get(&self.lane.chat)))
    }

    pub(super) fn check_reserved(&self, hub: &Hub, reservation: Option<&Running>) -> Result<()> {
        let owner = self.hub.upgrade().context("open_tools_local_turn_required")?;
        ensure!(
            std::ptr::eq(owner.as_ref(), hub)
                && self.lane.session.is_none()
                && reservation.is_some_and(|r| r.matches(&owner, &self.cancel)),
            "open_tools_local_turn_required"
        );
        let runtime = hub.runtime(&self.lane.chat);
        ensure!(
            runtime.message.as_deref() == Some(self.entry_id.as_str())
                && runtime.lane.as_ref() == Some(&self.lane)
                && runtime.status == BotStatus::Working,
            "open_tools_local_turn_required"
        );
        Ok(())
    }

    pub(crate) fn check_db(&self, db: &Connection) -> Result<BotConfig> {
        let bot = &self.lane.chat;
        ensure!(
            !*self.cancel.borrow()
                && self.cancel.has_changed().is_ok()
                && Store::entry_on(db, &self.entry_id)?.is_some_and(|e| {
                    e.is_user_authored()
                        && e.bot_id == *bot
                        && e.session_id == *bot
                        && e.turn == self.turn
                        && e.data["contentOrigin"]["ingress"]["caller"] == "local"
                        && e.data["contentOrigin"].get("hubOrigin").is_none()
                }),
            "open_tools_local_turn_required"
        );
        Store::bot_on(db, bot)?
            .filter(|r| !r.deleted && !r.config.hidden && !r.config.is_group() && r.config.space_id == self.space)
            .map(|r| r.config)
            .context("open_tools_local_turn_required")
    }
}
