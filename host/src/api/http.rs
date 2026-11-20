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
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
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
    secured(routes, hub)
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

async fn health(State(hub): State<Arc<Hub>>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "hostId": hub.host_id,
        "computerId": hub.identity.computer_id(),
        "version": env!("CARGO_PKG_VERSION"),
        "binaryPath": crate::service::binary_identity().map(|(path, _)| path),
        "environment": crate::environment::Environment::current().name(),
        "binaryHash": crate::service::binary_identity().map(|(_, hash)| hash),
        "busy": hub.busy(),
    }))
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
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    command(State(hub), Path("donnaGuardRuntime".into()), ConnectInfo(peer), headers, body).await
}

async fn command(
    State(hub): State<Arc<Hub>>,
    Path(method): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    // Classifica a identidade antes de ler o corpo e antes do dispatch.
    let caller = crate::fork::ponte_credencial::autenticar(&hub, &headers, peer).ok_or_else(unauthorized)?;
    let body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body)
            .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("invalid JSON: {e}"), None))?
    };
    dispatch(&hub, &caller, &method, body).await.map(Json).map_err(|e| {
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

async fn term_stream(
    State(hub): State<Arc<Hub>>,
    Path(id): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    if !authorized(&hub, &headers, peer) {
        return Err(unauthorized());
    }
    let events = term_events(&hub, &id, &Caller::Local)
        .map_err(|e| {
            let (status, message) = error_status(&e);
            ApiError(status, message, super::error_code(&e))
        })?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "that terminal is gone".into(), None))?;
    let events = events.map(|v| Ok(Event::default().data(v.to_string())));
    Ok(Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

#[derive(Deserialize)]
struct EventsQuery {
    since: Option<i64>,
    client: Option<String>,
    #[serde(rename = "spaceId")]
    space_id: Option<String>,
}

async fn events(
    State(hub): State<Arc<Hub>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(q): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    if !authorized(&hub, &headers, peer) {
        return Err(unauthorized());
    }
    let events = super::scoped_events_stream(
        &hub,
        q.since.unwrap_or(0),
        q.client.as_deref(),
        &Caller::Local,
        q.space_id.as_deref(),
    )
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"), None))?;
    let events = events.map(|v| Ok(Event::default().data(v.to_string())));
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
