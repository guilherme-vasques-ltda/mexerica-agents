//! Lado Hub do ciclo das pastas do motor; nenhum arquivo privado do job é aberto aqui.
use anyhow::{Result, ensure};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};
fn call(operation: &str, args: &[&OsStr]) -> Result<String> {
    let output = std::process::Command::new(super::motor_exec::helper_path())
        .arg("background")
        .arg(operation)
        .args(args)
        .env_clear()
        .output()?;
    ensure!(output.status.success(), "operação de pasta do background recusada como motor");
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
pub(crate) fn create() -> Result<PathBuf> {
    #[cfg(test)]
    if std::env::var_os("DONNABOT_MOTOR_TEST_HELPER").is_some() {
        return Ok(PathBuf::from(call("criar", &[])?));
    }
    super::motor_roots::prepare_workspace(&super::motor_roots::work().join("background"))?;
    Ok(PathBuf::from(call("criar", &[])?))
}
pub(crate) fn remove(path: &Path) -> Result<()> {
    call("remover", &[path.as_os_str()])?;
    Ok(())
}
pub(crate) fn has_auth(path: &Path) -> Result<bool> {
    match call("tem-auth", &[path.as_os_str()])?.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => anyhow::bail!("resposta inválida do ajudante"),
    }
}
