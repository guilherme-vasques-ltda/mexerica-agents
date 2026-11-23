//! Token de sessão Clerk v2: RS256 com `kid` da JWKS fixada, `iss` exato, `azp` permitido, `sid`
//! não vazio, `exp`/`nbf`/`iat` com folga de 5 s e vida curta (`exp − iat ≤ 60`). Nenhuma recusa
//! repete o token ou parte dele.
use super::Recusa;
use super::vinculo::Vinculo;
use crate::remote::crypto;
use serde_json::Value;

/// Folga de relógio, em segundos.
pub(crate) const FOLGA_S: i64 = 5;
/// Vida máxima do token (`exp − iat`), em segundos.
pub(crate) const VIDA_MAX_S: i64 = 60;
const MAX_TOKEN: usize = 16 * 1024;
/// Cabeçalhos que trariam chave, URL de chave ou extensão crítica: nunca aceitos.
const CABECALHOS_PROIBIDOS: &[&str] = &["jku", "jwk", "x5u", "x5c", "x5t", "x5t#S256", "crit", "b64", "zip"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Org {
    pub(crate) id: String,
    pub(crate) rol: String,
}

/// O que sai de um token verificado; o resto do token não é guardado.
#[derive(Clone, Debug)]
pub(crate) struct Afirmacoes {
    pub(crate) sub: String,
    pub(crate) exp: i64,
    /// Emissão, em segundos: ordena tokens do mesmo `sub` (papel mais recente vence).
    pub(crate) iat: i64,
    pub(crate) org: Option<Org>,
}

/// Token separado, com o cabeçalho já conferido; afirmações só depois da assinatura.
pub(crate) struct Partes<'a> {
    pub(crate) kid: String,
    assinado: &'a str,
    corpo: &'a str,
    assinatura: Vec<u8>,
}

fn json(parte: &str) -> Result<Value, Recusa> {
    let bytes = crypto::unb64(parte).map_err(|_| Recusa::AuthInvalid)?;
    serde_json::from_slice(&bytes).map_err(|_| Recusa::AuthInvalid)
}

pub(crate) fn separar(token: &str) -> Result<Partes<'_>, Recusa> {
    if token.len() > MAX_TOKEN {
        return Err(Recusa::AuthInvalid);
    }
    let mut pedacos = token.split('.');
    let (Some(cab), Some(corpo), Some(assinatura), None) =
        (pedacos.next(), pedacos.next(), pedacos.next(), pedacos.next())
    else {
        return Err(Recusa::AuthInvalid);
    };
    let cabecalho = json(cab)?;
    let Some(cabecalho) = cabecalho.as_object() else { return Err(Recusa::AuthInvalid) };
    if cabecalho.get("alg").and_then(Value::as_str) != Some("RS256")
        || cabecalho.get("typ").is_some_and(|t| t.as_str() != Some("JWT"))
        || CABECALHOS_PROIBIDOS.iter().any(|k| cabecalho.contains_key(*k))
    {
        return Err(Recusa::AuthInvalid);
    }
    let Some(kid) = cabecalho.get("kid").and_then(Value::as_str).filter(|k| !k.is_empty() && k.len() <= 128) else {
        return Err(Recusa::AuthInvalid);
    };
    let assinatura = crypto::unb64(assinatura).map_err(|_| Recusa::AuthInvalid)?;
    Ok(Partes { kid: kid.to_owned(), assinado: &token[..cab.len() + 1 + corpo.len()], corpo, assinatura })
}

/// Assinatura contra a chave da JWKS, depois as afirmações. `agora` em segundos.
pub(crate) fn conferir(
    partes: &Partes<'_>,
    chave: &ring::signature::RsaPublicKeyComponents<Vec<u8>>,
    vinculo: &Vinculo,
    agora: i64,
) -> Result<Afirmacoes, Recusa> {
    chave
        .verify(&ring::signature::RSA_PKCS1_2048_8192_SHA256, partes.assinado.as_bytes(), &partes.assinatura)
        .map_err(|_| Recusa::AuthInvalid)?;
    afirmacoes(&json(partes.corpo)?, vinculo, agora)
}

fn afirmacoes(a: &Value, vinculo: &Vinculo, agora: i64) -> Result<Afirmacoes, Recusa> {
    let recusa = Err(Recusa::AuthInvalid);
    let texto = |k: &str| a[k].as_str().filter(|s| !s.is_empty() && s.len() <= 512);
    if a["v"].as_i64() != Some(2) || texto("iss") != Some(vinculo.emissor.as_str()) {
        return recusa;
    }
    if !texto("azp").is_some_and(|azp| vinculo.azp.iter().any(|p| p == azp)) || texto("sid").is_none() {
        return recusa;
    }
    let (Some(exp), Some(nbf), Some(iat)) = (a["exp"].as_i64(), a["nbf"].as_i64(), a["iat"].as_i64()) else {
        return recusa;
    };
    let vida = exp.checked_sub(iat);
    if !vida.is_some_and(|v| v > 0 && v <= VIDA_MAX_S)
        || agora >= exp.saturating_add(FOLGA_S)
        || nbf > agora.saturating_add(FOLGA_S)
        || iat > agora.saturating_add(FOLGA_S)
        || nbf > exp
    {
        return recusa;
    }
    // Sessão pendente (tarefas) ou personificação por terceiro não abrem acesso remoto.
    if a.get("sts").is_some_and(|s| s.as_str() != Some("active")) || a.get("act").is_some() {
        return recusa;
    }
    let Some(sub) = texto("sub") else { return recusa };
    let org = match a.get("o") {
        None => None,
        Some(o) => {
            let (Some(id), Some(rol)) = (o["id"].as_str(), o["rol"].as_str()) else { return recusa };
            if id.is_empty() || rol.is_empty() {
                return recusa;
            }
            Some(Org { id: id.to_owned(), rol: rol.to_owned() })
        }
    };
    Ok(Afirmacoes { sub: sub.to_owned(), exp, iat, org })
}
