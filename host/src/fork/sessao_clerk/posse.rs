//! Prova de posse da chave do aparelho em cada pedido: assinatura Ed25519 da chave pareada sobre
//! `hubId`, o hash do handshake deste canal (`th`), método, tempo e nonce. O tempo vale ±30 s e
//! cada nonce, uma vez por canal. A prova só vale no canal em que foi feita: outro canal (ou o
//! mesmo aparelho depois de um reinício do Hub) tem outro `th`, então a memória de nonces pode
//! ficar só neste canal, com teto próprio.
use super::Recusa;
use crate::remote::crypto;
use serde_json::Value;
use std::collections::{HashSet, VecDeque};

pub(crate) const JANELA_MS: i64 = 30_000;
/// Versão do formato da prova no fio (`pop.v`).
pub(crate) const VERSAO: i64 = 1;
/// Nonces guardados por canal; cheio, a prova é recusada até a janela liberar espaço.
pub(crate) const MAX_POR_CANAL: usize = 1024;
const DOMINIO: &[u8] = b"mexerica-pop-v1";

/// Bytes assinados: domínio, `hubId`, `th` (base64url), método, `ts` decimal e nonce (base64url),
/// separados por NUL.
pub(crate) fn mensagem(hub_id: &str, th: &[u8; 32], metodo: &str, ts: i64, nonce: &str) -> Vec<u8> {
    let (th, ts) = (crypto::b64(th), ts.to_string());
    let mut m = DOMINIO.to_vec();
    for parte in [hub_id.as_bytes(), th.as_bytes(), metodo.as_bytes(), ts.as_bytes(), nonce.as_bytes()] {
        m.push(0);
        m.extend_from_slice(parte);
    }
    m
}

/// Nonces já usados neste canal, em ordem de chegada, até saírem da janela do próprio `ts`.
#[derive(Default)]
pub(crate) struct Nonces {
    fila: VecDeque<(String, i64)>,
    vistos: HashSet<String>,
    /// O maior relógio já visto neste canal: um recuo do relógio não reabre a janela de um nonce
    /// que já saiu da memória.
    maximo: i64,
}

impl Nonces {
    /// `pop` = `{"v": 1, "ts": ms, "nonce": base64url de 16 a 64 bytes, "sig": base64url de 64 bytes}`.
    /// O nonce só é gasto depois de a assinatura conferir.
    pub(crate) fn conferir(
        &mut self,
        hub_id: &str,
        th: &[u8; 32],
        dk: &str,
        metodo: &str,
        pop: &Value,
        agora: i64,
    ) -> Result<(), Recusa> {
        let recusa = Err(Recusa::DeviceUnpaired);
        let (Some(VERSAO), Some(ts), Some(nonce), Some(sig)) =
            (pop["v"].as_i64(), pop["ts"].as_i64(), pop["nonce"].as_str(), pop["sig"].as_str())
        else {
            return recusa;
        };
        self.maximo = self.maximo.max(agora);
        // Fora da janela no relógio atual ou já vencido no maior relógio visto: um nonce assim pode
        // ter saído da memória, então nunca vale.
        if agora.abs_diff(ts) > JANELA_MS.unsigned_abs()
            || ts.saturating_add(JANELA_MS) < self.maximo
            || !crypto::unb64(nonce).is_ok_and(|n| (16..=64).contains(&n.len()))
        {
            return recusa;
        }
        let (Ok(chave), Ok(sig)) = (crypto::unb64_n::<32>(dk), crypto::unb64_n::<64>(sig)) else { return recusa };
        if crypto::verify(&chave, &mensagem(hub_id, th, metodo, ts, nonce), &sig).is_err() {
            return recusa;
        }
        // Sai só o que nenhum relógio aceito ainda aceitaria (`ts + janela` abaixo do maior visto).
        let (maximo, vistos) = (self.maximo, &mut self.vistos);
        self.fila.retain(|(velho, expira)| {
            let fica = *expira >= maximo;
            if !fica {
                vistos.remove(velho);
            }
            fica
        });
        if self.vistos.contains(nonce) {
            return recusa;
        }
        if self.fila.len() >= MAX_POR_CANAL {
            tracing::warn!("prova de posse recusada: memória de nonces do canal cheia");
            return recusa;
        }
        self.vistos.insert(nonce.to_owned());
        self.fila.push_back((nonce.to_owned(), ts.saturating_add(JANELA_MS)));
        Ok(())
    }
}
