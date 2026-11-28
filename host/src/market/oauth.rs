// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Sign-in for remote MCP connectors (MCP authorization: OAuth 2.1 + PKCE,
//! protected resource metadata, dynamic client registration).
//!
//! The tokens live on this computer. The sign-in page redirects to one of two places:
//! - `http://127.0.0.1:<port>/oauth/callback`, served by this host, when the user signs
//!   in on this computer (desktop app, TUI);
//! - `<cloud>/v1/oauth/callback`, a stateless page that bounces to `codync://oauth?…`,
//!   which the phone's sign-in sheet catches and hands back with `connectorSignInFinish`;
//! - `http://127.0.0.1:<app port>/oauth/callback`, served by the desktop app itself when the Hub runs on another
//!   computer (SSH tunnel on a random local port): the app hands the return back with `connectorSignInFinish`
//!   (`fork/conectores/oauth_desktop.rs`).
//!
//! The authorization code alone is useless: the PKCE verifier never leaves this host.

// MODIFICADO pela DONNA Bot em 2026-11-04: alvo esperado e Context/Setup originais do proxy, adaptação autoral Donna.
// Em 2026-11-05 (M2-A): cliente sem redirects e URLs por papel via `oauth_urls`, ganchos curtos.
// Em 2026-11-05 (M2-B): emissor original sem aparar barra e `issuer` exato nos metadados.
// Em 2026-11-05 (M3-A): metadados aceitos declaram PKCE S256 antes de DCR/Grant/link.
// Em 2026-11-05 (M3-B): emissor e política de `iss` da tentativa conferidos no retorno.
// Em 2026-11-06 (M3-C): PRM validado por candidato e resource preso à tentativa/concessão.
use crate::LockExt;
use crate::api::devices::Caller;
use crate::fork::conectores::catalogo_servicos::{AppProprioConfig, Auth, RegistroOauth};
use crate::fork::conectores::oauth_binding::{self as binding, Origin, StartIntent};
use crate::fork::conectores::oauth_iss::Esperado;
use crate::fork::conectores::oauth_refresh::{self, fresh};
use crate::fork::conectores::oauth_runtime::RuntimeIntent;
use crate::fork::conectores::oauth_urls as urls;
use crate::fork::conectores::{oauth_cimd, oauth_registration, oauth_resource, oauth_scopes, oauth_tentativa};
use crate::hub::Hub;
use crate::market::{self, Connector};
use crate::store::Store;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TIMEOUT: Duration = Duration::from_secs(20);
/// How long a started sign-in waits for its callback.
const PENDING_FOR: Duration = Duration::from_secs(15 * 60);

/// A connector's OAuth client and tokens. Present (maybe without tokens) when the server asks for sign-in.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OAuth {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub(crate) client_secret_basic: bool,
    /// The redirect URIs `client_id` was registered with.
    #[serde(default)]
    pub redirect_uris: Vec<String>,
    #[serde(default)]
    pub token_endpoint: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Unix seconds.
    #[serde(default)]
    pub expires_at: Option<u64>,
}

/// O `Debug` nunca imprime a chave do app nem os tokens: um `{:?}` esquecido num log não vaza nada.
impl std::fmt::Debug for OAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let oculto = |preenchido: bool| if preenchido { "<oculto>" } else { "<vazio>" };
        f.debug_struct("OAuth")
            .field("client_id", &oculto(!self.client_id.is_empty()))
            .field("client_secret", &oculto(self.client_secret.is_some()))
            .field("client_secret_basic", &self.client_secret_basic)
            .field("redirect_uris", &self.redirect_uris)
            .field("token_endpoint", &self.token_endpoint)
            .field("access_token", &oculto(!self.access_token.is_empty()))
            .field("refresh_token", &oculto(self.refresh_token.is_some()))
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl OAuth {
    pub fn signed_in(&self) -> bool {
        !self.access_token.is_empty()
    }
}

/// Recusa estável: o login do Google (app próprio) só vale neste computador, pelo retorno do app ou do Hub.
pub(crate) const APP_PROPRIO_SO_LOOPBACK: &str =
    "google_so_neste_computador: o login do Google só pode ser feito neste computador, pelo app da Mexerica.";

/// Where the sign-in page sends the user back to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Callback {
    /// This host's loopback page (the browser runs on this computer).
    Host,
    /// The cloud's bounce page, caught by the phone app.
    App,
    /// The desktop app's own loopback listener on this fixed port (`oauth_desktop`), for a Hub on another computer:
    /// the browser returns to the app, which hands the code back over its authenticated channel.
    Desktop(u16),
}

struct Pending {
    snapshot: Connector,
    origin: Origin,
    callback: Callback,
    verifier: String,
    redirect_uri: String,
    resource: String,
    /// `false` no app próprio do Google: o servidor de login dele não usa o parâmetro `resource` (RFC 8707).
    enviar_resource: bool,
    iss: Esperado,
    tentativa: Option<String>,
    started: Instant,
}

impl Pending {
    fn viva(&self) -> Result<()> {
        anyhow::ensure!(self.started.elapsed() < PENDING_FOR, "Este login expirou. Inicie o login de novo.");
        oauth_tentativa::viva(self.tentativa.as_deref())
    }
}

/// Cancelar vale por tentativa: a marca é gravada antes de tudo, e o que já estava registrado sai junto.
pub(crate) fn cancelar_tentativa(tentativa: &str) {
    oauth_tentativa::cancelar(tentativa);
    if let Some(map) = PENDING.locked().as_mut() {
        map.retain(|_, p| p.tentativa.as_deref() != Some(tentativa));
    }
}

static PENDING: Mutex<Option<HashMap<String, Pending>>> = Mutex::new(None);

pub(crate) fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

pub(crate) fn loopback_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}/oauth/callback")
}

fn app_uri(store: &Store) -> Option<String> {
    crate::remote::cloud::url(store).map(|base| {
        let suffix = if crate::environment::Environment::current() == crate::environment::Environment::Dev {
            "/dev"
        } else {
            ""
        };
        format!("{base}/v1/oauth/callback{suffix}")
    })
}

async fn get_json(url: &str) -> Result<Value> {
    let res = urls::cliente()?.get(url).header("accept", "application/json").timeout(TIMEOUT).send().await;
    // 3xx não é sucesso: a descoberta segue no próximo well-known, sem seguir `Location`.
    let res = res.ok().filter(|r| r.status().is_success()).ok_or_else(|| anyhow!(urls::METADADOS))?;
    res.json().await.map_err(|_| anyhow!(urls::METADADOS))
}

/// `true` when the server wants sign-in: an unauthenticated `initialize` gets 401, or the server
/// lets `initialize` through, refuses only tool calls, and says so in its resource metadata.
pub async fn required(url: &str, headers: &std::collections::BTreeMap<String, String>) -> bool {
    let Ok(client) = urls::cliente() else { return false };
    let mut req = client
        .post(url)
        .header("accept", "application/json, text/event-stream")
        .json(&json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": crate::mcp::PROTOCOL_VERSION, "capabilities": {},
            "clientInfo": {"name": "codync", "version": env!("CARGO_PKG_VERSION")},
        }}))
        .timeout(TIMEOUT);
    // Headers MCP reservados pertencem à sessão, nunca à configuração.
    let configured = || headers.iter().filter(|(k, _)| !crate::fork::conectores::mcp_version::reservado(k));
    for (k, v) in configured() {
        req = req.header(k, v);
    }
    let Ok(res) = req.send().await else { return false };
    if res.status() == reqwest::StatusCode::UNAUTHORIZED {
        return true;
    }
    if res.status().is_success() {
        // Headers the user gave (an API key) are their way in; don't ask for sign-in on top.
        return headers.is_empty()
            && resource_metadata(url)
                .await
                .is_some_and(|v| v.documento["authorization_servers"].as_array().is_some_and(|a| !a.is_empty()));
    }
    // Só 404/405 indicam o transporte anterior; 400 não abre fallback.
    if !matches!(res.status().as_u16(), 404 | 405) {
        return false;
    }
    // Not streamable HTTP: ask the older SSE endpoint instead (the stream is dropped unread).
    let mut req = client.get(url).header("accept", "text/event-stream").timeout(TIMEOUT);
    for (k, v) in configured() {
        req = req.header(k, v);
    }
    req.send().await.is_ok_and(|r| r.status() == reqwest::StatusCode::UNAUTHORIZED)
}

/// `resource_metadata="…"` from a `WWW-Authenticate: Bearer …` header.
fn resource_metadata_hint(header: &str) -> Option<String> {
    let (_, rest) = header.split_once("resource_metadata=")?;
    let rest = rest.trim_start_matches('"');
    let end = rest.find(['"', ',', ' ']).unwrap_or(rest.len());
    Some(rest[..end].to_owned()).filter(|s| s.starts_with("https://") || s.starts_with("http://"))
}

/// `https://a.b/x/y` → (`https://a.b`, `/x/y`), without a trailing `/`.
fn split(url: &str) -> Result<(String, String)> {
    let u = reqwest::Url::parse(url)?;
    let path = u.path().trim_end_matches('/').to_owned();
    Ok((u.origin().ascii_serialization(), path))
}

/// The protected resource metadata (RFC 9728): which authorization server, which scopes.
async fn resource_metadata(url: &str) -> Option<oauth_resource::Validado> {
    let (Ok(conector), Ok(client)) = (urls::conector(url), urls::cliente()) else { return None };
    let hint = client
        .post(url)
        .header("accept", "application/json, text/event-stream")
        .json(&json!({"jsonrpc": "2.0", "id": 0, "method": "ping"}))
        .timeout(TIMEOUT)
        .send()
        .await
        .ok()
        .and_then(|r| r.headers().get("www-authenticate")?.to_str().ok().and_then(resource_metadata_hint))
        .filter(|hint| urls::derivada(&conector, hint).is_ok());
    for candidato in oauth_resource::candidatos(url, hint) {
        if let Ok(v) = get_json(&candidato.url).await
            && let Some(validado) = candidato.conferir(v)
        {
            return Some(validado);
        }
    }
    None
}

/// The authorization server metadata (RFC 8414, then `OpenID` discovery).
/// M2-B: a barra final sai só do endereço de consulta (RFC 8414 §3.1); cada
/// resposta precisa declarar `issuer` idêntico ao emissor escolhido (§3.3).
async fn server_metadata(issuer: &str) -> Result<Value> {
    let (origin, path) = split(issuer)?;
    let mut outro_emissor = false;
    for t in [
        format!("{origin}/.well-known/oauth-authorization-server{path}"),
        format!("{origin}/.well-known/openid-configuration{path}"),
        format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/')),
    ] {
        if let Ok(v) = get_json(&t).await
            && v["authorization_endpoint"].is_string()
        {
            if urls::mesmo_emissor(issuer, &v) {
                return Ok(v);
            }
            outro_emissor = true;
        }
    }
    bail!(if outro_emissor { urls::EMISSOR } else { urls::METADADOS })
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

pub(crate) fn invalidate(original: &Connector) {
    if let Some(map) = PENDING.locked().as_mut() {
        map.retain(|_, p| p.snapshot.oauth_binding != original.oauth_binding);
    }
}

/// App próprio: o servidor de login é o da ficha. O recurso pode anunciá-lo com uma barra final a mais; qualquer outro
/// emissor (ou nenhum) recusa o login, sem eco do que foi anunciado.
fn emissor_fixado(documento: &Value, emissor: &str) -> Result<String> {
    let anunciado = documento["authorization_servers"][0].as_str().unwrap_or_default();
    anyhow::ensure!(!emissor.is_empty() && anunciado.strip_suffix('/').unwrap_or(anunciado) == emissor, urls::EMISSOR);
    Ok(emissor.to_owned())
}

fn emissor_da_ficha(documento: &Value, emissor: &str) -> Result<String> {
    anyhow::ensure!(
        !emissor.is_empty()
            && documento["authorization_servers"]
                .as_array()
                .is_some_and(|lista| lista.iter().any(|s| s.as_str() == Some(emissor))),
        urls::EMISSOR
    );
    Ok(emissor.to_owned())
}

fn cliente_da_ficha(
    config: AppProprioConfig,
    resource: &str,
    meta: &Value,
    authorize: &str,
    token: &str,
) -> Result<bool> {
    anyhow::ensure!(resource == config.recurso, oauth_resource::ERRO);
    anyhow::ensure!(authorize == config.autorizacao && token == config.token, urls::METADADOS);
    let Some(metodos) = meta.get("token_endpoint_auth_methods_supported") else {
        return Ok(true);
    };
    let metodos = metodos
        .as_array()
        .filter(|metodos| metodos.iter().all(Value::is_string))
        .ok_or_else(|| anyhow!(urls::METADADOS))?;
    let tem = |metodo| metodos.iter().any(|m| m.as_str() == Some(metodo));
    anyhow::ensure!(tem("client_secret_basic") || tem("client_secret_post"), urls::METADADOS);
    Ok(tem("client_secret_basic"))
}

#[cfg(test)]
#[test]
fn app_proprio_aceita_emissor_exato_em_qualquer_posicao() {
    let emissor = "https://auth.example.invalid";
    assert_eq!(
        emissor_da_ficha(&json!({"authorization_servers": ["https://outro.example.invalid", emissor]}), emissor)
            .unwrap(),
        emissor
    );
    for anuncio in [json!({}), json!({"authorization_servers": ["https://auth.example.invalid/"]})] {
        assert!(emissor_da_ficha(&anuncio, emissor).is_err());
    }
}

#[test]
fn app_proprio_exige_recurso_endpoints_e_autenticacao_anunciados() {
    let tipo = &crate::fork::conectores::catalogo_servicos::fixture_app_proprio().tipos[0];
    let Auth::AppProprio { configuracao: Some(config), .. } = tipo.auth else { panic!("fixture") };
    let meta = json!({"token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post"]});
    let metodo = |meta: &Value| cliente_da_ficha(config, config.recurso, meta, config.autorizacao, config.token);
    assert!(metodo(&meta).unwrap());
    assert!(metodo(&json!({})).unwrap(), "RFC 8414: omitido tem default client_secret_basic");
    assert!(!metodo(&json!({"token_endpoint_auth_methods_supported": ["client_secret_post"]})).unwrap());
    for ruim in [json!(null), json!(["none"]), json!(["client_secret_basic", 42])] {
        assert!(metodo(&json!({"token_endpoint_auth_methods_supported": ruim})).is_err());
    }
    assert!(
        cliente_da_ficha(config, "https://servico.example.invalid/mcp/", &meta, config.autorizacao, config.token)
            .is_err()
    );
    assert!(
        cliente_da_ficha(config, config.recurso, &meta, config.autorizacao, "https://outro.example.invalid/token")
            .is_err()
    );
    assert!(
        cliente_da_ficha(config, config.recurso, &meta, "https://outro.example.invalid/authorize", config.token)
            .is_err()
    );
}

/// Starts signing in to connector `id`: the page to open.
pub(crate) async fn start(
    hub: &Arc<Hub>,
    intent: StartIntent,
    callback: Callback,
    tentativa: Option<String>,
) -> Result<Value> {
    iniciar(
        hub,
        intent,
        callback,
        tentativa,
        #[cfg(test)]
        None,
    )
    .await
}

async fn iniciar(
    hub: &Arc<Hub>,
    intent: StartIntent,
    callback: Callback,
    tentativa: Option<String>,
    #[cfg(test)] cimd_teste: Option<(&Auth, &str)>,
) -> Result<Value> {
    oauth_tentativa::viva(tentativa.as_deref())?;
    let store = &hub.store;
    let port = hub.port;
    if crate::environment::Environment::is_open() && !matches!(callback, Callback::Desktop(_)) {
        bail!("login_sem_ouvinte: o retorno do login só chega pelo ouvinte do app neste computador.");
    }
    let app = app_uri(store);
    let redirect_uri = match callback {
        Callback::Host => loopback_uri(port),
        Callback::Desktop(desktop) => loopback_uri(desktop),
        Callback::App => app
            .clone()
            .filter(|uri| {
                reqwest::Url::parse(uri)
                    .is_ok_and(|uri| matches!(uri.scheme(), "http" | "https") && uri.host_str().is_some())
            })
            .ok_or_else(|| {
                anyhow!("The Mexerica cloud is off on this computer, so sign in from the computer itself.")
            })?,
    };
    let (origin, c) = intent.begin(hub)?;
    let auth = market::contas::auth_da_conta(&c)?;
    #[cfg(test)]
    let auth = cimd_teste.map(|(auth, _)| auth).or(auth);
    let cimd = match auth {
        Some(Auth::Oauth { registro: RegistroOauth::Cimd, client_id_metadata_document_url }) => {
            Some(oauth_cimd::url(*client_id_metadata_document_url)?)
        }
        _ => None,
    };
    let cimd = cimd
        .map(|documento| -> Result<_> {
            let permitidos = oauth_cimd::redirects(port);
            anyhow::ensure!(
                matches!(callback, Callback::Host | Callback::Desktop(_)) && permitidos.contains(&redirect_uri),
                oauth_cimd::SO_LOOPBACK
            );
            Ok((documento, permitidos))
        })
        .transpose()?;
    // Conta com o app OAuth da própria pessoa (Google): o servidor de login é o que a ficha fixa, não o que a descoberta
    // disser, e não há cadastro automático (o ID e a chave são os do app dela).
    let proprio = auth.filter(|auth| matches!(auth, Auth::AppProprio { .. }));
    let google = matches!(proprio, Some(Auth::AppProprio { provedor: "google", configuracao: None, .. }));
    // O Google só volta pelo loopback (do app ou do Hub): a página da nuvem nunca recebe o código do app próprio.
    if proprio.is_some() && callback == Callback::App {
        bail!(if google {
            APP_PROPRIO_SO_LOOPBACK
        } else {
            "O login com app próprio exige retorno neste computador."
        });
    }
    let url = c.url.clone().ok_or_else(|| anyhow!("Conector remoto indisponível para entrar."))?;
    let conector = urls::conector(&url)?;
    let scopes = if proprio.is_some() { Ok(None) } else { oauth_scopes::inicial(&url).await };
    binding::check(store, &c, Some((hub, &origin)), || Ok(()))?;
    let scopes = oauth_scopes::capturar(scopes?)?;
    let resource = resource_metadata(&url).await;
    binding::check(store, &c, Some((hub, &origin)), || Ok(()))?;
    let resource = resource.ok_or_else(|| anyhow!(oauth_resource::ERRO))?;
    // Conta de ficha: só os escopos dos grupos ligados (o app nunca os informa). Conta sem ficha segue o caminho antigo.
    let scopes = market::contas::escopos_do_login(&c, scopes, &resource.documento)?;
    let issuer = match proprio {
        // O anúncio do recurso pode trazer uma barra final que o `issuer` dos metadados não tem: só isso se relevia, e o
        // emissor usado daqui em diante é o da ficha.
        Some(Auth::AppProprio { emissor, .. }) if google => emissor_fixado(&resource.documento, emissor)?,
        Some(Auth::AppProprio { emissor, .. }) => emissor_da_ficha(&resource.documento, emissor)?,
        Some(_) => bail!(urls::EMISSOR),
        None => resource.documento["authorization_servers"][0]
            .as_str()
            .map_or_else(|| split(&url).map(|(origin, _)| origin), |s| Ok(s.to_owned()))?,
    };
    urls::derivada(&conector, &issuer)?;
    let meta = server_metadata(&issuer).await.with_context(|| format!("signing in to {}", c.name))?;
    binding::check(store, &c, Some((hub, &origin)), || Ok(()))?;
    // Fora do `with_context`: o app mostra exatamente esta frase.
    anyhow::ensure!(urls::pkce_s256(&meta), urls::PKCE);
    if cimd.is_some() {
        oauth_cimd::anunciar(&meta)?;
    }
    let iss = Esperado::capturar(&issuer, &meta)?;
    let authorize = text(&meta["authorization_endpoint"]).ok_or_else(|| anyhow!("no authorization endpoint"))?;
    let token_endpoint = text(&meta["token_endpoint"]).ok_or_else(|| anyhow!("no token endpoint"))?;
    let app_da_ficha = if let Some(Auth::AppProprio { configuracao: Some(config), .. }) = proprio {
        let basico = cliente_da_ficha(
            *config,
            &resource.resource,
            &meta,
            meta["authorization_endpoint"].as_str().unwrap_or_default(),
            meta["token_endpoint"].as_str().unwrap_or_default(),
        )?;
        let app = market::google_app::AppDaFicha::resolver(
            c.servico.as_deref().unwrap_or_default(),
            c.tipo.as_deref().unwrap_or_default(),
        )?
        .ler(store, &c.space)?
        .ok_or_else(|| anyhow!(market::google_app::APP_DA_FICHA_AUSENTE))?;
        Some((app, basico))
    } else {
        None
    };
    let mut link = urls::derivada(&conector, &authorize)?;
    let token_url = urls::derivada(&conector, &token_endpoint)?;
    anyhow::ensure!(
        oauth_resource::endpoint_sem_resource(&link) && oauth_resource::endpoint_sem_resource(&token_url),
        oauth_resource::ENDPOINT
    );

    oauth_scopes::conferir_endpoint(&link)?;
    oauth_scopes::conferir_endpoint(&token_url)?;
    if cimd.is_some() {
        oauth_cimd::conferir_endpoint(&link)?;
        oauth_cimd::conferir_endpoint(&token_url)?;
    }

    // Reuso só com emissão vinculada pelo Hub ao mesmo issuer e redirects compatíveis.
    let desktop = match callback {
        Callback::Desktop(desktop) => Some(loopback_uri(desktop)),
        _ => None,
    };
    // App próprio: a página da nuvem não entra nos endereços de retorno.
    let app = app.filter(|_| proprio.is_none());
    let wanted: Vec<String> = std::iter::once(loopback_uri(port)).chain(app).chain(desktop).collect();
    let mut oauth = c.oauth.clone().unwrap_or_default();
    let needs_registration =
        cimd.is_none() && (proprio.is_some() || !oauth_registration::reutilizavel(&c, &issuer, &wanted));
    if let Some((documento, permitidos)) = &cimd {
        let cliente = oauth_cimd::cliente(
            documento,
            &redirect_uri,
            permitidos,
            #[cfg(test)]
            cimd_teste.map(|(_, destino)| destino),
        )
        .await;
        binding::check(store, &c, Some((hub, &origin)), || Ok(()))?;
        oauth = cliente?;
    } else if google {
        // O ID e a chave do app do Space vêm do cofre, a cada login (trocar o app não deixa cadastro antigo para trás).
        let app = market::google_app::para_a_conta(store, &c.space)?;
        oauth.client_id = app.client_id;
        oauth.client_secret = app.client_secret;
        oauth.client_secret_basic = false;
        oauth.redirect_uris = wanted;
    } else if let Some((app, basico)) = app_da_ficha {
        oauth.client_id = app.client_id;
        oauth.client_secret = Some(app.client_secret);
        oauth.client_secret_basic = basico;
        oauth.redirect_uris = wanted;
    } else if needs_registration {
        let register = text(&meta["registration_endpoint"]).ok_or_else(|| anyhow!(oauth_registration::SEM_CADASTRO))?;
        urls::derivada(&conector, &register)?;
        let res = urls::cliente()?
            .post(&register)
            .json(&json!({
                "client_name": "Mexerica",
                "redirect_uris": wanted,
                "grant_types": ["authorization_code", "refresh_token"],
                "response_types": ["code"],
                "token_endpoint_auth_method": "none",
            }))
            .timeout(TIMEOUT)
            .send()
            .await;
        binding::check(store, &c, Some((hub, &origin)), || Ok(()))?;
        let res = res.map_err(|_| anyhow!(urls::CADASTRO))?;
        // 3xx com JSON válido não é cadastro; `error_for_status` não recusaria 3xx.
        anyhow::ensure!(res.status().is_success(), urls::CADASTRO);
        let response = res.json::<Value>().await;
        binding::check(store, &c, Some((hub, &origin)), || Ok(()))?;
        let v = response.unwrap_or(Value::Null);
        oauth.client_id = text(&v["client_id"]).ok_or_else(|| anyhow!(urls::CADASTRO))?;
        oauth.client_secret = text(&v["client_secret"]);
        oauth.client_secret_basic = false;
        oauth.redirect_uris = wanted;
    }
    oauth.token_endpoint = token_endpoint;
    let client_id = oauth.client_id.clone();
    let (c, ()) = binding::change(
        store,
        &c,
        Some((hub, &origin)),
        || Ok(()),
        |c| {
            c.oauth = Some(oauth);
            let binding = c.oauth_binding.as_mut().ok_or_else(|| anyhow!("Vínculo ausente"))?;
            if cimd.is_some() {
                binding.clear_registration();
            } else if needs_registration {
                binding.register(&issuer);
            }
            binding.grant(&resource.resource, &issuer, &redirect_uri, &scopes);
            Ok(())
        },
    )?;

    let verifier = crate::remote::crypto::b64(&crate::remote::crypto::random::<32>());
    let challenge = crate::remote::crypto::b64(&crate::remote::crypto::sha256(&[verifier.as_bytes()]));
    let state = crate::remote::crypto::b64(&crate::remote::crypto::random::<16>());
    {
        let mut q = link.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", &client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state);
        if google {
            // Sem `offline` e `consent` o Google não devolve o refresh token, e o login expiraria em uma hora.
            q.append_pair("access_type", "offline").append_pair("prompt", "select_account consent");
        } else {
            q.append_pair("resource", &resource.resource);
        }
        if !scopes.is_empty() {
            q.append_pair("scope", &scopes.join(" "));
        }
    }
    binding::check(store, &c, Some((hub, &origin)), || {
        let mut pending = PENDING.locked();
        let map = pending.get_or_insert_with(HashMap::new);
        oauth_tentativa::viva(tentativa.as_deref())?;
        map.retain(|_, p| p.started.elapsed() < PENDING_FOR);
        if tentativa.is_some() {
            map.retain(|_, p| {
                let anterior = p.tentativa.is_some() && p.tentativa != tentativa && p.snapshot.id == c.id;
                if anterior && let Some(velha) = p.tentativa.as_deref() {
                    oauth_tentativa::cancelar(velha);
                }
                !anterior
            });
        }
        map.insert(
            state,
            Pending {
                snapshot: c.clone(),
                origin: origin.clone(),
                callback,
                verifier,
                redirect_uri,
                resource: resource.resource,
                enviar_resource: !google,
                iss,
                tentativa,
                started: Instant::now(),
            },
        );
        Ok(())
    })?;
    let callback = match callback {
        Callback::Host => "host",
        Callback::App => "app",
        Callback::Desktop(_) => "desktop",
    };
    Ok(json!({"url": link.as_str(), "callback": callback}))
}

/// Trades the code from the sign-in page for tokens.
pub(crate) async fn finish(
    hub: &Arc<Hub>,
    caller: Option<&Caller>,
    state: &str,
    code: Option<&str>,
    error: Option<&str>,
    iss: Option<&str>,
) -> Result<Connector> {
    let store = &hub.store;
    let p = PENDING
        .locked()
        .as_mut()
        .and_then(|m| m.remove(state))
        .filter(|p| p.started.elapsed() < PENDING_FOR)
        .ok_or_else(|| anyhow!("Este login expirou. Inicie o login de novo."))?;
    match caller {
        Some(caller) if p.origin.matches(caller) => {}
        None if p.callback == Callback::Host => {} // O state original autoriza o loopback, sem caller fabricado.
        _ => bail!("Callback não pertence ao operador original"),
    }
    if error.is_some()
        && let Some(tentativa) = p.tentativa.as_deref()
    {
        oauth_tentativa::cancelar(tentativa);
    }
    p.viva()?;
    binding::check(store, &p.snapshot, Some((hub, &p.origin)), || Ok(()))?;
    // M3-B: antes de usar o código ou mostrar qualquer erro vindo do retorno.
    p.iss.conferir(iss)?;
    if error.is_some() {
        bail!("O login não foi concluído. Inicie o login novamente.");
    }
    let code = code.ok_or_else(|| anyhow!("O login não devolveu o código de acesso"))?;
    let oauth = p.snapshot.oauth.as_ref().ok_or_else(|| anyhow!("unknown connector"))?;
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", &p.redirect_uri),
        ("code_verifier", &p.verifier),
    ];
    if p.enviar_resource {
        form.push(("resource", &p.resource));
    }
    let response = token_post(oauth, &form).await;
    #[cfg(test)]
    upstream02_tests::after_token(hub).await;
    binding::check(store, &p.snapshot, Some((hub, &p.origin)), || {
        p.viva()?;
        Ok(())
    })?;
    let (status, v) = response?;
    let renewal = oauth_refresh::classify(status, &v);
    if renewal == oauth_refresh::Renewal::Unregistered {
        binding::change(
            store,
            &p.snapshot,
            Some((hub, &p.origin)),
            || {
                p.viva()?;
                Ok(())
            },
            oauth_registration::apagar,
        )?;
    }
    if renewal != oauth_refresh::Renewal::Renewed {
        bail!("Não foi possível concluir a conexão. Inicie o login novamente.");
    }
    let (saved, ()) = binding::change(
        store,
        &p.snapshot,
        Some((hub, &p.origin)),
        || {
            p.viva()?;
            Ok(())
        },
        |c| {
            c.oauth_binding.as_mut().ok_or_else(|| anyhow!("Vínculo ausente"))?.validate_login_scopes(&v)?;
            store_tokens(c, &v);
            Ok(())
        },
    )?;
    binding::check(store, &saved, Some((hub, &p.origin)), || {
        p.viva()?;
        Ok(saved.clone())
    })
}

/// Transporte separado da classificação OAuth; erros não carregam URL/query ou corpo externo.
pub(crate) async fn token_post(oauth: &OAuth, form: &[(&str, &str)]) -> Result<(reqwest::StatusCode, Value)> {
    let retry = || anyhow!("Não foi possível renovar a conexão agora. Tente novamente em instantes.");
    let res = token_request(oauth, form)?.send().await.map_err(|_| retry())?;
    // 3xx volta como `Ok`: `classify` recusa sem fallback; o `Location` nunca é seguido.
    let status = res.status();
    let v: Value = res.json().await.unwrap_or(Value::Null);
    Ok((status, v))
}

fn token_request(oauth: &OAuth, form: &[(&str, &str)]) -> Result<reqwest::RequestBuilder> {
    let retry = || anyhow!("Não foi possível renovar a conexão agora. Tente novamente em instantes.");
    let mut form: Vec<(&str, &str)> = form.to_vec();
    let mut req = urls::cliente().map_err(|_| retry())?.post(&oauth.token_endpoint);
    if oauth.client_secret_basic {
        let secret = oauth.client_secret.as_deref().filter(|s| !s.is_empty()).ok_or_else(retry)?;
        req = req.basic_auth(encode(&oauth.client_id), Some(encode(secret)));
    } else {
        form.push(("client_id", &oauth.client_id));
        if let Some(secret) = &oauth.client_secret {
            form.push(("client_secret", secret));
        }
    }
    let body = form.iter().map(|(k, v)| format!("{}={}", encode(k), encode(v))).collect::<Vec<_>>().join("&");
    Ok(req
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "application/json")
        .body(body)
        .timeout(TIMEOUT))
}

#[cfg(test)]
#[test]
fn app_proprio_envia_credencial_em_um_unico_metodo_de_autenticacao() {
    let mut cliente = OAuth {
        client_id: "fixture:id".into(),
        client_secret: Some("fixture+secret".into()),
        token_endpoint: "https://auth.example.invalid/token".into(),
        client_secret_basic: true,
        ..Default::default()
    };
    let form = [("grant_type", "refresh_token"), ("resource", "https://servico.example.invalid/mcp")];
    let request = token_request(&cliente, &form).unwrap().build().unwrap();
    let header = request.headers().get(reqwest::header::AUTHORIZATION).unwrap();
    assert!(header.is_sensitive());
    assert!(header.to_str().unwrap().starts_with("Basic "));
    let body = |r: &reqwest::Request| String::from_utf8(r.body().unwrap().as_bytes().unwrap().to_vec()).unwrap();
    assert!(!body(&request).contains("client_"));
    assert!(body(&request).contains("resource="));

    cliente.client_secret_basic = false;
    let request = token_request(&cliente, &form).unwrap().build().unwrap();
    assert!(!request.headers().contains_key(reqwest::header::AUTHORIZATION));
    assert!(body(&request).contains("client_id=fixture%3Aid&client_secret=fixture%2Bsecret"));
}

fn encode(s: &str) -> String {
    reqwest::Url::parse_with_params("x:", [("", s)])
        .ok()
        .and_then(|u| u.query().map(|q| q.trim_start_matches('=').to_owned()))
        .unwrap_or_default()
}

pub(crate) fn store_tokens(c: &mut Connector, v: &Value) {
    let o = c.oauth.get_or_insert_with(OAuth::default);
    o.access_token = text(&v["access_token"]).unwrap_or_default();
    if let Some(r) = text(&v["refresh_token"]) {
        o.refresh_token = Some(r);
    }
    o.expires_at = v["expires_in"].as_u64().map(|s| now().saturating_add(s));
}

/// Forgets a connector's tokens (keeps its registration).
pub(crate) fn sign_out(hub: &Hub, original: &Connector, intent: &RuntimeIntent) -> Result<Connector> {
    binding::change_runtime(hub, original, intent, |c| {
        if let Some(o) = &mut c.oauth {
            o.access_token.clear();
            o.refresh_token = None;
            o.expires_at = None;
        }
        c.oauth_binding.as_mut().ok_or_else(|| anyhow!("Vínculo ausente"))?.reconnect();
        Ok(())
    })
    .map(|(c, ())| c)
}

/// Proxy vinculado à fotografia e ao Context/Setup originais, nunca a um ID recapturado.
/// `stale`: the server just said 401, so refresh even if the token looks valid.
pub(crate) async fn target(
    hub: &Hub,
    expected: &Connector,
    intent: &RuntimeIntent,
    stale: bool,
) -> Result<oauth_refresh::Target> {
    let store = &hub.store;
    let c = intent.current(hub, expected)?;
    let url = c.url.clone().ok_or_else(|| anyhow!("{} isn't a remote connector", c.name))?;
    let mut headers = std::collections::BTreeMap::new();
    for (k, v) in &c.headers {
        headers.insert(k.clone(), market::passwords::resolve(store, v).await?);
        intent.current(hub, expected)?;
    }
    // Escolher o par atual da MESMA fonte depois dos awaits de resolução de senha.
    let mut resolved = intent.current(hub, expected)?;
    if let Some(o) = &resolved.oauth {
        // O 401 pertence ao access usado pela fotografia esperada, não a um par
        // já rotacionado legitimamente. Isso não é a identidade da concessão.
        let stale = stale && expected.oauth.as_ref().is_some_and(|old| old.access_token == o.access_token);
        let o = fresh(hub, &resolved, o, intent, stale).await?;
        resolved.oauth = Some(o);
    }
    binding::check_runtime(hub, &resolved, intent, || {
        oauth_refresh::Target::new(hub, &url, headers, resolved.oauth.as_ref())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_resource_metadata_hint() {
        let h = r#"Bearer error="invalid_token", resource_metadata="https://mcp.example.com/.well-known/oauth-protected-resource""#;
        assert_eq!(
            resource_metadata_hint(h).as_deref(),
            Some("https://mcp.example.com/.well-known/oauth-protected-resource")
        );
        assert_eq!(resource_metadata_hint("Bearer realm=\"x\""), None);
        assert_eq!(resource_metadata_hint("Bearer resource_metadata=\"javascript:x\""), None);
    }

    #[test]
    fn app_proprio_so_aceita_o_emissor_da_ficha_com_ou_sem_barra_final() {
        let anuncio = |s: &str| json!({"authorization_servers": [s]});
        let google = "https://accounts.google.com";
        assert_eq!(emissor_fixado(&anuncio("https://accounts.google.com/"), google).unwrap(), google);
        assert_eq!(emissor_fixado(&anuncio("https://accounts.google.com"), google).unwrap(), google);
        for outro in [
            "https://accounts.google.com.evil.example",
            "https://evil.example/https://accounts.google.com",
            "http://accounts.google.com",
            "https://Accounts.google.com",
            "https://accounts.google.com//",
            "https://accounts.google.com/x",
            "",
        ] {
            assert_eq!(emissor_fixado(&anuncio(outro), google).unwrap_err().to_string(), urls::EMISSOR, "{outro}");
        }
        assert!(emissor_fixado(&json!({}), google).is_err());
        assert!(emissor_fixado(&json!({"authorization_servers": []}), google).is_err());
    }

    #[test]
    fn splits_origin_and_path() {
        assert_eq!(split("https://mcp.linear.app/mcp/").unwrap(), ("https://mcp.linear.app".into(), "/mcp".into()));
        assert_eq!(split("https://a.b").unwrap(), ("https://a.b".into(), String::new()));
    }

    /// Serves `initialize` with `status`, and protected resource metadata when `metadata`.
    async fn server(status: u16, metadata: bool) -> String {
        use axum::{Json, Router, http::StatusCode, routing};
        let code = StatusCode::from_u16(status).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let mut router = Router::new().route("/mcp", routing::post(move || async move { (code, Json(json!({}))) }));
        if metadata {
            router = router.route(
                "/.well-known/oauth-protected-resource",
                routing::get(move || {
                    let base = base.clone();
                    async move { Json(json!({"resource":base,"authorization_servers": ["https://auth.example"]})) }
                }),
            );
        }
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        url
    }

    #[tokio::test]
    async fn sign_in_is_required_when_the_server_asks_for_it() {
        let none = std::collections::BTreeMap::new();
        assert!(required(&server(401, false).await, &none).await);
        assert!(!required(&server(200, false).await, &none).await);
        // Lets `initialize` through, refuses tool calls, and publishes its metadata.
        let lazy = server(200, true).await;
        assert!(required(&lazy, &none).await);
        let key = std::collections::BTreeMap::from([("x-api-key".to_owned(), "k".to_owned())]);
        assert!(!required(&lazy, &key).await);
    }

    /// M1: sonda usa a oferta comum, tira headers MCP reservados e preserva o
    /// ping não autenticado como único caminho até os metadados de login.
    #[tokio::test]
    async fn sonda_preserva_ping_de_desafio_e_fallback_so_404_405() {
        use axum::{Json, Router, http::HeaderMap, http::StatusCode, response::IntoResponse, routing};
        use std::sync::{Arc, Mutex};
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (log, hint) = (seen.clone(), format!("{base}/meta"));
        let get_log = seen.clone();
        let status = Arc::new(Mutex::new(200_u16));
        let current = status.clone();
        let resource = format!("{base}/mcp");
        let router = Router::new()
            .route(
                "/mcp",
                routing::post(move |headers: HeaderMap, Json(frame): Json<Value>| {
                    let (log, hint, current) = (log.clone(), hint.clone(), current.clone());
                    async move {
                        let reserved = headers.get_all("mcp-protocol-version").iter().count()
                            + headers.get_all("mcp-session-id").iter().count();
                        let method = frame["method"].as_str().unwrap_or_default();
                        let offered = frame["params"]["protocolVersion"].as_str().unwrap_or("-");
                        log.lock().unwrap().push(format!("{method}:{offered}:{reserved}"));
                        if method == "ping" {
                            let challenge = format!("Bearer resource_metadata=\"{hint}\"");
                            return (StatusCode::UNAUTHORIZED, [("www-authenticate", challenge)]).into_response();
                        }
                        let code = StatusCode::from_u16(*current.lock().unwrap()).unwrap();
                        (code, Json(json!({}))).into_response()
                    }
                })
                .get(move |headers: HeaderMap| {
                    let reserved = headers.get_all("mcp-protocol-version").iter().count()
                        + headers.get_all("mcp-session-id").iter().count();
                    get_log.lock().unwrap().push(format!("get:{reserved}"));
                    async { StatusCode::UNAUTHORIZED }
                }),
            )
            .route(
                "/meta",
                routing::get(move || {
                    let resource = resource.clone();
                    async move { Json(json!({"resource":resource,"authorization_servers":["https://auth.example"]})) }
                }),
            );
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let url = format!("{base}/mcp");
        // initialize 200 sem hint, well-known 404, ping 401 com URL única de metadados.
        assert!(required(&url, &std::collections::BTreeMap::new()).await);
        assert_eq!(*seen.lock().unwrap(), ["initialize:2025-06-18:0", "ping:-:0"]);
        let reserved = std::collections::BTreeMap::from([
            ("MCP-Protocol-Version".to_owned(), "1999-01-01".to_owned()),
            ("mcp-session-id".to_owned(), "sessao-alheia".to_owned()),
        ]);
        for (code, login) in [(400, false), (404, true), (405, true)] {
            seen.lock().unwrap().clear();
            *status.lock().unwrap() = code;
            assert_eq!(required(&url, &reserved).await, login, "HTTP {code}");
            let expected: &[&str] =
                if login { &["initialize:2025-06-18:0", "get:0"] } else { &["initialize:2025-06-18:0"] };
            assert_eq!(*seen.lock().unwrap(), expected, "HTTP {code}");
        }
    }

    #[test]
    fn form_values_are_encoded() {
        assert_eq!(encode("a b&c=d/+"), "a+b%26c%3Dd%2F%2B");
    }

    #[tokio::test]
    async fn unknown_state_is_refused() {
        let hub = crate::fork::conectores::oauth_binding::tests::hub();
        let err = finish(&hub, Some(&Caller::Local), "nope", Some("code"), None, None).await.unwrap_err();
        assert!(err.to_string().contains("expirou"));
    }
}

#[cfg(test)]
#[path = "../fork/conectores/oauth_tests.rs"]
pub(crate) mod upstream02_tests;
