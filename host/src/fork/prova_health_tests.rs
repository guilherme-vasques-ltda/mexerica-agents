//! A prova do token local: nenhuma prova, prova de outro token, nonce repetido ou tamanho errado
//! não abrem a identidade do `/health`; nenhum token de teste é real.
#![allow(clippy::unwrap_used)]
use super::*;

struct Fixture {
    hub: std::sync::Arc<Hub>,
    dir: std::path::PathBuf,
}
impl Fixture {
    fn new(token: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("prova-health-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Hub::new(
            crate::store::Store::open(std::path::Path::new(":memory:")).unwrap(),
            "host-ficticio".into(),
            crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
            token.into(),
            0,
        );
        Self { hub, dir }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const TOKEN: &str = "token-local-ficticio";
const NONCE: [u8; 32] = [7; 32];

fn nonce() -> String {
    crate::remote::crypto::b64(&NONCE)
}

#[test]
fn a_prova_do_token_local_abre_e_outro_token_nao() {
    let f = Fixture::new(TOKEN);
    assert!(confere(TOKEN, &nonce(), &prova(TOKEN, &NONCE)));
    assert!(!confere(TOKEN, &nonce(), &prova("outro-token-ficticio", &NONCE)));
    assert!(!confere(TOKEN, &nonce(), &prova(TOKEN, &[9; 32])), "a prova é do nonce que veio");
    assert!(!confere(TOKEN, &nonce(), ""), "sem prova não há identidade");
    assert!(!confere(TOKEN, "", &prova(TOKEN, &NONCE)), "sem nonce não há identidade");
    assert!(autorizada(&f.hub, &nonce(), &prova(TOKEN, &NONCE), "127.0.0.1:5000".parse().unwrap()));
    let mapeado = "[::ffff:127.0.0.1]:5000".parse().unwrap();
    assert!(autorizada(&f.hub, &nonce(), &prova(TOKEN, &NONCE), mapeado), "loopback visto por `--bind ::`");
    let de_fora = "192.0.2.10:5000".parse().unwrap();
    assert!(!autorizada(&f.hub, &nonce(), &prova(TOKEN, &NONCE), de_fora), "a prova não vale de fora do loopback");
}

#[test]
fn formato_errado_da_prova_e_recusado_sem_panico() {
    let _f = Fixture::new(TOKEN);
    let certa = prova(TOKEN, &NONCE);
    for errada in [
        "zz".repeat(32),
        "a".repeat(63),
        "a".repeat(65),
        certa.to_uppercase(),
        format!("{certa} "),
        format!("0x{certa}"),
    ] {
        assert!(!confere(TOKEN, &nonce(), &errada), "{errada}");
    }
    for errado in ["", "nao-e-base64url!!", &crate::remote::crypto::b64(&[7; 31])] {
        assert!(!confere(TOKEN, errado, &certa), "{errado}");
    }
}
