// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! `donnabot-host` (pacote codync-host): runs coding-agent bots over ACP and serves the Codync apps.

mod agent;
mod analytics;
mod api;
mod background;
mod chat;
mod compat;
mod computers;
mod environment;
mod fork;
mod fork_avatars;
mod guard_hub;
mod hub;
mod market;
mod mcp;
mod open_hub;
mod remote;
mod routines;
mod screen;
mod service;
mod shell;
mod store;
mod tui;
mod update;
mod usage;
mod voice;

use agent::backends;
use remote::identity;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;
use std::fmt::Write as _;
use std::future::IntoFuture;
use std::io::Read;
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// Poison-tolerant locking. Every critical section in the host is a short,
/// self-contained update, so a panic elsewhere never leaves the data half
/// written — and a daemon must not wedge every later caller over one panic.
pub trait LockExt<T> {
    type Guard<'a>: std::ops::DerefMut<Target = T>
    where
        Self: 'a,
        T: 'a;
    #[track_caller]
    fn locked(&self) -> Self::Guard<'_>;
}

impl<T> LockExt<T> for Mutex<T> {
    type Guard<'a>
        = MutexGuard<'a, T>
    where
        T: 'a;

    #[track_caller]
    fn locked(&self) -> Self::Guard<'_> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Espera curta por um flock exclusivo. Um processo filho criado por outra thread herda a
/// descrição aberta até o exec e segura o lock por ~1 ms depois do drop (medido no #281).
/// `Ok(false)`: contenção além de `wait`, um detentor real. Outros erros saem como estão.
pub(crate) const FLOCK_WAIT: Duration = Duration::from_secs(1);
pub(crate) fn lock_file(file: &std::fs::File, wait: Duration) -> std::io::Result<bool> {
    let deadline = std::time::Instant::now() + wait;
    loop {
        match fs2::FileExt::try_lock_exclusive(file) {
            Ok(()) => return Ok(true),
            Err(e) if e.raw_os_error() != fs2::lock_contended_error().raw_os_error() => return Err(e),
            Err(_) if std::time::Instant::now() >= deadline => return Ok(false),
            Err(_) => std::thread::sleep(Duration::from_millis(1)),
        }
    }
}

/// Prova comum dos locks com `lock_file`: cópia herdada solta em 50 ms é esperada; detentor real
/// é recusado com `refused` depois de `FLOCK_WAIT`.
#[cfg(test)]
pub(crate) fn assert_flock_waits(lock: impl Fn() -> Result<std::fs::File>, refused: &str) {
    let copy = lock().unwrap().try_clone().unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        drop(copy);
    });
    let held = lock().expect("cópia herdada deve ser esperada");
    release.join().unwrap();
    let started = std::time::Instant::now();
    let error = lock().expect_err("detentor real continua recusado");
    assert!(started.elapsed() >= FLOCK_WAIT);
    assert_eq!(error.to_string(), refused);
    drop(held);
}

/// One shared HTTP client (connection pool + TLS config) for the whole process.
pub fn http() -> &'static reqwest::Client {
    static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
        // Only fails without system CA certificates (bare containers).
        reqwest::Client::builder().build().expect("TLS setup failed: install the ca-certificates package")
    });
    &CLIENT
}

/// Cliente HTTP para a nuvem e chamadas que levam credencial (assinatura `Codync-Sig`,
/// bearer, tickets): nunca segue redirecionamento, para a credencial não ir a outro endereço.
pub fn http_cloud() -> &'static reqwest::Client {
    static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("TLS setup failed: install the ca-certificates package")
    });
    &CLIENT
}

/// Cliente HTTP para fornecedores de IA (`OpenAI`, Gemini) em chamadas que levam credencial
/// (bearer, `x-goog-api-key`): nunca segue redirecionamento. O reqwest só remove
/// `Authorization` entre origens, não cabeçalhos personalizados, e 307/308 reenviam o corpo.
pub fn http_fornecedor() -> &'static reqwest::Client {
    static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("TLS setup failed: install the ca-certificates package")
    });
    &CLIENT
}

#[derive(Parser)]
#[command(
    name = "donnabot-host",
    version,
    about = "Mexerica host: runs your coding-agent bots and serves the Mexerica apps"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Sub>,
}

#[derive(Subcommand)]
enum Sub {
    /// Executor de referência no computador do dono: cadastrar, liberar um local e atender pedidos de planilha.
    Executor {
        #[command(subcommand)]
        action: computers::helper::Action,
    },
    /// Auditoria interna, executada como motor pelo ajudante.
    #[command(hide = true)]
    AuditarCodex {
        #[arg(long)]
        home: std::path::PathBuf,
    },
    /// Operação interna executada pelo ajudante como motor.
    #[command(hide = true)]
    MemoriaWorkspace {
        #[arg(long)]
        workspace: std::path::PathBuf,
        #[arg(long)]
        on: bool,
    },
    /// Coordena a atualização privilegiada como o usuário do Hub (para, faz backup, chama o wrapper).
    #[cfg(target_os = "linux")]
    CoordenarAtualizacao {
        /// Somente prova em VM: deixa a janela de aceite pendente (exige --prova).
        #[arg(long, hide = true)]
        sem_aceite: bool,
        /// Somente prova em VM: volta completa mesmo saudável (exige --prova).
        #[arg(long, hide = true)]
        forcar_reversao: bool,
        /// Confirma o uso das opções de prova.
        #[arg(long, hide = true)]
        prova: bool,
        #[arg(long, default_value_t = 10)]
        saude_segundos: u64,
    },
    /// Instalação limpa do Hub no layout gerido; chamada como o usuário do Hub pela etapa
    /// administrativa (`donnabot-instalar-atualizacao instalar --hub-novo`).
    #[cfg(target_os = "linux")]
    InstalarGerido {
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        porta: u16,
        #[arg(long, default_value_t = 10)]
        saude_segundos: u64,
    },
    /// Inventaria ou migra referências de raízes, com o serviço parado.
    MigrarRaizes {
        #[arg(long)]
        data_dir: std::path::PathBuf,
        #[arg(long)]
        inventario: bool,
        #[arg(long, hide = true)]
        trava_stdin: bool,
    },
    /// Programa isolado de consulta do executor, sem abrir o Hub.
    Runner {
        #[command(subcommand)]
        action: fork::onde_roda::runner::Action,
    },
    /// Procedimento administrativo D430 da instalação Linux isolada (nascimento e ponte local).
    PersonalLink {
        #[command(subcommand)]
        action: fork::personal_link::installation::Acao,
    },
    /// Install one signed grant from standard input, against the installed trusted root.
    Bootstrap {
        #[command(subcommand)]
        action: BootstrapAction,
    },
    /// Run the host in the foreground (what the background service runs).
    Serve {
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
        /// Address to bind. Defaults to all interfaces so the phone can reach it over Tailscale/LAN
        /// (`127.0.0.1` with `--only-this-mac`).
        #[arg(long)]
        bind: Option<String>,
        /// Hub local do Mac: só aceita `--bind 127.0.0.1` e não inicia o relay de saída.
        #[arg(long)]
        only_this_mac: bool,
        /// Interface Web local servida por este Hub: só em `127.0.0.1`, com sessão própria
        /// do navegador. O diretório precisa conter `index.html`.
        #[arg(long, env = "MEXERICA_WEB_DIR")]
        web_dir: Option<std::path::PathBuf>,
    },
    /// Show a one-time pairing QR code and link (the host must be running; the Mexerica iPhone app comes later).
    Pair {
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
        /// Print machine-readable JSON instead of a QR code.
        #[arg(long)]
        json: bool,
    },
    /// This computer's identity and local API details (for SSH setups).
    Info {
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
        #[arg(long)]
        json: bool,
    },
    /// List or revoke the devices allowed to use this computer.
    Devices {
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
        #[command(subcommand)]
        action: Option<DevicesAction>,
    },
    /// List, approve or deny account devices asking to use this computer.
    Access {
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
        #[command(subcommand)]
        action: Option<AccessAction>,
    },
    /// Show or change the Mexerica cloud (reach this computer from anywhere).
    Cloud {
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        #[arg(long)]
        disable: bool,
        /// Approve each device by typing the six-digit code shown on the device (`code`).
        #[arg(long, value_parser = ["code"])]
        approval: Option<String>,
    },
    /// Install and start the host as a background service (launchd / systemd --user).
    Install {
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
        /// Linux: ajudante do mesmo pacote, instalado com sudo.
        #[arg(long)]
        ajudante: Option<std::path::PathBuf>,
    },
    /// Etapa administrativa do instalador Linux, chamada via sudo.
    #[command(hide = true)]
    InstallMotor {
        #[command(subcommand)]
        action: fork::linux_installer::RootAction,
    },
    /// Migração com cópia e volta, exclusiva da instalação de teste do dono (D330 F4).
    MigrarMotor {
        #[command(subcommand)]
        action: fork::linux_installer::Migration,
    },
    /// Stop and remove the background service.
    Uninstall {
        /// Linux: remove também a conta do motor; mantém todos os dados.
        #[arg(long)]
        remover_usuario_motor: bool,
        /// Mac: 1 remove o serviço; 2 remove também logs e caches regeneráveis. Nunca apaga dados
        /// nem toca em `~/.claude`. Sem `--nivel` vale o comportamento herdado.
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=2))]
        nivel: Option<u8>,
    },
    /// Stop the background service, keeping its configuration and all data.
    Stop,
    /// Update a standalone host, or inspect/configure its automatic updates.
    Update {
        #[arg(long, conflicts_with_all = ["status", "worker"])]
        check: bool,
        /// Read update progress without checking or installing a release.
        #[arg(long, conflicts_with = "worker")]
        status: bool,
        #[arg(long, value_parser = ["on", "off"], conflicts_with_all = ["check", "status", "worker"])]
        auto: Option<String>,
        /// Allow a manual update to interrupt active bots (their sessions resume).
        #[arg(long)]
        force: bool,
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
        #[arg(long)]
        json: bool,
        #[arg(long, hide = true)]
        worker: bool,
    },
    /// The versions and environment of this host (JSON). The desktop app reads `environment`
    /// from it before installing the host; hosts before 2.19.3 also read `minApp` from a
    /// downloaded update.
    #[command(hide = true)]
    Compat,
    /// Show whether the host is installed and running.
    Status {
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
    },
    /// Claude Code statusLine command: forwards usage limits to the host, then prints
    /// the wrapped command's status line (`statusline -- <command>`) or a short default.
    Statusline {
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
        #[arg(last = true)]
        wrapped: Vec<String>,
    },
    /// Rotate the local API token used by this computer's own apps and helpers.
    ResetToken,
    /// Message your bots from this terminal.
    Tui {
        /// Host to connect to (default: this computer). The API only answers local
        /// callers: for another computer, forward its port with `ssh -L` and use the tunnel.
        #[arg(long)]
        url: Option<String>,
        /// Token for `--url` (default: this computer's `~/.mexerica/token`).
        #[arg(long, env = "CODYNC_TOKEN", hide_env_values = true)]
        token: Option<String>,
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
    },
    /// npm on this computer's Node, else on Mexerica's own (downloaded first): what setup
    /// terminals install npm CLIs with.
    #[command(hide = true)]
    Npm {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Add Mexerica's own Node.js, for agents that run on it (an agent's setup step).
    #[command(hide = true)]
    Node,
    /// Instalador interno, executado apenas com o UID do motor.
    #[command(hide = true)]
    MotorInstall,
    /// Built-in MCP servers that bots are started with (stdio).
    #[command(hide = true)]
    Mcp {
        #[command(subcommand)]
        server: McpServer,
    },
}

#[derive(Subcommand)]
enum BootstrapAction {
    /// Signed managed-cloud health, without the loopback token or key material.
    ControlHealth {
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
    },
    /// Read public bootstrap IDs, hash and durable state without reopening the ceremony.
    Inspect {
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
        #[arg(long)]
        json: bool,
    },
    InstallGrant {
        #[arg(long, required = true)]
        stdin: bool,
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
    },
}

#[derive(Subcommand)]
enum DevicesAction {
    /// Show every allowed device (the default).
    List,
    /// Remove a device's access; it disconnects at once.
    Revoke { key: String },
}

#[derive(Subcommand)]
enum AccessAction {
    /// Show pending requests; approve by typing the code shown on the device (the default).
    List,
    /// Approve with the 6-digit code typed from the device; optionally supply request ID and --code.
    Approve {
        request: String,
        #[arg(long)]
        code: Option<String>,
    },
    /// Deny a request.
    Deny { request: String },
}

#[derive(Subcommand)]
enum McpServer {
    /// Launch a local connector without exposing its credentials in ACP config.
    Local {
        #[arg(long)]
        connector: String,
        #[arg(long)]
        context: String,
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
    },
    /// Invite the user to connect services securely.
    Connectors {
        #[arg(long)]
        bot: String,
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
    },
    /// Scheduled and event-triggered bot routines.
    Routines {
        #[arg(long)]
        bot: String,
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
    },
    /// Send the user chat messages.
    Chat {
        #[arg(long)]
        bot: String,
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
    },
    /// Discover and ask the user's other bots for help.
    Team {
        #[arg(long)]
        bot: String,
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
    },
    /// Create bots and change any bot's setup, for a bot that may manage the team.
    Manage {
        #[arg(long)]
        bot: String,
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
    },
    /// Search the bot's own chat history.
    Memory {
        #[arg(long)]
        bot: String,
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
    },
    /// Read and update the Space's Tasks under the bot's own authority.
    Tasks {
        #[arg(long)]
        bot: String,
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
    },
    /// Ponte curada: frames para Session mantida no Hub, sem credenciais remotas.
    Curated {
        #[arg(long)]
        context: String,
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
    },
    /// A remote connector, with its sign-in handled by the host.
    Remote {
        #[arg(long)]
        connector: String,
        #[arg(long)]
        context: String,
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
    },
    /// Request approved work on the owner's computers through the Hub.
    Computers {
        #[arg(long)]
        bot: String,
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
    },
    /// See and operate this computer's desktop.
    Computer {
        #[arg(long)]
        bot: String,
        #[arg(long, default_value_t = service::DEFAULT_PORT)]
        port: u16,
    },
}

/// The database id + local API token live in the database; the token is mirrored to
/// `install`: as travas de porta e pasta rodam antes de qualquer efeito colateral (banco, status
/// line do Claude Code, serviço), para nunca tocar em nada do Codync instalado.
fn install(
    port: u16,
    claude_settings: Option<&std::path::Path>,
    start_service: impl FnOnce(u16) -> Result<()>,
) -> Result<()> {
    environment::ensure_own_port(port)?;
    environment::ensure_own_data_dir(&service::home(), &service::data_dir())?;
    open_store()?;
    if let Some(settings) = claude_settings {
        match service::ensure_statusline(settings) {
            Ok(true) => println!(
                "Claude Code's status line now also reports usage limits to Mexerica (your own status line still shows)."
            ),
            Ok(false) => {}
            Err(e) => eprintln!("Couldn't set Claude Code's status line: {e}"),
        }
    }
    start_service(port)?;
    println!("Mexerica host installed and started on port {port}. Check it with {} status.", self_command());
    Ok(())
}

/// How to run this binary again in a hint: its absolute path, so a hint never sends anyone to
/// another installation's host (the Codync one may be on the same computer).
fn self_command() -> String {
    service::current_exe().map_or_else(|_| "donnabot-host".to_owned(), |exe| format!("'{}'", exe.display()))
}

/// `~/.mexerica/token` (0600) for local helpers like the statusline command.
fn open_store() -> Result<(store::Store, String, String)> {
    let dir = service::data_dir();
    environment::ensure_own_data_dir(&service::home(), &dir)?;
    #[cfg(unix)]
    fork::private_fs::inspect(&dir)?;
    // PL1 recusa antes de criar/endurecer a pasta; a criação e o 0700 do motor vêm depois das recusas (HOST-MERGE-01).
    let store = fork::personal_bootstrap::provisioning::open_legacy_store(&dir)?;
    opened_store(store)
}

fn store_host_id(store: &store::Store) -> Result<String> {
    Ok(if let Some(v) = store.kv_get("host_id") {
        v
    } else {
        let v = uuid::Uuid::new_v4().to_string();
        store.kv_set("host_id", &v)?;
        // A new install, whichever command made it: its first start is `host_started.first_run`.
        store.kv_set(analytics::KV_STARTED, "")?;
        v
    })
}

fn opened_store(store: store::Store) -> Result<(store::Store, String, String)> {
    let host_id = store_host_id(&store)?;
    let token = match store.kv_get("token") {
        Some(v) => v,
        None => rotate_token(&store)?,
    };
    Ok((store, host_id, token))
}

fn rotate_token(store: &store::Store) -> Result<String> {
    let t = new_token();
    store.kv_set("token", &t)?;
    write_token_file(&t)?;
    Ok(t)
}

fn new_token() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

fn write_token_file(token: &str) -> Result<()> {
    let path = service::data_dir().join("token");
    #[cfg(unix)]
    fork::private_fs::write(&path, token.as_bytes())?;
    #[cfg(not(unix))]
    std::fs::write(&path, token).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Ponte interna do navegador: roda dentro do namespace, só copia bytes ao proxy.
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    if raw_args.first().map(String::as_str) == Some("__browser-bridge") {
        anyhow::ensure!(!environment::Environment::is_open(), "browser bridge unavailable in Mexerica Open");
        return fork::browser::bridge::run_bridge_process(&raw_args[1..]);
    }
    // Ponte do Claude Code oficial: ACP com o Hub na entrada/saída padrão.
    if raw_args.first().map(String::as_str) == Some(fork::claude_ponte::MODO) {
        // A saída padrão é o protocolo ACP: o registro da ponte vai só para a saída de erro.
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "donnabot_host=info".into()),
            )
            .log_internal_errors(false)
            .init();
        return fork::claude_ponte::executar(&raw_args[1..]).await;
    }
    let cli = if environment::Environment::is_open() { open_hub::cli() } else { Cli::parse() };
    // MCP de sessão usa só credencial da ponte. A descoberta com SETUP_ENV
    // ainda lê token local e conserva todas as guardas de pasta.
    // O executor usa sua própria pasta de estado (--home ou dados do usuário/Mexerica/executor),
    // separada da pasta do Hub, mas aplica as mesmas guardas de ambiente, propriedade e permissões.
    if !environment::Environment::is_open() {
        if let Some(Sub::Executor { action }) = cli.cmd.as_ref() {
            fork::data_home::validate_environment()?;
            let executor_dir = computers::helper::state_dir(action)?;
            environment::ensure_own_data_dir(&service::home(), &executor_dir)?;
            #[cfg(unix)]
            fork::private_fs::inspect(&executor_dir)?;
        } else if !matches!(cli.cmd.as_ref(), Some(Sub::Mcp { .. }))
            || std::env::var_os(fork::ponte_credencial::SETUP_ENV).is_some()
        {
            fork::data_home::validate_environment()?;
            environment::ensure_own_data_dir(&service::home(), &service::data_dir())?;
            #[cfg(unix)]
            fork::private_fs::inspect(&service::data_dir())?;
        }
    }
    match cli.cmd.unwrap_or(Sub::Serve { port: service::DEFAULT_PORT, bind: None, only_this_mac: false, web_dir: None })
    {
        Sub::AuditarCodex { home } => fork::motor_codex_audit::local(&home),
        Sub::MemoriaWorkspace { workspace, on } => agent::own_memory::configure_workspace("qwen", &workspace, on),
        #[cfg(target_os = "linux")]
        Sub::CoordenarAtualizacao { sem_aceite, forcar_reversao, prova, saude_segundos } => {
            fork::coordenador_atualizacao::executar(fork::coordenador_atualizacao::Opcoes {
                sem_aceite,
                forcar_reversao,
                saude_segundos,
                prova,
            })
        }
        #[cfg(target_os = "linux")]
        Sub::InstalarGerido { porta, saude_segundos } => fork::instalacao_gerida::executar(porta, saude_segundos),
        Sub::MigrarRaizes { data_dir, inventario, trava_stdin } => {
            fork::motor_migration::run(&data_dir, inventario, trava_stdin)
        }
        Sub::Executor { action } => computers::helper::main(action).await,
        Sub::Runner { action } => fork::onde_roda::runner::run(action).await,
        Sub::PersonalLink { action } => fork::personal_link::installation::executar(action).await,
        Sub::Bootstrap { action: BootstrapAction::ControlHealth { port } } => {
            let value = local_call(port, "controlHealth", json!({})).await?;
            println!("{}", serde_json::to_string(&value)?);
            Ok(())
        }
        Sub::Bootstrap { action: BootstrapAction::Inspect { port, json: as_json } } => {
            let value = local_call(port, "bootstrapInspect", json!({})).await?;
            if as_json {
                println!("{}", serde_json::to_string(&value)?);
            } else {
                println!("{}", value["state"].as_str().unwrap_or("unavailable"));
            }
            Ok(())
        }
        Sub::Bootstrap { action: BootstrapAction::InstallGrant { stdin: _, port } } => {
            let mut data = Vec::new();
            std::io::stdin().take((fork::bootstrap::codec::LIMIT + 1) as u64).read_to_end(&mut data)?;
            anyhow::ensure!(data.len() <= fork::bootstrap::codec::LIMIT, "grant too large");
            let wire = std::str::from_utf8(&data).context("grant must be UTF-8")?;
            local_call(port, "bootstrapInstallGrant", json!({"envelope": wire})).await?;
            println!("Grant installed.");
            Ok(())
        }
        Sub::Serve { port, bind, only_this_mac, web_dir } => {
            if environment::Environment::is_open() {
                #[cfg(unix)]
                return open_hub::serve(port, bind.as_deref(), only_this_mac).await;
                #[cfg(not(unix))]
                anyhow::bail!("Mexerica Open Hub requires a Unix socket");
            }
            let mac_service = cfg!(target_os = "macos") && service::started_by_service();
            let (bind, scope) = service::mac_local::resolve_serve_with_web(
                bind.as_deref(),
                only_this_mac,
                mac_service,
                web_dir.is_some(),
            )?;
            serve(&bind, port, scope, web_dir).await
        }
        Sub::Pair { port, json } => pair(port, json).await,
        Sub::Info { port, json } => info(port, json).await,
        Sub::Devices { port, action } => match action.unwrap_or(DevicesAction::List) {
            DevicesAction::List => {
                let v = local_call(port, "devices", json!({})).await?;
                for d in v["devices"].as_array().into_iter().flatten() {
                    println!(
                        "{}  {} ({}, {}){}",
                        d["key"].as_str().unwrap_or_default(),
                        d["name"].as_str().unwrap_or_default(),
                        d["platform"].as_str().unwrap_or_default(),
                        d["source"].as_str().unwrap_or_default(),
                        if d["connected"] == true { "  connected" } else { "" }
                    );
                }
                Ok(())
            }
            DevicesAction::Revoke { key } => {
                local_call(port, "revokeDevice", json!({"key": key})).await?;
                println!("Revoked.");
                Ok(())
            }
        },
        Sub::Access { port, action } => access(port, action.unwrap_or(AccessAction::List)).await,
        Sub::Cloud { port, enable, disable, approval } => {
            if let Some(approval) = approval {
                local_call(port, "setApproval", json!({ "approval": approval })).await?;
            }
            let status = if enable || disable {
                local_call(port, "setCloud", json!({"enabled": !disable})).await?
            } else {
                local_call(port, "cloudStatus", json!({})).await?
            };
            let text = |k: &str| status[k].as_str().unwrap_or("-").to_owned();
            let owner = status["owner"]["email"].as_str().or(status["owner"]["userId"].as_str()).unwrap_or("none");
            println!(
                "enabled:    {}\nurl:        {}\nregistered: {}\nconnected:  {}\naccount:    {owner}\napproval:   {}\nlast error: {}",
                status["enabled"],
                text("url"),
                status["registered"],
                status["connected"],
                text("approval"),
                text("lastError"),
            );
            Ok(())
        }
        Sub::InstallMotor { action } => fork::linux_installer::privileged(action).await,
        Sub::MigrarMotor { action } => fork::linux_installer::migrate(action),
        Sub::Install { port, ajudante } => {
            if cfg!(target_os = "linux") {
                return fork::linux_installer::install(port, ajudante);
            }
            // Windows: the shell Claude Code runs status lines in varies (Git Bash, PowerShell), so
            // the line stays the user's; usage still comes from `claude -p /usage` and ACP.
            let settings = (cfg!(unix) && environment::Environment::current() == environment::Environment::Main)
                .then(dirs::home_dir)
                .flatten()
                .map(|home| home.join(".claude/settings.json"));
            install(port, settings.as_deref(), service::install)
        }
        Sub::Compat => {
            println!(
                "{}",
                serde_json::json!({"version": env!("CARGO_PKG_VERSION"), "minApp": compat::MIN_APP, "environment": environment::Environment::current().name()})
            );
            Ok(())
        }
        Sub::Update { check, status, auto, force, port, json, worker } => {
            let status = if let Some(auto) = auto {
                update::set_automatic(auto == "on")?
            } else if status {
                update::status()?
            } else if check {
                update::check().await?
            } else if !worker && service::installed() {
                tokio::task::spawn_blocking(move || update::spawn_worker(port, force)).await??;
                update::status()?
            } else {
                update::apply(port, force, worker).await?
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                println!(
                    "Host {} · {}",
                    status["currentVersion"].as_str().unwrap_or("unknown"),
                    status["state"]["phase"].as_str().unwrap_or("ready")
                );
                println!("Automatic updates: {}", if status["automatic"] == true { "on" } else { "off" });
                if let Some(version) = status["state"]["availableVersion"].as_str() {
                    println!("Available version: {version}");
                }
                if let Some(error) = status["state"]["error"].as_str() {
                    println!("Last error: {error}");
                }
            }
            Ok(())
        }
        Sub::Stop => {
            tokio::task::spawn_blocking(service::stop).await??;
            println!("Mexerica host stopped.");
            Ok(())
        }
        Sub::Uninstall { remover_usuario_motor, nivel } => {
            if cfg!(target_os = "linux") {
                return fork::linux_installer::uninstall(remover_usuario_motor);
            }
            anyhow::ensure!(!remover_usuario_motor, "remoção de usuário do motor só existe no Linux");
            // Só confirma depois de o serviço sumir; um erro aqui impede o app de gravar hostUninstalled.
            tokio::task::spawn_blocking(service::uninstall).await??;
            if environment::Environment::current() == environment::Environment::Main
                && service::mac_local::touches_claude_settings(nivel)
                && let Some(home) = dirs::home_dir()
                && let Err(e) = service::restore_statusline(&home.join(".claude/settings.json"))
            {
                eprintln!("Couldn't restore Claude Code's status line: {e:#}");
            }
            if nivel == Some(2) {
                let removed = service::mac_local::remove_regenerable(&service::data_dir())?;
                tracing::info!(removed = removed.len(), "regenerable logs and caches removed");
            }
            println!("Mexerica host service removed. Data is kept in {}", service::data_dir().display());
            Ok(())
        }
        Sub::Status { port } => {
            let running = http()
                .get(format!("http://127.0.0.1:{port}/health"))
                .timeout(Duration::from_secs(2))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success());
            println!(
                "installed: {}\nrunning:   {running}\nport:      {port}\ndata:      {}",
                service::installed(),
                service::data_dir().display()
            );
            Ok(())
        }
        Sub::MotorInstall => fork::motor_install::run().await,
        Sub::Node => agent::node::add().await,
        Sub::Npm { args } => std::process::exit(agent::node::npm(&args).await?),
        Sub::Statusline { port, wrapped } => {
            statusline(port, wrapped.join(" ")).await;
            Ok(())
        }
        Sub::ResetToken => {
            let (store, _, _) = open_store()?;
            rotate_token(&store)?;
            println!("Token rotated. Restart the host so local helpers pick it up.");
            Ok(())
        }
        Sub::Mcp { server: McpServer::Connectors { bot, port } } => {
            mcp::serve(bot, port, mcp::Server::Connectors).await
        }
        Sub::Mcp { server: McpServer::Computer { bot, port } } => mcp::serve(bot, port, mcp::Server::Computer).await,
        Sub::Mcp { server: McpServer::Computers { bot, port } } => mcp::serve(bot, port, mcp::Server::Computers).await,
        Sub::Mcp { server: McpServer::Routines { bot, port } } => mcp::serve(bot, port, mcp::Server::Routines).await,
        Sub::Mcp { server: McpServer::Chat { bot, port } } => mcp::serve(bot, port, mcp::Server::Chat).await,
        Sub::Mcp { server: McpServer::Team { bot, port } } => mcp::serve(bot, port, mcp::Server::Team).await,
        Sub::Mcp { server: McpServer::Manage { bot, port } } => mcp::serve(bot, port, mcp::Server::Manage).await,
        Sub::Mcp { server: McpServer::Memory { bot, port } } => mcp::serve(bot, port, mcp::Server::Memory).await,
        Sub::Mcp { server: McpServer::Tasks { bot, port } } => mcp::serve(bot, port, mcp::Server::Tasks).await,
        Sub::Mcp { server: McpServer::Local { connector, port, context } } => {
            mcp::serve_local(connector, port, context).await
        }
        Sub::Mcp { server: McpServer::Curated { port, context } } => fork::conectores::serve(port, context).await,
        Sub::Mcp { server: McpServer::Remote { connector, port, context } } => {
            mcp::serve_remote(connector, port, context).await
        }
        Sub::Tui { url, token, port } => {
            tui::run(url.unwrap_or_else(|| format!("http://127.0.0.1:{port}")), token).await
        }
    }
}

/// launchd appends to `host.log` forever. The service already holds it open as
/// stdout (append mode), so truncate in place rather than rotating.
fn cap_log() {
    let log = service::data_dir().join("host.log");
    if std::fs::metadata(&log).is_ok_and(|m| m.len() > 10 * 1024 * 1024)
        && let Ok(f) = std::fs::OpenOptions::new().write(true).open(&log)
    {
        let _ = f.set_len(0);
    }
}

async fn serve(
    bind: &str,
    port: u16,
    scope: service::mac_local::Scope,
    web_dir: Option<std::path::PathBuf>,
) -> Result<()> {
    scope.check_bind(bind)?;
    let web_root = web_dir.map(|dir| fork::web_local::WebConfig::validate_dir(&dir)).transpose()?;
    environment::ensure_own_port(port)?;
    environment::ensure_own_data_dir(&service::home(), &service::data_dir())?;
    // C1-05: host.lock ANTES de qualquer acesso D430 e de prepare PL1; D430 nunca roda prepare PL1.
    let instance_lock = tokio::task::spawn_blocking(service::lock_host).await??;
    // Coordenação da atualização interrompida (queda): nenhum par grava antes da retomada.
    #[cfg(target_os = "linux")]
    fork::coordenador_atualizacao::exigir_sem_coordenacao_interrompida(&service::data_dir())?;
    // Confere o seletor depois de adquirir a trava que o coordenador mantém na troca.
    #[cfg(target_os = "linux")]
    fork::guarda_atualizacao::verificar()?;
    if fork::personal_link::installation::selecionado(&service::data_dir())? {
        anyhow::ensure!(
            web_root.is_none(),
            "a interface Web local não se aplica à instalação isolada; inicie o Hub sem --web-dir"
        );
        return fork::personal_link::installation::servir(instance_lock, bind, port, shutdown_signal()).await;
    }
    let personal = fork::personal_bootstrap::provisioning::prepare(&service::data_dir())?;
    let personal_listener =
        personal.as_ref().map(fork::personal_bootstrap::provisioning::Prepared::listener).transpose()?;
    let _instance_lock = instance_lock;
    tokio::task::spawn_blocking(service::capture_binary_identity).await??;
    let listener =
        tokio::net::TcpListener::bind((bind, port)).await.with_context(|| format!("binding {bind}:{port}"))?;
    let bound_port = listener.local_addr()?.port();
    cap_log();
    configure_logging();
    agent::harness::install_docs().await;
    chat::context::load_harness_context().await;
    let (opened, fixed_identity, personal) = match personal {
        Some(prepared) => {
            let (store, identity, installation) = prepared.open_store()?;
            (opened_store(store)?, Some(identity), Some(installation))
        }
        None => (open_store()?, None, None),
    };
    let (store, host_id, token) = opened;
    chat::manage::recover(&store);
    write_token_file(&token)?;
    let identity = match fixed_identity {
        Some(identity) => identity,
        None => identity::Identity::load_or_create(&service::data_dir())?,
    };
    tokio::task::spawn_blocking(backends::hydrate_path).await?;
    let hub = hub::Hub::new(store, host_id, identity, token, port);
    if let Some(root) = web_root {
        hub.set_web_local(std::sync::Arc::new(fork::web_local::WebState::new(root, bound_port)))?;
        tracing::info!(url = %format!("http://127.0.0.1:{bound_port}/"), "local web UI enabled");
    }
    if let Some(personal) = personal {
        personal.install(&hub)?;
    }
    if let Some(listener) = personal_listener {
        let hub = hub.clone();
        tokio::spawn(async move {
            let _ = listener.run(hub).await;
        });
    }
    if fork::bootstrap::provisioning::load(&hub).is_err() {
        tracing::warn!("bootstrap provisioning unavailable");
    }
    hub.start()?;
    analytics::host_started(&hub, service::started_by_service());
    background::start(&hub, scope);
    tracing::info!(version = env!("CARGO_PKG_VERSION"), bind, port, "donnabot-host listening");
    // Peer addresses: some settings may only be changed from this computer.
    let app = api::router(hub.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>();
    // SSE/WebSocket clients can stay connected forever. Stop agents immediately
    // on the signal instead of waiting for those connections to finish.
    let server = axum::serve(listener, app).into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result?,
        () = shutdown_signal() => {},
    }
    hub.shutdown().await;
    agent::acp::stopped().await;
    analytics::flush_before_exit(&hub).await;
    tracing::info!("donnabot-host stopped");
    // Uma leitura OS pode continuar no prompt; o drop do runtime esperaria por ela.
    std::process::exit(0)
}

fn configure_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "donnabot_host=info".into()),
        )
        .log_internal_errors(false)
        .init();
}

/// Resolves on Ctrl-C or SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::warn!(%error, "can't listen for Ctrl-C");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(error) => {
                tracing::warn!(%error, "can't listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = term => {}
    }
}

/// Calls the running host's local API (`~/.mexerica/token` over loopback).
async fn local_call(port: u16, method: &str, body: serde_json::Value) -> Result<serde_json::Value> {
    let not_running =
        || anyhow::anyhow!("donnabot-host isn't running here. Start it with {} install (or serve).", self_command());
    let token = fork::data_home::read_text(&service::data_dir().join("token")).map_err(|_| not_running())?;
    let res = http()
        .post(format!("http://127.0.0.1:{port}/api/{method}"))
        .bearer_auth(token.trim())
        .json(&body)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Host response not confirmed; check before repeating."))?;
    let status = res.status();
    let v: serde_json::Value = res.json().await.context("Host response not confirmed; check before repeating")?;
    if !status.is_success() {
        anyhow::bail!("{}", v["error"].as_str().unwrap_or("the host refused"));
    }
    Ok(v)
}

async fn pair(port: u16, as_json: bool) -> Result<()> {
    let hello = local_call(port, "hello", json!({})).await?;
    let p = local_call(port, "pairing", json!({})).await?;
    if as_json {
        println!(
            "{}",
            json!({
                "name": hello["name"], "computerId": hello["computerId"], "signKey": hello["signKey"],
                "boxKey": hello["boxKey"], "urls": p["urls"], "cloud": hello["cloud"],
                "pairingUrl": p["pairingUrl"], "expiresAt": p["expiresAt"],
            })
        );
        return Ok(());
    }
    let url = p["pairingUrl"].as_str().unwrap_or_default();
    let code = qrcode::QrCode::new(url.as_bytes())?;
    println!("{}", code.render::<qrcode::render::unicode::Dense1x2>().quiet_zone(true).build());
    println!(
        "Pairing link for {} (valid for 10 minutes). Pairing from the iPhone comes later: the Mexerica iPhone app isn't available yet.\n{url}\n",
        environment::Environment::current().display_name()
    );
    if hello["cloud"].is_null()
        && p["urls"].as_array().is_none_or(|u| {
            !u.iter().any(|u| u.as_str().is_some_and(|u| u.contains("://100.") || u.contains(".ts.net")))
        })
    {
        println!(
            "Tip: the phone reaches this computer on the same network only. Use Tailscale to reach it from anywhere."
        );
    }
    Ok(())
}

/// What `access approve` prints once the Hub saved the device; `None` when the answer can't be trusted.
fn approved_message(result: &serde_json::Value) -> Option<String> {
    // Only an absent field means an older Hub; null or any malformed value is "not confirmed".
    match result.get("confirmCode") {
        None => Some("Aprovado.".to_owned()),
        Some(serde_json::Value::String(c)) if c.len() == 6 && c.bytes().all(|b| b.is_ascii_digit()) => {
            Some(format!("Aprovado. Agora digite este código no aparelho: {} {}", &c[..3], &c[3..]))
        }
        _ => None,
    }
}

fn access_line(r: &serde_json::Value) -> String {
    let state = if r["codeReady"] == true { "digite o código que o aparelho mostra" } else { "(esperando o aparelho)" };
    format!(
        "{}  {} ({}, {})  prazo {}  {}",
        r["requestId"].as_str().unwrap_or_default(),
        r["deviceName"].as_str().unwrap_or_default(),
        r["platform"].as_str().unwrap_or_default(),
        r["email"].as_str().unwrap_or("sem e-mail"),
        r["deadline"].as_i64().unwrap_or_default(),
        state
    )
}

/// O pedido a decidir: pelo ID, ou o único pronto quando a aprovação veio só com o código. Nunca compara com o
/// código do aparelho aqui: quem confere o que a pessoa digitou é o Hub, uma vez só.
fn pick_request<'a>(
    requests: &'a [serde_json::Value],
    id: Option<&str>,
    approve: bool,
) -> Result<&'a serde_json::Value> {
    let matches: Vec<_> = requests
        .iter()
        .filter(|r| match id {
            Some(id) => r["requestId"].as_str() == Some(id),
            None => approve && r["codeReady"] == true,
        })
        .collect();
    let [r] = matches.as_slice() else {
        anyhow::bail!("No unique request matches; give the request ID from `access list`.");
    };
    if approve && r["codeReady"] != true {
        anyhow::bail!("The device hasn't shown its code yet; wait for it and try again.");
    }
    Ok(r)
}

async fn access(port: u16, action: AccessAction) -> Result<()> {
    let (request, typed_code, approve) = match action {
        AccessAction::List => (None, None, false),
        AccessAction::Approve { request, code } => {
            let (id, typed) = match code {
                Some(code) => (Some(request), code),
                None => (None, request),
            };
            if typed.len() != 6 || !typed.bytes().all(|b| b.is_ascii_digit()) {
                anyhow::bail!("Approval requires the six-digit code typed from the device.");
            }
            (id, Some(typed), true)
        }
        AccessAction::Deny { request } => (Some(request), None, false),
    };
    let list = local_call(port, "accessRequests", json!({})).await?;
    let requests = list["requests"].as_array().context("invalid request list")?;
    if request.is_none() && typed_code.is_none() {
        if requests.is_empty() {
            println!("No devices are asking for access.");
        }
        for r in requests {
            println!("{}", access_line(r));
        }
        return Ok(());
    }
    let r = pick_request(requests, request.as_deref(), approve)?;
    let id = r["requestId"].as_str().filter(|id| !id.is_empty()).context("invalid request id")?;
    let mut body = json!({"requestId": id, "approve": approve});
    let mut expected_key = None;
    if approve {
        let expected = fork::enrollment::approval::Expected {
            device_key: r["deviceKey"].as_str().unwrap_or_default().to_owned(),
            code: typed_code.context("code is required")?,
            transcript: r["transcript"].as_str().unwrap_or_default().to_owned(),
        };
        if !expected.valid()
            || r["deadline"].as_i64().is_none_or(|d| d < store::now_ms())
            || r["expiresAt"] != r["deadline"]
        {
            anyhow::bail!("The request is incomplete or expired; check before deciding.");
        }
        expected_key = Some(expected.device_key.clone());
        body["expect"] =
            json!({"deviceKey": expected.device_key, "code": expected.code, "transcript": expected.transcript});
    }
    let result = local_call(port, "decideAccessRequest", body).await?;
    match result["result"].as_str() {
        Some("approved")
            if approve
                && result["deviceKey"].as_str() == expected_key.as_deref()
                && result["grantId"].as_str().is_some_and(|g| g.starts_with("grt_") && g.len() > 4)
                && approved_message(&result).is_some() =>
        {
            println!("{}", approved_message(&result).unwrap_or_default());
        }
        Some("denied") if !approve => println!("Denied."),
        Some("refused") if result["reason"] == "transcriptMismatch" => {
            anyhow::bail!("The code doesn't match the device. The request was cancelled; ask again from the device.")
        }
        Some("refused") => {
            anyhow::bail!("Request refused: {}", result["reason"].as_str().unwrap_or("invalid response"))
        }
        _ => anyhow::bail!("Result not confirmed; check before repeating."),
    }
    Ok(())
}

/// Reads the identity and token only; never creates them (that's `serve`'s job).
async fn info(port: u16, as_json: bool) -> Result<()> {
    let dir = service::data_dir();
    let (Some(id), Ok(token)) = (identity::Identity::load(&dir)?, fork::data_home::read_text(&dir.join("token")))
    else {
        eprintln!("start {} once first", self_command());
        std::process::exit(1);
    };
    let running = http()
        .get(format!("http://127.0.0.1:{port}/health"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .is_ok_and(|r| r.status().is_success());
    let v = json!({
        "name": service::host_name(),
        "computerId": id.computer_id(),
        "signKey": id.sign_pub_b64(),
        "boxKey": id.box_pub_b64(),
        "version": env!("CARGO_PKG_VERSION"),
        "port": port,
        "token": token.trim(),
        "running": running,
    });
    if as_json {
        println!("{v}");
    } else {
        println!(
            "name:     {}\ncomputer: {}\nversion:  {}\nport:     {port}\nrunning:  {running}",
            v["name"].as_str().unwrap_or_default(),
            v["computerId"].as_str().unwrap_or_default(),
            env!("CARGO_PKG_VERSION"),
        );
    }
    Ok(())
}

/// Never blocks Claude Code: 1 s budget, all errors ignored.
async fn statusline(port: u16, wrapped: String) {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let v: serde_json::Value = serde_json::from_str(&input).unwrap_or_default();
    // D330/R8: o statusline do motor não herda a credencial MCP. Sem fallback Local.
    if std::env::var_os(fork::ponte_credencial::MODO_ENV).is_none()
        && let Ok(token) = fork::data_home::read_text(&service::data_dir().join("token"))
    {
        let _ = http()
            .post(format!("http://127.0.0.1:{port}/ingest/statusline"))
            .bearer_auth(token.trim())
            .json(&v)
            .timeout(Duration::from_secs(1))
            .send()
            .await;
    }
    if !wrapped.trim().is_empty() {
        // The user's own status line gets the same JSON on stdin.
        use std::io::Write;
        if let Ok(mut child) = shell::line(&wrapped).std().stdin(std::process::Stdio::piped()).spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(input.as_bytes());
            }
            let _ = child.wait();
        }
        return;
    }
    let model = v["model"]["display_name"].as_str().unwrap_or("Claude");
    let mut line = model.to_owned();
    for (k, label) in [("five_hour", "5h"), ("seven_day", "7d")] {
        if let Some(p) = v["rate_limits"][k]["used_percentage"].as_f64() {
            let _ = write!(line, " · {label} {p:.0}%");
        }
    }
    println!("{line}");
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn approval_rejects_id_without_typed_code_before_any_call() {
        let result = super::access(0, super::AccessAction::Approve { request: "req_fixture".into(), code: None }).await;
        assert!(result.is_err());
    }

    #[test]
    fn access_line_never_shows_the_device_code() {
        let request = serde_json::json!({
            "requestId": "req_1",
            "deviceName": "iPhone",
            "platform": "ios",
            "email": "user@example.com",
            "deadline": 123,
            "code": "123456",
            "codeReady": true,
        });
        let line = super::access_line(&request);
        assert!(line.contains("req_1"));
        assert!(line.contains("digite o código que o aparelho mostra"));
        assert!(!line.contains("123456"));
        assert!(!line.contains("123 456"));
    }

    #[test]
    fn approval_picks_the_request_without_reading_the_device_code() {
        use serde_json::json;
        let ready = json!({"requestId": "req_1", "codeReady": true, "code": "111111"});
        let waiting = json!({"requestId": "req_2", "codeReady": false});
        let list = [ready.clone(), waiting.clone()];
        assert_eq!(super::pick_request(&list, None, true).unwrap(), &ready, "the only ready one, whatever was typed");
        assert_eq!(super::pick_request(&list, Some("req_1"), true).unwrap(), &ready);
        assert!(super::pick_request(&list, Some("req_2"), true).is_err(), "no code on the device yet");
        assert_eq!(super::pick_request(&list, Some("req_2"), false).unwrap(), &waiting, "deny by ID");
        assert!(super::pick_request(&list, None, false).is_err());
        assert!(super::pick_request(&list, Some("111111"), false).is_err(), "the code never selects a request");
        let two = [ready.clone(), json!({"requestId": "req_3", "codeReady": true})];
        assert!(super::pick_request(&two, None, true).is_err(), "two ready requests need the ID");
    }

    #[test]
    fn access_line_waits_for_the_device_without_a_code() {
        let request = serde_json::json!({
            "requestId": "req_2",
            "deviceName": "iPhone",
            "platform": "ios",
            "email": "user@example.com",
            "deadline": 123,
        });
        let line = super::access_line(&request);
        assert!(line.contains("(esperando o aparelho)"));
    }

    #[test]
    fn approval_prints_the_confirmation_code_only_when_well_formed() {
        use serde_json::json;
        assert_eq!(
            super::approved_message(&json!({"confirmCode": "244257"})).as_deref(),
            Some("Aprovado. Agora digite este código no aparelho: 244 257")
        );
        assert_eq!(super::approved_message(&json!({})).as_deref(), Some("Aprovado."));
        assert_eq!(super::approved_message(&json!({"confirmCode": null})), None);
        for bad in [json!("24425"), json!("2442570"), json!("24425a"), json!(244_257), json!("２４４２５７")] {
            assert_eq!(super::approved_message(&json!({"confirmCode": bad})), None);
        }
    }

    /// Com a porta do Codync, `install` para antes de abrir o banco, mexer na status line do
    /// Claude Code ou instalar o serviço.
    #[test]
    fn install_refuses_codync_port_before_any_side_effect() {
        let dir = std::env::temp_dir().join(format!("donnabot-install-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.json");
        let original = r#"{"statusLine":{"type":"command","command":"sh ~/mine.sh"}}"#;
        std::fs::write(&settings, original).unwrap();
        let mut started = false;
        let result = super::install(19222, Some(&settings), |_| {
            started = true;
            Ok(())
        });
        assert!(result.is_err());
        assert!(!started, "o serviço não pode ser instalado");
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), original);
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod bootstrap_cli_tests {
    use super::*;
    #[test]
    fn install_grant_requires_explicit_stdin_and_rejects_extra_arguments() {
        assert!(Cli::try_parse_from(["donnabot-host", "bootstrap", "install-grant"]).is_err());
        assert!(Cli::try_parse_from(["donnabot-host", "bootstrap", "install-grant", "--stdin"]).is_ok());
        assert!(
            Cli::try_parse_from(["donnabot-host", "bootstrap", "install-grant", "--stdin", "--root", "x"]).is_err()
        );
    }
}

#[cfg(test)]
mod mac_local_cli_tests {
    use super::*;
    use service::mac_local::{Scope, resolve_serve, touches_claude_settings};

    fn parse_serve(args: &[&str]) -> (Option<String>, bool) {
        let mut full = vec!["donnabot-host", "serve"];
        full.extend_from_slice(args);
        match Cli::try_parse_from(full).unwrap().cmd {
            Some(Sub::Serve { bind, only_this_mac, .. }) => (bind, only_this_mac),
            _ => panic!("expected serve"),
        }
    }

    #[test]
    fn serve_only_this_mac_without_bind_parses_and_listens_on_loopback() {
        let (bind, only) = parse_serve(&["--port", "19422", "--only-this-mac"]);
        assert_eq!((bind.as_deref(), only), (None, true));
        let (bind, scope) = resolve_serve(bind.as_deref(), only, true).unwrap();
        assert_eq!((bind.as_str(), scope), ("127.0.0.1", Scope::ThisMacOnly));
    }

    #[test]
    fn inherited_plist_arguments_never_reach_network_scope_as_mac_service() {
        // Plist antigo: só `serve --port N`.
        let (bind, only) = parse_serve(&["--port", "19422"]);
        assert!(resolve_serve(bind.as_deref(), only, true).is_err());
        let (bind, only) = parse_serve(&["--port", "19422", "--bind", "0.0.0.0"]);
        assert!(resolve_serve(bind.as_deref(), only, true).is_err());
        // Remoto/terminal mantém o padrão herdado.
        let (bind, scope) = resolve_serve(None, false, false).unwrap();
        assert_eq!((bind.as_str(), scope), ("0.0.0.0", Scope::Network));
    }

    #[test]
    fn uninstall_levels_parse_and_only_the_legacy_path_touches_claude() {
        let nivel = |args: &[&str]| {
            let mut full = vec!["donnabot-host", "uninstall"];
            full.extend_from_slice(args);
            match Cli::try_parse_from(full).unwrap().cmd {
                Some(Sub::Uninstall { nivel, .. }) => nivel,
                _ => panic!("expected uninstall"),
            }
        };
        assert!(touches_claude_settings(nivel(&[])));
        assert!(!touches_claude_settings(nivel(&["--nivel", "1"])));
        assert!(!touches_claude_settings(nivel(&["--nivel", "2"])));
        assert!(Cli::try_parse_from(["donnabot-host", "uninstall", "--nivel", "3"]).is_err());
    }
}
