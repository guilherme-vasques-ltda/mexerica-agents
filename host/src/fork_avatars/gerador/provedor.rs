//! Fronteira do provedor de imagem. A implementação real chama `images/edits` da `OpenAI`
//! com a chave do cofre; testes injetam um provedor falso. Erros externos nunca são lidos,
//! retornados ou registrados — todo fracasso vira código genérico.
use anyhow::Result;
use base64::Engine as _;
use std::time::Duration;

const ENDPOINT: &str = "https://api.openai.com/v1/images/edits";
const MODELO: &str = "gpt-image-2.5-sunburst";
const MAX_RESPOSTA: usize = 24 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(240);

/// Uma edição de imagem: referência + prompt → PNG. O resultado é sempre validado.
/// Futuro boxed para ficar object-safe (jobs guardam `Arc<dyn Provedor>`).
pub trait Provedor: Send + Sync {
    fn editar<'a>(
        &'a self,
        referencia: &'a [u8],
        prompt: &'a str,
        size: &'a str,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Vec<u8>>> + Send + 'a>>;
}

pub struct OpenAi {
    key: zeroize::Zeroizing<String>,
    endpoint: String,
}
impl OpenAi {
    pub fn new(key: zeroize::Zeroizing<String>) -> Self {
        Self { key, endpoint: ENDPOINT.to_owned() }
    }

    /// Endpoint alternativo só em teste: produção fica fixo em `ENDPOINT`.
    #[cfg(test)]
    pub(crate) fn com_endpoint(key: zeroize::Zeroizing<String>, endpoint: String) -> Self {
        Self { key, endpoint }
    }
}

/// Corpo multipart como em `voice.rs`: campos fixos + `image[]` com a referência.
fn corpo(boundary: &str, prompt: &str, size: &str, imagem: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(imagem.len() + 2048);
    let mut campo = |name: &str, value: &str| {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes(),
        );
    };
    campo("model", MODELO);
    campo("prompt", prompt);
    campo("size", size);
    campo("background", "transparent");
    campo("output_format", "png");
    campo("n", "1");
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"image[]\"; filename=\"referencia.png\"\r\nContent-Type: image/png\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(imagem);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

impl Provedor for OpenAi {
    fn editar<'a>(
        &'a self,
        referencia: &'a [u8],
        prompt: &'a str,
        size: &'a str,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Vec<u8>>> + Send + 'a>> {
        Box::pin(self.editar_impl(referencia, prompt, size))
    }
}

impl OpenAi {
    async fn editar_impl(&self, referencia: &[u8], prompt: &str, size: &str) -> Result<Vec<u8>> {
        let boundary = format!("donnabot-{}", uuid::Uuid::new_v4().simple());
        let resposta = crate::http_fornecedor()
            .post(&self.endpoint)
            .bearer_auth(&*self.key)
            .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
            .body(corpo(&boundary, prompt, size, referencia))
            .timeout(TIMEOUT)
            .send()
            .await
            .map_err(|_| super::falha("GERACAO_INDISPONIVEL"))?;
        if !resposta.status().is_success() {
            return Err(super::falha("GERACAO_INDISPONIVEL"));
        }
        let mut bytes = Vec::new();
        let mut stream = resposta.bytes_stream();
        {
            use futures::StreamExt;
            while let Some(parte) = stream.next().await {
                let parte = parte.map_err(|_| super::falha("GERACAO_INDISPONIVEL"))?;
                if bytes.len() + parte.len() > MAX_RESPOSTA {
                    return Err(super::falha("GERACAO_INDISPONIVEL"));
                }
                bytes.extend_from_slice(&parte);
            }
        }
        let data: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| super::falha("IMAGEM_INVALIDA"))?;
        let b64 = data["data"][0]["b64_json"].as_str().filter(|s| s.len() <= MAX_RESPOSTA);
        let png = b64
            .and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok())
            .filter(|png| png.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]));
        png.ok_or_else(|| super::falha("IMAGEM_INVALIDA"))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    //! 3xx do provedor de imagem é recusado: o cliente não segue, a chamada falha com o
    //! erro genérico do módulo e nenhum `Authorization` chega ao destino do `Location`.
    use super::*;
    use axum::extract::Request;
    use axum::http::{HeaderMap, StatusCode, header};
    use axum::routing::any;
    use std::sync::{Arc, Mutex};

    struct Spy {
        credentials: Mutex<Vec<String>>,
        hits: Mutex<usize>,
    }

    async fn servidor(corpo: String, spy: Arc<Spy>) -> String {
        let app = axum::Router::new().fallback(any(move |req: Request| {
            let s = spy.clone();
            async move {
                let headers: &HeaderMap = req.headers();
                if let Some(v) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
                    s.credentials.lock().unwrap().push(v.to_owned());
                }
                *s.hits.lock().unwrap() += 1;
                (StatusCode::OK, [(header::CONTENT_TYPE, "application/json")], corpo.clone())
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        base
    }

    async fn servidor_redirect(code: StatusCode, target: &str) -> String {
        let location = format!("{target}/v1/images/edits");
        let app = axum::Router::new().fallback(any(move || {
            let location = location.clone();
            async move { (code, [(header::LOCATION, location)]) }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        base
    }

    #[tokio::test]
    async fn edicao_recusa_todo_redirect_sem_vazar_credencial() {
        let spy = Arc::new(Spy { credentials: Mutex::new(vec![]), hits: Mutex::new(0) });
        let target = servidor("{}".to_owned(), spy.clone()).await;
        for code in [
            StatusCode::MOVED_PERMANENTLY,
            StatusCode::FOUND,
            StatusCode::TEMPORARY_REDIRECT,
            StatusCode::PERMANENT_REDIRECT,
        ] {
            let base = servidor_redirect(code, &target).await;
            let provedor = OpenAi::com_endpoint(zeroize::Zeroizing::new("sk-fixture".to_owned()), base);
            let err = provedor.editar(b"png", "prompt", "1024x1024").await.unwrap_err();
            assert_eq!(err.to_string(), "GERACAO_INDISPONIVEL", "{code}: erro inesperado");
        }
        assert_eq!(*spy.hits.lock().unwrap(), 0, "o destino do redirect nunca foi alcançado");
        assert!(spy.credentials.lock().unwrap().is_empty(), "nenhum bearer pode vazar");
    }

    #[tokio::test]
    async fn edicao_aceita_um_2xx_normal() {
        let spy = Arc::new(Spy { credentials: Mutex::new(vec![]), hits: Mutex::new(0) });
        let png = base64::engine::general_purpose::STANDARD.encode([137, 80, 78, 71, 13, 10, 26, 10, 1]);
        let corpo = format!(r#"{{"data":[{{"b64_json":"{png}"}}]}}"#);
        let base = servidor(corpo, spy.clone()).await;
        let provedor = OpenAi::com_endpoint(zeroize::Zeroizing::new("sk-fixture".to_owned()), base);
        let out = provedor.editar(b"png", "prompt", "1024x1024").await.unwrap();
        assert!(out.starts_with(&[137, 80, 78, 71]));
        assert_eq!(spy.credentials.lock().unwrap().as_slice(), ["Bearer sk-fixture"]);
    }
}
