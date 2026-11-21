//! Classifica a credencial antes das rotas, sem alterar os ingressos independentes.
use crate::{api::devices::Caller, hub::Hub, remote::crypto};
use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use std::{net::SocketAddr, sync::Arc};

pub(crate) fn autenticar(hub: &Hub, headers: &HeaderMap, peer: SocketAddr) -> Option<Caller> {
    autenticar_com_origin(hub, headers, peer, false)
}

/// `require_origin` obriga o header `Origin` quando a chamada vem da sessão Web local;
/// credenciais `Bearer` mantêm o comportamento herdado, sem depender de `Origin`.
pub(crate) fn autenticar_com_origin(
    hub: &Hub,
    headers: &HeaderMap,
    peer: SocketAddr,
    require_origin: bool,
) -> Option<Caller> {
    if !peer.ip().to_canonical().is_loopback()
        || (crate::environment::Environment::is_open() && peer != crate::open_hub::SOCKET_PEER)
    {
        return None;
    }
    if headers.contains_key("authorization") {
        let given = headers.get("authorization")?.to_str().ok()?.strip_prefix("Bearer ")?;
        if given.is_empty() {
            return None;
        }
        if let Some(caller) = hub.pontes.autenticar(hub, given) {
            return Some(caller);
        }
        return crypto::ct_eq(given.as_bytes(), hub.token.as_bytes()).then_some(Caller::Local);
    }
    hub.web_local()?.authenticate_api(headers, peer, require_origin)
}

pub(crate) async fn limitar_bearer(
    State(hub): State<Arc<Hub>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let peer = request.extensions().get::<ConnectInfo<SocketAddr>>().map(|p| p.0);
    let caller = peer.and_then(|peer| autenticar(&hub, request.headers(), peer));
    // A Ponte não abre SSE/terminais, canal E2E, webhook ou callbacks. Autenticações
    // próprias dessas rotas (sem token Ponte) continuam nas respectivas fronteiras.
    if matches!(caller, Some(Caller::Ponte { .. })) && !request.uri().path().starts_with("/api/") {
        return StatusCode::FORBIDDEN.into_response();
    }
    if caller.is_none()
        && request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("Bearer ponte_"))
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(request).await
}
