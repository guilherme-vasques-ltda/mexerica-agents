//! OpenSSH absoluto, arquivos privados próprios e sondas negativas por conexão.
use super::tipos::{ConnectionPackage, RUNNER_PORT};
use anyhow::{Result, anyhow, ensure};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    process::{Child, Command},
};

pub const SSH: &str = "/usr/bin/ssh";
pub const SSH_VERSION: &str = "OpenSSH_9.2p1";
pub const RUNNER_STATE: &str = "/var/lib/donna-runner";
const LIMIT: usize = 64 * 1024;
const SENTINEL: &str = "DONNA_WORKPLACE_FORBIDDEN_V1";
pub struct PrivateDir {
    pub path: PathBuf,
}
impl PrivateDir {
    pub fn hub(root: &Path) -> Result<Self> {
        private_root(root)?;
        cleanup_stale(root)?;
        let path = root.join(uuid::Uuid::new_v4().to_string());
        private_create(&path)?;
        write_private(&path.join(".donna-workplace-owner"), std::process::id().to_string().as_bytes())?;
        Ok(Self { path })
    }
    pub fn file(&self, name: &str, content: &[u8]) -> Result<PathBuf> {
        ensure!(matches!(name, "identity" | "config" | "known_hosts" | "global_known_hosts"), "private_file_invalid");
        let path = self.path.join(name);
        write_private(&path, content)?;
        Ok(path)
    }
}
fn cleanup_stale(root: &Path) -> Result<()> {
    #[cfg(target_os = "linux")]
    for item in std::fs::read_dir(root)? {
        let item = item?;
        let name = item.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !uuid::Uuid::parse_str(name).is_ok_and(|id| id.to_string() == name) {
            continue;
        }
        let path = item.path();
        let meta = std::fs::symlink_metadata(&path)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            continue;
        }
        if private_root(&path).is_err() {
            continue;
        }
        let marker = path.join(".donna-workplace-owner");
        let Ok(meta) = std::fs::symlink_metadata(&marker) else {
            continue;
        };
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 16 {
            continue;
        }
        let Ok(pid) = std::fs::read_to_string(marker).and_then(|s| s.parse::<u32>().map_err(std::io::Error::other))
        else {
            continue;
        };
        if pid == 0 || Path::new("/proc").join(pid.to_string()).try_exists()? {
            continue;
        }
        std::fs::remove_dir_all(path)?;
    }
    #[cfg(not(target_os = "linux"))]
    let _ = root;
    Ok(())
}
impl Drop for PrivateDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
pub fn private_create(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new().mode(0o700).create(path)?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        anyhow::bail!("platform_not_admitted");
    }
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
pub fn private_root(path: &Path) -> Result<()> {
    if !path.try_exists()? {
        private_create(path)?;
    }
    let meta = std::fs::symlink_metadata(path)?;
    ensure!(meta.is_dir() && !meta.file_type().is_symlink(), "private_directory_invalid");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        // SAFETY: geteuid não recebe ponteiros nem modifica estado do processo.
        let uid = unsafe { libc::geteuid() };
        ensure!(meta.mode() & 0o777 == 0o700 && meta.uid() == uid, "private_directory_permissions");
    }
    Ok(())
}
pub fn write_private(path: &Path, content: &[u8]) -> Result<()> {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut f = options.open(path)?;
    f.write_all(content)?;
    f.sync_all()?;
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
pub struct Transport {
    _dir: PrivateDir,
    config: PathBuf,
    host: String,
    #[cfg(test)]
    executable: PathBuf,
}
impl Transport {
    pub fn new(root: &Path, package: &ConnectionPackage, resource: &str, private: &str) -> Result<Self> {
        package.validate(resource)?;
        let dir = PrivateDir::hub(root)?;
        let identity = dir.file("identity", private.as_bytes())?;
        let known = dir.file("known_hosts", format!("{}\n", package.known_hosts_line).as_bytes())?;
        let global = dir.file("global_known_hosts", b"")?;
        let quote = |p: &Path| -> Result<String> {
            let s = p.to_str().ok_or_else(|| anyhow!("private_path_invalid"))?;
            ensure!(!s.contains(['\n', '\r', '"', '\\', '%']), "private_path_invalid");
            Ok(format!("\"{s}\""))
        };
        let config = format!(
            "Host *\n  HostName {}\n  Port {}\n  User donna-runner\n  HostKeyAlias {}\n  IdentityFile {}\n  UserKnownHostsFile {}\n  GlobalKnownHostsFile {}\n  StrictHostKeyChecking yes\n  HostKeyAlgorithms ssh-ed25519\n  PubkeyAcceptedAlgorithms ssh-ed25519\n  PreferredAuthentications publickey\n  PasswordAuthentication no\n  KbdInteractiveAuthentication no\n  UpdateHostKeys no\n  CheckHostIP no\n  IdentitiesOnly yes\n  IdentityAgent none\n  BatchMode yes\n  ControlMaster no\n  ControlPath none\n  ControlPersist no\n  PermitLocalCommand no\n  VerifyHostKeyDNS no\n  CanonicalizeHostname no\n  ProxyCommand none\n  ProxyJump none\n  ForwardAgent no\n  ForwardX11 no\n  RequestTTY no\n  ConnectTimeout 4\n  ConnectionAttempts 1\n  ServerAliveInterval 3\n  ServerAliveCountMax 1\n  EscapeChar none\n  LogLevel DEBUG3\n",
            package.host,
            package.port,
            resource,
            quote(&identity)?,
            quote(&known)?,
            quote(&global)?
        );
        let config = dir.file("config", config.as_bytes())?;
        Ok(Self {
            _dir: dir,
            config,
            host: resource.into(),
            #[cfg(test)]
            executable: PathBuf::from(SSH),
        })
    }
    #[cfg(test)]
    pub fn with_test_executable(mut self, path: PathBuf) -> Self {
        self.executable = path;
        self
    }
    fn command(&self) -> Command {
        #[cfg(test)]
        let executable = &self.executable;
        #[cfg(not(test))]
        let executable = Path::new(SSH);
        let mut c = Command::new(executable);
        c.env_clear()
            .arg("-F")
            .arg(&self.config)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        c
    }
    pub fn connect(&self) -> Result<Child> {
        self.command()
            .args(["-T", "-W", &format!("127.0.0.1:{RUNNER_PORT}"), "--", &self.host])
            .spawn()
            .map_err(|_| anyhow!("ssh_unavailable"))
    }
    pub async fn probes(&self, check: &impl Fn() -> Result<()>) -> Result<()> {
        check()?;
        #[cfg(test)]
        let executable = &self.executable;
        #[cfg(not(test))]
        let executable = Path::new(SSH);
        let mut version = Command::new(executable);
        version.env_clear().arg("-V");
        let v = bounded(version, b"").await?;
        ensure!(v.stderr.split([' ', ',']).next() == Some(SSH_VERSION) && v.success, "ssh_version_not_admitted");
        for probe in [Probe::Exec, Probe::Shell, Probe::Pty, Probe::Remote, Probe::OtherTarget, Probe::OtherSocket] {
            check()?;
            let mut c = self.command();
            match probe {
                Probe::Exec => {
                    c.args(["-T", "--", &self.host, "printf DONNA_WORKPLACE_FORBIDDEN_V1"]);
                }
                Probe::Shell => {
                    c.args(["-T", "--", &self.host]);
                }
                Probe::Pty => {
                    c.args(["-tt", "--", &self.host]);
                }
                Probe::Remote => {
                    c.args(["-N", "-o", "ExitOnForwardFailure=yes", "-R", "0:127.0.0.1:9", "--", &self.host]);
                }
                Probe::OtherTarget => {
                    c.args(["-T", "-W", "127.0.0.1:9", "--", &self.host]);
                }
                Probe::OtherSocket => {
                    // O destino Unix remoto deve ser recusado, não apenas estar ausente.
                    let reservation = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
                    let port = reservation.local_addr()?.port();
                    drop(reservation);
                    let remote = format!("127.0.0.1:{port}:/nonexistent/donna-workplace-probe.sock");
                    c.args(["-N", "-o", "ExitOnForwardFailure=yes", "-L", &remote, "--", &self.host]);
                    let mut child = c.spawn()?;
                    let stderr = child.stderr.take().ok_or_else(|| anyhow!("ssh_pipe"))?;
                    let startup = format!("Local forwarding listening on 127.0.0.1 port {port}.");
                    let result = tokio::time::timeout(Duration::from_secs(5), async {
                        use tokio::io::AsyncBufReadExt as _;
                        let mut logs = String::new();
                        let mut reader = tokio::io::BufReader::new(stderr.take((LIMIT + 1) as u64));
                        loop {
                            let mut line = String::new();
                            ensure!(reader.read_line(&mut line).await? > 0, "ssh_probe_ambiguous");
                            logs.push_str(&line);
                            ensure!(logs.len() <= LIMIT, "ssh_output_limit");
                            if line.contains(&startup) {
                                break;
                            }
                        }
                        ensure!(child.try_wait()?.is_none(), "ssh_probe_ambiguous");
                        let socket = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await?;
                        loop {
                            let mut line = String::new();
                            ensure!(reader.read_line(&mut line).await? > 0, "ssh_probe_ambiguous");
                            logs.push_str(&line);
                            ensure!(logs.len() <= LIMIT, "ssh_output_limit");
                            if denied(probe, false, "", &logs) {
                                drop(socket);
                                return Ok::<_, anyhow::Error>(());
                            }
                        }
                    })
                    .await;
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    result.map_err(|_| anyhow!("ssh_probe_ambiguous"))??;
                    continue;
                }
            }
            let out = bounded(c, b"printf DONNA_WORKPLACE_FORBIDDEN_V1\nexit\n").await?;
            ensure!(denied(probe, out.success, &out.stdout, &out.stderr), "ssh_restrictions_unproven");
        }
        check()?;
        Ok(())
    }
    pub async fn remote_socket_probe(
        &self,
        op: uuid::Uuid,
        ready: bool,
        check: &impl Fn() -> Result<()>,
    ) -> Result<()> {
        ensure!(ready, "ssh_socket_probe_ambiguous");
        check()?;
        let listen = format!("{RUNNER_STATE}/probes/{op}/listen.sock");
        let forward = format!("{listen}:127.0.0.1:9");
        let mut c = self.command();
        c.args(["-N", "-o", "ExitOnForwardFailure=yes", "-R", &forward, "--", &self.host]);
        let out = bounded(c, b"").await?;
        check()?;
        ensure!(
            remote_socket_denied(ready, &listen, out.success, &out.stdout, &out.stderr),
            "ssh_socket_probe_ambiguous"
        );
        Ok(())
    }
}
/// Falha genérica só serve junto do controle positivo autenticado de bind/unlink
/// pelo runner, no diretório privado desta operação e mesmo usuário do SSH.
pub fn remote_socket_denied(ready: bool, listen: &str, success: bool, stdout: &str, stderr: &str) -> bool {
    ready
        && !success
        && stdout.is_empty()
        && stderr.contains("Authenticated to ")
        && stderr.contains(&format!("remote forward failure for: listen {listen}"))
        && stderr.contains(&format!("remote port forwarding failed for listen path {listen}"))
        && !stderr.contains("remote forward success")
}
#[derive(Clone, Copy)]
pub enum Probe {
    Exec,
    Shell,
    Pty,
    Remote,
    OtherTarget,
    OtherSocket,
}
/// Exit code sozinho nunca prova negação. Logs ficam no processo, não na API.
pub fn denied(probe: Probe, success: bool, stdout: &str, stderr: &str) -> bool {
    if success || stdout.contains(SENTINEL) || !stderr.contains("Authenticated to ") {
        return false;
    }
    match probe {
        Probe::Exec | Probe::Shell => {
            stdout.is_empty() && stderr.contains("Exit status 1") && stderr.contains("rcvd eof")
        }
        Probe::Pty => stderr.contains("PTY allocation request failed on channel"),
        Probe::Remote => {
            stderr.contains("remote forward failure for: listen 0") && stderr.contains("remote port forwarding failed")
        }
        Probe::OtherTarget | Probe::OtherSocket => {
            stderr.contains("open failed: administratively prohibited:")
                && !stderr.contains("connect failed: Connection refused")
                && !stderr.contains("connect failed: No such file")
        }
    }
}
struct Output {
    success: bool,
    stdout: String,
    stderr: String,
}
async fn bounded(mut command: Command, input: &[u8]) -> Result<Output> {
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = command.spawn().map_err(|_| anyhow!("ssh_unavailable"))?;
    let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("ssh_pipe"))?;
    let mut stdout = child.stdout.take().ok_or_else(|| anyhow!("ssh_pipe"))?.take((LIMIT + 1) as u64);
    let mut stderr = child.stderr.take().ok_or_else(|| anyhow!("ssh_pipe"))?.take((LIMIT + 1) as u64);
    let mut a = Vec::new();
    let mut b = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(6), async {
        stdin.write_all(input).await?;
        stdin.shutdown().await?;
        drop(stdin);
        let (_, _, status) = tokio::try_join!(stdout.read_to_end(&mut a), stderr.read_to_end(&mut b), child.wait())?;
        Ok::<_, anyhow::Error>(status)
    })
    .await;
    let status = if let Ok(result) = result {
        result?
    } else {
        let _ = child.kill().await;
        let _ = child.wait().await;
        anyhow::bail!("ssh_probe_ambiguous");
    };
    ensure!(a.len() <= LIMIT && b.len() <= LIMIT, "ssh_output_limit");
    Ok(Output { success: status.success(), stdout: String::from_utf8(a)?, stderr: String::from_utf8(b)? })
}
