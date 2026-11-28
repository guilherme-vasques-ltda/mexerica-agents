use super::{Hub, Mediation, OpenMediation};
use crate::{
    fork::{
        conectores::{catalogo_local::Processo, mcp_version::Perfil},
        spaces::connector_forward,
    },
    guard_hub::mcp::{
        http,
        stdio::{Frame, Frames},
    },
};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{
    future::poll_fn,
    sync::Arc,
    task::{Poll, ready},
    time::Duration,
};
use tokio::process::{ChildStdin, ChildStdout};

pub(super) struct Session<'a> {
    pub(super) mediation: OpenMediation<'a>,
    input: ChildStdin,
    output: Frames<ChildStdout>,
    _process: Processo,
}

impl<'a> Session<'a> {
    pub(super) async fn new(hub: &Arc<Hub>, mut mediation: OpenMediation<'a>) -> Result<Self> {
        mediation.account_secrets(&mediation.saved.clone());
        let mut process = Processo::iniciar(
            hub.clone(),
            mediation.intent.clone(),
            mediation.saved.clone(),
            #[cfg(test)]
            mediation.artifact_root,
        )
        .await?;
        mediation.secret(&process.home().to_string_lossy());
        let (input, output) = process.pipes()?;
        Ok(Self { mediation, input, output: Frames::new(output), _process: process })
    }

    pub(super) async fn exchange(&mut self, mut frame: Value) -> Result<Option<Value>> {
        let mcp_method = http::method(&frame)?.to_owned();
        let id = uuid::Uuid::new_v4().to_string();
        let notification = mcp_method == "notifications/initialized";
        if !notification {
            frame["id"] = Value::String(id.clone());
        }
        let mediation = &mut self.mediation;
        connector_forward::write_with(&mut self.input, &frame, async |input, commit| {
            poll_fn(|cx| {
                let (generation, sent) = mediation
                    .authorize_send(&frame, &mcp_method, || connector_forward::poll_commit(input, cx, commit))?;
                ready!(sent)?;
                mediation.record_send(&mcp_method, generation);
                Poll::Ready(Ok(()))
            })
            .await
        })
        .await?;
        if notification {
            return Ok(None);
        }
        let ids = [serde_json::to_string(&id)?];
        let response = match tokio::time::timeout(Duration::from_secs(60), self.output.read(&ids)).await?? {
            Some(Frame::Message(response))
                if response["jsonrpc"] == "2.0"
                    && response["id"] == id
                    && response.get("method").is_none()
                    && (response.get("result").is_some() != response.get("error").is_some()) =>
            {
                response
            }
            Some(_) => {
                mediation.observe(json!({}), "invalid").await?;
                anyhow::bail!("Resposta MCP inválida ou não correlacionada");
            }
            None => anyhow::bail!("Resposta MCP ausente"),
        };
        let version = (mcp_method == "initialize").then(|| Perfil::BridgeLegacy.resultado(&response["result"]));
        let observed = mediation.observe(response, &mcp_method).await?;
        if let Some(version) = version {
            version.ok().context("Versão MCP inválida")?;
        }
        Ok(observed)
    }
}
