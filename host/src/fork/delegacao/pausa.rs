//! Vínculo durável de um cartão de permissão ao turno delegado. Não retoma nem projeta pedidos.
use crate::fork::{
    sessoes::{ExecutionRef, bindings},
    spaces,
};
use crate::store::{BotConfig, Entry, Lane, Store, now_ms};
use anyhow::{Result, ensure};
use rusqlite::{Connection, params};
use serde_json::Value;

pub(crate) struct Pedido<'a> {
    pub id: &'a str,
    pub fence: i64,
}

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    // A integridade não depende do pragma foreign_keys; a escrita confere os vínculos na transação.
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_delegated_permission_checkpoints(
        request_id TEXT NOT NULL,
        attempt INTEGER NOT NULL CHECK(attempt=1), fence INTEGER NOT NULL CHECK(fence>=1), space_id TEXT NOT NULL,
        execution_ref TEXT NOT NULL, card_id TEXT NOT NULL,
        card_revision INTEGER NOT NULL, created_at INTEGER NOT NULL,
        PRIMARY KEY(card_id), UNIQUE(request_id,attempt,fence,card_id));
        CREATE INDEX IF NOT EXISTS donna_delegated_permission_request ON donna_delegated_permission_checkpoints(request_id,attempt,fence);",
    )?;
    Ok(())
}

/// Chamado dentro da mesma transação IMMEDIATE do cartão, sem lock adicional ou publicação.
pub(crate) fn inserir_na(
    c: &Connection,
    cfg: &BotConfig,
    lane: &Lane,
    turn: i64,
    data: &Value,
    execution: &ExecutionRef,
    pedido: &Pedido<'_>,
) -> Result<(Option<Entry>, Entry)> {
    spaces::require_space(c, &cfg.space_id)?;
    ensure!(
        !cfg.is_group() && !cfg.context.as_ref().is_some_and(|ctx| ctx.foreign_identity),
        "delegated_permission_identity_invalid"
    );
    let binding_id =
        execution.acp_binding().ok_or_else(|| anyhow::anyhow!("delegated_permission_execution_invalid"))?;
    let binding = bindings::resolve(c, binding_id, &cfg.id)?;
    bindings::assert_binding_live(c, &binding)?;
    ensure!(
        binding.session_ref.chat_id == lane.chat && binding.session_ref.id == lane.session_id(),
        "delegated_permission_execution_invalid"
    );
    let valid: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_session_requests r
         JOIN bots target ON target.id=r.target_bot_id AND target.deleted=0
         JOIN bots source ON source.id=r.requester_bot_id AND source.deleted=0
         WHERE r.id=?1 AND r.schema_version=1 AND r.kind='inter_bot' AND r.state='running'
         AND r.attempt=1 AND r.fence=?2 AND r.target_bot_id=?3 AND r.target_chat_id=?4
         AND r.target_session_id=?5
         AND CASE WHEN json_type(target.config,'$.kind') IS NULL THEN 'agent'
             WHEN json_type(target.config,'$.kind')='text' THEN json_extract(target.config,'$.kind') END = 'agent'
         AND CASE WHEN json_type(source.config,'$.kind') IS NULL THEN 'agent'
             WHEN json_type(source.config,'$.kind')='text' THEN json_extract(source.config,'$.kind') END = 'agent'
         AND CASE WHEN json_type(target.config,'$.spaceId') IS NULL THEN ?6
             WHEN json_type(target.config,'$.spaceId')='text' THEN json_extract(target.config,'$.spaceId') END = ?7
         AND CASE WHEN json_type(source.config,'$.spaceId') IS NULL THEN ?6
             WHEN json_type(source.config,'$.spaceId')='text' THEN json_extract(source.config,'$.spaceId') END = ?7)",
        params![pedido.id, pedido.fence, cfg.id, lane.chat, lane.session_id(), spaces::PERSONAL, cfg.space_id],
        |r| r.get(0),
    )?;
    ensure!(valid, "delegated_permission_stale");
    let execution_ref = crate::fork::retornos::referencia(binding_id, execution.turn_token())?;
    let pair = Store::insert_permission_card_on(c, cfg, lane, turn, data, execution)?;
    c.execute(
        "INSERT INTO donna_delegated_permission_checkpoints
        (request_id,attempt,fence,space_id,execution_ref,card_id,card_revision,created_at)
        VALUES(?1,1,?2,?3,?4,?5,?6,?7)",
        params![pedido.id, pedido.fence, cfg.space_id, execution_ref, pair.1.id, pair.1.rev, now_ms()],
    )?;
    Ok(pair)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::{LockExt, hub::Hub, remote::identity::Identity, store::Store};
    use serde_json::json;
    use std::sync::{Arc, Barrier};

    #[test]
    fn migracao_em_banco_antigo_e_idempotente_e_confere_colunas() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE donna_session_requests(id TEXT PRIMARY KEY); CREATE TABLE entries(id TEXT PRIMARY KEY);",
        )
        .unwrap();
        migrate(&c).unwrap();
        migrate(&c).unwrap();
        for (attempt, fence) in [(0, 1), (1, 0), (2, 1)] {
            let result = c.execute("INSERT INTO donna_delegated_permission_checkpoints(request_id,attempt,fence,space_id,execution_ref,card_id,card_revision,created_at) VALUES('r',?1,?2,'s','e','c',1,1)", params![attempt,fence]);
            assert!(result.is_err());
        }
    }

    #[test]
    fn cerca_vencida_e_corrida_nao_deixam_cartao_parcial_ou_evento() {
        let dir = std::env::temp_dir().join(format!("checkpoint-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("test.db")).unwrap();
        for id in ["a", "b"] {
            let cfg: BotConfig =
                serde_json::from_value(json!({"id":id,"name":id,"backend":"claude","cwd":dir})).unwrap();
            store.save_bot(&cfg).unwrap();
        }
        let hub = Hub::new(store, "teste".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
        let execution = bindings::fixture_execution(&hub, "b");
        let cfg = hub.store.bot("b").unwrap().unwrap().config;
        hub.store.db.locked().execute(
            "INSERT INTO donna_session_requests(id,schema_version,kind,mode,requester_bot_id,requester_chat_id,
             requester_session_id,requester_execution_ref,target_bot_id,target_chat_id,target_session_id,chain_id,
             admission_id,origin_entry_id,target_entry_id,attempt,fence,state,created_at,updated_at)
             VALUES('r',1,'inter_bot','wait','a','a','a','origin','b','b','b','chain','admission','o','t',1,2,'running',1,1)", [],
        ).unwrap();
        let mut events = hub.events.subscribe();
        let card = json!({"status":"pending","options":[]});
        let error = hub
            .add_permission_card(&cfg, &Lane::main("b"), 1, &card, &execution, Some(&Pedido { id: "r", fence: 1 }))
            .unwrap_err();
        assert!(format!("{error:#}").contains("delegated_permission_stale"));
        assert!(events.try_recv().is_err());
        assert!(hub.store.history("b", i64::MAX, 100).unwrap().is_empty());
        // Space ausente, nulo ou divergente, bots removidos e pedido fechado não
        // podem deixar cartão nem vínculo. As mutações são restauradas a cada caso.
        for (change, restore) in [
            (
                "UPDATE bots SET config=json_set(config,'$.spaceId','outro') WHERE id='a'",
                "UPDATE bots SET config=json_set(config,'$.spaceId','personal') WHERE id='a'",
            ),
            (
                "UPDATE bots SET config=json_set(config,'$.spaceId',null) WHERE id='b'",
                "UPDATE bots SET config=json_set(config,'$.spaceId','personal') WHERE id='b'",
            ),
            (
                "UPDATE bots SET config=json_set(config,'$.kind','group') WHERE id='a'",
                "UPDATE bots SET config=json_set(config,'$.kind','agent') WHERE id='a'",
            ),
            (
                "UPDATE bots SET config=json_set(config,'$.kind','group') WHERE id='b'",
                "UPDATE bots SET config=json_set(config,'$.kind','agent') WHERE id='b'",
            ),
            ("UPDATE bots SET deleted=1 WHERE id='a'", "UPDATE bots SET deleted=0 WHERE id='a'"),
            (
                "UPDATE donna_session_requests SET state='finished' WHERE id='r'",
                "UPDATE donna_session_requests SET state='running' WHERE id='r'",
            ),
        ] {
            // Guarda a configuração original, inclusive o ID real do Pessoal.
            let original: Vec<(String, String, i64)> = {
                let db = hub.store.db.locked();
                let mut q = db.prepare("SELECT id,config,deleted FROM bots WHERE id IN ('a','b') ORDER BY id").unwrap();
                q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                    .unwrap()
                    .map(std::result::Result::unwrap)
                    .collect()
            };
            hub.store.db.locked().execute_batch(change).unwrap();
            let error = hub
                .add_permission_card(&cfg, &Lane::main("b"), 1, &card, &execution, Some(&Pedido { id: "r", fence: 2 }))
                .unwrap_err();
            assert!(format!("{error:#}").contains("delegated_permission_stale"));
            hub.store.db.locked().execute_batch(restore).unwrap();
            for (id, config, deleted) in original {
                hub.store
                    .db
                    .locked()
                    .execute("UPDATE bots SET config=?2,deleted=?3 WHERE id=?1", params![id, config, deleted])
                    .unwrap();
            }
            assert!(events.try_recv().is_err());
            assert!(hub.store.history("b", i64::MAX, 100).unwrap().is_empty());
        }
        let mut foreign = cfg.clone();
        foreign.context = Some(spaces::ExecutionContext {
            conversation_id: "outro".into(),
            memory_id: "outro".into(),
            foreign_identity: true,
        });
        let error = hub
            .add_permission_card(&foreign, &Lane::main("b"), 1, &card, &execution, Some(&Pedido { id: "r", fence: 2 }))
            .unwrap_err();
        assert!(format!("{error:#}").contains("delegated_permission_identity_invalid"));
        let mut group = cfg.clone();
        group.kind = crate::store::BotKind::Group;
        let error = hub
            .add_permission_card(&group, &Lane::main("b"), 1, &card, &execution, Some(&Pedido { id: "r", fence: 2 }))
            .unwrap_err();
        assert!(format!("{error:#}").contains("delegated_permission_identity_invalid"));
        let error = hub
            .add_permission_card(&cfg, &Lane::main("a"), 1, &card, &execution, Some(&Pedido { id: "r", fence: 2 }))
            .unwrap_err();
        assert!(format!("{error:#}").contains("delegated_permission_execution_invalid"));
        assert!(events.try_recv().is_err());
        hub.store.db.locked().execute_batch("CREATE TRIGGER falha_checkpoint BEFORE INSERT ON donna_delegated_permission_checkpoints BEGIN SELECT RAISE(ABORT,'falha'); END;").unwrap();
        let error = hub
            .add_permission_card(&cfg, &Lane::main("b"), 1, &card, &execution, Some(&Pedido { id: "r", fence: 2 }))
            .unwrap_err();
        assert!(format!("{error:#}").contains("falha"));
        assert!(events.try_recv().is_err());
        assert!(hub.store.history("b", i64::MAX, 100).unwrap().is_empty());
        hub.store.db.locked().execute_batch("DROP TRIGGER falha_checkpoint").unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let hub = hub.clone();
                let cfg = cfg.clone();
                let execution = execution.clone();
                let barrier = barrier.clone();
                let card = card.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    hub.add_permission_card(
                        &cfg,
                        &Lane::main("b"),
                        1,
                        &card,
                        &execution,
                        Some(&Pedido { id: "r", fence: 2 }),
                    )
                    .is_ok()
                })
            })
            .collect();
        barrier.wait();
        assert_eq!(workers.into_iter().map(|t| t.join().unwrap()).filter(|ok| *ok).count(), 2);
        let db = hub.store.db.locked();
        assert_eq!(
            db.query_row("SELECT count(*) FROM donna_delegated_permission_checkpoints", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM entries WHERE kind='permission'", [], |r| r.get::<_, i64>(0)).unwrap(),
            2
        );
        drop(db);
        let mut published = 0;
        while let Ok(event) = events.try_recv() {
            if event["entry"]["kind"] == "permission" {
                let id = event["entry"]["id"].as_str().unwrap();
                assert_eq!(
                    hub.store
                        .db
                        .locked()
                        .query_row(
                            "SELECT count(*) FROM donna_delegated_permission_checkpoints WHERE card_id=?1",
                            [id],
                            |r| r.get::<_, i64>(0)
                        )
                        .unwrap(),
                    1
                );
                published += 1;
            }
        }
        assert_eq!(published, 2, "evento só após confirmação do vínculo");
        let ordinary = hub.add_permission_card(&cfg, &Lane::main("b"), 1, &card, &execution, None).unwrap();
        assert_eq!(
            hub.store
                .db
                .locked()
                .query_row(
                    "SELECT count(*) FROM donna_delegated_permission_checkpoints WHERE card_id=?1",
                    [&ordinary.id],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        // Configuração antiga sem spaceId usa Pessoal; null continua recusado acima.
        hub.store
            .db
            .locked()
            .execute("UPDATE bots SET config=json_remove(config,'$.spaceId') WHERE id='a'", [])
            .unwrap();
        assert!(
            hub.add_permission_card(&cfg, &Lane::main("b"), 1, &card, &execution, Some(&Pedido { id: "r", fence: 2 }))
                .is_ok()
        );
        // Uma entrada apagada não bloqueia cartões novos, nem cria prova de retomada.
        hub.store
            .db
            .locked()
            .execute(
                "DELETE FROM entries WHERE id=(SELECT card_id FROM donna_delegated_permission_checkpoints LIMIT 1)",
                [],
            )
            .unwrap();
        assert!(
            hub.add_permission_card(&cfg, &Lane::main("b"), 1, &card, &execution, Some(&Pedido { id: "r", fence: 2 }))
                .is_ok()
        );
        while events.try_recv().is_ok() {}
        hub.store.db.locked().execute("UPDATE bots SET deleted=1 WHERE id='b'", []).unwrap();
        let error = hub
            .add_permission_card(&cfg, &Lane::main("b"), 1, &card, &execution, Some(&Pedido { id: "r", fence: 2 }))
            .unwrap_err();
        assert!(format!("{error:#}").contains("chamada vencida"));
        assert!(events.try_recv().is_err());
        hub.store.db.locked().execute("UPDATE bots SET deleted=0 WHERE id='b'", []).unwrap();
        let execution = bindings::fixture_execution(&hub, "b");
        bindings::revoke(&hub.store, "b", Some("fixture-acp")).unwrap();
        let error = hub
            .add_permission_card(&cfg, &Lane::main("b"), 1, &card, &execution, Some(&Pedido { id: "r", fence: 2 }))
            .unwrap_err();
        assert!(format!("{error:#}").contains("chamada vencida"));
        assert!(events.try_recv().is_err());
        drop(events);
        drop(hub);
        let reopened = Store::open(&dir.join("test.db")).unwrap();
        let db = reopened.db.locked();
        assert_eq!(
            db.query_row("SELECT count(*) FROM donna_delegated_permission_checkpoints", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            4
        );
        assert_eq!(db.query_row("SELECT count(*) FROM donna_delegated_permission_checkpoints p LEFT JOIN entries e ON e.id=p.card_id WHERE e.id IS NULL", [], |r| r.get::<_,i64>(0)).unwrap(), 1);
        crate::fork::retornos::recuperar_na(&db).unwrap();
        assert_eq!(
            db.query_row("SELECT state FROM donna_session_requests WHERE id='r'", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "interrupted"
        );
        drop(db);
        drop(reopened);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
