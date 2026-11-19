//! D539: perfil por turno e manutenção determinística no agendador já existente.
use crate::{
    LockExt,
    chat::memory,
    hub::Hub,
    store::{BotConfig, Store},
};
use anyhow::Result;
use codync_mem::{lifecycle, profile};
use rusqlite::{OptionalExtension, params};

pub(crate) fn turn(store: &Store, cfg: &BotConfig, channel: bool) -> String {
    if channel {
        return String::new();
    }
    let result = store.memory_read(|db| {
        let tx = db.unchecked_transaction()?;
        let body = profile::load(&tx, cfg.memory_id(), memory::shared(cfg))?;
        tx.commit()?;
        Ok(body)
    });
    match result {
        Ok(body) => format!("{body}\n\n"),
        Err(error) => {
            tracing::warn!(bot=%cfg.id, %error, "perfil de memória indisponível");
            String::new()
        }
    }
}

pub(crate) fn maintain(hub: &Hub, bot: &str, now: i64) -> Result<()> {
    let Ok(_write) = hub.memory_write() else { return Ok(()) };
    #[cfg(test)]
    {
        let probe = hub.store.before_memory_maintain.locked().take();
        if let Some(probe) = probe {
            probe();
        }
    }
    // MX-280: depois do sinal de parada a manutenção não começa nem commita.
    // Pular não avança `donna_memory_age_checks` — a próxima abertura refaz.
    if hub.routines.stopping() {
        return Ok(());
    }
    // Mesma fronteira das escritas existentes; publicação sempre depois de soltar MB e DB.
    let (own, shared, memory_id) = {
        let _boundary = hub.store.memory_boundary.locked();
        // Configuração e exclusão também usam MB: não decidir sobre compartilhamento com uma captura vencida.
        let Some(row) = hub.store.bot(bot)?.filter(|r| !r.deleted && !r.config.is_group()) else { return Ok(()) };
        let cfg = row.config;
        let mut db = hub.store.db.locked();
        // Retenção independe do modo de aprendizado e do intervalo de envelhecimento.
        {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            crate::fork::memory_auto::recibo_injecao::prune(&tx, now, None)?;
            if hub.routines.stopping() {
                return Ok(());
            }
            tx.commit()?;
        }
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if crate::fork::memory_learning::mode_in(&tx)? != crate::fork::memory_learning::Mode::Auto {
            return Ok(());
        }
        let last: Option<i64> = tx
            .query_row("SELECT at FROM donna_memory_age_checks WHERE bot=?1", [cfg.memory_id()], |r| r.get(0))
            .optional()?;
        if last.is_some_and(|last| now.saturating_sub(last) < lifecycle::CHECK_MS) {
            return Ok(());
        }
        let has_own = !lifecycle::candidates(&tx, Some(cfg.memory_id()), now)?.is_empty();
        let has_shared = memory::shared(&cfg) && !lifecycle::candidates(&tx, None, now)?.is_empty();
        let (own, shared) = if has_own || has_shared {
            let rev = crate::store::next_rev(&tx)?;
            let own = lifecycle::age(&tx, Some(cfg.memory_id()), now, rev)?;
            let shared = if memory::shared(&cfg) { lifecycle::age(&tx, None, now, rev)? } else { 0 };
            tx.execute("INSERT INTO kv(k,v) VALUES('memory.rev',?1) ON CONFLICT(k) DO UPDATE SET v=?1", [rev])?;
            (own, shared)
        } else {
            (0, 0)
        };
        tx.execute(
            "INSERT INTO donna_memory_age_checks(bot,at) VALUES(?1,?2) ON CONFLICT(bot) DO UPDATE SET at=?2",
            params![cfg.memory_id(), now],
        )?;
        // Recheck na fronteira do commit: parada no meio da Tx faz rollback,
        // nunca grava marcadores de uma manutenção que o dono não verá.
        if hub.routines.stopping() {
            return Ok(());
        }
        tx.commit()?;
        (own, shared, cfg.memory_id().to_owned())
    };
    if own > 0 {
        memory::changed(hub, Some(&memory_id));
    }
    if shared > 0 {
        memory::changed(hub, None);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::identity::Identity;
    use codync_mem::{
        nature,
        store::{self as mem, New, Writer},
    };

    #[test]
    fn profile_uses_memory_namespace_and_maintenance_archives_once_with_notification() {
        let dir = std::env::temp_dir().join(format!("d539-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let cfg: BotConfig =
            serde_json::from_value(serde_json::json!({"id":"bot","name":"Bot","backend":"claude","cwd":dir})).unwrap();
        store.save_bot(&cfg).unwrap();
        let fact = store
            .memory_write(|db, _, rev| {
                let entry = mem::add(
                    db,
                    Writer::Bot("bot"),
                    New { bot: Some("bot"), text: "Mora em Recife", room: None, evidence: &["m1".into()] },
                    1,
                    rev,
                )?;
                nature::set(db, entry.id, nature::KindNature::Inferred, Some("bot"), None)?;
                Ok(entry.id)
            })
            .unwrap();
        let before = turn(&store, &cfg, false);
        assert!(before.contains("Mora em Recife"));
        assert_eq!(before, turn(&store, &cfg, false), "perfil presente em turnos sucessivos");
        let mut additional = cfg.clone();
        additional.context = Some(crate::fork::spaces::ExecutionContext {
            conversation_id: "extra".into(),
            memory_id: "bot".into(),
            foreign_identity: false,
        });
        assert_eq!(before, turn(&store, &additional, false));
        let hub = Hub::new(store, "test".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0);
        let now = lifecycle::WEAK_AGE_MS + 2;
        for mode in ["off", "suggest"] {
            hub.store.kv_set(crate::fork::memory_learning::KEY, mode).unwrap();
            maintain(&hub, "bot", now).unwrap();
            assert_eq!(hub.store.memory_read(|db| mem::get(db, fact)).unwrap().unwrap().state, mem::State::Active);
        }
        hub.store.kv_set(crate::fork::memory_learning::KEY, "auto").unwrap();
        maintain(&hub, "bot", now).unwrap();
        assert_eq!(hub.store.memory_read(|db| mem::get(db, fact)).unwrap().unwrap().state, mem::State::Archived);
        assert!(!turn(&hub.store, &cfg, false).contains("Mora em Recife"));
        assert_ne!(memory::current_revision(&hub.store, "bot"), "");
        let rev = hub.store.memory_rev();
        maintain(&hub, "bot", now + 1).unwrap();
        assert_eq!(rev, hub.store.memory_rev());
        drop(hub);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
