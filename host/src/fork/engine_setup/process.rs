//! Processos de setup: argv fechado, grupo próprio, limites e nenhuma shell.
use super::transport::{Challenge, Presentation};
use crate::fork::engine_accounts::{Backend, LaunchReference, probe};
use crate::fork::motor_exec::Child;
use anyhow::{Result, anyhow, bail, ensure};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    path::Path,
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout, Command},
    sync::{mpsc, watch},
};

pub(crate) fn command(executable: &Path, directory: &Path, backend: Backend) -> Command {
    let mut command = crate::fork::motor_exec::command(executable, directory);
    command
        .env_clear()
        .envs(environment(directory, backend))
        .current_dir(crate::fork::motor_exec::hub_cwd(directory))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        if !crate::fork::motor_install::worker() {
            command.process_group(0);
        }
        // SAFETY: só altera a máscara no filho antes do exec, sem acessar estado compartilhado.
        unsafe {
            command.pre_exec(|| {
                libc::umask(0o077);
                Ok(())
            });
        }
    }
    command
}

pub(crate) fn environment(directory: &Path, backend: Backend) -> Vec<(String, String)> {
    // Nenhum proxy, endpoint, seletor de perfil, hook ou token herdado no login.
    let mut env = Vec::new();
    if crate::fork::motor_exec::configured() {
        env.push(("DONNABOT_MOTOR_PRIVATE".into(), "1".into()));
    }
    for key in ["PATH", "LANG", "LC_ALL", "SYSTEMROOT", "WINDIR"] {
        if let Ok(value) = std::env::var(key) {
            env.push((key.into(), value));
        }
    }
    env.extend([
        ("HOME".into(), directory.parent().unwrap_or(directory).to_string_lossy().into_owned()),
        (backend.profile_variable().into(), directory.to_string_lossy().into_owned()),
        ("DISABLE_AUTOUPDATER".into(), "1".into()),
        ("GROK_DISABLE_AUTOUPDATER".into(), "1".into()),
        ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".into(), "1".into()),
        ("BROWSER".into(), "/usr/bin/false".into()),
        ("TERM".into(), "dumb".into()),
    ]);
    env
}

pub(crate) enum Control {
    Code(String),
    Resize(u16, u16),
}
pub(crate) enum Event {
    Challenge(Challenge),
    Verifying,
}

pub(crate) struct Rpc {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    pending: VecDeque<Value>,
}
impl Drop for Rpc {
    fn drop(&mut self) {
        kill_group(self.child.id());
    }
}
impl Rpc {
    pub(crate) fn start(executable: &Path, directory: &Path) -> Result<Self> {
        let cmd = command(executable, directory, Backend::Codex);
        Self::spawn(cmd)
    }
    /// O painel global conserva o ambiente filtrado e o perfil global existentes.
    /// O setup/uso individual continua usando `start`, sem compartilhar esta seleção.
    pub(crate) fn start_usage(
        executable: &Path,
        home: &Path,
        env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    ) -> Result<Self> {
        let mut cmd = command(executable, home, Backend::Codex);
        cmd.env_clear().envs(env);
        Self::spawn(cmd)
    }
    fn spawn(mut cmd: Command) -> Result<Self> {
        let child = crate::fork::motor_exec::spawn(cmd.args(["app-server"]).stdin(Stdio::piped()))?;
        Self::from_child(child)
    }
    fn from_child(mut child: Child) -> Result<Self> {
        let input = child.stdin.take().ok_or_else(|| anyhow!("setup_unavailable"))?;
        let output = BufReader::new(child.stdout.take().ok_or_else(|| anyhow!("setup_unavailable"))?);
        Ok(Self { child, input: Some(input), output, pending: VecDeque::new() })
    }
    pub(crate) async fn write(&mut self, value: Value) -> Result<()> {
        let input = self.input.as_mut().ok_or_else(|| anyhow!("auth_shutdown_failed"))?;
        input.write_all(format!("{value}\n").as_bytes()).await?;
        input.flush().await?;
        Ok(())
    }
    async fn finish(&mut self) -> Result<()> {
        // Codex removes its own arg0 helper symlinks during normal EOF shutdown.
        // Reap before validating/promoting the profile; Drop still kills on error/cancel.
        drop(self.input.take());
        let status = tokio::time::timeout(Duration::from_secs(5), self.child.wait())
            .await
            .map_err(|_| anyhow!("auth_shutdown_failed"))?
            .map_err(|_| anyhow!("auth_shutdown_failed"))?;
        ensure!(status.success(), "auth_shutdown_failed");
        Ok(())
    }
    async fn line(&mut self) -> Result<Value> {
        use tokio::io::AsyncReadExt;
        let mut bytes = Vec::new();
        let count = (&mut self.output).take(65_537).read_until(b'\n', &mut bytes).await?;
        ensure!(count > 0 && count <= 65_536, "auth_protocol_invalid");
        Ok(serde_json::from_slice(&bytes)?)
    }
    pub(crate) async fn next(&mut self) -> Result<Value> {
        if let Some(value) = self.pending.pop_front() {
            return Ok(value);
        }
        self.line().await
    }
    pub(crate) async fn call(&mut self, id: u64, method: &str, params: Value) -> Result<Value> {
        self.write(json!({"id":id,"method":method,"params":params})).await?;
        loop {
            let value = self.line().await?;
            if value["id"] == id {
                ensure!(value.get("error").is_none(), "auth_capability_unavailable");
                return Ok(value["result"].clone());
            }
            if value.get("method").is_some() && value.get("id").is_some() {
                self.write(json!({"id":value["id"],"error":{"code":-32601,"message":"setup only"}})).await?;
            } else {
                ensure!(self.pending.len() < 64, "auth_protocol_limit");
                self.pending.push_back(value);
            }
        }
    }
    pub(crate) async fn initialize(&mut self) -> Result<()> {
        self.call(1, "initialize", json!({"clientInfo":{"name":"donnabot-setup","version":"1"}})).await?;
        self.write(json!({"method":"initialized"})).await
    }
    pub(crate) async fn proof(&mut self, executable: &Path, directory: &Path) -> Result<probe::Observation> {
        let mut observation = probe::Observation::cli(Backend::Codex, executable);
        observation.launch = LaunchReference::CliProfile { executable: executable.into(), directory: directory.into() };
        probe::codex_account(&mut observation, &self.call(3, "account/read", json!({"refreshToken":false})).await?);
        if observation.identity.is_some() {
            observation.catalog_observed = true;
            observation.models =
                probe::codex_models(&self.call(4, "model/list", json!({"includeHidden":false})).await?);
        }
        Ok(observation)
    }
}

pub(crate) fn kill_group(pid: Option<u32>) {
    // Filhos do instalador herdam o grupo externo, encerrado pelo Hub no cancelamento.
    if crate::fork::motor_install::worker() {
        return;
    }
    if crate::fork::motor_install::orchestrating() {
        if let Some(pid) = pid
            && let Err(error) = crate::fork::motor_exec::kill_group(pid)
        {
            tracing::warn!(%error, "falha ao encerrar grupo do motor");
        }
        return;
    }
    #[cfg(unix)]
    if let Some(pid) = pid.and_then(|pid| i32::try_from(pid).ok()) {
        // SAFETY: PID é exclusivamente o grupo criado por este módulo, nunca vindo do cliente.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    let _ = pid;
}

pub(crate) async fn login(
    backend: Backend,
    executable: &Path,
    directory: &Path,
    events: mpsc::Sender<Event>,
    input: mpsc::Receiver<Control>,
    mut cancel: watch::Receiver<bool>,
) -> Result<probe::Observation> {
    let flow = async {
        match backend {
            Backend::Codex => codex(executable, directory, events).await,
            Backend::Claude => claude(executable, directory, events, input).await,
            Backend::Grok => super::grok::login(executable, directory, events).await,
        }
    };
    tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(600), flow) => result.map_err(|_| anyhow!("setup_expired"))?,
        _ = cancel.changed() => bail!("setup_cancelled"),
    }
}

async fn codex(executable: &Path, directory: &Path, events: mpsc::Sender<Event>) -> Result<probe::Observation> {
    let rpc = Rpc::start(executable, directory)?;
    codex_device_flow(rpc, executable, directory, events).await
}

#[cfg(unix)]
pub(crate) async fn login_open_codex(
    directory: &Path,
    events: mpsc::Sender<Event>,
) -> Result<(std::path::PathBuf, probe::Observation)> {
    let launch = crate::fork::open_codex::app_server(directory).await?;
    let executable = launch.executable.clone();
    let child = crate::fork::motor_exec::start_open_codex(launch).map_err(|_| anyhow!("open_codex_unavailable"))?;
    let proof = codex_device_flow(Rpc::from_child(child)?, &executable, directory, events).await?;
    crate::fork::open_codex::confirm_profile(directory).map_err(|_| anyhow!("open_codex_unavailable"))?;
    Ok((executable, proof))
}

async fn codex_device_flow(
    mut rpc: Rpc,
    executable: &Path,
    directory: &Path,
    events: mpsc::Sender<Event>,
) -> Result<probe::Observation> {
    rpc.initialize().await?;
    let started = rpc.call(2, "account/login/start", json!({"type":"chatgptDeviceCode"})).await?;
    ensure!(started["type"] == "chatgptDeviceCode", "auth_capability_unavailable");
    let login = started["loginId"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 100)
        .ok_or_else(|| anyhow!("auth_protocol_invalid"))?
        .to_owned();
    let challenge = super::transport::challenge(
        Backend::Codex,
        "device_code",
        started["verificationUrl"].as_str().ok_or_else(|| anyhow!("auth_url_invalid"))?,
        started["userCode"].as_str(),
    )?;
    ensure!(challenge.code.is_some(), "auth_protocol_invalid");
    events.send(Event::Challenge(challenge)).await?;
    loop {
        let value = rpc.next().await?;
        if value["method"] == "account/login/completed" {
            ensure!(value["params"]["loginId"] == login, "auth_correlation_invalid");
            ensure!(value["params"]["success"] == true, "auth_failed");
            break;
        }
    }
    events.send(Event::Verifying).await?;
    let proof = rpc.proof(executable, directory).await?;
    rpc.finish().await?;
    Ok(proof)
}

struct Pty {
    group: Option<crate::fork::motor_exec::Group>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn Write + Send>,
}
impl Drop for Pty {
    fn drop(&mut self) {
        if self.group.is_none() {
            kill_group(self.child.process_id());
            let _ = self.child.kill();
        }
    }
}

async fn claude(
    executable: &Path,
    directory: &Path,
    events: mpsc::Sender<Event>,
    mut input: mpsc::Receiver<Control>,
) -> Result<probe::Observation> {
    use portable_pty::{PtySize, native_pty_system};
    let pair = native_pty_system().openpty(PtySize { rows: 24, cols: 100, pixel_width: 0, pixel_height: 0 })?;
    let mut cmd = crate::fork::motor_exec::pty(executable, directory)?;
    cmd.args(["auth", "login"]);
    cmd.env_clear();
    for (key, value) in environment(directory, Backend::Claude) {
        cmd.env(key, value);
    }
    cmd.cwd(crate::fork::motor_exec::hub_cwd(directory));
    #[cfg(unix)]
    cmd.umask(Some(0o077));
    let child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;
    let group = crate::fork::motor_exec::Group::pty(child.process_id());
    let mut pty = Pty { child, master: pair.master, writer, group };
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(16);
    std::thread::spawn(move || {
        let mut bytes = [0_u8; 2048];
        while let Ok(n) = reader.read(&mut bytes) {
            if n == 0 || tx.blocking_send(bytes[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut presentation = Presentation::default();
    let mut offered = false;
    let mut submitted = false;
    let mut output_closed = false;
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    loop {
        tokio::select! {
            bytes = rx.recv(), if !output_closed => if let Some(bytes) = bytes {
                if let Some(challenge) = presentation.push(&bytes)? && !offered { events.send(Event::Challenge(challenge)).await?; offered = true; }
            } else {
                // EOF/erro do leitor encerra só o transporte. O filho pode ainda estar vivo:
                // aguardar try_wait no tick, sob o prazo/cancelamento originais do login.
                output_closed = true;
            },
            control = input.recv() => match control {
                Some(Control::Code(code)) => { ensure!(offered && !submitted,"auth_input_unavailable"); super::transport::validate_code(&code)?; pty.writer.write_all(format!("{code}\n").as_bytes())?; pty.writer.flush()?; submitted = true; },
                Some(Control::Resize(cols,rows)) => pty.master.resize(PtySize {cols,rows,pixel_width:0,pixel_height:0})?,
                None => bail!("setup_cancelled"),
            },
            _ = tick.tick() => if let Some(status) = pty.child.try_wait()? { ensure!(status.success(),"auth_failed"); break; },
        }
    }
    ensure!(offered, "auth_correlation_invalid");
    events.send(Event::Verifying).await?;
    Ok(probe::inspect(Backend::Claude, executable, directory).await)
}

pub(crate) async fn claude_catalog(
    executable: &Path,
    directory: &Path,
) -> Result<Vec<crate::fork::engine_accounts::Model>> {
    crate::fork::background_policy::enforce(false)
        .await
        .map_err(|_| crate::fork::background_notice::Problem::Policy)?;
    let commands = crate::fork::engine_accounts::managed::adapter_commands("claude").await?;
    catalog_from_commands(executable, directory, &commands).await
}

pub(super) async fn catalog_from_commands(
    executable: &Path,
    directory: &Path,
    commands: &[String],
) -> Result<Vec<crate::fork::engine_accounts::Model>> {
    catalog_from_commands_mode(executable, directory, commands, false).await
}

pub(crate) async fn background_claude_catalog(
    executable: &Path,
    directory: &Path,
) -> Result<Vec<crate::fork::engine_accounts::Model>> {
    crate::fork::background_policy::enforce(false)
        .await
        .map_err(|_| crate::fork::background_notice::Problem::Policy)?;
    let commands = crate::fork::background_adapter::commands();
    catalog_from_commands_mode(executable, directory, &commands, true).await
}

async fn catalog_from_commands_mode(
    executable: &Path,
    directory: &Path,
    commands: &[String],
    background: bool,
) -> Result<Vec<crate::fork::engine_accounts::Model>> {
    let mut env = Vec::new();
    crate::agent::harness::apply(&mut env, "claude", "");
    env.extend(environment(directory, Backend::Claude));
    env.push(("CLAUDE_CODE_EXECUTABLE".into(), executable.to_string_lossy().into_owned()));
    let command = commands.first().ok_or_else(|| anyhow!("catalog_unavailable"))?;
    let conn = if background {
        crate::fork::background_policy::enforce(false)
            .await
            .map_err(|_| crate::fork::background_notice::Problem::Policy)?;
        crate::agent::bot::start_background_agent(command, &directory.to_string_lossy(), &env, Duration::from_secs(20))
            .await?
    } else {
        crate::agent::bot::start_agent_protected(command, &directory.to_string_lossy(), &env, Duration::from_secs(20))
            .await?
    };
    if background && conn.adapter != crate::agent::bot::Adapter::Claude {
        conn.acp.kill_and_wait().await;
        return Err(crate::fork::background_notice::Problem::Version.into());
    }
    let result = tokio::time::timeout(Duration::from_secs(20), async {
        let response = conn
            .acp
            .request(
                "session/new",
                json!({"cwd":directory,"mcpServers":[],"_meta":{
                    "claudeCode":{"options":{"tools":[],"disallowedTools":["*"],"strictMcpConfig":true,
                        "persistSession":false,"settingSources":[],"settings":{"disableAllHooks":true}}}
                }}),
            )
            .await?;
        let catalog = crate::agent::auth::model_catalog(&response, &Value::Null);
        let sid = response["sessionId"].as_str().ok_or_else(|| anyhow!("catalog_unavailable"))?;
        let config_id = crate::agent::auth::model_config(&response).and_then(|value| value["id"].as_str());
        let mut models = Vec::new();
        for item in catalog["models"].as_array().into_iter().flatten().take(32) {
            if let Some(id) = item["id"].as_str().filter(|id| {
                !id.is_empty()
                    && id.len() <= 100
                    && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
            }) {
                let options = if let Some(config_id) = config_id {
                    let selected = conn
                        .acp
                        .request(
                            "session/set_config_option",
                            json!({
                                "sessionId":sid,"configId":config_id,"value":id
                            }),
                        )
                        .await;
                    selected.ok().map_or_else(Vec::new, |value| claude_effort(&value, id))
                } else {
                    Vec::new()
                };
                models.push(crate::fork::engine_accounts::Model { id: id.into(), options });
            }
        }
        Ok::<_, anyhow::Error>(models)
    })
    .await;
    if background {
        conn.acp.kill_and_wait().await;
    } else {
        conn.acp.kill().await;
    }
    result?
}

fn claude_effort(response: &Value, model: &str) -> Vec<crate::fork::engine_accounts::ModelOption> {
    if crate::agent::auth::model_catalog(response, &Value::Null)["currentModelId"] != model {
        return Vec::new();
    }
    let Some(option) = response["configOptions"].as_array().into_iter().flatten().find(|v| v["id"] == "effort") else {
        return Vec::new();
    };
    let values: Vec<String> = option["options"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["value"].as_str())
        .filter(|v| matches!(*v, "default" | "low" | "medium" | "high" | "xhigh" | "max"))
        .map(str::to_owned)
        .collect();
    if values.is_empty() {
        return Vec::new();
    }
    let default = option["currentValue"].as_str().filter(|v| values.iter().any(|known| known == v)).map(str::to_owned);
    vec![crate::fork::engine_accounts::ModelOption { id: "effort".into(), values, default }]
}

pub(crate) struct ProcessGroup(pub Option<u32>);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        kill_group(self.0);
    }
}

/// Consultas auxiliares de descoberta também pertencem ao ciclo cancelável do setup.
pub(crate) async fn output_with_group(mut command: Command) -> std::io::Result<std::process::Output> {
    command.kill_on_drop(true).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    if !crate::fork::motor_install::worker() {
        command.process_group(0);
    }
    let child = crate::fork::motor_exec::spawn(&mut command)?;
    let _group = ProcessGroup(child.id());
    child.wait_with_output().await
}

#[cfg(test)]
mod effort_tests {
    use super::*;

    #[tokio::test]
    #[cfg(unix)]
    async fn catalogo_de_fundo_recusa_nome_ou_versao_antes_de_abrir_sessao() {
        let policy = std::env::temp_dir().join(format!("catalogo-politica-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&policy).unwrap();
        let _root = crate::fork::background_policy::test_policy_root(&policy);
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/memory_agent.py");
        for mode in ["--adapter-version-next", "--adapter-name-mismatch", "--adapter-other", "--adapter-current"] {
            let directory = std::env::temp_dir().join(format!("background-handshake-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&directory).unwrap();
            let log = directory.join("requests.log");
            let command = format!(
                "python3 -u {} {mode} {}",
                crate::shell::quote(&fixture.to_string_lossy()),
                crate::shell::quote(&log.to_string_lossy())
            );
            let result = catalog_from_commands_mode(Path::new("/fixture/claude"), &directory, &[command], true).await;
            let requests = std::fs::read_to_string(&log).unwrap();
            if mode == "--adapter-current" {
                assert!(result.unwrap().iter().any(|model| model.id == "haiku"));
                assert!(requests.lines().any(|method| method == "session/new"));
            } else {
                let error = result.unwrap_err();
                assert!(matches!(
                    error.downcast_ref::<crate::fork::background_notice::Problem>(),
                    Some(crate::fork::background_notice::Problem::Version)
                ));
                assert_eq!(requests, "initialize\n", "sem session/new nem prompt: {mode}");
                assert!(error.to_string().contains(crate::fork::background_adapter::VERSION));
            }
            std::fs::remove_dir_all(directory).unwrap();
        }
        std::fs::remove_dir(policy).unwrap();
    }

    #[test]
    fn esforco_so_e_associado_ao_modelo_confirmado() {
        let response = json!({"configOptions":[
            {"id":"model","category":"model","type":"select","currentValue":"haiku","options":[]},
            {"id":"effort","type":"select","currentValue":"low","options":[
                {"value":"low"},{"value":"high"},{"value":"não autorizado"}
            ]}
        ]});
        let options = claude_effort(&response, "haiku");
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].values, ["low", "high"]);
        assert_eq!(options[0].default.as_deref(), Some("low"));
        assert_eq!(claude_effort(&response, "outro"), Vec::new());
    }
}
