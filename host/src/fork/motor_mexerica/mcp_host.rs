//! Cliente MCP do LADO DO HUB para o Motor Mexerica.
//!
//! O motor nunca fala com servidor MCP: quem abre os servidores (as mesmas
//! definições `{name, command, args, env}` que os outros motores recebem em
//! `session/new`) é o Hub, que descobre as ferramentas (`tools/list`), as
//! declara ao motor por `set_tools` e executa cada `tool_request` por
//! `tools/call`. Servidores internos do Hub (`<exe> mcp <nome>`) rodam na
//! identidade do Hub; os demais (conectores) abrem pelo ajudante de motor,
//! como já acontece com os outros motores. Definição sem `command` (URL
//! remota) é recusada: o motor não ganha caminho de rede por aqui.

use super::tools::{ToolFuture, ToolOrigin, ToolRunner, ToolSpec};
use anyhow::{Context, Result, anyhow, bail, ensure};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, oneshot};

const PROTOCOL: &str = "2025-06-18";
const SETUP_TIMEOUT: Duration = Duration::from_secs(30);
/// Prazo de uma chamada; a aprovação humana do Hub é anterior a ela.
const CALL_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_PAGES: usize = 20;
const MAX_TOOLS: usize = 256;
/// Teto de bytes (descrição e esquema) lidos de um servidor numa listagem, e prazo total dela: um conector
/// não faz o Hub alocar nem esperar sem limite.
const MAX_DISCOVERY_BYTES: usize = 4 * 1024 * 1024;
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(60);
/// Prazo da consulta `browser_status` v3 que o Hub faz entre turnos.
const STATUS_TIMEOUT: Duration = Duration::from_secs(15);
/// Servidor interno do Hub que oferece as ferramentas de navegador.
const MEMORY: &str = "memory";

type Waiters = Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

struct Server {
    name: String,
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    waiters: Waiters,
    next_id: std::sync::atomic::AtomicU64,
    /// Servidor interno do Hub (`<exe> mcp <nome>` de `HUB_SERVERS`), não conector.
    own: bool,
    /// Só para o `kill_on_drop`/grupo: os servidores morrem com o `McpHost`.
    _child: crate::fork::motor_exec::Child,
}

impl Server {
    async fn write(&self, message: Value) -> Result<()> {
        let mut line = serde_json::to_vec(&message)?;
        line.push(b'\n');
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(&line).await?;
        stdin.flush().await?;
        Ok(())
    }

    async fn request(&self, method: &str, params: Value, limit: Duration) -> Result<Value> {
        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.waiters.lock().map_err(|_| anyhow!("estado do servidor MCP corrompido"))?.insert(id, tx);
        // Pedido largado (turno cancelado, timeout): `notifications/cancelled`
        // corta a chamada no servidor, como no caminho ACP.
        let mut cancel = CancelGuard { server: self, id, armed: true };
        let sent = self.write(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).await;
        if let Err(error) = sent {
            self.forget(id);
            cancel.armed = false;
            return Err(error.context(format!("escrevendo no servidor MCP {}", self.name)));
        }
        match tokio::time::timeout(limit, rx).await {
            Ok(Ok(Ok(result))) => {
                cancel.armed = false;
                Ok(result)
            }
            Ok(Ok(Err(error))) => {
                cancel.armed = false;
                bail!("{error}")
            }
            Ok(Err(_)) => bail!("o servidor MCP {} encerrou antes de responder", self.name),
            Err(_) => {
                self.forget(id);
                bail!("o servidor MCP {} não respondeu no prazo ({method})", self.name)
            }
        }
    }

    fn forget(&self, id: u64) {
        if let Ok(mut waiters) = self.waiters.lock() {
            waiters.remove(&id);
        }
    }
}

/// Servidores MCP abertos pelo Hub para um motor, e a tabela nome→(servidor, ferramenta).
pub(crate) struct McpHost {
    servers: Vec<Server>,
    catalog: std::sync::Mutex<Catalog>,
}

/// Tabela nome→rota e lista de ferramentas declaradas ao motor. Muda só no `refresh_browser`, entre turnos.
#[derive(Default, Clone)]
struct Catalog {
    routes: HashMap<String, Route>,
    specs: Vec<ToolSpec>,
}

/// Destino de uma ferramenta exposta: o servidor que o PRÓPRIO Hub abriu e o
/// nome original. É daqui — nunca de dado do motor — que sai a identidade.
#[derive(Clone)]
struct Route {
    index: usize,
    original: String,
}

/// O que o Hub sabe de uma ferramenta exposta (identidade e classe).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ToolClass {
    pub server: String,
    pub tool: String,
    /// Servidor interno do Hub (e não conector).
    #[cfg_attr(not(test), allow(dead_code))]
    pub hub: bool,
}

impl ToolClass {
    /// Liberada sem cartão: só o que a regra `allow` dos motores ACP libera
    /// (`engine_permissions::mcp_released`, hoje nada). `readOnlyHint` e servidor
    /// interno não liberam sozinhos: no Claude e no Codex o cartão abre igual.
    pub(crate) fn released(&self) -> bool {
        crate::agent::engine_permissions::mcp_released(&self.server, &self.tool)
    }
}

/// Nome mostrado ao modelo: `mcp__<servidor>__<ferramenta>`, só `[A-Za-z0-9_-]`, até 64.
/// `__` e `_` nas pontas são recusados: quem lê `mcp__a__b` como `split_once("__")`
/// não pode receber servidor nem ferramenta ambíguos.
fn exposed_name(server: &str, tool: &str) -> Option<String> {
    let clean = |text: &str| {
        text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            && !text.contains("__")
            && !text.starts_with('_')
            && !text.ends_with('_')
    };
    let name = format!("mcp__{server}__{tool}");
    (clean(server) && clean(tool) && !server.is_empty() && !tool.is_empty() && name.len() <= 64).then_some(name)
}

impl McpHost {
    /// Abre cada definição stdio e descobre as ferramentas. Qualquer falha
    /// encerra o que já abriu e recusa tudo (falha fechada).
    pub(crate) async fn start(definitions: &Value, cwd: &std::path::Path) -> Result<Self> {
        Self::start_as(definitions, cwd, std::env::current_exe().ok().as_deref()).await
    }

    /// Como [`McpHost::start`], com o executável que conta como "o Hub" explícito
    /// (provas: o executável do teste não fala MCP, então a prova usa um servidor
    /// de mentira no lugar dele).
    pub(crate) async fn start_as(
        definitions: &Value,
        cwd: &std::path::Path,
        hub_exe: Option<&std::path::Path>,
    ) -> Result<Self> {
        let mut host = Self { servers: Vec::new(), catalog: std::sync::Mutex::new(Catalog::default()) };
        for definition in definitions.as_array().map(Vec::as_slice).unwrap_or_default() {
            host.add(definition, cwd, hub_exe).await?;
        }
        Ok(host)
    }

    pub(crate) fn specs(&self) -> Vec<ToolSpec> {
        self.catalog.lock().map(|catalog| catalog.specs.clone()).unwrap_or_default()
    }

    fn route(&self, name: &str) -> Option<Route> {
        self.catalog.lock().ok()?.routes.get(name).cloned()
    }

    /// Cliente do Hub no contrato v3 do navegador: consulta `browser_status` v3 pelo servidor `memory` (o que
    /// marca o vínculo como negociado se o navegador estiver disponível) e lista as ferramentas de novo. Só roda
    /// entre turnos. Devolve `true` se o conjunto de ferramentas mudou e o motor precisa de `set_tools`.
    pub(crate) async fn refresh_browser(&self) -> Result<bool> {
        let Some((index, server)) =
            self.servers.iter().enumerate().find(|(_, server)| server.own && server.name == MEMORY)
        else {
            return Ok(false);
        };
        let status = json!({"name": crate::fork::browser::STATUS_TOOL, "arguments": {"version": 3, "target": "host"}});
        // Fail-closed: sem a confirmação da negociação v3 neste turno, a relistagem não continua. O erro sobe
        // para o chamador, que mantém o catálogo anterior; as ferramentas de página só voltam com a consulta de pé.
        let negotiated = server
            .request("tools/call", status, STATUS_TIMEOUT)
            .await
            .context("consulta de navegador v3 recusada; catálogo mantido")?;
        // A ponte real devolve a própria falha como resultado com isError (não como erro de RPC): sem a
        // confirmação da negociação v3, a relistagem não continua e o catálogo anterior fica intacto.
        ensure!(negotiated["isError"] != true, "consulta de navegador v3 falhou; catálogo mantido");
        // A substituição é montada inteira numa cópia e só entra se a listagem completa couber e for válida:
        // refresh que falha (erro da ponte, página malformada, ferramentas demais) preserva o catálogo que já funcionava.
        let mut catalog = self.catalog.lock().map_err(|_| anyhow!("catálogo de ferramentas corrompido"))?.clone();
        let prefix = format!("mcp__{MEMORY}__");
        let names = |specs: &[ToolSpec]| -> std::collections::BTreeSet<String> {
            specs.iter().filter(|spec| spec.name.starts_with(&prefix)).map(|spec| spec.name.clone()).collect()
        };
        let before = names(&catalog.specs);
        catalog.routes.retain(|_, route| route.index != index);
        catalog.specs.retain(|spec| !spec.name.starts_with(&prefix));
        let listed = Self::discover(server, true, None, MAX_TOOLS.saturating_sub(catalog.routes.len())).await?;
        for (exposed, original, spec) in listed {
            ensure!(
                catalog.routes.insert(exposed.clone(), Route { index, original }).is_none(),
                "ferramenta repetida: {exposed}"
            );
            catalog.specs.push(spec);
        }
        let changed = before != names(&catalog.specs);
        *self.catalog.lock().map_err(|_| anyhow!("catálogo de ferramentas corrompido"))? = catalog;
        Ok(changed)
    }

    /// Lista as ferramentas de um servidor já iniciado: (nome exposto, nome original, declaração). `room` é quantas
    /// ainda cabem no catálogo: a leitura para na página que passaria dele (ou do teto de bytes), antes de copiar
    /// qualquer ferramenta, e uma página sem a lista de ferramentas é erro, não lista vazia.
    async fn discover(
        server: &Server,
        own: bool,
        account_label: Option<String>,
        room: usize,
    ) -> Result<Vec<(String, String, ToolSpec)>> {
        tokio::time::timeout(DISCOVERY_TIMEOUT, Self::discover_pages(server, own, account_label, room))
            .await
            .map_err(|_| anyhow!("o servidor MCP {} não terminou de listar as ferramentas no prazo", server.name))?
    }

    async fn discover_pages(
        server: &Server,
        own: bool,
        account_label: Option<String>,
        room: usize,
    ) -> Result<Vec<(String, String, ToolSpec)>> {
        let mut found = Vec::new();
        let mut bytes = 0usize;
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let params = cursor.as_ref().map_or_else(|| json!({}), |c| json!({"cursor": c}));
            let page = server.request("tools/list", params, SETUP_TIMEOUT).await?;
            let tools = page["tools"].as_array().with_context(|| {
                format!("o servidor MCP {} respondeu tools/list sem a lista de ferramentas", server.name)
            })?;
            ensure!(found.len() + tools.len() <= room, "ferramentas demais para o motor");
            bytes += tools
                .iter()
                .map(|tool| tool["description"].as_str().map_or(0, str::len) + tool["inputSchema"].to_string().len())
                .sum::<usize>();
            ensure!(bytes <= MAX_DISCOVERY_BYTES, "o servidor MCP {} listou ferramentas demais em bytes", server.name);
            for tool in tools {
                let original = tool["name"].as_str().context("ferramenta MCP sem nome")?;
                let exposed = exposed_name(&server.name, original)
                    .with_context(|| format!("nome de ferramenta MCP inválido: {}/{original}", server.name))?;
                found.push((
                    exposed.clone(),
                    original.to_owned(),
                    ToolSpec {
                        name: exposed,
                        description: tool["description"].as_str().unwrap_or_default().to_owned(),
                        input_schema: tool["inputSchema"].clone(),
                        origin: if own {
                            ToolOrigin::Hub
                        } else {
                            ToolOrigin::Connector { account_label: account_label.clone() }
                        },
                    },
                ));
            }
            match &page["nextCursor"] {
                Value::Null => return Ok(found),
                Value::String(next) => cursor = Some(next.clone()),
                _ => bail!("o servidor MCP {} respondeu tools/list com cursor inválido", server.name),
            }
        }
        bail!("o servidor MCP {} paginou ferramentas demais", server.name)
    }

    /// Identidade e classe de uma ferramenta exposta, pela tabela que o Hub montou.
    pub(crate) fn classify(&self, name: &str) -> Option<ToolClass> {
        let route = self.route(name)?;
        let server = self.servers.get(route.index)?;
        Some(ToolClass { server: server.name.clone(), tool: route.original.clone(), hub: server.own })
    }

    async fn add(
        &mut self,
        definition: &Value,
        cwd: &std::path::Path,
        hub_exe: Option<&std::path::Path>,
    ) -> Result<()> {
        let name = definition["name"].as_str().context("definição MCP sem nome")?.to_owned();
        let command = definition["command"].as_str().context("só servidor MCP stdio é aceito pelo Hub do motor")?;
        let args: Vec<&str> = definition["args"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        ensure!(self.servers.iter().all(|s| s.name != name), "servidor MCP repetido: {name}");

        // "Do Hub" é o critério de `internal_servers`: nome de HUB_SERVERS com
        // a definição exata (`<exe> mcp <nome>`) — conectores curados,
        // `connectors`, `composio`, `team`, `tasks` e `computer` abrem pelo
        // ajudante do motor e saem como conector, nunca como internos.
        let own = hub_exe.is_some_and(|exe| {
            std::path::Path::new(command) == exe
                && crate::agent::engine_permissions::HUB_SERVERS.contains(&name.as_str())
                && args.first() == Some(&"mcp")
                && args.get(1) == Some(&name.as_str())
        });
        let mut process =
            if own { tokio::process::Command::new(command) } else { crate::fork::motor_exec::command(command, cwd) };
        process.args(&args);
        process
            .env_clear()
            .envs(crate::agent::environment::inherited(std::env::vars_os()))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        for entry in definition["env"].as_array().into_iter().flatten() {
            if let (Some(key), Some(value)) = (entry["name"].as_str(), entry["value"].as_str()) {
                process.env(key, value);
            }
        }
        if own {
            process.current_dir(cwd);
        }
        #[cfg(unix)]
        process.process_group(0);
        let mut child =
            crate::fork::motor_exec::spawn(&mut process).with_context(|| format!("abrindo o servidor MCP {name}"))?;
        let stdin = child.stdin.take().context("stdin do servidor MCP ausente")?;
        let stdout = child.stdout.take().context("stdout do servidor MCP ausente")?;
        let waiters = Waiters::default();
        spawn_reader(stdout, waiters.clone());
        let server = Server {
            name: name.clone(),
            stdin: Arc::new(Mutex::new(stdin)),
            waiters,
            next_id: std::sync::atomic::AtomicU64::new(1),
            own,
            _child: child,
        };
        server
            .request(
                "initialize",
                json!({"protocolVersion": PROTOCOL, "capabilities": {},
                       "clientInfo": {"name": "mexerica-hub", "version": env!("CARGO_PKG_VERSION")}}),
                SETUP_TIMEOUT,
            )
            .await?;
        server.write(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await?;
        let index = self.servers.len();
        let label = definition["accountLabel"].as_str().map(str::to_owned);
        let catalog = self.catalog.get_mut().map_err(|_| anyhow!("catálogo de ferramentas corrompido"))?;
        let listed = Self::discover(&server, own, label, MAX_TOOLS.saturating_sub(catalog.routes.len())).await?;
        for (exposed, original, spec) in listed {
            ensure!(
                catalog.routes.insert(exposed.clone(), Route { index, original }).is_none(),
                "ferramenta repetida: {exposed}"
            );
            catalog.specs.push(spec);
        }
        self.servers.push(server);
        Ok(())
    }
}

/// Guarda de cancelamento de um pedido MCP: descartado armado, avisa o
/// servidor com `notifications/cancelled` e solta quem espera.
struct CancelGuard<'a> {
    server: &'a Server,
    id: u64,
    armed: bool,
}

impl Drop for CancelGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.server.forget(self.id);
        let stdin = self.server.stdin.clone();
        let id = self.id;
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let mut line = serde_json::to_vec(&json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/cancelled",
                    "params": {"requestId": id, "reason": "pedido largado pelo Hub"},
                }))?;
                line.push(b'\n');
                let _ = stdin.lock().await.write_all(&line).await;
                Ok::<_, anyhow::Error>(())
            });
        }
    }
}

/// Teto de uma linha JSONL lida do motor ou de um servidor MCP: uma linha
/// gigante de um conector não vai inteira para a memória do Hub.
pub(crate) const MAX_LINE: usize = 8 * 1024 * 1024;

/// `read_until` com teto: `n == MAX_LINE + 1` sem `\n` no fim = linha grande
/// demais; a fronteira fecha em vez de segurar o buffer.
pub(crate) async fn read_line<R: tokio::io::AsyncBufRead + Unpin>(reader: &mut R, buf: &mut Vec<u8>) -> Result<usize> {
    use tokio::io::AsyncReadExt;
    buf.clear();
    let n = reader.take(MAX_LINE as u64 + 1).read_until(b'\n', buf).await?;
    ensure!(n <= MAX_LINE || buf.last() == Some(&b'\n'), "linha JSONL acima de {MAX_LINE} bytes");
    Ok(n)
}

/// Lê as linhas do servidor: respostas acordam quem espera; pedidos do servidor
/// ao cliente (amostragem, raízes) são recusados — o Hub não os oferece.
fn spawn_reader(stdout: tokio::process::ChildStdout, waiters: Waiters) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        let mut buffer = Vec::new();
        while let Ok(n) = read_line(&mut reader, &mut buffer).await
            && n > 0
        {
            let Ok(message) = serde_json::from_slice::<Value>(&buffer) else { continue };
            let Some(id) = message["id"].as_u64() else { continue };
            if message["method"].is_string() {
                continue;
            }
            let result = match message.get("error") {
                Some(error) => Err(error["message"].as_str().unwrap_or("erro do servidor MCP").to_owned()),
                None => Ok(message["result"].clone()),
            };
            let waiter = waiters.lock().ok().and_then(|mut w| w.remove(&id));
            if let Some(waiter) = waiter {
                let _ = waiter.send(result);
            }
        }
        // Saída fechada ou linha acima do teto: ninguém mais espera resposta.
        if let Ok(mut waiters) = waiters.lock() {
            waiters.clear();
        }
    });
}

impl ToolRunner for McpHost {
    fn call<'a>(&'a self, _call_id: &'a str, name: &'a str, input: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let Some(Route { index, original }) = self.route(name) else {
                return Err("ferramenta desconhecida".to_owned());
            };
            let server = &self.servers[index];
            let arguments = if input.is_object() { input } else { json!({}) };
            let result = server
                .request("tools/call", json!({"name": original, "arguments": arguments}), CALL_TIMEOUT)
                .await
                .map_err(|error| format!("{error:#}"))?;
            let text = result["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|block| (block["type"] == "text").then(|| block["text"].as_str()).flatten())
                .collect::<Vec<_>>()
                .join("\n");
            if result["isError"] == true { Err(text) } else { Ok(text) }
        })
    }
}
