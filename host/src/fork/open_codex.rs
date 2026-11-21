use crate::fork::private_fs::{self, Directory};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Component, Path, PathBuf},
};
use tokio::process::{Child, Command};

const SELECTION: &str = "codex-local.json";
const VERSION: &str = "0.164.0";
const AUTH_CONFIG: &[u8] = b"cli_auth_credentials_store = \"file\"\n";
const MAX_EXECUTABLE: u64 = 256 * 1024 * 1024;
const ADMIN_POLICY: &str = "/private/etc/codex";
/// Certificados públicos do macOS: o TLS do Codex usa este arquivo em vez do Chaveiro. O `.env` do perfil, que o Codex
/// leria antes de iniciar e poderia trocar este caminho, fica fora do alcance do processo isolado.
const CA_BUNDLE: &str = "/private/etc/ssl/cert.pem";
/// Conversa só de texto: desliga as ferramentas do Codex 0.164.0 antes de qualquer turno. O Seatbelt continua barrando
/// execução de outro programa e leitura fora do perfil; o turno também recusa qualquer item que não seja texto.
pub(crate) const CHAT_OVERRIDES: [&str; 19] = [
    "sandbox_mode=\"read-only\"",
    "approval_policy=\"never\"",
    "features.shell_tool=false",
    "features.unified_exec=false",
    "features.view_image=false",
    "features.multi_agent=false",
    "features.apps=false",
    "features.plugins=false",
    "features.memories=false",
    "features.image_generation=false",
    "features.hooks=false",
    "features.sleep_tool=false",
    "features.tool_suggest=false",
    "features.browser_use=false",
    "features.computer_use=false",
    "features.skill_mcp_dependency_install=false",
    "tools.experimental_request_user_input.enabled=false",
    "mcp_oauth_credentials_store=\"file\"",
    "web_search=\"disabled\"",
];
const SANDBOX: &str = "/usr/bin/sandbox-exec";
const POLICY: &str = r#"(version 1)
(deny default)
(allow process-exec (literal (param "CODEX")))
(allow process-info-pidinfo (target self))
(allow sysctl-read)
(allow file-read* file-map-executable
  (literal (param "CODEX"))
  (subpath "/System/Library")
  (subpath "/usr/lib")
  (subpath "/System/Cryptexes/OS")
  (subpath "/System/Volumes/Preboot/Cryptexes/OS")
  (subpath "/private/preboot/Cryptexes/OS")
  (subpath "/private/var/db/dyld"))
(allow file-read-metadata
  (path-ancestors (param "CODEX"))
  (path-ancestors (param "PROFILE"))
  (path-ancestors "/System/Volumes/Preboot/Cryptexes/OS")
  (path-ancestors "/private/preboot/Cryptexes/OS")
  (path-ancestors "/private/var/db/dyld"))
(allow file-read* (subpath (param "PROFILE")) (subpath "/usr/share") (subpath "/private/etc/codex"))
(allow file-read-data (literal "/"))
(allow file-read-metadata (literal "/") (literal "/dev") (literal "/etc") (literal "/private") (literal "/private/etc") (literal "/private/var") (literal "/private/var/run"))
(allow file-read* (literal "/dev/null") (literal "/dev/urandom") (literal "/dev/random"))
(allow file-write* (subpath (param "PROFILE")) (literal "/dev/null"))
(deny file-read* file-write* (literal (string-append (param "PROFILE") "/.codex/.env")))
"#;
const LOGIN_NETWORK: &str = r#"
(allow network-outbound (remote tcp "*:443"))
(allow network-outbound (literal "/private/var/run/mDNSResponder"))
(allow file-read-metadata (literal "/var"))
(allow file-read-data file-read-metadata (literal "/private/etc/ssl/cert.pem"))
(allow mach-lookup (global-name "com.apple.mDNSResponder") (global-name "com.apple.trustd.agent") (global-name "com.apple.system.opendirectoryd.libinfo"))
"#;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    executable: PathBuf,
    version: String,
    sha256: String,
}

impl Selection {
    fn load(open: &Path) -> Result<Self> {
        let owner_home = open.parent().context("perfil Open inválido")?;
        ensure!(open.file_name().is_some_and(|s| s == ".mexerica-open"), "perfil Open inválido");
        let bytes = private_fs::read_limited(&open.join(SELECTION), 4096)?.context("Codex não configurado na Open")?;
        let selection: Self = serde_json::from_slice(&bytes)?;
        ensure!(selection.version == VERSION, "versão de Codex não admitida");
        ensure!(
            selection.sha256.len() == 64
                && selection.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "impressão do executável inválida"
        );
        let path = &selection.executable;
        ensure!(
            path.is_absolute()
                && path.components().all(|part| matches!(part, Component::RootDir | Component::Normal(_)))
                && path
                    .file_name()
                    .is_some_and(|leaf| matches!(leaf.to_str(), Some("codex" | "codex-aarch64-apple-darwin")))
                && [
                    owner_home.join(".local/bin"),
                    owner_home.join(".local/share"),
                    PathBuf::from("/opt/homebrew"),
                    PathBuf::from("/usr/local/bin"),
                    PathBuf::from("/Applications/Codex.app"),
                ]
                .iter()
                .any(|root| path.starts_with(root)),
            "caminho do executável não admitido"
        );
        let uid = private_fs::uid();
        for parent in
            path.ancestors().skip(1).filter(|path| *path != Path::new("/")).collect::<Vec<_>>().into_iter().rev()
        {
            let meta = fs::symlink_metadata(parent)?;
            ensure!(
                meta.is_dir()
                    && (meta.uid() == 0 || meta.uid() == uid)
                    && (meta.mode() & 0o022 == 0 || (meta.uid() == 0 && meta.mode() & 0o1000 != 0)),
                "pai do executável inseguro"
            );
        }
        let mut file = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path)?;
        let meta = file.metadata()?;
        ensure!(
            meta.is_file()
                && meta.nlink() == 1
                && (meta.uid() == 0 || meta.uid() == uid)
                && meta.mode() & 0o7022 == 0
                && meta.mode() & 0o100 != 0
                && meta.len() > 0
                && meta.len() <= MAX_EXECUTABLE,
            "executável inválido"
        );
        let mut digest = Sha256::new();
        let mut bytes = 0;
        let mut buffer = [0; 16 * 1024];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            bytes += count as u64;
            ensure!(bytes <= MAX_EXECUTABLE, "executável acima do limite");
            digest.update(&buffer[..count]);
        }
        ensure!(bytes == meta.len() && hex(&digest.finalize()) == selection.sha256, "executável não confere");
        Ok(selection)
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn prepare_profile(open: &Path, directory: &Path) -> Result<()> {
    let accounts = open.join("home/.mexerica-open/engine-setup/accounts");
    ensure!(
        directory.starts_with(&accounts)
            && directory.file_name().is_some_and(|part| part == ".codex")
            && directory.components().all(|part| matches!(part, Component::RootDir | Component::Normal(_))),
        "perfil Codex fora dos dados da Open"
    );
    ensure!(Directory::open(directory, false)?.0.metadata()?.mode() & 0o7777 == 0o700, "perfil Codex sem modo privado");
    let config = directory.join("config.toml");
    match private_fs::read_limited(&config, 4096)? {
        Some(bytes) => ensure!(bytes == AUTH_CONFIG, "armazenamento do Codex não é exclusivo da Open"),
        None => private_fs::create_atomic(&config, AUTH_CONFIG)?,
    }
    Ok(())
}

fn command(executable: &Path, directory: &Path, login: bool) -> Result<Command> {
    use std::process::Stdio;
    let home = directory.parent().context("perfil Codex inválido")?;
    let tmp = home.join("tmp");
    Directory::open(&tmp, true)?;
    let policy = if login { format!("{POLICY}{LOGIN_NETWORK}") } else { POLICY.to_owned() };
    let mut command = Command::new(SANDBOX);
    command
        .args(["-p", &policy])
        .arg(format!("-DCODEX={}", executable.to_str().context("caminho Codex inválido")?))
        .arg(format!("-DPROFILE={}", home.to_str().context("caminho do perfil inválido")?))
        .arg("--")
        .arg(executable)
        .env_clear()
        .envs([("PATH", "/usr/bin:/bin"), ("TERM", "dumb")])
        .env("HOME", home)
        .env("CODEX_HOME", directory)
        .env("TMPDIR", tmp)
        .env("SSL_CERT_FILE", CA_BUNDLE)
        .current_dir(home)
        .stdin(if login { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .process_group(0);
    Ok(command)
}

pub(crate) struct AppServer {
    command: Command,
    pub(crate) executable: PathBuf,
}

impl AppServer {
    pub(crate) fn spawn(mut self) -> std::io::Result<Child> {
        self.command.spawn()
    }
}

pub(crate) async fn app_server(directory: &Path, chat: bool) -> Result<AppServer> {
    use std::time::Duration;
    use tokio::io::AsyncReadExt;
    ensure!(cfg!(target_os = "macos"), "open_codex_platform");
    ensure!(crate::environment::Environment::is_open(), "open_codex_unavailable");
    let profile = directory.to_owned();
    let selection = tokio::task::spawn_blocking(move || {
        let home = crate::service::home();
        let open = home.parent().context("open_codex_unavailable")?;
        let selection = Selection::load(open).inspect_err(|error| tracing::warn!(%error, "Codex da Open recusado"))?;
        prepare_profile(open, &profile)
            .inspect_err(|error| tracing::warn!(%error, "perfil do Codex da Open recusado"))?;
        if !admin_policy_safe(Path::new(ADMIN_POLICY)) {
            tracing::warn!(path = ADMIN_POLICY, "política do administrador do Codex aceita escrita de outros");
            anyhow::bail!("open_codex_unavailable");
        }
        if !root_owned(Path::new(CA_BUNDLE)) {
            tracing::warn!(path = CA_BUNDLE, "certificados do macOS não são só do root");
            anyhow::bail!("open_codex_unavailable");
        }
        Ok::<_, anyhow::Error>(selection)
    })
    .await
    .ok()
    .and_then(Result::ok)
    .context("open_codex_unavailable")?;
    let mut probe = command(&selection.executable, directory, false)?;
    let probe = async {
        let mut child = probe.arg("--version").spawn()?;
        let mut version = Vec::new();
        child.stdout.take().context("open_codex_unavailable")?.take(257).read_to_end(&mut version).await?;
        ensure!(version.len() <= 256 && child.wait().await?.success(), "open_codex_unavailable");
        ensure!(std::str::from_utf8(&version)?.trim() == format!("codex-cli {VERSION}"), "open_codex_unavailable");
        Ok::<_, anyhow::Error>(())
    };
    if !matches!(tokio::time::timeout(Duration::from_secs(10), probe).await, Ok(Ok(()))) {
        anyhow::bail!("open_codex_unavailable");
    }
    let mut command = command(&selection.executable, directory, true)?;
    command.args(["-c", "cli_auth_credentials_store=\"file\""]);
    if chat {
        let place = directory.parent().context("open_codex_unavailable")?.join("chat");
        Directory::open(&place, true)?;
        command.current_dir(place);
        for setting in CHAT_OVERRIDES {
            command.args(["-c", setting]);
        }
    }
    command.args(["app-server", "--listen", "stdio://"]);
    Ok(AppServer { command, executable: selection.executable })
}

/// O arquivo e cada pasta acima dele são do root e não aceitam escrita de grupo nem de outros. Só lê as permissões
/// Unix: ACLs não entram, e o arquivo pode mudar entre esta leitura e o uso pelo Codex.
fn root_owned(path: &Path) -> bool {
    path.ancestors().all(|part| {
        fs::symlink_metadata(part)
            .is_ok_and(|meta| meta.uid() == 0 && meta.mode() & 0o022 == 0 && meta.is_file() == (part == path))
    })
}

/// A política do administrador em /etc/codex vale só quando é do root e ninguém mais a altera.
fn admin_policy_safe(directory: &Path) -> bool {
    let safe = |meta: &fs::Metadata| meta.uid() == 0 && meta.mode() & 0o022 == 0;
    match fs::symlink_metadata(directory) {
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        Ok(meta) if !meta.is_dir() || !safe(&meta) => false,
        Ok(_) => ["managed_config.toml", "config.toml", "requirements.toml"].iter().all(|name| {
            match fs::symlink_metadata(directory.join(name)) {
                Err(error) => error.kind() == std::io::ErrorKind::NotFound,
                Ok(meta) => meta.is_file() && safe(&meta),
            }
        }),
    }
}

pub(crate) fn confirm_profile(directory: &Path) -> Result<()> {
    let home = crate::service::home();
    let open = home.parent().context("open_codex_unavailable")?;
    prepare_profile(open, directory)?;
    let meta = fs::symlink_metadata(directory.join("auth.json"))?;
    ensure!(
        meta.is_file()
            && meta.nlink() == 1
            && meta.uid() == private_fs::uid()
            && meta.mode() & 0o7777 == 0o600
            && meta.len() > 0,
        "open_codex_unavailable"
    );
    Ok(())
}

#[cfg(test)]
mod tests;
