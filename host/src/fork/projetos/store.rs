use super::model::{
    CommentWrite, CommentsRead, Create, EnsureDefault, List, Project, ProjectState, Read, Save, Task, TaskMeta,
    TaskMetaSave, TaskState, TasksPage,
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

fn parse<T: DeserializeOwned>(body: Value) -> Result<T> {
    serde_json::from_value(body).map_err(|_| anyhow::anyhow!("invalid_field: solicitação de Projetos inválida."))
}
fn uuid(id: &str) -> Result<()> {
    ensure!(uuid::Uuid::parse_str(id).is_ok_and(|u| u.to_string() == id), "invalid_field: UUID canônico obrigatório.");
    Ok(())
}
fn text(value: &str, max: usize, required: bool) -> Result<()> {
    ensure!(
        value.chars().count() <= max
            && (!required || !value.trim().is_empty())
            && !value.chars().any(|c| (c.is_control() && c != '\n' && c != '\t')
                || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')),
        "projects_limit: texto vazio ou longo demais."
    );
    Ok(())
}
pub(crate) fn bots(c: &Connection, space: &str) -> Result<Value> {
    let mut st = c.prepare("SELECT config,avatar_config FROM bots WHERE deleted=0 AND json_extract(config,'$.spaceId')=?1 AND COALESCE(json_extract(config,'$.hidden'),0)=0 ORDER BY id")?;
    let rows = st.query_map([space], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?;
    let mut result = Vec::new();
    for row in rows {
        let (raw, avatar) = row?;
        let cfg = crate::fork_avatars::restored(&raw, avatar.as_deref())?;
        if cfg.space_id == space && !cfg.hidden && !cfg.is_group() {
            result.push(json!({"id":cfg.id,"name":cfg.name}));
        }
    }
    Ok(json!(result))
}
fn reference(id: Option<&str>, old: Option<&str>, bots: &Value) -> Result<()> {
    if let Some(id) = id {
        ensure!(
            Some(id) == old || bots.as_array().is_some_and(|b| b.iter().any(|b| b["id"].as_str() == Some(id))),
            "invalid_field: responsável indisponível."
        );
    }
    Ok(())
}
pub(crate) fn load(c: &Connection, hub: &str, space: &str, id: &str) -> Result<(Project, Vec<Task>)> {
    uuid(id)?;
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT project,tasks FROM donna_projects WHERE hub_id=?1 AND space_id=?2 AND id=?3",
            params![hub, space, id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (project, tasks) = row.ok_or_else(|| anyhow::anyhow!("project_not_found: Projeto indisponível."))?;
    Ok((serde_json::from_str(&project)?, serde_json::from_str(&tasks)?))
}
pub(crate) fn snapshot(c: &Connection, hub: &str, space: &str, id: &str, extended_history: bool) -> Result<Value> {
    let (project, tasks) = load(c, hub, space, id)?;
    let owner_only = if extended_history { "" } else { " AND json_extract(entry,'$.author')='owner'" };
    let total: i64 = c.query_row(
        &format!(
            "SELECT COUNT(*) FROM donna_project_history WHERE hub_id=?1 AND space_id=?2 AND project_id=?3{owner_only}"
        ),
        params![hub, space, id],
        |r| r.get(0),
    )?;
    let mut st = c.prepare(&format!(
        "SELECT entry FROM donna_project_history WHERE hub_id=?1 AND space_id=?2 AND project_id=?3{owner_only} ORDER BY seq DESC LIMIT 100"
    ))?;
    let history = st
        .query_map(params![hub, space, id], |r| r.get::<_, String>(0))?
        .map(|r| Ok(serde_json::from_str::<Value>(&r?)?))
        .collect::<Result<Vec<_>>>()?;
    let meta = task_meta(c, hub, space, id)?;
    let declarations = task_declarations(c, hub, space, id)?;
    let mut answer = json!({"version":1,"hubId":hub,"spaceId":space,"project":project,"tasks":tasks,"meta":meta,"declarations":declarations,"history":history,"historyOmitted":(total-100).max(0),"bots":bots(c, space)?});
    if !extended_history {
        let all: i64 = c.query_row(
            "SELECT COUNT(*) FROM donna_project_history WHERE hub_id=?1 AND space_id=?2 AND project_id=?3",
            params![hub, space, id],
            |r| r.get(0),
        )?;
        let hidden = (all - total).max(0);
        if hidden > 0 {
            answer["historyPartial"] = true.into();
            answer["historyHidden"] = hidden.into();
        }
    }
    Ok(answer)
}
pub(crate) fn task_meta(c: &Connection, hub: &str, space: &str, project: &str) -> Result<Value> {
    let mut st = c.prepare(
        "SELECT task_id,meta FROM donna_task_meta WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 ORDER BY task_id",
    )?;
    let mut result = serde_json::Map::new();
    for row in st.query_map(params![hub, space, project], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (id, raw) = row?;
        let meta: TaskMeta = serde_json::from_str(&raw)?;
        result.insert(id, serde_json::to_value(meta)?);
    }
    Ok(Value::Object(result))
}
fn task_declarations(c: &Connection, hub: &str, space: &str, project: &str) -> Result<Value> {
    let mut st = c.prepare(
        "SELECT task_id,bot_id,at FROM donna_task_declarations WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 ORDER BY task_id",
    )?;
    let mut result = serde_json::Map::new();
    for row in st.query_map(params![hub, space, project], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
    })? {
        let (id, bot, at) = row?;
        result.insert(id, json!({"botId":bot,"at":at}));
    }
    Ok(Value::Object(result))
}
/// Declaração durável de conclusão por bot: gravada somente na transição real
/// para `done` (estado anterior diferente), na mesma transação da escrita.
/// Hoje só restam declarações legadas; nenhuma frente de bot grava mais.
#[cfg(test)]
pub(crate) fn declare_task_done(
    c: &Connection,
    hub: &str,
    space: &str,
    project: &str,
    task: &str,
    bot: &str,
) -> Result<()> {
    c.execute(
        "INSERT INTO donna_task_declarations(hub_id,space_id,project_id,task_id,bot_id,at) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(hub_id,space_id,project_id,task_id) DO UPDATE SET bot_id=excluded.bot_id,at=excluded.at",
        params![hub, space, project, task, bot, crate::store::now_ms()],
    )?;
    Ok(())
}
pub(crate) fn clear_task_declaration(c: &Connection, hub: &str, space: &str, project: &str, task: &str) -> Result<()> {
    c.execute(
        "DELETE FROM donna_task_declarations WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4",
        params![hub, space, project, task],
    )?;
    Ok(())
}
fn default_project(c: &Connection, hub: &str, space: &str) -> Result<Option<Project>> {
    let mut st = c.prepare("SELECT project FROM donna_projects WHERE hub_id=?1 AND space_id=?2 ORDER BY id")?;
    let rows = st.query_map(params![hub, space], |r| r.get::<_, String>(0))?;
    let mut found = None;
    for row in rows {
        let project: Project = serde_json::from_str(&row?)?;
        if project.state == ProjectState::Active && project.is_default && found.is_none() {
            found = Some(project);
        }
    }
    Ok(found)
}
pub(crate) fn history_as(
    c: &Connection,
    (hub, space, id): (&str, &str, &str),
    at: i64,
    author: &str,
    action: &str,
    task: Option<&str>,
    summary: &str,
) -> Result<()> {
    let mut entry =
        json!({"id":uuid::Uuid::new_v4().to_string(),"at":at,"author":author,"action":action,"summary":summary});
    if let Some(task) = task {
        entry["taskId"] = task.into();
    }
    c.execute(
        "INSERT INTO donna_project_history(hub_id,space_id,project_id,entry) VALUES(?1,?2,?3,?4)",
        params![hub, space, id, serde_json::to_string(&entry)?],
    )?;
    Ok(())
}
fn history(
    c: &Connection,
    ids: (&str, &str),
    id: &str,
    at: i64,
    action: &str,
    task: Option<&str>,
    summary: &str,
) -> Result<()> {
    history_as(c, (ids.0, ids.1, id), at, "owner", action, task, summary)
}
pub(crate) fn validate(tasks: &[Task], old: &[Task], roster: &Value) -> Result<()> {
    ensure!(tasks.len() <= 100, "projects_limit: máximo de 100 tarefas.");
    let ids: BTreeSet<_> = tasks.iter().map(|t| t.id.as_str()).collect();
    ensure!(ids.len() == tasks.len(), "invalid_field: tarefa duplicada.");
    ensure!(
        old.iter().all(|t| ids.contains(t.id.as_str())),
        "invalid_field: cancele a tarefa para preservar seu registro."
    );
    for task in tasks {
        uuid(&task.id)?;
        text(&task.title, 200, true)?;
        text(&task.description, 4000, false)?;
        text(&task.acceptance, 2000, false)?;
        text(&task.evidence, 4000, false)?;
        let previous = old.iter().find(|t| t.id == task.id).and_then(|t| t.assignee_bot_id.as_deref());
        reference(task.assignee_bot_id.as_deref(), previous, roster)?;
        let mut path = BTreeSet::new();
        let mut cursor = Some(task.id.as_str());
        while let Some(id) = cursor {
            ensure!(path.insert(id) && path.len() <= 3, "invalid_field: ciclo ou mais de três níveis.");
            let node = tasks
                .iter()
                .find(|t| t.id == id)
                .ok_or_else(|| anyhow::anyhow!("invalid_field: pai fora do Projeto."))?;
            cursor = node.parent_task_id.as_deref();
        }
    }
    Ok(())
}
fn replay(
    c: &Connection,
    request: &str,
    payload: &str,
    hub: &str,
    space: &str,
    extended_history: bool,
) -> Result<Option<Value>> {
    uuid(request)?;
    let row: Option<(String, String, i64)> = c
        .query_row(
            "SELECT payload,project_id,revision FROM donna_project_receipts WHERE hub_id=?1 AND space_id=?2 AND request_id=?3",
            params![hub, space, request],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    if let Some((previous, id, revision)) = row {
        ensure!(previous == payload, "projects_idempotency: pedido reutilizado com outro conteúdo.");
        return Ok(Some(
            json!({"snapshot":snapshot(c,hub,space,&id,extended_history)?,"receipt":{"clientRequestId":request,"revision":revision}}),
        ));
    }
    Ok(None)
}
fn comment_replay(c: &Connection, request: &str, payload: &str, hub: &str, space: &str) -> Result<Option<Value>> {
    uuid(request)?;
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT payload,entry FROM donna_task_comments WHERE hub_id=?1 AND space_id=?2 AND request_id=?3",
            params![hub, space, request],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((previous, entry)) = row {
        ensure!(previous == payload, "projects_idempotency: pedido reutilizado com outro conteúdo.");
        return Ok(Some(
            json!({"comment":serde_json::from_str::<Value>(&entry)?,"receipt":{"clientRequestId":request}}),
        ));
    }
    Ok(None)
}

pub(super) fn call(c: &Connection, hub: &str, method: &str, mut body: Value) -> Result<(Value, bool)> {
    let space_id = body["spaceId"].as_str().unwrap_or_default().to_owned();
    let space = space_id.as_str();
    let extended_history = body["historyAuthors"].as_bool().unwrap_or(false);
    if let Some(map) = body.as_object_mut() {
        map.remove("historyAuthors");
    }
    body.sort_all_objects();
    let canonical = serde_json::to_vec(&json!({"method":method,"body":body}))?;
    let payload = Sha256::digest(canonical).iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    });
    match method {
        "donnaTaskComments" => {
            let r: CommentsRead = parse(body)?;
            super::scope(c, &r.hub, &r.space, hub)?;
            uuid(&r.project)?;
            uuid(&r.task)?;
            let (project, tasks) = load(c, hub, space, &r.project)?;
            ensure!(project.id == r.project, "projects_invalid: identidade do Projeto divergente.");
            ensure!(tasks.iter().any(|t| t.id == r.task), "invalid_field: tarefa indisponível.");
            let total: i64 = c.query_row(
                "SELECT COUNT(*) FROM donna_task_comments WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4",
                params![hub, space, project.id, r.task],
                |r| r.get(0),
            )?;
            let mut st = c.prepare(
                "SELECT entry FROM donna_task_comments WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 ORDER BY seq DESC LIMIT 100",
            )?;
            let mut comments = st
                .query_map(params![hub, space, project.id, r.task], |r| r.get::<_, String>(0))?
                .map(|r| Ok(serde_json::from_str::<Value>(&r?)?))
                .collect::<Result<Vec<_>>>()?;
            comments.reverse();
            Ok((
                json!({"version":1,"hubId":hub,"spaceId":r.space,"projectId":project.id,"taskId":r.task,"comments":comments,"omitted":(total-100).max(0)}),
                false,
            ))
        }
        "donnaTaskComment" => {
            let r: CommentWrite = parse(body)?;
            super::scope(c, &r.hub_id, &r.space_id, hub)?;
            uuid(&r.project_id)?;
            uuid(&r.task_id)?;
            uuid(&r.client_request_id)?;
            if let Some(answer) = comment_replay(c, &r.client_request_id, &payload, hub, space)? {
                return Ok((answer, false));
            }
            let (project, tasks) = load(c, hub, space, &r.project_id)?;
            ensure!(project.id == r.project_id, "projects_invalid: identidade do Projeto divergente.");
            ensure!(project.state == ProjectState::Active, "invalid_field: Projeto arquivado.");
            ensure!(tasks.iter().any(|t| t.id == r.task_id), "invalid_field: tarefa indisponível.");
            text(&r.text, 4000, true)?;
            ensure!(r.mentions.len() <= 20, "projects_limit: máximo de 20 menções.");
            let roster = bots(c, space)?;
            let names: BTreeMap<String, String> = roster
                .as_array()
                .map(|bots| {
                    bots.iter()
                        .filter_map(|b| Some((b["id"].as_str()?.to_owned(), b["name"].as_str()?.to_owned())))
                        .collect()
                })
                .unwrap_or_default();
            let available: BTreeSet<String> = names.keys().cloned().collect();
            let mut mentions = Vec::new();
            let mut seen = BTreeSet::new();
            for mention in r.mentions {
                ensure!(
                    !mention.trim().is_empty() && available.contains(&mention),
                    "invalid_field: menção indisponível."
                );
                // A menção declarada precisa de `@Nome` real no texto — mesmo
                // predicado compartilhado do Hub; a audiência gravada são os
                // IDs, estável a renomeação posterior do bot.
                ensure!(
                    crate::fork::mentions::text_mentions(
                        &r.text,
                        names.get(&mention).map(String::as_str).unwrap_or_default()
                    ),
                    "invalid_field: menção sem @ no texto."
                );
                if seen.insert(mention.clone()) {
                    mentions.push(mention);
                }
            }
            let bot_ids = mentions.clone();
            let notify = if mentions.is_empty() {
                json!({"botIds":[],"state":"not_notified","reason":"no_mention"})
            } else if r.only_note {
                json!({"botIds":bot_ids,"state":"not_notified","reason":"only_note"})
            } else {
                json!({"botIds":bot_ids,"state":"notifying"})
            };
            let comment = json!({
                "id":uuid::Uuid::new_v4().to_string(),
                "at":crate::store::now_ms(),
                "author":"owner",
                "text":r.text,
                "mentions":mentions,
                "onlyNote":r.only_note,
                "notify":notify,
                // Marcador de validação gravado pelo Hub: prova de que cada
                // menção teve `@Nome` real na escrita. Clientes não forjam —
                // o entry é montado aqui e `author`/`mentionCheck` no corpo
                // são recusados. Entradas legadas sem o campo ficam sem prova.
                "mentionCheck":2,
            });
            c.execute(
                "INSERT INTO donna_task_comments(hub_id,space_id,project_id,task_id,comment_id,request_id,payload,entry) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![hub,space,project.id,r.task_id,comment["id"].as_str().unwrap_or_default(),r.client_request_id,payload,serde_json::to_string(&comment)?],
            )?;
            Ok((json!({"comment":comment,"receipt":{"clientRequestId":r.client_request_id}}), true))
        }
        "donnaProjectEnsureDefault" => {
            let r: EnsureDefault = parse(body)?;
            super::scope(c, &r.hub_id, &r.space_id, hub)?;
            if let Some(project) = default_project(c, hub, space)? {
                return Ok((
                    json!({"version":1,"hubId":hub,"spaceId":r.space_id,"snapshot":snapshot(c,hub,space,&project.id,extended_history)?,"created":false}),
                    false,
                ));
            }
            let mut st =
                c.prepare("SELECT id,project FROM donna_projects WHERE hub_id=?1 AND space_id=?2 ORDER BY id")?;
            let rows = st
                .query_map(params![hub, space], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut adopt = None;
            for (id, raw) in rows {
                let project: Project = serde_json::from_str(&raw)?;
                if project.state == ProjectState::Active && project.name == "Geral" && !project.is_default {
                    adopt = Some((id, project));
                    break;
                }
            }
            let now = crate::store::now_ms();
            let (project, created) = if let Some((id, mut project)) = adopt {
                project.is_default = true;
                project.revision = project
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("projects_limit: revisão esgotada."))?;
                project.updated_at = now;
                c.execute(
                    "UPDATE donna_projects SET project=?4 WHERE hub_id=?1 AND space_id=?2 AND id=?3",
                    params![hub, space, id, serde_json::to_string(&project)?],
                )?;
                history_as(
                    c,
                    (hub, space, id.as_str()),
                    now,
                    "system",
                    "updated",
                    None,
                    "Projeto marcado como Geral.",
                )?;
                (project, false)
            } else {
                let count: i64 = c.query_row(
                    "SELECT COUNT(*) FROM donna_projects WHERE hub_id=?1 AND space_id=?2",
                    params![hub, space],
                    |r| r.get(0),
                )?;
                ensure!(count < 50, "projects_limit: máximo de 50 Projetos.");
                let project = Project {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: "Geral".into(),
                    objective: "Tarefas simples sem projeto específico.".into(),
                    coordinator_bot_id: None,
                    state: ProjectState::Active,
                    is_default: true,
                    revision: 1,
                    created_at: now,
                    updated_at: now,
                };
                c.execute(
                    "INSERT INTO donna_projects(hub_id,space_id,id,project,tasks) VALUES(?1,?2,?3,?4,'[]')",
                    params![hub, space, project.id, serde_json::to_string(&project)?],
                )?;
                history_as(
                    c,
                    (hub, space, project.id.as_str()),
                    now,
                    "system",
                    "created",
                    None,
                    "Projeto Geral criado automaticamente.",
                )?;
                (project, true)
            };
            Ok((
                json!({"version":1,"hubId":hub,"spaceId":r.space_id,"snapshot":snapshot(c,hub,space,&project.id,extended_history)?,"created":created}),
                true,
            ))
        }
        "donnaTasks" => {
            let r: TasksPage = parse(body)?;
            super::scope(c, &r.hub, &r.space, hub)?;
            if let Some(cursor) = r.after_project.as_deref() {
                uuid(cursor)?;
            }
            let mut st = c.prepare(
                "SELECT id,project,tasks FROM donna_projects WHERE hub_id=?1 AND space_id=?2 AND id>?3 ORDER BY id",
            )?;
            let rows = st.query_map(params![hub, space, r.after_project.as_deref().unwrap_or("")], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            })?;
            let mut page = None;
            let mut more = false;
            for row in rows {
                let (id, project, tasks) = row?;
                let project: Project = serde_json::from_str(&project)?;
                ensure!(project.id == id, "projects_invalid: identidade do Projeto divergente.");
                if project.state == ProjectState::Archived {
                    continue;
                }
                if page.is_some() {
                    more = true;
                    break;
                }
                let tasks: Vec<Task> = serde_json::from_str(&tasks)?;
                page = Some((project, tasks));
            }
            let next = page.as_ref().and_then(|(project, _)| more.then(|| project.id.clone()));
            let data = page
                .map(|(project, tasks)| -> Result<Value> {
                    let meta = task_meta(c, hub, space, &project.id)?;
                    let declarations = task_declarations(c, hub, space, &project.id)?;
                    Ok(json!({"project":project,"tasks":tasks,"meta":meta,"declarations":declarations}))
                })
                .transpose()?;
            Ok((json!({"version":1,"hubId":hub,"spaceId":r.space,"page":data,"next":next}), false))
        }
        "donnaTaskMetaSave" => {
            let r: TaskMetaSave = parse(body)?;
            super::scope(c, &r.hub_id, &r.space_id, hub)?;
            uuid(&r.task_id)?;
            if let Some(mut answer) = replay(c, &r.client_request_id, &payload, hub, space, extended_history)? {
                answer["meta"] = task_meta(c, hub, space, &r.project_id)?;
                return Ok((answer, false));
            }
            let (mut project, tasks) = load(c, hub, space, &r.project_id)?;
            ensure!(project.state == ProjectState::Active, "invalid_field: Projeto arquivado.");
            ensure!(
                project.revision == r.expected_revision,
                "projects_conflict: o Projeto mudou; releia antes de salvar."
            );
            let task = tasks
                .iter()
                .find(|t| t.id == r.task_id)
                .ok_or_else(|| anyhow::anyhow!("invalid_field: tarefa indisponível."))?;
            ensure!(r.meta.due_at.is_none_or(|at| at >= 0), "invalid_field: prazo inválido.");
            ensure!(
                r.meta.phases.len() <= 20 && (r.meta.phases.is_empty() || task.parent_task_id.is_none()),
                "invalid_field: fases inválidas."
            );
            ensure!(
                r.meta.current_phase.is_none_or(|index| index < r.meta.phases.len()),
                "invalid_field: fase atual inválida."
            );
            for phase in &r.meta.phases {
                text(phase, 120, true)?;
            }
            let now = crate::store::now_ms();
            c.execute("INSERT INTO donna_task_meta(hub_id,space_id,project_id,task_id,meta) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(hub_id,space_id,project_id,task_id) DO UPDATE SET meta=excluded.meta", params![hub,space,project.id,r.task_id,serde_json::to_string(&r.meta)?])?;
            project.revision =
                project.revision.checked_add(1).ok_or_else(|| anyhow::anyhow!("projects_limit: revisão esgotada."))?;
            project.updated_at = now;
            c.execute(
                "UPDATE donna_projects SET project=?4 WHERE hub_id=?1 AND space_id=?2 AND id=?3",
                params![hub, space, project.id, serde_json::to_string(&project)?],
            )?;
            history(
                c,
                (hub, space),
                &project.id,
                now,
                "task_meta_updated",
                Some(&r.task_id),
                "Alteração manual: prioridade, prazo ou fases.",
            )?;
            c.execute("INSERT INTO donna_project_receipts(hub_id,space_id,request_id,payload,project_id,revision) VALUES(?1,?2,?3,?4,?5,?6)", params![hub,space,r.client_request_id,payload,project.id,project.revision])?;
            Ok((
                json!({"snapshot":snapshot(c,hub,space,&project.id,extended_history)?,"meta":task_meta(c,hub,space,&project.id)?,"receipt":{"clientRequestId":r.client_request_id,"revision":project.revision}}),
                true,
            ))
        }
        "donnaProjects" => {
            let r: List = parse(body)?;
            super::scope(c, &r.hub_id, &r.space_id, hub)?;
            let mut st = c.prepare("SELECT project FROM donna_projects WHERE hub_id=?1 AND space_id=?2 ORDER BY id")?;
            let mut projects = Vec::new();
            let mut default_project_id = None;
            for row in st.query_map(params![hub, space], |r| r.get::<_, String>(0))? {
                let project: Project = serde_json::from_str(&row?)?;
                if project.state == ProjectState::Active && project.is_default && default_project_id.is_none() {
                    default_project_id = Some(project.id.clone());
                }
                projects.push(serde_json::to_value(project)?);
            }
            Ok((
                json!({"version":1,"hubId":hub,"spaceId":r.space_id,"projects":projects,"bots":bots(c, space)?,"defaultProjectId":default_project_id,"capabilities":["manualProjects","taskList","taskMeta","taskComments","defaultProject","taskStart","taskSchedule","taskAgenda","taskSessions"]}),
                false,
            ))
        }
        "donnaProject" => {
            let r: Read = parse(body)?;
            super::scope(c, &r.hub_id, &r.space_id, hub)?;
            Ok((snapshot(c, hub, space, &r.project, extended_history)?, false))
        }
        "donnaProjectCreate" | "donnaProjectSave" => {
            let now = crate::store::now_ms();
            let (project, tasks, request) = if method == "donnaProjectCreate" {
                let r: Create = parse(body)?;
                super::scope(c, &r.hub_id, &r.space_id, hub)?;
                if let Some(answer) = replay(c, &r.client_request_id, &payload, hub, space, extended_history)? {
                    return Ok((answer, false));
                }
                let count: i64 = c.query_row(
                    "SELECT COUNT(*) FROM donna_projects WHERE hub_id=?1 AND space_id=?2",
                    params![hub, space],
                    |r| r.get(0),
                )?;
                ensure!(count < 50, "projects_limit: máximo de 50 Projetos.");
                text(&r.name, 120, true)?;
                text(&r.objective, 2000, false)?;
                reference(r.coordinator_bot_id.as_deref(), None, &bots(c, space)?)?;
                let p = Project {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: r.name,
                    objective: r.objective,
                    coordinator_bot_id: r.coordinator_bot_id,
                    state: ProjectState::Active,
                    is_default: false,
                    revision: 1,
                    created_at: now,
                    updated_at: now,
                };

                (p, vec![], r.client_request_id)
            } else {
                let r: Save = parse(body)?;
                super::scope(c, &r.hub_id, &r.space_id, hub)?;
                if let Some(answer) = replay(c, &r.client_request_id, &payload, hub, space, extended_history)? {
                    return Ok((answer, false));
                }
                let (mut p, old) = load(c, hub, space, &r.project_id)?;
                ensure!(
                    p.revision == r.expected_revision,
                    "projects_conflict: o Projeto mudou; releia antes de salvar."
                );
                text(&r.name, 120, true)?;
                text(&r.objective, 2000, false)?;
                let roster = bots(c, space)?;
                reference(r.coordinator_bot_id.as_deref(), p.coordinator_bot_id.as_deref(), &roster)?;
                validate(&r.tasks, &old, &roster)?;
                if p.name != r.name
                    || p.objective != r.objective
                    || p.coordinator_bot_id != r.coordinator_bot_id
                    || p.state != r.state
                {
                    let mut fields = Vec::new();
                    if p.name != r.name {
                        fields.push("nome");
                    }
                    if p.objective != r.objective {
                        fields.push("objetivo");
                    }
                    if p.coordinator_bot_id != r.coordinator_bot_id {
                        fields.push("coordenador");
                    }
                    if p.state != r.state {
                        fields.push("estado");
                    }
                    history(
                        c,
                        (hub, space),
                        &p.id,
                        now,
                        "updated",
                        None,
                        &format!("Alteração manual: {}.", fields.join(", ")),
                    )?;
                }
                for task in &r.tasks {
                    let previous = old.iter().find(|t| t.id == task.id);
                    if previous.is_none() {
                        super::delegacao_meta::admit(c, (hub, space, &p.id), &task.id, now, None, None)?;
                    }
                    if previous != Some(task) {
                        let summary = if let Some(before) = previous {
                            let mut fields = Vec::new();
                            if before.title != task.title {
                                fields.push("título");
                            }
                            if before.description != task.description {
                                fields.push("descrição");
                            }
                            if before.acceptance != task.acceptance {
                                fields.push("aceite");
                            }
                            if before.evidence != task.evidence {
                                fields.push("evidência");
                            }
                            if before.parent_task_id != task.parent_task_id {
                                fields.push("pai");
                            }
                            if before.assignee_bot_id != task.assignee_bot_id {
                                fields.push("responsável");
                            }
                            if before.state != task.state {
                                fields.push("estado");
                            }
                            format!(
                                "Alteração manual: {}. Estado: {} → {}.",
                                fields.join(", "),
                                state_name(before.state),
                                state_name(task.state)
                            )
                        } else {
                            format!("Tarefa criada manualmente. Estado: {}.", state_name(task.state))
                        };
                        history(
                            c,
                            (hub, space),
                            &p.id,
                            now,
                            if previous.is_some() { "task_updated" } else { "task_created" },
                            Some(&task.id),
                            &summary,
                        )?;
                    }
                }
                // Declarações de conclusão por bot são fato gravado: sobrevivem a
                // saves que mantêm done→done, somem quando a tarefa sai de done,
                // quando some do Projeto ou quando o dono conclui (transição
                // para done = conferência humana).
                for task in &r.tasks {
                    let was_done = old.iter().any(|t| t.id == task.id && t.state == TaskState::Done);
                    if task.state != TaskState::Done || !was_done {
                        clear_task_declaration(c, hub, space, &p.id, &task.id)?;
                    }
                }
                for task in &old {
                    if !r.tasks.iter().any(|t| t.id == task.id) {
                        clear_task_declaration(c, hub, space, &p.id, &task.id)?;
                    }
                }
                p.name = r.name;
                p.objective = r.objective;
                p.coordinator_bot_id = r.coordinator_bot_id;
                p.state = r.state;
                p.revision += 1;
                p.updated_at = now;
                (p, r.tasks, r.client_request_id)
            };
            c.execute("INSERT INTO donna_projects(hub_id,space_id,id,project,tasks) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(hub_id,space_id,id) DO UPDATE SET project=excluded.project,tasks=excluded.tasks",params![hub,space,project.id,serde_json::to_string(&project)?,serde_json::to_string(&tasks)?])?;
            if project.revision == 1 {
                history(c, (hub, space), &project.id, now, "created", None, "Projeto criado manualmente.")?;
            }
            c.execute(
                "INSERT INTO donna_project_receipts(hub_id,space_id,request_id,payload,project_id,revision) VALUES(?1,?2,?3,?4,?5,?6)",
                params![hub, space, request, payload, project.id, project.revision],
            )?;
            Ok((
                json!({"snapshot":snapshot(c,hub,space,&project.id,extended_history)?,"receipt":{"clientRequestId":request,"revision":project.revision}}),
                true,
            ))
        }
        _ => anyhow::bail!("invalid_field: método indisponível."),
    }
}

fn state_name(state: TaskState) -> &'static str {
    match state {
        TaskState::Backlog => "A fazer",
        TaskState::InProgress => "Em andamento declarado pelo dono",
        TaskState::Review => "Em conferência",
        TaskState::Done => "Concluída manualmente",
        TaskState::Blocked => "Bloqueada",
        TaskState::Cancelled => "Cancelada",
    }
}
