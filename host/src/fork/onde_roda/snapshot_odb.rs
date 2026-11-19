//! Cópia limitada por descritores; o Git nunca recebe a localização original.
use super::{FormatoGit, SelecaoGit, snapshot::Budget};
use anyhow::{Context as _, Result, bail, ensure};
use std::collections::BTreeSet;
use std::ffi::{CStr, CString};
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Component, Path, PathBuf};

pub(super) struct Private {
    pub root: PathBuf,
    pub git: PathBuf,
    pub spool: PathBuf,
    directories: Vec<PathBuf>,
    cleaned: bool,
    sealed: bool,
}

impl Private {
    pub fn create() -> Result<Self> {
        // /tmp é pai local admitido, não vem do ambiente ou pacote.
        let _parent = directory(Path::new("/tmp"))?;
        let root = PathBuf::from(format!("/tmp/donna-snapshot-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root)?;
        let mut value = Self {
            git: root.join("git"),
            spool: root.join("spool"),
            directories: vec![root.clone()],
            root,
            cleaned: false,
            sealed: false,
        };
        for name in ["git", "git/objects", "git/objects/pack", "git/refs", "home", "spool"] {
            let path = value.root.join(name);
            fs::DirBuilder::new().mode(0o700).create(&path)?;
            value.directories.push(path);
        }
        Ok(value)
    }

    pub fn copy(&mut self, origin: &Path, selection: &SelecaoGit<'_>, budget: &Budget<'_>) -> Result<()> {
        ensure!(!self.sealed, "snapshot_odb_already_sealed");
        // Não aceitar uma montagem falsa de /proc para reabrir o inode O_PATH.
        let proc = directory(Path::new("/proc"))?;
        let mut statfs = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: statfs recebe memória válida, descritor vivo; retorno checado antes de ler.
        unsafe {
            ensure!(libc::fstatfs(proc.as_raw_fd(), statfs.as_mut_ptr()) == 0, "snapshot_procfs");
            ensure!(statfs.assume_init().f_type == 0x9fa0, "snapshot_procfs");
        }
        let source = directory(origin)?;
        let mut counts = Counts { entries: 0, files: 0, bytes: 0 };
        for name in names(&source, &mut counts, budget)? {
            budget.check()?;
            if name == "info" {
                let info = dir_at(&source, &name)?;
                for entry in names(&info, &mut counts, budget)? {
                    ensure!(!matches!(entry.as_str(), "alternates" | "http-alternates"), "snapshot_odb_alternates");
                }
            } else if name == "pack" {
                let pack = dir_at(&source, &name)?;
                let mut packs = BTreeSet::new();
                let mut indexes = BTreeSet::new();
                for entry in names(&pack, &mut counts, budget)? {
                    let (stem, extension) = entry.rsplit_once('.').context("snapshot_odb_layout")?;
                    ensure!(
                        stem.starts_with("pack-") && hex(&stem[5..], selection.formato.largura()),
                        "snapshot_odb_layout"
                    );
                    match extension {
                        "pack" => {
                            packs.insert(stem.to_owned());
                        }
                        "idx" => {
                            indexes.insert(stem.to_owned());
                        }
                        _ => bail!("snapshot_odb_layout"),
                    }
                    copy_file(&pack, &entry, &self.git.join("objects/pack").join(&entry), &mut counts, budget)?;
                }
                ensure!(packs == indexes, "snapshot_odb_pack_pair");
            } else {
                ensure!(hex(&name, 2), "snapshot_odb_layout");
                let fanout = dir_at(&source, &name)?;
                let destination = self.git.join("objects").join(&name);
                fs::DirBuilder::new().mode(0o700).create(&destination)?;
                self.directories.push(destination.clone());
                for entry in names(&fanout, &mut counts, budget)? {
                    ensure!(hex(&entry, selection.formato.largura() - 2), "snapshot_odb_layout");
                    copy_file(&fanout, &entry, &destination.join(&entry), &mut counts, budget)?;
                }
            }
        }
        // source/proc e todos os FDs relativos saem do escopo antes da selagem.
        drop(source);
        drop(proc);
        let config = match selection.formato {
            FormatoGit::Sha1 => "[core]\nrepositoryformatversion=0\nbare=true\nhooksPath=/dev/null\n",
            FormatoGit::Sha256 => {
                "[core]\nrepositoryformatversion=1\nbare=true\nhooksPath=/dev/null\n[extensions]\nobjectFormat=sha256\n"
            }
        };
        self.write("git/config", config.as_bytes())?;
        self.write("git/HEAD", b"ref: refs/heads/unused\n")?;
        for path in &self.directories {
            if path != &self.spool {
                fs::set_permissions(path, fs::Permissions::from_mode(0o500))?;
            }
        }
        budget.check()?;
        self.sealed = true;
        Ok(())
    }

    pub fn require_sealed(&self) -> Result<()> {
        ensure!(self.sealed && !self.cleaned, "snapshot_odb_not_sealed");
        Ok(())
    }

    fn write(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let path = self.root.join(name);
        let mut f = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path)?;
        f.write_all(bytes)?;
        f.set_permissions(fs::Permissions::from_mode(0o400))?;
        Ok(())
    }

    pub fn body(&self, index: usize, bytes: &[u8]) -> Result<String> {
        let name = format!("{index:05}");
        self.write(&format!("spool/{name}"), bytes)?;
        Ok(name)
    }

    pub fn seal_spool(&self) -> Result<()> {
        fs::set_permissions(&self.spool, fs::Permissions::from_mode(0o500))?;
        Ok(())
    }

    pub fn cleanup(&mut self) -> Result<()> {
        if !self.cleaned {
            for path in &self.directories {
                fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
            }
            fs::remove_dir_all(&self.root)?;
            self.cleaned = true;
        }
        Ok(())
    }
}

impl Drop for Private {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            tracing::warn!(%error, "snapshot_private_cleanup");
        }
    }
}

struct Counts {
    entries: usize,
    files: usize,
    bytes: u64,
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn open_at(parent: i32, name: &CStr, flags: i32) -> Result<OwnedFd> {
    // SAFETY: string C válida; somente o FD retornado passa a pertencer ao OwnedFd.
    let fd = unsafe { libc::openat(parent, name.as_ptr(), flags | libc::O_CLOEXEC | libc::O_NOFOLLOW) };
    ensure!(fd >= 0, "snapshot_odb_open: {}", std::io::Error::last_os_error());
    // SAFETY: fd é novo, válido e tem um único dono.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn dir_at(parent: &OwnedFd, name: &str) -> Result<OwnedFd> {
    open_at(parent.as_raw_fd(), &CString::new(name)?, libc::O_RDONLY | libc::O_DIRECTORY)
}

fn directory(path: &Path) -> Result<OwnedFd> {
    ensure!(path.is_absolute(), "snapshot_odb_absolute");
    let mut fd = open_at(libc::AT_FDCWD, c"/", libc::O_RDONLY | libc::O_DIRECTORY)?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                use std::os::unix::ffi::OsStrExt as _;
                fd = open_at(fd.as_raw_fd(), &CString::new(name.as_bytes())?, libc::O_RDONLY | libc::O_DIRECTORY)?;
            }
            _ => bail!("snapshot_odb_path"),
        }
    }
    Ok(fd)
}

struct Directory(*mut libc::DIR);
impl Drop for Directory {
    fn drop(&mut self) {
        // SAFETY: este guard possui o DIR válido até closedir.
        unsafe {
            libc::closedir(self.0);
        }
    }
}

fn names(fd: &OwnedFd, counts: &mut Counts, budget: &Budget<'_>) -> Result<Vec<String>> {
    // fdopendir assume propriedade; duplicar com CLOEXEC, sem reabrir path da origem.
    // SAFETY: duplicação de FD vivo; fdopendir assume só o novo descritor.
    let dir = unsafe {
        let copy = libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3);
        ensure!(copy >= 0, "snapshot_odb_dir");
        let dir = libc::fdopendir(copy);
        if dir.is_null() {
            libc::close(copy);
        }
        ensure!(!dir.is_null(), "snapshot_odb_dir");
        dir
    };
    let guard = Directory(dir);
    let mut result = Vec::new();
    loop {
        budget.check()?;
        // SAFETY: readdir usa DIR exclusivo; CStr é consumida antes da próxima chamada.
        let name = unsafe {
            *libc::__errno_location() = 0;
            let entry = libc::readdir(guard.0);
            if entry.is_null() {
                ensure!(*libc::__errno_location() == 0, "snapshot_odb_readdir");
                break;
            }
            CStr::from_ptr((*entry).d_name.as_ptr())
        };
        if matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        counts.entries = counts.entries.checked_add(1).context("snapshot_overflow")?;
        ensure!(counts.entries <= 32768, "snapshot_odb_entries");
        ensure!(name.to_bytes().len() <= 255, "snapshot_odb_name");
        result.push(name.to_str()?.to_owned());
    }
    Ok(result)
}

fn stat(fd: i32) -> Result<libc::stat> {
    let mut value = std::mem::MaybeUninit::uninit();
    // SAFETY: fstat inicializa a memória; só ler em sucesso.
    unsafe {
        ensure!(libc::fstat(fd, value.as_mut_ptr()) == 0, "snapshot_odb_stat");
        Ok(value.assume_init())
    }
}

fn same(a: &libc::stat, b: &libc::stat) -> bool {
    a.st_dev == b.st_dev
        && a.st_ino == b.st_ino
        && a.st_mode == b.st_mode
        && a.st_nlink == b.st_nlink
        && a.st_size == b.st_size
        && a.st_mtime == b.st_mtime
        && a.st_mtime_nsec == b.st_mtime_nsec
        && a.st_ctime == b.st_ctime
        && a.st_ctime_nsec == b.st_ctime_nsec
}

fn copy_file(parent: &OwnedFd, name: &str, destination: &Path, counts: &mut Counts, budget: &Budget<'_>) -> Result<()> {
    budget.check()?;
    let pinned = open_at(parent.as_raw_fd(), &CString::new(name)?, libc::O_PATH)?;
    let before = stat(pinned.as_raw_fd())?;
    ensure!(before.st_mode & libc::S_IFMT == libc::S_IFREG && before.st_nlink == 1, "snapshot_odb_regular");
    let size = u64::try_from(before.st_size)?;
    ensure!(size <= 128 * 1024 * 1024, "snapshot_odb_file_bytes");
    counts.files = counts.files.checked_add(1).context("snapshot_overflow")?;
    counts.bytes = counts.bytes.checked_add(size).context("snapshot_overflow")?;
    ensure!(counts.files <= 16384 && counts.bytes <= 256 * 1024 * 1024, "snapshot_odb_budget");
    // Ponte procfs confiável para o inode O_PATH já conferido, não symlink da origem.
    let mut input = File::open(format!("/proc/self/fd/{}", pinned.as_raw_fd()))?;
    ensure!(same(&before, &stat(input.as_raw_fd())?), "snapshot_odb_changed");
    let mut output = OpenOptions::new().write(true).create_new(true).mode(0o600).open(destination)?;
    #[cfg(test)]
    if let Some(observer) = budget.copy_observer.take() {
        observer(input.as_raw_fd());
    }
    let mut remaining = size;
    let mut buffer = vec![0_u8; 65536];
    while remaining != 0 {
        budget.check()?;
        let length = usize::try_from(remaining.min(65536))?;
        input.read_exact(&mut buffer[..length])?;
        output.write_all(&buffer[..length])?;
        remaining -= u64::try_from(length)?;
    }
    ensure!(input.read(&mut buffer[..1])? == 0, "snapshot_odb_changed");
    ensure!(
        same(&before, &stat(input.as_raw_fd())?) && same(&before, &stat(pinned.as_raw_fd())?),
        "snapshot_odb_changed"
    );
    ensure!(output.metadata()?.len() == size, "snapshot_odb_copy_size");
    output.set_permissions(fs::Permissions::from_mode(0o400))?;
    budget.check()
}
