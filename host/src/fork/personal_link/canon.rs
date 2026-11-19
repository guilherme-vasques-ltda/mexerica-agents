//! Canonicalização J da proposta §4: UTF-8 compacto, só arrays de aridade fixa, literais/IDs
//! ASCII restritos, inteiros seguros e booleans. Sem objeto, null, float, negativo ou default.
//! Envelope assinado = `[payload, assinaturaB64]`, assinado sobre `J(payload)`.
use crate::remote::{crypto, identity::Identity};
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;

pub(crate) const LIMITE: usize = 64 * 1024;
const PROFUNDIDADE: usize = 12;
pub(crate) const MAX_INT: u64 = 9_007_199_254_740_991;

fn texto(s: &str) -> bool {
    !s.is_empty() && s.len() <= 1024 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._/:-".contains(&b))
}

fn valida(v: &Value, nivel: usize) -> Result<()> {
    ensure!(nivel <= PROFUNDIDADE, "not_allowed: J profundo demais");
    match v {
        Value::Array(itens) => itens.iter().try_for_each(|x| valida(x, nivel + 1)),
        Value::String(s) => {
            ensure!(texto(s), "not_allowed: literal J inválido");
            Ok(())
        }
        Value::Number(n) => {
            ensure!(n.as_u64().is_some_and(|n| n <= MAX_INT), "not_allowed: inteiro J inválido");
            Ok(())
        }
        Value::Bool(_) => Ok(()),
        _ => bail!("not_allowed: tipo J recusado"),
    }
}

/// Limite estrutural ANTES de o parser alocar arrays aninhados (também usado pelo frame B).
pub(super) fn profundidade(bytes: &[u8]) -> Result<()> {
    let (mut nivel, mut texto, mut escape) = (0usize, false, false);
    for &b in bytes {
        if texto {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                texto = false;
            }
        } else {
            match b {
                b'"' => texto = true,
                b'[' | b'{' => {
                    nivel += 1;
                    ensure!(nivel <= PROFUNDIDADE + 1, "not_allowed: J profundo demais");
                }
                b']' | b'}' => nivel = nivel.saturating_sub(1),
                _ => {}
            }
        }
    }
    Ok(())
}

/// Bytes recebidos precisam ser exatamente `J(valor)` antes de qualquer interpretação.
pub(crate) fn ler(bytes: &[u8]) -> Result<Value> {
    ensure!(!bytes.is_empty() && bytes.len() <= LIMITE, "not_allowed: tamanho J inválido");
    profundidade(bytes)?;
    let valor: Value = serde_json::from_slice(bytes).context("not_allowed: J ilegível")?;
    ensure!(valor.is_array(), "not_allowed: J precisa ser array");
    valida(&valor, 0)?;
    ensure!(serde_json::to_vec(&valor)? == bytes, "not_allowed: J não canônico");
    Ok(valor)
}

pub(crate) fn j(valor: &Value) -> Result<Vec<u8>> {
    ensure!(valor.is_array(), "not_allowed: J precisa ser array");
    valida(valor, 0)?;
    let bytes = serde_json::to_vec(valor)?;
    ensure!(bytes.len() <= LIMITE, "not_allowed: J grande demais");
    Ok(bytes)
}

/// `hash(x) = SHA256(J(x))` em hex minúsculo.
pub(crate) fn hash(valor: &Value) -> Result<String> {
    Ok(crate::fork::bootstrap::codec::hash_bytes(&j(valor)?))
}

/// Envelope verificado. `valor` é o próprio `[payload, assinatura]`; nunca reconstruído com defaults.
#[derive(Clone, Debug)]
pub(crate) struct Assinado {
    valor: Value,
}
impl Assinado {
    pub(crate) fn payload(&self) -> &[Value] {
        self.valor[0].as_array().map_or(&[], Vec::as_slice)
    }
    pub(crate) fn valor(&self) -> &Value {
        &self.valor
    }
    pub(crate) fn bytes(&self) -> Result<Vec<u8>> {
        j(&self.valor)
    }
    pub(crate) fn digest(&self) -> Result<String> {
        hash(&self.valor)
    }
}

pub(crate) fn assinar(identidade: &Identity, payload: Value) -> Result<Assinado> {
    let bytes = j(&payload)?;
    let assinatura = identidade.sign(&bytes);
    let valor = Value::Array(vec![payload, Value::String(crypto::b64(&assinatura))]);
    j(&valor)?;
    Ok(Assinado { valor })
}

/// Confere aridade, domínio e assinatura Ed25519 estrita pela chave esperada.
pub(crate) fn verificar(valor: &Value, dominio: &str, aridade: usize, chave: &str) -> Result<Assinado> {
    j(valor)?;
    let par = valor.as_array().context("not_allowed: envelope inválido")?;
    ensure!(par.len() == 2, "not_allowed: envelope inválido");
    let payload = par[0].as_array().context("not_allowed: payload inválido")?;
    ensure!(
        payload.len() == aridade && payload[0] == dominio && payload[1] == 1,
        "not_allowed: domínio ou aridade divergentes"
    );
    let assinatura = b64n::<64>(&par[1])?;
    let chave = b64n::<32>(&Value::String(chave.to_owned()))?;
    crypto::verify(&chave, &j(&par[0])?, &assinatura)
        .map_err(|_| anyhow::anyhow!("not_allowed: assinatura inválida"))?;
    Ok(Assinado { valor: valor.clone() })
}

pub(crate) fn b64n<const N: usize>(v: &Value) -> Result<[u8; N]> {
    let s = v.as_str().context("not_allowed: campo base64url ausente")?;
    let bytes = crypto::unb64_n::<N>(s)?;
    ensure!(crypto::b64(&bytes) == s, "not_allowed: base64url não canônico");
    Ok(bytes)
}

pub(crate) fn hex64(v: &Value) -> Result<&str> {
    let s = v.as_str().context("not_allowed: digest ausente")?;
    ensure!(
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "not_allowed: digest inválido"
    );
    Ok(s)
}

pub(crate) fn inteiro(v: &Value) -> Result<u64> {
    v.as_u64().filter(|n| *n <= MAX_INT).context("not_allowed: inteiro ausente")
}

pub(crate) fn aleatorio<const N: usize>() -> String {
    crypto::b64(&crypto::random::<N>())
}
