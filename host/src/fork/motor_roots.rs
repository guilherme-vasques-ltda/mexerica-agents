//! D330 F3: contrato das raízes. Ausência de configuração preserva o legado.
use super::motor_exec;
use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};

pub(crate) const MOTOR: &str = "/var/lib/donnabot/motor";
pub(crate) const WORK: &str = "/var/lib/donnabot/trabalho";
pub(crate) fn work() -> PathBuf {
    PathBuf::from(WORK)
}
pub(crate) fn bot(id: &str) -> PathBuf {
    work().join("bots").join(id)
}
pub(crate) fn accounts() -> PathBuf {
    Path::new(MOTOR).join("engine-setup/accounts")
}
pub(crate) fn workspace(id: &str) -> PathBuf {
    bot(id).join("workspace")
}
pub(crate) fn uploads(id: &str) -> PathBuf {
    bot(id).join("uploads")
}

pub(crate) fn prepare_workspace(path: &Path) -> Result<()> {
    ensure!(path.starts_with(work()) && path != work(), "pasta fora da raiz de trabalho");
    #[cfg(unix)]
    {
        let dir = motor_exec::fd::Directory::create(path, 0o2770)?;
        motor_exec::fd::own(&dir.0)?;
        dir.mode(0o2770)?;
        Ok(())
    }
    #[cfg(not(unix))]
    anyhow::bail!("motor separado exige Unix")
}

pub(crate) fn profile(operation: &str, args: &[&std::ffi::OsStr]) -> Result<Vec<u8>> {
    let output =
        std::process::Command::new(motor_exec::helper_path()).arg("conta-perfil").arg(operation).args(args).output()?;
    ensure!(output.status.success(), "profile_invalid: operação de perfil recusada pelo motor");
    Ok(output.stdout)
}

pub(crate) fn conversation(space: &str, chat: &str) -> PathBuf {
    work().join("spaces").join(space).join("conversations").join(chat)
}
