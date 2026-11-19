//! Cópia de arquivos entre host e cópia de trabalho, para provedores de [`Modo::Montagem`].
//! O agente controla a cópia: links, hardlinks e pastas trocadas por link são recusados. Tudo
//! é relativo ao descritor da cópia já conferido (identidade do preparo), nunca ao caminho.
//!
//! [`Modo::Montagem`]: super::contrato::Modo::Montagem
use super::{
    contrato::{COPIA_MAX_BYTES, relativo},
    montagem::abrir_relativa,
};
use crate::fork::private_fs::Directory;
use anyhow::{Context as _, Result, ensure};
use std::{
    ffi::CString,
    fs::File,
    io::{Read as _, Write as _},
    os::{
        fd::{AsRawFd as _, FromRawFd as _},
        unix::{
            ffi::OsStrExt as _,
            fs::{MetadataExt as _, OpenOptionsExt as _},
        },
    },
    path::Path,
};

/// Host → cópia. Pastas que faltam nascem 0700; arquivo existente só se for comum e próprio.
pub(crate) fn para_dentro(copia: &File, origem: &Path, destino: &Path) -> Result<()> {
    let (pasta, nome) = dividir(destino)?;
    let mut fonte = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(origem)
        .context("copia_origem_ilegivel")?;
    let meta = fonte.metadata()?;
    ensure!(meta.is_file() && meta.len() <= COPIA_MAX_BYTES, "copia_origem_recusada");
    let alvo = Directory(abrir_relativa(copia, pasta, true)?);
    let mut arquivo = alvo.file(nome, libc::O_WRONLY | libc::O_CREAT)?;
    // Origem e destino no mesmo inode (caminho idêntico ou link): truncar apagaria a origem.
    let destino_meta = arquivo.metadata()?;
    ensure!((meta.dev(), meta.ino()) != (destino_meta.dev(), destino_meta.ino()), "copia_mesmo_arquivo");
    arquivo.set_len(0)?;
    copiar(&mut fonte, &mut arquivo)
}

/// Cópia → host. Origem comum, sem link nem hardlink; destino novo, nunca sobrescrito.
pub(crate) fn para_fora(copia: &File, origem: &Path, destino: &Path) -> Result<()> {
    let (pasta, nome) = dividir(origem)?;
    let fonte_dir = Directory(abrir_relativa(copia, pasta, false)?);
    let nome = CString::new(nome.as_bytes())?;
    // SAFETY: pai conferido sem links; O_NOFOLLOW recusa link final e O_NONBLOCK, FIFO.
    let fd = unsafe {
        libc::openat(
            fonte_dir.0.as_raw_fd(),
            nome.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    ensure!(fd >= 0, "copia_origem_recusada: {}", std::io::Error::last_os_error());
    // SAFETY: fd recém-aberto, transferido uma única vez.
    let mut fonte = unsafe { File::from_raw_fd(fd) };
    let meta = fonte.metadata()?;
    ensure!(meta.is_file() && meta.nlink() == 1 && meta.len() <= COPIA_MAX_BYTES, "copia_origem_recusada");
    ensure!(meta.uid() == crate::fork::private_fs::uid(), "copia_origem_de_outro_dono");
    let destino_dir = Directory::open(destino.parent().context("copia_destino_invalido")?, false)?;
    let destino_nome = destino.file_name().context("copia_destino_invalido")?;
    let mut arquivo = destino_dir.file(destino_nome, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)?;
    copiar(&mut fonte, &mut arquivo)
}

fn dividir(caminho: &Path) -> Result<(&Path, &std::ffi::OsStr)> {
    relativo(caminho)?;
    let nome = caminho.file_name().context("caminho_fora_da_copia")?;
    Ok((caminho.parent().unwrap_or(Path::new("")), nome))
}

fn copiar(fonte: &mut File, destino: &mut File) -> Result<()> {
    let mut total: u64 = 0;
    let mut bloco = vec![0_u8; 64 * 1024];
    loop {
        let n = fonte.read(&mut bloco)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        ensure!(total <= COPIA_MAX_BYTES, "copia_excede_limite");
        destino.write_all(&bloco[..n])?;
    }
    destino.sync_all()?;
    Ok(())
}
