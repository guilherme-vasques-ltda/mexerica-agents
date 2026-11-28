use super::Prepared;
use anyhow::Result;
use std::{
    ffi::{CStr, CString},
    fs::Metadata,
    io,
    mem::MaybeUninit,
    os::unix::{ffi::OsStrExt as _, fs::MetadataExt as _},
};

#[derive(Clone)]
pub(super) struct Identity {
    executable: (u64, u64),
    directory: (u64, u64),
    executable_path: CString,
    directory_path: CString,
}

impl Identity {
    pub(super) fn capture(prepared: &Prepared<'_>) -> Result<Self> {
        let key = |m: Metadata| (m.dev(), m.ino());
        Ok(Self {
            executable: key(prepared.executable.metadata()?),
            directory: key(prepared.directory.metadata()?),
            executable_path: CString::new(prepared.request.argv[0].as_str())?,
            directory_path: CString::new(prepared.request.cwd.as_os_str().as_bytes())?,
        })
    }

    pub(super) fn recheck(&self) -> io::Result<()> {
        recheck_path(&self.directory_path, self.directory, libc::S_IFDIR)?;
        recheck_path(&self.executable_path, self.executable, libc::S_IFREG)
    }
}

fn recheck_path(path: &CStr, expected: (u64, u64), kind: libc::mode_t) -> io::Result<()> {
    let mut status = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: lstat recebe um caminho terminado em NUL e espaço para gravar uma struct stat.
    if unsafe { libc::lstat(path.as_ptr(), status.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: lstat bem-sucedido inicializa o resultado.
    let status = unsafe { status.assume_init() };
    if (u64::from(status.st_dev.cast_unsigned()), status.st_ino) != expected
        || status.st_mode & libc::S_IFMT != kind
        || (kind == libc::S_IFREG && status.st_mode & (libc::S_ISUID | libc::S_ISGID) != 0)
    {
        return Err(io::Error::from_raw_os_error(libc::ESTALE));
    }
    Ok(())
}
