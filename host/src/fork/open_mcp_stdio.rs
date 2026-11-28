use super::{Hub, LocalStart, Mediation, OpenMediation, rule};
use crate::{
    LockExt,
    fork::{
        conectores::{catalogo_local::Processo, mcp_version::Perfil},
        spaces::connector_forward,
    },
    guard_hub::mcp::{
        http,
        stdio::{Frame, Frames},
    },
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    future::poll_fn,
    sync::Arc,
    task::{Poll, ready},
    time::Duration,
};
use tokio::process::{ChildStdin, ChildStdout};

// Linux garante escrita não bloqueante inteira ou EAGAIN até PIPE_BUF no pipe do filho.
const PIPE_BUF_LINUX: usize = 4096;

pub(super) fn validate_call_size(mut frame: Value) -> Result<()> {
    frame["id"] = Value::String(uuid::Uuid::nil().to_string());
    wire_bytes(&frame).map(|_| ())
}

fn wire_bytes(frame: &Value) -> Result<Vec<u8>> {
    let mut wire = serde_json::to_vec(frame)?;
    wire.push(b'\n');
    ensure!(wire.len() <= PIPE_BUF_LINUX, "Pedido MCP acima do limite atômico");
    Ok(wire)
}

pub(super) struct Session<'a> {
    pub(super) mediation: OpenMediation<'a>,
    input: ChildStdin,
    output: Frames<ChildStdout>,
    _process: Processo,
}

impl<'a> Session<'a> {
    pub(super) async fn new(hub: &Arc<Hub>, mut mediation: OpenMediation<'a>, approval: LocalStart) -> Result<Self> {
        let hub = hub.clone();
        let intent = mediation.intent.clone();
        let expected = mediation.saved.clone();
        let proof = approval.clone();
        let mut process = Processo::iniciar(
            &mediation.saved,
            move |command| {
                intent.with_open_preflight(
                    &hub,
                    &expected,
                    || {
                        let boundary = hub.store.memory_boundary.locked();
                        if let Some((params, _)) = &proof.call {
                            rule(&hub, &proof.card.bot_id, &expected.id, params)?;
                        }
                        Ok(boundary)
                    },
                    |db, current| {
                        proof.check(db, current)?;
                        command.spawn().context("Não foi possível iniciar o MCP do catálogo")
                    },
                )
            },
            #[cfg(test)]
            mediation.artifact_root,
        )
        .await?;
        mediation.local_start = Some(approval);
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
        let wire = wire_bytes(&frame)?;
        let mediation = &mut self.mediation;
        let input = &mut self.input;
        tokio::time::timeout(
            Duration::from_secs(30),
            poll_fn(|cx| {
                let (generation, sent) = mediation
                    .authorize_send(&frame, &mcp_method, || connector_forward::poll_commit(input, cx, &wire))?;
                ready!(sent)?;
                mediation.record_send(&mcp_method, generation);
                Poll::Ready(Ok::<(), anyhow::Error>(()))
            }),
        )
        .await??;
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
