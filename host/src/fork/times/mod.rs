//! Quadro administrativo do dono. Consumidores: `cadeia` (D320), leitura da própria
//! cadeia, um nível acima e um abaixo, e `resumo` (MB-3, resumo do dia). Nenhuma política de execução.
pub(crate) mod api;
pub(crate) mod cadeia;
pub(crate) mod listagem;
pub(crate) mod resumo;
#[cfg(test)]
pub(crate) use resumo::{resumo_testes_change, resumo_testes_snapshot};
mod lider;
mod model;
mod schema;
mod source;
mod store;
mod validation;

/// Tabelas e, em seguida, a migração da regra do chefe único (D340).
pub(crate) fn migrate(c: &rusqlite::Connection) -> anyhow::Result<()> {
    schema::migrate(c)?;
    lider::migrate(c)
}
pub(crate) use store::on_bot_deleted;

/// Recusa antes de parar ator/rotinas; a mesma condição é revalidada na transação SQL.
pub(crate) fn check_bot_deletion(store: &crate::store::Store, id: &str) -> anyhow::Result<()> {
    use crate::LockExt;
    store::check_bot_deletion(&store.db.locked(), id)
}

#[cfg(test)]
mod cadeia_testes;
#[cfg(test)]
mod tests;
