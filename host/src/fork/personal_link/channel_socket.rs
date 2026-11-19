//! WebSocket binário sem remontagem/compressão. Limites antes de alocar payload.
use super::canon::LIMITE;
use anyhow::{Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(super) const HTTP_MAX: usize = 4096;
pub(super) async fn upgrade<S: AsyncRead + AsyncWrite + Unpin>(s: &mut S) -> Result<()> {
    let mut header = Vec::with_capacity(HTTP_MAX);
    while !header.ends_with(b"\r\n\r\n") {
        ensure!(header.len() < HTTP_MAX, "not_allowed: upgrade grande");
        header.push(s.read_u8().await?);
    }
    let text = std::str::from_utf8(&header)?;
    let mut lines = text.split("\r\n");
    ensure!(lines.next() == Some("GET /personal-link/channel HTTP/1.1"), "not_allowed: rota");
    let mut fields = std::collections::BTreeMap::new();
    for line in lines.filter(|l| !l.is_empty()) {
        let (name, value) = line.split_once(':').ok_or_else(|| anyhow::anyhow!("not_allowed: header"))?;
        let name = name.to_ascii_lowercase();
        ensure!(
            !name.is_empty()
                && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
                && fields.insert(name, value.trim()).is_none(),
            "not_allowed: header duplicado"
        );
    }
    let field = |name: &str| fields.get(name).copied().unwrap_or_default();
    ensure!(
        field("upgrade").eq_ignore_ascii_case("websocket")
            && field("connection").eq_ignore_ascii_case("upgrade")
            && field("sec-websocket-version") == "13"
            && !field("host").is_empty(),
        "not_allowed: upgrade"
    );
    ensure!(
        !fields.contains_key("sec-websocket-extensions")
            && !fields.contains_key("sec-websocket-protocol")
            && !fields.contains_key("transfer-encoding")
            && !fields.contains_key("content-length"),
        "not_allowed: modo"
    );
    let key = field("sec-websocket-key");
    let decoded = STANDARD.decode(key)?;
    ensure!(decoded.len() == 16 && STANDARD.encode(&decoded) == key, "not_allowed: chave upgrade");
    let accept = tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes());
    s.write_all(format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").as_bytes()).await?;
    Ok(())
}

/// Cliente obrigatoriamente mascarado; FIN=1, RSV=0, opcode binário apenas.
/// Close/ping/pong não concedem prazo: este corte encerra também nesses frames.
pub(super) async fn read<R: AsyncRead + Unpin>(r: &mut R) -> Result<Vec<u8>> {
    ensure!(r.read_u8().await? == 0x82, "not_allowed: tipo/fragmentação/compressão");
    let second = r.read_u8().await?;
    ensure!(second & 0x80 != 0, "not_allowed: máscara");
    let small = second & 0x7f;
    let len = match small {
        126 => {
            let n = usize::from(r.read_u16().await?);
            ensure!(n >= 126, "not_allowed: comprimento");
            n
        }
        127 => anyhow::bail!("not_allowed: tamanho"),
        n => usize::from(n),
    };
    // Limite inclui cabeçalho WebSocket e aplicação.
    ensure!(len > 0 && len + if small == 126 { 8 } else { 6 } <= LIMITE, "not_allowed: tamanho");
    let mut mask = [0; 4];
    r.read_exact(&mut mask).await?;
    let mut bytes = vec![0; len];
    r.read_exact(&mut bytes).await?;
    for (i, b) in bytes.iter_mut().enumerate() {
        *b ^= mask[i % 4];
    }
    Ok(bytes)
}
pub(super) async fn write<W: AsyncWrite + Unpin>(w: &mut W, bytes: &[u8]) -> Result<()> {
    ensure!(bytes.len() + 4 <= LIMITE, "not_allowed: tamanho saída");
    w.write_u8(0x82).await?;
    if bytes.len() < 126 {
        w.write_u8(u8::try_from(bytes.len())?).await?;
    } else {
        w.write_u8(126).await?;
        w.write_u16(u16::try_from(bytes.len())?).await?;
    }
    w.write_all(bytes).await?;
    w.flush().await?;
    Ok(())
}
