//! Caminhos ancorados em descritores: nenhum componente segue links.
use anyhow::{Context, Result, ensure};
use std::{
    ffi::{CString, OsStr},
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, PermissionsExt},
        },
    },
    path::{Component, Path},
};

pub(crate) struct Directory(pub File);
fn name(value: &OsStr) -> Result<CString> {
    ensure!(
        !value.as_bytes().is_empty() && !value.as_bytes().contains(&b'/') && value != "." && value != "..",
        "componente inválido"
    );
    Ok(CString::new(value.as_bytes())?)
}
fn owned(fd: i32) -> Result<File> {
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: openat devolveu um descritor novo, agora pertencente a File.
    Ok(unsafe { File::from_raw_fd(fd) })
}
impl Directory {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        Self::walk(path, None)
    }
    pub(crate) fn create(path: &Path, mode: u32) -> Result<Self> {
        Self::walk(path, Some(mode))
    }
    fn walk(path: &Path, mode: Option<u32>) -> Result<Self> {
        ensure!(path.is_absolute(), "caminho precisa ser absoluto");
        let mut dir = Self(File::open("/")?);
        for part in path.components() {
            match part {
                Component::RootDir => {}
                Component::Normal(value) => dir = dir.child(value, mode)?,
                _ => anyhow::bail!("travessia de ancestral recusada"),
            }
        }
        Ok(dir)
    }
    pub(crate) fn child(&self, value: &OsStr, mode: Option<u32>) -> Result<Self> {
        let value = name(value)?;
        if let Some(mode) = mode {
            #[cfg(not(target_os = "linux"))]
            let mkdir_mode = libc::mode_t::try_from(mode).context("modo de diretório inválido")?;
            #[cfg(target_os = "linux")]
            let mkdir_mode = mode;
            // SAFETY: nome de um único componente e diretório vivo.
            let rc = unsafe { libc::mkdirat(self.0.as_raw_fd(), value.as_ptr(), mkdir_mode) };
            if rc == 0 {
                let dir = self.child(OsStr::from_bytes(value.as_bytes()), None)?;
                dir.mode(mode)?;
                return Ok(dir);
            }
            ensure!(
                std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists,
                "criando diretório: {}",
                std::io::Error::last_os_error()
            );
        }
        // SAFETY: openat ancorado; O_NOFOLLOW vale em cada ancestral.
        Ok(Self(
            owned(unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    value.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    0,
                )
            })
            .context("abrindo diretório sem links")?,
        ))
    }
    pub(crate) fn mode(&self, mode: u32) -> Result<()> {
        self.0.set_permissions(std::fs::Permissions::from_mode(mode))?;
        Ok(())
    }
    pub(crate) fn file(&self, value: &OsStr, flags: i32, mode: u32) -> Result<File> {
        let value = name(value)?;
        // SAFETY: nome validado, fd vivo, flags recusam link e arquivo especial bloqueante.
        let file = owned(unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                value.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                mode,
            )
        })?;
        ensure!(file.metadata()?.is_file(), "arquivo especial recusado");
        Ok(file)
    }
    pub(crate) fn names(&self) -> Result<Vec<std::ffi::OsString>> {
        #[cfg(target_os = "linux")]
        let base = "/proc/self/fd";
        #[cfg(not(target_os = "linux"))]
        let base = "/dev/fd";
        std::fs::read_dir(Path::new(base).join(self.0.as_raw_fd().to_string()))?
            .map(|entry| Ok(entry?.file_name()))
            .collect()
    }
}
pub(crate) fn own(file: &File) -> Result<()> {
    let meta = file.metadata()?;
    // SAFETY: getter sem efeitos colaterais.
    let uid = unsafe { libc::geteuid() };
    ensure!(meta.uid() == uid && (!meta.is_file() || meta.nlink() == 1), "dono ou hardlink inseguro");
    Ok(())
}
