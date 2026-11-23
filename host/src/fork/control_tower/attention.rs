use super::{delegated_permissions, model::Observation, projection};
use crate::{
    LockExt,
    fork::{
        projetos::model::{Project, ProjectState, Task, TaskState},
        routine_creation,
    },
    hub::Hub,
    store::now_ms,
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Read {
    pub(super) space_id: String,
    #[serde(default = "yes")]
    pub(super) include_resolved_today: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Permission,
    DelegatedPermission,
    OwnerQuestion,
    TaskProposal,
    TaskReview,
    RoutineReview,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum Severity {
    Blocking,
    ActionRequired,
}

impl Kind {
    fn label(self) -> (&'static str, Severity, &'static str) {
        match self {
            Self::Permission => ("attention.permission", Severity::Blocking, "respond_permission"),
            Self::DelegatedPermission => ("attention.delegatedPermission", Severity::Blocking, "respond_permission"),
            Self::OwnerQuestion => ("attention.ownerQuestion", Severity::Blocking, "answer_question"),
            Self::TaskProposal => ("attention.taskProposal", Severity::ActionRequired, "decide_proposal"),
            Self::TaskReview => ("attention.taskReview", Severity::ActionRequired, "review_task"),
            Self::RoutineReview => ("attention.routineReview", Severity::ActionRequired, "review_routine"),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Subject {
    id: String,
    kind: Kind,
    title_key: &'static str,
    summary: Option<String>,
    target: Value,
}

impl Subject {
    fn new(id: String, kind: Kind, summary: Option<&str>, target: Value) -> Self {
        Self {
            id,
            kind,
            title_key: kind.label().0,
            summary: summary.map(|text| text.chars().take(240).collect()),
            target,
        }
    }

    fn pending(self, space_id: &str, created_at: i64) -> Item {
        let (_, severity, action_kind) = self.kind.label();
        Item {
            dedupe_key: self.id.clone(),
            subject: self,
            space_id: space_id.to_owned(),
            severity,
            action_kind,
            approver_role: "owner",
            created_at,
        }
    }

    fn resolved(self, resolved_at: i64, resolution_kind: &'static str) -> ResolvedItem {
        ResolvedItem { subject: self, resolved_at, resolution_kind }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    #[serde(flatten)]
    subject: Subject,
    dedupe_key: String,
    severity: Severity,
    space_id: String,
    action_kind: &'static str,
    approver_role: &'static str,
    created_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResolvedItem {
    #[serde(flatten)]
    subject: Subject,
    resolved_at: i64,
    resolution_kind: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Gap {
    source: &'static str,
    code: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttentionQueue {
    version: u8,
    hub_id: String,
    space_id: String,
    collected_at: i64,
    items: Vec<Item>,
    resolved_today: Vec<ResolvedItem>,
    coverage_gaps: Vec<Gap>,
}

#[derive(Default)]
struct Rows {
    items: Vec<Item>,
    resolved: Vec<ResolvedItem>,
}

impl AttentionQueue {
    fn add(&mut self, source: &'static str, read: Result<Rows>) {
        match read {
            Ok(rows) => {
                self.items.extend(rows.items);
                self.resolved_today.extend(rows.resolved);
            }
            Err(_) => self.gap(source),
        }
    }

    fn gap(&mut self, source: &'static str) {
        self.coverage_gaps.push(Gap { source, code: "source_unavailable" });
    }
}

pub(super) fn project(hub: &Hub, space: &str, include_resolved: bool) -> Result<AttentionQueue> {
    let now = now_ms();
    let today = include_resolved.then_some(now.div_euclid(86_400_000) * 86_400_000..=now);
    let mut queue = AttentionQueue {
        version: 1,
        hub_id: hub.host_id.clone(),
        space_id: space.into(),
        collected_at: now,
        items: vec![],
        resolved_today: vec![],
        coverage_gaps: vec![],
    };
    let (roster, conversations) = {
        let db = hub.store.db.locked();
        let tx = db.unchecked_transaction()?;
        crate::fork::spaces::require_space(&tx, space)?;
        let roster = projection::roster(&tx, space);
        if let Ok(roster) = &roster {
            let ids: Vec<_> = roster.iter().map(|cfg| cfg.id.as_str()).collect();
            queue.add("permissions", permissions(&tx, space, &ids, today.as_ref()));
            queue.add("delegated_permissions", delegated_permissions::read(&tx, space, &ids, -1).map(|found| Rows {
                items: found.items.into_iter().map(|row| Subject::new(format!("permission:{}", row.entry_id), Kind::DelegatedPermission, None,
                    json!({"botId":row.target_bot_id,"requesterBotId":row.requester_bot_id,"entryId":row.entry_id,"conversation":row.conversation})).pending(space, row.requested_at)).collect(),
                resolved: vec![],
            }));
        } else {
            queue.gap("permissions");
            queue.gap("delegated_permissions");
        }
        queue.add("proposals", proposals(&tx, &hub.host_id, space, today.as_ref()));
        let activity = task_history(&tx, &hub.host_id, space, now);
        queue.add("task_reviews", tasks(&tx, &hub.host_id, space, today.as_ref(), activity.as_ref().ok()));
        if activity.is_err() && !queue.coverage_gaps.iter().any(|gap| gap.source == "task_reviews") {
            queue.gap("task_reviews");
        }
        queue.add("routine_reviews", routines(&tx, space, now, today.as_ref()));
        let conversations = projection::conversations(&tx, space);
        tx.commit()?;
        (roster, conversations)
    };
    let mut question_gap = false;
    if let (Ok(roster), Ok(conversations)) = (roster, conversations) {
        for cfg in &roster {
            let projected = projection::attention_questions(hub, cfg, space, &conversations);
            let verified = verified_questions(hub, space, cfg, projected, today.as_ref());
            match verified {
                Ok((rows, complete)) => {
                    queue.items.extend(rows.items);
                    queue.resolved_today.extend(rows.resolved);
                    question_gap |= !complete;
                }
                Err(_) => question_gap = true,
            }
        }
    } else {
        question_gap = true;
    }
    if question_gap {
        queue.gap("owner_questions");
    }

    let mut by_key = BTreeMap::new();
    for item in queue.items.drain(..) {
        by_key.insert(item.dedupe_key.clone(), item);
    }
    queue.resolved_today.retain(|row| !by_key.contains_key(&row.subject.id));
    queue.items = by_key.into_values().collect();
    queue.items.sort_by_key(|item| (item.severity, Reverse(item.created_at)));
    queue
        .resolved_today
        .sort_by(|a, b| b.resolved_at.cmp(&a.resolved_at).then_with(|| a.subject.id.cmp(&b.subject.id)));
    Ok(queue)
}

type Today = std::ops::RangeInclusive<i64>;

fn permissions(c: &Connection, space: &str, roster: &[&str], today: Option<&Today>) -> Result<Rows> {
    let mut statement = c.prepare(
        "SELECT e.id,c.bot_id,c.id,c.revision,s.id,e.created_at,e.updated_at,json_extract(e.data,'$.status')
         FROM donna_space_conversations c
         JOIN entries e ON e.bot_id=c.id AND e.kind='permission'
         JOIN donna_bot_sessions s ON s.id=COALESCE(e.session,e.bot_id) AND s.chat_id=c.id
         WHERE c.space_id=?1 AND EXISTS(SELECT 1 FROM json_each(?2) WHERE value=c.bot_id)
         AND ((e.updated_at BETWEEN ?3 AND ?4 AND json_extract(e.data,'$.status')<>'pending')
              OR (c.closed=0 AND s.state='open' AND json_extract(e.data,'$.status')='pending'))",
    )?;
    let mut rows = statement.query(params![
        space,
        serde_json::to_string(roster)?,
        today.map(Today::start),
        today.map(Today::end)
    ])?;
    let mut found = Rows::default();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let status: String = row.get(7)?;
        let target = json!({"entryId":id,"botId":row.get::<_,String>(1)?,"conversation":{"conversationId":row.get::<_,String>(2)?,"revision":row.get::<_,i64>(3)?},"sessionId":row.get::<_,String>(4)?});
        let subject = Subject::new(format!("permission:{id}"), Kind::Permission, None, target);
        match status.as_str() {
            "pending" => found.items.push(subject.pending(space, row.get(5)?)),
            "answered" => found.resolved.push(subject.resolved(row.get(6)?, "answered")),
            "withdrawn" | "cancelled" | "expired" => found.resolved.push(subject.resolved(row.get(6)?, "cancelled")),
            _ => anyhow::bail!("attention_permission_state_invalid"),
        }
    }
    Ok(found)
}

fn proposals(c: &Connection, hub: &str, space: &str, today: Option<&Today>) -> Result<Rows> {
    let mut statement = c.prepare(
        "SELECT id,project_id,bot_id,title,state,created_at,decided_at FROM donna_task_proposals
         WHERE hub_id=?1 AND space_id=?2 AND (state='pending' OR (state IN ('approved','rejected') AND decided_at BETWEEN ?3 AND ?4))",
    )?;
    let mut rows = statement.query(params![hub, space, today.map(Today::start), today.map(Today::end)])?;
    let mut found = Rows::default();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let title: String = row.get(3)?;
        let subject = Subject::new(
            format!("task_proposal:{id}"),
            Kind::TaskProposal,
            Some(&title),
            json!({"proposalId":id,"projectId":row.get::<_,String>(1)?,"botId":row.get::<_,String>(2)?}),
        );
        match row.get::<_, String>(4)?.as_str() {
            "pending" => found.items.push(subject.pending(space, row.get(5)?)),
            "approved" => found.resolved.push(subject.resolved(row.get(6)?, "approved")),
            "rejected" => found.resolved.push(subject.resolved(row.get(6)?, "rejected")),
            _ => anyhow::bail!("attention_proposal_state_invalid"),
        }
    }
    Ok(found)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct History {
    at: i64,
    author: String,
    action: String,
    task_id: Option<String>,
    summary: String,
}

#[derive(Default)]
struct TaskActivity {
    review_at: Option<i64>,
    resolution: Option<(i64, &'static str)>,
}
type TaskHistory = HashMap<String, HashMap<String, TaskActivity>>;

fn task_history(c: &Connection, hub: &str, space: &str, now: i64) -> Result<TaskHistory> {
    let mut statement =
        c.prepare("SELECT project_id,entry FROM donna_project_history WHERE hub_id=?1 AND space_id=?2 ORDER BY seq")?;
    let mut rows = statement.query(params![hub, space])?;
    let mut history = TaskHistory::new();
    while let Some(row) = rows.next()? {
        let entry: History = serde_json::from_str(&row.get::<_, String>(1)?)?;
        let Some(task) = entry.task_id else { continue };
        if entry.at > now {
            continue;
        }
        let activity = history.entry(row.get(0)?).or_default().entry(task).or_default();
        // The existing journal has no structured from/to fields. Classify only the producer's exact transition forms.
        let transition = if entry.action == "task_updated" && entry.author == "owner" {
            entry.summary.rsplit_once(". Estado: ").and_then(|(_, state)| state.strip_suffix('.'))
        } else if entry.action == "task_updated" && entry.author.starts_with("bot:") {
            entry
                .summary
                .strip_prefix("Bot alterou a subtarefa: estado ")
                .and_then(|state| state.split_once([',', '.']).map(|(state, _)| state))
        } else {
            None
        };
        if let Some((from, to)) = transition.and_then(|state| state.split_once(" → ")) {
            if to == "Em conferência" {
                activity.review_at = Some(entry.at);
                activity.resolution = None;
            }
            if from == "Em conferência" {
                let resolution = match to {
                    "Concluída manualmente" if entry.author == "owner" => Some("accepted"),
                    "A fazer" | "Em andamento declarado pelo dono" | "Em andamento" | "Bloqueada" => Some("reopened"),
                    "Cancelada" => Some("cancelled"),
                    _ => None,
                };
                if let Some(resolution) = resolution {
                    activity.resolution = Some((entry.at, resolution));
                }
            }
        } else if entry.author == "owner"
            && entry.action == "task_created"
            && entry.summary == "Tarefa criada manualmente. Estado: Em conferência."
        {
            activity.review_at = Some(entry.at);
        }
    }
    Ok(history)
}

fn tasks(c: &Connection, hub: &str, space: &str, today: Option<&Today>, history: Option<&TaskHistory>) -> Result<Rows> {
    let mut statement = c.prepare("SELECT id,project,tasks FROM donna_projects WHERE hub_id=?1 AND space_id=?2")?;
    let mut rows = statement.query(params![hub, space])?;
    let mut found = Rows::default();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let project: Project = serde_json::from_str(&row.get::<_, String>(1)?)?;
        ensure!(project.id == id, "attention_project_identity_invalid");
        let activity = history.and_then(|history| history.get(&id));
        for task in serde_json::from_str::<Vec<Task>>(&row.get::<_, String>(2)?)? {
            let state = activity.and_then(|activity| activity.get(&task.id));
            let target = json!({"projectId":id,"taskId":task.id});
            let subject =
                Subject::new(format!("task_review:{id}:{}", task.id), Kind::TaskReview, Some(&task.title), target);
            if task.state == TaskState::Review && project.state == ProjectState::Active {
                found.items.push(
                    subject.pending(space, state.and_then(|state| state.review_at).unwrap_or(project.created_at)),
                );
            } else if task.state != TaskState::Review
                && let Some((at, kind)) = state.and_then(|state| state.resolution)
                && today.is_some_and(|today| today.contains(&at))
            {
                found.resolved.push(subject.resolved(at, kind));
            }
        }
    }
    Ok(found)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoutineReview {
    entry_id: String,
    routine_id: String,
    bot_id: String,
    name: String,
    created_at: i64,
}

fn routines(c: &Connection, space: &str, now: i64, today: Option<&Today>) -> Result<Rows> {
    let Observation::Available { value, .. } = routine_creation::projection(c, space, now)? else {
        anyhow::bail!("attention_routine_reviews_unavailable");
    };
    let mut found = Rows::default();
    for row in serde_json::from_value::<Vec<RoutineReview>>(serde_json::to_value(value)?["items"].take())? {
        let target = json!({"entryId":row.entry_id,"routineId":row.routine_id,"botId":row.bot_id});
        found.items.push(
            Subject::new(row.entry_id, Kind::RoutineReview, Some(&row.name), target).pending(space, row.created_at),
        );
    }
    let Some(today) = today else { return Ok(found) };
    let ids = c.prepare("SELECT e.id FROM entries e JOIN bots b ON b.id=e.bot_id AND b.deleted=0
        WHERE e.kind='notice' AND json_extract(b.config,'$.spaceId')=?1 AND json_extract(e.data,'$.routineCreation.spaceId')=?1
        AND json_extract(e.data,'$.routineCreation.decision') IN ('kept','enabled','cancelled','external_granted','external_revoked')
        AND e.updated_at BETWEEN ?2 AND ?3")?
        .query_map(params![space, today.start(), today.end()], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for id in ids {
        let (entry, card) = routine_creation::card_on(c, space, &id)?;
        let kind = match card.decision.as_str() {
            "kept" | "enabled" => "kept",
            "external_granted" => "approved",
            _ => "cancelled",
        };
        let target = json!({"entryId":id,"routineId":card.routine_id,"botId":card.bot_id});
        found.resolved.push(
            Subject::new(id, Kind::RoutineReview, entry.data["routineCreation"]["name"].as_str(), target)
                .resolved(entry.updated_at, kind),
        );
    }
    Ok(found)
}

fn verified_questions(
    hub: &Hub,
    space: &str,
    cfg: &crate::store::BotConfig,
    projected: Observation<Vec<super::model::Question>>,
    today: Option<&Today>,
) -> Result<(Rows, bool)> {
    let entries = {
        let db = hub.store.db.locked();
        let mut statement = db.prepare("SELECT e.id,c.id,c.revision,s.id,json_extract(e.data,'$.ownerQuestion.status'),json_extract(e.data,'$.ownerQuestion.closedAt')
            FROM donna_space_conversations c JOIN entries e ON e.bot_id=c.id
            JOIN donna_bot_sessions s ON s.id=COALESCE(e.session,e.bot_id) AND s.chat_id=c.id
            WHERE c.space_id=?1 AND e.kind='notice' AND json_extract(e.data,'$.card')='ownerQuestion'
            AND json_extract(e.data,'$.author')=?2
            AND ( (c.closed=0 AND s.state='open' AND json_extract(e.data,'$.ownerQuestion.status')='pending')
                 OR json_extract(e.data,'$.ownerQuestion.closedAt') BETWEEN ?3 AND ?4 )")?;
        statement
            .query_map(params![space, cfg.id, today.map(Today::start), today.map(Today::end)], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut found = Rows::default();
    let mut complete = true;
    let questions = match projected {
        Observation::Available { value, .. } => value,
        Observation::Unavailable => vec![],
    };
    for (id, chat, revision, session, status, closed_at) in entries {
        if !crate::fork::runtime_audiencia::conversation_in_space(hub, cfg, space, &chat)? {
            continue;
        }
        let target = json!({"entryId":id,"botId":cfg.id,"conversation":{"conversationId":chat,"revision":revision},"sessionId":session});
        match status.as_str() {
            "pending" => {
                if let Some(q) = questions.iter().find(|q| q.entry_id == id && q.conversation.conversation_id == chat) {
                    found.items.push(
                        Subject::new(format!("owner_question:{id}"), Kind::OwnerQuestion, Some(&q.text), target)
                            .pending(space, q.asked_at),
                    );
                } else {
                    complete = false;
                }
            }
            "answered" | "withdrawn" => found.resolved.push(
                Subject::new(format!("owner_question:{id}"), Kind::OwnerQuestion, None, target).resolved(
                    closed_at.context("attention_question_resolution_missing")?,
                    if status == "answered" { "answered" } else { "cancelled" },
                ),
            ),
            _ => anyhow::bail!("attention_question_state_invalid"),
        }
    }
    Ok((found, complete))
}
