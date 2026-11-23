//! MX-142 fatia 3a: leitura e escrita de Tarefas (Projetos) por bots, sempre dentro
//! de uma transação que primeiro confere o `ExecutionRef` vivo. O JSON de
//! `donna_projects`/`donna_project_history` é tratado como opaco: campos que o dono
//! ou outras frentes adicionarem são preservados. Metadados de tarefa
//! (`donna_task_meta`) nunca são tocados aqui.
use crate::LockExt;
use crate::fork::projetos::delegacao_meta::{self, OriginKind};
use crate::fork::sessoes::{ExecutionRef, bindings};
use crate::hub::Hub;
use anyhow::{Context, Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

pub(crate) struct Outcome {
    pub value: Value,
    /// `true` quando `donna_projects` ou `donna_task_comments` mudou: o chamador emite `donnaProjectsChanged`.
    pub changed: bool,
    pub space: String,
}

const MAX_TASKS: usize = 100;
const MAX_DEPTH: usize = 3;
const MAX_MENTIONS: usize = 20;
const COMMENT_TEXT: usize = 4000;
const COMMENTS_PAGE: i64 = 20;
const PERSONAL: &str = crate::fork::spaces::PERSONAL;

struct Actor {
    id: String,
    space: String,
    /// `donna_bot_sessions.id` da execução viva: prova causal dos avisos.
    session: String,
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
        "task_limit: texto vazio ou longo demais."
    );
    Ok(())
}

/// Bot chamador: vínculo vivo do `ExecutionRef` + identidade do bot na mesma transação.
fn actor(c: &Connection, bot: &str, execution: &ExecutionRef) -> Result<Actor> {
    let session = bindings::assert_execution_live(c, execution)?;
    let binding_id = execution.acp_binding().ok_or_else(|| anyhow!("chamada vencida"))?;
    let binding = bindings::resolve(c, binding_id, bot)?;
    ensure!(binding.bot == bot, "chamada vencida: vínculo de outro bot");
    let config: Option<String> =
        c.query_row("SELECT config FROM bots WHERE id=?1 AND deleted=0", [bot], |r| r.get(0)).optional()?;
    let config: Value = config
        .map(|raw| serde_json::from_str(&raw))
        .transpose()?
        .ok_or_else(|| anyhow!("bot_not_found: bot indisponível."))?;
    let space = config["spaceId"].as_str().unwrap_or(PERSONAL).to_owned();
    ensure!(
        config["hidden"].as_bool() != Some(true) && config["kind"].as_str() != Some("group"),
        "bot_not_found: bot indisponível."
    );
    Ok(Actor { id: bot.to_owned(), space, session: session.id })
}

/// Roster do Space no formato mínimo (id, nome): bots visíveis, sem grupo nem ocultos.
fn roster(c: &Connection, space: &str) -> Result<Vec<(String, String)>> {
    let mut st = c.prepare("SELECT config FROM bots WHERE deleted=0")?;
    let rows = st.query_map([], |r| r.get::<_, String>(0))?;
    let mut result = Vec::new();
    for row in rows {
        let config: Value = serde_json::from_str(&row?)?;
        let id = config["id"].as_str().unwrap_or_default();
        if id.is_empty()
            || config["spaceId"].as_str().unwrap_or(PERSONAL) != space
            || config["hidden"].as_bool() == Some(true)
            || config["kind"].as_str() == Some("group")
        {
            continue;
        }
        result.push((id.to_owned(), config["name"].as_str().unwrap_or(id).to_owned()));
    }
    Ok(result)
}

pub(crate) fn reports_to(c: &Connection, space: &str, bot: &str) -> Result<Option<String>> {
    let v: Option<Option<String>> = c
        .query_row(
            "SELECT reports_to_bot_id FROM donna_bot_org WHERE space_id=?1 AND bot_id=?2",
            params![space, bot],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?;
    Ok(v.flatten())
}

struct Loaded {
    project: Value,
    tasks: Vec<Value>,
}

fn load(c: &Connection, hub: &str, space: &str, project_id: &str, active_only: bool) -> Result<Loaded> {
    uuid(project_id)?;
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT project,tasks FROM donna_projects WHERE hub_id=?1 AND space_id=?2 AND id=?3",
            params![hub, space, project_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (project, tasks) = row.ok_or_else(|| anyhow!("project_not_found: Projeto indisponível."))?;
    let project: Value = serde_json::from_str(&project)?;
    ensure!(!active_only || project["state"].as_str() != Some("archived"), "project_archived: Projeto arquivado.");
    let tasks: Vec<Value> = serde_json::from_str(&tasks)?;
    Ok(Loaded { project, tasks })
}

fn field<'a>(task: &'a Value, key: &str) -> &'a str {
    task[key].as_str().unwrap_or("")
}

fn find<'a>(tasks: &'a [Value], id: &str) -> Option<&'a Value> {
    tasks.iter().find(|t| field(t, "id") == id)
}

/// Número de ancestrais (níveis acima). A validação do Projeto exige self+ancestrais ≤ 3,
/// ou seja, no máximo `MAX_DEPTH - 1` ancestrais; ciclos são limitados por `MAX_TASKS`.
fn depth_of(tasks: &[Value], id: &str) -> usize {
    let mut depth = 0;
    let mut cursor = find(tasks, id).and_then(|t| t["parentTaskId"].as_str());
    for _ in 0..MAX_TASKS {
        let Some(parent) = cursor else { break };
        depth += 1;
        cursor = find(tasks, parent).and_then(|t| t["parentTaskId"].as_str());
    }
    depth
}

fn root_of(tasks: &[Value], id: &str) -> String {
    let mut current = id.to_owned();
    for _ in 0..MAX_TASKS {
        let Some(parent) = find(tasks, &current).and_then(|t| t["parentTaskId"].as_str().map(str::to_owned)) else {
            return current;
        };
        current = parent;
    }
    current
}

/// `full`: dona da subtarefa, coordenador do Projeto ou supervisor direto do responsável.
/// `summary`: mesma árvore de plano de uma subtarefa do bot (títulos e estados apenas).
struct Access {
    full: bool,
    summary: bool,
}

fn access(
    c: &Connection,
    space: &str,
    actor: &Actor,
    coordinator: Option<&str>,
    tasks: &[Value],
    task: &Value,
) -> Result<Access> {
    let assignee = task["assigneeBotId"].as_str();
    let mut full = coordinator == Some(actor.id.as_str()) || assignee == Some(actor.id.as_str());
    if !full && let Some(assignee) = assignee {
        full = reports_to(c, space, assignee)?.as_deref() == Some(actor.id.as_str());
    }
    let summary = !full
        && tasks.iter().any(|t| {
            field(t, "assigneeBotId") == actor.id && root_of(tasks, field(t, "id")) == root_of(tasks, field(task, "id"))
        });
    Ok(Access { full, summary })
}

fn project_view(project: &Value) -> Value {
    json!({
        "id": project["id"], "name": project["name"], "objective": project["objective"],
        "coordinatorBotId": project["coordinatorBotId"], "state": project["state"],
        "revision": project["revision"],
    })
}

fn task_view(task: &Value, full: bool) -> Value {
    let mut view = json!({
        "id": task["id"], "parentTaskId": task["parentTaskId"], "assigneeBotId": task["assigneeBotId"],
        "title": task["title"], "state": task["state"],
    });
    if full {
        view["description"] = task["description"].clone();
        view["acceptance"] = task["acceptance"].clone();
        view["evidence"] = task["evidence"].clone();
    }
    view
}

#[allow(
    clippy::too_many_arguments,
    reason = "Coords+autor+ação+resumo são uma única entrada de histórico; agrupar criaria struct descartável."
)]
fn history(
    c: &Connection,
    hub: &str,
    space: &str,
    project_id: &str,
    at: i64,
    author: &str,
    action: &str,
    task: &str,
    summary: &str,
) -> Result<()> {
    let entry = json!({
        "id": uuid::Uuid::new_v4().to_string(), "at": at, "author": author,
        "action": action, "summary": summary, "taskId": task,
    });
    c.execute(
        "INSERT INTO donna_project_history(hub_id,space_id,project_id,entry) VALUES(?1,?2,?3,?4)",
        params![hub, space, project_id, serde_json::to_string(&entry)?],
    )?;
    Ok(())
}

/// Grava `project`/`tasks` com revision e updatedAt incrementados, na mesma transação.
fn write_project(c: &Connection, hub: &str, space: &str, loaded: &mut Loaded, now: i64) -> Result<()> {
    loaded.project["revision"] = json!(loaded.project["revision"].as_i64().unwrap_or(0) + 1);
    loaded.project["updatedAt"] = json!(now);
    c.execute(
        "UPDATE donna_projects SET project=?4,tasks=?5 WHERE hub_id=?1 AND space_id=?2 AND id=?3",
        params![
            hub,
            space,
            field(&loaded.project, "id"),
            serde_json::to_string(&loaded.project)?,
            serde_json::to_string(&loaded.tasks)?
        ],
    )?;
    Ok(())
}

fn state_name(state: &str) -> &str {
    match state {
        "backlog" => "A fazer",
        "in_progress" => "Em andamento",
        "review" => "Em conferência",
        "done" => "Concluída",
        "blocked" => "Bloqueada",
        _ => "outro estado",
    }
}

fn require_full(
    c: &Connection,
    space: &str,
    actor: &Actor,
    coordinator: Option<&str>,
    tasks: &[Value],
    task: &Value,
) -> Result<()> {
    let access = access(c, space, actor, coordinator, tasks, task)?;
    ensure!(access.full, "task_forbidden: sem autoridade sobre esta subtarefa.");
    Ok(())
}

fn arg<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args[key].as_str().filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("invalid_field: {key} é obrigatório."))
}

pub(crate) fn read(hub: &Hub, bot: &str, execution: &ExecutionRef, args: &Value) -> Result<Outcome> {
    let c = hub.store.db.locked();
    let tx = c.unchecked_transaction()?;
    let actor = actor(&tx, bot, execution)?;
    let project_id = arg(args, "projectId")?;
    let loaded = load(&tx, &hub.host_id, &actor.space, project_id, false)?;
    let coordinator = loaded.project["coordinatorBotId"].as_str();
    let mut out = json!({"project": project_view(&loaded.project), "tasks": []});
    if let Some(task_id) = args["taskId"].as_str() {
        let task = find(&loaded.tasks, task_id).ok_or_else(|| anyhow!("task_not_found: subtarefa indisponível."))?;
        let access = access(&tx, &actor.space, &actor, coordinator, &loaded.tasks, task)?;
        ensure!(access.full || access.summary, "task_forbidden: subtarefa fora do seu acesso.");
        let mut view = task_view(task, access.full);
        if access.full {
            view["comments"] = comments(&tx, &hub.host_id, &actor.space, project_id, task_id, &actor.id)?;
        }
        out["task"] = view;
        return Ok(Outcome { value: out, changed: false, space: actor.space });
    }
    let mut tasks = Vec::new();
    for task in &loaded.tasks {
        let access = access(&tx, &actor.space, &actor, coordinator, &loaded.tasks, task)?;
        if access.full || access.summary {
            tasks.push(task_view(task, access.full));
        }
    }
    out["tasks"] = json!(tasks);
    Ok(Outcome { value: out, changed: false, space: actor.space })
}

/// Comentários visíveis ao bot: os que ele mesmo escreveu e os endereçados a
/// ele por menção (sem `onlyNote`). Comentário do dono sem @ ou só-anotar
/// nunca sai — filtrado em silêncio, sem eco nem contagem do que foi oculto.
fn comments(c: &Connection, hub: &str, space: &str, project_id: &str, task_id: &str, bot: &str) -> Result<Value> {
    let mut st = c.prepare(
        "SELECT entry FROM donna_task_comments WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 ORDER BY seq DESC LIMIT ?5",
    )?;
    let rows = st.query_map(params![hub, space, project_id, task_id, COMMENTS_PAGE], |r| r.get::<_, String>(0))?;
    let own = format!("bot:{bot}");
    let mut entries = Vec::new();
    for row in rows {
        let entry: Value = serde_json::from_str(&row?)?;
        // Menção só vale com a prova anti-forja gravada na escrita
        // (`mentionCheck==2`, mesma régua do outbox): entrada legada sem a
        // marca não expõe o corpo a ninguém, inclusive ao citado.
        let mention_check = entry["mentionCheck"].as_i64() == Some(2);
        let mentioned = mention_check
            && entry["mentions"].as_array().is_some_and(|list| list.iter().any(|m| m.as_str() == Some(bot)));
        let only_note = entry["onlyNote"].as_bool().unwrap_or(false);
        if entry["author"].as_str() == Some(own.as_str()) || (mentioned && !only_note) {
            entries.push(entry);
        }
    }
    entries.reverse();
    Ok(json!(entries))
}

pub(crate) fn update(hub: &Hub, bot: &str, execution: &ExecutionRef, args: &Value) -> Result<Outcome> {
    let mut c = hub.store.db.locked();
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let actor = actor(&tx, bot, execution)?;
    let project_id = arg(args, "projectId")?;
    let task_id = arg(args, "taskId")?;
    let mut loaded = load(&tx, &hub.host_id, &actor.space, project_id, true)?;
    let Some(pos) = loaded.tasks.iter().position(|t| field(t, "id") == task_id) else {
        return Err(anyhow!("task_not_found: subtarefa indisponível."));
    };
    require_full(
        &tx,
        &actor.space,
        &actor,
        loaded.project["coordinatorBotId"].as_str(),
        &loaded.tasks,
        &loaded.tasks[pos],
    )?;
    let current_state = field(&loaded.tasks[pos], "state");
    let new_state = args["state"].as_str();
    let changes_assignment = args.get("assigneeBotId").is_some_and(|id| id != &loaded.tasks[pos]["assigneeBotId"]);
    if changes_assignment
        || new_state == Some("cancelled")
        || (matches!(current_state, "in_progress" | "review") && matches!(new_state, Some("backlog" | "blocked")))
    {
        let origin = delegacao_meta::origin(&tx, (&hub.host_id, &actor.space, project_id), task_id)
            .context("direct_owner_order_protected: origem da tarefa indisponível.")?;
        ensure!(origin != OriginKind::OwnerDirect, "direct_owner_order_protected: só o dono pode alterar esta ordem.");
    }
    ensure!(!changes_assignment, "task_forbidden: apenas o usuário pode reatribuir uma tarefa.");
    ensure!(field(&loaded.tasks[pos], "state") != "cancelled", "task_forbidden: subtarefa cancelada.");
    // Trava de aceite humano: tarefa concluída só o usuário altera ou reabre —
    // nenhum bot (responsável, supervisor, coordenador) escreve nela.
    ensure!(
        field(&loaded.tasks[pos], "state") != "done",
        "task_accepted: tarefa concluída só pode ser alterada ou reaberta pelo usuário."
    );
    // A prova do trabalho é do usuário: qualquer envio de `evidence`, mesmo
    // nulo, é recusado antes de tocar a tarefa.
    ensure!(
        !args.as_object().is_some_and(|o| o.contains_key("evidence")),
        "task_forbidden: apenas o usuário pode registrar ou alterar a prova de uma tarefa."
    );
    // Concluir também é gesto do usuário: bot só declara estados abertos e
    // entrega o trabalho em `review` para conferência.
    let state = match args.get("state") {
        Some(state) if !state.is_null() => {
            let state = state.as_str().ok_or_else(|| anyhow!("invalid_field: estado de bot não permitido."))?;
            ensure!(
                state != "done",
                "task_forbidden: apenas o usuário pode concluir uma tarefa; mova para review para enviar à conferência."
            );
            ensure!(
                matches!(state, "backlog" | "in_progress" | "review" | "blocked"),
                "invalid_field: estado de bot não permitido."
            );
            Some(state.to_owned())
        }
        _ => None,
    };
    let description = args["description"].as_str().map(str::to_owned);
    if let Some(d) = &description {
        text(d, 4000, false)?;
    }
    let mut changed_fields = Vec::new();
    {
        let task = &mut loaded.tasks[pos];
        if let Some(state) = state
            && field(task, "state") != state
        {
            changed_fields.push(format!("estado {} → {}", state_name(field(task, "state")), state_name(&state)));
            task["state"] = json!(state);
        }
        if let Some(description) = description
            && field(task, "description") != description
        {
            changed_fields.push("descrição".to_owned());
            task["description"] = json!(description);
        }
    }
    if changed_fields.is_empty() {
        return Ok(Outcome {
            value: json!({"project": project_view(&loaded.project), "task": task_view(&loaded.tasks[pos], true)}),
            changed: false,
            space: actor.space,
        });
    }
    let now = crate::store::now_ms();
    history(
        &tx,
        &hub.host_id,
        &actor.space,
        project_id,
        now,
        &format!("bot:{}", actor.id),
        "task_updated",
        task_id,
        &format!("Bot alterou a subtarefa: {}.", changed_fields.join(", ")),
    )?;
    write_project(&tx, &hub.host_id, &actor.space, &mut loaded, now)?;
    tx.commit()?;
    Ok(Outcome {
        value: json!({"project": project_view(&loaded.project), "task": task_view(&loaded.tasks[pos], true)}),
        changed: true,
        space: actor.space,
    })
}

/// Turno da entry-poke que disparou esta execução, quando há prova causal:
/// a admissão viva do bot precisa ter o MESMO `execution_token` do
/// `ExecutionRef` e a entry do aviso (`clientNonce` do comentário) precisa
/// estar entre as `entry_ids` admitidas do turno. `None` = sem prova.
fn reply_turn(c: &Connection, execution: &ExecutionRef, bot: &str, comment_id: &str) -> Result<Option<i64>> {
    let Some(dispatch) = crate::fork::bot_admission::current_on(c, bot)? else { return Ok(None) };
    if dispatch.execution_token != execution.turn_token() {
        return Ok(None);
    }
    let nonce = format!("mx142-task-aviso-{comment_id}-{bot}");
    let poke = c
        .query_row(
            "SELECT id,turn FROM entries WHERE bot_id=?1 AND kind='user' AND json_valid(data) AND json_extract(data,'$.clientNonce')=?2",
            params![bot, nonce],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )
        .optional()?;
    let Some((entry_id, turn)) = poke else { return Ok(None) };
    Ok(dispatch.entry_ids.iter().any(|id| id == &entry_id).then_some(turn))
}

pub(crate) fn comment(hub: &Hub, bot: &str, execution: &ExecutionRef, args: &Value) -> Result<Outcome> {
    let mut c = hub.store.db.locked();
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let actor = actor(&tx, bot, execution)?;
    let project_id = arg(args, "projectId")?;
    let task_id = arg(args, "taskId")?;
    let body = arg(args, "text")?;
    text(body, COMMENT_TEXT, true)?;
    let loaded = load(&tx, &hub.host_id, &actor.space, project_id, true)?;
    let task = find(&loaded.tasks, task_id).ok_or_else(|| anyhow!("task_not_found: subtarefa indisponível."))?;
    require_full(&tx, &actor.space, &actor, loaded.project["coordinatorBotId"].as_str(), &loaded.tasks, task)?;
    let roster = roster(&tx, &actor.space)?;
    let mut mentions: Vec<String> = Vec::new();
    if let Some(list) = args["mentions"].as_array() {
        for item in list {
            let id = item.as_str().ok_or_else(|| anyhow!("invalid_field: mentions deve ser uma lista de botId."))?;
            ensure!(roster.iter().any(|(rid, _)| rid == id), "invalid_field: menção fora do Space.");
            if !mentions.iter().any(|m| m == id) {
                mentions.push(id.to_owned());
            }
        }
    }
    ensure!(mentions.len() <= MAX_MENTIONS, "task_limit: máximo de 20 menções.");
    // Menção verificada na gravação: toda menção declarada precisa ter `@Nome`
    // real no texto — lista divergente é recusada para o bot corrigir, nunca
    // persistida como evidência de audiência ou de aviso.
    for id in &mentions {
        let name = roster.iter().find(|(rid, _)| rid == id).map(|(_, name)| name.as_str()).unwrap_or_default();
        ensure!(
            crate::fork::mentions::text_mentions(body, name),
            "invalid_field: menção sem @ correspondente no texto."
        );
    }
    let request_id = match args["requestId"].as_str() {
        Some(id) => {
            uuid(id)?;
            format!("bot:{}:{}", actor.id, id)
        }
        None => format!("bot:{}:{}", actor.id, uuid::Uuid::new_v4()),
    };
    let canonical = json!({"method":"task_comment","bot":actor.id,"projectId":project_id,"taskId":task_id,"text":body,"mentions":mentions});
    let payload = Sha256::digest(serde_json::to_vec(&canonical)?).iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    });
    let prior: Option<(String, String)> = tx
        .query_row(
            "SELECT comment_id,payload FROM donna_task_comments WHERE hub_id=?1 AND space_id=?2 AND request_id=?3",
            params![hub.host_id, actor.space, request_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((comment_id, previous)) = prior {
        ensure!(previous == payload, "task_idempotency: pedido reutilizado com outro conteúdo.");
        let entry: Value = serde_json::from_str(
            &tx.query_row(
                "SELECT entry FROM donna_task_comments WHERE hub_id=?1 AND space_id=?2 AND comment_id=?3",
                params![hub.host_id, actor.space, comment_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| anyhow!("task_not_found: comentário indisponível."))?,
        )?;
        tx.commit()?;
        return Ok(Outcome { value: json!({"comment": entry}), changed: false, space: actor.space });
    }
    let comment_id = uuid::Uuid::new_v4().to_string();
    let entry = json!({
        "id": comment_id, "at": crate::store::now_ms(), "author": format!("bot:{}", actor.id),
        "text": body, "mentions": mentions,
        "onlyNote": false, "mentionCheck": 2,
        "notify": {"botIds": mentions, "state": "not_notified", "reason": "bot_comment"},
    });
    tx.execute(
        "INSERT INTO donna_task_comments(hub_id,space_id,project_id,task_id,comment_id,request_id,payload,entry) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            hub.host_id,
            actor.space,
            project_id,
            task_id,
            comment_id,
            request_id,
            payload,
            serde_json::to_string(&entry)?
        ],
    )?;
    // Resposta a aviso do dono NA MESMA transação: `replyToCommentId` só vale
    // com prova causal entry↔turno (o poke do comentário está entre as
    // entradas admitidas desta execução). Sem prova, a entrega fica `sent`.
    let mut answered = Value::Null;
    if let Some(reply) = args["replyToCommentId"].as_str() {
        let proven = reply_turn(&tx, execution, &actor.id, reply)?;
        answered = json!(match proven {
            Some(turn) => crate::fork::tarefas_runtime::avisos::mark_answered_tx(
                &tx,
                &hub.host_id,
                &actor.space,
                project_id,
                task_id,
                reply,
                &actor.id,
                &actor.session,
                turn,
            )?,
            None => false,
        });
    }
    tx.commit()?;
    let mut value = json!({"comment": entry});
    if !answered.is_null() {
        value["answered"] = answered;
    }
    Ok(Outcome { value, changed: true, space: actor.space })
}

pub(crate) fn create_child(hub: &Hub, bot: &str, execution: &ExecutionRef, args: &Value) -> Result<Outcome> {
    let mut c = hub.store.db.locked();
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let actor = actor(&tx, bot, execution)?;
    let project_id = arg(args, "projectId")?;
    let parent_id = arg(args, "parentTaskId")?;
    let title = arg(args, "title")?;
    text(title, 200, true)?;
    let description = args["description"].as_str().unwrap_or("");
    let acceptance = args["acceptance"].as_str().unwrap_or("");
    text(description, 4000, false)?;
    text(acceptance, 2000, false)?;
    let mut loaded = load(&tx, &hub.host_id, &actor.space, project_id, true)?;
    let Some(pos) = loaded.tasks.iter().position(|t| field(t, "id") == parent_id) else {
        return Err(anyhow!("task_not_found: subtarefa indisponível."));
    };
    require_full(
        &tx,
        &actor.space,
        &actor,
        loaded.project["coordinatorBotId"].as_str(),
        &loaded.tasks,
        &loaded.tasks[pos],
    )?;
    // Idempotência do gesto inteiro: mesmo requestId + mesmo payload devolve a
    // subtarefa já criada, sem duplicar — retry após resposta perdida é seguro
    // (inclusive com start, pois o clientRequestId do início deriva dele).
    // start=true exige requestId: sem um id que o chamador possa repetir, um
    // retry após resposta perdida criaria a filha de novo antes do início.
    ensure!(
        args["start"].as_bool() != Some(true) || args["requestId"].is_string(),
        "invalid_field: start=true exige requestId para a repetição idempotente do gesto inteiro."
    );
    let request_id = match args["requestId"].as_str() {
        Some(id) => {
            uuid(id)?;
            format!("bot:{}:{}", actor.id, id)
        }
        None => String::new(),
    };
    let mut request_payload = String::new();
    if !request_id.is_empty() {
        let canonical = json!({
            "method": "task_create_child", "bot": actor.id, "projectId": project_id,
            "parentTaskId": parent_id, "title": title, "description": description,
            "acceptance": acceptance, "assigneeBotId": args["assigneeBotId"], "start": args["start"],
        });
        request_payload = Sha256::digest(serde_json::to_vec(&canonical)?).iter().fold(String::new(), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        });
        let prior: Option<(String, String, String)> = tx
            .query_row(
                "SELECT payload,project_id,task_id FROM donna_task_requests WHERE hub_id=?1 AND space_id=?2 AND request_id=?3",
                params![hub.host_id, actor.space, request_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((previous, prior_project, prior_task)) = prior {
            ensure!(previous == request_payload, "task_idempotency: pedido reutilizado com outro conteúdo.");
            let loaded = load(&tx, &hub.host_id, &actor.space, &prior_project, true)?;
            let task = loaded
                .tasks
                .iter()
                .find(|t| field(t, "id") == prior_task)
                .cloned()
                .ok_or_else(|| anyhow!("task_not_found: subtarefa indisponível."))?;
            // O replay devolve a tarefa como ela está AGORA: revalida a
            // audiência na mesma transação (mesma régua do task_read) para
            // não vazar descrição/evidência se a filha foi reatribuída ou o
            // acesso caiu desde a criação.
            let coordinator = loaded.project["coordinatorBotId"].as_str();
            let access = access(&tx, &actor.space, &actor, coordinator, &loaded.tasks, &task)?;
            ensure!(access.full || access.summary, "task_forbidden: subtarefa fora do seu acesso.");
            tx.commit()?;
            return Ok(Outcome {
                value: json!({"project": project_view(&loaded.project), "task": task_view(&task, access.full)}),
                changed: false,
                space: actor.space,
            });
        }
    }
    ensure!(field(&loaded.tasks[pos], "state") != "cancelled", "task_forbidden: subtarefa cancelada.");
    let scope = (hub.host_id.as_str(), actor.space.as_str(), project_id);
    delegacao_meta::require_capacity(&tx, scope, &loaded.tasks, Some(parent_id), 1)?;
    // Mesma régua do validate() do Projeto: self + ancestrais ≤ MAX_DEPTH.
    let child_depth = depth_of(&loaded.tasks, parent_id) + 1;
    ensure!(child_depth < MAX_DEPTH, "task_limit: máximo de três níveis.");
    ensure!(loaded.tasks.len() < MAX_TASKS, "task_limit: máximo de 100 tarefas.");
    let assignee = args["assigneeBotId"].as_str().map(str::to_owned);
    if let Some(assignee) = &assignee {
        ensure!(
            roster(&tx, &actor.space)?.iter().any(|(id, _)| id == assignee),
            "invalid_field: responsável indisponível."
        );
    }
    let now = crate::store::now_ms();
    let id = uuid::Uuid::new_v4().to_string();
    let task = json!({
        "id": id, "parentTaskId": parent_id, "assigneeBotId": assignee,
        "title": title, "description": description, "acceptance": acceptance,
        "evidence": "", "state": "backlog",
    });
    loaded.tasks.push(task.clone());
    let author = format!("bot:{}", actor.id);
    let summary = match &assignee {
        Some(a) => format!("Bot criou a subtarefa \"{title}\" atribuída a {a}."),
        None => format!("Bot criou a subtarefa \"{title}\"."),
    };
    history(&tx, &hub.host_id, &actor.space, project_id, now, &author, "task_created", &id, &summary)?;
    write_project(&tx, &hub.host_id, &actor.space, &mut loaded, now)?;
    delegacao_meta::admit(&tx, scope, &id, now, Some(&actor.id), Some(parent_id))?;
    if !request_id.is_empty() {
        tx.execute(
            "INSERT INTO donna_task_requests(hub_id,space_id,request_id,payload,project_id,task_id) VALUES(?1,?2,?3,?4,?5,?6)",
            params![hub.host_id, actor.space, request_id, request_payload, project_id, id],
        )?;
    }
    tx.commit()?;
    Ok(Outcome {
        value: json!({"project": project_view(&loaded.project), "task": task_view(&task, true)}),
        changed: true,
        space: actor.space,
    })
}
