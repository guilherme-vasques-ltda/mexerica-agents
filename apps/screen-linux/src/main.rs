//! `codync-screen`: Remote screen for Linux. The host (`codync-host`) starts it
//! while Remote screen is on; it requires the host’s explicit `--socket` path and serves
//! the same JSON-RPC as the macOS helper (see `host/src/screen/mod.rs`):
//! capture and input through the xdg `RemoteDesktop` + `ScreenCast` portals
//! (approved once, then restored from a saved token), video as H.264 over
//! GStreamer `webrtcbin`, and the accessibility tree over AT-SPI.

mod a11y;
mod fork_input;
mod fork_socket;
mod input;
mod lifecycle;
mod lifecycle_request;
mod native_stop;
mod portal;
mod stream;
mod video;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{Mutex, mpsc};

/// Messages to the host, one JSON line each.
pub type Outbox = mpsc::UnboundedSender<Value>;

pub struct Helper {
    config: fork_socket::Config,
    portal: Arc<Mutex<Option<portal::Portal>>>,
    last_status: Mutex<Option<Value>>,
    sessions: lifecycle::Sessions,
    out: Outbox,
    /// GStreamer calls `ended` from its own threads, outside the Tokio runtime.
    rt: tokio::runtime::Handle,
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = fork_socket::Config::from_args(std::env::args_os().skip(1))?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "codync_screen=info".into()),
        )
        .init();
    gst::init().context("starting GStreamer")?;
    // Starting the helper must not display a system dialog. Setup explicitly opens a session.
    let portal = Arc::new(Mutex::new(None));
    loop {
        match config.connect().await {
            Ok(sock) => {
                tracing::info!("connected to codync-host");
                if let Err(error) = serve(sock, &portal, &config).await {
                    tracing::warn!(error = format!("{error:#}"), "host link ended");
                }
            }
            Err(error) => tracing::debug!(%error, "codync-host isn't listening yet"),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn serve(
    sock: UnixStream,
    portal: &Arc<Mutex<Option<portal::Portal>>>,
    config: &fork_socket::Config,
) -> Result<()> {
    let (rd, mut wr) = sock.into_split();
    let (out, mut outbox) = mpsc::unbounded_channel::<Value>();
    let writer = tokio::spawn(async move {
        while let Some(msg) = outbox.recv().await {
            let mut line = msg.to_string();
            line.push('\n');
            if wr.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
    });
    let helper = Arc::new(Helper {
        config: config.clone(),
        portal: portal.clone(),
        last_status: Mutex::new(None),
        sessions: lifecycle::Sessions::default(),
        out: out.clone(),
        rt: tokio::runtime::Handle::current(),
    });
    helper.send_status().await;
    let watched = helper.clone();
    let status_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3)).await;
            watched.send_status().await;
        }
    });
    let mut lines = BufReader::new(rd).lines();
    let result = loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break Ok(()),
            Err(error) => break Err(error.into()),
        };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let (Some(method), Some(id)) = (
            msg["method"].as_str().map(str::to_owned),
            msg.get("id").cloned(),
        ) else {
            continue;
        };
        let helper = helper.clone();
        // Capture lifecycle in reader order, before spawning or polling any await.
        let prepared =
            lifecycle_request::prepare_rpc(&helper, &method, &msg["params"], id.as_i64());
        tokio::spawn(async move {
            let reply =
                lifecycle_request::respond(&helper, &method, &msg["params"], id, prepared).await;
            let _ = helper.out.send(reply);
        });
    };
    writer.abort();
    status_task.abort();
    let _ = helper.sessions.close_all(true);
    result
}

async fn handle(
    h: &Arc<Helper>,
    method: &str,
    p: &Value,
    prepared: Result<Option<Arc<lifecycle::Operation>>>,
) -> Result<Value> {
    let operation = prepared?;
    if method == "answer" {
        return lifecycle_request::answer(h, p, operation.context("native_stop_lifecycle")?).await;
    }
    if method == "requestPermission" {
        if p["permission"] != "portal" {
            return Err(anyhow!("Linux uses the screen sharing portal"));
        }
        let mut portal = h.portal.lock().await;
        if portal.as_ref().is_none_or(|p| !p.has_input()) {
            let restore = portal.as_ref().is_none_or(|p| !p.is_active());
            let next = portal::Portal::start(h.config.state_dir(), restore).await?;
            if let Some(previous) = portal.as_ref() {
                if previous.is_active() {
                    previous.session().close().await?;
                }
                let _ = h.sessions.close_all(false);
            }
            *portal = Some(next);
        }
        drop(portal);
        h.send_status().await;
        return Ok(json!({}));
    }
    // These actions were captured in reader order; {} remains legacy control.
    if matches!(method, "close" | "closeAll") {
        return Ok(json!({}));
    }
    let portal = h.portal().await?;
    let display = || portal.display(p["display"].as_u64());
    Ok(match method {
        "screenshot" => {
            let (w, h2) = (
                p["width"].as_u64().unwrap_or(1280),
                p["height"].as_u64().unwrap_or(800),
            );
            let d = display()?;
            let fd = portal.pipewire_fd().await?;
            let jpeg = stream::screenshot(fd, &d, u32::try_from(w)?, u32::try_from(h2)?).await?;
            json!({"data": jpeg})
        }
        "input" => {
            let d = display()?;
            // Caminho one-shot do Hub, ordenado pelo leitor: sem sessão própria
            // a cancelar; o cancelamento vive no consumidor da sessão.
            let alive = || true;
            input::perform(&portal, &d, &p["event"], &alive).await?;
            json!({})
        }
        "uiTree" => a11y::frontmost(&display()?).await?,
        "focusedField" => a11y::focused_field().await?,
        "openApp" => {
            input::open_app(
                p["name"]
                    .as_str()
                    .ok_or_else(|| anyhow!("name is required"))?,
            )
            .await?;
            json!({})
        }
        other => return Err(anyhow!("unknown method {other}")),
    })
}

impl Helper {
    async fn portal(&self) -> Result<portal::Portal> {
        self.portal
            .lock()
            .await
            .clone()
            .filter(portal::Portal::is_active)
            .ok_or_else(|| anyhow!("Set up Computer access in Codync on this computer first."))
    }

    async fn send_status(&self) {
        let status = self.portal.lock().await.as_ref().map_or_else(
            || json!({"platform": "linux", "permissionApp": "Codync Screen", "capture": false, "input": false, "displays": []}),
            portal::Portal::status,
        );
        let mut last = self.last_status.lock().await;
        if last.as_ref() == Some(&status) {
            return;
        }
        *last = Some(status.clone());
        let _ = self
            .out
            .send(json!({"jsonrpc": "2.0", "method": "status", "params": status}));
    }

    /// A session's WebRTC connection ended on its own.
    pub(crate) fn ended(self: &Arc<Self>, id: &str, operation: &Arc<lifecycle::Operation>) {
        let this = self.clone();
        let id = id.to_owned();
        let operation = operation.clone();
        self.rt.spawn(async move {
            let _ = this.sessions.finish(&id, &operation, || {
                let _ = this.out.send(json!({"jsonrpc": "2.0", "method": "session", "params": {"session": id, "state": "closed"}}));
            });
        });
    }
}
