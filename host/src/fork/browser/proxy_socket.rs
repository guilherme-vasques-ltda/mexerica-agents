//! Operações do socket relativas ao diretório aberto, sem seguir links no caminho.
use std::{
    ffi::{CString, OsStr},
    fs::File,
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    },
    path::{Component, Path},
};

fn cstr(s: &OsStr) -> io::Result<CString> {
    CString::new(s.as_bytes()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL no caminho"))
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "caminho de socket inseguro")
}
fn owned_fd(fd: libc::c_int) -> io::Result<File> {
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: o descritor recém-criado pertence exclusivamente a este File.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}
fn stat_fd(fd: libc::c_int) -> io::Result<libc::stat> {
    // SAFETY: stat é inicializado pelo kernel se fstat retornar sucesso.
    let mut stat = unsafe { std::mem::zeroed() };
    // SAFETY: stat aponta para memória válida e fd é fornecido pelo chamador.
    if unsafe { libc::fstat(fd, &raw mut stat) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(stat)
}
fn stat_at(dir: &File, name: &CString) -> io::Result<Option<libc::stat>> {
    // SAFETY: ponteiros válidos durante a chamada; não segue symlinks.
    let mut stat = unsafe { std::mem::zeroed() };
    // SAFETY: os descritores e ponteiros permanecem válidos durante a chamada.
    if unsafe { libc::fstatat(dir.as_raw_fd(), name.as_ptr(), &raw mut stat, libc::AT_SYMLINK_NOFOLLOW) } == 0 {
        return Ok(Some(stat));
    }
    let err = io::Error::last_os_error();
    if err.kind() == io::ErrorKind::NotFound { Ok(None) } else { Err(err) }
}
pub(super) fn is_owned_directory(stat: &libc::stat, uid: libc::uid_t) -> bool {
    stat.st_uid == uid && stat.st_mode & 0o7777 == 0o700
}
pub(super) fn is_owned_socket(stat: &libc::stat, uid: libc::uid_t) -> bool {
    stat.st_mode & libc::S_IFMT == libc::S_IFSOCK && stat.st_uid == uid
}
fn unlink(dir: &File, name: &CString) -> io::Result<()> {
    // SAFETY: descritor e nome são válidos e o nome é relativo ao diretório.
    if unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub(super) struct SocketFile {
    dir: File,
    name: CString,
    dev: libc::dev_t,
    ino: libc::ino_t,
}
impl SocketFile {
    pub(super) fn bind(path: &Path) -> io::Result<(tokio::net::UnixListener, Self)> {
        use std::os::unix::net::UnixListener;
        if !path.is_absolute() {
            return Err(invalid());
        }
        let name = cstr(path.file_name().ok_or_else(invalid)?)?;
        let parent = path.parent().ok_or_else(invalid)?;
        // SAFETY: string literal terminada em NUL e flags válidas.
        let mut dir =
            owned_fd(unsafe { libc::open(c"/".as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) })?;
        // SAFETY: geteuid não recebe argumentos.
        let uid = unsafe { libc::geteuid() };
        let safe_ancestor = |stat: &libc::stat| {
            (stat.st_uid == 0 || stat.st_uid == uid) && (stat.st_mode & 0o022 == 0 || stat.st_mode & libc::S_ISVTX != 0)
        };
        if !safe_ancestor(&stat_fd(dir.as_raw_fd())?) {
            return Err(invalid());
        }
        for part in parent.components() {
            match part {
                Component::RootDir => (),
                Component::Normal(part) => {
                    let part = cstr(part)?;
                    // SAFETY: dir e part permanecem válidos; O_NOFOLLOW recusa links em cada componente.
                    dir = owned_fd(unsafe {
                        libc::openat(
                            dir.as_raw_fd(),
                            part.as_ptr(),
                            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                        )
                    })?;
                    if !safe_ancestor(&stat_fd(dir.as_raw_fd())?) {
                        return Err(invalid());
                    }
                }
                _ => return Err(invalid()),
            }
        }
        let stat = stat_fd(dir.as_raw_fd())?;
        if !is_owned_directory(&stat, uid) {
            return Err(invalid());
        }
        if let Some(stat) = stat_at(&dir, &name)? {
            if !is_owned_socket(&stat, uid) {
                return Err(invalid());
            }
            unlink(&dir, &name)?;
        }
        // Preserve nomes arbitrários em bytes, sem conversão UTF-8.
        let mut anchored = std::ffi::OsString::from(format!("/proc/self/fd/{}/", dir.as_raw_fd()));
        anchored.push(OsStr::from_bytes(name.as_bytes()));
        let listener = UnixListener::bind(&anchored)?;
        let stat = stat_at(&dir, &name)?.ok_or_else(invalid)?;
        if !is_owned_socket(&stat, uid) {
            return Err(invalid());
        }
        let socket = Self { dir, name, dev: stat.st_dev, ino: stat.st_ino };
        // O_PATH abre o inode do socket, sem seguir symlink; chmod em /proc/self/fd fixa esse inode.
        // SAFETY: descritor e nome válidos, com O_NOFOLLOW.
        let fd = owned_fd(unsafe {
            libc::openat(
                socket.dir.as_raw_fd(),
                socket.name.as_ptr(),
                libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        let opened = stat_fd(fd.as_raw_fd())?;
        if !is_owned_socket(&opened, uid) || opened.st_dev != socket.dev || opened.st_ino != socket.ino {
            return Err(invalid());
        }
        let proc_path = CString::new(format!("/proc/self/fd/{}", fd.as_raw_fd())).map_err(|_| invalid())?;
        // SAFETY: proc_path aponta para o descritor aberto acima, que permanece vivo.
        if unsafe { libc::chmod(proc_path.as_ptr(), 0o600) } != 0 {
            return Err(io::Error::last_os_error());
        }
        listener.set_nonblocking(true)?;
        Ok((tokio::net::UnixListener::from_std(listener)?, socket))
    }
}
impl Drop for SocketFile {
    fn drop(&mut self) {
        if stat_at(&self.dir, &self.name)
            .ok()
            .flatten()
            .is_some_and(|s| s.st_mode & libc::S_IFMT == libc::S_IFSOCK && s.st_dev == self.dev && s.st_ino == self.ino)
        {
            let _ = unlink(&self.dir, &self.name);
        }
    }
}
