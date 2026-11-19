//! Finaliza a assinatura original sem renovar autoridade nem deixar o cliente sem stream.
use super::{Caller, Hub, Job, ToDevice, autoridade, close};
use crate::LockExt;
use anyhow::Result;
use futures::{StreamExt, stream::BoxStream};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::{sync::mpsc, task::Id};

fn propria(jobs: &HashMap<u64, Job>, id: u64, tarefa: Id) -> bool {
    matches!(jobs.get(&id), Some(Job::Sub(job)) if job.id() == tarefa)
}

fn finalizar(jobs: &Mutex<HashMap<u64, Job>>, id: u64, tarefa: Id, fechamento: Option<mpsc::Permit<'_, ToDevice>>) {
    let mut lista = jobs.locked();
    if propria(&lista, id, tarefa) {
        lista.remove(&id);
        if let Some(vaga) = fechamento {
            // Só controle negativo constante; o cliente autentica um canal novo.
            vaga.send(ToDevice::Close(close::REKEY, "subscription authorization ended"));
        }
    }
}

#[allow(clippy::too_many_arguments)] // A guarda da operação acompanha o stream até a última admissão.
pub(super) async fn executar(
    id: u64,
    mut stream: BoxStream<'static, Value>,
    hub: Arc<Hub>,
    caller: Arc<Caller>,
    jobs: Arc<Mutex<HashMap<u64, Job>>>,
    tx: mpsc::Sender<ToDevice>,
    login: Option<Arc<crate::fork::terminal_login::Login>>,
) {
    let tarefa = tokio::task::id();
    let resultado: Result<()> = async {
        while let Some(ev) = stream.next().await {
            let resposta = ToDevice::Inner(json!({"id": id, "ev": ev}));
            let vaga = tx.reserve().await?;
            let send = || {
                let lista = jobs.locked();
                if !propria(&lista, id, tarefa) {
                    return false;
                }
                vaga.send(resposta);
                true
            };
            let enviada = if let Some(login) = &login {
                // A tentativa encerrada perde seus bytes, mas não encerra o transporte válido.
                match login.deliver(send) {
                    Ok(delivered) => delivered,
                    Err(_) => break,
                }
            } else {
                autoridade::com_saida_caller(&hub, &caller, send)?
            };
            if !enviada {
                return Ok(());
            }
        }
        let resposta = ToDevice::Inner(json!({"id": id, "end": true}));
        let vaga = tx.reserve().await?;
        let send = || {
            let lista = jobs.locked();
            if propria(&lista, id, tarefa) {
                vaga.send(resposta);
            }
        };
        // Controle negativo constante, sem bytes do PTY; basta o transporte original.
        autoridade::com_saida_caller(&hub, &caller, send)?;
        Ok(())
    }
    .await;
    drop(stream);
    let fechamento = if resultado.is_err() { tx.reserve().await.ok() } else { None };
    // A tarefa antiga nunca remove nem fecha uma assinatura que reutilizou seu id.
    finalizar(&jobs, id, tarefa, fechamento);
}

#[cfg(test)]
mod provas;
