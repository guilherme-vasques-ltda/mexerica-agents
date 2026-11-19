//! M3-E: fronteiras da declaração OAuth, com registro inteiro e HTTP sintético.
use super::*;
use crate::market::oauth;

const M3E_ERRO: &str = crate::fork::conectores::oauth_token_scopes::ERRO;

fn m3e_validation(c: &market::Connector) -> Value {
    serde_json::to_value(c.oauth_binding.as_ref().unwrap()).unwrap()["scopeValidation"].clone()
}

fn m3e_omitir_pedido(f: &Fixture) {
    f.redirects.lock().unwrap().insert(
        "/.well-known/oauth-protected-resource/mcp".into(),
        (
            StatusCode::OK,
            String::new(),
            json!({"resource":format!("{}/mcp",f.provider),
            "authorization_servers":[f.provider],"scopes_supported":[]}),
        ),
    );
}

#[tokio::test]
async fn m3e_login_valida_pedido_antes_do_par_e_target() {
    let f = Fixture::new().await;
    *f.initial.lock().unwrap() = Some((StatusCode::UNAUTHORIZED, vec![r#"Bearer scope="Read write""#.into()]));
    for scope in [
        None,
        Some(json!("write Read Read")),
        Some(json!("Read write extra")),
        Some(json!("Read")),
        Some(json!("alternativo")),
        Some(json!("read write")),
        Some(Value::Null),
        Some(json!(17)),
        Some(json!("")),
        Some(json!(" Read")),
        Some(json!("Read  write")),
        Some(json!("Read\twrite")),
        Some(json!("réad")),
    ] {
        let state = f.start().await;
        let before = f.saved();
        let mut body = json!({"access_token":"m3e-access","refresh_token":"m3e-refresh","expires_in":3600});
        if let Some(scope) = &scope {
            body["scope"] = scope.clone();
        }
        f.reply(200, body);
        let accepted = scope.as_ref().is_none_or(|s| s == "write Read Read");
        let html = f.callback(&state).await;
        assert_eq!(html.contains("notes conectado"), accepted);
        if accepted {
            let saved = f.saved();
            assert_eq!(saved.oauth.as_ref().unwrap().access_token, "m3e-access");
            assert_eq!(saved.oauth.as_ref().unwrap().refresh_token.as_deref(), Some("m3e-refresh"));
            assert!(m3e_validation(&saved)["evidencia"].get("Validado").is_some());
            assert_eq!(f.runtime_target(false).await.unwrap()["headers"]["Authorization"], "Bearer m3e-access");
        } else {
            assert!(html.contains(M3E_ERRO), "{html}");
            assert_eq!(f.saved(), before, "nem parte do par nem metadata pode ser salva");
            assert!(f.runtime_target(false).await.is_err());
        }
    }
}

#[tokio::test]
async fn m3e_omissao_nao_inventa_intencao_e_declarado_estabiliza_refresh() {
    let f = Fixture::new().await;
    m3e_omitir_pedido(&f);
    for explicit in [false, true] {
        *f.token_reply.lock().unwrap() = None;
        let state = f.start().await;
        if explicit {
            f.reply(200, json!({"access_token":"declared","refresh_token":"r","scope":"read"}));
        }
        assert!(f.callback(&state).await.contains("notes conectado"));
        let original = f.saved();
        if explicit {
            assert_eq!(m3e_validation(&original)["evidencia"], json!({"Declarado":["read"]}));
        } else {
            assert!(m3e_validation(&original).is_null());
        }
        f.reply(200, json!({"access_token":"rotated","refresh_token":"r2","scope":"read","expires_in":3600}));
        f.runtime_target(true).await.unwrap();
        assert_eq!(
            f.saved().oauth_binding,
            original.oauth_binding,
            "refresh não promove desconhecido nem muda revision"
        );
        let form = f.token_forms.lock().unwrap().last().unwrap().clone();
        assert!(!reqwest::Url::parse(&format!("http://x/?{form}")).unwrap().query_pairs().any(|(k, _)| k == "scope"));
        f.reply(200, json!({"access_token":"different","refresh_token":"r3","scope":"other"}));
        let result = f.runtime_target(true).await;
        assert_eq!(result.is_err(), explicit, "Declarado limita mudança sem inventar intenção");
    }
}

#[tokio::test]
async fn m3e_refresh_preserva_prova_e_par_rotativo_sem_revogar_contexto() {
    let f = Fixture::new().await;
    let original = f.refresh_ready(Some(oauth::now() + 30)).await;
    let setup = crate::fork::conectores::oauth_binding::issue_setup(&f.hub.store, &original).unwrap();
    let intent =
        crate::fork::conectores::oauth_runtime::RuntimeIntent::capture(&f.hub, &setup.id, "notes", "initialize")
            .unwrap();
    for scope in [None, Some("read read")] {
        let expected = intent.current(&f.hub, &original).unwrap();
        let access = format!("rotating-{}", scope.is_some());
        let mut body = json!({"access_token":access,"refresh_token":"rotated-refresh","expires_in":3600});
        if let Some(scope) = scope {
            body["scope"] = json!(scope);
        }
        f.reply(200, body);
        oauth::target(&f.hub, &expected, &intent, true).await.unwrap().into_test_value().unwrap();
        let saved = f.saved();
        let mut wanted = expected.clone();
        let o = wanted.oauth.as_mut().unwrap();
        o.access_token = access;
        o.refresh_token = Some("rotated-refresh".into());
        o.expires_at = saved.oauth.as_ref().unwrap().expires_at;
        assert!(o.expires_at.unwrap() > oauth::now());
        assert_eq!(saved, wanted, "rotação só muda o par/prazo, não Binding/revision");
        assert_eq!(intent.current(&f.hub, &original).unwrap(), saved, "contexto original continua vivo");
    }
    let adopted = oauth::fresh(&f.hub, &original, original.oauth.as_ref().unwrap(), &intent, false).await.unwrap();
    assert_eq!(f.saved().oauth, Some(adopted), "fotografia com par velho adota rotação da mesma fonte");
}

#[tokio::test]
async fn m3e_refresh_retido_persiste_reopen_bloqueia_target_e_recupera_login() {
    let f = Fixture::with_disk(true).await;
    let original = f.refresh_ready(Some(oauth::now() + 30)).await;
    let setup = crate::fork::conectores::oauth_binding::issue_setup(&f.hub.store, &original).unwrap();
    let intent =
        crate::fork::conectores::oauth_runtime::RuntimeIntent::capture(&f.hub, &setup.id, "notes", "initialize")
            .unwrap();
    let target_a = oauth::target(&f.hub, &original, &intent, false).await;
    // A já materializou um target válido antes da retenção.
    assert!(target_a.is_ok());
    let current = f.saved();
    f.reply(
        200,
        json!({"access_token":"DO-NOT-SAVE","refresh_token":"DO-NOT-SAVE","scope":"read write","expires_in":1}),
    );
    let error = oauth::target(&f.hub, &current, &intent, true).await.err().unwrap();
    assert_eq!(error.to_string(), M3E_ERRO);
    let saved = f.saved();
    let mut expected = serde_json::to_value(&current).unwrap();
    expected["oauthBinding"]["revision"] = json!(expected["oauthBinding"]["revision"].as_u64().unwrap() + 1);
    expected["oauthBinding"]["scopeValidation"]["uso"] = json!("Retido");
    assert_eq!(
        serde_json::to_value(&saved).unwrap(),
        expected,
        "só retenção/revision mudam; par e prova são preservados"
    );
    assert!(
        crate::fork::conectores::oauth_binding::check_runtime(&f.hub, &current, &intent, || Ok(())).is_err(),
        "A em voo perdeu a fonte"
    );
    assert_eq!(saved.public()["auth"], "signedOut");
    assert!(saved.acp(std::path::Path::new("host"), 0).is_none());
    assert!(saved.public().get("oauthBinding").is_none());
    let calls = f.token_calls.load(Ordering::SeqCst);
    assert_eq!(f.runtime_target(false).await.unwrap_err().to_string(), M3E_ERRO);
    assert_eq!(f.token_calls.load(Ordering::SeqCst), calls, "retido não renova nem entrega token antigo");
    // Outra conexão SQLite/Store abre o ciphertext persistido, sem JSON roundtrip como prova.
    let store = Store::open(&f.dir.join("fixture.db")).unwrap();
    *store.secret_key.lock().unwrap() = Some(zeroize::Zeroizing::new(vec![42; 32]));
    assert!(store.kv_read("connectors").unwrap().unwrap().starts_with("vault:v1:"));
    let reopened = Hub::new(store, "fixture".into(), Identity::load_or_create(&f.dir).unwrap(), "fixture".into(), 0);
    let after = market::connectors(&reopened.store).unwrap().remove(0);
    assert_eq!(after, saved);
    let setup2 = crate::fork::conectores::oauth_binding::issue_setup(&reopened.store, &after).unwrap();
    let intent2 =
        crate::fork::conectores::oauth_runtime::RuntimeIntent::capture(&reopened, &setup2.id, "notes", "initialize")
            .unwrap();
    assert_eq!(oauth::target(&reopened, &after, &intent2, false).await.err().unwrap().to_string(), M3E_ERRO);
    drop(setup2);
    drop(reopened);
    *f.token_reply.lock().unwrap() = None;
    let state = f.start().await;
    assert_eq!(f.saved().oauth.as_ref().unwrap().access_token, "");
    assert!(m3e_validation(&f.saved()).is_null(), "nova intenção não herda prova/retenção");
    assert!(f.callback(&state).await.contains("notes conectado"));
    let recovered = f.saved();
    assert_eq!(recovered.public()["auth"], "signedIn");
    assert!(recovered.acp(std::path::Path::new("host"), 0).is_some());
    f.runtime_target(false).await.unwrap();
}

#[tokio::test]
async fn m3e_scope_invalido_retido_e_transitorio_nao_contamina_classify() {
    let f = Fixture::new().await;
    for scope in [Value::Null, json!(""), json!(false), json!(" read"), json!("write")] {
        *f.token_reply.lock().unwrap() = None;
        let original = f.refresh_ready(Some(oauth::now() + 30)).await;
        for (status, error) in
            [(503, "invalid_grant"), (408, "invalid_grant"), (429, "invalid_grant"), (400, "temporarily_unavailable")]
        {
            f.reply(status, json!({"error":error,"scope":scope}));
            f.runtime_target(false).await.unwrap();
            assert_eq!(f.saved(), original, "transitório com scope lixo não retém/revoga");
        }
        f.reply(200, json!({"access_token":"bad","refresh_token":"rotated","scope":scope}));
        assert_eq!(f.runtime_target(false).await.unwrap_err().to_string(), M3E_ERRO);
        let retained = f.saved();
        assert_eq!(retained.oauth, original.oauth);
        assert!(retained.oauth_binding.as_ref().unwrap().scopes_retained());
        assert_eq!(f.runtime_target(false).await.unwrap_err().to_string(), M3E_ERRO);
    }
}

#[tokio::test]
async fn m3e_old_nao_grava_nem_retem_new_e_authority_revogada_nao_aplica_resposta() {
    for mode in 0..3 {
        let f = Fixture::new().await;
        let original = f.refresh_ready(Some(0)).await;
        let setup = crate::fork::conectores::oauth_binding::issue_setup(&f.hub.store, &original).unwrap();
        f.reply(200, json!({"access_token":"old-result","refresh_token":"old-rotation","scope":"extra"}));
        f.token.arm();
        let body = json!({"context":setup.id,"connector":"notes","transport":"http",
            "bridgeId":uuid::Uuid::new_v4().to_string(),"method":"initialize"});
        let call = {
            let f = f.clone();
            tokio::spawn(async move { f.rpc("donnaGuardRuntime", body).await })
        };
        f.token.wait().await;
        if mode == 1 {
            use crate::LockExt;
            f.hub.store.db.locked().execute("DELETE FROM donna_guard_setup WHERE id=?1", [&setup.id]).unwrap();
        } else if mode == 0 {
            *f.token_reply.lock().unwrap() = None;
            let state = f.start().await;
            assert!(f.callback(&state).await.contains("notes conectado"));
        } else {
            market::update_connectors(&f.hub.store, |all| {
                let o = all[0].oauth.as_mut().unwrap();
                o.access_token = "concurrent-access".into();
                o.refresh_token = Some("concurrent-refresh".into());
                o.expires_at = Some(oauth::now() + 3600);
                Ok(())
            })
            .unwrap();
        }
        let expected = f.saved();
        f.token.open();
        let (status, response) = call.await.unwrap();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(response.get("headers").is_none());
        assert_eq!(f.saved(), expected, "OLD/Authority revogada não salva par nem retém NEW");
    }
}

#[tokio::test]
async fn m3e_legado_sem_prova_nao_adota_grant_como_confirmacao() {
    let f = Fixture::new().await;
    for grant in [false, true] {
        *f.token_reply.lock().unwrap() = None;
        f.refresh_ready(Some(0)).await;
        let mut all = serde_json::to_value(market::connectors(&f.hub.store).unwrap()).unwrap();
        let binding = all[0]["oauthBinding"].as_object_mut().unwrap();
        binding.remove("scopeValidation");
        if !grant {
            binding.remove("grant");
        }
        market::vault::write(&f.hub.store, "connectors", &all.to_string()).unwrap();
        let original = f.saved();
        f.reply(
            200,
            json!({"access_token":"legacy-rotated","refresh_token":"r2","scope":"different","expires_in":3600}),
        );
        f.runtime_target(false).await.unwrap();
        assert_eq!(f.saved().oauth_binding, original.oauth_binding);
        assert!(m3e_validation(&f.saved()).is_null(), "Grant.scopes não comprova escopo da concessão inicial");
        let body = f.token_forms.lock().unwrap().last().unwrap().clone();
        let form = reqwest::Url::parse(&format!("http://x/?{body}")).unwrap();
        assert!(!form.query_pairs().any(|(k, _)| k == "scope"));
        assert_eq!(form.query_pairs().any(|(k, _)| k == "resource"), grant);
    }
}
