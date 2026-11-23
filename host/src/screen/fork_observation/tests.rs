//! Fixture de transporte, sem prova nativa de exclusão de superfície privada.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::{
    remote::identity::Identity,
    store::{BotConfig, Store},
};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct Fixture {
    hub: Arc<Hub>,
    dir: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("donna-observadores-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("state.db")).unwrap();
        let hub = Hub::new(store, "fixture".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
        let bot: BotConfig = serde_json::from_value(json!({"id":"bot", "name":"Bot", "computer":true})).unwrap();
        hub.store.save_bot(&bot).unwrap();
        crate::fork::tela_por_computador::call(
            &hub,
            &Caller::Local,
            &json!({
                "botId":"bot", "target":crate::fork::tela_por_computador::host_target(&hub), "enabled":true
            }),
        )
        .unwrap();
        *hub.screen.enabled.locked() = Some(true);
        Self { hub, dir }
    }
    fn access(&self, enabled: bool) {
        let body = json!({"botId":"bot", "target":crate::fork::tela_por_computador::host_target(&self.hub),
            "enabled":enabled});
        crate::fork::tela_por_computador::call(&self.hub, &Caller::Local, &body).unwrap();
    }
    fn attempt(&self) -> Arc<Attempt> {
        let execution = bindings::fixture_execution(&self.hub, "bot");
        Attempt::capture(&self.hub, &Caller::Local, "bot", &execution, true).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn d502_hold_antes_claim_check_waiting_e_type_login_sem_retomada() {
    use crate::fork::atividades::revisao::estado::fixture;
    let f = Fixture::new();
    let attempt = f.attempt();
    assert!(attempt.check_waiting().is_ok());
    assert!(attempt.verify().is_ok(), "Sempre válido, runtime/binding originais vivos");
    fixture::hold(&f.hub, "original");
    assert!(attempt.check_waiting().is_err(), "admitted=false também recusa");
    assert!(attempt.claim().is_err());
    assert!(f.hub.screen.control.locked().active_agent().is_none());
    for name in ["type_login", "get_desktop_state", "ui_tree"] {
        let error = super::super::computer::computer(&f.hub, "bot", name, json!({}), Some(&attempt)).await.unwrap_err();
        assert_eq!(error.to_string(), DENIED);
    }
    assert!(f.hub.screen.control.locked().active_agent().is_none());
    assert!(fixture::abort(&f.hub, "original"));
    assert!(attempt.verify().is_err(), "tentativa recusada não retoma nem adota grant");
    assert_eq!(f.hub.runtime("bot").memory_turn_token.as_deref(), Some("fixture-turn"));
}

#[tokio::test]
#[cfg(unix)]
async fn d502_prefixo_parcial_sem_continuacao_ou_retry() {
    use crate::fork::atividades::revisao::estado::fixture;
    use tokio::io::AsyncReadExt;
    let f = Fixture::new();
    let attempt = f.attempt();
    let (mut host, mut peer) = tokio::net::UnixStream::pair().unwrap();
    host.writable().await.unwrap();
    let frame = format!("{{\"method\":\"screenshot\",\"padding\":\"{}\"}}\n", "x".repeat(16 * 1024 * 1024));
    {
        let writing = write(&mut host, frame.as_bytes(), &attempt, None);
        tokio::pin!(writing);
        assert!(futures::poll!(&mut writing).is_pending());
        fixture::hold(&f.hub, "original");
        assert!(writing.await.unwrap_err().sent);
    }
    assert!(fixture::abort(&f.hub, "original"));
    assert!(write(&mut host, frame.as_bytes(), &attempt, None).await.is_err());
    host.shutdown().await.unwrap();
    let mut received = Vec::new();
    peer.read_to_end(&mut received).await.unwrap();
    assert!(!received.is_empty() && received.len() < frame.len());
    assert!(!received.ends_with(b"\n"));
}

#[tokio::test]
#[cfg(unix)]
async fn writer_revogado_antes_de_enviar_nao_admite_frame_no_peer() {
    for change in ["permission_aba", "screen_access_off", "screen_access_aba", "turn", "binding", "d502_hold"] {
        let f = Fixture::new();
        let attempt = f.attempt();
        // O quadro fica pronto antes de mudar a fonte; só depois entra no writer real.
        let (mut host, peer) = tokio::net::UnixStream::pair().unwrap();
        match change {
            "d502_hold" => crate::fork::atividades::revisao::estado::fixture::hold(&f.hub, "original"),
            "screen_access_off" | "screen_access_aba" => {
                for enabled in [false, true] {
                    if enabled && change == "screen_access_off" {
                        break;
                    }
                    crate::fork::tela_por_computador::call(&f.hub, &Caller::Local, &json!({
                        "botId":"bot", "target":crate::fork::tela_por_computador::host_target(&f.hub), "enabled":enabled
                    })).unwrap();
                }
            }
            "permission_aba" => {
                let mut bot = f.hub.store.bot("bot").unwrap().unwrap().config;
                bot.computer = false;
                f.hub.store.save_bot(&bot).unwrap();

                bot.computer = true;
                f.hub.store.save_bot(&bot).unwrap();
            }
            "turn" => f.hub.set_runtime("bot", |r| r.memory_turn_token = Some("successor".into())),
            _ => bindings::revoke(&f.hub.store, "bot", None).unwrap(),
        }
        let error = write(&mut host, b"{\"method\":\"screenshot\"}\n", &attempt, None).await.unwrap_err();
        assert!(!error.sent, "nenhum byte foi escrito");
        host.shutdown().await.unwrap();
        let mut frame_text = String::new();
        assert_eq!(BufReader::new(peer).read_line(&mut frame_text).await.unwrap(), 0, "zero admissão no peer");
        assert!(attempt.with_send(None, || ()).is_err());
        if change == "turn" {
            assert_eq!(f.hub.runtime("bot").memory_turn_token.as_deref(), Some("successor"));
        }
    }
}

#[tokio::test]
#[cfg(unix)]
async fn transporte_comum_admite_e_retorno_antigo_nao_projete_nem_altere_sucessor() {
    let f = Fixture::new();
    let attempt = f.attempt();
    let (mut host, peer) = tokio::net::UnixStream::pair().unwrap();
    write(&mut host, b"{\"method\":\"uiTree\"}\n", &attempt, None).await.unwrap();
    let mut frame_text = String::new();
    assert!(BufReader::new(peer).read_line(&mut frame_text).await.unwrap() > 0);
    assert_eq!(serde_json::from_str::<serde_json::Value>(&frame_text).unwrap()["method"], "uiTree");
    f.hub.set_runtime("bot", |r| r.memory_turn_token = Some("successor".into()));
    let mut projected = false;
    assert!(attempt.with_send(None, || projected = true).is_err());
    assert!(!projected);
    assert_eq!(f.hub.runtime("bot").memory_turn_token.as_deref(), Some("successor"));
}

#[cfg(unix)]
fn attach_helper(
    f: &Fixture,
) -> (Arc<super::super::helper::Link>, tokio::sync::mpsc::UnboundedReceiver<super::super::helper::Outgoing>) {
    use std::sync::{Mutex, atomic::AtomicI64};
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let link =
        Arc::new(super::super::helper::Link { id: 7, tx, pending: Mutex::default(), next_id: AtomicI64::new(1) });
    *f.hub.screen.link.locked() = Some(link.clone());
    (link, rx)
}

#[tokio::test]
#[cfg(unix)]
async fn fila_real_helper_recusa_origem_revogada_e_preserva_conexao_sucessora() {
    let f = Fixture::new();
    let (link, queue) = attach_helper(&f);
    let attempt = f.attempt();
    let request = link.request_observed("screenshot", json!({}), &attempt);
    tokio::pin!(request);
    assert!(futures::poll!(&mut request).is_pending(), "pedido enfileirado sem writer");
    assert_eq!(link.pending.locked().len(), 1);
    let (replacement, _replacement_queue) = attach_helper(&f);
    assert!(!Arc::ptr_eq(&link, &replacement), "o mesmo id não é a mesma conexão");
    let (host, peer) = tokio::net::UnixStream::pair().unwrap();
    let writer = tokio::spawn(helper_writer(host, queue, link.clone()));
    assert!(request.await.is_err());
    assert!(link.pending.locked().is_empty());
    assert!(Arc::ptr_eq(f.hub.screen.link.locked().as_ref().unwrap(), &replacement));
    writer.abort();
    let _ = writer.await;
    let mut frame_text = String::new();
    assert_eq!(BufReader::new(peer).read_line(&mut frame_text).await.unwrap(), 0);
}

#[tokio::test]
#[cfg(unix)]
async fn unixstream_pending_real_reconfere_origem_apos_envio_parcial() {
    use tokio::io::AsyncReadExt;
    for successor in [false, true] {
        let f = Fixture::new();
        let attempt = f.attempt();
        let (mut host, mut peer) = tokio::net::UnixStream::pair().unwrap();
        host.writable().await.unwrap();
        // O peer não lê até o corte: o buffer do socket real acaba no meio do frame.
        let frame = format!("{{\"method\":\"screenshot\",\"padding\":\"{}\"}}\n", "x".repeat(16 * 1024 * 1024));
        {
            let writing = write(&mut host, frame.as_bytes(), &attempt, None);
            tokio::pin!(writing);
            assert!(futures::poll!(&mut writing).is_pending());
            if successor {
                f.hub.set_runtime("bot", |r| r.memory_turn_token = Some("successor".into()));
            } else {
                f.access(false);
                f.access(true);
            }
            assert!(writing.await.unwrap_err().sent, "envio parcial é incerto, nunca zero observação");
        }
        host.shutdown().await.unwrap();
        let mut received = Vec::new();
        peer.read_to_end(&mut received).await.unwrap();
        assert!(!received.is_empty() && received.len() < frame.len());
        assert!(!received.ends_with(b"\n"));
        assert_eq!(
            f.hub.runtime("bot").memory_turn_token.as_deref(),
            Some(if successor { "successor" } else { "fixture-turn" })
        );
    }
}

#[tokio::test]
#[cfg(unix)]
async fn unixstream_pending_sem_bytes_nao_admite_pedido_apos_revogacao() {
    use tokio::io::AsyncReadExt;
    let f = Fixture::new();
    let attempt = f.attempt();
    let (mut host, mut peer) = tokio::net::UnixStream::pair().unwrap();
    host.writable().await.unwrap();
    let filler = vec![b' '; 64 * 1024];
    let mut occupied = 0;
    loop {
        match host.try_write(&filler) {
            Ok(0) => panic!("socket fechado"),
            Ok(n) => occupied += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) => panic!("{e}"),
        }
    }
    {
        let writing = write(&mut host, b"{\"method\":\"screenshot\"}\n", &attempt, None);
        tokio::pin!(writing);
        assert!(futures::poll!(&mut writing).is_pending());
        bindings::revoke(&f.hub.store, "bot", None).unwrap();
        assert!(!writing.await.unwrap_err().sent);
    }
    host.shutdown().await.unwrap();
    let mut received = Vec::new();
    peer.read_to_end(&mut received).await.unwrap();
    assert_eq!(received.len(), occupied);
    assert!(received.iter().all(|b| *b == b' '), "nenhum byte do pedido novo no peer");
}

#[tokio::test]
#[cfg(unix)]
async fn cua_exchange_real_recusa_retorno_revogado_antes_de_projetar() {
    for change in ["grant", "successor", "d502_hold"] {
        let successor = change == "successor";
        let f = Fixture::new();
        let attempt = f.attempt();
        let (host, peer) = tokio::net::UnixStream::pair().unwrap();
        let (admitted_tx, admitted_rx) = tokio::sync::oneshot::channel();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let driver = tokio::spawn(async move {
            let mut peer = BufReader::new(peer);
            let mut frame_text = String::new();
            peer.read_line(&mut frame_text).await.unwrap();
            let frame: serde_json::Value = serde_json::from_str(&frame_text).unwrap();
            assert_eq!(frame["name"], "get_desktop_state");
            admitted_tx.send(()).unwrap();
            reply_rx.await.unwrap();
            peer.get_mut()
                .write_all(b"{\"ok\":true,\"result\":{\"structuredContent\":{\"private\":\"SENTINELA\"}}}\n")
                .await
                .unwrap();
        });
        let body = json!({"method":"call", "name":"get_desktop_state"});
        let exchange = super::super::cua::exchange_inner(host, &body, Some(&attempt));
        tokio::pin!(exchange);
        tokio::select! {
            result = &mut exchange => panic!("retorno antes da barreira: {result:?}"),
            result = admitted_rx => result.unwrap(),
        }
        if successor {
            f.hub.set_runtime("bot", |r| r.memory_turn_token = Some("successor".into()));
        } else if change == "grant" {
            f.access(false);
            f.access(true);
        }
        if change == "d502_hold" {
            crate::fork::atividades::revisao::estado::fixture::hold(&f.hub, "original");
        }
        reply_tx.send(()).unwrap();
        let error = exchange.await.unwrap_err().to_string();
        assert!(!error.contains("SENTINELA"));
        assert_eq!(
            f.hub.runtime("bot").memory_turn_token.as_deref(),
            Some(if successor { "successor" } else { "fixture-turn" })
        );
        driver.await.unwrap();
    }
}

#[tokio::test]
#[cfg(unix)]
async fn authority_original_revogada_nao_adota_nova_concessao_no_writer() {
    use crate::store::{Device, DeviceSource, Scope, now_ms};
    let f = Fixture::new();
    let device = Device {
        key: "fixture-device".into(),
        name: "Fixture".into(),
        platform: "fixture".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: now_ms(),
        last_seen_at: None,
    };
    f.hub.store.put_device(&device).unwrap();
    let (_, caller) = autoridade::autenticar(&f.hub, &device.key).unwrap();
    let execution = bindings::fixture_execution(&f.hub, "bot");
    let attempt = Attempt::capture(&f.hub, &caller, "bot", &execution, true).unwrap();
    assert!(f.hub.revoke_device(&device.key).unwrap());
    // Reconcessão de aparelho Local = o passo do pareamento por código (`readmitir_pareado`), que libera o bloqueio e grava o aparelho numa Tx.
    crate::fork::workspace::readmitir_pareado(&f.hub.store, &device).unwrap();
    let (mut host, peer) = tokio::net::UnixStream::pair().unwrap();
    assert!(!write(&mut host, b"{\"method\":\"screenshot\"}\n", &attempt, None).await.unwrap_err().sent);
    host.shutdown().await.unwrap();
    let mut frame_text = String::new();
    assert_eq!(BufReader::new(peer).read_line(&mut frame_text).await.unwrap(), 0);
    let (_, caller) = autoridade::autenticar(&f.hub, &device.key).unwrap();
    assert!(Attempt::capture(&f.hub, &caller, "bot", &execution, true).is_ok(), "controle novo usa fonte nova");
}

#[tokio::test]
#[cfg(unix)]
async fn fila_helper_waiter_cancelado_nao_envia_nem_remove_outro_job() {
    let f = Fixture::new();
    let (link, queue) = attach_helper(&f);
    let attempt = f.attempt();
    {
        let request = link.request_observed("screenshot", json!({}), &attempt);
        tokio::pin!(request);
        assert!(futures::poll!(&mut request).is_pending());
    }
    assert!(link.pending.locked().is_empty(), "drop retira apenas waiter original");
    let (successor, _successor_result) = tokio::sync::oneshot::channel();
    link.pending.locked().insert(99, successor);
    let (host, peer) = tokio::net::UnixStream::pair().unwrap();
    let writer = tokio::spawn(helper_writer(host, queue, link.clone()));
    // Um quadro administrativo posterior funciona como barreira da fila exercitada.
    let control = link.request("ping", json!({}));
    tokio::pin!(control);
    assert!(futures::poll!(&mut control).is_pending());
    let mut peer = BufReader::new(peer);
    let mut frame_text = String::new();
    peer.read_line(&mut frame_text).await.unwrap();
    let frame: serde_json::Value = serde_json::from_str(&frame_text).unwrap();
    assert_eq!(frame["method"], "ping", "pedido cancelado não precede o controle no peer");
    assert!(link.pending.locked().contains_key(&99), "job alheio intacto");
    writer.abort();
    let _ = writer.await;
}

// Ingresso usado por mcp: provisionamento do ambiente e autenticação HTTP da Ponte.
fn mcp_caller(f: &Fixture) -> (Caller, serde_json::Value) {
    use crate::fork::ponte_credencial;
    let binding = bindings::fixture_binding(&f.hub.store, "bot");
    bindings::fixture_execution(&f.hub, "bot");
    let mut servers = vec![json!({"env":[]})];
    ponte_credencial::provisionar(&f.hub, "bot", None, &binding, &mut servers).unwrap();
    let token = servers[0]["env"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == ponte_credencial::ENV)
        .unwrap()["value"]
        .as_str()
        .unwrap();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    let caller = ponte_credencial::autenticar(&f.hub, &headers, "127.0.0.1:12345".parse().unwrap()).unwrap();
    assert!(matches!(&caller, Caller::Ponte { .. }));
    assert!(Autoridade::capturar(&f.hub, &caller).is_err(), "Ponte não ganha controle genérico");
    (caller, json!({"botId":"bot", "sessionBinding":binding.id.as_str(), "name":"screenshot", "arguments":{}}))
}

#[tokio::test]
#[cfg(unix)]
async fn ponte_autenticada_dispatch_computer_chega_ao_peer_helper_real() {
    let f = Fixture::new();
    let (caller, body) = mcp_caller(&f);
    let (host, peer) = tokio::net::UnixStream::pair().unwrap();
    let mut events = f.hub.events.subscribe();
    let connection = tokio::spawn(super::super::helper::run_link(f.hub.screen.clone(), host));
    let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
    let driver = tokio::spawn(async move {
        let mut peer = BufReader::new(peer);
        let status = json!({"jsonrpc":"2.0","method":"status","params":{"platform":"fixture","capture":true,"displays":[super::super::tests::retina()]}});
        peer.get_mut().write_all(format!("{status}\n").as_bytes()).await.unwrap();
        let mut frame_text = String::new();
        peer.read_line(&mut frame_text).await.unwrap();
        let frame: serde_json::Value = serde_json::from_str(&frame_text).unwrap();
        assert_eq!(frame["method"], "screenshot");
        let reply = json!({"jsonrpc":"2.0","id":frame["id"],"result":{"data":"JPEG_FIXTURE"}});
        peer.get_mut().write_all(format!("{reply}\n").as_bytes()).await.unwrap();
        // Mantém a conexão original viva até a projeção completar.
        finish_rx.await.unwrap();
    });
    assert_eq!(events.recv().await.unwrap()["type"], "screen");
    let result = crate::api::dispatch(&f.hub, &caller, "computerCall", body).await.unwrap();
    assert_eq!(result["content"][0]["data"], "JPEG_FIXTURE");
    finish_tx.send(()).unwrap();
    driver.await.unwrap();
    connection.await.unwrap();
}

#[tokio::test]
#[cfg(unix)]
async fn ponte_original_na_fila_dispatch_nao_adota_revogacao_ou_substituta() {
    for change in ["revogar", "substituir"] {
        let f = Fixture::new();
        let (link, queue) = attach_helper(&f);
        f.hub.screen.status.locked().displays = vec![super::super::tests::retina()];
        let (caller, body) = mcp_caller(&f);
        let request = crate::api::dispatch(&f.hub, &caller, "computerCall", body.clone());
        tokio::pin!(request);
        assert!(futures::poll!(&mut request).is_pending(), "ingresso real deve chegar à fila do helper");
        assert_eq!(link.pending.locked().len(), 1);
        let successor = if change == "substituir" {
            Some(mcp_caller(&f).0)
        } else {
            f.hub.pontes.revogar_bot("bot");
            None
        };
        let (host, peer) = tokio::net::UnixStream::pair().unwrap();
        let writer = tokio::spawn(helper_writer(host, queue, link.clone()));
        assert!(request.await.is_err());
        writer.abort();
        let _ = writer.await;
        let mut frame_text = String::new();
        assert_eq!(BufReader::new(peer).read_line(&mut frame_text).await.unwrap(), 0, "zero envio antigo no peer");
        if let Some(successor) = successor {
            assert!(crate::fork::ponte_credencial::conferir(&f.hub, &successor, "computerCall", &body).is_ok());
        }
    }
}

#[tokio::test]
#[cfg(unix)]
async fn ponte_dispatch_retorno_antigo_nao_projeta_nem_revoga_credencial_sucessora() {
    let f = Fixture::new();
    let (caller, body) = mcp_caller(&f);
    let (host, peer) = tokio::net::UnixStream::pair().unwrap();
    let mut events = f.hub.events.subscribe();
    let connection = tokio::spawn(super::super::helper::run_link(f.hub.screen.clone(), host));
    let (admitted_tx, admitted_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
    let driver = tokio::spawn(async move {
        let mut peer = BufReader::new(peer);
        let status = json!({"jsonrpc":"2.0","method":"status","params":{"platform":"fixture","capture":true,"displays":[super::super::tests::retina()]}});
        peer.get_mut().write_all(format!("{status}\n").as_bytes()).await.unwrap();
        let mut frame_text = String::new();
        peer.read_line(&mut frame_text).await.unwrap();
        let frame: serde_json::Value = serde_json::from_str(&frame_text).unwrap();
        assert_eq!(frame["method"], "screenshot");
        admitted_tx.send(()).unwrap();
        release_rx.await.unwrap();
        let reply = json!({"jsonrpc":"2.0","id":frame["id"],"result":{"data":"SENTINELA_ANTIGA"}});
        peer.get_mut().write_all(format!("{reply}\n").as_bytes()).await.unwrap();
        finish_rx.await.unwrap();
    });
    events.recv().await.unwrap();
    let request = crate::api::dispatch(&f.hub, &caller, "computerCall", body.clone());
    tokio::pin!(request);
    tokio::select! {
        result = &mut request => panic!("retorno antes da barreira: {result:?}"),
        result = admitted_rx => result.unwrap(),
    }
    let (successor, _) = mcp_caller(&f);
    release_tx.send(()).unwrap();
    let error = request.await.unwrap_err().to_string();
    assert!(!error.contains("SENTINELA_ANTIGA"));
    assert!(crate::fork::ponte_credencial::conferir(&f.hub, &successor, "computerCall", &body).is_ok());
    assert_eq!(f.hub.runtime("bot").memory_turn_token.as_deref(), Some("fixture-turn"));
    finish_tx.send(()).unwrap();
    driver.await.unwrap();
    connection.await.unwrap();
}

#[test]
#[cfg(unix)]
fn envio_real_progride_no_corte_de_attach_e_detach_sem_adotar_sucessor() {
    use crate::screen::{ViewerState, fork_viewer, viewer::IceConfig};
    use std::sync::atomic::{AtomicBool, Ordering};
    for detach in [false, true] {
        let f = Fixture::new();
        let screen = &f.hub.screen;
        let (original_link, _original_queue) = attach_helper(&f);
        let prepared = screen.prepare("local", IceConfig::default()).unwrap();
        let id = prepared["session"].as_str().unwrap();
        let original_row = screen.sessions.locked().rows.get(id).unwrap().clone();
        screen.takeover(true);
        let original = f.attempt();
        let (successor_link, _successor_queue) = attach_helper(&f);
        *screen.link.locked() = Some(original_link.clone());
        let progressed = Arc::new(AtomicBool::new(false));
        let observed_progress = progressed.clone();
        let original_id = original_link.id;
        fork_viewer::on_link_retired_for_test(move || {
            // attach/detach só continua depois do with_send REAL nessa outra thread.
            // Com link ainda retido, esta junção bloqueia no ciclo real de travas.
            std::thread::spawn(move || {
                let mut sent = false;
                assert!(original.with_send(Some(original_id), || sent = true).is_err());
                assert!(!sent, "a origem antiga não ganha a conexão sucessora");
                observed_progress.store(true, Ordering::SeqCst);
            })
            .join()
            .unwrap();
        });
        if detach {
            screen.detach_viewer_link(&original_link);
            assert!(screen.link.locked().is_none());
        } else {
            screen.attach_viewer_link(successor_link.clone());
            assert!(Arc::ptr_eq(screen.link.locked().as_ref().unwrap(), &successor_link));
        }
        assert!(progressed.load(Ordering::SeqCst));
        assert!(Arc::ptr_eq(screen.sessions.locked().rows.get(id).unwrap(), &original_row));
        assert!(original_row.locked().state == ViewerState::Uncertain);
        assert!(Arc::ptr_eq(&original_row.locked().link, &original_link));
        assert_eq!(screen.state()["userControl"], false);
        if detach {
            screen.attach_viewer_link(successor_link.clone());
        }
        let prepared = screen.prepare("local", IceConfig::default()).unwrap();
        let successor_id = prepared["session"].as_str().unwrap();
        let successor_row = screen.sessions.locked().rows.get(successor_id).unwrap().clone();
        screen.takeover(true);
        let successor = f.attempt();
        let mut sent = false;
        successor.with_send(Some(successor_link.id), || sent = true).unwrap();
        assert!(sent, "controle positivo real usa sua própria origem");
        screen.detach_viewer_link(&original_link);
        assert!(Arc::ptr_eq(screen.link.locked().as_ref().unwrap(), &successor_link));
        assert!(Arc::ptr_eq(screen.sessions.locked().rows.get(successor_id).unwrap(), &successor_row));
        assert!(successor_row.locked().state == ViewerState::Prepared);
        assert_eq!(screen.state()["userControl"], true);
        assert!(successor.with_send(Some(successor_link.id), || ()).is_ok());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn d511_desligado_pede_uma_vez_sem_captura_ax_cua_type_login_ou_claim() {
    use crate::store::{Device, DeviceSource, Scope, now_ms};

    let f = Fixture::new();
    let (caller, mut body) = mcp_caller(&f);
    let (link, mut queue) = attach_helper(&f);
    crate::fork::tela_por_computador::call(
        &f.hub,
        &Caller::Local,
        &json!({
            "botId":"bot", "target":crate::fork::tela_por_computador::host_target(&f.hub), "enabled":false
        }),
    )
    .unwrap();
    let grant = json!({"botId":"bot", "target":crate::fork::tela_por_computador::host_target(&f.hub),
        "enabled":true});
    assert!(crate::api::dispatch(&f.hub, &caller, crate::fork::tela_por_computador::METHOD, grant).await.is_err());
    for name in ["screenshot", "ui_tree", "click", "type", "type_text", "type_login"] {
        body["name"] = json!(name);
        body["arguments"] = if name == "type_login" {
            json!({"login":"fixture","field":"password"})
        } else {
            json!({"x":1,"y":1,"text":"fixture"})
        };
        let waiting = tokio::time::timeout(
            std::time::Duration::from_millis(20),
            crate::api::dispatch(&f.hub, &caller, "computerCall", body.clone()),
        )
        .await;
        assert!(waiting.is_err(), "D511 aguarda o dono sem executar a ferramenta");
        assert!(queue.try_recv().is_err(), "zero envio ao helper");
        assert!(link.pending.locked().is_empty());
        assert!(f.hub.screen.control.locked().active_agent().is_none(), "sem claim");
    }
    let pending: i64 = f.hub.store.db.locked().query_row(
        "SELECT COUNT(*) FROM entries WHERE kind='permission' AND json_extract(data,'$.toolKind')='screen_access' AND json_extract(data,'$.status')='pending'",[],|r| r.get(0)).unwrap();
    assert_eq!(pending, 1, "seis ferramentas reaproveitam o único pedido");
    let id: String = f
        .hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT id FROM entries WHERE kind='permission' AND json_extract(data,'$.toolKind')='screen_access'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let card = f.hub.store.entry(&id).unwrap();
    let response = json!({"botId":card.bot_id,"entryId":card.id,"entryRev":card.rev,"optionId":"always"});
    assert!(
        crate::api::dispatch(&f.hub, &caller, "respondPermission", response).await.is_err(),
        "a Ponte não responde seu próprio cartão"
    );
    assert_eq!(f.hub.store.entry(&card.id).unwrap().rev, card.rev);
    f.hub
        .store
        .put_device(&Device {
            key: "deny-owner".into(),
            name: "Teste".into(),
            platform: "fixture".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: now_ms(),
            last_seen_at: None,
        })
        .unwrap();
    let (_, owner) = autoridade::autenticar(&f.hub, "deny-owner").unwrap();
    let h = f.hub.clone();
    *f.hub.store.before_owner_write.locked() = Some(Box::new(move || {
        h.store.db.locked().execute("UPDATE bots SET deleted=1 WHERE id='bot'", []).unwrap();
    }));
    crate::api::dispatch(
        &f.hub,
        &owner,
        "respondPermission",
        json!({"botId":"bot","entryId":card.id,"entryRev":card.rev,"optionId":"deny"}),
    )
    .await
    .unwrap();
    assert_eq!(f.hub.store.entry(&card.id).unwrap().data["status"], "answered");
    let denials: i64 = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM donna_screen_access_audit WHERE choice='deny'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(denials, 1, "a recusa do cartão sem ator também é auditada");
}

#[tokio::test]
async fn d510_off_on_nao_projete_retorno_antigo_mas_nova_tentativa_funciona() {
    let f = Fixture::new();
    let old = f.attempt();
    for enabled in [false, true] {
        crate::fork::tela_por_computador::call(
            &f.hub,
            &Caller::Local,
            &json!({
                "botId":"bot", "target":crate::fork::tela_por_computador::host_target(&f.hub), "enabled":enabled
            }),
        )
        .unwrap();
    }
    let mut delivered = false;
    assert!(old.with_send(None, || delivered = true).is_err());
    assert!(!delivered);
    // Usa a mesma execução viva: só uma captura nova pode observar a chave nova.
    let current = Attempt::capture(&f.hub, &Caller::Local, "bot", &old.execution, true).unwrap();
    assert!(current.verify().is_ok());
}
