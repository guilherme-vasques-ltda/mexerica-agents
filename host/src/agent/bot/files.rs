// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! File copies run outside the actor; publication still belongs to its original turn.
use super::Actor;
use crate::chat::files::{self, Snapshot};
use crate::fork::activity_code::ActivityKind;
use crate::store::EntryKind;
use anyhow::{Result, anyhow, ensure};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::{mpsc, oneshot};

type Reply = oneshot::Sender<Result<()>>;
pub(super) struct Completed {
    epoch: u64,
    chat: String,
    binding: Option<String>,
    execution: Option<crate::fork::sessoes::ExecutionRef>,
    turn_token: Option<String>,
    context: Option<Box<crate::fork::spaces::tool_context::ToolContext>>,
    cancelled: Arc<AtomicBool>,
    reply: Reply,
    result: Result<Snapshot>,
}

pub(super) struct FileShares {
    epoch: u64,
    pub summary: Option<String>,
    pending: Vec<Arc<AtomicBool>>,
    pub tx: mpsc::UnboundedSender<Completed>,
    pub rx: mpsc::UnboundedReceiver<Completed>,
}
impl Default for FileShares {
    fn default() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self { epoch: 0, summary: None, pending: Vec::new(), tx, rx }
    }
}
impl FileShares {
    pub fn cancel(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        for flag in self.pending.drain(..) {
            flag.store(true, Ordering::Release);
        }
    }
}
impl Drop for FileShares {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl Actor {
    pub(super) fn begin_file(
        &mut self,
        source: String,
        name: Option<String>,
        cancelled: Arc<AtomicBool>,
        execution: Option<crate::fork::sessoes::ExecutionRef>,
        reply: Reply,
        context: Option<Box<crate::fork::spaces::tool_context::ToolContext>>,
    ) {
        if let Err(error) = crate::fork::sessoes::bindings::recheck_optional(&self.hub, execution.as_ref())
            .and_then(|()| self.can_send_file())
        {
            let _ = reply.send(Err(error));
            return;
        }
        let binding = if self.cfg.context.is_some() {
            match crate::fork::spaces::execution_stamp(&self.hub.store, &self.cfg) {
                Ok(stamp) => Some(stamp),
                Err(error) => {
                    let _ = reply.send(Err(error));
                    return;
                }
            }
        } else {
            None
        };
        let chat = self.lane.chat.clone();
        self.files.pending.retain(|flag| !flag.load(Ordering::Acquire));
        if self.files.pending.len() >= 4 {
            let _ = reply.send(Err(anyhow!("too many pending file shares")));
            return;
        }
        self.files.pending.push(cancelled.clone());
        let epoch = self.files.epoch;
        let turn_token = context
            .as_ref()
            .map_or_else(|| self.hub.runtime(&self.cfg.id).memory_turn_token, |c| c.turn.memory_turn_token.clone());
        let root = files::root(&chat);
        let cwd = PathBuf::from(&self.cfg.cwd);
        let hub = self.hub.clone();
        let tx = self.files.tx.clone();
        tokio::spawn(async move {
            let flag = cancelled.clone();
            let result = tokio::task::spawn_blocking(move || {
                files::snapshot_for_share(&root, &cwd, &source, name.as_deref(), &flag, &hub.store)
            })
            .await;
            let result = result.map_err(anyhow::Error::from).and_then(std::convert::identity);
            let _ =
                tx.send(Completed { epoch, chat, binding, execution, turn_token, context, cancelled, reply, result });
        });
    }

    fn can_send_file(&self) -> Result<i64> {
        ensure!(!self.stop_requested && !self.waiting_for_reset(), "turn is stopping or waiting for the subscription");
        ensure!(
            self.active_group.is_none() && self.active_request.is_none() && self.active_routine.is_none(),
            "send_file is only available in the bot's own chat"
        );
        self.turn.ok_or_else(|| anyhow!("no turn is running"))
    }

    pub(super) fn finish_file(&mut self, completed: Completed) {
        self.files.pending.retain(|flag| !Arc::ptr_eq(flag, &completed.cancelled));
        let result = (|| {
            ensure!(
                completed.epoch == self.files.epoch
                    && !completed.cancelled.load(Ordering::Acquire)
                    && !completed.reply.is_closed(),
                "file sharing cancelled"
            );
            if let Some(context) = &completed.context {
                context.check_actor(&self.hub, &self.cfg, &self.lane, self.turn_session.as_deref())?;
            }
            ensure!(completed.chat == self.lane.chat, "file_context_changed");
            ensure!(completed.turn_token == self.hub.runtime(&self.cfg.id).memory_turn_token, "file_turn_changed");
            if let Some(binding) = &completed.binding {
                ensure!(
                    crate::fork::spaces::execution_stamp(&self.hub.store, &self.cfg)? == *binding,
                    "file_binding_changed"
                );
            }
            crate::fork::sessoes::bindings::recheck_optional(&self.hub, completed.execution.as_ref())?;
            let turn = self.can_send_file()?;
            ensure!(self.hub.store.bot(&self.cfg.id)?.is_some_and(|bot| !bot.deleted), "bot was deleted");
            let mut snapshot = completed.result?;
            let text = format!("{} ({} bytes)", snapshot.meta.name, snapshot.meta.size);
            self.close_seg();
            self.hub
                .add_entry_in_execution(
                    &self.lane,
                    EntryKind::Agent,
                    turn,
                    &json!({"text": text, "final": true,"author":self.cfg.id, "files": [snapshot.meta]}),
                    completed.execution.as_ref(),
                )
                .ok_or_else(|| anyhow!("couldn't save the file message"))?;
            snapshot.commit();
            self.files.summary = Some(text);
            self.set_activity("Working…", ActivityKind::Working);
            Ok(())
        })();
        let _ = completed.reply.send(result);
    }
}
