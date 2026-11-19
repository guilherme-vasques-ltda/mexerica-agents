//! Filesystem boundary for memory projections. Parents cannot redirect reads or atomic replacements.

use anyhow::{Context, Result, bail};
use std::fs::{File, Metadata};
use std::io::{Read, Write};
#[cfg(windows)]
use std::path::Path;
use std::path::{Component, PathBuf};

pub struct Directory {
    #[cfg(windows)]
    path: PathBuf,
    #[cfg(unix)]
    handle: File,
    // Denying FILE_SHARE_DELETE keeps all parents in place for the lifetime of the operation.
    #[cfg(windows)]
    _parents: Vec<File>,
}

impl Directory {
    pub fn open(layer: Option<&str>) -> Result<Self> {
        let root = std::fs::canonicalize(crate::service::data_dir()).context("opening the memory root")?;
        let relative = match layer {
            Some(id) => PathBuf::from("bots").join(id).join("bot-memory"),
            None => PathBuf::from("shared-memory"),
        };
        if relative.components().any(|c| !matches!(c, Component::Normal(_))) {
            bail!("invalid memory folder");
        }
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            use std::os::unix::fs::OpenOptionsExt;
            let mut handle =
                File::options().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW).open(&root)?;
            for component in relative.components() {
                let name = c_name(component.as_os_str())?;
                // SAFETY: the fd and NUL-terminated single component remain live for the call.
                let fd = unsafe { libc::openat(handle.as_raw_fd(), name.as_ptr(), directory_flags(), 0) };
                handle = if fd < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
                    // SAFETY: mkdirat is relative to the same live directory descriptor.
                    let rc = unsafe { libc::mkdirat(handle.as_raw_fd(), name.as_ptr(), 0o700) };
                    if rc < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists {
                        return Err(std::io::Error::last_os_error().into());
                    }
                    // SAFETY: O_NOFOLLOW still refuses a link substituted after mkdirat.
                    owned(unsafe { libc::openat(handle.as_raw_fd(), name.as_ptr(), directory_flags(), 0) })?
                } else {
                    owned(fd).context("memory folder must be a real directory, without links")?
                };
            }
            Ok(Self { handle })
        }
        #[cfg(windows)]
        {
            let mut path = root;
            let mut parents = vec![lock_directory(&path)?];
            for component in relative.components() {
                path.push(component);
                match std::fs::create_dir(&path) {
                    Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(e.into()),
                    _ => {}
                }
                parents.push(lock_directory(&path)?);
            }
            Ok(Self { path, _parents: parents })
        }
    }

    pub fn names(&self) -> Result<Vec<String>> {
        #[cfg(unix)]
        let path = {
            use std::os::fd::AsRawFd;
            #[cfg(target_os = "linux")]
            let base = "/proc/self/fd";
            #[cfg(not(target_os = "linux"))]
            let base = "/dev/fd";
            PathBuf::from(base).join(self.handle.as_raw_fd().to_string())
        };
        #[cfg(windows)]
        let path = self.path.clone();
        std::fs::read_dir(path)?.map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned())).collect()
    }

    pub fn read(&self, name: &str) -> Result<Option<(String, Metadata)>> {
        Self::check_name(name)?;
        let opened = self.open_file(name);
        let mut file = match opened {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            #[cfg(unix)]
            Err(e) if e.raw_os_error() == Some(libc::ELOOP) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let meta = file.metadata()?;
        if !meta.is_file() || is_link(&meta) {
            return Ok(None);
        }
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        Ok(Some((content, meta)))
    }

    #[cfg(unix)]
    fn open_file(&self, name: &str) -> std::io::Result<File> {
        use std::os::fd::AsRawFd;
        let name = c_name(name.as_ref())?;
        // SAFETY: the live fd anchors the parent; O_NOFOLLOW refuses final symlinks and O_NONBLOCK avoids FIFOs.
        owned(unsafe {
            libc::openat(
                self.handle.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                0,
            )
        })
    }

    #[cfg(windows)]
    fn open_file(&self, name: &str) -> std::io::Result<File> {
        use std::os::windows::fs::OpenOptionsExt;
        File::options().read(true).custom_flags(0x0020_0000).open(self.path.join(name))
    }

    pub fn put(&self, name: &str, content: Option<&str>) -> Result<()> {
        Self::check_name(name)?;
        let Some(content) = content else { return self.remove(name) };
        let temp = format!(".{}.tmp", uuid::Uuid::new_v4());
        let result = (|| -> Result<()> {
            #[cfg(unix)]
            let mut file = {
                use std::os::fd::AsRawFd;
                let temp = c_name(temp.as_ref())?;
                // SAFETY: exclusive creation is anchored to the directory; links are never opened for writing.
                owned(unsafe {
                    libc::openat(
                        self.handle.as_raw_fd(),
                        temp.as_ptr(),
                        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                        0o600,
                    )
                })?
            };
            #[cfg(windows)]
            let mut file = File::options().write(true).create_new(true).open(self.path.join(&temp))?;
            file.write_all(content.as_bytes())?;
            drop(file);
            #[cfg(unix)]
            {
                use std::os::fd::AsRawFd;
                let (temp, name) = (c_name(temp.as_ref())?, c_name(name.as_ref())?);
                // SAFETY: both single-component names are relative to the same live directory descriptor.
                if unsafe {
                    libc::renameat(self.handle.as_raw_fd(), temp.as_ptr(), self.handle.as_raw_fd(), name.as_ptr())
                } < 0
                {
                    return Err(std::io::Error::last_os_error().into());
                }
            }
            #[cfg(windows)]
            std::fs::rename(self.path.join(&temp), self.path.join(name))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = self.remove(&temp);
        }
        result.context("replacing the memory projection")
    }

    fn remove(&self, name: &str) -> Result<()> {
        #[cfg(unix)]
        let result = {
            use std::os::fd::AsRawFd;
            let name = c_name(name.as_ref())?;
            // SAFETY: unlinkat removes the named entry itself, never a symlink's target.
            let rc = unsafe { libc::unlinkat(self.handle.as_raw_fd(), name.as_ptr(), 0) };
            if rc < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
        };
        #[cfg(windows)]
        let result = std::fs::remove_file(self.path.join(name));
        match result {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    fn check_name(name: &str) -> Result<()> {
        if !codync_mem::files::valid_name(name) {
            bail!("Memory file names are lowercase letters, digits and dashes, ending in .md (like people.md).");
        }
        Ok(())
    }
}

#[cfg(unix)]
fn directory_flags() -> libc::c_int {
    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC
}

#[cfg(unix)]
fn c_name(name: &std::ffi::OsStr) -> std::io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(name.as_bytes()).map_err(std::io::Error::other)
}

#[cfg(unix)]
fn owned(fd: libc::c_int) -> std::io::Result<File> {
    use std::os::fd::FromRawFd;
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: successful openat returned a fresh owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(windows)]
fn lock_directory(path: &Path) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    let handle = File::options().read(true).share_mode(3).custom_flags(0x0220_0000).open(path)?;
    let metadata = handle.metadata()?;
    if !metadata.is_dir() || is_link(&metadata) {
        bail!("memory folder must be a real directory, without links");
    }
    Ok(handle)
}

fn is_link(meta: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(unix)]
    {
        meta.file_type().is_symlink()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn an_open_directory_cannot_be_redirected_by_a_path_replacement() {
        let root = crate::service::data_dir();
        std::fs::create_dir_all(&root).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let dir = Directory::open(Some(&id)).unwrap();
        let parent = root.join("bots").join(&id);
        let path = parent.join("bot-memory");
        let moved = parent.join("original");
        let other = parent.join("other-workspace");
        std::fs::create_dir(&other).unwrap();
        std::fs::rename(&path, &moved).unwrap();
        std::os::unix::fs::symlink(&other, &path).unwrap();
        dir.put("people.md", Some("registered")).unwrap();
        assert!(!other.join("people.md").exists());
        assert_eq!(std::fs::read_to_string(moved.join("people.md")).unwrap(), "registered");
        assert_eq!(dir.read("people.md").unwrap().unwrap().0, "registered");
        assert_eq!(dir.names().unwrap(), vec!["people.md"]);
        assert!(Directory::open(Some(&id)).is_err());
        // A final-file symlink is replaced, while its target stays untouched.
        std::fs::write(other.join("target.md"), "unrelated").unwrap();
        std::fs::remove_file(moved.join("people.md")).unwrap();
        std::os::unix::fs::symlink(other.join("target.md"), moved.join("people.md")).unwrap();
        assert!(dir.read("people.md").unwrap().is_none());
        dir.put("people.md", Some("registered")).unwrap();
        assert_eq!(std::fs::read_to_string(other.join("target.md")).unwrap(), "unrelated");
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir_all(parent).unwrap();
    }
}
