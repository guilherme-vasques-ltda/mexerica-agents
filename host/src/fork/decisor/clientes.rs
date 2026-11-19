//! Clientes Decision atrás de [`DecisionProvider`]. Contratos conferidos nas docs oficiais em
//! 13/11/2026: `OpenRouter` (`POST https://openrouter.ai/api/alpha/decisions`, `Bearer`, campo `state`;
//! lista de modelos em `GET /api/v1/models?output_modalities=decisions`) e Google
//! (`POST /v1beta/decisions`, `x-goog-api-key`, campo `input`). A `TypeSafe` direta não tem API
//! oficial verificável e fica de fora.
use super::{Alvo, Chave, DecisionProvider, ErroDecisao, Pedido, ProvedorDecisor, Saida};
use futures::future::BoxFuture;
use reqwest::{StatusCode, header::HeaderValue};
use serde_json::{Map, Value, json};
use std::time::Duration;

const PERGUNTA: &str = "escolha";
const TEMPO: Duration = Duration::from_secs(10);
const LIMITE_RESPOSTA: usize = 1 << 20;

pub(crate) const URL_OPENROUTER: &str = "https://openrouter.ai";
pub(crate) const URL_GOOGLE: &str = "https://generativelanguage.googleapis.com";

/// Cliente HTTP do decisor: sem redirecionamento, para a chave nunca ir a outro endereço.
pub(crate) fn cliente_http() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build()
}

/// Fábrica padrão usada por `sugestao::decidir`.
pub(crate) fn construir(http: &reqwest::Client, alvo: &Alvo) -> Box<dyn DecisionProvider> {
    let (http, modelo) = (http.clone(), alvo.modelo.clone());
    match alvo.provedor {
        ProvedorDecisor::OpenRouter => Box::new(OpenRouter { http, base: URL_OPENROUTER.into(), modelo }),
        ProvedorDecisor::Google => Box::new(Google { http, base: URL_GOOGLE.into(), modelo }),
    }
}

pub(crate) struct OpenRouter {
    pub http: reqwest::Client,
    pub base: String,
    pub modelo: String,
}
pub(crate) struct Google {
    pub http: reqwest::Client,
    pub base: String,
    pub modelo: String,
}

fn corpo(modelo: &str, campo_estado: &str, pedido: &Pedido<'_>) -> Value {
    let criterios: Map<String, Value> =
        pedido.opcoes.iter().map(|o| (o.id.clone(), Value::String(o.descricao.clone()))).collect();
    json!({
        "model": modelo,
        campo_estado: pedido.estado,
        "questions": { PERGUNTA: { "type": "choice", "instructions": pedido.pergunta, "criteria": criterios } },
    })
}

impl DecisionProvider for OpenRouter {
    fn decidir<'a>(&'a self, chave: &'a Chave, pedido: &'a Pedido<'a>) -> BoxFuture<'a, Result<Saida, ErroDecisao>> {
        Box::pin(async move {
            let url = format!("{}/api/alpha/decisions", self.base);
            let corpo = corpo(&self.modelo, "state", pedido);
            enviar(&self.http, &url, "authorization", &format!("Bearer {}", chave.expor()), &corpo).await
        })
    }
}

impl DecisionProvider for Google {
    fn decidir<'a>(&'a self, chave: &'a Chave, pedido: &'a Pedido<'a>) -> BoxFuture<'a, Result<Saida, ErroDecisao>> {
        Box::pin(async move {
            let url = format!("{}/v1beta/decisions", self.base);
            enviar(&self.http, &url, "x-goog-api-key", chave.expor(), &corpo(&self.modelo, "input", pedido)).await
        })
    }
}

async fn enviar(
    http: &reqwest::Client,
    url: &str,
    cabecalho: &'static str,
    valor: &str,
    corpo: &Value,
) -> Result<Saida, ErroDecisao> {
    let mut segredo = HeaderValue::from_str(valor).map_err(|_| ErroDecisao::ChaveInvalida)?;
    segredo.set_sensitive(true);
    let resposta = http
        .post(url)
        .header(cabecalho, segredo)
        .json(corpo)
        .timeout(TEMPO)
        .send()
        .await
        .map_err(|_| ErroDecisao::Indisponivel)?;
    let status = resposta.status();
    if !status.is_success() {
        return Err(erro_do_status(status));
    }
    let bytes = resposta.bytes().await.map_err(|_| ErroDecisao::Indisponivel)?;
    if bytes.len() > LIMITE_RESPOSTA {
        return Err(ErroDecisao::RespostaInvalida);
    }
    analisar(&serde_json::from_slice(&bytes).map_err(|_| ErroDecisao::RespostaInvalida)?)
}

fn erro_do_status(status: StatusCode) -> ErroDecisao {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::PAYMENT_REQUIRED => ErroDecisao::Autenticacao,
        StatusCode::TOO_MANY_REQUESTS => ErroDecisao::LimiteTaxa,
        s if s.is_client_error() => ErroDecisao::Recusado,
        _ => ErroDecisao::Indisponivel,
    }
}

/// `answers.<pergunta>` é `{type:"choice", choice, ...}` ou `{type:"refusal"}` (abstenção).
/// Só o rótulo escolhido sai daqui; qualquer outro texto do provedor é descartado.
pub(crate) fn analisar(corpo: &Value) -> Result<Saida, ErroDecisao> {
    let resposta = &corpo["answers"][PERGUNTA];
    match resposta["type"].as_str() {
        Some("refusal") => Ok(Saida::Abstencao),
        Some("choice") => {
            resposta["choice"].as_str().map(|c| Saida::Escolha(c.to_owned())).ok_or(ErroDecisao::RespostaInvalida)
        }
        _ => Err(ErroDecisao::RespostaInvalida),
    }
}

/// Filtra a resposta de `GET /api/v1/models?output_modalities=decisions` para ids de modelos de
/// decisão. Defesa extra: só aceita entrada cujo `architecture.output_modalities` contenha `decisions`.
pub(crate) fn filtrar_modelos_decisao(lista: &Value) -> Vec<String> {
    let Some(dados) = lista["data"].as_array() else { return Vec::new() };
    let mut ids: Vec<String> = dados
        .iter()
        .filter(|m| {
            m["architecture"]["output_modalities"]
                .as_array()
                .is_some_and(|mods| mods.iter().any(|x| x.as_str() == Some("decisions")))
        })
        .filter_map(|m| m["id"].as_str())
        .filter(|id| super::config::modelo_valido(id))
        .map(str::to_owned)
        .collect();
    ids.sort();
    ids
}

/// Lista pública (sem chave) dos modelos de decisão do `OpenRouter`, para a escolha do usuário.
pub(crate) async fn listar_modelos(http: &reqwest::Client, base: &str) -> Result<Vec<String>, ErroDecisao> {
    let resposta = http
        .get(format!("{base}/api/v1/models?output_modalities=decisions"))
        .timeout(TEMPO)
        .send()
        .await
        .map_err(|_| ErroDecisao::Indisponivel)?;
    if !resposta.status().is_success() {
        return Err(erro_do_status(resposta.status()));
    }
    let bytes = resposta.bytes().await.map_err(|_| ErroDecisao::Indisponivel)?;
    if bytes.len() > 8 * LIMITE_RESPOSTA {
        return Err(ErroDecisao::RespostaInvalida);
    }
    Ok(filtrar_modelos_decisao(&serde_json::from_slice(&bytes).map_err(|_| ErroDecisao::RespostaInvalida)?))
}
