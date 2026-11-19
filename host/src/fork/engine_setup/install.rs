//! Catálogo fechado e fixado. Download não executa script nem gerenciador de pacotes.
use crate::fork::engine_accounts::{Backend, managed::private_dir};
use crate::fork::motor_install::{self, EngineAction, Reply, Request};
use anyhow::{Result, anyhow, ensure};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

static CODEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static CLAUDE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const LIMIT: u64 = 400 * 1024 * 1024;

struct Staging(PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Artifact {
    url: String,
    sha256: &'static str,
    version: &'static str,
    member: Option<String>,
}

fn artifact(backend: Backend) -> Result<Artifact> {
    ensure!(cfg!(target_os = "linux"), "install_platform_unavailable: instalação guiada disponível no Hub Linux.");
    let arm = cfg!(target_arch = "aarch64");
    ensure!(arm || cfg!(target_arch = "x86_64"), "install_platform_unavailable");
    Ok(match backend {
        Backend::Codex => {
            let arch = if arm { "aarch64" } else { "x86_64" };
            let member = format!("codex-{arch}-unknown-linux-musl");
            Artifact {
                url: format!("https://github.com/openai/codex/releases/download/rust-v0.164.0/{member}.tar.gz"),
                sha256: if arm {
                    "29a507a2a3fd8f6cd94b96b762d281d299d15d7902dee6132d314b78cf9108eb"
                } else {
                    "90dd0b03b28afcb7fa733a4f73b413db2916a47347efd43fb669cc1c774ee8e4"
                },
                version: "0.164.0",
                member: Some(member),
            }
        }
        Backend::Claude => {
            let platform = if arm { "linux-arm64" } else { "linux-x64" };
            Artifact {
                url: format!(
                    "https://storage.googleapis.com/claude-code-dist-86c565f3-f756-42ad-8dfa-d59b1c096819/claude-code-releases/2.1.309/{platform}/claude"
                ),
                sha256: if arm {
                    "9796dd26899337f687739a0b39a7353b639f062a3436a7b941fcf19a1b757a9b"
                } else {
                    "4d704a8aecbe038affa3000f498ae17c1b4f753add8facb8764120affc99b658"
                },
                version: "2.1.309",
                member: None,
            }
        }
    })
}

/// Só a versão auditada é reutilizada; não altera instalação global incompatível.
pub(crate) async fn compatible(path: &Path, backend: Backend, directory: &Path) -> bool {
    let version = match backend {
        Backend::Codex => "0.164.0",
        Backend::Claude => "2.1.309",
    };
    let output = async {
        let mut child =
            crate::fork::motor_exec::spawn(super::process::command(path, directory, backend).arg("--version"))?;
        let _group = super::process::ProcessGroup(child.id());
        let mut bytes = Vec::new();
        child.stdout.take().ok_or_else(|| anyhow!("version_unavailable"))?.take(4097).read_to_end(&mut bytes).await?;
        ensure!(bytes.len() <= 4096 && child.wait().await?.success(), "version_unavailable");
        let text = String::from_utf8(bytes)?;
        Ok::<_, anyhow::Error>(text.split_whitespace().any(|word| word == version))
    };
    matches!(tokio::time::timeout(Duration::from_secs(10), output).await, Ok(Ok(true)))
}

pub(crate) fn installed_path(root: &Path, backend: Backend) -> Result<PathBuf> {
    let a = artifact(backend)?;
    Ok(root.join(format!("{}-{}-{}", backend.id(), a.version, std::env::consts::ARCH)).join(backend.id()))
}

pub(crate) async fn existing(root: &Path, backend: Backend, directory: &Path) -> Option<PathBuf> {
    if motor_install::orchestrating() {
        if root != motor_install::binaries() {
            return None;
        }
        return match motor_install::invoke(Request::Engine {
            backend,
            directory: directory.into(),
            action: EngineAction::Existing,
        })
        .await
        .ok()?
        {
            Reply::Path { path } => path.filter(|p| motor_install::published(p)),
            _ => None,
        };
    }
    if let Ok(a) = artifact(backend) {
        let installed =
            root.join(format!("{}-{}-{}", backend.id(), a.version, std::env::consts::ARCH)).join(backend.id());
        if (if crate::fork::motor_exec::configured() {
            motor_install::published(&installed)
        } else {
            installed.is_file()
        }) && compatible(&installed, backend, directory).await
        {
            return Some(installed);
        }
    }
    if crate::fork::motor_exec::configured() {
        return None;
    }
    let path = crate::agent::backends::which(backend.id())?;
    compatible(&path, backend, directory).await.then_some(path)
}

pub(crate) async fn install(root: &Path, backend: Backend, directory: &Path) -> Result<PathBuf> {
    if motor_install::orchestrating() {
        ensure!(root == motor_install::binaries(), "instalação em raiz antiga do Hub recusada");
        return match motor_install::invoke(Request::Engine {
            backend,
            directory: directory.into(),
            action: EngineAction::Install,
        })
        .await?
        {
            Reply::Path { path: Some(path) } if motor_install::published(&path) => Ok(path),
            _ => Err(anyhow!("instalação do motor não conferida")),
        };
    }
    let _lock = match backend {
        Backend::Codex => CODEX.lock().await,
        Backend::Claude => CLAUDE.lock().await,
    };
    if let Some(path) = existing(root, backend, directory).await {
        return Ok(path);
    }
    let a = artifact(backend)?;
    private_dir(root)?;
    let staging = root.join(format!(".stage-{}", uuid::Uuid::new_v4()));
    private_dir(&staging)?;
    let _cleanup = Staging(staging.clone());
    let result = async {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(240))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let allowed = attempt.previous().len() < 5
                    && attempt.url().scheme() == "https"
                    && matches!(
                        attempt.url().host_str(),
                        Some(
                            "github.com"
                                | "release-assets.githubusercontent.com"
                                | "objects.githubusercontent.com"
                                | "storage.googleapis.com"
                        )
                    );
                if allowed { attempt.follow() } else { attempt.error("download_redirect_refused") }
            }))
            .build()?;
        let mut response = client.get(&a.url).send().await?.error_for_status()?;
        ensure!(response.content_length().is_none_or(|n| n <= LIMIT), "install_size_limit");
        let download = staging.join("artifact");
        let mut file = tokio::fs::File::create(&download).await?;
        let mut size = 0_u64;
        let mut digest = Sha256::new();
        while let Some(chunk) = response.chunk().await? {
            size += chunk.len() as u64;
            ensure!(size <= LIMIT, "install_size_limit");
            digest.update(&chunk);
            file.write_all(&chunk).await?;
        }
        file.sync_all().await?;
        drop(file);
        ensure!(
            digest.finalize().iter().fold(String::new(), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            }) == a.sha256,
            "install_integrity_invalid"
        );
        promote(&staging, root, backend, &a, directory).await
    }
    .await;
    if staging.exists() {
        let _ = std::fs::remove_dir_all(staging);
    }
    result
}

async fn promote(staging: &Path, root: &Path, backend: Backend, a: &Artifact, directory: &Path) -> Result<PathBuf> {
    let download = staging.join("artifact");
    let mut input = std::fs::File::open(&download)?;
    let mut digest = Sha256::new();
    let mut bytes = vec![0_u8; 65536];
    let mut total = 0_u64;
    loop {
        let n = input.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        ensure!(total <= LIMIT, "install_size_limit");
        digest.update(&bytes[..n]);
    }
    let checksum = digest.finalize().iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    });
    ensure!(checksum == a.sha256, "install_integrity_invalid");
    let target = staging.join(backend.id());
    extract(&download, &target, a.member.as_deref())?;
    ensure!(compatible(&target, backend, directory).await, "cli_incompatible");
    std::fs::remove_file(download)?;
    let destination = root.join(format!("{}-{}-{}", backend.id(), a.version, std::env::consts::ARCH));
    // Prefixo imutável por versão; nunca substituir arquivo de turno ou instalação anterior.
    ensure!(!destination.exists(), "install_conflict: instalação existente preservada.");
    std::fs::rename(staging, &destination)?;
    Ok(destination.join(backend.id()))
}

fn extract(download: &Path, target: &Path, member: Option<&str>) -> Result<()> {
    let mut output = std::fs::OpenOptions::new().create_new(true).write(true).open(target)?;
    if let Some(member) = member {
        let file = std::fs::File::open(download)?;
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
        let mut found = false;
        for (index, entry) in archive.entries()?.enumerate() {
            ensure!(index < 64, "install_archive_invalid");
            let entry = entry?;
            ensure!(entry.header().entry_type().is_file(), "install_archive_invalid");
            ensure!(entry.path()?.as_ref() == Path::new(member) && !found, "install_archive_invalid");
            ensure!(entry.size() <= LIMIT * 2, "install_size_limit");
            std::io::copy(&mut entry.take(LIMIT * 2 + 1), &mut output)?;
            found = true;
        }
        ensure!(found, "install_archive_invalid");
    } else {
        let mut file = std::fs::File::open(download)?;
        std::io::copy(&mut file, &mut output)?;
    }
    output.flush()?;
    output.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn catalogo_fixado_e_extracao_nao_executa_instalador() {
        for backend in [Backend::Codex, Backend::Claude] {
            let a = artifact(backend).unwrap();
            assert_eq!(a.sha256.len(), 64);
            assert!(a.url.starts_with("https://"));
        }
        let root = std::env::temp_dir().join(format!("d164-artifact-{}", uuid::Uuid::new_v4()));
        private_dir(&root).unwrap();
        let source = root.join("source");
        std::fs::write(&source, b"fixture nao executavel").unwrap();
        let target = root.join("target");
        extract(&source, &target, None).unwrap();
        assert_eq!(std::fs::read(target).unwrap(), b"fixture nao executavel");
        let archive = root.join("bad.tar.gz");
        let encoder =
            flate2::write::GzEncoder::new(std::fs::File::create(&archive).unwrap(), flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o700);
        header.set_cksum();
        tar.append_link(&mut header, "codex", "/tmp/fora").unwrap();
        tar.into_inner().unwrap().finish().unwrap();
        assert!(extract(&archive, &root.join("bad"), Some("codex")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod promotion_tests {
    use super::*;
    #[tokio::test]
    async fn integridade_errada_preserva_anterior_e_instalacao_fixture_e_atomica() {
        let root = std::env::temp_dir().join(format!("d164-promote-{}", uuid::Uuid::new_v4()));
        private_dir(&root).unwrap();
        let profile = root.join("profile");
        private_dir(&profile).unwrap();
        let old = root.join("previous");
        std::fs::write(&old, b"anterior").unwrap();
        let staging = root.join("stage");
        private_dir(&staging).unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/engine_setup_cli.py");
        std::fs::copy(&fixture, staging.join("artifact")).unwrap();
        let mut artifact =
            Artifact { url: "https://fixture.invalid".into(), sha256: "invalid", version: "0.164.0", member: None };
        assert!(
            promote(&staging, &root, Backend::Codex, &artifact, &profile)
                .await
                .unwrap_err()
                .to_string()
                .starts_with("install_integrity_invalid")
        );
        assert_eq!(std::fs::read(&old).unwrap(), b"anterior");
        assert!(!staging.join("codex").exists());
        let digest = Sha256::digest(std::fs::read(&fixture).unwrap());
        let hex = digest.iter().fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        });
        // O catálogo de produção é estático; este artefato fictício existe só no teste.
        artifact.sha256 = Box::leak(hex.into_boxed_str());
        let installed = promote(&staging, &root, Backend::Codex, &artifact, &profile).await.unwrap();
        assert_eq!(std::fs::read(installed).unwrap(), std::fs::read(fixture).unwrap());
        assert_eq!(std::fs::read(old).unwrap(), b"anterior");
        assert!(!staging.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
