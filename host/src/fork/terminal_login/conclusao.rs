//! Conclusão sob a autoridade de contas: commits e admissão de reenvio têm cercas próprias.
use super::{Authority, Hub, LockExt, Result, Value, anyhow, ensure, json};
use crate::{
    agent::bot::Cmd,
    store::{Entry, Lane, Store},
};

pub(crate) fn finish(hub: &Hub, backend: &str, space: &str, authority: &Authority) -> Result<()> {
    let _sign_in = crate::market::requests::SIGN_IN.locked();
    for card in hub.store.pending_sign_ins(backend)? {
        let mut ready = card.data.clone();
        ready["connectionRequest"]["status"] = "ready".into();
        let bot = hub.store.bot(&card.bot_id)?.filter(|b| !b.deleted && b.config.space_id == space);
        if bot.is_none() {
            continue;
        }
        let mut plans = Vec::new();
        if crate::store::now_ms() - card.created_at <= 24 * 60 * 60 * 1000
            && let Some(bot) = bot
        {
            for id in
                card.data["connectionRequest"]["entryIds"].as_array().into_iter().flatten().filter_map(Value::as_str)
            {
                if let Some(entry) = hub.store.entry(id).filter(|entry| entry.bot_id == card.bot_id) {
                    let text = crate::chat::prompt::of_entry(hub, &bot.config, &entry.data);
                    // Resolve o canal ANTES da cerca de saída; dentro dela só o enqueue síncrono.
                    let sender = hub.login_command_sender(&card.bot_id)?;
                    plans.push((entry, bot.rev, text, sender));
                }
            }
        }
        #[cfg(test)]
        boundary(hub, Boundary::Commit);
        let _emit = hub.emit_lock.locked();
        #[cfg(test)]
        boundary(hub, Boundary::Send);
        let snapshots = plans.iter().map(|(entry, rev, _, _)| (entry.clone(), *rev)).collect::<Vec<_>>();
        let updated = authority.commit_and_admit(
            hub,
            |db| {
                ensure!(
                    Store::bot_on(db, &card.bot_id)?.is_some_and(|b| !b.deleted && b.config.space_id == space),
                    "Contexto do login alterado."
                );
                let current = Store::entry_on(db, &card.id)?.ok_or_else(|| anyhow!("Pedido de login encerrado."))?;
                ensure!(current.rev == card.rev, "Pedido de login alterado.");
                let mut updated: Vec<Entry> = Store::update_entry_on(db, &card.id, &ready, None)?.into_iter().collect();
                for (entry, bot_rev) in &snapshots {
                    ensure!(Store::entry_on(db, &entry.id)?.is_some_and(|e| e.rev == entry.rev), "Mensagem alterada.");
                    ensure!(
                        Store::bot_on(db, &card.bot_id)?.is_some_and(|b| !b.deleted && b.rev == *bot_rev),
                        "Bot alterado."
                    );
                    let mut data = entry.data.clone();
                    data["status"] = "queued".into();
                    updated.extend(Store::update_entry_on(db, &entry.id, &data, None)?);
                }
                Ok(updated)
            },
            |updated| -> Result<Vec<Entry>> {
                for (entry, _, text, sender) in plans {
                    sender
                        .send(Cmd::Send { lane: Lane::main(&entry.bot_id), entry_id: entry.id, text })
                        .map_err(|_| anyhow!("Bot encerrado."))?;
                }
                Ok(updated)
            },
        )??;
        for entry in updated {
            let _ = hub.events.send(json!({"type":"entry","rev":entry.rev,"entry":entry}));
        }
    }
    Ok(())
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Boundary {
    Commit,
    Send,
}
#[cfg(test)]
type TestHook = (std::path::PathBuf, Boundary, Box<dyn FnOnce() + Send>);
#[cfg(test)]
static HOOKS: std::sync::Mutex<Vec<TestHook>> = std::sync::Mutex::new(Vec::new());
#[cfg(test)]
pub(crate) fn before_test(hub: &Hub, stage: Boundary, hook: impl FnOnce() + Send + 'static) {
    HOOKS.locked().push((hub.store.engine_setup_root.clone(), stage, Box::new(hook)));
}
#[cfg(test)]
fn boundary(hub: &Hub, stage: Boundary) {
    let hook = {
        let mut hooks = HOOKS.locked();
        hooks
            .iter()
            .position(|(root, at, _)| *root == hub.store.engine_setup_root && *at == stage)
            .map(|i| hooks.remove(i).2)
    };
    if let Some(hook) = hook {
        hook();
    }
}
