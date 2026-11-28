#![allow(clippy::unwrap_used)]
use super::*;
use crate::{
    environment::Environment,
    fork::{
        conectores::{catalogo_servicos, oauth_binding},
        donna_fixa::pedido::Origem,
        open_chat,
    },
    hub::BotStatus,
    store::{EntryKind, Lane},
};
use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};
use std::sync::atomic::{AtomicUsize, Ordering};

const ACCOUNT: &str = "fixture-mcp";
const OLD: &str = "fixture-old-access";
const NEW: &str = "fixture-new-access";
const SESSION: &str = "fixture-private-session";

struct Server {
    url: Url,
    sent: Arc<AtomicUsize>,
    refreshed: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn server() -> Server {
    let sent = Arc::new(AtomicUsize::new(0));
    let refreshed = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = Url::parse(&format!("http://{}/mcp?private=fixture-query-part", listener.local_addr().unwrap())).unwrap();
    let router = Router::new()
        .route("/token", post({
            let refreshed = refreshed.clone();
            move |body: String| async move {
                assert!(body.contains("refresh_token=fixture-old-refresh"));
                refreshed.fetch_add(1, Ordering::SeqCst);
                Json(json!({"access_token":NEW,"refresh_token":"fixture-new-refresh","expires_in":3600}))
            }
        }))
        .route("/mcp", post({
            let sent = sent.clone();
            let url = url.to_string();
            move |headers: HeaderMap, Json(frame): Json<Value>| async move {
                let bearer = headers["authorization"].to_str().unwrap();
                if frame["method"] == "initialize" && bearer.strip_prefix("Bearer ") == Some(OLD) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                assert_eq!(bearer.strip_prefix("Bearer "), Some(NEW));
                let result = match frame["method"].as_str().unwrap() {
                    "initialize" => json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},
                        "serverInfo":{"name":"fixture"},"instructions":"do not forward these instructions"}),
                    "notifications/initialized" => return StatusCode::ACCEPTED.into_response(),
                    "tools/list" => json!({"tools":[
                        {"name":"list_issues","description":"Read fixture issues","inputSchema":{"type":"object"}},
                        {"name":"delete_issue","description":"Delete a fixture issue","inputSchema":{"type":"object"},
                            "annotations":{"destructiveHint":true}}
                    ]}),
                    "tools/call" => {
                        sent.fetch_add(1, Ordering::SeqCst);
                        assert_eq!(frame["params"], json!({"name":"list_issues","arguments":{"query":"sample"}}));
                        assert_eq!(headers["mcp-session-id"], SESSION);
                        json!({"content":[{"type":"text","text":format!("fixture-result {OLD} {NEW} {SESSION} {url}")}],
                            "structuredContent":{"echo":"fixture-new-refresh","note":"fixture-structured"}})
                    }
                    _ => unreachable!(),
                };
                ([("mcp-session-id", SESSION)], Json(json!({"jsonrpc":"2.0","id":frame["id"],"result":result}))).into_response()
            }
        }));
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Server { url, sent, refreshed, task }
}

fn fixture(endpoint: &Url, ingress: &str, same_space: bool) -> (Arc<Hub>, open_chat::Ticket, Option<LocalTurn>) {
    let catalog = catalogo_servicos::servico("linear").unwrap().tipo("linear.oficial_oauth").unwrap();
    let mut token = endpoint.join("token").unwrap();
    token.set_query(None);
    let account: Connector = serde_json::from_value(json!({
        "id":ACCOUNT,"name":"Fixture account","url":catalog.url,"servico":"linear","tipo":catalog.id,"liberacoes":["ler"],
        "space":if same_space { crate::fork::spaces::PERSONAL } else { "00000000-0000-4000-8000-000000000099" },
        "oauth":{"clientId":"fixture-client","tokenEndpoint":token.as_str(),"accessToken":OLD,"refreshToken":"fixture-old-refresh"}
    })).unwrap();
    fixture_with_account(account, ingress)
}

fn fixture_with_account(account: Connector, ingress: &str) -> (Arc<Hub>, open_chat::Ticket, Option<LocalTurn>) {
    let hub = oauth_binding::tests::hub();
    let bot = uuid::Uuid::new_v4().to_string();
    let lane = Lane::main(&bot);
    let cfg = serde_json::from_value(json!({"id":bot,"name":"Fixture bot","connectors":[ACCOUNT]})).unwrap();
    hub.store.save_bot(&cfg).unwrap();
    market::save_connectors(&hub.store, &[account]).unwrap();
    let ticket = open_chat::admit_for_test(&hub, &bot, "fixture-codex").unwrap();
    let turn = hub.store.alloc_turn(&bot).unwrap();
    let entry = hub
        .add_entry(
            &lane,
            EntryKind::User,
            turn,
            &json!({
                "text":"use fixture connector","contentOrigin":{"ingress":{"caller":ingress}}
            }),
        )
        .unwrap();
    hub.set_runtime(&bot, |runtime| {
        runtime.status = BotStatus::Working;
        runtime.message = Some(entry.id.clone());
        runtime.lane = Some(lane.clone());
    });
    let fake_cancel = tokio::sync::watch::channel(false);
    assert!(LocalTurn::capture(&hub, &lane, &entry.id, turn, fake_cancel.1).is_err());
    let proof = LocalTurn::capture(&hub, &lane, &entry.id, turn, open_chat::ticket_cancel(&ticket)).ok();
    (hub, ticket, proof)
}

async fn next_card(hub: &Hub, events: &mut tokio::sync::broadcast::Receiver<Value>) -> Entry {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let event = events.recv().await.unwrap();
            if event["entry"]["kind"] == "permission"
                && let Some(card) = event["entry"]["id"].as_str().and_then(|id| hub.store.entry(id))
                && card.data["status"] == "pending"
            {
                return card;
            }
        }
    })
    .await
    .unwrap()
}

fn answer(hub: &Arc<Hub>, card: &Entry, option: &str) {
    open_tool_actions::respond(hub, card, card.rev, &json!({"source":"app"}), Some(option), Some(&Origem::Local))
        .unwrap();
}

fn link_fixture_tool(hub: &Hub, proof: &LocalTurn) {
    let bot = &proof.lane().chat;
    let list = ferramentas::listar(&hub.store, bot, ACCOUNT).unwrap();
    let item = list["ferramentas"].as_array().unwrap().iter().find(|item| item["nome"] == "list_issues").unwrap();
    let pedido = ferramentas::PedidoLigar {
        bot,
        conta: ACCOUNT,
        ferramenta: "list_issues",
        digest: item["digest"].as_str().unwrap(),
    };
    ferramentas::conferir_ligar(&hub.store, &pedido, true).unwrap();
    let db = hub.store.db.locked();
    ferramentas::conferir_apresentacao(&db, &pedido, item["apresentacao"].as_str()).unwrap();
    ferramentas::gravar_liberacao(&db, &pedido).unwrap();
}

#[tokio::test]
async fn mcp_open_planetscale_nao_tenta_sse_legado() {
    if !Environment::is_open() {
        return;
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let gets = Arc::new(AtomicUsize::new(0));
    let legacy = gets.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = Url::parse(&format!("http://{}/mcp", listener.local_addr().unwrap())).unwrap();
    let router = Router::new().route(
        "/mcp",
        post(move |headers: HeaderMap| async move {
            assert_eq!(headers["authorization"], "Bearer fixture-service");
            count.fetch_add(1, Ordering::SeqCst);
            StatusCode::METHOD_NOT_ALLOWED
        })
        .get(move || async move {
            legacy.fetch_add(1, Ordering::SeqCst);
            StatusCode::NO_CONTENT
        }),
    );
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let catalog = catalogo_servicos::servico("planetscale").unwrap().tipo("planetscale.oficial_chave").unwrap();
    let account = serde_json::from_value(json!({
        "id":ACCOUNT,"name":"Fixture account","url":catalog.url,"servico":"planetscale","tipo":catalog.id,
        "liberacoes":["insights"],"space":crate::fork::spaces::PERSONAL,
        "headers":{"Authorization":"Bearer fixture-service"}
    }))
    .unwrap();
    let (hub, _ticket, proof) = fixture_with_account(account, "local");
    let proof = proof.unwrap();
    RuntimeIntent::capture_open(&hub, &proof, ACCOUNT).unwrap().snapshot(&hub).unwrap();
    let result = Box::pin(run(&hub, &proof, LIST, &json!({"account":ACCOUNT}), Some(endpoint))).await;
    task.abort();
    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(gets.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn mcp_open_stale_discovery_cannot_replace_or_invalidate_newer_inventory() {
    if !Environment::is_open() {
        return;
    }
    let endpoint = Url::parse("https://mcp.example.invalid/mcp").unwrap();
    let (hub, _ticket, proof) = fixture(&endpoint, "local", true);
    let proof = proof.unwrap();
    let account = market::connectors(&hub.store).unwrap().remove(0);
    let mut db = hub.store.db.locked();
    let earlier = ferramentas::iniciar_descoberta_open(&db, ACCOUNT).unwrap();
    let latest = ferramentas::iniciar_descoberta_open(&db, ACCOUNT).unwrap();
    let install = |db: &mut rusqlite::Connection, generation, description, cursor: Option<&str>| {
        let mut frame = json!({"result":{"tools":[
            {"name":"list_issues","description":description,"inputSchema":{"type":"object"}}
        ]}});
        if let Some(cursor) = cursor {
            frame["result"]["nextCursor"] = json!(cursor);
        }
        let expected = ferramentas::ler_ferramenta(&frame["result"]["tools"][0]).unwrap().digest;
        let mut mediated = json!({"message":frame});
        ferramentas::filtrar_lista_open(db, &proof.lane().chat, &account, &frame, &mut mediated, generation, |_| Ok(()))
            .map(|()| expected)
    };
    let expected = install(&mut db, latest, "Latest", None).unwrap();
    assert!(install(&mut db, earlier, "Stale", None).is_err());
    assert!(install(&mut db, earlier, "Stale partial", Some("page")).is_err());
    drop(db);
    let list = ferramentas::listar(&hub.store, &proof.lane().chat, ACCOUNT).unwrap();
    assert_eq!(list["inventarioPronto"], true);
    assert_eq!(list["ferramentas"][0]["digest"], expected);
}

#[tokio::test]
async fn mcp_open_refreshes_and_sends_approved_call_once_with_mediated_return() {
    if !Environment::is_open() {
        return;
    }
    let remote = server().await;
    let (hub, _ticket, proof) = fixture(&remote.url, "local", true);
    let proof = proof.unwrap();
    let args_list = json!({"account":ACCOUNT});
    let list = run(&hub, &proof, LIST, &args_list, Some(remote.url.clone())).await.unwrap();
    assert_eq!(list["tools"].as_array().unwrap().as_slice(), [] as [Value; 0]);
    link_fixture_tool(&hub, &proof);
    let list = run(&hub, &proof, LIST, &args_list, Some(remote.url.clone())).await.unwrap();
    assert_eq!(list["tools"].as_array().unwrap().len(), 1);
    assert_eq!(list["tools"][0]["name"], "list_issues");
    assert_eq!(list["provenance"]["origin"], "external");
    assert_eq!(remote.refreshed.load(Ordering::SeqCst), 1);
    let mut events = hub.events.subscribe();
    let mut saved = market::connectors(&hub.store).unwrap().remove(0);
    saved.oauth.as_mut().unwrap().access_token = OLD.into();
    saved.oauth.as_mut().unwrap().refresh_token = Some("fixture-old-refresh".into());
    market::save_connectors(&hub.store, &[saved]).unwrap();
    let args = json!({"account":ACCOUNT,"tool":"list_issues","arguments":{"query":"sample"}});
    let (result, ()) = tokio::join!(run(&hub, &proof, CALL, &args, Some(remote.url.clone())), async {
        let card = next_card(&hub, &mut events).await;
        assert_eq!(remote.sent.load(Ordering::SeqCst), 0);
        assert!(card.data["detail"].as_str().unwrap().contains("sample"));
        answer(&hub, &card, "allow");
    });
    let value = result.unwrap();
    let out = value.to_string();
    assert!(out.contains("fixture-result") && out.contains("fixture-structured"));
    for secret in [OLD, NEW, SESSION, "fixture-new-refresh", "fixture-query-part", remote.url.as_str()] {
        assert!(!out.contains(secret));
    }
    assert_eq!(remote.sent.load(Ordering::SeqCst), 1);
    let receipt = &value["result"]["_meta"]["donnabot/guard"]["receipt"];
    let (events, exposed) = hub.store.guard_test_snapshot().unwrap();
    assert_eq!(exposed, 1);
    let event = events
        .into_iter()
        .map(|line| serde_json::from_str::<Value>(&line).unwrap())
        .find(|event| event["receipt"] == *receipt)
        .unwrap();
    assert_eq!(event["session"], proof.lane().session_id());
    assert_eq!(event["turn"], proof.turn());
    assert_eq!(event["transport"], "http");
    assert!(event["gatewayContext"].is_null());
}

#[tokio::test]
async fn mcp_open_denies_wrong_origin_space_grant_and_revoked_card() {
    if !Environment::is_open() {
        return;
    }
    let remote = server().await;
    let (_hub, _ticket, proof) = fixture(&remote.url, "device", true);
    assert!(proof.is_none());
    let (hub, _ticket, proof) = fixture(&remote.url, "local", false);
    assert!(run(&hub, &proof.unwrap(), LIST, &json!({"account":ACCOUNT}), Some(remote.url.clone())).await.is_err());
    assert_eq!(remote.refreshed.load(Ordering::SeqCst), 0);
    let (hub, _ticket, proof) = fixture(&remote.url, "local", true);
    let proof = proof.unwrap();
    let mut denied = json!({"account":ACCOUNT,"tool":"delete_issue","arguments":{}});
    assert!(run(&hub, &proof, CALL, &denied, Some(remote.url.clone())).await.is_err());
    denied["tool"] = json!("list_issues");
    link_fixture_tool(&hub, &proof);
    for rejection in ["deny", "revoke", "cancel"] {
        let mut events = hub.events.subscribe();
        let (result, ()) = tokio::join!(run(&hub, &proof, CALL, &denied, Some(remote.url.clone())), async {
            let card = next_card(&hub, &mut events).await;
            match rejection {
                "deny" => answer(&hub, &card, "deny"),
                "revoke" => {
                    answer(&hub, &card, "allow");
                    ferramentas::gravar_desligamento(
                        &hub.store.db.locked(),
                        &proof.lane().chat,
                        ACCOUNT,
                        "list_issues",
                    )
                    .unwrap();
                }
                _ => {
                    answer(&hub, &card, "allow");
                    open_chat::stop(&proof.lane().chat);
                }
            }
        });
        assert!(result.is_err());
        if rejection == "revoke" {
            link_fixture_tool(&hub, &proof);
        }
    }
    assert_eq!(remote.sent.load(Ordering::SeqCst), 0);
}
