use super::model::{
    Blocker, Bucket, Capabilities, Counts, Delivery, ObservedExecution, Origin, Perspective, Read, Snapshot, WorkCard,
};
use crate::{
    fork::{
        projetos::{
            delegacao_meta::{self, BlockerKind},
            model::TaskState,
        },
        tarefas::db::reports_to,
    },
    store::now_ms,
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, params};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Project {
    id: String,
    name: String,
    coordinator_bot_id: Option<String>,
    created_at: Option<i64>,
    updated_at: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Task {
    id: String,
    parent_task_id: Option<String>,
    assignee_bot_id: Option<String>,
    title: String,
    evidence: String,
    state: TaskState,
    updated_at: Option<i64>,
}

pub(super) fn snapshot(db: &Connection, hub: &str, read: Read) -> Result<Snapshot> {
    let mut metadata = delegacao_meta::load_space(db, hub, &read.space_id)?;
    let mut projects =
        db.prepare("SELECT id,project,tasks FROM donna_projects WHERE hub_id=?1 AND space_id=?2 ORDER BY id")?;
    let mut rows = projects.query(params![hub, read.space_id])?;
    let mut cards = Vec::new();
    let mut counts = Counts::default();
    while let Some(row) = rows.next()? {
        let project_id = row.get::<_, String>(0)?;
        let project: Project = serde_json::from_str(row.get_ref(1)?.as_str()?)?;
        ensure!(project.id == project_id, "tower_view_unavailable: referência de projeto inválida.");
        let tasks: Vec<Task> = serde_json::from_str(row.get_ref(2)?.as_str()?)?;
        for task in tasks {
            let meta = metadata.remove(&(project_id.clone(), task.id.clone())).unwrap_or_default();
            match read.perspective {
                Perspective::Director => {
                    let Some(director) = read.director_bot_id.as_deref() else { continue };
                    if meta.mandate_director_bot_id.as_deref() != Some(director)
                        && project.coordinator_bot_id.as_deref() != Some(director)
                    {
                        let Some(assignee) = task.assignee_bot_id.as_deref() else { continue };
                        if reports_to(db, &read.space_id, assignee)?.as_deref() != Some(director) {
                            continue;
                        }
                    }
                }
                Perspective::Bot if read.bot_id.is_none() || task.assignee_bot_id != read.bot_id => continue,
                _ => {}
            }

            let bucket = match task.state {
                TaskState::Cancelled => continue,
                TaskState::Done => Bucket::Done,
                TaskState::Review => Bucket::NeedsYou,
                _ if meta.blocker_kind == BlockerKind::OwnerDecision => Bucket::NeedsYou,
                _ if meta.blocker_kind != BlockerKind::None => Bucket::WaitingOrBlocked,
                TaskState::Blocked => Bucket::WaitingOrBlocked,
                TaskState::InProgress => Bucket::ObservedWorking,
                TaskState::Backlog => Bucket::Backlog,
            };
            match bucket {
                Bucket::NeedsYou => counts.needs_you += 1,
                Bucket::ObservedWorking => counts.observed_working += 1,
                Bucket::WaitingOrBlocked => counts.waiting_or_blocked += 1,
                Bucket::Backlog | Bucket::Done => {}
            }
            let declared_updated_at = task
                .updated_at
                .or(project.updated_at)
                .or(project.created_at)
                .ok_or_else(|| anyhow!("tower_view_unavailable: data declarada indisponível."))?;
            cards.push(WorkCard {
                task_id: task.id,
                project_id: project_id.clone(),
                project_name: project.name.clone(),
                space_id: read.space_id.clone(),
                parent_task_id: task.parent_task_id,
                title: task.title,
                state: task.state,
                bucket,
                origin: Origin {
                    kind: meta.origin_kind,
                    root_bot_id: meta.root_bot_id,
                    delegated_by_bot_id: meta.delegated_by_bot_id,
                    routine_id: meta.routine_id,
                    admitted_at: meta.admitted_at,
                },
                requester_label: meta.requester_kind,
                requester_bot_id: meta.requester_bot_id,
                approver_role: "owner",
                director_bot_id: meta.mandate_director_bot_id.or_else(|| project.coordinator_bot_id.clone()),
                assignee_bot_id: task.assignee_bot_id,
                next_action: meta.next_action,
                blocker: Blocker { kind: meta.blocker_kind, reason: meta.blocker_reason, attention_item_id: None },
                delivery: Delivery {
                    has_evidence: !task.evidence.is_empty(),
                    evidence_summary: None,
                    accepted_by_owner: task.state == TaskState::Done,
                },
                declared_updated_at,
                observed_execution: ObservedExecution {
                    active: false,
                    status: "unavailable",
                    observed_at: None,
                    machine_label: None,
                },
                capabilities: Capabilities {
                    can_director_interrupt: matches!(read.perspective, Perspective::Director)
                        && meta.origin_kind.allows_bot_interrupt(),
                    can_owner_accept: matches!(read.perspective, Perspective::Overview)
                        && task.state == TaskState::Review,
                },
            });
        }
    }
    counts.total_visible = cards.len();
    Ok(Snapshot {
        version: 1,
        hub_id: hub.into(),
        space_id: read.space_id,
        perspective: read.perspective,
        director_bot_id: read.director_bot_id,
        bot_id: read.bot_id,
        collected_at: now_ms(),
        cards,
        counts,
    })
}
