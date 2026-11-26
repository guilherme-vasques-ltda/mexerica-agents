//! Fila de mensagens da conversa da Open: a mensagem enviada com uma resposta em curso fica `queued` e o Hub a entrega,
//! em ordem de envio, quando a resposta termina. O app só mostra o estado; nada aqui é lista nova da Open.
use crate::LockExt;
use crate::fork::autoridade::Autoridade;
use crate::fork::open_chat::{RUNNING, Running, admit_locked, mark_unsent, run};
use crate::hub::Hub;
use crate::store::{BotConfig, Lane};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

struct Queued {
    hub: Arc<Hub>,
    cfg: BotConfig,
    lane: Lane,
    turn: i64,
    entry: (String, Option<Autoridade>),
    text: String,
}

#[cfg(test)]
#[path = "open_chat_fila_tests.rs"]
mod tests;

/// Sempre tomada com `RUNNING` já segura: a decisão "tem resposta em curso?" e a entrada na fila são um passo só.
static PENDING: Mutex<Option<HashMap<String, VecDeque<Queued>>>> = Mutex::new(None);

/// Entrega a mensagem já gravada: começa o turno se o bot está livre, senão entra no fim da fila do bot.
pub(crate) fn submit(
    hub: &Arc<Hub>,
    cfg: &BotConfig,
    lane: Lane,
    turn: i64,
    entry: (String, Option<Autoridade>),
    text: String,
) {
    let mut guard = RUNNING.locked();
    start_or_queue(&mut guard, Queued { hub: hub.clone(), cfg: cfg.clone(), lane, turn, entry, text });
}

fn start_or_queue(guard: &mut Option<HashMap<String, Running>>, queued: Queued) {
    let running = guard.get_or_insert_with(HashMap::new);
    running.retain(|_, r| r.is_live());
    if running.contains_key(&queued.cfg.id) {
        PENDING.locked().get_or_insert_with(HashMap::new).entry(queued.cfg.id.clone()).or_default().push_back(queued);
        return;
    }
    let Queued { hub, cfg, lane, turn, entry, text } = queued;
    match admit_locked(guard, &hub, &cfg) {
        Ok(ticket) => run(&hub, ticket, &cfg, lane, turn, entry, text),
        Err(error) => {
            let message = error.to_string();
            let code = message.split(':').next().unwrap_or_default();
            mark_unsent(&hub, &entry.0, code, "");
        }
    }
}

/// Chamada quando o turno do bot termina: libera a reserva e começa o próximo da fila, se houver. Sob a mesma trava
/// em que `submit` decide, então uma mensagem nunca fica esperando um turno que já acabou.
pub(crate) fn finish(hub: &Arc<Hub>, bot: &str) {
    let mut guard = RUNNING.locked();
    if let Some(running) = guard.as_mut() {
        running.remove(bot);
    }
    loop {
        let next = PENDING.locked().as_mut().and_then(|pending| pending.get_mut(bot)).and_then(VecDeque::pop_front);
        let Some(mut next) = next else { return };
        next.hub = hub.clone();
        start_or_queue(&mut guard, next);
        if guard.as_ref().is_some_and(|running| running.contains_key(bot)) {
            return;
        }
    }
}

/// A fila vive na memória: na partida, o que ficou `queued` de uma execução anterior nunca será entregue. Marca como
/// não enviada com o motivo, em vez de deixar a mensagem presa e contando no limite da fila.
pub(crate) fn recover(store: &crate::store::Store) {
    let ids: Vec<String> = {
        let db = store.db.locked();
        let Ok(mut stmt) = db.prepare(
            "SELECT id FROM entries WHERE kind = 'user'
                AND (CASE WHEN json_valid(data) THEN json_extract(data, '$.status') END) = 'queued'",
        ) else {
            return;
        };
        let Ok(rows) = stmt.query_map([], |row| row.get(0)) else { return };
        rows.flatten().collect()
    };
    for id in ids {
        let Some(mut data) = store.entry(&id).map(|entry| entry.data) else { continue };
        data["status"] = serde_json::json!("failed");
        data["reason"] = serde_json::json!({"code": "open_chat_failed", "accountId": ""});
        if let Err(error) = store.update_entry(&id, &data) {
            tracing::warn!(%error, "mensagem da fila da Open não foi marcada na partida");
        }
    }
}

/// `stop` interrompe a resposta e também as mensagens que esperavam depois dela.
pub(crate) fn cancel_pending(bot: &str) {
    let _running = RUNNING.locked();
    let dropped = PENDING.locked().as_mut().and_then(|pending| pending.remove(bot)).unwrap_or_default();
    for queued in dropped {
        mark_unsent(&queued.hub, &queued.entry.0, "open_chat_cancelled", "");
    }
}
