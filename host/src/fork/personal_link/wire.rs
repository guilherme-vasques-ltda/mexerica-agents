//! Objetos EXATOS do complemento v2 §2.2 e da proposta §1/§4 (arrays, domínios e ordem).
//! Não há assinador genérico: cada função monta um único objeto fechado.
use super::canon::{self, Assinado};
use crate::remote::identity::Identity;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

pub(crate) const DESAFIO: &str = "donna.personal-link/initial-challenge/v1";
pub(crate) const ACEITE: &str = "donna.personal-link/initial-accept/v1";
pub(crate) const GESTAO: &str = "donna.personal-link/initial-management/v1";
pub(crate) const ADMISSAO: &str = "donna.personal-link/root-admission/v1";
pub(crate) const PERSISTIDO: &str = "donna.personal-link/initial-persisted/v1";
pub(crate) const COMPLETO: &str = "donna.personal-link/initial-complete/v1";
pub(crate) const RECIBO_GESTAO: &str = "donna.personal-link/initial-management-receipt/v1";
pub(crate) const PRAZO: &str = "donna.personal-link/deadline/v1";
/// Marca local de nascimento D430 (registro do Hub, não wire de cliente).
pub(crate) const NASCIMENTO: &str = "donna.personal-link/installation-birth/v1";

pub(crate) const POLITICA_ID: &str = "donna.personal-link/core";

/// `P=[policyId,policyVersion,policyDigest,rights,delegation,rightsRevision]`.
pub(crate) fn politica(direitos: &[&str], revisao: u64) -> Value {
    json!([POLITICA_ID, 1, super::policy_digest(), direitos, false, revisao])
}
/// P0: connect-only, revisão 1.
pub(crate) fn p0() -> Value {
    politica(&["link.connect"], 1)
}
/// P1: gestão inicial explícita, sem delegação, revisão 2.
pub(crate) fn p1() -> Value {
    politica(&["devices.manage", "link.connect"], 2)
}

/// `D=["donna.personal-link/deadline/v1",1,bootId,R,bornBoottimeMs,ttlMs]`.
pub(crate) fn prazo(boot_id: &str, runtime: &str, nascido_ms: u64, ttl_ms: u64) -> Value {
    json!([PRAZO, 1, boot_id, runtime, nascido_ms, ttl_ms])
}

/// Contexto fixo de H nesta instância. Só a instalação aberta o constrói.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Contexto {
    pub h: String,
    pub g: String,
    pub instalacao: String,
    pub revisao_instalacao: u64,
}

pub(crate) struct Desafio<'a> {
    pub c: &'a str,
    pub r: &'a str,
    pub op: &'a str,
    pub t: &'a str,
    pub n_t: &'a str,
    pub n_h: &'a str,
    pub d30: &'a Value,
    pub root_id: &'a str,
}

pub(crate) fn desafio(id: &Identity, k: &Contexto, d: &Desafio<'_>) -> Result<Assinado> {
    canon::assinar(
        id,
        json!([DESAFIO, 1, k.h, k.g, d.c, d.r, d.op, d.t, d.n_t, d.n_h, d.d30, d.root_id, k.revisao_instalacao, p0()]),
    )
}

/// T → H: `[initial-accept,1,H,G,C,R,op,T,N_T,N_H,hash(signedChallenge),P0]`, assinado T.
pub(crate) fn aceite(valor: &Value, desafio: &Assinado) -> Result<Assinado> {
    let d = desafio.payload();
    let t = d[7].as_str().unwrap_or_default();
    let aceite = canon::verificar(valor, ACEITE, 12, t)?;
    let a = aceite.payload();
    ensure!(
        a[2..10] == d[2..10] && a[10] == desafio.digest()? && a[11] == p0(),
        "not_allowed: aceite não corresponde ao desafio atual"
    );
    Ok(aceite)
}

pub(crate) struct Admissao<'a> {
    pub desafio: &'a Assinado,
    pub aceite: &'a Assinado,
    pub admission_id: &'a str,
}

/// `RA=[root-admission,1,H,G,C,R,op,T,rootId,installRevision,admissionId,hash(sCh),hash(sAc),P0]`, assinado H.
pub(crate) fn admissao(id: &Identity, a: &Admissao<'_>) -> Result<Assinado> {
    let d = a.desafio.payload();
    canon::assinar(
        id,
        json!([
            ADMISSAO,
            1,
            d[2],
            d[3],
            d[4],
            d[5],
            d[6],
            d[7],
            d[11],
            d[12],
            a.admission_id,
            a.desafio.digest()?,
            a.aceite.digest()?,
            p0()
        ]),
    )
}

pub(crate) fn ler_admissao(valor: &Value, h: &str) -> Result<Assinado> {
    let ra = canon::verificar(valor, ADMISSAO, 14, h)?;
    ensure!(ra.payload()[13] == p0(), "not_allowed: política da raiz divergente");
    Ok(ra)
}

/// `PC=[initial-persisted,1,H,G,C,Ratual,op,T,admissionId,hash(signedRA),N_H2,D30]`, assinado T.
pub(crate) fn persistido(valor: &Value, ra: &Assinado, r_atual: &str, n_h2: &str, d30: &Value) -> Result<Assinado> {
    let r = ra.payload();
    let t = r[7].as_str().unwrap_or_default();
    let pc = canon::verificar(valor, PERSISTIDO, 12, t)?;
    let p = pc.payload();
    ensure!(
        p[2..5] == r[2..5]
            && p[5] == r_atual
            && p[6] == r[6]
            && p[7] == r[7]
            && p[8] == r[10]
            && p[9] == ra.digest()?
            && p[10] == n_h2
            && p[11] == *d30,
        "not_allowed: confirmação de persistência divergente"
    );
    Ok(pc)
}

/// `RC=[initial-complete,1,H,G,C,Ratual,admissionId,hash(signedRA),hash(signedPC),P0]`, assinado H.
pub(crate) fn completo(id: &Identity, ra: &Assinado, pc: &Assinado) -> Result<Assinado> {
    let r = ra.payload();
    let p = pc.payload();
    canon::assinar(id, json!([COMPLETO, 1, r[2], r[3], r[4], p[5], r[10], ra.digest()?, pc.digest()?, p0()]))
}

pub(crate) struct Gestao<'a> {
    pub r: &'a str,
    pub op: &'a str,
    pub t: &'a str,
    pub c: &'a str,
    pub digest_raiz: &'a str,
    pub revisao_anterior: u64,
    pub n_h: &'a str,
    pub d30: &'a Value,
}

/// Payload a ser assinado por T:
/// `[initial-management,1,H,G,C,R,op,T,rootAdmissionDigest,oldRightsRevision,P1,N_H,D30,installRevision]`.
pub(crate) fn gestao_payload(k: &Contexto, g: &Gestao<'_>) -> Value {
    json!([
        GESTAO,
        1,
        k.h,
        k.g,
        g.c,
        g.r,
        g.op,
        g.t,
        g.digest_raiz,
        g.revisao_anterior,
        p1(),
        g.n_h,
        g.d30,
        k.revisao_instalacao
    ])
}

pub(crate) fn gestao(valor: &Value, esperado: &Value) -> Result<Assinado> {
    let t = esperado[7].as_str().unwrap_or_default();
    let m = canon::verificar(valor, GESTAO, 14, t)?;
    ensure!(m.valor()[0] == *esperado, "not_allowed: consentimento de gestão divergente");
    Ok(m)
}

/// `MR=[initial-management-receipt,1,H,G,C,R,op2,T,rootId,hash(signedRA),hash(signedManagement),P1]`, assinado H.
pub(crate) fn recibo_gestao(id: &Identity, ra: &Assinado, m: &Assinado) -> Result<Assinado> {
    let r = ra.payload();
    let g = m.payload();
    canon::assinar(
        id,
        json!([RECIBO_GESTAO, 1, r[2], r[3], r[4], g[5], g[6], r[7], r[8], ra.digest()?, m.digest()?, p1()]),
    )
}

/// `chainDigestT = SHA256(J([signedRA,signedRC,signedManagement,signedMR]))`, sem J duplo.
pub(crate) fn cadeia_t(ra: &Assinado, rc: &Assinado, m: &Assinado, mr: &Assinado) -> Result<String> {
    canon::hash(&json!([ra.valor(), rc.valor(), m.valor(), mr.valor()]))
}

/// Marca de nascimento: `[installation-birth,1,H,box,G,installId,installRevision,masterKeyId]`.
pub(crate) fn nascimento(id: &Identity, k: &Contexto, master_key_id: &str) -> Result<Assinado> {
    canon::assinar(
        id,
        json!([NASCIMENTO, 1, k.h, id.box_pub_b64(), k.g, k.instalacao, k.revisao_instalacao, master_key_id]),
    )
}
