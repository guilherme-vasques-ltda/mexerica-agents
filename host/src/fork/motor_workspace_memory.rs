//! Memória nativa Qwen: caminho vindo do bot nunca é aberto com os direitos do Hub.
use anyhow::{Result, ensure};
use std::path::Path;
pub(crate) fn delegate(workspace: &Path, on: bool) -> Result<()> {
    let executable = crate::service::current_exe()?;
    let mut command = std::process::Command::new(super::motor_exec::helper_path());
    command.args(["--cwd", "/", "--"]).arg(executable).arg("memoria-workspace").arg("--workspace").arg(workspace);
    if on {
        command.arg("--on");
    }
    let status = command.env_clear().status()?;
    ensure!(status.success(), "memória nativa recusada pelo motor");
    Ok(())
}
