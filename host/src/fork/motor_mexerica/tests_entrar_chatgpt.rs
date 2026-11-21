#![cfg(unix)]
//! Provas do «Entrar com ChatGPT»: URL de autorização com o desafio S256 do vetor do RFC 7636 e o
//! caminho exato `/auth/callback` (nunca `localhost`), `prompt=consent` só quando pedido,
//! `agent_name_hint` só no primeiro registro e as dicas só na reautorização; tentativa de um uso só
//! (o `state` errado não derruba o login em andamento) e com prazo de dez minutos; retorno negado
//! sem troca; registro novo sem `client_id` emitido recusado e `client_id` trocado na
//! reautorização recusado; ID token recusado por assinatura, chave, algoritmo, emissor, público,
//! prazo e `nonce`, e conta trocada na reautorização sem trocar credenciais; escopo sem o uso do
//! plano registra conectado sem inferência; fluxo feliz gravando o registro CIFRADO no cofre sem
//! devolver token algum; renovação serializada que grava o refresh novo antes de devolver o
//! access, cada erro que invalida apagando os tokens, `invalid_client` e rede/5xx sem apagar; e o
//! «Sair» revogando e apagando de qualquer jeito, avisando quando a revogação não foi confirmada.
//! Os endereços do emissor viram de um servidor local de mentira; o ID token das provas é assinado
//! por uma chave RSA fictícia gerada só para este arquivo. A tentativa e o endereço injetado são
//! globais, então as provas que os tocam correm em série.
use super::entrar_chatgpt as login;
use super::*;
use crate::LockExt;
use crate::api::devices::{Caller, permit};
use crate::remote::crypto;
use std::sync::atomic::Ordering;

/// Serializa as provas que leem ou escrevem a tentativa e o endereço injetado.
static SERIE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Valores fictícios: nenhum pode aparecer em erro, log nem resposta.
const CODIGO: &str = "codigo-ficticio-do-retorno";
const ACCESS: &str = "access-ficticio-achat-1";
const REFRESH: &str = "refresh-ficticio-achat-1";
const ACCESS_NOVO: &str = "access-ficticio-novo-1";
const REFRESH_NOVO: &str = "refresh-ficticio-novo-1";
const ACCESS_VELHO: &str = "access-ficticio-velho-1";
const REFRESH_VELHO: &str = "refresh-ficticio-velho-1";
const CLIENTE: &str = "oaiapp_teste-1";
const SUB: &str = "sub-ficticio-1";
const ROTULO: &str = "conta@example.invalid";
const ESCOPO_CHEIO: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";

/// Chave RSA efêmera só das provas: gerada na hora pelo `openssl` do sistema, uma por processo de
/// teste, nunca escrita no repositório. O JWKS de mentira publica o módulo e o expoente; os ID
/// tokens são assinados pelo par completo.
fn chave_de_prova() -> &'static (ring::rsa::KeyPair, String, String) {
    use std::process::Command;
    use std::sync::OnceLock;
    static CHAVE: OnceLock<(ring::rsa::KeyPair, String, String)> = OnceLock::new();
    CHAVE.get_or_init(|| {
        let gerada = Command::new("openssl").args(["genrsa", "2048"]).output().expect("openssl para a chave de prova");
        assert!(gerada.status.success(), "geração da chave de prova falhou");
        let caminho = std::env::temp_dir().join(format!("mexerica-chave-de-prova-{}.pem", std::process::id()));
        std::fs::write(&caminho, &gerada.stdout).expect("escrever a chave de prova");
        let rodar = |args: &[&str]| {
            Command::new("openssl")
                .args(["rsa", "-in"])
                .arg(&caminho)
                .args(args)
                .output()
                .expect("openssl para a chave de prova")
        };
        let der = rodar(&["-outform", "DER"]);
        assert!(der.status.success(), "conversão da chave de prova falhou");
        // O `openssl` novo devolve PKCS nº 8 e o antigo PKCS nº 1: os dois formatos servem.
        let par = ring::rsa::KeyPair::from_pkcs8(&der.stdout)
            .or_else(|_| ring::rsa::KeyPair::from_der(&der.stdout))
            .expect("chave de prova gerada legível");
        // Módulo (hexadecimal) e expoente (decimal) do par: o JWKS de mentira publica os dois em base64url.
        let modulo = rodar(&["-noout", "-modulus"]);
        assert!(modulo.status.success(), "leitura do módulo da chave de prova falhou");
        let hex = String::from_utf8(modulo.stdout).expect("módulo legível");
        let hex = hex.trim().strip_prefix("Modulus=").expect("linha do módulo");
        let bytes: Vec<u8> =
            (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
        let texto = rodar(&["-noout", "-text"]);
        assert!(texto.status.success(), "leitura do expoente da chave de prova falhou");
        let texto = String::from_utf8(texto.stdout).expect("texto da chave legível");
        let expoente: u64 = texto
            .lines()
            .find_map(|linha| linha.trim().strip_prefix("publicExponent: "))
            .and_then(|valor| valor.split_whitespace().next())
            .and_then(|valor| valor.parse().ok())
            .expect("expoente da chave de prova");
        let sem_zeros = expoente.to_be_bytes();
        let sem_zeros = sem_zeros.iter().skip_while(|byte| **byte == 0).copied().collect::<Vec<u8>>();
        let _ = std::fs::remove_file(&caminho);
        (par, crypto::b64(&bytes), crypto::b64(&sem_zeros))
    })
}

const KID: &str = "chave-de-prova";

fn agora() -> i64 {
    crate::store::now_ms() / 1000
}

/// JWKS de mentira com a chave efêmera das provas.
fn jwks_de_prova() -> Value {
    let (_, n, e) = chave_de_prova();
    json!({"keys": [{"kty": "RSA", "kid": KID, "n": n, "e": e}]})
}

/// Assina o ID token de mentira com a chave efêmera das provas.
fn id_token_de_prova(cabecalho: &Value, afirmacoes: &Value) -> String {
    let (par, _, _) = chave_de_prova();
    let cab = crypto::b64(&serde_json::to_vec(cabecalho).unwrap());
    let cor = crypto::b64(&serde_json::to_vec(afirmacoes).unwrap());
    let mensagem = format!("{cab}.{cor}");
    let mut assinatura = vec![0; par.public().modulus_len()];
    par.sign(
        &ring::signature::RSA_PKCS1_SHA256,
        &ring::rand::SystemRandom::new(),
        mensagem.as_bytes(),
        &mut assinatura,
    )
    .expect("assinatura de prova");
    format!("{mensagem}.{}", crypto::b64(&assinatura))
}

/// Cabeçalho padrão do ID token de prova: RS256 com a chave conhecida.
fn cabecalho_de_prova() -> Value {
    json!({"alg": "RS256", "kid": KID})
}

/// Afirmacoes padrão do ID token de prova, com o `nonce` da tentativa em curso.
fn afirmacoes_de_prova(nonce: &str, sub: &str) -> Value {
    json!({
        "iss": "https://auth.openai.com",
        "aud": CLIENTE,
        "sub": sub,
        "email": ROTULO,
        "nonce": nonce,
        "exp": agora() + 3600,
    })
}

/// ID token de prova com a assinatura trocada (o primeiro byte da assinatura muda).
fn id_token_com_assinatura_trocada(cabecalho: &Value, afirmacoes: &Value) -> String {
    let token = id_token_de_prova(cabecalho, afirmacoes);
    let (mensagem, assinatura) = token.rsplit_once('.').unwrap();
    let mut chars: Vec<char> = assinatura.chars().collect();
    chars[0] = if chars[0] == 'A' { 'B' } else { 'A' };
    format!("{mensagem}.{}", chars.into_iter().collect::<String>())
}

/// Resposta da troca do código no servidor de mentira.
#[derive(Clone, Debug)]
enum RespostaTroca {
    /// 200 sem os campos obrigatórios.
    Nenhuma,
    Corpo(Value),
    Redireciona,
    /// 200 com corpo acima do teto: resposta completa, só o tamanho a difere da aceita.
    Gigante,
    /// Espera o sinal antes de responder com o corpo dado: para provar corridas com o «Sair» e com
    /// um segundo login durante a troca.
    Pausa(Value),
}

/// Resposta da renovação no servidor de mentira.
#[derive(Clone, Copy)]
enum ModoRenovacao {
    Boa,
    /// 400 com o código de erro dado.
    Erro(&'static str),
    Falha(u16),
}

/// Resposta da revogação no servidor de mentira.
#[derive(Clone, Copy)]
enum ModoRevogacao {
    Confirma,
    Falha(u16),
}

/// O que o servidor de mentira recebeu e como responde.
struct Ficho {
    /// Solta a troca em pausa.
    soltar: tokio::sync::Notify,
    trocas: std::sync::Mutex<Vec<Value>>,
    renovacoes: std::sync::Mutex<Vec<Value>>,
    revogacoes: std::sync::Mutex<Vec<Value>>,
    resposta_troca: std::sync::Mutex<RespostaTroca>,
    renovacao: std::sync::Mutex<ModoRenovacao>,
    revogacao: std::sync::Mutex<ModoRevogacao>,
    seguido: std::sync::atomic::AtomicBool,
}

fn ficho() -> Arc<Ficho> {
    Arc::new(Ficho {
        soltar: tokio::sync::Notify::new(),
        trocas: std::sync::Mutex::new(Vec::new()),
        renovacoes: std::sync::Mutex::new(Vec::new()),
        revogacoes: std::sync::Mutex::new(Vec::new()),
        resposta_troca: std::sync::Mutex::new(RespostaTroca::Nenhuma),
        renovacao: std::sync::Mutex::new(ModoRenovacao::Boa),
        revogacao: std::sync::Mutex::new(ModoRevogacao::Confirma),
        seguido: std::sync::atomic::AtomicBool::new(false),
    })
}

/// Servidor de mentira do emissor: troca, renovação, revogação e lista de chaves.
async fn servidor_de_mentira(registros: Arc<Ficho>) -> String {
    use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing};
    async fn token_endpoint(
        axum::extract::State(registros): axum::extract::State<Arc<Ficho>>,
        axum::extract::Form(form): axum::extract::Form<std::collections::HashMap<String, String>>,
    ) -> impl IntoResponse {
        let grant = form.get("grant_type").cloned().unwrap_or_default();
        if grant == "authorization_code" {
            registros.trocas.lock().unwrap().push(serde_json::to_value(&form).unwrap());
            // A resposta sai da guarda antes do `await` da pausa: a guarda não atravessa um await.
            let resposta = registros.resposta_troca.lock().unwrap().clone();
            match resposta {
                RespostaTroca::Nenhuma => (StatusCode::OK, Json(json!({"note": "sem os campos"}))).into_response(),
                RespostaTroca::Corpo(corpo) => (StatusCode::OK, Json(corpo)).into_response(),
                RespostaTroca::Redireciona => (StatusCode::FOUND, [("location", "/seguir".to_owned())]).into_response(),
                RespostaTroca::Gigante => (
                    StatusCode::OK,
                    Json(json!({
                        "access_token": ACCESS,
                        "refresh_token": REFRESH,
                        "id_token": "id-token-ficticio",
                        "expires_in": 3600,
                        "scope": ESCOPO_CHEIO,
                        "enchimento": "a".repeat(80_000),
                    })),
                )
                    .into_response(),
                RespostaTroca::Pausa(corpo) => {
                    registros.soltar.notified().await;
                    (StatusCode::OK, Json(corpo.clone())).into_response()
                }
            }
        } else if grant == "refresh_token" {
            registros.renovacoes.lock().unwrap().push(serde_json::to_value(&form).unwrap());
            match *registros.renovacao.lock().unwrap() {
                ModoRenovacao::Boa => (
                    StatusCode::OK,
                    Json(json!({
                        "access_token": ACCESS_NOVO,
                        "refresh_token": REFRESH_NOVO,
                        "expires_in": 3600,
                    })),
                )
                    .into_response(),
                ModoRenovacao::Erro(codigo) => {
                    (StatusCode::BAD_REQUEST, Json(json!({"error": codigo}))).into_response()
                }
                ModoRenovacao::Falha(status) => {
                    (StatusCode::from_u16(status).unwrap(), Json(json!({}))).into_response()
                }
            }
        } else {
            (StatusCode::BAD_REQUEST, Json(json!({"error": "grant_desconhecido"}))).into_response()
        }
    }
    async fn revogacao_endpoint(
        axum::extract::State(registros): axum::extract::State<Arc<Ficho>>,
        axum::extract::Form(form): axum::extract::Form<std::collections::HashMap<String, String>>,
    ) -> impl IntoResponse {
        registros.revogacoes.lock().unwrap().push(serde_json::to_value(&form).unwrap());
        match *registros.revogacao.lock().unwrap() {
            ModoRevogacao::Confirma => StatusCode::OK.into_response(),
            ModoRevogacao::Falha(status) => StatusCode::from_u16(status).unwrap().into_response(),
        }
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let jwks = jwks_de_prova();
    let armadilha = registros.clone();
    let router = Router::new()
        .route("/api/accounts/oauth/token", routing::post(token_endpoint).with_state(registros.clone()))
        .route("/api/accounts/oauth/revoke", routing::post(revogacao_endpoint).with_state(registros))
        .route(
            "/.well-known/jwks.json",
            routing::get(move || {
                let jwks = jwks.clone();
                async move { Json(jwks) }
            }),
        )
        .route(
            "/seguir",
            routing::get(move || {
                let armadilha = armadilha.clone();
                async move {
                    armadilha.seguido.store(true, Ordering::SeqCst);
                    StatusCode::OK
                }
            }),
        );
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    base
}

/// Hub de prova com o cofre aberto: a chave do cofre é direto da fixture, sem serviço do sistema.
fn hub_de_prova() -> Arc<crate::hub::Hub> {
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    *hub.store.secret_key.locked() = Some(zeroize::Zeroizing::new(vec![7; 32]));
    hub
}

/// Estado, `nonce` e verificador da tentativa em curso.
fn tentativa_atual() -> (String, String, String) {
    let tentativa = login::TENTATIVA.locked();
    let t = tentativa.as_ref().expect("tentativa em curso");
    (crypto::b64(&t.state), t.nonce.clone(), t.verifier.to_string())
}

/// Grava a conta de prova direto no cofre, como o fluxo feliz faria.
fn semear_conta(hub: &Arc<crate::hub::Hub>, conta: &Value) {
    crate::market::vault::write(&hub.store, login::SLOT_CONTA, &conta.to_string()).unwrap();
}

/// A conta guardada no cofre, como JSON.
fn conta_guardada(hub: &Arc<crate::hub::Hub>) -> Value {
    let texto = crate::market::vault::read(&hub.store, login::SLOT_CONTA).unwrap().expect("conta guardada");
    serde_json::from_str(&texto).unwrap()
}

/// Conta conectada de prova, com validade do access dada em segundos de época.
fn conta_conectada(expira_em: i64) -> Value {
    json!({
        "client_id": CLIENTE,
        "sub": SUB,
        "rotulo": ROTULO,
        "escopos": ESCOPO_CHEIO,
        "access_token": ACCESS_VELHO,
        "refresh_token": REFRESH_VELHO,
        "expira_em": expira_em,
    })
}

/// Pares de consulta da URL, na ordem.
fn pares(url: &str) -> Vec<(String, String)> {
    reqwest::Url::parse(url).unwrap().query_pairs().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// Tudo o que o Hub registrou em log durante `run` (o `tracing` deste executor de prova).
async fn logged<F: Future<Output = ()>>(run: F) -> String {
    #[derive(Clone)]
    struct Buf(Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Buf {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let buf = Buf(Arc::default());
    let writer = buf.clone();
    let subscriber =
        tracing_subscriber::fmt().with_max_level(tracing::Level::TRACE).with_writer(move || writer.clone()).finish();
    let guard = tracing::subscriber::set_default(subscriber);
    run.await;
    drop(guard);
    String::from_utf8_lossy(&buf.0.lock().unwrap().clone()).into_owned()
}

/// URL de autorização do registro novo: todos os parâmetros, o desafio do vetor do RFC 7636 e o
/// caminho exato do retorno; sem dicas e sem `prompt=consent` quando não pedido.
#[tokio::test]
async fn url_de_autorizacao_do_registro_leva_todos_os_parametros_e_o_desafio_do_vetor_do_rfc_7636() {
    let _serie = SERIE.lock().await;
    *login::BASE_EM_TESTE.locked() = None;
    // Vetor do RFC 7636 (apêndice B): verificador → desafio S256.
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    let desafio = crypto::b64(&crypto::sha256(&[verifier.as_bytes()]));
    assert_eq!(desafio, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    let url =
        login::url_autorizacao(19424, verifier, "estado-ficticio", "nonce-ficticio", "host-id-ficticio", None, false)
            .unwrap();
    assert!(url.starts_with("https://auth.openai.com/api/accounts/authorize?"), "{url}");
    assert!(!url.contains("localhost"), "{url}");
    let esperados: Vec<(String, String)> = vec![
        ("client_id".into(), "dynamic_agent_client".into()),
        ("agent_name_hint".into(), "Mexerica".into()),
        ("ext_agent_host_id".into(), "host-id-ficticio".into()),
        ("response_type".into(), "code".into()),
        ("redirect_uri".into(), "http://127.0.0.1:19424/auth/callback".into()),
        ("scope".into(), ESCOPO_CHEIO.into()),
        ("resource".into(), "https://api.openai.com/v1".into()),
        ("state".into(), "estado-ficticio".into()),
        ("nonce".into(), "nonce-ficticio".into()),
        ("code_challenge".into(), desafio),
        ("code_challenge_method".into(), "S256".into()),
    ];
    assert_eq!(pares(&url), esperados);
    // O `prompt=consent` só entra quando é pedido.
    let sem_consentir =
        login::url_autorizacao(19425, verifier, "estado-ficticio", "nonce-ficticio", "host-id-ficticio", None, false)
            .unwrap();
    assert!(!sem_consentir.contains("prompt="), "{sem_consentir}");
    let com_consentir =
        login::url_autorizacao(19425, verifier, "estado-ficticio", "nonce-ficticio", "host-id-ficticio", None, true)
            .unwrap();
    let pares = pares(&com_consentir);
    assert!(pares.contains(&("prompt".into(), "consent".into())), "{pares:?}");
}

/// Reautorização: o `client_id` guardado, sem `agent_name_hint`, com a dica do rótulo; o registro
/// novo não leva dica nenhuma.
#[test]
fn url_de_reautorizacao_usa_o_cliente_guardado_e_a_dica_do_rotulo_somente_nela() {
    let dicas = Some((CLIENTE, Some(ROTULO)));
    let url = login::url_autorizacao(
        19424,
        "verificador-ficticio",
        "estado-ficticio",
        "nonce-ficticio",
        "host-id-ficticio",
        dicas,
        false,
    )
    .unwrap();
    let consultas = pares(&url);
    assert!(consultas.contains(&("client_id".into(), CLIENTE.into())), "{consultas:?}");
    assert!(consultas.contains(&("login_hint".into(), ROTULO.into())), "{consultas:?}");
    // O ID token guardado nunca sai do Hub: a dica é só o rótulo da conta.
    assert!(!consultas.iter().any(|(k, _)| k == "id_token_hint"), "{consultas:?}");
    assert!(!consultas.iter().any(|(k, _)| k == "agent_name_hint"), "{consultas:?}");
    assert!(!consultas.iter().any(|(k, _)| k == "prompt"), "{consultas:?}");
    // Sem rótulo guardado: nenhuma dica.
    let so_token = Some((CLIENTE, None));
    let consultas = pares(
        &login::url_autorizacao(
            19424,
            "verificador-ficticio",
            "estado-ficticio",
            "nonce-ficticio",
            "host-id-ficticio",
            so_token,
            false,
        )
        .unwrap(),
    );
    assert!(!consultas.iter().any(|(k, _)| k == "login_hint"), "{consultas:?}");
}

/// A porta do ouvinte é obrigatória e vem da lista fechada do app; a tentativa guarda `state`,
/// `nonce` e verificador novos, e o identificador do Hub nasce uma vez só e fica em kv.
#[tokio::test]
async fn entrar_exige_porta_do_ouvinte_e_guarda_tentativa_e_host_id_estaveis() {
    let _serie = SERIE.lock().await;
    *login::BASE_EM_TESTE.locked() = None;
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    for pedido in [
        json!({"spaceId": crate::fork::spaces::PERSONAL}),
        json!({"loopbackPort": null, "spaceId": crate::fork::spaces::PERSONAL}),
        json!({"loopbackPort": 8123, "spaceId": crate::fork::spaces::PERSONAL}),
        json!({"loopbackPort": "19424", "spaceId": crate::fork::spaces::PERSONAL}),
        json!({"loopbackPort": 65536, "spaceId": crate::fork::spaces::PERSONAL}),
    ] {
        let erro = login::entrar(&hub, &pedido).await.unwrap_err().to_string();
        assert!(erro.starts_with("porta_invalida"), "{erro} para {pedido}");
    }
    let resposta =
        login::entrar(&hub, &json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).await.unwrap();
    let url = resposta["url"].as_str().unwrap();
    assert!(url.starts_with("https://auth.openai.com/api/accounts/authorize?"), "{url}");
    let (state, nonce, verifier) = tentativa_atual();
    assert_eq!(state.len(), 43, "state de 32 bytes em base64url sem preenchimento");
    assert_eq!(nonce.len(), 43, "nonce de 32 bytes em base64url sem preenchimento");
    assert_eq!(verifier.len(), 43, "verificador de 32 bytes em base64url sem preenchimento");
    // O desafio na URL é o S256 do verificador guardado.
    let desafio = crypto::b64(&crypto::sha256(&[verifier.as_bytes()]));
    assert!(pares(url).contains(&("code_challenge".into(), desafio)));
    // O identificador do Hub: um `urn:uuid:` v4, o mesmo nas duas chamadas.
    let host_id = hub.store.kv_read(login::KV_HOST_ID).unwrap().unwrap();
    assert!(host_id.starts_with("urn:uuid:") && host_id.len() == 45, "{host_id}");
    assert_eq!(host_id.as_bytes()[23], b'4', "versão 4: {host_id}");
    login::entrar(&hub, &json!({"loopbackPort": 19425, "consentir": true, "spaceId": crate::fork::spaces::PERSONAL}))
        .await
        .unwrap();
    assert_eq!(hub.store.kv_read(login::KV_HOST_ID).unwrap().as_deref(), Some(host_id.as_str()));
    assert!(pares(resposta["url"].as_str().unwrap()).iter().any(|(k, _)| k == "ext_agent_host_id"));
    // A tentativa nova substitui qualquer pendente.
    assert_eq!(login::TENTATIVA.locked().as_ref().unwrap().porta, 19425);
}

/// `state` errado não consome a tentativa; o certo consome uma vez só; e a tentativa expira.
#[tokio::test]
async fn state_errado_nao_consome_e_o_certo_consome_uma_vez_so() {
    let _serie = SERIE.lock().await;
    *login::BASE_EM_TESTE.locked() = None;
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    login::entrar(&hub, &json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).await.unwrap();
    let (state, _, _) = tentativa_atual();
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
        &json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": CODIGO, "client_id": CLIENTE}),
    )
    .await
    .unwrap();
    assert_eq!(acesso, json!({"ok": false, "erro": "sem_resposta"}));
    assert!(login::TENTATIVA.locked().is_none(), "o retorno com o state certo não consumiu a tentativa");
    let erro =
        login::concluir(&hub, &json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": CODIGO}))
            .await
            .unwrap_err()
            .to_string();
    assert!(erro.starts_with("entrada_invalida"), "{erro}");
}

#[tokio::test]
async fn tentativa_expira_em_dez_minutos() {
    let _serie = SERIE.lock().await;
    *login::BASE_EM_TESTE.locked() = None;
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    login::entrar(&hub, &json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).await.unwrap();
    let state = {
        let mut tentativa = login::TENTATIVA.locked();
        let t = tentativa.as_mut().unwrap();
        t.criada_em =
            std::time::Instant::now().checked_sub(login::VALIDADE + std::time::Duration::from_secs(1)).unwrap();
        crypto::b64(&t.state)
    };
    let erro =
        login::concluir(&hub, &json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": CODIGO}))
            .await
            .unwrap_err()
            .to_string();
    assert!(erro.starts_with("entrada_invalida"), "{erro}");
}

/// Retorno negado pelo emissor: valida o `state`, encerra e não troca código nenhum.
#[tokio::test]
async fn retorno_negado_valida_o_state_e_nao_troca_o_codigo() {
    let _serie = SERIE.lock().await;
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    let registros = ficho();
    let base = servidor_de_mentira(registros.clone()).await;
    *login::BASE_EM_TESTE.locked() = Some(base);
    login::entrar(&hub, &json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).await.unwrap();
    let (state, _, _) = tentativa_atual();
    let acesso = login::concluir(
        &hub,
        &json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "erro": "access_denied"}),
    )
    .await
    .unwrap();
    assert_eq!(acesso, json!({"ok": false, "erro": "entrada_negada"}));
    // O `state` que não confere também não troca nada.
    login::entrar(&hub, &json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).await.unwrap();
    let alheio = crypto::b64(&[9u8; 32]);
    login::concluir(&hub, &json!({"spaceId": crate::fork::spaces::PERSONAL, "state": alheio, "erro": "access_denied"}))
        .await
        .unwrap_err();
    assert!(registros.trocas.lock().unwrap().is_empty(), "o retorno negado chegou à troca do código");
    *login::BASE_EM_TESTE.locked() = None;
}

/// Registro novo: sem `client_id` emitido, ou sem o prefixo dele, o registro está incompleto e a
/// troca nunca acontece.
#[tokio::test]
async fn registro_novo_sem_cliente_emitido_e_recusado() {
    let _serie = SERIE.lock().await;
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    let registros = ficho();
    let base = servidor_de_mentira(registros.clone()).await;
    *login::BASE_EM_TESTE.locked() = Some(base);
    for retorno in [
        json!({"state": "-", "spaceId": crate::fork::spaces::PERSONAL}),
        json!({"state": "-", "spaceId": crate::fork::spaces::PERSONAL, "client_id": "cliente-sem-prefixo-1"}),
        json!({"state": "-", "spaceId": crate::fork::spaces::PERSONAL, "client_id": Value::Null}),
    ] {
        login::entrar(&hub, &json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).await.unwrap();
        let (state, _, _) = tentativa_atual();
        let mut pedido = retorno;
        pedido["state"] = Value::String(state);
        pedido["code"] = Value::String(CODIGO.to_owned());
        let acesso = login::concluir(&hub, &pedido).await.unwrap();
        assert_eq!(acesso, json!({"ok": false, "erro": "registro_incompleto"}), "{pedido}");
    }
    assert!(registros.trocas.lock().unwrap().is_empty(), "o registro incompleto chegou à troca");
    *login::BASE_EM_TESTE.locked() = None;
}

/// Reautorização com `client_id` diferente do guardado: recusa sem trocar credenciais.
#[tokio::test]
async fn reautorizacao_com_cliente_diferente_e_recusada_sem_trocar_credenciais() {
    let _serie = SERIE.lock().await;
    let hub = hub_de_prova();
    semear_conta(&hub, &conta_conectada(agora() + 3600));
    let registros = ficho();
    let base = servidor_de_mentira(registros.clone()).await;
    *login::BASE_EM_TESTE.locked() = Some(base);
    login::entrar(&hub, &json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).await.unwrap();
    let (state, _, _) = tentativa_atual();
    let acesso =
        login::concluir(&hub, &json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": CODIGO, "client_id": "oaiapp_outro-9"})).await.unwrap();
    assert_eq!(acesso, json!({"ok": false, "erro": "cliente_trocado"}));
    assert!(registros.trocas.lock().unwrap().is_empty(), "o cliente trocado chegou à troca");
    // Nada mudou no cofre.
    assert_eq!(conta_guardada(&hub)["access_token"], ACCESS_VELHO);
    *login::BASE_EM_TESTE.locked() = None;
}

/// ID token de prova do caso dado, com o `nonce` da tentativa em curso.
fn token_do_caso(caso: &str, nonce: &str) -> String {
    match caso {
        "assinatura errada" => id_token_com_assinatura_trocada(&cabecalho_de_prova(), &afirmacoes_de_prova(nonce, SUB)),
        "chave desconhecida" => {
            let mut cab = cabecalho_de_prova();
            cab["kid"] = Value::String("chave-alheia".into());
            id_token_de_prova(&cab, &afirmacoes_de_prova(nonce, SUB))
        }
        "algoritmo none" => {
            let mut cab = cabecalho_de_prova();
            cab["alg"] = Value::String("none".into());
            let token = id_token_de_prova(&cab, &afirmacoes_de_prova(nonce, SUB));
            format!("{}.{}", token.rsplit_once('.').unwrap().0, "AA")
        }
        "algoritmo HS256" => {
            let mut cab = cabecalho_de_prova();
            cab["alg"] = Value::String("HS256".into());
            id_token_de_prova(&cab, &afirmacoes_de_prova(nonce, SUB))
        }
        "emissor errado" => {
            let mut afirm = afirmacoes_de_prova(nonce, SUB);
            afirm["iss"] = Value::String("https://emissor.example.invalid".into());
            id_token_de_prova(&cabecalho_de_prova(), &afirm)
        }
        "publico errado" => {
            let mut afirm = afirmacoes_de_prova(nonce, SUB);
            afirm["aud"] = Value::String("oaiapp_outro-1".into());
            id_token_de_prova(&cabecalho_de_prova(), &afirm)
        }
        "publico extra" => {
            let mut afirm = afirmacoes_de_prova(nonce, SUB);
            afirm["aud"] = json!([CLIENTE, "oaiapp_outro-1"]);
            id_token_de_prova(&cabecalho_de_prova(), &afirm)
        }
        "cliente autorizado alheio" => {
            let mut afirm = afirmacoes_de_prova(nonce, SUB);
            afirm["azp"] = Value::String("oaiapp_outro-1".into());
            id_token_de_prova(&cabecalho_de_prova(), &afirm)
        }
        "vencido" => {
            let mut afirm = afirmacoes_de_prova(nonce, SUB);
            afirm["exp"] = Value::from(agora() - 3600);
            id_token_de_prova(&cabecalho_de_prova(), &afirm)
        }
        "nonce errado" => id_token_de_prova(&cabecalho_de_prova(), &afirmacoes_de_prova("nonce-alheio", SUB)),
        _ => unreachable!("caso desconhecido: {caso}"),
    }
}

/// ID token recusado por: assinatura errada, chave desconhecida, algoritmo `none` e HS256, emissor
/// errado, público errado, vencido e nonce errado. Nada é gravado e nenhum valor sai na resposta.
#[tokio::test]
async fn id_token_recusado_por_assinatura_chave_algoritmo_emissor_publico_prazo_e_nonce() {
    let _serie = SERIE.lock().await;
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    let registros = ficho();
    let base = servidor_de_mentira(registros.clone()).await;
    *login::BASE_EM_TESTE.locked() = Some(base);
    for caso in [
        "assinatura errada",
        "chave desconhecida",
        "algoritmo none",
        "algoritmo HS256",
        "emissor errado",
        "publico errado",
        "publico extra",
        "cliente autorizado alheio",
        "vencido",
        "nonce errado",
    ] {
        login::entrar(&hub, &json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).await.unwrap();
        let (state, nonce, _) = tentativa_atual();
        let token = token_do_caso(caso, &nonce);
        *registros.resposta_troca.lock().unwrap() = RespostaTroca::Corpo(json!({
            "access_token": ACCESS,
            "refresh_token": REFRESH,
            "id_token": token,
            "expires_in": 3600,
            "scope": ESCOPO_CHEIO,
        }));
        let acesso = login::concluir(
            &hub,
            &json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": CODIGO, "client_id": CLIENTE}),
        )
        .await
        .unwrap();
        assert_eq!(acesso, json!({"ok": false, "erro": "id_invalido"}), "caso {caso}");
        let texto = acesso.to_string();
        assert!(!texto.contains(ACCESS), "vazou o access no caso {caso}: {texto}");
        assert!(!texto.contains(REFRESH), "vazou o refresh no caso {caso}: {texto}");
        assert!(!texto.contains(&token), "vazou o id token no caso {caso}: {texto}");
        assert!(hub.store.kv_read(login::SLOT_CONTA).unwrap().is_none(), "o caso {caso} gravou a conta");
    }
    *login::BASE_EM_TESTE.locked() = None;
}

/// A troca que falha não repete código nem verificador, não segue redirecionamento e recusa corpo
/// acima do teto.
#[tokio::test]
async fn troca_que_falha_nao_repete_codigo_nem_verificador_e_nao_segue_redirecionamento() {
    let _serie = SERIE.lock().await;
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    let registros = ficho();
    let base = servidor_de_mentira(registros.clone()).await;
    *login::BASE_EM_TESTE.locked() = Some(base);
    let mut verifiers = Vec::new();
    for (resposta, esperado) in [
        (RespostaTroca::Nenhuma, json!({"ok": false, "erro": "resposta_invalida"})),
        (RespostaTroca::Gigante, json!({"ok": false, "erro": "resposta_invalida"})),
        (RespostaTroca::Redireciona, json!({"ok": false, "erro": "fornecedor_falhou", "status": 302})),
    ] {
        *registros.resposta_troca.lock().unwrap() = resposta.clone();
        login::entrar(&hub, &json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).await.unwrap();
        let (state, _, verifier) = tentativa_atual();
        verifiers.push(verifier);
        let acesso = login::concluir(
            &hub,
            &json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": CODIGO, "client_id": CLIENTE}),
        )
        .await
        .unwrap();
        assert_eq!(acesso, esperado, "resposta {resposta:?}");
        let texto = acesso.to_string();
        assert!(!texto.contains(CODIGO), "vazou o código: {texto}");
        assert!(!texto.contains(verifiers.last().unwrap().as_str()), "vazou o verificador: {texto}");
        assert!(login::TENTATIVA.locked().is_none(), "a tentativa tinha de ser consumida");
    }
    assert!(!registros.seguido.load(Ordering::SeqCst), "o redirecionamento foi seguido");
    // Cada troca levou o código, o verificador da tentativa, o retorno exato e o recurso — e nada mais.
    let trocas = registros.trocas.lock().unwrap();
    assert_eq!(trocas.len(), 3);
    for (corpo, verifier) in trocas.iter().zip(&verifiers) {
        assert_eq!(corpo["grant_type"], "authorization_code", "{corpo}");
        assert_eq!(corpo["client_id"], CLIENTE, "{corpo}");
        assert_eq!(corpo["code"], CODIGO, "{corpo}");
        assert_eq!(corpo["code_verifier"], verifier.as_str(), "{corpo}");
        assert_eq!(corpo["redirect_uri"], "http://127.0.0.1:19424/auth/callback", "{corpo}");
        assert_eq!(corpo["resource"], "https://api.openai.com/v1", "{corpo}");
        assert_eq!(corpo.as_object().unwrap().len(), 6, "{corpo}");
    }
    *login::BASE_EM_TESTE.locked() = None;
}

/// Reautorização com outra conta (sub diferente): recusa sem trocar as credenciais guardadas.
#[tokio::test]
async fn reautorizacao_com_conta_diferente_e_recusada_sem_trocar_credenciais() {
    let _serie = SERIE.lock().await;
    let hub = hub_de_prova();
    semear_conta(&hub, &conta_conectada(agora() + 3600));
    let registros = ficho();
    let base = servidor_de_mentira(registros.clone()).await;
    *login::BASE_EM_TESTE.locked() = Some(base);
    login::entrar(&hub, &json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).await.unwrap();
    let (state, nonce, _) = tentativa_atual();
    let token = id_token_de_prova(&cabecalho_de_prova(), &afirmacoes_de_prova(&nonce, "sub-outro-1"));
    *registros.resposta_troca.lock().unwrap() = RespostaTroca::Corpo(json!({
        "access_token": ACCESS,
        "refresh_token": REFRESH,
        "id_token": token,
        "expires_in": 3600,
        "scope": ESCOPO_CHEIO,
    }));
    let acesso = login::concluir(
        &hub,
        &json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": CODIGO, "client_id": CLIENTE}),
    )
    .await
    .unwrap();
    assert_eq!(acesso, json!({"ok": false, "erro": "conta_diferente"}));
    // As credenciais guardadas continuam as mesmas.
    let conta = conta_guardada(&hub);
    assert_eq!(conta["access_token"], ACCESS_VELHO);
    assert_eq!(conta["refresh_token"], REFRESH_VELHO);
    assert_eq!(conta["sub"], SUB);
    *login::BASE_EM_TESTE.locked() = None;
}

/// Escopo concedido sem o uso do plano: o login fica registrado, conectado, sem inferência.
#[tokio::test]
async fn escopo_sem_uso_do_plano_registra_conectado_sem_inferencia() {
    let _serie = SERIE.lock().await;
    let hub = hub_de_prova();
    let registros = ficho();
    let base = servidor_de_mentira(registros.clone()).await;
    *login::BASE_EM_TESTE.locked() = Some(base);
    login::entrar(&hub, &json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).await.unwrap();
    let (state, nonce, _) = tentativa_atual();
    let token = id_token_de_prova(&cabecalho_de_prova(), &afirmacoes_de_prova(&nonce, SUB));
    *registros.resposta_troca.lock().unwrap() = RespostaTroca::Corpo(json!({
        "access_token": ACCESS,
        "refresh_token": REFRESH,
        "id_token": token,
        "expires_in": 3600,
        "scope": "openid profile email offline_access resource.invoke",
    }));
    let acesso = login::concluir(
        &hub,
        &json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": CODIGO, "client_id": CLIENTE}),
    )
    .await
    .unwrap();
    assert_eq!(acesso["ok"], true, "{acesso}");
    assert_eq!(acesso["conta"]["rotulo"], ROTULO, "{acesso}");
    assert_eq!(acesso["conta"]["planoHabilitado"], false, "{acesso}");
    let conta = conta_guardada(&hub);
    assert!(!conta["escopos"].as_str().unwrap().contains("chatgpt.tokens.use.direct"), "{conta}");
    let estado = login::estado(&hub).await.unwrap();
    assert_eq!(estado["conectado"], true, "{estado}");
    assert_eq!(estado["planoHabilitado"], false, "{estado}");
    assert_eq!(estado["precisaEntrar"], false, "{estado}");
    *login::BASE_EM_TESTE.locked() = None;
}

/// Hub do caso à parte, com o motor ligado (`test_support::case` reexecuta o teste pelo nome):
/// fora dele a prova volta cedo; dentro, a chave do cofre vem direto da fixture.
fn hub_do_caso(teste: &str) -> Option<Arc<crate::hub::Hub>> {
    crate::fork::motor_exec::test_support::case(&format!("fork::motor_mexerica::tests_entrar_chatgpt::{teste}"))?;
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    *hub.store.secret_key.locked() = Some(zeroize::Zeroizing::new(vec![7; 32]));
    Some(hub)
}

/// Fluxo feliz (registro novo), pelos métodos do app com o motor ligado: a resposta não leva token
/// algum, o registro vai CIFRADO ao cofre, o estado e o sair seguem pelo mesmo caminho, e o login
/// seguinte é reautorização com o cliente emitido e as dicas.
#[tokio::test(flavor = "current_thread")]
async fn fluxo_feliz_grava_o_registro_cifrado_sem_devolver_token() {
    let Some(hub) = hub_do_caso("fluxo_feliz_grava_o_registro_cifrado_sem_devolver_token") else {
        return;
    };
    let registros = ficho();
    let base = servidor_de_mentira(registros.clone()).await;
    *login::BASE_EM_TESTE.locked() = Some(base);

    let log = logged(async {
        let resposta = login::call(
            &hub,
            &Caller::Local,
            "chatgptEntrar",
            json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL}),
        )
        .await
        .unwrap();
        let url = resposta["url"].as_str().unwrap();
        let pares = pares(url);
        assert!(pares.contains(&("client_id".into(), "dynamic_agent_client".into())), "{pares:?}");
        assert!(pares.contains(&("agent_name_hint".into(), "Mexerica".into())), "{pares:?}");
        assert!(
            pares.iter().any(|(k, v)| k == "redirect_uri" && v == "http://127.0.0.1:19424/auth/callback"),
            "{pares:?}"
        );
        assert!(pares.iter().any(|(k, v)| k == "scope" && v == ESCOPO_CHEIO), "{pares:?}");
        assert!(!url.contains("localhost"), "{url}");
        assert!(!pares.iter().any(|(k, _)| k == "id_token_hint" || k == "login_hint"), "{pares:?}");
        let (state, nonce, verifier) = tentativa_atual();
        let token = id_token_de_prova(&cabecalho_de_prova(), &afirmacoes_de_prova(&nonce, SUB));
        *registros.resposta_troca.lock().unwrap() = RespostaTroca::Corpo(json!({
            "access_token": ACCESS,
            "refresh_token": REFRESH,
            "id_token": token,
            "expires_in": 3600,
            "scope": ESCOPO_CHEIO,
        }));
        let acesso = login::call(
            &hub,
            &Caller::Local,
            "chatgptEntrarConcluir",
            json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": CODIGO, "client_id": CLIENTE}),
        )
        .await
        .unwrap();
        assert_eq!(acesso, json!({"ok": true, "conta": {"rotulo": ROTULO, "planoHabilitado": true}}), "{acesso}");
        let texto = acesso.to_string();
        assert!(!texto.contains(ACCESS), "vazou o access: {texto}");
        assert!(!texto.contains(REFRESH), "vazou o refresh: {texto}");
        assert!(!texto.contains(&token), "vazou o id token: {texto}");
        assert!(!texto.contains(CODIGO), "vazou o código: {texto}");
        assert!(!texto.contains(&verifier), "vazou o verificador: {texto}");
        // O registro foi gravado cifrado, com a identidade validada.
        let conta = conta_guardada(&hub);
        assert_eq!(conta["client_id"], CLIENTE, "{conta}");
        assert_eq!(conta["sub"], SUB, "{conta}");
        assert_eq!(conta["rotulo"], ROTULO, "{conta}");
        assert_eq!(conta["access_token"], ACCESS, "{conta}");
        assert_eq!(conta["refresh_token"], REFRESH, "{conta}");
        let cifrado = hub.store.kv_read(login::SLOT_CONTA).unwrap().unwrap();
        assert!(!cifrado.contains(ACCESS), "o access não está cifrado no banco");
        assert!(!cifrado.contains(REFRESH), "o refresh não está cifrado no banco");
        // A troca levou o verificador da tentativa.
        let trocas = registros.trocas.lock().unwrap();
        assert_eq!(trocas.len(), 1);
        assert_eq!(trocas[0]["code_verifier"], verifier.as_str(), "{:?}", trocas[0]);
    })
    .await;
    // Nada de segredo em log.
    assert!(!log.contains(ACCESS), "vazou o access em log: {log}");
    assert!(!log.contains(REFRESH), "vazou o refresh em log: {log}");
    assert!(!log.contains(CODIGO), "vazou o código em log: {log}");
    // O estado e o sair seguem pelo mesmo caminho de método.
    let estado = login::call(&hub, &Caller::Local, "chatgptEstado", json!({})).await.unwrap();
    assert_eq!(estado["conectado"], true, "{estado}");
    assert_eq!(estado["planoHabilitado"], true, "{estado}");
    assert_eq!(estado["rotulo"], ROTULO, "{estado}");
    let sair = login::call(&hub, &Caller::Local, "chatgptSair", json!({})).await.unwrap();
    assert_eq!(sair, json!({"ok": true, "revogado": true}));
    let conta = conta_guardada(&hub);
    assert_eq!(conta["access_token"], Value::Null, "{conta}");
    assert_eq!(conta["refresh_token"], Value::Null, "{conta}");
    assert_eq!(conta["client_id"], CLIENTE, "{conta}");
    // O login seguinte é reautorização: cliente emitido e o e-mail guardado, sem o nome do agente
    // e sem o id_token_hint, apagado no sair.
    let resposta = login::call(
        &hub,
        &Caller::Local,
        "chatgptEntrar",
        json!({"loopbackPort": 19425, "spaceId": crate::fork::spaces::PERSONAL}),
    )
    .await
    .unwrap();
    let pares = pares(resposta["url"].as_str().unwrap());
    assert!(pares.contains(&("client_id".into(), CLIENTE.into())), "{pares:?}");
    assert!(!pares.iter().any(|(k, _)| k == "id_token_hint"), "{pares:?}");
    assert!(pares.contains(&("login_hint".into(), ROTULO.into())), "{pares:?}");
    assert!(!pares.iter().any(|(k, _)| k == "agent_name_hint"), "{pares:?}");
    *login::BASE_EM_TESTE.locked() = None;
}

/// Renovação: grava o refresh novo antes de devolver o access, e duas renovações simultâneas
/// viram UMA chamada ao emissor, com o refresh antigo no pedido.
#[tokio::test]
async fn renovacao_grava_o_refresh_novo_antes_de_devolver_e_uma_so_vez() {
    let _serie = SERIE.lock().await;
    let hub = hub_de_prova();
    let registros = ficho();
    let base = servidor_de_mentira(registros.clone()).await;
    *login::BASE_EM_TESTE.locked() = Some(base);
    // Faltam menos de cinco minutos: renova.
    semear_conta(&hub, &conta_conectada(agora() + 60));
    let (uma, outra) = tokio::join!(login::token_fresco(&hub), login::token_fresco(&hub));
    assert_eq!(uma.unwrap().as_deref().map(String::as_str), Some(ACCESS_NOVO));
    assert_eq!(outra.unwrap().as_deref().map(String::as_str), Some(ACCESS_NOVO));
    // Uma chamada só ao emissor, com o refresh antigo, sem `scope` no pedido.
    {
        let renovacoes = registros.renovacoes.lock().unwrap();
        assert_eq!(renovacoes.len(), 1, "as duas renovações viraram mais de uma chamada");
        assert_eq!(renovacoes[0]["grant_type"], "refresh_token", "{:?}", renovacoes[0]);
        assert_eq!(renovacoes[0]["client_id"], CLIENTE, "{:?}", renovacoes[0]);
        assert_eq!(renovacoes[0]["refresh_token"], REFRESH_VELHO, "{:?}", renovacoes[0]);
        assert_eq!(renovacoes[0]["resource"], "https://api.openai.com/v1", "{:?}", renovacoes[0]);
        assert_eq!(renovacoes[0].as_object().unwrap().len(), 4, "{:?}", renovacoes[0]);
    }
    // O refresh novo já está guardado quando o access volta.
    let conta = conta_guardada(&hub);
    assert_eq!(conta["refresh_token"], REFRESH_NOVO, "{conta}");
    assert_eq!(conta["access_token"], ACCESS_NOVO, "{conta}");
    // A resposta da renovação não declara `scope`: o plano fica desabilitado até uma nova entrada
    // reconfirmar os escopos com o emissor.
    let estado = login::estado(&hub).await.unwrap();
    assert_eq!(estado["conectado"], true, "{estado}");
    assert_eq!(estado["planoHabilitado"], false, "{estado}");
    // Longe do prazo, renovação nenhuma.
    semear_conta(&hub, &conta_conectada(agora() + 3600));
    assert_eq!(login::token_fresco(&hub).await.unwrap().as_deref().map(String::as_str), Some(ACCESS_VELHO));
    assert_eq!(registros.renovacoes.lock().unwrap().len(), 1, "renovou sem estar perto do prazo");
    *login::BASE_EM_TESTE.locked() = None;
}

/// Cada erro que invalida o refresh apaga os tokens e marca «precisa entrar de novo», conservando
/// `client_id`, identidade e rótulo.
#[tokio::test]
async fn renovacao_com_erro_que_invalida_apaga_os_tokens_e_marca_precisa_entrar() {
    let _serie = SERIE.lock().await;
    let hub = hub_de_prova();
    let registros = ficho();
    let base = servidor_de_mentira(registros.clone()).await;
    *login::BASE_EM_TESTE.locked() = Some(base);
    for codigo in [
        "invalid_grant",
        "invalid_refresh_token",
        "token_expired",
        "refresh_token_expired",
        "refresh_token_invalidated",
        "refresh_token_reused",
    ] {
        semear_conta(&hub, &conta_conectada(agora() + 60));
        *registros.renovacao.lock().unwrap() = ModoRenovacao::Erro(codigo);
        let fresco = login::token_fresco(&hub).await.unwrap();
        assert!(fresco.is_none(), "o erro {codigo} devolveu um access");
        let conta = conta_guardada(&hub);
        assert_eq!(conta["access_token"], Value::Null, "o erro {codigo} não apagou o access: {conta}");
        assert_eq!(conta["refresh_token"], Value::Null, "o erro {codigo} não apagou o refresh: {conta}");
        assert_eq!(conta["id_token"], Value::Null, "o erro {codigo} não apagou o id token: {conta}");
        assert_eq!(conta["client_id"], CLIENTE, "o erro {codigo} derrubou o client_id: {conta}");
        assert_eq!(conta["sub"], SUB, "o erro {codigo} derrubou a identidade: {conta}");
        assert_eq!(conta["rotulo"], ROTULO, "o erro {codigo} derrubou o rótulo: {conta}");
        let estado = login::estado(&hub).await.unwrap();
        assert_eq!(estado["conectado"], false, "o erro {codigo}: {estado}");
        assert_eq!(estado["precisaEntrar"], true, "o erro {codigo}: {estado}");
        assert_eq!(estado["rotulo"], ROTULO, "o erro {codigo}: {estado}");
        assert_eq!(estado["planoHabilitado"], false, "o erro {codigo}: {estado}");
    }
    *login::BASE_EM_TESTE.locked() = None;
}

/// `invalid_client` e rede/5xx: erro explícito sem apagar nada.
#[tokio::test]
async fn renovacao_com_invalid_client_ou_rede_nao_apaga_nada() {
    let _serie = SERIE.lock().await;
    let hub = hub_de_prova();
    let registros = ficho();
    let base = servidor_de_mentira(registros.clone()).await;
    *login::BASE_EM_TESTE.locked() = Some(base);
    // 5xx do emissor: erro, tokens intactos, o estado segue conectado.
    semear_conta(&hub, &conta_conectada(agora() + 60));
    *registros.renovacao.lock().unwrap() = ModoRenovacao::Falha(500);
    let erro = login::token_fresco(&hub).await.unwrap_err().to_string();
    assert!(erro.starts_with("renovacao_falhou"), "{erro}");
    let conta = conta_guardada(&hub);
    assert_eq!(conta["access_token"], ACCESS_VELHO, "{conta}");
    assert_eq!(conta["refresh_token"], REFRESH_VELHO, "{conta}");
    let estado = login::estado(&hub).await.unwrap();
    assert_eq!(estado["conectado"], true, "o 5xx apagou a sessão: {estado}");
    // `invalid_client`: erro explícito, sem apagar.
    semear_conta(&hub, &conta_conectada(agora() + 60));
    *registros.renovacao.lock().unwrap() = ModoRenovacao::Erro("invalid_client");
    let erro = login::token_fresco(&hub).await.unwrap_err().to_string();
    assert!(erro.contains("invalid_client"), "{erro}");
    let conta = conta_guardada(&hub);
    assert_eq!(conta["access_token"], ACCESS_VELHO, "{conta}");
    assert_eq!(conta["refresh_token"], REFRESH_VELHO, "{conta}");
    // Sem injeção, a renovação falha fechada (rede): erro sem apagar nada.
    *login::BASE_EM_TESTE.locked() = None;
    semear_conta(&hub, &conta_conectada(agora() + 60));
    let erro = login::token_fresco(&hub).await.unwrap_err().to_string();
    assert!(erro.starts_with("renovacao_falhou"), "{erro}");
    let conta = conta_guardada(&hub);
    assert_eq!(conta["access_token"], ACCESS_VELHO, "{conta}");
    assert_eq!(conta["refresh_token"], REFRESH_VELHO, "{conta}");
    // O estado devolve sem apagar quando a renovação falha por rede.
    let estado = login::estado(&hub).await.unwrap();
    assert_eq!(estado["conectado"], true, "{estado}");
}

/// Sair: revoga o refresh e apaga os tokens de qualquer jeito, conservando `client_id`,
/// identidade e rótulo; sem confirmação, a tela é avisada.
#[tokio::test]
async fn sair_revoga_e_apaga_os_tokens_de_qualquer_jeito() {
    let _serie = SERIE.lock().await;
    let hub = hub_de_prova();
    let registros = ficho();
    let base = servidor_de_mentira(registros.clone()).await;
    *login::BASE_EM_TESTE.locked() = Some(base);
    // Revogação confirmada: tokens apagados, registro conservado.
    semear_conta(&hub, &conta_conectada(agora() + 3600));
    *registros.revogacao.lock().unwrap() = ModoRevogacao::Confirma;
    let sair = login::sair(&hub).await.unwrap();
    assert_eq!(sair, json!({"ok": true, "revogado": true}));
    {
        let revogacoes = registros.revogacoes.lock().unwrap();
        assert_eq!(revogacoes.len(), 1);
        assert_eq!(revogacoes[0]["token"], REFRESH_VELHO, "{:?}", revogacoes[0]);
        assert_eq!(revogacoes[0]["token_type_hint"], "refresh_token", "{:?}", revogacoes[0]);
        assert_eq!(revogacoes[0]["client_id"], CLIENTE, "{:?}", revogacoes[0]);
    }
    let conta = conta_guardada(&hub);
    assert_eq!(conta["access_token"], Value::Null, "{conta}");
    assert_eq!(conta["refresh_token"], Value::Null, "{conta}");
    assert_eq!(conta["client_id"], CLIENTE, "{conta}");
    assert_eq!(conta["sub"], SUB, "{conta}");
    assert_eq!(conta["rotulo"], ROTULO, "{conta}");
    // 5xx: apaga e avisa que a revogação não foi confirmada.
    semear_conta(&hub, &conta_conectada(agora() + 3600));
    *registros.revogacao.lock().unwrap() = ModoRevogacao::Falha(500);
    let sair = login::sair(&hub).await.unwrap();
    assert_eq!(sair, json!({"ok": true, "revogado": false}));
    assert_eq!(conta_guardada(&hub)["refresh_token"], Value::Null);
    // Rede: o mesmo.
    *login::BASE_EM_TESTE.locked() = None;
    semear_conta(&hub, &conta_conectada(agora() + 3600));
    let sair = login::sair(&hub).await.unwrap();
    assert_eq!(sair, json!({"ok": true, "revogado": false}));
    assert_eq!(conta_guardada(&hub)["refresh_token"], Value::Null);
    // Sem sessão: nada a revogar.
    let sair = login::sair(&hub).await.unwrap();
    assert_eq!(sair, json!({"ok": true, "revogado": true}));
}

/// O «Sair» durante a troca (o servidor de mentira segura a resposta) não grava nada depois.
#[tokio::test]
async fn sair_durante_a_troca_em_andamento_nao_grava_depois() {
    let _serie = SERIE.lock().await;
    let hub = hub_de_prova();
    let registros = ficho();
    let base = servidor_de_mentira(registros.clone()).await;
    *login::BASE_EM_TESTE.locked() = Some(base);
    login::entrar(&hub, &json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).await.unwrap();
    let (state, nonce, _) = tentativa_atual();
    *registros.resposta_troca.lock().unwrap() = RespostaTroca::Pausa(json!({
        "access_token": ACCESS,
        "refresh_token": REFRESH,
        "id_token": id_token_de_prova(&cabecalho_de_prova(), &afirmacoes_de_prova(&nonce, SUB)),
        "expires_in": 3600,
        "scope": ESCOPO_CHEIO,
    }));
    let pedido =
        json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": CODIGO, "client_id": CLIENTE});
    let tarefa = tokio::spawn({
        let hub = hub.clone();
        async move { login::concluir(&hub, &pedido).await.unwrap() }
    });
    let prazo = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while registros.trocas.lock().unwrap().is_empty() {
        assert!(std::time::Instant::now() < prazo, "a troca não chegou ao servidor de mentira");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    // O «Sair» de verdade, com a troca em andamento: quem chegou tarde não grava.
    login::sair(&hub).await.unwrap();
    registros.soltar.notify_waiters();
    let resposta = tarefa.await.unwrap();
    assert_eq!(resposta, json!({"ok": false, "erro": "sessao_encerrada"}), "{resposta}");
    assert!(hub.store.kv_read(login::SLOT_CONTA).unwrap().is_none(), "a conta foi gravada depois do «Sair»");
    *login::BASE_EM_TESTE.locked() = None;
}

/// Um segundo login durante a troca do primeiro: a conclusão do primeiro não grava por cima.
#[tokio::test]
async fn segundo_login_durante_a_troca_anula_a_conclusao_do_primeiro() {
    let _serie = SERIE.lock().await;
    let hub = hub_de_prova();
    let registros = ficho();
    let base = servidor_de_mentira(registros.clone()).await;
    *login::BASE_EM_TESTE.locked() = Some(base);
    login::entrar(&hub, &json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).await.unwrap();
    let (state, nonce, _) = tentativa_atual();
    *registros.resposta_troca.lock().unwrap() = RespostaTroca::Pausa(json!({
        "access_token": ACCESS,
        "refresh_token": REFRESH,
        "id_token": id_token_de_prova(&cabecalho_de_prova(), &afirmacoes_de_prova(&nonce, SUB)),
        "expires_in": 3600,
        "scope": ESCOPO_CHEIO,
    }));
    let pedido =
        json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": CODIGO, "client_id": CLIENTE});
    let tarefa = tokio::spawn({
        let hub = hub.clone();
        async move { login::concluir(&hub, &pedido).await.unwrap() }
    });
    let prazo = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while registros.trocas.lock().unwrap().is_empty() {
        assert!(std::time::Instant::now() < prazo, "a troca não chegou ao servidor de mentira");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    // O segundo login troca a tentativa em curso: a conclusão do primeiro não pode mais gravar.
    login::entrar(&hub, &json!({"loopbackPort": 19425, "spaceId": crate::fork::spaces::PERSONAL})).await.unwrap();
    registros.soltar.notify_waiters();
    let resposta = tarefa.await.unwrap();
    assert_eq!(resposta, json!({"ok": false, "erro": "sessao_encerrada"}), "{resposta}");
    assert!(hub.store.kv_read(login::SLOT_CONTA).unwrap().is_none(), "o primeiro login gravou por cima");
    *login::BASE_EM_TESTE.locked() = None;
}

/// «Entrar com outra conta»: ignora o registro guardado e faz o registro dinâmico de novo, sem dica.
#[tokio::test]
async fn entrar_com_outra_conta_ignora_o_registro_guardado() {
    let _serie = SERIE.lock().await;
    *login::BASE_EM_TESTE.locked() = None;
    let hub = hub_de_prova();
    semear_conta(&hub, &conta_conectada(agora() + 3600));
    let resposta = login::entrar(
        &hub,
        &json!({"loopbackPort": 19424, "novaConta": true, "spaceId": crate::fork::spaces::PERSONAL}),
    )
    .await
    .unwrap();
    let consultas = pares(resposta["url"].as_str().unwrap());
    assert!(consultas.contains(&("client_id".into(), "dynamic_agent_client".into())), "{consultas:?}");
    assert!(consultas.contains(&("agent_name_hint".into(), "Mexerica".into())), "{consultas:?}");
    assert!(!consultas.iter().any(|(k, _)| k == "login_hint"), "{consultas:?}");
}

/// O login só acontece com o Space pessoal aberto: sem o campo ou com outro Space, recusa fechado.
#[tokio::test]
async fn login_com_chatgpt_so_no_space_pessoal() {
    let _serie = SERIE.lock().await;
    *login::BASE_EM_TESTE.locked() = None;
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    *login::TENTATIVA.locked() = None;
    for pedido in [json!({"loopbackPort": 19424}), json!({"loopbackPort": 19424, "spaceId": "space-alheio"})] {
        let erro = login::entrar(&hub, &pedido).await.unwrap_err().to_string();
        assert!(erro.starts_with("space_invalido"), "{erro} para {pedido}");
    }
    let erro = login::concluir(&hub, &json!({"state": "qualquer", "code": "codigo"})).await.unwrap_err().to_string();
    assert!(erro.starts_with("space_invalido"), "{erro}");
    assert!(login::TENTATIVA.locked().is_none(), "a recusa não guarda tentativa");
}

/// O «Sair» derruba a entrada pendente: o retorno que chegar depois dele não grava nada.
#[tokio::test]
async fn sair_derruba_a_entrada_pendente() {
    let _serie = SERIE.lock().await;
    *login::BASE_EM_TESTE.locked() = None;
    let hub = hub_de_prova();
    login::entrar(&hub, &json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).await.unwrap();
    let (state, _, _) = tentativa_atual();
    login::sair(&hub).await.unwrap();
    let erro = login::concluir(
        &hub,
        &json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": CODIGO, "client_id": CLIENTE}),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(erro.starts_with("entrada_invalida"), "{erro}");
    assert!(hub.store.kv_read(login::SLOT_CONTA).unwrap().is_none());
}

/// Uma entrada que já estava na rede quando o «Sair» aconteceu não grava nada depois dele.
#[tokio::test]
async fn entrada_em_andamento_nao_grava_depois_do_sair() {
    let _serie = SERIE.lock().await;
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    let registros = ficho();
    let base = servidor_de_mentira(registros.clone()).await;
    *login::BASE_EM_TESTE.locked() = Some(base);
    login::entrar(&hub, &json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL})).await.unwrap();
    let (state, nonce, _) = tentativa_atual();
    *registros.resposta_troca.lock().unwrap() = RespostaTroca::Corpo(json!({
        "access_token": ACCESS,
        "refresh_token": REFRESH,
        "id_token": id_token_de_prova(&cabecalho_de_prova(), &afirmacoes_de_prova(&nonce, SUB)),
        "expires_in": 3600,
        "scope": ESCOPO_CHEIO,
    }));
    login::SESSAO.fetch_add(1, Ordering::SeqCst);
    let resposta = login::concluir(
        &hub,
        &json!({"spaceId": crate::fork::spaces::PERSONAL, "state": state, "code": CODIGO, "client_id": CLIENTE}),
    )
    .await
    .unwrap();
    assert_eq!(resposta, json!({"ok": false, "erro": "sessao_encerrada"}));
    assert!(hub.store.kv_read(login::SLOT_CONTA).unwrap().is_none());
    *login::BASE_EM_TESTE.locked() = None;
}

/// Sem registro: o estado pede entrada, sem rótulo.
#[tokio::test]
async fn estado_sem_registro_pede_entrada() {
    let _serie = SERIE.lock().await;
    *login::BASE_EM_TESTE.locked() = None;
    let hub = hub_de_prova();
    let estado = login::estado(&hub).await.unwrap();
    assert_eq!(estado, json!({"conectado": false, "rotulo": null, "planoHabilitado": false, "precisaEntrar": false}));
}

/// Os métodos são só deste computador: o app pareado e o pareamento não passam, e o motor
/// desligado recusa antes de tocar qualquer estado.
#[tokio::test]
async fn metodos_de_entrar_com_chatgpt_sao_so_deste_computador_e_do_motor_ligado() {
    use crate::store::Scope;
    let _serie = SERIE.lock().await;
    *login::BASE_EM_TESTE.locked() = None;
    let aparelho = Caller::Device { key: "k".into(), scopes: vec![Scope::Control, Scope::Screen], autoridade: None };
    for metodo in CHATGPT_METHODS {
        assert!(permit(&Caller::Local, metodo).is_ok());
        assert!(permit(&aparelho, metodo).is_err(), "{metodo} não pode vir de um aparelho pareado");
        assert!(permit(&Caller::Pairing { key: "k".into() }, metodo).is_err());
    }
    // O método em si recusa chamador que não é Local antes de tocar qualquer estado.
    let hub = crate::fork::conectores::oauth_binding::tests::hub();
    for metodo in CHATGPT_METHODS {
        let erro = login::call(
            &hub,
            &aparelho,
            metodo,
            json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL}),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(erro.starts_with("remote_setup_unavailable"), "{metodo}: {erro}");
    }
    // No processo de provas a flag do motor fica desligada; recusa com o motor desligado.
    if enabled() {
        return;
    }
    for metodo in CHATGPT_METHODS {
        let erro = login::call(
            &hub,
            &Caller::Local,
            metodo,
            json!({"loopbackPort": 19424, "spaceId": crate::fork::spaces::PERSONAL}),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(erro.starts_with("motor_desligado"), "{metodo}: {erro}");
    }
}

/// Controle positivo da validação: um ID token bem assinado, com as afirmações certas, passa.
#[tokio::test]
async fn id_token_bem_assinado_passa_na_conferencia() {
    let _serie = SERIE.lock().await;
    let registros = ficho();
    let base = servidor_de_mentira(registros.clone()).await;
    *login::BASE_EM_TESTE.locked() = Some(base.clone());
    let token = id_token_de_prova(&cabecalho_de_prova(), &afirmacoes_de_prova("nonce-ficticio", SUB));
    let identidade = entrar_chatgpt_jwt::validar(
        &base,
        &token,
        &entrar_chatgpt_jwt::Esperado { client_id: CLIENTE, nonce: "nonce-ficticio" },
    )
    .await
    .unwrap();
    assert_eq!(identidade.sub, SUB);
    assert_eq!(identidade.email.as_deref(), Some(ROTULO));
    *login::BASE_EM_TESTE.locked() = None;
}
