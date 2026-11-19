//! Invalidação no canal existente; nunca transportar snapshot privado pelo broadcast.
use super::{EVENT, access};
use crate::{
    api::devices::Caller,
    hub::{Hub, Runtime},
};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) fn runtime_changed(hub: &Hub, id: &str, before: Option<&Runtime>, after: &Runtime) {
    let Some(row) = hub.store.bot(id).ok().flatten().filter(|r| !r.deleted && !r.config.is_group()) else {
        return;
    };
    let mut spaces = BTreeSet::from([row.config.space_id.clone()]);
    for rt in before.into_iter().chain(std::iter::once(after)) {
        if let Some(lane) = &rt.lane
            && let Ok(space) = crate::fork::spaces::chat_space(hub, &lane.chat)
        {
            spaces.insert(space);
        }
    }
    for space in spaces {
        let mut previous = json!({});
        let mut current = json!({});
        // Comparar somente projeções canônicas. Mudança privada que continua
        // indisponível não emite hint no Space alheio, nem na primeira observação.
        crate::fork::runtime_audiencia::project_in_space(hub, &row.config, &space, before, &mut previous);
        crate::fork::runtime_audiencia::project_in_space(hub, &row.config, &space, Some(after), &mut current);
        if previous == current {
            continue;
        }
        let chat = current["workingChat"].as_str().or_else(|| previous["workingChat"].as_str()).unwrap_or(id);
        let _ = hub.events.send(json!({"type":EVENT,"spaceId":space,"botId":id,"conversationId":chat}));
    }
}

pub(crate) fn event(hub: &Hub, caller: &Caller, space: &str, input: &Value) -> Option<Value> {
    if input["type"] != EVENT || input["spaceId"] != space || access::require(hub, caller).is_err() {
        return None;
    }
    hub.store.require_space(space).ok()?;
    // Vínculo removido: a invalidação capturada antes do commit revalida ingresso e Space, sem exigir o bot.
    if input["rosterChanged"] != true {
        let bot = input["botId"].as_str()?;
        let chat = input["conversationId"].as_str()?;
        let row = hub.store.bot(bot).ok().flatten().filter(|r| !r.deleted && !r.config.is_group())?;
        // Bot no roster e conversa aberta isoladamente não provam a relação.
        // Revalidar pela fonte canônica, sem exigir que a lane ainda seja a capturada:
        // a saída legítima também precisa invalidar o cartão do contexto anterior.
        if !crate::fork::runtime_audiencia::permits_conversation_in_space(hub, &row.config, space, chat) {
            return None;
        }
    }
    access::require(hub, caller).ok()?;
    Some(json!({"type":EVENT,"hubId":hub.host_id,"spaceId":space}))
}
