//! Protocolo/identidade no peer real; resposta {} não comprova término nativo.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::screen::{helper, tests::retina, viewer::IceConfig};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::broadcast,
};

async fn connect(screen: &Arc<Screen>) -> (BufReader<UnixStream>, tokio::task::JoinHandle<()>) {
    let mut events = screen.events.subscribe();
    let (host, mut peer) = UnixStream::pair().unwrap();
    let task = tokio::spawn(helper::run_link(screen.clone(), host));
    let status =
        json!({"jsonrpc":"2.0","method":"status","params":{"platform":"fixture","capture":true,"displays":[retina()]}});
    peer.write_all(format!("{status}\n").as_bytes()).await.unwrap();
    events.recv().await.unwrap();
    (BufReader::new(peer), task)
}
fn screen() -> Arc<Screen> {
    let (events, _) = broadcast::channel(32);
    Arc::new(Screen::new(Some(true), events))
}
async fn next(peer: &mut BufReader<UnixStream>) -> Value {
    let mut frame = String::new();
    peer.read_line(&mut frame).await.unwrap();
    serde_json::from_str(&frame).unwrap()
}
async fn reply(peer: &mut BufReader<UnixStream>, frame: &Value, result: Value) {
    peer.get_mut()
        .write_all(format!("{}\n", json!({"jsonrpc":"2.0","id":frame["id"],"result":result})).as_bytes())
        .await
        .unwrap();
}
async fn reject_close(screen: &Screen, peer: &mut BufReader<UnixStream>, close: &Value) {
    // Status posterior é barreira do MESMO reader: resposta de erro já consumida.
    let mut events = screen.events.subscribe();
    peer.get_mut()
        .write_all(
            format!("{}\n", json!({"jsonrpc":"2.0","id":close["id"],"error":{"message":"close recusado"}})).as_bytes(),
        )
        .await
        .unwrap();
    peer.get_mut().write_all(format!("{}\n", json!({"jsonrpc":"2.0","method":"status","params":{"platform":"fixture","capture":true,"displays":[retina()]}})).as_bytes()).await.unwrap();
    events.recv().await.unwrap();
}

fn replace(screen: &Screen, id: &str) -> ViewerRef {
    let fresh = screen.prepare("local", IceConfig::default()).unwrap();
    let mut sessions = screen.sessions.locked();
    let successor = sessions.rows.remove(fresh["session"].as_str().unwrap()).unwrap();
    sessions.rows.insert(id.into(), successor.clone());
    successor
}

#[tokio::test]
async fn close_antigo_nao_remove_sucessor_com_mesmo_id() {
    let screen = screen();
    let (mut peer, connection) = connect(&screen).await;
    let prepared = screen.prepare("local", IceConfig::default()).unwrap();
    let id = prepared["session"].as_str().unwrap();
    {
        let closing = screen.close(id);
        tokio::pin!(closing);
        assert!(futures::poll!(&mut closing).is_pending());
        let frame = next(&mut peer).await;
        assert_eq!(frame["method"], "close");
        let successor = replace(&screen, id);
        screen.takeover(true);
        reply(&mut peer, &frame, json!({})).await;
        assert!(closing.await.is_err());
        assert!(Arc::ptr_eq(screen.sessions.locked().rows.get(id).unwrap(), &successor));
        assert_eq!(screen.state()["userControl"], true);
    }
    drop(peer);
    connection.await.unwrap();
}

#[tokio::test]
async fn answer_antigo_nao_inicia_nem_fecha_sucessor() {
    let screen = screen();
    let (mut peer, connection) = connect(&screen).await;
    let prepared = screen.prepare("local", IceConfig::default()).unwrap();
    let id = prepared["session"].as_str().unwrap();
    {
        let offering = screen.offer("local", "offer", id, None);
        tokio::pin!(offering);
        assert!(futures::poll!(&mut offering).is_pending());
        let frame = next(&mut peer).await;
        assert_eq!(frame["method"], "answer");
        let successor = replace(&screen, id);
        reply(&mut peer, &frame, json!({"sdp":"late"})).await;
        assert!(offering.await.is_err());
        assert!(Arc::ptr_eq(screen.sessions.locked().rows.get(id).unwrap(), &successor));
        assert!(successor.locked().state == ViewerState::Prepared);
        let mut bytes = [0; 128];
        assert_eq!(
            peer.get_ref().try_read(&mut bytes).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "sem cleanup pelo id sucessor"
        );
    }
    drop(peer);
    connection.await.unwrap();
}

#[tokio::test]
async fn close_original_nao_usa_helper_substituto_e_controle_comum_funciona() {
    let screen = screen();
    let (mut old_peer, old_connection) = connect(&screen).await;
    let prepared = screen.prepare("local", IceConfig::default()).unwrap();
    let id = prepared["session"].as_str().unwrap();
    let closing = screen.close(id);
    tokio::pin!(closing);
    assert!(futures::poll!(&mut closing).is_pending());
    let old_frame = next(&mut old_peer).await;
    let (mut peer, connection) = connect(&screen).await;
    let successor = replace(&screen, id);
    old_peer
        .get_mut()
        .write_all(
            format!("{}\n", json!({"jsonrpc":"2.0","method":"session","params":{"session":id,"state":"closed"}}))
                .as_bytes(),
        )
        .await
        .unwrap();
    reply(&mut old_peer, &old_frame, json!({})).await;
    assert!(closing.await.is_err());
    assert!(Arc::ptr_eq(screen.sessions.locked().rows.get(id).unwrap(), &successor));
    let mut bytes = [0; 128];
    assert_eq!(peer.get_ref().try_read(&mut bytes).unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
    {
        let valid = screen.close(id);
        tokio::pin!(valid);
        assert!(futures::poll!(&mut valid).is_pending());
        let frame = next(&mut peer).await;
        assert_eq!(frame["method"], "close");
        reply(&mut peer, &frame, json!({})).await;
        valid.await.unwrap();
        assert!(!screen.sessions.locked().rows.contains_key(id));
    }
    drop(old_peer);
    old_connection.await.unwrap();
    drop(peer);
    connection.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn vigia_antigo_nao_fecha_registro_sucessor() {
    use crate::{hub::Hub, remote::identity::Identity, store::Store};
    let dir = std::env::temp_dir().join(format!("viewer-watch-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
        "fixture".into(),
        Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    *hub.screen.enabled.locked() = Some(true);
    let (peer, connection) = connect(&hub.screen).await;
    let prepared = hub.screen.prepare("local", IceConfig::default()).unwrap();
    let id = prepared["session"].as_str().unwrap();
    {
        let watching = super::super::viewer::watch_viewer(hub.clone(), id.into(), "local".into());
        tokio::pin!(watching);
        assert!(futures::poll!(&mut watching).is_pending());
        let successor = replace(&hub.screen, id);
        tokio::time::advance(std::time::Duration::from_secs(5)).await;
        watching.await;
        assert!(Arc::ptr_eq(hub.screen.sessions.locked().rows.get(id).unwrap(), &successor));
        let mut bytes = [0; 128];
        assert_eq!(peer.get_ref().try_read(&mut bytes).unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
    }
    drop(peer);
    connection.await.unwrap();
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn erro_de_answer_antigo_nao_expira_nem_expoe_erro_ao_sucessor() {
    let screen = screen();
    let (mut peer, connection) = connect(&screen).await;
    let prepared = screen.prepare("local", IceConfig::default()).unwrap();
    let id = prepared["session"].as_str().unwrap();
    {
        let offering = screen.offer("local", "offer", id, None);
        tokio::pin!(offering);
        assert!(futures::poll!(&mut offering).is_pending());
        let frame = next(&mut peer).await;
        let successor = replace(&screen, id);
        let expiry = successor.locked().ice.as_ref().unwrap().expires_at;
        peer.get_mut()
            .write_all(
                format!("{}\n", json!({"jsonrpc":"2.0","id":frame["id"],"error":{"message":"SENTINELA_ERRO_ANTIGO"}}))
                    .as_bytes(),
            )
            .await
            .unwrap();
        let error = offering.await.unwrap_err().to_string();
        assert!(!error.contains("SENTINELA_ERRO_ANTIGO"));
        assert_eq!(successor.locked().ice.as_ref().unwrap().expires_at, expiry);
        assert!(Arc::ptr_eq(screen.sessions.locked().rows.get(id).unwrap(), &successor));
        let mut bytes = [0; 128];
        assert_eq!(peer.get_ref().try_read(&mut bytes).unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
    }
    drop(peer);
    connection.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn vigia_conserva_origem_antes_do_primeiro_poll() {
    use crate::{hub::Hub, remote::identity::Identity, store::Store};
    let dir = std::env::temp_dir().join(format!("viewer-unpolled-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
        "fixture".into(),
        Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    *hub.screen.enabled.locked() = Some(true);
    let (peer, connection) = connect(&hub.screen).await;
    let prepared = hub.screen.prepare("local", IceConfig::default()).unwrap();
    let id = prepared["session"].as_str().unwrap();
    {
        // Construção real usada por tokio::spawn, ainda sem poll da tarefa.
        let watching = super::super::viewer::watch_viewer(hub.clone(), id.into(), "local".into());
        let successor = replace(&hub.screen, id);
        successor.locked().ice.as_mut().unwrap().expires_at = 0;
        tokio::time::advance(std::time::Duration::from_secs(5)).await;
        watching.await;
        assert!(Arc::ptr_eq(hub.screen.sessions.locked().rows.get(id).unwrap(), &successor));
        let mut bytes = [0; 128];
        assert_eq!(
            peer.get_ref().try_read(&mut bytes).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "vigia antigo não enfileira close do sucessor mesmo com dono/id iguais"
        );
    }
    drop(peer);
    connection.await.unwrap();
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(start_paused = true)]
async fn vigia_revogado_mantem_original_apos_erro_sem_tocar_sucessor() {
    use crate::{
        hub::Hub,
        remote::identity::Identity,
        store::{Device, DeviceSource, Scope, Store, now_ms},
    };
    for replace_after_error in [false, true] {
        let dir = std::env::temp_dir().join(format!("viewer-revoked-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Hub::new(
            Store::open(std::path::Path::new(":memory:")).unwrap(),
            "fixture".into(),
            Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        *hub.screen.enabled.locked() = Some(true);
        let device = Device {
            key: "fixture-viewer".into(),
            name: "Fixture".into(),
            platform: "fixture".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Screen],
            lease_until: None,
            created_at: now_ms(),
            last_seen_at: None,
        };
        hub.store.put_device(&device).unwrap();
        assert!(crate::api::devices::authorize(&hub, &device.key).is_ok());
        let (mut peer, connection) = connect(&hub.screen).await;
        let prepared = hub.screen.prepare(&device.key, IceConfig::default()).unwrap();
        let id = prepared["session"].as_str().unwrap();
        {
            let offering = hub.screen.offer(&device.key, "offer", id, None);
            tokio::pin!(offering);
            assert!(futures::poll!(&mut offering).is_pending());
            let frame = next(&mut peer).await;
            assert_eq!(frame["method"], "answer");
            reply(&mut peer, &frame, json!({"sdp":"answer"})).await;
            offering.await.unwrap();
        }
        let row = hub.screen.sessions.locked().rows.get(id).unwrap().clone();
        assert!(row.locked().state == ViewerState::Started);
        {
            let watching = super::super::viewer::watch_viewer(hub.clone(), id.into(), device.key.clone());
            tokio::pin!(watching);
            assert!(futures::poll!(&mut watching).is_pending());
            assert!(hub.revoke_device(&device.key).unwrap());
            assert!(crate::api::devices::authorize(&hub, &device.key).is_err());
            tokio::time::advance(std::time::Duration::from_secs(5)).await;
            assert!(futures::poll!(&mut watching).is_pending());
            let close = next(&mut peer).await;
            assert_eq!(close["method"], "close");
            assert_eq!(close["params"]["session"], id);
            reject_close(&hub.screen, &mut peer, &close).await;
            assert!(futures::poll!(&mut watching).is_pending(), "erro de close não abandona original ativo");
            assert!(Arc::ptr_eq(hub.screen.sessions.locked().rows.get(id).unwrap(), &row));
            assert!(row.locked().state == ViewerState::ClosingStarted);
            let mut bytes = [0; 128];
            assert_eq!(
                peer.get_ref().try_read(&mut bytes).unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock,
                "sem busy-loop: aguarda intervalo herdado"
            );
            if replace_after_error {
                // Mesmo dono/id; registro novo nunca é adotado pelo vigia antigo.
                let prepared = hub.screen.prepare(&device.key, IceConfig::default()).unwrap();
                let successor = {
                    let mut sessions = hub.screen.sessions.locked();
                    let successor = sessions.rows.remove(prepared["session"].as_str().unwrap()).unwrap();
                    sessions.rows.insert(id.into(), successor.clone());
                    successor
                };
                tokio::time::advance(std::time::Duration::from_secs(5)).await;
                watching.await;
                assert!(Arc::ptr_eq(hub.screen.sessions.locked().rows.get(id).unwrap(), &successor));
                assert_eq!(peer.get_ref().try_read(&mut bytes).unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
            } else {
                tokio::time::advance(std::time::Duration::from_secs(5)).await;
                assert!(futures::poll!(&mut watching).is_pending());
                let next_close = next(&mut peer).await;
                assert_eq!(next_close["method"], "close");
                assert_eq!(next_close["params"]["session"], id);
                reply(&mut peer, &next_close, json!({})).await;
                watching.await;
                assert!(
                    !hub.screen.sessions.locked().rows.contains_key(id),
                    "controle comum positivo; não é término nativo"
                );
            }
        }
        drop(peer);
        connection.await.unwrap();
        drop(hub);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[tokio::test]
async fn troca_de_link_retira_so_viewers_antigos_e_conserva_origem_incerta() {
    let screen = screen();
    let (mut old_peer, old_connection) = connect(&screen).await;
    let mut originals = Vec::new();
    for index in 0..32 {
        let owner = if index < 4 { "local".to_owned() } else { format!("owner{}", index / 4) };
        let prepared = screen.prepare(&owner, IceConfig::default()).unwrap();
        let id = prepared["session"].as_str().unwrap().to_owned();
        originals.push((id.clone(), screen.sessions.locked().rows.get(&id).unwrap().clone()));
    }
    {
        let offering = screen.offer("local", "offer", &originals[0].0, None);
        tokio::pin!(offering);
        assert!(futures::poll!(&mut offering).is_pending());
        let answer = next(&mut old_peer).await;
        reply(&mut old_peer, &answer, json!({"sdp":"answer"})).await;
        offering.await.unwrap();
    }
    let old_link = originals[0].1.locked().link.clone();
    screen.takeover(true);
    assert_eq!(screen.state()["viewers"], 1);
    assert!(screen.prepare("local", IceConfig::default()).is_err());
    assert!(screen.prepare("another-owner", IceConfig::default()).is_err());
    let (mut peer, connection) = connect(&screen).await;
    assert_eq!(screen.state()["viewers"], 0);
    assert_eq!(screen.state()["userControl"], false);
    for (id, row) in &originals {
        assert!(Arc::ptr_eq(screen.sessions.locked().rows.get(id).unwrap(), row));
        assert!(row.locked().state == ViewerState::Uncertain);
        assert!(Arc::ptr_eq(&row.locked().link, &old_link));
        assert!(!screen.owns_session(id, "local"));
    }
    let successor = screen.prepare("local", IceConfig::default()).unwrap();
    let id = successor["session"].as_str().unwrap();
    let successor = screen.sessions.locked().rows.get(id).unwrap().clone();
    for _ in 0..3 {
        screen.prepare("local", IceConfig::default()).unwrap();
    }
    assert!(screen.prepare("local", IceConfig::default()).is_err(), "quota4 vale apenas para a conexão ativa");
    for index in 4..32 {
        screen.prepare(&format!("owner{}", index / 4), IceConfig::default()).unwrap();
    }
    assert!(screen.prepare("another-owner", IceConfig::default()).is_err(), "quota32 vale apenas para a conexão ativa");
    {
        let offering = screen.offer("local", "offer", id, None);
        tokio::pin!(offering);
        assert!(futures::poll!(&mut offering).is_pending());
        let answer = next(&mut peer).await;
        reply(&mut peer, &answer, json!({"sdp":"answer"})).await;
        offering.await.unwrap();
    }
    screen.takeover(true);
    // Notificação/EOF antigos não apagam proveniência incerta nem sucessor.
    old_peer
        .get_mut()
        .write_all(
            format!(
                "{}\n",
                json!({"jsonrpc":"2.0","method":"session","params":{"session":originals[0].0,"state":"closed"}})
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    drop(old_peer);
    old_connection.await.unwrap();
    assert!(Arc::ptr_eq(screen.sessions.locked().rows.get(id).unwrap(), &successor));
    assert_eq!(screen.state()["viewers"], 1);
    assert_eq!(screen.state()["userControl"], true);
    for (id, row) in &originals {
        assert!(Arc::ptr_eq(screen.sessions.locked().rows.get(id).unwrap(), row));
        assert!(row.locked().state == ViewerState::Uncertain);
    }
    drop(peer);
    connection.await.unwrap();
    assert!(successor.locked().state == ViewerState::Uncertain);
    assert_eq!(screen.state()["viewers"], 0);
    assert_eq!(screen.state()["userControl"], false);
}

async fn start_viewers(screen: &Screen, peer: &mut BufReader<UnixStream>, count: usize) -> Vec<String> {
    use crate::screen::viewer::IceServer;
    let mut ids = Vec::new();
    for index in 0..count {
        let owner = format!("owner{}", index / 4);
        let ice = IceConfig {
            ice_servers: vec![IceServer {
                urls: vec!["turn:fixture.invalid".into()],
                username: Some("fixture".into()),
                credential: Some("fixture".into()),
            }],
            ..IceConfig::default()
        };
        let prepared = screen.prepare(&owner, ice).unwrap();
        let id = prepared["session"].as_str().unwrap().to_owned();
        {
            let offering = screen.offer(&owner, "offer", &id, None);
            tokio::pin!(offering);
            assert!(futures::poll!(&mut offering).is_pending());
            let request = next(peer).await;
            assert_eq!(request["method"], "answer");
            reply(peer, &request, json!({"sdp":"answer"})).await;
            offering.await.unwrap();
        }
        ids.push(id);
    }
    ids
}

#[tokio::test]
async fn reconexoes_limitam_incertos_limpam_ice_e_resumo_permanece_sem_adotar_sucessor() {
    let make_screen = screen;
    let screen = make_screen();
    let (mut peer, mut connection) = connect(&screen).await;
    let mut original_ids = Vec::new();
    for reconnect in 0..10 {
        original_ids.extend(start_viewers(&screen, &mut peer, 32).await);
        screen.takeover(true);
        assert_eq!(screen.state()["viewers"], 32);
        let (next_peer, next_connection) = connect(&screen).await;
        drop(peer);
        connection.await.unwrap();
        peer = next_peer;
        connection = next_connection;
        {
            let sessions = screen.sessions.locked();
            assert_eq!(sessions.rows.len(), ((reconnect + 1) * 32).min(256));
            assert_eq!(sessions.uncertainty_summarized, reconnect >= 8);
            for row in sessions.rows.values() {
                let row = row.locked();
                assert!(row.state == ViewerState::Uncertain);
                assert!(row.ice.is_none(), "sem imprimir ICE ou credenciais");
                assert_eq!(row.pending_until, 0);
            }
        }
        assert_eq!(screen.state()["viewers"], 0);
        assert_eq!(screen.state()["userControl"], false);
        assert!(screen.link.locked().as_ref().unwrap().pending.locked().is_empty());
        let mut bytes = [0; 128];
        assert_eq!(
            peer.get_ref().try_read(&mut bytes).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "nenhum cleanup/admissão na conexão sucessora"
        );
    }
    {
        let sessions = screen.sessions.locked();
        for id in &original_ids[..64] {
            assert!(!sessions.rows.contains_key(id));
        }
        for id in &original_ids[64..] {
            assert!(sessions.rows.contains_key(id));
        }
        assert!(sessions.uncertainty_summarized);
    }
    let ids = start_viewers(&screen, &mut peer, 1).await;
    let id = &ids[0];
    let successor = screen.sessions.locked().rows.get(id).unwrap().clone();
    screen.takeover(true);
    assert!(successor.locked().state == ViewerState::Started);
    assert!(successor.locked().ice.is_some());
    assert_eq!(screen.state()["viewers"], 1);
    assert_eq!(screen.state()["userControl"], true);
    {
        let closing = screen.close(id);
        tokio::pin!(closing);
        assert!(futures::poll!(&mut closing).is_pending());
        let request = next(&mut peer).await;
        assert_eq!(request["method"], "close");
        reply(&mut peer, &request, json!({})).await;
        closing.await.unwrap();
    }
    assert!(screen.sessions.locked().uncertainty_summarized, "controle comum não limpa o resumo");
    assert_eq!(screen.sessions.locked().rows.len(), 256);
    drop(peer);
    connection.await.unwrap();
    assert!(screen.sessions.locked().uncertainty_summarized, "EOF não limpa o resumo");
    // Um novo boot tem outra memória; ausência de lista/resumo NÃO é fato nativo.
    let restarted = make_screen();
    assert!(!restarted.sessions.locked().uncertainty_summarized);
}

#[tokio::test]
async fn close_preparado_antes_de_answer_libera_reserva_e_vaga() {
    let screen = screen();
    let (mut peer, connection) = connect(&screen).await;
    let prepared = screen.prepare("local", IceConfig::default()).unwrap();
    let id = prepared["session"].as_str().unwrap();
    for _ in 0..3 {
        screen.prepare("local", IceConfig::default()).unwrap();
    }
    assert!(screen.prepare("local", IceConfig::default()).is_err());
    assert!(screen.sessions.locked().rows[id].locked().state == ViewerState::Prepared);
    {
        let closing = screen.close(id);
        tokio::pin!(closing);
        assert!(futures::poll!(&mut closing).is_pending());
        let frame = next(&mut peer).await;
        assert_eq!(frame["method"], "close", "nenhum answer antes do close");
        assert_eq!(frame["params"]["session"], id);
        // The helper's no-entry response is legacy control, not native proof.
        reply(&mut peer, &frame, json!({})).await;
        closing.await.unwrap();
    }
    assert!(!screen.sessions.locked().rows.contains_key(id));
    assert!(screen.prepare("local", IceConfig::default()).is_ok());
    assert_eq!(screen.state()["viewers"], 0);
    drop(peer);
    connection.await.unwrap();
}

// Cuts are outside sessions: the admission itself must stay synchronous.
fn pause_offer() -> tokio::sync::oneshot::Sender<()> {
    let (resume, paused) = tokio::sync::oneshot::channel();
    on_offer_admission_for_test(paused);
    resume
}

async fn fifo_barrier(screen: &Screen, peer: &mut BufReader<UnixStream>) {
    let prepared = screen.prepare("barrier", IceConfig::default()).unwrap();
    let closing = screen.close(prepared["session"].as_str().unwrap());
    tokio::pin!(closing);
    assert!(futures::poll!(&mut closing).is_pending());
    let frame = next(peer).await;
    assert_eq!(frame["method"], "close", "nenhum answer tardio antes da barreira FIFO");
    assert_eq!(frame["params"]["session"], prepared["session"]);
    reply(peer, &frame, json!({})).await;
    closing.await.unwrap();
}

async fn denied_without_frame(offering: impl Future<Output = Result<Value>>, peer: &mut BufReader<UnixStream>) {
    tokio::select! {
        result = offering => assert!(result.is_err()),
        _ = next(peer) => panic!("offer recusada não pode enviar frame"),
    }
}

#[tokio::test]
async fn close_antes_de_answer_bloqueia_envio_com_e_sem_ack() {
    for ack in [false, true] {
        let screen = screen();
        let (mut peer, connection) = connect(&screen).await;
        let prepared = screen.prepare("local", IceConfig::default()).unwrap();
        let id = prepared["session"].as_str().unwrap();
        let original = Original::capture(&screen, id.into()).unwrap();
        let resume = pause_offer();
        let offering = screen.offer("local", "offer", id, None);
        tokio::pin!(offering);
        assert!(futures::poll!(&mut offering).is_pending());
        {
            let closing = screen.close(id);
            tokio::pin!(closing);
            assert!(futures::poll!(&mut closing).is_pending());
            let close = next(&mut peer).await;
            assert_eq!(close["method"], "close");
            assert!(original.closing());
            assert!(original.current(&screen));
            if ack {
                reply(&mut peer, &close, json!({})).await;
                closing.await.unwrap();
                resume.send(()).unwrap();
                denied_without_frame(offering, &mut peer).await;
            } else {
                resume.send(()).unwrap();
                denied_without_frame(offering, &mut peer).await;
                fifo_barrier(&screen, &mut peer).await;
                reject_close(&screen, &mut peer, &close).await;
                assert!(closing.await.is_err());
                assert!(original.current(&screen));
                assert!(original.closing());
                assert!(screen.offer("local", "offer", id, None).await.is_err());
                assert!(original.start(&screen).is_err());
            }
        }
        fifo_barrier(&screen, &mut peer).await;
        drop(peer);
        connection.await.unwrap();
    }
}

#[tokio::test]
async fn answer_enfileirado_nao_bloqueia_close_pending() {
    let screen = screen();
    let (mut peer, connection) = connect(&screen).await;
    let prepared = screen.prepare("local", IceConfig::default()).unwrap();
    let id = prepared["session"].as_str().unwrap();
    {
        let offering = screen.offer("local", "offer", id, None);
        tokio::pin!(offering);
        assert!(futures::poll!(&mut offering).is_pending());
        let closing = screen.close(id);
        tokio::pin!(closing);
        assert!(futures::poll!(&mut closing).is_pending());
        let original = Original::capture(&screen, id.into()).unwrap();
        // Both waiters exist before releasing either response/negotiation.
        assert_eq!(original.link.pending.locked().len(), 2);
        let answer = next(&mut peer).await;
        let close = next(&mut peer).await;
        assert_eq!(answer["method"], "answer");
        assert_eq!(close["method"], "close");
        reject_close(&screen, &mut peer, &close).await;
        assert!(closing.await.is_err());
        assert!(original.closing());
        reply(&mut peer, &answer, json!({"sdp":"late answer"})).await;
        denied_without_frame(offering, &mut peer).await;
        assert!(original.row.locked().state == ViewerState::ClosingPrepared);
        assert_eq!(screen.state()["viewers"], 0);
        fifo_barrier(&screen, &mut peer).await;
    }
    drop(peer);
    connection.await.unwrap();
}

#[tokio::test]
async fn disable_ordena_close_all_e_preserva_sucessor() {
    use crate::store::Store;
    for answer_first in [false, true] {
        for success in [false, true] {
            let screen = screen();
            let store = Store::open(std::path::Path::new(":memory:")).unwrap();
            let (mut peer, connection) = connect(&screen).await;
            let prepared = screen.prepare("local", IceConfig::default()).unwrap();
            let id = prepared["session"].as_str().unwrap();
            let original = Original::capture(&screen, id.into()).unwrap();
            let another = screen.prepare("local", IceConfig::default()).unwrap();
            let another = Original::capture(&screen, another["session"].as_str().unwrap().into()).unwrap();
            let resume = if answer_first { None } else { Some(pause_offer()) };
            {
                let offering = screen.offer("local", "offer", id, None);
                tokio::pin!(offering);
                assert!(futures::poll!(&mut offering).is_pending());
                let disabling = screen.set_enabled(&store, false);
                tokio::pin!(disabling);
                assert!(futures::poll!(&mut disabling).is_pending());
                assert!(original.closing() && another.closing());
                let answer = if answer_first {
                    let answer = next(&mut peer).await;
                    assert_eq!(answer["method"], "answer");
                    Some(answer)
                } else {
                    None
                };
                let close_all = next(&mut peer).await;
                assert_eq!(close_all["method"], "closeAll");
                if let Some(resume) = resume {
                    resume.send(()).unwrap();
                }
                screen.set_enabled(&store, true).await.unwrap();
                if let Some(answer) = answer {
                    reply(&mut peer, &answer, json!({"sdp":"late"})).await;
                }
                denied_without_frame(offering, &mut peer).await;
                assert!(original.start(&screen).is_err());
                let successor = replace(&screen, id);
                {
                    let valid = screen.offer("local", "new", id, None);
                    tokio::pin!(valid);
                    assert!(futures::poll!(&mut valid).is_pending());
                    let frame = next(&mut peer).await;
                    assert_eq!(frame["method"], "answer");
                    reply(&mut peer, &frame, json!({"sdp":"new"})).await;
                    valid.await.unwrap();
                }
                screen.takeover(true);
                if success {
                    reply(&mut peer, &close_all, json!({})).await;
                } else {
                    reject_close(&screen, &mut peer, &close_all).await;
                }
                disabling.await.unwrap();
                assert!(Arc::ptr_eq(screen.sessions.locked().rows.get(id).unwrap(), &successor));
                assert!(successor.locked().state == ViewerState::Started);
                assert_eq!(screen.state()["userControl"], true);
                assert_eq!(screen.state()["viewers"], 1);
                if success {
                    assert!(!another.current(&screen));
                } else {
                    assert!(another.current(&screen) && another.closing());
                }
                assert!(original.closing());
                fifo_barrier(&screen, &mut peer).await;
            }
            drop(peer);
            connection.await.unwrap();
        }
    }
    // ACK do Link anterior não remove o sucessor reconectado com o mesmo id.
    {
        let screen = screen();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let (mut old_peer, old_connection) = connect(&screen).await;
        let prepared = screen.prepare("local", IceConfig::default()).unwrap();
        let id = prepared["session"].as_str().unwrap();
        {
            let disabling = screen.set_enabled(&store, false);
            tokio::pin!(disabling);
            assert!(futures::poll!(&mut disabling).is_pending());
            let close_all = next(&mut old_peer).await;
            assert_eq!(close_all["method"], "closeAll");
            screen.set_enabled(&store, true).await.unwrap();
            let (mut peer, connection) = connect(&screen).await;
            let successor = replace(&screen, id);
            let offering = screen.offer("local", "new", id, None);
            tokio::pin!(offering);
            assert!(futures::poll!(&mut offering).is_pending());
            let answer = next(&mut peer).await;
            assert_eq!(answer["method"], "answer");
            reply(&mut peer, &answer, json!({"sdp":"new"})).await;
            offering.await.unwrap();
            screen.takeover(true);
            reply(&mut old_peer, &close_all, json!({})).await;
            disabling.await.unwrap();
            assert!(Arc::ptr_eq(screen.sessions.locked().rows.get(id).unwrap(), &successor));
            assert!(successor.locked().state == ViewerState::Started);
            assert_eq!(screen.state()["userControl"], true);
            fifo_barrier(&screen, &mut peer).await;
            drop(peer);
            connection.await.unwrap();
        }
        drop(old_peer);
        old_connection.await.unwrap();
    }
    // Empty closeAll remains the existing legacy RPC, not a native fact.
    let screen = screen();
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let (mut peer, connection) = connect(&screen).await;
    {
        let disabling = screen.set_enabled(&store, false);
        tokio::pin!(disabling);
        assert!(futures::poll!(&mut disabling).is_pending());
        let frame = next(&mut peer).await;
        assert_eq!(frame["method"], "closeAll");
        reply(&mut peer, &frame, json!({})).await;
        disabling.await.unwrap();
    }
    drop(peer);
    connection.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn espera_enfileirada_descartada_limpa_so_waiter_original() {
    let screen = screen();
    let (mut old_peer, old_connection) = connect(&screen).await;
    let old = screen.link().unwrap();
    let unpolled = old.enqueue("close", &json!({"session":"fixture"})).unwrap();
    let first = next(&mut old_peer).await;
    let mut waiting = Box::pin(old.enqueue("close", &json!({"session":"other"})).unwrap());
    let other = next(&mut old_peer).await;
    assert!(futures::poll!(&mut waiting).is_pending());
    assert_eq!(old.pending.locked().len(), 2);
    let (mut peer, connection) = connect(&screen).await;
    let new = screen.link().unwrap();
    let valid = new.enqueue("close", &json!({"session":"new"})).unwrap();
    let frame = next(&mut peer).await;
    assert_eq!(frame["id"], first["id"], "id numérico igual em Link diferente");
    drop(unpolled);
    assert!(!old.pending.locked().contains_key(&first["id"].as_i64().unwrap()));
    assert!(old.pending.locked().contains_key(&other["id"].as_i64().unwrap()));
    drop(waiting);
    assert!(old.pending.locked().is_empty());
    reply(&mut old_peer, &first, json!({"stale":true})).await;
    assert_eq!(new.pending.locked().len(), 1);
    reply(&mut peer, &frame, json!({})).await;
    assert_eq!(valid.await.unwrap(), json!({}));
    assert!(new.pending.locked().is_empty());
    // An unpolled enqueue keeps the original timeout, rather than gaining 15s.
    let delayed = new.enqueue("close", &json!({"session":"delayed"})).unwrap();
    let _ = next(&mut peer).await;
    tokio::time::advance(super::super::HELPER_TIMEOUT).await;
    assert!(delayed.await.is_err());
    assert!(new.pending.locked().is_empty());
    drop(old_peer);
    old_connection.await.unwrap();
    drop(peer);
    connection.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn vigia_closing_reenabled_retenta_origem_valida() {
    use crate::{hub::Hub, remote::identity::Identity, store::Store};
    let dir = std::env::temp_dir().join(format!("viewer-closing-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
        "fixture".into(),
        Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    *hub.screen.enabled.locked() = Some(true);
    let (mut peer, connection) = connect(&hub.screen).await;
    let prepared = hub.screen.prepare("local", IceConfig::default()).unwrap();
    let id = prepared["session"].as_str().unwrap();
    let original = Original::capture(&hub.screen, id.into()).unwrap();
    // Cancelar sem primeiro poll limpa a espera, mas não desfaz Closing/enqueue.
    let wait = original.enqueue_close(&hub.screen).unwrap();
    drop(wait);
    assert!(original.link.pending.locked().is_empty());
    let first = next(&mut peer).await;
    assert_eq!(first["method"], "close");
    hub.screen.set_enabled(&hub.store, true).await.unwrap();
    assert!(original.closing());
    assert!(!original.expired(), "retry depende de Closing, não de prazo/autoridade revogada");
    {
        let watching = super::super::viewer::watch_viewer(hub.clone(), id.into(), "local".into());
        tokio::pin!(watching);
        assert!(futures::poll!(&mut watching).is_pending());
        tokio::time::advance(std::time::Duration::from_secs(5)).await;
        assert!(futures::poll!(&mut watching).is_pending());
        let retry = next(&mut peer).await;
        assert_eq!(retry["method"], "close");
        assert_eq!(retry["params"]["session"], id);
        reply(&mut peer, &retry, json!({})).await;
        watching.await;
        assert!(!original.current(&hub.screen));
    }
    drop(peer);
    connection.await.unwrap();
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn c2_original_answer_restart_and_close_are_distinct() {
    let screen = screen();
    let (mut peer, connection) = connect(&screen).await;
    let prepared = screen.prepare("local", IceConfig::default()).unwrap();
    let id = prepared["session"].as_str().unwrap();
    let original = Original::capture(&screen, id.into()).unwrap();
    for turn in 0..2 {
        let wait = original.enqueue_answer(&screen, &json!({"session":id})).unwrap();
        let bound = original.row.locked().origin_answer.unwrap();
        let frame = next(&mut peer).await;
        if turn == 0 {
            assert_eq!(bound, frame["id"].as_i64().unwrap());
            assert!(frame["params"].get("answerId").is_none());
        } else {
            assert_eq!(frame["params"]["answerId"], bound);
            assert_ne!(frame["id"], bound);
        }
        reply(&mut peer, &frame, json!({"sdp":"answer"})).await;
        wait.await.unwrap();
    }
    let a = original.row.locked().origin_answer.unwrap();
    original.start(&screen).unwrap();
    let close = original.close_individual(&screen);
    tokio::pin!(close);
    assert!(futures::poll!(&mut close).is_pending());
    let frame = next(&mut peer).await;
    assert_eq!(frame["params"]["answerId"], a);
    assert_ne!(frame["id"], a);
    reply(&mut peer, &frame, json!({"nativeStop":{"answerId":a,"state":"null"}})).await;
    match close.await.unwrap() {
        CloseOutcome::PipelineNull(fact) => {
            assert!(std::ptr::eq(fact.original, &raw const original));
            assert_eq!(fact.answer, a);
        }
        CloseOutcome::ControlOnly => panic!("expected original pipeline fact"),
    }
    assert!(!screen.sessions.locked().rows.contains_key(id));
    drop(peer);
    connection.await.unwrap();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn c2_rejects_invalid_results_and_preserves_successors() {
    for case in 0..15 {
        let screen = screen();
        let (mut peer, connection) = connect(&screen).await;
        let prepared = screen.prepare("local", IceConfig::default()).unwrap();
        let id = prepared["session"].as_str().unwrap();
        let original = Original::capture(&screen, id.into()).unwrap();
        let answer = original.enqueue_answer(&screen, &json!({"session":id})).unwrap();
        let first = next(&mut peer).await;
        let a = first["id"].as_i64().unwrap();
        reply(&mut peer, &first, json!({})).await;
        answer.await.unwrap();
        original.start(&screen).unwrap();
        let close = original.close_individual(&screen);
        tokio::pin!(close);
        assert!(futures::poll!(&mut close).is_pending());
        let frame = next(&mut peer).await;
        let successor = if case == 8 { Some(replace(&screen, id)) } else { None };
        if case == 9 {
            screen.sessions.locked().rows.remove(id);
        }
        let (mut new_peer, new_connection) = if case == 10 {
            let (p, c) = connect(&screen).await;
            (Some(p), Some(c))
        } else {
            (None, None)
        };
        if let Some(peer) = new_peer.as_mut() {
            // Mesmos números no outro Link não resolvem o pending original.
            let mut events = screen.events.subscribe();
            reply(peer, &frame, json!({"nativeStop":{"answerId":a,"state":"null"}})).await;
            peer.get_mut().write_all(format!("{}\n",json!({"jsonrpc":"2.0","method":"status","params":{"platform":"fixture","capture":true,"displays":[retina()]}})).as_bytes()).await.unwrap();
            events.recv().await.unwrap();
            assert!(futures::poll!(&mut close).is_pending());
        }
        let result = match case {
            0 => json!({"nativeStop":{"answerId":frame["id"],"state":"null"}}),
            1 => json!({"nativeStop":{"answerId":a+90,"state":"null"}}),
            2 => json!({"nativeStop":{"answerId":a.to_string(),"state":"null"}}),
            3 => json!({"nativeStop":{"answerId":null,"state":"null"}}),
            4 => json!({"nativeStop":{"answerId":a,"state":null}}),
            5 => json!({"nativeStop":null}),
            6 => json!({}),
            7 => json!({"nativeStop":{"answerId":a,"state":"NULL"}}),
            12 => {
                json!({"nativeStop":{"answerId":serde_json::from_str::<Value>(&format!("{a}.0")).unwrap(),"state":"null"}})
            }
            13 => json!({"nativeStop":{"answerId":a,"state":"null"},"error":"transition"}),
            14 => json!({"nativeStop":{"answerId":a,"state":"null","unexpected":true}}),
            _ => json!({"nativeStop":{"answerId":a,"state":"null"}}),
        };
        if case == 11 {
            reject_close(&screen, &mut peer, &frame).await;
        } else {
            reply(&mut peer, &frame, result).await;
        }
        let result = close.await;
        if case == 6 {
            assert!(matches!(result.unwrap(), CloseOutcome::ControlOnly));
        } else {
            assert!(result.is_err());
        }
        if let Some(successor) = successor {
            assert!(Arc::ptr_eq(&screen.sessions.locked().rows[id], &successor));
        }
        drop(new_peer);
        if let Some(c) = new_connection {
            c.await.unwrap();
        }
        drop(peer);
        connection.await.unwrap();
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn c2_prepared_retry_drop_timeout_and_notification_stay_control() {
    for case in 0..5 {
        let screen = screen();
        let (mut peer, connection) = connect(&screen).await;
        let prepared = screen.prepare("local", IceConfig::default()).unwrap();
        let id = prepared["session"].as_str().unwrap();
        let original = Original::capture(&screen, id.into()).unwrap();
        let answer = original.enqueue_answer(&screen, &json!({"session":id})).unwrap();
        let first = next(&mut peer).await;
        let a = first["id"].as_i64().unwrap();
        reply(&mut peer, &first, json!({})).await;
        answer.await.unwrap();
        if case != 0 {
            original.start(&screen).unwrap();
        }
        let mut abandoned = None;
        {
            let close = original.close_individual(&screen);
            tokio::pin!(close);
            assert!(futures::poll!(&mut close).is_pending());
            let frame = next(&mut peer).await;
            if case == 0 {
                assert!(frame["params"].get("answerId").is_none());
            }
            if case == 2 {
                tokio::time::pause();
                tokio::time::advance(std::time::Duration::from_secs(15)).await;
                assert!(close.await.is_err());
                tokio::time::resume();
            } else if case == 3 {
                peer.get_mut()
                    .write_all(
                        format!(
                            "{}\n",
                            json!({"jsonrpc":"2.0","method":"session","params":{"session":id,"state":"closed"}})
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
                reply(&mut peer, &frame, json!({"nativeStop":{"answerId":a,"state":"null"}})).await;
                assert!(close.await.is_err());
            } else if case != 1 {
                reply(&mut peer, &frame, json!({"nativeStop":{"answerId":a,"state":"null"}})).await;
                if case == 0 {
                    assert!(close.await.is_err());
                } else {
                    close.await.unwrap().discard();
                }
            }
            if case == 1 || case == 2 {
                abandoned = Some(frame);
            }
            // case1 drops the original waiter while the frame remains in flight.
        }
        if case == 1 || case == 2 {
            // The next individual attempt is already ClosingStarted and must be legacy.
            let retry = original.close_individual(&screen);
            tokio::pin!(retry);
            assert!(futures::poll!(&mut retry).is_pending());
            let frame = next(&mut peer).await;
            assert!(frame["params"].get("answerId").is_none());
            let old = abandoned.as_ref().unwrap();
            assert!(!original.link.pending.locked().contains_key(&old["id"].as_i64().unwrap()));
            reply(&mut peer, old, json!({"nativeStop":{"answerId":a,"state":"null"}})).await;
            // FIFO status barrier on the reader after the abandoned reply.
            let mut events = screen.events.subscribe();
            peer.get_mut().write_all(format!("{}\n",json!({"jsonrpc":"2.0","method":"status","params":{"platform":"fixture","capture":true,"displays":[retina()]}})).as_bytes()).await.unwrap();
            events.recv().await.unwrap();
            assert!(futures::poll!(&mut retry).is_pending(), "old C must not resolve new waiter");
            reply(&mut peer, &frame, json!({})).await;
            assert!(matches!(retry.await.unwrap(), CloseOutcome::ControlOnly));
        }
        fifo_barrier(&screen, &mut peer).await;
        drop(peer);
        connection.await.unwrap();
    }
}
