// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! HTTP routes: bearer-token checks for this computer's apps, SSE streams, the channel
//! upgrade, OAuth callbacks and local routine webhooks.

use super::devices::Caller;
#[cfg(test)]
use super::events_stream;
use super::{dispatch, error_status, term_events};
use crate::hub::Hub;
use crate::{market, usage};
use axum::body::Bytes;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequest as _, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::{Stream, StreamExt, stream};
use serde::Deserialize;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

fn desktop_routes() -> Router<Arc<Hub>> {
    Router::new()
        .route("/health", get(health))
        .route("/events", get(events))
        // A ponte aceita até 4 MiB de frame; sua validação inclui os argumentos.
        .route("/api/donnaGuardRuntime", post(guard_runtime).layer(DefaultBodyLimit::max(5 * 1024 * 1024)))
        .route("/api/{method}", post(command))
        .route("/term/{id}", get(term_stream))
}

fn secured(routes: Router<Arc<Hub>>, hub: Arc<Hub>) -> Router {
    routes
        .layer(axum::middleware::from_fn_with_state(hub.clone(), crate::fork::ponte_credencial::limitar_bearer))
        .with_state(hub)
}

#[cfg(unix)]
pub(crate) fn open_socket_router(hub: Arc<Hub>) -> Router {
    secured(desktop_routes(), hub)
}

pub fn router(hub: Arc<Hub>) -> Router {
    let routes = desktop_routes()
        .route("/hooks/routines/{id}", post(routine_hook))
        .route("/oauth/callback", get(oauth_callback));
    let routes = if crate::environment::Environment::is_open() {
        routes
    } else {
        routes
            .route("/computers/enroll-hello", post(crate::computers::pairing::enroll_hello))
            .route("/ingest/statusline", post(statusline))
            .route("/channel", get(channel))
            .route("/personal-possession", get(crate::fork::personal_bootstrap::channel::upgrade))
            .route("/personal-prepare", get(crate::fork::personal_bootstrap::prepare::upgrade))
    };
    let web = hub.web_local().cloned();
    let routes = match &web {
        Some(web) => crate::fork::web_local::attach_routes(routes, web.clone()),
        None => routes,
    };
    let routes = secured(routes, hub);
    // O middleware de segurança fica por fora de `secured`: até as respostas precoces de
    // `limitar_bearer` passam por `Host`/`Origin`/`Sec-Fetch-Site` e levam os headers.
    match web {
        Some(web) => crate::fork::web_local::wrap_security(routes, web),
        None => routes,
    }
}

/// Loopback, also as an IPv4-mapped IPv6 address (`::ffff:127.0.0.1` under `--bind ::`).
fn is_loopback(ip: IpAddr) -> bool {
    ip.to_canonical().is_loopback()
}

fn authorized(hub: &Hub, headers: &HeaderMap, peer: SocketAddr) -> bool {
    matches!(crate::fork::ponte_credencial::autenticar(hub, headers, peer), Some(Caller::Local))
}

fn unauthorized() -> ApiError {
    ApiError(StatusCode::UNAUTHORIZED, "invalid token".into(), None)
}

/// Enrola o stream do SSE: sem guarda (`Bearer` local), os itens seguem como antes;
/// com guarda de sessão Web, o stream termina no logout, no despejo ou na expiração,
/// e cada item ainda é conferido contra a sessão antes de sair.
fn stream_com_guarda(
    stream: impl Stream<Item = Result<Event, Infallible>> + Send + 'static,
    guard: Option<crate::fork::web_local::SessaoStreamGuard>,
) -> Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> {
    let Some(guard) = guard else { return Box::pin(stream) };
    let stream = Box::pin(stream);
    Box::pin(stream::unfold((stream, guard), |(mut stream, mut guard)| async move {
        tokio::select! {
            biased;
            () = guard.aguardar_fim() => None,
            item = stream.next() => {
                let item = item?;
                guard.esta_ativa().then_some((item, (stream, guard)))
            }
        }
    }))
}

struct ApiError(StatusCode, String, Option<&'static str>);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = json!({"error": self.1});
        if let Some(code) = self.2 {
            body["code"] = code.into();
        }
        (self.0, Json(body)).into_response()
    }
}

/// Sem prova, só o mínimo de vida e a atividade. A identidade do Hub e do binário exige a prova do
/// token local (`?nonce=` e `?proof=`: um nonce novo e o HMAC do token sobre ele), então o token
/// nunca viaja para a porta. Prova de outro token ou vinda de fora do loopback é recusada em vez de
/// cair na resposta anônima.
async fn health(
    State(hub): State<Arc<Hub>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(q): Query<HealthQuery>,
) -> Result<Json<Value>, ApiError> {
    let mut body = json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "environment": crate::environment::Environment::current().name(),
        "busy": hub.busy(),
    });
    let (Some(nonce), Some(proof)) = (q.nonce.as_deref(), q.proof.as_deref()) else {
        if q.nonce.is_none() && q.proof.is_none() {
            return Ok(Json(body));
        }
        return Err(unauthorized());
    };
    let Some(nonce) = crate::fork::prova_health::aceitar(&hub, nonce, proof, peer) else {
        return Err(unauthorized());
    };
    let binary = crate::service::binary_identity();
    let computer_id = hub.identity.computer_id();
    let identidade = crate::fork::prova_health::identidade(
        &hub.host_id,
        &computer_id,
        binary.map(|(path, hash)| (path.as_str(), hash.as_str())),
    );
    body["hostId"] = json!(hub.host_id);
    body["computerId"] = json!(computer_id);
    body["binaryPath"] = json!(binary.map(|(path, _)| path));
    body["binaryHash"] = json!(binary.map(|(_, hash)| hash));
    // O recibo prova ao cliente que quem respondeu tem o mesmo token, para este nonce e esta identidade.
    body["receipt"] = json!(crate::fork::prova_health::recibo(&hub.token, &nonce, &identidade));
    Ok(Json(body))
}

async fn statusline(
    State(hub): State<Arc<Hub>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    if !authorized(&hub, &headers, peer) {
        return StatusCode::UNAUTHORIZED;
    }
    let Ok(body) = serde_json::from_slice::<Value>(&body) else {
        return StatusCode::BAD_REQUEST;
    };
    usage::ingest_statusline(&hub, &body);
    StatusCode::NO_CONTENT
}

async fn guard_runtime(
    State(hub): State<Arc<Hub>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    request: Request,
) -> Result<Json<Value>, ApiError> {
    command(State(hub), Path("donnaGuardRuntime".into()), ConnectInfo(peer), headers, request).await
}

async fn command(
    State(hub): State<Arc<Hub>>,
    Path(method): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    request: Request,
) -> Result<Json<Value>, ApiError> {
    // Classifica a identidade e o nome antes de ler o corpo (que só é lido aqui, com o limite da rota).
    let caller =
        crate::fork::ponte_credencial::autenticar_com_origin(&hub, &headers, peer, true).ok_or_else(unauthorized)?;
    if crate::fork::browser::local::METHODS.contains(&method.as_str())
        && (!headers.contains_key("authorization") || !matches!(caller, Caller::Local))
    {
        return Err(ApiError(StatusCode::FORBIDDEN, "forbidden".into(), None));
    }
    // Nome sem classe é recusado antes de ler o corpo: 404 (403 na Open) sem passar por 400 ou 413.
    let origem = if headers.contains_key("authorization") { super::origem(&caller) } else { "web" };
    if let Some(recusa) = super::recusa_sem_classe(origem, &method) {
        let (status, message) = error_status(&recusa);
        return Err(ApiError(status, message, None));
    }
    let body = Bytes::from_request(request, &()).await.map_err(|e| ApiError(e.status(), e.body_text(), None))?;
    let body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body)
            .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("invalid JSON: {e}"), None))?
    };
    let dispatched = dispatch(&hub, &caller, &method, body);
    #[cfg(unix)]
    let dispatched = if peer == crate::open_hub::SOCKET_PEER
        && matches!(caller, Caller::Local)
        && headers.contains_key("authorization")
    {
        crate::fork::browser::turn::NATIVE.scope(hub.workspace.atual(), dispatched).await
    } else {
        dispatched.await
    };
    #[cfg(not(unix))]
    let dispatched = dispatched.await;
    dispatched.map(Json).map_err(|e| {
        let (status, message) = error_status(&e);
        ApiError(status, message, super::error_code(&e))
    })
}

#[derive(Deserialize)]
struct OAuthCallback {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
    /// M3-B: emissor do retorno, decodificado uma vez pelo `Query` e conferido no Hub.
    iss: Option<String>,
}

/// `GET /oauth/callback`: where a connector's sign-in page returns when the browser runs on
/// this computer. No bearer token: the one-time `state` (and the host-held PKCE verifier) authenticate.
async fn oauth_callback(
    State(hub): State<Arc<Hub>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    q: Result<Query<OAuthCallback>, axum::extract::rejection::QueryRejection>,
) -> Response {
    if !is_loopback(peer.ip()) {
        return StatusCode::FORBIDDEN.into_response();
    }
    // M3-B: campo repetido não vira o primeiro valor; recusa fixa, sem ecoar a query.
    let result = match q {
        Ok(Query(q)) => {
            let error = q.error_description.or(q.error);
            let state = q.state.as_deref().unwrap_or_default();
            market::oauth::finish(&hub, None, state, q.code.as_deref(), error.as_deref(), q.iss.as_deref()).await
        }
        Err(_) => Err(anyhow::anyhow!(crate::fork::conectores::oauth_iss::FORMA)),
    };
    let (title, detail) = match &result {
        Ok(c) => (format!("{} conectado", c.name), "Pode fechar esta aba e voltar para a Mexerica.".to_owned()),
        Err(e) => ("O login não foi concluído".to_owned(), format!("{e:#}")),
    };
    let escape = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    axum::response::Html(format!(
        "<!doctype html><html lang=\"pt-BR\"><meta charset=utf-8><meta name=viewport content=\"width=device-width\"><title>Mexerica</title>\
         <body style=\"font:16px system-ui;margin:20vh auto;max-width:28em;padding:0 16px;text-align:center\">\
         <h2>{}</h2><p>{}</p></body></html>",
        escape(&title),
        escape(&detail)
    ))
    .into_response()
}

#[derive(Deserialize)]
struct HealthQuery {
    nonce: Option<String>,
    proof: Option<String>,
}

#[derive(Deserialize)]
struct ChannelQuery {
    v: Option<u32>,
}

/// `GET /channel?v=1`: the direct E2E channel. No bearer token: the handshake authenticates.
async fn channel(
    State(hub): State<Arc<Hub>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(q): Query<ChannelQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if q.v != Some(1) {
        return (StatusCode::UPGRADE_REQUIRED, Json(json!({"error": {"code": "upgradeRequired"}}))).into_response();
    }
    ws.max_message_size(crate::remote::channel::MAX_WS_MESSAGE)
        .on_upgrade(move |socket| crate::remote::channel::serve_direct(hub, socket, peer.ip()))
}

#[derive(Deserialize)]
struct TermQuery {
    #[serde(rename = "workspaceEpoch")]
    workspace_epoch: Option<String>,
}

async fn term_stream(
    State(hub): State<Arc<Hub>>,
    Path(id): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(q): Query<TermQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let guard = crate::fork::ponte_credencial::autenticar_stream(&hub, &headers, peer).ok_or_else(unauthorized)?;
    let origem = super::origem(&Caller::Local);
    let retrato = crate::fork::workspace::epoch_da_query(&hub, origem, q.workspace_epoch.as_deref())
        .and_then(|epoch| crate::fork::workspace::abrir(&hub, origem, epoch))
        .map_err(|e| {
            let (status, message) = error_status(&e);
            ApiError(status, message, super::error_code(&e))
        })?;
    let events = term_events(&hub, &id, &Caller::Local)
        .map_err(|e| {
            let (status, message) = error_status(&e);
            ApiError(status, message, super::error_code(&e))
        })?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "that terminal is gone".into(), None))?;
    let events = crate::fork::workspace::cortar(&hub, &retrato, events);
    let events = events.map(|v| Ok(Event::default().data(v.to_string())));
    let events = stream_com_guarda(events, guard);
    Ok(Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

#[derive(Deserialize)]
struct EventsQuery {
    since: Option<i64>,
    client: Option<String>,
    #[serde(rename = "spaceId")]
    space_id: Option<String>,
    #[serde(rename = "workspaceEpoch")]
    workspace_epoch: Option<String>,
}

async fn events(
    State(hub): State<Arc<Hub>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(q): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let guard = crate::fork::ponte_credencial::autenticar_stream(&hub, &headers, peer).ok_or_else(unauthorized)?;
    let events =
        crate::fork::workspace::epoch_da_query(&hub, super::origem(&Caller::Local), q.workspace_epoch.as_deref())
            .and_then(|epoch| {
                super::scoped_events_stream_at(
                    &hub,
                    q.since.unwrap_or(0),
                    q.client.as_deref(),
                    &Caller::Local,
                    q.space_id.as_deref(),
                    epoch,
                )
            })
            .map_err(|e| {
                if crate::fork::workspace::is_mismatch(&e) {
                    ApiError(StatusCode::CONFLICT, e.to_string(), Some(crate::fork::workspace::EPOCH_MISMATCH))
                } else {
                    ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"), None)
                }
            })?;
    let events = events.map(|v| Ok(Event::default().data(v.to_string())));
    let events = stream_com_guarda(events, guard);
    Ok(Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

/// A scoped webhook credential authorizes only firing its own routine.
/// `POST /hooks/routines/:id`: a webhook delivery made on this computer (the public one
/// arrives through the relay, `remote::relay`). Same checks and answers either way.
async fn routine_hook(
    State(hub): State<Arc<Hub>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    use crate::routines::hooks::{MAX_BODY, Refused};
    if body.len() > MAX_BODY {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, "event too large".into(), None));
    }
    let headers =
        headers.iter().filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned()))).collect();
    hub.routines.receive(&hub, &id, &headers, &body).map(Json).map_err(|e| {
        let status = match e {
            Refused::Unauthorized => StatusCode::UNAUTHORIZED,
            Refused::Paused | Refused::Busy => StatusCode::CONFLICT,
            Refused::ChainUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Refused::Invalid(_) => StatusCode::BAD_REQUEST,
        };
        ApiError(status, e.to_string(), None)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_streams_hold_pushes_until_the_last_one_closes() {
        let dir = std::env::temp_dir().join(format!("codync-phone-events-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let hub = Hub::new(
            crate::store::Store::open(std::path::Path::new(":memory:")).expect("store"),
            "test".into(),
            crate::remote::identity::Identity::load_or_create(&dir).expect("identity"),
            "test".into(),
            19222,
        );
        let desktop = events_stream(&hub, 0, Some("macos"), &Caller::Local).expect("desktop events");
        assert!(!hub.phone_connected());
        let android = events_stream(&hub, 0, Some("android"), &Caller::Local).expect("Android events");
        let ios = events_stream(&hub, 0, Some("ios"), &Caller::Local).expect("iOS events");
        assert!(hub.phone_connected());
        drop(android);
        assert!(hub.phone_connected(), "the iPhone remains connected");
        drop(ios);
        assert!(!hub.phone_connected(), "desktop streams do not hold phone alerts");
        drop(desktop);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// O nome é classificado antes de o corpo ser lido: nome sem classe dá 404 sem tocar no corpo (antes, um corpo
    /// grande dava 413 e um JSON inválido, 400), com qualquer prefixo; a autenticação vem antes; método com classe lê
    /// o corpo.
    /// Tudo o que foi registrado em log enquanto `run` rodou nesta thread.
    async fn registrado<T>(run: impl Future<Output = T>) -> (String, T) {
        #[derive(Clone, Default)]
        struct Buf(Arc<std::sync::Mutex<Vec<u8>>>);
        impl std::io::Write for Buf {
            fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
                self.0.lock().expect("log").extend_from_slice(data);
                Ok(data.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buf = Buf::default();
        let writer = buf.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let guard = tracing::subscriber::set_default(subscriber);
        let out = run.await;
        drop(guard);
        let bytes = buf.0.lock().expect("log").clone();
        (String::from_utf8_lossy(&bytes).into_owned(), out)
    }

    #[tokio::test]
    async fn unknown_method_is_refused_before_the_body_is_read() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = std::env::temp_dir().join(format!("codync-http-unknown-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let hub = Hub::new(
            crate::store::Store::open(std::path::Path::new(":memory:")).expect("store"),
            "test".into(),
            crate::remote::identity::Identity::load_or_create(&dir).expect("identity"),
            "local-test".into(),
            0,
        );
        let peer: SocketAddr = "127.0.0.1:9".parse().expect("endereço");
        let cabecalhos = |token: &str| {
            let mut headers = HeaderMap::new();
            headers.insert("authorization", format!("Bearer {token}").parse().expect("cabeçalho"));
            headers
        };
        let chamar = |method: &str, token: &str| {
            let lido = Arc::new(AtomicBool::new(false));
            let marca = lido.clone();
            let corpo = stream::once(async move {
                marca.store(true, Ordering::SeqCst);
                Ok::<_, std::io::Error>(Bytes::from_static(b"{"))
            });
            let request = Request::new(axum::body::Body::from_stream(corpo));
            let resposta =
                command(State(hub.clone()), Path(method.to_owned()), ConnectInfo(peer), cabecalhos(token), request);
            async move { (resposta.await.err().map(|e| e.0), lido.load(Ordering::SeqCst)) }
        };
        for method in ["metodoInexistente", "donnaWorkplaceInexistente"] {
            assert_eq!(chamar(method, "local-test").await, (Some(StatusCode::NOT_FOUND), false), "{method}");
            assert_eq!(chamar(method, "invalido").await, (Some(StatusCode::UNAUTHORIZED), false), "{method}");
        }
        assert_eq!(chamar("hello", "local-test").await, (Some(StatusCode::BAD_REQUEST), true));

        // A recusa deixa log com motivo fixo e origem, sem o nome do cliente, o token ou o corpo.
        let cru = "metodoMarcador7f3\nINJETADO reason=forjado";
        let aparelho = Caller::Device { key: "chave-marcadora-9c1".into(), scopes: vec![], autoridade: None };
        let (log, respostas) = registrado(async {
            let http = chamar(cru, "local-test").await;
            let canal = dispatch(&hub, &aparelho, cru, json!({"segredo": "corpo-marcador-5e2"})).await;
            (http, canal.map_err(|e| error_status(&e).0))
        })
        .await;
        assert_eq!(respostas, ((Some(StatusCode::NOT_FOUND), false), Err(StatusCode::NOT_FOUND)), "resposta igual");
        assert_eq!(log.matches("unknown_rpc_method").count(), 2, "{log}");
        assert!(log.contains("origin=\"local\"") && log.contains("origin=\"aparelho\""), "{log}");
        assert!(log.contains(&format!("method_len={}", cru.len())), "{log}");
        for vazado in ["Marcador7f3", "INJETADO", "forjado", "local-test", "chave-marcadora-9c1", "corpo-marcador-5e2"]
        {
            assert!(!log.contains(vazado), "{vazado} no log: {log}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    const IDENTITY: [&str; 4] = ["hostId", "computerId", "binaryPath", "binaryHash"];

    #[tokio::test]
    async fn health_shows_identity_only_with_the_token_proof() {
        use crate::remote::crypto;
        let dir = std::env::temp_dir().join(format!("codync-health-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let token = "token-local-ficticio";
        let hub = Hub::new(
            crate::store::Store::open(std::path::Path::new(":memory:")).expect("store"),
            "host-ficticio".into(),
            crate::remote::identity::Identity::load_or_create(&dir).expect("identity"),
            token.into(),
            19222,
        );
        let loopback: SocketAddr = "127.0.0.1:50000".parse().expect("addr");
        let mapped: SocketAddr = "[::ffff:127.0.0.1]:50000".parse().expect("addr");
        let outside: SocketAddr = "192.0.2.10:50000".parse().expect("addr");
        let call = |peer: SocketAddr, pair: Option<(String, String)>| {
            let (nonce, proof) = pair.unwrap_or_default();
            health(
                State(hub.clone()),
                ConnectInfo(peer),
                Query(HealthQuery {
                    nonce: Some(nonce).filter(|n| !n.is_empty()),
                    proof: Some(proof).filter(|p| !p.is_empty()),
                }),
            )
        };
        // Um par novo por chamada: cada nonce vale uma vez só.
        let par = |who: &str| {
            let raw = crypto::random::<32>();
            (crypto::b64(&raw), crate::fork::prova_health::prova(who, &raw), raw)
        };
        let local = |nonce: &str, proof: &str| Some((nonce.to_owned(), proof.to_owned()));

        let Ok(Json(anonymous)) = call(loopback, None).await else { panic!("anonymous health must answer") };
        assert_eq!(anonymous["ok"], true);
        assert_eq!(anonymous["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(anonymous["environment"], crate::environment::Environment::current().name());
        assert_eq!(anonymous["busy"], false);
        assert_eq!(anonymous.as_object().map(serde_json::Map::len), Some(4), "{anonymous}");
        for field in IDENTITY {
            assert!(anonymous.get(field).is_none(), "{field} leaked: {anonymous}");
        }

        let (nonce, proof, raw) = par(token);
        let Ok(Json(identified)) = call(loopback, local(&nonce, &proof)).await else {
            panic!("the proof of the local token must answer")
        };
        assert_eq!(identified["hostId"], "host-ficticio");
        assert_eq!(identified["computerId"], hub.identity.computer_id());
        assert_eq!(identified.as_object().map(serde_json::Map::len), Some(9), "{identified}");
        for field in IDENTITY {
            assert!(identified.get(field).is_some(), "{field} missing: {identified}");
        }
        assert_eq!(identified["ok"], true);
        // O recibo prova a resposta: só quem tem o token responde àquele nonce com essa identidade.
        let campos = |body: &Value| {
            let texto = |field: &str| body[field].as_str().unwrap_or_default().to_owned();
            crate::fork::prova_health::identidade(
                &texto("hostId"),
                &texto("computerId"),
                Some((&texto("binaryPath"), &texto("binaryHash"))),
            )
        };
        assert_eq!(
            identified["receipt"].as_str(),
            Some(crate::fork::prova_health::recibo(token, &raw, &campos(&identified)).as_str()),
            "the receipt must prove the answer"
        );
        assert_ne!(
            identified["receipt"].as_str(),
            Some(crate::fork::prova_health::recibo("outro-token-ficticio", &raw, &campos(&identified)).as_str()),
            "another token cannot produce this receipt"
        );
        assert_ne!(
            identified["receipt"].as_str(),
            Some(crate::fork::prova_health::recibo(token, &crypto::random::<32>(), &campos(&identified)).as_str()),
            "the receipt is bound to the nonce"
        );

        let (mapped_nonce, mapped_proof, _) = par(token);
        assert!(
            call(mapped, local(&mapped_nonce, &mapped_proof)).await.is_ok(),
            "IPv4 loopback seen through `--bind ::`"
        );

        let (outro, outro_proof, _) = par("outro-token-ficticio");
        let (ilegivel, _, _) = par(token);
        let (_, sem_nonce_proof, _) = par(token);
        let refused = [
            ("prova de outro token", local(&outro, &outro_proof)),
            ("prova de outro nonce", local(&ilegivel, &outro_proof)),
            ("prova ilegível", local(&ilegivel, "nao-e-hexadecimal")),
            ("nonce ilegível", local("nao-e-base64url!!", &sem_nonce_proof)),
        ];
        for (motivo, pair) in refused {
            let Err(error) = call(loopback, pair).await else { panic!("{motivo} must be refused") };
            let response = error.into_response();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{motivo}");
            let body = axum::body::to_bytes(response.into_body(), 4096).await.expect("body");
            let body = String::from_utf8_lossy(&body);
            assert!(!body.contains(token) && !body.contains(&outro_proof), "{body}");
        }
        let Err(error) = call(loopback, Some((ilegivel.clone(), String::new()))).await else {
            panic!("proof without nonce must be refused")
        };
        assert_eq!(error.into_response().status(), StatusCode::UNAUTHORIZED, "half a pair");

        // A mesma prova não abre a identidade duas vezes.
        let Err(error) = call(loopback, local(&nonce, &proof)).await else {
            panic!("a replayed proof must be refused")
        };
        assert_eq!(error.into_response().status(), StatusCode::UNAUTHORIZED, "nonce repetido");

        let (fora, fora_proof, _) = par(token);
        let Err(error) = call(outside, local(&fora, &fora_proof)).await else {
            panic!("a correct proof from outside the loopback must be refused")
        };
        assert_eq!(error.into_response().status(), StatusCode::UNAUTHORIZED, "fora do loopback");
        // O nonce recusado de fora não foi consumido: do loopback, ele ainda vale.
        assert!(call(loopback, local(&fora, &fora_proof)).await.is_ok(), "a nonce refused for origin stays unused");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_loopback_counts_as_this_computer() {
        let ip = |s: &str| s.parse::<IpAddr>().expect("test addresses are valid");
        assert!(is_loopback(ip("127.0.0.1")));
        assert!(is_loopback(ip("::1")));
        assert!(is_loopback(ip("::ffff:127.0.0.1")), "IPv4 loopback seen through `--bind ::`");
        assert!(!is_loopback(ip("192.168.1.20")));
        assert!(!is_loopback(ip("::ffff:192.168.1.20")));
        assert!(!is_loopback(ip("100.101.102.103")));
    }
}
