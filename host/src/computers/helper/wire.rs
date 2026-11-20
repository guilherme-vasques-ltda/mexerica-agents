//! Cliente do canal E2E direto do Hub (o mesmo que celulares e o executor de teste usam): handshake autenticado
//! pela chave do Hub fixada no cadastro, quadros cifrados e chamadas com id. Nenhum transporte novo.
use crate::remote::crypto::{self, Opener, Sealer};
use anyhow::{Context, Result, anyhow, bail, ensure};
use ed25519_dalek::SigningKey;
use futures::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};
use x25519_dalek::StaticSecret;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// O Hub respondeu com erro (recusa definitiva). Falha de rede ou de canal é outro tipo de erro: lá o resultado
/// pode ou não ter chegado, e o pedido continua pendente até o próximo hello.
#[derive(Debug)]
pub struct HubError(pub String);
impl std::fmt::Display for HubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for HubError {}
impl HubError {
    /// Recusa definitiva (código estável do Hub): reenviar nunca vai dar certo. Qualquer outro erro do Hub pode ser
    /// passageiro (falha ao gravar no banco, por exemplo) e o resultado guardado tem de ser tentado de novo.
    pub fn terminal(&self) -> bool {
        const CODES: [&str; 7] = [
            "invalid_arguments",
            "job_cancelled",
            "job_expired",
            "lease_lost",
            "unknown_job",
            "too_large",
            "not_allowed_here",
        ];
        CODES.iter().any(|c| self.0.starts_with(&format!("{c}:")))
    }
}

pub struct Session {
    ws: Socket,
    sealer: Sealer,
    opener: Opener,
    next_id: u64,
}

pub fn channel_url(base: &str) -> Result<String> {
    let rest = base.trim_end_matches('/');
    let url = if let Some(r) = rest.strip_prefix("https://") {
        format!("wss://{r}")
    } else if let Some(r) = rest.strip_prefix("http://") {
        format!("ws://{r}")
    } else {
        bail!("endereço do Hub inválido")
    };
    Ok(format!("{url}/channel?v=1"))
}

async fn next_text(ws: &mut Socket, wait: Duration) -> Result<Value> {
    let end = tokio::time::Instant::now() + wait;
    loop {
        let msg = tokio::time::timeout_at(end, ws.next()).await.context("o Hub não respondeu a tempo")?;
        match msg {
            Some(Ok(Message::Text(t))) => return Ok(serde_json::from_str(t.as_str()).unwrap_or(Value::Null)),
            Some(Ok(Message::Close(frame))) => bail!(
                "canal fechado pelo Hub: {}",
                frame.map_or_else(|| "sem motivo".into(), |f| format!("{} {}", u16::from(f.code), f.reason))
            ),
            Some(Ok(_)) => {}
            Some(Err(e)) => bail!("falha no canal: {e}"),
            None => bail!("canal encerrado"),
        }
    }
}

impl Session {
    /// Abre o canal e confere que quem responde é o Hub fixado: a assinatura do welcome tem de vir da chave dele.
    pub async fn connect(base: &str, key: &SigningKey, hub_sign_pub: &[u8; 32], pair: bool) -> Result<Self> {
        let (mut ws, _) =
            tokio::time::timeout(Duration::from_secs(20), tokio_tungstenite::connect_async(channel_url(base)?))
                .await
                .context("tempo esgotado ao conectar no Hub")?
                .context("não foi possível conectar no Hub")?;
        let secret = StaticSecret::from(crypto::random::<32>());
        let ek = crypto::x25519_pub(&secret);
        let n = crypto::random::<32>();
        let dk = key.verifying_key().to_bytes();
        let cid = crypto::computer_id_raw(hub_sign_pub);
        let hello = json!({"t":"hello","v":1,"dk":crypto::b64(&dk),"ek":crypto::b64(&ek),"n":crypto::b64(&n),
            "sig":crypto::b64(&crypto::sign(key,&crypto::hs1_input(&cid,&dk,&ek,&n))),"pair":pair});
        ws.send(Message::Text(hello.to_string().into())).await?;
        let welcome = next_text(&mut ws, Duration::from_secs(20)).await?;
        if welcome["t"] == "reject" {
            bail!("o Hub recusou a conexão: {}", welcome["message"].as_str().unwrap_or("sem motivo"));
        }
        ensure!(welcome["t"] == "welcome", "resposta inesperada do Hub");
        let eh = crypto::unb64_n::<32>(welcome["ek"].as_str().unwrap_or_default()).context("welcome inválido")?;
        let th = crypto::transcript_hash(&cid, &dk, &ek, &n, &eh);
        crypto::verify(
            hub_sign_pub,
            &th,
            &crypto::unb64_n::<64>(welcome["sig"].as_str().unwrap_or_default()).context("welcome inválido")?,
        )
        .context("este Hub não é o Hub cadastrado (assinatura diferente); conexão recusada")?;
        let keys = crypto::channel_keys(&crypto::x25519(&secret, &eh)?, &th);
        Ok(Self { ws, sealer: Sealer::new(keys.d2h), opener: Opener::new(keys.h2d, 16 << 20), next_id: 0 })
    }

    async fn send(&mut self, v: &Value) -> Result<()> {
        let frames =
            self.sealer.seal(v.to_string().as_bytes()).ok_or_else(|| anyhow!("canal esgotou a chave; reconecte"))?;
        for f in frames {
            self.ws.send(Message::Text(f.to_string().into())).await?;
        }
        Ok(())
    }

    async fn recv(&mut self, wait: Duration) -> Result<Value> {
        loop {
            let v = next_text(&mut self.ws, wait).await?;
            if v["t"] != "f" {
                continue;
            }
            if let Some(bytes) = self.opener.open(&v).map_err(|e| anyhow!("quadro inválido: {e:?}"))? {
                return Ok(serde_json::from_slice(&bytes)?);
            }
        }
    }

    /// Uma chamada; devolve o corpo ok ou o erro do Hub com a mensagem dele.
    pub async fn call(&mut self, method: &str, body: Value, wait: Duration) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({"id":id,"m":method,"b":body})).await?;
        // Um prazo só para a chamada inteira: quadros de outros ids não renovam o tempo.
        let end = tokio::time::Instant::now() + wait;
        loop {
            let v = self.recv(end.saturating_duration_since(tokio::time::Instant::now())).await?;
            if v["id"] != id {
                continue;
            }
            if let Some(err) = v.get("err") {
                return Err(HubError(err["message"].as_str().unwrap_or("erro do Hub").to_owned()).into());
            }
            return Ok(v["ok"].clone());
        }
    }

    /// Depois do cadastro o Hub fecha com 4100; qualquer outro fim é falha.
    pub async fn expect_paired_close(&mut self) -> Result<()> {
        let end = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            match tokio::time::timeout_at(end, self.ws.next()).await.context("o Hub não concluiu o cadastro")? {
                Some(Ok(Message::Close(Some(f)))) if u16::from(f.code) == 4100 => return Ok(()),
                Some(Ok(Message::Close(f))) => bail!(
                    "cadastro recusado: {}",
                    f.map_or_else(|| "sem motivo".into(), |f| format!("{} {}", u16::from(f.code), f.reason))
                ),
                Some(Ok(_)) => {}
                _ => bail!("o canal caiu antes de concluir o cadastro"),
            }
        }
    }

    pub async fn close(mut self) {
        let _ = self.ws.close(None).await;
    }
}
