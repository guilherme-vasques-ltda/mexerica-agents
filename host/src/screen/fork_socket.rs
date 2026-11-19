//! D517-A: listener e supervisor usam o mesmo caminho capturado em Screen.

use std::path::{Path, PathBuf};

pub(super) fn listener_path(data_dir: &Path) -> std::io::Result<PathBuf> {
    if data_dir.is_absolute() {
        return Ok(data_dir.join("screen.sock"));
    }
    Ok(relative_listener_path(data_dir, &std::env::current_dir()?))
}

fn relative_listener_path(data_dir: &Path, cwd: &Path) -> PathBuf {
    cwd.join(data_dir).join("screen.sock")
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn helper_args(socket: &Path) -> [std::ffi::OsString; 2] {
    ["--socket".into(), socket.as_os_str().to_owned()]
}

#[cfg(test)]
// A fonte importada é conferida pelo fmt de screen_linux, com sua própria configuração.
#[rustfmt::skip]
#[path = "../../../apps/screen-linux/src/fork_socket.rs"]
mod helper_config;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn d520_selected_home_is_shared_by_bind_and_helper() {
        for folder in [".mexerica", ".mexerica-dev"] {
            let default = Path::new("/synthetic/home").join(folder);
            for configured in [None, Some("/synthetic/override com espaços")] {
                let selected = crate::fork::data_home::select(default.clone(), |key| {
                    if key == "MEXERICA_HOME" { configured.map(Into::into) } else { None }
                })
                .unwrap();
                let listener = listener_path(&selected).unwrap();
                let helper = helper_config::Config::from_args(helper_args(&listener)).unwrap();
                assert_eq!(helper.state_dir().join("screen.sock"), listener);
            }
        }
    }

    #[tokio::test]
    async fn d517_host_arguments_select_listener_and_isolate_upstream() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let fixture = std::env::temp_dir().join(format!("d517-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&fixture).unwrap();
        let custom = fixture.join("host com espaços");
        let upstream = fixture.join(".codync");
        std::fs::create_dir(&custom).unwrap();
        std::fs::create_dir(&upstream).unwrap();
        let socket = listener_path(&custom).unwrap();
        // Override relativo resolve no CWD capturado; não se altera env/CWD global.
        let override_dir = Path::new("host com espaços");
        let captured_cwd = std::env::current_dir().unwrap();
        assert_eq!(listener_path(override_dir).unwrap(), captured_cwd.join(override_dir).join("screen.sock"));
        let relative = relative_listener_path(override_dir, &fixture);
        assert_eq!(relative, socket);
        let relative_config = helper_config::Config::from_args(helper_args(&relative)).unwrap();
        assert_eq!(relative_config.state_dir(), custom);
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let upstream_listener = tokio::net::UnixListener::bind(upstream.join("screen.sock")).unwrap();
        let config = helper_config::Config::from_args(helper_args(&socket)).unwrap();
        assert_eq!(config.state_dir(), custom);
        // O mesmo objeto e seu connect são usados pelo loop de reconnect real.
        for _ in 0..2 {
            let connection = relative_config.connect().await.unwrap();
            let (accepted, _) = listener.accept().await.unwrap();
            drop((connection, accepted));
        }
        assert!(tokio::time::timeout(std::time::Duration::from_millis(20), upstream_listener.accept()).await.is_err());
        assert!(!custom.join("screen-restore-token").exists());
        assert!(!upstream.join("screen-restore-token").exists());
        drop((listener, upstream_listener));
        std::fs::remove_dir_all(&fixture).unwrap();
    }
}
