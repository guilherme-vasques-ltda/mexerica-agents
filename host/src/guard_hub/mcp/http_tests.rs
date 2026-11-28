#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use crate::{
    LockExt,
    api::devices::Caller,
    guard_hub::mcp::{self, contexto::Context, runtime},
    hub::Hub,
    remote::identity::Identity,
    store::{EntryKind, Lane, Store},
};
use axum::{Json, Router, body::Body, http::StatusCode, response::Response as AxumResponse, routing};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct Server {
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn server(router: Router) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Server { url, task }
}

struct Fixture {
    hub: Arc<Hub>,
    context: Context,
    bridge: String,
    dir: std::path::PathBuf,
}

impl Fixture {
    fn new(url: &str) -> Arc<Self> {
        Self::with_oauth(url, true)
    }

    fn with_oauth(url: &str, oauth: bool) -> Arc<Self> {
        Self::with_account(url, oauth, false)
    }

    fn with_account(url: &str, oauth: bool, planetscale: bool) -> Arc<Self> {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let bot = serde_json::from_value(json!({"id":"bot","name":"Fixture","connectors":["connector"]})).unwrap();
        store.save_bot(&bot).unwrap();
        let dir = std::env::temp_dir().join(format!("remote-mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Hub::new(store, "fixture".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
        let connector = serde_json::from_value(json!({"id":"connector","name":"Fixture","url":url,
            "headers":{"Authorization":"Bearer alphabeticaltoken"},
            "servico":planetscale.then_some("planetscale"),
            "tipo":planetscale.then_some("planetscale.oficial_chave"),
            "liberacoes":if planetscale {vec!["insights"]} else {vec![]},
            "oauth":if oauth {json!({"accessToken":"oldalphabeticaltoken","refreshToken":"oldrefreshalphabetical",
                "expiresAt":null})} else {Value::Null}}))
        .unwrap();
        crate::market::save_connectors(&hub.store, &[connector]).unwrap();
        let binding = crate::fork::sessoes::bindings::issue(&hub.store, "bot", "bot", "bot", None).unwrap();
        crate::fork::sessoes::bindings::activate(&hub.store, &binding, "session").unwrap();
        let canonical = crate::market::connectors(&hub.store).unwrap().remove(0);
        let context = crate::fork::conectores::oauth_binding::issue_context(
            &hub.store,
            &canonical,
            "bot",
            "bot",
            None,
            binding.id.as_str(),
        )
        .unwrap();
        hub.start_bot_turn("bot", |r| {
            r.session = Some("session".into());
            r.lane = Some(Lane::main("bot"));
        })
        .unwrap();
        Arc::new(Self { hub, context, bridge: uuid::Uuid::new_v4().to_string(), dir })
    }

    async fn call(&self, action: &str, extra: Value) -> Result<Value> {
        let mut body =
            json!({"context":self.context.id,"connector":"connector","transport":"http","bridgeId":self.bridge});
        body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        mcp::call(&self.hub, &Caller::Local, action, body).await
    }

    fn mediation(self: &Arc<Self>) -> Guard {
        Guard { fixture: self.clone(), acquired: false, deny: false, streamable_only: false }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Guard {
    fixture: Arc<Fixture>,
    acquired: bool,
    deny: bool,
    streamable_only: bool,
}

impl Mediation for Guard {
    async fn target(&mut self, method: &str, stale: bool) -> Result<Target> {
        let target = self
            .fixture
            .call("donnaGuardRuntime", json!({"method":method,"stale":stale,"refreshLease":self.acquired}))
            .await?;
        self.acquired = true;
        let mut headers = HeaderMap::new();
        for (key, value) in target["headers"].as_object().unwrap() {
            headers.insert(
                reqwest::header::HeaderName::from_bytes(key.as_bytes()).unwrap(),
                reqwest::header::HeaderValue::from_str(value.as_str().unwrap()).unwrap(),
            );
        }
        Ok(Target {
            url: target["url"].as_str().unwrap().parse().unwrap(),
            headers,
            streamable_only: self.streamable_only || target["streamableOnly"].as_bool().unwrap(),
        })
    }

    async fn observe(&mut self, frame: Value, method: &str) -> Result<Option<Value>> {
        if self.deny {
            bail!("fixture: observe indisponível");
        }
        let answer = self.fixture.call("donnaGuardObserve", json!({"frame":frame,"method":method})).await?;
        // Prova na fronteira de entrega, não depois de o consumidor já ter recebido.
        let (events, _) = self.fixture.hub.store.guard_test_snapshot().unwrap();
        assert_ne!(events, [] as [String; 0]);
        if answer["message"].is_object() {
            let message = &answer["message"];
            let receipt = if message.get("result").is_some() {
                &message["result"]["_meta"]["donnabot/guard"]["receipt"]
            } else {
                &message["error"]["data"]["_meta"]["donnabot/guard"]["receipt"]
            };
            assert!(events.iter().any(|event| {
                let event: Value = serde_json::from_str(event).unwrap();
                event["receipt"] == *receipt && event["transport"] == "http"
            }));
            assert!(self.fixture.hub.store.guard_external_exposed("bot").unwrap());
            let marked: i64 = self
                .fixture
                .hub
                .store
                .db
                .locked()
                .query_row(
                    "SELECT (SELECT COUNT(*) FROM donna_session_gateway_pending WHERE bot='bot') +
                    (SELECT COUNT(*) FROM donna_memory_sessions WHERE bot='bot' AND marked_at IS NOT NULL)",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(marked, 1);
        }
        Ok(answer.get("message").filter(|m| m.is_object()).cloned())
    }
}

fn request(id: i64, method: &str) -> Value {
    let mut frame = json!({"jsonrpc":"2.0","id":id,"method":method,"params":{"capabilities":{"sampling":{},"roots":{},"elicitation":{}}}});
    if method == "initialize" {
        frame["params"]["protocolVersion"] = json!(crate::mcp::PROTOCOL_VERSION);
    }
    frame
}

fn reply(frame: &Value) -> Value {
    let result = match frame["method"].as_str().unwrap() {
        "initialize" => {
            assert_eq!(frame["params"]["capabilities"], json!({}));
            let mut result = json!({"protocolVersion":crate::mcp::PROTOCOL_VERSION,"capabilities":{"tools":{}},"instructions":"alphabeticaltoken"});
            // Fixture M1: versão escolhida pelo "fornecedor"; "omit" retira o campo.
            match frame["params"].get("fixtureVersion") {
                Some(version) if version == "omit" => {
                    result.as_object_mut().unwrap().remove("protocolVersion");
                }
                Some(version) => result["protocolVersion"] = version.clone(),
                None => {}
            }
            result
        }
        "tools/list" => {
            json!({"tools":[{"name":"fixture","description":"alphabeticaltoken oldalphabeticaltoken newalphabeticaltoken oldrefreshalphabetical","inputSchema":{"type":"object"}}]})
        }
        _ => {
            json!({"content":[{"type":"text","text":"ação 🦀 alphabeticaltoken oldalphabeticaltoken newalphabeticaltoken oldrefreshalphabetical"}]})
        }
    };
    json!({"jsonrpc":"2.0","id":frame["id"],"result":result})
}

fn sse(value: &Value) -> String {
    format!("data: {value}\r\n\r\n")
}

#[tokio::test]
async fn http_json_e_streamable_sse_persistem_descoberta_resultado_e_memoria_antes_de_liberar() {
    let remote = server(Router::new().route(
        "/mcp",
        routing::post(|Json(frame): Json<Value>| async move {
            let value = reply(&frame);
            if frame["method"] == "tools/call" {
                let chunks: Vec<_> =
                    sse(&value).as_bytes().chunks(2).map(|chunk| Ok::<_, std::io::Error>(chunk.to_vec())).collect();
                AxumResponse::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::from_stream(futures::stream::iter(chunks)))
                    .unwrap()
            } else {
                AxumResponse::new(Body::from(value.to_string()))
            }
        }),
    ))
    .await;
    let fixture = Fixture::new(&remote.url);
    let mut session = Session::new(fixture.mediation()).unwrap();
    let init = session.exchange(request(1, "initialize")).await.unwrap().unwrap();
    assert_eq!(init["id"], 1);
    assert!(init["result"].get("instructions").is_none());
    crate::market::update_connectors(&fixture.hub.store, |all| {
        all[0].oauth.as_mut().unwrap().access_token = "newalphabeticaltoken".into();
        Ok(())
    })
    .unwrap();
    for method in ["tools/list", "tools/call"] {
        let result = session.exchange(request(2, method)).await.unwrap().unwrap();
        for secret in ["alphabeticaltoken", "oldalphabeticaltoken", "newalphabeticaltoken", "oldrefreshalphabetical"] {
            assert!(!result.to_string().contains(secret));
        }
        assert_eq!(result["id"], 2);
        assert!(result["result"]["_meta"]["donnabot/guard"]["receipt"].is_string());
        assert_eq!(result["result"]["_meta"]["donnabot/guard"]["origin"], "external");
        if method == "tools/list" {
            memory_without_owner_transport_is_refused(&fixture).await;
        } else {
            assert!(result["result"]["content"][0]["text"].as_str().unwrap().contains("ação 🦀"));
        }
    }
}

async fn memory_without_owner_transport_is_refused(fixture: &Fixture) {
    fixture.hub.store.memory_session_start("bot", "session", false).unwrap();
    crate::fork::memory::gateway::bind(&fixture.hub, "bot", Some("session")).unwrap();
    let entry = fixture
        .hub
        .add_entry(&Lane::main("bot"), EntryKind::User, 1, &json!({"text":"Guarde o fato descoberto"}))
        .unwrap();
    fixture.hub.set_runtime("bot", |r| r.message = Some(entry.id));
    let permission = fixture.hub.store.bot("bot").unwrap().unwrap().config.permission;
    assert!(fixture.hub.store.memory_session_marked("bot", "session").unwrap());
    assert!(fixture.hub.runtime("bot").memory_turn_token.is_some());
    let memory =
        crate::chat::memory::tool_call(&fixture.hub, "bot", "remember", &json!({"text":"Fato externo sintético"}))
            .await
            .unwrap_err();
    assert!(memory.to_string().contains("current memory transport"));
    assert_eq!(fixture.hub.store.memory_read(|db| codync_mem::store::layer(db, Some("bot"))).unwrap().len(), 0);
    assert_eq!(fixture.hub.store.memory_read(|db| codync_mem::store::layer(db, None)).unwrap().len(), 0);
    assert_eq!(fixture.hub.store.bot("bot").unwrap().unwrap().config.permission, permission);
    let db = fixture.hub.store.db.locked();
    for table in [
        "donna_memory_auto_jobs",
        "donna_memory_auto_proposals",
        "donna_memory_auto_receipts",
        "donna_memory_auto_non_saved",
    ] {
        assert_eq!(
            db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get::<_, i64>(0)).unwrap(),
            0,
            "{table}"
        );
    }
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM entries WHERE kind='notice' AND json_extract(data,'$.memory') IS NOT NULL",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn legado_inspeciona_post_erro_notificacao_e_pedido_reverso_sem_entregar_raw() {
    use futures::{StreamExt, stream};
    let observed = Arc::new(AtomicUsize::new(0));
    let calls = observed.clone();
    let protocol = Arc::new(AtomicUsize::new(0));
    let protocol_replies = protocol.clone();
    let (events, _) = tokio::sync::broadcast::channel::<String>(8);
    let stream_events = events.clone();
    let remote = server(Router::new().route("/mcp", routing::post(|| async { StatusCode::METHOD_NOT_ALLOWED })
        .get(move || {
            let receiver = stream_events.subscribe();
            async move {
                let endpoint = stream::once(async { Ok::<_, std::io::Error>("event: endpoint\ndata: /post\n\n".to_owned()) });
                let replies = stream::unfold(receiver, |mut receiver| async move {
                    receiver.recv().await.ok().map(|event| (Ok::<_, std::io::Error>(event), receiver))
                });
                AxumResponse::builder().header("content-type", "text/event-stream")
                    .body(Body::from_stream(endpoint.chain(replies))).unwrap()
            }
        }))
        .route("/post", routing::post(move |Json(frame): Json<Value>| {
            let calls = calls.clone();
            let protocol = protocol_replies.clone();
            let events = events.clone();
            async move {
                if frame.get("method").is_none() {
                    if frame["id"] == "ping" { assert_eq!(frame["result"], json!({})); }
                    else { assert_eq!(frame["error"]["code"], -32601); }
                    protocol.fetch_add(1, Ordering::SeqCst);
                    return AxumResponse::builder().status(202).body(Body::empty()).unwrap();
                }
                calls.fetch_add(1, Ordering::SeqCst);
                if frame["method"] == "tools/list" {
                    events.send(sse(&reply(&frame))).unwrap();
                    return AxumResponse::new(Body::from(json!({"jsonrpc":"2.0","method":"notifications/message","params":{"level":"info","data":"alphabeticaltoken"}}).to_string()));
                }
                if frame["method"] == "tools/call" {
                    return AxumResponse::new(Body::from(json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-32603,"message":"alphabeticaltoken","data":"oldalphabeticaltoken"}}).to_string()));
                }
                let noise = sse(&json!({"jsonrpc":"2.0","method":"notifications/message","params":{"level":"info","data":"alphabeticaltoken"}}))
                    + &sse(&json!({"id":"ping","method":"ping"}))
                    + &sse(&json!({"id":"reverse","method":"sampling/createMessage","params":{"text":"alphabeticaltoken"}}))
                    + &sse(&json!({"id":999,"result":{"content":[{"type":"text","text":"alphabeticaltoken"}]}}));
                AxumResponse::builder().header("content-type", "text/event-stream")
                    .body(Body::from(noise + &sse(&reply(&frame)))).unwrap()
            }
        }))).await;
    let fixture = Fixture::new(&remote.url);
    let mut session = Session::new(fixture.mediation()).unwrap();
    session.exchange(request(1, "initialize")).await.unwrap();
    let discovery = session.exchange(request(2, "tools/list")).await.unwrap().unwrap();
    assert_eq!(discovery["result"]["tools"].as_array().unwrap().len(), 1);
    let result = session.exchange(request(3, "tools/call")).await.unwrap().unwrap();
    assert!(result.get("error").is_some());
    assert!(!result.to_string().contains("alphabeticaltoken"));
    assert!(!result.to_string().contains("reverse"));
    assert_eq!(observed.load(Ordering::SeqCst), 3);
    assert_eq!(protocol.load(Ordering::SeqCst), 2);
    let (events, _) = fixture.hub.store.guard_test_snapshot().unwrap();
    assert!(events.iter().any(|e| e.contains("server-request")));
    assert!(events.iter().any(|e| e.contains("notification")));
    assert!(events.iter().all(|e| !e.contains("alphabeticaltoken")));
}

#[tokio::test]
async fn planetscale_recusa_recursos_e_prompts_antes_de_entregar_alvo() {
    let url = "https://mcp.pscale.dev/mcp/planetscale-insights-only";
    let fixture = Fixture::with_account(url, false, true);
    for mcp_method in ["resources/list", "resources/templates/list", "resources/read", "prompts/list", "prompts/get"] {
        let denied =
            fixture.call("donnaGuardRuntime", json!({"method":mcp_method,"streamableOnly":false})).await.unwrap();
        assert_eq!(denied, json!({"ruleDenied":true}));
    }
    let allowed = fixture.call("donnaGuardRuntime", json!({"method":"initialize"})).await.unwrap();
    assert_eq!(allowed["url"], url);
    assert_eq!(allowed["streamableOnly"], true);
    assert!(allowed["headers"].get("Authorization").is_some());
}

#[tokio::test]
async fn perfil_streamable_nao_envia_header_a_outro_caminho_na_mesma_origem() {
    for status in [StatusCode::NOT_FOUND, StatusCode::METHOD_NOT_ALLOWED, StatusCode::TEMPORARY_REDIRECT] {
        let forbidden = Arc::new(AtomicUsize::new(0));
        let get = forbidden.clone();
        let post = forbidden.clone();
        let remote = server(
            Router::new()
                .route(
                    "/mcp",
                    routing::post(move || async move { (status, [("location", "/outro")]) }).get(move || {
                        get.fetch_add(1, Ordering::SeqCst);
                        async { ([("content-type", "text/event-stream")], "event: endpoint\ndata: /outro\n\n") }
                    }),
                )
                .route(
                    "/outro",
                    routing::any(move || {
                        post.fetch_add(1, Ordering::SeqCst);
                        async { StatusCode::NO_CONTENT }
                    }),
                ),
        )
        .await;
        let fixture = Fixture::with_oauth(&remote.url, false);
        let mut guard = fixture.mediation();
        guard.streamable_only = true;
        assert!(Session::new(guard).unwrap().exchange(request(1, "initialize")).await.is_err());
        assert_eq!(forbidden.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn origem_estrangeira_e_redirect_nao_recebem_authorization_e_tools_call_nao_repete() {
    let received = Arc::new(AtomicUsize::new(0));
    let count = received.clone();
    let foreign = server(Router::new().route(
        "/mcp",
        routing::any(move || {
            count.fetch_add(1, Ordering::SeqCst);
            async { "nao deveria chegar" }
        }),
    ))
    .await;
    let endpoint = foreign.url.clone();
    let remote = server(Router::new().route(
        "/mcp",
        routing::post(|| async { StatusCode::METHOD_NOT_ALLOWED }).get(move || {
            let endpoint = endpoint.clone();
            async move { ([("content-type", "text/event-stream")], format!("event: endpoint\ndata: {endpoint}\n\n")) }
        }),
    ))
    .await;
    let fixture = Fixture::new(&remote.url);
    assert!(Session::new(fixture.mediation()).unwrap().exchange(request(1, "initialize")).await.is_err());
    let destination = foreign.url.clone();
    let redirects = server(Router::new().route(
        "/mcp",
        routing::post(move || {
            let destination = destination.clone();
            async move { (StatusCode::TEMPORARY_REDIRECT, [("location", destination)], "alphabeticaltoken") }
        }),
    ))
    .await;
    let fixture = Fixture::new(&redirects.url);
    assert!(Session::new(fixture.mediation()).unwrap().exchange(request(1, "initialize")).await.is_err());
    assert_eq!(received.load(Ordering::SeqCst), 0);
    let posts = Arc::new(AtomicUsize::new(0));
    let count = posts.clone();
    let remote = server(Router::new().route(
        "/mcp",
        routing::post(move |Json(frame): Json<Value>| {
            let count = count.clone();
            async move {
                if frame["method"] == "initialize" {
                    (StatusCode::OK, Json(reply(&frame)))
                } else {
                    count.fetch_add(1, Ordering::SeqCst);
                    (StatusCode::UNAUTHORIZED, Json(json!({"error":"alphabeticaltoken"})))
                }
            }
        }),
    ))
    .await;
    let fixture = Fixture::new(&remote.url);
    let mut session = Session::new(fixture.mediation()).unwrap();
    session.exchange(request(1, "initialize")).await.unwrap();
    assert!(session.exchange(request(2, "tools/call")).await.is_err());
    assert_eq!(posts.load(Ordering::SeqCst), 1);
}

#[test]
fn parser_limita_bytes_frames_e_preserva_utf8_e_crlf_divididos() {
    let wire = "data: {\"text\":\"ação 🦀\"}\r\n\r\n";
    let mut parser = eventos::Parser::default();
    for byte in wire.as_bytes() {
        parser.push(&[*byte]).unwrap();
    }
    let event = parser.pop().unwrap().unwrap();
    let frame: Value = serde_json::from_str(&eventos::text(&event)).unwrap();
    assert_eq!(frame["text"], "ação 🦀");
    assert!(parser.pop().unwrap().is_none());
    parser.push(&vec![b'x'; MAX_FRAME]).unwrap();
    assert!(parser.push(b"x").is_err());
    assert!(method(&json!([request(1, "tools/call")])).is_err());
    assert!(method(&json!({"jsonrpc":"2.0","method":"tools/call"})).is_err());
}

#[tokio::test]
async fn falhas_de_contexto_configuracao_cache_cofre_recibo_e_exposicao_retem_retorno() {
    let remote = server(
        Router::new().route("/mcp", routing::post(|Json(frame): Json<Value>| async move { Json(reply(&frame)) })),
    )
    .await;
    // Cada fixture representa uma fronteira distinta, sem mexer no cofre do SO.
    let fixture = Fixture::new(&remote.url);
    let mut guard = fixture.mediation();
    guard.target("initialize", false).await.unwrap();
    let mut body = json!({"method":"tools/call","frame":{"id":1,"result":{"content":[{"type":"text","text":"alphabeticaltoken"}]}}});
    fixture.hub.set_runtime("bot", |r| r.lane = None);
    assert!(fixture.call("donnaGuardObserve", body.clone()).await.is_err());
    fixture.hub.set_runtime("bot", |r| r.lane = Some(Lane::main("other")));
    assert!(fixture.call("donnaGuardObserve", body.clone()).await.is_err());
    fixture.hub.set_runtime("bot", |r| r.lane = Some(Lane::main("bot")));
    crate::market::update_connectors(&fixture.hub.store, |all| {
        all[0].headers.insert("x-key".into(), "newheader".into());
        Ok(())
    })
    .unwrap();
    assert!(fixture.call("donnaGuardObserve", body.clone()).await.is_err());
    assert!(guard.target("tools/call", false).await.is_err());
    let fixture = Fixture::new(&remote.url);
    let mut guard = fixture.mediation();
    guard.target("initialize", false).await.unwrap();
    let namespace = fixture.hub.store.guard_runtime_namespace().unwrap();
    runtime::release(&namespace, "connector", &fixture.context.id, Some(&fixture.bridge));
    assert!(fixture.call("donnaGuardObserve", body.clone()).await.is_err());
    assert!(guard.target("tools/call", false).await.is_err());
    let fixture = Fixture::new(&remote.url);
    let mut guard = fixture.mediation();
    guard.target("initialize", false).await.unwrap();
    *fixture.hub.store.secret_key.locked() = None;
    assert!(fixture.call("donnaGuardObserve", body.clone()).await.is_err());
    let fixture = Fixture::new(&remote.url);
    let mut guard = fixture.mediation();
    guard.target("initialize", false).await.unwrap();
    fixture
        .hub
        .store
        .db
        .locked()
        .execute_batch(
            "CREATE TRIGGER fail_guard BEFORE INSERT ON donna_guard_events BEGIN SELECT RAISE(ABORT,'fixture'); END;",
        )
        .unwrap();
    assert!(fixture.call("donnaGuardObserve", body.clone()).await.is_err());
    let fixture = Fixture::new(&remote.url);
    let mut guard = fixture.mediation();
    guard.target("initialize", false).await.unwrap();
    fixture.hub.store.db.locked().execute_batch("CREATE TRIGGER fail_exposure BEFORE INSERT ON donna_memory_sessions BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(fixture.call("donnaGuardObserve", body.clone()).await.is_err());
    let fixture = Fixture::new(&remote.url);
    let mut guard = fixture.mediation();
    guard.target("initialize", false).await.unwrap();
    fixture.hub.store.guard_context_remove(&fixture.context.id).unwrap();
    assert!(fixture.call("donnaGuardObserve", body.clone()).await.is_err());
    let fixture = Fixture::new(&remote.url);
    let mut guard = fixture.mediation();
    guard.target("initialize", false).await.unwrap();
    let mut bot = fixture.hub.store.bot("bot").unwrap().unwrap().config;
    bot.connectors.clear();
    fixture.hub.store.save_bot(&bot).unwrap();
    assert!(fixture.call("donnaGuardObserve", body.clone()).await.is_err());
    let fixture = Fixture::new(&remote.url);
    let context = Context::issue(&fixture.hub.store, "bot", "connector", "bot", Some("dead-run")).unwrap();
    body["context"] = json!(context.id);
    assert!(fixture.call("donnaGuardRuntime", body).await.is_err());
    let fixture = Fixture::new(&remote.url);
    let mut guard = fixture.mediation();
    guard.deny = true;
    let mut session = Session::new(guard).unwrap();
    assert!(session.exchange(request(1, "initialize")).await.is_err());
}

#[tokio::test]
async fn setup_e_getter_sem_contexto_nao_liberam_tools_call_nem_criam_bot() {
    let remote = server(
        Router::new().route("/mcp", routing::post(|Json(frame): Json<Value>| async move { Json(reply(&frame)) })),
    )
    .await;
    let fixture = Fixture::new(&remote.url);
    let setup = mcp::setup::Setup::issue(&fixture.hub.store, "connector").unwrap();
    let before = fixture.hub.store.bots().unwrap().len();
    for method in ["initialize", "tools/list", "ping"] {
        let answer = fixture.call("donnaGuardRuntime", json!({"context":setup.id,"method":method})).await.unwrap();
        assert_eq!(answer["setup"], true);
    }
    for action in ["donnaGuardRuntime", "donnaGuardObserve"] {
        assert!(
            fixture
                .call(action, json!({"context":setup.id,"method":"tools/call","frame":{"id":1,"result":{}}}))
                .await
                .is_err()
        );
    }
    assert_eq!(fixture.hub.store.bots().unwrap().len(), before);
    let body = json!({"id":"connector"});
    assert!(crate::api::dispatch(&fixture.hub, &Caller::Local, "connectorTarget", body).await.is_err());
    let device = Caller::Device { key: "fixture".into(), scopes: vec![crate::store::Scope::Control], autoridade: None };
    assert!(mcp::call(&fixture.hub, &device, "donnaGuardRuntime", json!({"context":setup.id,"connector":"connector","transport":"http","method":"initialize","bridgeId":fixture.bridge})).await.is_err());
    let id = setup.id.clone();
    drop(setup);
    assert!(fixture.call("donnaGuardRuntime", json!({"context":id,"method":"initialize"})).await.is_err());
}

#[tokio::test]
async fn batch_id_forjado_e_limite_de_frames_nao_viram_resposta_do_pedido() {
    let remote = server(Router::new().route(
        "/mcp",
        routing::post(|Json(frame): Json<Value>| async move {
            if frame["method"] == "initialize" {
                Json(reply(&frame))
            } else {
                Json(json!([reply(&frame), reply(&frame)]))
            }
        }),
    ))
    .await;
    let fixture = Fixture::new(&remote.url);
    let mut session = Session::new(fixture.mediation()).unwrap();
    session.exchange(request(1, "initialize")).await.unwrap();
    assert!(session.exchange(request(2, "tools/call")).await.is_err());
    let remote = server(Router::new().route(
        "/mcp",
        routing::post(|| async {
            let noise = sse(&json!({"id":"forged","result":{"content":[{"type":"text","text":"alphabeticaltoken"}]}}));
            ([("content-type", "text/event-stream")], noise.repeat(65))
        }),
    ))
    .await;
    let fixture = Fixture::new(&remote.url);
    assert!(Session::new(fixture.mediation()).unwrap().exchange(request(1, "initialize")).await.is_err());
    let remote = server(Router::new().route("/mcp", routing::post(|| async { "x".repeat(MAX_FRAME + 1) }))).await;
    let fixture = Fixture::new(&remote.url);
    assert!(Session::new(fixture.mediation()).unwrap().exchange(request(1, "initialize")).await.is_err());
}

struct Blocked {
    guard: Guard,
    arrived: Arc<tokio::sync::Notify>,
}

impl Mediation for Blocked {
    async fn target(&mut self, method: &str, stale: bool) -> Result<Target> {
        self.guard.target(method, stale).await
    }
    async fn observe(&mut self, _frame: Value, _method: &str) -> Result<Option<Value>> {
        self.arrived.notify_one();
        std::future::pending().await
    }
}

#[tokio::test]
async fn cancelamento_e_eof_interrompem_rede_e_observe_sem_aguardar_timeout() {
    let arrived = Arc::new(tokio::sync::Notify::new());
    let notify = arrived.clone();
    let remote = server(Router::new().route(
        "/mcp",
        routing::post(move || {
            let notify = notify.clone();
            async move {
                notify.notify_one();
                std::future::pending::<String>().await
            }
        }),
    ))
    .await;
    for cancel in [false, true] {
        let fixture = Fixture::new(&remote.url);
        let mut session = Session::new(fixture.mediation()).unwrap();
        let (mut engine, proxy) = tokio::io::duplex(8192);
        let (input, output) = tokio::io::split(proxy);
        engine.write_all(format!("{}\n", request(1, "initialize")).as_bytes()).await.unwrap();
        let task = tokio::spawn(async move { super::super::ponte_remota::relay(input, output, &mut session).await });
        tokio::time::timeout(Duration::from_secs(2), arrived.notified()).await.unwrap();
        if cancel {
            engine
                .write_all(
                    b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\"params\":{\"requestId\":1}}\n",
                )
                .await
                .unwrap();
        } else {
            engine.shutdown().await.unwrap();
        }
        tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap().unwrap();
    }
}

#[tokio::test]
async fn id_duplicado_nao_abre_segundo_post_e_eof_interrompe_observe_bloqueado() {
    let arrived = Arc::new(tokio::sync::Notify::new());
    let remote = server(
        Router::new().route("/mcp", routing::post(|Json(frame): Json<Value>| async move { Json(reply(&frame)) })),
    )
    .await;
    let fixture = Fixture::new(&remote.url);
    let guard = Blocked { guard: fixture.mediation(), arrived: arrived.clone() };
    let mut session = Session::new(guard).unwrap();
    let (engine, proxy) = tokio::io::duplex(8192);
    let (input, output) = tokio::io::split(proxy);
    let (engine_in, mut engine_out) = tokio::io::split(engine);
    let mut lines = BufReader::new(engine_in).lines();
    let wire = format!("{}\n", request(1, "initialize"));
    engine_out.write_all(wire.as_bytes()).await.unwrap();
    let task = tokio::spawn(async move { super::super::ponte_remota::relay(input, output, &mut session).await });
    tokio::time::timeout(Duration::from_secs(2), arrived.notified()).await.unwrap();
    engine_out.write_all(wire.as_bytes()).await.unwrap();
    let error = tokio::time::timeout(Duration::from_secs(2), lines.next_line()).await.unwrap().unwrap().unwrap();
    assert!(error.contains("ID repetido"));
    assert!(!error.contains("alphabeticaltoken"));
    engine_out.shutdown().await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap().unwrap();
}

#[tokio::test]
async fn status_401_texto_e_fallback_html_sao_retidos_sem_impedir_negociacao() {
    use futures::{StreamExt, stream};
    let posts = Arc::new(AtomicUsize::new(0));
    let count = posts.clone();
    let remote = server(
        Router::new()
            .route(
                "/mcp",
                routing::post(move || {
                    let count = count.clone();
                    async move {
                        if count.fetch_add(1, Ordering::SeqCst) == 0 {
                            (StatusCode::UNAUTHORIZED, "alphabeticaltoken")
                        } else {
                            (StatusCode::METHOD_NOT_ALLOWED, "<html>alphabeticaltoken</html>")
                        }
                    }
                })
                .get(|| async {
                    let endpoint = stream::once(async { Ok::<_, std::io::Error>("event: endpoint\ndata: /post\n\n") });
                    AxumResponse::builder()
                        .header("content-type", "text/event-stream")
                        .body(Body::from_stream(endpoint.chain(stream::pending())))
                        .unwrap()
                }),
            )
            .route("/post", routing::post(|Json(frame): Json<Value>| async move { Json(reply(&frame)) })),
    )
    .await;
    // Este caso é manual por header desde a criação, sem remover necessidade de OAuth existente.
    let fixture = Fixture::with_oauth(&remote.url, false);
    let result = Session::new(fixture.mediation()).unwrap().exchange(request(1, "initialize")).await.unwrap().unwrap();
    assert_eq!(result["id"], 1);
    assert!(!result.to_string().contains("alphabeticaltoken"));
    assert_eq!(posts.load(Ordering::SeqCst), 2);
    assert!(fixture.hub.store.guard_test_snapshot().unwrap().0.len() >= 4);
}

#[tokio::test]
async fn ping_reverso_streamable_responde_depois_de_observar_sem_entregar_ao_motor() {
    use futures::{StreamExt, stream};
    let acknowledged = Arc::new(tokio::sync::Notify::new());
    let remote = server(Router::new().route(
        "/mcp",
        routing::post(move |headers: HeaderMap, Json(frame): Json<Value>| {
            let acknowledged = acknowledged.clone();
            async move {
                let accept = headers.get("accept").unwrap().to_str().unwrap();
                assert!(accept.contains("application/json") && accept.contains("text/event-stream"));
                if frame.get("method").is_none() {
                    assert_eq!(frame["id"], "ping");
                    assert_eq!(frame["result"], json!({}));
                    // Ping antes do resultado aceito: nenhuma versão MCP negociada.
                    assert!(headers.get("mcp-protocol-version").is_none());
                    acknowledged.notify_one();
                    return AxumResponse::builder().status(202).body(Body::empty()).unwrap();
                }
                let ping = stream::once(async { Ok::<_, std::io::Error>(sse(&json!({"id":"ping","method":"ping"}))) });
                let result = stream::once(async move {
                    acknowledged.notified().await;
                    Ok::<_, std::io::Error>(sse(&reply(&frame)))
                });
                AxumResponse::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::from_stream(ping.chain(result)))
                    .unwrap()
            }
        }),
    ))
    .await;
    let fixture = Fixture::new(&remote.url);
    let mut session = Session::new(fixture.mediation()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), session.exchange(request(1, "initialize")))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(result["id"], 1);
    assert!(result["result"].is_object());
    assert!(fixture.hub.store.guard_test_snapshot().unwrap().0.iter().any(|e| e.contains("server-request")));
}

#[tokio::test]
async fn initialized_seguido_de_descoberta_preserva_readiness_com_fila_limitada() {
    let remote = server(Router::new().route(
        "/mcp",
        routing::post(|Json(frame): Json<Value>| async move {
            if frame["method"] == "notifications/initialized" {
                AxumResponse::builder().status(202).body(Body::empty()).unwrap()
            } else {
                AxumResponse::new(Body::from(reply(&frame).to_string()))
            }
        }),
    ))
    .await;
    let fixture = Fixture::new(&remote.url);
    let mut session = Session::new(fixture.mediation()).unwrap();
    let (engine, proxy) = tokio::io::duplex(8192);
    let (input, output) = tokio::io::split(proxy);
    let (engine_in, mut engine_out) = tokio::io::split(engine);
    let mut lines = BufReader::new(engine_in).lines();
    let task = tokio::spawn(async move { super::super::ponte_remota::relay(input, output, &mut session).await });
    engine_out.write_all(format!("{}\n", request(1, "initialize")).as_bytes()).await.unwrap();
    let init = tokio::time::timeout(Duration::from_secs(3), lines.next_line()).await.unwrap().unwrap().unwrap();
    assert_eq!(serde_json::from_str::<Value>(&init).unwrap()["id"], 1);
    engine_out
        .write_all(
            format!("{{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}}\n{}\n", request(2, "tools/list"))
                .as_bytes(),
        )
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), lines.next_line()).await.unwrap().unwrap().unwrap();
    assert!(serde_json::from_str::<Value>(&result).unwrap()["result"]["tools"].is_array());
    engine_out.shutdown().await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap().unwrap();
}

#[path = "http_regressoes.rs"]
mod regressoes;

#[path = "http_versoes.rs"]
mod versoes;
