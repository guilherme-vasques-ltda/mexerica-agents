//! M3-F: origem da emissão, não igualdade de IDs, controla o reuso do cadastro.
use super::*;
use crate::fork::conectores::{oauth_binding as binding, oauth_registration};
use crate::market::oauth;

fn issuer(f: &Fixture) -> Option<String> {
    f.saved().oauth_binding.as_ref().unwrap().registration_issuer().map(str::to_owned)
}

fn secret(f: &Fixture, secret: &str) {
    *f.registration_reply.lock().unwrap() = json!({"client_id":"fixture-client","client_secret":secret});
}

fn sem_selo(f: &Fixture) {
    // Simula registro persistido pelo binário antigo, inclusive Grant/prova de scope.
    let mut all = serde_json::to_value(market::connectors(&f.hub.store).unwrap()).unwrap();
    all[0]["oauthBinding"].as_object_mut().unwrap().remove("registrationIssuer");
    market::vault::write(&f.hub.store, "connectors", &all.to_string()).unwrap();
}

fn sem_pending(f: &Fixture) {
    let url = format!("{}/mcp", f.provider);
    let pending = super::super::PENDING.lock().unwrap();
    assert!(!pending.as_ref().is_some_and(|m| m.values().any(|p| p.snapshot.url.as_deref() == Some(url.as_str()))));
}

fn cadastro_limpo(f: &Fixture, segredo_antigo: &str) {
    let frames = f.registration_frames.lock().unwrap();
    assert!(!frames.is_empty());
    for (headers, body) in frames.iter() {
        assert!(!headers.contains_key("authorization"));
        assert!(!headers.contains_key("x-private"));
        assert!(body.get("client_id").is_none());
        assert!(body.get("client_secret").is_none());
        assert!(!format!("{headers:?}{body}").contains(segredo_antigo));
    }
}

#[tokio::test]
async fn m3f_troca_emissor_com_ids_iguais_emite_b_sem_credencial_a_e_reusa_b() {
    let a = Fixture::new().await;
    let b = Fixture::new().await;
    market::update_connectors(&a.hub.store, |all| {
        all[0].headers.insert("x-private".into(), "header-a".into());
        Ok(())
    })
    .unwrap();
    secret(&a, "secret-a");
    let state = a.start().await;
    assert!(a.callback(&state).await.contains("notes conectado"));
    assert_eq!(issuer(&a).as_deref(), Some(a.provider.as_str()));
    secret(&b, "secret-b");
    *a.issuer.lock().unwrap() = b.provider.clone();
    let (status, reply) = a.rpc("connectorSignIn", json!({"id":"notes"})).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let link = reqwest::Url::parse(reply["url"].as_str().unwrap()).unwrap();
    assert_eq!(link.as_str().split('?').next().unwrap(), format!("{}/authorize", b.provider));
    assert_eq!(link.query_pairs().find(|(k, _)| k == "client_id").unwrap().1, "fixture-client");
    let state = link.query_pairs().find(|(k, _)| k == "state").unwrap().1.into_owned();
    assert!(a.callback(&state).await.contains("notes conectado"));
    assert_eq!(issuer(&a).as_deref(), Some(b.provider.as_str()));
    assert_eq!(a.registration_frames.lock().unwrap().len(), 1);
    assert_eq!(b.registration_frames.lock().unwrap().len(), 1, "ID igual não dispensa DCR B");
    cadastro_limpo(&b, "secret-a");
    let forms = b.token_forms.lock().unwrap().clone();
    assert_eq!(forms.len(), 1);
    assert!(forms[0].contains("client_secret=secret-b"));
    assert!(!forms[0].contains("secret-a"));
    assert_eq!(a.token_forms.lock().unwrap().len(), 1, "token novo não vai a A");
    let next = a.start().await;
    assert!(a.callback(&next).await.contains("notes conectado"));
    assert_eq!(b.registration_frames.lock().unwrap().len(), 1, "mesmo issuer reusa B");
    assert_eq!(a.saved().oauth.as_ref().unwrap().client_secret.as_deref(), Some("secret-b"));
    assert!(a.saved().public().get("oauthBinding").is_none());
}

#[tokio::test]
async fn m3f_emissor_nao_e_origem_dos_endpoints_de_cadastro_e_token() {
    let a = Fixture::new().await;
    let endpoints = Fixture::new().await;
    secret(&endpoints, "cross-origin-secret");
    a.redirects.lock().unwrap().insert(
        "/.well-known/oauth-authorization-server".into(),
        (
            StatusCode::OK,
            String::new(),
            json!({"issuer":a.provider,
        "authorization_endpoint":format!("{}/authorize", endpoints.provider),
        "token_endpoint":format!("{}/token",endpoints.provider),
        "registration_endpoint":format!("{}/register",endpoints.provider),
        "code_challenge_methods_supported":["S256"]}),
        ),
    );
    let state = a.start().await;
    assert!(a.callback(&state).await.contains("notes conectado"));
    assert_eq!(issuer(&a).as_deref(), Some(a.provider.as_str()));
    assert_eq!(a.registration_frames.lock().unwrap().len(), 0);
    assert_eq!(endpoints.registration_frames.lock().unwrap().len(), 1);
    assert!(endpoints.token_forms.lock().unwrap()[0].contains("client_secret=cross-origin-secret"));
    let next = a.start().await;
    assert!(a.callback(&next).await.contains("notes conectado"));
    assert_eq!(endpoints.registration_frames.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn m3f_legado_com_grant_e_scope_validado_nao_prova_origem_do_cadastro() {
    let f = Fixture::new().await;
    secret(&f, "legacy-secret");
    let state = f.start().await;
    assert!(f.callback(&state).await.contains("notes conectado"));
    sem_selo(&f);
    let legacy = f.saved();
    let old_binding = serde_json::to_value(&legacy.oauth_binding).unwrap();
    assert!(old_binding.get("registrationIssuer").is_none());
    assert!(old_binding["grant"].is_object());
    assert!(old_binding["scopeValidation"]["evidencia"]["Validado"].is_array());
    // Migração não desloga e refresh não inventa marcador nem altera Binding/revision.
    assert_eq!(f.runtime_target(false).await.unwrap()["headers"]["Authorization"], "Bearer fixture-access-0");
    assert!(f.runtime_target(true).await.is_ok());
    assert_eq!(f.saved().oauth_binding, legacy.oauth_binding);
    assert!(issuer(&f).is_none());
    secret(&f, "new-secret");
    let state = f.start().await;
    assert_eq!(f.registration_frames.lock().unwrap().len(), 2);
    assert_eq!(f.saved().oauth.as_ref().unwrap().client_secret.as_deref(), Some("new-secret"));
    assert_eq!(issuer(&f).as_deref(), Some(f.provider.as_str()));
    assert!(f.callback(&state).await.contains("notes conectado"));
    assert!(f.token_forms.lock().unwrap().last().unwrap().contains("client_secret=new-secret"));
}

#[tokio::test]
async fn m3f_sem_dcr_ou_resposta_invalida_recusa_sem_selo_pending_link_ou_eco() {
    for mode in ["unsupported", "legacy", "redirect", "error", "malformed"] {
        let a = Fixture::new().await;
        let b = Fixture::new().await;
        secret(&a, "private-old-secret");
        let state = a.start().await;
        assert!(a.callback(&state).await.contains("notes conectado"));
        if mode == "legacy" {
            sem_selo(&a);
            // O bug antigo pode ter gravado Grant B com cadastro emitido por A.
            let mut all = serde_json::to_value(market::connectors(&a.hub.store).unwrap()).unwrap();
            all[0]["oauthBinding"]["grant"]["issuer"] = json!(b.provider);
            market::vault::write(&a.hub.store, "connectors", &all.to_string()).unwrap();
        }
        *a.issuer.lock().unwrap() = b.provider.clone();
        if matches!(mode, "unsupported" | "legacy") {
            b.redirects.lock().unwrap().insert(
                "/.well-known/oauth-authorization-server".into(),
                (
                    StatusCode::OK,
                    String::new(),
                    json!({"issuer":b.provider,
                "authorization_endpoint":format!("{}/authorize", b.provider),
                "token_endpoint":format!("{}/token", b.provider),"code_challenge_methods_supported":["S256"]}),
                ),
            );
        } else {
            let status = match mode {
                "redirect" => StatusCode::TEMPORARY_REDIRECT,
                "error" => StatusCode::BAD_REQUEST,
                _ => StatusCode::OK,
            };
            b.redirects.lock().unwrap().insert(
                "/register".into(),
                (
                    status,
                    format!("{}/must-not-follow", a.provider),
                    if mode == "malformed" {
                        json!({"error":"private-remote-body"})
                    } else {
                        json!({"client_id":"private-remote-id","client_secret":"private-remote-body"})
                    },
                ),
            );
        }
        let (status, reply) = a.rpc("connectorSignIn", json!({"id":"notes"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{mode}: {reply}");
        assert!(reply.get("url").is_none());
        let error = reply.to_string();
        for forbidden in
            [a.provider.as_str(), b.provider.as_str(), "private-old-secret", "private-remote-body", "private-remote-id"]
        {
            assert!(!error.contains(forbidden), "{mode}: {error}");
        }
        assert!(error.contains(if matches!(mode, "unsupported" | "legacy") {
            oauth_registration::SEM_CADASTRO
        } else {
            crate::fork::conectores::oauth_urls::CADASTRO
        }));
        assert!(!a.paths.lock().unwrap().iter().any(|p| p == "/must-not-follow"));
        let saved = a.saved();
        assert_eq!(saved.oauth.as_ref().unwrap().client_secret.as_deref(), Some("private-old-secret"));
        assert_eq!(saved.oauth.as_ref().unwrap().access_token, "");
        assert_eq!(
            issuer(&a).as_deref(),
            if mode == "legacy" { None } else { Some(a.provider.as_str()) },
            "não atribuir cadastro A a B"
        );
        assert!(saved.oauth_binding.as_ref().unwrap().resource().is_none());
        assert_eq!(b.token_calls.load(Ordering::SeqCst), 0);
        sem_pending(&a);
    }
}

#[tokio::test]
async fn m3f_lifetime_do_selo_acompanha_cadastro_nao_concessao_ou_par() {
    let f = Fixture::new().await;
    secret(&f, "lifetime-secret");
    let original = f.refresh_ready(Some(oauth::now() + 30)).await;
    f.runtime_target(true).await.unwrap();
    assert_eq!(f.saved().oauth_binding, original.oauth_binding);
    let current = f.saved();
    let setup = binding::issue_setup(&f.hub.store, &current).unwrap();
    let intent =
        crate::fork::conectores::oauth_runtime::RuntimeIntent::capture(&f.hub, &setup.id, "notes", "initialize")
            .unwrap();
    oauth::sign_out(&f.hub, &current, &intent).unwrap();
    assert_eq!(issuer(&f).as_deref(), Some(f.provider.as_str()));
    assert_eq!(f.saved().oauth.as_ref().unwrap().client_secret.as_deref(), Some("lifetime-secret"));
    let state = f.start().await;
    assert!(f.callback(&state).await.contains("notes conectado"));
    assert_eq!(f.registration_frames.lock().unwrap().len(), 1);
    f.reply(401, json!({"error":"invalid_client"}));
    assert!(f.runtime_target(true).await.is_err());
    assert!(issuer(&f).is_none());
    assert_eq!(f.saved().oauth, Some(oauth::OAuth::default()));
    *f.token_reply.lock().unwrap() = None;
    let state = f.start().await;
    assert!(f.callback(&state).await.contains("notes conectado"));
    assert_eq!(f.registration_frames.lock().unwrap().len(), 2);
    market::update_connectors(&f.hub.store, |all| {
        all[0].headers.insert("x-change".into(), "new".into());
        Ok(())
    })
    .unwrap();
    assert!(issuer(&f).is_none());
    assert_eq!(f.saved().oauth, Some(oauth::OAuth::default()));
}

#[tokio::test]
async fn m3f_dcr_em_voo_revalida_authority_e_cas_antes_de_persistir_ou_publicar() {
    use crate::{
        api::devices::Caller,
        store::{Device, DeviceSource, Scope},
    };
    // Cabeçalhos: Authority revogada/recriada. Corpo JSON: outra intenção ganha CAS.
    for body_window in [false, true] {
        let a = Fixture::new().await;
        let b = Fixture::new().await;
        a.hub.store.kv_set("test_cloud_url", "https://fixture.invalid").unwrap();
        let device = Device {
            key: "m3f-operator".into(),
            name: "fixture".into(),
            platform: "fixture".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: 1,
            last_seen_at: None,
        };
        a.hub.store.put_device(&device).unwrap();
        secret(&a, "race-a");
        let state = a.start().await;
        assert!(a.callback(&state).await.contains("notes conectado"));
        *a.issuer.lock().unwrap() = b.provider.clone();
        secret(&b, "race-old-b");
        let gate = if body_window { &b.registration_body } else { &b.registration };
        gate.arm();
        let call = {
            let a = a.clone();
            tokio::spawn(async move {
                api::dispatch(
                    &a.hub,
                    &Caller::Device { key: "m3f-operator".into(), scopes: vec![Scope::Control], autoridade: None },
                    "connectorSignIn",
                    json!({"id":"notes"}),
                )
                .await
            })
        };
        gate.wait().await;
        if body_window {
            secret(&b, "race-current-b");
            let state = a.start().await;
            assert!(a.callback(&state).await.contains("notes conectado"));
        } else {
            a.hub.store.remove_device(&device.key).unwrap();
            a.hub.store.put_device(&device).unwrap();
        }
        let before = a.saved();
        let raw = a.hub.store.kv_read("connectors").unwrap();
        gate.open();
        assert!(call.await.unwrap().is_err());
        assert_eq!(a.saved(), before);
        assert_eq!(a.hub.store.kv_read("connectors").unwrap(), raw);
        sem_pending(&a);
        if body_window {
            assert_eq!(issuer(&a).as_deref(), Some(b.provider.as_str()));
            assert_eq!(a.saved().oauth.as_ref().unwrap().client_secret.as_deref(), Some("race-current-b"));
        } else {
            assert_eq!(issuer(&a).as_deref(), Some(a.provider.as_str()));
            assert_eq!(b.token_calls.load(Ordering::SeqCst), 0);
        }
        cadastro_limpo(&b, "race-a");
    }
}

#[tokio::test]
async fn m3f_invalid_client_no_callback_apaga_cadastro_e_selo_antes_do_relogin() {
    let f = Fixture::new().await;
    secret(&f, "revoked-client-secret");
    let state = f.start().await;
    f.reply(401, json!({"error":"invalid_client"}));
    assert!(!f.callback(&state).await.contains("notes conectado"));
    assert!(issuer(&f).is_none());
    assert_eq!(f.saved().oauth, Some(oauth::OAuth::default()));
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 1, "código não é repetido");
    assert!(!f.callback(&state).await.contains("notes conectado"));
    assert_eq!(f.token_calls.load(Ordering::SeqCst), 1);
    *f.token_reply.lock().unwrap() = None;
    secret(&f, "replacement-client-secret");
    let state = f.start().await;
    assert_eq!(f.registration_frames.lock().unwrap().len(), 2, "recadastro exigido");
    assert!(f.callback(&state).await.contains("notes conectado"));
    assert_eq!(f.saved().oauth.as_ref().unwrap().client_secret.as_deref(), Some("replacement-client-secret"));
    assert_eq!(issuer(&f).as_deref(), Some(f.provider.as_str()));
}

#[tokio::test]
async fn m3f_invalid_client_antigo_no_callback_nao_apaga_new_nem_ignora_origin() {
    use crate::{
        api::devices::Caller,
        store::{Device, DeviceSource, Scope},
    };
    for revoke in [false, true] {
        let f = Fixture::new().await;
        f.hub.store.kv_set("test_cloud_url", "https://fixture.invalid").unwrap();
        let device = Device {
            key: "callback-operator".into(),
            name: "fixture".into(),
            platform: "fixture".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: 1,
            last_seen_at: None,
        };
        f.hub.store.put_device(&device).unwrap();
        let caller = || Caller::Device { key: device.key.clone(), scopes: vec![Scope::Control], autoridade: None };
        let started = api::dispatch(&f.hub, &caller(), "connectorSignIn", json!({"id":"notes"})).await.unwrap();
        let link = reqwest::Url::parse(started["url"].as_str().unwrap()).unwrap();
        let state = link.query_pairs().find(|(k, _)| k == "state").unwrap().1.into_owned();
        f.reply(401, json!({"error":"invalid_client"}));
        f.token.arm();
        let old = {
            let f = f.clone();
            let caller = caller();
            tokio::spawn(async move {
                api::dispatch(&f.hub, &caller, "connectorSignInFinish", json!({"state":state,"code":"old-code"})).await
            })
        };
        f.token.wait().await;
        if revoke {
            f.hub.store.remove_device(&device.key).unwrap();
            f.hub.store.put_device(&device).unwrap();
        } else {
            *f.token_reply.lock().unwrap() = None;
            let state = f.start().await;
            assert!(f.callback(&state).await.contains("notes conectado"));
        }
        let current = f.saved();
        let raw = f.hub.store.kv_read("connectors").unwrap();
        f.token.open();
        assert!(old.await.unwrap().is_err());
        assert_eq!(f.saved(), current);
        assert_eq!(f.hub.store.kv_read("connectors").unwrap(), raw);
        assert_eq!(issuer(&f).as_deref(), Some(f.provider.as_str()));
        sem_pending(&f);
    }
}
