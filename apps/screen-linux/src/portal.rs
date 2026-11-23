//! One xdg `RemoteDesktop` session with screen cast sources: input injection and
//! PipeWire streams for every monitor. Persisted with a restore token so the user
//! approves once.

use anyhow::{Context, Result, anyhow};
use ashpd::desktop::PersistMode;
use ashpd::desktop::Session;
use ashpd::desktop::remote_desktop::{DeviceType, RemoteDesktop, SelectDevicesOptions};
use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::io::Write;
use std::os::fd::OwnedFd;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A monitor as the portal streams it. `width`/`height` are logical pixels: the
/// coordinate space of input events and of the host's `Display` points.
#[derive(Clone, Debug)]
pub struct Display {
    pub id: u32,
    pub node: u32,
    pub x: i32,
    pub y: i32,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone)]
pub struct Portal(Arc<Inner>);

struct Inner {
    remote: RemoteDesktop,
    screencast: Screencast,
    session: Session<RemoteDesktop>,
    displays: Vec<Display>,
    input: bool,
    active: AtomicBool,
}

impl Portal {
    /// Apenas fixture: proxies reais apontados ao barramento privado do teste.
    #[cfg(test)]
    pub(crate) async fn for_input_test(connection: zbus::Connection) -> Result<Self> {
        let remote = RemoteDesktop::with_connection(connection.clone()).await?;
        let screencast = Screencast::with_connection(connection).await?;
        let session = remote.create_session(Default::default()).await?;
        Ok(Self(Arc::new(Inner {
            remote,
            screencast,
            session,
            displays: vec![Display {
                id: 0,
                node: 1,
                x: 0,
                y: 0,
                width: 64.0,
                height: 64.0,
            }],
            input: true,
            active: AtomicBool::new(true),
        })))
    }

    pub async fn start(data: &Path, restore: bool) -> Result<Self> {
        let token_file = data.join("screen-restore-token");
        let token = if restore {
            match read_token(&token_file) {
                Ok(token) => Some(token),
                Err(error) => {
                    tracing::warn!(error = %error, "couldn't read the portal restore token");
                    None
                }
            }
        } else {
            None
        };
        let remote = RemoteDesktop::new()
            .await
            .context("the RemoteDesktop portal isn't available")?;
        let screencast = Screencast::new()
            .await
            .context("the ScreenCast portal isn't available")?;
        let session = remote.create_session(Default::default()).await?;
        remote
            .select_devices(
                &session,
                SelectDevicesOptions::default()
                    .set_devices(DeviceType::Keyboard | DeviceType::Pointer)
                    .set_persist_mode(PersistMode::ExplicitlyRevoked)
                    .set_restore_token(token.as_deref().map(str::trim)),
            )
            .await?;
        screencast
            .select_sources(
                &session,
                SelectSourcesOptions::default()
                    .set_cursor_mode(CursorMode::Embedded)
                    .set_sources(ashpd::enumflags2::BitFlags::from(SourceType::Monitor))
                    .set_multiple(true),
            )
            .await?;
        let started = remote
            .start(&session, None, Default::default())
            .await?
            .response()
            .context("remote access to this screen was not allowed")?;
        if let Some(t) = started.restore_token() {
            save_token(&token_file, t);
        }
        let displays = started
            .streams()
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let (x, y) = s.position().unwrap_or((0, 0));
                let (w, h) = s.size().unwrap_or((1920, 1080));
                Display {
                    id: u32::try_from(i).unwrap_or(0),
                    node: s.pipe_wire_node_id(),
                    x,
                    y,
                    width: f64::from(w),
                    height: f64::from(h),
                }
            })
            .collect::<Vec<_>>();
        if displays.is_empty() {
            return Err(anyhow!("the portal shared no screen"));
        }
        let input = started
            .devices()
            .contains(DeviceType::Pointer | DeviceType::Keyboard);
        let portal = Self(Arc::new(Inner {
            remote,
            screencast,
            session,
            displays,
            input,
            active: AtomicBool::new(true),
        }));
        // Ending sharing in the system UI must make setup available again.
        let watched = portal.clone();
        tokio::spawn(async move {
            match watched.session().receive_closed().await {
                Ok(mut closed) => {
                    let _ = closed.next().await;
                }
                Err(error) => tracing::warn!(%error, "could not watch the screen sharing session"),
            }
            watched.0.active.store(false, Ordering::Relaxed);
        });
        Ok(portal)
    }

    pub fn is_active(&self) -> bool {
        self.0.active.load(Ordering::Relaxed)
    }

    pub fn has_input(&self) -> bool {
        self.is_active() && self.0.input
    }

    pub fn status(&self) -> Value {
        json!({
            "platform": "linux",
            "permissionApp": "Codync Screen",
            "capture": self.is_active(),
            "input": self.has_input(),
            "displays": self.0.displays.iter().map(|d| json!({
                "id": d.id,
                "name": format!("Display {}", d.id + 1),
                "width": d.width,
                "height": d.height,
                "main": d.id == 0,
            })).collect::<Vec<_>>(),
        })
    }

    pub fn display(&self, id: Option<u64>) -> Result<Display> {
        let id = id.map_or(0, |i| u32::try_from(i).unwrap_or(u32::MAX));
        self.0
            .displays
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .ok_or_else(|| anyhow!("no such display"))
    }

    /// A fresh PipeWire remote for one consumer (each pipeline owns its fd).
    pub async fn pipewire_fd(&self) -> Result<OwnedFd> {
        Ok(self
            .0
            .screencast
            .open_pipe_wire_remote(&self.0.session, Default::default())
            .await?)
    }

    pub fn remote(&self) -> &RemoteDesktop {
        &self.0.remote
    }

    pub fn session(&self) -> &Session<RemoteDesktop> {
        &self.0.session
    }
}

fn save_token(path: &Path, token: &str) {
    let result = save_token_atomically(path, |file| file.write_all(token.as_bytes()));
    if let Err(error) = result {
        tracing::warn!(error = %error, "couldn't save the portal restore token");
    }
}

fn read_token(path: &Path) -> Result<String> {
    use rustix::fs::{AtFlags, FileType, Mode, OFlags, fstat, openat, statat};
    use std::io::Read;

    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("token path has no parent"))?;
    let parent_fd = openat(
        rustix::fs::CWD,
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    let parent_metadata = fstat(&parent_fd)?;
    if FileType::from_raw_mode(parent_metadata.st_mode) != FileType::Directory
        || parent_metadata.st_uid != rustix::process::geteuid().as_raw()
        || parent_metadata.st_mode & 0o022 != 0
        || parent_metadata.st_mode & 0o700 != 0o700
    {
        return Err(anyhow!("token directory is not privately controlled"));
    }
    let filename = path
        .file_name()
        .ok_or_else(|| anyhow!("invalid token file name"))?;
    let metadata = statat(&parent_fd, filename, AtFlags::SYMLINK_NOFOLLOW)?;
    if FileType::from_raw_mode(metadata.st_mode) != FileType::RegularFile
        || metadata.st_uid != rustix::process::geteuid().as_raw()
        || metadata.st_nlink != 1
    {
        return Err(anyhow!("token path is not a private regular owned file"));
    }
    let fd = openat(
        &parent_fd,
        filename,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )?;
    let opened = fstat(&fd)?;
    if opened.st_dev != metadata.st_dev
        || opened.st_ino != metadata.st_ino
        || opened.st_uid != metadata.st_uid
        || opened.st_nlink != 1
        || FileType::from_raw_mode(opened.st_mode) != FileType::RegularFile
        || opened.st_mode & 0o077 != 0
    {
        return Err(anyhow!("token path changed during validation"));
    }
    let mut token = String::new();
    std::fs::File::from(fd).read_to_string(&mut token)?;
    Ok(token)
}

fn save_token_atomically<F>(path: &Path, write: F) -> Result<()>
where
    F: FnOnce(&mut std::fs::File) -> std::io::Result<()>,
{
    use rustix::fs::{AtFlags, FileType, Mode, OFlags, fstat, openat, renameat, unlinkat};
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("token path has no parent"))?;
    let owner = rustix::process::geteuid().as_raw();
    let parent_file = openat(
        rustix::fs::CWD,
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    let parent_metadata = fstat(&parent_file)?;
    if FileType::from_raw_mode(parent_metadata.st_mode) != FileType::Directory
        || parent_metadata.st_uid != owner
        || parent_metadata.st_mode & 0o022 != 0
        || parent_metadata.st_mode & 0o700 != 0o700
    {
        return Err(anyhow!("token directory is not privately controlled"));
    }
    let filename = path
        .file_name()
        .ok_or_else(|| anyhow!("invalid token file name"))?;
    validate_existing_token(&parent_file, filename, owner)?;

    let (temp_name, mut file) = loop {
        let candidate = format!(
            ".screen-restore-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        );
        match openat(
            &parent_file,
            candidate.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW,
            Mode::from_raw_mode(0o600),
        ) {
            Ok(fd) => break (candidate, std::fs::File::from(fd)),
            Err(rustix::io::Errno::EXIST) => continue,
            Err(error) => return Err(error.into()),
        }
    };
    let result = (|| -> Result<()> {
        write(&mut file)?;
        file.flush()?;
        file.sync_all()?;
        validate_existing_token(&parent_file, filename, owner)?;
        let actual_parent = fstat(&parent_file)?;
        if actual_parent.st_dev != parent_metadata.st_dev
            || actual_parent.st_ino != parent_metadata.st_ino
        {
            return Err(anyhow!("token directory changed before publication"));
        }
        renameat(&parent_file, temp_name.as_str(), &parent_file, filename)?;
        rustix::fs::fsync(&parent_file)?;
        Ok(())
    })();
    drop(file);
    if result.is_err() {
        let _ = unlinkat(&parent_file, temp_name.as_str(), AtFlags::empty());
    }
    result
}

fn validate_existing_token<Fd: std::os::fd::AsFd>(
    parent: &Fd,
    filename: &std::ffi::OsStr,
    owner: u32,
) -> Result<()> {
    use rustix::fs::{AtFlags, FileType, Mode, OFlags, fstat, openat, statat};

    match statat(parent, filename, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(metadata)
            if FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile
                && metadata.st_uid == owner
                && metadata.st_nlink == 1 =>
        {
            let fd = openat(
                parent,
                filename,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK,
                Mode::empty(),
            )?;
            let opened = fstat(&fd)?;
            if opened.st_dev != metadata.st_dev
                || opened.st_ino != metadata.st_ino
                || opened.st_uid != owner
                || opened.st_nlink != 1
                || FileType::from_raw_mode(opened.st_mode) != FileType::RegularFile
            {
                return Err(anyhow!("token path changed during validation"));
            }
            Ok(())
        }
        Ok(_) => Err(anyhow!("token path is not a private regular owned file")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod token_tests {
    use super::{save_token, save_token_atomically};
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};

    #[test]
    fn restore_token_is_private_and_never_follows_links() {
        let dir = std::env::temp_dir().join(format!("screen-token-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .expect("create synthetic token fixture");
        let path = dir.join("token");
        save_token_atomically(&path, |file| file.write_all(b"synthetic-token"))
            .expect("create private token");
        let metadata = std::fs::symlink_metadata(&path).expect("read token fixture metadata");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read token fixture"),
            "synthetic-token"
        );

        let updated = dir.join("updated");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&updated)
            .expect("create previous token fixture")
            .write_all(b"previous-token")
            .expect("write previous token fixture");
        save_token(&updated, "replacement-token");
        assert_eq!(
            std::fs::metadata(&updated)
                .expect("read updated token metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::read_to_string(&updated).expect("read updated token fixture"),
            "replacement-token"
        );

        let target = dir.join("target");
        std::fs::write(&target, "unchanged").expect("create synthetic target");
        let link = dir.join("link");
        std::os::unix::fs::symlink(&target, &link).expect("create synthetic symlink");
        save_token_atomically(&link, |file| file.write_all(b"must-not-follow"))
            .expect_err("refuse symlink");
        assert_eq!(
            std::fs::read_to_string(&target).expect("read synthetic target"),
            "unchanged"
        );
        assert!(
            std::fs::symlink_metadata(&link)
                .expect("inspect synthetic link")
                .file_type()
                .is_symlink()
        );

        let existing = dir.join("existing");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&existing)
            .expect("create existing fixture")
            .write_all(b"preserve")
            .expect("write existing fixture");
        save_token_atomically(&existing, |file| file.write_all(b"must-not-truncate"))
            .expect("replace owned regular token");
        assert_eq!(
            std::fs::read_to_string(&existing).expect("read existing fixture"),
            "must-not-truncate"
        );

        let hardlink = dir.join("hardlink");
        std::fs::hard_link(&existing, &hardlink).expect("create synthetic hardlink");
        save_token_atomically(&hardlink, |file| {
            file.write_all(b"must-not-replace-linked-file")
        })
        .expect_err("refuse a multiply linked token target");
        assert_eq!(
            std::fs::read_to_string(&existing).expect("read linked token fixture"),
            "must-not-truncate"
        );

        let failed = dir.join("failed");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&failed)
            .expect("create prior token")
            .write_all(b"old-token")
            .expect("write prior token");
        save_token_atomically(&failed, |_| Err(std::io::Error::other("synthetic failure")))
            .expect_err("preserve old token on write failure");
        assert_eq!(
            std::fs::read_to_string(&failed).expect("read prior token"),
            "old-token"
        );

        std::fs::remove_dir_all(&dir).expect("remove synthetic token fixture");
    }
}
