//! D520-B1: criação privada ancorada no pai conferido, sem seguir links.
use anyhow::{Context, Result, ensure};
use std::{
    ffi::{CString, OsStr},
    fs::{File, Permissions},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, PermissionsExt},
        },
    },
    path::{Component, Path},
};

pub(crate) fn uid() -> u32 {
    // SAFETY: getter sem ponteiros ou efeitos colaterais.
    unsafe { libc::geteuid() }
}
fn name(value: &OsStr) -> Result<CString> {
    ensure!(!value.is_empty() && value != "." && value != ".." && !value.as_bytes().contains(&b'/'), "nome inválido");
    Ok(CString::new(value.as_bytes())?)
}
fn owned(fd: i32) -> Result<File> {
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: fd recém-aberto transferido uma única vez.
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn parent(file: &File, expected: u32) -> Result<()> {
    let m = file.metadata()?;
    ensure!(m.is_dir() && (m.uid() == 0 || m.uid() == expected), "dono/tipo do pai recusado");
    // /tmp de fixtures pode ser sticky root; não é default nem área de backup do produto.
    ensure!(m.mode() & 0o022 == 0 || (m.uid() == 0 && m.mode() & 0o1000 != 0), "pai gravável por terceiros");
    Ok(())
}

pub(crate) struct Directory(pub(crate) File);
impl Directory {
    pub(crate) fn open(path: &Path, create: bool) -> Result<Self> {
        Self::for_owner(path, create, uid(), None)
    }
    /// Só o instalador root informa UID/GID conferidos por NSS; nunca vem de request de cliente.
    pub(crate) fn for_owner(path: &Path, create: bool, expected: u32, gid: Option<u32>) -> Result<Self> {
        let absolute = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir()?.join(path) };
        let mut dir = File::open("/")?;
        let names: Vec<_> = absolute.components().filter(|c| *c != Component::RootDir).collect();
        ensure!(!names.is_empty(), "raiz não é diretório privado");
        for (index, part) in names.iter().enumerate() {
            parent(&dir, expected)?;
            let Component::Normal(value) = part else { anyhow::bail!("componente de pasta privado inválido") };
            let value = name(value)?;
            // SAFETY: pai vivo e nome único; cada ancestral recusa symlinks.
            let mut fd = unsafe {
                libc::openat(
                    dir.as_raw_fd(),
                    value.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            let mut created = false;
            if fd < 0 && create && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
                // SAFETY: criação relativa ao pai já conferido, modo privado desde o nascimento.
                let rc = unsafe { libc::mkdirat(dir.as_raw_fd(), value.as_ptr(), 0o700) };
                ensure!(rc == 0, "criando pasta privada: {}", std::io::Error::last_os_error());
                created = true;
                // SAFETY: reabre sem seguir link e confere o fd antes de conteúdo.
                fd = unsafe {
                    libc::openat(
                        dir.as_raw_fd(),
                        value.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
            }
            let next = owned(fd)?;
            if created {
                if expected != uid() {
                    let gid = gid.context("GID exigido para criação pelo instalador")?;
                    ensure!(
                        // SAFETY: só o fd recém-criado muda de dono; existente nunca recebe chown.
                        unsafe { libc::fchown(next.as_raw_fd(), expected, gid) } == 0,
                        "dono da pasta nova recusado"
                    );
                }
                next.set_permissions(Permissions::from_mode(0o700))?;
            }
            parent(&next, expected)?;
            if index + 1 == names.len() {
                ensure!(next.metadata()?.uid() == expected, "diretório privado pertence a outro dono");
                if create {
                    next.set_permissions(Permissions::from_mode(0o700))?;
                    ensure!(next.metadata()?.mode() & 0o7777 == 0o700, "modo privado de diretório indisponível");
                }
            }
            dir = next;
        }
        Ok(Self(dir))
    }
    pub(crate) fn file(&self, value: &OsStr, flags: i32) -> Result<File> {
        parent(&self.0, uid())?;
        ensure!(self.0.metadata()?.uid() == uid(), "pai privado de outro dono");
        let value = name(value)?;
        // Nunca O_TRUNC antes de verificar dono/tipo/link e modo no descritor.
        ensure!(flags & libc::O_TRUNC == 0, "truncamento antes da conferência recusado");
        // SAFETY: pai e nome conferidos, O_NOFOLLOW/O_NONBLOCK recusam links/especiais bloqueantes.
        let file = owned(unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                value.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                0o600,
            )
        })?;
        let m = file.metadata()?;
        ensure!(m.is_file() && m.uid() == uid() && m.nlink() == 1, "arquivo privado: dono/tipo/hardlink recusado");
        file.set_permissions(Permissions::from_mode(0o600))?;
        ensure!(file.metadata()?.mode() & 0o7777 == 0o600, "modo privado indisponível");
        Ok(file)
    }
}

/// Inspeção sem criação/chmod antes das cercas PL1. Nenhum conteúdo é aberto.
pub(crate) fn inspect(path: &Path) -> Result<()> {
    let absolute = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir()?.join(path) };
    let parts: Vec<_> = absolute.components().filter(|c| *c != Component::RootDir).collect();
    ensure!(!parts.is_empty(), "raiz não é diretório privado");
    // Validar também componentes depois de uma eventual ausência, sem criar nada.
    ensure!(parts.iter().all(|p| matches!(p, Component::Normal(_))), "componente de pasta privado inválido");
    let mut dir = File::open("/")?;
    for (index, part) in parts.iter().enumerate() {
        parent(&dir, uid())?;
        let Component::Normal(value) = part else { anyhow::bail!("componente de pasta privado inválido") };
        let value = name(value)?;
        // SAFETY: pai conferido e vivo; cada ancestral é aberto sem seguir links.
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                value.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
            // O pai existente conserva política de ancestral, não de folha privada.
            // A criação futura conferirá todos os descritores e o dono da folha criada.
            return Ok(());
        }
        let next = owned(fd)?;
        parent(&next, uid())?;
        if index + 1 == parts.len() {
            let metadata = next.metadata()?;
            ensure!(
                metadata.uid() == uid() && metadata.mode() & 0o022 == 0,
                "diretório privado: dono ou modo impróprio"
            );
        }
        dir = next;
    }
    Ok(())
}

pub(crate) fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let dir = Directory::open(path.parent().context("pai ausente")?, false)?;
    let mut file = dir.file(path.file_name().context("nome ausente")?, libc::O_WRONLY | libc::O_CREAT)?;
    file.set_len(0)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub(crate) fn create_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let dir = Directory::open(path.parent().context("pai ausente")?, false)?;
    let target = name(path.file_name().context("nome ausente")?)?;
    let temporary = format!(".identity-{}", uuid::Uuid::new_v4());
    let temp = name(OsStr::new(&temporary))?;
    let mut created = false;
    let result = (|| {
        let mut file = dir.file(OsStr::new(&temporary), libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)?;
        created = true;
        file.write_all(bytes)?;
        file.sync_all()?;
        ensure!(
            // SAFETY: ambos nomes relativos ao mesmo pai vivo. linkat não sobrescreve destino existente.
            unsafe { libc::linkat(dir.0.as_raw_fd(), temp.as_ptr(), dir.0.as_raw_fd(), target.as_ptr(), 0) } == 0,
            "publicação exclusiva da identidade recusada"
        );
        Ok(())
    })();
    if created {
        // SAFETY: remove só o temporário exclusivo desta chamada, no pai capturado.
        unsafe { libc::unlinkat(dir.0.as_raw_fd(), temp.as_ptr(), 0) };
    }
    dir.0.sync_all()?;
    result
}

pub(crate) fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    read_bytes(path, None)
}

pub(crate) fn read_limited(path: &Path, limit: u64) -> Result<Option<Vec<u8>>> {
    read_bytes(path, Some(limit))
}

fn read_bytes(path: &Path, limit: Option<u64>) -> Result<Option<Vec<u8>>> {
    use std::io::Read;
    let directory = match Directory::open(path.parent().context("pai ausente")?, false) {
        Ok(directory) => directory,
        Err(error)
            if error.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let value = name(path.file_name().context("nome ausente")?)?;
    // SAFETY: pai vivo e conferido, abertura sem seguir link e sem bloquear em FIFO.
    let fd = unsafe {
        libc::openat(
            directory.0.as_raw_fd(),
            value.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
        return Ok(None);
    }
    let mut file = owned(fd)?;
    let m = file.metadata()?;
    ensure!(
        m.is_file() && m.uid() == uid() && m.nlink() == 1 && m.mode() & 0o7777 == 0o600,
        "leitura privada: dono/tipo/modo/hardlink recusado"
    );
    let mut bytes = Vec::new();
    if let Some(limit) = limit {
        ensure!(m.len() <= limit, "arquivo privado acima do limite");
        file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
        ensure!(bytes.len() as u64 <= limit, "arquivo privado acima do limite");
    } else {
        file.read_to_end(&mut bytes)?;
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    #[cfg(target_os = "linux")]
    fn inspect_distinguishes_missing_leaf_from_root_sticky_ancestor_without_effects() {
        let parent = Path::new("/tmp");
        let before = std::fs::symlink_metadata(parent).unwrap();
        assert!(before.is_dir() && before.uid() == 0 && before.mode() & 0o1000 != 0);
        let leaf = parent.join(format!("mexerica-inspect-{}", uuid::Uuid::new_v4()));
        assert!(!leaf.exists());
        inspect(&leaf).unwrap();
        inspect(&leaf.join("missing-child")).unwrap();
        assert!(!leaf.exists(), "inspeção não cria a folha nem seu pai");
        let after = std::fs::symlink_metadata(parent).unwrap();
        assert_eq!((before.uid(), before.gid(), before.mode()), (after.uid(), after.gid(), after.mode()));
        assert!(inspect(parent).is_err(), "o ancestral sticky não pode ser selecionado como dados");
        let directory = Directory::open(&leaf, true).unwrap();
        assert_eq!(directory.0.metadata().unwrap().uid(), uid());
        assert_eq!(directory.0.metadata().unwrap().mode() & 0o7777, 0o700);
        inspect(&leaf).unwrap();
        if uid() == 0 {
            let foreign = leaf.join("foreign-owner");
            Directory::for_owner(&foreign, true, 65534, Some(before.gid())).unwrap();
            assert!(inspect(&foreign).is_err(), "folha de outro UID continua recusada");
        } else {
            assert!(inspect(Path::new("/usr")).is_err(), "ancestral root selecionado como folha é recusado");
        }
        let alias = leaf.with_extension("alias");
        std::os::unix::fs::symlink(&leaf, &alias).unwrap();
        assert!(inspect(&alias.join("missing-child")).is_err());
        std::fs::remove_file(alias).unwrap();
        std::fs::remove_dir_all(leaf).unwrap();
    }

    #[test]
    fn private_modes_and_links_under_permissive_umask() {
        const CHILD: &str = "MEXERICA_PRIVATE_FS_FIXTURE";
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "fork::private_fs::tests::private_modes_and_links_under_permissive_umask"])
                .env(CHILD, "1")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        // SAFETY: só o subprocesso exclusivo desta fixture altera a umask.
        unsafe { libc::umask(0) };
        let root = std::env::temp_dir().join(format!("mexerica-home-{}", uuid::Uuid::new_v4()));
        let dir = Directory::open(&root, true).unwrap();
        assert_eq!(dir.0.metadata().unwrap().mode() & 0o7777, 0o700);
        write(&root.join("token"), b"SINTETICO").unwrap();
        assert_eq!(std::fs::metadata(root.join("token")).unwrap().mode() & 0o7777, 0o600);
        create_atomic(&root.join("identity.json"), b"IDENTIDADE_SINTETICA").unwrap();
        assert!(create_atomic(&root.join("identity.json"), b"RECUSADO").is_err());
        assert_eq!(std::fs::metadata(root.join("identity.json")).unwrap().mode() & 0o7777, 0o600);
        assert_eq!(std::fs::read(root.join("identity.json")).unwrap(), b"IDENTIDADE_SINTETICA");
        let database = dir.file(OsStr::new("codync.db"), libc::O_RDWR | libc::O_CREAT | libc::O_EXCL).unwrap();
        assert_eq!(database.metadata().unwrap().mode() & 0o7777, 0o600);
        std::os::unix::fs::symlink("token", root.join("alias")).unwrap();
        assert!(write(&root.join("alias"), b"RECUSADO").is_err());
        std::os::unix::fs::symlink(&root, root.with_extension("alias")).unwrap();
        assert!(Directory::open(&root.with_extension("alias"), true).is_err());
        assert!(Directory::for_owner(&root, false, uid().wrapping_add(1), None).is_err());
        std::fs::hard_link(root.join("token"), root.join("hard")).unwrap();
        assert!(write(&root.join("token"), b"RECUSADO").is_err());
        assert_eq!(std::fs::read(root.join("token")).unwrap(), b"SINTETICO");
        std::fs::remove_file(root.with_extension("alias")).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
