use crate::fork::projetos::{
    delegacao_meta::{BlockerKind, OriginKind, RequesterKind},
    model::TaskState,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Perspective {
    Overview,
    Director,
    Bot,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Read {
    pub hub_id: Option<String>,
    pub space_id: String,
    pub perspective: Perspective,
    pub director_bot_id: Option<String>,
    pub bot_id: Option<String>,
}

impl Read {
    pub fn validate(&self) -> Result<()> {
        let valid = match (&self.perspective, &self.director_bot_id, &self.bot_id) {
            (Perspective::Overview, None, None) => true,
            (Perspective::Director, Some(id), None) | (Perspective::Bot, None, Some(id)) => !id.trim().is_empty(),
            _ => false,
        };
        ensure!(valid, "invalid_field: seletor incompatível com a visão da Torre.");
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Bucket {
    NeedsYou,
    ObservedWorking,
    WaitingOrBlocked,
    Backlog,
    Done,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Origin {
    pub kind: OriginKind,
    pub root_bot_id: Option<String>,
    pub delegated_by_bot_id: Option<String>,
    pub routine_id: Option<String>,
    pub admitted_at: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Blocker {
    pub kind: BlockerKind,
    pub reason: Option<String>,
    pub attention_item_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Delivery {
    pub has_evidence: bool,
    pub evidence_summary: Option<String>,
    pub accepted_by_owner: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ObservedExecution {
    pub active: bool,
    pub status: &'static str,
    pub observed_at: Option<i64>,
    pub machine_label: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Capabilities {
    pub can_director_interrupt: bool,
    pub can_owner_accept: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WorkCard {
    pub task_id: String,
    pub project_id: String,
    pub project_name: String,
    pub space_id: String,
    pub parent_task_id: Option<String>,
    pub title: String,
    pub state: TaskState,
    pub bucket: Bucket,
    pub origin: Origin,
    pub requester_label: RequesterKind,
    pub requester_bot_id: Option<String>,
    pub approver_role: &'static str,
    pub director_bot_id: Option<String>,
    pub assignee_bot_id: Option<String>,
    pub next_action: Option<String>,
    pub blocker: Blocker,
    pub delivery: Delivery,
    pub declared_updated_at: i64,
    pub observed_execution: ObservedExecution,
    pub capabilities: Capabilities,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Counts {
    pub needs_you: usize,
    pub observed_working: usize,
    pub waiting_or_blocked: usize,
    pub total_visible: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Snapshot {
    pub version: u8,
    pub hub_id: String,
    pub space_id: String,
    pub perspective: Perspective,
    pub director_bot_id: Option<String>,
    pub bot_id: Option<String>,
    pub collected_at: i64,
    pub cards: Vec<WorkCard>,
    pub counts: Counts,
}
