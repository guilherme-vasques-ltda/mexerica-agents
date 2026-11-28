use super::*;
use crate::{
    api::devices::Caller,
    fork::conectores::{
        catalogo_servicos::{Auth, RegistroOauth},
        oauth_cimd, oauth_desktop, oauth_urls,
    },
    market::oauth::{self, Callback, StartIntent, loopback_uri},
};

const CLIENTE: &str = "https://client.example.invalid:443/documento";
const DOCUMENTO: &str = "/client-metadata";

#[test]
fn cimd_nao_aceita_parametros_oauth_preenchidos_no_endpoint() {
    for query in ["clie%6et_id=outro", "client_secret=fixture-secret", "state=fixture-state", "code_challenge=outro"] {
        let url = reqwest::Url::parse(&format!("https://auth.example.invalid/endpoint?{query}")).unwrap();
        assert!(oauth_cimd::conferir_endpoint(&url).is_err());
    }
    let url = reqwest::Url::parse("https://auth.example.invalid/endpoint?tenant=fixture").unwrap();
    assert!(oauth_cimd::conferir_endpoint(&url).is_ok());
}

fn responder(f: &Fixture, path: &str, status: StatusCode, location: &str, body: Value) {
    f.redirects.lock().unwrap().insert(path.into(), (status, location.into(), body));
}

fn metadados(f: &Fixture, issuer: &str, cimd: Option<Value>, pkce: &'static str) {
    let mut body = json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{}/authorize", f.provider),
        "token_endpoint": format!("{}/token", f.provider),
        "registration_endpoint": format!("{}/register", f.provider),
        "code_challenge_methods_supported": [pkce],
        "authorization_response_iss_parameter_supported": true,
    });
    if let Some(cimd) = cimd {
        body["client_id_metadata_document_supported"] = cimd;
    }
    for path in ["/.well-known/oauth-authorization-server", "/.well-known/openid-configuration"] {
        responder(f, path, StatusCode::OK, "", body.clone());
    }
}

async fn iniciar(f: &Arc<Fixture>, url: Option<&'static str>) -> anyhow::Result<Value> {
    let intent = StartIntent::capture(&f.hub, &Caller::Local, "notes").unwrap();
    let auth = Auth::Oauth { registro: RegistroOauth::Cimd, client_id_metadata_document_url: url };
    oauth::iniciar(
        &f.hub,
        intent,
        Callback::Desktop(oauth_desktop::PORTAS[0]),
        None,
        Some((&auth, &format!("{}{DOCUMENTO}", f.provider))),
    )
    .await
}

fn documento(f: &Fixture) -> Value {
    json!({
        "client_id": CLIENTE,
        "client_name": "Cliente fictício",
        "redirect_uris": oauth_cimd::redirects(f.hub.port),
        "token_endpoint_auth_method": "none"
    })
}

#[tokio::test]
async fn cimd_login_sem_dcr_sem_segredo_e_com_state_iss_pkce() {
    let f = Fixture::with_disk(true).await;
    *f.registration_reply.lock().unwrap() =
        json!({"client_id":"fixture-dcr-client","client_secret":"fixture-dcr-secret"});
    let (status, _) = f.rpc("connectorSignIn", json!({"id":"notes","loopbackPort":oauth_desktop::PORTAS[0]})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(f.saved().oauth_binding.unwrap().registration_issuer().is_some());
    assert!(f.saved().oauth.unwrap().client_secret.is_some());
    assert_eq!(f.registration_frames.lock().unwrap().len(), 1);

    metadados(&f, &f.provider, Some(json!(true)), "S256");
    responder(&f, DOCUMENTO, StatusCode::OK, "", documento(&f));
    f.paths.lock().unwrap().clear();
    let abrir = || async {
        let login = iniciar(&f, Some(CLIENTE)).await.unwrap();
        assert_eq!(login["callback"], "desktop");
        let url = reqwest::Url::parse(login["url"].as_str().unwrap()).unwrap();
        let params: std::collections::BTreeMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(params["client_id"], CLIENTE);
        assert_eq!(params["redirect_uri"], loopback_uri(oauth_desktop::PORTAS[0]));
        assert_eq!(params["code_challenge_method"], "S256");
        assert!(!params["state"].is_empty() && !params["code_challenge"].is_empty());
        (params["state"].clone(), params["code_challenge"].clone())
    };
    let (state, _) = abrir().await;
    for (state, iss) in [("fixture-wrong-state", f.provider.as_str()), (&state, "https://other.example.invalid")] {
        let (status, _) = f.rpc("connectorSignInFinish", json!({"state":state,"code":"fixture-code","iss":iss})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(f.token_calls.load(Ordering::SeqCst), 0);
    }
    let (state, challenge) = abrir().await;
    let (status, _) =
        f.rpc("connectorSignInFinish", json!({"state":state,"code":"fixture-code","iss":f.provider})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 1);
    assert!(!f.paths.lock().unwrap().iter().any(|p| p == "/register"));
    assert_eq!(f.registration_frames.lock().unwrap().len(), 1);
    let forms = f.token_forms.lock().unwrap();
    let form = reqwest::Url::parse(&format!("https://token.example.invalid/?{}", forms[0])).unwrap();
    let params: std::collections::BTreeMap<_, _> = form.query_pairs().into_owned().collect();
    assert_eq!(params["client_id"], CLIENTE);
    assert!(!params.contains_key("client_secret"));
    assert_eq!(
        challenge,
        crate::remote::crypto::b64(&crate::remote::crypto::sha256(&[params["code_verifier"].as_bytes()]))
    );

    let store = Store::open(&f.dir.join("fixture.db")).unwrap();
    *store.secret_key.lock().unwrap() = Some(zeroize::Zeroizing::new(vec![42; 32]));
    let saved = market::connectors(&store).unwrap().remove(0);
    assert!(saved.oauth_binding.unwrap().registration_issuer().is_none());
    let cliente = saved.oauth.unwrap();
    assert_eq!(cliente.client_id, CLIENTE);
    assert!(cliente.signed_in() && cliente.client_secret.is_none() && !cliente.client_secret_basic);
    let req = oauth::token_request(&cliente, &[("grant_type", "refresh_token")]).unwrap().build().unwrap();
    assert!(!req.headers().contains_key(reqwest::header::AUTHORIZATION));
}

#[tokio::test]
async fn cimd_recusa_configuracao_e_metadados_sem_tentar_dcr() {
    let f = Fixture::new().await;
    f.paths.lock().unwrap().clear();
    for (url, erro) in [(None, oauth_cimd::AUSENTE), (Some("file:///fixture"), oauth_cimd::URL_INVALIDA)] {
        assert_eq!(iniciar(&f, url).await.unwrap_err().to_string(), erro);
        assert!(f.paths.lock().unwrap().is_empty());
    }
    for suporte in [None, Some(json!(false)), Some(json!("true"))] {
        metadados(&f, &f.provider, suporte, "S256");
        assert_eq!(iniciar(&f, Some(CLIENTE)).await.unwrap_err().to_string(), oauth_cimd::SEM_SUPORTE);
    }
    metadados(&f, &format!("{}/", f.provider), Some(json!(true)), "S256");
    let recusa = iniciar(&f, Some(CLIENTE)).await.unwrap_err();
    assert!(recusa.chain().any(|e| e.to_string() == oauth_urls::EMISSOR));
    metadados(&f, &f.provider, Some(json!(true)), "plain");
    assert_eq!(iniciar(&f, Some(CLIENTE)).await.unwrap_err().to_string(), oauth_urls::PKCE);
    assert!(!f.paths.lock().unwrap().iter().any(|p| matches!(p.as_str(), DOCUMENTO | "/register" | "/token")));
}

#[tokio::test]
async fn cimd_recusa_documento_invalido_sem_tentar_dcr_nem_seguir_redirect() {
    let f = Fixture::new().await;
    metadados(&f, &f.provider, Some(json!(true)), "S256");
    let base = documento(&f);
    let mut sem_nome = base.clone();
    sem_nome.as_object_mut().unwrap().remove("client_name");
    responder(&f, DOCUMENTO, StatusCode::OK, "", sem_nome);
    assert!(iniciar(&f, Some(CLIENTE)).await.unwrap_err().to_string().contains("validar o documento CIMD"));
    let mut remoto = base["redirect_uris"].as_array().unwrap().clone();
    remoto.push(json!("https://callback.example.invalid/oauth/callback"));
    for (campo, valor) in [
        ("client_id", json!("https://client.example.invalid/documento")),
        ("redirect_uris", json!([loopback_uri(oauth_desktop::PORTAS[1])])),
        ("redirect_uris", json!(remoto)),
        ("redirect_uris", json!([loopback_uri(oauth_desktop::PORTAS[0]), loopback_uri(9)])),
        ("token_endpoint_auth_method", json!("client_secret_post")),
        ("client_secret", json!("fixture-secret")),
        ("client_secret", Value::Null),
        ("client_secret_expires_at", json!(0)),
        ("client_secret_expires_at", Value::Null),
        ("client_name", json!("")),
        ("client_name", json!(" \t")),
        ("client_name", json!("x".repeat(129))),
        ("client_uri", json!("x".repeat(17 * 1024))),
    ] {
        let mut invalido = base.clone();
        invalido[campo] = valor;
        responder(&f, DOCUMENTO, StatusCode::OK, "", invalido);
        assert!(iniciar(&f, Some(CLIENTE)).await.unwrap_err().to_string().contains("validar o documento CIMD"));
    }
    responder(&f, "/destino", StatusCode::OK, "", base.clone());
    responder(&f, DOCUMENTO, StatusCode::FOUND, &format!("{}/destino", f.provider), base);
    assert!(iniciar(&f, Some(CLIENTE)).await.is_err());
    assert!(!f.paths.lock().unwrap().iter().any(|p| matches!(p.as_str(), "/destino" | "/register" | "/token")));
    assert!(f.saved().oauth_binding.unwrap().resource().is_none());
}
