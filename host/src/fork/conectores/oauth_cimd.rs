use super::{oauth_desktop, oauth_urls};
use crate::market::oauth::{OAuth, loopback_uri};
use anyhow::{Result, anyhow, ensure};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

pub(crate) const AUSENTE: &str = "O catálogo não configurou a URL do documento CIMD; login recusado.";
pub(crate) const URL_INVALIDA: &str = "A URL do documento CIMD no catálogo é inválida; login recusado.";
pub(crate) const SEM_SUPORTE: &str = "O servidor de login não anuncia suporte a CIMD; login recusado.";
pub(crate) const SO_LOOPBACK: &str = "O login com CIMD exige retorno neste computador.";
const DOCUMENTO: &str = "Não foi possível obter ou validar o documento CIMD; login recusado.";
const LIMITE: usize = 16 * 1024;
const LIMITE_NOME: usize = 128;

pub(crate) fn url(configurada: Option<&str>) -> Result<&str> {
    let raw = configurada.ok_or_else(|| anyhow!(AUSENTE))?;
    let (autoridade, caminho) = raw
        .strip_prefix("https://")
        .and_then(|s| s.split(['?', '#']).next())
        .and_then(|s| s.split_once('/'))
        .ok_or_else(|| anyhow!(URL_INVALIDA))?;
    let url = oauth_urls::conector(raw).map_err(|_| anyhow!(URL_INVALIDA))?;
    ensure!(
        !autoridade.is_empty()
            && !autoridade.contains('@')
            && !raw.contains('\\')
            && !raw.chars().any(|c| c.is_control() || c.is_whitespace())
            && !caminho.is_empty()
            && url.path().strip_prefix('/') == Some(caminho),
        URL_INVALIDA
    );
    Ok(raw)
}

pub(crate) fn anunciar(meta: &Value) -> Result<()> {
    ensure!(meta.get("client_id_metadata_document_supported").and_then(Value::as_bool) == Some(true), SEM_SUPORTE);
    Ok(())
}

pub(crate) fn conferir_endpoint(url: &reqwest::Url) -> Result<()> {
    ensure!(
        !url.query_pairs().any(|(chave, _)| matches!(
            chave.as_ref(),
            "client_id"
                | "client_secret"
                | "client_assertion"
                | "client_assertion_type"
                | "redirect_uri"
                | "response_type"
                | "state"
                | "code_challenge"
                | "code_challenge_method"
                | "grant_type"
                | "code"
                | "code_verifier"
                | "refresh_token"
        )),
        "O endpoint de login CIMD contém parâmetros OAuth reservados; login recusado."
    );
    Ok(())
}

pub(crate) fn redirects(porta_hub: u16) -> Vec<String> {
    std::iter::once(porta_hub).chain(oauth_desktop::PORTAS).map(loopback_uri).collect()
}

#[derive(Deserialize)]
struct Documento {
    client_id: String,
    client_name: String,
    redirect_uris: Vec<String>,
    token_endpoint_auth_method: String,
    #[serde(default, rename = "client_secret", deserialize_with = "recusar_segredo")]
    _client_secret: (),
    #[serde(default, rename = "client_secret_expires_at", deserialize_with = "recusar_segredo")]
    _client_secret_expires_at: (),
}

fn recusar_segredo<'de, D: serde::Deserializer<'de>>(_: D) -> Result<(), D::Error> {
    Err(serde::de::Error::custom(DOCUMENTO))
}

pub(crate) async fn cliente(
    url: &str,
    escolhido: &str,
    permitidos: &[String],
    #[cfg(test)] destino_de_teste: Option<&str>,
) -> Result<OAuth> {
    #[cfg(test)]
    let destino = destino_de_teste.unwrap_or(url);
    #[cfg(not(test))]
    let destino = url;
    let mut resposta = oauth_urls::cliente()?
        .get(destino)
        .header("accept", "application/json")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|_| anyhow!(DOCUMENTO))?;
    ensure!(resposta.status().is_success() && resposta.content_length().is_none_or(|n| n <= LIMITE as u64), DOCUMENTO);
    let mut corpo = Vec::new();
    while let Some(chunk) = resposta.chunk().await.map_err(|_| anyhow!(DOCUMENTO))? {
        ensure!(chunk.len() <= LIMITE - corpo.len(), DOCUMENTO);
        corpo.extend_from_slice(&chunk);
    }
    let documento: Documento = serde_json::from_slice(&corpo).map_err(|_| anyhow!(DOCUMENTO))?;
    ensure!(
        documento.client_id == url
            && !documento.client_name.trim().is_empty()
            && documento.client_name.chars().count() <= LIMITE_NOME
            && documento.token_endpoint_auth_method == "none"
            && documento.redirect_uris.iter().any(|r| r == escolhido)
            && documento.redirect_uris.iter().all(|r| permitidos.contains(r)),
        DOCUMENTO
    );
    Ok(OAuth { client_id: url.into(), redirect_uris: documento.redirect_uris, ..OAuth::default() })
}
