//! D538/D544: compatibilidade de persistência da antiga Agenda, sem API ou autorização.
//! Conserva eventos, recursos e grants existentes; não provisiona nem habilita nada.
use super::{model::PluginError, store::db_error};
use rusqlite::Connection;

pub(super) fn migrate(c: &Connection) -> Result<(), PluginError> {
    let tx = c.unchecked_transaction().map_err(db_error)?;
    tx.execute_batch(include_str!("agenda.sql")).map_err(db_error)?;
    tx.commit().map_err(db_error)
}
