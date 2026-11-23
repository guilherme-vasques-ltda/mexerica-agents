#![cfg(unix)]
//! Provas do «Entrar com OpenRouter»: URL de autorização com o desafio S256 do vetor do RFC 7636,
//! porta do ouvinte obrigatória, tentativa de um uso só (o `state` errado não derruba o login em
//! andamento), prazo de dez minutos, troca que nunca segue redirecionamento, recusa corpo acima do
//! teto e nunca repete código, verificador ou chave em erro, e o fluxo feliz gravando a chave no
//! cofre pelo caminho da chave colada, sem devolvê-la. A troca usa um servidor local de mentira; a
//! listagem final usa o fornecedor falso do motor. A tentativa e o endereço injetado são globais,
//! então as provas que os tocam correm em série.
use super::entrar_openrouter as login;
use super::*;
use crate::LockExt;
use crate::api::devices::{Caller, permit};
use crate::remote::crypto;
use std::sync::atomic::Ordering;

/// Serializa as provas que leem ou escrevem a tentativa e o endereço injetado.
static SERIE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(target_os = "linux")]
const CHAVE: &str = "chave-ficticia-do-openrouter-123";

#[derive(Clone, Copy, Debug)]
enum Modo {
    Recusa(u16),
    SemChave,
    Gigante,
    Redireciona,
    #[cfg(target_os = "linux")]
    Chave(&'static str),
}

/// Servidor de mentira da troca: registra cada corpo recebido e responde conforme o modo.
async fn servidor_de_troca(
    modo: Arc<std::sync::Mutex<Modo>>,
    corpos: Arc<std::sync::Mutex<Vec<Value>>>,
    seguido: Arc<std::sync::atomic::AtomicBool>,
) -> String {
    use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let alvo = base.clone();
    let router = Router::new()
        .route(
            "/api/v1/auth/keys",
            routing::post(move |Json(corpo): Json<Value>| {
                let (modo, corpos, alvo) = (modo.clone(), corpos.clone(), alvo.clone());
                async move {
                    corpos.lock().unwrap().push(corpo);
                    match *modo.lock().unwrap() {
                        Modo::Recusa(status) => {
                            (StatusCode::from_u16(status).unwrap(), Json(json!({"error": "recusa"}))).into_response()
                        }
                        Modo::SemChave => Json(json!({"note": "sem o campo da chave"})).into_response(),
                        // JSON válido acima do teto: sem o teto o parse passaria e a chave seria recusada
                        // só pelo formato (`chave_invalida`), não pelo tamanho do corpo.
                        Modo::Gigante => {
                            (StatusCode::OK, format!("{{\"key\": \"{}\"}}", "a".repeat(80_000))).into_response()
                        }
                        Modo::Redireciona => {
                            (StatusCode::FOUND, [("location", format!("{alvo}/seguir"))]).into_response()
                        }
                        #[cfg(target_os = "linux")]
                        Modo::Chave(chave) => Json(json!({"key": chave})).into_response(),
                    }
                }
            }),
        )
        .route(
            "/seguir",
            routing::get(move || {
                let seguido = seguido.clone();
                async move {
                    seguido.store(true, Ordering::SeqCst);
                    StatusCode::OK
                }
            }),
        );
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    base
}

/// O Hub da prova do fluxo feliz, num processo à parte com o motor ligado (`test_support::case`
/// reexecuta o teste pelo nome).
#[cfg(target_os = "linux")]
fn hub_de_prova(teste: &str) -> Option<Arc<crate::hub::Hub>> {
    crate::fork::motor_exec::test_support::case(&format!("fork::motor_mexerica::tests_entrar_openrouter::{teste}"))?;
    if !tests::require_motor() {
        return None;
    }
    let dir = crate::store::private_fixture_dir("motor-entrar-openrouter");
    let store = Store::open(&dir.join("test.db")).unwrap();
    *store.secret_key.locked() = Some(zeroize::Zeroizing::new(vec![7; 32]));
    Some(crate::hub::Hub::new(
        store,
        "test".into(),
        crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
        "test".into(),
        1,
    ))
}

#[tokio::test]
async fn url_de_autorizacao_leva_todos_os_parametros_e_o_desafio_do_vetor_do_rfc_7636() {
    let _serie = SERIE.lock().await;
    *login::BASE_EM_TESTE.locked() = None;
    // Vetor do RFC 7636 (apêndice B): verificador → desafio S256.
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    let desafio = crypto::b64(&crypto::sha256(&[verifier.as_bytes()]));
    assert_eq!(desafio, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    let url = reqwest::Url::parse(&login::url_autorizacao(19425, verifier, "estado-ficticio").unwrap()).unwrap();
    assert!(url.as_str().starts_with("https://openrouter.ai/auth?"), "{url}");
    let pares: Vec<(String, String)> = url.query_pairs().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let esperados: Vec<(String, String)> = vec![
        ("callback_url".into(), "http://127.0.0.1:19425/oauth/callback".into()),
        ("code_challenge".into(), desafio),
        ("code_challenge_method".into(), "S256".into()),
        ("state".into(), "estado-ficticio".into()),
        ("key_label".into(), "Mexerica".into()),
    ];
    assert_eq!(pares, esperados);
}

#[tokio::test]
async fn entrar_exige_porta_do_ouvinte_e_guarda_uma_tentativa_nova() {
    let _serie = SERIE.lock().await;
    *login::BASE_EM_TESTE.locked() = None;
    for pedido in [
        json!({"spaceId": crate::fork::spaces::PERSONAL}),
        json!({"loopbackPort": null, "spaceId": crate::fork::spaces::PERSONAL}),
    ] {
        let erro = login::entrar(&pedido).unwrap_err().to_string();
        assert!(erro.starts_with("porta_invalida"), "{erro} para {pedido}");
    }
    for pedido in [
        json!({"loopbackPort": 8123, "spaceId": crate::fork::spaces::PERSONAL}),
        json!({"loopbackPort": "19424", "spaceId": crate::fork::spaces::PERSONAL}),
        json!({"loopbackPort": 65536, "spaceId": crate::fork::spaces::PERSONAL}),
    ] {
        let erro = login::entrar(&pedido).unwrap_err().to_string();
        assert!(erro.starts_with("porta_invalida"), "{erro} para {pedido}");
    }
    let resposta = login::entrar(&json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).unwrap();
    let url = resposta["url"].as_str().unwrap();
    assert!(url.starts_with("https://openrouter.ai/auth?"), "{url}");
    {
        let tentativa = login::TENTATIVA.locked();
        let t = tentativa.as_ref().unwrap();
        assert_eq!(t.porta, 19424);
        assert_eq!(t.verifier.len(), 43, "verificador de 32 bytes em base64url sem preenchimento");
        assert_eq!(t.state.len(), 32);
    }
    // A tentativa nova substitui qualquer pendente.
    login::entrar(&json!({"loopbackPort": 19425, "spaceId": crate::fork::spaces::PERSONAL})).unwrap();
    assert_eq!(login::TENTATIVA.locked().as_ref().unwrap().porta, 19425);
}

#[tokio::test]
async fn state_errado_nao_consome_e_o_certo_consome_uma_vez_so() {
    let _serie = SERIE.lock().await;
    *login::BASE_EM_TESTE.locked() = None;
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    login::entrar(&json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).unwrap();
    let state = crypto::b64(&login::TENTATIVA.locked().as_ref().unwrap().state);
    // Ausente, ilegível ou de outra tentativa: recusa fixa, sem eco e sem consumir.
    let alheio = crypto::b64(&[7u8; 32]);
    for pedido in [
        json!({"spaceId": crate::fork::spaces::PERSONAL}),
        json!({"spaceId": crate::fork::spaces::PERSONAL, "state": "estado-estranho", "code": "x"}),
        json!({"spaceId": crate::fork::spaces::PERSONAL, "state": alheio, "code": "x"}),
        json!({"spaceId": crate::fork::spaces::PERSONAL, "code": "x"}),
    ] {
        let erro = login::concluir(&hub, &pedido).await.unwrap_err().to_string();
        assert!(erro.starts_with("entrada_invalida"), "{erro} para {pedido}");
        assert!(!erro.contains(&state), "o erro repetiu o state: {erro}");
    }
    assert!(login::TENTATIVA.locked().is_some(), "o retorno estranho derrubou o login em andamento");
    // O state certo consome (a troca sem injeção falha fechada) e o segundo uso não passa.
    let acesso = login::concluir(
        &hub,
        &json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": "codigo-ficticio-1"}),
    )
    .await
    .unwrap();
    assert_eq!(acesso, json!({"ok": false, "erro": "sem_resposta"}));
    assert!(login::TENTATIVA.locked().is_none(), "o retorno com o state certo não consumiu a tentativa");
    let erro = login::concluir(
        &hub,
        &json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": "codigo-ficticio-1"}),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(erro.starts_with("entrada_invalida"), "{erro}");
    // O retorno com erro do fornecedor também consome, sem ecoar o motivo recebido.
    login::entrar(&json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).unwrap();
    let state = crypto::b64(&login::TENTATIVA.locked().as_ref().unwrap().state);
    let acesso = login::concluir(
        &hub,
        &json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "erro": "access_denied"}),
    )
    .await
    .unwrap();
    assert_eq!(acesso, json!({"ok": false, "erro": "entrada_negada"}));
    assert!(login::TENTATIVA.locked().is_none());
}

#[tokio::test]
async fn tentativa_expira_em_dez_minutos() {
    let _serie = SERIE.lock().await;
    *login::BASE_EM_TESTE.locked() = None;
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    login::entrar(&json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).unwrap();
    let state = {
        let mut tentativa = login::TENTATIVA.locked();
        let t = tentativa.as_mut().unwrap();
        t.criada_em =
            std::time::Instant::now().checked_sub(login::VALIDADE + std::time::Duration::from_secs(1)).unwrap();
        crypto::b64(&t.state)
    };
    let erro = login::concluir(
        &hub,
        &json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": "codigo-ficticio-1"}),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(erro.starts_with("entrada_invalida"), "{erro}");
}

#[tokio::test]
async fn troca_recusada_nao_repete_codigo_nem_chave_e_nao_segue_redirecionamento() {
    let _serie = SERIE.lock().await;
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    let modo = Arc::new(std::sync::Mutex::new(Modo::Recusa(400)));
    let corpos = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seguido = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let base = servidor_de_troca(modo.clone(), corpos.clone(), seguido.clone()).await;
    *login::BASE_EM_TESTE.locked() = Some(base);
    let codigo = "codigo-ficticio-do-retorno";
    let mut verifiers = Vec::new();
    for (novo, esperado) in [
        (Modo::Recusa(400), json!({"ok": false, "erro": "fornecedor_falhou", "status": 400})),
        (Modo::Recusa(500), json!({"ok": false, "erro": "fornecedor_falhou", "status": 500})),
        (Modo::Redireciona, json!({"ok": false, "erro": "fornecedor_falhou", "status": 302})),
        (Modo::SemChave, json!({"ok": false, "erro": "resposta_invalida"})),
        (Modo::Gigante, json!({"ok": false, "erro": "resposta_invalida"})),
    ] {
        *modo.lock().unwrap() = novo;
        login::entrar(&json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).unwrap();
        let (state, verifier) = {
            let t = login::TENTATIVA.locked();
            let t = t.as_ref().unwrap();
            (crypto::b64(&t.state), t.verifier.to_string())
        };
        verifiers.push(verifier);
        let acesso =
            login::concluir(&hub, &json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": codigo}))
                .await
                .unwrap();
        assert_eq!(acesso, esperado, "modo {novo:?}");
        let texto = acesso.to_string();
        assert!(!texto.contains(codigo), "vazou o código: {texto}");
        assert!(!texto.contains(verifiers.last().unwrap().as_str()), "vazou o verificador: {texto}");
        assert!(login::TENTATIVA.locked().is_none(), "a tentativa tinha de ser consumida");
        assert!(!seguido.load(Ordering::SeqCst), "o redirecionamento foi seguido no modo {novo:?}");
    }
    // Cada troca levou o código, o verificador da tentativa e o método S256 — e nada mais.
    let corpos = corpos.lock().unwrap();
    assert_eq!(corpos.len(), 5);
    for (corpo, verifier) in corpos.iter().zip(&verifiers) {
        assert_eq!(corpo["code"], codigo, "{corpo}");
        assert_eq!(corpo["code_verifier"], verifier.as_str(), "{corpo}");
        assert_eq!(corpo["code_challenge_method"], "S256", "{corpo}");
        assert_eq!(corpo.as_object().unwrap().len(), 3, "{corpo}");
    }
    *login::BASE_EM_TESTE.locked() = None;
}

#[tokio::test]
#[cfg(target_os = "linux")]
async fn fluxo_feliz_troca_o_codigo_e_salva_a_chave_no_cofre_sem_devolve_la() {
    let Some(hub) = hub_de_prova("fluxo_feliz_troca_o_codigo_e_salva_a_chave_no_cofre_sem_devolve_la") else {
        return;
    };
    let modo = Arc::new(std::sync::Mutex::new(Modo::Chave(CHAVE)));
    let corpos = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seguido = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let base = servidor_de_troca(modo, corpos.clone(), seguido).await;
    *login::BASE_EM_TESTE.locked() = Some(base);
    login::GRAVACAO_FAUX.store(true, Ordering::Relaxed);

    let resposta = login::call(
        &hub,
        &Caller::Local,
        "openrouterEntrar",
        json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL}),
    )
    .await
    .unwrap();
    let url = reqwest::Url::parse(resposta["url"].as_str().unwrap()).unwrap();
    let (state, verifier) = {
        let t = login::TENTATIVA.locked();
        let t = t.as_ref().unwrap();
        (crypto::b64(&t.state), t.verifier.to_string())
    };
    // O desafio na URL é o S256 do verificador guardado.
    let pares: Vec<(String, String)> = url.query_pairs().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let desafio = crypto::b64(&crypto::sha256(&[verifier.as_bytes()]));
    assert!(pares.contains(&("code_challenge".into(), desafio)), "{pares:?}");

    let acesso = login::call(
        &hub,
        &Caller::Local,
        "openrouterEntrarConcluir",
        json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": "codigo-ficticio-do-retorno"}),
    )
    .await
    .unwrap();
    assert_eq!(acesso["ok"], true, "{acesso}");
    assert!(acesso["modelos"].as_array().unwrap().iter().any(|m| m["id"] == "faux/faux-1"), "{acesso}");
    let texto = acesso.to_string();
    assert!(!texto.contains(CHAVE), "a chave voltou ao app: {texto}");
    assert_eq!(crate::market::vault::read(&hub.store, &credential_slot("faux")).unwrap().as_deref(), Some(CHAVE));
    let cifrado = hub.store.kv_read(&credential_slot("faux")).unwrap().unwrap();
    assert!(!cifrado.contains(CHAVE), "a chave não está cifrada no banco");
    assert!(login::TENTATIVA.locked().is_none(), "a tentativa tinha de ser consumida");
    let corpos = corpos.lock().unwrap();
    assert_eq!(corpos.len(), 1);
    assert_eq!(corpos[0]["code"], "codigo-ficticio-do-retorno", "{corpos:?}");
    assert_eq!(corpos[0]["code_verifier"], verifier.as_str(), "{corpos:?}");
    assert_eq!(corpos[0]["code_challenge_method"], "S256", "{corpos:?}");

    login::GRAVACAO_FAUX.store(false, Ordering::Relaxed);
    *login::BASE_EM_TESTE.locked() = None;
}

#[tokio::test]
async fn login_do_openrouter_so_no_space_pessoal() {
    let _serie = SERIE.lock().await;
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    // A tentativa é global: começa limpa para provar que a recusa não cria nenhuma.
    *login::TENTATIVA.locked() = None;
    let alheio = json!({"spaceId": "space-alheio"});
    let erro = login::entrar(&json!({"loopbackPort": 19424, "spaceId": "space-alheio"})).unwrap_err().to_string();
    assert!(erro.starts_with("space_invalido"), "{erro}");
    let erro = login::concluir(&hub, &json!({"state": "qualquer", "code": "codigo", "spaceId": "space-alheio"}))
        .await
        .unwrap_err()
        .to_string();
    assert!(erro.starts_with("space_invalido"), "{erro}");
    assert!(login::TENTATIVA.locked().is_none(), "a recusa não guarda tentativa");
    // Sem o campo não presume o Pessoal: recusa fechado.
    let erro = login::entrar(&json!({"loopbackPort": 19424})).unwrap_err().to_string();
    assert!(erro.starts_with("space_invalido"), "{erro}");
    // Com o Space pessoal aberto segue normal.
    let resposta = login::entrar(&json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).unwrap();
    assert!(resposta["url"].as_str().unwrap().starts_with("https://openrouter.ai/auth"));
    let _ = alheio;
}

#[tokio::test]
async fn entrar_com_openrouter_e_so_deste_computador() {
    use crate::store::Scope;
    let aparelho = Caller::Device { key: "k".into(), scopes: vec![Scope::Control, Scope::Screen], autoridade: None };
    for metodo in ENTRAR_METHODS {
        assert!(permit(&Caller::Local, metodo).is_ok());
        assert!(permit(&aparelho, metodo).is_err(), "{metodo} não pode vir de um aparelho pareado");
        assert!(permit(&Caller::Pairing { key: "k".into() }, metodo).is_err());
    }
    // O método em si recusa chamador que não é Local antes de tocar qualquer estado.
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    for metodo in ENTRAR_METHODS {
        let erro = login::call(&hub, &aparelho, metodo, json!({"loopbackPort": 19424})).await.unwrap_err().to_string();
        assert!(erro.starts_with("remote_setup_unavailable"), "{erro}");
    }
}

#[tokio::test]
async fn motor_desligado_recusa_o_login() {
    // No processo de provas a flag do motor fica desligada; ligada, a recusa é a do resto do motor.
    if enabled() {
        return;
    }
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    for metodo in ENTRAR_METHODS {
        let erro =
            login::call(&hub, &Caller::Local, metodo, json!({"loopbackPort": 19424})).await.unwrap_err().to_string();
        assert!(erro.starts_with("motor_desligado"), "{erro}");
    }
}
