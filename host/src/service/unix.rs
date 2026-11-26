// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! The background service on macOS (a launchd agent) and Linux (a systemd user unit).

use super::{create_parent, current_exe, data_dir, home, run, wait_for_host_exit};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const LABEL: &str = crate::environment::Environment::current().service_label();
const UNIT: &str = crate::environment::Environment::current().systemd_unit();

/// Stops the installed job, preserving its configuration for a later start.
/// A manually started daemon is never mistaken for the service we own.
pub fn stop() -> Result<()> {
    if cfg!(target_os = "macos") {
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("gui/{}/{LABEL}", current_uid())])
            .stderr(Stdio::null())
            .status()?;
    } else if installed() {
        run("systemctl", &["--user", "stop", UNIT])?;
    }
    wait_for_host_exit()
}

pub fn start() -> Result<()> {
    if cfg!(target_os = "macos") {
        run("launchctl", &["bootstrap", &format!("gui/{}", current_uid()), &launchd_plist().to_string_lossy()])
    } else {
        run("systemctl", &["--user", "start", UNIT])
    }
}

/// Whether launchd or systemd started this process as the installed service, not a terminal or
/// the desktop app's development run.
pub fn started_by_service() -> bool {
    if cfg!(target_os = "macos") {
        // launchd names the job; a process started from a terminal or an app has another name.
        std::env::var("XPC_SERVICE_NAME").is_ok_and(|name| name == LABEL)
    } else {
        // systemd (248+) names the service's main process; a shell inherits its terminal's.
        std::env::var("SYSTEMD_EXEC_PID").is_ok_and(|pid| pid == std::process::id().to_string())
    }
}

/// O processo principal da unidade executa `exe` e é dono do socket que escuta em `port`
/// (`/proc`), sem perguntar nada a quem atende na porta. Leitura que falha ou consulta ao
/// systemd que passa do prazo conta como não.
#[cfg(target_os = "linux")]
pub async fn serves(port: u16, exe: &Path, deadline: tokio::time::Instant) -> bool {
    let query = tokio::process::Command::new("systemctl")
        .args(["--user", "show", "--property=MainPID", "--value", UNIT])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output();
    let Ok(Ok(out)) = tokio::time::timeout_at(deadline, query).await else { return false };
    String::from_utf8(out.stdout)
        .ok()
        .and_then(|pid| pid.trim().parse().ok())
        .is_some_and(|pid| crate::fork::linux_installer::process_serves(pid, port, exe).unwrap_or(false))
}

/// Refuse to replace one installation while restarting a service owned by another.
pub fn require_executable(expected: &Path) -> Result<()> {
    let configured = if cfg!(target_os = "macos") {
        let output = Command::new("/usr/bin/plutil")
            .args(["-extract", "ProgramArguments.0", "raw", "-o", "-"])
            .arg(launchd_plist())
            .output()?;
        if !output.status.success() {
            bail!("cannot read the installed host service");
        }
        PathBuf::from(String::from_utf8(output.stdout)?.trim())
    } else {
        let unit = std::fs::read_to_string(systemd_unit())?;
        let command =
            unit.lines().find_map(|line| line.strip_prefix("ExecStart=")).context("service has no ExecStart")?;
        let args = shlex::split(command).context("invalid service ExecStart")?;
        let exe = args.first().context("service has no executable")?;
        PathBuf::from(exe.replace("%%", "%").replace("$$", "$"))
    };
    if configured.canonicalize()? != expected.canonicalize()? {
        bail!("the installed service runs another host binary; run that binary's update command");
    }
    Ok(())
}

fn launchd_plist() -> PathBuf {
    home().join(format!("Library/LaunchAgents/{LABEL}.plist"))
}

fn systemd_unit() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| home().join(".config")).join("systemd/user").join(UNIT)
}

/// One quoted systemd word: a PATH with spaces (WSL's `Program Files`) stays whole, and `%`
/// is never read as a specifier. `ExecStart` also expands `$`, so it is doubled there.
fn systemd_quote(s: &str, exec: bool) -> String {
    let s = s.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%");
    format!("\"{}\"", if exec { s.replace('$', "$$") } else { s })
}

/// Installs and starts the host as a per-user background service. The current
/// PATH is captured so the service finds `npx`, `claude`, `codex`, …
pub fn install(port: u16) -> Result<()> {
    let exe = current_exe().context("locating the donnabot-host binary")?;
    install_executable(port, &exe.to_string_lossy(), None)
}

/// Same unit, with an explicit executable: the managed install (#812) points the unit at
/// the root-owned selector `/opt/donnabot/releases/ativo/donnabot-host`, never at the
/// slot the selector resolves to today. `bind` pins `serve --bind` on Linux; `None` writes
/// `127.0.0.1`, so the unit never listens beyond this computer without an explicit address
/// (on macOS the plist always binds `127.0.0.1` with `--only-this-mac`).
pub fn install_executable(port: u16, exe: &str, bind: Option<&str>) -> Result<()> {
    install_executable_inner(port, exe, bind, ModoInstalacao::Iniciar)
}

#[cfg(target_os = "linux")]
pub(crate) fn preparar_executable(port: u16, exe: &str, bind: Option<&str>) -> Result<()> {
    install_executable_inner(port, exe, bind, ModoInstalacao::Preparar)
}

#[derive(Clone, Copy)]
enum ModoInstalacao {
    Iniciar,
    #[cfg(target_os = "linux")]
    Preparar,
}

fn install_executable_inner(port: u16, exe: &str, bind: Option<&str>, modo: ModoInstalacao) -> Result<()> {
    let log = data_dir().join("host.log");
    crate::fork::private_fs::Directory::open(&data_dir(), true)?;
    if cfg!(target_os = "macos") {
        let data = data_dir();
        let plist = super::mac_local::plist(&super::mac_local::PlistSpec {
            label: LABEL,
            exe,
            port,
            path: &super::mac_local::service_path(&home()),
            data_dir: &data.to_string_lossy(),
            log: &log.to_string_lossy(),
        });
        let file = launchd_plist();
        create_parent(&file)?;
        let uid = current_uid();
        // Not loaded yet is the normal case here.
        let _ =
            Command::new("launchctl").args(["bootout", &format!("gui/{uid}/{LABEL}")]).stderr(Stdio::null()).status();
        wait_for_host_exit()?;
        std::fs::write(&file, plist).with_context(|| format!("writing {}", file.display()))?;
        run("launchctl", &["bootstrap", &format!("gui/{uid}"), &file.to_string_lossy()])?;
    } else {
        let path = std::env::var("PATH").unwrap_or_default();
        let motor = if crate::fork::motor_exec::configured() {
            format!(
                "UMask=0077\nExecStartPre={helper} cleanup\nExecStopPost={helper} cleanup\n",
                helper = crate::fork::motor_exec::helper_path().display()
            )
        } else {
            String::new()
        };
        let unit = systemd_unit_text(&motor, exe, port, bind, &path, &data_dir().to_string_lossy());
        let file = systemd_unit();
        create_parent(&file)?;
        std::fs::write(&file, unit).with_context(|| format!("writing {}", file.display()))?;
        std::fs::File::open(&file)?.sync_all().context("syncing systemd unit")?;
        // Persist each directory entry, including newly created ancestors, before acceptance.
        sync_ancestors(&file)?;
        run("systemctl", &["--user", "daemon-reload"])?;
        run("systemctl", &["--user", "stop", UNIT])?;
        wait_for_host_exit()?;
        match modo {
            ModoInstalacao::Iniciar => run("systemctl", &["--user", "enable", "--now", UNIT])?,
            #[cfg(target_os = "linux")]
            ModoInstalacao::Preparar => run("systemctl", &["--user", "enable", UNIT])?,
        }
        let user_dir = file.parent().context("systemd unit has no parent directory")?;
        let wants = user_dir.join("default.target.wants");
        std::fs::File::open(&wants)?.sync_all().with_context(|| format!("syncing {}", wants.display()))?;
        std::fs::File::open(user_dir)?.sync_all().with_context(|| format!("syncing {}", user_dir.display()))?;
        println!("Tip: `loginctl enable-linger $USER` keeps the host running while you're logged out.");
    }
    Ok(())
}

/// fsync dos ancestrais do arquivo, de baixo para cima. Um ancestral preexistente sem
/// leitura para o usuário (ex.: `/home` 0711) não pode ser aberto e nunca foi criado por
/// esta instalação: pular só esse caso mantém os demais erros fatais.
fn sync_ancestors(file: &Path) -> Result<()> {
    for dir in file.ancestors().skip(1) {
        let fd = match std::fs::File::open(dir) {
            Ok(fd) => fd,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => continue,
            Err(e) => return Err(e).with_context(|| format!("syncing {}", dir.display())),
        };
        fd.sync_all().with_context(|| format!("syncing {}", dir.display()))?;
    }
    Ok(())
}

/// Text of the per-user systemd unit (pure, so the `--bind` it pins is testable).
fn systemd_unit_text(motor: &str, exe: &str, port: u16, bind: Option<&str>, path: &str, data: &str) -> String {
    format!(
        "[Unit]\nDescription=Mexerica host\nAfter=network-online.target\n\n[Service]\n{motor}ExecStart={exe} serve --port {port} --bind {bind}\nEnvironment={path}\nEnvironment={data}\nRestart=always\nRestartSec=3\n\n[Install]\nWantedBy=default.target\n",
        exe = systemd_quote(exe, true),
        bind = systemd_quote(bind.unwrap_or(super::mac_local::LOOPBACK), true),
        path = systemd_quote(&format!("PATH={path}"), false),
        data = systemd_quote(&format!("MEXERICA_HOME={data}"), false),
    )
}

/// macOS: confirms the service is gone (`launchctl print` fails for absence) before returning
/// Ok, and reports a failed `bootout` or an unremovable plist. Linux: best effort, as before.
pub fn uninstall() -> Result<()> {
    if cfg!(target_os = "macos") {
        let target = format!("gui/{}/{LABEL}", current_uid());
        let bootout =
            Command::new("launchctl").args(["bootout", &target]).output().context("running launchctl bootout")?;
        let detail =
            format!("{}{}", String::from_utf8_lossy(&bootout.stdout), String::from_utf8_lossy(&bootout.stderr));
        let print = Command::new("launchctl").args(["print", &target]).output().context("running launchctl print")?;
        let output = format!("{}{}", String::from_utf8_lossy(&print.stdout), String::from_utf8_lossy(&print.stderr));
        super::mac_local::ensure_removed(&detail, &super::mac_local::classify_print(print.status.code(), &output))?;
        let file = launchd_plist();
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).with_context(|| format!("removing {}", file.display())),
        }
    } else {
        let _ = run("systemctl", &["--user", "disable", "--now", UNIT]);
        let _ = std::fs::remove_file(systemd_unit());
        let _ = run("systemctl", &["--user", "daemon-reload"]);
    }
    Ok(())
}

pub fn installed() -> bool {
    if cfg!(target_os = "macos") { launchd_plist().exists() } else { systemd_unit().exists() }
}

fn current_uid() -> String {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or_else(|| "501".into(), |s| s.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::{sync_ancestors, systemd_quote, systemd_unit_text};

    /// SEG P3-6: um ancestral preexistente sem leitura para o usuário (ex.: `/home`
    /// 0711) não pode derrubar o install; outros erros continuam fatais.
    #[test]
    fn sync_ancestors_tolera_ancestral_sem_leitura() {
        let raiz = std::env::temp_dir().join(format!("mx365-sync-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(raiz.join("a/b")).unwrap();
        let arquivo = raiz.join("a/b/unidade.service");
        std::fs::write(&arquivo, b"x").unwrap();
        sync_ancestors(&arquivo).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let modo = std::fs::Permissions::from_mode(0o000);
            std::fs::set_permissions(raiz.join("a"), modo).unwrap();
            assert!(sync_ancestors(&arquivo).is_ok());
            std::fs::set_permissions(raiz.join("a"), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        assert!(sync_ancestors(&raiz.join("inexistente/unidade.service")).is_err());
        std::fs::remove_dir_all(&raiz).unwrap();
    }

    /// #812 / SEG Other: a unidade do layout gerido (instalação limpa) escuta só em
    /// loopback; nunca `0.0.0.0` nem sem `--bind`.
    #[test]
    fn managed_unit_pins_loopback_bind() {
        let exe = "/opt/donnabot/releases/ativo/donnabot-host";
        let unit = systemd_unit_text("", exe, 19422, Some("127.0.0.1"), "/usr/bin", "/srv/dados");
        let exec = unit.lines().find_map(|l| l.strip_prefix("ExecStart=")).unwrap_or_default();
        let argv: Vec<String> = shlex::split(exec).unwrap_or_default();
        assert_eq!(argv, [exe, "serve", "--port", "19422", "--bind", "127.0.0.1"], "{exec}");
        assert!(!unit.contains("0.0.0.0"), "{unit}");
    }

    /// Sem endereço informado, a unidade grava `--bind 127.0.0.1`; um endereço explícito
    /// é gravado como veio (escuta na rede só por escolha de quem instala).
    #[test]
    fn unit_without_bind_pins_loopback_and_keeps_explicit_bind() {
        let argv = |bind| {
            let unit = systemd_unit_text("", "/x/donnabot-host", 19422, bind, "/usr/bin", "/srv/dados");
            let exec = unit.lines().find_map(|l| l.strip_prefix("ExecStart=")).unwrap_or_default().to_owned();
            shlex::split(&exec).unwrap_or_default()
        };
        assert_eq!(argv(None), ["/x/donnabot-host", "serve", "--port", "19422", "--bind", "127.0.0.1"]);
        assert_eq!(argv(Some("0.0.0.0")), ["/x/donnabot-host", "serve", "--port", "19422", "--bind", "0.0.0.0"]);
    }

    #[test]
    fn unit_values_stay_one_word() {
        let path = r#"/usr/bin:/mnt/c/Program Files/a"b\c:/x%y"#;
        assert_eq!(
            systemd_quote(&format!("PATH={path}"), false),
            r#""PATH=/usr/bin:/mnt/c/Program Files/a\"b\\c:/x%%y""#
        );
        let exe = systemd_quote("/home/me/My $bin/codync-host", true);
        assert_eq!(exe, r#""/home/me/My $$bin/codync-host""#);
        let parsed = shlex::split(&format!("{exe} serve")).unwrap();
        assert_eq!(parsed[0].replace("%%", "%").replace("$$", "$"), "/home/me/My $bin/codync-host");
    }
}
