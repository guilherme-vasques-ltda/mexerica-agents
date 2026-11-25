// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Minimal ACP (Agent Client Protocol) client: JSON-RPC 2.0 over a child
//! process's stdio. Updates are kept as `serde_json::Value` on purpose so a
//! newer adapter adding a variant never breaks parsing.

#[path = "fork_skill_io.rs"]
mod fork_skill_io;

use crate::LockExt;
use crate::fork::motor_exec::Child;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

/// stderr lines kept to explain a crash.
const STDERR_TAIL_LINES: usize = 12;
/// How long a stopped agent gets to exit by itself before what's left of it is killed.
const EXIT_GRACE: Duration = Duration::from_secs(10);

/// Agents [`Acp::kill`] is still stopping, for the host to wait for before it exits.
static STOPPING: std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>> = std::sync::Mutex::new(Vec::new());

pub enum Incoming {
    Notification { method: String, params: Value },
    Request { id: Value, method: String, params: Value },
    Closed { stderr_tail: String },
}

/// In-flight requests by JSON-RPC id. A std mutex: it's never held across an `.await`.
pub(crate) type Pending = Arc<std::sync::Mutex<HashMap<i64, oneshot::Sender<Result<Value, Value>>>>>;

/// A JSON-RPC error answer (downcast from `request`'s error to read the code).
#[derive(Debug)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    /// The adapter's own category (`data.errorKind`: Claude's `rate_limit`, `overloaded`, …), kept only
    /// when it's an identifier, as it can go to usage analytics.
    pub kind: Option<String>,
    /// Detalhes estruturados necessários para reconhecer limite e horário de reset.
    pub data: Value,
}

/// ACP's "authentication required".
pub const AUTH_REQUIRED: i64 = -32000;
/// JSON-RPC's "internal error".
pub const INTERNAL_ERROR: i64 = -32603;

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RpcError {}

/// Whether an agent's error asks for signing in: ACP's code, or adapters that only say so.
pub fn needs_sign_in(e: &anyhow::Error) -> bool {
    let rpc = |c: &(dyn std::error::Error + 'static)| {
        c.downcast_ref::<RpcError>()
            .is_some_and(|r| r.code == AUTH_REQUIRED || r.message.to_lowercase().contains("auth"))
    };
    e.chain().any(rpc) || e.to_string().to_lowercase().contains("auth")
}

/// Whether a failed prompt may have left its session dead in the agent: an internal error the
/// adapter gave no category. Claude's adapter answers every later prompt on a session whose query
/// stream ended with one ("The Claude Agent session has ended") until the session is closed.
pub fn may_end_session(e: &anyhow::Error) -> bool {
    e.chain().filter_map(|c| c.downcast_ref::<RpcError>()).any(|r| r.code == INTERNAL_ERROR && r.kind.is_none())
}

/// A way an agent fails that the host itself recognizes, kept as a type so usage
/// analytics can count it (`turn_finished.error`) without its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    /// No way to start the agent on this computer.
    NotSetUp,
    /// The agent stopped answering.
    Timeout,
    /// The agent process went away mid-request.
    Exited,
}

#[derive(Debug)]
pub struct Failure {
    pub kind: FailureKind,
    message: String,
}

impl Failure {
    pub fn error(kind: FailureKind, message: impl Into<String>) -> anyhow::Error {
        Self { kind, message: message.into() }.into()
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

/// A request already written to the agent, waiting for its response.
pub struct Answer {
    rx: oneshot::Receiver<std::result::Result<Value, Value>>,
}

impl Answer {
    pub async fn response(self) -> Result<Value> {
        match self.rx.await {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => {
                let msg = e["message"].as_str().map_or_else(|| e.to_string(), str::to_owned);
                let message = match e["data"]["details"].as_str().or(e["data"].as_str()) {
                    Some(d) => format!("{msg} ({d})"),
                    None => msg,
                };
                // O texto final do erro pode vir por caminho que não passou pela máscara (o
                // adaptador interno, por exemplo): mascara aqui também, já montado.
                let message = crate::fork::planos_codigo::mascarar_codigo(&message).into_owned();
                let kind = e["data"]["errorKind"]
                    .as_str()
                    .filter(|k| {
                        !k.is_empty() && k.len() <= 40 && k.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                    })
                    .map(str::to_owned);
                Err(RpcError { code: e["code"].as_i64().unwrap_or_default(), message, kind, data: e["data"].clone() }
                    .into())
            }
            // The reader drops every waiter when the process goes away.
            Err(_) => Err(Failure::error(FailureKind::Exited, "agent process exited")),
        }
    }
}

/// O adaptador interno existe só no Unix (o `Engine` é `cfg(unix)`); fora
/// dele um tipo inerte mantém o campo sem `cfg` espalhado.
#[cfg(unix)]
type MotorLink = crate::fork::motor_mexerica::link::Link;
#[cfg(not(unix))]
pub(crate) struct MotorLink;
#[cfg(not(unix))]
impl MotorLink {
    pub(crate) fn pid(&self) -> Option<u32> {
        None
    }
    pub(crate) async fn shutdown(&self) {}
}

/// Onde os quadros do cliente vão parar: o stdin do processo ACP ou o canal
/// duplex do adaptador interno (Motor Mexerica). Ambos são `AsyncWrite` de
/// JSONL — os escopos de escrita (`write_checked`, `respond_scoped`) valem
/// igual para os dois.
pub(crate) enum Writer {
    Child(ChildStdin),
    Link(tokio::io::DuplexStream),
}

/// Provas que seguram o stdin do agente olham o descritor; o adaptador
/// interno não tem descritor de cliente.
#[cfg(unix)]
impl std::os::fd::AsRawFd for Writer {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        match self {
            Writer::Child(w) => w.as_raw_fd(),
            Writer::Link(_) => -1,
        }
    }
}

impl tokio::io::AsyncWrite for Writer {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match &mut *self {
            Writer::Child(w) => std::pin::Pin::new(w).poll_write(cx, buf),
            Writer::Link(w) => std::pin::Pin::new(w).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut *self {
            Writer::Child(w) => std::pin::Pin::new(w).poll_flush(cx),
            Writer::Link(w) => std::pin::Pin::new(w).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut *self {
            Writer::Child(w) => std::pin::Pin::new(w).poll_shutdown(cx),
            Writer::Link(w) => std::pin::Pin::new(w).poll_shutdown(cx),
        }
    }
}

pub struct Acp {
    antigravity_mode: Option<Arc<std::sync::Mutex<HashSet<String>>>>,
    protected: bool,
    background: bool,
    /// `None` once stopped: the closed pipe tells the agent to exit.
    stdin: tokio::sync::Mutex<Option<Writer>>,
    /// Adaptador interno (Motor Mexerica): `kill`/`kill_and_wait` o derrubam.
    link: Option<Arc<MotorLink>>,
    scoped_cancel: tokio::sync::watch::Sender<()>,
    #[cfg(test)]
    scoped_entered: tokio::sync::Notify,
    #[cfg(test)]
    scoped_pending: tokio::sync::Notify,
    #[cfg(test)]
    stopped: tokio::sync::Notify,
    next_id: AtomicI64,
    pending: Pending,
    /// `None` once stopped (handed to [`reap`]).
    child: tokio::sync::Mutex<Option<Child>>,
    /// Sessão do plano de código que este processo usa. Só é solta depois de a saída do processo ser observada: o
    /// plano (e a chave) não fica livre enquanto o OpenCode antigo ainda roda.
    plano: tokio::sync::Mutex<Option<crate::fork::planos_codigo::Concessao>>,
    /// The process (and, on Unix, process group) leader.
    pid: Option<u32>,
}

impl Acp {
    /// Spawns `command` through the shell so user-provided commands (npx, env vars) just work.
    /// `env` stays out of the command line (it can hold API keys).
    pub fn spawn(
        command: &str,
        cwd: &str,
        env: &[(String, String)],
    ) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<Incoming>)> {
        Self::spawn_mode(command, cwd, env, false, false)
    }

    /// Descoberta de setup: saída nativa não vai a logs nem à cauda de erro.
    pub(crate) fn spawn_protected(
        command: &str,
        cwd: &str,
        env: &[(String, String)],
    ) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<Incoming>)> {
        Self::spawn_mode(command, cwd, env, true, false)
    }

    pub(crate) fn spawn_background(
        command: &str,
        cwd: &str,
        env: &[(String, String)],
    ) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<Incoming>)> {
        Self::spawn_mode(command, cwd, env, false, true)
    }

    fn spawn_mode(
        command: &str,
        cwd: &str,
        env: &[(String, String)],
        protected: bool,
        background: bool,
    ) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<Incoming>)> {
        let shell = crate::shell::exec(command);
        let mut process = if crate::fork::motor_exec::configured() {
            let mut process = crate::fork::motor_exec::command(shell.program, std::path::Path::new(cwd));
            process.args(&shell.args);
            process
        } else {
            shell.tokio()
        };
        process
            .env_clear()
            .envs(super::environment::inherited(std::env::vars_os()))
            .envs(shell.env)
            .envs(env.iter().map(|(k, v)| (k, v)));
        Self::spawn_process(process, command, cwd, protected, background, false)
    }

    pub(crate) fn spawn_antigravity(
        cli: &crate::fork::antigravity::Official,
        cwd: &str,
    ) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<Incoming>)> {
        let mut process = Command::new(cli.path());
        process.arg("acp").env_clear().envs(std::env::vars_os().filter(|(name, _)| {
            name.to_str().is_some_and(|name| {
                [
                    "HOME",
                    "USERPROFILE",
                    "PATH",
                    "SYSTEMROOT",
                    "LANG",
                    "LC_ALL",
                    "DBUS_SESSION_BUS_ADDRESS",
                    "DISPLAY",
                    "WAYLAND_DISPLAY",
                    "XDG_RUNTIME_DIR",
                ]
                .iter()
                .any(|key| name.eq_ignore_ascii_case(key))
            })
        }));
        Self::spawn_process(process, "agy acp", cwd, true, false, true)
    }

    fn spawn_process(
        mut process: Command,
        command: &str,
        cwd: &str,
        protected: bool,
        background: bool,
        antigravity: bool,
    ) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<Incoming>)> {
        process
            .current_dir(crate::fork::motor_exec::hub_cwd(std::path::Path::new(cwd)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // Its own group, so `kill` can stop everything it started.
        #[cfg(unix)]
        process.process_group(0);
        let mut child =
            crate::fork::motor_exec::spawn(&mut process).with_context(|| format!("starting `{command}`"))?;
        let stdin = Writer::Child(child.stdin.take().expect("stdin is piped"));
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        // Deliberately unbounded: `session/load` replays a whole history as notifications
        // *before* its response, while the bot is still awaiting that response. A bounded
        // queue would stall this reader, and with it the response — a deadlock.
        let (tx, rx) = mpsc::unbounded_channel();
        let pending = Pending::default();
        let antigravity_mode = antigravity.then(|| Arc::new(std::sync::Mutex::new(HashSet::new())));

        let tail = Arc::new(std::sync::Mutex::new(VecDeque::<String>::with_capacity(STDERR_TAIL_LINES)));
        {
            let tail = tail.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if protected || background {
                        continue;
                    }
                    let line = crate::fork::planos_codigo::mascarar_codigo(&line).into_owned();
                    tracing::debug!(target: "agent", stderr = %line);
                    let mut t = tail.locked();
                    if t.len() == STDERR_TAIL_LINES {
                        t.pop_front();
                    }
                    t.push_back(line);
                }
            });
        }
        {
            let pending = pending.clone();
            let antigravity_mode = antigravity_mode.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    // O parse vem primeiro, na linha original: mascarar antes faria dois nomes de campo literais
                    // distintos colidirem no texto bruto. Os textos já interpretados (escapes desfeitos) são
                    // mascarados a seguir — é aqui que um código escrito em escapes de JSON aparece.
                    let Ok(mut msg) = serde_json::from_str::<Value>(&line) else {
                        if !protected && !background {
                            let line = crate::fork::planos_codigo::mascarar_codigo(&line);
                            tracing::debug!(target: "agent", stdout = %line, "ignoring non-JSON output");
                        }
                        continue;
                    };
                    crate::fork::planos_codigo::mascarar_json(&mut msg);
                    let method = msg["method"].as_str().map(str::to_owned);
                    match (method, msg.get("id").cloned()) {
                        (Some(method), Some(id)) if !id.is_null() => {
                            let _ = tx.send(Incoming::Request { id, method, params: msg["params"].clone() });
                        }
                        (Some(method), _) => {
                            if let Some(modes) = &antigravity_mode
                                && method == "session/update"
                                && let Some(sid) = msg["params"]["sessionId"].as_str()
                                && crate::fork::antigravity::mode_diverged(&msg["params"]["update"])
                            {
                                modes.locked().insert(sid.to_owned());
                            }
                            let _ = tx.send(Incoming::Notification { method, params: msg["params"].clone() });
                        }
                        (None, Some(id)) => {
                            if let Some(id) = id.as_i64()
                                && let Some(waiter) = pending.locked().remove(&id)
                            {
                                let res = match msg.get("error") {
                                    Some(err) => Err(err.clone()),
                                    None => Ok(msg["result"].clone()),
                                };
                                let _ = waiter.send(res);
                            }
                        }
                        _ => {}
                    }
                }
                // Process gone: dropping the waiters fails them as `FailureKind::Exited`.
                pending.locked().clear();
                let stderr_tail = tail.locked().iter().cloned().collect::<Vec<_>>().join("\n");
                let _ = tx.send(Incoming::Closed { stderr_tail });
            });
        }

        let pid = child.id();
        let acp = Arc::new(Self {
            antigravity_mode,
            protected,
            background,
            stdin: tokio::sync::Mutex::new(Some(stdin)),
            link: None,
            scoped_cancel: tokio::sync::watch::channel(()).0,
            #[cfg(test)]
            scoped_entered: tokio::sync::Notify::new(),
            #[cfg(test)]
            scoped_pending: tokio::sync::Notify::new(),
            #[cfg(test)]
            stopped: tokio::sync::Notify::new(),
            next_id: AtomicI64::new(1),
            pending,
            child: tokio::sync::Mutex::new(Some(child)),
            plano: tokio::sync::Mutex::new(None),
            pid,
        });
        Ok((acp, rx))
    }

    /// Adaptador interno no lugar do processo ACP: o bot fala o mesmo ACP
    /// (mesmos métodos, mesma fila `Incoming`), e cada chamada vira uma
    /// operação no `Engine` do Motor Mexerica dentro do Hub.
    #[cfg(unix)]
    pub(crate) fn spawn_link(link: Arc<MotorLink>) -> (Arc<Self>, mpsc::UnboundedReceiver<Incoming>) {
        let (bot_side, adapter_side) = tokio::io::duplex(64 * 1024);
        let (tx, rx) = mpsc::unbounded_channel();
        let pending = Pending::default();
        link.attach(adapter_side, tx, pending.clone());
        let acp = Arc::new(Self {
            antigravity_mode: None,
            protected: false,
            background: false,
            stdin: tokio::sync::Mutex::new(Some(Writer::Link(bot_side))),
            link: Some(link),
            scoped_cancel: tokio::sync::watch::channel(()).0,
            #[cfg(test)]
            scoped_entered: tokio::sync::Notify::new(),
            #[cfg(test)]
            scoped_pending: tokio::sync::Notify::new(),
            #[cfg(test)]
            stopped: tokio::sync::Notify::new(),
            next_id: AtomicI64::new(1),
            pending,
            child: tokio::sync::Mutex::new(None),
            plano: tokio::sync::Mutex::new(None),
            pid: None,
        });
        (acp, rx)
    }

    /// Entrega a sessão do plano de código ao processo: ela vive até a saída dele ser observada.
    pub(crate) async fn entregar_plano(&self, concessao: crate::fork::planos_codigo::Concessao) {
        *self.plano.lock().await = Some(concessao);
    }

    /// Este processo usa um plano de código?
    pub(crate) async fn tem_plano(&self) -> bool {
        self.plano.lock().await.is_some()
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid.or_else(|| self.link.as_ref().and_then(|link| link.pid()))
    }

    async fn write(&self, msg: Value) -> Result<()> {
        let mut line = serde_json::to_vec(&msg)?;
        line.push(b'\n');
        let mut stdin = self.stdin.lock().await;
        let stdin = stdin.as_mut().ok_or_else(|| Failure::error(FailureKind::Exited, "agent stopped"))?;
        stdin.write_all(&line).await?;
        stdin.flush().await?;
        Ok(())
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.send(method, params).await?.response().await
    }

    /// Writes a request and returns its pending answer: anything written after this
    /// (a `session/cancel`) reaches the agent after the request itself.
    pub async fn send(&self, method: &str, params: Value) -> Result<Answer> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.locked().insert(id, tx);
        self.write(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).await?;
        Ok(Answer { rx })
    }

    pub(crate) async fn send_guarded_prompt(
        &self,
        params: Value,
        scope: impl Fn(&mut dyn FnMut() -> std::task::Poll<Result<()>>) -> std::task::Poll<Result<()>>,
    ) -> Result<Answer> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut line = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":"session/prompt","params":params}))?;
        line.push(b'\n');
        let mut input = self.stdin.lock().await;
        let writer = input.as_mut().ok_or_else(|| Failure::error(FailureKind::Exited, "agent stopped"))?;
        let (tx, rx) = oneshot::channel();
        self.pending.locked().insert(id, tx);
        let mut started = false;
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = loop {
            let result =
                crate::fork::spaces::tool_effects::write_scoped_progress(writer, &line, &scope, &mut started).await;
            if started
                || Instant::now() >= deadline
                || !result.as_ref().is_err_and(crate::fork::autoridade::sqlite_temporariamente_ocupado)
            {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        if result.is_err() {
            self.pending.locked().remove(&id);
            if started {
                input.take();
            }
        }
        result.map(|()| Answer { rx })
    }

    pub(crate) async fn send_antigravity_prompt(
        &self,
        params: Value,
        scope: impl Fn(&mut dyn FnMut() -> std::task::Poll<Result<()>>) -> std::task::Poll<Result<()>>,
    ) -> Result<Answer> {
        let sid = params["sessionId"].as_str().context("Sessão ACP ausente.")?.to_owned();
        let modes = self.antigravity_mode.as_ref().context("Transporte oficial ACP ausente.")?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.locked().insert(id, tx);
        let mut line = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":"session/prompt","params":params}))?;
        line.push(b'\n');
        let mut input = self.stdin.lock().await;
        let result = if let Some(writer) = input.as_mut() {
            crate::fork::spaces::tool_effects::write_scoped(writer, &line, |effect| {
                let modes = modes.locked();
                if modes.contains(&sid) {
                    return std::task::Poll::Ready(Err(anyhow::anyhow!(
                        "O Antigravity mudou de modo antes do envio. Reenvie a mensagem."
                    )));
                }
                scope(effect)
            })
            .await
        } else {
            Err(Failure::error(FailureKind::Exited, "agent stopped"))
        };
        if result.is_err() {
            input.take();
            self.pending.locked().remove(&id);
        }
        result.map(|()| Answer { rx })
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.write(json!({"jsonrpc": "2.0", "method": method, "params": params})).await
    }

    pub async fn respond(&self, id: Value, result: Value) -> Result<()> {
        self.write(json!({"jsonrpc": "2.0", "id": id, "result": result})).await
    }

    /// Permissão contextual: revogação e fechamento do quadro têm o mesmo commit síncrono.
    pub(crate) async fn respond_checked(
        &self,
        id: Value,
        result: Value,
        boundary: &std::sync::Mutex<()>,
        check: impl Fn() -> Result<()>,
    ) -> Result<()> {
        self.write_checked(json!({"jsonrpc":"2.0", "id":id, "result":result}), boundary, check).await
    }

    async fn write_checked(
        &self,
        frame: Value,
        boundary: &std::sync::Mutex<()>,
        check: impl Fn() -> Result<()>,
    ) -> Result<()> {
        let mut line = serde_json::to_vec(&frame)?;
        line.push(b'\n');
        let mut input = self.stdin.lock().await;
        let writer = input.as_mut().ok_or_else(|| Failure::error(FailureKind::Exited, "agent stopped"))?;
        let result = crate::fork::spaces::tool_effects::write_checked(writer, &line, boundary, check).await;
        if result.is_err() {
            // Prefixo sem fechamento não concede, mesmo no EOF. Não o retomar.
            input.take();
        }
        result
    }

    #[cfg(test)]
    pub(crate) async fn hold_input(&self) -> tokio::sync::MutexGuard<'_, Option<Writer>> {
        self.stdin.lock().await
    }

    pub async fn respond_error(&self, id: Value, code: i64, message: &str) -> Result<()> {
        self.write(json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})).await
    }

    /// Encerra o processo e seus filhos antes de liberar recursos privados do job.
    pub async fn kill_and_wait(&self) {
        self.stdin.lock().await.take();
        if let Some(link) = &self.link {
            link.shutdown().await;
        }
        let child = self.child.lock().await.take();
        if let Some(child) = child {
            if self.protected {
                crate::fork::engine_setup::kill_setup_group(child.id());
            }
            Box::pin(reap(child)).await;
            drop(self.plano.lock().await.take());
        }
    }

    /// Stops the agent and everything it started, without waiting for it. Closing its stdin
    /// lets it exit by itself first: Claude Code killed during an OAuth refresh loses the new
    /// refresh token, and the user's login with it.
    pub async fn kill(&self) {
        self.stdin.lock().await.take();
        if let Some(link) = &self.link {
            let link = link.clone();
            tokio::spawn(async move {
                link.shutdown().await;
            });
        }
        let child = self.child.lock().await.take();
        if let Some(child) = child {
            #[cfg(test)]
            self.stopped.notify_one();
            if self.protected {
                crate::fork::engine_setup::kill_setup_group(child.id());
            }
            // Processo de um plano de código: quem é dono do processo observa a saída antes de soltar o plano, então
            // trocar ou remover a chave nunca acontece com o OpenCode antigo ainda rodando. Não guarda PID.
            let plano = self.plano.lock().await.take();
            if let Some(plano) = plano {
                // Caixa para o futuro de `kill` não crescer com a espera: o encerramento do plano vive no heap.
                Box::pin(async move {
                    reap(child).await;
                    drop(plano);
                })
                .await;
                return;
            }
            let mut stopping = STOPPING.locked();
            stopping.retain(|task| !task.is_finished());
            stopping.push(tokio::spawn(reap(child)));
        }
    }
}

impl Drop for Acp {
    fn drop(&mut self) {
        let plano = self.plano.get_mut().take();
        if self.protected
            && let Some(child) = self.child.get_mut().as_ref()
        {
            crate::fork::engine_setup::kill_setup_group(child.id());
        } else if (self.background || plano.is_some())
            && let Some(child) = self.child.get_mut().take()
        {
            self.stdin.get_mut().take();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                let mut stopping = STOPPING.locked();
                stopping.retain(|task| !task.is_finished());
                stopping.push(runtime.spawn(async move {
                    reap(child).await;
                    drop(plano);
                }));
            } else {
                crate::fork::engine_setup::kill_setup_group(child.id());
            }
        }
    }
}

/// Waits until every agent being stopped has exited or been killed (at most [`EXIT_GRACE`]).
pub async fn stopped() {
    let tasks = std::mem::take(&mut *STOPPING.locked());
    futures::future::join_all(tasks).await;
}

/// Gives the agent [`EXIT_GRACE`] to exit with everything it started (ACP adapters spawn
/// CLIs, MCP servers and tools), then kills its process group (Windows: process tree).
async fn reap(mut child: Child) {
    let Some(pid) = child.id() else { return };
    let deadline = Instant::now() + EXIT_GRACE;
    let exited = tokio::time::timeout_at(deadline, child.wait()).await.is_ok();
    if !exited || outlived(pid, deadline).await {
        let mut tree = if cfg!(windows) {
            let mut c = Command::new("taskkill");
            c.args(["/T", "/F", "/PID", &pid.to_string()]);
            c
        } else {
            let mut c = if crate::fork::motor_exec::configured() {
                let mut c = Command::new(crate::fork::motor_exec::helper_path());
                c.args(["kill-group", &pid.to_string()]);
                c
            } else {
                let mut c = Command::new("/bin/kill");
                c.args(["-KILL", "--", &format!("-{pid}")]);
                c
            };
            c.env_clear();
            c
        };
        let _ = tree.stdout(Stdio::null()).stderr(Stdio::null()).status().await;
    }
    // Already exited is fine; anything else is worth a log line.
    if let Err(error) = child.kill().await
        && error.kind() != std::io::ErrorKind::InvalidInput
    {
        tracing::debug!(%error, "couldn't kill agent process");
    }
}

/// Whether part of the exited agent's process group is still running at `deadline`.
#[cfg(unix)]
async fn outlived(pid: u32, deadline: Instant) -> bool {
    let Ok(group) = libc::pid_t::try_from(pid) else { return true };
    // SAFETY: signal 0 sends nothing; it only checks that the group still has a process.
    let running = || {
        // SAFETY: sinal 0 apenas consulta o grupo. EPERM também prova que existe.
        (unsafe { libc::killpg(group, 0) }) == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    };
    while running() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    running()
}

/// Windows has no process groups: what the agent started got the same closed pipes, and its
/// tree can't be found once it's gone.
#[cfg(windows)]
#[allow(clippy::unused_async)] // the Unix check's signature
async fn outlived(_pid: u32, _deadline: Instant) -> bool {
    false
}

/// Plain-text rendering of an ACP content block list / tool-call content list.
pub fn content_text(v: &Value) -> String {
    match v {
        Value::Array(items) => items.iter().map(content_text).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n"),
        Value::Object(o) => match o.get("type").and_then(Value::as_str) {
            Some("text") => o.get("text").and_then(Value::as_str).unwrap_or_default().to_owned(),
            Some("content") => o.get("content").map(content_text).unwrap_or_default(),
            Some("resource_link") => o.get("uri").and_then(Value::as_str).unwrap_or_default().to_owned(),
            Some("resource") => {
                o.get("resource").and_then(|r| r.get("text")).and_then(Value::as_str).unwrap_or_default().to_owned()
            }
            _ => String::new(),
        },
        _ => String::new(),
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn guarded_prompt_rechecks_after_sqlite_contention_before_any_bytes() {
        use std::{cell::Cell, task::Poll};
        for cut in [false, true] {
            let script = r#"if IFS= read -r line; then printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"stopReason":"end_turn"}}'; fi"#;
            let mut process = Command::new("/bin/sh");
            process.args(["-c", script]);
            let (acp, _updates) = Acp::spawn_process(process, "fixture", "/tmp", true, false, false).unwrap();
            let calls = Cell::new(0);
            let result = acp
                .send_guarded_prompt(json!({"sessionId":"fixture","prompt":[]}), |effect| {
                    let attempt = calls.get();
                    calls.set(attempt + 1);
                    if attempt == 0 {
                        return Poll::Ready(Err(rusqlite::Error::SqliteFailure(
                            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                            None,
                        )
                        .into()));
                    }
                    if cut {
                        return Poll::Ready(Err(crate::fork::autoridade::EpochDaRequisicaoEncerrado.into()));
                    }
                    effect()
                })
                .await;
            if cut {
                assert!(result.err().unwrap().is::<crate::fork::autoridade::EpochDaRequisicaoEncerrado>());
                assert_eq!(calls.get(), 2);
            } else {
                let answer =
                    tokio::time::timeout(Duration::from_secs(3), result.unwrap().response()).await.unwrap().unwrap();
                assert_eq!(answer["stopReason"], "end_turn");
                assert!(calls.get() > 1);
            }
            acp.kill().await;
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn antigravity_modo_observado_bloqueia_escrita_mesmo_sem_drenar_notificacao() {
        let script = r#"printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fixture","update":{"sessionUpdate":"current_mode_update","currentModeId":"auto"}}}'; cat"#;
        let mut process = Command::new("/bin/sh");
        process.args(["-c", script]);
        let (acp, mut updates) = Acp::spawn_process(process, "fixture", "/tmp", true, false, true).unwrap();
        let notification = tokio::time::timeout(Duration::from_secs(5), updates.recv()).await.unwrap();
        assert!(matches!(notification, Some(Incoming::Notification { .. })));
        let result = acp
            .send_antigravity_prompt(json!({"sessionId":"fixture","prompt":[]}), |_| {
                panic!("escrita após divergência observada")
            })
            .await;
        assert!(result.is_err());
        acp.kill().await;
    }

    #[test]
    fn content_text_flattens_blocks() {
        let v = json!([
            {"type": "content", "content": {"type": "text", "text": "a"}},
            {"type": "diff", "path": "/x"},
            {"type": "text", "text": "b"}
        ]);
        assert_eq!(content_text(&v), "a\nb");
        assert_eq!(truncate("héllo", 2), "h…");
    }

    #[tokio::test]
    async fn an_error_keeps_the_adapters_category_only_when_it_is_an_identifier() {
        let kind = |data: Value| async move {
            let (tx, rx) = oneshot::channel();
            let _ = tx.send(Err(json!({"code": -32603, "message": "Internal error", "data": data})));
            let e = Answer { rx }.response().await.expect_err("an error answer");
            e.downcast::<RpcError>().expect("an RpcError").kind
        };
        assert_eq!(kind(json!({"errorKind": "rate_limit"})).await.as_deref(), Some("rate_limit"));
        assert_eq!(kind(json!({"errorKind": "Usage: /Users/me"})).await, None);
        assert_eq!(kind(json!({"errorKind": ""})).await, None);
        assert_eq!(kind(json!("details")).await, None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fundo_cancelado_fecha_entrada_e_recolhe_processo() {
        let dir = std::env::temp_dir().join(format!("donnabot-fundo-reap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let marker = dir.join("encerrou");
        let script =
            format!("import sys,pathlib; sys.stdin.read(); pathlib.Path({:?}).touch()", marker.to_string_lossy());
        let command = format!("python3 -u -c {}", crate::shell::quote(&script));
        let (acp, _rx) = Acp::spawn_background(&command, dir.to_str().unwrap(), &[]).unwrap();
        drop(acp);
        stopped().await;
        assert!(marker.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn encerramento_aguarda_o_processo_antes_de_liberar_recursos() {
        let dir = std::env::temp_dir().join(format!("donnabot-reap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let marker = dir.join("encerrado");
        let script = r"import json, os, sys
r = json.loads(sys.stdin.readline())
print(json.dumps({'jsonrpc':'2.0', 'id':r['id'], 'result':{}}), flush=True)
sys.stdin.read()
open(os.environ['TEST_MARKER'], 'w').write('encerrado')
";
        let command = format!("python3 -u -c {}", crate::shell::quote(script));
        let env = vec![("TEST_MARKER".into(), marker.to_string_lossy().into_owned())];
        let (acp, _rx) = Acp::spawn(&command, "/tmp", &env).unwrap();
        acp.request("initialize", json!({})).await.unwrap();
        acp.kill_and_wait().await;
        assert_eq!(std::fs::read_to_string(marker).unwrap(), "encerrado");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn spawn_nao_repassa_segredos_do_hub_ao_motor() {
        const FIXTURE: &str = "DONNABOT_TEST_HUB_ENV";
        if std::env::var_os(FIXTURE).is_none() {
            // Um Hub fictício em outro processo: não muda o ambiente global dos testes paralelos.
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "agent::acp::tests::spawn_nao_repassa_segredos_do_hub_ao_motor"])
                .env(FIXTURE, "1")
                .env("ANTHROPIC_API_KEY", "chave-ficticia-do-hub")
                .env("OPENAI_API_KEY", "outra-chave-ficticia-do-hub")
                .env("OP_SERVICE_ACCOUNT_TOKEN", "cofre-ficticio")
                .env("GH_TOKEN", "github-ficticio")
                .env("INITIAL_AGENT_MODE", "agent-full-access")
                .env("CODEX_PATH", "/launcher-inseguro")
                .output()
                .await
                .unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
            return;
        }
        let script = r"import json, os, sys
r = json.loads(sys.stdin.readline())
names = ['ANTHROPIC_API_KEY', 'OP_SERVICE_ACCOUNT_TOKEN', 'GH_TOKEN', 'CODEX_PATH']
result = {name: name in os.environ for name in names}
result['ownKey'] = os.environ.get('OPENAI_API_KEY') == 'chave-propria-ficticia'
result['mode'] = os.environ.get('INITIAL_AGENT_MODE')
result['home'] = bool(os.environ.get('HOME'))
print(json.dumps({'jsonrpc': '2.0', 'id': r['id'], 'result': result}), flush=True)
sys.stdin.read()
";
        let command = format!("python3 -u -c {}", crate::shell::quote(script));
        let mut env = vec![("OPENAI_API_KEY".into(), "chave-propria-ficticia".into())];
        super::super::harness::apply(&mut env, "codex", "job");
        let (acp, _rx) = Acp::spawn(&command, "/tmp", &env).unwrap();
        let result =
            tokio::time::timeout(Duration::from_secs(10), acp.request("initialize", json!({}))).await.unwrap().unwrap();
        for name in ["ANTHROPIC_API_KEY", "OP_SERVICE_ACCOUNT_TOKEN", "GH_TOKEN", "CODEX_PATH"] {
            assert_eq!(result[name], false, "credencial herdada: {name}");
        }
        assert_eq!(result["ownKey"], true);
        assert_eq!(result["mode"], "workspace-write");
        assert_eq!(result["home"], true, "o login por assinatura continua na pasta do Hub");
        acp.kill().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stopping_an_agent_closes_its_descendants_pipes() {
        let (acp, mut rx) = Acp::spawn(
            r#"sh -c 'sleep 60 & echo "{\"jsonrpc\":\"2.0\",\"method\":\"ready\"}"; read line'"#,
            "/tmp",
            &[],
        )
        .unwrap();
        let ready = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await.unwrap();
        assert!(matches!(ready, Some(Incoming::Notification { .. })));
        acp.kill().await;
        // Killing only the shell leaves sleep holding stdout for 60 seconds.
        let closed = tokio::time::timeout(EXIT_GRACE + Duration::from_secs(3), rx.recv()).await.unwrap();
        assert!(matches!(closed, Some(Incoming::Closed { .. })));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_stopped_agent_finishes_by_itself_first() {
        // The adapter exits on its closed stdin; the CLI it started saves its login a second later.
        let (acp, mut rx) = Acp::spawn(
            r#"sh -c 'read line; (sleep 1; echo "{\"jsonrpc\":\"2.0\",\"method\":\"saved\"}") &'"#,
            "/tmp",
            &[],
        )
        .unwrap();
        acp.kill().await;
        let saved = tokio::time::timeout(EXIT_GRACE, rx.recv()).await.unwrap();
        assert!(matches!(saved, Some(Incoming::Notification { method, .. }) if method == "saved"));
        assert!(acp.notify("x", json!({})).await.is_err(), "a stopped agent takes no more messages");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn request_response_roundtrip() {
        // A fake agent: answers `initialize`, then sends a notification and exits.
        let script = r#"read l; id=$(echo "$l" | sed 's/.*"id":\([0-9]*\).*/\1/'); echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"ok\":true}}"; echo '{"jsonrpc":"2.0","method":"session/update","params":{"x":1}}'"#;
        let (acp, mut rx) = Acp::spawn(&format!("sh -c '{}'", script.replace('\'', "'\\''")), "/tmp", &[]).unwrap();
        let res = acp.request("initialize", json!({})).await.unwrap();
        assert_eq!(res["ok"], true);
        match rx.recv().await.unwrap() {
            Incoming::Notification { method, .. } => assert_eq!(method, "session/update"),
            _ => panic!("expected notification"),
        }
        assert!(matches!(rx.recv().await.unwrap(), Incoming::Closed { .. }));
    }
}
