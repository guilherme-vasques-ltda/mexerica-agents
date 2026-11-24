// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! The screen helper's socket: accepting helpers, JSON-RPC requests to them, and the Linux helper process.
//! Windows has no screen helper yet, so nothing listens there.

#[cfg(unix)]
use super::protocol::HelperStatus;
use super::{HELPER_TIMEOUT, Screen};
use crate::LockExt;
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
#[cfg(unix)]
use tokio::io::{AsyncBufReadExt, BufReader};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};

use super::fork_observation::{self, Attempt};

pub(super) type Pending = Mutex<HashMap<i64, oneshot::Sender<Result<Value, String>>>>;

/// One connected screen helper.
pub(super) struct Outgoing {
    pub(super) line: String,
    pub(super) id: i64,
    pub(super) attempt: Option<Arc<Attempt>>,
}

pub(super) struct Link {
    #[cfg_attr(windows, expect(dead_code, reason = "Windows has no screen helper yet"))]
    pub(super) id: u64,
    pub(super) tx: mpsc::UnboundedSender<Outgoing>,
    pub(super) pending: Pending,
    pub(super) next_id: AtomicI64,
    #[cfg(target_os = "linux")]
    pub(super) restore_notice: Mutex<super::fork_restore_notice::LinkNotice>,
}

impl Link {
    #[cfg(unix)]
    pub(super) fn accept_status(&self, screen: &Screen, params: &Value) {
        if let Ok(st) = serde_json::from_value::<HelperStatus>(params.clone()) {
            #[cfg(target_os = "linux")]
            {
                let mut notice = self.restore_notice.locked();
                notice.observe(params);
                let changed = notice.new_public_status(&st);
                if st.platform == "linux" && !changed {
                    return;
                }
            }
            *screen.status.locked() = st;
            screen.emit();
        } else {
            #[cfg(target_os = "linux")]
            self.restore_notice.locked().observe(&Value::Null);
            tracing::warn!("bad status from the screen helper");
        }
    }

    pub(super) async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.request_inner(method, params, None).await
    }

    pub(super) async fn request_observed(&self, method: &str, params: Value, attempt: &Arc<Attempt>) -> Result<Value> {
        let result = self.request_inner(method, params, Some(attempt.clone())).await;
        attempt.with_send(Some(self.id), || ())?;
        result
    }

    async fn request_inner(&self, method: &str, params: Value, attempt: Option<Arc<Attempt>>) -> Result<Value> {
        self.enqueue_inner(method, &params, attempt)?.1.await
    }

    /// Enqueue real antes do retorno; a future já possui o waiter, mesmo sem poll.
    pub(super) fn enqueue<'a>(
        &'a self,
        method: &str,
        params: &Value,
    ) -> Result<impl Future<Output = Result<Value>> + Send + 'a + use<'a>> {
        Ok(self.enqueue_identified(method, params)?.1)
    }

    pub(super) fn enqueue_identified<'a>(
        &'a self,
        method: &str,
        params: &Value,
    ) -> Result<(i64, impl Future<Output = Result<Value>> + Send + 'a + use<'a>)> {
        self.enqueue_inner(method, params, None)
    }

    fn enqueue_inner<'a>(
        &'a self,
        method: &str,
        params: &Value,
        attempt: Option<Arc<Attempt>>,
    ) -> Result<(i64, impl Future<Output = Result<Value>> + Send + 'a + use<'a>)> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.locked().insert(id, tx);
        let waiter = Waiter { link: self, id };
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
        if self.tx.send(Outgoing { line, id, attempt }).is_err() {
            bail!("the screen helper disconnected");
        }
        // O prazo original começa no enqueue, não no primeiro poll da espera.
        let timeout = if method == "requestPermission" { Duration::from_secs(120) } else { HELPER_TIMEOUT };
        let deadline = tokio::time::Instant::now() + timeout;
        let method = method.to_owned();
        Ok((id, async move {
            let _waiter = waiter;
            match tokio::time::timeout_at(deadline, rx).await {
                Ok(Ok(Ok(v))) => Ok(v),
                Ok(Ok(Err(message))) => Err(anyhow!(message)),
                Ok(Err(_)) => bail!("the screen helper disconnected"),
                Err(_) => bail!("the screen helper didn't answer `{method}` in time"),
            }
        }))
    }
}

// Drop retira somente o waiter desta conexão/tentativa; não encerra o observador.
struct Waiter<'a> {
    link: &'a Link,
    id: i64,
}
impl Drop for Waiter<'_> {
    fn drop(&mut self) {
        self.link.pending.locked().remove(&self.id);
    }
}

#[cfg(windows)]
pub async fn serve_helpers(_screen: Arc<Screen>) {}

/// Accepts screen helpers on the socket captured by Screen. The newest connection wins.
#[cfg(unix)]
pub async fn serve_helpers(screen: Arc<Screen>) {
    let path = match &screen.helper_socket {
        Ok(path) => path,
        Err(error) => {
            tracing::error!(%error, "não foi possível resolver o socket do helper");
            return;
        }
    };
    let _ = std::fs::remove_file(path);
    let listener = match UnixListener::bind(path) {
        Ok(l) => l,
        Err(error) => {
            tracing::error!(%error, path = %path.display(), "can't listen for the screen helper");
            return;
        }
    };
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            tracing::warn!(%error, "couldn't restrict the screen helper socket");
        }
    }
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(run_link(screen.clone(), stream));
            }
            Err(error) => {
                tracing::warn!(%error, "screen helper accept failed");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn graphical_session() -> bool {
    true
}

#[cfg(windows)]
pub(super) fn graphical_session() -> bool {
    false
}

/// A Wayland socket in this user's runtime dir, or a running X server.
#[cfg(target_os = "linux")]
pub(super) fn graphical_session() -> bool {
    let has = |dir: &std::path::Path, prefix: &str| {
        std::fs::read_dir(dir)
            .is_ok_and(|entries| entries.flatten().any(|e| e.file_name().to_string_lossy().starts_with(prefix)))
    };
    std::env::var_os("XDG_RUNTIME_DIR").is_some_and(|dir| has(dir.as_ref(), "wayland-"))
        || has(std::path::Path::new("/tmp/.X11-unix"), "X")
}

/// Linux: the host runs `codync-screen` itself while Remote screen is on (on macOS
/// launchd runs Codync Screen for the app). Restarts it if it exits; stops it when turned off.
#[cfg(target_os = "linux")]
pub async fn supervise_linux_helper(screen: Arc<Screen>) {
    let socket = match &screen.helper_socket {
        Ok(path) => path,
        Err(error) => {
            tracing::error!(%error, "não foi possível resolver o socket do helper");
            return;
        }
    };
    let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("codync-screen")));
    let program = exe.filter(|p| p.is_file()).unwrap_or_else(|| "codync-screen".into());
    let mut child: Option<tokio::process::Child> = None;
    let mut backoff = Duration::from_secs(1);
    loop {
        let running = child.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)));
        if screen.enabled() && !running {
            match tokio::process::Command::new(&program)
                .args(super::fork_socket::helper_args(socket))
                .kill_on_drop(true)
                .spawn()
            {
                Ok(c) => child = Some(c),
                Err(error) => tracing::warn!(%error, program = %program.display(), "can't start codync-screen"),
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(30));
            continue;
        }
        if !screen.enabled()
            && let Some(mut c) = child.take()
        {
            let _ = c.kill().await;
        }
        if running && screen.link.locked().is_some() {
            backoff = Duration::from_secs(1);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[cfg(unix)]
pub(super) async fn run_link(screen: Arc<Screen>, stream: UnixStream) {
    let (rd, wr) = stream.into_split();
    let (tx, rx) = mpsc::unbounded_channel::<Outgoing>();
    let link = Arc::new(Link {
        id: screen.next_link.fetch_add(1, Ordering::Relaxed),
        tx,
        pending: Mutex::default(),
        next_id: AtomicI64::new(1),
        #[cfg(target_os = "linux")]
        restore_notice: Mutex::default(),
    });
    screen.attach_viewer_link(link.clone());
    tracing::info!("screen helper connected");
    let writer = tokio::spawn(fork_observation::helper_writer(wr, rx, link.clone()));
    let mut lines = BufReader::new(rd).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            tracing::debug!("ignoring non-JSON from the screen helper");
            continue;
        };
        match (msg["method"].as_str(), msg["id"].as_i64()) {
            (None, Some(id)) => {
                let waiter = link.pending.locked().remove(&id);
                if let Some(waiter) = waiter {
                    #[cfg(target_os = "linux")]
                    link.restore_notice.locked().close_reply(id, &msg);
                    let res = match msg.get("error") {
                        Some(e) => Err(e["message"].as_str().map_or_else(|| e.to_string(), str::to_owned)),
                        None => Ok(msg["result"].clone()),
                    };
                    let _ = waiter.send(res);
                }
            }
            (Some("status"), _) => link.accept_status(&screen, &msg["params"]),
            (Some("session"), _) => {
                if msg["params"]["state"] == "closed"
                    && let Some(s) = msg["params"]["session"].as_str()
                {
                    screen.session_closed(s, &link);
                }
            }
            (Some(method), _) => tracing::debug!(method, "unknown screen helper notification"),
            _ => {}
        }
    }
    writer.abort();
    link.pending.locked().clear();
    screen.detach_viewer_link(&link);
    tracing::info!("screen helper disconnected");
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::screen::tests::retina;
    use crate::screen::viewer::{IceConfig, IceServer};
    use tokio::io::AsyncWriteExt;
    use tokio::sync::broadcast;

    #[tokio::test]
    async fn helper_roundtrip_over_the_socket() {
        let (tx, _) = broadcast::channel(8);
        let screen = Arc::new(Screen::new(Some(true), tx));
        let (a, b) = UnixStream::pair().unwrap();
        let (observed_tx, mut observed_rx) = mpsc::unbounded_channel();
        // Fake helper reports a display and records the host's ICE configuration.
        tokio::spawn(async move {
            let (rd, mut wr) = b.into_split();
            let status = json!({"jsonrpc": "2.0", "method": "status", "params": {"platform": "test", "capture": true, "displays": [retina()]}});
            wr.write_all(format!("{status}\n").as_bytes()).await.unwrap();
            let mut lines = BufReader::new(rd).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                let m: Value = serde_json::from_str(&l).unwrap();
                observed_tx.send(m.clone()).unwrap();
                let res = json!({"jsonrpc": "2.0", "id": m["id"], "result": {"sdp": format!("answer to {}", m["params"]["sdp"].as_str().unwrap_or_default())}});
                wr.write_all(format!("{res}\n").as_bytes()).await.unwrap();
            }
        });
        tokio::spawn(run_link(screen.clone(), a));
        tokio::time::timeout(Duration::from_secs(60), async {
            while !screen.status.locked().capture {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("o helper da fixture deve anunciar capture");
        let local = screen.prepare("local", IceConfig::default()).unwrap();
        let res = screen.offer("local", "offer", local["session"].as_str().unwrap(), None).await.unwrap();
        assert_eq!(res["sdp"], "answer to offer");
        assert_eq!(screen.state()["viewers"], 1);
        assert_eq!(screen.state()["displays"][0]["name"], "Built-in");
        let direct = observed_rx.recv().await.unwrap();
        assert_eq!(direct["params"]["iceServers"], json!([]));
        assert_eq!(direct["params"]["maxBitrateBps"], 16_000_000);
        let config = IceConfig {
            ice_servers: vec![IceServer {
                urls: vec!["turn:turn.cloudflare.com:3478?transport=udp".into()],
                username: Some("u".into()),
                credential: Some("p".into()),
            }],
            ..IceConfig::default()
        };
        screen.takeover(true);
        let prepared = screen.prepare("phone", config).unwrap();
        screen.session_closed(res["session"].as_str().unwrap(), &screen.link().unwrap());
        assert_eq!(screen.state()["userControl"], true, "renewal keeps takeover while the replacement is pending");
        let id = prepared["session"].as_str().unwrap();
        assert!(screen.offer("other", "offer", id, None).await.is_err());
        screen.offer("phone", "offer", id, None).await.unwrap();
        let remote = observed_rx.recv().await.unwrap();
        assert_eq!(remote["params"]["iceServers"], prepared["iceServers"]);
        assert_eq!(remote["params"]["maxBitrateBps"], 4_000_000);
        assert_eq!(remote["params"]["maxFramerate"], 30);
        screen.sessions.locked().rows.get(id).unwrap().locked().ice.as_mut().unwrap().expires_at = 0;
        assert!(screen.offer("phone", "offer", id, None).await.is_err());
    }
}
