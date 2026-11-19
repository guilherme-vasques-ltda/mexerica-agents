//! API de grupos: serializa edição, recuperação e publicação por revisão.
use super::Hub;
use crate::{LockExt, store::bot_groups::Edit};
use anyhow::{Result, anyhow, ensure};
use serde_json::{Value, json};
use std::{ops::ControlFlow, sync::Arc};

pub(crate) async fn call(hub: &Arc<Hub>, method: &str, body: Value) -> Result<ControlFlow<Value, Value>> {
    if !matches!(method, "botGroups" | "botGroupsEdit") {
        return Ok(ControlFlow::Continue(body));
    }
    let space = match body.get("spaceId") {
        None => crate::fork::spaces::PERSONAL,
        Some(value) => value.as_str().ok_or_else(|| anyhow!("spaceId inválido"))?,
    };
    if method == "botGroups" {
        return Ok(ControlFlow::Break(serde_json::to_value(hub.store.bot_groups_in_space(space)?)?));
    }
    if body["op"] == "place" {
        ensure!(body.get("groupId").is_some(), "groupId ausente");
    }
    let edit: Edit = serde_json::from_value(body.clone())?;
    let value = change(hub, space, edit).await?;
    Ok(ControlFlow::Break(value))
}

pub(super) async fn change(hub: &Arc<Hub>, space: &str, edit: Edit) -> Result<Value> {
    let _team = hub.store.team_lock.lock().await;
    crate::chat::manage::recover_live(hub);
    let _emit = hub.emit_lock.locked();
    let (snapshot, created, affected) = hub.store.edit_bot_groups(space, edit)?;
    for id in affected {
        if let Some(bot) = hub.bot_value(&id) {
            let _ = hub.events.send(json!({"type":"bot","rev":bot["rev"],"bot":bot}));
        }
    }
    let _ = hub.events.send(json!({"type":"botGroups","spaceId":space,"snapshot":snapshot}));
    let mut value = json!({"snapshot":snapshot});
    if let Some(id) = created {
        value["created"] = id.into();
    }
    Ok(value)
}

/// Quem chama já segura `emit_lock`; usado por exclusão e desfazer após o commit.
pub(crate) fn publish_locked(hub: &Hub, space: &str) {
    match hub.store.bot_groups_in_space(space) {
        Ok(snapshot) => {
            let _ = hub.events.send(json!({"type":"botGroups","spaceId":space,"snapshot":snapshot}));
        }
        Err(error) => tracing::warn!(%error, "não foi possível publicar grupos após exclusão"),
    }
}

pub(crate) fn publish(hub: &Hub, space: &str) {
    let _emit = hub.emit_lock.locked();
    publish_locked(hub, space);
}
