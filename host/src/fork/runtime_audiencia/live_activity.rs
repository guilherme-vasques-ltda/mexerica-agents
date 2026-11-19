//! Fio móvel existente: só estados observados para a audiência nativa podem ser enviados.
use crate::hub::{BotStatus, Hub};
use serde_json::{Value, json};

const SWIFT_REFERENCE_EPOCH: f64 = 978_307_200.0;

struct Activity {
    status: BotStatus,
    started_at: Option<i64>,
}

impl Activity {
    fn terminal(&self) -> bool {
        matches!(self.status, BotStatus::Idle | BotStatus::Error)
    }

    fn payload(&self) -> Value {
        #[allow(clippy::cast_precision_loss)]
        let started_at = self.started_at.map(|ms| ms as f64 / 1000.0 - SWIFT_REFERENCE_EPOCH);
        let timestamp = crate::store::now_ms() / 1000;
        json!({
            "event": if self.terminal() { "end" } else { "update" },
            "timestamp": timestamp,
            "staleDate": timestamp + 15 * 60,
            "contentState": {"status": self.status, "activity": "", "startedAt": started_at},
        })
    }
}

fn current(hub: &Hub, bot_id: &str) -> Option<Activity> {
    let row = hub.store.bot(bot_id).ok().flatten().filter(|row| !row.deleted)?;
    let state = hub.bot_json(&row);
    let status = match state["status"].as_str()? {
        "idle" => BotStatus::Idle,
        "working" => BotStatus::Working,
        "needsInput" => BotStatus::NeedsInput,
        "waiting" => BotStatus::Waiting,
        "error" => BotStatus::Error,
        // Ausência, desconhecido e unavailable nunca se tornam um estado interno.
        _ => return None,
    };
    // Repouso global sem lane não comprova de qual conversa terminou o trabalho.
    // Não encerra um ticket pessoal ao limpar a lane de um turno estrangeiro.
    if status == BotStatus::Idle && state["workingChat"].as_str().is_none() {
        return None;
    }
    // Registro pode chegar entre o envio humano e o início do ator. A proteção vale
    // também para a chamada direta do Hub, sem terminar o ticket durante essa janela.
    if status == BotStatus::Idle && hub.store.last_message(bot_id).is_some_and(|m| m.author.is_none()) {
        return None;
    }
    Some(Activity { status, started_at: state["startedAt"].as_i64() })
}

/// O mesmo caminho serve registro e mudanças de runtime, sem aceitar Runtime cru.
/// Callbacks síncronos permitem capturar o transporte em D70 sem rede/conta/relay reais.
pub(crate) fn send(
    hub: &Hub,
    bot_id: &str,
    relay: impl FnOnce() -> Option<String>,
    mut post: impl FnMut(String, Value),
) {
    if current(hub, bot_id).is_none() {
        return;
    }
    let Some(relay) = relay() else { return };
    // Revalida depois de preparar o transporte e antes de consumir qualquer ticket.
    let Some(activity) = current(hub, bot_id) else { return };
    let tickets =
        if activity.terminal() { hub.store.take_activity_tickets(bot_id) } else { hub.store.activity_tickets(bot_id) };
    let payload = activity.payload();
    for ticket in tickets {
        post(format!("{relay}/push"), json!({"ticket":ticket,"liveActivity":payload}));
    }
}
