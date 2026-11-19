#![cfg(unix)]

use fs2::FileExt;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
};

#[test]
fn migracao_direta_recusa_trava_ocupada_e_aceita_trava_herdada() -> anyhow::Result<()> {
    let data = std::env::temp_dir().join(format!("migrar-raizes-lock-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&data)?;
    fs::set_permissions(&data, fs::Permissions::from_mode(0o700))?;
    let lock = fs::OpenOptions::new().create_new(true).read(true).write(true).open(data.join("host.lock"))?;
    lock.try_lock_exclusive()?;
    let run = |stdin: Stdio, inherited: bool| -> anyhow::Result<std::process::Output> {
        let mut command = Command::new(env!("CARGO_BIN_EXE_donnabot-host"));
        command.arg("migrar-raizes").arg("--data-dir").arg(&data).stdin(stdin);
        if inherited {
            command.arg("--trava-stdin");
        }
        Ok(command.output()?)
    };
    let refused = run(Stdio::null(), false)?;
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("another donnabot-host is already running"));
    let invalid = run(Stdio::null(), true)?;
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("trava herdada não corresponde"));
    let other = fs::File::create(data.join("outra-trava"))?;
    let invalid_file = run(Stdio::from(other), true)?;
    assert!(!invalid_file.status.success());
    assert!(String::from_utf8_lossy(&invalid_file.stderr).contains("trava herdada não corresponde"));
    let occupied_fd = fs::OpenOptions::new().read(true).write(true).open(data.join("host.lock"))?;
    let occupied = run(Stdio::from(occupied_fd), true)?;
    assert!(!occupied.status.success());
    assert!(String::from_utf8_lossy(&occupied.stderr).contains("host.lock ocupado; migração recusada"));
    let inherited = run(Stdio::from(lock.try_clone()?), true)?;
    assert!(!inherited.status.success()); // A trava passou; o banco ausente é o próximo erro.
    assert!(String::from_utf8_lossy(&inherited.stderr).contains("codync.db"));
    drop(lock);
    let available = run(Stdio::null(), false)?;
    assert!(!available.status.success());
    assert!(String::from_utf8_lossy(&available.stderr).contains("codync.db"));
    fs::remove_dir_all(data)?;
    Ok(())
}
