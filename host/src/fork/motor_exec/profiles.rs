//! Operações de perfil executadas só após a troca de identidade.
use super::fd::{self, Directory};
use anyhow::{Context, Result, ensure};
use std::{
    ffi::OsString,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
const ROOT: &str = "/var/lib/donnabot/motor/engine-setup/accounts";
fn account(id: &str) -> Result<PathBuf> {
    ensure!(
        id.len() == 67 && id.starts_with("ac_") && id[3..].bytes().all(|b| b.is_ascii_hexdigit()),
        "conta inválida"
    );
    Ok(Path::new(ROOT).join(id))
}
fn home(path: &Path) -> Result<PathBuf> {
    let relative = path.strip_prefix(ROOT)?;
    let parts: Vec<_> = relative.components().collect();
    ensure!(matches!(parts.len(), 2 | 3), "perfil fora da raiz do motor");
    let id = parts[0].as_os_str().to_str().context("conta inválida")?;
    account(id)?;
    let generation = parts[1].as_os_str().to_str().context("geração inválida")?;
    ensure!(generation == "legacy" || uuid::Uuid::parse_str(generation).is_ok(), "geração inválida");
    if parts.len() == 3 {
        ensure!(matches!(parts[2].as_os_str().to_str(), Some(".codex" | ".claude")), "perfil inválido");
    }
    fd::own(&Directory::open(path)?.0)?;
    Ok(if parts.len() == 2 { path.to_path_buf() } else { path.parent().context("perfil sem geração")?.to_path_buf() })
}
pub(super) fn protect(dir: &Directory, deleting: bool) -> Result<()> {
    fd::own(&dir.0)?;
    dir.mode(0o700)?;
    for name in dir.names()? {
        if let Ok(child) = dir.child(&name, None) {
            protect(&child, deleting)?;
            if deleting {
                remove(dir, &name, true)?;
            }
        } else {
            let file = dir.file(&name, libc::O_RDONLY, 0)?;
            fd::own(&file)?;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            if deleting {
                remove(dir, &name, false)?;
            }
        }
    }
    Ok(())
}
pub(super) fn run(args: &[OsString]) -> Result<()> {
    // SAFETY: ajudante single-thread, já sem capacidades e como motor.
    unsafe {
        libc::umask(0o077);
    }
    match args.first().and_then(|v| v.to_str()) {
        Some("login") if args.len() == 4 => {
            use std::io::Write;
            let deadline = args[2].to_str().context("prazo inválido")?;
            let _: i64 = deadline.parse().context("prazo inválido")?;
            let operation = args[3].to_str().context("operação inválida")?;
            uuid::Uuid::parse_str(operation)?;
            let path = account(args[1].to_str().context("conta inválida")?)?.join(operation);
            let dir = Directory::create(&path, 0o700)?;
            fd::own(&dir.0)?;
            drop(dir.file(".donnabot-profile".as_ref(), libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL, 0o600)?);
            let mut marker =
                dir.file(".donnabot-login-operation".as_ref(), libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL, 0o600)?;
            writeln!(marker, "{} {deadline}", path.file_name().context("operação inválida")?.to_string_lossy())?;
            marker.sync_all()?;
            println!("{}", path.display());
            Ok(())
        }
        Some("criar") if args.len() == 3 => {
            let base = account(args[1].to_str().context("conta inválida")?)?;
            ensure!(matches!(args[2].to_str(), Some("codex" | "claude")), "motor inválido");
            let path = base.join(uuid::Uuid::new_v4().to_string());
            let dir = Directory::create(&path, 0o700)?;
            fd::own(&dir.0)?;
            let marker =
                dir.file(".donnabot-profile".as_ref(), libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL, 0o600)?;
            drop(marker);
            let profile = if args[2] == "codex" { ".codex" } else { ".claude" };
            dir.child(profile.as_ref(), Some(0o700))?;
            println!("{}", path.join(profile).display());
            Ok(())
        }
        Some("avisar-login") if args.len() == 2 => {
            let path = account(args[1].to_str().context("conta inválida")?)?;
            println!("{}", super::login_cleanup::count_unmarked(&path)?);
            Ok(())
        }
        Some("cancelar-login") if args.len() == 3 => {
            let path = home(Path::new(&args[1]))?;
            super::login_cleanup::cancel(&path, args[2].to_str().context("prazo inválido")?)
        }
        Some("limpar-login") if args.len() == 4 => {
            let path = home(Path::new(&args[1]))?;
            let cancelled = match args[3].to_str() {
                Some("cancelado") => true,
                Some("vencido") => false,
                _ => anyhow::bail!("estado inválido"),
            };
            super::login_cleanup::remove(&path, args[2].to_str().context("prazo inválido")?, cancelled)
        }
        Some("substituir-login") if args.len() == 2 => {
            let path = home(Path::new(&args[1]))?;
            super::login_cleanup::remove_replaced(&path)
        }
        Some(operation @ ("validar" | "proteger" | "remover")) if args.len() == 2 => {
            let path = Path::new(&args[1]);
            let home = home(path)?;
            let dir = Directory::open(&home)?;
            fd::own(&dir.0)?;
            if operation != "validar" {
                protect(&dir, operation == "remover")?;
            }
            if operation == "remover" {
                remove(
                    &Directory::open(home.parent().context("perfil sem conta")?)?,
                    home.file_name().context("perfil sem nome")?,
                    true,
                )?;
            }
            Ok(())
        }
        _ => anyhow::bail!("conta-perfil criar ID codex|claude ou validar|proteger|remover PERFIL"),
    }
}

pub(super) fn remove(dir: &Directory, name: &std::ffi::OsStr, is_dir: bool) -> Result<()> {
    use std::os::{fd::AsRawFd, unix::ffi::OsStrExt};
    let name = std::ffi::CString::new(name.as_bytes())?;
    // SAFETY: nome enumerado de um componente, fd vivo; unlinkat não segue a folha.
    let rc = unsafe { libc::unlinkat(dir.0.as_raw_fd(), name.as_ptr(), if is_dir { libc::AT_REMOVEDIR } else { 0 }) };
    ensure!(rc == 0, "removendo perfil: {}", std::io::Error::last_os_error());
    Ok(())
}
