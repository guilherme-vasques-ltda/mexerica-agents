//! Proxy CONNECT por sessão. Não habilita navegação sem os demais gates de contenção.

use std::{collections::HashSet, io, net::SocketAddr, sync::Arc, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
    time::timeout,
};

use super::network;
use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

const MAX_HEADER: usize = 8192;
const MAX_CONNECTIONS: usize = 32;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const TUNNEL_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Endpoint {
    Tcp(SocketAddr),
    Unix(std::path::PathBuf),
}

/// O supervisor deve guardar esta instância durante toda a sessão.
#[allow(dead_code)]
pub(super) struct Proxy {
    addr: Endpoint,
    task: JoinHandle<()>,
    token: Arc<str>,
    auth_failures: Arc<std::sync::atomic::AtomicUsize>,
    #[cfg(target_os = "linux")]
    socket: Option<super::proxy_socket::SocketFile>,
}

#[allow(dead_code)]
impl Proxy {
    pub(super) async fn start() -> io::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| io::Error::other("fonte aleatória indisponível"))?;
        let token: Arc<str> = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes).into();
        let auth_failures = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let task = tokio::spawn(serve_counted(
            listener,
            Arc::new(SystemResolver),
            Arc::clone(&token),
            Arc::new(HashSet::new()),
            Arc::clone(&auth_failures),
        ));
        Ok(Self {
            addr: Endpoint::Tcp(addr),
            task,
            token,
            auth_failures,
            #[cfg(target_os = "linux")]
            socket: None,
        })
    }

    /// Cria um endpoint privado para a sessão, sem habilitar navegação por si só.
    // O contrato assíncrono exige runtime antes de registrar o listener.
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
    #[cfg(target_os = "linux")]
    pub(super) async fn start_unix(path: &std::path::Path) -> io::Result<Self> {
        Self::start_unix_inner(path, HashSet::new()).await
    }

    /// Somente testes: libera destinos locais exatos que a política pública recusaria.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) async fn start_unix_test(path: &std::path::Path, allowed: HashSet<SocketAddr>) -> io::Result<Self> {
        Self::start_unix_inner(path, allowed).await
    }

    // O contrato assíncrono exige runtime antes de registrar o listener.
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
    #[cfg(target_os = "linux")]
    async fn start_unix_inner(path: &std::path::Path, allowed: HashSet<SocketAddr>) -> io::Result<Self> {
        let (listener, socket) = super::proxy_socket::SocketFile::bind(path)?;
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| io::Error::other("fonte aleatória indisponível"))?;
        let token: Arc<str> = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes).into();
        let auth_failures = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let task = tokio::spawn(serve_counted(
            listener,
            Arc::new(SystemResolver),
            Arc::clone(&token),
            Arc::new(allowed),
            Arc::clone(&auth_failures),
        ));
        Ok(Self { addr: Endpoint::Unix(path.to_owned()), task, token, auth_failures, socket: Some(socket) })
    }

    pub(super) fn token(&self) -> &str {
        &self.token
    }

    #[cfg(test)]
    pub(super) fn auth_failures(&self) -> usize {
        self.auth_failures.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Ainda não há implementação ancorada para outros sistemas Unix.
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
    #[cfg(all(unix, not(target_os = "linux")))]
    pub(super) async fn start_unix(_path: &std::path::Path) -> io::Result<Self> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "socket Unix seguro indisponível nesta plataforma"))
    }

    pub(super) fn addr(&self) -> &Endpoint {
        &self.addr
    }

    pub(super) async fn stop(&mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
        #[cfg(target_os = "linux")]
        drop(self.socket.take());
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

trait Listener: Send + 'static {
    type Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static;
    fn accept(&self) -> impl Future<Output = io::Result<Self::Stream>> + Send;
}

impl Listener for TcpListener {
    type Stream = TcpStream;
    async fn accept(&self) -> io::Result<Self::Stream> {
        Ok(TcpListener::accept(self).await?.0)
    }
}

#[cfg(unix)]
impl Listener for tokio::net::UnixListener {
    type Stream = tokio::net::UnixStream;
    async fn accept(&self) -> io::Result<Self::Stream> {
        Ok(tokio::net::UnixListener::accept(self).await?.0)
    }
}

trait Resolver: Send + Sync + 'static {
    fn resolve(&self, host: &str, port: u16) -> impl Future<Output = anyhow::Result<Vec<SocketAddr>>> + Send;

    fn connect(&self, address: SocketAddr) -> impl Future<Output = io::Result<TcpStream>> + Send {
        TcpStream::connect(address)
    }
}

struct SystemResolver;
impl Resolver for SystemResolver {
    async fn resolve(&self, host: &str, port: u16) -> anyhow::Result<Vec<SocketAddr>> {
        network::resolve(host, port).await
    }
}

#[cfg(test)]
async fn serve<R: Resolver, L: Listener>(
    listener: L,
    resolver: Arc<R>,
    token: Arc<str>,
    allowed: Arc<HashSet<SocketAddr>>,
) {
    serve_counted(listener, resolver, token, allowed, Arc::new(std::sync::atomic::AtomicUsize::new(0))).await;
}

async fn serve_counted<R: Resolver, L: Listener>(
    listener: L,
    resolver: Arc<R>,
    token: Arc<str>,
    allowed: Arc<HashSet<SocketAddr>>,
    auth_failures: Arc<std::sync::atomic::AtomicUsize>,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(stream) => {
                    if connections.len() < MAX_CONNECTIONS {
                        connections.spawn(handle(stream, Arc::clone(&resolver), Arc::clone(&token), Arc::clone(&allowed), Arc::clone(&auth_failures)));
                    }
                }
                Err(_) => break,
            },
            Some(_) = connections.join_next(), if !connections.is_empty() => {},
        }
    }
    // JoinSet também cancela túneis ativos quando a tarefa da sessão é abortada.
}

async fn handle<R: Resolver, S: AsyncRead + AsyncWrite + Unpin>(
    mut client: S,
    resolver: Arc<R>,
    token: Arc<str>,
    allowed: Arc<HashSet<SocketAddr>>,
    auth_failures: Arc<std::sync::atomic::AtomicUsize>,
) {
    let result = timeout(REQUEST_TIMEOUT, establish(&mut client, &*resolver, &token, &allowed)).await;
    if matches!(result, Ok(Err(ProxyError::Auth))) {
        auth_failures.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    match result {
        Ok(Ok(mut upstream)) => {
            if client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await.is_ok() {
                let _ = timeout(TUNNEL_TIMEOUT, tokio::io::copy_bidirectional(&mut client, &mut upstream)).await;
            }
        }
        Ok(Err(ProxyError::Auth)) => {
            let _ = client
                .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"Mexerica\"\r\nProxy-Authenticate: Bearer\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")
                .await;
        }
        _ => {
            let _ = client
                .write_all(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Type: text/plain\r\nContent-Length: 21\r\n\r\nsomente HTTPS na v1\n")
                .await;
        }
    }
}

#[derive(Debug)]
enum ProxyError {
    Auth,
    Other,
}

async fn establish<R: Resolver, S: AsyncRead + Unpin>(
    client: &mut S,
    resolver: &R,
    token: &str,
    allowed: &HashSet<SocketAddr>,
) -> Result<TcpStream, ProxyError> {
    let request = read_header(client).await.map_err(|_| ProxyError::Other)?;
    let text = std::str::from_utf8(&request).map_err(|_| ProxyError::Other)?;
    let mut credentials = text.split("\r\n").skip(1).filter_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("proxy-authorization").then_some(value.trim())
    });
    let supplied = credentials.next().and_then(supplied_token);
    if credentials.next().is_some() || !supplied.as_deref().is_some_and(|v| token_matches(token, v)) {
        return Err(ProxyError::Auth);
    }
    let line = text.split("\r\n").next().unwrap_or_default();
    if line.starts_with("CONNECT ") {
        connect_request(text, resolver, allowed).await.map_err(|_| ProxyError::Other)
    } else {
        // Na v1 só HTTPS por CONNECT; HTTP simples não é encaminhado.
        Err(ProxyError::Other)
    }
}

fn supplied_token(value: &str) -> Option<String> {
    if let Some(bearer) = value.strip_prefix("Bearer ") {
        return Some(bearer.to_owned());
    }
    let (scheme, encoded) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = base64::engine::general_purpose::STANDARD.decode(encoded).ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (username, password) = decoded.split_once(':')?;
    // O CDP entrega usuário `mx`; `mexerica` é aceito do preparo anterior (#964).
    matches!(username, "mx" | "mexerica").then(|| password.to_owned())
}

fn token_matches(expected: &str, supplied: &str) -> bool {
    // HMAC verifica a tag em tempo constante, inclusive para valores de tamanho diferente.
    let Ok(mut expected_mac) = Hmac::<Sha256>::new_from_slice(b"mexerica proxy token comparison") else { return false };
    expected_mac.update(expected.as_bytes());
    let tag = expected_mac.finalize().into_bytes();
    let Ok(mut supplied_mac) = Hmac::<Sha256>::new_from_slice(b"mexerica proxy token comparison") else { return false };
    supplied_mac.update(supplied.as_bytes());
    supplied_mac.verify_slice(&tag).is_ok()
}

async fn read_header(client: &mut (impl AsyncRead + Unpin)) -> anyhow::Result<Vec<u8>> {
    use anyhow::ensure;
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") && request.len() < MAX_HEADER {
        let mut byte = [0];
        ensure!(client.read(&mut byte).await? == 1, "header incompleto");
        request.push(byte[0]);
    }
    ensure!(request.ends_with(b"\r\n\r\n"), "header grande demais");
    Ok(request)
}

async fn resolve_host<R: Resolver>(resolver: &R, host: &str, port: u16) -> anyhow::Result<Vec<SocketAddr>> {
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    resolver.resolve(host, port).await
}

async fn checked_connect<R: Resolver>(
    resolver: &R,
    host: &str,
    port: u16,
    allowed: &HashSet<SocketAddr>,
) -> anyhow::Result<TcpStream> {
    use anyhow::Context;
    let addresses = resolve_host(resolver, host, port).await?;
    if addresses.is_empty() || !addresses.iter().all(|address| allowed.contains(address)) {
        network::validate_addresses(&addresses, port)?;
    }
    let mut last_error = None;
    for address in addresses {
        match resolver.connect(address).await {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.context("sem endereço")?.into())
}

async fn connect_request<R: Resolver>(
    text: &str,
    resolver: &R,
    allowed: &HashSet<SocketAddr>,
) -> anyhow::Result<TcpStream> {
    use anyhow::{Context, ensure};
    let line = text.split("\r\n").next().context("requisição vazia")?;
    let mut parts = line.split(' ');
    ensure!(parts.next() == Some("CONNECT"), "método recusado");
    let authority = parts.next().context("destino ausente")?;
    ensure!(matches!(parts.next(), Some("HTTP/1.1" | "HTTP/1.0")) && parts.next().is_none(), "versão inválida");
    let url = reqwest::Url::parse(&format!("http://{authority}/"))?;
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.query().is_none()
            && url.fragment().is_none(),
        "destino inválido"
    );
    // Exigir porta explícita; Url normaliza :80, então leia-a da autoridade original.
    let (host_part, port_part) = authority.rsplit_once(':').context("porta ausente")?;
    ensure!(
        !host_part.is_empty() && !port_part.is_empty() && port_part.bytes().all(|b| b.is_ascii_digit()),
        "porta inválida"
    );
    let port: u16 = port_part.parse()?;
    ensure!(port == 443 || allowed.iter().any(|address| address.port() == port), "porta recusada");
    let host = url.host_str().context("host ausente")?.trim_matches(['[', ']']);
    // Mesmo um resolvedor substituído deve obedecer à política; nunca resolva novamente.
    checked_connect(resolver, host, port, allowed).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[cfg(unix)]
    struct TestDir(std::path::PathBuf);
    #[cfg(unix)]
    impl TestDir {
        fn new() -> Self {
            use std::os::unix::fs::PermissionsExt;
            let mut bytes = [0u8; 16];
            getrandom::fill(&mut bytes).unwrap();
            let name = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
            let path = std::env::temp_dir().canonicalize().unwrap().join(name);
            std::fs::create_dir(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            Self(path)
        }
    }
    #[cfg(unix)]
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    async fn unix_request(path: &std::path::Path, token: Option<&str>, target: &str) -> String {
        let mut client = tokio::net::UnixStream::connect(path).await.unwrap();
        let header = token.map(|t| format!("Proxy-Authorization: Bearer {t}\r\n")).unwrap_or_default();
        client.write_all(format!("CONNECT {target} HTTP/1.1\r\n{header}\r\n").as_bytes()).await.unwrap();
        let mut response = Vec::new();
        while !response.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            client.read_exact(&mut byte).await.unwrap();
            response.push(byte[0]);
        }
        String::from_utf8(response).unwrap()
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn unix_autentica_filtra_e_remove_socket() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TestDir::new();
        let path = dir.0.join("proxy.sock");
        let proxy = Proxy::start_unix(&path).await.unwrap();
        assert_eq!(std::fs::symlink_metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        for token in [None, Some("errado")] {
            assert!(unix_request(&path, token, "127.0.0.1:443").await.starts_with("HTTP/1.1 407"));
        }
        assert!(unix_request(&path, Some(&proxy.token), "127.0.0.1:443").await.starts_with("HTTP/1.1 403"));
        drop(proxy);
        assert!(!path.exists());
        let mut proxy = Proxy::start_unix(&path).await.unwrap();
        proxy.stop().await;
        assert!(!path.exists());
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn unix_reinicia_apos_queda_e_preserva_substituicoes() {
        let dir = TestDir::new();
        let path = dir.0.join("proxy.sock");
        let proxy = Proxy::start_unix(&path).await.unwrap();
        assert_eq!(proxy.addr(), &Endpoint::Unix(path.clone()));
        proxy.task.abort();
        std::mem::forget(proxy); // simula processo morto sem executar Drop
        let proxy = Proxy::start_unix(&path).await.unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"outro arquivo").unwrap();
        drop(proxy);
        assert_eq!(std::fs::read(&path).unwrap(), b"outro arquivo");
        assert!(Proxy::start_unix(&path).await.is_err());
        std::fs::remove_file(&path).unwrap();
        let proxy = Proxy::start_unix(&path).await.unwrap();
        std::fs::remove_file(&path).unwrap();
        let replacement = std::os::unix::net::UnixListener::bind(&path).unwrap();
        drop(proxy);
        assert!(path.exists());
        drop(replacement);
        assert!(Proxy::start_unix(std::path::Path::new("relativo.sock")).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unix_connect_valido_usa_o_mesmo_fluxo() {
        let dir = TestDir::new();
        let path = dir.0.join("proxy.sock");
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let fake = Arc::new(Fake {
            answers: vec![vec!["8.8.8.8:443".parse().unwrap()]],
            calls: AtomicUsize::new(0),
            target: upstream.local_addr().unwrap(),
        });
        let task = tokio::spawn(serve(listener, fake, Arc::from("teste"), Arc::new(HashSet::new())));
        assert!(unix_request(&path, Some("teste"), "exemplo.test:443").await.starts_with("HTTP/1.1 200"));
        task.abort();
        let _ = task.await;
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn unix_recusa_caminhos_inseguros() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = TestDir::new();
        let path = dir.0.join("proxy.sock");
        std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Proxy::start_unix(&path).await.is_err());
        std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o700)).unwrap();
        symlink("ausente", &path).unwrap();
        assert!(Proxy::start_unix(&path).await.is_err());
        std::fs::remove_file(&path).unwrap();
        let link = dir.0.join("link");
        symlink(&dir.0, &link).unwrap();
        assert!(Proxy::start_unix(&link.join("proxy.sock")).await.is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unix_recusa_outro_dono() {
        // SAFETY: valores sintéticos, não passados ao sistema.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        stat.st_uid = 123;
        stat.st_mode = libc::S_IFDIR | 0o700;
        assert!(super::super::proxy_socket::is_owned_directory(&stat, 123));
        assert!(!super::super::proxy_socket::is_owned_directory(&stat, 124));
        stat.st_mode = libc::S_IFSOCK | 0o600;
        assert!(super::super::proxy_socket::is_owned_socket(&stat, 123));
        assert!(!super::super::proxy_socket::is_owned_socket(&stat, 124));
    }

    struct Fake {
        answers: Vec<Vec<SocketAddr>>,
        calls: AtomicUsize,
        target: SocketAddr,
    }
    impl Resolver for Fake {
        fn resolve(&self, _: &str, _: u16) -> impl Future<Output = anyhow::Result<Vec<SocketAddr>>> + Send {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(self.answers[n.min(self.answers.len() - 1)].clone()))
        }
        async fn connect(&self, _: SocketAddr) -> io::Result<TcpStream> {
            TcpStream::connect(self.target).await
        }
    }

    async fn request(addr: SocketAddr, line: &str) -> (TcpStream, String) {
        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(format!("{line}\r\nProxy-Authorization: Bearer teste\r\n\r\n").as_bytes()).await.unwrap();
        let mut response = Vec::new();
        while !response.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            client.read_exact(&mut byte).await.unwrap();
            response.push(byte[0]);
        }
        (client, String::from_utf8(response).unwrap())
    }

    #[tokio::test]
    async fn recusa_credencial_ausente_ou_errada() {
        let proxy = Proxy::start().await.unwrap();
        for header in ["", "Proxy-Authorization: Bearer errado\r\n"] {
            let mut client = TcpStream::connect(match proxy.addr() {
                Endpoint::Tcp(addr) => *addr,
                Endpoint::Unix(_) => panic!("endpoint inesperado"),
            })
            .await
            .unwrap();
            client.write_all(format!("CONNECT example.com:443 HTTP/1.1\r\n{header}\r\n").as_bytes()).await.unwrap();
            let mut response = Vec::new();
            client.read_to_end(&mut response).await.unwrap();
            assert!(response.starts_with(b"HTTP/1.1 407"));
        }
        assert!(!token_matches(&proxy.token, "errado"));
        assert!(token_matches(&proxy.token, &proxy.token));
    }

    #[tokio::test]
    async fn filtra_dns_porta_e_rebinding_e_fecha_tuneis() {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let fake = Arc::new(Fake {
            answers: vec![vec!["8.8.8.8:443".parse().unwrap()], vec!["127.0.0.1:443".parse().unwrap()]],
            calls: AtomicUsize::new(0),
            target: upstream.local_addr().unwrap(),
        });
        let task = tokio::spawn(serve(listener, Arc::clone(&fake), Arc::from("teste"), Arc::new(HashSet::new())));
        let (mut client, response) = request(addr, "CONNECT exemplo.test:443 HTTP/1.1").await;
        assert!(response.starts_with("HTTP/1.1 200"));
        let (mut server, _) = upstream.accept().await.unwrap();
        client.write_all(b"oi").await.unwrap();
        let mut data = [0; 2];
        server.read_exact(&mut data).await.unwrap();
        assert_eq!(&data, b"oi");
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
        let (_, response) = request(addr, "CONNECT exemplo.test:443 HTTP/1.1").await;
        assert!(response.starts_with("HTTP/1.1 403"));
        let (_, response) = request(addr, "CONNECT exemplo.test:22 HTTP/1.1").await;
        assert!(response.starts_with("HTTP/1.1 403"));
        let (_, response) = request(addr, "CONNECT exemplo.test:80 HTTP/1.1").await;
        assert!(response.starts_with("HTTP/1.1 403"), "CONNECT :80 deve ser recusado na v1");
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
        task.abort();
        let _ = task.await;
        assert_eq!(client.read(&mut data).await.unwrap(), 0);
    }

    async fn origem_http(
        responder_407: bool,
    ) -> (SocketAddr, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>, JoinHandle<()>) {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = upstream.local_addr().unwrap();
        let received = Arc::new(Mutex::new(Vec::new()));
        let connections = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&received);
        let counted = Arc::clone(&connections);
        let server = tokio::spawn(async move {
            while let Ok((mut stream, _)) = upstream.accept().await {
                counted.fetch_add(1, Ordering::SeqCst);
                let seen = Arc::clone(&seen);
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut byte = [0u8; 1];
                    while !request.ends_with(b"\r\n\r\n") {
                        if stream.read(&mut byte).await.unwrap() == 0 {
                            break;
                        }
                        request.push(byte[0]);
                    }
                    seen.lock().unwrap().push(String::from_utf8_lossy(&request).into_owned());
                    if responder_407 {
                        let _ = stream
                            .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"origem\"\r\nContent-Length: 0\r\n\r\n")
                            .await;
                    } else {
                        // Keep-alive de propósito: mesmo sem fechar, o proxy deve encerrar após a resposta.
                        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").await;
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                });
            }
        });
        (target, connections, received, server)
    }

    #[tokio::test]
    async fn http_simples_recusado_sem_tocar_origem() {
        let (target, connections, _received, server) = origem_http(false).await;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let fake = Arc::new(Fake { answers: vec![vec![target]], calls: AtomicUsize::new(0), target });
        let mut allowed = HashSet::new();
        allowed.insert(target);
        let task = tokio::spawn(serve(listener, fake, Arc::from("teste"), Arc::new(allowed)));
        for method in ["GET", "HEAD"] {
            let mut client = TcpStream::connect(addr).await.unwrap();
            client
                .write_all(
                    format!("{method} http://exemplo.test/ HTTP/1.1\r\nHost: exemplo.test\r\nProxy-Authorization: Basic bXg6dGVzdGU=\r\n\r\n")
                        .as_bytes(),
                )
                .await
                .unwrap();
            let mut response = Vec::new();
            client.read_to_end(&mut response).await.unwrap();
            assert!(response.starts_with(b"HTTP/1.1 403"), "{}", String::from_utf8_lossy(&response));
            assert!(String::from_utf8_lossy(&response).contains("somente HTTPS na v1"));
        }
        assert_eq!(connections.load(Ordering::SeqCst), 0, "HTTP simples tocou a origem");
        task.abort();
        server.abort();
    }

    #[tokio::test]
    async fn limita_conexoes_simultaneas() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let fake = Arc::new(Fake {
            answers: vec![vec!["8.8.8.8:443".parse().unwrap()]],
            calls: AtomicUsize::new(0),
            target: addr,
        });
        let task = tokio::spawn(serve(listener, fake, Arc::from("teste"), Arc::new(HashSet::new())));
        let mut clients = Vec::new();
        for _ in 0..MAX_CONNECTIONS {
            let mut client = TcpStream::connect(addr).await.unwrap();
            client.write_all(b"CONNECT ").await.unwrap();
            clients.push(client);
        }
        // A fila do listener preserva a ordem de aceitação das conexões anteriores.
        let mut excess = TcpStream::connect(addr).await.unwrap();
        let mut byte = [0];
        let closed = timeout(Duration::from_secs(2), excess.read(&mut byte)).await.unwrap();
        assert!(matches!(closed, Ok(0) | Err(_)));
        task.abort();
        let _ = task.await;
        drop(clients);
    }

    #[tokio::test]
    async fn recusa_enderecos_internos_e_resposta_mista() {
        for ip in [
            "127.0.0.1:443",
            "10.0.0.1:443",    // vazamento-ok: IP privado para teste de recusa
            "169.254.1.1:443", // vazamento-ok: IP reservado para teste de recusa
            "[::1]:443",
            "[fe80::1]:443", // vazamento-ok: endereço reservado para teste de recusa
            "[::ffff:127.0.0.1]:443",
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let fake = Arc::new(Fake {
                answers: vec![vec!["8.8.8.8:443".parse().unwrap(), ip.parse().unwrap()]],
                calls: AtomicUsize::new(0),
                target: addr,
            });
            let task = tokio::spawn(serve(listener, fake, Arc::from("teste"), Arc::new(HashSet::new())));
            let (_, response) = request(addr, "CONNECT exemplo.test:443 HTTP/1.1").await;
            assert!(response.starts_with("HTTP/1.1 403"), "{ip}");
            task.abort();
            let _ = task.await;
        }
    }
}
