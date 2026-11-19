//! A volta conserva o diretório ativo e o inode de host.lock, retido pelo chamador.
use anyhow::Result;
use std::{fs, path::Path};

pub(super) fn restore(
    data: &Path,
    backup: &Path,
    after: &Path,
    mut copy: impl FnMut(&Path, &Path) -> Result<()>,
) -> Result<()> {
    // Preserva mensagens posteriores à troca, sem abrir uma janela no lock ativo.
    for entry in fs::read_dir(data)? {
        let entry = entry?;
        if entry.file_name() != "host.lock" {
            fs::rename(entry.path(), after.join(entry.file_name()))?;
        }
    }
    // O lock da cópia é apenas histórico; jamais substitui o arquivo em uso.
    for entry in fs::read_dir(backup)? {
        let entry = entry?;
        if entry.file_name() != "host.lock" {
            copy(&entry.path(), &data.join(entry.file_name()))?;
        }
    }
    Ok(())
}
