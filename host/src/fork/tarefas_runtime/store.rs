//! Persistência do runtime de tarefas: agenda de início, recibos idempotentes,
//! sessões vinculadas e entregas de avisos. Tudo escopado por hub+Space.
use crate::{LockExt, fork::sessoes, hub::Hub, store::now_ms};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) const MAX_ATTEMPTS: i64 = 5;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Coords {
    pub hub_id: String,
    pub space_id: String,
    pub project_id: String,
    pub task_id: String,
    pub client_request_id: Option<String>,
    pub start_at: Option<i64>,
}

pub(crate) struct TaskRow {
    pub project: Value,
    pub task: Value,
}

/// Lê projeto+tarefa direto de `donna_projects` (o JSON já é a fonte de verdade
/// mantida pela frente de dados). Falha fechada em qualquer divergência.
pub(crate) fn load_task(c: &Connection, hub: &str, space: &str, project: &str, task: &str) -> Result<TaskRow> {
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT project,tasks FROM donna_projects WHERE hub_id=?1 AND space_id=?2 AND id=?3",
            params![hub, space, project],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (project_raw, tasks_raw) = row.context("invalid_field: projeto indisponível.")?;
    let project: Value = serde_json::from_str(&project_raw)?;
    ensure!(project["state"].as_str() == Some("active"), "invalid_field: projeto arquivado.");
    let tasks: Vec<Value> = serde_json::from_str(&tasks_raw)?;
    let task = tasks
        .iter()
        .find(|t| t["id"].as_str() == Some(task))
        .cloned()
        .context("invalid_field: tarefa indisponível.")?;
    Ok(TaskRow { project, task })
}

/// O hubId do corpo precisa ser o hub real da conexão — UUID válido mas
/// divergente é recusado (antes era só validado e ignorado).
fn check_hub(received: &str, hub: &str) -> Result<()> {
    ensure!(received == hub, "forbidden: hub divergente.");
    Ok(())
}

fn check_uuid(v: &str) -> Result<()> {
    uuid::Uuid::parse_str(v).map(|_| ()).map_err(|_| anyhow::anyhow!("invalid_field: identificador inválido."))
}

fn parse_coords(body: &Value, need_request: bool) -> Result<Coords> {
    let r: Coords =
        serde_json::from_value(body.clone()).map_err(|_| anyhow::anyhow!("invalid_field: solicitação inválida."))?;
    check_uuid(&r.hub_id)?;
    check_uuid(&r.space_id)?;
    check_uuid(&r.project_id)?;
    check_uuid(&r.task_id)?;
    if need_request {
        check_uuid(r.client_request_id.as_deref().context("invalid_field: clientRequestId obrigatório.")?)?;
    }
    ensure!(r.start_at.is_none_or(|at| at >= 0), "invalid_field: data de início inválida.");
    Ok(r)
}

fn replay(c: &Connection, request: &str, payload: &str, hub: &str, space: &str) -> Result<Option<Value>> {
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT payload,result FROM donna_task_receipts WHERE hub_id=?1 AND space_id=?2 AND request_id=?3",
            params![hub, space, request],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        Some((saved, result)) if saved == payload => Ok(Some(serde_json::from_str(&result)?)),
        Some(_) => Err(anyhow::anyhow!("idempotency_conflict: clientRequestId reutilizado com payload diferente.")),
        None => Ok(None),
    }
}

fn receipt(c: &Connection, request: &str, payload: &str, result: &Value, hub: &str, space: &str) -> Result<()> {
    c.execute(
        "INSERT INTO donna_task_receipts(hub_id,space_id,request_id,payload,result) VALUES(?1,?2,?3,?4,?5)",
        params![hub, space, request, payload, serde_json::to_string(result)?],
    )?;
    Ok(())
}

/// Bots visíveis do Space (mesma regra de `projetos::store::bots`).
fn roster(c: &Connection, space: &str) -> Result<Vec<(String, String)>> {
    let mut st =
        c.prepare("SELECT id,config FROM bots WHERE deleted=0 AND json_extract(config,'$.spaceId')=?1 ORDER BY id")?;
    let rows = st.query_map(params![space], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (id, raw) = row?;
        let cfg: Value = serde_json::from_str(&raw)?;
        if cfg["hidden"].as_bool() == Some(true) || cfg["group"].as_bool() == Some(true) {
            continue;
        }
        out.push((id, cfg["name"].as_str().unwrap_or_default().to_owned()));
    }
    Ok(out)
}

pub(crate) struct Agenda {
    pub bot_id: String,
    pub start_at: i64,
    pub state: String,
    pub session_id: Option<String>,
    pub attempts: i64,
    pub nonce: String,
    pub last_error: Option<String>,
    /// requestId D220 do kickoff delegado (só início via bot); `None` no
    /// caminho do dono. É a ponta tarefa→delegação para auditoria.
    pub request_id: Option<String>,
}

pub(crate) fn agenda_get(c: &Connection, hub: &str, space: &str, project: &str, task: &str) -> Result<Option<Agenda>> {
    c.query_row(
        "SELECT bot_id,start_at,state,session_id,attempts,nonce,last_error,request_id FROM donna_task_agenda
         WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4",
        params![hub, space, project, task],
        |r| {
            Ok(Agenda {
                bot_id: r.get(0)?,
                start_at: r.get(1)?,
                state: r.get(2)?,
                session_id: r.get(3)?,
                attempts: r.get(4)?,
                nonce: r.get(5)?,
                last_error: r.get(6)?,
                request_id: r.get(7)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn agenda_public(a: &Agenda) -> Value {
    json!({
        "state": a.state,
        "startAt": a.start_at,
        "botId": a.bot_id,
        "sessionId": a.session_id,
        "attempts": a.attempts,
        "lastError": a.last_error,
        "requestId": a.request_id,
    })
}

/// Garante a sessão vinculada da tarefa para o bot responsável. Sessão fechada
/// é substituída por uma nova (o vínculo é por tarefa+bot, não por sessão).
pub(crate) fn ensure_session(
    c: &Connection,
    hub: &str,
    space: &str,
    project: &str,
    task: &str,
    bot: &str,
    title: &str,
) -> Result<String> {
    let existing: Option<String> = c
        .query_row(
            "SELECT session_id FROM donna_task_sessions WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND bot_id=?5",
            params![hub, space, project, task, bot],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = existing
        && matches!(sessoes::store::get(c, &id), Ok(Some(s)) if s.state == "open")
    {
        return Ok(id);
    }
    let title: String = title.chars().take(sessoes::store::TITLE_MAX).collect();
    let session = sessoes::store::create(
        c,
        &sessoes::store::NewSession {
            chat: bot.to_owned(),
            origin: sessoes::origin::SessionOrigin::Task { task_id: task.to_owned() },
            source_session: None,
            parent_session: None,
            title,
            runs_on: None,
        },
    )?;
    let id = session.id.clone();
    c.execute(
        "INSERT INTO donna_task_sessions(hub_id,space_id,project_id,task_id,bot_id,session_id,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(hub_id,space_id,project_id,task_id,bot_id) DO UPDATE SET session_id=excluded.session_id,created_at=excluded.created_at",
        params![hub, space, project, task, bot, id, now_ms()],
    )?;
    Ok(id)
}

pub(crate) fn task_of_session(c: &Connection, session_id: &str) -> Result<Option<(String, String, String, String)>> {
    c.query_row(
        "SELECT hub_id,space_id,project_id,task_id FROM donna_task_sessions WHERE session_id=?1",
        params![session_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn save_agenda(
    c: &Connection,
    hub: &str,
    space: &str,
    project: &str,
    task: &str,
    a: &Agenda,
    not_before: i64,
) -> Result<()> {
    c.execute(
        "INSERT INTO donna_task_agenda(hub_id,space_id,project_id,task_id,bot_id,start_at,not_before,state,session_id,nonce,attempts,last_error,request_id,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
         ON CONFLICT(hub_id,space_id,project_id,task_id) DO UPDATE SET
         bot_id=excluded.bot_id,start_at=excluded.start_at,not_before=excluded.not_before,state=excluded.state,
         session_id=excluded.session_id,nonce=excluded.nonce,attempts=excluded.attempts,last_error=excluded.last_error,
         request_id=excluded.request_id,updated_at=excluded.updated_at",
        params![hub, space, project, task, a.bot_id, a.start_at, not_before, a.state, a.session_id, a.nonce, a.attempts, a.last_error, a.request_id, now_ms(), now_ms()],
    )?;
    Ok(())
}

/// `donnaTaskSchedule`: grava/limpa o início futuro. Não exige responsável —
/// a tarefa pode ser atribuída depois; o gate roda de novo no disparo.
fn schedule(c: &Connection, hub: &str, r: &Coords) -> Result<Value> {
    let space = &r.space_id;
    let project = &r.project_id;
    let task = &r.task_id;
    let request = r.client_request_id.clone().unwrap_or_default();
    match r.start_at {
        None => {
            c.execute(
                "DELETE FROM donna_task_agenda WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4",
                params![hub, space, project, task],
            )?;
            Ok(json!({"receipt": {"clientRequestId": request}, "agenda": Value::Null}))
        }
        Some(at) => {
            let row = load_task(c, hub, space, project, task)?;
            let existing = agenda_get(c, hub, space, project, task)?;
            let (prev_bot, prev_session) = match existing {
                Some(a) => (Some(a.bot_id), a.session_id),
                None => (None, None),
            };
            // Reagendamento reconcilia o alvo com o responsável ATUAL
            // (TEC-25): se a tarefa trocou de assignee, o novo claim é do
            // novo bot — a sessão vinculada era do bot anterior e não é
            // reutilizada. A linha antiga em donna_task_sessions permanece
            // (auditoria por bot_id).
            let bot_id = row.task["assigneeBotId"].as_str().map(str::to_owned).or(prev_bot.clone()).unwrap_or_default();
            let session_id = if prev_bot.as_deref() == Some(bot_id.as_str()) { prev_session } else { None };
            let agenda = Agenda {
                bot_id,
                start_at: at,
                state: "scheduled".into(),
                session_id,
                attempts: 0,
                // Nonce novo a cada reagendamento: é o fencing token do claim —
                // um fire velho em voo não conclui sobre a agenda nova.
                nonce: uuid::Uuid::new_v4().to_string(),
                last_error: None,
                request_id: None,
            };
            save_agenda(c, hub, space, project, task, &agenda, at)?;
            Ok(json!({
                "receipt": {"clientRequestId": request},
                "agenda": agenda_public(&agenda),
            }))
        }
    }
}

pub(crate) fn call(c: &Connection, hub: &str, method: &str, body: Value) -> Result<(Value, bool)> {
    match method {
        "donnaTaskSchedule" => {
            let r = parse_coords(&body, true)?;
            check_hub(&r.hub_id, hub)?;
            let request = r.client_request_id.clone().context("invalid_field: clientRequestId ausente.")?;
            let payload = serde_json::to_string(&body)?;
            super::scope(c, hub, &r.space_id, hub)?;
            check_uuid(&r.space_id)?;
            if let Some(answer) = replay(c, &request, &payload, hub, &r.space_id)? {
                return Ok((answer, false));
            }
            let result = schedule(c, hub, &r)?;
            receipt(c, &request, &payload, &result, hub, &r.space_id)?;
            Ok((result, true))
        }
        "donnaTaskAgenda" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Read {
                hub_id: String,
                space_id: String,
            }
            let r: Read =
                serde_json::from_value(body).map_err(|_| anyhow::anyhow!("invalid_field: solicitação inválida."))?;
            check_uuid(&r.hub_id)?;
            check_hub(&r.hub_id, hub)?;
            check_uuid(&r.space_id)?;
            super::scope(c, hub, &r.space_id, hub)?;
            let mut st = c.prepare(
                "SELECT project_id,task_id,bot_id,start_at,state,session_id,attempts,last_error FROM donna_task_agenda
                 WHERE hub_id=?1 AND space_id=?2 ORDER BY start_at",
            )?;
            let rows = st.query_map(params![hub, r.space_id], |row| {
                Ok(json!({
                    "projectId": row.get::<_, String>(0)?,
                    "taskId": row.get::<_, String>(1)?,
                    "botId": row.get::<_, String>(2)?,
                    "startAt": row.get::<_, i64>(3)?,
                    "state": row.get::<_, String>(4)?,
                    "sessionId": row.get::<_, Option<String>>(5)?,
                    "attempts": row.get::<_, i64>(6)?,
                    "lastError": row.get::<_, Option<String>>(7)?,
                }))
            })?;
            let agendas: Vec<Value> = rows.collect::<std::result::Result<_, _>>()?;
            Ok((json!({"version":1,"hubId":hub,"spaceId":r.space_id,"agendas":agendas}), false))
        }
        "donnaTaskSessions" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            #[allow(
                clippy::struct_field_names,
                reason = "Coords da tarefa: todos terminam em Id por contrato camelCase."
            )]
            struct Read {
                hub_id: String,
                space_id: String,
                project_id: String,
                task_id: String,
            }
            let r: Read =
                serde_json::from_value(body).map_err(|_| anyhow::anyhow!("invalid_field: solicitação inválida."))?;
            check_uuid(&r.hub_id)?;
            check_hub(&r.hub_id, hub)?;
            check_uuid(&r.space_id)?;
            check_uuid(&r.project_id)?;
            check_uuid(&r.task_id)?;
            super::scope(c, hub, &r.space_id, hub)?;
            let _ = load_task(c, hub, &r.space_id, &r.project_id, &r.task_id)?;
            let mut st = c.prepare(
                "SELECT ts.bot_id,ts.session_id,b.deleted,b.config FROM donna_task_sessions ts
                 LEFT JOIN bots b ON b.id=ts.bot_id
                 WHERE ts.hub_id=?1 AND ts.space_id=?2 AND ts.project_id=?3 AND ts.task_id=?4 ORDER BY ts.created_at",
            )?;
            let rows = st.query_map(params![hub, r.space_id, r.project_id, r.task_id], |row| {
                let bot_id: String = row.get(0)?;
                let session_id: String = row.get(1)?;
                let deleted: Option<i64> = row.get(2)?;
                let cfg_raw: Option<String> = row.get(3)?;
                Ok((bot_id, session_id, deleted, cfg_raw))
            })?;
            let mut sessions = Vec::new();
            for row in rows {
                let (bot_id, session_id, deleted, cfg_raw) = row?;
                let cfg = cfg_raw.and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
                // Fail-closed (SEG custódia 72d9fdb3): o nome só aparece se o bot
                // está vivo, visível e ainda pertence a ESTE space — bot oculto,
                // removido ou movido de Space não pode vazar o nome pela listagem.
                let name_visible = deleted == Some(0)
                    && cfg.as_ref().is_some_and(|cfg| {
                        cfg["hidden"].as_bool() != Some(true)
                            && cfg["kind"].as_str() != Some("group")
                            && cfg["spaceId"].as_str().unwrap_or(crate::fork::spaces::PERSONAL) == r.space_id
                    });
                let title = if name_visible {
                    cfg.as_ref()
                        .and_then(|cfg| cfg["name"].as_str().map(str::to_owned))
                        .unwrap_or_else(|| bot_id.clone())
                } else {
                    bot_id.clone()
                };
                let state = match sessoes::store::get(c, &session_id) {
                    Ok(Some(s)) if s.state == "open" => "idle",
                    Ok(Some(_)) => "done",
                    _ => "not_opened",
                };
                sessions.push(json!({
                    "botId": bot_id,
                    "sessionId": session_id,
                    "title": title,
                    "state": state,
                    "parentSessionId": Value::Null,
                }));
            }
            let agenda = agenda_get(c, hub, &r.space_id, &r.project_id, &r.task_id)?;
            Ok((
                json!({
                    "version":1,"hubId":hub,"spaceId":r.space_id,
                    "projectId":r.project_id,"taskId":r.task_id,
                    "sessions":sessions,
                    "agenda": agenda.as_ref().map(agenda_public),
                }),
                false,
            ))
        }
        _ => Err(anyhow::anyhow!("method_not_found: {method}")),
    }
}

/// Valida coordenadas+projeto+tarefa+responsável e devolve o que o disparo
/// precisa. `refused` já é resposta final; `fire` segue para sessão+poke.
pub(crate) enum Gate {
    Refused(String),
    Fire { bot_id: String, title: String, project_name: String },
}

pub(crate) fn gate(c: &Connection, hub: &str, space: &str, project: &str, task: &str) -> Result<Gate> {
    let row = load_task(c, hub, space, project, task)?;
    let project_name = row.project["name"].as_str().unwrap_or_default().to_owned();
    let title = row.task["title"].as_str().unwrap_or_default().to_owned();
    // Tarefa encerrada/cancelada nunca dispara início — nem por dono nem por bot.
    if matches!(row.task["state"].as_str(), Some("done" | "cancelled")) {
        return Ok(Gate::Refused("task_closed".into()));
    }
    let Some(bot_id) = row.task["assigneeBotId"].as_str().map(str::to_owned) else {
        return Ok(Gate::Refused("no_assignee".into()));
    };
    let roster = roster(c, space)?;
    if !roster.iter().any(|(id, _)| id == &bot_id) {
        return Ok(Gate::Refused("bot_unavailable".into()));
    }
    Ok(Gate::Fire { bot_id, title, project_name })
}

/// `*` -> `answered` SOMENTE com prova causal entry↔turno: a execução que
/// comenta (`session_id` + `message_turn` do `ToolContext`) precisa ter consumido
/// a entry 'user' cujo `clientNonce` é o da entrega (`mx142-task-aviso-<c>-<b>`)
/// NAQUELA sessão. A sessão da entrega é evidência da PRÓPRIA entry
/// (`e.session`), não da coluna `deliveries.session_id` — que pode estar NULL
/// numa janela anterior ao envio ou em linhas legadas. A prova independe do
/// status gravado: resposta antes da marca 'sent', durante 'sending' ou após
/// retry também é ack válido. Sem entry/nonce/turn casando -> false.
#[allow(
    clippy::too_many_arguments,
    reason = "Prova causal carrega coords+comentário+bot+sessão+turno; agrupar criaria struct descartável."
)]
pub(crate) fn mark_answered_in_session(
    c: &Connection,
    hub: &str,
    space: &str,
    project: &str,
    task: &str,
    comment_id: &str,
    bot_id: &str,
    session_id: &str,
    message_turn: i64,
) -> Result<bool> {
    let changed = c.execute(
        "UPDATE donna_task_deliveries SET status='answered',answered_at=?9
         WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 AND task_id=?4 AND comment_id=?5 AND bot_id=?6
         AND status != 'answered'
         AND EXISTS(SELECT 1 FROM entries e
             WHERE e.bot_id=?6 AND e.session=?7 AND e.turn=?8 AND e.kind='user'
               AND json_extract(e.data,'$.clientNonce')='mx142-task-aviso-'||?5||'-'||?6)",
        params![hub, space, project, task, comment_id, bot_id, session_id, message_turn, now_ms()],
    )?;
    Ok(changed > 0)
}

/// `session_id` → coordenadas da tarefa vinculada (fallback de contexto para
/// as tools do executor A).
pub(crate) async fn session_task(hub: &Arc<Hub>, session_id: &str) -> Result<Option<(String, String, String, String)>> {
    let hub = hub.clone();
    let session_id = session_id.to_owned();
    tokio::task::spawn_blocking(move || {
        let db = hub.store.db.locked();
        task_of_session(&db, &session_id)
    })
    .await?
}
