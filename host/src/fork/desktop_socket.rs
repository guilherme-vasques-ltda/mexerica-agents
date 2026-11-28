use crate::fork::private_fs::{self, Directory};
use anyhow::{Context, Result, ensure};
use std::fs::{self, Permissions};
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use tokio::net::UnixListener;
use tokio::sync::oneshot;

pub(crate) const PEER: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 0);

pub(crate) struct SocketFile {
    path: PathBuf,
    dev: u64,
    ino: u64,
}

fn validate_path(path: &Path) -> Result<()> {
    let parent = path.parent().context("private Hub socket has no directory")?;
    ensure!(
        path.is_absolute()
            && path.file_name().is_some_and(|name| name == "s")
            && path.as_os_str().as_bytes().len() <= 103
            && parent.canonicalize()? == parent
            && Directory::open(parent, false)?.0.metadata()?.mode() & 0o7777 == 0o700,
        "invalid private Hub socket directory"
    );
    Ok(())
}

pub(crate) fn validate_endpoint(path: &Path) -> Result<()> {
    validate_path(path)?;
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.file_type().is_socket() && metadata.uid() == private_fs::uid() && metadata.mode() & 0o7777 == 0o600,
        "invalid private Hub socket"
    );
    Ok(())
}

impl SocketFile {
    pub(crate) fn bind(path: PathBuf) -> Result<(UnixListener, Self)> {
        validate_path(&path)?;
        let listener = UnixListener::bind(&path).context("binding the private Hub socket")?;
        let metadata = fs::symlink_metadata(&path)?;
        ensure!(metadata.file_type().is_socket() && metadata.uid() == private_fs::uid(), "unexpected socket identity");
        let file = Self { path, dev: metadata.dev(), ino: metadata.ino() };
        fs::set_permissions(&file.path, Permissions::from_mode(0o600))?;
        Ok((listener, file))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SocketFile {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.file_type().is_socket() && m.dev() == self.dev && m.ino() == self.ino)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(crate) fn watch_stdin() -> Result<oneshot::Receiver<()>> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: fstat só escreve na estrutura válida; o descritor é stdin da invocação.
    let result = unsafe { libc::fstat(libc::STDIN_FILENO, stat.as_mut_ptr()) };
    ensure!(result == 0, "private Hub requires piped stdin");
    // SAFETY: fstat retornou sucesso.
    let kind = unsafe { stat.assume_init() }.st_mode & libc::S_IFMT;
    ensure!(matches!(kind, libc::S_IFIFO | libc::S_IFSOCK), "private Hub requires piped stdin");
    let (tx, rx) = oneshot::channel();
    std::thread::Builder::new().name("desktop-hub-stdin".into()).spawn(move || {
        let _ = io::copy(&mut io::stdin().lock(), &mut io::sink());
        let _ = tx.send(());
    })?;
    Ok(rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[tokio::test]
    async fn socket_has_private_mode_and_cleanup_preserves_a_replacement() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().canonicalize()?;
        fs::set_permissions(&root, Permissions::from_mode(0o700))?;
        let path = root.join("s");
        let (listener, guard) = SocketFile::bind(path.clone())?;
        assert_eq!(fs::symlink_metadata(&path)?.mode() & 0o7777, 0o600);
        let original = root.join("original");
        fs::rename(&path, &original)?;
        fs::write(&path, b"replacement")?;
        drop(guard);
        drop(listener);
        assert_eq!(fs::read(&path)?, b"replacement");

        fs::remove_file(&path)?;
        let (_listener, guard) = SocketFile::bind(path.clone())?;
        drop(guard);
        assert!(!path.exists());
        Ok(())
    }

    #[tokio::test]
    async fn socket_refuses_a_shared_directory_and_symlink() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().canonicalize()?;
        fs::set_permissions(&root, Permissions::from_mode(0o700))?;
        let real = root.join("real");
        fs::create_dir(&real)?;
        fs::set_permissions(&real, Permissions::from_mode(0o755))?;
        assert!(SocketFile::bind(real.join("s")).is_err());
        fs::set_permissions(&real, Permissions::from_mode(0o700))?;
        let link = root.join("link");
        symlink(&real, &link)?;
        assert!(SocketFile::bind(link.join("s")).is_err());
        assert!(!real.join("s").exists());
        Ok(())
    }
}
