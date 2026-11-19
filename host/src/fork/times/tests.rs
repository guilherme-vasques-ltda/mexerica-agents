#![allow(clippy::unwrap_used)]
use super::{
    api,
    model::{Change, Member},
    store,
};
use crate::{
    LockExt,
    api::{devices::Caller, dispatch},
    fork::spaces::PERSONAL,
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, Device, DeviceSource, Scope, Store},
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

fn roster(store: &Store) {
    for id in ["donna", "alpha", "beta"] {
        let cfg: BotConfig = serde_json::from_value(json!({
            "id":id,"name":if id == "beta" {"Donna"} else {id},"backend":"fixture",
            "cwd":"/private-times-folder","description":"private-times-prompt",
            "command":"private-times-command","model":"private-times-model"
        }))
        .unwrap();
        store.save_bot(&cfg).unwrap();
    }
    store.set_team_settings(&json!({"primaryBotId":"donna"})).unwrap();
}

fn fixture() -> Store {
    let store = Store::open(Path::new(":memory:")).unwrap();
    roster(&store);
    store
}

fn read(store: &Store) -> Value {
    serde_json::to_value(store::snapshot(&store.db.locked(), PERSONAL).unwrap()).unwrap()
}

fn intent(snapshot: &Value) -> Value {
    json!({
        "spaceId":snapshot["spaceId"],"expectedRevision":snapshot["revision"],
        "catalogVersion":snapshot["catalogVersion"],"nodes":snapshot["nodes"],
        "members":snapshot["members"],"routes":snapshot["routes"],
        "clientRequestId":uuid::Uuid::new_v4().to_string()
    })
}

fn apply(store: &Store, request: Value, save: bool) -> anyhow::Result<(Value, bool)> {
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    let result = store::change(&tx, &serde_json::from_value::<Change>(request).unwrap(), save)?;
    tx.commit()?;
    Ok(result)
}

fn member<'a>(request: &'a mut Value, id: &str) -> &'a mut Value {
    request["members"].as_array_mut().unwrap().iter_mut().find(|m| m["botId"] == id).unwrap()
}

fn area_intent(snapshot: &Value) -> Value {
    let mut request = intent(snapshot);
    let area = uuid::Uuid::new_v4().to_string();
    request["nodes"] = json!([{"id":area,"name":"Engenharia","parentId":null,"managerBotId":"alpha","position":0}]);
    member(&mut request, "alpha")["role"] = json!("manager");
    member(&mut request, "alpha")["teamId"] = json!(area);
    member(&mut request, "beta")["teamId"] = json!(area);
    member(&mut request, "beta")["parentBotId"] = json!("alpha");
    member(&mut request, "beta")["reportsTo"] = json!({"kind":"bot","botId":"alpha"});
    member(&mut request, "beta")["ownerContact"] = json!("on_request");
    request
}

fn configs(store: &Store) -> Value {
    json!(
        store
            .bots()
            .unwrap()
            .into_iter()
            .map(|r| json!({"config":r.config,"deleted":r.deleted,"rev":r.rev,"session":r.session_id}))
            .collect::<Vec<_>>()
    )
}

#[test]
fn crud_preview_cas_receipt_and_noop_preserve_bot_configuration() {
    let store = fixture();
    let before = configs(&store);
    let initial = read(&store);
    assert_eq!(initial["primaryBotId"], "donna", "homônimo não elege principal");
    let request = area_intent(&initial);
    let mut preview = request.clone();
    preview.as_object_mut().unwrap().remove("clientRequestId");
    let (proposed, changed) = apply(&store, preview, false).unwrap();
    assert_eq!(proposed["preview"], true);
    assert!(!changed);
    assert_eq!(read(&store), initial);
    let (first, changed) = apply(&store, request.clone(), true).unwrap();
    assert!(changed);
    assert!(first["snapshot"]["revision"].as_i64().unwrap() > initial["revision"].as_i64().unwrap());
    assert_eq!(first["receipt"]["revision"], first["snapshot"]["revision"]);
    assert_eq!(apply(&store, request.clone(), true).unwrap().0, first);
    let mut reordered = request.clone();
    reordered["members"].as_array_mut().unwrap().reverse();
    assert_eq!(apply(&store, reordered, true).unwrap().0, first);
    let mut stale = request.clone();
    stale["clientRequestId"] = json!(uuid::Uuid::new_v4().to_string());
    assert!(apply(&store, stale, true).unwrap_err().to_string().contains("times_conflict"));
    let mut divergent = request.clone();
    divergent["nodes"][0]["name"] = json!("Outra intenção");
    assert!(apply(&store, divergent, true).unwrap_err().to_string().contains("request_conflict"));
    let current = read(&store);
    let (noop, changed) = apply(&store, intent(&current), true).unwrap();
    assert!(!changed);
    assert_eq!(noop["snapshot"], current);
    let mut rename = intent(&current);
    rename["nodes"][0]["name"] = json!("Plataforma");
    let updated = apply(&store, rename, true).unwrap().0;
    let retry = apply(&store, request, true).unwrap();
    assert!(!retry.1);
    assert_eq!(retry.0["receipt"], first["receipt"]);
    assert_eq!(retry.0["snapshot"], updated["snapshot"], "recibo antigo acompanha estado atual, sem reaplicar");
    assert_eq!(configs(&store), before, "Times não escreve BotConfig ou revisão do bot");
}

#[test]
fn graphs_and_compound_operations_fail_without_partial_state_or_receipt() {
    let store = fixture();
    let initial = read(&store);
    let base = area_intent(&initial);
    let mut invalid = Vec::new();
    let mut area_cycle = base.clone();
    let area_id = area_cycle["nodes"][0]["id"].clone();
    area_cycle["nodes"][0]["parentId"] = area_id;
    invalid.push((area_cycle, "area_cycle"));
    let mut subbot_cycle = base.clone();
    member(&mut subbot_cycle, "alpha")["parentBotId"] = json!("beta");
    invalid.push((subbot_cycle, "subbot_cycle"));
    let mut report_cycle = base.clone();
    member(&mut report_cycle, "alpha")["reportsTo"] = json!({"kind":"bot","botId":"beta"});
    member(&mut report_cycle, "beta")["role"] = json!("manager");
    invalid.push((report_cycle, "report_cycle"));
    let mut compound_cycle = base.clone();
    member(&mut compound_cycle, "beta")["parentBotId"] = Value::Null;
    member(&mut compound_cycle, "alpha")["parentBotId"] = json!("beta");
    invalid.push((compound_cycle, "compound_cycle"));
    let mut contact = base.clone();
    member(&mut contact, "alpha")["ownerContact"] = json!("on_request");
    invalid.push((contact, "invalid_contact"));
    let mut downgrade = base;
    member(&mut downgrade, "alpha")["role"] = json!("executor");
    invalid.push((downgrade, "manager_unavailable"));
    for (request, code) in invalid {
        let error = apply(&store, request, true).unwrap_err();
        assert!(error.to_string().contains(code), "{error}");
        assert_eq!(read(&store), initial);
    }
    let count: i64 = store.db.locked().query_row("SELECT COUNT(*) FROM donna_team_receipts", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn sql_failure_rolls_back_graph_revision_and_nonce_then_allows_retry() {
    let store = fixture();
    let initial = read(&store);
    let request = area_intent(&initial);
    let rev = store.current_rev();
    store.db.locked().execute_batch("CREATE TEMP TRIGGER times_test_fail BEFORE INSERT ON donna_team_receipts BEGIN SELECT RAISE(ABORT,'falha de teste'); END;").unwrap();
    assert!(apply(&store, request.clone(), true).is_err());
    assert_eq!(read(&store), initial);
    assert_eq!(store.current_rev(), rev);
    store.db.locked().execute_batch("DROP TRIGGER times_test_fail").unwrap();
    assert!(apply(&store, request, true).unwrap().1);
    let violations: i64 =
        store.db.locked().query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0)).unwrap();
    assert_eq!(violations, 0);
}

#[test]
fn proposed_routes_require_existing_bots_and_explicit_removal_before_delete() {
    let store = fixture();
    let before = configs(&store);
    let mut request = intent(&read(&store));
    request["routes"] = json!([
        {"fromBotId":"alpha","toBotId":"beta","state":"proposed"},
        {"fromBotId":"beta","toBotId":"alpha","state":"proposed"}
    ]);
    let saved = apply(&store, request, true).unwrap().0["snapshot"].clone();
    assert_eq!(configs(&store), before);
    assert!(store.delete_bot("alpha").unwrap_err().to_string().contains("times_dependents"));
    let mut invalid = intent(&saved);
    invalid["routes"][0]["toBotId"] = json!("missing");
    assert!(apply(&store, invalid, true).unwrap_err().to_string().contains("invalid_route"));
    let mut active = intent(&saved);
    active["routes"][0]["state"] = json!("active");
    assert!(serde_json::from_value::<Change>(active).is_err());
    let mut clear = intent(&saved);
    clear["routes"] = json!([]);
    apply(&store, clear, true).unwrap();
    store.delete_bot("alpha").unwrap();
    assert!(store.bot("alpha").unwrap().unwrap().deleted);
}

#[test]
fn source_rejects_cross_space_group_deleted_and_forged_roster() {
    let store = fixture();
    let mut spaces = store.spaces().unwrap();
    let mut work = spaces.spaces[0].clone();
    work.id = uuid::Uuid::new_v4().to_string();
    work.kind = "space".into();
    work.name = "Trabalho".into();
    spaces.spaces.push(work.clone());
    store.kv_set("donna.spaces.v1", &serde_json::to_string(&spaces).unwrap()).unwrap();
    for (id, space, kind) in
        [("cross", work.id.as_str(), "agent"), ("group", PERSONAL, "group"), ("deleted", PERSONAL, "agent")]
    {
        let cfg = serde_json::from_value(json!({"id":id,"name":id,"spaceId":space,"kind":kind})).unwrap();
        store.save_bot(&cfg).unwrap();
    }
    store.delete_bot("deleted").unwrap();
    let initial = read(&store);
    assert_eq!(initial["bots"].as_array().unwrap().len(), 3);
    for id in ["cross", "group", "deleted", "missing"] {
        let mut request = intent(&initial);
        member(&mut request, "beta")["botId"] = json!(id);
        assert!(apply(&store, request, true).unwrap_err().to_string().contains("bot_catalog_conflict"));
        let mut request = area_intent(&initial);
        request["nodes"][0]["managerBotId"] = json!(id);
        assert!(apply(&store, request, true).unwrap_err().to_string().contains("manager_unavailable"));
    }
    let other = store::snapshot(&store.db.locked(), &work.id).unwrap();
    assert!(other.primary_bot_id.is_none());
    assert_eq!(other.bots.len(), 1);
    assert!(store::snapshot(&store.db.locked(), "missing-space").is_err());
    assert_eq!(read(&store), initial);
}

#[test]
fn catalog_changes_require_review_and_primary_projection_keeps_other_relations() {
    let store = fixture();
    let initial = read(&store);
    let request = area_intent(&initial);
    let first = apply(&store, request.clone(), true).unwrap().0["snapshot"].clone();
    let stale = intent(&first);
    store.sem_donna_fixa(); // D366: exercita a troca/exclusão legada, sem Donna fixa.
    store.set_team_settings(&json!({"primaryBotId":"alpha"})).unwrap();
    let current = read(&store);
    assert_eq!(current["primaryBotId"], "alpha");
    assert_ne!(current["catalogVersion"], first["catalogVersion"]);
    assert!(apply(&store, stale, true).unwrap_err().to_string().contains("catalog_conflict"));
    let mut projected = intent(&current);
    assert_eq!(member(&mut projected, "alpha"), &json!(Member::initial("alpha".into(), true)));
    assert_eq!(member(&mut projected, "beta")["reportsTo"], json!({"kind":"bot","botId":"alpha"}));
    assert_eq!(member(&mut projected, "beta")["parentBotId"], "alpha");
    assert!(apply(&store, projected, true).unwrap().1, "confirmação materializa a projeção da nova principal");
    let retried = apply(&store, request, true).unwrap().0;
    assert_eq!(retried["snapshot"]["primaryBotId"], "alpha");
    let mut cfg = store.bot("beta").unwrap().unwrap().config;
    cfg.hidden = true;
    let stale = intent(&read(&store));
    store.save_bot(&cfg).unwrap();
    assert!(apply(&store, stale, true).unwrap_err().to_string().contains("catalog_conflict"));
    let mut edit_primary = intent(&read(&store));
    member(&mut edit_primary, "alpha")["role"] = json!("executor");
    assert!(apply(&store, edit_primary, true).is_err());
}

#[test]
fn delete_and_downgrade_require_explicit_reallocation_in_final_graph() {
    let store = fixture();
    let saved = apply(&store, area_intent(&read(&store)), true).unwrap().0["snapshot"].clone();
    let rev = store.current_rev();
    assert!(store.delete_bot("alpha").unwrap_err().to_string().contains("times_dependents"));
    assert!(!store.bot("alpha").unwrap().unwrap().deleted);
    assert_eq!(store.current_rev(), rev);
    let mut remove_area = intent(&saved);
    remove_area["nodes"] = json!([]);
    assert!(apply(&store, remove_area, true).unwrap_err().to_string().contains("team_not_found"));
    let mut reallocate = intent(&saved);
    reallocate["nodes"] = json!([]);
    for id in ["alpha", "beta"] {
        let row = member(&mut reallocate, id);
        row["teamId"] = Value::Null;
        row["parentBotId"] = Value::Null;
        row["role"] = json!("executor");
        row["reportsTo"] = json!({"kind":"owner"});
        row["ownerContact"] = json!("reports");
    }
    apply(&store, reallocate, true).unwrap();
    store.delete_bot("alpha").unwrap();
    assert_eq!(read(&store)["members"].as_array().unwrap().len(), 2);
    let mut request = intent(&read(&store));
    member(&mut request, "beta")["reportsTo"] = json!({"kind":"bot","botId":"donna"});
    apply(&store, request, true).unwrap();
    assert!(store.delete_bot("donna").is_err());
    assert_eq!(store.team_settings().unwrap().primary_bot_id.as_deref(), Some("donna"));
}

#[test]
fn migration_second_open_preserves_graph_receipt_and_configs() {
    let dir = std::env::temp_dir().join(format!("donnabot-times-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("host.db");
    let store = Store::open(&path).unwrap();
    roster(&store);
    let request = area_intent(&read(&store));
    let first = apply(&store, request.clone(), true).unwrap().0;
    let before = configs(&store);
    drop(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(read(&reopened), first["snapshot"]);
    assert_eq!(apply(&reopened, request, true).unwrap().0["receipt"], first["receipt"]);
    assert_eq!(configs(&reopened), before);
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn bounded_closed_json_and_depth_do_not_persist_invalid_graphs() {
    let store = fixture();
    let initial = read(&store);
    let mut request = intent(&initial);
    request["actor"] = json!("owner");
    assert!(serde_json::from_value::<Change>(request).is_err());
    let mut request = intent(&initial);
    member(&mut request, "beta")["reportsTo"] = json!({"kind":"owner","botId":"donna"});
    assert!(serde_json::from_value::<Change>(request).is_err());
    let mut request = area_intent(&initial);
    request["nodes"][0]["id"] = json!("not-a-uuid");
    assert!(apply(&store, request, true).is_err());
    let mut request = intent(&initial);
    let mut nodes = Vec::new();
    let mut parent: Option<String> = None;
    for _ in 0..9 {
        let id = uuid::Uuid::new_v4().to_string();
        nodes.push(json!({"id":id,"name":"Área","parentId":parent,"managerBotId":null,"position":0}));
        parent = Some(id);
    }
    request["nodes"] = json!(nodes);
    assert!(apply(&store, request, true).unwrap_err().to_string().contains("times_depth"));
    assert_eq!(read(&store), initial);
}

fn with_hub() -> (Arc<Hub>, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("donnabot-times-api-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(fixture(), "times-fixture".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
    (hub, dir)
}

fn cloud_owner(hub: &Hub) -> (Device, Caller) {
    use crate::remote::cloud;
    cloud::update_status(hub, |s| {
        s.owner = Some(cloud::Owner { user_id: "dono-times".into(), email: None });
    });
    hub.cloud_synced.store(true, std::sync::atomic::Ordering::Release);
    let device = Device {
        key: "controle-times".into(),
        name: "Teste".into(),
        platform: "fixture".into(),
        source: DeviceSource::Account,
        grant_id: Some("concessao-times".into()),
        scopes: vec![Scope::Control],
        lease_until: Some(crate::store::now_ms() + 60_000),
        created_at: 0,
        last_seen_at: None,
    };
    hub.store.put_device(&device).unwrap();
    let (_, caller) = crate::fork::autoridade::autenticar(hub, &device.key).unwrap();
    (device, caller)
}

#[tokio::test]
async fn autoridade_revogada_na_espera_do_quadro_nao_grava_nem_emite_recibo() {
    let (hub, dir) = with_hub();
    let (mut device, caller) = cloud_owner(&hub);
    let before = read(&hub.store);
    let revision = hub.store.current_rev();
    let request = area_intent(&before);
    let mut events = hub.events.subscribe();
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let pending = api::call(&hub, &caller, "donnaTimesSave", request);
    tokio::pin!(pending);
    // Poll determinístico: a captura já terminou e o pedido está esperando team_lock.
    tokio::select! {
        biased;
        result = &mut pending => panic!("pedido não esperou a trava: {result:?}"),
        () = std::future::ready(()) => {}
    }
    device.scopes.clear();
    hub.store.put_device(&device).unwrap();
    drop(guard);
    assert!(pending.await.is_err());
    assert_eq!(read(&hub.store), before);
    assert_eq!(hub.store.current_rev(), revision);
    let receipts: i64 =
        hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_team_receipts", [], |r| r.get(0)).unwrap();
    assert_eq!(receipts, 0);
    assert!(events.try_recv().is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn cloud_control_salva_e_replay_conserva_recibo_e_revisao() {
    let (hub, dir) = with_hub();
    let (_, caller) = cloud_owner(&hub);
    let snapshot = dispatch(&hub, &caller, "donnaTimes", json!({})).await.unwrap();
    let request = area_intent(&snapshot);
    let mut preview = request.clone();
    preview.as_object_mut().unwrap().remove("clientRequestId");
    let preview = dispatch(&hub, &caller, "donnaTimesPreview", preview).await.unwrap();
    let mut events = hub.events.subscribe();
    let saved = dispatch(&hub, &caller, "donnaTimesSave", request.clone()).await.unwrap();
    assert_eq!(preview["nodes"], saved["snapshot"]["nodes"]);
    assert_eq!(events.try_recv().unwrap()["type"], "times");
    let revision = hub.store.current_rev();
    let replay = dispatch(&hub, &caller, "donnaTimesSave", request).await.unwrap();
    assert_eq!(replay, saved);
    assert_eq!(hub.store.current_rev(), revision);
    assert!(events.try_recv().is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn owner_transport_and_forged_json_do_not_offer_times_to_bots() {
    let (hub, dir) = with_hub();
    let (_, owner) = cloud_owner(&hub);
    let snapshot = dispatch(&hub, &owner, "donnaTimes", json!({})).await.unwrap();
    for key in ["description", "backend", "cwd", "model", "command", "forkEditor", "manageTeam", "sessionId", "memory"]
    {
        assert!(snapshot["bots"].as_array().unwrap().iter().all(|b| b.get(key).is_none()));
    }
    assert!(!snapshot.to_string().contains("private-times"));
    let before = configs(&hub.store);
    for caller in [
        Caller::Device { key: "owner".into(), scopes: vec![Scope::Control], autoridade: None },
        Caller::Device { key: "read".into(), scopes: vec![], autoridade: None },
        Caller::Device { key: "bot".into(), scopes: vec![Scope::Executor], autoridade: None },
        Caller::Device { key: "devices".into(), scopes: vec![Scope::OwnerDevices], autoridade: None },
        Caller::Pairing { key: "pairing".into() },
    ] {
        for method in api::METHODS {
            assert!(
                api::call(&hub, &caller, method, json!({"actor":"owner","owner":true,"spaceId":PERSONAL}))
                    .await
                    .is_err()
            );
            assert!(dispatch(&hub, &caller, method, json!({})).await.is_err());
        }
    }
    assert!(dispatch(&hub, &owner, "donnaTimes", json!({"caller":"Local"})).await.is_err());
    let mut preview = area_intent(&snapshot);
    assert!(dispatch(&hub, &owner, "donnaTimesPreview", preview.clone()).await.is_err());
    preview["clientRequestId"] = Value::Null;
    assert!(dispatch(&hub, &owner, "donnaTimesPreview", preview).await.is_err());
    for methods in [crate::chat::team::tools(), crate::chat::manage::tools()] {
        assert!(!methods.to_string().contains("donnaTimes"));
    }
    assert!(crate::chat::team::call(&hub, "donna", "donnaTimes", &json!({})).await.is_err());
    assert_eq!(configs(&hub.store), before);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn lock_revalidates_roster_and_events_only_follow_committed_changes() {
    let (hub, dir) = with_hub();
    let request = area_intent(&read(&hub.store));
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let worker_hub = hub.clone();
    let pending = tokio::spawn(async move { dispatch(&worker_hub, &Caller::Local, "donnaTimesSave", request).await });
    tokio::task::yield_now().await;
    let cfg = serde_json::from_value(json!({"id":"new","name":"Novo"})).unwrap();
    hub.store.save_bot(&cfg).unwrap();
    drop(guard);
    assert!(pending.await.unwrap().unwrap_err().to_string().contains("catalog_conflict"));
    let mut events = hub.events.subscribe();
    let request = area_intent(&read(&hub.store));
    let mut preview = request.clone();
    preview.as_object_mut().unwrap().remove("clientRequestId");
    dispatch(&hub, &Caller::Local, "donnaTimesPreview", preview).await.unwrap();
    assert!(events.try_recv().is_err());
    let saved = dispatch(&hub, &Caller::Local, "donnaTimesSave", request.clone()).await.unwrap();
    assert_eq!(
        events.try_recv().unwrap(),
        json!({
            "type":"times","spaceId":PERSONAL,"revision":saved["snapshot"]["revision"]
        })
    );
    assert_eq!(read(&hub.store), saved["snapshot"]);
    dispatch(&hub, &Caller::Local, "donnaTimesSave", request).await.unwrap();
    assert!(events.try_recv().is_err());
    hub.set_runtime("alpha", |r| r.activity = "não interromper".into());
    assert!(hub.delete_bot("alpha").is_err());
    assert_eq!(hub.runtime("alpha").activity, "não interromper");
    std::fs::remove_dir_all(dir).unwrap();
}

fn area_of(snapshot: &Value) -> &Value {
    &snapshot["nodes"][0]
}

fn role(snapshot: &Value, id: &str) -> Value {
    snapshot["members"].as_array().unwrap().iter().find(|m| m["botId"] == id).unwrap()["role"].clone()
}

#[test]
fn d340_sole_member_becomes_chief_and_follows_entry_and_exit() {
    let store = fixture();
    let initial = read(&store);
    let mut request = intent(&initial);
    let area = uuid::Uuid::new_v4().to_string();
    request["nodes"] = json!([{"id":area,"name":"Roteiros","parentId":null,"managerBotId":null,"position":0}]);
    member(&mut request, "alpha")["teamId"] = json!(area);
    let mut preview = request.clone();
    preview.as_object_mut().unwrap().remove("clientRequestId");
    let proposed = apply(&store, preview, false).unwrap().0;
    let (saved, changed) = apply(&store, request.clone(), true).unwrap();
    assert!(changed);
    let saved = &saved["snapshot"];
    assert_eq!(area_of(saved)["managerBotId"], "alpha");
    assert_eq!(role(saved, "alpha"), "manager");
    assert_eq!(proposed["nodes"], saved["nodes"], "prévia e Salvar iguais");
    assert_eq!(proposed["members"], saved["members"]);
    let first = apply(&store, request.clone(), true).unwrap();
    assert_eq!(first.0["snapshot"], *saved, "replay devolve o recibo original, sem conflito");
    assert!(!first.1);

    // Entra o 2º membro: o chefe continua.
    let mut join = intent(saved);
    member(&mut join, "beta")["teamId"] = json!(area);
    let joined = apply(&store, join, true).unwrap().0["snapshot"].clone();
    assert_eq!(area_of(&joined)["managerBotId"], "alpha");

    // Sai o 2º e depois o único (chefe): a área fica sem chefe e o papel não é rebaixado.
    let mut leave = intent(&joined);
    member(&mut leave, "beta")["teamId"] = json!(null);
    let alone = apply(&store, leave, true).unwrap().0["snapshot"].clone();
    assert_eq!(area_of(&alone)["managerBotId"], "alpha");
    let mut out = intent(&alone);
    member(&mut out, "alpha")["teamId"] = json!(null);
    let empty = apply(&store, out, true).unwrap().0["snapshot"].clone();
    assert_eq!(area_of(&empty)["managerBotId"], Value::Null);
    assert_eq!(role(&empty, "alpha"), "manager");
}

#[test]
fn d340_informed_chief_and_two_members_without_chief_do_not_change() {
    let store = fixture();
    let initial = read(&store);
    // Chefe informado de fora (alpha nunca foi membro direto); beta é o único membro e não vira chefe.
    let mut first = area_intent(&initial);
    member(&mut first, "alpha")["teamId"] = json!(null);
    let informed = apply(&store, first, true).unwrap().0["snapshot"].clone();
    assert_eq!(area_of(&informed)["managerBotId"], "alpha");
    assert_eq!(role(&informed, "beta"), "executor");
    // O único membro sai e a área fica vazia: o chefe informado de fora fica como está.
    let mut outside = intent(&informed);
    member(&mut outside, "beta")["teamId"] = json!(null);
    member(&mut outside, "beta")["parentBotId"] = json!(null);
    outside["nodes"][0]["managerBotId"] = json!("alpha");
    let mut second = outside["nodes"][0].clone();
    second["id"] = json!(uuid::Uuid::new_v4().to_string());
    second["name"] = json!("Dupla");
    outside["nodes"].as_array_mut().unwrap().push(second.clone());
    let kept = apply(&store, outside, true).unwrap().0["snapshot"].clone();
    let node = |id: &Value| kept["nodes"].as_array().unwrap().iter().find(|n| n["id"] == *id).unwrap().clone();
    assert_eq!(node(&informed["nodes"][0]["id"])["managerBotId"], "alpha");
    // Dois membros sem chefe: nenhum vira chefe.
    let mut pair = intent(&kept);
    let dupla = second["id"].clone();
    pair["nodes"].as_array_mut().unwrap().iter_mut().find(|n| n["id"] == dupla).unwrap()["managerBotId"] = json!(null);
    member(&mut pair, "beta")["teamId"] = dupla.clone();
    member(&mut pair, "alpha")["teamId"] = dupla.clone();
    let both = apply(&store, pair, true).unwrap().0["snapshot"].clone();
    let n = both["nodes"].as_array().unwrap().iter().find(|n| n["id"] == dupla).unwrap();
    assert_eq!(n["managerBotId"], Value::Null);
    assert_eq!(role(&both, "beta"), "executor");
}

#[test]
fn d340_migration_youtube_joio_and_roteiros_crivo_top_down_once() {
    // Caso real do dono: Youtube sem chefe só com o Joio (alpha) e a subequipe Roteiros sem chefe só com o Crivo
    // (beta). Subequipe e membros dela não contam para a Youtube.
    let store = fixture();
    let initial = read(&store);
    let mut request = intent(&initial);
    let (youtube, roteiros) = (uuid::Uuid::new_v4().to_string(), uuid::Uuid::new_v4().to_string());
    request["nodes"] = json!([
        {"id":youtube,"name":"Youtube","parentId":null,"managerBotId":null,"position":0},
        {"id":roteiros,"name":"Roteiros","parentId":youtube,"managerBotId":null,"position":1}
    ]);
    member(&mut request, "alpha")["teamId"] = json!(youtube);
    member(&mut request, "beta")["teamId"] = json!(roteiros);
    let saved = apply(&store, request, true).unwrap().0["snapshot"].clone();
    let manager_of = |s: &Value, id: &str| {
        s["nodes"].as_array().unwrap().iter().find(|n| n["id"] == id).unwrap()["managerBotId"].clone()
    };
    assert_eq!(manager_of(&saved, &youtube), "alpha");
    assert_eq!(manager_of(&saved, &roteiros), "beta");
    assert_eq!(
        member(&mut saved.clone(), "beta")["reportsTo"],
        json!({"kind":"owner"}),
        "Salvar só grava chefe e papel"
    );
    // Estado legado (antes da D340): as duas sem chefe, os dois executores reportando a Você.
    store
        .db
        .locked()
        .execute_batch("UPDATE donna_team_nodes SET manager_bot_id=NULL; UPDATE donna_bot_org SET role='executor' WHERE bot_id IN ('alpha','beta');")
        .unwrap();
    let legacy = read(&store);
    super::migrate(&store.db.locked()).unwrap();
    let mut migrated = read(&store);
    assert_eq!(manager_of(&migrated, &youtube), "alpha", "Joio chefe da Youtube");
    assert_eq!(manager_of(&migrated, &roteiros), "beta", "Crivo chefe da Roteiros");
    assert_eq!(role(&migrated, "alpha"), "manager");
    assert_eq!(role(&migrated, "beta"), "manager");
    assert_eq!(
        member(&mut migrated, "beta")["reportsTo"],
        json!({"kind":"bot","botId":"alpha"}),
        "Crivo reporta ao Joio"
    );
    assert_eq!(member(&mut migrated, "alpha")["reportsTo"], json!({"kind":"owner"}));
    assert!(migrated["revision"].as_i64().unwrap() > legacy["revision"].as_i64().unwrap());
    super::migrate(&store.db.locked()).unwrap();
    assert_eq!(read(&store), migrated, "2ª execução não muda nada nem a revisão");
}

/// D366: o Berga fica no topo, gerente e reportando a Você; nenhuma escrita o põe abaixo de alguém.
#[test]
fn d366_donna_nao_fica_abaixo_de_ninguem() {
    let store = fixture();
    let initial = read(&store);
    for change in [
        json!({"reportsTo":{"kind":"bot","botId":"alpha"},"ownerContact":"on_request"}),
        json!({"parentBotId":"alpha"}),
        json!({"role":"executor"}),
    ] {
        let mut request = intent(&initial);
        member(&mut request, "alpha")["role"] = json!("manager");
        for (key, value) in change.as_object().unwrap() {
            member(&mut request, "donna")[key] = value.clone();
        }
        assert!(apply(&store, request, true).is_err(), "{change}");
        assert_eq!(read(&store), initial);
    }
}
