//! Admission boundary for webhook executions until the Hub owns chain provenance.
use crate::{
    hub::Hub,
    routines::{Routine, hooks::Refused},
    store::{EntryKind, Lane},
};
use serde_json::json;

/// Refuse rather than invent a new root from an external request. The context is
/// deliberately not derived from the event, delivery ID, headers, or cloud.
pub(crate) fn admit(hub: &Hub, routine: &Routine) -> Result<(), Refused> {
    let _ = hub.add_entry_once(
        &format!("routine-chain-unavailable-{}", routine.id),
        &Lane::main(&routine.bot_id),
        EntryKind::Notice,
        0,
        &json!({"text":"Webhook bloqueado: controle de cadeia do Hub indisponível.", "style":"error", "routineId":routine.id}),
    );
    Err(Refused::ChainUnavailable)
}
