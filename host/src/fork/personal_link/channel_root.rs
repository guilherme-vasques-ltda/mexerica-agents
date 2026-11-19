//! Snapshot original da única cadeia entregue em B. Somente Connection recebida, nenhuma escrita.
use super::{
    canon,
    wire::{self, Contexto},
    writer,
};
use anyhow::{Context, Result, ensure};
use rusqlite::Connection;
use serde_json::{Value, json};

fn prazo_historico(d: &Value, r: &Value) -> Result<()> {
    let a = d.as_array().filter(|a| a.len() == 6).context("not_allowed: D original")?;
    let boot = a[2].as_str().context("not_allowed: bootId")?;
    ensure!(
        a[0] == wire::PRAZO
            && a[1] == 1
            && uuid::Uuid::parse_str(boot).is_ok_and(|id| id.to_string() == boot)
            && a[3] == *r
            && canon::inteiro(&a[4]).is_ok()
            && a[5] == 30_000,
        "not_allowed: D original"
    );
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RaizCanal {
    pub(super) k: Contexto,
    pub(super) t: String,
    pub(super) c: String,
    pub(super) ra: String,
    pub(super) chain: String,
    // Todos os bytes originais e referências da cabeça, não só digests.
    original: Value,
    binding: Vec<u8>,
    grant: i64,
    consent: i64,
}
impl RaizCanal {
    pub(crate) fn ler(db: &Connection, contexto: &Contexto, chave: &str) -> Result<Self> {
        let (birth, master): (Vec<u8>, String) =
            db.query_row("SELECT birth,master_key_id FROM personal_link_installation WHERE singleton=1", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
        writer::validar(db, contexto, &birth, &master)?;
        let root = writer::raiz(db, contexto, chave)?.context("not_allowed: raiz ausente")?;
        ensure!(!root.fechada, "not_allowed: raiz fechada");
        let rc = root.rc.context("not_allowed: raiz incompleta")?;
        let (gestao, mr) = root.gestao.context("not_allowed: gestão ausente")?;
        let ra = root.ra;
        let admissao_payload = ra.payload();
        let completo_payload = rc.payload();
        let gestao_payload = gestao.payload();
        let recibo_payload = mr.payload();
        let (root_id, pc_bytes, ch_bytes, ac_bytes): (String,Vec<u8>,Vec<u8>,Vec<u8>)=db.query_row(
            "SELECT r.root_id,p.bytes,ch.bytes,ac.bytes FROM personal_link_roots r JOIN personal_link_journal p ON p.seq=r.persisted_seq JOIN personal_link_journal ch ON ch.seq=r.challenge_seq JOIN personal_link_journal ac ON ac.seq=r.accept_seq WHERE r.key=? AND r.state='complete'",[chave],|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?;
        let ch = canon::verificar(&canon::ler(&ch_bytes)?, wire::DESAFIO, 14, &contexto.h)?;
        let ac = wire::aceite(&canon::ler(&ac_bytes)?, &ch)?;
        let pc = canon::verificar(&canon::ler(&pc_bytes)?, wire::PERSISTIDO, 12, chave)?;
        let cp = pc.payload();
        for (prazo, runtime) in
            [(&ch.payload()[10], &ch.payload()[5]), (&cp[11], &cp[5]), (&gestao_payload[12], &gestao_payload[5])]
        {
            prazo_historico(prazo, runtime)?;
            canon::b64n::<32>(runtime)?;
        }
        for value in [&ch.payload()[8], &ch.payload()[9], &cp[10], &gestao_payload[11]] {
            canon::b64n::<32>(value)?;
        }
        canon::b64n::<16>(&gestao_payload[6])?;

        ensure!(
            admissao_payload[8] == root_id
                && admissao_payload[11] == ch.digest()?
                && admissao_payload[12] == ac.digest()?
                && cp[2..5] == admissao_payload[2..5]
                && cp[6..8] == admissao_payload[6..8]
                && cp[8] == admissao_payload[10]
                && cp[9] == ra.digest()?
                && completo_payload[5] == cp[5]
                && completo_payload[8] == pc.digest()?
                && cp[11][3] == cp[5],
            "not_allowed: cadeia original divergente"
        );
        let contexto_cliente = canon::hex64(&admissao_payload[4])?.to_owned();
        canon::b64n::<32>(&admissao_payload[2])?;
        canon::b64n::<32>(&admissao_payload[3])?;
        canon::b64n::<32>(&admissao_payload[7])?;
        canon::b64n::<16>(&admissao_payload[8])?;
        canon::b64n::<16>(&admissao_payload[10])?;
        ensure!(
            admissao_payload[2] == contexto.h
                && admissao_payload[3] == contexto.g
                && admissao_payload[7] == chave
                && admissao_payload[9] == contexto.revisao_instalacao
                && admissao_payload[13] == wire::p0(),
            "not_allowed: raiz divergente"
        );
        ensure!(
            completo_payload[2..5] == admissao_payload[2..5]
                && completo_payload[6] == admissao_payload[10]
                && completo_payload[7] == ra.digest()?
                && completo_payload[9] == wire::p0(),
            "not_allowed: RC divergente"
        );
        ensure!(
            gestao_payload[2..5] == admissao_payload[2..5]
                && gestao_payload[7] == chave
                && gestao_payload[8] == ra.digest()?
                && gestao_payload[9] == 1
                && gestao_payload[10] == wire::p1()
                && gestao_payload[13] == admissao_payload[9],
            "not_allowed: M divergente"
        );
        ensure!(
            recibo_payload[2..5] == admissao_payload[2..5]
                && recibo_payload[5..8] == gestao_payload[5..8]
                && recibo_payload[8] == admissao_payload[8]
                && recibo_payload[9] == ra.digest()?
                && recibo_payload[10] == gestao.digest()?
                && recibo_payload[11] == wire::p1(),
            "not_allowed: MR divergente"
        );
        let (revision,policy,grant,consent): (i64,Vec<u8>,i64,i64)=db.query_row(
            "SELECT revision,policy,grant_seq,consent_seq FROM personal_link_rights WHERE key=? ORDER BY revision DESC LIMIT 1",[chave],|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?;
        ensure!(revision == 2 && policy == canon::j(&wire::p1())?, "not_allowed: cabeça divergente");
        let (binding, tombstone): (Vec<u8>, i64) =
            db.query_row("SELECT binding,tombstone FROM personal_link_origins WHERE key=?", [chave], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
        ensure!(tombstone == 0 && super::generation(&binding, chave)? == contexto.g, "not_allowed: origem fechada");
        Ok(Self {
            k: contexto.clone(),
            t: chave.into(),
            c: contexto_cliente,
            ra: ra.digest()?,
            chain: wire::cadeia_t(&ra, &rc, &gestao, &mr)?,
            original: json!([ra.valor(), rc.valor(), gestao.valor(), mr.valor()]),
            binding,
            grant,
            consent,
        })
    }
    pub(crate) fn conferir(&self, db: &Connection) -> Result<()> {
        ensure!(*self == Self::ler(db, &self.k, &self.t)?, "not_allowed: captura da raiz mudou");
        Ok(())
    }
    pub(super) fn open(&self) -> Value {
        json!([
            "donna.personal-link/channel-open/v1",
            1,
            "manager",
            self.k.h,
            self.k.g,
            self.c,
            self.t,
            self.ra,
            self.chain
        ])
    }
}
