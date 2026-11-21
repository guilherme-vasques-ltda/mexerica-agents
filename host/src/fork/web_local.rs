//! Interface Web local (`--web-dir`): arquivos estáticos e sessão de navegador servida
//! pelo próprio Hub, sempre em `127.0.0.1`. Falha fechada: `Host`/`Origin`/`Sec-Fetch-Site`
//! fora do esperado são recusados antes de qualquer rota, a sessão vive só em memória com
//! cookie `HttpOnly` + CSRF, e o diretório servido é percorrido sem symlinks.
//!
//! Nada aqui registra token, cookie, CSRF ou segredo em log.

use crate::{LockExt, api::devices::Caller, hub::Hub, remote::crypto};
use anyhow::{Context, Result, ensure};
use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Extension, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(crate) const COOKIE: &str = "mexerica_web_session";
pub(crate) const CSRF_HEADER: &str = "x-mexerica-csrf";
pub(crate) const WEB_HEADER: &str = "x-mexerica-web";

/// Validade absoluta de uma sessão (12 h) e expiração por inatividade (4 h).
const SESSION_TTL: Duration = Duration::from_secs(12 * 60 * 60);
const SESSION_IDLE: Duration = Duration::from_secs(4 * 60 * 60);
const MAX_SESSIONS: usize = 16;

/// Janela deslizante do login: no máximo 10 falhas por minuto.
const LOGIN_WINDOW: Duration = Duration::from_secs(60);
const MAX_LOGIN_FAILURES: usize = 10;

const CSP_HTML: &str = "default-src 'self'; script-src 'self'; worker-src 'self' blob:; img-src 'self' data: blob:; \
style-src 'self' 'unsafe-inline'; font-src 'self' data:; connect-src 'self'; media-src 'self' blob: data:; \
object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";
const CSP_OTHER: &str = "default-src 'none'; frame-ancestors 'none'";

/// Configuração recebida pelo `serve`: valida o diretório antes de o Hub abrir a porta.
pub(crate) struct WebConfig;

impl WebConfig {
    /// Falha fechada: o diretório precisa existir, ser um diretório de verdade (não um
    /// symlink), canônico, com `index.html` como arquivo regular sem symlink.
    pub(crate) fn validate_dir(dir: &Path) -> Result<PathBuf> {
        let meta = std::fs::symlink_metadata(dir).with_context(|| format!("reading {}", dir.display()))?;
        ensure!(meta.is_dir(), "a interface Web local exige um diretório: {}", dir.display());
        let index = dir.join("index.html");
        let index_meta = std::fs::symlink_metadata(&index).with_context(|| format!("reading {}", index.display()))?;
        ensure!(
            index_meta.is_file(),
            "a interface Web local exige index.html como arquivo regular em {}",
            dir.display()
        );
        let canonical = std::fs::canonicalize(dir).with_context(|| format!("canonicalizing {}", dir.display()))?;
        ensure!(
            std::fs::canonicalize(&index).is_ok_and(|p| p == canonical.join("index.html")),
            "index.html não resolve dentro do diretório canônico",
        );
        Ok(canonical)
    }
}

/// Uma sessão aberta: só o hash do id fica em memória; o CSRF precisa voltar ao cliente.
/// `revogado` avisa os streams SSE abertos assim que a sessão sai do store.
struct Session {
    id_hash: [u8; 32],
    csrf: String,
    revogado: tokio::sync::watch::Sender<bool>,
    created: Instant,
    last_seen: Instant,
}

/// Sessões em memória: máx. 16, TTL absoluto de 12 h e TTL ocioso de 4 h.
#[derive(Default)]
struct SessionStore {
    sessions: Vec<Session>,
}

impl SessionStore {
    fn create(&mut self) -> (String, String) {
        let id = crypto::b64(&crypto::random::<32>());
        let csrf = crypto::b64(&crypto::random::<32>());
        let (revogado, _) = tokio::sync::watch::channel(false);
        self.sessions.push(Session {
            id_hash: crypto::sha256(&[id.as_bytes()]),
            csrf: csrf.clone(),
            revogado,
            created: Instant::now(),
            last_seen: Instant::now(),
        });
        if self.sessions.len() > MAX_SESSIONS {
            let despejada = self.sessions.remove(0);
            let _ = despejada.revogado.send(true);
        }
        (id, csrf)
    }

    /// Localiza o índice da sessão sem atualizar `last_seen`: vencida, sai do store
    /// e sinaliza os streams dela. O `last_seen` só avança depois de o CSRF conferir.
    fn index(&mut self, id: &str) -> Option<usize> {
        let now = Instant::now();
        let hash = crypto::sha256(&[id.as_bytes()]);
        let index = self.sessions.iter().position(|s| crypto::ct_eq(&s.id_hash, &hash))?;
        let vencida = now.duration_since(self.sessions[index].created) > SESSION_TTL
            || now.duration_since(self.sessions[index].last_seen) > SESSION_IDLE;
        if vencida {
            let expirada = self.sessions.remove(index);
            let _ = expirada.revogado.send(true);
            return None;
        }
        Some(index)
    }

    fn remove(&mut self, id: &str) {
        let hash = crypto::sha256(&[id.as_bytes()]);
        if let Some(index) = self.sessions.iter().position(|s| crypto::ct_eq(&s.id_hash, &hash)) {
            let removida = self.sessions.remove(index);
            let _ = removida.revogado.send(true);
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.sessions.len()
    }
}

/// Limite de tentativas de login: 10 falhas em 60 s bloqueiam com `429`.
#[derive(Default)]
struct LoginLimiter {
    failures: std::collections::VecDeque<Instant>,
}

impl LoginLimiter {
    fn blocked(&self) -> bool {
        let now = Instant::now();
        self.failures.iter().filter(|t| now.duration_since(**t) < LOGIN_WINDOW).count() >= MAX_LOGIN_FAILURES
    }

    fn record_failure(&mut self) {
        let now = Instant::now();
        self.failures.push_back(now);
        while self.failures.front().is_some_and(|t| now.duration_since(*t) >= LOGIN_WINDOW) {
            self.failures.pop_front();
        }
    }

    fn clear(&mut self) {
        self.failures.clear();
    }
}

/// Estado da interface Web local: um por Hub, instalado pelo `serve` quando há `--web-dir`.
/// Sem `Debug`: id de sessão, cookie e CSRF nunca aparecem em log ou erro.
pub(crate) struct WebState {
    root: PathBuf,
    expected_host: String,
    expected_origin: String,
    sessions: Mutex<SessionStore>,
    limiter: Mutex<LoginLimiter>,
}

impl WebState {
    pub(crate) fn new(root: PathBuf, port: u16) -> Self {
        // Porta 80: o navegador manda a autoridade canônica, sem o sufixo `:80`.
        let authority = if port == 80 { "127.0.0.1".to_owned() } else { format!("127.0.0.1:{port}") };
        Self {
            root,
            expected_host: authority.clone(),
            expected_origin: format!("http://{authority}"),
            sessions: Mutex::new(SessionStore::default()),
            limiter: Mutex::new(LoginLimiter::default()),
        }
    }

    fn base_guard(&self, headers: &HeaderMap, peer: SocketAddr) -> bool {
        peer.ip().to_canonical().is_loopback() && self.host_ok(headers)
    }

    fn host_ok(&self, headers: &HeaderMap) -> bool {
        single_header(headers, "host").is_some_and(|host| host.eq_ignore_ascii_case(&self.expected_host))
    }

    fn origin_ok(&self, headers: &HeaderMap) -> bool {
        match check_header(headers, "origin") {
            HeaderCheck::Absent => true,
            HeaderCheck::Single(origin) => self.eq_origin(origin),
            HeaderCheck::Invalid => false,
        }
    }

    fn session_csrf(&self, headers: &HeaderMap) -> Option<String> {
        let id = cookie_value(headers, COOKIE)?;
        let mut store = self.sessions.locked();
        let index = store.index(&id)?;
        Some(store.sessions[index].csrf.clone())
    }

    pub(crate) fn remove_session(&self, headers: &HeaderMap) {
        if let Some(id) = cookie_value(headers, COOKIE) {
            self.sessions.locked().remove(&id);
        }
    }

    fn session_json(hub: &Hub, csrf: &str) -> Value {
        json!({
            "authenticated": true,
            "csrfToken": csrf,
            "computerId": hub.identity.computer_id(),
            "hostId": hub.host_id,
            "hostName": crate::service::host_name(),
        })
    }

    /// Fronteira comum das autenticações por sessão Web: loopback, `Host` exato,
    /// `Origin` (obrigatório quando pedido, e sempre igual quando presente),
    /// `Sec-Fetch-Site` `same-origin` e cookie de sessão. Devolve o id do cookie.
    fn checar_fronteira(&self, headers: &HeaderMap, peer: SocketAddr, require_origin: bool) -> Option<String> {
        if !peer.ip().to_canonical().is_loopback() || !self.host_ok(headers) {
            return None;
        }
        match check_header(headers, "origin") {
            HeaderCheck::Invalid => return None,
            HeaderCheck::Absent if require_origin => return None,
            HeaderCheck::Single(origin) if !self.eq_origin(origin) => return None,
            _ => {}
        }
        match check_header(headers, "sec-fetch-site") {
            HeaderCheck::Invalid => return None,
            HeaderCheck::Single(site) if !site.eq_ignore_ascii_case("same-origin") => return None,
            _ => {}
        }
        cookie_value(headers, COOKIE)
    }

    /// Autentica uma chamada de API/SSE pela sessão Web: loopback, `Host` exato,
    /// `Origin` (obrigatório quando pedido, e sempre igual quando presente),
    /// `Sec-Fetch-Site` `same-origin`, cookie válido e CSRF correspondente.
    /// `last_seen` só avança depois de o CSRF conferir: cookie sem CSRF não
    /// estende o TTL ocioso.
    pub(crate) fn authenticate_api(
        &self,
        headers: &HeaderMap,
        peer: SocketAddr,
        require_origin: bool,
    ) -> Option<Caller> {
        let id = self.checar_fronteira(headers, peer, require_origin)?;
        let csrf = single_header(headers, CSRF_HEADER)?;
        let mut store = self.sessions.locked();
        let index = store.index(&id)?;
        if !crypto::ct_eq(csrf.as_bytes(), store.sessions[index].csrf.as_bytes()) {
            return None;
        }
        store.sessions[index].last_seen = Instant::now();
        Some(Caller::Local)
    }

    /// Autentica um stream SSE pela sessão Web com as mesmas validações de
    /// `authenticate_api` (sem exigir `Origin`) e devolve a guarda que encerra o
    /// stream no logout, no despejo por limite de sessões ou na expiração.
    pub(crate) fn autenticar_stream(
        self: &Arc<Self>,
        headers: &HeaderMap,
        peer: SocketAddr,
    ) -> Option<SessaoStreamGuard> {
        let id = self.checar_fronteira(headers, peer, false)?;
        let csrf = single_header(headers, CSRF_HEADER)?;
        let mut store = self.sessions.locked();
        let index = store.index(&id)?;
        if !crypto::ct_eq(csrf.as_bytes(), store.sessions[index].csrf.as_bytes()) {
            return None;
        }
        store.sessions[index].last_seen = Instant::now();
        Some(SessaoStreamGuard {
            web: Arc::clone(self),
            id_hash: store.sessions[index].id_hash,
            revogado: store.sessions[index].revogado.subscribe(),
        })
    }

    fn eq_origin(&self, origin: &str) -> bool {
        origin.eq_ignore_ascii_case(&self.expected_origin)
    }

    fn sec_fetch_ok(path: &str, method: &Method, headers: &HeaderMap) -> bool {
        let site = match check_header(headers, "sec-fetch-site") {
            HeaderCheck::Absent => return true,
            HeaderCheck::Invalid => return false,
            HeaderCheck::Single(site) => site.to_ascii_lowercase(),
        };
        let api_like = path == "/api"
            || path.starts_with("/api/")
            || path == "/events"
            || path.starts_with("/term/")
            || path == "/web"
            || path.starts_with("/web/");
        let get_head = method == Method::GET || method == Method::HEAD;
        if api_like || !get_head {
            return site == "same-origin";
        }
        let same_none = ["same-origin", "none"];
        if path == "/oauth/callback" {
            return same_none.contains(&site.as_str()) || site == "cross-site";
        }
        let navigating = matches!(check_header(headers, "sec-fetch-mode"), HeaderCheck::Single(m) if m.eq_ignore_ascii_case("navigate"))
            && matches!(check_header(headers, "sec-fetch-dest"), HeaderCheck::Single(d) if d.eq_ignore_ascii_case("document"));
        if (path == "/" || path == "/index.html") && navigating {
            return same_none.contains(&site.as_str()) || site == "cross-site";
        }
        same_none.contains(&site.as_str())
    }

    fn denied(path: &str) -> Response {
        let mut response = (StatusCode::FORBIDDEN, Json(json!({"error": "forbidden"}))).into_response();
        WebState::harden(&mut response, path);
        response
    }

    /// Headers de segurança em toda resposta, sem duplicar o que já veio definido.
    fn harden(response: &mut Response, path: &str) {
        let html = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|ct| ct.to_ascii_lowercase().starts_with("text/html"));
        let headers = response.headers_mut();
        set_header(headers, "x-content-type-options", "nosniff");
        set_header(headers, "x-frame-options", "DENY");
        set_header(headers, "referrer-policy", "no-referrer");
        set_header(headers, "cross-origin-opener-policy", "same-origin");
        set_header(headers, "cross-origin-resource-policy", "same-origin");
        set_header(headers, "permissions-policy", "camera=(), microphone=(self), geolocation=(), payment=(), usb=()");
        set_header(headers, "content-security-policy", if html { CSP_HTML } else { CSP_OTHER });
        let no_store = path == "/health"
            || path == "/api"
            || path.starts_with("/api/")
            || path == "/web/auth"
            || path.starts_with("/web/auth/")
            || path == "/events"
            || path.starts_with("/term/");
        if no_store {
            // Substitui, não só preenche: o SSE já chega com `cache-control: no-cache`.
            headers.insert(HeaderName::from_static("cache-control"), HeaderValue::from_static("no-store"));
        }
    }

    fn static_file(&self, raw: &str) -> Result<(PathBuf, &'static str, &'static str), StatusCode> {
        let rel = Self::relative_path(raw).ok_or(StatusCode::BAD_REQUEST)?;
        let file = self.resolve(&rel).ok_or(StatusCode::NOT_FOUND)?;
        let mime = mime_for(&file).ok_or(StatusCode::NOT_FOUND)?;
        let cache = if raw.starts_with("/assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
        Ok((file, mime, cache))
    }

    /// Caminho relativo permitido: recusa travessia, barra invertida, nulo, codificações
    /// de ponto/barra e qualquer segmento vazio, ponto ou dotfile.
    fn relative_path(raw: &str) -> Option<PathBuf> {
        let decoded = percent_decode(raw)?;
        for text in [raw, decoded.as_str()] {
            let lower = text.to_ascii_lowercase();
            if ["..", "\\", "\0", "%2e", "%2f", "%5c"].iter().any(|bad| lower.contains(*bad)) {
                return None;
            }
        }
        let mut rel = PathBuf::new();
        for segment in decoded.trim_start_matches('/').split('/') {
            if segment.is_empty() || segment.starts_with('.') {
                return None;
            }
            rel.push(segment);
        }
        if rel.as_os_str().is_empty() {
            return None;
        }
        Some(rel)
    }

    /// Caminha componente por componente recusando symlink em qualquer nível e confirma
    /// que o destino canônico é um arquivo regular dentro da raiz.
    fn resolve(&self, rel: &Path) -> Option<PathBuf> {
        let mut current = self.root.clone();
        for segment in rel.components() {
            let Component::Normal(part) = segment else { return None };
            current.push(part);
            let meta = std::fs::symlink_metadata(&current).ok()?;
            if meta.file_type().is_symlink() {
                return None;
            }
        }
        let canonical = std::fs::canonicalize(&current).ok()?;
        if !canonical.starts_with(&self.root) || !std::fs::metadata(&canonical).ok()?.is_file() {
            return None;
        }
        Some(canonical)
    }
}

/// Guarda de um stream SSE aberto pela sessão Web: o stream termina no logout, no
/// despejo por limite de sessões ou na expiração (TTL absoluto ou inatividade). O
/// check de vida nunca atualiza `last_seen`, então o keep-alive do SSE não estende
/// o TTL ocioso.
pub(crate) struct SessaoStreamGuard {
    web: Arc<WebState>,
    id_hash: [u8; 32],
    revogado: tokio::sync::watch::Receiver<bool>,
}

impl SessaoStreamGuard {
    /// Sessão ainda presente e dentro da validade, sem atualizar `last_seen`.
    /// Vencida, sai do store e sinaliza todos os streams dela.
    pub(crate) fn esta_ativa(&self) -> bool {
        let now = Instant::now();
        let mut store = self.web.sessions.locked();
        let Some(index) = store.sessions.iter().position(|s| crypto::ct_eq(&s.id_hash, &self.id_hash)) else {
            return false;
        };
        let vencida = now.duration_since(store.sessions[index].created) > SESSION_TTL
            || now.duration_since(store.sessions[index].last_seen) > SESSION_IDLE;
        if vencida {
            let expirada = store.sessions.remove(index);
            let _ = expirada.revogado.send(true);
            return false;
        }
        true
    }

    /// Retorna quando a sessão é revogada ou expira. O intervalo de 5 s cobre a
    /// expiração por TTL/inatividade, que não sinaliza sozinha.
    pub(crate) async fn aguardar_fim(&mut self) {
        if *self.revogado.borrow() || !self.esta_ativa() {
            return;
        }
        let mut relogio = tokio::time::interval(Duration::from_secs(5));
        loop {
            tokio::select! {
                result = self.revogado.changed() => {
                    if result.is_err() || *self.revogado.borrow() {
                        return;
                    }
                }
                _ = relogio.tick() => {
                    if !self.esta_ativa() {
                        return;
                    }
                }
            }
        }
    }
}

/// Monta as rotas `/web/auth/*`, o fallback estático e o estado da interface Web local sobre
/// o router existente; chamado só quando o Hub tem a interface Web local ativa.
pub(crate) fn attach_routes(routes: Router<Arc<Hub>>, web: Arc<WebState>) -> Router<Arc<Hub>> {
    routes
        .route("/web/auth/session", get(session))
        .route("/web/auth/login", post(login).layer(DefaultBodyLimit::max(4096)))
        .route("/web/auth/logout", post(logout))
        .fallback(serve_static)
        .method_not_allowed_fallback(serve_static)
        .layer(Extension(web))
}

/// Middleware de segurança aplicado por fora de `secured`: até as respostas precoces de
/// `limitar_bearer` passam pelo check de `Host`/`Origin`/`Sec-Fetch-Site` e levam os
/// headers de segurança.
pub(crate) fn wrap_security(router: Router, web: Arc<WebState>) -> Router {
    router.layer(middleware::from_fn_with_state(web, security_middleware))
}

/// Fronteira comum a TODAS as rotas quando a interface Web local está ativa: loopback,
/// `Host` exato (contra DNS rebinding), `Origin` quando presente, `Sec-Fetch-Site`,
/// `OPTIONS` recusado e nunca um header `Access-Control-Allow-Origin`.
async fn security_middleware(State(web): State<Arc<WebState>>, request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    let peer = request.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
    let allowed = peer.is_some_and(|peer| {
        peer.ip().to_canonical().is_loopback()
            && request.method() != Method::OPTIONS
            && web.host_ok(request.headers())
            && web.origin_ok(request.headers())
            && WebState::sec_fetch_ok(&path, request.method(), request.headers())
    });
    if !allowed {
        return WebState::denied(&path);
    }
    let mut response = next.run(request).await;
    WebState::harden(&mut response, &path);
    response
}

async fn session(
    State(hub): State<Arc<Hub>>,
    Extension(web): Extension<Arc<WebState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let path = "/web/auth/session";
    if !web.base_guard(&headers, peer) || single_header(&headers, WEB_HEADER) != Some("1") {
        return WebState::denied(path);
    }
    match web.session_csrf(&headers) {
        Some(csrf) => {
            let body = WebState::session_json(&hub, &csrf);
            (StatusCode::OK, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
        }
        None => {
            (StatusCode::UNAUTHORIZED, [(header::CACHE_CONTROL, "no-store")], Json(json!({"authenticated": false})))
                .into_response()
        }
    }
}

async fn login(
    State(hub): State<Arc<Hub>>,
    Extension(web): Extension<Arc<WebState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = "/web/auth/login";
    if !web.base_guard(&headers, peer)
        || single_header(&headers, "origin") != Some(web.expected_origin.as_str())
        || single_header(&headers, WEB_HEADER) != Some("1")
    {
        return WebState::denied(path);
    }
    let json_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|ct| ct.to_ascii_lowercase().starts_with("application/json"));
    if !json_type {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, Json(json!({"error": "expected application/json"})))
            .into_response();
    }
    let given = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|v| v.get("token").and_then(Value::as_str).map(|t| t.trim().to_owned()));
    // Admissão, verificação e registro acontecem sob o mesmo lock: dois logins
    // simultâneos não passam pela admissão antes de qualquer registro de falha.
    {
        let mut limiter = web.limiter.locked();
        if limiter.blocked() {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, "60"), (header::CACHE_CONTROL, "no-store")],
                Json(json!({"error": "too many attempts"})),
            )
                .into_response();
        }
        match given.as_deref().filter(|t| !t.is_empty()) {
            Some(token) if crypto::ct_eq(token.as_bytes(), hub.token.as_bytes()) => limiter.clear(),
            _ => {
                limiter.record_failure();
                return unauthorized_login();
            }
        }
    }
    let (id, csrf) = web.sessions.locked().create();
    let cookie = format!("{COOKIE}={id}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200");
    (
        StatusCode::OK,
        [(header::SET_COOKIE, cookie.as_str()), (header::CACHE_CONTROL, "no-store")],
        Json(WebState::session_json(&hub, &csrf)),
    )
        .into_response()
}

fn unauthorized_login() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"authenticated": false, "error": "invalid token"})),
    )
        .into_response()
}

async fn logout(
    Extension(web): Extension<Arc<WebState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let path = "/web/auth/logout";
    if !web.base_guard(&headers, peer) || single_header(&headers, "origin") != Some(web.expected_origin.as_str()) {
        return WebState::denied(path);
    }
    if web.authenticate_api(&headers, peer, true).is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!({"authenticated": false})),
        )
            .into_response();
    }
    web.remove_session(&headers);
    (
        StatusCode::NO_CONTENT,
        [
            (header::SET_COOKIE, format!("{COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0").as_str()),
            (header::CACHE_CONTROL, "no-store"),
        ],
    )
        .into_response()
}

/// Rotas que nunca caem no fallback estático.
fn reserved(path: &str) -> bool {
    path == "/api"
        || path.starts_with("/api/")
        || path == "/events"
        || path.starts_with("/term/")
        || path == "/health"
        || path.starts_with("/hooks/")
        || path.starts_with("/oauth/")
        || path == "/web"
        || path.starts_with("/web/")
        || path == "/channel"
}

async fn serve_static(Extension(web): Extension<Arc<WebState>>, request: Request) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    if reserved(&path) {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "not found"}))).into_response();
    }
    if method != Method::GET && method != Method::HEAD {
        return (StatusCode::METHOD_NOT_ALLOWED, Json(json!({"error": "method not allowed"}))).into_response();
    }
    let (file, mime, cache) = if path == "/" || path == "/index.html" {
        // Resolve pelo mesmo caminho endurecido dos estáticos: symlink trocado depois do
        // boot nunca é servido como documento.
        match web.resolve(Path::new("index.html")) {
            Some(file) => (file, "text/html; charset=utf-8", "no-store"),
            None => return (StatusCode::NOT_FOUND, Json(json!({"error": "not found"}))).into_response(),
        }
    } else {
        match web.static_file(&path) {
            Ok(found) => found,
            Err(status) => return (status, Json(json!({"error": "not found"}))).into_response(),
        }
    };
    let Ok(bytes) = tokio::fs::read(&file).await else {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "not found"}))).into_response();
    };
    let mut response =
        (StatusCode::OK, [(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, cache)], bytes).into_response();
    if method == Method::HEAD {
        *response.body_mut() = Body::empty();
    }
    response
}

/// Um header com um único valor; ausente, duplicado ou não-UTF8 devolve `None`.
fn single_header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(value)
}

/// Leitura de header que separa "ausente" de "inválido": mais de um valor, ou valor
/// não-UTF8, nunca é aceito como se fosse um único valor.
enum HeaderCheck<'a> {
    Absent,
    Single(&'a str),
    Invalid,
}

fn check_header<'a>(headers: &'a HeaderMap, name: &str) -> HeaderCheck<'a> {
    let mut values = headers.get_all(name).iter();
    let Some(first) = values.next() else { return HeaderCheck::Absent };
    if values.next().is_some() {
        return HeaderCheck::Invalid;
    }
    match first.to_str() {
        Ok(value) => HeaderCheck::Single(value),
        Err(_) => HeaderCheck::Invalid,
    }
}

/// Valor do cookie `name`. Percorre todos os headers `cookie` e todos os segmentos: se
/// `name` aparece mais de uma vez, devolve `None` em vez de aceitar o primeiro.
fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    let mut found: Option<String> = None;
    for value in headers.get_all("cookie") {
        let raw = value.to_str().ok()?;
        for part in raw.split(';').map(str::trim) {
            let Some(value) = part.strip_prefix(prefix.as_str()) else { continue };
            if found.is_some() {
                return None;
            }
            found = Some(value.to_owned());
        }
    }
    found
}

fn set_header(headers: &mut HeaderMap, name: &'static str, value: &'static str) {
    if !headers.contains_key(name) {
        headers.insert(HeaderName::from_static(name), HeaderValue::from_static(value));
    }
}

/// Allowlist estrita de extensões MIME servidas como estático.
fn mime_for(file: &Path) -> Option<&'static str> {
    let ext = file.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "json" => "application/json; charset=utf-8",
        "webmanifest" => "application/manifest+json; charset=utf-8",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "txt" => "text/plain; charset=utf-8",
        _ => return None,
    })
}

fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let hi = char::from(hex[0]).to_digit(16)?;
            let lo = char::from(hex[1]).to_digit(16)?;
            out.push(u8::try_from(hi * 16 + lo).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use serde_json::json;

    struct Fixture {
        addr: SocketAddr,
        dir: PathBuf,
        server: tokio::task::JoinHandle<()>,
    }

    async fn fixture() -> Fixture {
        let parent = std::env::temp_dir().join(format!("web-local-{}", uuid::Uuid::new_v4()));
        let dir = parent.join("site");
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), b"<!doctype html><html><body>mexerica</body></html>").unwrap();
        std::fs::write(dir.join("assets/app.js"), b"console.log(1)").unwrap();
        std::fs::write(dir.join("arquivo.rs"), b"fn main() {}").unwrap();
        std::fs::write(parent.join("secret.txt"), b"segredo").unwrap();
        #[cfg(unix)]
        {
            let _ = std::os::unix::fs::symlink(dir.join("assets/app.js"), dir.join("link_dentro.js"));
            let _ = std::os::unix::fs::symlink(parent.join("secret.txt"), dir.join("link_fora.txt"));
        }
        let root = WebConfig::validate_dir(&dir).unwrap();
        let store = crate::store::Store::open(Path::new(":memory:")).unwrap();
        let id_dir = parent.join("id");
        std::fs::create_dir_all(&id_dir).unwrap();
        let hub = Hub::new(
            store,
            "hub-web".into(),
            crate::remote::identity::Identity::load_or_create(&id_dir).unwrap(),
            "token-web".into(),
            0,
        );
        hub.start().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        hub.set_web_local(Arc::new(WebState::new(root, addr.port()))).unwrap();
        let app = crate::api::router(hub.clone()).into_make_service_with_connect_info::<SocketAddr>();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Fixture { addr, dir: parent, server }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.server.abort();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// Pedido cru, sem cliente que normalizasse o caminho ou sobrescrevesse o `Host`.
    async fn raw(addr: SocketAddr, request: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).await.unwrap();
        out
    }

    fn origin(addr: SocketAddr) -> String {
        format!("http://{addr}")
    }

    #[test]
    fn sessoes_limitam_a_dezesseis_e_descartam_a_mais_antiga() {
        let mut store = SessionStore::default();
        let (first, _) = store.create();
        for _ in 0..MAX_SESSIONS {
            store.create();
        }
        assert_eq!(store.len(), MAX_SESSIONS);
        assert!(store.index(&first).is_none(), "a sessão mais antiga sai ao passar do limite");
        let (recent, _) = store.create();
        assert!(store.index(&recent).is_some());
        store.remove(&recent);
        assert!(store.index(&recent).is_none());
    }

    #[tokio::test]
    async fn api_e_events_sem_autenticacao_recusam() {
        let f = fixture().await;
        let client = reqwest::Client::new();
        let r = client.post(format!("http://{}/api/hello", f.addr)).json(&json!({})).send().await.unwrap();
        assert_eq!(r.status(), 401);
        let r = client.get(format!("http://{}/events", f.addr)).send().await.unwrap();
        assert_eq!(r.status(), 401);
        let r = client.get(format!("http://{}/api/inexistente", f.addr)).send().await.unwrap();
        assert_eq!(r.status(), 404);
        assert_eq!(r.json::<Value>().await.unwrap()["error"], "not found");
    }

    #[tokio::test]
    async fn login_recusa_sem_origin_sem_cabecalho_web_e_com_token_errado() {
        let f = fixture().await;
        let client = reqwest::Client::new();
        let url = format!("http://{}/web/auth/login", f.addr);
        let web_header = [("x-mexerica-web", "1")];
        // Sem Origin.
        let r = client.post(&url).headers(headers_of(web_header)).json(&json!({"token":"x"})).send().await.unwrap();
        assert_eq!(r.status(), 403);
        // Origin de outro site.
        let r = client
            .post(&url)
            .headers(headers_of(web_header))
            .header("origin", "https://evil.example.invalid")
            .json(&json!({"token":"x"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 403);
        // Sem o header próprio do login.
        let r = client.post(&url).header("origin", origin(f.addr)).json(&json!({"token":"x"})).send().await.unwrap();
        assert_eq!(r.status(), 403);
        // Cabecalhos completos, token errado.
        let r = client
            .post(&url)
            .headers(headers_of(web_header))
            .header("origin", origin(f.addr))
            .json(&json!({"token":"errado"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401);
        assert_eq!(r.json::<Value>().await.unwrap()["error"], "invalid token");
    }

    #[tokio::test]
    async fn login_bloqueia_na_decima_primeira_tentativa() {
        let f = fixture().await;
        let client = reqwest::Client::new();
        let url = format!("http://{}/web/auth/login", f.addr);
        for attempt in 1..=10 {
            let r = client
                .post(&url)
                .headers(headers_of([("x-mexerica-web", "1")]))
                .header("origin", origin(f.addr))
                .json(&json!({"token":"errado"}))
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), 401, "tentativa {attempt}");
        }
        let r = client
            .post(&url)
            .headers(headers_of([("x-mexerica-web", "1")]))
            .header("origin", origin(f.addr))
            .json(&json!({"token":"token-web"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 429);
        assert_eq!(r.headers().get("retry-after").and_then(|v| v.to_str().ok()), Some("60"));
    }

    #[tokio::test]
    async fn login_sucesso_cria_sessao_e_autentica_api_e_events() {
        let f = fixture().await;
        let client = reqwest::Client::new();
        let url = format!("http://{}/web/auth/login", f.addr);
        let r = client
            .post(&url)
            .headers(headers_of([("x-mexerica-web", "1")]))
            .header("origin", origin(f.addr))
            .json(&json!({"token":"token-web"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        let cookie = r.headers().get("set-cookie").and_then(|v| v.to_str().ok()).unwrap().to_owned();
        assert!(cookie.starts_with("mexerica_web_session="), "cookie: {cookie}");
        assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
        let body = r.json::<Value>().await.unwrap();
        let session_id = cookie.split(';').next().unwrap().trim_start_matches("mexerica_web_session=").to_owned();
        let csrf = body["csrfToken"].as_str().unwrap().to_owned();
        assert_eq!(body["authenticated"], true);
        assert!(csrf.len() >= 32);
        assert!(!body.to_string().contains("token-web"), "o token do Hub nunca sai no corpo");

        let auth = |extra: reqwest::RequestBuilder| {
            extra.header("cookie", format!("mexerica_web_session={session_id}")).header("x-mexerica-csrf", csrf.clone())
        };
        // Cookie sem CSRF: recusado.
        let r = client
            .post(format!("http://{}/api/hello", f.addr))
            .header("cookie", format!("mexerica_web_session={session_id}"))
            .header("origin", origin(f.addr))
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401);
        // CSRF errado, um único header: recusado pelo valor em si.
        let r = client
            .post(format!("http://{}/api/hello", f.addr))
            .header("cookie", format!("mexerica_web_session={session_id}"))
            .header("x-mexerica-csrf", "errado")
            .header("origin", origin(f.addr))
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401);
        // CSRF duplicado (válido + errado): recusado.
        let r = client
            .post(format!("http://{}/api/hello", f.addr))
            .header("cookie", format!("mexerica_web_session={session_id}"))
            .header("x-mexerica-csrf", &csrf)
            .header("x-mexerica-csrf", "errado")
            .header("origin", origin(f.addr))
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401);
        // Sem Origin no POST da API: recusado.
        let r = auth(client.post(format!("http://{}/api/hello", f.addr))).json(&json!({})).send().await.unwrap();
        assert_eq!(r.status(), 401);
        // Cookie + CSRF + Origin: a API responde.
        let r = auth(client.post(format!("http://{}/api/hello", f.addr)).header("origin", origin(f.addr)))
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers().get("cache-control").and_then(|v| v.to_str().ok()), Some("no-store"));
        // GET /events aceita cookie + CSRF, sem exigir Origin.
        let r = auth(client.get(format!("http://{}/events", f.addr))).send().await.unwrap();
        assert_eq!(r.status(), 200);
        assert!(r.headers()["content-type"].to_str().unwrap().starts_with("text/event-stream"));
        assert_eq!(r.headers()["cache-control"], "no-store");
        let r = client
            .get(format!("http://{}/events", f.addr))
            .header("cookie", format!("mexerica_web_session={session_id}"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401, "events sem CSRF é recusado");
        // Bearer continua funcionando como antes.
        let r = client
            .post(format!("http://{}/api/hello", f.addr))
            .bearer_auth("token-web")
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        // A sessão também responde em /web/auth/session.
        let r = client
            .get(format!("http://{}/web/auth/session", f.addr))
            .headers(headers_of([("x-mexerica-web", "1")]))
            .header("cookie", format!("mexerica_web_session={session_id}"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(r.json::<Value>().await.unwrap()["authenticated"], true);
    }

    #[tokio::test]
    async fn logout_invalida_a_sessao() {
        let f = fixture().await;
        let client = reqwest::Client::new();
        let r = client
            .post(format!("http://{}/web/auth/login", f.addr))
            .headers(headers_of([("x-mexerica-web", "1")]))
            .header("origin", origin(f.addr))
            .json(&json!({"token":"token-web"}))
            .send()
            .await
            .unwrap();
        let cookie = r.headers().get("set-cookie").and_then(|v| v.to_str().ok()).unwrap().to_owned();
        let session_id = cookie.split(';').next().unwrap().trim_start_matches("mexerica_web_session=").to_owned();
        let csrf = r.json::<Value>().await.unwrap()["csrfToken"].as_str().unwrap().to_owned();
        let r = client
            .post(format!("http://{}/web/auth/logout", f.addr))
            .header("origin", origin(f.addr))
            .header("cookie", format!("mexerica_web_session={session_id}"))
            .header("x-mexerica-csrf", &csrf)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 204);
        let cleared = r.headers().get("set-cookie").and_then(|v| v.to_str().ok()).unwrap();
        assert!(cleared.starts_with("mexerica_web_session=;") && cleared.contains("Max-Age=0"));
        let r = client
            .get(format!("http://{}/web/auth/session", f.addr))
            .headers(headers_of([("x-mexerica-web", "1")]))
            .header("cookie", format!("mexerica_web_session={session_id}"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401);
        let r = client
            .post(format!("http://{}/api/hello", f.addr))
            .header("origin", origin(f.addr))
            .header("cookie", format!("mexerica_web_session={session_id}"))
            .header("x-mexerica-csrf", &csrf)
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401);
    }

    #[tokio::test]
    async fn stream_events_encerra_ao_fazer_logout() {
        use futures::StreamExt;
        let f = fixture().await;
        let client = reqwest::Client::new();
        let r = client
            .post(format!("http://{}/web/auth/login", f.addr))
            .headers(headers_of([("x-mexerica-web", "1")]))
            .header("origin", origin(f.addr))
            .json(&json!({"token":"token-web"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        let cookie = r.headers().get("set-cookie").and_then(|v| v.to_str().ok()).unwrap().to_owned();
        let session_id = cookie.split(';').next().unwrap().trim_start_matches("mexerica_web_session=").to_owned();
        let csrf = r.json::<Value>().await.unwrap()["csrfToken"].as_str().unwrap().to_owned();
        let response = client
            .get(format!("http://{}/events?since=0&client=web", f.addr))
            .header("cookie", format!("mexerica_web_session={session_id}"))
            .header("x-mexerica-csrf", &csrf)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let mut body = response.bytes_stream();
        // O catch-up entrega o `hello` na hora: o primeiro frame chega sem esperar keep-alive.
        let mut frame = Vec::new();
        while !frame.ends_with(b"\n\n") {
            let chunk = tokio::time::timeout(Duration::from_secs(5), body.next())
                .await
                .expect("o primeiro frame chega em 5 s")
                .unwrap()
                .unwrap();
            frame.extend_from_slice(&chunk);
        }
        assert!(frame.windows(5).any(|w| w == b"data:"), "primeiro frame: {frame:?}");
        let r = client
            .post(format!("http://{}/web/auth/logout", f.addr))
            .header("origin", origin(f.addr))
            .header("cookie", format!("mexerica_web_session={session_id}"))
            .header("x-mexerica-csrf", &csrf)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 204);
        let fim = tokio::time::timeout(Duration::from_secs(2), async {
            while let Some(chunk) = body.next().await {
                chunk.unwrap();
            }
        })
        .await;
        assert!(fim.is_ok(), "o stream termina em até 2 s após o logout");
    }

    #[tokio::test]
    async fn sessao_stream_guard_encerra_no_despejo_e_expiracao() {
        let web = Arc::new(WebState::new(PathBuf::from("/"), 0));
        // Despejo: o 17º login remove a sessão mais antiga e sinaliza a guarda na hora.
        let (id_hash, receiver) = {
            let mut store = web.sessions.locked();
            store.create();
            (store.sessions[0].id_hash, store.sessions[0].revogado.subscribe())
        };
        let mut guarda = SessaoStreamGuard { web: web.clone(), id_hash, revogado: receiver };
        assert!(guarda.esta_ativa());
        for _ in 0..MAX_SESSIONS {
            web.sessions.locked().create();
        }
        assert!(*guarda.revogado.borrow(), "o despejo sinaliza a guarda imediatamente");
        assert!(!guarda.esta_ativa());
        tokio::time::timeout(Duration::from_millis(100), guarda.aguardar_fim()).await.unwrap();

        // Expiração por inatividade: o check de vida não estende `last_seen`.
        let (id_hash, receiver) = {
            let mut store = web.sessions.locked();
            store.create();
            let last = store.sessions.len() - 1;
            (store.sessions[last].id_hash, store.sessions[last].revogado.subscribe())
        };
        let mut guarda = SessaoStreamGuard { web: web.clone(), id_hash, revogado: receiver };
        let quase_vencida = Instant::now()
            .checked_sub(SESSION_IDLE.checked_sub(Duration::from_secs(60)).expect("SESSION_IDLE maior que 60 s"))
            .unwrap();
        let vencida = Instant::now().checked_sub(SESSION_IDLE + Duration::from_secs(1)).unwrap();
        {
            let mut store = web.sessions.locked();
            let index = store.sessions.iter().position(|s| crypto::ct_eq(&s.id_hash, &id_hash)).unwrap();
            store.sessions[index].last_seen = quase_vencida;
        }
        assert!(guarda.esta_ativa());
        {
            let store = web.sessions.locked();
            let index = store.sessions.iter().position(|s| crypto::ct_eq(&s.id_hash, &id_hash)).unwrap();
            assert_eq!(store.sessions[index].last_seen, quase_vencida, "o check de vida não estende o TTL ocioso");
        }
        {
            let mut store = web.sessions.locked();
            let index = store.sessions.iter().position(|s| crypto::ct_eq(&s.id_hash, &id_hash)).unwrap();
            store.sessions[index].last_seen = vencida;
        }
        assert!(!guarda.esta_ativa(), "vencida por inatividade, a guarda encerra");
        assert!(*guarda.revogado.borrow(), "a expiração sinaliza a guarda");
        {
            let store = web.sessions.locked();
            assert!(
                store.sessions.iter().all(|s| !crypto::ct_eq(&s.id_hash, &id_hash)),
                "a sessão vencida sai do store"
            );
        }
        tokio::time::timeout(Duration::from_millis(100), guarda.aguardar_fim()).await.unwrap();
    }

    #[tokio::test]
    async fn host_origin_e_sec_fetch_de_outros_sites_sao_recusados() {
        let f = fixture().await;
        let client = reqwest::Client::new();
        let host = f.addr.to_string();
        // Host de outro domínio (DNS rebinding) em documento, health e API.
        for target in ["/", "/health", "/api/hello"] {
            let request =
                format!("GET {target} HTTP/1.1\r\nHost: evil.example.invalid:19423\r\nConnection: close\r\n\r\n");
            let out = raw(f.addr, &request).await;
            assert!(out.starts_with("HTTP/1.1 403"), "{target}: {}", out.lines().next().unwrap_or_default());
        }
        // Origin de outro site.
        let r = client
            .get(format!("http://{addr}/", addr = f.addr))
            .header("origin", "https://evil.example.invalid")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 403);
        let r = client
            .post(format!("http://{addr}/api/hello", addr = f.addr))
            .bearer_auth("token-web")
            .header("origin", "https://evil.example.invalid")
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 403);
        // Sec-Fetch-Site de outro site, inclusive na API autenticada por Bearer.
        let r = client
            .post(format!("http://{addr}/api/hello", addr = f.addr))
            .bearer_auth("token-web")
            .header("sec-fetch-site", "cross-site")
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 403);
        let r = client
            .get(format!("http://{addr}/events", addr = f.addr))
            .bearer_auth("token-web")
            .header("sec-fetch-site", "cross-site")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 403);
        // OPTIONS nunca é respondido e nunca há CORS.
        let r =
            client.request(Method::OPTIONS, format!("http://{addr}/api/hello", addr = f.addr)).send().await.unwrap();
        assert_eq!(r.status(), 403);
        assert!(r.headers().get("access-control-allow-origin").is_none());
        // O pedido cru com Host correto segue passando.
        let request = format!("GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
        let out = raw(f.addr, &request).await;
        assert!(out.starts_with("HTTP/1.1 200"), "{}", out.lines().next().unwrap_or_default());
    }

    #[tokio::test]
    async fn headers_duplicados_de_origem_e_sec_fetch_site_recusam() {
        let f = fixture().await;
        let host = f.addr.to_string();
        let valid = origin(f.addr);
        let evil = "https://evil.example.invalid";
        for (first, second) in [(valid.as_str(), evil), (evil, evil), (valid.as_str(), valid.as_str())] {
            let request = format!(
                "GET / HTTP/1.1\r\nHost: {host}\r\nOrigin: {first}\r\nOrigin: {second}\r\nConnection: close\r\n\r\n"
            );
            let out = raw(f.addr, &request).await;
            assert!(
                out.starts_with("HTTP/1.1 403"),
                "origin duplicado ({first}, {second}): {}",
                out.lines().next().unwrap_or_default()
            );
        }
        let request = format!(
            "GET / HTTP/1.1\r\nHost: {host}\r\nSec-Fetch-Site: same-origin\r\nSec-Fetch-Site: same-origin\r\nConnection: close\r\n\r\n"
        );
        let out = raw(f.addr, &request).await;
        assert!(
            out.starts_with("HTTP/1.1 403"),
            "sec-fetch-site duplicado: {}",
            out.lines().next().unwrap_or_default()
        );
        let request = format!(
            "GET /api/hello HTTP/1.1\r\nHost: {host}\r\nSec-Fetch-Site: same-origin\r\nSec-Fetch-Site: none\r\nConnection: close\r\n\r\n"
        );
        let out = raw(f.addr, &request).await;
        assert!(
            out.starts_with("HTTP/1.1 403"),
            "sec-fetch-site duplicado na API: {}",
            out.lines().next().unwrap_or_default()
        );
    }

    #[tokio::test]
    async fn cookie_de_sessao_duplicado_recusa() {
        let f = fixture().await;
        let client = reqwest::Client::new();
        let r = client
            .post(format!("http://{}/web/auth/login", f.addr))
            .headers(headers_of([("x-mexerica-web", "1")]))
            .header("origin", origin(f.addr))
            .json(&json!({"token":"token-web"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        let cookie = r.headers().get("set-cookie").and_then(|v| v.to_str().ok()).unwrap().to_owned();
        let session_id = cookie.split(';').next().unwrap().trim_start_matches("mexerica_web_session=").to_owned();
        let host = f.addr.to_string();
        // Um cookie válido autentica a sessão; dois recusam.
        let request = format!(
            "GET /web/auth/session HTTP/1.1\r\nHost: {host}\r\nx-mexerica-web: 1\r\nCookie: mexerica_web_session={session_id}\r\nConnection: close\r\n\r\n"
        );
        let out = raw(f.addr, &request).await;
        assert!(out.starts_with("HTTP/1.1 200"), "cookie único: {}", out.lines().next().unwrap_or_default());
        let request = format!(
            "GET /web/auth/session HTTP/1.1\r\nHost: {host}\r\nx-mexerica-web: 1\r\nCookie: mexerica_web_session={session_id}; mexerica_web_session={session_id}\r\nConnection: close\r\n\r\n"
        );
        let out = raw(f.addr, &request).await;
        assert!(
            out.starts_with("HTTP/1.1 401"),
            "cookie repetido no mesmo header: {}",
            out.lines().next().unwrap_or_default()
        );
        let request = format!(
            "GET /web/auth/session HTTP/1.1\r\nHost: {host}\r\nx-mexerica-web: 1\r\nCookie: mexerica_web_session={session_id}\r\nCookie: mexerica_web_session={session_id}\r\nConnection: close\r\n\r\n"
        );
        let out = raw(f.addr, &request).await;
        assert!(
            out.starts_with("HTTP/1.1 401"),
            "cookie repetido em dois headers: {}",
            out.lines().next().unwrap_or_default()
        );
    }

    #[tokio::test]
    async fn bearer_da_ponte_invalido_leva_headers_de_seguranca() {
        let f = fixture().await;
        let host = f.addr.to_string();
        // Host inválido: recusado antes da autenticação, mesmo com Bearer.
        let request = "GET /api/hello HTTP/1.1\r\nHost: evil.example.invalid\r\nAuthorization: Bearer ponte_invalido\r\nConnection: close\r\n\r\n";
        let out = raw(f.addr, request).await;
        assert!(out.starts_with("HTTP/1.1 403"), "{}", out.lines().next().unwrap_or_default());
        let lower = out.to_ascii_lowercase();
        assert!(lower.contains("x-content-type-options: nosniff"), "403 sem nosniff: {out}");
        assert!(lower.contains("x-frame-options: deny"), "403 sem x-frame-options: {out}");
        // Host válido: a resposta precoce do Bearer inválido chega com os headers de segurança.
        let request = format!(
            "GET /api/hello HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer ponte_invalido\r\nConnection: close\r\n\r\n"
        );
        let out = raw(f.addr, &request).await;
        assert!(out.starts_with("HTTP/1.1 401"), "{}", out.lines().next().unwrap_or_default());
        let lower = out.to_ascii_lowercase();
        assert!(lower.contains("x-content-type-options: nosniff"), "401 sem nosniff: {out}");
        assert!(lower.contains("x-frame-options: deny"), "401 sem x-frame-options: {out}");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn index_trocado_por_symlink_depois_do_boot_nao_e_servido() {
        let f = fixture().await;
        let client = reqwest::Client::new();
        let r = client.get(format!("http://{}/", f.addr)).send().await.unwrap();
        assert_eq!(r.status(), 200);
        let site = f.dir.join("site");
        std::fs::remove_file(site.join("index.html")).unwrap();
        std::os::unix::fs::symlink(f.dir.join("secret.txt"), site.join("index.html")).unwrap();
        for path in ["/", "/index.html"] {
            let r = client.get(format!("http://{}{path}", f.addr)).send().await.unwrap();
            assert_eq!(r.status(), 404, "{path} com index.html symlinkado");
            assert!(!r.text().await.unwrap().contains("segredo"), "{path} não pode servir o alvo do symlink");
        }
    }

    #[tokio::test]
    async fn estaticos_entregam_index_com_headers_duros_e_cache_por_rota() {
        let f = fixture().await;
        let client = reqwest::Client::new();
        let r = client.get(format!("http://{addr}/", addr = f.addr)).send().await.unwrap();
        assert_eq!(r.status(), 200);
        assert!(r.headers()["content-type"].to_str().unwrap().starts_with("text/html"));
        let csp = r.headers()["content-security-policy"].to_str().unwrap();
        assert!(csp.contains("frame-ancestors 'none'") && csp.contains("connect-src 'self'"));
        assert_eq!(r.headers()["x-frame-options"], "DENY");
        assert_eq!(r.headers()["x-content-type-options"], "nosniff");
        assert_eq!(r.headers()["referrer-policy"], "no-referrer");
        assert_eq!(r.headers()["cache-control"], "no-store");
        assert!(r.text().await.unwrap().contains("mexerica"));
        let r = client.get(format!("http://{addr}/assets/app.js", addr = f.addr)).send().await.unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["cache-control"].to_str().unwrap(), "public, max-age=31536000, immutable");
        // Extensão fora da allowlist: 404, nunca servida.
        let r = client.get(format!("http://{addr}/arquivo.rs", addr = f.addr)).send().await.unwrap();
        assert_eq!(r.status(), 404);
        // Navegação de documento com Sec-Fetch-Site none é aceita.
        let r = client
            .get(format!("http://{addr}/", addr = f.addr))
            .header("sec-fetch-site", "none")
            .header("sec-fetch-mode", "navigate")
            .header("sec-fetch-dest", "document")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
    }

    #[tokio::test]
    async fn traversal_dotfiles_e_symlinks_nunca_sao_servidos() {
        let f = fixture().await;
        let host = f.addr.to_string();
        for path in ["/../secret.txt", "/%2e%2e/secret.txt", "/%2fetc%2fpasswd", "//secret.txt"] {
            let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
            let out = raw(f.addr, &request).await;
            let status = out.lines().next().unwrap_or_default();
            assert!(status.starts_with("HTTP/1.1 4"), "{path}: {status}");
            assert!(!out.contains("segredo"), "{path} não pode vazar conteúdo fora da raiz");
        }
        let client = reqwest::Client::new();
        let r = client.get(format!("http://{addr}/.env", addr = f.addr)).send().await.unwrap();
        assert_eq!(r.status(), 400);
        #[cfg(unix)]
        {
            let r = client.get(format!("http://{addr}/link_dentro.js", addr = f.addr)).send().await.unwrap();
            assert_eq!(r.status(), 404, "symlink dentro da raiz também é recusado");
            let r = client.get(format!("http://{addr}/link_fora.txt", addr = f.addr)).send().await.unwrap();
            assert_eq!(r.status(), 404);
        }
    }

    use reqwest::header::HeaderMap;

    fn headers_of<const N: usize>(pairs: [(&'static str, &'static str); N]) -> HeaderMap {
        use reqwest::header::{HeaderName, HeaderValue};
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(HeaderName::from_static(name), HeaderValue::from_str(value).unwrap());
        }
        headers
    }
}
