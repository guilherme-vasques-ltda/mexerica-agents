//! Fronteira transacional do produtor. IDs aleatórios e leituras auxiliares ficam fora dos callbacks.
use super::modelo::{ContextoPersistido, Escopo, Fase, Origem, Pedido, Recibo, Recusa, Tipo};
use crate::fork::atividades::{modelo::CoreError, store::with_store_tx};
use crate::store::Store;
use anyhow::Result;
use std::sync::TryLockError;
use std::time::Duration;

const ESPERA_ORFAS: Duration = Duration::from_millis(200);

pub(crate) fn pedido(
    tipo: Tipo,
    evento: String,
    intencao: String,
    bot: &str,
    run: &str,
    anterior: Option<Recibo>,
) -> Pedido {
    Pedido {
        tipo,
        evento,
        intencao,
        anterior,
        candidato: Recibo {
            chain_id: uuid::Uuid::new_v4().to_string(),
            admission_id: uuid::Uuid::new_v4().to_string(),
            run_id: run.into(),
            bot_id: bot.into(),
            level: 1,
            ordinal: 1,
        },
        rotina: None,
        origem: Origem::Interna,
        escopo: Escopo::ConversaDoBot { bot_id: bot.into(), space_id: None },
    }
}

pub(crate) fn admitir(store: &Store, p: &Pedido) -> Result<Recibo> {
    Ok(with_store_tx(store, |tx| tx.chain_admit(p))??)
}
pub(crate) fn despachar(store: &Store, r: &Recibo) -> Result<()> {
    Ok(with_store_tx(store, |tx| tx.chain_dispatch(r))??)
}
pub(crate) fn concluir(store: &Store, r: &Recibo) -> Result<()> {
    Ok(with_store_tx(store, |tx| tx.chain_finish(r))?)
}

pub(crate) fn fase(store: &Store, r: &Recibo) -> Result<Fase> {
    let db = store.db.try_lock().map_err(|_| CoreError::StoreBusy)?;
    super::store::phase(&db, r)
}

/// Herda o contexto original persistido; não resolve uma concessão nova no cadastro.
pub(crate) fn contexto(store: &Store, r: &Recibo) -> Result<ContextoPersistido> {
    let db = store.db.try_lock().map_err(|_| CoreError::StoreBusy)?;
    Ok(super::store::contexto(&db, r)?.ok_or(Recusa::ReciboInvalido)?)
}

pub(crate) fn vinculada(store: &Store, r: &Recibo) -> Result<bool> {
    let db = store.db.try_lock().map_err(|_| CoreError::StoreBusy)?;
    super::store::vinculada(&db, r)
}

/// Uma chamada manual sem nonce retoma só a reserva que ainda não ganhou Run.
pub(crate) fn manual_pendente(store: &Store, rotina: &str) -> Result<Option<String>> {
    use rusqlite::OptionalExtension;
    let db = store.db.try_lock().map_err(|_| CoreError::StoreBusy)?;
    Ok(db.query_row(
        "SELECT occurrence FROM donna_chain_roots WHERE routine_id=?1 AND source='manual' AND linked=0 ORDER BY rowid LIMIT 1",
        [rotina],
        |row| row.get(0),
    ).optional()?)
}

/// Única leitura que espera pelo banco (até 200 ms): o tick da agenda, os limites de uso e as conversas
/// o seguram por instantes e recusar na hora gerava `StoreBusy` (MEX-1434). Só pode ser chamada sem
/// nenhuma trava de Rotinas ou do Store; as demais leituras daqui recusam na hora e quem precisa de
/// espera repete a operação inteira fora das travas (`retry_store_busy`).
pub(crate) fn orfas(store: &Store) -> Result<Vec<(String, String, String)>> {
    let db = store.db.try_lock_for(ESPERA_ORFAS).map_err(|error| match error {
        TryLockError::WouldBlock => CoreError::StoreBusy,
        TryLockError::Poisoned(_) => CoreError::StorePoisoned,
    })?;
    Ok(db
        .prepare("SELECT id,bot_id,routine_id FROM donna_chain_roots WHERE linked=0 AND routine_id IS NOT NULL")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

/// Revalida a reserva antes do aviso; quem publica mantém o estado de Rotinas travado.
pub(crate) fn orfa_pendente(store: &Store, chain: &str) -> Result<bool> {
    let db = store.db.try_lock().map_err(|_| CoreError::StoreBusy)?;
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM donna_chain_roots WHERE id=?1 AND linked=0)", [chain], |row| {
        row.get(0)
    })?)
}

/// Só persiste digest; ordem das chaves do payload não cria uma intenção diferente.
pub(crate) fn digest<T: serde::Serialize>(value: &T) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let mut value = serde_json::to_value(value)?;
    value.sort_all_objects();
    let bytes = serde_json::to_vec(&value)?;
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(digest, "{byte:02x}")?;
    }
    Ok(digest)
}
