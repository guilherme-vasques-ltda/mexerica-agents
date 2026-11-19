//! Pastas efêmeras do job: criadas/removidas como motor, sem publicar dados do Hub.
use super::{
    fd::{self, Directory},
    profiles,
};
use anyhow::{Context, Result, ensure};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};
const ROOT: &str = "/var/lib/donnabot/trabalho/background";
fn valid(path: &Path) -> Result<()> {
    let id = path.strip_prefix(ROOT)?;
    ensure!(id.components().count() == 1, "job fora da raiz privada do motor");
    uuid::Uuid::parse_str(
        id.to_str().context("job inválido")?.strip_prefix("donnabot-background-").context("job inválido")?,
    )?;
    Ok(())
}
pub(super) fn run(args: &[OsString]) -> Result<()> {
    // SAFETY: ajudante single-thread já no UID do motor.
    unsafe {
        libc::umask(0o077);
    }
    match args.first().and_then(|v| v.to_str()) {
        Some("criar") if args.len() == 1 => {
            for ancestor in Path::new(ROOT).ancestors() {
                ensure!(
                    matches!(std::fs::symlink_metadata(ancestor.join(".codex")),Err(e) if e.kind() == std::io::ErrorKind::NotFound),
                    "configuração de ancestral impede isolar background"
                );
            }
            let path = Path::new(ROOT).join(format!("donnabot-background-{}", uuid::Uuid::new_v4()));
            let dir = Directory::create(&path, 0o700)?;
            fd::own(&dir.0)?;
            for name in ["work", "codex", "config", "cache", "data", "state"] {
                dir.child(name.as_ref(), Some(0o700))?;
            }
            println!("{}", path.display());
            Ok(())
        }
        Some("remover") if args.len() == 2 => {
            let path = PathBuf::from(&args[1]);
            valid(&path)?;
            let dir = Directory::open(&path)?;
            profiles::protect(&dir, true)?;
            profiles::remove(&Directory::open(Path::new(ROOT))?, path.file_name().context("job inválido")?, true)
        }
        Some("tem-auth") if args.len() == 2 => {
            let result =
                Directory::open(Path::new(&args[1])).and_then(|dir| dir.file("auth.json".as_ref(), libc::O_RDONLY, 0));
            let exists = match result {
                Ok(_) => true,
                Err(e)
                    if e.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
                {
                    false
                }
                Err(e) => return Err(e),
            };
            println!("{exists}");
            Ok(())
        }
        _ => anyhow::bail!("background criar | remover PASTA | tem-auth PERFIL"),
    }
}
