#![allow(clippy::unwrap_used)]
//! Regressão D70 pelo Hub real; ator sintético no canal, sem iniciar harness.
use crate::{
    LockExt,
    agent::bot::{BotHandle, Cmd},
    api::devices::Caller,
    fork::{control_tower, spaces::PERSONAL},
    hub::{BotStatus, Hub},
    remote::identity::Identity,
    routines::Routines,
    store::{BotConfig, Store},
};
use rusqlite::params;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::mpsc;

#[derive(Debug, PartialEq, Eq)]
enum ActorEvent {
    Refresh,
    Shutdown { committed: bool },
}

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
    bot: String,
    other: String,
    grant: String,
    actor: tokio::task::AbortHandle,
    received: mpsc::UnboundedReceiver<ActorEvent>,
}

impl Fixture {
    fn new() -> Self {
        let dir = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("donnabot-delete-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(Path::new(":memory:")).unwrap();
        let bot = uuid::Uuid::new_v4().to_string();
        let other = uuid::Uuid::new_v4().to_string();
        for id in [&bot, &other] {
            let cfg: BotConfig = serde_json::from_value(json!({
                "id":id,"name":"Fixture","backend":"fixture","cwd":dir,"notify":false,"connectors":["fixture"]
            }))
            .unwrap();
            store.save_bot(&cfg).unwrap();
        }
        let state = json!({"paused":false,"routines":[
            {"id":"routine-target","botId":bot,"name":"Alvo","instruction":"fixture","triggers":[{"type":"webhook"}],
             "enabled":true,"createdAt":1,"updatedAt":1,"nextRuns":[null],"webhookKey":"synthetic-target-key","deleted":false},
            {"id":"routine-other","botId":other,"name":"Outro","instruction":"fixture","triggers":[{"type":"webhook"}],
             "enabled":true,"createdAt":1,"updatedAt":1,"nextRuns":[null],"webhookKey":"synthetic-other-key","deleted":false}
        ],"runs":[
            {"id":"run-target","routineId":"routine-target","botId":bot,"status":"running","createdAt":1,
             "finishedAt":null,"detail":null,"event":{"source":"schedule"},"deliveryId":null,"startedAt":1}
        ]});
        store.kv_set("routines.v1", &state.to_string()).unwrap();
        let hub_id = uuid::Uuid::new_v4().to_string();
        let installation = uuid::Uuid::new_v4().to_string();
        let resource = uuid::Uuid::new_v4().to_string();
        let grant = uuid::Uuid::new_v4().to_string();
        let context = json!({"version":1,"hubId":hub_id,"spaceId":PERSONAL,"conversationId":bot,"botId":bot,
            "audience":{"kind":"principal"},"teamId":null,"privacyDomainId":uuid::Uuid::new_v4().to_string(),
            "domainRevision":1,"conversationRevision":1});
        {
            // Só fixtures persistidas do registro: nenhum ResolvedScope fabricado.
            let c = store.db.locked();
            c.execute(
                "INSERT INTO donna_plugin_installations VALUES(?1,1,?2,'scatty','enabled',2,2)",
                params![installation, hub_id],
            )
            .unwrap();
            c.execute(
                "INSERT INTO donna_plugin_resources VALUES(?1,?2,'native',?3,?4,NULL,'calendar',1,1)",
                params![resource, installation, hub_id, PERSONAL],
            )
            .unwrap();
            c.execute("INSERT INTO donna_plugin_grants VALUES(?1,?2,?3,?4,?5,'calendar_read','read',2,1,1,NULL,NULL,1,1,'active')", params![grant, installation, resource, bot, context.to_string()]).unwrap();
        }
        let hub = Hub::new(store, hub_id, Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
        hub.routines.restore(&hub.store).unwrap();
        hub.store.db.locked().execute(
            "INSERT INTO donna_guard_contexts(id,bot,connector,lane,run,data) VALUES('context-target',?1,'fixture',?1,'run-target','{}')", [&bot],
        ).unwrap();
        hub.set_runtime(&bot, |rt| {
            rt.status = BotStatus::Working;
            rt.activity = "Trabalho preservado".into();
            rt.started_at = Some(7);
        });
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (observed, received) = mpsc::unbounded_channel();
        let actor_hub = hub.clone();
        let actor_bot = bot.clone();
        let task = tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Cmd::RefreshTools => {
                        let _ = observed.send(ActorEvent::Refresh);
                    }
                    Cmd::Shutdown => {
                        let committed = actor_hub.store.bot(&actor_bot).unwrap().unwrap().deleted;
                        // Simula um callback tardio/encerramento que aguardava a reserva.
                        actor_hub.set_runtime(&actor_bot, |rt| rt.activity = "callback tardio".into());
                        let _ = observed.send(ActorEvent::Shutdown { committed });
                        break;
                    }
                    _ => {}
                }
            }
        });
        let actor = task.abort_handle();
        hub.bots.locked().insert(bot.clone(), BotHandle { tx, task });
        Self { hub, dir, bot, other, grant, actor, received }
    }

    fn durable(&self) -> Value {
        let bot = self.hub.bot_value_for_test(&self.bot);
        let routines = self.hub.store.kv_read("routines.v1").unwrap().unwrap();
        let c = self.hub.store.db.locked();
        let grant: (String, i64, i64) = c
            .query_row("SELECT state,generation,revision FROM donna_plugin_grants WHERE id=?1", [&self.grant], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        let contexts: i64 = c.query_row("SELECT count(*) FROM donna_guard_contexts", [], |r| r.get(0)).unwrap();
        json!({"bot":bot,"routines":routines,"grant":grant,"contexts":contexts})
    }

    async fn next_actor_event(&mut self) -> ActorEvent {
        tokio::time::timeout(Duration::from_secs(2), self.received.recv()).await.unwrap().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.actor.abort();
        std::fs::remove_dir_all(&self.dir).unwrap();
    }
}

#[tokio::test]
async fn hub_delete_failure_keeps_routines_runtime_actor_and_grants_coherent() {
    // Inclui a limpeza que antes falhava só depois do commit.
    for failure in ["plugin_trigger", "plugin_overflow", "guard_cleanup"] {
        let mut f = Fixture::new();
        {
            let c = f.hub.store.db.locked();
            match failure {
                "plugin_trigger" => c.execute_batch("CREATE TEMP TRIGGER reject_plugin_revoke BEFORE UPDATE ON donna_plugin_grants BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap(),
                "plugin_overflow" => { c.execute("UPDATE donna_plugin_grants SET generation=9007199254740991 WHERE id=?1", [&f.grant]).unwrap(); }
                "guard_cleanup" => c.execute_batch("CREATE TEMP TRIGGER reject_guard_cleanup BEFORE DELETE ON donna_guard_contexts BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap(),
                _ => unreachable!(),
            }
        }
        let before = f.durable();
        let hooks = f.hub.routines.hooks();
        let hook_updates = f.hub.routines.subscribe_hooks();
        let mut events = f.hub.events.subscribe();
        assert!(f.hub.delete_bot(&f.bot).is_err(), "{failure}");
        assert_eq!(f.durable(), before, "{failure}");
        assert_eq!(serde_json::to_value(f.hub.routines.hooks()).unwrap(), serde_json::to_value(hooks).unwrap());
        assert!(!hook_updates.has_changed().unwrap());
        assert!(events.try_recv().is_err());
        assert_eq!(f.hub.runtime(&f.bot).status, BotStatus::Working);
        assert_eq!(f.hub.runtime(&f.bot).activity, "Trabalho preservado");
        assert_eq!(f.hub.runtime(&f.bot).started_at, Some(7));
        assert!(f.hub.routines.guard_run_live(&f.bot, "run-target"));
        f.hub.send_cmd(&f.bot, Cmd::RefreshTools).unwrap();
        assert_eq!(f.next_actor_event().await, ActorEvent::Refresh, "Shutdown não pode preceder um commit");
        let restored = Routines::default();
        restored.restore(&f.hub.store).unwrap();
        assert_eq!(restored.list(&f.bot), f.hub.routines.list(&f.bot));
    }
}

#[tokio::test]
async fn hub_delete_success_commits_revocation_and_routines_before_shutdown() {
    let mut f = Fixture::new();
    let other = f.hub.routines.list(&f.other);
    let mut events = f.hub.events.subscribe();
    f.hub.delete_bot(&f.bot).unwrap();
    assert_eq!(f.next_actor_event().await, ActorEvent::Shutdown { committed: true });
    assert!(f.hub.store.bot(&f.bot).unwrap().unwrap().deleted);
    assert!(!f.hub.bots.locked().contains_key(&f.bot));
    assert!(!f.hub.runtime.locked().contains_key(&f.bot));
    assert!(f.hub.send_cmd(&f.bot, Cmd::RefreshTools).is_err());
    let snapshot = f.durable();
    assert_eq!(snapshot["grant"], json!(["revoked", 2, 2]));
    assert_eq!(snapshot["contexts"], 0);
    let target = f.hub.routines.list(&f.bot);
    assert_eq!(target["routines"], json!([]));
    assert_eq!(target["runs"][0]["status"], "cancelled");
    assert_eq!(target["runs"][0]["reportPending"], false);
    assert!(!f.hub.routines.guard_run_live(&f.bot, "run-target"));
    assert!(f.hub.routines.hooks().iter().all(|h| h.id != "routine-target"));
    assert_eq!(f.hub.routines.list(&f.other), other);
    let restored = Routines::default();
    restored.restore(&f.hub.store).unwrap();
    assert_eq!(restored.list(&f.bot), target);
    assert_eq!(restored.list(&f.other), other);
    let deleted = events.try_recv().unwrap();
    assert_eq!(deleted["type"], "bot");
    assert_eq!(deleted["bot"]["id"], f.bot);
    assert_eq!(deleted["bot"]["deleted"], true);
    let notification = events.try_recv().unwrap();
    assert!(
        notification == json!({"type":control_tower::EVENT,"spaceId":PERSONAL,"rosterChanged":true}),
        "a única notificação adicional deve ser a invalidação de roster no Space afetado"
    );
    assert!(
        control_tower::event(&f.hub, &Caller::Local, PERSONAL, &notification)
            == Some(json!({"type":control_tower::EVENT,"hubId":f.hub.host_id,"spaceId":PERSONAL})),
        "a invalidação pós-commit entrega apenas o corpo mínimo ao destinatário autorizado"
    );
    let unexpected = events.try_recv();
    let kind = match unexpected.as_ref().ok().and_then(|event| event["type"].as_str()) {
        Some("bot") => "bot",
        Some("donnaControlTowerChanged") => "donnaControlTowerChanged",
        Some("screen") => "screen",
        Some(_) => "outro",
        None => "nenhum-tipo",
    };
    assert!(unexpected.is_err(), "tipo residual: {kind}; quantidade: {}", usize::from(unexpected.is_ok()));
}

#[tokio::test]
async fn hub_delete_reports_post_commit_group_cleanup_without_preserving_actor() {
    let mut f = Fixture::new();
    let group = uuid::Uuid::new_v4().to_string();
    let cfg: BotConfig = serde_json::from_value(json!({
        "id":group,"name":"Grupo fixture","kind":"group","members":[f.bot,f.other]
    }))
    .unwrap();
    f.hub.store.save_bot(&cfg).unwrap();
    f.hub.store.db.locked().execute_batch(
        "CREATE TEMP TRIGGER reject_group_cleanup BEFORE UPDATE ON bots WHEN json_extract(OLD.config,'$.kind')='group' BEGIN SELECT RAISE(ABORT,'synthetic'); END;",
    ).unwrap();
    let error = f.hub.delete_bot(&f.bot).unwrap_err();
    assert!(error.to_string().starts_with("bot_delete_committed:"));
    assert_eq!(f.next_actor_event().await, ActorEvent::Shutdown { committed: true });
    assert!(f.hub.store.bot(&f.bot).unwrap().unwrap().deleted);
    assert!(!f.hub.bots.locked().contains_key(&f.bot));
    assert!(!f.hub.runtime.locked().contains_key(&f.bot));
    assert_eq!(f.durable()["grant"], json!(["revoked", 2, 2]));
    assert_eq!(f.hub.routines.list(&f.bot)["routines"], json!([]));
    // O erro declara esta pendência, sem fingir rollback da exclusão confirmada.
    assert!(f.hub.store.bot(&group).unwrap().unwrap().config.members.contains(&f.bot));
}

/// R350-3: o canal liderado pelo bot apagado fica sem líder na mesma transação da exclusão.
#[tokio::test]
async fn deleting_the_channel_leader_clears_it_with_the_deletion() {
    let with_leader = |f: &Fixture| -> String {
        let group = uuid::Uuid::new_v4().to_string();
        let cfg: BotConfig = serde_json::from_value(json!({
            "id":group,"name":"Canal","kind":"group","members":[f.bot,f.other],"leader":f.bot
        }))
        .unwrap();
        f.hub.store.save_bot(&cfg).unwrap();
        group
    };
    let leader = |f: &Fixture, group: &str| f.hub.store.bot(group).unwrap().unwrap().config.leader;

    // Falha dentro da exclusão: nada confirma, o líder fica.
    let f = Fixture::new();
    let group = with_leader(&f);
    f.hub.store.db.locked().execute_batch("CREATE TEMP TRIGGER reject_plugin_revoke BEFORE UPDATE ON donna_plugin_grants BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    assert!(f.hub.delete_bot(&f.bot).is_err());
    assert!(!f.hub.store.bot(&f.bot).unwrap().unwrap().deleted);
    assert_eq!(leader(&f, &group).as_deref(), Some(f.bot.as_str()));

    // Falha só na limpeza depois do commit (ou o Hub para ali): o líder já saiu, com revisão nova.
    let f = Fixture::new();
    let group = with_leader(&f);
    let rev = f.hub.store.bot(&group).unwrap().unwrap().rev;
    f.hub.store.db.locked().execute_batch(
        "CREATE TEMP TRIGGER reject_group_cleanup BEFORE UPDATE ON bots WHEN json_extract(OLD.config,'$.kind')='group' AND json_extract(OLD.config,'$.leader') IS NULL BEGIN SELECT RAISE(ABORT,'synthetic'); END;",
    ).unwrap();
    let mut events = f.hub.events.subscribe();
    let error = f.hub.delete_bot(&f.bot).unwrap_err();
    assert!(error.to_string().starts_with("bot_delete_committed:"));
    assert!(f.hub.store.bot(&f.bot).unwrap().unwrap().deleted);
    assert_eq!(leader(&f, &group), None);
    let stored = f.hub.store.bot(&group).unwrap().unwrap().rev;
    assert!(stored > rev, "o líder saiu com revisão nova");
    // O app aplica os eventos em ordem e avança o cursor; um evento posterior não pode esconder o canal.
    f.hub.emit_bot(&f.other);
    let mut seen = std::collections::HashMap::new();
    let mut cursor = 0;
    for e in std::iter::from_fn(|| events.try_recv().ok()).filter(|e| e["type"] == "bot") {
        cursor = cursor.max(e["rev"].as_i64().unwrap());
        seen.insert(e["bot"]["id"].as_str().unwrap().to_owned(), e["bot"].clone());
    }
    assert!(cursor > stored, "houve evento posterior ao do canal");
    let shown = &seen[group.as_str()];
    assert!(shown["leader"].is_null(), "o canal sem líder chegou mesmo com a limpeza de members falhando");
    assert_eq!(shown["rev"], stored);

    // Caminho normal: sem líder e sem o membro, e o canal é publicado.
    let f = Fixture::new();
    let group = with_leader(&f);
    let mut events = f.hub.events.subscribe();
    f.hub.delete_bot(&f.bot).unwrap();
    assert_eq!(leader(&f, &group), None);
    assert_eq!(f.hub.store.bot(&group).unwrap().unwrap().config.members, std::slice::from_ref(&f.other));
    let published = std::iter::from_fn(|| events.try_recv().ok())
        .any(|e| e["type"] == "bot" && e["bot"]["id"] == group.as_str() && e["bot"]["leader"].is_null());
    assert!(published, "o canal sem líder chega ao app");
}
