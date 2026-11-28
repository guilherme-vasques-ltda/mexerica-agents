//! R7 (D302): o Hub baixa o wacli **oficial** na primeira vez que o plugin fica ligado.
//!
//! Versão e hashes fixados abaixo, tirados do `checksums.txt` oficial da release. Só
//! HTTPS do GitHub releases do `openclaw/wacli`; o SHA256 do pacote é conferido antes
//! de extrair, e o do executável antes de pôr no lugar. Nada que falhe no hash fica no
//! disco nem é executado. Atualizar = trocar a tabela (novo PR). O wacli roda como
//! processo separado (libsignal é GPL-3.0); nunca é linkado ao Hub.
//!
//! Destino: `<dados do Hub>/whatsapp/bin/wacli` (`wacli.exe` no Windows), o caminho
//! padrão que o supervisor do número (R2) procura. Quando fica pronto, o R2 sobe os
//! números conectados (`reconcile`).
use crate::{LockExt, hub::Hub};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

pub(super) const VERSION: &str = "0.25.1";
const BASE: &str = "https://github.com/openclaw/wacli/releases/download/v0.25.1";
/// Pacote e executável têm ~10 MB e ~30 MB; o limite só barra resposta absurda.
const LIMIT: u64 = 200 * 1024 * 1024;
/// Executável descompactado (~30 MB hoje). Acima disto, recusa; nunca corta em silêncio.
const EXE_LIMIT: u64 = 100 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(600);

/// Um pacote oficial: arquivo da release, SHA256 do arquivo (`checksums.txt`) e
/// SHA256 do executável de dentro dele (conferido ao extrair e no binário instalado).
#[derive(Clone, Copy, Debug)]
pub(super) struct Target {
    pub file: &'static str,
    pub sha256: &'static str,
    pub exe: &'static str,
    pub exe_sha256: &'static str,
}

/// `checksums.txt` de <https://github.com/openclaw/wacli/releases/tag/v0.25.1>, baixado em
/// 05/11/2026; hash do executável calculado no mesmo dia, de pacotes que bateram com ele.
/// Windows só tem amd64; Linux/Windows arm64 de 32 bits e outros sistemas não têm binário.
fn official() -> Option<Target> {
    let (file, sha256, exe, exe_sha256) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => (
            "wacli_0.25.1_linux_amd64.tar.gz",
            "53bbb450f0a47e3b080173d61fd6cec72241d8065bee17e592275983b7abd281",
            "wacli",
            "a211b0da0258858874ddd759a5357b21c581005d39bec64b326a4de7f7fef5cb",
        ),
        ("linux", "aarch64") => (
            "wacli_0.25.1_linux_arm64.tar.gz",
            "798ea62226c5017fe2d7c7642d614afae7ea1b30376081b529910bf8f9ab2ce2",
            "wacli",
            "c81300a31b615434ad0c6b1737c0458cc683ca9d99c056a535c28b7cbf3fd7c0",
        ),
        ("macos", "aarch64") => (
            "wacli_0.25.1_darwin_arm64.tar.gz",
            "fcb25052e2a4c6db6fb50eee5ee8c13ddf524111e47130dd22e101e879995e67",
            "wacli",
            "202689202a0537f0705fdab8406f004f2571204cfe95295f848374b1bf3a6334",
        ),
        ("macos", "x86_64") => (
            "wacli_0.25.1_darwin_amd64.tar.gz",
            "2ab332120c0e5fd7356c8f1bd9e536f67dbd620584cb39e88d45df79b3267cea",
            "wacli",
            "cb8dbe988df8125f057e903422626794228a6e0313087bf839d8984827adcb15",
        ),
        ("windows", "x86_64") => (
            "wacli_0.25.1_windows_amd64.zip",
            "bcb0238e4ad38b5795ceeac77bbdf50d40d63b82c75dd1ee9a441735089b3302",
            "wacli.exe",
            "5f4441812619fe0ca3e9ca1efa8fef3a5d83bf5745160fc9cfae7a13b8e58c42",
        ),
        _ => return None,
    };
    Some(Target { file, sha256, exe, exe_sha256 })
}

/// Mesmo nome que o R2 procura em `whatsapp/bin/`.
pub(super) fn installed_path(root: &Path) -> PathBuf {
    root.join("bin").join(if cfg!(windows) { "wacli.exe" } else { "wacli" })
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Status {
    /// Ainda não conferido neste processo.
    Unknown,
    Downloading,
    Ready,
    Failed(String),
}

pub(super) struct Downloader {
    root: PathBuf,
    base: String,
    target: Option<Target>,
    status: Mutex<Status>,
}

/// Mesma raiz padrão do R2 (`State::default`). Nos testes, sem alvo: nenhum teste
/// que passa pela RPC baixa da rede; os testes daqui usam um `Downloader` próprio.
static OFFICIAL: LazyLock<Arc<Downloader>> = LazyLock::new(|| {
    let target = if cfg!(test) { None } else { official() };
    Arc::new(Downloader::new(crate::service::data_dir().join("whatsapp"), BASE.to_owned(), target))
});

/// Estado do wacli para a leitura do app; com o plugin ligado, baixa se faltar.
pub(super) fn follow(hub: &Arc<Hub>, enabled: bool, retry: bool) -> Value {
    if crate::open_hub::require_executor().is_err() {
        return json!({"state": "unsupported", "message": "A Open não tem executor isolado para o wacli."});
    }
    let hub = hub.clone();
    Downloader::follow(&OFFICIAL, enabled, retry, move || super::number::reconcile(&hub))
}

impl Downloader {
    pub(super) fn new(root: PathBuf, base: String, target: Option<Target>) -> Self {
        Self { root, base, target, status: Mutex::new(Status::Unknown) }
    }

    /// Plugin desligado nunca baixa. Ligado: confere/baixa em segundo plano, uma vez;
    /// depois de falhar, só baixa de novo quando o dono pede (`retry`).
    pub(super) fn follow(
        this: &Arc<Self>,
        enabled: bool,
        retry: bool,
        on_ready: impl FnOnce() + Send + 'static,
    ) -> Value {
        let Some(target) = this.target else {
            return json!({"state": "unsupported", "message": "Sem wacli oficial para este computador."});
        };
        let mut status = this.status.locked();
        if *status == Status::Ready && !installed_path(&this.root).is_file() {
            // Apagado depois de pronto: conferir de novo.
            *status = Status::Unknown;
        }
        let start = enabled
            && match &*status {
                Status::Unknown => true,
                Status::Failed(_) => retry,
                Status::Downloading | Status::Ready => false,
            };
        if start && let Ok(runtime) = tokio::runtime::Handle::try_current() {
            *status = Status::Downloading;
            let me = this.clone();
            runtime.spawn(async move {
                let next = match me.ensure(target).await {
                    Ok(()) => {
                        *me.status.locked() = Status::Ready;
                        on_ready();
                        return;
                    }
                    Err(error) => {
                        tracing::warn!(error = %format!("{error:#}"), "whatsapp: wacli oficial não foi instalado");
                        Status::Failed(reason(&error))
                    }
                };
                *me.status.locked() = next;
            });
        }
        match &*status {
            Status::Downloading => json!({"state": "downloading", "version": VERSION}),
            Status::Ready => json!({"state": "ready", "version": VERSION}),
            Status::Failed(reason) => json!({"state": "failed", "version": VERSION, "reason": reason}),
            Status::Unknown => json!({"state": "off", "version": VERSION}),
        }
    }

    /// Binário já certo fica; faltando ou com outro hash, baixa o oficial por cima.
    async fn ensure(&self, target: Target) -> Result<()> {
        let current = installed_path(&self.root);
        let same = tokio::task::spawn_blocking(move || file_sha256(&current).ok()).await?;
        if same.as_deref() == Some(target.exe_sha256) {
            return Ok(());
        }
        install(&self.root, &self.base, target).await
    }
}

/// Motivo curto para a tela; o detalhe fica no log.
fn reason(error: &anyhow::Error) -> String {
    let text = error.to_string();
    if text.starts_with("wacli_") {
        return text;
    }
    if error.chain().any(|e| e.downcast_ref::<reqwest::Error>().is_some()) {
        return "wacli_download: não foi possível baixar do GitHub.".into();
    }
    "wacli_install: não foi possível instalar.".into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn file_sha256(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            return Ok(hex(&digest.finalize()));
        }
        digest.update(&buffer[..n]);
    }
}

fn private_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).context("wacli_dir")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).context("wacli_dir")?;
    }
    Ok(())
}

/// Pasta temporária dentro da raiz; some em qualquer saída. O ponto no nome a tira
/// da lista de pastas de bot do R2.
struct Staging(PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) async fn install(root: &Path, base: &str, target: Target) -> Result<()> {
    install_with(root, base, target, EXE_LIMIT).await
}

/// `exe_limit` só muda nos testes, para provar a recusa sem arquivo gigante.
pub(super) async fn install_with(root: &Path, base: &str, target: Target, exe_limit: u64) -> Result<()> {
    private_dir(root)?;
    let staging = Staging(root.join(format!(".wacli-{}", uuid::Uuid::new_v4())));
    private_dir(&staging.0)?;
    let archive = staging.0.join("download");
    let url = format!("{base}/{}", target.file);
    let mut response = crate::http().get(&url).timeout(TIMEOUT).send().await?.error_for_status()?;
    ensure!(response.content_length().is_none_or(|n| n <= LIMIT), "wacli_download: arquivo grande demais.");
    let mut file = std::fs::File::create(&archive)?;
    let mut digest = Sha256::new();
    let mut size = 0_u64;
    while let Some(chunk) = response.chunk().await? {
        size += chunk.len() as u64;
        ensure!(size <= LIMIT, "wacli_download: arquivo grande demais.");
        digest.update(&chunk);
        file.write_all(&chunk)?;
    }
    drop(file);
    // Antes de abrir o pacote: hash diferente do oficial = recusa (a pasta some no Drop).
    ensure!(hex(&digest.finalize()) == target.sha256, "wacli_integrity: o arquivo baixado não é o oficial.");
    let dir = staging.0.clone();
    let exe = tokio::task::spawn_blocking(move || -> Result<PathBuf> {
        let exe = dir.join("exe");
        extract(&archive, &exe, target.exe, exe_limit)?;
        ensure!(file_sha256(&exe)? == target.exe_sha256, "wacli_integrity: o executável não é o oficial.");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))?;
        }
        Ok(exe)
    })
    .await??;
    let bin = root.join("bin");
    private_dir(&bin)?;
    // Troca atômica: quem lê `bin/wacli` vê o antigo inteiro ou o novo inteiro.
    std::fs::rename(&exe, installed_path(root)).context("wacli_install: não foi possível pôr no lugar.")?;
    Ok(())
}

/// Copia só o executável `name` da raiz do pacote (tar.gz ou zip). O pacote já passou no hash.
fn extract(archive: &Path, out: &Path, name: &str, limit: u64) -> Result<()> {
    let mut output = std::fs::OpenOptions::new().create_new(true).write(true).open(out)?;
    let mut input = std::fs::File::open(archive)?;
    let mut magic = [0_u8; 2];
    input.read_exact(&mut magic)?;
    drop(input);
    let found = if magic == [0x1f, 0x8b] {
        from_tar(archive, &mut output, name, limit)?
    } else {
        from_zip(archive, &mut output, name, limit)?
    };
    ensure!(found, "wacli_integrity: o pacote não tem o executável.");
    output.sync_all()?;
    Ok(())
}

/// Copia até `limit` bytes; um byte a mais = recusa explícita.
fn copy_limited(input: impl Read, output: &mut std::fs::File, limit: u64) -> Result<()> {
    let copied = std::io::copy(&mut input.take(limit + 1), output)?;
    ensure!(copied <= limit, "wacli_integrity: executável grande demais.");
    Ok(())
}

fn from_tar(archive: &Path, output: &mut std::fs::File, name: &str, limit: u64) -> Result<bool> {
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(std::fs::File::open(archive)?));
    for entry in tar.entries()? {
        let entry = entry?;
        // `./wacli` no pacote oficial: só o arquivo comum na raiz, nunca link.
        let at_root = {
            let path = entry.path()?;
            let mut parts = path.components().filter(|c| !matches!(c, std::path::Component::CurDir));
            parts.next().is_some_and(|c| c.as_os_str() == name) && parts.next().is_none()
        };
        if entry.header().entry_type().is_file() && at_root {
            copy_limited(entry, output, limit)?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// Leitor mínimo de zip (só cabeçalhos locais, "stored" ou "deflate"), para o pacote
/// do Windows sem dependência nova. O hash do executável confere o resultado.
fn from_zip(archive: &Path, output: &mut std::fs::File, name: &str, limit: u64) -> Result<bool> {
    let bytes = std::fs::read(archive)?;
    let u16_at = |at: usize| bytes.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let mut at = 0_usize;
    while u32_at(at) == Some(0x0403_4b50) {
        let (Some(flags), Some(method), Some(size), Some(name_len), Some(extra_len)) =
            (u16_at(at + 6), u16_at(at + 8), u32_at(at + 18), u16_at(at + 26), u16_at(at + 28))
        else {
            bail!("wacli_integrity: pacote zip inválido.");
        };
        // Bit 0 = cifrado; bit 3 = tamanho só depois dos dados. O oficial não usa nenhum.
        ensure!(flags & 0b1001 == 0, "wacli_integrity: pacote zip inválido.");
        let start = at + 30 + usize::from(name_len) + usize::from(extra_len);
        let end = start + usize::try_from(size)?;
        let entry =
            bytes.get(at + 30..at + 30 + usize::from(name_len)).context("wacli_integrity: pacote zip inválido.")?;
        let data = bytes.get(start..end).context("wacli_integrity: pacote zip inválido.")?;
        if entry == name.as_bytes() {
            match method {
                0 => copy_limited(data, output, limit)?,
                8 => copy_limited(flate2::read::DeflateDecoder::new(data), output, limit)?,
                _ => bail!("wacli_integrity: pacote zip inválido."),
            }
            return Ok(true);
        }
        at = end;
    }
    Ok(false)
}

#[cfg(test)]
#[path = "binario_tests.rs"]
mod tests;
