//! Leitura de documentos públicos root-owned sem seguir links.
use anyhow::{Result, ensure};
use std::{
    fs::File,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
};

#[derive(Clone, Copy)]
pub(crate) enum Nome {
    Certidao,
    Status,
    Release,
}
impl Nome {
    fn componente(self) -> &'static std::ffi::CStr {
        match self {
            Self::Certidao => c"transacao.json",
            Self::Status => c"status.json",
            Self::Release => c"release.json",
        }
    }
}

fn sem_acl(file: &File) -> Result<()> {
    for nome in [c"system.posix_acl_access", c"system.posix_acl_default"] {
        // SAFETY: descritor válido e nome fixo; somente consulta de tamanho.
        let n = unsafe { libc::fgetxattr(file.as_raw_fd(), nome.as_ptr(), std::ptr::null_mut(), 0) };
        ensure!(
            n == -1 && matches!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ENODATA | libc::ENOTSUP)),
            "ACL em caminho da certidão"
        );
    }
    Ok(())
}

fn diretorio() -> Result<File> {
    diretorio_opcional()?.ok_or_else(|| anyhow::anyhow!("diretório da certidão indisponível"))
}

/// Errno da última chamada é ENOENT (ausência comprovada, não falha de acesso).
fn ausente() -> bool {
    std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT)
}

/// Diretório de publicação validado; `None` só quando ele (ou um ancestral) não existe.
fn diretorio_opcional() -> Result<Option<File>> {
    let mut dir = File::open("/")?;
    for parte in [c"opt", c"donnabot", c"releases"] {
        let meta = dir.metadata()?;
        ensure!(
            meta.is_dir() && meta.uid() == 0 && meta.gid() == 0 && meta.mode() & 0o7022 == 0,
            "ancestral da certidão inseguro"
        );
        sem_acl(&dir)?;
        // SAFETY: componente fixo relativo ao diretório validado; não segue links.
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                parte.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 && ausente() {
            return Ok(None);
        }
        ensure!(fd >= 0, "diretório da certidão indisponível: {}", std::io::Error::last_os_error());
        // SAFETY: posse do descritor novo passa uma vez ao File.
        dir = unsafe { File::from_raw_fd(fd) };
    }
    let meta = dir.metadata()?;
    ensure!(
        meta.is_dir() && meta.uid() == 0 && meta.gid() == 0 && meta.mode() & 0o7777 == 0o755,
        "diretório da certidão inseguro"
    );
    sem_acl(&dir)?;
    Ok(Some(dir))
}

/// Lê somente documentos fixos em diretório root-owned.
pub(crate) fn ler(nome: Nome) -> Result<Vec<u8>> {
    ler_opcional(nome)?.ok_or_else(|| anyhow::anyhow!("certidão indisponível"))
}

/// Como [`ler`], mas distingue a ausência comprovada (ENOENT do documento ou do diretório
/// de publicação: `None`) de qualquer outra falha de acesso, integridade ou I/O (erro).
pub(crate) fn ler_opcional(nome: Nome) -> Result<Option<Vec<u8>>> {
    let Some(dir) = diretorio_opcional()? else { return Ok(None) };
    // SAFETY: nome fixo sob diretório validado, sem seguir links nem bloquear.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            nome.componente().as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 && ausente() {
        return Ok(None);
    }
    ensure!(fd >= 0, "certidão indisponível: {}", std::io::Error::last_os_error());
    // SAFETY: posse do descritor novo passa uma vez ao File.
    let file = unsafe { File::from_raw_fd(fd) };
    let meta = file.metadata()?;
    ensure!(
        meta.is_file()
            && meta.uid() == 0
            && meta.gid() == 0
            && meta.nlink() == 1
            && meta.mode() & 0o7777 == 0o644
            && meta.len() <= 4096,
        "certidão insegura"
    );
    sem_acl(&file)?;
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 4096, "certidão grande demais");
    Ok(Some(bytes))
}

/// Confere o seletor root e admite somente os dois alvos fixos.
pub(crate) fn seletor() -> Result<String> {
    let dir = diretorio()?;
    // SAFETY: ponteiro válido para consulta do inode sem seguir o link.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    ensure!(
        // SAFETY: nome fixo em diretório previamente validado.
        unsafe { libc::fstatat(dir.as_raw_fd(), c"ativo".as_ptr(), &raw mut stat, libc::AT_SYMLINK_NOFOLLOW) } == 0,
        "seletor indisponível"
    );
    ensure!(
        stat.st_mode & libc::S_IFMT == libc::S_IFLNK && stat.st_uid == 0 && stat.st_gid == 0 && stat.st_nlink == 1,
        "seletor inseguro"
    );
    let mut bytes = [0u8; 64];
    // SAFETY: buffer válido e tamanho declarado; readlinkat não inclui NUL.
    let tamanho =
        unsafe { libc::readlinkat(dir.as_raw_fd(), c"ativo".as_ptr(), bytes.as_mut_ptr().cast(), bytes.len()) };
    let tamanho = usize::try_from(tamanho).unwrap_or(0);
    ensure!(tamanho > 0 && tamanho < bytes.len(), "seletor inválido");
    let alvo = std::str::from_utf8(&bytes[..tamanho])?;
    ensure!(matches!(alvo, "slot-a" | "slot-b"), "alvo de seletor inválido");
    Ok(alvo.to_owned())
}

/// Qualquer objeto com esse nome bloqueia o início, mesmo se estiver inválido.
pub(crate) fn exigir_sem_emergencia() -> Result<()> {
    let dir = diretorio()?;
    // SAFETY: consulta do nome fixo sem seguir links.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: nome fixo sem seguir links; ponteiro para estrutura local inicializada.
    let status =
        unsafe { libc::fstatat(dir.as_raw_fd(), c"emergencia".as_ptr(), &raw mut stat, libc::AT_SYMLINK_NOFOLLOW) };
    ensure!(
        status == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT),
        "serviço em bloqueio de emergência"
    );
    Ok(())
}
