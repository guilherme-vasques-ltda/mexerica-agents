//! Orçamento do mesmo pedido humano queued; não é orçamento de envio ou D72.
use super::{
    BotConfig, LockExt, Origin, Phase, Result, Store, Ticket, captured, ensure, json, key, memory_matches_on, write,
};

/// Reabrir o preparo conserva a captura original, inclusive extensões e orçamento.
pub(crate) fn reuse_reserved(
    store: &Store,
    cfg: &BotConfig,
    ticket: &Ticket,
    chat: &str,
    token: &str,
    origin: &Origin,
    ids: &[String],
) -> Result<()> {
    let db = store.db.locked();
    let (record, _) = captured(&db, &cfg.id, ticket)?;
    ensure!(record.phase == Phase::Reserved, "admission_permission_already_used");
    ensure!(
        record.execution_token == token
            && record.conversation_id == chat
            && &record.origin == origin
            && record.entry_ids == ids,
        "admission_dispatch_conflict"
    );
    ensure!(memory_matches_on(&db, cfg, ticket)?, "admission_memory_policy_changed");
    Ok(())
}

/// O commit precede `capture_engine/session/compose`. Crash durante preparo gasta
/// a tentativa já reservada; nunca devolve orçamento ou autoriza uma quarta.
pub(crate) fn begin_human_setup(store: &Store, bot: &str, ticket: &Ticket) -> Result<()> {
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    let (record, mut value) = captured(&tx, bot, ticket)?;
    ensure!(record.phase == Phase::Reserved, "admission_permission_already_used");
    ensure!(record.origin == Origin::Human && !record.entry_ids.is_empty(), "admission_setup_not_human");
    ensure!(record.setup_attempts < 3 || record.setup_ready, "admission_setup_exhausted");
    value["setupAttempts"] = json!(record.setup_attempts + u8::from(!record.setup_ready));
    value["setupReady"] = json!(false);
    write(&tx, &key(bot), &value)?;
    tx.commit()?;
    Ok(())
}

/// Pausa/espera de permit não é falha de preparo, inclusive após restart.
pub(crate) fn mark_human_setup_ready(store: &Store, bot: &str, ticket: &Ticket) -> Result<()> {
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    let (record, mut value) = captured(&tx, bot, ticket)?;
    ensure!(record.phase == Phase::Reserved, "admission_permission_already_used");
    if record.origin == Origin::Human && record.setup_attempts > 0 && !record.setup_ready {
        value["setupReady"] = json!(true);
        write(&tx, &key(bot), &value)?;
    }
    tx.commit()?;
    Ok(())
}
