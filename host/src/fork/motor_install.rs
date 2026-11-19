//! Instalação de CLIs pelo motor; o Hub conserva somente a orquestração.
use crate::fork::engine_accounts::Backend;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

mod transport;

/// Nenhum pedido permite escolher uma raiz de instalação ou um programa no Hub.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Request {
    Registry { agent: Value },
    Locate { name: String },
    Published { path: PathBuf },
    Node,
    Npm { args: Vec<String> },
    Engine { backend: Backend, directory: PathBuf, action: EngineAction },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Reply {
    Command { program: String, args: String },
    Path { path: Option<PathBuf> },
    Status { code: i32 },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EngineAction {
    Install,
    Existing,
}

use anyhow::{Context, Result, ensure};
use std::{
    path::{Component, Path},
    sync::OnceLock,
};
static WORKER: OnceLock<bool> = OnceLock::new();

pub(crate) fn worker() -> bool {
    *WORKER.get_or_init(|| {
        #[cfg(unix)]
        {
            use std::{
                io::Read,
                os::unix::fs::{MetadataExt, OpenOptionsExt},
            };
            // SAFETY: consulta de identidade; nenhuma variável de ambiente ativa o trabalhador.
            let uid = unsafe { libc::geteuid() };
            if !super::motor_exec::configured() || uid == 0 {
                return false;
            }
            let Ok(mut config) = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(super::motor_exec::CONFIG)
            else {
                return false;
            };
            if !config
                .metadata()
                .is_ok_and(|m| m.is_file() && m.uid() == 0 && m.mode() & 0o7777 == 0o644 && m.len() <= 4096)
            {
                return false;
            }
            let mut text = String::new();
            if config.read_to_string(&mut text).is_err() {
                return false;
            }
            let hub = text.lines().find_map(|l| l.trim().strip_prefix("hub_uid=")).and_then(|v| v.parse::<u32>().ok());
            hub.is_some_and(|hub| hub != uid)
                && std::fs::symlink_metadata(super::motor_roots::MOTOR)
                    .is_ok_and(|m| m.is_dir() && m.uid() == uid && m.mode() & 0o777 == 0o700)
        }
        #[cfg(not(unix))]
        false
    })
}
pub(crate) fn orchestrating() -> bool {
    super::motor_exec::configured() && !worker()
}
pub(crate) fn root() -> PathBuf {
    if super::motor_exec::configured() { super::motor_roots::MOTOR.into() } else { crate::service::data_dir() }
}
pub(crate) fn binaries() -> PathBuf {
    root().join("engine-setup/bin")
}

pub(crate) fn environment(command: &mut tokio::process::Command) {
    let root = root();
    let path = format!(
        "{}:{}/bin:/usr/local/bin:/usr/bin:/bin",
        crate::agent::node::search_dirs()[0].display(),
        root.join("node/global").display()
    );
    command
        .env_clear()
        .env("HOME", &root)
        .env("PATH", path)
        .env("TMPDIR", root.join("tmp"))
        .env("npm_config_cache", root.join("npm-cache"))
        .env("npm_config_prefix", root.join("node/global"))
        .env("npm_config_userconfig", "/dev/null")
        .env("npm_config_globalconfig", root.join("npm-globalrc"));
}
fn command() -> Result<tokio::process::Command> {
    let executable = std::env::current_exe()?;
    #[cfg(test)]
    let executable = std::env::var_os("DONNABOT_MOTOR_PROOF_HOST").map_or(executable, PathBuf::from);
    let mut command = super::motor_exec::command(executable, &root());
    command.arg("motor-install").current_dir("/");
    environment(&mut command);
    Ok(command)
}
pub(crate) async fn invoke(request: Request) -> Result<Reply> {
    static INSTALL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _install = match &request {
        Request::Engine { action: EngineAction::Existing, .. } => None,
        _ => Some(INSTALL.lock().await),
    };
    tokio::time::timeout(std::time::Duration::from_secs(25 * 60), transport::exchange(command()?, &request))
        .await
        .context("instalação do motor excedeu o prazo")?
}

// A consulta síncrona só lê metadados; nunca instala nem executa um binário encontrado.
pub(crate) fn locate(name: &str) -> Option<PathBuf> {
    if worker() {
        return local_executable(Path::new(name));
    }
    lookup(&Request::Locate { name: name.into() })
}
fn lookup(request: &Request) -> Option<PathBuf> {
    use std::{io::Write, process::Stdio};
    let mut command = command().ok()?.into_std();
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let bytes = serde_json::to_vec(request).ok()?;
    child.stdin.take()?.write_all(&bytes).ok()?;
    let output = child.wait_with_output().ok()?;
    if !output.status.success() || output.stdout.len() > 65536 {
        return None;
    }
    match serde_json::from_slice(&output.stdout).ok()? {
        Reply::Path { path } => path,
        _ => None,
    }
}

pub(crate) fn executable(path: &Path) -> bool {
    if !super::motor_exec::configured() {
        return path.is_absolute() && path.is_file();
    }
    !path.starts_with(crate::service::data_dir())
        && path.is_absolute()
        && allowed(path)
        && path.to_str().and_then(locate).is_some()
}

fn allowed(path: &Path) -> bool {
    path.is_absolute()
        && !path.components().any(|c| matches!(c, Component::ParentDir))
        && (path.starts_with(root())
            || ["/usr/bin", "/usr/local/bin", "/usr/local/lib", "/usr/lib", "/usr/share/nodejs", "/bin"]
                .iter()
                .any(|p| path.starts_with(p)))
}
fn local_executable(path: &Path) -> Option<PathBuf> {
    let mut paths = if path.is_absolute() {
        vec![path.to_path_buf()]
    } else {
        if path.components().count() != 1 {
            return None;
        }
        std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(path)).collect()
    };
    if let Some(backend) = match path.to_str() {
        Some("codex") => Some(Backend::Codex),
        Some("claude") => Some(Backend::Claude),
        _ => None,
    } && let Ok(installed) = super::engine_setup::install::installed_path(&binaries(), backend)
    {
        paths.insert(0, installed);
    }
    paths.into_iter().filter(|p| allowed(p)).find_map(|p| {
        let resolved = std::fs::canonicalize(&p).ok()?;
        if p.starts_with(root()) && !resolved.starts_with(root()) {
            return None;
        }
        if !allowed(&resolved) {
            return None;
        }
        let meta = std::fs::metadata(&resolved).ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o111 == 0 {
                return None;
            }
        }
        meta.is_file().then_some(resolved)
    })
}

/// O Hub consulta o motor; só o trabalhador abre os descritores privados sem links.
pub(crate) fn published(path: &Path) -> bool {
    if !path.starts_with(root()) {
        return false;
    }
    if worker() {
        return published_path(path).is_ok();
    }
    lookup(&Request::Published { path: path.into() }).as_deref() == Some(path)
}
pub(crate) fn published_path(path: &Path) -> Result<PathBuf> {
    #[cfg(unix)]
    {
        use super::motor_exec::fd::{self, Directory};
        use std::os::unix::fs::PermissionsExt;
        ensure!(worker() && path.starts_with(root()), "publicação fora da raiz do motor");
        let parent = Directory::open(path.parent().context("binário sem pasta")?)?;
        fd::own(&parent.0)?;
        let file = parent.file(path.file_name().context("binário sem nome")?, libc::O_RDONLY, 0)?;
        fd::own(&file)?;
        ensure!(file.metadata()?.permissions().mode() & 0o111 != 0, "binário não executável");
        Ok(path.into())
    }
    #[cfg(not(unix))]
    anyhow::bail!("publicação separada exige Unix")
}

pub(crate) fn prepare() -> Result<()> {
    #[cfg(unix)]
    if worker() {
        use super::motor_exec::fd::{self, Directory};
        for name in ["tmp", "npm-cache", "node", "node/global", "agents", "engine-setup", "engine-setup/bin"] {
            let dir = Directory::create(&root().join(name), 0o700)?;
            fd::own(&dir.0)?;
        }
    }
    Ok(())
}

pub(crate) async fn run() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use super::motor_exec::fd::{self, Directory};
        use std::io::Write;
        ensure!(super::motor_exec::configured(), "instalador exige modo separado");
        let dir = Directory::open(Path::new(super::motor_roots::MOTOR))?;
        fd::own(&dir.0)?;
        // SAFETY: getter e umask do processo instalador, antes de executar terceiros.
        ensure!(unsafe { libc::geteuid() } != 0, "instalador root recusado");
        // SAFETY: só altera a máscara do instalador, antes de executar terceiros.
        unsafe {
            libc::umask(0o077);
        }
        ensure!(worker(), "UID do Hub recusado no instalador");
        let mut response = transport::response_pipe()?;
        let request: Request = transport::request()?;
        if let Request::Locate { name } = &request {
            let reply = Reply::Path { path: local_executable(Path::new(name)) };
            serde_json::to_writer(&mut response, &reply)?;
            return Ok(());
        }
        if let Request::Published { path } = &request {
            let reply = Reply::Path { path: published_path(path).ok() };
            serde_json::to_writer(&mut response, &reply)?;
            return Ok(());
        }
        prepare()?;
        let reply = match request {
            Request::Locate { name } => Reply::Path { path: local_executable(Path::new(&name)) },
            Request::Published { path } => Reply::Path { path: published_path(&path).ok() },
            Request::Registry { agent } => {
                let cmd = Box::pin(crate::agent::registry::command(&agent, |_, _| {})).await?;
                Reply::Command { program: cmd.program, args: cmd.args }
            }
            Request::Node => {
                Box::pin(crate::agent::node::add()).await?;
                Reply::Status { code: 0 }
            }
            Request::Npm { args } => Reply::Status { code: Box::pin(crate::agent::node::npm(&args)).await? },
            Request::Engine { backend, directory, action } => {
                ensure!(directory.starts_with(super::motor_roots::accounts()), "perfil fora da raiz do motor");
                fd::own(&Directory::open(&directory)?.0)?;
                let path = match action {
                    EngineAction::Install => {
                        Some(Box::pin(super::engine_setup::install::install(&binaries(), backend, &directory)).await?)
                    }
                    EngineAction::Existing => {
                        Box::pin(super::engine_setup::install::existing(&binaries(), backend, &directory)).await
                    }
                };
                ensure!(path.as_deref().is_none_or(published), "binário fora da raiz do motor");
                Reply::Path { path }
            }
        };
        serde_json::to_writer(&mut response, &reply)?;
        response.flush()?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    anyhow::bail!("instalador separado exige Linux")
}

pub(crate) fn npm_prefix() -> String {
    if !super::motor_exec::configured() {
        return String::new();
    }
    let values = [
        ("npm_config_cache", root().join("npm-cache")),
        ("npm_config_prefix", root().join("node/global")),
        ("npm_config_userconfig", "/dev/null".into()),
        ("npm_config_globalconfig", root().join("npm-globalrc")),
    ];
    crate::shell::env_prefix(values.iter().map(|(k, v)| (*k, v.to_str().unwrap_or_default())))
}

pub(crate) fn check_npx(directory: &Path) -> Result<()> {
    if !worker() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use super::motor_exec::fd::Directory;
        let dir = Directory::open(directory)?;
        dir.file("package.json".as_ref(), libc::O_RDONLY, 0)?;
        let bins = directory.join("node_modules/.bin");
        for entry in std::fs::read_dir(&bins)? {
            let resolved = std::fs::canonicalize(entry?.path())?;
            published_path(&resolved)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
