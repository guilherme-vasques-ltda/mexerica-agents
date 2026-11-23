//! Endereço próprio do Motor Mexerica: um serviço qualquer nos formatos Chat Completions, Responses ou
//! Anthropic Messages, com endereço, chave e (se o serviço não lista modelos) o modelo digitado pela pessoa.
//!
//! Tudo fica num único registro cifrado do cofre, no slot do próprio endereço (`motor-mexerica/proprio-<id>`): a chave
//! e o destino juntos. Assim a geração da credencial muda quando muda o destino, e a conversa guardada não segue para
//! um destino que não a recebeu antes (veja `link.rs`). O índice dos endereços (só ids) fica fora do cofre. O portão de
//! rede de verdade é o do motor (`motor/mexerica/src/destino.ts`), na hora de testar e em cada conexão; aqui só se
//! confere a forma do endereço antes de guardar.
use super::credential_slot;
use crate::store::Store;
use anyhow::{Context, Result, anyhow, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zeroize::Zeroizing;

pub(super) const INDICE: &str = "motor-mexerica.proprios";
const MAX_ENDERECOS: usize = 20;
const MAX_URL: usize = 2048;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Formato {
    ChatCompletions,
    Responses,
    AnthropicMessages,
}

/// O registro cifrado. A chave nunca sai daqui para o app.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Registro {
    pub(super) key: String,
    pub(super) base_url: String,
    pub(super) formato: Formato,
    pub(super) local: bool,
    pub(super) nome: String,
}

impl Drop for Registro {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.key);
    }
}

pub(crate) fn eh_proprio(id: &str) -> bool {
    id.strip_prefix("proprio-").is_some_and(|resto| {
        (8..=32).contains(&resto.len()) && resto.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    })
}

pub(super) fn novo_id() -> String {
    format!("proprio-{}", &uuid::Uuid::new_v4().simple().to_string()[..12])
}

/// Índice ausente é lista vazia; índice ILEGÍVEL é erro: sem saber quais endereços existem, quem protege a
/// conversa não pode presumir que não há chaves a mascarar (a proteção falha fechada, não calada).
pub(super) fn ids(store: &Store) -> Result<Vec<String>> {
    Ok(store
        .kv_read(INDICE)?
        .map(|raw| serde_json::from_str::<Vec<String>>(&raw).context("índice de endereços ilegível"))
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .filter(|id| eh_proprio(id))
        .collect())
}

fn gravar_ids(store: &Store, ids: &[String]) -> Result<()> {
    store.kv_set(INDICE, &serde_json::to_string(ids)?)
}

pub(super) fn existe(store: &Store, id: &str) -> Result<bool> {
    Ok(eh_proprio(id) && ids(store)?.iter().any(|i| i == id))
}

pub(super) fn incluir(store: &Store, id: &str) -> Result<()> {
    let mut lista = ids(store)?;
    if !lista.iter().any(|i| i == id) {
        ensure!(lista.len() < MAX_ENDERECOS, "limite_enderecos: há endereços próprios demais.");
        lista.push(id.to_owned());
        gravar_ids(store, &lista)?;
    }
    Ok(())
}

pub(super) fn retirar(store: &Store, id: &str) -> Result<()> {
    let lista: Vec<String> = ids(store)?.into_iter().filter(|i| i != id).collect();
    gravar_ids(store, &lista)
}

/// A forma do endereço: HTTPS (HTTP só com a opção local), sem usuário ou senha, sem consulta nem fragmento, com host.
/// Sem a barra final. A resolução e a conexão de fato são conferidas no motor.
pub(super) fn conferir_url(texto: &str, local: bool) -> Result<String> {
    let texto = texto.trim();
    ensure!(!texto.is_empty() && texto.len() <= MAX_URL, "endereco_invalido: informe o endereço do serviço.");
    let url = reqwest::Url::parse(texto).map_err(|_| anyhow!("endereco_invalido: o endereço não é válido."))?;
    ensure!(
        url.scheme() == "https" || (local && url.scheme() == "http"),
        "endereco_invalido: use https (http só para endereço local)."
    );
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "endereco_invalido: tire usuário e senha do endereço."
    );
    ensure!(url.query().is_none() && url.fragment().is_none(), "endereco_invalido: o endereço não pode ter ? nem #.");
    ensure!(url.host_str().is_some_and(|h| !h.is_empty()), "endereco_invalido: o endereço não tem servidor.");
    ensure!(
        !url.host_str()
            .map(|h| h.trim_end_matches('.'))
            .is_some_and(|h| h == "api.meta.ai" || h.ends_with(".api.meta.ai")),
        "{}",
        crate::fork::assinatura_pessoal::META
    );
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

pub(super) fn conferir_nome(texto: &str) -> Result<String> {
    let nome = texto.trim();
    ensure!(
        !nome.is_empty() && nome.chars().count() <= 60 && !nome.chars().any(char::is_control),
        "nome_invalido: dê um nome curto ao endereço."
    );
    Ok(nome.to_owned())
}

/// Modelo digitado (serviço sem lista): o mesmo formato de id que o motor aceita.
pub(super) fn conferir_modelo(texto: &str) -> Result<String> {
    let modelo = texto.trim();
    let valido = (1..=200).contains(&modelo.len())
        && modelo.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && modelo.chars().all(|c| c.is_ascii_alphanumeric() || "._:~/-".contains(c))
        && !modelo.contains("..")
        && !modelo.contains("//");
    ensure!(valido, "modelo_invalido: o nome do modelo não é válido.");
    Ok(modelo.to_owned())
}

pub(super) fn ler(raw: &str) -> Result<Registro> {
    serde_json::from_str(raw).context("registro do endereço próprio ilegível")
}

pub(super) fn registro(store: &Store, id: &str) -> Result<Option<Registro>> {
    crate::market::vault::read(store, &credential_slot(id))?.map(|raw| ler(&Zeroizing::new(raw))).transpose()
}

pub(super) fn serializar(registro: &Registro) -> Result<Zeroizing<String>> {
    Ok(Zeroizing::new(serde_json::to_string(registro)?))
}

/// O segredo de um registro do cofre: a chave do endereço próprio, ou o próprio registro nos demais fornecedores.
pub(crate) fn segredo(provider: &str, raw: &str) -> Result<String> {
    if !eh_proprio(provider) {
        return Ok(raw.to_owned());
    }
    Ok(ler(raw)?.key.clone())
}

/// O que o processo do motor recebe do cofre para um endereço próprio: a chave e o destino.
pub(crate) fn credencial_do_motor(raw: &str) -> Result<Value> {
    let r = ler(raw)?;
    conferir_url(&r.base_url, r.local)?;
    ensure!(
        !crate::fork::assinatura_pessoal::known_meta_credential("", &r.key),
        "{}",
        crate::fork::assinatura_pessoal::META
    );
    Ok(
        json!({"type": "api_key", "key": r.key, "endpoint": {"baseUrl": r.base_url, "formato": r.formato, "local": r.local}}),
    )
}

/// O que o app vê de um endereço: tudo menos a chave.
pub(super) fn publico(id: &str, r: &Registro) -> Value {
    json!({"id": id, "nome": r.nome, "url": r.base_url, "formato": r.formato, "local": r.local})
}
