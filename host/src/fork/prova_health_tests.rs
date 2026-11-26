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
const NOVO_NONCE: [u8; 32] = [11; 32];
const OUTRO_NONCE: [u8; 32] = [13; 32];

fn nonce() -> String {
    crate::remote::crypto::b64(&NONCE)
}
fn novo_nonce() -> String {
    crate::remote::crypto::b64(&NOVO_NONCE)
}
fn outro_nonce() -> String {
    crate::remote::crypto::b64(&OUTRO_NONCE)
}

#[test]
fn a_prova_do_token_local_abre_e_outro_token_nao() {
    let f = Fixture::new(TOKEN);
    let loopback = || "127.0.0.1:5000".parse().unwrap();
    assert!(confere(TOKEN, &nonce(), &prova(TOKEN, &NONCE)));
    assert!(!confere(TOKEN, &nonce(), &prova("outro-token-ficticio", &NONCE)));
    assert!(!confere(TOKEN, &nonce(), &prova(TOKEN, &[9; 32])), "a prova é do nonce que veio");
    assert!(!confere(TOKEN, &nonce(), ""), "sem prova não há identidade");
    assert!(!confere(TOKEN, "", &prova(TOKEN, &NONCE)), "sem nonce não há identidade");
    assert_eq!(aceitar(&f.hub, &nonce(), &prova(TOKEN, &NONCE), loopback()), Some(NONCE));
    let mapeado: SocketAddr = "[::ffff:127.0.0.1]:5000".parse().unwrap();
    assert_eq!(
        aceitar(&f.hub, &novo_nonce(), &prova(TOKEN, &NOVO_NONCE), mapeado),
        Some(NOVO_NONCE),
        "loopback visto por `--bind ::`"
    );
    let de_fora: SocketAddr = "192.0.2.10:5000".parse().unwrap();
    assert_eq!(
        aceitar(&f.hub, &outro_nonce(), &prova(TOKEN, &OUTRO_NONCE), de_fora),
        None,
        "a prova não vale de fora do loopback"
    );
    assert_eq!(
        aceitar(&f.hub, &outro_nonce(), &prova(TOKEN, &OUTRO_NONCE), loopback()),
        Some(OUTRO_NONCE),
        "um nonce recusado pela origem continua livre"
    );
}

#[test]
fn cada_nonce_abre_uma_vez_so() {
    // Cada teste usa o próprio nonce: a marca de consumo é do processo, como no Hub de verdade.
    const UMA_VEZ: [u8; 32] = [21; 32];
    let f = Fixture::new(TOKEN);
    let loopback: SocketAddr = "127.0.0.1:5000".parse().unwrap();
    let nonce = crate::remote::crypto::b64(&UMA_VEZ);
    let prova = prova(TOKEN, &UMA_VEZ);
    assert_eq!(aceitar(&f.hub, &nonce, &prova, loopback), Some(UMA_VEZ));
    assert_eq!(aceitar(&f.hub, &nonce, &prova, loopback), None, "uma prova capturada não volta a abrir");
}

#[test]
fn o_recibo_prova_a_resposta_e_fica_preso_ao_nonce_e_a_identidade() {
    let texto = identidade("host-ficticio", "computador-ficticio", Some(("/bin/host-ficticio", "hash-ficticio")));
    let certo = recibo(TOKEN, &NONCE, &texto);
    assert_eq!(certo, recibo(TOKEN, &NONCE, &texto), "o mesmo pedido dá o mesmo recibo");
    assert_ne!(certo, recibo("outro-token-ficticio", &NONCE, &texto), "outro token não produz o recibo");
    assert_ne!(certo, recibo(TOKEN, &[9; 32], &texto), "o recibo é do nonce que foi provado");
    let trocado = identidade("host-ficticio", "outro-computador", Some(("/bin/host-ficticio", "hash-ficticio")));
    assert_ne!(certo, recibo(TOKEN, &NONCE, &trocado), "uma identidade trocada muda o recibo");
    assert_eq!(certo.len(), 64);
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

#[test]
fn a_lista_de_nonces_nao_cresce_sem_limite() {
    // O teto é o que o processo usa: provas válidas entram uma a uma e, passando dele, o nonce mais
    // antigo sai — a memória fica presa no teto e uma prova desse nonce antigo volta a abrir só a
    // identidade, que é tudo o que uma repetição alcança.
    let f = Fixture::new(TOKEN);
    let loopback: SocketAddr = "127.0.0.1:5000".parse().unwrap();
    let mut primeiro = [0u8; 32];
    primeiro[0] = 250;
    let primeiro_b64 = crate::remote::crypto::b64(&primeiro);
    let primeira_prova = prova(TOKEN, &primeiro);
    assert_eq!(aceitar(&f.hub, &primeiro_b64, &primeira_prova, loopback), Some(primeiro));
    for i in 0..TETO_NONCES {
        let mut nonce = [0u8; 32];
        nonce[..2].copy_from_slice(&u16::try_from(i).unwrap().to_be_bytes());
        nonce[2] = 99;
        let b64 = crate::remote::crypto::b64(&nonce);
        let prova = prova(TOKEN, &nonce);
        assert_eq!(aceitar(&f.hub, &b64, &prova, loopback), Some(nonce), "nonce {i} é novo");
    }
    let vistos = f.hub.consumidos.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(vistos.ordem.len() <= TETO_NONCES, "a memória fica no teto, não no tanto de provas");
    drop(vistos);
    assert_eq!(
        aceitar(&f.hub, &primeiro_b64, &primeira_prova, loopback),
        Some(primeiro),
        "o nonce mais antigo já saiu da lista; repetir a prova dele revela só a identidade, nunca credencial"
    );
}
