// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Immutable snapshots explicitly published by a bot, independent of its workspace.
use crate::chat::uploads::MAX_FILE;
use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[path = "files/fork-mask.rs"]
mod fork_mask;

pub const CHUNK_SIZE: usize = 384 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SharedFile {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

/// Removes an unpublished snapshot on every failure, including a dropped actor.
pub struct Snapshot {
    pub meta: SharedFile,
    path: PathBuf,
    committed: bool,
}

impl Snapshot {
    pub fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub fn root(bot: &str) -> PathBuf {
    crate::service::data_dir().join("bots").join(bot).join("files")
}

fn path(root: &Path, id: &str) -> Result<PathBuf> {
    let id = uuid::Uuid::parse_str(id).context("invalid file id")?;
    Ok(root.join(id.to_string()))
}

/// Display names never control storage paths; hidden and extensionless files are valid.
pub fn name(value: &str) -> Result<String> {
    ensure!(!value.is_empty() && value.len() <= 200, "file name must be 1–200 bytes");
    ensure!(
        value != "." && value != ".." && !value.contains(['/', '\\', ':']) && !value.chars().any(char::is_control),
        "invalid file name"
    );
    Ok(value.to_owned())
}

#[cfg(unix)]
fn open_regular(path: &Path) -> Result<File> {
    ensure!(std::fs::symlink_metadata(path)?.file_type().is_file(), "only regular files can be shared");
    crate::fork::motor_exec::fd::Directory::open(path.parent().context("file has no parent")?)?.file(
        path.file_name().context("file has no name")?,
        libc::O_RDONLY,
        0,
    )
}

#[cfg(windows)]
fn open_regular(path: &Path) -> Result<File> {
    use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        FILE_SHARE_WRITE,
    };

    let mut parents = Vec::new();
    let ancestors: Vec<_> = path.ancestors().skip(1).filter(|p| p.parent().is_some()).collect();
    for parent in ancestors.into_iter().rev() {
        let dir = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(parent)?;
        let metadata = dir.metadata()?;
        ensure!(
            metadata.is_dir() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
            "invalid file parent"
        );
        parents.push(dir);
    }
    let file = OpenOptions::new().read(true).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "only regular files can be shared"
    );
    Ok(file)
}

#[cfg(test)]
pub fn snapshot(
    root: &Path,
    cwd: &Path,
    source: &str,
    display: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<Snapshot> {
    snapshot_with_mask(root, cwd, source, display, cancelled, None)
}

pub(crate) fn snapshot_for_share(
    root: &Path,
    cwd: &Path,
    source: &str,
    display: Option<&str>,
    cancelled: &AtomicBool,
    store: &crate::store::Store,
    visible_logins: &[String],
) -> Result<Snapshot> {
    let mask = fork_mask::Mask::from_store(store, visible_logins)?;
    snapshot_with_mask(root, cwd, source, display, cancelled, mask.as_ref())
}

fn snapshot_with_mask(
    root: &Path,
    cwd: &Path,
    source: &str,
    display: Option<&str>,
    cancelled: &AtomicBool,
    mask: Option<&fork_mask::Mask>,
) -> Result<Snapshot> {
    let original = cwd.join(source);
    let separate = crate::fork::motor_exec::configured();
    let source = if separate { original.clone() } else { original.canonicalize().context("finding the file")? };
    let display = match display {
        Some(value) => name(value)?,
        None => name(original.file_name().and_then(|v| v.to_str()).context("file name is not UTF-8; provide name")?)?,
    };
    let display = match mask {
        Some(mask) => mask.name(display)?,
        None => display,
    };
    let mut local = if separate { None } else { Some(open_regular(&source)?) };
    let mut delegated = if separate { Some(crate::fork::motor_exec::read::Input::open(&source)?) } else { None };
    let before = local.as_ref().map(File::metadata).transpose()?;
    ensure!(before.as_ref().is_none_or(|m| m.len() <= MAX_FILE), "file exceeds 100 MiB");
    std::fs::create_dir_all(root).context("creating the shared file folder")?;
    let id = uuid::Uuid::new_v4().to_string();
    let final_path = path(root, &id)?;
    let partial = final_path.with_extension("partial");
    let mut owned = Snapshot {
        meta: SharedFile { id, name: display, size: 0, sha256: String::new() },
        path: partial.clone(),
        committed: false,
    };
    let mut output = OpenOptions::new().write(true).create_new(true).open(&partial)?;
    let (raw_size, size, digest) = if let Some(input) = delegated.as_mut() {
        let copied = copy_masked(input, &mut output, cancelled, mask)?;
        input.finish()?;
        copied
    } else {
        copy_masked(local.as_mut().context("arquivo local ausente")?, &mut output, cancelled, mask)?
    };
    owned.meta.size = size;
    if let (Some(input), Some(before)) = (&local, &before) {
        let after = input.metadata()?;
        ensure!(
            raw_size == before.len() && after.len() == before.len() && after.modified()? == before.modified()?,
            "source file changed while sharing; retry"
        );
    }
    output.sync_all()?;
    drop(output);
    ensure!(!cancelled.load(Ordering::Acquire), "file sharing cancelled");
    std::fs::rename(&partial, &final_path)?;
    owned.path = final_path;
    owned.meta.sha256 = digest;
    Ok(owned)
}

fn copy_masked(
    input: &mut impl std::io::Read,
    output: &mut impl std::io::Write,
    cancelled: &AtomicBool,
    mask: Option<&fork_mask::Mask>,
) -> Result<(u64, u64, String)> {
    if let Some(mask) = mask {
        return mask.copy(input, output, cancelled);
    }
    let (size, digest) = copy(input, output, cancelled)?;
    Ok((size, size, digest))
}

fn copy(
    input: &mut impl std::io::Read,
    output: &mut impl std::io::Write,
    cancelled: &AtomicBool,
) -> Result<(u64, String)> {
    let mut hash = Sha256::new();
    let mut size = 0;
    let mut buffer = vec![0; CHUNK_SIZE];
    loop {
        ensure!(!cancelled.load(Ordering::Acquire), "file sharing cancelled");
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size += count as u64;
        ensure!(size <= MAX_FILE, "file exceeds 100 MiB");
        output.write_all(&buffer[..count])?;
        hash.update(&buffer[..count]);
    }
    Ok((size, digest_hex(&hash.finalize())))
}

pub fn digest_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Formatting into a String cannot fail.
        let _ = write!(value, "{byte:02x}");
    }
    value
}

pub fn read(root: &Path, meta: &SharedFile, offset: u64) -> Result<Vec<u8>> {
    ensure!(meta.size <= MAX_FILE && offset <= meta.size, "invalid file offset or size");
    let mut file = open_regular(&path(root, &meta.id)?)?;
    ensure!(file.metadata()?.len() == meta.size, "shared file is missing or damaged");
    file.seek(SeekFrom::Start(offset))?;
    let length = usize::try_from((meta.size - offset).min(CHUNK_SIZE as u64))?;
    let mut data = vec![0; length];
    file.read_exact(&mut data)?;
    Ok(data)
}

#[cfg(test)]
mod tests;
