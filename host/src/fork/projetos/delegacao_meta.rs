use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OriginKind {
    OwnerDirect,
    Delegated,
    Routine,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RequesterKind {
    Owner,
    Bot,
    Routine,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BlockerKind {
    PermissionPause,
    OwnerDecision,
    Dependency,
    Manual,
    #[default]
    None,
}

#[derive(Default)]
pub(crate) struct Meta {
    pub origin_kind: OriginKind,
    pub root_bot_id: Option<String>,
    pub delegated_by_bot_id: Option<String>,
    pub routine_id: Option<String>,
    pub admitted_at: Option<i64>,
    pub requester_kind: RequesterKind,
    pub requester_bot_id: Option<String>,
    pub mandate_director_bot_id: Option<String>,
    pub next_action: Option<String>,
    pub blocker_kind: BlockerKind,
    pub blocker_reason: Option<String>,
}

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_task_delegation_meta(
        hub_id TEXT NOT NULL, space_id TEXT NOT NULL, project_id TEXT NOT NULL, task_id TEXT NOT NULL,
        origin_kind TEXT NOT NULL CHECK(origin_kind IN ('owner_direct','delegated','routine','unknown')),
        root_bot_id TEXT, delegated_by_bot_id TEXT, routine_id TEXT, admitted_at INTEGER,
        requester_kind TEXT NOT NULL CHECK(requester_kind IN ('owner','bot','routine','unknown')),
        requester_bot_id TEXT, approver_role TEXT NOT NULL DEFAULT 'owner' CHECK(approver_role='owner'),
        mandate_director_bot_id TEXT, next_action TEXT CHECK(length(next_action)<=240),
        blocker_kind TEXT NOT NULL DEFAULT 'none' CHECK(blocker_kind IN ('permission_pause','owner_decision','dependency','manual','none')),
        blocker_reason TEXT CHECK(length(blocker_reason)<=240),
        PRIMARY KEY(hub_id,space_id,project_id,task_id),
        FOREIGN KEY(hub_id,space_id,project_id) REFERENCES donna_projects(hub_id,space_id,id) ON DELETE CASCADE);
        CREATE TRIGGER IF NOT EXISTS donna_task_delegation_origin_immutable
        BEFORE UPDATE OF origin_kind ON donna_task_delegation_meta
        WHEN NEW.origin_kind <> OLD.origin_kind
        BEGIN SELECT RAISE(ABORT,'origin_kind is immutable'); END;",
    )?;
    Ok(())
}

fn chain_root(c: &Connection, (hub, space, project): (&str, &str, &str), parent: &str) -> Result<Option<String>> {
    let parent_meta: Option<(String, Option<String>)> = c
        .query_row(
            "SELECT origin_kind,root_bot_id FROM donna_task_delegation_meta WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4",
            params![hub, space, project, parent],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(match parent_meta {
        Some((origin, root)) if origin == "delegated" => root,
        _ => None,
    })
}

pub(crate) fn admit(
    c: &Connection,
    scope: (&str, &str, &str),
    task: &str,
    at: i64,
    author_bot: Option<&str>,
    parent: Option<&str>,
) -> Result<()> {
    let (kind, requester) = if author_bot.is_some() { ("delegated", "bot") } else { ("owner_direct", "owner") };
    let root = match (author_bot, parent) {
        (Some(bot), Some(parent)) => Some(chain_root(c, scope, parent)?.unwrap_or_else(|| bot.to_owned())),
        (bot, _) => bot.map(str::to_owned),
    };
    let (hub, space, project) = scope;
    c.execute(
        "INSERT INTO donna_task_delegation_meta(
            hub_id,space_id,project_id,task_id,origin_kind,root_bot_id,delegated_by_bot_id,
            admitted_at,requester_kind,requester_bot_id)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?7) ON CONFLICT(hub_id,space_id,project_id,task_id) DO NOTHING",
        params![hub, space, project, task, kind, root, author_bot, at, requester],
    )?;
    Ok(())
}

fn read_origin(raw: &str) -> Result<OriginKind> {
    Ok(match raw {
        "owner_direct" => OriginKind::OwnerDirect,
        "delegated" => OriginKind::Delegated,
        "routine" => OriginKind::Routine,
        "unknown" => OriginKind::Unknown,
        _ => anyhow::bail!("origin_kind inválido."),
    })
}

pub(crate) fn origin(c: &Connection, (hub, space, project): (&str, &str, &str), task: &str) -> Result<OriginKind> {
    let raw: Option<String> = c
        .query_row(
            "SELECT origin_kind FROM donna_task_delegation_meta WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4",
            params![hub, space, project, task],
            |r| r.get(0),
        )
        .optional()?;
    raw.as_deref().map_or(Ok(OriginKind::Unknown), read_origin)
}

pub(crate) fn load_space(c: &Connection, hub: &str, space: &str) -> Result<BTreeMap<(String, String), Meta>> {
    let mut st = c.prepare(
        "SELECT project_id,task_id,origin_kind,root_bot_id,delegated_by_bot_id,routine_id,admitted_at,
                requester_kind,requester_bot_id,mandate_director_bot_id,next_action,blocker_kind,blocker_reason
         FROM donna_task_delegation_meta WHERE hub_id=?1 AND space_id=?2",
    )?;
    let mut rows = st.query(params![hub, space])?;
    let mut metadata = BTreeMap::new();
    while let Some(r) = rows.next()? {
        let requester_kind = match r.get_ref(7)?.as_str()? {
            "owner" => RequesterKind::Owner,
            "bot" => RequesterKind::Bot,
            "routine" => RequesterKind::Routine,
            "unknown" => RequesterKind::Unknown,
            _ => anyhow::bail!("requester_kind inválido."),
        };
        let blocker_kind = match r.get_ref(11)?.as_str()? {
            "permission_pause" => BlockerKind::PermissionPause,
            "owner_decision" => BlockerKind::OwnerDecision,
            "dependency" => BlockerKind::Dependency,
            "manual" => BlockerKind::Manual,
            "none" => BlockerKind::None,
            _ => anyhow::bail!("blocker_kind inválido."),
        };
        metadata.insert(
            (r.get(0)?, r.get(1)?),
            Meta {
                origin_kind: read_origin(r.get_ref(2)?.as_str()?)?,
                root_bot_id: r.get(3)?,
                delegated_by_bot_id: r.get(4)?,
                routine_id: r.get(5)?,
                admitted_at: r.get(6)?,
                requester_kind,
                requester_bot_id: r.get(8)?,
                mandate_director_bot_id: r.get(9)?,
                next_action: r.get(10)?,
                blocker_kind,
                blocker_reason: r.get(12)?,
            },
        );
    }
    Ok(metadata)
}

/// Conta delegações nos quatro estados abertos do Space, mais o lote admitido.
/// A profundidade soma um elo novo aos ancestrais com origem delegada, inclusive os encerrados.
pub(crate) fn require_capacity(
    c: &Connection,
    (hub, space, project): (&str, &str, &str),
    tasks: &[Value],
    parent: Option<&str>,
    added: usize,
) -> Result<()> {
    let policy = crate::fork::governanca::store::read_policy(c, hub, space)?;
    let mut depth = 1;
    let mut cursor = parent;
    let mut visited = BTreeSet::new();
    while let Some(id) = cursor {
        ensure!(visited.insert(id), "invalid_field: ciclo na cadeia de tarefas.");
        let task =
            tasks.iter().find(|t| t["id"].as_str() == Some(id)).context("invalid_field: ancestral indisponível.")?;
        if origin(c, (hub, space, project), id)? == OriginKind::Delegated {
            depth += 1;
        }
        cursor = match task.get("parentTaskId") {
            None | Some(Value::Null) => None,
            Some(value) => Some(value.as_str().context("invalid_field: ancestral inválido.")?),
        };
    }
    ensure!(
        depth <= usize::from(policy.max_subagent_depth),
        "governance_depth_limit: profundidade de delegação excedida."
    );
    let active: i64 = c.query_row(
        "SELECT COUNT(*) FROM donna_projects p JOIN json_each(p.tasks) t
         JOIN donna_task_delegation_meta m ON m.hub_id=p.hub_id AND m.space_id=p.space_id AND m.project_id=p.id
            AND m.task_id=json_extract(t.value,'$.id')
         WHERE p.hub_id=?1 AND p.space_id=?2 AND m.origin_kind='delegated'
            AND json_extract(t.value,'$.state') IN ('backlog','in_progress','review','blocked')",
        params![hub, space],
        |r| r.get(0),
    )?;
    ensure!(
        active.saturating_add(i64::try_from(added)?) <= i64::from(policy.max_concurrent_subagents),
        "governance_concurrency_limit: limite de delegações abertas excedido."
    );
    Ok(())
}
