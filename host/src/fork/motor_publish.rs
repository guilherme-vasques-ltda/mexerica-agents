//! Cópias públicas só de leitura; a fonte permanece privada no Hub.
use super::motor_exec::fd::{self, Directory};
use anyhow::{Result, ensure};
use std::{
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
fn put(dir: &Directory, name: &std::ffi::OsStr, bytes: &[u8]) -> Result<()> {
    fd::own(&dir.0)?;
    let mut file = dir.file(name, libc::O_WRONLY | libc::O_CREAT, 0o600)?;
    fd::own(&file)?;
    file.set_len(0)?;
    file.write_all(bytes)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o640))?;
    Ok(())
}
pub(crate) fn document(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = Directory::create(path.parent().ok_or_else(|| anyhow::anyhow!("documento sem pasta"))?, 0o2770)?;
    fd::own(&dir.0)?;
    dir.mode(0o2750)?;
    put(&dir, path.file_name().ok_or_else(|| anyhow::anyhow!("documento sem nome"))?, bytes)
}
fn copy(source: &Directory, dest: &Directory, budget: &mut u64) -> Result<()> {
    fd::own(&dest.0)?;
    dest.mode(0o2750)?;
    for name in source.names()? {
        if let Ok(child) = source.child(&name, None) {
            let target = dest.child(&name, Some(0o2750))?;
            copy(&child, &target, budget)?;
        } else {
            let mut input = source.file(&name, libc::O_RDONLY, 0)?;
            let size = input.metadata()?.len();
            ensure!(size <= *budget, "publicação excede 100 MiB");
            *budget -= size;
            let mut bytes = Vec::new();
            (&mut input).take(size + 1).read_to_end(&mut bytes)?;
            ensure!(bytes.len() as u64 == size, "fonte mudou durante publicação");
            put(dest, &name, &bytes)?;
        }
    }
    Ok(())
}
pub(crate) fn skill(source: &Path, id: &str) -> Result<PathBuf> {
    ensure!(
        !id.is_empty()
            && Path::new(id).components().all(|p| matches!(p, std::path::Component::Normal(_)))
            && !id.contains('/'),
        "skill inválida"
    );
    let parent = super::motor_roots::work().join("skills");
    let parent_dir = Directory::create(&parent, 0o2770)?;
    fd::own(&parent_dir.0)?;
    parent_dir.mode(0o2750)?;
    let path = parent.join(id);
    let src = Directory::open(source)?;
    let dst = Directory::create(&path, 0o2770)?;
    copy(&src, &dst, &mut (100 * 1024 * 1024))?;
    Ok(path)
}
