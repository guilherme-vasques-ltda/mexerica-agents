//! D541/D543: registros manuais do dono, sem runtime, ferramentas ou autoridade para bots.
pub(crate) mod delegacao_meta;
pub(crate) mod model;
pub(crate) mod store;
#[cfg(test)]
mod tests;

use crate::{
    api::devices::{Caller, Forbidden},
    fork::autoridade::Autoridade,
    hub::Hub,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) const METHODS: &[&str] = &[
    "donnaProjects",
    "donnaProject",
    "donnaProjectCreate",
    "donnaProjectSave",
    "donnaProjectEnsureDefault",
    "donnaTasks",
    "donnaTaskMetaSave",
    "donnaTaskComments",
    "donnaTaskComment",
];
pub(crate) const LOCAL_MANUAL_METHODS: &[&str] = &[
    "donnaProjects",
    "donnaProject",
    "donnaProjectCreate",
    "donnaProjectSave",
    "donnaProjectEnsureDefault",
    "donnaTasks",
    "donnaTaskMetaSave",
];
pub(crate) const EVENT: &str = "donnaProjectsChanged";

pub(crate) fn migrate(c: &rusqlite::Connection) -> Result<()> {
    let tx = c.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_projects(
        hub_id TEXT NOT NULL, space_id TEXT NOT NULL, id TEXT NOT NULL,
        project TEXT NOT NULL, tasks TEXT NOT NULL, PRIMARY KEY(hub_id,space_id,id));
        CREATE TABLE IF NOT EXISTS donna_project_history(
        seq INTEGER PRIMARY KEY AUTOINCREMENT, hub_id TEXT NOT NULL, space_id TEXT NOT NULL,
        project_id TEXT NOT NULL, entry TEXT NOT NULL,
        FOREIGN KEY(hub_id,space_id,project_id) REFERENCES donna_projects(hub_id,space_id,id));
        CREATE INDEX IF NOT EXISTS donna_project_history_project
        ON donna_project_history(hub_id,space_id,project_id,seq);
        CREATE TABLE IF NOT EXISTS donna_task_meta(
        hub_id TEXT NOT NULL, space_id TEXT NOT NULL, project_id TEXT NOT NULL,
        task_id TEXT NOT NULL, meta TEXT NOT NULL,
        PRIMARY KEY(hub_id,space_id,project_id,task_id),
        FOREIGN KEY(hub_id,space_id,project_id) REFERENCES donna_projects(hub_id,space_id,id));
        CREATE TABLE IF NOT EXISTS donna_task_declarations(
        hub_id TEXT NOT NULL, space_id TEXT NOT NULL, project_id TEXT NOT NULL,
        task_id TEXT NOT NULL, bot_id TEXT NOT NULL, at INTEGER NOT NULL,
        PRIMARY KEY(hub_id,space_id,project_id,task_id),
        FOREIGN KEY(hub_id,space_id,project_id) REFERENCES donna_projects(hub_id,space_id,id));
        CREATE TABLE IF NOT EXISTS donna_task_comments(
        seq INTEGER PRIMARY KEY AUTOINCREMENT, hub_id TEXT NOT NULL, space_id TEXT NOT NULL,
        project_id TEXT NOT NULL, task_id TEXT NOT NULL, comment_id TEXT NOT NULL,
        request_id TEXT NOT NULL, payload TEXT NOT NULL, entry TEXT NOT NULL,
        UNIQUE(hub_id,space_id,request_id),
        FOREIGN KEY(hub_id,space_id,project_id) REFERENCES donna_projects(hub_id,space_id,id));
        CREATE INDEX IF NOT EXISTS donna_task_comments_project
        ON donna_task_comments(hub_id,space_id,project_id,task_id,seq);
        CREATE TABLE IF NOT EXISTS donna_project_receipts(
        hub_id TEXT NOT NULL, space_id TEXT NOT NULL, request_id TEXT NOT NULL,
        payload TEXT NOT NULL, project_id TEXT NOT NULL, revision INTEGER NOT NULL,
        PRIMARY KEY(hub_id,space_id,request_id),
        FOREIGN KEY(hub_id,space_id,project_id) REFERENCES donna_projects(hub_id,space_id,id));",
    )?;
    delegacao_meta::migrate(&tx)?;
    tx.commit()?;
    Ok(())
}

fn scope(c: &rusqlite::Connection, hub: &str, space: &str, expected_hub: &str) -> Result<()> {
    if hub != expected_hub {
        return Err(Forbidden.into());
    }
    crate::fork::spaces::require_space(c, space).map_err(|_| Forbidden)?;
    Ok(())
}

#[derive(Clone)]
struct CommentCoords {
    space: String,
    project: String,
    task: String,
}

fn comment_coords(body: &Value, method: &str) -> Option<CommentCoords> {
    if !matches!(method, "donnaTaskComment" | "donnaTaskComments") {
        return None;
    }
    Some(CommentCoords {
        space: body["spaceId"].as_str().unwrap_or_default().to_owned(),
        project: body["projectId"].as_str().unwrap_or_default().to_owned(),
        task: body["taskId"].as_str().unwrap_or_default().to_owned(),
    })
}

struct PendingDelivery {
    coords: CommentCoords,
    comment_id: String,
    author_kind: String,
    text: String,
    attachment_ids: Vec<String>,
    notify: Value,
}

fn pending_delivery(coords: CommentCoords, answer: &Value) -> Option<PendingDelivery> {
    let comment = &answer["comment"];
    if comment["notify"]["state"].as_str() != Some("notifying") {
        return None;
    }
    let author = comment["author"].as_str().unwrap_or("owner");
    let author_kind = if author.starts_with("bot:") {
        "bot"
    } else if author == "system" {
        "system"
    } else {
        "owner"
    }
    .to_owned();
    Some(PendingDelivery {
        coords,
        comment_id: comment["id"].as_str()?.to_owned(),
        author_kind,
        text: comment["text"].as_str().unwrap_or_default().to_owned(),
        attachment_ids: comment["attachmentIds"]
            .as_array()
            .map(|ids| ids.iter().filter_map(|id| id.as_str().map(ToOwned::to_owned)).collect())
            .unwrap_or_default(),
        notify: comment["notify"].clone(),
    })
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    crate::api::devices::permit(caller, method)?;
    // A guarda também falha em quedas de sincronização; não as converta em recusa definitiva (403).
    let authority = Autoridade::capturar(hub, caller)?;
    ensure!(serde_json::to_vec(&body)?.len() <= 524_288, "projects_limit: solicitação excede 512 KiB.");
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let hub_task = hub.clone();
    let method_task = method.to_owned();
    let coords = comment_coords(&body, method);
    let coords_for_task = coords.clone();
    let (mut answer, pending) = tokio::task::spawn_blocking(move || -> Result<(Value, Option<PendingDelivery>)> {
        let _guard = guard;
        if matches!(
            method_task.as_str(),
            "donnaProjectCreate"
                | "donnaProjectSave"
                | "donnaProjectEnsureDefault"
                | "donnaTaskMetaSave"
                | "donnaTaskComment"
        ) {
            crate::fork::donna_fixa::pedido::antes_de_gravar(&hub_task.store);
        }
        let space = body["spaceId"].as_str().unwrap_or_default().to_owned();
        let (answer, changed) =
            authority.with_db(&hub_task, None, |db| store::call(db, &hub_task.host_id, &method_task, body))?;
        let pending = if changed && method_task == "donnaTaskComment" {
            coords_for_task.and_then(|coords| pending_delivery(coords, &answer))
        } else {
            None
        };
        // Revalida também a saída; a autoridade original nunca é recapturada.
        crate::fork::autoridade::com_saida_validada(
            &hub_task,
            crate::fork::autoridade::OrigemSaida::Controle(&authority, None),
            |db, _, _, _, _| scope(db, &hub_task.host_id, &space, &hub_task.host_id),
            || {
                if changed {
                    let _ = hub_task.events.send(json!({"type":EVENT,"hubId":hub_task.host_id,"spaceId":space}));
                }
                (answer, pending)
            },
        )
    })
    .await??;
    if method == "donnaTaskComments"
        && let Some(coords) = coords
        && let Some(comments) = answer["comments"].as_array_mut()
    {
        for comment in comments {
            let Some(id) = comment["id"].as_str().map(ToOwned::to_owned) else { continue };
            if let Ok(Some(notify)) =
                crate::fork::tarefas_runtime::avisos::status_of(hub, &coords.space, &coords.project, &coords.task, &id)
                    .await
            {
                comment["notify"] = notify;
            }
        }
    }
    if let Some(pending) = pending {
        let _ = crate::fork::tarefas_runtime::avisos::deliver(
            hub,
            &pending.coords.space,
            &pending.coords.project,
            &pending.coords.task,
            &pending.comment_id,
            &pending.author_kind,
            &pending.text,
            &pending.attachment_ids,
            &pending.notify,
        )
        .await;
    }
    Ok(answer)
}

pub(crate) fn event(hub: &Hub, caller: &Caller, space: &str, input: &Value) -> Option<Value> {
    if input["type"] != EVENT || input["hubId"] != hub.host_id || input["spaceId"] != space {
        return None;
    }
    crate::api::devices::permit(caller, METHODS[0]).ok()?;
    let authority = Autoridade::capturar(hub, caller).ok()?;
    crate::fork::autoridade::com_saida_validada(
        hub,
        crate::fork::autoridade::OrigemSaida::Controle(&authority, None),
        |db, _, _, _, _| scope(db, &hub.host_id, space, &hub.host_id),
        || json!({"type":EVENT,"hubId":hub.host_id,"spaceId":space}),
    )
    .ok()
}
