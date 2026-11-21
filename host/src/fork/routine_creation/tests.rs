//! D544/D547: fronteiras reais, SQLite sintético e nenhum motor externo.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::{
    api::{devices::Caller, dispatch},
    fork::{
        sessoes::{ExecutionRef, bindings},
        spaces::PERSONAL,
    },
    remote::identity::Identity,
    store::{Device, DeviceSource, Scope, Store},
};
use std::{
    path::PathBuf,
    sync::{Arc, Barrier},
};

struct Fixture {
    hub: Arc<Hub>,
    execution: ExecutionRef,
    dir: PathBuf,
}
fn task() -> Value {
    json!({"name":"Revisão semanal","instruction":"Somente tarefa sintética","triggers":[{"type":"cron","expression":"0 9 * * 1","timeZone":"America/Sao_Paulo"}]})
}

#[tokio::test]
async fn mx472_concessao_explicita_atomica_e_evento_ainda_recusado() {
    let f = Fixture::new();
    let body = json!({"name":"Evento", "instruction":"Dados externos", "triggers":[{"type":"event","source":"github","event":"pull_request.opened","filters":{"repo":"org/repo"}}]});
    let id = f.call(&body).unwrap()["routine"]["id"].as_str().unwrap().to_owned();
    let key = format!("donna.routine-external-grant.v1.{}", crate::agent::registry::sha256_hex(id.as_bytes()));
    assert!(f.hub.store.kv_read(&key).unwrap().is_none()); // enabled não é concessão
    let card = f.cards()["items"][0].clone();
    let mut request = json!({"entryId":card["entryId"],"entryRev":card["entryRev"],"choice":"grantExternal","routineRevision":card["routineRevision"],"externalGrant":{"repository":"org/repo","event":"pull_request.opened","action":{"type":"notify","text":"Avisar"}}});
    f.hub
        .store
        .put_device(&Device {
            key: "controle-real-fixture".into(),
            name: "fixture".into(),
            platform: "fixture".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: now_ms(),
            last_seen_at: None,
        })
        .unwrap();
    let (_, device) = crate::fork::autoridade::autenticar(&f.hub, "controle-real-fixture").unwrap();
    let mut review_body = request.clone();
    review_body["hubId"] = json!(f.hub.host_id);
    review_body["spaceId"] = json!(PERSONAL);
    let error = dispatch(&f.hub, &device, REVIEW, review_body.clone()).await.unwrap_err();
    assert!(format!("{error:#}").contains("routine_external_owner_required"), "{error:#}");
    let binding = bindings::issue(&f.hub.store, "bot", "bot", "bot", None).unwrap();
    bindings::activate(&f.hub.store, &binding, "acp-bridge-valid").unwrap();
    let mut servers = vec![json!({"env":[]})];
    crate::fork::ponte_credencial::provisionar(&f.hub, "bot", None, &binding, &mut servers).unwrap();
    let token =
        servers[0]["env"].as_array().unwrap().iter().find(|e| e["name"] == crate::fork::ponte_credencial::ENV).unwrap()
            ["value"]
            .as_str()
            .unwrap();
    let bridge = f.hub.pontes.autenticar(&f.hub, token).unwrap();
    let error = dispatch(&f.hub, &bridge, REVIEW, review_body).await.unwrap_err();
    assert!(error.is::<crate::api::devices::Forbidden>(), "{error:#}");
    let mut wrong = request.clone();
    wrong.as_object_mut().unwrap().remove("externalGrant");
    assert!(f.api(REVIEW, wrong).await.is_err());
    wrong = request.clone();
    wrong["choice"] = json!("keep");
    assert!(f.api(REVIEW, wrong).await.is_err());
    wrong = request.clone();
    wrong["externalGrant"]["action"]["type"] = json!("workflow");
    assert!(f.api(REVIEW, wrong).await.is_err());
    wrong = request.clone();
    wrong["externalGrant"]["repository"] = json!("org/other");
    assert!(f.api(REVIEW, wrong).await.is_err());
    for repo in ["./repo", "../repo", "-x/repo"] {
        let mut bad = request.clone();
        bad["externalGrant"]["repository"] = json!(repo);
        assert!(f.api(REVIEW, bad).await.is_err());
    }
    let mut bad = request.clone();
    bad["externalGrant"]["action"]["unknown"] = json!(true);
    assert!(f.api(REVIEW, bad).await.is_err());
    wrong = request.clone();
    wrong["routineRevision"] = json!("stale");
    assert!(f.api(REVIEW, wrong).await.is_err());
    wrong = request.clone();
    wrong["spaceId"] = json!("other");
    assert!(f.api(REVIEW, wrong).await.is_err());
    assert!(f.hub.store.kv_read(&key).unwrap().is_none());
    f.hub.store.db.locked().execute_batch("CREATE TRIGGER mx472_fail BEFORE UPDATE ON entries WHEN NEW.id LIKE 'routine-created:%' BEGIN SELECT RAISE(ABORT,'fail'); END;").unwrap();
    assert!(f.api(REVIEW, request.clone()).await.is_err());
    assert!(f.hub.store.kv_read(&key).unwrap().is_none());
    f.hub.store.db.locked().execute_batch("DROP TRIGGER mx472_fail").unwrap();
    let result = f.api(REVIEW, request.clone()).await.unwrap();
    assert_eq!(result["decision"], "external_granted");
    let saved: Value = serde_json::from_str(&f.hub.store.kv_read(&key).unwrap().unwrap()).unwrap();
    assert_eq!(saved["origin"], "localTransport");
    assert_eq!(saved["spaceId"], PERSONAL);
    assert_eq!(saved["botId"], "bot");
    assert_eq!(saved["routineId"], id);
    assert_eq!(saved["entryId"], card["entryId"]);
    assert_eq!(saved["reviewedEntryRevision"], card["entryRev"]);
    assert_eq!(saved["contract"], request["externalGrant"]);
    assert!(f.api(REVIEW, request.clone()).await.is_err());
    request["externalGrant"]["action"]["text"] = json!("Outra ação");
    assert!(f.api(REVIEW, request).await.is_err());
    let store = Store::open(&f.dir.join("hub.db")).unwrap();
    assert_eq!(store.kv_read(&key).unwrap(), f.hub.store.kv_read(&key).unwrap());
    let credentials = f.hub.routines.credentials(&f.hub, "bot", &id, false).unwrap();
    let mut headers = crate::routines::hooks::Headers::new();
    headers.insert("authorization".into(), format!("Bearer {}", credentials["key"].as_str().unwrap()));
    let event = json!({"source":"github","event":"pull_request.opened","repo":"org/repo"});
    assert_eq!(
        f.hub.routines.receive(&f.hub, &id, &headers, event.to_string().as_bytes()).unwrap_err(),
        crate::routines::hooks::Refused::ChainUnavailable
    );
    assert_eq!(f.hub.routines.list("bot")["runs"].as_array().unwrap().as_slice(), [] as [Value; 0]);
    let mut edit = body;
    edit["id"] = json!(id);
    edit["botId"] = json!("bot");
    edit["instruction"] = json!("Nova ação");
    f.api("saveRoutine", edit).await.unwrap();
    assert!(f.hub.store.kv_read(&key).unwrap().is_none());
    let raw: Value = serde_json::from_str(&f.raw()).unwrap();
    let routine: Routine = serde_json::from_value(raw["routines"][0].clone()).unwrap();
    assert_ne!(external_grant::authority_revision(&routine).unwrap(), saved["authorityRevision"].as_str().unwrap());
    let current = f.cards()["items"][0].clone();
    f.api(REVIEW, json!({"entryId":current["entryId"],"entryRev":current["entryRev"],"choice":"grantExternal","routineRevision":current["routineRevision"],"externalGrant":{"repository":"org/repo","event":"pull_request.opened","action":{"type":"notify","text":"Nova ação"}}})).await.unwrap();
    let renewed: Value = serde_json::from_str(&f.hub.store.kv_read(&key).unwrap().unwrap()).unwrap();
    assert_ne!(saved["grantId"], renewed["grantId"]);
    f.api("deleteRoutine", json!({"botId":"bot","id":id})).await.unwrap();
    assert!(f.hub.store.kv_read(&key).unwrap().is_none());
}

#[tokio::test]
async fn mx472_cas_so_uma_concessao_e_limpeza_do_bot() {
    let f = Fixture::new();
    f.hub
        .store
        .save_bot(&serde_json::from_value(json!({"id":"other-bot","name":"Outro","backend":"fixture"})).unwrap())
        .unwrap();
    let execution = start(&f.hub, "other-bot", "acp-other");
    let body = json!({"name":"Evento", "instruction":"Dados externos", "triggers":[{"type":"event","source":"github","event":"pull_request.opened","filters":{"repo":"org/repo"}}]});
    let id = crate::routines::call_invoked_in_execution(
        &f.hub,
        "other-bot",
        "save_routine",
        &body,
        &execution,
        Some("grant-test"),
    )
    .unwrap()["routine"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let key = format!("donna.routine-external-grant.v1.{}", crate::agent::registry::sha256_hex(id.as_bytes()));
    let card = f.cards()["items"][0].clone();
    let req = json!({"entryId":card["entryId"],"entryRev":card["entryRev"],"choice":"grantExternal","routineRevision":card["routineRevision"],"externalGrant":{"repository":"org/repo","event":"pull_request.opened","action":{"type":"notify","text":"Avisar"}}});
    let (a, b) = tokio::join!(f.api(REVIEW, req.clone()), f.api(REVIEW, req));
    assert_ne!(a.is_ok(), b.is_ok());
    assert!(f.hub.store.kv_read(&key).unwrap().is_some());
    f.hub.routines.delete_bot_committed(&f.hub.store, "other-bot").unwrap();
    assert!(f.hub.store.kv_read(&key).unwrap().is_none());
}

#[tokio::test]
async fn mx472_recusa_gatilhos_fora_do_contrato() {
    for triggers in [
        json!([{"type":"event","source":"github","event":"*","filters":{"repo":"org/repo"}}]),
        json!([{"type":"event","source":"github","event":"pull_request.opened","filters":{"repo":"org/repo","extra":"x"}}]),
        json!([{"type":"webhook"}]),
        json!([{"type":"webhook"},{"type":"event","source":"github","event":"pull_request.opened","filters":{"repo":"org/repo"}}]),
    ] {
        let f = Fixture::new();
        f.call(&json!({"name":"Evento","instruction":"Dados","triggers":triggers})).unwrap();
        let card = f.cards()["items"][0].clone();
        let req = json!({"entryId":card["entryId"],"entryRev":card["entryRev"],"choice":"grantExternal","routineRevision":card["routineRevision"],"externalGrant":{"repository":"org/repo","event":"pull_request.opened","action":{"type":"notify","text":"Avisar"}}});
        assert!(f.api(REVIEW, req).await.is_err());
    }
}

#[tokio::test]
async fn mx472_registro_ilegivel_nao_bloqueia_pausa_ou_exclusao() {
    let f = Fixture::new();
    let id = f.call(&task()).unwrap()["routine"]["id"].as_str().unwrap().to_owned();
    let key = "donna.routine-external-grant.v1.corrompida";
    f.hub.store.kv_set(key, "{invalid").unwrap();
    f.api("setRoutineEnabled", json!({"botId":"bot","id":id,"enabled":false})).await.unwrap();
    assert!(f.hub.store.kv_read(key).unwrap().is_none());
    f.hub.store.kv_set(key, "{invalid").unwrap();
    f.api("deleteRoutine", json!({"botId":"bot","id":id})).await.unwrap();
    assert!(f.hub.store.kv_read(key).unwrap().is_none());
    f.hub.store.kv_set(key, "{invalid").unwrap();
    f.hub.store.db.locked().execute_batch("BEGIN IMMEDIATE").unwrap();
    {
        let db = f.hub.store.db.locked();
        external_grant::delete_bot(&db, "bot").unwrap();
        db.execute_batch("ROLLBACK").unwrap();
    }
    assert!(f.hub.store.kv_read(key).unwrap().is_some());
    external_grant::delete_bot(&f.hub.store.db.locked(), "bot").unwrap();
    assert!(f.hub.store.kv_read(key).unwrap().is_none());
}

#[tokio::test]
async fn mx472_concessao_nao_libera_pausa_d125() {
    let f = Fixture::new();
    mark_session(&f, "bot");
    let body = json!({"name":"Evento", "instruction":"Dados externos", "triggers":[{"type":"event","source":"github","event":"pull_request.opened","filters":{"repo":"org/repo"}}]});
    let id = f.call(&body).unwrap()["routine"]["id"].as_str().unwrap().to_owned();
    let card = f.cards()["items"][0].clone();
    assert_eq!(card["externalReleasePending"], true);
    f.api(REVIEW, json!({"entryId":card["entryId"],"entryRev":card["entryRev"],"choice":"grantExternal","routineRevision":card["routineRevision"],"externalGrant":{"repository":"org/repo","event":"pull_request.opened","action":{"type":"task","title":"Revisar","description":"Verificar"}}})).await.unwrap();
    assert_eq!(f.hub.routines.list("bot")["routines"][0]["enabled"], false);
    let key = format!("donna.routine-external-grant.v1.{}", crate::agent::registry::sha256_hex(id.as_bytes()));
    let saved: Value = serde_json::from_str(&f.hub.store.kv_read(&key).unwrap().unwrap()).unwrap();
    let current = f.cards()["items"][0].clone();
    f.api("setRoutineEnabled", json!({"botId":"bot","id":id,"enabled":true})).await.unwrap();
    let raw: Value = serde_json::from_str(&f.raw()).unwrap();
    let routine: Routine = serde_json::from_value(raw["routines"][0].clone()).unwrap();
    assert_eq!(external_grant::authority_revision(&routine).unwrap(), saved["authorityRevision"].as_str().unwrap());
    f.hub.store.db.locked().execute_batch("CREATE TRIGGER mx472_revoke_fail BEFORE UPDATE ON entries WHEN NEW.id LIKE 'routine-created:%' BEGIN SELECT RAISE(ABORT,'fail'); END;").unwrap();
    assert!(
        f.api(REVIEW, json!({"entryId":current["entryId"],"entryRev":current["entryRev"],"choice":"revokeExternal"}))
            .await
            .is_err()
    );
    assert!(f.hub.store.kv_read(&key).unwrap().is_some());
    f.hub.store.db.locked().execute_batch("DROP TRIGGER mx472_revoke_fail").unwrap();
    let revoked = f
        .api(REVIEW, json!({"entryId":current["entryId"],"entryRev":current["entryRev"],"choice":"revokeExternal"}))
        .await
        .unwrap();
    assert!(f.hub.store.kv_read(&key).unwrap().is_none());
    assert!(
        f.api(REVIEW, json!({"entryId":current["entryId"],"entryRev":current["entryRev"],"choice":"revokeExternal"}))
            .await
            .is_err()
    );
    let updated = f.cards()["items"][0].clone();
    assert_eq!(updated["entryRev"], revoked["rev"]);
    f.api(REVIEW, json!({"entryId":updated["entryId"],"entryRev":updated["entryRev"],"choice":"grantExternal","routineRevision":updated["routineRevision"],"externalGrant":{"repository":"ORG/REPO","event":"pull_request.opened","action":{"type":"notify","text":"Avisar"}}})).await.unwrap();
    let newer: Value = serde_json::from_str(&f.hub.store.kv_read(&key).unwrap().unwrap()).unwrap();
    assert_ne!(saved["grantId"], newer["grantId"]);
    assert_eq!(newer["contract"]["repository"], "org/repo");
    f.api("setRoutineEnabled", json!({"botId":"bot","id":id,"enabled":false})).await.unwrap();
    assert!(
        crate::routines::call_in_execution(
            &f.hub,
            "bot",
            "set_routine_enabled",
            &json!({"id":id,"enabled":true}),
            &f.execution
        )
        .is_err()
    );
}
fn start(hub: &Arc<Hub>, bot: &str, acp: &str) -> ExecutionRef {
    let binding = bindings::issue(&hub.store, bot, bot, bot, None).unwrap();
    bindings::activate(&hub.store, &binding, acp).unwrap();
    let entry = hub
        .store
        .insert_entry(&Lane::main(bot), EntryKind::User, 1, &json!({"text":"Pedido humano sintético"}))
        .unwrap();
    hub.start_bot_turn(bot, |r| {
        r.lane = Some(Lane::main(bot));
        r.session = Some(acp.into());
        r.message = Some(entry.id);
    })
    .unwrap();
    bindings::capture(hub, bot, binding.id.as_str()).unwrap()
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("routine-create-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("hub.db")).unwrap();
        store
            .save_bot(
                &serde_json::from_value(json!({"id":"bot","name":"Berga","backend":"fixture","cwd":dir})).unwrap(),
            )
            .unwrap();
        store.set_team_settings(&json!({"primaryBotId":"bot"})).unwrap();
        let hub = Hub::new(store, "fixture".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
        hub.routines.restore(&hub.store).unwrap();
        let execution = start(&hub, "bot", "acp-owner");
        Self { hub, execution, dir }
    }
    fn call(&self, body: &Value) -> Result<Value> {
        self.invoked(body, &uuid::Uuid::new_v4().to_string())
    }
    fn invoked(&self, body: &Value, invocation: &str) -> Result<Value> {
        crate::routines::call_invoked_in_execution(
            &self.hub,
            "bot",
            "save_routine",
            body,
            &self.execution,
            Some(invocation),
        )
    }
    async fn api(&self, method: &str, mut body: Value) -> Result<Value> {
        body["hubId"] = json!(self.hub.host_id);
        if body.get("spaceId").is_none() {
            body["spaceId"] = json!(PERSONAL);
        }
        dispatch(&self.hub, &Caller::Local, method, body).await
    }
    fn cards(&self) -> Value {
        serde_json::to_value(projection(&self.hub.store.db.locked(), PERSONAL, now_ms()).unwrap()).unwrap()["value"]
            .clone()
    }
    async fn limit(&self, limit: u32) {
        self.api(SAVE, json!({"botId":"bot","routineCreationLimit24h":limit})).await.unwrap();
    }
    fn raw(&self) -> String {
        self.hub.store.kv_read("routines.v1").unwrap().unwrap()
    }
    fn count(&self) -> usize {
        self.hub.routines.list("bot")["routines"].as_array().unwrap().len()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn d544_cota_conta_todo_mcp_berga_humano_app_isento_e_cartao_nao_pausa() {
    let f = Fixture::new();
    for _ in 0..10 {
        f.call(&task()).unwrap();
    }
    assert_eq!(f.count(), 10);
    let error = f.call(&task()).unwrap_err().to_string();
    assert!(error.starts_with("routine_creation_limit:"));
    assert_eq!(f.count(), 10);
    let cards = f.cards();
    assert_eq!(cards["items"].as_array().unwrap().len(), 10);
    assert!(cards["items"].as_array().unwrap().iter().all(|c| c["status"] == "active"));
    assert_eq!(cards["blocked"][0]["limit"], 10);
    assert_eq!(cards["blocked"][0]["used"], 10);
    assert_eq!(cards["blocked"][0]["settingsAvailable"], false);
    assert!(cards["blocked"][0]["howToIncrease"].as_str().unwrap().contains("Trabalho que ninguém pediu"));
    // A criação direta pelo dono no app não usa o MCP nem gasta crédito.
    let mut body = task();
    body["botId"] = json!("bot");
    f.api("saveRoutine", body).await.unwrap();
    assert_eq!(f.count(), 11);
    assert_eq!(f.cards()["items"].as_array().unwrap().len(), 10);
    assert_eq!(f.api(POLICY, json!({"botId":"bot"})).await.unwrap()["used24h"], 10);
}

#[tokio::test]
async fn d544_manter_cancelar_idempotentes_e_sem_restituir_cota() {
    let f = Fixture::new();
    f.limit(2).await;
    let kept = f.call(&task()).unwrap()["routine"]["id"].clone();
    let cancelled = f.call(&task()).unwrap()["routine"]["id"].clone();
    let cards = f.cards();
    for (id, choice, decision) in [(kept.clone(), "keep", "kept"), (cancelled.clone(), "cancel", "cancelled")] {
        let card = cards["items"].as_array().unwrap().iter().find(|c| c["routineId"] == id).unwrap();
        let request = json!({"entryId":card["entryId"],"entryRev":card["entryRev"],"choice":choice});
        let answer = f.api(REVIEW, request.clone()).await.unwrap();
        assert_eq!(answer["decision"], decision);
        assert_eq!(f.api(REVIEW, request.clone()).await.unwrap(), answer);
        let mut conflict = request;
        conflict["choice"] = json!(if choice == "keep" { "cancel" } else { "keep" });
        assert!(f.api(REVIEW, conflict).await.is_err());
    }
    assert_eq!(f.cards()["items"].as_array().unwrap().as_slice(), [] as [Value; 0]);
    assert_eq!(f.count(), 1);
    assert_eq!(f.hub.routines.list("bot")["routines"][0]["id"], kept);
    assert_eq!(f.hub.routines.list("bot")["routines"][0]["enabled"], true);
    assert!(f.call(&task()).unwrap_err().to_string().starts_with("routine_creation_limit:"));
}

#[tokio::test]
async fn d544_aviso_quota_e_rotina_rollback_juntos_na_falha_e_replay_nao_duplica() {
    let f = Fixture::new();
    let before = f.raw();
    f.hub.store.db.locked().execute_batch("CREATE TRIGGER fail_creation_notice BEFORE INSERT ON entries WHEN json_extract(NEW.data,'$.routineCreation') IS NOT NULL BEGIN SELECT RAISE(ABORT,'synthetic_notice_failure'); END;").unwrap();
    let mut events = f.hub.events.subscribe();
    assert!(f.invoked(&task(), "one-invocation").is_err());
    assert_eq!(f.raw(), before);
    assert_eq!(f.cards()["items"].as_array().unwrap().as_slice(), [] as [Value; 0]);
    assert!(events.try_recv().is_err());
    assert!(read(&f.hub.store.db.locked(), &key("quota", PERSONAL, "bot")).unwrap().is_none());
    f.hub.store.db.locked().execute_batch("DROP TRIGGER fail_creation_notice").unwrap();
    let saved = f.invoked(&task(), "one-invocation").unwrap();
    assert_eq!(f.invoked(&task(), "one-invocation").unwrap(), saved);
    assert_eq!(f.count(), 1);
    assert_eq!(f.cards()["items"].as_array().unwrap().len(), 1);
    assert_eq!(f.api(POLICY, json!({"botId":"bot"})).await.unwrap()["used24h"], 1);
    let mut changed = task();
    changed["name"] = json!("Outra intenção");
    assert!(f.invoked(&changed, "one-invocation").is_err());
    assert_eq!(f.count(), 1);
}

#[tokio::test]
async fn d544_ultima_vaga_concorrente_admite_so_uma_criacao() {
    let f = Fixture::new();
    f.limit(1).await;
    let barrier = Arc::new(Barrier::new(8));
    let outcomes = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|i| {
                let barrier = barrier.clone();
                let fixture = &f;
                scope.spawn(move || {
                    barrier.wait();
                    fixture.invoked(&task(), &format!("concurrent-{i}")).is_ok()
                })
            })
            .collect();
        workers.into_iter().map(|h| h.join().unwrap()).filter(|ok| *ok).count()
    });
    assert_eq!(outcomes, 1);
    assert_eq!(f.count(), 1);
    assert_eq!(f.cards()["items"].as_array().unwrap().len(), 1);
    assert_eq!(f.cards()["blocked"].as_array().unwrap().len(), 1);
}

#[test]
fn d544_janela_movel_exata_e_relogio_recuando_nao_devolvem_credito() {
    let f = Fixture::new();
    let now = 2 * WINDOW;
    let db = f.hub.store.db.locked();
    write(
        &db,
        &key("quota", PERSONAL, "bot"),
        &Ledger { timestamps: vec![now - WINDOW, now - WINDOW + 1, now + 1000], blocked_at: None },
    )
    .unwrap();
    assert_eq!(ledger(&db, PERSONAL, "bot", now).unwrap().timestamps, vec![now - WINDOW + 1, now + 1000]);
    assert_eq!(ledger(&db, PERSONAL, "bot", now - 100).unwrap().timestamps.len(), 3);
}

#[tokio::test]
async fn d544_reinicio_preserva_cota_e_cartao_sem_resposta() {
    let f = Fixture::new();
    f.limit(1).await;
    f.call(&task()).unwrap();
    let store = Store::open(&f.dir.join("hub.db")).unwrap();
    let hub = Hub::new(store, "fixture".into(), Identity::load_or_create(&f.dir).unwrap(), "fixture".into(), 0);
    hub.routines.restore(&hub.store).unwrap();
    let execution = start(&hub, "bot", "acp-after-restart");
    assert!(
        crate::routines::call_invoked_in_execution(
            &hub,
            "bot",
            "save_routine",
            &task(),
            &execution,
            Some("after-restart")
        )
        .unwrap_err()
        .to_string()
        .starts_with("routine_creation_limit:")
    );
    assert_eq!(hub.routines.list("bot")["routines"][0]["enabled"], true);
    let cards = serde_json::to_value(projection(&hub.store.db.locked(), PERSONAL, now_ms()).unwrap()).unwrap();
    assert_eq!(cards["value"]["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn d544_card_policy_e_criacao_ficam_no_space_do_executor() {
    let f = Fixture::new();
    let spaces = f.hub.store.spaces().unwrap();
    let created = f.api("donnaSpaceCreate", json!({"expectedRevision":spaces.revision,"name":"Trabalho","tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}})).await.unwrap();
    let work =
        created["spaces"].as_array().unwrap().iter().find(|s| s["id"] != PERSONAL).unwrap()["id"].as_str().unwrap();
    let other: BotConfig =
        serde_json::from_value(json!({"id":"work-bot","name":"Trabalho","backend":"fixture","spaceId":work})).unwrap();
    f.hub.store.save_bot(&other).unwrap();
    let other_execution = start(&f.hub, "work-bot", "acp-work");
    let mut forged = task();
    forged["botId"] = json!("work-bot");
    forged["spaceId"] = json!(work);
    f.call(&forged).unwrap();
    crate::routines::call_invoked_in_execution(
        &f.hub,
        "work-bot",
        "save_routine",
        &task(),
        &other_execution,
        Some("work-create"),
    )
    .unwrap();
    let personal = f.cards();
    assert_eq!(personal["items"].as_array().unwrap().len(), 1);
    let at_work =
        serde_json::to_value(projection(&f.hub.store.db.locked(), work, now_ms()).unwrap()).unwrap()["value"].clone();
    assert_eq!(at_work["items"].as_array().unwrap().len(), 1);
    assert_eq!(at_work["items"][0]["botId"], "work-bot");
    let card = &personal["items"][0];
    let before = f.raw();
    assert!(
        f.api(REVIEW, json!({"spaceId":work,"entryId":card["entryId"],"entryRev":card["entryRev"],"choice":"cancel"}))
            .await
            .is_err()
    );
    assert!(f.api(SAVE, json!({"spaceId":work,"botId":"bot","routineCreationLimit24h":99})).await.is_err());
    let error = f.api(REVIEW, json!({"spaceId":work,"entryId":card["entryId"],"entryRev":card["entryRev"],"choice":"grantExternal","routineRevision":card["routineRevision"],"externalGrant":{"repository":"org/repo","event":"pull_request.opened","action":{"type":"notify","text":"Avisar"}}})).await.unwrap_err();
    assert!(format!("{error:#}").contains("cross_space_reference"), "{error:#}");
    assert_eq!(f.raw(), before);
    assert_eq!(f.api(POLICY, json!({"spaceId":work,"botId":"work-bot"})).await.unwrap()["used24h"], 1);
}

#[tokio::test]
async fn d544_sessao_revogada_id_ambiguo_e_aparelho_sem_control_nao_gravam() {
    let f = Fixture::new();
    let before = f.raw();
    let binding = bindings::issue(&f.hub.store, "bot", "bot", "bot", None).unwrap();
    bindings::activate(&f.hub.store, &binding, "acp-bridge").unwrap();
    let mut servers = vec![json!({"env":[]})];
    crate::fork::ponte_credencial::provisionar(&f.hub, "bot", None, &binding, &mut servers).unwrap();
    let token =
        servers[0]["env"].as_array().unwrap().iter().find(|e| e["name"] == crate::fork::ponte_credencial::ENV).unwrap()
            ["value"]
            .as_str()
            .unwrap();
    let bridge = f.hub.pontes.autenticar(&f.hub, token).unwrap();
    assert!(
        dispatch(
            &f.hub,
            &bridge,
            REVIEW,
            json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"entryId":"none","entryRev":1,"choice":"grantExternal"})
        )
        .await
        .is_err()
    );
    assert!(
        dispatch(
            &f.hub,
            &Caller::Device { key: "synthetic".into(), scopes: vec![Scope::Control], autoridade: None },
            REVIEW,
            json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"entryId":"none","entryRev":1,"choice":"grantExternal"})
        )
        .await
        .is_err()
    );
    let mut invalid = task();
    invalid["id"] = Value::Null;
    assert!(f.call(&invalid).is_err());
    assert!(
        dispatch(
            &f.hub,
            &Caller::Device { key: "synthetic".into(), scopes: vec![], autoridade: None },
            SAVE,
            json!({"hubId":"fixture","spaceId":PERSONAL,"botId":"bot","routineCreationLimit24h":99})
        )
        .await
        .is_err()
    );
    assert!(
        dispatch(
            &f.hub,
            &Caller::Device { key: "synthetic".into(), scopes: vec![Scope::Control], autoridade: None },
            SAVE,
            json!({"hubId":"fixture","spaceId":PERSONAL,"botId":"bot","routineCreationLimit24h":99})
        )
        .await
        .is_err()
    );
    bindings::revoke(&f.hub.store, "bot", Some("acp-owner")).unwrap();
    assert!(f.call(&task()).is_err());
    assert_eq!(f.raw(), before);
    assert_eq!(f.cards()["items"].as_array().unwrap().as_slice(), [] as [Value; 0]);
}

#[tokio::test]
async fn d544_sem_pedido_humano_rotina_cria_filha_com_mesma_cadeia_sem_recibo_do_modelo() {
    use crate::fork::cadeias::{admissao, modelo::Recibo};
    let mut f = Fixture::new();
    let parent = f.hub.routines.save(&f.hub, "bot", &task()).unwrap()["routine"]["id"].as_str().unwrap().to_owned();
    let queued = f.hub.routines.enqueue(&f.hub.store, "bot", &parent, Value::Null, None, true).unwrap();
    let run_id = queued["run"]["id"].as_str().unwrap().to_owned();
    let mut raw: Value = serde_json::from_str(&f.raw()).unwrap();
    let run = raw["runs"].as_array_mut().unwrap().iter_mut().find(|r| r["id"] == run_id).unwrap();
    let receipt: Recibo = serde_json::from_value(run["_chain"].clone()).unwrap();
    admissao::despachar(&f.hub.store, &receipt).unwrap();
    run["status"] = json!("running");
    run["startedAt"] = json!(now_ms());
    f.hub.store.kv_set("routines.v1", &raw.to_string()).unwrap();
    f.hub.routines.restore(&f.hub.store).unwrap();
    let slot = crate::routines::session_slot(&run_id);
    let binding = bindings::issue_in(&f.hub.store, "bot", "bot", "bot", Some(&slot), None).unwrap();
    bindings::activate(&f.hub.store, &binding, "acp-parent-run").unwrap();
    f.hub.store.kv_set(&crate::store::run_session_key("bot", &slot), "acp-parent-run").unwrap();
    f.hub
        .start_bot_turn("bot", |r| {
            r.lane = Some(Lane::main("bot"));
            r.session = Some("acp-parent-run".into());
            r.message = None;
        })
        .unwrap();
    f.execution = bindings::capture(&f.hub, "bot", binding.id.as_str()).unwrap();
    f.hub.store.kv_set("turn.inflight.bot",&json!({"admissionVersion":1,"executionToken":f.execution.turn_token(),"attempt":"synthetic","phase":"permitted","conversationId":"bot","origin":{"kind":"routine","id":run_id},"entryIds":[]}).to_string()).unwrap();
    let mut body = task();
    body["_creationOrigin"] = json!({"chain_id":"forged-by-model"});
    let saved = f.call(&body).unwrap();
    assert!(saved["routine"].get("_creationOrigin").is_none(), "procedência privada não vai ao modelo");
    let raw: Value = serde_json::from_str(&f.raw()).unwrap();
    let child = raw["routines"].as_array().unwrap().iter().find(|r| r["id"] == saved["routine"]["id"]).unwrap();
    assert_eq!(child["_creationOrigin"]["creator"]["chain_id"], receipt.chain_id);
    assert_eq!(f.cards()["items"].as_array().unwrap().len(), 1);
    assert_eq!(f.api(POLICY, json!({"botId":"bot"})).await.unwrap()["used24h"], 1);
    // A capacidade de criar não autoriza editar/excluir nem reenviar a execução.
    assert!(
        crate::routines::call_in_execution(
            &f.hub,
            "bot",
            "delete_routine",
            &json!({"id":saved["routine"]["id"]}),
            &f.execution
        )
        .is_err()
    );
}

// MX-106/D125: rotina criada a partir de conteúdo de fora nasce pausada, avisa o dono e espera o OK
// dele no app. A marca é a do Hub (sessão visível e ACP da execução), nunca texto do modelo.
fn mark_session(f: &Fixture, session: &str) {
    f.hub.store.memory_session_mark("bot", session, "entrada externa").unwrap();
}
fn notice(f: &Fixture, routine: &str) -> String {
    let entry = entry_on(&f.hub.store.db.locked(), &format!("routine-created:{routine}")).unwrap().unwrap();
    entry.data["text"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn d125_conteudo_de_fora_nasce_pausada_avisa_e_entra_no_mission_board() {
    let f = Fixture::new();
    mark_session(&f, "bot");
    let saved = f.call(&task()).unwrap();
    assert_eq!(saved["routine"]["enabled"], false, "a criação marcada nasce pausada: {saved}");
    assert_eq!(saved["routine"]["nextRunAt"], Value::Null);
    let id = saved["routine"]["id"].as_str().unwrap();
    let text = notice(&f, id);
    assert!(text.contains("pausada"), "{text}");
    assert!(text.contains("conteúdo de fora"), "{text}");
    assert!(text.contains("ligar no Mission Board"), "{text}");
    let cards = f.cards();
    assert_eq!(cards["items"].as_array().unwrap().len(), 1, "{cards}");
    assert_eq!(cards["items"][0]["status"], "paused", "{cards}");
    assert_eq!(cards["items"][0]["nextRunAt"], Value::Null);
    assert_eq!(f.count(), 1);
    assert_eq!(f.api(POLICY, json!({"botId":"bot"})).await.unwrap()["used24h"], 1);
}

fn listed(f: &Fixture, bot: &str) -> Vec<Value> {
    f.hub.routines.list(bot)["routines"].as_array().unwrap().clone()
}

#[tokio::test]
async fn app_duplica_desligada_e_pausa_comum_nao_vira_liberacao_pendente() {
    let f = Fixture::new();
    let src = f.call(&task()).unwrap()["routine"]["id"].as_str().unwrap().to_owned();
    f.api("setRoutineEnabled", json!({"botId":"bot","id":src,"enabled":false})).await.unwrap();
    let copy = f.api("duplicateRoutine", json!({"botId":"bot","id":src})).await.unwrap();
    let copy_id = copy["routine"]["id"].as_str().unwrap();
    assert_ne!(copy_id, src);
    assert_eq!(copy["routine"]["name"], "Revisão semanal (cópia)");
    assert_eq!(copy["routine"]["enabled"], false);
    assert_eq!(copy["routine"]["externalReleasePending"], false);
    let paused = listed(&f, "bot");
    assert_eq!(paused.len(), 2);
    assert!(paused.iter().all(|r| r["externalReleasePending"] == false));
}

#[tokio::test]
async fn app_duplica_rotina_de_fora_e_a_copia_de_liberada_nasce_pendente_de_novo() {
    let f = Fixture::new();
    mark_session(&f, "bot");
    let src = f.call(&task()).unwrap()["routine"]["id"].as_str().unwrap().to_owned();
    let copy = f.api("duplicateRoutine", json!({"botId":"bot","id":src})).await.unwrap();
    let copy_id = copy["routine"]["id"].as_str().unwrap().to_owned();
    assert_eq!(copy["routine"]["enabled"], false);
    assert_eq!(copy["routine"]["externalReleasePending"], true);
    let released = f.api("setRoutineEnabled", json!({"botId":"bot","id":copy_id,"enabled":true})).await.unwrap();
    assert_eq!(released["routine"]["enabled"], true);
    assert_eq!(released["routine"]["externalReleasePending"], false);
    let again = f.api("duplicateRoutine", json!({"botId":"bot","id":copy_id})).await.unwrap();
    assert_eq!(again["routine"]["enabled"], false);
    assert_eq!(again["routine"]["externalReleasePending"], true);
}

#[tokio::test]
async fn duplicar_pelo_app_exige_autoridade_do_dono_e_nao_cruza_bots() {
    let f = Fixture::new();
    let src = f.call(&task()).unwrap()["routine"]["id"].as_str().unwrap().to_owned();
    let body = json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"botId":"bot","id":src});
    for scopes in [vec![], vec![Scope::Screen], vec![Scope::Control]] {
        let caller = Caller::Device { key: "synthetic".into(), scopes, autoridade: None };
        assert!(dispatch(&f.hub, &caller, "duplicateRoutine", body.clone()).await.is_err());
    }
    assert!(!crate::fork::ponte_credencial::PERMITIDOS.contains(&"duplicateRoutine"));
    assert_eq!(listed(&f, "bot").len(), 1, "nenhuma cópia nasceu das recusas");
    f.api("duplicateRoutine", json!({"botId":"bot","id":src})).await.unwrap();
    assert_eq!(listed(&f, "bot").len(), 2);

    f.hub
        .store
        .save_bot(
            &serde_json::from_value(json!({"id":"outro","name":"Outro","backend":"fixture","cwd":f.dir})).unwrap(),
        )
        .unwrap();
    let alheia = f.hub.routines.save(&f.hub, "outro", &task()).unwrap()["routine"]["id"].as_str().unwrap().to_owned();
    let error = f.api("duplicateRoutine", json!({"botId":"bot","id":alheia})).await.unwrap_err();
    assert_eq!(format!("{error:#}"), "routine not found");
    assert!(f.api("duplicateRoutine", json!({"botId":"sumido","id":src})).await.is_err());
    assert!(f.api("duplicateRoutine", json!({"botId":"bot"})).await.is_err());
    assert_eq!(listed(&f, "bot").len(), 2);
    assert_eq!(listed(&f, "outro").len(), 1);
}

#[tokio::test]
async fn duplicate_herda_marca_externa_e_modelo_nao_liga_a_copia() {
    let f = Fixture::new();
    mark_session(&f, "bot");
    let src = f.call(&task()).unwrap()["routine"]["id"].as_str().unwrap().to_owned();
    // Sessão limpa, sem marca: a cópia herda a marca da origem mesmo assim.
    let clean = start(&f.hub, "bot", "acp-clean");
    let copy =
        crate::routines::call_in_execution(&f.hub, "bot", "duplicate_routine", &json!({"id": src}), &clean).unwrap();
    let copy_id = copy["routine"]["id"].as_str().unwrap();
    assert_ne!(copy_id, src.as_str());
    assert_eq!(copy["routine"]["enabled"], false);
    assert_eq!(copy["routine"]["externalReleasePending"], true);
    assert!(notice(&f, copy_id).contains("pendente da sua liberação"));
    let enable = crate::routines::call_in_execution(
        &f.hub,
        "bot",
        "set_routine_enabled",
        &json!({"id": copy_id, "enabled": true}),
        &clean,
    )
    .unwrap_err()
    .to_string();
    assert!(enable.starts_with("routine_external_release:"), "{enable}");
    let listed = f.hub.routines.list("bot")["routines"].clone();
    assert_eq!(listed.as_array().unwrap().len(), 2);
    assert!(listed.as_array().unwrap().iter().any(|r| r["id"] == copy_id && r["externalReleasePending"] == true));
    assert!(listed.as_array().unwrap().iter().any(|r| r["id"] == src && r["enabled"] == false));
    let direct = f.hub.routines.duplicate_in_origin(&f.hub, "bot", &src, None, || {}).unwrap();
    let direct_id = direct["routine"]["id"].as_str().unwrap();
    let error = crate::routines::call_in_execution(
        &f.hub,
        "bot",
        "set_routine_enabled",
        &json!({"id": direct_id, "enabled": true}),
        &clean,
    )
    .unwrap_err()
    .to_string();
    assert!(error.starts_with("routine_external_release:"), "{error}");
}

#[tokio::test]
async fn d125_sem_marca_a_politica_d544_continua_ativa() {
    let f = Fixture::new();
    let saved = f.call(&task()).unwrap();
    assert_eq!(saved["routine"]["enabled"], true, "sem a marca vale a D544: {saved}");
    assert!(saved["routine"]["nextRunAt"].is_number());
    let text = notice(&f, saved["routine"]["id"].as_str().unwrap());
    assert!(text.contains("Ela está ativa."), "{text}");
    assert!(!text.contains("conteúdo de fora"), "{text}");
    assert_eq!(f.cards()["items"][0]["status"], "active");
}

#[tokio::test]
async fn d125_marca_de_outra_sessao_ou_de_outra_acp_nao_pausa() {
    let f = Fixture::new();
    mark_session(&f, "outra-conversa");
    f.hub.store.memory_session_mark("bot", "acp-de-outra-execucao", "leitura externa").unwrap();
    assert_eq!(f.call(&task()).unwrap()["routine"]["enabled"], true);
    // A marca da ACP exata desta execução conta.
    bindings::mark(&f.hub.store.db.locked(), "bot", "acp-owner", "gateway MCP").unwrap();
    assert_eq!(f.call(&task()).unwrap()["routine"]["enabled"], false);
}

#[tokio::test]
async fn d125_modelo_nao_liga_nem_roda_rotina_de_conteudo_de_fora() {
    let f = Fixture::new();
    mark_session(&f, "bot");
    let id = f.call(&task()).unwrap()["routine"]["id"].as_str().unwrap().to_owned();
    let before = f.raw();
    let enable = crate::routines::call_in_execution(
        &f.hub,
        "bot",
        "set_routine_enabled",
        &json!({"id":id,"enabled":true}),
        &f.execution,
    )
    .unwrap_err()
    .to_string();
    assert!(enable.starts_with("routine_external_release:"), "{enable}");
    let mut update = task();
    update["id"] = json!(id);
    update["enabled"] = json!(true);
    let update = f.call(&update).unwrap_err().to_string();
    assert!(update.starts_with("routine_external_release:"), "{update}");
    let run = crate::routines::call_in_execution(&f.hub, "bot", "run_routine", &json!({"id":id}), &f.execution)
        .unwrap_err()
        .to_string();
    assert!(run.starts_with("routine_external_release:"), "{run}");
    assert_eq!(f.raw(), before);
    assert_eq!(f.count(), 1);
    assert_eq!(f.hub.routines.list("bot")["routines"][0]["enabled"], false);
    assert_eq!(f.hub.routines.list("bot")["runs"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn d125_webhook_e_evento_recusam_rotina_externa_pausada() {
    let f = Fixture::new();
    mark_session(&f, "bot");
    let body = json!({"name":"Externa","instruction":"Sintética","triggers":[{"type":"webhook"}]});
    let id = f.call(&body).unwrap()["routine"]["id"].as_str().unwrap().to_owned();
    let credentials =
        crate::routines::call_in_execution(&f.hub, "bot", "routine_webhook", &json!({"id":id}), &f.execution).unwrap();
    let mut headers = crate::routines::hooks::Headers::new();
    headers.insert("authorization".into(), format!("Bearer {}", credentials["key"].as_str().unwrap()));
    assert!(matches!(
        f.hub.routines.receive(&f.hub, &id, &headers, br#"{"source":"fixture","event":"changed"}"#),
        Err(crate::routines::hooks::Refused::Paused)
    ));
    headers.insert("x-github-event".into(), "issues".into());
    assert!(matches!(
        f.hub.routines.receive(&f.hub, &id, &headers, br#"{"action":"opened"}"#),
        Err(crate::routines::hooks::Refused::Paused)
    ));
}

#[tokio::test]
async fn d125_dono_libera_no_app_e_a_rotina_volta_a_agendar() {
    let f = Fixture::new();
    mark_session(&f, "bot");
    let id = f.call(&task()).unwrap()["routine"]["id"].as_str().unwrap().to_owned();
    let released = f.api("setRoutineEnabled", json!({"botId":"bot","id":id,"enabled":true})).await.unwrap();
    assert_eq!(released["routine"]["enabled"], true);
    assert!(released["routine"]["nextRunAt"].is_number(), "{released}");
    assert_eq!(f.cards()["items"][0]["status"], "active");
    // Depois da liberação o modelo pode pausar, mas religar continua sendo gesto do dono.
    crate::routines::call_in_execution(
        &f.hub,
        "bot",
        "set_routine_enabled",
        &json!({"id":id,"enabled":false}),
        &f.execution,
    )
    .unwrap();
    let error = crate::routines::call_in_execution(
        &f.hub,
        "bot",
        "set_routine_enabled",
        &json!({"id":id,"enabled":true}),
        &f.execution,
    )
    .unwrap_err()
    .to_string();
    assert!(error.starts_with("routine_external_release:"), "{error}");
    assert_eq!(f.hub.routines.list("bot")["routines"][0]["enabled"], false);
}

#[tokio::test]
async fn d125_ligar_no_cartao_e_atomico_e_replay_nao_religa() {
    let f = Fixture::new();
    mark_session(&f, "bot");
    let id = f.call(&task()).unwrap()["routine"]["id"].as_str().unwrap().to_owned();
    let card = f.cards()["items"][0].clone();
    assert_eq!(card["externalReleasePending"], true);
    let old_request = json!({"entryId":card["entryId"],"entryRev":card["entryRev"],"choice":"enable","routineRevision":card["routineRevision"]});
    let stale = json!({"entryId":card["entryId"],"entryRev":card["entryRev"].as_i64().unwrap() + 1,"choice":"enable","routineRevision":card["routineRevision"]});
    assert!(f.api(REVIEW, stale).await.is_err());
    let wrong_routine =
        json!({"entryId":card["entryId"],"entryRev":card["entryRev"],"choice":"enable","routineRevision":"wrong"});
    assert!(f.api(REVIEW, wrong_routine).await.is_err());
    assert_eq!(f.hub.routines.list("bot")["routines"][0]["enabled"], false);
    let mut edit = task();
    edit["botId"] = json!("bot");
    edit["id"] = json!(id);
    edit["enabled"] = json!(false);
    edit["instruction"] = json!("Conteúdo alterado depois da fotografia");
    f.api("saveRoutine", edit).await.unwrap();
    assert!(f.api(REVIEW, old_request).await.is_err());
    let current = f.cards()["items"][0].clone();
    let request = json!({"entryId":current["entryId"],"entryRev":current["entryRev"],"choice":"enable","routineRevision":current["routineRevision"]});
    let answer = f.api(REVIEW, request.clone()).await.unwrap();
    assert_eq!(answer["decision"], "enabled");
    assert_eq!(f.hub.routines.list("bot")["routines"][0]["enabled"], true);
    assert!(f.hub.routines.list("bot")["routines"][0]["nextRunAt"].is_number());
    assert_eq!(f.hub.routines.list("bot")["runs"].as_array().unwrap().len(), 0);
    f.api("setRoutineEnabled", json!({"botId":"bot","id":id,"enabled":false})).await.unwrap();
    assert!(f.api(REVIEW, request).await.is_err());
    assert_eq!(f.hub.routines.list("bot")["routines"][0]["enabled"], false);
    let store = Store::open(&f.dir.join("hub.db")).unwrap();
    let hub = Hub::new(store, "fixture".into(), Identity::load_or_create(&f.dir).unwrap(), "fixture".into(), 0);
    hub.routines.restore(&hub.store).unwrap();
    assert_eq!(hub.routines.list("bot")["routines"][0]["enabled"], false);
}

#[tokio::test]
async fn d125_pausa_de_conteudo_de_fora_sobrevive_ao_reinicio() {
    let f = Fixture::new();
    mark_session(&f, "bot");
    let id = f.call(&task()).unwrap()["routine"]["id"].as_str().unwrap().to_owned();
    let store = Store::open(&f.dir.join("hub.db")).unwrap();
    let hub = Hub::new(store, "fixture".into(), Identity::load_or_create(&f.dir).unwrap(), "fixture".into(), 0);
    hub.routines.restore(&hub.store).unwrap();
    assert_eq!(hub.routines.list("bot")["routines"][0]["enabled"], false);
    let execution = start(&hub, "bot", "acp-after-restart");
    let error = crate::routines::call_in_execution(
        &hub,
        "bot",
        "set_routine_enabled",
        &json!({"id":id,"enabled":true}),
        &execution,
    )
    .unwrap_err()
    .to_string();
    assert!(error.starts_with("routine_external_release:"), "{error}");
}

fn manual_task() -> Value {
    json!({"name":"Botão","instruction":"Somente tarefa sintética","manual":true,"triggers":[]})
}

#[tokio::test]
async fn rotina_manual_pelo_bot_exige_intencao_explicita_e_roda_sob_pedido() {
    let f = Fixture::new();
    for body in [
        json!({"name":"Esquecida","instruction":"Somente tarefa sintética","triggers":[]}),
        json!({"name":"Esquecida","instruction":"Somente tarefa sintética"}),
        json!({"name":"Esquecida","instruction":"Somente tarefa sintética","triggers":[],"manual":false}),
    ] {
        assert!(f.call(&body).is_err(), "sem manual:true, lista vazia é recusada");
    }
    assert_eq!(f.count(), 0);
    let created = f.call(&manual_task()).unwrap();
    let id = created["routine"]["id"].as_str().unwrap().to_owned();
    assert_eq!(created["routine"]["nextRunAt"], Value::Null);
    let listed = f.hub.routines.list("bot");
    assert_eq!(listed["routines"][0]["manual"], true);
    assert_eq!(listed["routines"][0]["triggers"], json!([]));
    assert_eq!(listed["routines"][0]["agenda"], "Só quando eu mandar");
    let run =
        crate::routines::call_in_execution(&f.hub, "bot", "run_routine", &json!({"id":id}), &f.execution).unwrap();
    assert_eq!(run["run"]["routineId"], id);
    assert_eq!(f.hub.routines.list("bot")["runs"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn rotina_manual_do_app_roda_agora_e_nunca_agenda() {
    let f = Fixture::new();
    let saved = f.hub.routines.save(&f.hub, "bot", &manual_task()).unwrap();
    let id = saved["routine"]["id"].as_str().unwrap().to_owned();
    assert_eq!(saved["routine"]["nextRunAt"], Value::Null);
    assert!(!f.hub.routines.keeps_awake(), "nada agendado: o Hub não fica acordado por ela");
    assert_eq!(f.hub.routines.list("bot")["runs"].as_array().unwrap().len(), 0);
    let queued = f.hub.routines.enqueue(&f.hub.store, "bot", &id, json!({"source":"test"}), None, true).unwrap();
    assert_eq!(queued["run"]["routineId"], id);
    assert_eq!(f.hub.routines.list("bot")["runs"].as_array().unwrap().len(), 1);
}
