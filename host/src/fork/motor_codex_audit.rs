//! Auditoria do perfil efetivo com os direitos do motor, sem conteúdo bruto no Hub.
use crate::agent::codex_audit::Warning;
use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};
const MAX: usize = 256 * 1024;
fn executable() -> Result<PathBuf> {
    #[cfg(test)]
    if let Some(path) = std::env::var_os("DONNABOT_MOTOR_TEST_AUDIT_EXE") {
        return Ok(path.into());
    }
    Ok(crate::service::current_exe()?)
}
pub(crate) fn check(home: &Path) -> Result<Vec<Warning>> {
    let output = std::process::Command::new(super::motor_exec::helper_path())
        .args(["--cwd", "/", "--"])
        .arg(executable()?)
        .arg("auditar-codex")
        .arg("--home")
        .arg(home)
        .env_clear()
        .output()?;
    ensure!(
        output.status.success() && output.stdout.len() <= MAX,
        "Não foi possível conferir a configuração do Codex com os direitos do motor."
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}
pub(crate) fn local(home: &Path) -> Result<()> {
    use std::io::Write;
    let warnings = crate::agent::codex_audit::check_local(home)?;
    let bytes = serde_json::to_vec(&warnings)?;
    ensure!(bytes.len() <= MAX, "resultado da auditoria Codex excede o limite");
    std::io::stdout().lock().write_all(&bytes)?;
    Ok(())
}
