//! Leitura por descritores. Políticas root e serviceUID distintas, sem caminhos de requests.
use anyhow::Result;
use std::path::Path;

#[cfg(unix)]
mod unix {
    use anyhow::{Context, Result, ensure};
    use std::ffi::CString;
    use std::fs::{File, Metadata};
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::MetadataExt;
    use std::path::{Component, Path};

    #[derive(Clone, Copy)]
    enum Owner {
        Root,
        Service,
    }
    fn service_uid() -> u32 {
        // SAFETY: geteuid não recebe ponteiros nem altera o processo.
        unsafe { libc::geteuid() }
    }
    fn directory(m: &Metadata, owner: Owner, last: bool, root_uid: u32) -> Result<()> {
        let uid = service_uid();
        ensure!(m.is_dir() && m.mode() & 0o022 == 0, "personal parent is not protected");
        match owner {
            Owner::Root => ensure!(m.uid() == root_uid, "personal parent is not root owned"),
            Owner::Service => {
                ensure!(m.uid() == 0 || m.uid() == uid, "personal identity parent owner mismatch");
                if last {
                    ensure!(m.uid() == uid && m.mode() & 0o7777 == 0o700, "personal data directory must be private");
                }
            }
        }
        Ok(())
    }
    fn open_at(parent: &File, name: &str, flags: i32) -> Result<Option<File>> {
        let name = CString::new(name)?;
        // SAFETY: parent fica vivo, nome termina em NUL e o retorno é um novo fd próprio.
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            )
        };
        if fd < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::NotFound {
                return Ok(None);
            }
            return Err(e.into());
        }
        // SAFETY: fd recém-aberto é transferido uma única vez para File.
        Ok(Some(unsafe { File::from_raw_fd(fd) }))
    }
    fn unchanged(a: &Metadata, b: &Metadata) -> bool {
        (
            a.dev(),
            a.ino(),
            a.uid(),
            a.gid(),
            a.mode(),
            a.nlink(),
            a.len(),
            a.mtime(),
            a.mtime_nsec(),
            a.ctime(),
            a.ctime_nsec(),
        ) == (
            b.dev(),
            b.ino(),
            b.uid(),
            b.gid(),
            b.mode(),
            b.nlink(),
            b.len(),
            b.mtime(),
            b.mtime_nsec(),
            b.ctime(),
            b.ctime_nsec(),
        )
    }
    fn relative(
        mut parent: File,
        names: &[String],
        owner: Owner,
        root_uid: u32,
        limit: usize,
    ) -> Result<Option<Vec<u8>>> {
        ensure!(!names.is_empty(), "personal file path missing");
        directory(&parent.metadata()?, owner, false, root_uid)?;
        for (i, name) in names[..names.len() - 1].iter().enumerate() {
            let Some(next) = open_at(&parent, name, libc::O_RDONLY | libc::O_DIRECTORY)? else { return Ok(None) };
            directory(&next.metadata()?, owner, i + 2 == names.len(), root_uid)?;
            parent = next;
        }
        let Some(mut file) = open_at(&parent, &names[names.len() - 1], libc::O_RDONLY)? else { return Ok(None) };
        let before = file.metadata()?;
        let uid = match owner {
            Owner::Root => root_uid,
            Owner::Service => service_uid(),
        };
        let mode = match owner {
            Owner::Root => 0o640,
            Owner::Service => 0o600,
        };
        ensure!(
            before.is_file() && before.uid() == uid && before.mode() & 0o7777 == mode && before.nlink() == 1,
            "personal file ownership/type/mode/link mismatch"
        );
        if matches!(owner, Owner::Root) {
            ensure!(parent.metadata()?.mode() & 0o7777 == 0o750, "personal root directory mode mismatch");
        }
        ensure!(before.len() > 0 && before.len() <= limit as u64, "personal file size invalid");
        let mut bytes = Vec::new();
        (&mut file).take(limit as u64 + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() == usize::try_from(before.len())?
                && bytes.len() <= limit
                && unchanged(&before, &file.metadata()?),
            "personal file changed during read"
        );
        Ok(Some(bytes))
    }
    fn absolute(path: &Path, owner: Owner, limit: usize) -> Result<Option<Vec<u8>>> {
        ensure!(path.is_absolute(), "personal file path must be absolute");
        let mut names = Vec::new();
        for c in path.components() {
            match c {
                Component::RootDir => (),
                Component::Normal(s) => names.push(s.to_str().context("non UTF-8 personal path")?.to_owned()),
                _ => anyhow::bail!("invalid personal path component"),
            }
        }
        let root = File::open("/")?;
        relative(root, &names, owner, 0, limit)
    }
    pub(super) fn root(path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
        absolute(path, Owner::Root, limit)
    }
    pub(super) fn identity(dir: &Path) -> Result<Option<Vec<u8>>> {
        absolute(&dir.join("identity.json"), Owner::Service, 4096)
    }

    #[cfg(test)]
    pub(super) fn fixture(path: &Path, limit: usize) -> Result<Vec<u8>> {
        let parent = path.parent().context("fixture parent")?;
        let name = path.file_name().and_then(|n| n.to_str()).context("fixture name")?;
        relative(File::open(parent)?, &[name.to_owned()], Owner::Root, service_uid(), limit)?
            .context("fixture root missing")
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::fs::{PermissionsExt, symlink};
        #[test]
        fn identity_uses_service_uid_and_private_data_modes_without_creating_missing_h() -> Result<()> {
            let base = std::env::temp_dir().join(format!("personal-identity-fs-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&base)?;
            std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700))?;
            let data = base.join("data");
            std::fs::create_dir(&data)?;
            std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700))?;
            let read =
                || relative(File::open(&base)?, &["data".into(), "identity.json".into()], Owner::Service, 0, 4096);
            assert!(read()?.is_none());
            assert!(!data.join("identity.json").exists());
            let path = data.join("identity.json");
            std::fs::write(&path, b"private-fixture")?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            assert_eq!(read()?, Some(b"private-fixture".to_vec()));
            std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o750))?;
            assert!(read().is_err());
            std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700))?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640))?;
            assert!(read().is_err());
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            // O UID esperado vem do processo; não há campo de pedido para escolhê-lo.
            let wrong_uid = service_uid().wrapping_add(1);
            assert!(
                relative(File::open(&base)?, &["data".into(), "identity.json".into()], Owner::Root, wrong_uid, 4096)
                    .is_err()
            );
            std::fs::remove_dir_all(base)?;
            Ok(())
        }
        #[test]
        fn descriptors_reject_parent_links_hardlinks_modes_and_fifo() -> Result<()> {
            let base = std::env::temp_dir().join(format!("personal-fs-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&base)?;
            let dir = base.join("root");
            std::fs::create_dir(&dir)?;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o750))?;
            let p = dir.join("config");
            std::fs::write(&p, b"{}")?;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o640))?;
            let read = |name: &str| relative(File::open(&dir)?, &[name.to_owned()], Owner::Root, service_uid(), 16);
            assert_eq!(read("config")?, Some(b"{}".to_vec()));
            std::fs::hard_link(&p, dir.join("hard"))?;
            assert!(read("config").is_err());
            std::fs::remove_file(dir.join("hard"))?;
            symlink("config", dir.join("link"))?;
            assert!(read("link").is_err());
            symlink("root", base.join("alias"))?;
            assert!(
                relative(File::open(&base)?, &["alias".into(), "config".into()], Owner::Root, service_uid(), 16)
                    .is_err()
            );
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o660))?;
            assert!(read("config").is_err());
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o640))?;
            std::fs::write(&p, b"more-than-16-bytes")?;
            assert!(read("config").is_err());
            std::fs::write(&p, b"")?;
            assert!(read("config").is_err());
            let fifo = CString::new(dir.join("fifo").to_str().context("fixture path")?)?;
            // SAFETY: caminho de fixture privado, C string válida, sem bloqueio de leitura.
            ensure!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o640) } == 0, "fixture FIFO unavailable");
            assert!(read("fifo").is_err());
            if service_uid() != 0 {
                assert!(root(&p, 16).is_err());
            }
            std::fs::remove_dir_all(base)?;
            Ok(())
        }
    }
}

pub(super) fn root_file(path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
    #[cfg(unix)]
    {
        unix::root(path, limit)
    }
    #[cfg(not(unix))]
    {
        let _ = (path, limit);
        anyhow::bail!("personal root reader requires Unix")
    }
}
pub(super) fn identity_file(dir: &Path) -> Result<Option<Vec<u8>>> {
    #[cfg(unix)]
    {
        unix::identity(dir)
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        anyhow::bail!("personal identity reader requires Unix")
    }
}

#[cfg(all(test, unix))]
pub(super) fn fixture_root(path: &Path, limit: usize) -> Result<Vec<u8>> {
    unix::fixture(path, limit)
}
#[cfg(all(test, not(unix)))]
pub(super) fn fixture_root(_path: &Path, _limit: usize) -> Result<Vec<u8>> {
    anyhow::bail!("personal fixture requires Unix")
}
