//! Prova de posse do token local para o `/health`. O cliente manda um nonce novo e o HMAC-SHA256
//! do token sobre ele; assim a identidade do Hub sai para quem já tem o token sem o token viajar
//! para uma porta que outro processo possa ocupar. Cada prova vale só para o nonce que veio.
use crate::hub::Hub;
use hmac::{Hmac, KeyInit, Mac as _};
use sha2::Sha256;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Domínio desta prova: um HMAC do token aqui nunca vale como credencial de outro uso.
const ROTULO: &[u8] = b"mexerica/health-proof/v1";
/// Domínio do recibo que prova a resposta ao cliente.
const RECIBO: &[u8] = b"mexerica/health-receipt/v1";
/// O nonce é sempre deste tamanho; sem isso a mensagem assinada ficaria ambígua.
const NONCE: usize = 32;
/// Enquanto um nonce aceito continua recusado: cobre o tempo em que a prova ainda poderia ser repetida.
const JANELA: Duration = Duration::from_secs(300);

fn hmac(token: &str, nonce: &[u8]) -> [u8; 32] {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(token.as_bytes()).expect("o HMAC-SHA256 aceita chave de qualquer tamanho");
    mac.update(ROTULO);
    mac.update(nonce);
    mac.finalize().into_bytes().into()
}

/// A prova hexadecimal que este Hub espera para um nonce. Só os testes calculam do lado de cá: o
/// cliente que prova o token é o app.
#[cfg(test)]
pub(crate) fn prova(token: &str, nonce: &[u8]) -> String {
    hexadecimal(&hmac(token, nonce))
}

/// 32 bytes em hexadecimal minúsculo, do mesmo jeito que o cliente manda.
fn hexadecimal(bytes: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// 32 bytes em hexadecimal minúsculo, ou nada quando o texto não tem esse tamanho nem esse formato.
fn bytes(proof: &str) -> Option<[u8; 32]> {
    if proof.len() != 64 || !proof.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(proof.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Confere o par `nonce`/`proof`: nonce de 32 bytes em base64url e prova hexadecimal, comparada em
/// tempo constante. Qualquer coisa fora disso é recusa.
pub(crate) fn confere(token: &str, nonce: &str, proof: &str) -> bool {
    let Ok(nonce) = crate::remote::crypto::unb64_n::<NONCE>(nonce) else { return false };
    let Some(proof) = bytes(proof) else { return false };
    crate::remote::crypto::ct_eq(&proof, &hmac(token, &nonce))
}

/// Só este computador: o loopback, ou o socket do perfil isolado quando ele é o dono da conexão.
fn deste_computador(peer: SocketAddr) -> bool {
    peer.ip().to_canonical().is_loopback()
        || (crate::environment::Environment::is_open() && peer == crate::open_hub::SOCKET_PEER)
}

/// Nonces já aceitos e quando. Um par nonce/prova vale uma vez só, então uma prova capturada não
/// volta a abrir a identidade do Hub.
fn consumidos() -> &'static Mutex<HashMap<[u8; 32], Instant>> {
    static CONSUMIDOS: OnceLock<Mutex<HashMap<[u8; 32], Instant>>> = OnceLock::new();
    CONSUMIDOS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Aceita o par uma única vez: confere origem e prova e registra o nonce na mesma operação, sem
/// consumir o nonce de uma tentativa recusada. Devolve o nonce, que o handler usa no recibo.
pub(crate) fn aceitar(hub: &Hub, nonce: &str, proof: &str, peer: SocketAddr) -> Option<[u8; 32]> {
    if !deste_computador(peer) || !confere(&hub.token, nonce, proof) {
        return None;
    }
    let nonce = crate::remote::crypto::unb64_n::<NONCE>(nonce).ok()?;
    let mut vistos = consumidos().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    vistos.retain(|_, quando| quando.elapsed() < JANELA);
    vistos.insert(nonce, Instant::now()).is_none().then_some(nonce)
}

/// A identidade que sai, em ordem fixa: o recibo cobre exatamente o que o cliente lê.
pub(crate) fn identidade(host_id: &str, computer_id: &str, binary: Option<(&str, &str)>) -> String {
    let (path, hash) = binary.unwrap_or(("", ""));
    format!("{host_id}\n{computer_id}\n{path}\n{hash}")
}

/// Recibo da resposta: prova ao cliente que quem respondeu tem o mesmo token, preso ao nonce que
/// ele escolheu e à identidade que saiu.
pub(crate) fn recibo(token: &str, nonce: &[u8], identidade: &str) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(token.as_bytes()).expect("o HMAC-SHA256 aceita chave de qualquer tamanho");
    mac.update(RECIBO);
    mac.update(nonce);
    mac.update(identidade.as_bytes());
    hexadecimal(&mac.finalize().into_bytes().into())
}

#[cfg(test)]
#[path = "prova_health_tests.rs"]
mod tests;
