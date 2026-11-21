//! Proposta e decisão de subtarefas: o bot propõe sem criar nada; o dono
//! aprova ou recusa, e a aprovação cria as tarefas e agenda os inícios numa
//! transação só. O início efetivo segue pelo tick do runtime de tarefas.
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

pub(crate) const METHODS: &[&str] = &["donnaTaskProposals", "donnaTaskProposalDecide"];

pub(crate) fn migrate(c: &rusqlite::Connection) -> Result<()> {
    let tx = c.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_task_proposals(
        hub_id TEXT NOT NULL, space_id TEXT NOT NULL, id TEXT NOT NULL,
        project_id TEXT NOT NULL, bot_id TEXT NOT NULL, request_id TEXT NOT NULL,
        title TEXT NOT NULL, items TEXT NOT NULL, state TEXT NOT NULL, version INTEGER NOT NULL,
        created_at INTEGER NOT NULL, decided_at INTEGER, decision_request_id TEXT,
        PRIMARY KEY(hub_id,space_id,id), UNIQUE(hub_id,space_id,request_id));
        CREATE INDEX IF NOT EXISTS donna_task_proposals_space
        ON donna_task_proposals(hub_id,space_id,state,created_at);",
    )?;
    // A agenda do runtime ganha a coluna da proposta que agendou o início;
    // só quando a tabela já existe, na mesma régua do `request_id`.
    let agenda_ready: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='donna_task_agenda')",
        [],
        |r| r.get(0),
    )?;
    if agenda_ready {
        let has_proposal_id: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('donna_task_agenda') WHERE name='proposal_id')",
            [],
            |r| r.get(0),
        )?;
        if !has_proposal_id {
            tx.execute("ALTER TABLE donna_task_agenda ADD COLUMN proposal_id TEXT", [])?;
        }
    }
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

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    crate::api::devices::permit(caller, method)?;
    // A guarda também falha em quedas de sincronização; não as converta em recusa definitiva (403).
    let authority = Autoridade::capturar(hub, caller)?;
    ensure!(serde_json::to_vec(&body)?.len() <= 524_288, "proposals_limit: solicitação excede 512 KiB.");
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let hub_task = hub.clone();
    let method_task = method.to_owned();
    tokio::task::spawn_blocking(move || -> Result<Value> {
        let _guard = guard;
        if method_task == "donnaTaskProposalDecide" {
            crate::fork::donna_fixa::pedido::antes_de_gravar(&hub_task.store);
        }
        let space = body["spaceId"].as_str().unwrap_or_default().to_owned();
        let (answer, changed) =
            authority.with_db(&hub_task, None, |db| store::call(db, &hub_task.host_id, &method_task, body))?;
        crate::fork::autoridade::com_saida_validada(
            &hub_task,
            crate::fork::autoridade::OrigemSaida::Controle(&authority, None),
            |db, _, _, _, _| scope(db, &hub_task.host_id, &space, &hub_task.host_id),
            || {
                if changed {
                    let _ = hub_task.events.send(json!({
                        "type": crate::fork::projetos::EVENT,
                        "hubId": hub_task.host_id,
                        "spaceId": space,
                    }));
                }
                answer
            },
        )
    })
    .await?
}
