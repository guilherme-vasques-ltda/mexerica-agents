//! Persistência e regras das propostas de subtarefas: a proposta pendente não
//! cria tarefa nem agenda; a decisão do dono cria as tarefas raiz, o histórico
//! do Projeto e as intenções de início — tudo numa transação só.
use crate::fork::tarefas::db::Outcome;
use crate::fork::{
    projetos::delegacao_meta,
    sessoes::{ExecutionRef, bindings},
};
use crate::{LockExt, hub::Hub, store::now_ms};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

const PENDING_LIMIT: i64 = 20;
const ITEMS_MAX: usize = 10;
const TASKS_LIMIT: usize = 100;
const PERSONAL: &str = crate::fork::spaces::PERSONAL;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct List {
    hub_id: String,
    space_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Decide {
    hub_id: String,
    space_id: String,
    proposal_id: String,
    expected_version: i64,
    client_request_id: String,
    decision: Decision,
    omit_item_ids: Option<Vec<String>>,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Decision {
    Approve,
    Reject,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ItemIn {
    title: String,
    description: Option<String>,
    acceptance: Option<String>,
    assignee_bot_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    id: String,
    title: String,
    description: String,
    acceptance: String,
    assignee_bot_id: Option<String>,
    included: bool,
    task_id: Option<String>,
}

struct Proposal {
    id: String,
    project_id: String,
    bot_id: String,
    title: String,
    items: Vec<Item>,
    state: String,
    version: i64,
    created_at: i64,
    decided_at: Option<i64>,
    decision_request_id: Option<String>,
}

/// Bot chamador: vínculo vivo do `ExecutionRef` + identidade na mesma transação.
struct Actor {
    id: String,
    space: String,
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
        "proposal_limit: texto vazio ou longo demais."
    );
    Ok(())
}

fn actor(c: &Connection, bot: &str, execution: &ExecutionRef) -> Result<Actor> {
    bindings::assert_execution_live(c, execution)?;
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
    Ok(Actor { id: bot.to_owned(), space })
}

/// Projeto alvo da proposta: explícito precisa existir e estar ativo; omitido
/// cai no Projeto padrão do Space, ou recusa sem um.
fn resolve_project(c: &Connection, hub: &str, space: &str, given: Option<&str>) -> Result<String> {
    if let Some(id) = given {
        uuid(id)?;
        let row: Option<String> = c
            .query_row(
                "SELECT project FROM donna_projects WHERE hub_id=?1 AND space_id=?2 AND id=?3",
                params![hub, space, id],
                |r| r.get(0),
            )
            .optional()?;
        let raw = row.ok_or_else(|| anyhow!("project_not_found: Projeto indisponível."))?;
        let project: Value = serde_json::from_str(&raw)?;
        ensure!(project["state"].as_str() == Some("active"), "project_archived: Projeto arquivado.");
        return Ok(id.to_owned());
    }
    let mut st = c.prepare("SELECT id,project FROM donna_projects WHERE hub_id=?1 AND space_id=?2 ORDER BY id")?;
    let rows = st.query_map(params![hub, space], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (id, raw) = row?;
        let project: Value = serde_json::from_str(&raw)?;
        if project["state"].as_str() == Some("active") && project["isDefault"].as_bool() == Some(true) {
            return Ok(id);
        }
    }
    Err(anyhow!("proposal_invalid: o Space não tem Projeto padrão ativo; informe projectId."))
}

/// Impressão canônica do gesto de propor: mesmo requestId com outro conteúdo é conflito.
fn canonical_payload(
    bot: &str,
    project: &str,
    title: &str,
    items: &[(String, String, String, Option<String>)],
) -> Result<String> {
    let list: Vec<Value> = items
        .iter()
        .map(|(title, description, acceptance, assignee)| {
            json!({"title": title, "description": description, "acceptance": acceptance, "assigneeBotId": assignee})
        })
        .collect();
    let raw =
        serde_json::to_vec(&json!({"method":"task_propose","bot":bot,"projectId":project,"title":title,"items":list}))?;
    Ok(Sha256::digest(raw).iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    }))
}

/// Ferramenta do bot `task_propose`: cria SÓ o registro pendente — nenhuma
/// tarefa, nenhuma agenda, nenhum envio. O `ExecutionRef` vivo é conferido na
/// mesma transação, e a idempotência é por `requestId`.
pub(crate) fn propose(hub: &Hub, bot: &str, execution: &ExecutionRef, args: &Value) -> Result<Outcome> {
    let mut c = hub.store.db.locked();
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let actor = actor(&tx, bot, execution)?;
    let title = args["title"].as_str().ok_or_else(|| anyhow!("invalid_field: title é obrigatório."))?;
    text(title, 120, true)?;
    let request_id = args["requestId"].as_str().ok_or_else(|| anyhow!("invalid_field: requestId é obrigatório."))?;
    uuid(request_id)?;
    let raw_items = args["items"].as_array().ok_or_else(|| anyhow!("invalid_field: items é obrigatório."))?;
    ensure!(!raw_items.is_empty() && raw_items.len() <= ITEMS_MAX, "proposal_limit: a proposta leva de 1 a 10 itens.");
    let roster = crate::fork::projetos::store::bots(&tx, &actor.space)?;
    let mut items: Vec<(String, String, String, Option<String>)> = Vec::new();
    for raw in raw_items {
        let item: ItemIn = serde_json::from_value(raw.clone()).map_err(|_| anyhow!("invalid_field: item inválido."))?;
        text(&item.title, 120, true)?;
        text(item.description.as_deref().unwrap_or(""), 2000, false)?;
        text(item.acceptance.as_deref().unwrap_or(""), 2000, false)?;
        if let Some(bot_id) = &item.assignee_bot_id {
            ensure!(
                roster.as_array().is_some_and(|list| list.iter().any(|b| b["id"].as_str() == Some(bot_id.as_str()))),
                "invalid_field: responsável indisponível."
            );
        }
        items.push((
            item.title,
            item.description.unwrap_or_default(),
            item.acceptance.unwrap_or_default(),
            item.assignee_bot_id,
        ));
    }
    let project_id = resolve_project(&tx, &hub.host_id, &actor.space, args["projectId"].as_str())?;
    let canonical = canonical_payload(&actor.id, &project_id, title, &items)?;
    let prior: Option<(String, String, String)> = tx
        .query_row(
            "SELECT project_id,title,items FROM donna_task_proposals WHERE hub_id=?1 AND space_id=?2 AND request_id=?3",
            params![hub.host_id, actor.space, request_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    if let Some((prior_project, prior_title, prior_items_raw)) = prior {
        let prior_items: Vec<Item> = serde_json::from_str(&prior_items_raw)?;
        let tuples: Vec<_> = prior_items
            .iter()
            .map(|i| (i.title.clone(), i.description.clone(), i.acceptance.clone(), i.assignee_bot_id.clone()))
            .collect();
        ensure!(
            canonical_payload(&actor.id, &prior_project, &prior_title, &tuples)? == canonical,
            "idempotency_conflict: pedido reutilizado com outro conteúdo."
        );
        let prior_id: String = tx
            .query_row(
                "SELECT id FROM donna_task_proposals WHERE hub_id=?1 AND space_id=?2 AND request_id=?3",
                params![hub.host_id, actor.space, request_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| anyhow!("proposal_not_found: proposta indisponível."))?;
        let proposal = load_proposal(&tx, &hub.host_id, &actor.space, &prior_id)?;
        let value = propose_answer(&tx, &hub.host_id, &actor.space, &proposal);
        tx.commit()?;
        return Ok(Outcome { value, changed: false, space: actor.space });
    }
    let pending: i64 = tx.query_row(
        "SELECT COUNT(*) FROM donna_task_proposals WHERE hub_id=?1 AND space_id=?2 AND state='pending'",
        params![hub.host_id, actor.space],
        |r| r.get(0),
    )?;
    ensure!(pending < PENDING_LIMIT, "proposal_limit: máximo de 20 propostas pendentes por Space.");
    let now = now_ms();
    let id = uuid::Uuid::new_v4().to_string();
    let stored: Vec<Item> = items
        .into_iter()
        .map(|(title, description, acceptance, assignee_bot_id)| Item {
            id: uuid::Uuid::new_v4().to_string(),
            title,
            description,
            acceptance,
            assignee_bot_id,
            included: true,
            task_id: None,
        })
        .collect();
    tx.execute(
        "INSERT INTO donna_task_proposals(hub_id,space_id,id,project_id,bot_id,request_id,title,items,state,version,created_at,decided_at,decision_request_id)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'pending',1,?9,NULL,NULL)",
        params![
            hub.host_id,
            actor.space,
            id,
            project_id,
            actor.id,
            request_id,
            title,
            serde_json::to_string(&stored)?,
            now
        ],
    )?;
    let proposal = Proposal {
        id,
        project_id,
        bot_id: actor.id.clone(),
        title: title.to_owned(),
        items: stored,
        state: "pending".into(),
        version: 1,
        created_at: now,
        decided_at: None,
        decision_request_id: None,
    };
    let value = propose_answer(&tx, &hub.host_id, &actor.space, &proposal);
    tx.commit()?;
    Ok(Outcome { value, changed: true, space: actor.space })
}

fn propose_answer(c: &Connection, hub: &str, space: &str, proposal: &Proposal) -> Value {
    let mut value = json!({"proposal": view(c, hub, space, proposal).unwrap_or(Value::Null)});
    value["message"] = json!(match proposal.state.as_str() {
        "approved" =>
            "Esta proposta já foi aprovada pelo dono: as tarefas existem e o início \
             segue a agenda, no máximo duas ao mesmo tempo no Space e uma por bot.",
        "rejected" => "Esta proposta foi recusada pelo dono: nenhuma tarefa foi criada.",
        _ =>
            "Proposta registrada e aguardando o ok do dono. Nada foi criado nem iniciado: \
             as subtarefas só surgem depois da aprovação, e aí começam sozinhas, \
             no máximo duas ao mesmo tempo no Space e uma por bot.",
    });
    value
}

fn proposal_of(row: &rusqlite::Row<'_>) -> rusqlite::Result<Proposal> {
    let items_raw: String = row.get(4)?;
    let items: Vec<Item> = serde_json::from_str(&items_raw)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?;
    Ok(Proposal {
        id: row.get(0)?,
        project_id: row.get(1)?,
        bot_id: row.get(2)?,
        title: row.get(3)?,
        items,
        state: row.get(5)?,
        version: row.get(6)?,
        created_at: row.get(7)?,
        decided_at: row.get(8)?,
        decision_request_id: row.get(9)?,
    })
}

fn load_proposal(c: &Connection, hub: &str, space: &str, id: &str) -> Result<Proposal> {
    uuid(id)?;
    c.query_row(
        "SELECT id,project_id,bot_id,title,items,state,version,created_at,decided_at,decision_request_id
         FROM donna_task_proposals WHERE hub_id=?1 AND space_id=?2 AND id=?3",
        params![hub, space, id],
        proposal_of,
    )
    .optional()?
    .ok_or_else(|| anyhow!("proposal_not_found: proposta indisponível."))
}

/// Visão de uma proposta para o dono: itens com o estado real da tarefa no
/// Projeto e o estado de início vindo da agenda.
fn view(c: &Connection, hub: &str, space: &str, p: &Proposal) -> Result<Value> {
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT project,tasks FROM donna_projects WHERE hub_id=?1 AND space_id=?2 AND id=?3",
            params![hub, space, p.project_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let mut project_name = Value::Null;
    let mut task_states: BTreeMap<String, String> = BTreeMap::new();
    if let Some((raw, tasks_raw)) = row {
        let project: Value = serde_json::from_str(&raw)?;
        if let Some(name) = project["name"].as_str() {
            project_name = json!(name);
        }
        for task in serde_json::from_str::<Vec<Value>>(&tasks_raw)? {
            if let (Some(id), Some(state)) = (task["id"].as_str(), task["state"].as_str()) {
                task_states.insert(id.to_owned(), state.to_owned());
            }
        }
    }
    let mut agenda_states: BTreeMap<String, String> = BTreeMap::new();
    {
        let mut st =
            c.prepare("SELECT task_id,state FROM donna_task_agenda WHERE hub_id=?1 AND space_id=?2 AND project_id=?3")?;
        let rows =
            st.query_map(params![hub, space, p.project_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (id, state) = row?;
            agenda_states.insert(id, state);
        }
    }
    let items: Vec<Value> = p
        .items
        .iter()
        .map(|i| {
            json!({
                "id": i.id, "title": i.title, "description": i.description,
                "acceptance": i.acceptance, "assigneeBotId": i.assignee_bot_id,
                "included": i.included, "taskId": i.task_id,
                "taskState": i.task_id.as_ref().and_then(|id| task_states.get(id)).cloned(),
                "startState": i.task_id.as_ref().and_then(|id| agenda_states.get(id)).cloned(),
            })
        })
        .collect();
    Ok(json!({
        "id": p.id, "version": p.version, "state": p.state, "botId": p.bot_id,
        "projectId": p.project_id, "projectName": project_name, "title": p.title,
        "createdAt": p.created_at, "decidedAt": p.decided_at, "items": items,
    }))
}

fn decide(c: &Connection, hub: &str, body: Value) -> Result<(Value, bool)> {
    let r: Decide =
        serde_json::from_value(body).map_err(|_| anyhow!("invalid_field: solicitação de decisão inválida."))?;
    super::scope(c, &r.hub_id, &r.space_id, hub)?;
    uuid(&r.proposal_id)?;
    uuid(&r.client_request_id)?;
    let space = r.space_id.as_str();
    let mut omit = BTreeSet::new();
    for id in r.omit_item_ids.clone().unwrap_or_default() {
        uuid(&id)?;
        omit.insert(id);
    }
    let p = load_proposal(c, hub, space, &r.proposal_id)?;
    let answer = |proposal: &Proposal| -> Result<Value> {
        Ok(json!({"version":1,"hubId":hub,"spaceId":space,"proposal":view(c, hub, space, proposal)?}))
    };
    // Decidida: repetição da mesma decisão devolve a visão atual; outra chave nunca refaz.
    if p.state != "pending" {
        if p.decision_request_id.as_deref() == Some(r.client_request_id.as_str()) {
            return Ok((answer(&p)?, false));
        }
        return Err(anyhow!("proposal_decided: a proposta já foi decidida; releia a lista."));
    }
    ensure!(p.version == r.expected_version, "proposal_conflict: a proposta mudou; releia antes de decidir.");
    let now = now_ms();
    let next = p.version.checked_add(1).ok_or_else(|| anyhow!("proposals_limit: versão esgotada."))?;
    if r.decision == Decision::Reject {
        c.execute(
            "UPDATE donna_task_proposals SET state='rejected',version=?4,decided_at=?5,decision_request_id=?6
             WHERE hub_id=?1 AND space_id=?2 AND id=?3",
            params![hub, space, p.id, next, now, r.client_request_id],
        )?;
        let fresh = load_proposal(c, hub, space, &p.id)?;
        return Ok((answer(&fresh)?, true));
    }
    let mut items = p.items.clone();
    for id in &omit {
        ensure!(items.iter().any(|i| &i.id == id), "proposal_invalid: item a omitir não existe na proposta.");
    }
    for item in &mut items {
        if omit.contains(&item.id) {
            item.included = false;
        }
    }
    let included = items.iter().filter(|i| i.included).count();
    ensure!(included > 0, "proposal_invalid: a decisão precisa manter ao menos um item.");
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT project,tasks FROM donna_projects WHERE hub_id=?1 AND space_id=?2 AND id=?3",
            params![hub, space, p.project_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (project_raw, tasks_raw) = row.ok_or_else(|| anyhow!("proposal_invalid: Projeto indisponível."))?;
    let mut project: Value = serde_json::from_str(&project_raw)?;
    ensure!(project["state"].as_str() == Some("active"), "proposal_invalid: Projeto arquivado.");
    let mut tasks: Vec<Value> = serde_json::from_str(&tasks_raw)?;
    ensure!(tasks.len() + included <= TASKS_LIMIT, "projects_limit: máximo de 100 tarefas.");
    let scope = (hub, space, p.project_id.as_str());
    delegacao_meta::require_capacity(c, scope, &tasks, None, included)?;
    let roster = crate::fork::projetos::store::bots(c, space)?;
    for item in items.iter().filter(|i| i.included) {
        if let Some(bot) = &item.assignee_bot_id {
            ensure!(
                roster.as_array().is_some_and(|list| list.iter().any(|b| b["id"].as_str() == Some(bot.as_str()))),
                "proposal_invalid: o item «{}» aponta um responsável fora do Space.",
                item.title
            );
        }
    }
    // Cria as tarefas raiz, o histórico do Projeto e as intenções de início,
    // tudo na mesma transação; o tick existente do runtime executa os inícios.
    for item in items.iter_mut().filter(|i| i.included) {
        let task_id = uuid::Uuid::new_v4().to_string();
        item.task_id = Some(task_id.clone());
        tasks.push(json!({
            "id": task_id, "parentTaskId": null, "assigneeBotId": item.assignee_bot_id,
            "title": item.title, "description": item.description, "acceptance": item.acceptance,
            "evidence": "", "state": "backlog",
        }));
        delegacao_meta::admit(c, scope, &task_id, now, Some(&p.bot_id))?;
        crate::fork::projetos::store::history_as(
            c,
            (hub, space, p.project_id.as_str()),
            now,
            "owner",
            "task_created",
            Some(task_id.as_str()),
            "Tarefa criada pela aprovação da proposta. Estado: A fazer.",
        )?;
        if let Some(bot) = &item.assignee_bot_id {
            c.execute(
                "INSERT INTO donna_task_agenda(hub_id,space_id,project_id,task_id,bot_id,start_at,not_before,state,session_id,nonce,attempts,last_error,request_id,proposal_id,created_at,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?6,'scheduled',NULL,?7,0,NULL,NULL,?8,?6,?6)",
                params![hub, space, p.project_id, task_id, bot, now, uuid::Uuid::new_v4().to_string(), p.id],
            )?;
        }
    }
    project["revision"] = json!(project["revision"].as_i64().unwrap_or(0) + 1);
    project["updatedAt"] = json!(now);
    c.execute(
        "UPDATE donna_projects SET project=?4,tasks=?5 WHERE hub_id=?1 AND space_id=?2 AND id=?3",
        params![hub, space, p.project_id, serde_json::to_string(&project)?, serde_json::to_string(&tasks)?],
    )?;
    c.execute(
        "UPDATE donna_task_proposals SET items=?4,state='approved',version=?5,decided_at=?6,decision_request_id=?7
         WHERE hub_id=?1 AND space_id=?2 AND id=?3",
        params![hub, space, p.id, serde_json::to_string(&items)?, next, now, r.client_request_id],
    )?;
    let fresh = load_proposal(c, hub, space, &p.id)?;
    Ok((answer(&fresh)?, true))
}

fn list(c: &Connection, hub: &str, body: Value) -> Result<(Value, bool)> {
    let r: List = serde_json::from_value(body).map_err(|_| anyhow!("invalid_field: solicitação inválida."))?;
    super::scope(c, &r.hub_id, &r.space_id, hub)?;
    let mut st = c.prepare(
        "SELECT id,project_id,bot_id,title,items,state,version,created_at,decided_at,decision_request_id
         FROM donna_task_proposals WHERE hub_id=?1 AND space_id=?2
           AND (state='pending' OR rowid IN (
             SELECT rowid FROM donna_task_proposals WHERE hub_id=?1 AND space_id=?2 AND state<>'pending'
             ORDER BY created_at DESC, rowid DESC LIMIT 50))
         ORDER BY created_at DESC, rowid DESC",
    )?;
    let rows = st.query_map(params![hub, r.space_id], proposal_of)?;
    let mut proposals = Vec::new();
    for row in rows {
        let proposal = row?;
        proposals.push(view(c, hub, &r.space_id, &proposal)?);
    }
    Ok((
        json!({"version":1,"hubId":hub,"spaceId":r.space_id,"limits":{"perBot":1,"total":2},"proposals":proposals}),
        false,
    ))
}

pub(super) fn call(c: &Connection, hub: &str, method: &str, body: Value) -> Result<(Value, bool)> {
    match method {
        "donnaTaskProposals" => list(c, hub, body),
        "donnaTaskProposalDecide" => decide(c, hub, body),
        _ => Err(anyhow!("invalid_field: método indisponível.")),
    }
}
