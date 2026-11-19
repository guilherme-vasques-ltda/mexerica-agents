//! D70: fluxo real HTTP do Hub e provedor sintético local; barreiras, sem sleep.
//! MODIFICADO pela DONNA Bot em 2026-11-04: casos de renovação adaptados de
//! host/src/market/oauth/tests.rs, Codync e82c417424e06d1da71a865845f230d665b59ff1.
//! leepokai; Co-Authored-By: Claude Opus 5.5. Apache-2.0; `THIRD_PARTY.md`.
//! Fixtures/guardas e provas canônicas próprias da Mexerica; sem matriz ampla.
use crate::{api, hub::Hub, market, remote::identity::Identity, store::Store};
use axum::{Json, Router, http::StatusCode, routing};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use tokio::sync::Semaphore;

pub(crate) struct Gate {
    armed: AtomicBool,
    entered: Semaphore,
    release: Semaphore,
}
impl Gate {
    fn new() -> Arc<Self> {
        Arc::new(Self { armed: AtomicBool::new(false), entered: Semaphore::new(0), release: Semaphore::new(0) })
    }
    fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }
    async fn cross(&self) {
        if self.armed.swap(false, Ordering::SeqCst) {
            self.entered.add_permits(1);
            self.release.acquire().await.unwrap().forget();
        }
    }
    pub(crate) async fn wait(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(20), self.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
    pub(crate) fn open(&self) {
        self.release.add_permits(1);
    }
}
type ProbeFrames = Arc<std::sync::Mutex<Vec<(axum::http::HeaderMap, Value)>>>;
type RawChallenges = Arc<std::sync::Mutex<Option<Vec<axum::http::HeaderValue>>>>;
type InitialReply = Arc<std::sync::Mutex<Option<(StatusCode, Vec<String>)>>>;
type Redirects = Arc<std::sync::Mutex<std::collections::HashMap<String, (StatusCode, String, Value)>>>;
struct Fixture {
    hub: Arc<Hub>,
    api: String,
    provider: String,
    discovery: Arc<Gate>,
    initial: InitialReply,
    raw_challenges: RawChallenges,
    probe_frames: ProbeFrames,
    probe: Arc<Gate>,
    registration: Arc<Gate>,
    registration_body: Arc<Gate>,
    registration_reply: Arc<std::sync::Mutex<Value>>,
    registration_frames: ProbeFrames,
    token: Arc<Gate>,
    token_calls: Arc<AtomicUsize>,
    requests: Arc<AtomicUsize>,
    token_failure: Arc<AtomicBool>,
    token_temporary: Arc<AtomicBool>,
    token_reply: Arc<std::sync::Mutex<Option<(StatusCode, Value)>>>,
    token_forms: Arc<std::sync::Mutex<Vec<String>>>,
    /// M2-A: caminho → resposta 3xx com `Location`; `paths` registra a ordem dos pedidos.
    redirects: Redirects,
    paths: Arc<std::sync::Mutex<Vec<String>>>,
    issuer: Arc<std::sync::Mutex<String>>,
    snapshots: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Value>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    dir: std::path::PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
/// M2-B: os metadados declaram o `issuer` exato que a descoberta escolheu.
/// M3-A: e declaram PKCE S256; sem isso o login recusaria antes do cadastro.
fn as_metadata(
    issuer: String,
    base: String,
) -> impl Fn() -> std::future::Ready<Json<Value>> + Clone + Send + Sync + 'static {
    move || {
        std::future::ready(Json(json!({
            "issuer":issuer,
            "authorization_endpoint":format!("{base}/authorize"),
            "token_endpoint":format!("{base}/token"),
            "registration_endpoint":format!("{base}/register"),
            "code_challenge_methods_supported":["S256"]
        })))
    }
}
impl Fixture {
    async fn new() -> Arc<Self> {
        Self::with_disk(false).await
    }
    async fn with_disk(disk: bool) -> Arc<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider = format!("http://{}", listener.local_addr().unwrap());
        let discovery = Gate::new();
        let probe = Gate::new();
        let initial = Arc::new(std::sync::Mutex::new(None::<(StatusCode, Vec<String>)>));
        let raw_challenges: RawChallenges = Arc::default();
        let raw_reply = raw_challenges.clone();
        let probe_frames: ProbeFrames = Arc::default();
        let (probe_gate, initial_reply, frames) = (probe.clone(), initial.clone(), probe_frames.clone());
        let registration = Gate::new();
        let registration_body = Gate::new();
        let registration_reply = Arc::new(std::sync::Mutex::new(json!({"client_id":"fixture-client"})));
        let registration_frames: ProbeFrames = Arc::default();
        let token = Gate::new();
        let token_calls = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(AtomicUsize::new(0));
        let request_count = requests.clone();
        let (snapshot_tx, snapshot_rx) = tokio::sync::mpsc::unbounded_channel();
        let token_failure = Arc::new(AtomicBool::new(false));
        let failure = token_failure.clone();
        let token_temporary = Arc::new(AtomicBool::new(false));
        let temporary = token_temporary.clone();
        let token_reply = Arc::new(std::sync::Mutex::new(None));
        let reply = token_reply.clone();
        let token_forms = Arc::new(std::sync::Mutex::new(Vec::new()));
        let forms = token_forms.clone();
        let issuer = Arc::new(std::sync::Mutex::new(provider.clone()));
        let base = issuer.clone();
        let resource = format!("{provider}/mcp");
        let redirects: Redirects = Arc::default();
        let paths = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (redirect, seen) = (redirects.clone(), paths.clone());
        let discovery_gate = discovery.clone();
        let registration_gate = registration.clone();
        let (body_gate, register_reply, register_frames) =
            (registration_body.clone(), registration_reply.clone(), registration_frames.clone());
        let token_gate = token.clone();
        let token_counter = token_calls.clone();
        let router = Router::new()
            .route(
                "/agent-snapshot",
                routing::post(move |Json(snapshot): Json<Value>| {
                    let tx = snapshot_tx.clone();
                    async move {
                        tx.send(snapshot).unwrap();
                        StatusCode::OK
                    }
                }),
            )
            .route(
                "/mcp",
                routing::post(move |headers: axum::http::HeaderMap, Json(frame): Json<Value>| {
                    let (gate, reply, frames, raw) =
                        (probe_gate.clone(), initial_reply.clone(), frames.clone(), raw_reply.clone());
                    async move {
                        use axum::response::IntoResponse;
                        if frame["method"] == "initialize" {
                            frames.lock().unwrap().push((headers, frame));
                            gate.cross().await;
                            let raw = raw.lock().unwrap().clone();
                            if let Some(values) = raw {
                                let mut headers = axum::http::HeaderMap::new();
                                for value in values {
                                    headers.append(axum::http::header::WWW_AUTHENTICATE, value);
                                }
                                return (StatusCode::UNAUTHORIZED, headers).into_response();
                            }
                            let answer = reply.lock().unwrap().clone();
                            if let Some((status, values)) = answer {
                                let mut headers = axum::http::HeaderMap::new();
                                for value in values {
                                    headers.append(axum::http::header::WWW_AUTHENTICATE, value.parse().unwrap());
                                }
                                return (status, headers).into_response();
                            }
                        }
                        StatusCode::UNAUTHORIZED.into_response()
                    }
                }),
            )
            .route(
                "/.well-known/oauth-protected-resource/mcp",
                routing::get(move || {
                    let base = base.lock().unwrap().clone();
                    let resource = resource.clone();
                    let discovery_gate = discovery_gate.clone();
                    async move {
                        discovery_gate.cross().await;
                        Json(json!({"resource":resource,"authorization_servers":[base],"scopes_supported":["read"]}))
                    }
                }),
            )
            .route(
                "/.well-known/oauth-authorization-server",
                routing::get(as_metadata(provider.clone(), provider.clone())),
            )
            // Alternativos já previstos na descoberta; só respondem quando o primeiro falha.
            .route(
                "/.well-known/oauth-protected-resource",
                routing::get({
                    let base = provider.clone();
                    move || {
                        let base = base.clone();
                        async move {
                            Json(json!({"resource":base,"authorization_servers":[base],"scopes_supported":["read"]}))
                        }
                    }
                }),
            )
            .route("/.well-known/openid-configuration", routing::get(as_metadata(provider.clone(), provider.clone())))
            .route(
                "/register",
                routing::post(move |headers: axum::http::HeaderMap, Json(body): Json<Value>| {
                    let (gate, body_gate, reply, frames) =
                        (registration_gate.clone(), body_gate.clone(), register_reply.clone(), register_frames.clone());
                    async move {
                        frames.lock().unwrap().push((headers, body));
                        let response = reply.lock().unwrap().to_string();
                        gate.cross().await;
                        let stream = futures::stream::once(async move {
                            body_gate.cross().await;
                            Ok::<_, std::io::Error>(response)
                        });
                        axum::response::Response::builder()
                            .header("content-type", "application/json")
                            .body(axum::body::Body::from_stream(stream))
                            .unwrap()
                    }
                }),
            )
            .route(
                "/token",
                routing::post(move |body: String| {
                    let token_gate = token_gate.clone();
                    let token_counter = token_counter.clone();
                    let failure = failure.clone();
                    let temporary = temporary.clone();
                    let reply = reply.clone();
                    let forms = forms.clone();
                    async move {
                        forms.lock().unwrap().push(body);
                        let call = token_counter.fetch_add(1, Ordering::SeqCst);
                        let failed = failure.load(Ordering::SeqCst);
                        let unavailable = temporary.load(Ordering::SeqCst);
                        let answer = reply.lock().unwrap().clone();
                        token_gate.cross().await;
                        if let Some((status, value)) = answer {
                            (status, Json(value))
                        } else if unavailable {
                            (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"invalid_grant"})))
                        } else if failed {
                            (StatusCode::BAD_REQUEST, Json(json!({"error":"invalid_grant"})))
                        } else {
                            (
                                StatusCode::OK,
                                Json(json!({
                                    "access_token":format!("fixture-access-{call}"),
                                    "refresh_token":format!("fixture-refresh-{call}"),"expires_in":3600
                                })),
                            )
                        }
                    }
                }),
            );
        let router = router.layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let count = request_count.clone();
                let (redirect, seen) = (redirect.clone(), seen.clone());
                async move {
                    use axum::response::IntoResponse;
                    count.fetch_add(1, Ordering::SeqCst);
                    let path = request.uri().path().to_owned();
                    seen.lock().unwrap().push(path.clone());
                    let answer = redirect.lock().unwrap().get(&path).cloned();
                    if let Some((status, location, body)) = answer {
                        if path == "/mcp" {
                            return (status, [(axum::http::header::WWW_AUTHENTICATE, location)], Json(body))
                                .into_response();
                        }
                        return (status, [(axum::http::header::LOCATION, location)], Json(body)).into_response();
                    }
                    next.run(request).await
                }
            },
        ));
        let provider_task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let api_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = api_listener.local_addr().unwrap();
        let dir = std::env::temp_dir().join(format!("upstream02-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = if disk {
            let store = Store::open(&dir.join("fixture.db")).unwrap();
            *store.secret_key.lock().unwrap() = Some(zeroize::Zeroizing::new(vec![42; 32]));
            store
        } else {
            Store::open(std::path::Path::new(":memory:")).unwrap()
        };
        let hub = Hub::new(
            store,
            "fixture".into(),
            Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            address.port(),
        );
        let api_router = api::router(hub.clone());
        let api_task = tokio::spawn(async move {
            axum::serve(api_listener, api_router.into_make_service_with_connect_info::<std::net::SocketAddr>())
                .await
                .unwrap();
        });
        let fixture = Arc::new(Self {
            hub,
            api: format!("http://{address}"),
            provider,
            discovery,
            initial,
            raw_challenges,
            probe_frames,
            probe,
            registration,
            registration_body,
            registration_reply,
            registration_frames,
            token,
            token_calls,
            requests,
            token_failure,
            token_temporary,
            token_reply,
            token_forms,
            redirects,
            paths,
            issuer,
            snapshots: tokio::sync::Mutex::new(snapshot_rx),
            tasks: vec![provider_task, api_task],
            dir,
        });
        fixture.install().await;
        fixture
    }
    async fn rpc(&self, method: &str, body: Value) -> (StatusCode, Value) {
        let response = crate::http()
            .post(format!("{}/api/{method}", self.api))
            .bearer_auth("fixture")
            .json(&body)
            .send()
            .await
            .unwrap();
        (response.status(), response.json().await.unwrap())
    }
    async fn install(&self) {
        let (status, body) =
            self.rpc("installConnector", json!({"name":"notes","url":format!("{}/mcp",self.provider)})).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["connector"]["id"], "notes");
    }
    async fn recreate(&self) {
        assert_eq!(self.rpc("removeConnector", json!({"id":"notes"})).await.0, StatusCode::OK);
        self.install().await;
    }
    async fn start(&self) -> String {
        let (status, body) = self.rpc("connectorSignIn", json!({"id":"notes"})).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        reqwest::Url::parse(body["url"].as_str().unwrap())
            .unwrap()
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .to_string()
    }
    async fn callback(&self, state: &str) -> String {
        crate::http()
            .get(
                reqwest::Url::parse_with_params(
                    &format!("{}/oauth/callback", self.api),
                    [("state", state), ("code", "fixture-code")],
                )
                .unwrap(),
            )
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
    }
    fn saved(&self) -> market::Connector {
        market::connectors(&self.hub.store).unwrap().remove(0)
    }

    async fn runtime_target(&self, stale: bool) -> anyhow::Result<Value> {
        let canonical = self.saved();
        let setup = crate::fork::conectores::oauth_binding::issue_setup(&self.hub.store, &canonical)?;
        let intent = crate::fork::conectores::oauth_runtime::RuntimeIntent::capture(
            &self.hub,
            &setup.id,
            "notes",
            "initialize",
        )?;
        let expected = intent.snapshot(&self.hub)?;
        let result = super::target(&self.hub, &expected, &intent, stale)
            .await
            .and_then(crate::fork::conectores::oauth_refresh::Target::into_test_value);
        drop(setup);
        result
    }
}

#[tokio::test]
async fn upstream02_delayed_callback_cannot_reach_same_id_same_url() {
    let f = Fixture::new().await;
    let state = f.start().await;
    f.token.arm();
    let call = {
        let f = f.clone();
        tokio::spawn(async move { f.callback(&state).await })
    };
    f.token.wait().await;
    f.recreate().await;
    let replacement = f.saved();
    f.token.open();
    let html = call.await.unwrap();
    assert_eq!(f.saved(), replacement, "resposta velha alterou o registro canônico substituto");
    assert!(!html.contains("notes conectado"), "callback atrasado foi aceito: {html}");
}

#[tokio::test]
async fn upstream02_replacement_during_discovery_and_dcr_is_rejected() {
    for dcr in [false, true] {
        let f = Fixture::new().await;
        let gate = if dcr { &f.registration } else { &f.discovery };
        gate.arm();
        let call = {
            let f = f.clone();
            tokio::spawn(async move { f.rpc("connectorSignIn", json!({"id":"notes"})).await })
        };
        gate.wait().await;
        f.recreate().await;
        let replacement = f.saved();
        gate.open();
        assert_eq!(call.await.unwrap().0, StatusCode::BAD_REQUEST);
        assert_eq!(f.saved(), replacement);
    }
}
#[tokio::test]
async fn upstream02_removal_before_callback_and_after_token_response_leaves_no_record() {
    for during_post in [false, true] {
        let f = Fixture::new().await;
        let state = f.start().await;
        if during_post {
            f.token.arm();
            let call = {
                let f = f.clone();
                tokio::spawn(async move { f.callback(&state).await })
            };
            f.token.wait().await;
            f.rpc("removeConnector", json!({"id":"notes"})).await;
            f.token.open();
            assert!(!call.await.unwrap().contains("notes conectado"));
        } else {
            f.rpc("removeConnector", json!({"id":"notes"})).await;
            assert!(!f.callback(&state).await.contains("notes conectado"));
        }
        assert_eq!(market::connectors(&f.hub.store).unwrap(), Vec::<market::Connector>::new());
    }
}
#[tokio::test]
async fn upstream02_new_grant_wins_over_old_callback_and_state_is_one_use() {
    let f = Fixture::new().await;
    let old = f.start().await;
    f.token.arm();
    let call = {
        let f = f.clone();
        tokio::spawn(async move { f.callback(&old).await })
    };
    f.token.wait().await;
    let current = f.start().await;
    assert!(f.callback(&current).await.contains("notes conectado"));
    let saved = f.saved();
    f.token.open();
    assert!(!call.await.unwrap().contains("notes conectado"));
    assert_eq!(f.saved(), saved);
    assert!(!f.callback(&current).await.contains("notes conectado"));
    assert!(!f.callback("unknown-state").await.contains("notes conectado"));
}
#[tokio::test]
async fn upstream02_import_and_copy_never_accept_binding_or_select_bots() {
    let f = Fixture::new().await;
    let original = f.saved();
    let mut copy = serde_json::to_value(&original).unwrap();
    copy["name"] = json!("copy");
    copy["oauthBinding"]["profile"] = json!("curated");
    copy["oauthBinding"]["registrationIssuer"] = json!(f.provider);
    let (status, _) = f.rpc("installConnector", copy).await;
    assert_eq!(status, StatusCode::OK);
    let all = market::connectors(&f.hub.store).unwrap();
    let copy = all.iter().find(|c| c.id == "copy").unwrap();
    let old = serde_json::to_value(&original).unwrap();
    let copied = serde_json::to_value(copy).unwrap();
    assert_ne!(old["oauthBinding"]["instance"], copied["oauthBinding"]["instance"]);
    assert!(copied["oauthBinding"]["profile"].is_null());
    assert!(copied["oauthBinding"].get("registrationIssuer").is_none());
    assert!(copy.public().get("oauthBinding").is_none());
    let mut injected = old.clone();
    injected["name"] = json!("imported");
    injected["oauthBinding"]["registrationIssuer"] = json!(f.provider);
    let (status, _) =
        f.rpc("importConnectors", json!({"config":json!({"mcpServers":{"imported":injected}}).to_string()})).await;
    assert_eq!(status, StatusCode::OK);
    let imported = market::connectors(&f.hub.store).unwrap().into_iter().find(|c| c.id == "imported").unwrap();
    let imported = serde_json::to_value(imported).unwrap();
    assert_ne!(imported["oauthBinding"]["instance"], old["oauthBinding"]["instance"]);
    assert!(imported["oauthBinding"].get("registrationIssuer").is_none());
    let cfg = serde_json::from_value(json!({"id":"off","name":"off","connectors":[]})).unwrap();
    f.hub.store.save_bot(&cfg).unwrap();
    let state = f.start().await;
    assert!(f.callback(&state).await.contains("notes conectado"));
    assert_eq!(f.hub.store.bot("off").unwrap().unwrap().config.connectors, Vec::<String>::new());
}

#[tokio::test]
async fn upstream02_rotating_refreshes_share_pair_and_old_signout_cannot_clear_new_grant() {
    let f = Fixture::new().await;
    let state = f.start().await;
    assert!(f.callback(&state).await.contains("notes conectado"));
    market::update_connectors(&f.hub.store, |all| {
        all[0].oauth.as_mut().unwrap().expires_at = Some(0);
        Ok(())
    })
    .unwrap();
    let original = f.saved();
    let setup = crate::fork::conectores::oauth_binding::issue_setup(&f.hub.store, &original).unwrap();
    let intent = Arc::new(
        crate::fork::conectores::oauth_runtime::RuntimeIntent::capture(&f.hub, &setup.id, "notes", "initialize")
            .unwrap(),
    );
    let before = f.token_calls.load(Ordering::SeqCst);
    f.token.arm();
    let first = {
        let f = f.clone();
        let c = original.clone();
        let intent = intent.clone();
        tokio::spawn(async move { super::fresh(&f.hub, &c, c.oauth.as_ref().unwrap(), &intent, false).await })
    };
    f.token.wait().await;
    let second = {
        let f = f.clone();
        let c = original.clone();
        let intent = intent.clone();
        tokio::spawn(async move { super::fresh(&f.hub, &c, c.oauth.as_ref().unwrap(), &intent, false).await })
    };
    f.token.open();
    let a = first.await.unwrap().unwrap();
    let b = second.await.unwrap().unwrap();
    assert_eq!(a, b);
    assert_eq!(f.token_calls.load(Ordering::SeqCst), before + 1);
    assert_eq!(f.saved().oauth, Some(a));
    assert_eq!(
        serde_json::to_value(f.saved()).unwrap()["oauthBinding"],
        serde_json::to_value(&original).unwrap()["oauthBinding"]
    );
    let state = f.start().await;
    assert!(f.callback(&state).await.contains("notes conectado"));
    let current = f.saved();
    assert!(super::sign_out(&f.hub, &original, &intent).is_err());
    assert_eq!(f.saved(), current);
}
#[tokio::test]
async fn upstream02_old_refresh_after_reconnection_cannot_write_or_return_secret() {
    for failed in [false, true] {
        let f = Fixture::new().await;
        let state = f.start().await;
        f.callback(&state).await;
        market::update_connectors(&f.hub.store, |all| {
            all[0].oauth.as_mut().unwrap().expires_at = Some(0);
            Ok(())
        })
        .unwrap();
        f.token_failure.store(failed, Ordering::SeqCst);
        f.token.arm();
        let call = {
            let f = f.clone();
            tokio::spawn(async move { f.runtime_target(false).await })
        };
        f.token.wait().await;
        f.token_failure.store(false, Ordering::SeqCst);
        let state = f.start().await;
        f.callback(&state).await;
        let current = f.saved();
        f.token.open();
        assert!(call.await.unwrap().is_err());
        assert_eq!(f.saved(), current);
    }
}
#[tokio::test]
async fn upstream02_expired_state_is_rejected_without_token_post() {
    let f = Fixture::new().await;
    let state = f.start().await;
    {
        use crate::LockExt;
        let mut pending = super::PENDING.locked();
        pending.as_mut().unwrap().get_mut(&state).unwrap().started -= super::PENDING_FOR;
    }
    assert!(!f.callback(&state).await.contains("notes conectado"));
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn upstream02_device_regrant_during_token_post_cannot_save_callback() {
    use crate::{
        api::devices::Caller,
        store::{Device, DeviceSource, Scope},
    };
    let f = Fixture::new().await;
    f.hub.store.kv_set("test_cloud_url", "https://fixture.invalid").unwrap();
    let device = Device {
        key: "operator".into(),
        name: "fixture".into(),
        platform: "fixture".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: 1,
        last_seen_at: None,
    };
    f.hub.store.put_device(&device).unwrap();
    let caller = || Caller::Device { key: "operator".into(), scopes: vec![Scope::Control], autoridade: None };
    let result = api::dispatch(&f.hub, &caller(), "connectorSignIn", json!({"id":"notes"})).await.unwrap();
    let state = reqwest::Url::parse(result["url"].as_str().unwrap())
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .to_string();
    f.token.arm();
    let call = {
        let f = f.clone();
        tokio::spawn(async move {
            api::dispatch(
                &f.hub,
                &Caller::Device { key: "operator".into(), scopes: vec![Scope::Control], autoridade: None },
                "connectorSignInFinish",
                json!({"state":state,"code":"fixture"}),
            )
            .await
        })
    };
    f.token.wait().await;
    f.hub.store.remove_device("operator").unwrap();
    f.hub.store.put_device(&device).unwrap();
    let before = f.saved();
    f.token.open();
    assert!(call.await.unwrap().is_err());
    assert_eq!(f.saved(), before);
}

static UNLOCK: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<usize, Arc<Gate>>>> =
    std::sync::LazyLock::new(Default::default);
/// Barreira de teste entre captura e unlock; não toca no chaveiro da máquina.
pub(crate) async fn before_unlock(hub: &Arc<Hub>) {
    let gate = UNLOCK.lock().unwrap().remove(&(Arc::as_ptr(hub) as usize));
    if let Some(gate) = gate {
        gate.cross().await;
    }
}
#[tokio::test]
async fn upstream02_authority_is_captured_before_unlock_queue_and_never_recaptured() {
    use crate::{
        api::devices::Caller,
        store::{Device, DeviceSource, Scope},
    };
    let f = Fixture::new().await;
    f.hub.store.kv_set("test_cloud_url", "https://fixture.invalid").unwrap();
    let d = Device {
        key: "operator".into(),
        name: "fixture".into(),
        platform: "fixture".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: 1,
        last_seen_at: None,
    };
    f.hub.store.put_device(&d).unwrap();
    let before = f.saved();
    let gate = Gate::new();
    gate.arm();
    UNLOCK.lock().unwrap().insert(Arc::as_ptr(&f.hub) as usize, gate.clone());
    let call = {
        let f = f.clone();
        tokio::spawn(async move {
            api::dispatch(
                &f.hub,
                &Caller::Device { key: "operator".into(), scopes: vec![Scope::Control], autoridade: None },
                "connectorSignIn",
                json!({"id":"notes"}),
            )
            .await
        })
    };
    gate.wait().await;
    f.hub.store.remove_device(&d.key).unwrap();
    f.hub.store.put_device(&d).unwrap();
    gate.open();
    assert!(call.await.unwrap().is_err());
    assert_eq!(f.saved(), before);
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn upstream02_legacy_first_runtime_refresh_and_same_url_reconnect_obey_lease() {
    use crate::guard_hub::mcp::setup::Setup;
    let f = Fixture::new().await;
    let state = f.start().await;
    f.callback(&state).await;
    let mut legacy = serde_json::to_value(f.saved()).unwrap();
    legacy.as_object_mut().unwrap().remove("oauthBinding");
    market::vault::write(&f.hub.store, "connectors", &json!([legacy]).to_string()).unwrap();
    let setup = Setup::issue(&f.hub.store, "notes").unwrap();
    let bridge = uuid::Uuid::new_v4().to_string();
    let body =
        json!({"context":setup.id,"connector":"notes","transport":"http","bridgeId":bridge,"method":"initialize"});
    let (status, answer) = f.rpc("donnaGuardRuntime", body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let original = f.saved();
    let before = serde_json::to_value(&original).unwrap();
    assert!(before["oauthBinding"]["instance"].is_string());
    market::update_connectors(&f.hub.store, |all| {
        all[0].oauth.as_mut().unwrap().expires_at = Some(0);
        Ok(())
    })
    .unwrap();
    let mut refresh = body.clone();
    refresh["refreshLease"] = json!(true);
    let (status, answer) = f.rpc("donnaGuardRuntime", refresh.clone()).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(before["oauthBinding"], serde_json::to_value(f.saved()).unwrap()["oauthBinding"]);
    let state = f.start().await;
    f.callback(&state).await;
    assert_eq!(f.rpc("donnaGuardRuntime", refresh).await.0, StatusCode::BAD_REQUEST, "reconexão não herda lease");
    let mut old_context_new_bridge = body.clone();
    old_context_new_bridge["bridgeId"] = json!(uuid::Uuid::new_v4().to_string());
    assert_eq!(f.rpc("donnaGuardRuntime", old_context_new_bridge).await.0, StatusCode::BAD_REQUEST);
    let mut observe = body;
    observe["frame"] = json!({"jsonrpc":"2.0","id":1,"result":{}});
    assert_eq!(f.rpc("donnaGuardObserve", observe).await.0, StatusCode::BAD_REQUEST);
}

static AFTER_TOKEN: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<usize, Arc<Gate>>>> =
    std::sync::LazyLock::new(Default::default);
pub(crate) async fn after_token(hub: &Arc<Hub>) {
    let gate = AFTER_TOKEN.lock().unwrap().remove(&(Arc::as_ptr(hub) as usize));
    if let Some(gate) = gate {
        gate.cross().await;
    }
}
#[tokio::test]
async fn upstream02_removal_after_received_token_response_cannot_commit() {
    let f = Fixture::new().await;
    let state = f.start().await;
    let gate = Gate::new();
    gate.arm();
    AFTER_TOKEN.lock().unwrap().insert(Arc::as_ptr(&f.hub) as usize, gate.clone());
    let call = {
        let f = f.clone();
        tokio::spawn(async move { f.callback(&state).await })
    };
    gate.wait().await;
    f.recreate().await;
    let replacement = f.saved();
    gate.open();
    assert!(!call.await.unwrap().contains("notes conectado"));
    assert_eq!(f.saved(), replacement);
}

#[tokio::test]
async fn upstream02_account_lease_renewal_keeps_original_rpc_finish_valid() {
    use crate::{
        api::devices::Caller,
        store::{Device, DeviceSource, Scope},
    };
    let f = Fixture::new().await;
    f.hub.store.kv_set("test_cloud_url", "https://fixture.invalid").unwrap();
    f.hub.cloud_synced.store(true, Ordering::Release);
    let d = Device {
        key: "account-operator".into(),
        name: "fixture".into(),
        platform: "fixture".into(),
        source: DeviceSource::Account,
        grant_id: Some("account-grant".into()),
        scopes: vec![Scope::Control],
        lease_until: Some(crate::store::now_ms() + 60000),
        created_at: 1,
        last_seen_at: None,
    };
    f.hub.store.put_device(&d).unwrap();
    let caller = || Caller::Device { key: d.key.clone(), scopes: vec![Scope::Control], autoridade: None };
    let result = api::dispatch(&f.hub, &caller(), "connectorSignIn", json!({"id":"notes"})).await.unwrap();
    assert_eq!(result["callback"], "app");
    let state = reqwest::Url::parse(result["url"].as_str().unwrap())
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .to_string();
    f.hub.store.set_lease(&d.key, crate::store::now_ms() + 120_000).unwrap();
    let answer = api::dispatch(&f.hub, &caller(), "connectorSignInFinish", json!({"state":state,"code":"fixture"}))
        .await
        .unwrap();
    assert_eq!(answer["connector"]["auth"], "signedIn");
    assert!(answer["connector"].get("oauthBinding").is_none());
    assert!(f.saved().oauth.unwrap().signed_in());
}

#[tokio::test]
async fn upstream02_explicit_host_new_session_emits_new_context_without_enabling_bots() {
    use crate::{api::devices::Caller, hub::BotStatus};
    std::fs::create_dir_all(crate::service::data_dir()).unwrap();
    let f = Fixture::new().await;
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/oauth_context_agent.py");
    let command =
        format!("{} {}", crate::shell::python(&script), crate::shell::quote(&format!("{}/agent-snapshot", f.provider)));
    for (id, on) in [("selected", json!(["notes"])), ("off", json!([]))] {
        let cfg = serde_json::from_value(
            json!({"id":id,"name":id,"backend":"fixture","command":command,"cwd":f.dir,"connectors":on,"notify":false}),
        )
        .unwrap();
        f.hub.store.save_bot(&cfg).unwrap();
    }
    let state = f.start().await;
    f.callback(&state).await;
    f.hub.start().unwrap();
    let mut events = f.hub.events.subscribe();
    api::dispatch(&f.hub, &Caller::Local, "send", json!({"botId":"selected","text":"Primeira intenção explícita"}))
        .await
        .unwrap();
    let first = tokio::time::timeout(std::time::Duration::from_secs(20), f.snapshots.lock().await.recv())
        .await
        .unwrap()
        .unwrap();
    let context = |servers: &Value| {
        let notes = servers.as_array().unwrap().iter().find(|s| s["name"] == "notes").unwrap();
        let args = notes["args"].as_array().unwrap();
        let pos = args.iter().position(|v| v == "--context").unwrap();
        args[pos + 1].as_str().unwrap().to_owned()
    };
    let old = context(&first);
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let state = f.hub.runtime("selected").status;
            assert!(!matches!(state, BotStatus::Error));
            if matches!(state, BotStatus::Idle) {
                break;
            }
            events.recv().await.unwrap();
        }
    })
    .await
    .unwrap();
    let state = f.start().await;
    f.callback(&state).await;
    assert!(f.hub.store.guard_context(&old).unwrap().is_none());
    api::dispatch(&f.hub, &Caller::Local, "newSession", json!({"botId":"selected"})).await.unwrap();
    api::dispatch(&f.hub, &Caller::Local, "send", json!({"botId":"selected","text":"Nova intenção explícita"}))
        .await
        .unwrap();
    let second = tokio::time::timeout(std::time::Duration::from_secs(20), f.snapshots.lock().await.recv())
        .await
        .unwrap()
        .unwrap();
    let new = context(&second);
    assert_ne!(new, old);
    assert!(f.hub.store.guard_context(&new).unwrap().is_some());
    assert_eq!(f.hub.store.bot("off").unwrap().unwrap().config.connectors, Vec::<String>::new());
    assert_eq!(f.hub.store.bot("selected").unwrap().unwrap().config.connectors, ["notes"]);
    f.hub.shutdown().await;
}

#[tokio::test]
async fn upstream02_credential_header_change_keeps_oauth_signout_and_allows_new_login() {
    let f = Fixture::new().await;
    market::update_connectors(&f.hub.store, |all| {
        all[0].headers.insert("X-Workspace".into(), "before".into());
        Ok(())
    })
    .unwrap();
    let state = f.start().await;
    assert!(f.callback(&state).await.contains("notes conectado"));
    let original = f.saved();
    let setup = crate::fork::conectores::oauth_binding::issue_setup(&f.hub.store, &original).unwrap();
    let (status, _) = f.rpc("credentialUpdateConnector", json!({"id":"notes","fields":{"X-Workspace":"after"}})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "verificação exige novo login");
    let current = f.saved();
    assert_eq!(current.public()["auth"], "signedOut", "necessidade de OAuth não pode virar auth:none");
    assert_eq!(current.oauth, Some(market::oauth::OAuth::default()), "nenhum token ou cadastro antigo é reutilizado");
    assert!(current.acp(std::path::Path::new("fixture-host"), f.hub.port).is_none());
    let before = serde_json::to_value(&original).unwrap();
    let after = serde_json::to_value(&current).unwrap();
    assert_eq!(before["oauthBinding"]["instance"], after["oauthBinding"]["instance"]);
    assert_ne!(before["oauthBinding"]["generation"], after["oauthBinding"]["generation"]);
    assert_ne!(before["oauthBinding"]["revision"], after["oauthBinding"]["revision"]);
    let (_, listing) = f.rpc("connectors", json!({})).await;
    assert_eq!(listing["items"][0]["auth"], "signedOut");
    let (_, old) = f.rpc("donnaGuardRuntime", json!({"context":setup.id,"connector":"notes","transport":"http","bridgeId":uuid::Uuid::new_v4().to_string(),"method":"initialize"})).await;
    assert!(old["error"].is_string(), "contexto anterior continua revogado");
    let state = f.start().await;
    assert!(f.callback(&state).await.contains("notes conectado"));
    assert_eq!(f.saved().public()["auth"], "signedIn");
    assert_eq!(
        serde_json::to_value(f.saved()).unwrap()["oauthBinding"]["instance"],
        before["oauthBinding"]["instance"]
    );
}

#[tokio::test]
async fn upstream02_preunlock_start_cannot_adopt_same_id_same_url_replacement() {
    let f = Fixture::new().await;
    let gate = Gate::new();
    gate.arm();
    UNLOCK.lock().unwrap().insert(Arc::as_ptr(&f.hub) as usize, gate.clone());
    let call = {
        let f = f.clone();
        tokio::spawn(async move { f.rpc("connectorSignIn", json!({"id":"notes"})).await })
    };
    gate.wait().await;
    f.recreate().await;
    let state = f.start().await;
    assert!(f.callback(&state).await.contains("notes conectado"));
    let replacement = f.saved();
    let raw = f.hub.store.kv_read("connectors").unwrap();
    let requests = f.requests.load(Ordering::SeqCst);
    gate.open();
    let (status, response) = call.await.unwrap();
    assert_eq!(
        f.saved(),
        replacement,
        "pedido anterior ao unlock alterou substituto; status={status}, resposta={response}"
    );
    assert!(f.hub.store.kv_read("connectors").unwrap() == raw, "pedido antigo regravou registro canônico");
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(response.get("url").is_none());
    assert_eq!(f.requests.load(Ordering::SeqCst), requests, "pedido antigo fez rede/discovery/DCR/token");
    let next = f.start().await;
    assert!(!next.is_empty(), "nova intenção explícita é permitida");
}

#[tokio::test]
async fn upstream02_preunlock_other_write_rotation_migration_and_new_target_need_new_intent() {
    for change in ["other", "rotation", "migration", "new-target"] {
        let fixture = Fixture::new().await;
        let state = fixture.start().await;
        assert!(fixture.callback(&state).await.contains("notes conectado"));
        if change == "migration" {
            let mut legacy = serde_json::to_value(fixture.saved()).unwrap();
            legacy.as_object_mut().unwrap().remove("oauthBinding");
            market::vault::write(&fixture.hub.store, "connectors", &json!([legacy]).to_string()).unwrap();
        }
        let provider = if change == "new-target" { Some(Fixture::new().await) } else { None };
        let gate = Gate::new();
        gate.arm();
        UNLOCK.lock().unwrap().insert(Arc::as_ptr(&fixture.hub) as usize, gate.clone());
        let call = {
            let fixture = fixture.clone();
            tokio::spawn(async move { fixture.rpc("connectorSignIn", json!({"id":"notes"})).await })
        };
        gate.wait().await;
        match change {
            "other" => {
                assert_eq!(
                    fixture.rpc("installConnector", json!({"name":"other","command":"unused"})).await.0,
                    StatusCode::OK
                );
            }
            "rotation" => {
                fixture.runtime_target(true).await.unwrap();
            }
            "migration" => {
                market::connectors(&fixture.hub.store).unwrap();
            }
            "new-target" => {
                fixture.rpc("removeConnector", json!({"id":"notes"})).await;
                let provider = provider.as_ref().unwrap();
                assert_eq!(
                    fixture
                        .rpc("installConnector", json!({"name":"notes","url":format!("{}/mcp",provider.provider)}))
                        .await
                        .0,
                    StatusCode::OK
                );
                let state = fixture.start().await;
                fixture.callback(&state).await;
            }
            _ => unreachable!(),
        }
        let raw = fixture.hub.store.kv_read("connectors").unwrap();
        let saved = market::connectors(&fixture.hub.store).unwrap();
        let requests = fixture.requests.load(Ordering::SeqCst);
        let provider_requests = provider.as_ref().map(|p| p.requests.load(Ordering::SeqCst));
        let pending = {
            use crate::LockExt;
            super::PENDING.locked().as_ref().map_or(0, |m| {
                m.values()
                    .filter(|p| {
                        p.snapshot.id == "notes"
                            && p.snapshot.oauth_binding == saved.iter().find(|c| c.id == "notes").unwrap().oauth_binding
                    })
                    .count()
            })
        };
        gate.open();
        let (status, response) = call.await.unwrap();
        assert_eq!(status, StatusCode::BAD_REQUEST, "{change}: {response}");
        assert!(response.get("url").is_none());
        assert_eq!(response["error"], "A configuração mudou. Inicie uma nova intenção de conexão.");
        assert!(fixture.hub.store.kv_read("connectors").unwrap() == raw, "{change}: raw regravado");
        assert_eq!(market::connectors(&fixture.hub.store).unwrap(), saved);
        assert_eq!(fixture.requests.load(Ordering::SeqCst), requests);
        assert_eq!(provider.as_ref().map(|p| p.requests.load(Ordering::SeqCst)), provider_requests);
        let after_pending = {
            use crate::LockExt;
            super::PENDING.locked().as_ref().map_or(0, |m| {
                m.values()
                    .filter(|p| {
                        p.snapshot.id == "notes"
                            && p.snapshot.oauth_binding == saved.iter().find(|c| c.id == "notes").unwrap().oauth_binding
                    })
                    .count()
            })
        };
        assert_eq!(after_pending, pending);
        assert!(!fixture.start().await.is_empty(), "nova intenção explícita depois de {change}");
    }
}

#[tokio::test]
async fn upstream02_legacy_start_migrates_jointly_without_revoking_other_account() {
    use crate::LockExt;
    use crate::guard_hub::mcp::{contexto::Context, setup::Setup};
    let fixture = Fixture::new().await;
    let mut legacy = serde_json::to_value(fixture.saved()).unwrap();
    legacy.as_object_mut().unwrap().remove("oauthBinding");
    let other = json!({"id":"other","name":"Other","url":format!("{}/mcp",fixture.provider),"oauth":{"clientId":"other-client","accessToken":"other-private","refreshToken":"other-refresh"}});
    let other_oauth: market::oauth::OAuth = serde_json::from_value(other["oauth"].clone()).unwrap();
    let raw = zeroize::Zeroizing::new(json!([legacy, other]).to_string());
    fixture.hub.store.kv_set("connectors", &raw).unwrap();
    let cfg = serde_json::from_value(json!({"id":"both","name":"Both","connectors":["notes","other"]})).unwrap();
    fixture.hub.store.save_bot(&cfg).unwrap();
    let context = Context::issue(&fixture.hub.store, "both", "other", "both", None).unwrap();
    let setup = Setup::issue(&fixture.hub.store, "other").unwrap();
    let state = fixture.start().await;
    assert!(fixture.callback(&state).await.contains("notes conectado"));
    let all = market::connectors(&fixture.hub.store).unwrap();
    let other = all.iter().find(|c| c.id == "other").unwrap();
    assert_eq!(other.oauth.as_ref(), Some(&other_oauth));
    assert!(serde_json::to_value(other).unwrap()["oauthBinding"]["instance"].is_string());
    assert_eq!(Context::resolve(&fixture.hub.store, &context.id, "other").unwrap().id, context.id);
    assert!(
        fixture
            .hub
            .store
            .db
            .locked()
            .query_row("SELECT EXISTS(SELECT 1 FROM donna_guard_setup WHERE id=?1)", [&setup.id], |r| r
                .get::<_, bool>(0))
            .unwrap()
    );
    assert_eq!(fixture.hub.store.bot("both").unwrap().unwrap().config.connectors, ["notes", "other"]);
    assert!(fixture.hub.store.kv_read("connectors").unwrap().as_deref().is_some_and(|v| v.starts_with("vault:v1:")));
}

#[tokio::test]
async fn upstream02_preflight_failures_keep_raw_login_and_pending_unchanged() {
    use crate::{
        api::devices::Caller,
        store::{Device, DeviceSource, Scope},
    };
    for invalid in ["stdio", "missing-url", "bad-url", "fake-local", "app-off", "app-bad-uri"] {
        let fixture = Fixture::new().await;
        let state = fixture.start().await;
        fixture.callback(&state).await;
        if matches!(invalid, "stdio" | "missing-url" | "bad-url" | "fake-local") {
            let mut all = market::connectors(&fixture.hub.store).unwrap();
            match invalid {
                "stdio" => {
                    all[0].command = Some("unused".into());
                }
                "missing-url" => {
                    all[0].url = None;
                }
                // M2-A: `localhost.` resolve para a fixture; recusa antes de rede e de reconnect.
                "fake-local" => {
                    all[0].url = Some(format!("{}/mcp", fixture.provider.replace("127.0.0.1", "localhost.")));
                }
                _ => {
                    all[0].url = Some("https://".into());
                }
            }
            market::vault::write(&fixture.hub.store, "connectors", &serde_json::to_string(&all).unwrap()).unwrap();
        }
        let raw = fixture.hub.store.kv_read("connectors").unwrap();
        let requests = fixture.requests.load(Ordering::SeqCst);
        let result = if invalid.starts_with("app-") {
            let device = Device {
                key: "preflight-device".into(),
                name: "fixture".into(),
                platform: "fixture".into(),
                source: DeviceSource::Local,
                grant_id: None,
                scopes: vec![Scope::Control],
                lease_until: None,
                created_at: 1,
                last_seen_at: None,
            };
            fixture.hub.store.put_device(&device).unwrap();
            if invalid == "app-bad-uri" {
                fixture.hub.store.kv_set("test_cloud_url", "not-a-uri").unwrap();
            }
            api::dispatch(
                &fixture.hub,
                &Caller::Device { key: device.key, scopes: vec![Scope::Control], autoridade: None },
                "connectorSignIn",
                json!({"id":"notes"}),
            )
            .await
        } else {
            api::dispatch(&fixture.hub, &Caller::Local, "connectorSignIn", json!({"id":"notes"})).await
        };
        assert!(result.is_err(), "préflight {invalid}");
        assert!(fixture.hub.store.kv_read("connectors").unwrap() == raw, "préflight não pode limpar login/cadastro");
        assert_eq!(fixture.requests.load(Ordering::SeqCst), requests);
    }
}

#[tokio::test]
async fn upstream02_start_commit_failure_keeps_previous_pending_and_original_flow() {
    use crate::LockExt;
    let fixture = Fixture::new().await;
    let state = fixture.start().await;
    let raw = fixture.hub.store.kv_read("connectors").unwrap();
    let requests = fixture.requests.load(Ordering::SeqCst);
    assert!(super::PENDING.locked().as_ref().unwrap().contains_key(&state));
    fixture.hub.store.db.locked().execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE pending_parent(id INTEGER PRIMARY KEY); CREATE TABLE pending_failure(id INTEGER REFERENCES pending_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fail_pending_start AFTER UPDATE ON kv WHEN NEW.k='connectors' BEGIN INSERT INTO pending_failure VALUES(1); END").unwrap();
    let (status, response) = fixture.rpc("connectorSignIn", json!({"id":"notes"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(response.get("url").is_none());
    assert!(fixture.hub.store.kv_read("connectors").unwrap().eq(&raw));
    assert_eq!(fixture.requests.load(Ordering::SeqCst), requests);
    assert!(super::PENDING.locked().as_ref().unwrap().contains_key(&state));
    fixture.hub.store.db.locked().execute_batch("DROP TRIGGER fail_pending_start").unwrap();
    assert!(fixture.callback(&state).await.contains("notes conectado"));
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) enum RuntimeWindow {
    BeforeUnlock,
    BeforeTargetCapture,
    AfterTargetResolved,
    AfterDeliveryGuard,
}

type RuntimeWindows = std::collections::HashMap<(usize, RuntimeWindow), Arc<Gate>>;
static RUNTIME_WINDOWS: std::sync::LazyLock<std::sync::Mutex<RuntimeWindows>> =
    std::sync::LazyLock::new(Default::default);

pub(crate) fn pause_runtime(hub: &Arc<Hub>, window: RuntimeWindow) -> Arc<Gate> {
    let gate = Gate::new();
    gate.arm();
    RUNTIME_WINDOWS.lock().unwrap().insert((Arc::as_ptr(hub) as usize, window), gate.clone());
    gate
}

/// Só coordena as janelas reais; não muda autorização, snapshots ou resultado.
pub(crate) async fn runtime_window(hub: &Arc<Hub>, window: RuntimeWindow) {
    let gate = RUNTIME_WINDOWS.lock().unwrap().remove(&(Arc::as_ptr(hub) as usize, window));
    if let Some(gate) = gate {
        gate.cross().await;
    }
}

#[derive(Clone, Copy, Debug)]
enum RuntimeTestAuthority {
    Context,
    Setup,
}

#[derive(Clone, Copy)]
enum RuntimeTestReplacement {
    ReconnectWithRefresh,
    RecreateWithoutRefresh,
}

async fn runtime_handoff_cannot_mutate_replacement(
    window: RuntimeWindow,
    authority: RuntimeTestAuthority,
    replacement: RuntimeTestReplacement,
) {
    use crate::{hub::BotStatus, store::Lane};

    let fixture = Fixture::new().await;
    let state = fixture.start().await;
    assert!(fixture.callback(&state).await.contains("notes conectado"));
    let original = fixture.saved();
    // IDs vêm dos emissores Hub existentes; nenhum ID/autoridade é injetado no registro.
    let setup = match authority {
        RuntimeTestAuthority::Setup => {
            Some(crate::fork::conectores::oauth_binding::issue_setup(&fixture.hub.store, &original).unwrap())
        }
        RuntimeTestAuthority::Context => None,
    };
    let context_id = if let Some(setup) = &setup {
        setup.id.clone()
    } else {
        let cfg = serde_json::from_value(json!({"id":"runtime-bot","name":"Runtime fixture","connectors":["notes"]}))
            .unwrap();
        fixture.hub.store.save_bot(&cfg).unwrap();
        let execution = crate::fork::sessoes::bindings::fixture_execution(&fixture.hub, "runtime-bot");
        let context = crate::fork::conectores::oauth_binding::issue_context(
            &fixture.hub.store,
            &original,
            "runtime-bot",
            "runtime-bot",
            None,
            execution.acp_binding().unwrap(),
        )
        .unwrap();
        fixture.hub.set_runtime("runtime-bot", |runtime| {
            runtime.lane = Some(Lane::main("runtime-bot"));
            runtime.status = BotStatus::Working;
        });
        context.id
    };
    let gate = Gate::new();
    gate.arm();
    RUNTIME_WINDOWS.lock().unwrap().insert((Arc::as_ptr(&fixture.hub) as usize, window), gate.clone());
    let body = json!({"context":context_id,"connector":"notes","transport":"http",
        "bridgeId":uuid::Uuid::new_v4().to_string(),"method":"initialize","stale":true});
    let call = {
        let fixture = fixture.clone();
        tokio::spawn(async move { fixture.rpc("donnaGuardRuntime", body).await })
    };
    gate.wait().await;
    if matches!(replacement, RuntimeTestReplacement::RecreateWithoutRefresh) {
        if matches!(authority, RuntimeTestAuthority::Context) {
            // A API reserva slugs selecionados em bots. A fixture força o ABA mais forte
            // no registro canônico, sem relaxar essa proteção da API nem emitir autoridade.
            assert_eq!(fixture.rpc("removeConnector", json!({"id":"notes"})).await.0, StatusCode::OK);
            let mut recreated = original.clone();
            recreated.oauth = None;
            recreated.oauth_binding = None;
            market::save_connectors(&fixture.hub.store, &[recreated]).unwrap();
            let current = fixture.saved();
            assert_eq!(current.id, original.id);
            assert_eq!(current.url, original.url);
            assert_ne!(
                serde_json::to_value(&current).unwrap()["oauthBinding"]["instance"],
                serde_json::to_value(&original).unwrap()["oauthBinding"]["instance"]
            );
        } else {
            fixture.recreate().await;
        }
    }
    let state = fixture.start().await;
    assert!(fixture.callback(&state).await.contains("notes conectado"));
    market::update_connectors(&fixture.hub.store, |all| {
        let oauth = all[0].oauth.as_mut().unwrap();
        oauth.expires_at = Some(0);
        if matches!(replacement, RuntimeTestReplacement::RecreateWithoutRefresh) {
            oauth.refresh_token = None;
        }
        Ok(())
    })
    .unwrap();
    let expected = market::connectors(&fixture.hub.store).unwrap();
    let raw = fixture.hub.store.kv_read("connectors").unwrap();
    let requests = fixture.requests.load(Ordering::SeqCst);
    let token_posts = fixture.token_calls.load(Ordering::SeqCst);
    gate.open();
    let (status, response) = call.await.unwrap();
    assert_eq!(
        market::connectors(&fixture.hub.store).unwrap(),
        expected,
        "{authority:?}/{window:?}: runtime antigo alterou B; status={status}, resposta={response}"
    );
    assert!(fixture.hub.store.kv_read("connectors").unwrap().eq(&raw), "registro cifrado inteiro de B mudou");
    assert_eq!(fixture.requests.load(Ordering::SeqCst), requests, "nenhuma rede do pedido antigo");
    assert_eq!(fixture.token_calls.load(Ordering::SeqCst), token_posts, "nenhum token POST do pedido antigo");
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(response.get("url").is_none(), "URL de B não pode sair no pedido antigo");
    assert!(response.get("headers").is_none(), "headers de B não podem sair no pedido antigo");
    drop(setup);
}

#[tokio::test]
async fn upstream02_runtime_setup_unlock_cannot_refresh_new_grant_same_url() {
    runtime_handoff_cannot_mutate_replacement(
        RuntimeWindow::BeforeUnlock,
        RuntimeTestAuthority::Setup,
        RuntimeTestReplacement::ReconnectWithRefresh,
    )
    .await;
}

#[tokio::test]
async fn upstream02_runtime_context_unlock_cannot_sign_out_recreated_same_url() {
    runtime_handoff_cannot_mutate_replacement(
        RuntimeWindow::BeforeUnlock,
        RuntimeTestAuthority::Context,
        RuntimeTestReplacement::RecreateWithoutRefresh,
    )
    .await;
}

#[tokio::test]
async fn upstream02_runtime_setup_after_saved_cannot_sign_out_recreated_same_url() {
    runtime_handoff_cannot_mutate_replacement(
        RuntimeWindow::BeforeTargetCapture,
        RuntimeTestAuthority::Setup,
        RuntimeTestReplacement::RecreateWithoutRefresh,
    )
    .await;
}

#[tokio::test]
async fn upstream02_runtime_context_after_saved_cannot_refresh_new_grant_same_url() {
    runtime_handoff_cannot_mutate_replacement(
        RuntimeWindow::BeforeTargetCapture,
        RuntimeTestAuthority::Context,
        RuntimeTestReplacement::ReconnectWithRefresh,
    )
    .await;
}

#[tokio::test]
async fn upstream02_runtime_old_refresh_result_never_overwrites_or_signs_out_rotated_pair() {
    for failed in [false, true] {
        let fixture = Fixture::new().await;
        let state = fixture.start().await;
        fixture.callback(&state).await;
        market::update_connectors(&fixture.hub.store, |all| {
            all[0].oauth.as_mut().unwrap().expires_at = Some(0);
            Ok(())
        })
        .unwrap();
        fixture.token_failure.store(failed, Ordering::SeqCst);
        fixture.token.arm();
        let call = {
            let fixture = fixture.clone();
            tokio::spawn(async move { fixture.runtime_target(false).await })
        };
        fixture.token.wait().await;
        // Só rotação: a mesma instância/geração/revisão e o mesmo Setup continuam válidos.
        market::update_connectors(&fixture.hub.store, |all| {
            let oauth = all[0].oauth.as_mut().unwrap();
            oauth.access_token = "new-rotated-access".into();
            oauth.refresh_token = Some("new-rotated-refresh".into());
            oauth.expires_at = Some(super::now() + 3600);
            Ok(())
        })
        .unwrap();
        let expected = fixture.saved();
        let raw = fixture.hub.store.kv_read("connectors").unwrap();
        fixture.token.open();
        assert!(call.await.unwrap().is_err());
        assert_eq!(fixture.saved(), expected, "resposta OLD success/error não pode gravar ou limpar NEW");
        assert!(fixture.hub.store.kv_read("connectors").unwrap().eq(&raw));
    }
}

#[tokio::test]
async fn upstream02_runtime_setup_signout_and_rollback_keep_prior_secret_cache() {
    use crate::{LockExt, guard_hub::mcp::runtime};

    for fail_commit in [false, true] {
        let fixture = Fixture::new().await;
        let state = fixture.start().await;
        fixture.callback(&state).await;
        market::update_connectors(&fixture.hub.store, |all| {
            let oauth = all[0].oauth.as_mut().unwrap();
            oauth.refresh_token = None;
            oauth.expires_at = Some(0);
            Ok(())
        })
        .unwrap();
        let original = fixture.saved();
        let setup = crate::fork::conectores::oauth_binding::issue_setup(&fixture.hub.store, &original).unwrap();
        let namespace = fixture.hub.store.guard_runtime_namespace().unwrap();
        let bridge = uuid::Uuid::new_v4().to_string();
        runtime::remember(
            &namespace,
            "notes",
            &setup.id,
            &bridge,
            &[zeroize::Zeroizing::new("old-inflight-secret".into())],
        );
        let raw = fixture.hub.store.kv_read("connectors").unwrap();
        let requests = fixture.requests.load(Ordering::SeqCst);
        if fail_commit {
            fixture.hub.store.db.locked().execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE runtime_parent(id INTEGER PRIMARY KEY); CREATE TABLE runtime_failure(id INTEGER REFERENCES runtime_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fail_runtime AFTER UPDATE ON kv WHEN NEW.k='connectors' BEGIN INSERT INTO runtime_failure VALUES(1); END").unwrap();
        }
        let (status, response) = fixture
            .rpc(
                "donnaGuardRuntime",
                json!({"context":setup.id,"connector":"notes",
            "transport":"http","bridgeId":bridge,"method":"initialize","stale":true}),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(response.get("headers").is_none());
        assert_eq!(fixture.requests.load(Ordering::SeqCst), requests);
        let exists: bool = fixture
            .hub
            .store
            .db
            .locked()
            .query_row("SELECT EXISTS(SELECT 1 FROM donna_guard_setup WHERE id=?1)", [&setup.id], |row| row.get(0))
            .unwrap();
        if fail_commit {
            assert_eq!(fixture.saved(), original);
            assert!(fixture.hub.store.kv_read("connectors").unwrap().eq(&raw));
            assert!(exists, "rollback restaura a MESMA linha de autoridade");
        } else {
            assert_eq!(fixture.saved().public()["auth"], "signedOut");
            assert!(!exists, "sign_out autorizado pode apagar sua própria linha na mesma tx");
        }
        assert!(
            runtime::snapshot(&namespace, "notes", &setup.id, Some(&bridge)).is_some(),
            "cache em voo não é cleanup da guarda"
        );
    }
}

#[tokio::test]
async fn upstream02_runtime_revoked_setup_after_token_post_cannot_write_or_return_secret() {
    use crate::{LockExt, guard_hub::mcp::runtime};

    let fixture = Fixture::new().await;
    let state = fixture.start().await;
    fixture.callback(&state).await;
    market::update_connectors(&fixture.hub.store, |all| {
        all[0].oauth.as_mut().unwrap().expires_at = Some(0);
        Ok(())
    })
    .unwrap();
    let original = fixture.saved();
    let setup = crate::fork::conectores::oauth_binding::issue_setup(&fixture.hub.store, &original).unwrap();
    let bridge = uuid::Uuid::new_v4().to_string();
    let namespace = fixture.hub.store.guard_runtime_namespace().unwrap();
    runtime::remember(&namespace, "notes", &setup.id, &bridge, &[zeroize::Zeroizing::new("prior-secret".into())]);
    let raw = fixture.hub.store.kv_read("connectors").unwrap();
    fixture.token.arm();
    let body =
        json!({"context":setup.id,"connector":"notes","transport":"http","bridgeId":bridge,"method":"initialize"});
    let call = {
        let fixture = fixture.clone();
        tokio::spawn(async move { fixture.rpc("donnaGuardRuntime", body).await })
    };
    fixture.token.wait().await;
    fixture.hub.store.db.locked().execute("DELETE FROM donna_guard_setup WHERE id=?1", [&setup.id]).unwrap();
    fixture.token.open();
    let (status, response) = call.await.unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(response.get("url").is_none());
    assert!(response.get("headers").is_none());
    assert_eq!(fixture.saved(), original, "revogação do MESMO setup impede escrita, mesmo sem troca de binding");
    assert!(fixture.hub.store.kv_read("connectors").unwrap().eq(&raw));
    assert!(runtime::snapshot(&namespace, "notes", &setup.id, Some(&bridge)).is_some());
}

#[tokio::test]
async fn upstream02_runtime_expected_snapshot_adopts_same_grant_rotation_without_repeating_old_401() {
    use crate::guard_hub::mcp::runtime;

    let fixture = Fixture::new().await;
    let state = fixture.start().await;
    fixture.callback(&state).await;
    let original = fixture.saved();
    let setup = crate::fork::conectores::oauth_binding::issue_setup(&fixture.hub.store, &original).unwrap();
    let bridge = uuid::Uuid::new_v4().to_string();
    let gate = Gate::new();
    gate.arm();
    RUNTIME_WINDOWS
        .lock()
        .unwrap()
        .insert((Arc::as_ptr(&fixture.hub) as usize, RuntimeWindow::BeforeTargetCapture), gate.clone());
    let body = json!({"context":setup.id,"connector":"notes","transport":"http","bridgeId":bridge,
        "method":"initialize","stale":true});
    let call = {
        let fixture = fixture.clone();
        tokio::spawn(async move { fixture.rpc("donnaGuardRuntime", body).await })
    };
    gate.wait().await;
    let other = crate::fork::conectores::oauth_binding::issue_setup(&fixture.hub.store, &original).unwrap();
    let (status, response) = fixture
        .rpc(
            "donnaGuardRuntime",
            json!({"context":other.id,"connector":"notes",
        "transport":"http","bridgeId":uuid::Uuid::new_v4().to_string(),"method":"initialize","stale":true}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let rotated = fixture.saved();
    let raw = fixture.hub.store.kv_read("connectors").unwrap();
    let requests = fixture.requests.load(Ordering::SeqCst);
    let posts = fixture.token_calls.load(Ordering::SeqCst);
    gate.open();
    let (status, response) = call.await.unwrap();
    assert_eq!(status, StatusCode::OK, "rotação legítima mantém Context/Setup: {response}");
    assert_eq!(
        response["headers"]["Authorization"],
        format!("Bearer {}", rotated.oauth.as_ref().unwrap().access_token)
    );
    assert_eq!(fixture.requests.load(Ordering::SeqCst), requests, "401 OLD não exige outro POST no par NEW");
    assert_eq!(fixture.token_calls.load(Ordering::SeqCst), posts);
    assert_eq!(fixture.saved(), rotated);
    assert!(fixture.hub.store.kv_read("connectors").unwrap().eq(&raw));
    let namespace = fixture.hub.store.guard_runtime_namespace().unwrap();
    let cached = runtime::snapshot(&namespace, "notes", &setup.id, Some(&bridge)).unwrap();
    assert!(cached.iter().any(|value| value == &original.oauth.as_ref().unwrap().access_token));
    assert!(cached.iter().any(|value| value == &rotated.oauth.as_ref().unwrap().access_token));
}

// MODIFICADO pela DONNA Bot em 2026-11-04: fronteira adaptada do teste
// a_temporary_refresh_failure_keeps_the_grant, Codync e82c417424e06d1da71a865845f230d665b59ff1.
// Autoria upstream: leepokai; coautoria Claude Opus 5.5. Apache-2.0; THIRD_PARTY.md.
#[tokio::test]
async fn upstream03_temporary_refresh_keeps_canonical_login_and_pair_without_invalid_headers() {
    let f = Fixture::new().await;
    let state = f.start().await;
    assert!(f.callback(&state).await.contains("notes conectado"));
    market::update_connectors(&f.hub.store, |all| {
        all[0].oauth.as_mut().unwrap().expires_at = Some(0);
        Ok(())
    })
    .unwrap();
    let original = f.saved();
    let setup = crate::fork::conectores::oauth_binding::issue_setup(&f.hub.store, &original).unwrap();
    f.token_temporary.store(true, Ordering::SeqCst);
    let before = f.token_calls.load(Ordering::SeqCst);
    let (status, response) = f
        .rpc(
            "donnaGuardRuntime",
            json!({
                "context":setup.id,"connector":"notes","transport":"http",
                "bridgeId":uuid::Uuid::new_v4().to_string(),"method":"initialize"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(response.get("headers").is_none(), "token expirado não pode sair: {response}");
    assert_eq!(f.token_calls.load(Ordering::SeqCst), before + 1, "fixture deve alcançar o POST real");
    let saved = f.saved();
    assert!(saved.oauth.as_ref().unwrap().signed_in(), "503 encerrou o login canônico");
    assert_eq!(saved, original, "503 alterou o par ou a concessão originais");
}

impl Fixture {
    async fn refresh_ready(&self, expires: Option<u64>) -> market::Connector {
        let state = self.start().await;
        assert!(self.callback(&state).await.contains("notes conectado"));
        market::update_connectors(&self.hub.store, |all| {
            all[0].oauth.as_mut().unwrap().expires_at = expires;
            Ok(())
        })
        .unwrap();
        self.saved()
    }

    fn reply(&self, status: u16, body: Value) {
        *self.token_reply.lock().unwrap() = Some((StatusCode::from_u16(status).unwrap(), body));
    }

    async fn runtime_rpc(&self, original: &market::Connector, stale: bool) -> (StatusCode, Value) {
        let setup = crate::fork::conectores::oauth_binding::issue_setup(&self.hub.store, original).unwrap();
        self.rpc(
            "donnaGuardRuntime",
            json!({
                "context":setup.id,"connector":"notes","transport":"http",
                "bridgeId":uuid::Uuid::new_v4().to_string(),"method":"initialize","stale":stale
            }),
        )
        .await
    }
}

#[tokio::test]
async fn upstream03_temporary_fallback_requires_future_expiry_without_401() {
    // Casos de fronteira, sem produto cartesiano: cada falha cobre uma condição distinta.
    for (status, expires, stale, allowed) in [
        (503, Some(super::now() + 30), false, true),
        (429, Some(0), false, false),
        (408, Some(super::now() + 30), true, false),
        (500, None, true, false),
    ] {
        let f = Fixture::new().await;
        let original = f.refresh_ready(expires).await;
        f.reply(status, json!({"error":"invalid_grant"}));
        let before = f.token_calls.load(Ordering::SeqCst);
        let (code, response) = f.runtime_rpc(&original, stale).await;
        assert_eq!(f.token_calls.load(Ordering::SeqCst), before + 1);
        assert_eq!(f.saved(), original, "falha temporária não altera cadastro/par/binding");
        if allowed {
            assert_eq!(code, StatusCode::OK, "{response}");
            assert_eq!(
                response["headers"]["Authorization"],
                format!("Bearer {}", original.oauth.unwrap().access_token)
            );
        } else {
            assert_eq!(code, StatusCode::BAD_REQUEST, "{response}");
            assert!(response.get("headers").is_none());
        }
    }
}

#[tokio::test]
async fn upstream03_timeout_and_network_keep_pair_and_do_not_send_expired_access() {
    for timeout in [true, false] {
        let f = Fixture::new().await;
        f.refresh_ready(Some(0)).await;
        if !timeout {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let closed = format!("http://{}/token", listener.local_addr().unwrap());
            drop(listener);
            // Apenas fixture: endpoint local fechado, sem conta/servidor real ou novo grant.
            let mut all = market::connectors(&f.hub.store).unwrap();
            all[0].oauth.as_mut().unwrap().token_endpoint = closed;
            market::vault::write(&f.hub.store, "connectors", &serde_json::to_string(&all).unwrap()).unwrap();
        }
        let original = f.saved();
        let raw = f.hub.store.kv_read("connectors").unwrap();
        if timeout {
            f.token.arm();
        }
        let call = {
            let f = f.clone();
            let original = original.clone();
            tokio::spawn(async move { f.runtime_rpc(&original, false).await })
        };
        if timeout {
            f.token.wait().await;
            // Avança só o relógio Tokio após a barreira HTTP; sem esperar 20 s reais.
            tokio::time::pause();
            tokio::time::advance(std::time::Duration::from_secs(21)).await;
        }
        let (status, response) = call.await.unwrap();
        if timeout {
            tokio::time::resume();
            f.token.open();
        }
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(response.get("headers").is_none());
        assert_eq!(f.saved(), original);
        assert!(f.hub.store.kv_read("connectors").unwrap().eq(&raw));
    }
}

#[tokio::test]
async fn upstream03_inconclusive_or_empty_success_preserves_pair_without_headers() {
    for (status, body) in [
        (200, Value::Null),
        (200, json!({"access_token":" \t ","refresh_token":"unusable-rotation"})),
        (400, json!({"error":"invalid_target"})),
        (400, json!({"error":"unrecognized-secret-value","error_description":"never-log-this"})),
    ] {
        let f = Fixture::new().await;
        let original = f.refresh_ready(Some(super::now() + 30)).await;
        f.reply(status, body);
        let (code, response) = f.runtime_rpc(&original, false).await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert!(response.get("headers").is_none());
        assert_eq!(f.saved(), original, "inconclusivo não prova revogação nem rotação válida");
        assert!(!response.to_string().contains("never-log-this"));
        assert!(!response.to_string().contains("unrecognized-secret-value"));
    }
}

#[tokio::test]
async fn upstream03_confirmed_errors_affect_only_original_grant_or_registration() {
    for client in [false, true] {
        let f = Fixture::new().await;
        let original = f.refresh_ready(Some(0)).await;
        // Outra instância da mesma conta/URL não participa do CAS nem da limpeza.
        let mut other = original.clone();
        other.id = "sibling".into();
        other.name = "Sibling".into();
        other.oauth_binding = None;
        let mut all = market::connectors(&f.hub.store).unwrap();
        all.push(other);
        market::save_connectors(&f.hub.store, &all).unwrap();
        let sibling = market::connectors(&f.hub.store).unwrap().remove(1);
        let bot = serde_json::from_value(json!({"id":"off03","name":"Off","connectors":[]})).unwrap();
        f.hub.store.save_bot(&bot).unwrap();
        f.reply(
            if client { 401 } else { 400 },
            json!({"error":if client { "invalid_client" } else { "invalid_grant" }}),
        );
        let (status, response) = f.runtime_rpc(&original, false).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(response.get("headers").is_none());
        let saved = market::connectors(&f.hub.store).unwrap();
        assert_eq!(saved[1], sibling);
        let o = saved[0].oauth.as_ref().unwrap();
        assert!(!o.signed_in());
        assert!(o.refresh_token.is_none());
        assert!(o.expires_at.is_none());
        assert_eq!(o.client_id.is_empty(), client);
        assert_eq!(f.hub.store.bot("off03").unwrap().unwrap().config.connectors, Vec::<String>::new());
        let before = serde_json::to_value(&original).unwrap();
        let after = serde_json::to_value(&saved[0]).unwrap();
        assert_eq!(before["oauthBinding"]["instance"], after["oauthBinding"]["instance"]);
        assert_ne!(before["oauthBinding"]["generation"], after["oauthBinding"]["generation"]);
    }
}

#[tokio::test]
async fn upstream03_resource_is_exact_stored_grant_and_invalid_some_never_becomes_legacy() {
    for resource in [
        Some("https://resource.example/Exact/%2f?account=A%2FB"),
        None,
        Some(""),
        Some("https://resource.example/#fragment"),
    ] {
        let f = Fixture::new().await;
        f.refresh_ready(Some(0)).await;
        let mut all = serde_json::to_value(market::connectors(&f.hub.store).unwrap()).unwrap();
        if let Some(resource) = resource {
            all[0]["oauthBinding"]["grant"]["resource"] = json!(resource);
        } else {
            all[0]["oauthBinding"].as_object_mut().unwrap().remove("grant");
        }
        market::vault::write(&f.hub.store, "connectors", &all.to_string()).unwrap();
        let original = f.saved();
        let posts = f.token_calls.load(Ordering::SeqCst);
        let result = f.runtime_target(false).await;
        let valid = resource.is_none_or(|r| !r.is_empty() && !r.contains('#'));
        if valid {
            assert!(result.is_ok(), "{result:?}");
            let body = f.token_forms.lock().unwrap().last().unwrap().clone();
            let form = reqwest::Url::parse(&format!("http://fixture.invalid/?{body}")).unwrap();
            let resources: Vec<_> =
                form.query_pairs().filter(|(key, _)| key == "resource").map(|(_, v)| v.into_owned()).collect();
            assert_eq!(resources, resource.into_iter().map(str::to_owned).collect::<Vec<_>>());
            assert_eq!(f.token_calls.load(Ordering::SeqCst), posts + 1);
            assert_eq!(serde_json::to_value(f.saved()).unwrap()["oauthBinding"], all[0]["oauthBinding"]);
        } else {
            assert!(result.is_err());
            assert_eq!(f.token_calls.load(Ordering::SeqCst), posts);
            assert_eq!(f.saved(), original);
        }
    }
}

#[tokio::test]
async fn upstream03_zero_expiry_rotation_is_atomic_but_never_delivered_and_no_expiry_is_normal() {
    for expiry in [Some(0), None] {
        let f = Fixture::new().await;
        let original = f.refresh_ready(Some(0)).await;
        let mut answer = json!({"access_token":"rotated-access03","refresh_token":"rotated-refresh03"});
        if let Some(expiry) = expiry {
            answer["expires_in"] = json!(expiry);
        }
        f.reply(200, answer);
        let (status, response) = f.runtime_rpc(&original, false).await;
        if expiry.is_some() {
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert!(response.get("headers").is_none());
        } else {
            assert_eq!(status, StatusCode::OK, "{response}");
            assert_eq!(response["headers"]["Authorization"], "Bearer rotated-access03");
        }
        let saved = f.saved();
        let o = saved.oauth.as_ref().unwrap();
        assert_eq!(o.access_token, "rotated-access03");
        assert_eq!(o.refresh_token.as_deref(), Some("rotated-refresh03"));
        assert_eq!(o.expires_at.is_some(), expiry.is_some());
        assert_eq!(
            serde_json::to_value(&original).unwrap()["oauthBinding"],
            serde_json::to_value(&saved).unwrap()["oauthBinding"]
        );
        // Sem expiry novo não vira fallback com validade conhecida após 401.
        f.reply(503, Value::Null);
        assert!(f.runtime_target(true).await.is_err());
        assert_eq!(f.saved(), saved);
        let body = f.token_forms.lock().unwrap().last().unwrap().clone();
        assert!(body.contains("refresh_token=rotated-refresh03"), "não repetir refresh velho");
    }
}

#[tokio::test]
async fn upstream03_concurrent_candidate_cannot_bypass_expiry_or_same_access_401() {
    for same_access in [false, true] {
        let f = Fixture::new().await;
        let original = f.refresh_ready(Some(0)).await;
        let setup = crate::fork::conectores::oauth_binding::issue_setup(&f.hub.store, &original).unwrap();
        let intent =
            crate::fork::conectores::oauth_runtime::RuntimeIntent::capture(&f.hub, &setup.id, "notes", "initialize")
                .unwrap();
        // Janela entre a fotografia inicial e a aquisição do lock de refresh.
        market::update_connectors(&f.hub.store, |all| {
            let o = all[0].oauth.as_mut().unwrap();
            if !same_access {
                o.access_token = "concurrent-but-expired".into();
            }
            o.refresh_token = Some("new-concurrent-refresh".into());
            o.expires_at = Some(if same_access { super::now() + 3600 } else { super::now() });
            Ok(())
        })
        .unwrap();
        let current = f.saved();
        let posts = f.token_calls.load(Ordering::SeqCst);
        let result = super::fresh(&f.hub, &original, original.oauth.as_ref().unwrap(), &intent, same_access).await;
        assert!(result.is_err(), "par diferente não prova acesso válido/não recusado");
        assert_eq!(f.saved(), current);
        assert_eq!(f.token_calls.load(Ordering::SeqCst), posts);
    }
}

#[tokio::test]
async fn upstream03_temporary_response_rechecks_original_authority_and_future_expiry_after_network() {
    use crate::LockExt;
    for revoke in [false, true] {
        let f = Fixture::new().await;
        let original = f.refresh_ready(Some(super::now() + 30)).await;
        let setup = crate::fork::conectores::oauth_binding::issue_setup(&f.hub.store, &original).unwrap();
        f.reply(503, json!({"error":"invalid_grant"}));
        f.token.arm();
        let body = json!({"context":setup.id,"connector":"notes","transport":"http",
            "bridgeId":uuid::Uuid::new_v4().to_string(),"method":"initialize"});
        let call = {
            let f = f.clone();
            tokio::spawn(async move { f.rpc("donnaGuardRuntime", body).await })
        };
        f.token.wait().await;
        if revoke {
            f.hub.store.db.locked().execute("DELETE FROM donna_guard_setup WHERE id=?1", [&setup.id]).unwrap();
        } else {
            // O mesmo grant passa a ter acesso vencido: o snapshot antigo não é fallback.
            market::update_connectors(&f.hub.store, |all| {
                all[0].oauth.as_mut().unwrap().expires_at = Some(super::now());
                Ok(())
            })
            .unwrap();
        }
        let expected = f.saved();
        f.token.open();
        let (status, response) = call.await.unwrap();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(response.get("headers").is_none());
        assert_eq!(f.saved(), expected);
    }
}

type DeliveryClocks = std::collections::HashMap<usize, Arc<AtomicU64>>;
static DELIVERY_CLOCKS: std::sync::LazyLock<std::sync::Mutex<DeliveryClocks>> =
    std::sync::LazyLock::new(Default::default);

/// Relógio de fixture capturado junto do target. A última leitura é atômica, sem lock.
pub(crate) fn delivery_clock(hub: &Hub) -> Option<Arc<AtomicU64>> {
    DELIVERY_CLOCKS.lock().unwrap().get(&(std::ptr::from_ref(hub) as usize)).cloned()
}

struct DeliveryClock {
    hub: usize,
    now: Arc<AtomicU64>,
}
impl DeliveryClock {
    fn new(hub: &Hub) -> Self {
        let clock = Self { hub: std::ptr::from_ref(hub) as usize, now: Arc::new(AtomicU64::new(super::now())) };
        DELIVERY_CLOCKS.lock().unwrap().insert(clock.hub, clock.now.clone());
        clock
    }
}
impl Drop for DeliveryClock {
    fn drop(&mut self) {
        DELIVERY_CLOCKS.lock().unwrap().remove(&self.hub);
    }
}

#[tokio::test]
async fn upstream03_last_delivery_checks_target_a_after_entire_guard_even_with_current_b() {
    use crate::guard_hub::mcp::runtime;

    for (rotate, expire) in [(false, true), (true, true), (true, false)] {
        let f = Fixture::new().await;
        let expires = super::now() + 30;
        let original = f.refresh_ready(Some(expires)).await;
        let clock = DeliveryClock::new(&f.hub);
        let setup = crate::fork::conectores::oauth_binding::issue_setup(&f.hub.store, &original).unwrap();
        let bridge = uuid::Uuid::new_v4().to_string();
        let namespace = f.hub.store.guard_runtime_namespace().unwrap();
        f.reply(503, json!({"error":"invalid_grant"}));
        let resolved = Gate::new();
        resolved.arm();
        let guarded = Gate::new();
        guarded.arm();
        {
            let mut windows = RUNTIME_WINDOWS.lock().unwrap();
            let hub = Arc::as_ptr(&f.hub) as usize;
            windows.insert((hub, RuntimeWindow::AfterTargetResolved), resolved.clone());
            windows.insert((hub, RuntimeWindow::AfterDeliveryGuard), guarded.clone());
        }
        let body = json!({"context":setup.id,"connector":"notes","transport":"http",
            "bridgeId":bridge,"method":"initialize"});
        let call = {
            let f = f.clone();
            tokio::spawn(async move { f.rpc("donnaGuardRuntime", body).await })
        };
        resolved.wait().await;
        if rotate {
            market::update_connectors(&f.hub.store, |all| {
                let oauth = all[0].oauth.as_mut().unwrap();
                oauth.access_token = "current-b-valid".into();
                oauth.refresh_token = Some("refresh-b-valid".into());
                oauth.expires_at = Some(expires + 3600);
                Ok(())
            })
            .unwrap();
        }
        let canonical = f.saved();
        let raw = f.hub.store.kv_read("connectors").unwrap();
        resolved.open();
        guarded.wait().await;
        // A closure, validate_db e validate_live da guarda JÁ terminaram.
        // Vence exatamente A; B continua futuro. Só a última entrega lê este relógio.
        clock.now.store(if expire { expires } else { expires - 1 }, Ordering::SeqCst);
        guarded.open();
        let (status, response) = call.await.unwrap();
        if expire {
            assert_eq!(status, StatusCode::BAD_REQUEST, "Bearer A venceu depois da guarda: {response}");
            assert!(response.get("headers").is_none());
            assert!(response.get("url").is_none());
        } else {
            assert_eq!(status, StatusCode::OK, "{response}");
            assert_eq!(
                response["headers"]["Authorization"],
                format!("Bearer {}", original.oauth.as_ref().unwrap().access_token)
            );
            assert_eq!(response.as_object().unwrap().len(), 4, "testemunho nunca sai no DTO");
            assert!(!response.to_string().contains("current-b-valid"), "não trocar A por B na entrega");
        }
        assert_eq!(f.saved(), canonical, "expiração de entrega não apaga/renova login nem par B");
        assert!(f.hub.store.kv_read("connectors").unwrap().eq(&raw));
        let cache = runtime::snapshot(&namespace, "notes", &setup.id, Some(&bridge)).unwrap();
        assert!(cache.iter().any(|v| v == &original.oauth.as_ref().unwrap().access_token));
        if rotate {
            assert!(cache.iter().any(|v| v == "current-b-valid"), "cache de A+B permanece para redação em voo");
        }
    }
}

#[tokio::test]
async fn upstream03_last_delivery_preserves_manual_headers_without_oauth() {
    let f = Fixture::new().await;
    market::update_connectors(&f.hub.store, |all| {
        all[0].oauth = None;
        all[0].headers.insert("Authorization".into(), "Bearer manual-opaque".into());
        Ok(())
    })
    .unwrap();
    // A configuração atual exige novo binding; a fixture representa remoto manual puro.
    let mut all = market::connectors(&f.hub.store).unwrap();
    all[0].oauth = None;
    market::vault::write(&f.hub.store, "connectors", &serde_json::to_string(&all).unwrap()).unwrap();
    let original = f.saved();
    let clock = DeliveryClock::new(&f.hub);
    clock.now.store(u64::MAX, Ordering::SeqCst);
    let posts = f.token_calls.load(Ordering::SeqCst);
    let (status, response) = f.runtime_rpc(&original, false).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["headers"]["Authorization"], "Bearer manual-opaque");
    assert_eq!(response.as_object().unwrap().len(), 4);
    assert_eq!(f.token_calls.load(Ordering::SeqCst), posts);
    assert_eq!(f.saved(), original);
}

#[tokio::test]
async fn upstream03_invalid_present_refresh_preserves_pair_and_absent_refresh_remains_optional() {
    // Um eixo só: presença/forma do refresh em sucesso200, com acesso antigo ainda futuro.
    for refresh in [json!(" \t "), Value::Null, json!(17)] {
        let f = Fixture::new().await;
        let original = f.refresh_ready(Some(super::now() + 30)).await;
        let raw = f.hub.store.kv_read("connectors").unwrap();
        let posts = f.token_calls.load(Ordering::SeqCst);
        f.reply(200, json!({"access_token":"new-with-invalid-refresh","refresh_token":refresh,"expires_in":3600}));
        let (status, response) = f.runtime_rpc(&original, false).await;
        assert_eq!(f.token_calls.load(Ordering::SeqCst), posts + 1, "atingiu endpoint HTTP real");
        assert_eq!(f.saved(), original, "não gravar access novo com refresh velho nem encerrar login");
        assert!(f.hub.store.kv_read("connectors").unwrap().eq(&raw), "resposta inconclusiva não grava ciphertext");
        assert_eq!(status, StatusCode::BAD_REQUEST, "refresh presente inválido não é ausência: {response}");
        assert!(response.get("headers").is_none(), "não liberar novo nem fallback antigo");
        assert!(response.get("url").is_none());
    }
    // Ausência verdadeira continua sendo a resposta OAuth normal sem rotação do refresh.
    let f = Fixture::new().await;
    let original = f.refresh_ready(Some(0)).await;
    f.reply(200, json!({"access_token":"new-with-absent-refresh","expires_in":3600}));
    let (status, response) = f.runtime_rpc(&original, false).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["headers"]["Authorization"], "Bearer new-with-absent-refresh");
    let saved = f.saved();
    let oauth = saved.oauth.as_ref().unwrap();
    assert_eq!(oauth.access_token, "new-with-absent-refresh");
    assert_eq!(oauth.refresh_token, original.oauth.as_ref().unwrap().refresh_token);
    assert!(oauth.expires_at.is_some_and(|expiry| expiry > super::now()));
    assert_eq!(
        serde_json::to_value(&saved).unwrap()["oauthBinding"],
        serde_json::to_value(&original).unwrap()["oauthBinding"]
    );
}

/// M2-A: URL do conector com prefixo falso, usuário ou `#` é recusada antes de qualquer pedido.
#[tokio::test]
async fn m2a_instalacao_recusa_endereco_falso_antes_da_rede() {
    let f = Fixture::new().await;
    let port = f.provider.rsplit(':').next().unwrap().to_owned();
    let requests = f.requests.load(Ordering::SeqCst);
    for url in [
        format!("http://localhost.:{port}/mcp"),
        format!("http://localhost.exemplo.invalid:{port}/mcp"),
        format!("http://u:p@127.0.0.1:{port}/mcp"),
        format!("{}/mcp#frag", f.provider),
    ] {
        let (status, body) = f.rpc("installConnector", json!({"name":"falso","url":url})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{url}: {body}");
        assert!(!body.to_string().contains(&port), "erro fixo sem URL: {body}");
    }
    assert_eq!(f.requests.load(Ordering::SeqCst), requests);
    assert_eq!(market::connectors(&f.hub.store).unwrap().len(), 1);
}

/// M2-A: servidor de autorização legítimo em outra origem continua entrando.
#[tokio::test]
async fn m2a_servidor_de_login_em_outra_origem_continua_valido() {
    let f = Fixture::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let other = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let (count, provider, issuer) = (hits.clone(), f.provider.clone(), other.clone());
    let router = Router::new().route(
        "/.well-known/oauth-authorization-server",
        routing::get(move || {
            count.fetch_add(1, Ordering::SeqCst);
            as_metadata(issuer.clone(), provider.clone())()
        }),
    );
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    *f.issuer.lock().unwrap() = other;
    let state = f.start().await;
    assert!(f.callback(&state).await.contains("notes conectado"));
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    task.abort();
}

/// M2-A: 307/308 nunca levam pedido ao `Location`; metadados seguem no alternativo,
/// DCR e token recusam, refresh vira `Retry { fallback: false }` sem sign-out.
#[tokio::test]
async fn m2a_redirecionamentos_nunca_seguem_location() {
    let trap = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let location = format!("http://{}/destino", trap.local_addr().unwrap());
    let trapped = Arc::new(AtomicUsize::new(0));
    let count = trapped.clone();
    let trap_task = tokio::spawn(async move {
        while trap.accept().await.is_ok() {
            count.fetch_add(1, Ordering::SeqCst);
        }
    });
    let token = json!({"access_token":"redirecionado","refresh_token":"redirecionado","expires_in":3600});
    for status in [307, 308] {
        let code = StatusCode::from_u16(status).unwrap();
        let redirect = |f: &Fixture, path: &str, body: Value| {
            f.redirects.lock().unwrap().insert(path.to_owned(), (code, location.clone(), body));
        };

        // Metadados: o primeiro well-known redireciona; a descoberta segue no alternativo.
        let f = Fixture::new().await;
        redirect(&f, "/.well-known/oauth-protected-resource/mcp", json!({"authorization_servers":[location]}));
        redirect(&f, "/.well-known/oauth-authorization-server", json!({"authorization_endpoint":location}));
        let state = f.start().await;
        assert!(f.callback(&state).await.contains("notes conectado"), "HTTP {status}");
        let paths = f.paths.lock().unwrap().clone();
        assert!(paths.iter().any(|p| p == "/.well-known/oauth-protected-resource"), "{paths:?}");
        assert!(paths.iter().any(|p| p == "/.well-known/openid-configuration"), "{paths:?}");

        // DCR: 3xx com JSON válido não vira cadastro.
        let f = Fixture::new().await;
        redirect(&f, "/register", json!({"client_id":"redirecionado"}));
        let (answer, body) = f.rpc("connectorSignIn", json!({"id":"notes"})).await;
        assert_eq!(answer, StatusCode::BAD_REQUEST, "HTTP {status}: {body}");
        assert!(!body.to_string().contains(&location), "{body}");
        let oauth = f.saved().oauth.unwrap();
        assert!(oauth.client_id.is_empty() && !oauth.signed_in(), "HTTP {status}");

        // Token no login: recusado com erro fixo, nada gravado.
        let f = Fixture::new().await;
        let state = f.start().await;
        redirect(&f, "/token", token.clone());
        assert!(!f.callback(&state).await.contains("notes conectado"), "HTTP {status}");
        assert!(!f.saved().oauth.unwrap().signed_in(), "HTTP {status}");

        // Token no refresh: acesso ainda futuro; fallback:true liberaria, fallback:false recusa.
        let f = Fixture::new().await;
        let original = f.refresh_ready(Some(super::now() + 30)).await;
        redirect(&f, "/token", token.clone());
        let (answer, response) = f.runtime_rpc(&original, false).await;
        assert_eq!(answer, StatusCode::BAD_REQUEST, "HTTP {status}: {response}");
        assert!(response.get("headers").is_none());
        assert_eq!(f.saved(), original, "sem sign-out nem troca do par");
    }
    assert_eq!(trapped.load(Ordering::SeqCst), 0, "Location recebeu pedido");
    trap_task.abort();
}

/// M2-B: emissor em outra porta, anunciado com barra final, fica original na tentativa
/// e no Grant; a consulta apara a barra só para montar o well-known.
#[tokio::test]
async fn m2b_emissor_com_barra_em_outra_origem_fica_original_no_grant() {
    let f = Fixture::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let other = format!("http://{}/", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let (count, provider, issuer) = (hits.clone(), f.provider.clone(), other.clone());
    let router = Router::new().route(
        "/.well-known/oauth-authorization-server",
        routing::get(move || {
            count.fetch_add(1, Ordering::SeqCst);
            as_metadata(issuer.clone(), provider.clone())()
        }),
    );
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    *f.issuer.lock().unwrap() = other.clone();
    let state = f.start().await;
    let grant = &serde_json::to_value(f.saved()).unwrap()["oauthBinding"]["grant"];
    assert_eq!(grant["issuer"], json!(other), "{grant}");
    assert!(f.callback(&state).await.contains("notes conectado"));
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    task.abort();
}

/// M2-B: metadados de outro emissor são descartados; o próximo candidato do mesmo emissor
/// pode concluir. Sem nenhum candidato válido: erro fixo, sem cadastro nem token.
#[tokio::test]
async fn m2b_metadados_de_outro_emissor_nao_alimentam_o_login() {
    let sentinela = "https://sentinela-remota.invalid";
    let metadados = |issuer: Value| {
        json!({"issuer":issuer,"authorization_endpoint":format!("{sentinela}/authorize"),
            "token_endpoint":format!("{sentinela}/token"),"registration_endpoint":format!("{sentinela}/register")})
    };
    let responde = |f: &Fixture, path: &str, body: Value| {
        f.redirects.lock().unwrap().insert(path.to_owned(), (StatusCode::OK, sentinela.to_owned(), body));
    };

    // Primeiro candidato de outro emissor; o OpenID do mesmo emissor conclui.
    let f = Fixture::new().await;
    responde(&f, "/.well-known/oauth-authorization-server", metadados(json!(format!("{}/", f.provider))));
    let state = f.start().await;
    assert!(f.callback(&state).await.contains("notes conectado"));
    assert!(f.paths.lock().unwrap().iter().any(|p| p == "/.well-known/openid-configuration"));
    let grant = &serde_json::to_value(f.saved()).unwrap()["oauthBinding"]["grant"];
    assert_eq!(grant["issuer"], json!(f.provider), "{grant}");

    // Todos os candidatos incompatíveis: ausente, nulo, número, outro valor, barra final.
    let f = Fixture::new().await;
    let mut ausente = metadados(Value::Null);
    ausente.as_object_mut().unwrap().remove("issuer");
    let casos = [ausente, metadados(Value::Null), metadados(json!(1)), metadados(json!(sentinela))];
    for body in casos.into_iter().chain([metadados(json!(format!("{}/", f.provider)))]) {
        let issuer = body.get("issuer").cloned().unwrap_or_default();
        for path in ["/.well-known/oauth-authorization-server", "/.well-known/openid-configuration"] {
            responde(&f, path, body.clone());
        }
        let (status, answer) = f.rpc("connectorSignIn", json!({"id":"notes"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{issuer}: {answer}");
        let text = answer.to_string();
        assert!(text.contains(crate::fork::conectores::oauth_urls::EMISSOR), "{text}");
        assert!(!text.contains("sentinela") && !text.contains(&f.provider), "{text}");
        assert!(!f.paths.lock().unwrap().iter().any(|p| p == "/register" || p == "/token"));
        assert_eq!(f.token_calls.load(Ordering::SeqCst), 0);
        let oauth = f.saved().oauth.unwrap_or_default();
        assert!(oauth.client_id.is_empty() && !oauth.signed_in(), "{issuer}");
    }
}

/// M3-A: metadados do mesmo emissor sem PKCE S256 recusam com a frase exata que o app
/// mostra, sem cadastro, token, Grant novo nem link; vale também para cliente já cadastrado.
#[tokio::test]
async fn m3a_metadados_sem_pkce_s256_recusam_antes_de_cadastro_e_link() {
    use crate::fork::conectores::oauth_urls::PKCE;
    let sem_pkce = |f: &Fixture, metodos: Option<Value>| {
        let mut body = json!({"issuer":f.provider,"authorization_endpoint":format!("{}/authorize",f.provider),
            "token_endpoint":format!("{}/token",f.provider),"registration_endpoint":format!("{}/register",f.provider)});
        if let Some(m) = metodos {
            body["code_challenge_methods_supported"] = m;
        }
        for path in ["/.well-known/oauth-authorization-server", "/.well-known/openid-configuration"] {
            f.redirects.lock().unwrap().insert(path.to_owned(), (StatusCode::OK, String::new(), body.clone()));
        }
    };
    let recusa = |f: &Fixture, answer: (StatusCode, Value)| {
        assert_eq!(answer.0, StatusCode::BAD_REQUEST, "{}", answer.1);
        assert_eq!(answer.1, json!({"error": PKCE}));
        let saved = serde_json::to_value(f.saved()).unwrap();
        assert!(saved["oauthBinding"]["grant"].is_null(), "{saved}");
        assert!(!f.saved().oauth.unwrap_or_default().signed_in());
    };

    // Cliente novo: ausente e só plain recusam sem chegar ao cadastro ou ao token.
    for metodos in [None, Some(json!(["plain"]))] {
        let f = Fixture::new().await;
        sem_pkce(&f, metodos.clone());
        recusa(&f, f.rpc("connectorSignIn", json!({"id":"notes"})).await);
        assert!(!f.paths.lock().unwrap().iter().any(|p| p == "/register" || p == "/token"), "{metodos:?}");
        assert!(f.saved().oauth.unwrap_or_default().client_id.is_empty(), "{metodos:?}");
    }

    // Cliente já cadastrado: o login anterior termina, o novo para no PKCE, sem novo cadastro.
    let f = Fixture::new().await;
    let state = f.start().await;
    assert!(f.callback(&state).await.contains("notes conectado"));
    let tokens = f.token_calls.load(Ordering::SeqCst);
    f.paths.lock().unwrap().clear();
    sem_pkce(&f, Some(json!("S256")));
    recusa(&f, f.rpc("connectorSignIn", json!({"id":"notes"})).await);
    assert!(!f.paths.lock().unwrap().iter().any(|p| p == "/register" || p == "/token"));
    assert_eq!(f.token_calls.load(Ordering::SeqCst), tokens);
    assert_eq!(f.saved().oauth.unwrap().client_id, "fixture-client");
}

/// M3-A: com S256 anunciado (inclusive ao lado de plain), o link usa S256 e o challenge
/// é o SHA-256 do verifier enviado na troca do código.
#[tokio::test]
async fn m3a_link_usa_s256_e_verifier_corresponde_ao_challenge() {
    for metodos in [None, Some(json!(["plain", "S256"]))] {
        let f = Fixture::new().await;
        if let Some(m) = &metodos {
            let mut body = as_metadata(f.provider.clone(), f.provider.clone())().into_inner().0;
            body["code_challenge_methods_supported"] = m.clone();
            f.redirects
                .lock()
                .unwrap()
                .insert("/.well-known/oauth-authorization-server".into(), (StatusCode::OK, String::new(), body));
        }
        let (status, body) = f.rpc("connectorSignIn", json!({"id":"notes"})).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let link = reqwest::Url::parse(body["url"].as_str().unwrap()).unwrap();
        let q: std::collections::HashMap<_, _> = link.query_pairs().into_owned().collect();
        assert_eq!(q["code_challenge_method"], "S256", "{metodos:?}");
        assert!(f.callback(&q["state"]).await.contains("notes conectado"));
        let form = f.token_forms.lock().unwrap().last().unwrap().clone();
        let verifier = reqwest::Url::parse(&format!("http://x/?{form}"))
            .unwrap()
            .query_pairs()
            .find(|(k, _)| k == "code_verifier")
            .unwrap()
            .1
            .into_owned();
        let esperado = crate::remote::crypto::b64(&crate::remote::crypto::sha256(&[verifier.as_bytes()]));
        assert_eq!(q["code_challenge"], esperado, "{metodos:?}");
    }
}

/// M3-B: metadados do emissor com `authorization_response_iss_parameter_supported` ajustado.
/// `None` remove o campo (provedor legado); trocar depois do início não muda a tentativa.
fn m3b_anuncio(f: &Fixture, anuncio: Option<Value>) {
    let mut body = as_metadata(f.provider.clone(), f.provider.clone())().into_inner().0;
    if let Some(v) = anuncio {
        body["authorization_response_iss_parameter_supported"] = v;
    }
    f.redirects
        .lock()
        .unwrap()
        .insert("/.well-known/oauth-authorization-server".into(), (StatusCode::OK, String::new(), body));
}

impl Fixture {
    /// Retorno pelo loopback com pares arbitrários (inclusive repetidos), codificados uma vez.
    async fn m3b_retorno(&self, pares: &[(&str, &str)]) -> String {
        let url = reqwest::Url::parse_with_params(&format!("{}/oauth/callback", self.api), pares).unwrap();
        crate::http().get(url).send().await.unwrap().text().await.unwrap()
    }
}

/// M3-B: callback HTTP do Host com anúncio `true`: igual conclui; ausente, diferente, erro
/// alheio e campo repetido recusam com frase fixa, sem token POST nem eco externo; replay recusa.
#[tokio::test]
async fn m3b_callback_host_confere_iss_antes_do_codigo_e_do_erro() {
    use crate::fork::conectores::oauth_iss::{FORMA, RETORNO};
    let f = Fixture::new().await;
    m3b_anuncio(&f, Some(json!(true)));
    let provedor = f.provider.clone();
    let recusa = |html: &str, frase: &str| {
        assert!(html.contains(frase), "{html}");
        assert!(!html.contains("notes conectado") && !html.contains("SENTINELA"), "{html}");
        assert!(!html.contains(&provedor), "{html}");
    };

    let ausente = f.start().await;
    recusa(&f.m3b_retorno(&[("state", ausente.as_str()), ("code", "fixture-code")]).await, RETORNO);
    // Consumido: o mesmo state com o emissor certo não conclui depois.
    let replay =
        f.m3b_retorno(&[("state", ausente.as_str()), ("code", "fixture-code"), ("iss", f.provider.as_str())]).await;
    assert!(replay.contains("expirou") && !replay.contains("notes conectado"), "{replay}");

    for outro in [format!("{}/", f.provider), f.provider.to_uppercase(), format!(" {}", f.provider)] {
        let state = f.start().await;
        recusa(
            &f.m3b_retorno(&[("state", state.as_str()), ("code", "fixture-code"), ("iss", outro.as_str())]).await,
            RETORNO,
        );
    }
    let erro = f.start().await;
    let html = f
        .m3b_retorno(&[
            ("state", erro.as_str()),
            ("error", "access_denied"),
            ("error_description", "SENTINELA-EXTERNA"),
            ("error_uri", "https://SENTINELA.invalid/x"),
            ("iss", "https://alheio.invalid"),
        ])
        .await;
    recusa(&html, RETORNO);
    let repetido = f.start().await;
    let html = f
        .m3b_retorno(&[
            ("state", repetido.as_str()),
            ("code", "fixture-code"),
            ("iss", f.provider.as_str()),
            ("iss", "https://x"),
        ])
        .await;
    recusa(&html, FORMA);
    // Metadados trocados depois do início não afrouxam a tentativa já guardada.
    let congelada = f.start().await;
    m3b_anuncio(&f, None);
    recusa(&f.m3b_retorno(&[("state", congelada.as_str()), ("code", "fixture-code")]).await, RETORNO);
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 0);
    assert!(!f.saved().oauth.unwrap_or_default().signed_in());

    m3b_anuncio(&f, Some(json!(true)));
    let certo = f.start().await;
    let html =
        f.m3b_retorno(&[("state", certo.as_str()), ("code", "fixture-code"), ("iss", f.provider.as_str())]).await;
    assert!(html.contains("notes conectado"), "{html}");
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 1);
}

/// M3-B: anúncio presente com outro tipo recusa o início antes de cadastro, Grant, pending e link.
#[tokio::test]
async fn m3b_anuncio_de_tipo_invalido_recusa_antes_de_cadastro() {
    use crate::fork::conectores::oauth_iss::ANUNCIO;
    for anuncio in [json!("true"), Value::Null, json!(1)] {
        let f = Fixture::new().await;
        m3b_anuncio(&f, Some(anuncio.clone()));
        let (status, body) = f.rpc("connectorSignIn", json!({"id":"notes"})).await;
        assert_eq!((status, body), (StatusCode::BAD_REQUEST, json!({"error": ANUNCIO})), "{anuncio}");
        assert!(!f.paths.lock().unwrap().iter().any(|p| p == "/register" || p == "/token"), "{anuncio}");
        let saved = serde_json::to_value(f.saved()).unwrap();
        assert!(saved["oauthBinding"]["grant"].is_null(), "{saved}");
        assert!(f.saved().oauth.unwrap_or_default().client_id.is_empty(), "{anuncio}");
        // Outros testes rodam em paralelo; nenhum pending desta fixture pode ter nascido.
        let alvo = format!("{}/mcp", f.provider);
        let deste = super::PENDING
            .lock()
            .unwrap()
            .as_ref()
            .map_or(0, |m| m.values().filter(|p| p.snapshot.url.as_deref() == Some(alvo.as_str())).count());
        assert_eq!(deste, 0, "{anuncio}");
    }
}

/// M3-B: `connectorSignInFinish` pelo app com a Authority original. Legado (sem anúncio):
/// ausência segue; `iss` errado ou de outro tipo recusa sem token; o anúncio novo não vale
/// para a tentativa já iniciada. Com anúncio `true`, o emissor exato conclui.
#[tokio::test]
async fn m3b_finish_do_app_confere_iss_com_authority_original() {
    use crate::fork::conectores::oauth_iss::{FORMA, RETORNO};
    use crate::{
        api::devices::Caller,
        store::{Device, DeviceSource, Scope},
    };
    let f = Fixture::new().await;
    f.hub.store.kv_set("test_cloud_url", "https://fixture.invalid").unwrap();
    f.hub.cloud_synced.store(true, Ordering::Release);
    let d = Device {
        key: "m3b-operator".into(),
        name: "fixture".into(),
        platform: "fixture".into(),
        source: DeviceSource::Account,
        grant_id: Some("account-grant".into()),
        scopes: vec![Scope::Control],
        lease_until: Some(crate::store::now_ms() + 60000),
        created_at: 1,
        last_seen_at: None,
    };
    f.hub.store.put_device(&d).unwrap();
    let caller = || Caller::Device { key: d.key.clone(), scopes: vec![Scope::Control], autoridade: None };
    let iniciar = async || {
        let result = api::dispatch(&f.hub, &caller(), "connectorSignIn", json!({"id":"notes"})).await.unwrap();
        assert_eq!(result["callback"], "app");
        let link = reqwest::Url::parse(result["url"].as_str().unwrap()).unwrap();
        link.query_pairs().find(|(k, _)| k == "state").unwrap().1.into_owned()
    };
    let concluir = async |body: Value| api::dispatch(&f.hub, &caller(), "connectorSignInFinish", body).await;

    let errado = iniciar().await;
    let err = concluir(json!({"state":errado,"error":"SENTINELA","iss":format!("{}/", f.provider)})).await;
    assert_eq!(err.unwrap_err().to_string(), RETORNO);
    let tipo = iniciar().await;
    for iss in [json!(1), json!(true), json!([f.provider])] {
        let err = concluir(json!({"state":tipo,"code":"fixture","iss":iss})).await;
        assert_eq!(err.unwrap_err().to_string(), FORMA, "{iss}");
    }
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 0);

    // Legado: ausência e `null` seguem; anúncio surgido depois do início não muda a tentativa.
    let legado = iniciar().await;
    m3b_anuncio(&f, Some(json!(true)));
    let answer = concluir(json!({"state":legado,"code":"fixture","iss":null})).await.unwrap();
    assert_eq!(answer["connector"]["auth"], "signedIn");
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 1);

    let exigido = iniciar().await;
    let err = concluir(json!({"state":exigido,"code":"fixture"})).await;
    assert_eq!(err.unwrap_err().to_string(), RETORNO);
    let exigido = iniciar().await;
    let answer = concluir(json!({"state":exigido,"code":"fixture","iss":f.provider})).await.unwrap();
    assert_eq!(answer["connector"]["auth"], "signedIn");
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 2);
    // Outra autoridade com o state do app continua recusada antes do emissor.
    let alheio = iniciar().await;
    let err = api::dispatch(
        &f.hub,
        &Caller::Local,
        "connectorSignInFinish",
        json!({"state":alheio,"code":"fixture","iss":f.provider}),
    )
    .await;
    assert!(err.is_err());
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 2);
}

/// M3-C: rejeitar o documento inteiro antes de AS/DCR/Grant/Pending/link.
#[tokio::test]
async fn m3c_prm_invalido_recusa_login_e_sonda_sem_consumir_as() {
    let f = Fixture::new().await;
    let path = "/.well-known/oauth-protected-resource/mcp";
    let root = "/.well-known/oauth-protected-resource";
    for resource in [
        None,
        Some(Value::Null),
        Some(json!(7)),
        Some(json!("")),
        Some(json!(format!("{}/mcp/", f.provider))),
        Some(json!("https://outro.invalid/mcp")),
    ] {
        let mut body = json!({"authorization_servers":[f.provider],"scopes_supported":["SENTINELA"]});
        if let Some(resource) = resource {
            body["resource"] = resource;
        }
        {
            let mut replies = f.redirects.lock().unwrap();
            for p in [path, root] {
                replies.insert(p.into(), (StatusCode::OK, String::new(), body.clone()));
            }
            replies.insert("/mcp".into(), (StatusCode::OK, String::new(), json!({})));
        }
        assert!(!super::required(&format!("{}/mcp", f.provider), &std::collections::BTreeMap::new()).await);
        f.paths.lock().unwrap().clear();
        let (status, reply) = f.rpc("connectorSignIn", json!({"id":"notes"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{reply}");
        assert!(reply.to_string().contains(crate::fork::conectores::oauth_resource::ERRO), "{reply}");
        assert!(!reply.to_string().contains(&f.provider));
        assert!(reply.get("url").is_none());
        assert!(f.saved().oauth_binding.as_ref().unwrap().resource().is_none());
        assert_eq!(f.token_calls.load(Ordering::SeqCst), 0);
        assert!(
            !f.paths.lock().unwrap().iter().any(|p| p.contains("authorization-server")
                || p.contains("openid")
                || p == "/register"
                || p == "/token")
        );
    }
    f.redirects.lock().unwrap().remove("/mcp");
    assert!(super::required(&format!("{}/mcp", f.provider), &std::collections::BTreeMap::new()).await);
}

/// M3-C: raiz é outra identidade; o valor aceito fica preso à tentativa e concessão.
#[tokio::test]
async fn m3c_root_fallback_conserva_resource_do_link_ao_refresh_mesmo_grant() {
    let f = Fixture::new().await;
    f.redirects.lock().unwrap().insert(
        "/.well-known/oauth-protected-resource/mcp".into(),
        (
            StatusCode::OK,
            String::new(),
            json!({"resource":format!("{}/errado", f.provider),
            "authorization_servers":["https://SENTINELA.invalid"],"scopes_supported":["SENTINELA"]}),
        ),
    );
    let (status, body) = f.rpc("connectorSignIn", json!({"id":"notes"})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let link = reqwest::Url::parse(body["url"].as_str().unwrap()).unwrap();
    let q: std::collections::HashMap<_, _> = link.query_pairs().into_owned().collect();
    let resources: Vec<_> = link.query_pairs().filter(|(k, _)| k == "resource").map(|(_, v)| v.into_owned()).collect();
    assert_eq!(resources.as_slice(), std::slice::from_ref(&f.provider));
    assert_eq!(q["scope"], "read");
    assert_eq!(f.saved().oauth_binding.as_ref().unwrap().resource(), Some(f.provider.as_str()));
    let mut grant = f.saved().oauth_binding;
    grant.as_mut().unwrap().validate_login_scopes(&json!({})).unwrap();
    // Redescoberta no callback/refresh trocaria a identidade: nenhum dos dois pode consultá-la.
    f.redirects.lock().unwrap().insert(
        "/.well-known/oauth-protected-resource".into(),
        (StatusCode::OK, String::new(), json!({"resource":format!("{}/mcp", f.provider)})),
    );
    f.paths.lock().unwrap().clear();
    assert!(f.callback(&q["state"]).await.contains("notes conectado"));
    market::update_connectors(&f.hub.store, |all| {
        all[0].oauth.as_mut().unwrap().expires_at = Some(0);
        Ok(())
    })
    .unwrap();
    f.runtime_target(false).await.unwrap();
    assert_eq!(f.saved().oauth_binding, grant);
    let forms = f.token_forms.lock().unwrap();
    assert_eq!(forms.len(), 2);
    for form in forms.iter() {
        let url = reqwest::Url::parse(&format!("http://x/?{form}")).unwrap();
        let resources: Vec<_> =
            url.query_pairs().filter(|(k, _)| k == "resource").map(|(_, v)| v.into_owned()).collect();
        assert_eq!(resources.as_slice(), std::slice::from_ref(&f.provider));
    }
    assert!(!f.paths.lock().unwrap().iter().any(|p| p.contains("well-known")));
}

/// M3-C: challenge-only preserva M1; raiz com resource /mcp não substitui candidato path.
#[tokio::test]
async fn m3c_challenge_only_valido_e_root_com_path_recusado() {
    let f = Fixture::new().await;
    {
        let mut replies = f.redirects.lock().unwrap();
        replies.insert(
            "/mcp".into(),
            (
                StatusCode::UNAUTHORIZED,
                format!("Bearer resource_metadata=\"{}/metadata-unica\"", f.provider),
                json!({}),
            ),
        );
        replies.insert(
            "/metadata-unica".into(),
            (
                StatusCode::OK,
                String::new(),
                json!({"resource":format!("{}/mcp", f.provider),"authorization_servers":[f.provider]}),
            ),
        );
        for path in ["/.well-known/oauth-protected-resource/mcp", "/.well-known/oauth-protected-resource"] {
            replies.insert(path.into(), (StatusCode::NOT_FOUND, String::new(), json!({})));
        }
    }
    let state = f.start().await;
    assert!(f.callback(&state).await.contains("notes conectado"));
    assert!(!f.paths.lock().unwrap().iter().any(|p| p.contains("oauth-protected-resource")));
    {
        let mut replies = f.redirects.lock().unwrap();
        replies.remove("/mcp");
        replies.insert(
            "/.well-known/oauth-protected-resource".into(),
            (
                StatusCode::OK,
                String::new(),
                json!({"resource":format!("{}/mcp", f.provider),"authorization_servers":[f.provider]}),
            ),
        );
    }
    let (status, body) = f.rpc("connectorSignIn", json!({"id":"notes"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains(crate::fork::conectores::oauth_resource::ERRO));
    assert!(f.saved().oauth_binding.as_ref().unwrap().resource().is_none());
}

/// M3-C: nenhuma query remota acrescenta resource ao link/form escolhido pelo Hub.
#[tokio::test]
async fn m3c_endpoint_com_resource_recusa_antes_de_dcr_e_preserva_outras_queries() {
    let f = Fixture::new().await;
    for (endpoint, query) in [
        ("authorization_endpoint", "resource=outro&resource=terceiro"),
        ("authorization_endpoint", "%72esource=outro"),
        ("token_endpoint", "resour%63e=outro"),
    ] {
        let mut body = as_metadata(f.provider.clone(), f.provider.clone())().into_inner().0;
        let raw = body[endpoint].as_str().unwrap().to_owned();
        body[endpoint] = json!(format!("{raw}?{query}"));
        f.redirects
            .lock()
            .unwrap()
            .insert("/.well-known/oauth-authorization-server".into(), (StatusCode::OK, String::new(), body));
        f.paths.lock().unwrap().clear();
        let (status, reply) = f.rpc("connectorSignIn", json!({"id":"notes"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{reply}");
        assert!(reply.to_string().contains(crate::fork::conectores::oauth_resource::ENDPOINT));
        assert!(!reply.to_string().contains(&f.provider));
        assert!(reply.get("url").is_none());
        assert!(f.saved().oauth_binding.as_ref().unwrap().resource().is_none());
        assert!(!f.paths.lock().unwrap().iter().any(|p| p == "/register" || p == "/token"));
    }
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 0);
    let mut body = as_metadata(f.provider.clone(), f.provider.clone())().into_inner().0;
    body["authorization_endpoint"] = json!(format!("{}/authorize?tenant=a&x=1", f.provider));
    body["token_endpoint"] = json!(format!("{}/token?tenant=a&x=1", f.provider));
    f.redirects
        .lock()
        .unwrap()
        .insert("/.well-known/oauth-authorization-server".into(), (StatusCode::OK, String::new(), body));
    let (status, reply) = f.rpc("connectorSignIn", json!({"id":"notes"})).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let url = reqwest::Url::parse(reply["url"].as_str().unwrap()).unwrap();
    assert!(url.query().unwrap().starts_with("tenant=a&x=1&"));
    let resources: Vec<_> = url.query_pairs().filter(|(k, _)| k == "resource").map(|(_, v)| v.into_owned()).collect();
    assert_eq!(resources, [format!("{}/mcp", f.provider)]);
    assert_eq!(f.saved().oauth.as_ref().unwrap().token_endpoint, format!("{}/token?tenant=a&x=1", f.provider));
    let state = url.query_pairs().find(|(k, _)| k == "state").unwrap().1.into_owned();
    assert!(f.callback(&state).await.contains("notes conectado"));
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 1);
}

/// M3-D: o desafio da tentativa vence metadata; Grant no Pending não redescobre.
#[tokio::test]
async fn m3d_desafio_inicial_define_pedido_unico_e_snapshot_original() {
    let f = Fixture::new().await;
    f.redirects.lock().unwrap().insert(
        "/.well-known/oauth-protected-resource/mcp".into(),
        (
            StatusCode::OK,
            String::new(),
            json!({"resource":format!("{}/mcp",f.provider),
            "authorization_servers":[f.provider],"scopes_supported":["read","write"]}),
        ),
    );
    // Headers configurados não pertencem ao probe inicial.
    market::update_connectors(&f.hub.store, |all| {
        all[0].headers.insert("Authorization".into(), "SENTINELA".into());
        all[0].headers.insert("Cookie".into(), "SENTINELA".into());
        Ok(())
    })
    .unwrap();
    for scopes in ["read", "read write admin", "alternativo"] {
        *f.initial.lock().unwrap() = Some((
            StatusCode::UNAUTHORIZED,
            vec![
                r#"Basic realm="a,b", scope="ignorar""#.into(),
                format!("bEaReR realm=\"a,\\\"b\", scope=\"{scopes}\""),
            ],
        ));
        let (status, reply) = f.rpc("connectorSignIn", json!({"id":"notes"})).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let url = reqwest::Url::parse(reply["url"].as_str().unwrap()).unwrap();
        let pedido: Vec<_> = url.query_pairs().filter(|(k, _)| k == "scope").map(|(_, v)| v.into_owned()).collect();
        assert_eq!(pedido, [scopes]);
        let mut original = f.saved().oauth_binding;
        original.as_mut().unwrap().validate_login_scopes(&json!({})).unwrap();
        let grant = serde_json::to_value(&original).unwrap();
        assert_eq!(grant["grant"]["scopes"], json!(scopes.split(' ').collect::<Vec<_>>()));
        let state = url.query_pairs().find(|(k, _)| k == "state").unwrap().1.into_owned();
        *f.initial.lock().unwrap() = Some((StatusCode::UNAUTHORIZED, vec![r#"Bearer scope="trocado""#.into()]));
        assert!(f.callback(&state).await.contains("notes conectado"));
        assert_eq!(f.saved().oauth_binding, original);
    }
    assert_eq!(f.paths.lock().unwrap().iter().filter(|p| *p == "/register").count(), 1, "reuse preservado");
    for (headers, frame) in f.probe_frames.lock().unwrap().iter() {
        assert!(!headers.contains_key("authorization") && !headers.contains_key("cookie"));
        assert_eq!(frame["params"]["protocolVersion"], crate::mcp::PROTOCOL_VERSION);
    }
}

#[tokio::test]
async fn m3d_ausencia_e_status_sem_401_usam_so_prm_validado_ou_omissao() {
    let f = Fixture::new().await;
    for (status, headers) in [
        (StatusCode::UNAUTHORIZED, vec![r#"Basic scope="ignorar", Bearer realm="x""#.into()]),
        (StatusCode::OK, vec![r#"Bearer scope="ignorar""#.into()]),
        (StatusCode::BAD_REQUEST, vec![r#"Bearer scope="ignorar""#.into()]),
        (StatusCode::NOT_FOUND, vec![r#"Bearer scope="ignorar""#.into()]),
    ] {
        *f.initial.lock().unwrap() = Some((status, headers));
        let (status, reply) = f.rpc("connectorSignIn", json!({"id":"notes"})).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let url = reqwest::Url::parse(reply["url"].as_str().unwrap()).unwrap();
        assert_eq!(
            url.query_pairs().filter(|(k, _)| k == "scope").map(|(_, v)| v.into_owned()).collect::<Vec<_>>(),
            ["read"]
        );
    }
    *f.initial.lock().unwrap() = Some((StatusCode::UNAUTHORIZED, vec!["Bearer".into()]));
    f.redirects.lock().unwrap().insert(
        "/.well-known/oauth-protected-resource/mcp".into(),
        (
            StatusCode::OK,
            String::new(),
            json!({"resource":format!("{}/mcp",f.provider),"authorization_servers":[f.provider]}),
        ),
    );
    let (status, reply) = f.rpc("connectorSignIn", json!({"id":"notes"})).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let url = reqwest::Url::parse(reply["url"].as_str().unwrap()).unwrap();
    assert!(!url.query_pairs().any(|(k, _)| k == "scope"));
    assert_eq!(serde_json::to_value(f.saved().oauth_binding).unwrap()["grant"]["scopes"], json!([]));
}

#[tokio::test]
async fn m3d_ambiguidade_e_query_scope_recusam_antes_de_cadastro_inclusive_reuse() {
    use crate::fork::conectores::oauth_scopes::ERRO;
    let f = Fixture::new().await;
    let recusa = async || {
        f.paths.lock().unwrap().clear();
        let (status, reply) = f.rpc("connectorSignIn", json!({"id":"notes"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{reply}");
        assert_eq!(reply, json!({"error":ERRO}));
        assert!(f.saved().oauth_binding.as_ref().unwrap().resource().is_none());
        assert!(f.saved().oauth.as_ref().is_none_or(|o| o.access_token.is_empty()));
        assert!(!f.paths.lock().unwrap().iter().any(|p| p == "/register" || p == "/token"));
    };
    for headers in [
        vec![r#"Bearer scope="""#.into()],
        vec![r#"Bearer scope="read", scope="write""#.into()],
        vec![r#"Basic scope="read", Bearer scope="read""#.into(), r#"Bearer scope="write""#.into()],
    ] {
        *f.initial.lock().unwrap() = Some((StatusCode::UNAUTHORIZED, headers));
        recusa().await;
    }
    *f.initial.lock().unwrap() = None;
    let state = f.start().await;
    assert!(f.callback(&state).await.contains("notes conectado"));
    *f.initial.lock().unwrap() = Some((StatusCode::UNAUTHORIZED, vec![r#"Bearer scope="read  write""#.into()]));
    recusa().await; // Cadastro existente também não contorna a recusa.
    *f.initial.lock().unwrap() = None;
    for (endpoint, query) in [
        ("authorization_endpoint", "scope=a&scope=b"),
        ("authorization_endpoint", "%73cope=a"),
        ("token_endpoint", "scop%65=b"),
    ] {
        let mut doc = as_metadata(f.provider.clone(), f.provider.clone())().into_inner().0;
        doc[endpoint] = json!(format!("{}?{query}", doc[endpoint].as_str().unwrap()));
        f.redirects
            .lock()
            .unwrap()
            .insert("/.well-known/oauth-authorization-server".into(), (StatusCode::OK, String::new(), doc));
        recusa().await;
    }
    let mut doc = as_metadata(f.provider.clone(), f.provider.clone())().into_inner().0;
    doc["authorization_endpoint"] = json!(format!("{}/authorize?tenant=a&next=scope%3Db", f.provider));
    f.redirects
        .lock()
        .unwrap()
        .insert("/.well-known/oauth-authorization-server".into(), (StatusCode::OK, String::new(), doc));
    let (status, reply) = f.rpc("connectorSignIn", json!({"id":"notes"})).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let url = reqwest::Url::parse(reply["url"].as_str().unwrap()).unwrap();
    assert!(url.query().unwrap().starts_with("tenant=a&next=scope%3Db&"));
    assert_eq!(url.query_pairs().filter(|(k, _)| k == "scope").count(), 1);
}

#[tokio::test]
async fn m3d_probe_revalida_tentativa_antes_de_discovery() {
    let f = Fixture::new().await;
    f.probe.arm();
    let call = {
        let f = f.clone();
        tokio::spawn(async move { f.rpc("connectorSignIn", json!({"id":"notes"})).await })
    };
    f.probe.wait().await;
    f.recreate().await;
    let original = f.saved();
    f.paths.lock().unwrap().clear();
    f.probe.open();
    assert_eq!(call.await.unwrap().0, StatusCode::BAD_REQUEST);
    assert_eq!(f.saved(), original);
    assert!(f.paths.lock().unwrap().is_empty(), "await antigo não pode continuar rede");
}

/// R1-1: obs-text alheio a scope é opaco no HTTP; dentro de scope continua recusado.
#[tokio::test]
async fn m3d_obs_text_no_realm_preserva_scope_ascii_e_no_scope_recusa_sem_fallback() {
    use crate::fork::conectores::oauth_scopes::ERRO;
    use axum::http::HeaderValue;
    let f = Fixture::new().await;
    *f.raw_challenges.lock().unwrap() = Some(vec![HeaderValue::from_bytes(b"Bearer scope=\"r\xe9ad\"").unwrap()]);
    f.paths.lock().unwrap().clear();
    let (status, reply) = f.rpc("connectorSignIn", json!({"id":"notes"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(reply, json!({"error":ERRO}));
    assert!(f.saved().oauth_binding.as_ref().unwrap().resource().is_none());
    assert!(!f.paths.lock().unwrap().iter().any(|p| p == "/register" || p == "/token"));
    assert!(f.saved().oauth.as_ref().is_none_or(|o| o.access_token.is_empty()));

    *f.raw_challenges.lock().unwrap() = Some(vec![
        HeaderValue::from_bytes(b"Basic realm=\"caf\xff\"").unwrap(),
        HeaderValue::from_bytes(b"Bearer realm=\"caf\xe9\", opaque=\"\\\x80\", scope=\"read\"").unwrap(),
    ]);
    let (status, reply) = f.rpc("connectorSignIn", json!({"id":"notes"})).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let url = reqwest::Url::parse(reply["url"].as_str().unwrap()).unwrap();
    let scopes: Vec<_> = url.query_pairs().filter(|(k, _)| k == "scope").map(|(_, v)| v.into_owned()).collect();
    assert_eq!(scopes, ["read"]);
    assert_eq!(serde_json::to_value(f.saved().oauth_binding).unwrap()["grant"]["scopes"], json!(["read"]));
    let state = url.query_pairs().find(|(k, _)| k == "state").unwrap().1.into_owned();
    assert!(f.callback(&state).await.contains("notes conectado"));
}

// D70 M3-E: mesma fixture HTTP, sem outra infraestrutura.
#[path = "oauth_token_scope_tests.rs"]
mod m3e_tests;

// D70 M3-F: dois emissores sintéticos e cadastro capturado na fixture existente.
#[path = "oauth_registration_tests.rs"]
mod m3f_tests;

// Hub em outro computador: retorno pelo ouvinte do app desktop, repassado pelo canal autenticado.
#[path = "oauth_desktop_tests.rs"]
mod desktop_tests;
