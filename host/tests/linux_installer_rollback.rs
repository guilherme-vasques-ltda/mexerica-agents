#![cfg(target_os = "linux")]

#[path = "../src/fork/linux_installer/rollback_data.rs"]
mod rollback_data;

use anyhow::{Context, Result};
use fs2::FileExt;
use std::{fs, os::unix::fs::MetadataExt, path::Path, process::Stdio, time::Duration};

struct Fixture(std::path::PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn serve_refused(data: &Path, home: &Path) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let output = runtime.block_on(async {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_donnabot-host"));
        command
            .args(["serve", "--bind", "127.0.0.1", "--port", "0"])
            .env_clear()
            .env("HOME", home)
            .env("MEXERICA_HOME", data)
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .kill_on_drop(true);
        tokio::time::timeout(Duration::from_secs(5), command.output())
            .await
            .context("serve concorrente não terminou")?
            .context("iniciando serve concorrente")
    })?;
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("another donnabot-host is already running"),
        "serve não recusou o lock: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn volta_mantem_inode_do_lock_ativo_e_recusa_serve_durante_restauracao() -> Result<()> {
    let fixture = Fixture(std::env::temp_dir().join(format!("donna-f4-lock-{}", uuid::Uuid::new_v4())));
    let data = fixture.0.join("home/data");
    let backup = fixture.0.join("backup");
    let after = fixture.0.join("after");
    for path in [&data, &backup, &after] {
        fs::create_dir_all(path)?;
    }
    let db = rusqlite::Connection::open(backup.join("codync.db"))?;
    db.execute_batch("CREATE TABLE kv(k TEXT PRIMARY KEY, v TEXT NOT NULL);")?;
    drop(db);
    fs::write(backup.join("host.lock"), b"lock historico ficticio")?;
    fs::write(data.join("host.lock"), b"lock ativo ficticio")?;
    fs::write(data.join("mensagem-pos-troca"), b"preservar")?;
    let lock = fs::OpenOptions::new().read(true).write(true).open(data.join("host.lock"))?;
    lock.try_lock_exclusive()?;
    let inode = lock.metadata()?.ino();
    let directory_inode = fs::metadata(&data)?.ino();
    rollback_data::restore(&data, &backup, &after, |source, destination| {
        // Exatamente a fase em que o código antigo já tinha trocado a árvore.
        serve_refused(&data, &fixture.0.join("home"))?;
        fs::copy(source, destination)?;
        serve_refused(&data, &fixture.0.join("home"))?;
        Ok(())
    })?;
    assert_eq!(fs::metadata(&data)?.ino(), directory_inode);
    assert_eq!(fs::metadata(data.join("host.lock"))?.ino(), inode);
    assert_eq!(fs::read(data.join("codync.db"))?, fs::read(backup.join("codync.db"))?);
    assert_eq!(fs::read(data.join("host.lock"))?, b"lock ativo ficticio");
    assert_eq!(fs::read(after.join("mensagem-pos-troca"))?, b"preservar");
    let contender = fs::OpenOptions::new().read(true).write(true).open(data.join("host.lock"))?;
    assert_eq!(contender.try_lock_exclusive().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
    drop(lock);
    contender.try_lock_exclusive()?;
    Ok(())
}
