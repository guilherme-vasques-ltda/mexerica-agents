// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Node.js for the agents whose ACP adapter comes from npm (Claude, Codex, Gemini, …) and for
//! installing npm CLIs: the computer's own when it has one, else a checksum-pinned official
//! build that Codync adds into `~/.codync/node` when the user asks (an agent's setup, or
//! installing an npm CLI).

use crate::agent::{backends, registry};
use crate::fork::activity_code::ActivityKind;
use crate::fork::motor_install::{self, Reply, Request};
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};
use std::time::Duration;

const VERSION: &str = "24.22.3";
const MAX_ARCHIVE_BYTES: usize = 128 * 1024 * 1024;
static INSTALL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// nodejs.org's name for a platform, and its archive's SHA-256 (from `SHASUMS256.txt`).
fn archive(os: &str, arch: &str) -> Option<(&'static str, &'static str)> {
    Some(match (os, arch) {
        ("macos", "aarch64") => ("darwin-arm64", "ff6e17d4874b76d272212b908cc020d6e815fba5a6c7eaaa06add725840307fd"),
        ("macos", "x86_64") => ("darwin-x64", "977614af0504c81045e7fd85596dbca0e5f951b96681516dadafedcca8c447f7"),
        ("linux", "aarch64") => ("linux-arm64", "261af2af250dfd7e9f25cf151d1b2713a0adf8f10a9ad116c4947f68627affe8"),
        ("linux", "x86_64") => ("linux-x64", "061ec3e3062115705b56b51fa9cc330ab50d34b6969387d4e4b1a50c6809acc2"),
        ("windows", "aarch64") => ("win-arm64", "b3cc74a295b39d4f273271e96f58c06dd2b35757ca12abec64c7d0598b434f14"),
        ("windows", "x86_64") => ("win-x64", "e612af99f626ab415ea225af6bce5c971feb448cb5cae250bb1c3fa50c5f7d9f"),
        _ => return None,
    })
}

/// `~/.codync/node/<version>`: Codync's Node.
fn root() -> PathBuf {
    motor_install::root().join("node").join(VERSION)
}

/// Where `npm install -g` puts CLIs when it runs on Codync's Node; outlives Node updates.
fn global_prefix() -> PathBuf {
    motor_install::root().join("node").join("global")
}

/// The commands under an npm prefix: Windows keeps them at the top.
fn bin_dir(prefix: PathBuf) -> PathBuf {
    if cfg!(windows) { prefix } else { prefix.join("bin") }
}

/// Codync's Node and the CLIs installed with it, for the agent search path (after the
/// computer's own, so its Node wins).
pub fn search_dirs() -> [PathBuf; 2] {
    [bin_dir(root()), bin_dir(global_prefix())]
}

/// Node.js on a computer without its own: Codync's, missing or added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Missing,
    Added,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Added => "added",
        }
    }
}

/// `None` while the computer has its own Node; otherwise whether Codync's is there.
pub fn state() -> Option<State> {
    if backends::which("npx").is_some_and(|npx| !npx.starts_with(root())) {
        return None;
    }
    let installed = if crate::fork::motor_exec::configured() {
        motor_install::executable(&bin_dir(root()).join("node"))
    } else {
        root().join(".installed").exists()
    };
    Some(if installed { State::Added } else { State::Missing })
}

/// `codync-host node`: adds Codync's Node (an agent's setup step runs it in a terminal).
pub async fn add() -> Result<()> {
    if motor_install::orchestrating() {
        return match motor_install::invoke(Request::Node).await? {
            Reply::Status { code: 0 } => Ok(()),
            _ => anyhow::bail!("resposta inválida ao instalar Node"),
        };
    }
    motor_install::prepare()?;
    if state().is_none() {
        println!("This computer already has Node.js.");
        return Ok(());
    }
    install(&|line, _| println!("{line}")).await?;
    println!("Node.js {VERSION} is ready.");
    Ok(())
}

/// `codync-host npm …`: npm on the computer's Node, else on Codync's, whose global installs
/// land in `global_prefix`. Setup terminals install npm CLIs through it.
pub async fn npm(args: &[String]) -> Result<i32> {
    if motor_install::orchestrating() {
        return match motor_install::invoke(Request::Npm { args: args.to_vec() }).await? {
            Reply::Status { code } => Ok(code),
            _ => anyhow::bail!("resposta inválida ao executar npm"),
        };
    }
    motor_install::prepare()?;
    let mut command =
        if let Some(npm) = backends::which("npm") { tokio::process::Command::new(npm) } else { codync_npm().await? };
    if motor_install::worker() {
        motor_install::environment(&mut command);
        command.current_dir(motor_install::root());
    }
    let status = command.args(args).status().await.context("running npm")?;
    Ok(status.code().unwrap_or(1))
}

/// npm on Codync's Node (downloaded first), with its own Node first on PATH.
async fn codync_npm() -> Result<tokio::process::Command> {
    install(&|line, _| println!("{line}")).await?;
    let bin = bin_dir(root());
    let mut command = tokio::process::Command::new(bin.join(if cfg!(windows) { "npm.cmd" } else { "npm" }));
    let path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&path)))?;
    command.env("PATH", path).env("npm_config_prefix", global_prefix());
    Ok(command)
}

async fn install(progress: &impl Fn(&str, Option<ActivityKind>)) -> Result<()> {
    if motor_install::orchestrating() {
        return match motor_install::invoke(Request::Node).await? {
            Reply::Status { code: 0 } => Ok(()),
            _ => anyhow::bail!("resposta inválida ao instalar Node"),
        };
    }
    let _one_at_a_time = INSTALL.lock().await;
    let root = root();
    if tokio::fs::try_exists(root.join(".installed")).await.unwrap_or(false) {
        return Ok(());
    }
    let (platform, checksum) = archive(std::env::consts::OS, std::env::consts::ARCH)
        .context("Mexerica has no Node.js build for this computer: install Node.js from nodejs.org")?;
    let name = format!("node-v{VERSION}-{platform}");
    let file = format!("{name}.{}", if cfg!(windows) { "zip" } else { "tar.gz" });
    progress("Downloading Node.js…", Some(ActivityKind::Downloading { item: "Node.js".into() }));
    let mut response = crate::http()
        .get(format!("https://nodejs.org/dist/v{VERSION}/{file}"))
        .timeout(Duration::from_secs(600))
        .send()
        .await?
        .error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(bytes.len() + chunk.len() <= MAX_ARCHIVE_BYTES, "Node.js download exceeds size limit");
        bytes.extend_from_slice(&chunk);
    }
    ensure!(registry::sha256_hex(&bytes) == checksum, "Node.js download checksum mismatch");
    progress("Installing Node.js…", Some(ActivityKind::Installing { item: "Node.js".into() }));
    tokio::task::spawn_blocking(move || unpack(&root, &name, &file, &bytes)).await?.context("installing Node.js")
}

/// Blocking: extracts into a staging folder next to `root`, then moves it into place.
fn unpack(root: &Path, name: &str, file: &str, bytes: &[u8]) -> Result<()> {
    let parent = root.parent().context("Node.js folder has no parent")?;
    let stage = parent.join(format!(".staging-{}", uuid::Uuid::new_v4()));
    let placed = place(root, &stage, name, file, bytes);
    let _ = std::fs::remove_dir_all(&stage);
    if placed.is_ok() {
        remove_other_versions(root);
    }
    placed
}

fn place(root: &Path, stage: &Path, name: &str, file: &str, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(stage)?;
    let archive = stage.join(file);
    std::fs::write(&archive, bytes)?;
    let node = Path::new(name).join(if cfg!(windows) { "node.exe" } else { "bin/node" });
    registry::extract(&archive, stage, &node)?;
    let out = std::process::Command::new(stage.join(&node)).arg("--version").output()?;
    let runs = out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == format!("v{VERSION}");
    ensure!(runs, "the downloaded Node.js doesn't run on this computer");
    // Another process (a setup terminal's `codync-host npm`) may have finished first.
    if !root.join(".installed").exists() {
        let _ = std::fs::remove_dir_all(root);
        std::fs::rename(stage.join(name), root)?;
        if motor_install::worker() {
            motor_install::published_path(&root.join("bin/node"))?;
        }
        std::fs::write(root.join(".installed"), VERSION)?;
    }
    Ok(())
}

/// Older Nodes Codync downloaded (version-named folders; never `global` or a staging folder
/// another process is still filling).
fn remove_other_versions(root: &Path) {
    let Some(parent) = root.parent() else { return };
    let Ok(entries) = std::fs::read_dir(parent) else { return };
    for path in entries.flatten().map(|e| e.path()) {
        let versioned =
            path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(|c: char| c.is_ascii_digit()));
        if versioned && path != root && path.is_dir() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_platform_has_a_pinned_archive() {
        for os in ["macos", "linux", "windows"] {
            for arch in ["x86_64", "aarch64"] {
                let (platform, checksum) = archive(os, arch).expect("supported");
                assert_ne!(platform, "");
                assert!(checksum.len() == 64 && checksum.chars().all(|c| c.is_ascii_hexdigit()));
            }
        }
        assert!(archive("linux", "riscv64").is_none());
    }
}
