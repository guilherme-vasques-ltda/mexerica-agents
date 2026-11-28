// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Check the MCP handshake and tool discovery before reporting a connector ready.
use crate::store::Store;
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

pub async fn verify(store: &Store, id: &str, destination: (u16, Option<&Path>)) -> Result<Value> {
    verify_timeout(store, id, destination, Duration::from_secs(90)).await
}

async fn verify_timeout(
    store: &Store,
    id: &str,
    (port, socket): (u16, Option<&Path>),
    timeout: Duration,
) -> Result<Value> {
    let c = super::connectors(store)?.into_iter().find(|c| c.id == id).ok_or_else(|| anyhow!("Unknown connector"))?;
    if socket.is_some() && !super::contas::desktop_catalog(&c) {
        bail!("O Hub local só conecta contas do catálogo");
    }
    if !c.configured() {
        bail!("mcp_manual_required: configuração MCP incompleta; informe o alvo manualmente, sem MCP Registry.");
    }
    if c.command.is_some() && c.servico.is_some() {
        bail!("Conta do catálogo não roda como comando local");
    }
    if c.oauth.as_ref().is_some_and(|o| !o.signed_in()) {
        bail!("Sign in before checking this connection.");
    }
    let setup =
        if c.command.is_none() { Some(crate::fork::conectores::oauth_binding::issue_setup(store, &c)?) } else { None };
    let mut command = if let Some(exe) = &c.command {
        let cwd = dirs::home_dir().unwrap_or_else(|| "/".into());
        let mut command = crate::fork::motor_exec::command(exe, &cwd);
        if crate::fork::motor_exec::configured() {
            command.env_clear().envs(crate::agent::environment::inherited(std::env::vars_os()));
        }
        command.args(&c.args);
        for (name, value) in &c.env {
            command.env(name, super::passwords::resolve(store, value).await?);
        }
        command
    } else {
        let mut command = Command::new(std::env::current_exe()?);
        command.env(crate::fork::ponte_credencial::SETUP_ENV, "1");
        command.env_remove(crate::fork::ponte_credencial::ENV);
        if let Some(socket) = socket {
            command.env(crate::fork::bridge_http::SOCKET_ENV, socket);
        } else {
            command.env_remove(crate::fork::bridge_http::SOCKET_ENV);
        }
        let context = setup.as_ref().ok_or_else(|| anyhow!("Contexto de setup ausente"))?;
        command.arg("mcp");
        if crate::fork::conectores::catalogo_local::da_conta(&c).is_some() {
            command.args(["local", "--catalog-local"]);
        } else {
            command.arg("remote");
        }
        command.args(["--connector", id, "--port", &port.to_string(), "--context", &context.id]);
        command
    };
    let mut child = crate::fork::motor_exec::spawn(
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true),
    )
    .map_err(|_| anyhow!("Could not start the connector. Install its runtime on this computer and retry."))?;
    let mut input = child.stdin.take().ok_or_else(|| anyhow!("Connector input unavailable"))?;
    let output = child.stdout.take().ok_or_else(|| anyhow!("Connector output unavailable"))?;
    let mut lines = BufReader::new(output).lines();
    let check = async {
        for (id, method, params) in [
            (
                1,
                "initialize",
                json!({"protocolVersion":crate::mcp::PROTOCOL_VERSION,"capabilities":{},"clientInfo":{"name":"Donnabot","version":env!("CARGO_PKG_VERSION")}}),
            ),
            (2, "tools/list", json!({})),
        ] {
            let wire = format!("{}\n", json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
            input.write_all(wire.as_bytes()).await?;
            input.flush().await?;
            loop {
                let line = lines.next_line().await?.ok_or_else(|| anyhow!("Connector exited during setup"))?;
                if line.len() > 4 * 1024 * 1024 {
                    bail!("Connector response too large");
                }
                let Ok(reply) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if let (Some(method), Some(request)) = (reply["method"].as_str(), reply.get("id")) {
                    // Pedido reverso: resposta local de protocolo, nunca resultado do pedido.
                    let answer = if method == "ping" {
                        json!({"jsonrpc":"2.0","id":request,"result":{}})
                    } else {
                        json!({"jsonrpc":"2.0","id":request,"error":{"code":-32601,"message":"Pedido iniciado pelo servidor indisponível"}})
                    };
                    input.write_all(format!("{answer}\n").as_bytes()).await?;
                    input.flush().await?;
                    continue;
                }
                if reply.get("method").is_some() || reply["id"] != id {
                    continue;
                }
                if reply.get("error").is_some() {
                    bail!("Connector rejected setup. Check its credentials and try again.");
                }
                if id == 1 {
                    // Versão efetiva validada antes de initialized/tools/list; sem eco do valor remoto.
                    crate::fork::conectores::mcp_version::Perfil::BridgeLegacy
                        .resultado(&reply["result"])
                        .map_err(|error| anyhow!(error.message()))?;
                    input.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n").await?;
                    input.flush().await?;
                } else {
                    let tools = reply["result"]["tools"]
                        .as_array()
                        .ok_or_else(|| anyhow!("Connector did not return its tools"))?;
                    // Conta de ficha: conta apenas os nomes elegíveis, sem conceder acesso a eles.
                    let permitidas = super::contas::ferramentas_da_conta(&c)?;
                    let expostas =
                        crate::fork::conectores::catalogo_servicos::filtrar_ferramentas(tools, permitidas.as_ref());
                    return Ok(json!({"status":"ready","toolCount":expostas.len()}));
                }
                break;
            }
        }
        bail!("Connector verification did not complete")
    };
    let result = tokio::time::timeout(timeout, check)
        .await
        .map_err(|_| anyhow!("Connector setup timed out. Check the runtime and connection, then retry."));
    drop(input);
    child.kill().await?;
    result?
}

#[cfg(all(test, unix))]
mod cancellation_tests {
    use super::*;

    #[tokio::test]
    async fn desktop_rejects_custom_connector_before_starting_it() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let connector =
            serde_json::from_value(json!({"id":"custom","name":"Custom","command":"/fixture/not-a-runtime"})).unwrap();
        super::super::save_connectors(&store, &[connector]).unwrap();
        let result =
            verify(&store, "custom", (crate::fork::bridge_http::SOCKET_PORT, Some(Path::new("/fixture/socket")))).await;
        assert_eq!(result.unwrap_err().to_string(), "O Hub local só conecta contas do catálogo");
    }

    #[tokio::test]
    async fn motor_verify_persistente_e_timeout_encerram_grupo() {
        let Some(case) = crate::fork::motor_exec::test_support::case(
            "market::verify::cancellation_tests::motor_verify_persistente_e_timeout_encerram_grupo",
        ) else {
            return;
        };
        for hang in [false, true] {
            let fixture = case.fixture(if hang { "mcp-hang" } else { "mcp" }, hang);
            let store = Store::open(Path::new(":memory:")).unwrap();
            let connector =
                serde_json::from_value(json!({"id":"motor-prova","name":"Prova","command":fixture,"args":[]})).unwrap();
            super::super::save_connectors(&store, &[connector]).unwrap();
            let timeout = if hang { Duration::from_millis(500) } else { Duration::from_secs(5) };
            let result =
                tokio::time::timeout(Duration::from_secs(8), verify_timeout(&store, "motor-prova", (1, None), timeout))
                    .await
                    .unwrap();
            assert_eq!(result.is_ok(), !hang);
            case.stopped(&fixture).await;
        }
    }
}
