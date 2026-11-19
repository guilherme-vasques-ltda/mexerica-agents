// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Todos os gatilhos (turno, Flush, compactação e retomada) consomem a fila com prova durável.
use crate::{LockExt, chat::naming, hub::Hub, store::Store};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

pub enum DreamEvent {
    Turn(naming::Turn),
    Wake,
    Flush,
}
pub(crate) enum Ran {
    Done { more: bool },
    Later,
}

pub(super) fn set_bookmark(store: &Store, bot: &str, seq: i64) -> Result<()> {
    store.kv_set(&crate::fork::memory::dream_status::bookmark_key(bot), &seq.to_string())
}

pub fn spawn_dreamer(hub: Arc<Hub>, bot_id: String) -> mpsc::UnboundedSender<DreamEvent> {
    crate::fork::memory::dream_worker::spawn_dreamer(hub, bot_id)
}

fn learning_off(store: &Store) -> Result<bool> {
    Ok(crate::fork::memory_learning::mode(store)? == crate::fork::memory_learning::Mode::Off)
}

/// Em `off` (D389) nada conta como pendente: o consumidor fica quieto, sem laço de reexecução.
pub(crate) fn waiting(store: &Store, bot: &str) -> Result<usize> {
    if learning_off(store)? {
        return Ok(0);
    }
    crate::fork::memory::dream_status::read_bookmark(store, bot)?;
    let db = store.db.locked();
    crate::fork::memory_auto::queue::recover(&db, crate::store::now_ms())?;
    let count: i64 = db.query_row(
        "SELECT COUNT(*) FROM donna_memory_auto_jobs WHERE bot=?1 AND status='waiting'
        AND outcome='success' AND scope='allowed'",
        [bot],
        |r| r.get(0),
    )?;
    Ok(usize::try_from(count)?)
}

pub(crate) fn review_due(store: &Store, bot: &str) -> Result<bool> {
    if learning_off(store)? {
        return Ok(false);
    }
    // Sem prova durável não se varre histórico antigo, nem mesmo na revisão diária.
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    if crate::fork::memory::dream_status::read_reviewed(store, bot)?.as_deref() == Some(today.as_str()) {
        return Ok(false);
    }
    crate::fork::memory_auto::queue::schedule_review(store, bot)
}

pub(crate) async fn run(hub: &Arc<Hub>, bot_id: &str) -> Result<Ran> {
    // D389 `off`: pedidos antigos esperam na fila, sem chamar o modelo.
    if learning_off(&hub.store)? {
        return Ok(Ran::Done { more: false });
    }
    crate::fork::background_memory_auto::run(hub, bot_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    // A cobertura `external_content_is_redacted_before_consolidation` da main foi
    // portada para memory_auto::tests: prova durável e respostas reais de genUI,
    // sem reintroduzir a consolidação direta do histórico.
    #[test]
    fn fechamento_legado_ou_flush_nao_inventa_trabalho() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.memory_session_start("bot", "legacy", false).unwrap();
        store.memory_session_turn("bot", "bot", "legacy", 1).unwrap();
        store.memory_turn_begin("bot", "bot", "legacy", &[1], "owner").unwrap();
        store.memory_turn_finish("bot", "legacy").unwrap();
        assert_eq!(waiting(&store, "bot").unwrap(), 0);
        assert!(!review_due(&store, "bot").unwrap());
    }
}
