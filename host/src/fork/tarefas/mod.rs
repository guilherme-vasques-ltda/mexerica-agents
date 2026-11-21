//! MX-142 fatia 3a: ferramentas de bot para a área de Tarefas. Toda chamada exige um
//! `ExecutionRef` vivo conferido na mesma transação da gravação; a autoridade é a do
//! bot, nunca a do dono que o acionou. Comentários de bot nunca avisam ninguém.
mod db;
#[cfg(test)]
mod tests;

use crate::LockExt;
use crate::fork::sessoes::{ExecutionRef, bindings};
use crate::fork::spaces::tool_context::ToolContext;
use crate::hub::Hub;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) const INSTRUCTIONS: &str = "Tarefas do Space: leia e atualize só o que a sua autoridade cobre. \
Use task_read para ver o Projeto e as subtarefas visíveis a você (a sua completa; as demais só títulos e estados). \
Use task_update para mudar o estado de uma subtarefa sua entre backlog, in_progress, review e blocked, e para atualizar a descrição — ao terminar o trabalho mova para review (Em conferência). Só o usuário pode concluir (done), reabrir uma tarefa concluída ou registrar e alterar a prova (evidence). \
Coordenador do Projeto e supervisor direto do responsável também atualizam subtarefas de outros bots. \
Use task_comment para registrar texto na subtarefa (nunca avisa outro bot); se estiver respondendo a um aviso do dono, passe replyToCommentId com o id do comentário para marcá-lo como respondido. \
Use task_create_child para criar uma subtarefa filha de uma tarefa sobre a qual você tem autoridade plena, \
para você ou para outro bot do mesmo Space; os limites são 100 tarefas e 3 níveis. \
Atribuir não inicia trabalho por si só; para criar já iniciando use task_create_child com start=true (requestId obrigatório).";

/// DDL idêntica à de `projetos::migrate` (dona do schema): `IF NOT EXISTS` torna a
/// ordem de migração irrelevante enquanto as frentes não integram.
pub(crate) fn migrate(c: &rusqlite::Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_task_comments(
        seq INTEGER PRIMARY KEY AUTOINCREMENT, hub_id TEXT NOT NULL, space_id TEXT NOT NULL,
        project_id TEXT NOT NULL, task_id TEXT NOT NULL, comment_id TEXT NOT NULL,
        request_id TEXT NOT NULL, payload TEXT NOT NULL, entry TEXT NOT NULL,
        UNIQUE(hub_id,space_id,request_id),
        FOREIGN KEY(hub_id,space_id,project_id) REFERENCES donna_projects(hub_id,space_id,id));
        CREATE INDEX IF NOT EXISTS donna_task_comments_project
        ON donna_task_comments(hub_id,space_id,project_id,task_id,seq);
        CREATE TABLE IF NOT EXISTS donna_task_requests(
        hub_id TEXT NOT NULL, space_id TEXT NOT NULL, request_id TEXT NOT NULL,
        payload TEXT NOT NULL, project_id TEXT NOT NULL, task_id TEXT NOT NULL,
        UNIQUE(hub_id,space_id,request_id));",
    )?;
    Ok(())
}

pub(crate) fn tools() -> Value {
    json!([
        {
            "name": "task_read",
            "description": "Lê um Projeto de Tarefas do seu Space. Sem taskId devolve o Projeto e as subtarefas visíveis a você: as suas completas, as da mesma árvore de plano só com título e estado. Com taskId devolve a subtarefa no seu nível de acesso e, se for sua/coordenada/supervisionada, os últimos comentários. Projeto de outro Space ou subtarefa fora do seu acesso são recusados.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "projectId": {"type": "string", "description": "ID do Projeto (Tarefa)."},
                    "taskId": {"type": "string", "description": "ID da subtarefa; omita para listar as visíveis."},
                },
                "required": ["projectId"],
                "additionalProperties": false,
            },
            "annotations": {"readOnlyHint": true, "idempotentHint": true},
        },
        {
            "name": "task_update",
            "description": "Atualiza estado e descrição de uma subtarefa sobre a qual você tem autoridade plena: a sua, a de um subordinado direto (Reporta a) ou qualquer uma se você for o coordenador do Projeto. O estado só anda entre backlog, in_progress, review e blocked — ao terminar o trabalho mova para review (Em conferência); cancelled não é estado de bot. Concluir (done), reabrir tarefa concluída e registrar ou alterar a prova (evidence) são gestos só do usuário. Toda mudança fica no histórico da tarefa com o seu nome de bot.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "projectId": {"type": "string"},
                    "taskId": {"type": "string"},
                    "state": {"type": "string", "enum": ["backlog", "in_progress", "review", "blocked"], "description": "Novo estado declarado; ao terminar use review."},
                    "description": {"type": "string", "description": "Nota de progresso na subtarefa."},
                },
                "required": ["projectId", "taskId"],
                "additionalProperties": false,
            },
            "annotations": {"readOnlyHint": false, "idempotentHint": false},
        },
        {
            "name": "task_comment",
            "description": "Registra um comentário numa subtarefa sobre a qual você tem autoridade plena. O comentário entra no mesmo fio que o dono vê, assinado como bot:<seu id>, e NUNCA avisa nem acorda outro bot — menções aqui são só registro. Para pedir algo a outro bot use ask_bot/message_bot.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "projectId": {"type": "string"},
                    "taskId": {"type": "string"},
                    "text": {"type": "string", "description": "Texto do comentário (até 4000 caracteres)."},
                    "mentions": {"type": "array", "items": {"type": "string"}, "description": "botIds do Space citados (só registro, sem aviso)."},
                    "replyToCommentId": {"type": "string", "description": "ID de um comentário do dono que este comentário responde; marca o aviso como respondido."},
                    "requestId": {"type": "string", "description": "UUID opcional para repetição idempotente do mesmo comentário."},
                },
                "required": ["projectId", "taskId", "text"],
                "additionalProperties": false,
            },
            "annotations": {"readOnlyHint": false, "idempotentHint": false},
        },
        {
            "name": "task_create_child",
            "description": "Cria uma subtarefa filha de uma tarefa sobre a qual você tem autoridade plena, atribuída a você ou a outro bot visível do mesmo Space. Limites: 100 tarefas e 3 níveis por Projeto. Atribuir não inicia trabalho por si só; com start=true a subtarefa já é criada iniciada (requestId obrigatório para repetição idempotente).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "projectId": {"type": "string"},
                    "parentTaskId": {"type": "string", "description": "Tarefa-pai da nova subtarefa."},
                    "title": {"type": "string", "description": "Título (até 200 caracteres)."},
                    "description": {"type": "string"},
                    "acceptance": {"type": "string", "description": "Critério de aceite (até 2000 caracteres)."},
                    "assigneeBotId": {"type": "string", "description": "botId responsável; omita para tarefa sem responsável."},
                    "start": {"type": "boolean", "description": "true cria e já inicia a subtarefa; o resultado do início volta em start.state (started/already_running/refused/uncertain/failed)."},
                    "requestId": {"type": "string", "description": "UUID opcional para repetição idempotente; OBRIGATÓRIO quando start=true — repita o mesmo requestId para refazer o gesto sem duplicar."},
                },
                "required": ["projectId", "parentTaskId", "title"],
                "additionalProperties": false,
            },
            "annotations": {"readOnlyHint": false, "idempotentHint": false},
        }
    ])
}

pub(crate) async fn call_in(
    hub: &Arc<Hub>,
    from: &str,
    name: &str,
    args: &Value,
    context: Option<&ToolContext>,
) -> Result<Value> {
    call_bound(hub, from, name, args, context, context.and_then(|c| c.turn.execution.as_ref())).await
}

pub(crate) async fn call_in_execution(
    hub: &Arc<Hub>,
    from: &str,
    name: &str,
    args: &Value,
    execution: Option<&ExecutionRef>,
) -> Result<Value> {
    call_bound(hub, from, name, args, None, execution).await
}

async fn call_bound(
    hub: &Arc<Hub>,
    from: &str,
    name: &str,
    args: &Value,
    context: Option<&ToolContext>,
    execution: Option<&ExecutionRef>,
) -> Result<Value> {
    bindings::recheck_optional(hub, execution)?;
    if let Some(context) = context {
        context.check(hub)?;
    }
    let execution = execution.ok_or_else(|| anyhow!("chamada vencida: execução ausente"))?;
    let bot = context.map_or_else(|| from.to_owned(), |c| c.cfg.id.clone());
    // Contexto da tarefa corrente: sessão vinculada da execução preenche os
    // argumentos omitidos. A autoridade continua sendo conferida na transação.
    let mut args = args.clone();
    if let Some((hub_id, _space, project, task)) = session_coords(hub, execution).await
        && hub_id == hub.host_id
    {
        if args["projectId"].is_null() {
            args["projectId"] = json!(project);
        }
        let target = match name {
            "task_read" | "task_update" | "task_comment" => "taskId",
            "task_create_child" => "parentTaskId",
            _ => "",
        };
        if !target.is_empty() && args[target].is_null() {
            args[target] = json!(task);
        }
    }
    let args = &args;
    let write = matches!(name, "task_update" | "task_comment" | "task_create_child");
    let _guard = if write { Some(hub.store.team_lock.lock().await) } else { None };
    let mut outcome = match name {
        "task_read" => db::read(hub, &bot, execution, args)?,
        "task_update" => db::update(hub, &bot, execution, args)?,
        "task_comment" => db::comment(hub, &bot, execution, args)?,
        "task_create_child" => db::create_child(hub, &bot, execution, args)?,
        _ => anyhow::bail!("unknown task tool: {name}"),
    };
    // MX-142: "criar e já iniciar" — o start é do runtime, com clientRequestId
    // próprio; o resultado parcial fica honesto no retorno da tool.
    if name == "task_create_child"
        && args["start"].as_bool() == Some(true)
        && let Some(task_id) = outcome.value["task"]["id"].as_str().map(str::to_owned)
    {
        let project_id = outcome.value["project"]["id"]
            .as_str()
            .or_else(|| args["projectId"].as_str())
            .unwrap_or_default()
            .to_owned();
        // clientRequestId derivado do requestId da criação: replay do gesto
        // inteiro bate no recibo do runtime em vez de iniciar de novo.
        let request = match args["requestId"].as_str() {
            Some(id) => format!("bot:{bot}:{id}"),
            None => uuid::Uuid::new_v4().to_string(),
        };
        match crate::fork::tarefas_runtime::start_task_in_execution(
            hub,
            execution,
            &outcome.space,
            &project_id,
            &task_id,
            &request,
        )
        .await
        {
            Ok(start) => outcome.value["start"] = start["start"].clone(),
            Err(error) => {
                let msg = format!("{error:#}");
                let refused =
                    msg.starts_with("forbidden") || msg.starts_with("invalid_field") || msg.starts_with("idempotency");
                outcome.value["start"] = json!({
                    "state": if refused { "refused" } else { "uncertain" },
                    "reason": msg.chars().take(300).collect::<String>(),
                });
            }
        }
    }
    if outcome.changed {
        let _ = hub
            .events
            .send(json!({"type": crate::fork::projetos::EVENT, "hubId": hub.host_id, "spaceId": outcome.space}));
    }
    Ok(outcome.value)
}

/// Coordenadas da tarefa vinculada à sessão viva da execução, quando houver.
/// Só preenche argumentos omitidos; a autoridade é reconferida na transação.
async fn session_coords(hub: &Arc<Hub>, execution: &ExecutionRef) -> Option<(String, String, String, String)> {
    let session = {
        let hub = hub.clone();
        let execution = execution.clone();
        tokio::task::spawn_blocking(move || -> Result<String> {
            let c = hub.store.db.locked();
            Ok(bindings::assert_execution_live(&c, &execution)?.id)
        })
        .await
        .ok()?
        .ok()?
    };
    crate::fork::tarefas_runtime::session_task(hub, &session).await.ok().flatten()
}
