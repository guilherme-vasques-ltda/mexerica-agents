//! Aviso de isolamento: entrada já renderizada, uma vez por conversa.
use crate::{
    hub::Hub,
    store::{BotConfig, EntryKind, Lane},
};
use anyhow::Result;
use serde_json::json;
pub(crate) const TEXT: &str = "Neste espaço o Berga não usa suas instruções, ferramentas nem a memória do Pessoal.";
pub(crate) fn ensure(hub: &Hub, cfg: &BotConfig, chat: &str, space: &str) -> Result<()> {
    if cfg.space_id == space {
        return Ok(());
    }
    hub.add_entry_once(
        &format!("space-foreign-{chat}"),
        &Lane::main(chat),
        EntryKind::Notice,
        0,
        &json!({"text":TEXT,"style":"info"}),
    )?;
    Ok(())
}
pub(crate) fn backfill(hub: &Hub) -> Result<()> {
    for row in hub.store.bots()?.into_iter().filter(|b| !b.deleted) {
        for space in hub.store.spaces()?.spaces {
            for c in hub.store.space_conversations(&space.id)?.into_iter().filter(|c| c.bot_id == row.config.id) {
                ensure(hub, &row.config, &c.id, &c.space_id)?;
            }
        }
    }
    Ok(())
}
