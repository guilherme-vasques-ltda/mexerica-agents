//! Manifesto e arquivos de upload presos à conversa; UUID conhecido não resolve outro contexto.
use super::conversations::Conversation;
use crate::{
    LockExt,
    store::{BotConfig, Store},
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
const MAX: u64 = 100 * 1024 * 1024;
fn name(value: &str) -> Result<String> {
    ensure!(
        !value.is_empty()
            && value.len() <= 200
            && value != "."
            && value != ".."
            && !value.starts_with('.')
            && !value.contains(['/', '\\', ':'])
            && !value.chars().any(char::is_control),
        "invalid_upload_name"
    );
    Ok(value.into())
}
#[derive(Clone)]
struct Manifest {
    name: String,
    size: u64,
    done: bool,
    digest: Option<String>,
    revision: i64,
}
fn manifest(store: &Store, c: &Conversation, id: &str) -> Result<Option<Manifest>> {
    uuid::Uuid::parse_str(id)?;
    let value = store.db.locked().query_row("SELECT name,size,complete,sha256,revision FROM donna_space_uploads WHERE conversation_id=?1 AND space_id=?2 AND id=?3",params![c.id,c.space_id,id],|r|Ok(Manifest{name:r.get(0)?,size:u64::try_from(r.get::<_,i64>(1)?).map_err(|e|rusqlite::Error::FromSqlConversionFailure(1,rusqlite::types::Type::Integer,Box::new(e)))?,done:match r.get::<_,i64>(2)? {0=>false,1=>true,v=>return Err(rusqlite::Error::IntegralValueOutOfRange(2,v))},digest:r.get(3)?,revision:r.get(4)?})).optional()?;
    if let Some(m) = &value {
        name(&m.name)?;
        ensure!(m.size <= MAX && m.revision > 0, "invalid_upload_manifest");
        ensure!(
            if m.done {
                m.digest.as_ref().is_some_and(|v| {
                    v.len() == 64 && v.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                })
            } else {
                m.digest.is_none()
            },
            "invalid_upload_manifest"
        );
    }
    Ok(value)
}
fn value(c: &Conversation, id: &str, m: &Manifest) -> Value {
    json!({"id":id,"name":m.name,"size":m.size,"sha256":m.digest,"revision":m.revision,"spaceId":c.space_id,"conversationId":c.id})
}
fn relative(cfg: &BotConfig, id: &str) -> Result<PathBuf> {
    uuid::Uuid::parse_str(id)?;
    let context = cfg.context.as_ref().ok_or_else(|| anyhow!("additional_conversation_required"))?;
    Ok(PathBuf::from("spaces")
        .join(&cfg.space_id)
        .join("conversations")
        .join(&context.conversation_id)
        .join("workspace/uploads")
        .join(id))
}
#[cfg(unix)]
fn anchored(store: &Store, cfg: &BotConfig, id: &str, create: bool) -> Result<File> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{ffi::OsStrExt, fs::OpenOptionsExt},
        },
    };
    if crate::fork::motor_exec::configured() {
        let root = crate::chat::uploads::root(cfg);
        ensure!(root.starts_with(crate::fork::motor_roots::work()), "invalid_upload_root");
        let dir = if create {
            crate::fork::motor_exec::fd::Directory::create(&root, 0o2770)?
        } else {
            crate::fork::motor_exec::fd::Directory::open(&root)?
        };
        crate::fork::motor_exec::fd::own(&dir.0)?;
        dir.mode(0o2750)?;
        let dir = dir.child(id.as_ref(), create.then_some(0o2750))?;
        crate::fork::motor_exec::fd::own(&dir.0)?;
        dir.mode(0o2750)?;
        return Ok(dir.0);
    }
    let root = store.engine_setup_root.parent().ok_or_else(|| anyhow!("storage_unavailable"))?;
    let mut dir =
        File::options().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC).open(root)?;
    for component in relative(cfg, id)?.components() {
        ensure!(matches!(component, std::path::Component::Normal(_)), "invalid_upload_root");
        let text = CString::new(component.as_os_str().as_bytes())?;
        // SAFETY: o nome é um componente e o fd permanece vivo, sem seguir symlinks.
        let mut fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                text.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0,
            )
        };
        if fd < 0 && create && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
            // SAFETY: mkdirat e a abertura seguinte usam o mesmo descritor ancorado.
            let rc = unsafe { libc::mkdirat(dir.as_raw_fd(), text.as_ptr(), 0o700) };
            if rc < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists {
                return Err(std::io::Error::last_os_error().into());
            }
            // SAFETY: reabertura do mesmo componente relativo ao descritor ancorado.
            fd = unsafe {
                libc::openat(
                    dir.as_raw_fd(),
                    text.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    0,
                )
            };
        }
        ensure!(fd >= 0, "upload_directory_unavailable");
        // SAFETY: openat retornou um descritor novo, pertencente a esta operação.
        dir = unsafe { File::from_raw_fd(fd) };
    }
    Ok(dir)
}
#[cfg(not(unix))]
fn anchored(_store: &Store, _cfg: &BotConfig, _id: &str, _create: bool) -> Result<File> {
    Err(anyhow!("scoped_uploads_require_supported_filesystem"))
}
#[cfg(unix)]
fn open(dir: &File, filename: &str, write: bool) -> Result<File> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::MetadataExt,
        },
    };
    let text = CString::new(name(filename)?)?;
    let flags = if write { libc::O_RDWR | libc::O_CREAT } else { libc::O_RDONLY };
    // SAFETY: nome simples validado e descritor do pai fixam o arquivo, sem seguir links/FIFOs.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            text.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0o600,
        )
    };
    ensure!(fd >= 0, "upload_file_unavailable");
    // SAFETY: o descritor novo é transferido uma vez ao File.
    let file = unsafe { File::from_raw_fd(fd) };
    let meta = file.metadata()?;
    ensure!(meta.is_file() && meta.nlink() == 1, "upload_links_refused");
    if crate::fork::motor_exec::configured() {
        use std::os::unix::fs::PermissionsExt;
        crate::fork::motor_exec::fd::own(&file)?;
        file.set_permissions(std::fs::Permissions::from_mode(0o640))?;
    }
    Ok(file)
}
#[cfg(not(unix))]
fn open(_dir: &File, _name: &str, _write: bool) -> Result<File> {
    Err(anyhow!("scoped_uploads_require_supported_filesystem"))
}
fn digest(file: &mut File) -> Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut bytes = vec![0_u8; 64 * 1024];
    let mut total = 0;
    loop {
        let count = file.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        total += count;
        ensure!(u64::try_from(total)? <= MAX, "upload_too_large");
        hash.update(&bytes[..count]);
    }
    Ok(crate::chat::files::digest_hex(&hash.finalize()))
}
pub(super) fn append(store: &Store, c: &Conversation, cfg: &BotConfig, b: &Value) -> Result<Value> {
    use base64::Engine as _;
    let id = super::conversations::arg(b, "uploadId")?;
    let name = name(super::conversations::arg(b, "name")?)?;
    let offset = b["offset"].as_u64().ok_or_else(|| anyhow!("offset_required"))?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(super::conversations::arg(b, "data")?)?;
    ensure!(bytes.len() <= 384 * 1024, "upload_chunk_too_large");
    let end = offset.checked_add(u64::try_from(bytes.len())?).ok_or_else(|| anyhow!("upload_too_large"))?;
    ensure!(end <= MAX, "upload_too_large");
    let old = manifest(store, c, id)?;
    if let Some(m) = &old {
        ensure!(m.name == name, "upload_name_conflict");
    } else {
        ensure!(offset == 0, "upload_out_of_order");
    }
    let dir = anchored(store, cfg, id, true)?;
    let mut file = open(&dir, &name, !old.as_ref().is_some_and(|m| m.done))?;
    let size = file.metadata()?.len();
    ensure!(size == old.as_ref().map_or(0, |m| m.size), "upload_file_changed");
    if old.as_ref().is_none_or(|m| !m.done) && offset == size {
        file.seek(SeekFrom::End(0))?;
        file.write_all(&bytes)?;
    } else {
        ensure!(end <= size, "upload_out_of_order");
        file.seek(SeekFrom::Start(offset))?;
        let mut previous = vec![0; bytes.len()];
        file.read_exact(&mut previous)?;
        ensure!(previous == bytes, "upload_chunk_conflict");
    }
    let size = file.metadata()?.len();
    let done = b["done"] == true || old.as_ref().is_some_and(|m| m.done);
    let hash = if done { Some(digest(&mut file)?) } else { None };
    if let Some(m) = &old
        && m.done
    {
        ensure!(m.digest == hash, "upload_file_changed");
        return Ok(json!({"attachment":value(c,id,m),"replayed":true}));
    }
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    let rev = crate::store::next_rev(&tx)?;
    tx.execute("INSERT INTO donna_space_uploads(conversation_id,space_id,id,name,size,complete,sha256,revision) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(conversation_id,id) DO UPDATE SET size=?5,complete=?6,sha256=?7,revision=?8",params![c.id,c.space_id,id,name,i64::try_from(size)?,done,hash,rev])?;
    tx.commit()?;
    let result = Manifest { name, size, done, digest: hash, revision: rev };
    Ok(json!({"attachment":done.then(||value(c,id,&result)),"revision":rev}))
}
pub(super) fn resolve(store: &Store, c: &Conversation, cfg: &BotConfig, id: &str) -> Result<(PathBuf, Value)> {
    let m = manifest(store, c, id)?.filter(|m| m.done).ok_or_else(|| anyhow!("upload_not_found"))?;
    let dir = anchored(store, cfg, id, false)?;
    let mut file = open(&dir, &m.name, false)?;
    ensure!(file.metadata()?.len() == m.size && Some(digest(&mut file)?) == m.digest, "upload_file_changed");
    let root = if crate::fork::motor_exec::configured() {
        crate::chat::uploads::root(cfg)
    } else {
        Path::new(&cfg.cwd).join("uploads")
    };
    let path = root.join(id).join(&m.name);
    Ok((path, value(c, id, &m)))
}
pub(super) fn read(store: &Store, c: &Conversation, cfg: &BotConfig, b: &Value) -> Result<Value> {
    use base64::Engine as _;
    let id = super::conversations::arg(b, "uploadId")?;
    let offset = b["offset"].as_u64().ok_or_else(|| anyhow!("offset_required"))?;
    let m = manifest(store, c, id)?.filter(|m| m.done).ok_or_else(|| anyhow!("upload_not_found"))?;
    ensure!(offset <= m.size, "invalid_upload_offset");
    let dir = anchored(store, cfg, id, false)?;
    let mut file = open(&dir, &m.name, false)?;
    ensure!(file.metadata()?.len() == m.size && Some(digest(&mut file)?) == m.digest, "upload_file_changed");
    file.seek(SeekFrom::Start(offset))?;
    let mut data = vec![0; usize::try_from((m.size - offset).min(384 * 1024))?];
    file.read_exact(&mut data)?;
    Ok(json!({"data":base64::engine::general_purpose::STANDARD.encode(data),"size":m.size,"attachment":value(c,id,&m)}))
}

pub(super) fn validate_entry(store: &Store, cfg: &BotConfig, entry: &crate::store::Entry) -> Result<()> {
    let c = store.space_conversation(&cfg.space_id, cfg.session_owner())?;
    if let Some(values) = entry.data["attachments"].as_array() {
        for value in values {
            let id = value["id"].as_str().ok_or_else(|| anyhow!("invalid_upload_id"))?;
            let (_, current) = resolve(store, &c, cfg, id)?;
            ensure!(current == *value, "upload_binding_changed");
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn opener_refuses_non_component_even_without_a_manifest() {
        let root =
            std::fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("d169-r2-open-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let outside = root.join("synthetic-outside.txt");
        std::fs::write(&outside, b"SYNTHETIC_OUTSIDE_ONLY").unwrap();
        let dir = std::fs::File::open(&root).unwrap();
        for bad in [outside.to_str().unwrap(), "../synthetic-outside.txt", "a/b", "a\\b", "..", "a\0b"] {
            let error = super::open(&dir, bad, false).unwrap_err();
            assert!(error.to_string().contains("invalid_upload_name"));
        }
        drop(dir);
        std::fs::remove_dir_all(root).unwrap();
    }
}
