//! Resposta da request original: cancelar retira a admissão, sem abortar o trabalho.
use super::{Caller, Hub, Job, ToDevice, api, autoridade, close, err};
use crate::LockExt;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::{sync::mpsc, task::Id};

fn propria(jobs: &HashMap<u64, Job>, id: u64, tarefa: Id) -> bool {
    matches!(jobs.get(&id), Some(Job::Request(job)) if job.id() == tarefa)
}

fn retirar_propria(jobs: &Mutex<HashMap<u64, Job>>, id: u64, tarefa: Id) {
    let mut lista = jobs.locked();
    if propria(&lista, id, tarefa) {
        lista.remove(&id);
    }
}

pub(super) async fn executar(
    id: u64,
    method: String,
    b: Value,
    hub: Arc<Hub>,
    caller: Arc<Caller>,
    jobs: Arc<Mutex<HashMap<u64, Job>>>,
    tx: mpsc::Sender<ToDevice>,
) {
    let tarefa = tokio::task::id();
    let resposta = ToDevice::Inner(match api::dispatch(&hub, &caller, &method, b).await {
        Ok(ok) => json!({"id": id, "ok": ok}),
        Err(erro) => {
            let (status, message) = api::error_status(&erro);
            let mut response = err(id, status.as_u16(), &message);
            if let Some(code) = api::error_code(&erro) {
                response["err"]["code"] = code.into();
            }
            response
        }
    });
    if !propria(&jobs.locked(), id, tarefa) {
        return;
    }
    let Ok(vaga) = tx.reserve().await else {
        retirar_propria(&jobs, id, tarefa);
        return;
    };
    // A recusa não chama a callback; conservar a vaga para o fechamento negativo.
    let mut vaga = Some(vaga);
    if autoridade::com_saida_caller(&hub, &caller, || {
        let mut lista = jobs.locked();
        if propria(&lista, id, tarefa)
            && let Some(vaga) = vaga.take()
        {
            lista.remove(&id);
            vaga.send(resposta);
        }
    })
    .is_err()
    {
        let mut lista = jobs.locked();
        if propria(&lista, id, tarefa)
            && let Some(vaga) = vaga
        {
            lista.remove(&id);
            vaga.send(ToDevice::Close(close::REKEY, "request authorization ended"));
        }
    }
}
