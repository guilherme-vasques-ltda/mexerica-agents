//! Fixtures das provas: chaves RSA efêmeras geradas na hora pelo `openssl` do sistema (nunca
//! gravadas), JWKS e tokens de sessão assinados aqui, vínculo fictício em `*.example.invalid`.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use crate::hub::Hub;
use crate::remote::crypto;
use crate::store::now_ms;
use ed25519_dalek::SigningKey;
use serde_json::{Value, json};
use std::sync::OnceLock;

pub(crate) const EMISSOR: &str = "https://clerk.example.invalid";
pub(crate) const JWKS_URL: &str = "https://clerk.example.invalid/.well-known/jwks.json";
pub(crate) const AZP: &str = "https://app.example.invalid";
pub(crate) const DONO: &str = "user_dono_ficticio";
pub(crate) const OUTRO: &str = "user_outro_ficticio";
pub(crate) const ORG: &str = "org_time_ficticio";
pub(crate) const KID: &str = "kid-prova-1";
pub(crate) const KID_2: &str = "kid-prova-2";

pub(crate) struct ChaveRsa {
    par: ring::rsa::KeyPair,
    n: String,
    e: String,
}

fn gerar() -> ChaveRsa {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    let pem = Command::new("openssl").args(["genrsa", "2048"]).output().expect("openssl para a chave de prova");
    assert!(pem.status.success(), "geração da chave de prova falhou");
    // A chave vai pelo pipe, nunca por arquivo.
    let mut conversao = Command::new("openssl")
        .args(["rsa", "-outform", "DER"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("openssl para a chave de prova");
    conversao.stdin.take().expect("entrada").write_all(&pem.stdout).expect("chave de prova pelo pipe");
    let der = conversao.wait_with_output().expect("conversão da chave de prova");
    assert!(der.status.success(), "conversão da chave de prova falhou");
    let par = ring::rsa::KeyPair::from_pkcs8(&der.stdout)
        .or_else(|_| ring::rsa::KeyPair::from_der(&der.stdout))
        .expect("chave de prova legível");
    let publica = ring::rsa::PublicKeyComponents::<Vec<u8>>::from(par.public());
    ChaveRsa { n: crypto::b64(&publica.n), e: crypto::b64(&publica.e), par }
}

/// Duas chaves por processo de teste.
pub(crate) fn chaves() -> &'static [ChaveRsa; 2] {
    static CHAVES: OnceLock<[ChaveRsa; 2]> = OnceLock::new();
    CHAVES.get_or_init(|| [gerar(), gerar()])
}

pub(crate) fn jwk(kid: &str, chave: &ChaveRsa) -> Value {
    json!({"kty": "RSA", "kid": kid, "use": "sig", "alg": "RS256", "n": chave.n, "e": chave.e})
}

pub(crate) fn jwks(chaves: &[Value]) -> Vec<u8> {
    serde_json::to_vec(&json!({"keys": chaves})).unwrap()
}

/// JWKS padrão: as duas chaves.
pub(crate) fn jwks_padrao() -> Vec<u8> {
    let [a, b] = chaves();
    jwks(&[jwk(KID, a), jwk(KID_2, b)])
}

pub(crate) fn assinar(cabecalho: &Value, afirmacoes: &Value, chave: &ChaveRsa) -> String {
    let cab = crypto::b64(&serde_json::to_vec(cabecalho).unwrap());
    let cor = crypto::b64(&serde_json::to_vec(afirmacoes).unwrap());
    let mensagem = format!("{cab}.{cor}");
    let mut sig = vec![0; chave.par.public().modulus_len()];
    chave
        .par
        .sign(&ring::signature::RSA_PKCS1_SHA256, &ring::rand::SystemRandom::new(), mensagem.as_bytes(), &mut sig)
        .unwrap();
    format!("{mensagem}.{}", crypto::b64(&sig))
}

pub(crate) fn cabecalho() -> Value {
    json!({"alg": "RS256", "kid": KID, "typ": "JWT"})
}

/// Afirmações v2 válidas agora: vida de 60 s.
pub(crate) fn afirmacoes(sub: &str) -> Value {
    let agora = now_ms() / 1000;
    json!({"v": 2, "iss": EMISSOR, "azp": AZP, "sub": sub, "sid": "sess_ficticia", "iat": agora, "nbf": agora - 1, "exp": agora + 60})
}

pub(crate) fn com_org(mut a: Value, org: &str, rol: &str) -> Value {
    a["o"] = json!({"id": org, "rol": rol, "slg": "time-ficticio"});
    a
}

pub(crate) fn token(afirmacoes: &Value) -> String {
    assinar(&cabecalho(), afirmacoes, &chaves()[0])
}

pub(crate) fn vinculo_pessoal() -> String {
    json!({"schema": 1, "kind": "personal", "ownerUserId": DONO, "issuer": EMISSOR, "jwksUrl": JWKS_URL,
        "authorizedParties": [AZP]})
    .to_string()
}

pub(crate) fn vinculo_time() -> String {
    json!({"schema": 1, "kind": "team", "orgId": ORG, "creatorUserId": DONO, "issuer": EMISSOR, "jwksUrl": JWKS_URL,
        "authorizedParties": [AZP]})
    .to_string()
}

/// Grava o vínculo de prova e serve a JWKS padrão sem rede. Um vínculo `team` liga, só neste Hub, o interruptor do `team`.
pub(crate) fn vincular(hub: &Hub, vinculo: &str) {
    if super::vinculo::interpretar(vinculo).is_some_and(|v| v.kind() == "team") {
        hub.sessao_clerk.liberar_time_para_prova();
    }
    super::vinculo::gravar_para_prova(hub, vinculo);
    hub.sessao_clerk.jwks.servir(|| Ok(jwks_padrao()));
}

/// Prova de posse de um pedido no canal de hash `th`, assinada pela chave do aparelho.
pub(crate) fn pop(chave: &SigningKey, hub_id: &str, th: &[u8; 32], metodo: &str) -> Value {
    pop_em(chave, hub_id, th, metodo, now_ms())
}

pub(crate) fn pop_em(chave: &SigningKey, hub_id: &str, th: &[u8; 32], metodo: &str, ts: i64) -> Value {
    let nonce = crypto::b64(&crypto::random::<16>());
    let sig = crypto::sign(chave, &super::posse::mensagem(hub_id, th, metodo, ts, &nonce));
    json!({"v": super::posse::VERSAO, "ts": ts, "nonce": nonce, "sig": crypto::b64(&sig)})
}

/// Liga o aparelho Local `device` a `sub` com `papel` (como o `pair` num Hub com vínculo) e devolve
/// o `Caller` do canal com a guarda de uma sessão de `sub` admitida agora (prazo de 60 s).
pub(crate) fn aparelho_com_sessao(
    hub: &std::sync::Arc<Hub>,
    device: &crate::store::Device,
    sub: &str,
    papel: super::Papel,
) -> crate::api::devices::Caller {
    use super::{Guarda, Limite, Pareamento, Prazo, Sessao};
    let super::vinculo::Estado::Vinculado(v) = super::vinculo::ler(hub) else { panic!("Hub sem vínculo") };
    let limite = Limite::em(60_000);
    let par = Pareamento::de_prova(hub, &v, &device.key, sub, now_ms() / 1000, papel, limite);
    super::readmitir_vinculado(hub, device, &par).unwrap().unwrap();
    let (_, caller) = crate::fork::autoridade::autenticar(hub, &device.key).unwrap();
    let crate::api::devices::Caller::Device { key, scopes, autoridade: Some(original) } = caller else {
        panic!("aparelho sem autoridade do transporte")
    };
    let sessao = Sessao { sub: sub.into(), papel, limite };
    let guarda = Guarda::nova(hub, &key, &sessao, &v, hub.workspace.atual(), Prazo::Fixo(limite));
    crate::api::devices::Caller::Device {
        key,
        scopes,
        autoridade: Some(std::sync::Arc::new(original.com_sessao(std::sync::Arc::new(guarda)))),
    }
}

/// Um token mais novo de `sub` com outro papel (ou nenhum): registra como `sessionAuthenticate`.
pub(crate) fn trocar_papel(hub: &Hub, sub: &str, iat: i64, papel: Option<super::Papel>) {
    super::registrar_papel(hub, sub, iat, papel).unwrap();
}
