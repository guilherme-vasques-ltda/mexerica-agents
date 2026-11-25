// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
use super::release;
use crate::service;
use anyhow::{Context, Result, bail, ensure};
use std::{future::Future, path::Path, time::Duration};

#[derive(Clone, Copy, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Method {
    Standalone,
    AppBundle,
    Homebrew,
    Development,
}

pub fn method(executable: &Path) -> Method {
    let path = executable.to_string_lossy();
    // The desktop app's macOS bundle, or its Windows `resources` folder.
    if path.contains(".app/Contents/") || (cfg!(windows) && path.contains("\\resources\\")) {
        Method::AppBundle
    } else if path.contains("/Cellar/") || path.contains("/linuxbrew/") {
        Method::Homebrew
    } else if path.contains("/target/debug/") || path.contains("/target/release/") || cfg!(debug_assertions) {
        Method::Development
    } else {
        Method::Standalone
    }
}

/// O atualizador só lê `version` e `busy`, do `/health` anônimo: nenhuma credencial sai para quem
/// estiver na porta.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

pub fn require_standalone(executable: &Path) -> Result<()> {
    match method(executable) {
        Method::Standalone => Ok(()),
        Method::AppBundle => bail!("this host belongs to the Mexerica app; update the app from Settings > Updates"),
        Method::Homebrew => bail!(
            "este host pertence ao Homebrew; a distribuição e atualização da Mexerica estão temporariamente indisponíveis"
        ),
        Method::Development => bail!("this is a development build; rebuild it instead of replacing it with a release"),
    }
}

/// A unidade já executa `target` (`require_executable`) e o processo anterior saiu
/// (`service::stop`); o /health anônimo confirma a versão que voltou a responder, e depois da
/// resposta `unit_serves` confere quem é o processo, antes de o atualizador descartar a cópia
/// de volta.
async fn verify_running(port: u16, version: &str, target: &Path) -> Result<()> {
    let target = target.canonicalize().context("resolving the host executable")?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if crate::health_local(port, HEALTH_TIMEOUT).await.is_some_and(|health| health["version"] == version)
            && unit_serves(port, &target, deadline).await
        {
            return Ok(());
        }
        ensure!(tokio::time::Instant::now() < deadline, "the updated host did not become healthy within 30 seconds");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Linux: pelo `/proc`, o processo principal da unidade executa `target` e é dono da porta.
#[cfg(target_os = "linux")]
async fn unit_serves(port: u16, target: &Path, deadline: tokio::time::Instant) -> bool {
    service::serves(port, target, deadline).await
}

/// Sem `/proc` (macOS, Windows), a confirmação fica só na versão.
#[cfg(not(target_os = "linux"))]
async fn unit_serves(_port: u16, _target: &Path, _deadline: tokio::time::Instant) -> bool {
    true
}

/// The backup lives beside the executable, so replacement and rollback use
/// atomic rename on the same filesystem. It is retained if rollback fails.
pub async fn replace<Stop, Stopped, Verify, Verified, Restore, Restored>(
    target: &Path,
    candidate: &Path,
    stop: Stop,
    verify: Verify,
    restore: Restore,
) -> Result<()>
where
    Stop: FnOnce() -> Stopped,
    Stopped: Future<Output = Result<()>>,
    Verify: FnOnce() -> Verified,
    Verified: Future<Output = Result<()>>,
    Restore: FnOnce() -> Restored,
    Restored: Future<Output = Result<()>>,
{
    let backup = target.with_file_name(format!(".codync-host-backup-{}", uuid::Uuid::new_v4()));
    std::fs::hard_link(target, &backup).context("creating the rollback copy")?;
    if let Err(error) = stop().await {
        let _ = std::fs::remove_file(&backup);
        return Err(error.context("old host could not be stopped; executable was not replaced"));
    }
    let update = async {
        std::fs::rename(candidate, target).context("replacing the host executable")?;
        verify().await
    }
    .await;
    if let Err(error) = update {
        // Renaming a live executable is safe on Unix; processes retain their
        // original inode. restore() then stops the failed service and restarts it.
        std::fs::rename(&backup, target)
            .with_context(|| format!("update failed ({error:#}); restore the saved binary at {}", backup.display()))?;
        restore()
            .await
            .with_context(|| format!("update failed ({error:#}); restored the old binary but could not restart it"))?;
        bail!("update failed; restored the previous host: {error:#}");
    }
    std::fs::remove_file(backup)?;
    Ok(())
}

pub async fn apply(release: &release::Release, target: &Path, port: u16, force: bool) -> Result<()> {
    require_standalone(target)?;
    let managed = service::installed();
    if managed {
        service::require_executable(target)?;
    }
    let old_health = crate::health_local(port, HEALTH_TIMEOUT).await;
    if !force {
        ensure!(
            old_health.as_ref().is_none_or(|h| h["busy"] == false),
            "host is busy or its activity is unknown; retry when idle, or use --force"
        );
    }
    if !managed {
        drop(service::lock_host().context("stop the manually started host before updating its executable")?);
    }
    let bytes = release::archive(release).await?;
    let stage = target.with_file_name(format!(".codync-host-update-{}", uuid::Uuid::new_v4()));
    let helpers: Vec<Helper> = HELPERS.iter().map(|name| Helper::new(target, name)).collect();
    let result = async {
        release::extract(&bytes, &release.platform, &stage)?;
        let mut shipped = vec![];
        for helper in &helpers {
            if release::extract_file(&bytes, &release.platform, helper.name, &helper.stage)? {
                shipped.push(helper);
            }
        }
        let output = tokio::process::Command::new(&stage).arg("--version").kill_on_drop(true).output();
        let output = tokio::time::timeout(Duration::from_secs(10), output).await??;
        ensure!(
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).trim() == format!("codync-host {}", release.version),
            "downloaded executable reported the wrong version"
        );
        // Recheck immediately before stopping; downloads can take minutes.
        if !force && managed {
            let current =
                crate::health_local(port, HEALTH_TIMEOUT).await.context("cannot confirm the running host is idle")?;
            ensure!(current["busy"] == false, "host became busy while downloading; retry after its work finishes");
        }
        replace(
            target,
            &stage,
            || async {
                if managed {
                    tokio::task::spawn_blocking(service::stop).await??;
                }
                // Swapped while the host is stopped, so the new host never starts an old helper.
                // A failed swap keeps the old helper rather than leaving the host stopped.
                for helper in &shipped {
                    if let Err(error) = helper.swap() {
                        tracing::warn!(helper = helper.name, error = format!("{error:#}"), "helper not updated");
                    }
                }
                Ok(())
            },
            || async {
                if managed {
                    tokio::task::spawn_blocking(service::start).await??;
                    verify_running(port, &release.version, target).await?;
                }
                Ok(())
            },
            || async {
                for helper in &helpers {
                    helper.restore();
                }
                if managed {
                    tokio::task::spawn_blocking(service::stop).await??;
                    tokio::task::spawn_blocking(service::start).await??;
                    verify_running(port, env!("CARGO_PKG_VERSION"), target).await?;
                }
                Ok(())
            },
        )
        .await
    }
    .await;
    let _ = std::fs::remove_file(stage);
    for helper in &helpers {
        helper.finish();
    }
    result
}

/// Linux helpers in the host's archive: the Remote screen helper and the computer-use driver.
/// The host starts them from beside itself, so an update replaces them there too.
const HELPERS: &[&str] = &["codync-screen", "cua-driver"];

/// Swaps a helper beside the host for the staged one, keeping a copy to roll back to.
struct Helper {
    name: &'static str,
    path: std::path::PathBuf,
    stage: std::path::PathBuf,
    backup: std::path::PathBuf,
    swapped: std::sync::atomic::AtomicBool,
    kept: std::sync::atomic::AtomicBool,
}

impl Helper {
    fn new(target: &Path, name: &'static str) -> Self {
        let id = uuid::Uuid::new_v4();
        Self {
            name,
            path: target.with_file_name(name),
            stage: target.with_file_name(format!(".{name}-update-{id}")),
            backup: target.with_file_name(format!(".{name}-backup-{id}")),
            swapped: std::sync::atomic::AtomicBool::new(false),
            kept: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn swap(&self) -> Result<()> {
        if self.path.exists() {
            std::fs::hard_link(&self.path, &self.backup).with_context(|| format!("saving {}", self.name))?;
        }
        std::fs::rename(&self.stage, &self.path).with_context(|| format!("replacing {}", self.name))?;
        self.swapped.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    /// Puts the previous helper back (or removes a new one that had none before). A copy that
    /// could not be put back is kept for `finish`, with its path in the log.
    fn restore(&self) {
        if !self.swapped.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        if self.backup.exists() {
            if let Err(error) = std::fs::rename(&self.backup, &self.path) {
                self.kept.store(true, std::sync::atomic::Ordering::Relaxed);
                tracing::warn!(
                    helper = self.name,
                    backup = %self.backup.display(),
                    error = format!("{error:#}"),
                    "helper not restored; previous copy kept"
                );
            }
        } else {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn finish(&self) {
        let _ = std::fs::remove_file(&self.stage);
        if !self.kept.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = std::fs::remove_file(&self.backup);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_keeps_its_backup_when_restore_fails() {
        let dir = std::env::temp_dir().join(format!("codync-helper-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let helper = Helper::new(&dir.join("host"), "helper");
        std::fs::write(&helper.backup, b"previous").unwrap();
        std::fs::create_dir_all(helper.path.join("busy")).unwrap();
        helper.swapped.store(true, std::sync::atomic::Ordering::Relaxed);
        helper.restore();
        helper.finish();
        assert_eq!(std::fs::read(&helper.backup).unwrap(), b"previous");

        std::fs::remove_dir_all(&helper.path).unwrap();
        let restored = Helper::new(&dir.join("host"), "helper");
        std::fs::write(&restored.backup, b"previous").unwrap();
        restored.swapped.store(true, std::sync::atomic::Ordering::Relaxed);
        restored.restore();
        restored.finish();
        assert_eq!(std::fs::read(&restored.path).unwrap(), b"previous");
        assert!(!restored.backup.exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
