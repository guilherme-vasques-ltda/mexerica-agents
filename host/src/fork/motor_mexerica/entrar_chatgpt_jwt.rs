//! Validação do ID token de «Entrar com `ChatGPT`» (fail-closed, só RS256 contra o JWKS do emissor).
//!
//! Nada aqui presume: o cabeçalho precisa declarar RS256 (qualquer outro algoritmo, incluindo
//! «none», recusa), a assinatura é conferida contra a chave do JWKS indicada pelo `kid`, e as
//! afirmações seguem o contrato do emissor — emissor exato, público igual ao `client_id` emitido,
//! prazo de validade com folga de relógio de 60 s e o `nonce` da tentativa. O token e as suas
//! partes nunca vão a log nem a mensagem de erro.

use crate::remote::crypto;
use anyhow::{Result, anyhow, bail};
use serde_json::Value;
use std::time::Duration;

/// Emissor oficial: comparação exata, sem variante com barra no fim.
pub(super) const EMISSOR: &str = "https://auth.openai.com";
/// Prazo curto da busca do JWKS.
const PRAZO: Duration = Duration::from_secs(15);
/// Teto do corpo do JWKS.
const MAX_CORPO: usize = 64 * 1024;
/// Folga de relógio na validade do token.
pub(super) const FOLGA: i64 = 60;

/// O que a tentativa exige do ID token: o `client_id` emitido e o `nonce` gerado aqui.
pub(super) struct Esperado<'a> {
    pub(super) client_id: &'a str,
    pub(super) nonce: &'a str,
}

/// Identidade validada: o `sub` é a conta; o e-mail, se houver, é só rótulo.
pub(super) struct Identidade {
    pub(super) sub: String,
    pub(super) email: Option<String>,
}

/// Valida o ID token: assinatura RS256 contra o JWKS do emissor e afirmações do contrato. Nenhum
/// erro repete o token, a sua assinatura nem o corpo do JWKS.
pub(super) async fn validar(base: &str, token: &str, esperado: &Esperado<'_>) -> Result<Identidade> {
    let mut partes = token.split('.');
    let (cab, corpo, assinatura) = (partes.next(), partes.next(), partes.next());
    let (Some(cab), Some(corpo), Some(assinatura), None) = (cab, corpo, assinatura, partes.next()) else {
        bail!("id_invalido: o token não tem as três partes esperadas.");
    };
    let cabecalho: Value = decodificar(cab)?;
    let afirmacoes: Value = decodificar(corpo)?;
    ensure_alg_rs256(&cabecalho)?;
    let Some(kid) = cabecalho["kid"].as_str().filter(|kid| !kid.is_empty()) else {
        bail!("id_invalido: o token não declara a chave que o assinou.");
    };
    let chave = chave_do_jwks(base, kid).await?;
    // A assinatura cobre as partes codificadas, como foram: nunca remonta o JSON decodificado.
    let mensagem = format!("{cab}.{corpo}");
    let assinatura = crypto::unb64(assinatura).map_err(|_| anyhow!(RECUSA))?;
    chave
        .verify(&ring::signature::RSA_PKCS1_2048_8192_SHA256, mensagem.as_bytes(), &assinatura)
        .map_err(|_| anyhow!(RECUSA))?;
    conferir_afirmacoes(&afirmacoes, esperado)
}

/// Recusa fixa: nunca ecoa parte do token.
const RECUSA: &str = "id_invalido: o token não passou na conferência.";

/// Só RS256; qualquer outro algoritmo (incluindo «none») recusa sem tocar o JWKS.
fn ensure_alg_rs256(cabecalho: &Value) -> Result<()> {
    if cabecalho["alg"].as_str() != Some("RS256") {
        bail!("id_invalido: algoritmo de assinatura inesperado.");
    }
    Ok(())
}

/// Decodifica e lê uma parte do token como JSON.
fn decodificar(parte: &str) -> Result<Value> {
    let bytes = crypto::unb64(parte).map_err(|_| anyhow!(RECUSA))?;
    serde_json::from_slice(&bytes).map_err(|_| anyhow!(RECUSA))
}

/// Assinaturas, emissor, público, prazo e `nonce` das afirmações já decodificadas.
fn conferir_afirmacoes(afirmacoes: &Value, esperado: &Esperado<'_>) -> Result<Identidade> {
    if afirmacoes["iss"].as_str() != Some(EMISSOR) {
        bail!("{RECUSA} O emissor não é o oficial.");
    }
    let publico_ok = match &afirmacoes["aud"] {
        Value::String(aud) => aud == esperado.client_id,
        Value::Array(auds) => auds.len() == 1 && auds[0].as_str() == Some(esperado.client_id),
        _ => false,
    };
    if !publico_ok {
        bail!("{RECUSA} O público do token não é o client_id emitido.");
    }
    if afirmacoes.get("azp").is_some_and(|azp| azp.as_str() != Some(esperado.client_id)) {
        bail!("{RECUSA} O token foi emitido para outro cliente.");
    }
    let agora = crate::store::now_ms() / 1000;
    let Some(exp) = afirmacoes["exp"].as_i64() else { bail!("{RECUSA} O token não declara o prazo de validade.") };
    if exp < agora - FOLGA {
        bail!("{RECUSA} O token está vencido.");
    }
    let Some(nonce) = afirmacoes["nonce"].as_str() else {
        bail!("{RECUSA} O token não declara o nonce da tentativa.")
    };
    if !crypto::ct_eq(nonce.as_bytes(), esperado.nonce.as_bytes()) {
        bail!("{RECUSA} O nonce do token não é o da tentativa.");
    }
    let Some(sub) = afirmacoes["sub"].as_str().filter(|sub| !sub.is_empty()) else {
        bail!("{RECUSA} O token não declara a identidade da conta.");
    };
    let email = afirmacoes["email"].as_str().filter(|email| !email.is_empty());
    Ok(Identidade { sub: sub.to_owned(), email: email.map(str::to_owned) })
}

/// Chave RSA do JWKS pela declaração `kid`; busca por rede curta, sem redirecionamento e com teto.
async fn chave_do_jwks(base: &str, kid: &str) -> Result<ring::signature::RsaPublicKeyComponents<Vec<u8>>> {
    let destino = format!("{base}/.well-known/jwks.json");
    let cliente = crate::fork::conectores::oauth_urls::cliente()
        .map_err(|_| anyhow!("sem_resposta: o emissor não respondeu."))?;
    let mut resposta = cliente
        .get(&destino)
        .timeout(PRAZO)
        .send()
        .await
        .map_err(|_| anyhow!("sem_resposta: o emissor não respondeu."))?;
    if !resposta.status().is_success() {
        bail!("fornecedor_falhou: o emissor recusou a lista de chaves.");
    }
    let mut corpo = Vec::new();
    while let Some(parte) = resposta.chunk().await.map_err(|_| anyhow!("sem_resposta: o emissor não respondeu."))? {
        if corpo.len() + parte.len() > MAX_CORPO {
            bail!("resposta_invalida: a lista de chaves do emissor veio maior que o esperado.");
        }
        corpo.extend_from_slice(&parte);
    }
    let jwks: Value = serde_json::from_slice(&corpo)
        .map_err(|_| anyhow!("resposta_invalida: a lista de chaves do emissor não é legível."))?;
    let Some(chaves) = jwks["keys"].as_array() else {
        bail!("{RECUSA} A chave que assinou o token não está na lista do emissor.")
    };
    for chave in chaves {
        if chave["kid"].as_str() != Some(kid) || chave["kty"].as_str() != Some("RSA") {
            continue;
        }
        let (Some(n), Some(e)) = (chave["n"].as_str(), chave["e"].as_str()) else { continue };
        let (Ok(n), Ok(e)) = (crypto::unb64(n), crypto::unb64(e)) else { continue };
        return Ok(ring::signature::RsaPublicKeyComponents { n, e });
    }
    bail!("{RECUSA} A chave que assinou o token não está na lista do emissor.")
}
