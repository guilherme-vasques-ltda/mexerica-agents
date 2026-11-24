use super::*;
use crate::{
    LockExt,
    api::{devices::Caller, dispatch},
    hub::Hub,
    remote::identity::Identity,
    screen::helper::{Link, Outgoing},
    store::{Scope, Store},
};
use axum::http::HeaderMap;
use serde_json::json;
use std::{
    path::Path,
    sync::{Arc, Mutex, atomic::AtomicI64},
    time::Duration,
};
use tokio::sync::mpsc;

fn link(id: u64) -> (Arc<Link>, mpsc::UnboundedReceiver<Outgoing>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        Arc::new(Link {
            id,
            tx,
            pending: Mutex::default(),
            next_id: AtomicI64::new(1),
            restore_notice: Mutex::default(),
        }),
        rx,
    )
}

async fn portal_request(
    hub: &Arc<Hub>,
    link: &Link,
    rx: &mut mpsc::UnboundedReceiver<Outgoing>,
    denied: bool,
) -> Result<Value, String> {
    let call = dispatch(hub, &Caller::Local, "requestScreenPermission", json!({"permission":"portal"}));
    let reply = async {
        let frame = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("permission frame deadline")
            .expect("permission frame");
        let message: Value = serde_json::from_str(&frame.line).expect("permission JSON");
        assert_eq!(message["method"], "requestPermission");
        assert_eq!(message["params"], json!({"permission":"portal"}));
        let waiter = link.pending.locked().remove(&frame.id).expect("permission waiter");
        let result = if denied { Err("fixture denied".into()) } else { Ok(json!({})) };
        waiter.send(result).expect("permission reply");
    };
    let (result, ()) = tokio::join!(call, reply);
    result.map_err(|error| error.to_string())
}

#[tokio::test]
async fn aviso_portal_so_na_leitura_local_sem_mudar_permissao_ou_stream() {
    let dir = tempfile::tempdir().expect("fixture directory");
    let store = Store::open(Path::new(":memory:")).expect("fixture store");
    store.kv_set(super::super::KV_ENABLED, "1").expect("fixture setting");
    let hub = Hub::new(
        store,
        "fixture-hub".into(),
        Identity::load_or_create(dir.path()).expect("fixture identity"),
        "fixture-bearer".into(),
        0,
    );
    let (current, mut rx) = link(1);
    hub.screen.attach_viewer_link(current.clone());
    let device = Caller::Device { key: "fixture-device".into(), scopes: vec![Scope::Control], autoridade: None };
    let status = json!({"platform":"linux","capture":true,"input":true});
    current.accept_status(&hub.screen, &status);
    let mut events = hub.screen.events.subscribe();
    let fake_local = json!({"caller":"local","portalRestoreNotice":"stateDirectoryNotPrivate"});
    let remote = dispatch(&hub, &device, "screenStatus", fake_local.clone()).await.expect("remote status");
    let remote = serde_json::to_vec(&remote).expect("remote bytes");
    let granted = portal_request(&hub, &current, &mut rx, false).await.expect("granted without notice");
    let denied = portal_request(&hub, &current, &mut rx, true).await.expect_err("denied without notice");
    let mut with_notice = status.clone();
    with_notice["portalRestoreNotice"] = json!("stateDirectoryNotPrivate");
    current.accept_status(&hub.screen, &with_notice);
    assert!(matches!(events.try_recv(), Err(tokio::sync::broadcast::error::TryRecvError::Empty)));
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer fixture-bearer".parse().expect("fixture header"));
    for peer in ["127.0.0.1:12345", "127.0.0.1:23456"] {
        let peer = peer.parse().expect("fixture peer");
        let caller = crate::fork::ponte_credencial::autenticar_com_origin(&hub, &headers, peer, true)
            .expect("same authenticated ingress for desktop and forwarded loopback");
        let local = dispatch(&hub, &caller, "screenStatus", json!({})).await.expect("local status");
        assert_eq!(local["portalRestoreNotice"], "stateDirectoryNotPrivate");
    }
    let web = Arc::new(crate::fork::web_local::WebState::new(dir.path().to_owned(), 23001));
    let mut headers = web.fixture_session_headers();
    hub.set_web_local(web).expect("fixture web state");
    let peer = "127.0.0.1:34567".parse().expect("fixture web peer");
    let caller = crate::fork::ponte_credencial::autenticar_com_origin(&hub, &headers, peer, true)
        .expect("authenticated fixture web session");
    let local = dispatch(&hub, &caller, "screenStatus", json!({})).await.expect("web status");
    assert_eq!(local["portalRestoreNotice"], "stateDirectoryNotPrivate");
    headers.remove(crate::fork::web_local::CSRF_HEADER);
    assert!(crate::fork::ponte_credencial::autenticar_com_origin(&hub, &headers, peer, true).is_none());
    let result = dispatch(&hub, &device, "screenStatus", fake_local).await.expect("remote status");
    assert_eq!(serde_json::to_vec(&result).expect("remote bytes"), remote);
    assert!(
        dispatch(&hub, &device, "requestScreenPermission", json!({"caller":"local","permission":"portal"}))
            .await
            .is_err()
    );
    let hello = dispatch(&hub, &Caller::Local, "hello", json!({})).await.expect("hello");
    assert!(hello["screen"].get("portalRestoreNotice").is_none());
    assert_eq!(portal_request(&hub, &current, &mut rx, false).await.expect("granted with notice"), granted);
    assert_eq!(portal_request(&hub, &current, &mut rx, true).await.expect_err("denied with notice"), denied);
    assert!(granted.get("portalRestoreNotice").is_none());
    for unknown in [Value::Null, json!("fixture-unknown"), json!({"arbitrary":true})] {
        with_notice["portalRestoreNotice"] = json!("stateDirectoryNotPrivate");
        current.accept_status(&hub.screen, &with_notice);
        with_notice["portalRestoreNotice"] = unknown;
        current.accept_status(&hub.screen, &with_notice);
        assert!(hub.screen.portal_restore_notice().is_none());
        assert_eq!(hub.screen.state()["capture"], true);
    }
    with_notice["portalRestoreNotice"] = json!("stateDirectoryNotPrivate");
    current.accept_status(&hub.screen, &with_notice);
    let (successor, _) = link(2);
    hub.screen.attach_viewer_link(successor);
    current.accept_status(&hub.screen, &with_notice);
    assert!(hub.screen.portal_restore_notice().is_none());
}

#[test]
fn aviso_portal_nao_altera_cadencia_ou_bytes_dos_eventos_de_tela() {
    let base = json!({"platform":"linux","capture":true,"input":true});
    let next = json!({"platform":"linux","capture":true,"input":false});
    let public_sequence = [base.clone(), base.clone(), base.clone(), next.clone(), next, base];
    let collect_frames = |with_notice| {
        let (tx, mut events) = tokio::sync::broadcast::channel(8);
        let screen = crate::screen::Screen::new(Some(true), tx);
        let (link, _) = link(1);
        screen.attach_viewer_link(link.clone());
        public_sequence
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, mut frame)| {
                if with_notice && matches!(index, 1..=3) {
                    frame["portalRestoreNotice"] = json!("stateDirectoryNotPrivate");
                } else if with_notice && index == 4 {
                    frame["portalRestoreNotice"] = json!("unknown-fixture");
                }
                link.accept_status(&screen, &frame);
                assert_eq!(screen.portal_restore_notice().is_some(), with_notice && matches!(index, 1..=3));
                match events.try_recv() {
                    Ok(event) => {
                        assert!(event["screen"].get("portalRestoreNotice").is_none());
                        assert!(matches!(events.try_recv(), Err(tokio::sync::broadcast::error::TryRecvError::Empty)));
                        Some(serde_json::to_vec(&event).expect("event bytes"))
                    }
                    Err(tokio::sync::broadcast::error::TryRecvError::Empty) => None,
                    Err(error) => panic!("unexpected event error: {error}"),
                }
            })
            .collect::<Vec<_>>()
    };
    let without_notice = collect_frames(false);
    assert_eq!(without_notice.iter().map(Option::is_some).collect::<Vec<_>>(), [true, false, false, true, false, true]);
    assert_eq!(collect_frames(true), without_notice);
}

#[test]
fn aviso_portal_nao_reaparece_apos_corte_de_close_all() {
    let mut notice = LinkNotice::default();
    let status = json!({"platform":"linux","portalRestoreNotice":"stateDirectoryNotPrivate"});
    notice.observe(&status);
    assert!(notice.current().is_some());
    notice.suspend(Some(42));
    notice.observe(&status);
    notice.close_reply(41, &json!({"result":{}}));
    notice.close_reply(42, &json!({"error":{}}));
    assert!(notice.current().is_none());
    notice.close_reply(42, &json!({"result":{}}));
    notice.observe(&status);
    assert!(notice.current().is_none());
    notice.suspend(Some(43));
    notice.close_reply(43, &json!({"result":{}}));
    assert!(notice.current().is_none());
    notice.observe(&status);
    assert!(notice.current().is_some());
}
