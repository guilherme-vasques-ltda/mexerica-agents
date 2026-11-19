//! Login com o Hub em outro computador: o retorno cai no ouvinte do app desktop (porta fixa) e volta ao Hub pelo canal
//! autenticado. Mesma fixture HTTP do restante do OAuth (provedor e Hub reais, em processo).
use super::*;
use crate::api::devices::Caller;
use crate::store::{Device, DeviceSource, Scope};

const PORTA: u16 = 19424;
const URI: &str = "http://127.0.0.1:19424/oauth/callback";

async fn iniciar(f: &Fixture, corpo: Value) -> (Value, String) {
    let (status, reply) = f.rpc("connectorSignIn", corpo).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let link = reqwest::Url::parse(reply["url"].as_str().unwrap()).unwrap();
    let state = link.query_pairs().find(|(k, _)| k == "state").unwrap().1.into_owned();
    (reply, state)
}

async fn concluir(f: &Fixture, caller: &Caller, body: Value) -> anyhow::Result<Value> {
    api::dispatch(&f.hub, caller, "connectorSignInFinish", body).await
}

fn aparelho(f: &Fixture, chave: &str) -> Caller {
    let d = Device {
        key: chave.into(),
        name: "fixture".into(),
        platform: "fixture".into(),
        source: DeviceSource::Account,
        grant_id: Some("account-grant".into()),
        scopes: vec![Scope::Control],
        lease_until: Some(crate::store::now_ms() + 60000),
        created_at: 1,
        last_seen_at: None,
    };
    f.hub.store.put_device(&d).unwrap();
    Caller::Device { key: d.key, scopes: vec![Scope::Control], autoridade: None }
}

#[tokio::test]
async fn desktop_login_volta_pelo_canal_autenticado_com_a_porta_do_app_no_redirect() {
    let f = Fixture::new().await;
    let (reply, state) = iniciar(&f, json!({"id":"notes","loopbackPort":PORTA})).await;
    assert_eq!(reply["callback"], "desktop");
    let link = reqwest::Url::parse(reply["url"].as_str().unwrap()).unwrap();
    // O redirect é o do app, não o da porta do Hub; PKCE e state seguem presentes.
    assert_eq!(link.query_pairs().find(|(k, _)| k == "redirect_uri").unwrap().1, URI);
    assert_eq!(link.query_pairs().find(|(k, _)| k == "code_challenge_method").unwrap().1, "S256");
    assert!(link.query_pairs().any(|(k, _)| k == "code_challenge"));
    // O cadastro dinâmico leva a URI do app (e a do Hub, que o login local continua usando).
    let uris = f.registration_frames.lock().unwrap().last().unwrap().1["redirect_uris"].clone();
    assert!(uris.as_array().unwrap().iter().any(|u| u == URI), "{uris}");
    assert_eq!(
        f.saved().oauth_binding.as_ref().map(|b| serde_json::to_value(b).unwrap()["grant"]["redirect"].clone()),
        Some(json!(URI))
    );

    let answer = concluir(&f, &Caller::Local, json!({"state":state,"code":"fixture-code"})).await.unwrap();
    assert_eq!(answer["connector"]["auth"], "signedIn");
    // A troca do código usa o mesmo redirect_uri do pedido e o verifier que nunca saiu do Hub.
    let form = f.token_forms.lock().unwrap().last().unwrap().clone();
    assert!(form.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A19424%2Foauth%2Fcallback"), "{form}");
    assert!(form.contains("code_verifier="), "{form}");
    // O state é de uso único: repetir o retorno não troca código de novo.
    assert!(concluir(&f, &Caller::Local, json!({"state":state,"code":"fixture-code"})).await.is_err());
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn desktop_login_nao_conclui_pela_pagina_do_hub_nem_com_state_alheio() {
    let f = Fixture::new().await;
    let (_, state) = iniciar(&f, json!({"id":"notes","loopbackPort":PORTA})).await;
    // A página `GET /oauth/callback` é do login feito no próprio computador do Hub: não aceita o state do app.
    let pagina = f.callback(&state).await;
    assert!(!pagina.contains("notes conectado"), "{pagina}");
    assert!(pagina.contains("O login não foi concluído"), "{pagina}");
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 0);

    // Retorno com state inventado, ou de outra tentativa já usada: recusa sem tocar no provedor.
    let (_, outro) = iniciar(&f, json!({"id":"notes","loopbackPort":PORTA})).await;
    assert!(concluir(&f, &Caller::Local, json!({"state":"inventado","code":"x"})).await.is_err());
    assert!(concluir(&f, &Caller::Local, json!({"state":state,"code":"x"})).await.is_err());
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 0);
    // O state de uma tentativa não conclui com o `iss` de outro emissor.
    let err = concluir(&f, &Caller::Local, json!({"state":outro,"code":"x","iss":"https://outro.invalid"})).await;
    assert_eq!(err.unwrap_err().to_string(), crate::fork::conectores::oauth_iss::RETORNO);
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn desktop_login_so_vale_para_o_chamador_local_e_para_as_portas_do_app() {
    let f = Fixture::new().await;
    // Porta fora da lista (inclusive a do próprio Hub), de outro tipo ou como texto: recusa sem iniciar login.
    for ruim in [json!(19422), json!(1), json!(65535), json!("19424"), json!(true)] {
        let (status, body) = f.rpc("connectorSignIn", json!({"id":"notes","loopbackPort":ruim})).await;
        assert_ne!(status, StatusCode::OK, "{ruim}");
        assert!(body.to_string().contains(crate::fork::conectores::oauth_desktop::RECUSA), "{body}");
    }
    // Aparelho (celular) não pede porta de retorno; segue pela nuvem.
    f.hub.store.kv_set("test_cloud_url", "https://fixture.invalid").unwrap();
    f.hub.cloud_synced.store(true, Ordering::Release);
    let celular = aparelho(&f, "celular");
    let erro = api::dispatch(&f.hub, &celular, "connectorSignIn", json!({"id":"notes","loopbackPort":PORTA})).await;
    assert_eq!(erro.unwrap_err().to_string(), crate::fork::conectores::oauth_desktop::RECUSA);
    // Um aparelho também não conclui o state do app desktop.
    let (_, state) = iniciar(&f, json!({"id":"notes","loopbackPort":PORTA})).await;
    assert!(concluir(&f, &celular, json!({"state":state,"code":"x"})).await.is_err());
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 0);
    // E o login local de sempre (sem porta) segue pela página do Hub.
    let (reply, state) = iniciar(&f, json!({"id":"notes"})).await;
    assert_eq!(reply["callback"], "host");
    assert!(f.callback(&state).await.contains("notes conectado"));
}

#[tokio::test]
async fn pagina_do_retorno_do_hub_esta_em_portugues() {
    let f = Fixture::new().await;
    let state = f.start().await;
    let ok = f.callback(&state).await;
    assert!(
        ok.contains("lang=\"pt-BR\"") && ok.contains("notes conectado") && ok.contains("Pode fechar esta aba"),
        "{ok}"
    );
    let falha = f.callback("state-inexistente").await;
    assert!(falha.contains("O login não foi concluído") && falha.contains("Este login expirou"), "{falha}");
    for ingles in ["is connected", "Sign-in didn", "You can close", "expired. Start"] {
        assert!(!ok.contains(ingles) && !falha.contains(ingles), "{ingles}");
    }
}
