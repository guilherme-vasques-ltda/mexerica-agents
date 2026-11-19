//! Publicação de anexos: `UMask` privada, concessão explícita apenas no descritor.
use super::motor_exec::fd::{self, Directory};
use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
fn directory(root: &Path, id: &str, create: bool) -> Result<Directory> {
    ensure!(root.starts_with(super::motor_roots::work()), "anexo fora da raiz de trabalho");
    uuid::Uuid::parse_str(id)?;
    let dir = if create { Directory::create(root, 0o2770)? } else { Directory::open(root)? };
    fd::own(&dir.0)?;
    dir.mode(0o2750)?;
    let dir = dir.child(id.as_ref(), create.then_some(0o2750))?;
    fd::own(&dir.0)?;
    dir.mode(0o2750)?;
    Ok(dir)
}
pub(crate) fn append(root: &Path, id: &str, name: &str, offset: u64, bytes: &[u8]) -> Result<u64> {
    let dir = directory(root, id, true)?;
    let mut file = dir.file(name.as_ref(), libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND, 0o600)?;
    fd::own(&file)?;
    let len = file.metadata()?.len();
    let end = offset.checked_add(bytes.len() as u64).context("anexo grande demais")?;
    ensure!(end <= crate::chat::uploads::MAX_FILE, "anexo excede 100 MiB");
    if offset == len {
        file.write_all(bytes)?;
    } else {
        ensure!(end == len, "bloco fora de ordem");
    }
    file.set_permissions(std::fs::Permissions::from_mode(0o640))?;
    Ok(end)
}
fn open(root: &Path, id: &str) -> Result<(PathBuf, File)> {
    let dir = directory(root, id, false)?;
    if let Some(name) = dir.names()?.into_iter().next() {
        let file = dir.file(&name, libc::O_RDONLY, 0)?;
        fd::own(&file)?;
        return Ok((root.join(id).join(name), file));
    }
    anyhow::bail!("anexo vazio")
}
pub(crate) fn resolve(root: &Path, id: &str) -> Result<(PathBuf, serde_json::Value)> {
    let (path, file) = open(root, id)?;
    Ok((
        path.clone(),
        serde_json::json!({"id":id,"name":path.file_name().context("anexo sem nome")?.to_string_lossy(),"size":file.metadata()?.len()}),
    ))
}
pub(crate) fn read(root: &Path, id: &str, offset: u64, len: usize) -> Result<(Vec<u8>, u64)> {
    let (_, mut file) = open(root, id)?;
    let size = file.metadata()?.len();
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take((len as u64).min(crate::chat::uploads::MAX_FILE)).read_to_end(&mut bytes)?;
    Ok((bytes, size))
}
