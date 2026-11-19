use super::{
    model::{Change, Read},
    store,
};
use crate::{api::devices::Caller, fork::autoridade::Autoridade, hub::Hub};
use anyhow::{Result, ensure};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{ops::ControlFlow, sync::Arc};

pub(crate) const METHODS: &[&str] = &["donnaTimes", "donnaTimesPreview", "donnaTimesSave"];

fn parse<T: DeserializeOwned>(body: Value) -> Result<T> {
    serde_json::from_value(body)
        .map_err(|_| anyhow::anyhow!("invalid_field: solicitação de Times inválida ou campo desconhecido."))
}

pub(crate) async fn call(
    hub: &Arc<Hub>,
    caller: &Caller,
    method: &str,
    body: Value,
) -> Result<ControlFlow<Value, Value>> {
    if !METHODS.contains(&method) {
        return Ok(ControlFlow::Continue(body));
    }
    let autoridade = Autoridade::capturar(hub, caller)?;
    ensure!(serde_json::to_vec(&body)?.len() <= 524_288, "times_limit: solicitação excede 512 KiB.");
    if method == "donnaTimesPreview" {
        ensure!(body.get("clientRequestId").is_none(), "invalid_field: prévia não recebe clientRequestId.");
    }
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let method = method.to_owned();
    let hub = hub.clone();
    let answer = tokio::task::spawn_blocking(move || -> Result<Value> {
        // Worker conserva a trava após desconexão do solicitante, inclusive até o evento.
        let _guard = guard;
        let (answer, changed) = autoridade.with_db(&hub, None, |tx| {
            if method == "donnaTimes" {
                let request: Read = parse(body)?;
                Ok((
                    serde_json::to_value(store::snapshot(
                        tx,
                        request.space_id.as_deref().unwrap_or(crate::fork::spaces::PERSONAL),
                    )?)?,
                    false,
                ))
            } else {
                let request: Change = parse(body)?;
                let save = method == "donnaTimesSave";
                ensure!(!save || request.client_request_id.is_some(), "invalid_field: clientRequestId obrigatório.");
                store::change(tx, &request, save)
            }
        })?;
        if changed {
            let snapshot = &answer["snapshot"];
            let _ = hub.events.send(json!({
                "type":"times","spaceId":snapshot["spaceId"],"revision":snapshot["revision"]
            }));
        }
        Ok(answer)
    })
    .await??;
    Ok(ControlFlow::Break(answer))
}
