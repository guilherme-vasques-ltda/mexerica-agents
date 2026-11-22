//! Prova de chamada ao portal em D-Bus PRIVADO, sem compositor ou tecla real.

#![allow(clippy::unwrap_used)]
// O dispatch gerado pelo macro do zbus menciona binds sublinhados da fixture.
#![allow(clippy::used_underscore_binding)]
use super::*;
use crate::lifecycle_request::{prepare_rpc, respond};
use crate::native_stop::tests::live_pipeline;
use crate::portal::Portal;
use crate::stream::Session;
use anyhow::{Context, Result, anyhow};
use gst::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use std::io::BufRead;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Str};

struct PrivateBus {
    process: Child,
    directory: PathBuf,
}

impl PrivateBus {
    async fn start() -> Result<(Self, String)> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let directory =
            std::env::temp_dir().join(format!("input-fixture-{}-{nonce}", std::process::id()));
        std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let process = Command::new("dbus-daemon")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .args(["--session", "--nofork", "--print-address=1"])
            .arg(format!(
                "--address=unix:path={}",
                directory.join("bus").display()
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("fixture requer dbus-daemon local; não usa barramento do operador")?;
        let mut bus = Self { process, directory };
        let stdout = bus
            .process
            .stdout
            .take()
            .context("stdout do D-Bus privado")?;
        let address = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || -> Result<String> {
                let mut address = String::new();
                std::io::BufReader::new(stdout).read_line(&mut address)?;
                anyhow::ensure!(address.starts_with("unix:"), "D-Bus privado não iniciou");
                Ok(address.trim().to_owned())
            }),
        )
        .await???;
        Ok((bus, address))
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        // Somente o processo/socket criados por ESTA fixture.
        let _ = self.process.kill();
        let _ = self.process.wait();
        let _ = std::fs::remove_file(self.directory.join("bus"));
        let _ = std::fs::remove_dir(&self.directory);
    }
}

#[derive(Debug)]
struct Event {
    kind: &'static str,
    session: OwnedObjectPath,
    /// Keysym em eventos de tecla; código de botão em eventos de ponteiro.
    code: i32,
    state: u32,
    x: f64,
    y: f64,
    original_pipeline: gst::State,
}

struct RemoteDesktopFixture {
    events: Arc<Mutex<Vec<Event>>>,
    pipeline: gst::Pipeline,
}

fn invalid(message: &str) -> zbus::fdo::Error {
    zbus::fdo::Error::InvalidArgs(message.to_owned())
}

fn record(
    fixture: &RemoteDesktopFixture,
    kind: &'static str,
    session: OwnedObjectPath,
    code: i32,
    state: u32,
    x: f64,
    y: f64,
) -> zbus::fdo::Result<()> {
    fixture
        .events
        .lock()
        .map_err(|_| invalid("observador da fixture indisponível"))?
        .push(Event {
            kind,
            session,
            code,
            state,
            x,
            y,
            original_pipeline: fixture.pipeline.current_state(),
        });
    Ok(())
}

#[zbus::interface(name = "org.freedesktop.portal.RemoteDesktop")]
impl RemoteDesktopFixture {
    #[zbus(property, name = "version")]
    #[allow(clippy::unused_self)]
    fn version(&self) -> u32 {
        2
    }

    async fn create_session(
        &self,
        options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let sender = header.sender().ok_or_else(|| invalid("sender ausente"))?;
        let identity = sender.as_str().trim_start_matches(':').replace('.', "_");
        let token = |key: &str| {
            options
                .get(key)
                .and_then(|value| <&str>::try_from(value).ok())
                .ok_or_else(|| invalid("handle da fixture ausente"))
        };
        let request = OwnedObjectPath::try_from(format!(
            "/org/freedesktop/portal/desktop/request/{identity}/{}",
            token("handle_token")?
        ))
        .map_err(|_| invalid("request inválido"))?;
        let session = format!(
            "/org/freedesktop/portal/desktop/session/{identity}/{}",
            token("session_handle_token")?
        );
        let results = HashMap::from([("session_handle", OwnedValue::from(Str::from(session)))]);
        connection
            .emit_signal(
                Some(sender.clone()),
                request.as_str(),
                "org.freedesktop.portal.Request",
                "Response",
                &(0u32, results),
            )
            .await
            .map_err(|_| invalid("resposta da fixture não entregue"))?;
        Ok(request)
    }

    fn notify_keyboard_keysym(
        &self,
        session: OwnedObjectPath,
        _options: HashMap<String, OwnedValue>,
        keysym: i32,
        state: u32,
    ) -> zbus::fdo::Result<()> {
        record(self, "keysym", session, keysym, state, 0.0, 0.0)
    }

    fn notify_pointer_motion_absolute(
        &self,
        session: OwnedObjectPath,
        _options: HashMap<String, OwnedValue>,
        _stream: u32,
        x: f64,
        y: f64,
    ) -> zbus::fdo::Result<()> {
        record(self, "motion", session, 0, 0, x, y)
    }

    fn notify_pointer_button(
        &self,
        session: OwnedObjectPath,
        _options: HashMap<String, OwnedValue>,
        button: i32,
        state: u32,
    ) -> zbus::fdo::Result<()> {
        record(self, "button", session, button, state, 0.0, 0.0)
    }
}

fn helper() -> Arc<Helper> {
    let (out, _) = mpsc::unbounded_channel();
    Arc::new(Helper {
        config: crate::fork_socket::Config::from_args(
            ["--socket", "/tmp/input-fixture/screen.sock"].map(std::ffi::OsString::from),
        )
        .unwrap(),
        portal: Arc::new(tokio::sync::Mutex::new(None)),
        last_status: tokio::sync::Mutex::new(None),
        sessions: crate::lifecycle::Sessions::default(),
        out,
        rt: tokio::runtime::Handle::current(),
    })
}

async fn original(
    h: &Helper,
    id: &str,
    answer: i64,
) -> Result<(Arc<crate::lifecycle::Operation>, gst::Pipeline)> {
    // Mesmo produtor da admissão original do reader, sem criar autoridade nova.
    prepare_rpc(h, "answer", &json!({"session":id}), Some(answer))?;
    let operation = h.sessions.admit_answer(id, Some(answer), Some(answer))?;
    let pipeline = live_pipeline();
    operation.attach_pipeline(&pipeline)?;
    operation.install(Arc::new(Session::for_test(pipeline.clone())))?;
    operation
        .negotiate(&h.sessions, id, async { Ok(()) })
        .await?;
    Ok((operation, pipeline))
}

async fn consume_event(
    h: Arc<Helper>,
    original: &Arc<crate::lifecycle::Operation>,
    display: crate::portal::Display,
    view: View,
    event: Value,
    cut: Option<Cut>,
) -> Result<()> {
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(event)?;
    drop(tx);
    consume(
        Consumer {
            original: Arc::downgrade(original),
            helper: h,
            channels: Arc::default(),
            display,
            view,
            cut,
        },
        rx,
    )
    .await;
    Ok(())
}

async fn consume_text(
    h: Arc<Helper>,
    original: &Arc<crate::lifecycle::Operation>,
    text: &str,
    cut: Option<Cut>,
) -> Result<()> {
    let display = h.portal().await?.display(Some(0))?;
    let view = View::for_input_test(display.clone())?;
    consume_event(
        h,
        original,
        display,
        view,
        json!({"type":"text","text":text}),
        cut,
    )
    .await
}

#[tokio::test]
async fn input_received_before_close_must_not_call_portal_after_original_null() -> Result<()> {
    tokio::time::timeout(Duration::from_secs(20), async {
        let h = helper();
        let (operation, pipeline) = original(&h, "original", 7).await?;
        let (other, other_pipeline) = original(&h, "other", 8).await?;
        let (_bus, address) = PrivateBus::start().await?;
        let events = Arc::new(Mutex::new(Vec::<Event>::new()));
        let _server = zbus::connection::Builder::address(address.as_str())?
            .name("org.freedesktop.portal.Desktop")?
            .serve_at(
                "/org/freedesktop/portal/desktop",
                RemoteDesktopFixture {
                    events: events.clone(),
                    pipeline: pipeline.clone(),
                },
            )?
            .build()
            .await?;
        let client = zbus::connection::Builder::address(address.as_str())?.build().await?;
        let portal = Portal::for_input_test(client).await?;
        *h.portal.lock().await = Some(portal.clone());
        // Controle positivo: MESMO consumidor, perform, ashpd e endpoint privado.
        consume_text(h.clone(), &operation, "p", None).await?;
        let (received, reached) = oneshot::channel();
        let (resume, continued) = oneshot::channel();
        let pending_helper = h.clone();
        let pending_original = operation.clone();
        let pending = tokio::spawn(async move {
            consume_text(
                pending_helper,
                &pending_original,
                "q",
                Some(Cut {
                    received,
                    resume: continued,
                }),
            )
            .await
        });
        reached.await?;
        let params = json!({"session":"original","answerId":7});
        let prepared = prepare_rpc(&h, "close", &params, Some(9));
        let reply = respond(&h, "close", &params, json!(9), prepared).await;
        assert_eq!(
            reply["result"],
            json!({"nativeStop":{"answerId":7,"state":"null"}})
        );
        assert!(operation.session().is_err());
        assert_eq!(pipeline.current_state(), gst::State::Null);
        assert_eq!(other_pipeline.current_state(), gst::State::Playing);
        assert!(portal.is_active());
        resume
            .send(())
            .map_err(|()| anyhow!("consumidor não retido na barreira"))?;
        pending.await??;
        // Outra Operation e o portal continuam vivos: fechar A não mascara o sink.
        consume_text(h.clone(), &other, "r", None).await?;
        h.sessions.close_all(false)?;
        let observed = events
            .lock()
            .map_err(|_| anyhow!("observador indisponível"))?;
        let first = observed.first().context("controle positivo sem efeito")?;
        assert!(observed.iter().all(|e| e.session == first.session));
        for sym in [i32::from(b'p'), i32::from(b'r')] {
            let states: Vec<_> = observed
                .iter()
                .filter(|e| e.code == sym)
                .map(|e| e.state)
                .collect();
            assert_eq!(
                states,
                vec![1, 0],
                "controle positivo deve entregar press/release"
            );
        }
        let stale: Vec<_> = observed
            .iter()
            .filter(|e| e.code == i32::from(b'q'))
            .collect();
        assert!(
            stale
                .iter()
                .all(|e| e.original_pipeline == gst::State::Null)
        );
        assert_eq!(stale.len(), 0, "sink produtivo chamou NotifyKeyboardKeysym após close/Null da Operation original; endpoint é fixture, sem tecla real");
        Ok::<(), anyhow::Error>(())
    })
    .await?
}

#[tokio::test]
async fn drag_cut_inside_perform_releases_button_and_stops_after_close() -> Result<()> {
    tokio::time::timeout(Duration::from_secs(20), async {
        let h = helper();
        let (operation, pipeline) = original(&h, "dragging", 7).await?;
        let (_bus, address) = PrivateBus::start().await?;
        let events = Arc::new(Mutex::new(Vec::<Event>::new()));
        let _server = zbus::connection::Builder::address(address.as_str())?
            .name("org.freedesktop.portal.Desktop")?
            .serve_at(
                "/org/freedesktop/portal/desktop",
                RemoteDesktopFixture {
                    events: events.clone(),
                    pipeline: pipeline.clone(),
                },
            )?
            .build()
            .await?;
        let client = zbus::connection::Builder::address(address.as_str())?
            .build()
            .await?;
        let portal = Portal::for_input_test(client).await?;
        *h.portal.lock().await = Some(portal.clone());
        let display = h.portal().await?.display(Some(0))?;
        let view = View::for_input_test(display.clone())?;
        // Barreira DENTRO de perform: prende o arrasto entre passos do laço,
        // com o botão esquerdo já pressionado. Só o teste detém e retoma.
        let (signal, signaled) = oneshot::channel::<()>();
        let (release, resumed) = oneshot::channel::<()>();
        *input::DRAG_CUT.lock().expect("barreira do arrasto") = Some((signal, resumed));
        let drag_helper = h.clone();
        let drag_original = operation.clone();
        let consumer = tokio::spawn(async move {
            let _ = consume_event(
                drag_helper,
                &drag_original,
                display,
                view,
                json!({"type":"drag","x":10.0,"y":10.0,"toX":20.0,"toY":20.0}),
                None,
            )
            .await;
        });
        signaled.await?;
        // Close a meio do arrasto: o pipeline vai a Null com o botão seguro.
        let params = json!({"session":"dragging","answerId":7});
        let prepared = prepare_rpc(&h, "close", &params, Some(9));
        let reply = respond(&h, "close", &params, json!(9), prepared).await;
        assert_eq!(
            reply["result"],
            json!({"nativeStop":{"answerId":7,"state":"null"}})
        );
        assert!(operation.session().is_err());
        assert_eq!(pipeline.current_state(), gst::State::Null);
        assert!(portal.is_active());
        release
            .send(())
            .map_err(|()| anyhow!("arrasto não retido na barreira"))?;
        consumer.await?;
        let observed = events
            .lock()
            .map_err(|_| anyhow!("observador indisponível"))?;
        // Antes do close: um motion e o press do botão esquerdo. Depois do
        // close: nenhum motion; apenas a soltura de limpeza do botão seguro.
        let motions: Vec<_> = observed
            .iter()
            .filter(|e| e.kind == "motion")
            .map(|e| (e.x, e.y))
            .collect();
        assert_eq!(motions, vec![(10.0, 10.0)], "nenhum motion após o close");
        let buttons: Vec<_> = observed
            .iter()
            .filter(|e| e.kind == "button")
            .map(|e| (e.code, e.state))
            .collect();
        assert_eq!(
            buttons,
            vec![(0x110, 1), (0x110, 0)],
            "press antes do close; só a soltura de limpeza depois"
        );
        let release = observed
            .iter()
            .find(|e| e.kind == "button" && e.state == 0)
            .context("soltura de limpeza ausente")?;
        assert_eq!(release.original_pipeline, gst::State::Null);
        assert!(observed.iter().all(|e| e.kind != "keysym"));
        Ok::<(), anyhow::Error>(())
    })
    .await?
}

#[tokio::test]
async fn clipboard_is_denied_after_close_and_without_active_portal() -> Result<()> {
    tokio::time::timeout(Duration::from_secs(20), async {
        let h = helper();
        let (operation, _pipeline) = original(&h, "clipping", 7).await?;
        let (other, other_pipeline) = original(&h, "intact", 8).await?;
        let (_bus, address) = PrivateBus::start().await?;
        let events = Arc::new(Mutex::new(Vec::<Event>::new()));
        let _server = zbus::connection::Builder::address(address.as_str())?
            .name("org.freedesktop.portal.Desktop")?
            .serve_at(
                "/org/freedesktop/portal/desktop",
                RemoteDesktopFixture {
                    events: events.clone(),
                    pipeline: other_pipeline.clone(),
                },
            )?
            .build()
            .await?;
        let client = zbus::connection::Builder::address(address.as_str())?
            .build()
            .await?;
        let portal = Portal::for_input_test(client).await?;
        *h.portal.lock().await = Some(portal.clone());
        let display = h.portal().await?.display(Some(0))?;
        // Operation fechada com portal ativo: o efeito é negado e encerrar
        // descarta a mensagem, como nos eventos de entrada.
        let params = json!({"session":"clipping","answerId":7});
        let prepared = prepare_rpc(&h, "close", &params, Some(9));
        respond(&h, "close", &params, json!(9), prepared).await;
        assert!(operation.session().is_err());
        consume_event(
            h.clone(),
            &operation,
            display.clone(),
            View::for_input_test(display.clone())?,
            json!({"type":"clipboard","text":"negado"}),
            None,
        )
        .await?;
        // Portal revogado (watcher marca inativo; helper.portal() não entrega
        // mais sessão) com Operation aberta: efeito negado, consumidor segue.
        *h.portal.lock().await = None;
        consume_event(
            h.clone(),
            &other,
            display.clone(),
            View::for_input_test(display.clone())?,
            json!({"type":"clipboard","text":"negado"}),
            None,
        )
        .await?;
        // Nenhuma das negações tocou o portal de D-Bus.
        {
            let observed = events
                .lock()
                .map_err(|_| anyhow!("observador indisponível"))?;
            assert_eq!(
                observed.len(),
                0,
                "negação não consulta o portal de efeitos"
            );
        }
        // Controle positivo: com o portal de volta, a Operation intacta digita.
        *h.portal.lock().await = Some(portal.clone());
        consume_text(h.clone(), &other, "z", None).await?;
        let observed = events
            .lock()
            .map_err(|_| anyhow!("observador indisponível"))?;
        let states: Vec<_> = observed
            .iter()
            .filter(|e| e.kind == "keysym" && e.code == i32::from(b'z'))
            .map(|e| e.state)
            .collect();
        assert_eq!(states, vec![1, 0], "controle positivo press/release de z");
        Ok::<(), anyhow::Error>(())
    })
    .await?
}
