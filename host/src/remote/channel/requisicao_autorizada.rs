//! Resposta da request original: cancelar retira a admissão, sem abortar o trabalho.
use super::{Caller, Hub, Job, ToDevice, api, autoridade, close, err};
use crate::LockExt;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::{sync::mpsc, task::Id};

fn propria(jobs: &HashMap<u64, Job>, id: u64, tarefa: Id, admissao: super::barreira_workspace::Admissao) -> bool {
    matches!(jobs.get(&id), Some(Job::Request(job, a)) if job.id() == tarefa && *a == admissao)
}

fn retirar_propria(
    jobs: &Mutex<HashMap<u64, Job>>,
    id: u64,
    tarefa: Id,
    admissao: super::barreira_workspace::Admissao,
) {
    let mut lista = jobs.locked();
    if propria(&lista, id, tarefa, admissao) {
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
    (tx, admissao): (mpsc::Sender<ToDevice>, super::barreira_workspace::Admissao),
) {
    let tarefa = tokio::task::id();
    let resultado = api::dispatch(&hub, &caller, &method, b).await;
    let resposta = ToDevice::Inner(match resultado {
        Ok(ok) => json!({"id": id, "ok": ok}),
        Err(erro) => {
            let (status, message) = api::error_status(&erro);
            let mut response = err(id, status.as_u16(), &message);
            if let Some(code) = api::error_code(&erro) {
                response["err"]["code"] = code.into();
            } else if caller.is_web_chat() && crate::fork::workspace::is_mismatch(&erro) {
                response["err"]["code"] = crate::fork::workspace::EPOCH_MISMATCH.into();
            }
            response
        }
    });
    if !propria(&jobs.locked(), id, tarefa, admissao) {
        return;
    }
    let Ok(vaga) = tx.reserve().await else {
        retirar_propria(&jobs, id, tarefa, admissao);
        return;
    };
    // A recusa não chama a callback; conservar a vaga para o fechamento negativo.
    let mut vaga = Some(vaga);
    if autoridade::com_saida_caller(&hub, &caller, || {
        let mut lista = jobs.locked();
        if propria(&lista, id, tarefa, admissao)
            && let Some(vaga) = vaga.take()
        {
            lista.remove(&id);
            vaga.send(resposta);
        }
    })
    .is_err_and(|erro| {
        if erro.is::<autoridade::EpochDaRequisicaoEncerrado>() {
            retirar_propria(&jobs, id, tarefa, admissao);
            false
        } else {
            true
        }
    }) {
        let mut lista = jobs.locked();
        if propria(&lista, id, tarefa, admissao)
            && let Some(vaga) = vaga
        {
            lista.remove(&id);
            vaga.send(ToDevice::Close(close::REKEY, "request authorization ended"));
        }
    }
}
