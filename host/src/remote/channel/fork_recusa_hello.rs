use super::fork_assinatura_terminal::{Recusa, RecusaEstavel};
use crate::api::devices::RejectCode;
use crate::remote::{crypto, identity::Identity};
use rusqlite::Connection;

fn ler_fato(db: &Connection, dk: &str) -> anyhow::Result<Option<RecusaEstavel>> {
    if crate::store::personal_bootstrap::personal_origin_on(db, dk)?.is_some() {
        return Ok(None);
    }
    let (tem_linha, bloqueado, geracao): (bool, bool, Option<i64>) = db.query_row(
        "SELECT d.key IS NOT NULL, EXISTS(
             SELECT 1 FROM fork_workspace_revoked_devices r WHERE r.key=?1
             AND (d.key IS NULL OR r.grant_id=COALESCE(d.grant_id,''))),
             CASE WHEN d.key IS NULL THEN (SELECT geracao FROM donna_device_concessao WHERE chave=?1) END
         FROM (SELECT 1) LEFT JOIN devices d ON d.key=?1",
        [dk],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if bloqueado {
        return Ok(Some(RecusaEstavel::DeviceRevoked));
    }
    if tem_linha
        && crate::store::device_on(db, dk)?
            .is_some_and(|d| d.platform == "web" && !d.concessao_web_valida(crate::store::now_ms()))
    {
        return Ok(Some(RecusaEstavel::DeviceUnpaired));
    }
    if !tem_linha && geracao.is_some_and(|g| g > 1 && g < i64::MAX) {
        return Ok(Some(RecusaEstavel::DeviceUnpaired));
    }
    Ok(None)
}

pub(super) fn fato(db: &Connection, dk: &str) -> Option<RecusaEstavel> {
    let Ok(fato) = ler_fato(db, dk) else {
        tracing::warn!("não foi possível ler o fato persistido da recusa do canal");
        return None;
    };
    fato
}

impl RecusaEstavel {
    pub(super) fn codigo(self) -> RejectCode {
        match self {
            Self::DeviceRevoked => RejectCode::Revoked,
            Self::DeviceUnpaired => RejectCode::Unauthorized,
        }
    }
}

pub(super) fn prova(db: &Connection, recusa: &Recusa, identidade: &Identity) -> Option<[u8; 64]> {
    (fato(db, &crypto::b64(&recusa.dk)) == Some(recusa.estavel)).then(|| recusa.assinar(identidade))
}
