#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::prova::*;
use super::*;
use crate::remote::crypto;
use ed25519_dalek::SigningKey;
use serde_json::json;
use std::sync::Mutex;
use std::time::{Duration, Instant};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn vinculo(raw: &str) -> Vinculo {
    vinculo::interpretar(raw).expect("vínculo de prova")
}

/// Verifica o token como o Hub faz, com a chave do `kid` tirada da JWKS padrão.
fn conferir(token: &str, v: &Vinculo, agora: i64) -> Result<Afirmacoes, Recusa> {
    let partes = token::separar(token)?;
    let jwks: Value = serde_json::from_slice(&jwks_padrao()).unwrap();
    let k =
        jwks["keys"].as_array().unwrap().iter().find(|k| k["kid"] == partes.kid.as_str()).ok_or(Recusa::AuthInvalid)?;
    let chave = ring::signature::RsaPublicKeyComponents {
        n: crypto::unb64(k["n"].as_str().unwrap()).unwrap(),
        e: crypto::unb64(k["e"].as_str().unwrap()).unwrap(),
    };
    token::conferir(&partes, &chave, v, agora)
}

fn pasta() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sessao-clerk-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// O Hub sobre o banco de `dir`: abrir de novo a mesma pasta é um reinício.
fn hub_em(dir: &std::path::Path) -> Arc<Hub> {
    let store = crate::store::Store::open(&dir.join("t.db")).unwrap();
    let identity = crate::remote::identity::Identity::load_or_create(dir).unwrap();
    Hub::new(store, "host".into(), identity, "token".into(), 0)
}

fn hub() -> Arc<Hub> {
    hub_em(&pasta())
}

fn papel_de(hub: &Hub, sub: &str) -> Option<Papel> {
    papel_on(&hub.store.db.locked(), sub).unwrap().and_then(|(_, p)| p)
}

fn epoch_gravado(hub: &Hub) -> u64 {
    crate::fork::workspace::epoch_on(&hub.store.db.locked()).unwrap()
}

fn aparelho(key: &str) -> Device {
    Device {
        key: key.into(),
        name: "Telefone fictício".into(),
        platform: "ios".into(),
        source: crate::store::DeviceSource::Local,
        grant_id: None,
        scopes: vec![crate::store::Scope::Control],
        lease_until: None,
        created_at: now_ms(),
        last_seen_at: None,
    }
}

fn agora() -> i64 {
    now_ms() / 1000
}

#[test]
fn token_valido_passa_e_so_o_sub_e_a_org_saem() {
    let v = vinculo(&vinculo_pessoal());
    let a = conferir(&token(&com_org(afirmacoes(DONO), ORG, "admin")), &v, agora()).unwrap();
    assert_eq!(a.sub, DONO);
    assert_eq!(a.org, Some(token::Org { id: ORG.into(), rol: "admin".into() }));
    assert_eq!(v.papel(&a), Ok(Papel::Dono));
}

#[test]
fn algoritmo_trocado_ou_cabecalho_perigoso_nega_antes_da_chave() {
    let v = vinculo(&vinculo_pessoal());
    let a = afirmacoes(DONO);
    for cab in [
        json!({"alg": "HS256", "kid": KID}),
        json!({"alg": "none", "kid": KID}),
        json!({"alg": "RS512", "kid": KID}),
        json!({"kid": KID}),
        json!({"alg": "RS256"}),
        json!({"alg": "RS256", "kid": ""}),
        json!({"alg": "RS256", "kid": KID, "typ": "at+jwt"}),
        json!({"alg": "RS256", "kid": KID, "jku": "https://outro.example.invalid/jwks"}),
        json!({"alg": "RS256", "kid": KID, "jwk": {"kty": "RSA"}}),
        json!({"alg": "RS256", "kid": KID, "x5u": "https://outro.example.invalid/c"}),
        json!({"alg": "RS256", "kid": KID, "crit": ["exp"]}),
    ] {
        let t = assinar(&cab, &a, &chaves()[0]);
        assert_eq!(token::separar(&t).err(), Some(Recusa::AuthInvalid), "{cab}");
    }
    assert!(token::separar("a.b").is_err());
    assert!(token::separar("a.b.c.d").is_err());
    // Cabeçalho desconhecido do Clerk é ignorado.
    let t = assinar(&json!({"alg": "RS256", "kid": KID, "oiat": 1}), &a, &chaves()[0]);
    assert!(conferir(&t, &v, agora()).is_ok());
}

#[test]
fn assinatura_de_outra_chave_ou_corpo_trocado_nega() {
    let v = vinculo(&vinculo_pessoal());
    let outra = assinar(&cabecalho(), &afirmacoes(DONO), &chaves()[1]);
    assert_eq!(conferir(&outra, &v, agora()).err(), Some(Recusa::AuthInvalid));
    let bom = token(&afirmacoes(DONO));
    let mut partes: Vec<&str> = bom.split('.').collect();
    let trocado = crypto::b64(&serde_json::to_vec(&afirmacoes(OUTRO)).unwrap());
    partes[1] = &trocado;
    assert_eq!(conferir(&partes.join("."), &v, agora()).err(), Some(Recusa::AuthInvalid));
}

#[test]
fn iss_azp_versao_e_estado_da_sessao_errados_negam() {
    let v = vinculo(&vinculo_pessoal());
    let casos: Vec<(&str, Value)> = vec![
        ("iss", json!("https://clerk.test.example.invalid")),
        ("iss", json!(format!("{EMISSOR}/"))),
        ("azp", json!("https://malicioso.example.invalid")),
        ("azp", json!(null)),
        ("sid", json!(null)),
        ("sid", json!("")),
        ("v", json!(1)),
        ("sub", json!("")),
        ("sts", json!("pending")),
        ("act", json!({"sub": "user_impostor"})),
        ("o", json!({"id": ORG})),
    ];
    for (campo, valor) in casos {
        let mut a = afirmacoes(DONO);
        if valor.is_null() {
            a.as_object_mut().unwrap().remove(campo);
        } else {
            a[campo] = valor.clone();
        }
        assert_eq!(conferir(&token(&a), &v, agora()).err(), Some(Recusa::AuthInvalid), "{campo}={valor}");
    }
    let mut ativo = afirmacoes(DONO);
    ativo["sts"] = json!("active");
    assert!(conferir(&token(&ativo), &v, agora()).is_ok());
}

#[test]
fn vida_maior_que_60_s_e_relogio_fora_da_folga_negam() {
    let v = vinculo(&vinculo_pessoal());
    let t0 = agora();
    let com = |iat: i64, nbf: i64, exp: i64| {
        let mut a = afirmacoes(DONO);
        a["iat"] = json!(iat);
        a["nbf"] = json!(nbf);
        a["exp"] = json!(exp);
        token(&a)
    };
    assert_eq!(conferir(&com(t0, t0, t0 + 61), &v, t0).err(), Some(Recusa::AuthInvalid), "exp − iat > 60");
    assert_eq!(conferir(&com(t0, t0, t0), &v, t0).err(), Some(Recusa::AuthInvalid), "exp = iat");
    assert!(conferir(&com(t0, t0, t0 + 60), &v, t0).is_ok());
    // exp: até 5 s depois ainda vale; no 5º segundo, não.
    assert!(conferir(&com(t0, t0, t0 + 60), &v, t0 + 64).is_ok());
    assert_eq!(conferir(&com(t0, t0, t0 + 60), &v, t0 + 65).err(), Some(Recusa::AuthInvalid));
    // nbf e iat no futuro além da folga.
    assert!(conferir(&com(t0 + 5, t0 + 5, t0 + 60), &v, t0).is_ok());
    assert_eq!(conferir(&com(t0, t0 + 6, t0 + 60), &v, t0).err(), Some(Recusa::AuthInvalid), "nbf");
    assert_eq!(conferir(&com(t0 + 6, t0, t0 + 60), &v, t0).err(), Some(Recusa::AuthInvalid), "iat");
    let mut sem_nbf = afirmacoes(DONO);
    sem_nbf.as_object_mut().unwrap().remove("nbf");
    assert_eq!(conferir(&token(&sem_nbf), &v, t0).err(), Some(Recusa::AuthInvalid));
}

#[test]
fn papel_sai_so_do_token_e_org_ou_papel_divergente_nega() {
    let pessoal = vinculo(&vinculo_pessoal());
    let time = vinculo(&vinculo_time());
    let papel = |v: &Vinculo, a: Value| v.papel(&conferir(&token(&a), v, agora()).unwrap());
    assert_eq!(papel(&pessoal, afirmacoes(OUTRO)), Err(Recusa::WorkspaceForbidden), "sub ≠ ownerUserId");
    assert_eq!(papel(&time, afirmacoes(OUTRO)), Err(Recusa::WorkspaceForbidden), "sem org ativa");
    assert_eq!(papel(&time, com_org(afirmacoes(OUTRO), "org_outra", "admin")), Err(Recusa::WorkspaceForbidden));
    assert_eq!(papel(&time, com_org(afirmacoes(OUTRO), ORG, "org:admin")), Err(Recusa::WorkspaceForbidden));
    assert_eq!(papel(&time, com_org(afirmacoes(OUTRO), ORG, "billing")), Err(Recusa::WorkspaceForbidden));
    assert_eq!(papel(&time, com_org(afirmacoes(OUTRO), ORG, "member")), Ok(Papel::Membro));
    assert_eq!(papel(&time, com_org(afirmacoes(OUTRO), ORG, "admin")), Ok(Papel::Admin));
    assert_eq!(papel(&time, com_org(afirmacoes(DONO), ORG, "admin")), Ok(Papel::Dono), "criador admin");
    assert_eq!(papel(&time, com_org(afirmacoes(DONO), ORG, "member")), Ok(Papel::Membro), "criador rebaixado");
    // Campos de identidade no topo do token, fora de `sub` e `o`, não contam.
    let mut forjado = com_org(afirmacoes(OUTRO), ORG, "member");
    forjado["role"] = json!("admin");
    forjado["orgId"] = json!(ORG);
    forjado["userId"] = json!(DONO);
    assert_eq!(papel(&time, forjado), Ok(Papel::Membro));
}

#[test]
fn vinculo_ilegivel_ou_fora_do_formato_nao_vira_vinculo() {
    assert!(vinculo::interpretar(&vinculo_pessoal()).is_some());
    assert!(vinculo::interpretar(&vinculo_time()).is_some());
    let base: Value = serde_json::from_str(&vinculo_pessoal()).unwrap();
    let mudar = |campo: &str, valor: Value| {
        let mut v = base.clone();
        v[campo] = valor;
        vinculo::interpretar(&v.to_string())
    };
    assert!(mudar("schema", json!(2)).is_none());
    assert!(mudar("issuer", json!("http://clerk.example.invalid")).is_none());
    assert!(mudar("jwksUrl", json!("http://clerk.example.invalid/.well-known/jwks.json")).is_none());
    assert!(mudar("jwksUrl", json!("https://outro.example.invalid/.well-known/jwks.json")).is_none());
    assert!(mudar("jwksUrl", json!("https://clerk.example.invalid:8443/jwks")).is_none());
    assert!(mudar("jwksUrl", json!("https://u:p@clerk.example.invalid/jwks")).is_none());
    assert!(mudar("authorizedParties", json!([])).is_none());
    assert!(mudar("authorizedParties", json!([""])).is_none());
    assert!(mudar("orgId", json!(ORG)).is_none(), "pessoal com org");
    assert!(mudar("orgId", json!("")).is_none(), "campo presente e vazio não conta como ausente");
    assert!(mudar("creatorUserId", json!("")).is_none());
    assert!(mudar("ownerUserId", json!("")).is_none());
    assert!(mudar("kind", json!("team")).is_none(), "time sem org");
    assert!(mudar("extra", json!(1)).is_none(), "campo desconhecido");
    assert!(vinculo::interpretar("não é json").is_none());
}

#[test]
fn sem_vinculo_nada_muda_e_negar_sem_vinculo_e_um_interruptor_desligado() {
    const { assert!(!NEGAR_REMOTO_SEM_VINCULO) };
    assert!(matches!(exigencia_com(vinculo::Estado::SemVinculo, false, false), Exigencia::Livre));
    assert!(matches!(
        exigencia_com(vinculo::Estado::SemVinculo, true, false),
        Exigencia::Negar(Recusa::WorkspaceForbidden)
    ));
    assert!(matches!(exigencia_com(vinculo::Estado::Ilegivel, false, false), Exigencia::Negar(Recusa::AuthInvalid)));
    let hub = hub();
    assert!(matches!(exigencia(&hub), Exigencia::Livre));
    vinculo::gravar_para_prova(&hub, "{\"schema\":1}");
    assert!(matches!(exigencia(&hub), Exigencia::Negar(Recusa::AuthInvalid)));
    vinculo::gravar_para_prova(&hub, &vinculo_pessoal());
    assert!(matches!(exigencia(&hub), Exigencia::Sessao(_)));
}

fn url() -> reqwest::Url {
    reqwest::Url::parse(JWKS_URL).unwrap()
}

#[tokio::test]
async fn kid_desconhecido_busca_de_novo_com_limite_e_chave_removida_sai() {
    let j = jwks::Jwks::default();
    let [a, b] = chaves();
    let servido = Arc::new(Mutex::new(jwks(&[jwk(KID, a)])));
    let s = servido.clone();
    j.servir(move || Ok(s.lock().unwrap().clone()));
    let t0 = Instant::now();
    assert!(j.chave(&url(), KID, t0).await.is_ok());
    assert_eq!(j.buscas(), 1);
    assert!(j.chave(&url(), KID, t0 + ms(1)).await.is_ok());
    assert_eq!(j.buscas(), 1, "chave conhecida e JWKS nova: sem rede");
    // kid desconhecido: busca de novo, mas no máximo uma vez a cada 30 s.
    *servido.lock().unwrap() = jwks(&[jwk(KID, a), jwk(KID_2, b)]);
    assert_eq!(j.chave(&url(), "kid-inexistente", t0 + jwks::INTERVALO).await.err(), Some(Recusa::AuthInvalid));
    assert_eq!(j.buscas(), 2);
    assert_eq!(j.chave(&url(), "kid-inexistente", t0 + jwks::INTERVALO + ms(1)).await.err(), Some(Recusa::AuthInvalid));
    assert_eq!(j.buscas(), 2, "limite de frequência");
    assert!(j.chave(&url(), KID_2, t0 + jwks::INTERVALO + ms(2)).await.is_ok(), "veio na última busca");
    // Rotação: a chave removida da JWKS some na renovação seguinte.
    *servido.lock().unwrap() = jwks(&[jwk(KID_2, b)]);
    let depois = t0 + jwks::INTERVALO + jwks::TETO / 2;
    assert!(j.chave(&url(), KID_2, depois).await.unwrap().renovar, "passou da meia-vida");
    j.renovar(&url(), depois).await;
    assert_eq!(j.buscas(), 3, "renovação antes de vencer");
    assert_eq!(j.chave(&url(), KID, depois + ms(1)).await.err(), Some(Recusa::AuthInvalid), "chave removida");
}

#[tokio::test]
async fn jwks_vencida_nega_e_busca_que_falha_nao_estende_a_validade() {
    let j = jwks::Jwks::default();
    let falhar = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let f = falhar.clone();
    j.servir(move || {
        anyhow::ensure!(!f.load(std::sync::atomic::Ordering::SeqCst), "fora do ar");
        Ok(jwks_padrao())
    });
    let t0 = Instant::now();
    let ate = j.chave(&url(), KID, t0).await.unwrap().ate;
    assert_eq!(ate, t0 + jwks::TETO);
    falhar.store(true, std::sync::atomic::Ordering::SeqCst);
    // Renovação falha: segue o conjunto anterior até a validade, nunca além.
    let quase = t0 + jwks::TETO.saturating_sub(ms(1));
    assert!(j.chave(&url(), KID, quase).await.unwrap().renovar);
    j.renovar(&url(), quase).await;
    assert_eq!(j.buscas(), 2, "a renovação tentou e falhou");
    let ate = j.chave(&url(), KID, quase).await.unwrap().ate;
    assert_eq!(ate, t0 + jwks::TETO, "busca que falha não renova");
    assert_eq!(j.chave(&url(), KID, t0 + jwks::TETO).await.err(), Some(Recusa::AuthInvalid));
    falhar.store(false, std::sync::atomic::Ordering::SeqCst);
    let mais_tarde = t0 + jwks::TETO + jwks::INTERVALO;
    let ate = j.chave(&url(), KID, mais_tarde).await.unwrap().ate;
    assert_eq!(ate, mais_tarde + jwks::TETO, "só a busca bem-sucedida conta");
}

#[tokio::test]
async fn validade_da_jwks_vem_do_http_e_sem_frescor_vale_uma_hora() {
    let j = jwks::Jwks::default();
    let frescor = Arc::new(Mutex::new(Duration::from_secs(300)));
    let f = frescor.clone();
    j.servir_com_frescor(move || Ok((jwks_padrao(), *f.lock().unwrap())));
    let t0 = Instant::now();
    assert_eq!(j.chave(&url(), KID, t0).await.unwrap().ate, t0 + Duration::from_secs(300), "max-age abaixo do teto");
    // Vencida a validade, a verificação espera uma busca nova; sem frescor, vale 1 h.
    *frescor.lock().unwrap() = jwks::SEM_FRESCOR;
    let depois = t0 + Duration::from_secs(300);
    assert_eq!(j.chave(&url(), KID, depois).await.unwrap().ate, depois + jwks::SEM_FRESCOR);
    assert_eq!(j.buscas(), 2);
}

#[tokio::test]
async fn renovacao_em_segundo_plano_passada_a_meia_vida_e_falha_nao_estende() {
    let j = jwks::Jwks::default();
    let falhar = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let f = falhar.clone();
    j.servir_com_frescor(move || {
        anyhow::ensure!(!f.load(std::sync::atomic::Ordering::SeqCst), "fora do ar");
        Ok((jwks_padrao(), Duration::from_secs(600)))
    });
    let t0 = Instant::now();
    assert!(!j.chave(&url(), KID, t0).await.unwrap().renovar);
    assert!(!j.chave(&url(), KID, t0 + Duration::from_secs(299)).await.unwrap().renovar, "antes da meia-vida");
    let meia = t0 + Duration::from_secs(300);
    let r = j.chave(&url(), KID, meia).await.unwrap();
    assert!(r.renovar && r.ate == t0 + Duration::from_secs(600), "a verificação não espera a renovação");
    assert_eq!(j.buscas(), 1);
    // Falha em segundo plano: nada muda e a validade não cresce.
    falhar.store(true, std::sync::atomic::Ordering::SeqCst);
    j.renovar(&url(), meia).await;
    assert_eq!(j.buscas(), 2);
    assert_eq!(j.chave(&url(), KID, meia + ms(1)).await.unwrap().ate, t0 + Duration::from_secs(600));
    // Sucesso depois do intervalo: conjunto e validade novos.
    falhar.store(false, std::sync::atomic::Ordering::SeqCst);
    let depois = meia + jwks::INTERVALO;
    j.renovar(&url(), depois).await;
    assert_eq!(j.chave(&url(), KID, depois + ms(1)).await.unwrap().ate, depois + Duration::from_secs(600));
}

#[tokio::test]
async fn quatro_casos_de_frescor_pelo_http() {
    use axum::Router;
    use axum::http::{HeaderValue, StatusCode, header::CACHE_CONTROL};
    use axum::response::IntoResponse;
    use axum::routing::get;
    let corpo = jwks_padrao();
    let com = |cc: Option<&'static str>| {
        let corpo = corpo.clone();
        get(move || async move {
            let mut r = corpo.into_response();
            if let Some(cc) = cc {
                r.headers_mut().insert(CACHE_CONTROL, HeaderValue::from_static(cc));
            }
            r
        })
    };
    let app = Router::new()
        .route("/max-age", com(Some("public, max-age=600")))
        .route("/sem", com(None))
        .route("/no-cache", com(Some("no-cache")))
        .route("/no-store", com(Some("no-store")))
        .route("/falha", get(|| async { StatusCode::SERVICE_UNAVAILABLE }));
    let ouvinte = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let porta = ouvinte.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(ouvinte, app).await.unwrap() });
    let cliente = jwks::cliente(false).unwrap();
    let validade = async |p: &str| {
        let em = reqwest::Url::parse(&format!("http://127.0.0.1:{porta}{p}")).unwrap();
        jwks::buscar_com(&cliente, &em).await.map(|(_, v)| v)
    };
    assert_eq!(validade("/max-age").await.unwrap(), Duration::from_secs(600), "max-age");
    assert_eq!(validade("/sem").await.unwrap(), jwks::SEM_FRESCOR, "sem cabeçalho");
    assert_eq!(validade("/no-cache").await.unwrap(), jwks::SEM_FRESCOR, "no-cache");
    assert_eq!(validade("/no-store").await.unwrap(), jwks::SEM_FRESCOR, "no-store: só em memória");
    assert!(validade("/falha").await.is_err(), "falha não dá validade");
}

#[test]
fn frescor_respeita_max_age_age_e_sem_frescor_vale_uma_hora() {
    use reqwest::header::{AGE, CACHE_CONTROL, HeaderMap, HeaderValue};
    let com = |cc: Option<&'static str>, age: Option<&'static str>| {
        let mut h = HeaderMap::new();
        if let Some(cc) = cc {
            h.insert(CACHE_CONTROL, HeaderValue::from_static(cc));
        }
        if let Some(age) = age {
            h.insert(AGE, HeaderValue::from_static(age));
        }
        jwks::frescor(&h)
    };
    assert_eq!(com(Some("public, max-age=600"), None), Duration::from_secs(600));
    assert_eq!(com(Some("max-age=600"), Some("100")), Duration::from_secs(500));
    assert_eq!(com(Some("max-age=600"), Some("900")), jwks::SEM_FRESCOR, "vencido pelo Age");
    assert_eq!(com(Some("max-age=999999"), None), jwks::TETO, "teto de 24 h");
    for sem in ["no-cache", "no-store, max-age=600", "max-age=0", "public", "max-age=abc", "max-age=1, max-age=2"] {
        assert_eq!(com(Some(sem), None), jwks::SEM_FRESCOR, "{sem}");
    }
    assert_eq!(com(None, None), jwks::SEM_FRESCOR, "sem Cache-Control");
    assert_eq!(com(Some("max-age=600"), Some("x")), jwks::SEM_FRESCOR, "Age ilegível");
    let mut duas = HeaderMap::new();
    duas.insert(CACHE_CONTROL, HeaderValue::from_static("max-age=600"));
    duas.append(AGE, HeaderValue::from_static("100"));
    duas.append(AGE, HeaderValue::from_static("10"));
    assert_eq!(jwks::frescor(&duas), jwks::SEM_FRESCOR, "Age repetido");
}

#[tokio::test]
async fn chave_que_vence_durante_a_espera_e_recusada() {
    let j = jwks::Jwks::default();
    j.servir(|| Ok(jwks_padrao()));
    let t0 = Instant::now();
    assert!(j.chave(&url(), KID, t0).await.is_ok());
    j.vence_em(ms(50));
    tokio::time::sleep(ms(100)).await;
    // `agora` lido antes da espera: o conjunto parece válido, mas vale o relógio monotônico atual.
    assert_eq!(j.chave(&url(), KID, t0).await.err(), Some(Recusa::AuthInvalid));
}

#[tokio::test]
async fn jwks_so_aceita_rsa_de_assinatura_sem_kid_repetido_e_com_teto_de_entradas() {
    let j = jwks::Jwks::default();
    let [a, b] = chaves();
    let mut rs384 = jwk("kid-rs384", a);
    rs384["alg"] = json!("RS384");
    let mut cifra = jwk("kid-enc", a);
    cifra["use"] = json!("enc");
    // O repetido está na primeira e na última das 16 entradas.
    let mut lista = vec![jwk("kid-repetido", a), rs384, cifra, jwk(KID, a)];
    lista.extend((0..11).map(|i| jwk(&format!("kid-extra-{i}"), b)));
    lista.push(jwk("kid-repetido", b));
    assert_eq!(lista.len(), 16);
    let corpo = Arc::new(Mutex::new(jwks(&lista)));
    let c = corpo.clone();
    j.servir(move || Ok(c.lock().unwrap().clone()));
    let t0 = Instant::now();
    assert!(j.chave(&url(), KID, t0).await.is_ok());
    for kid in ["kid-repetido", "kid-rs384", "kid-enc"] {
        assert_eq!(j.chave(&url(), kid, t0 + jwks::INTERVALO * 2).await.err(), Some(Recusa::AuthInvalid), "{kid}");
    }
    // Mais de 16 entradas: o documento inteiro é recusado e o conjunto anterior segue.
    lista.push(jwk(KID_2, b));
    *corpo.lock().unwrap() = jwks(&lista);
    let depois = t0 + jwks::INTERVALO * 4;
    assert_eq!(j.chave(&url(), KID_2, depois).await.err(), Some(Recusa::AuthInvalid));
    assert!(j.chave(&url(), KID, depois + ms(1)).await.is_ok());
}

#[tokio::test]
async fn cliente_da_jwks_nao_segue_redirect_e_tem_teto_de_corpo() {
    use axum::{Router, response::Redirect, routing::get};
    let corpo = jwks_padrao();
    let grande = vec![b' '; 65 * 1024];
    let app = Router::new()
        .route("/jwks", get(move || async move { corpo.clone() }))
        .route("/desvio", get(|| async { Redirect::temporary("/jwks") }))
        .route("/grande", get(move || async move { grande.clone() }));
    let ouvinte = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let porta = ouvinte.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(ouvinte, app).await.unwrap() });
    let cliente = jwks::cliente(false).unwrap();
    let em = |p: &str| reqwest::Url::parse(&format!("http://127.0.0.1:{porta}{p}")).unwrap();
    assert!(jwks::buscar_com(&cliente, &em("/jwks")).await.is_ok());
    assert!(jwks::buscar_com(&cliente, &em("/desvio")).await.is_err(), "redirect não é seguido");
    assert!(jwks::buscar_com(&cliente, &em("/grande")).await.is_err(), "corpo acima do teto");
    // O cliente de produção recusa qualquer coisa que não seja HTTPS.
    assert!(jwks::buscar_com(&jwks::cliente(true).unwrap(), &em("/jwks")).await.is_err());
}

#[test]
fn prova_de_posse_amarra_hub_canal_metodo_tempo_versao_e_nonce_uma_vez() {
    let mut n = posse::Nonces::default();
    let th: [u8; 32] = crypto::random();
    let chave = SigningKey::from_bytes(&crypto::random());
    let dk = crypto::b64(chave.verifying_key().as_bytes());
    let agora = now_ms();
    let p = pop_em(&chave, "hub-a", &th, "hello", agora);
    assert_eq!(n.conferir("hub-a", &th, &dk, "hello", &p, agora), Ok(()));
    assert_eq!(n.conferir("hub-a", &th, &dk, "hello", &p, agora + 1), Err(Recusa::DeviceUnpaired), "replay");
    let p = pop_em(&chave, "hub-a", &th, "hello", agora);
    let recusa = Err(Recusa::DeviceUnpaired);
    assert_eq!(n.conferir("hub-b", &th, &dk, "hello", &p, agora), recusa, "outro Hub");
    assert_eq!(n.conferir("hub-a", &crypto::random(), &dk, "hello", &p, agora), recusa, "outro canal");
    assert_eq!(n.conferir("hub-a", &th, &dk, "send", &p, agora), recusa, "outro método");
    let outra = SigningKey::from_bytes(&crypto::random());
    let dk_outra = crypto::b64(outra.verifying_key().as_bytes());
    assert_eq!(n.conferir("hub-a", &th, &dk_outra, "hello", &p, agora), recusa, "outra chave");
    let velha = pop_em(&chave, "hub-a", &th, "hello", agora - posse::JANELA_MS - 1);
    assert_eq!(n.conferir("hub-a", &th, &dk, "hello", &velha, agora), recusa, "fora da janela");
    let futura = pop_em(&chave, "hub-a", &th, "hello", agora + posse::JANELA_MS + 1);
    assert_eq!(n.conferir("hub-a", &th, &dk, "hello", &futura, agora), recusa, "no futuro");
    assert_eq!(n.conferir("hub-a", &th, &dk, "hello", &json!({}), agora), recusa, "sem prova");
    for v in [json!(null), json!(2), json!("1")] {
        let mut sem_versao = p.clone();
        sem_versao["v"] = v.clone();
        assert_eq!(n.conferir("hub-a", &th, &dk, "hello", &sem_versao, agora), recusa, "v={v}");
    }
    // O nonce recusado por assinatura, canal ou versão não fica gasto.
    assert_eq!(n.conferir("hub-a", &th, &dk, "hello", &p, agora), Ok(()));
}

#[test]
fn nonces_tem_teto_por_canal_e_liberam_com_a_janela() {
    let mut n = posse::Nonces::default();
    let th: [u8; 32] = crypto::random();
    let chave = SigningKey::from_bytes(&crypto::random());
    let dk = crypto::b64(chave.verifying_key().as_bytes());
    let agora = now_ms();
    for _ in 0..posse::MAX_POR_CANAL {
        assert_eq!(n.conferir("h", &th, &dk, "hello", &pop_em(&chave, "h", &th, "hello", agora), agora), Ok(()));
    }
    let cheio = pop_em(&chave, "h", &th, "hello", agora);
    assert_eq!(n.conferir("h", &th, &dk, "hello", &cheio, agora), Err(Recusa::DeviceUnpaired), "teto do canal");
    // Outro canal tem a própria memória.
    let mut outro = posse::Nonces::default();
    assert_eq!(outro.conferir("h", &th, &dk, "hello", &cheio, agora), Ok(()));
    let depois = agora + 2 * posse::JANELA_MS;
    let nova = pop_em(&chave, "h", &th, "hello", depois);
    assert_eq!(n.conferir("h", &th, &dk, "hello", &nova, depois), Ok(()), "a janela libera espaço");
}

#[test]
fn papel_mais_recente_vence_e_token_velho_com_outro_papel_nega() {
    let hub = hub();
    let e0 = epoch_gravado(&hub);
    assert_eq!(registrar_papel(&hub, "u", 100, Some(Papel::Admin)), Ok(()));
    assert_eq!(epoch_gravado(&hub), e0, "primeiro registro não muda ninguém");
    assert_eq!(registrar_papel(&hub, "u", 90, Some(Papel::Admin)), Ok(()), "velho com o mesmo papel");
    assert_eq!(papel_on(&hub.store.db.locked(), "u").unwrap(), Some((100, Some(Papel::Admin))), "o iat não recua");
    assert_eq!(registrar_papel(&hub, "u", 110, Some(Papel::Membro)), Ok(()));
    assert_eq!((epoch_gravado(&hub), hub.workspace.atual()), (e0 + 1, e0 + 1), "troca sobe o epoch no commit");
    assert_eq!(crate::fork::workspace::reject_canal(&hub.workspace.retrato())["stableCode"], "role_changed");
    assert_eq!(papel_de(&hub, "u"), Some(Papel::Membro));
    assert_eq!(registrar_papel(&hub, "u", 105, Some(Papel::Admin)), Err(Recusa::AuthInvalid), "admin velho não volta");
    assert_eq!(registrar_papel(&hub, "u", 110, Some(Papel::Admin)), Err(Recusa::AuthInvalid), "mesmo iat, outro papel");
    assert_eq!(papel_de(&hub, "u"), Some(Papel::Membro));
    assert_eq!(epoch_gravado(&hub), e0 + 1, "recusa não sobe o epoch");
    assert_eq!(registrar_papel(&hub, "u", 120, None), Ok(()), "saiu da org");
    assert_eq!(papel_de(&hub, "u"), None);
    assert_eq!(epoch_gravado(&hub), e0 + 2);
    assert_eq!(crate::fork::workspace::reject_canal(&hub.workspace.retrato())["stableCode"], "workspace_revoked");
    assert_eq!(papel_de(&hub, "outro"), None);
}

#[test]
fn matriz_cobre_todo_metodo_e_nada_fora_dela() {
    use crate::fork::donna_fixa::metodos::METODOS;
    let faltando: Vec<&str> = METODOS.iter().map(|(m, _)| *m).filter(|m| matriz::minimo(m).is_none()).collect();
    assert!(faltando.is_empty(), "método sem entrada na matriz de papéis: {faltando:?}");
    let sobrando: Vec<&str> = matriz::MATRIZ
        .iter()
        .map(|(m, _)| *m)
        .filter(|m| !m.starts_with("sub:") && !METODOS.iter().any(|(x, _)| x == m))
        .collect();
    assert!(sobrando.is_empty(), "entrada sem método: {sobrando:?}");
    let mut vistos = std::collections::HashSet::new();
    assert!(matriz::MATRIZ.iter().all(|(m, _)| vistos.insert(*m)), "entrada repetida");
}

#[test]
fn membro_so_le_conversa_e_grava_o_proprio_e_o_resto_e_admin() {
    use matriz::permitido;
    for m in ["hello", "workspaceContext", "sub:events", "send", "donnaConversationSend", "donnaAppLanguageSave"] {
        assert!(permitido(Papel::Membro, m), "{m}");
    }
    for m in [
        "saveRoutine",
        "answerMemory",
        "respondPermission",
        "runRoutine",
        "createBot",
        "deleteBot",
        "readFile",
        "listDirs",
        "credentialStatus",
        "donnaEngineAccounts",
        "browserStreamSubscribe",
        "screenClose",
    ] {
        assert!(!permitido(Papel::Membro, m), "{m}");
        assert!(permitido(Papel::Admin, m) && permitido(Papel::Dono, m), "{m}");
    }
    for m in [
        "screenPrepare",
        "screenOffer",
        "screenTakeover",
        "executorHello",
        "devices",
        "sub:term",
        "metodoSemClasse",
        "",
    ] {
        for papel in [Papel::Membro, Papel::Admin, Papel::Dono] {
            assert!(!permitido(papel, m), "{m} negado para todos");
        }
    }
}

#[test]
fn papel_antigo_depois_de_reinicio_vale_no_maximo_a_vida_do_token() {
    // O Hub viu o rebaixamento; o registro é durável e vale depois de reiniciar.
    let dir = pasta();
    let antes = hub_em(&dir);
    assert_eq!(registrar_papel(&antes, "u", 90, Some(Papel::Admin)), Ok(()));
    assert_eq!(registrar_papel(&antes, "u", 100, Some(Papel::Membro)), Ok(()));
    assert_eq!(registrar_papel(&antes, "u", 95, Some(Papel::Admin)), Err(Recusa::AuthInvalid));
    let epoch = epoch_gravado(&antes);
    drop(antes);
    let depois = hub_em(&dir);
    assert_eq!(depois.workspace.atual(), epoch, "o epoch da troca também sobrevive");
    assert_eq!(
        registrar_papel(&depois, "u", 95, Some(Papel::Admin)),
        Err(Recusa::AuthInvalid),
        "admin velho não volta"
    );
    assert_eq!(registrar_papel(&depois, "u", 100, Some(Papel::Admin)), Err(Recusa::AuthInvalid), "nem com o mesmo iat");
    assert_eq!(papel_on(&depois.store.db.locked(), "u").unwrap(), Some((100, Some(Papel::Membro))));
    assert_eq!(depois.workspace.atual(), epoch);
    // A janela é a vida do token: 60 s, mais a folga de 5 s; depois disso o token antigo não vale.
    assert_eq!((token::VIDA_MAX_S, token::FOLGA_S), (60, 5));
    let v = vinculo(&vinculo_time());
    let t0 = agora();
    let mut a = com_org(afirmacoes(OUTRO), ORG, "admin");
    a["iat"] = json!(t0);
    a["nbf"] = json!(t0);
    a["exp"] = json!(t0 + token::VIDA_MAX_S);
    let t = token(&a);
    assert!(conferir(&t, &v, t0 + token::VIDA_MAX_S + token::FOLGA_S - 1).is_ok());
    assert_eq!(conferir(&t, &v, t0 + token::VIDA_MAX_S + token::FOLGA_S).err(), Some(Recusa::AuthInvalid));
    a["exp"] = json!(t0 + token::VIDA_MAX_S + 1);
    assert_eq!(conferir(&token(&a), &v, t0).err(), Some(Recusa::AuthInvalid), "vida maior que 60 s");
}

#[test]
fn vinculo_time_fica_desligado_para_uso_e_gravacao() {
    const { assert!(!VINCULO_TIME_LIBERADO) };
    let time = vinculo(&vinculo_time());
    let pessoal = vinculo(&vinculo_pessoal());
    assert_eq!(vinculo::conferir_gravacao(&time, VINCULO_TIME_LIBERADO), Err(Recusa::WorkspaceForbidden));
    assert_eq!(vinculo::conferir_gravacao(&pessoal, VINCULO_TIME_LIBERADO), Ok(()));
    assert_eq!(vinculo::conferir_gravacao(&time, true), Ok(()), "só o interruptor libera");
    // Um vínculo `team` gravado mesmo assim nega todo o remoto; o pessoal segue.
    let hub = hub();
    vinculo::gravar_para_prova(&hub, &vinculo_time());
    assert!(matches!(exigencia(&hub), Exigencia::Negar(Recusa::WorkspaceForbidden)));
    vinculo::gravar_para_prova(&hub, &vinculo_pessoal());
    assert!(matches!(exigencia(&hub), Exigencia::Sessao(_)));
    let time = vinculo::Estado::Vinculado(vinculo(&vinculo_time()));
    assert!(matches!(exigencia_com(time, false, true), Exigencia::Sessao(_)), "interruptor ligado");
}

#[test]
fn limite_vence_no_primeiro_relogio_e_recuo_do_civil_nao_estende() {
    let limite = Limite::em(60_000);
    assert!(!limite.vencido());
    assert!(limite.resta() <= Duration::from_secs(60));
    assert!(limite.expira_em_ms() <= now_ms() + 60_000);
    // Relógio civil recuado: o `exp` parece longe, mas o prazo monotônico já passou.
    let monotonico = Limite { exp_ms: now_ms() + 3_600_000, ate: Instant::now() };
    assert!(monotonico.vencido(), "o monotônico vence");
    assert_eq!(monotonico.resta(), Duration::ZERO);
    assert!(monotonico.expira_em_ms() <= now_ms(), "o cliente vê o menor prazo");
    // Relógio civil adiantado: o `exp` passou, mesmo com folga no monotônico.
    let civil = Limite { exp_ms: now_ms() - 1, ate: Instant::now() + Duration::from_secs(3600) };
    assert!(civil.vencido(), "o civil vence");
    assert_eq!(civil.resta(), Duration::ZERO);
}

#[test]
fn pareamento_relê_vinculo_prazo_e_papel_na_tx() {
    let hub = hub();
    vincular(&hub, &vinculo_pessoal());
    let v = vinculo(&vinculo_pessoal());
    let par = |iat, papel, limite| Pareamento { sub: DONO.into(), iat, papel, limite, workspace_id: v.workspace_id() };
    let gravou = |hub: &Hub, key: &str| hub.store.device(key).is_some();
    // Prazo vencido entre a verificação e a Tx: nada é gravado.
    let vencido = Limite { exp_ms: now_ms() + 60_000, ate: Instant::now() };
    assert_eq!(
        readmitir_vinculado(&hub, &aparelho("a"), &par(100, Papel::Dono, vencido)).unwrap(),
        Err(Recusa::AuthInvalid)
    );
    assert!(!gravou(&hub, "a"));
    // Vínculo trocado entre a verificação e a Tx.
    vincular(&hub, &vinculo_time());
    assert_eq!(
        readmitir_vinculado(&hub, &aparelho("b"), &par(100, Papel::Dono, Limite::em(60_000))).unwrap(),
        Err(Recusa::AuthInvalid)
    );
    assert!(!gravou(&hub, "b"));
    vincular(&hub, &vinculo_pessoal());
    // Papel registrado com `iat` mais novo e outro papel: o pareamento com o token velho não grava.
    assert_eq!(registrar_papel(&hub, DONO, 200, Some(Papel::Admin)), Ok(()));
    assert_eq!(
        readmitir_vinculado(&hub, &aparelho("c"), &par(150, Papel::Dono, Limite::em(60_000))).unwrap(),
        Err(Recusa::AuthInvalid)
    );
    assert!(!gravou(&hub, "c"));
    let e = epoch_gravado(&hub);
    assert_eq!(readmitir_vinculado(&hub, &aparelho("d"), &par(300, Papel::Dono, Limite::em(60_000))).unwrap(), Ok(()));
    assert!(gravou(&hub, "d"));
    assert_eq!(papel_de(&hub, DONO), Some(Papel::Dono));
    assert_eq!(epoch_gravado(&hub), e + 1, "a troca de papel no pareamento também sobe o epoch");
}

/// `deleteBot` espera o `team_lock`; o papel muda antes do efeito, que confere a guarda na Tx.
#[tokio::test]
async fn delete_bot_admitido_nao_roda_depois_que_o_papel_muda() {
    let hub = hub();
    vincular(&hub, &vinculo_pessoal());
    let bot = |id: &str| -> crate::store::BotConfig {
        serde_json::from_value(json!({"id": id, "name": id, "cwd": std::env::temp_dir(), "backend": "codex"})).unwrap()
    };
    for (id, sub, trocar) in [("dev", DONO, true), ("ops", OUTRO, false)] {
        hub.store.save_bot(&bot(id)).unwrap();
        let caller = aparelho_com_sessao(&hub, &aparelho(&format!("fone-{id}")), sub, Papel::Dono);
        let bloqueio = hub.store.team_lock.lock().await;
        let pedido = {
            let hub = hub.clone();
            tokio::spawn(async move { crate::api::dispatch(&hub, &caller, "deleteBot", json!({"botId": id})).await })
        };
        tokio::time::sleep(Duration::from_millis(300)).await;
        if trocar {
            let iat = papel_on(&hub.store.db.locked(), sub).unwrap().unwrap().0 + 1;
            trocar_papel(&hub, sub, iat, Some(Papel::Admin));
        }
        drop(bloqueio);
        let resultado = pedido.await.unwrap();
        let apagado = hub.store.bot(id).unwrap().is_none_or(|b| b.deleted);
        if trocar {
            assert!(format!("{:#}", resultado.unwrap_err()).contains("not_allowed"), "{id}");
            assert!(!apagado, "o efeito confere a guarda dentro do lock");
        } else {
            resultado.unwrap();
            assert!(apagado, "sem troca, o mesmo caminho apaga");
        }
    }
}

/// Os métodos cujo efeito ainda escapa da guarda da sessão: negados com vínculo até ter cerca.
pub(crate) const SEM_CERCA: &[&str] = &[
    "donnaConversationStop",
    "donnaConversationNewSession",
    "donnaConversationRetry",
    "stop",
    "newSession",
    "upload",
    "donnaConversationUpload",
    "reorderBots",
    "donnaConversationRead",
    "markRead",
    "registerDevice",
    "agentSetup",
    "termInput",
    "termResize",
    "termClose",
    "sub:term",
    "connectorRequestFinish",
    "retrySubscription",
];

#[test]
fn metodos_sem_cerca_ficam_negados_com_vinculo_para_todos_os_papeis() {
    let marcados: Vec<&str> =
        matriz::MATRIZ.iter().filter(|(_, m)| *m == matriz::Minimo::SemCerca).map(|(m, _)| *m).collect();
    assert_eq!(marcados, SEM_CERCA, "a lista do PR é a da matriz");
    for m in SEM_CERCA {
        assert!(matriz::sem_cerca(m), "{m}");
        for papel in [Papel::Membro, Papel::Admin, Papel::Dono] {
            assert!(!matriz::permitido(papel, m), "{m} negado para {papel:?}");
        }
    }
    // No despacho: aparelho num Hub com vínculo recusado; sem vínculo e o Local, nada muda.
    let hub = hub();
    hub.store.put_device(&aparelho("fone")).unwrap();
    let (_, fone) = crate::fork::autoridade::autenticar(&hub, "fone").unwrap();
    for m in SEM_CERCA {
        assert!(negar_sem_cerca(&hub, &fone, m).is_ok(), "{m} sem vínculo");
    }
    vincular(&hub, &vinculo_pessoal());
    for m in SEM_CERCA {
        assert!(negar_sem_cerca(&hub, &fone, m).unwrap_err().is::<crate::api::devices::Forbidden>(), "{m}");
        assert!(negar_sem_cerca(&hub, &Caller::Local, m).is_ok(), "{m} no computador");
    }
    assert!(negar_sem_cerca(&hub, &fone, "send").is_ok(), "fora da lista, segue a matriz");
}

#[test]
fn prova_e_papel_na_mesma_tx_reconferem_prazo_vinculo_epoch_e_aparelho() {
    let hub = hub();
    vincular(&hub, &vinculo_pessoal());
    let v = vinculo(&vinculo_pessoal());
    let par = Pareamento {
        sub: DONO.into(),
        iat: 100,
        papel: Papel::Dono,
        limite: Limite::em(60_000),
        workspace_id: v.workspace_id(),
    };
    readmitir_vinculado(&hub, &aparelho("fone"), &par).unwrap().unwrap();
    let (_, caller) = crate::fork::autoridade::autenticar(&hub, "fone").unwrap();
    let Caller::Device { autoridade: Some(original), .. } = caller else { panic!("sem autoridade") };
    let afirmacoes = |sub: &str, iat| Afirmacoes { sub: sub.into(), exp: agora() + 60, iat, org: None };
    let canal = |epoch| Canal { chave: "fone", original: &original, epoch };
    let e = epoch_gravado(&hub);
    let registrar = |canal: &Canal<'_>, sub: &str, iat, limite| {
        registrar_na_prova(&hub, canal, &v.workspace_id(), &afirmacoes(sub, iat), Some(Papel::Admin), limite)
    };
    let vencido = Limite { exp_ms: now_ms() + 60_000, ate: Instant::now() };
    assert_eq!(registrar(&canal(e), DONO, 200, vencido), Err(Recusa::AuthInvalid), "prazo vencido na Tx");
    assert_eq!(registrar(&canal(e + 1), DONO, 200, Limite::em(60_000)), Err(Recusa::AuthInvalid), "epoch do canal");
    assert_eq!(registrar(&canal(e), OUTRO, 200, Limite::em(60_000)), Err(Recusa::DeviceUnpaired), "sub ≠ userId");
    vincular(&hub, &vinculo_time());
    assert_eq!(registrar(&canal(e), DONO, 200, Limite::em(60_000)), Err(Recusa::AuthInvalid), "vínculo trocado");
    vincular(&hub, &vinculo_pessoal());
    // Concessão do aparelho trocada depois da abertura do canal.
    let mut trocado = hub.store.device("fone").unwrap();
    trocado.scopes = vec![crate::store::Scope::Control, crate::store::Scope::Screen];
    hub.store.put_device(&trocado).unwrap();
    assert_eq!(registrar(&canal(e), DONO, 200, Limite::em(60_000)), Err(Recusa::DeviceUnpaired), "concessão");
    assert_eq!((papel_de(&hub, DONO), epoch_gravado(&hub)), (Some(Papel::Dono), e), "nada foi gravado");
    // Tudo igual: papel e epoch no mesmo commit.
    readmitir_vinculado(&hub, &aparelho("fone"), &Pareamento { iat: 150, ..par }).unwrap().unwrap();
    let (_, caller) = crate::fork::autoridade::autenticar(&hub, "fone").unwrap();
    let Caller::Device { autoridade: Some(original), .. } = caller else { panic!("sem autoridade") };
    let canal = Canal { chave: "fone", original: &original, epoch: epoch_gravado(&hub) };
    assert_eq!(registrar(&canal, DONO, 200, Limite::em(60_000)), Ok(()));
    assert_eq!((papel_de(&hub, DONO), epoch_gravado(&hub)), (Some(Papel::Admin), canal.epoch + 1));
}

#[test]
fn nonce_na_borda_da_janela_e_depois_de_recuo_do_relogio_nao_repete() {
    let th = crypto::random();
    let chave = SigningKey::from_bytes(&crypto::random());
    let dk = crypto::b64(chave.verifying_key().as_bytes());
    let mut n = posse::Nonces::default();
    let t = now_ms();
    // Aceito no começo da janela do próprio `ts`; a repetição no fim exato da janela é recusada.
    let p = pop_em(&chave, "h", &th, "hello", t);
    assert_eq!(n.conferir("h", &th, &dk, "hello", &p, t - posse::JANELA_MS), Ok(()));
    assert_eq!(n.conferir("h", &th, &dk, "hello", &p, t + posse::JANELA_MS), Err(Recusa::DeviceUnpaired), "borda");
    // O relógio avança (a memória limpa o nonce) e depois recua: a janela não reabre.
    let q = pop_em(&chave, "h", &th, "hello", t);
    assert_eq!(n.conferir("h", &th, &dk, "hello", &q, t), Ok(()));
    let adiante = pop_em(&chave, "h", &th, "hello", t + 10 * posse::JANELA_MS);
    assert_eq!(n.conferir("h", &th, &dk, "hello", &adiante, t + 10 * posse::JANELA_MS), Ok(()));
    assert_eq!(n.conferir("h", &th, &dk, "hello", &q, t), Err(Recusa::DeviceUnpaired), "recuo do relógio");
    let novo = pop_em(&chave, "h", &th, "hello", t);
    assert_eq!(n.conferir("h", &th, &dk, "hello", &novo, t), Err(Recusa::DeviceUnpaired), "ts velho demais");
}

#[test]
fn nonce_vencido_sai_da_memoria_mesmo_atras_de_um_que_ainda_vale() {
    let th = crypto::random();
    let chave = SigningKey::from_bytes(&crypto::random());
    let dk = crypto::b64(chave.verifying_key().as_bytes());
    let mut n = posse::Nonces::default();
    let t = now_ms();
    // O primeiro vence por último; os de trás vencem antes e enchem a memória.
    let primeiro = pop_em(&chave, "h", &th, "hello", t + posse::JANELA_MS);
    assert_eq!(n.conferir("h", &th, &dk, "hello", &primeiro, t), Ok(()));
    for _ in 1..posse::MAX_POR_CANAL {
        let p = pop_em(&chave, "h", &th, "hello", t - posse::JANELA_MS);
        assert_eq!(n.conferir("h", &th, &dk, "hello", &p, t), Ok(()));
    }
    let cheia = pop_em(&chave, "h", &th, "hello", t);
    assert_eq!(n.conferir("h", &th, &dk, "hello", &cheia, t), Err(Recusa::DeviceUnpaired), "memória cheia");
    // Logo depois, só o primeiro ainda vale: os vencidos saem e cabe prova nova.
    let nova = pop_em(&chave, "h", &th, "hello", t + 1);
    assert_eq!(n.conferir("h", &th, &dk, "hello", &nova, t + 1), Ok(()));
    assert_eq!(
        n.conferir("h", &th, &dk, "hello", &primeiro, t + 1),
        Err(Recusa::DeviceUnpaired),
        "o primeiro segue gasto"
    );
}

#[tokio::test]
async fn jwks_com_validade_menor_que_o_intervalo_busca_de_novo_com_o_endpoint_saudavel() {
    let j = jwks::Jwks::default();
    j.servir_com_frescor(|| Ok((jwks_padrao(), Duration::from_secs(10))));
    let t0 = Instant::now();
    assert_eq!(j.chave(&url(), KID, t0).await.unwrap().ate, t0 + Duration::from_secs(10));
    let depois = t0 + Duration::from_secs(11);
    assert!(depois < t0 + jwks::INTERVALO);
    assert_eq!(j.chave(&url(), KID, depois).await.unwrap().ate, depois + Duration::from_secs(10), "sem ampliar o TTL");
    assert_eq!(j.buscas(), 2);
    // Com falha, o intervalo volta a valer: nada de martelar o endpoint.
    j.servir(|| anyhow::bail!("fora do ar"));
    let vencido = depois + Duration::from_secs(10);
    assert_eq!(j.chave(&url(), KID, vencido).await.err(), Some(Recusa::AuthInvalid));
    assert_eq!(j.chave(&url(), KID, vencido + ms(1)).await.err(), Some(Recusa::AuthInvalid));
    assert_eq!(j.buscas(), 3, "uma busca por intervalo depois de falhar");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn busca_da_jwks_nao_segura_o_conjunto_e_e_uma_so_para_quem_espera() {
    let j = Arc::new(jwks::Jwks::default());
    j.servir(|| Ok(jwks_padrao()));
    let t0 = Instant::now();
    assert!(j.chave(&url(), KID, t0).await.is_ok());
    j.servir(|| {
        std::thread::sleep(ms(400));
        Ok(jwks_padrao())
    });
    // `kid` desconhecido busca devagar; o `kid` conhecido não espera a busca.
    let depois = t0 + jwks::INTERVALO;
    let lenta = {
        let j = j.clone();
        tokio::spawn(async move { j.chave(&url(), "kid-desconhecido", depois).await })
    };
    tokio::time::sleep(ms(50)).await;
    let inicio = Instant::now();
    assert!(j.chave(&url(), KID, depois).await.is_ok());
    assert!(inicio.elapsed() < ms(200), "o GET não segura o conjunto");
    assert_eq!(lenta.await.unwrap().err(), Some(Recusa::AuthInvalid));
    // Conjunto vencido: quem chega durante a busca usa o resultado dela.
    j.vence_em(Duration::ZERO);
    let antes = j.buscas();
    let tarefas: Vec<_> = (0..4)
        .map(|_| {
            let j = j.clone();
            tokio::spawn(async move { j.chave(&url(), KID, Instant::now()).await })
        })
        .collect();
    for t in tarefas {
        assert!(t.await.unwrap().is_ok());
    }
    assert_eq!(j.buscas() - antes, 1, "uma busca só");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn kid_novo_durante_a_busca_espera_o_resultado_dela() {
    let j = Arc::new(jwks::Jwks::default());
    let [a, b] = chaves();
    j.servir(move || Ok(jwks(&[jwk(KID, a)])));
    let t0 = Instant::now();
    assert!(j.chave(&url(), KID, t0).await.is_ok());
    // Rotação: a busca pelo `kid` novo demora; um segundo pedido pelo mesmo `kid` chega durante ela.
    j.servir(move || {
        std::thread::sleep(ms(400));
        Ok(jwks(&[jwk(KID, a), jwk(KID_2, b)]))
    });
    let depois = t0 + jwks::INTERVALO;
    let primeira = {
        let j = j.clone();
        tokio::spawn(async move { j.chave(&url(), KID_2, depois).await })
    };
    tokio::time::sleep(ms(50)).await;
    assert!(j.chave(&url(), KID_2, depois + ms(1)).await.is_ok(), "esperou a busca em curso");
    assert!(primeira.await.unwrap().is_ok());
    assert_eq!(j.buscas(), 2, "uma busca só para os dois");
}

#[test]
fn bootstrap_da_nuvem_so_num_hub_sem_vinculo() {
    use crate::fork::bootstrap::codec::{self, Challenge, Grant, Proof};
    use crate::store::bootstrap::TrustedGeneration;
    let issuer = SigningKey::from_bytes(&[21; 32]);
    let device = SigningKey::from_bytes(&[22; 32]);
    for (vinculado, aceita) in [(true, false), (false, true)] {
        let hub = hub();
        let host = &hub.identity.sign;
        let now = now_ms();
        let cfg = TrustedGeneration {
            generation: crypto::b64(&[23; 32]),
            issuer_key_id: "issuer".into(),
            issuer_key: crypto::b64(issuer.verifying_key().as_bytes()),
            issuer: "donna".into(),
            deployment_id: "fixture".into(),
            env: "test".into(),
            manifest_hash: "a".repeat(64),
            resource_digest: "b".repeat(64),
            computer_id: "computer".into(),
            sign_key: crypto::b64(host.verifying_key().as_bytes()),
            box_key: crypto::b64(&[24; 32]),
        };
        let g = Grant(
            "donna/bootstrap-grant/v1".into(),
            "boot".into(),
            "intent".into(),
            cfg.resource_digest.clone(),
            cfg.generation.clone(),
            cfg.issuer_key_id.clone(),
            cfg.issuer.clone(),
            cfg.deployment_id.clone(),
            cfg.env.clone(),
            cfg.manifest_hash.clone(),
            "owner".into(),
            "device".into(),
            crypto::b64(device.verifying_key().as_bytes()),
            crypto::b64(&[25; 32]),
            cfg.computer_id.clone(),
            cfg.sign_key.clone(),
            cfg.box_key.clone(),
            crypto::b64(&[26; 32]),
            now - 1000,
            now + 200_000,
            vec!["control".into(), "owner.devices".into()],
        );
        let store = &hub.store;
        store.bootstrap_install_generation(&cfg).unwrap();
        store.bootstrap_install_grant(&codec::encode(&g, |b| crypto::sign(&issuer, b)).unwrap(), now).unwrap();
        let wire = store.bootstrap_challenge(host, "boot", &g.12, now).unwrap();
        let ch: Challenge = codec::decode(&wire, &host.verifying_key().to_bytes()).unwrap();
        let proof = Proof(
            "donna/bootstrap-proof/v1".into(),
            g.1.clone(),
            g.4.clone(),
            codec::hash(&g).unwrap(),
            codec::hash(&ch).unwrap(),
        );
        if vinculado {
            vinculo::gravar_para_prova(&hub, &vinculo_pessoal());
        }
        let commit =
            store.bootstrap_commit_proof(host, &codec::encode(&proof, |b| crypto::sign(&device, b)).unwrap(), now);
        assert_eq!(commit.is_ok(), aceita, "vinculado={vinculado}");
    }
}

#[tokio::test]
async fn send_de_grupo_e_negado_com_vinculo_e_titulo_e_leitura_conferem_a_guarda_na_tx() {
    use crate::fork::donna_fixa::pedido::{Origem, com_origem};
    use crate::fork::sessoes::{origin::SessionOrigin, store as sessoes};
    let hub = hub();
    for cfg in [
        json!({"id":"b","name":"Bot","backend":"claude"}),
        json!({"id":"sala","name":"Sala","kind":"group","members":["b"]}),
    ] {
        hub.store.save_bot(&serde_json::from_value(cfg).unwrap()).unwrap();
    }
    vincular(&hub, &vinculo_pessoal());
    let caller = aparelho_com_sessao(&hub, &aparelho("fone"), DONO, Papel::Dono);
    let entradas =
        || -> i64 { hub.store.db.locked().query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0)).unwrap() };
    let antes = entradas();
    let negado =
        crate::api::dispatch(&hub, &caller, "send", json!({"botId":"sala","text":"oi","clientNonce":"n1"})).await;
    assert!(negado.unwrap_err().is::<crate::api::devices::Forbidden>(), "rodada do grupo sem guarda");
    assert_eq!(entradas(), antes, "nada gravado");
    // Título da 1ª mensagem e leitura do `send` legado: gravam com a guarda viva; depois da troca
    // de papel, a própria Tx recusa.
    let nova = || {
        let new = sessoes::NewSession {
            chat: "b".into(),
            origin: SessionOrigin::Owner,
            source_session: None,
            parent_session: None,
            title: String::new(),
            runs_on: None,
        };
        sessoes::create(&hub.store.db.locked(), &new).unwrap().id
    };
    let titulo = |id: &str| sessoes::get(&hub.store.db.locked(), id).unwrap().unwrap().title;
    let origem = Origem::Dono(hub.clone(), crate::fork::autoridade::Autoridade::capturar(&hub, &caller).unwrap());
    let (viva, morta) = (nova(), nova());
    let lane = |id: &str| crate::store::Lane { chat: "b".into(), session: Some(id.into()) };
    com_origem(origem.clone(), || crate::fork::sessoes::api::title_first_message(&hub, &lane(&viva), "Primeira ideia"));
    assert_eq!(titulo(&viva), "Primeira ideia");
    assert!(com_origem(origem.clone(), || hub.store.mark_read("b")).is_ok());
    trocar_papel(&hub, DONO, now_ms() / 1000 + 1, Some(Papel::Admin));
    com_origem(origem.clone(), || crate::fork::sessoes::api::title_first_message(&hub, &lane(&morta), "Outra ideia"));
    assert_eq!(titulo(&morta), "", "título fora da guarda não é gravado");
    assert!(com_origem(origem, || hub.store.mark_read("b")).is_err(), "leitura fora da guarda não é gravada");
}
