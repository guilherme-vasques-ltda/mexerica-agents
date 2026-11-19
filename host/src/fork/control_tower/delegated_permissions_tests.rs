#![allow(clippy::unwrap_used)]
use super::METHOD;
use crate::{
    LockExt,
    api::{devices::Caller, dispatch},
    fork::spaces::PERSONAL,
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, Device, DeviceSource, EntryKind, Lane, Scope, Store, now_ms},
};
use rusqlite::params;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("mx478-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("db")).unwrap();
        for id in ["a", "b", "x"] {
            let cfg: BotConfig =
                serde_json::from_value(json!({"id":id,"name":id,"spaceId":PERSONAL,"backend":"fixture"})).unwrap();
            store.save_bot(&cfg).unwrap();
        }
        let hub = Hub::new(store, "hub".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
        Self { hub, dir }
    }
    fn request(&self, id: &str, source: &str, target: &str) {
        self.hub.store.db.locked().execute(
            "INSERT INTO donna_session_requests(id,schema_version,kind,mode,requester_bot_id,requester_chat_id,requester_session_id,requester_execution_ref,target_bot_id,target_chat_id,target_session_id,chain_id,admission_id,origin_entry_id,target_entry_id,attempt,fence,state,created_at,updated_at)
             VALUES(?1,1,'inter_bot','wait',?2,?2,?2,'privado',?3,?3,?3,'cadeia','admissao','origem','destino',1,2,'running',1,1)", params![id,source,target]).unwrap();
    }
    fn card(&self, request: &str, target: &str, status: &str) -> String {
        let entry = self
            .hub
            .store
            .insert_entry(
                &Lane::main(target),
                EntryKind::Permission,
                1,
                &json!({"status":status,"author":target,"command":"SENTINELA PRIVADA"}),
            )
            .unwrap();
        self.hub.store.db.locked().execute(
            "INSERT INTO donna_delegated_permission_checkpoints(request_id,attempt,fence,space_id,execution_ref,card_id,card_revision,created_at) VALUES(?1,1,2,?2,'segredo',?3,?4,?5)",
            params![request,PERSONAL,entry.id,entry.rev,now_ms()]).unwrap();
        entry.id
    }
    async fn snapshot(&self, caller: &Caller, space: &str) -> anyhow::Result<Value> {
        dispatch(&self.hub, caller, METHOD, json!({"hubId":self.hub.host_id,"spaceId":space})).await
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn cartoes_paralelos_autorizados_so_expoem_referencias_e_excluem_resolvidos_e_orfaos() {
    let f = Fixture::new();
    f.request("pedido", "a", "b");
    let first = f.card("pedido", "b", "pending");
    let second = f.card("pedido", "b", "pending");
    f.card("pedido", "b", "answered");
    f.request("same", "b", "b");
    f.card("same", "b", "pending");
    let orphan = f.card("pedido", "b", "pending");
    f.hub.store.db.locked().execute("DELETE FROM entries WHERE id=?1", [&orphan]).unwrap();
    let snapshot = f.snapshot(&Caller::Local, PERSONAL).await.unwrap();
    let observed = &snapshot["delegatedPermissions"];
    assert_eq!(observed["availability"], "available");
    let items = observed["value"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().any(|i| i["entryId"] == first));
    assert!(items.iter().any(|i| i["entryId"] == second));
    for item in items {
        assert_eq!(item.as_object().unwrap().len(), 7);
        assert_eq!(item["requesterBotId"], "a");
        assert_eq!(item["targetBotId"], "b");
        assert_eq!(item["conversation"]["conversationId"], "b");
        assert!(item["conversation"]["revision"].as_i64().is_some());
        assert_eq!(item["mode"], "wait");
    }
    let target = snapshot["bots"].as_array().unwrap().iter().find(|b| b["botId"] == "b").unwrap();
    let pending = target["pending"]["value"].as_array().unwrap();
    for item in items {
        assert!(pending.iter().any(|p| p["entryId"] == item["entryId"]
            && p["conversation"]["conversationId"] == item["conversation"]["conversationId"]));
    }
    // A cerca antiga e um cartão de outro pedido nunca viram item autorizado.
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_delegated_permission_checkpoints SET fence=99 WHERE card_id=?1", [&second])
        .unwrap();
    assert_eq!(
        f.snapshot(&Caller::Local, PERSONAL).await.unwrap()["delegatedPermissions"]["value"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_delegated_permission_checkpoints SET fence=2 WHERE card_id=?1", [&second])
        .unwrap();
    // Uma conclusão já registrada torna inconsistente um pedido que ainda diz running.
    f.hub.store.db.locked().execute(
        "INSERT INTO donna_session_results(id,request_id,schema_version,attempt,fence,outcome,end_reason,content,digest,created_at) VALUES('resultado','pedido',1,1,2,'failed','engine_error','none',printf('%064d',1),1)", [],
    ).unwrap();
    assert_eq!(
        f.snapshot(&Caller::Local, PERSONAL).await.unwrap()["delegatedPermissions"]["value"]["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    f.hub.store.db.locked().execute("DELETE FROM donna_session_results WHERE id='resultado'", []).unwrap();
    let entry = f.hub.store.entry(&first).unwrap();
    let mut data = entry.data;
    data["status"] = "answered".into();
    f.hub.store.update_entry(&first, &data).unwrap();
    assert_eq!(
        f.snapshot(&Caller::Local, PERSONAL).await.unwrap()["delegatedPermissions"]["value"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let wire = observed.to_string();
    for secret in ["SENTINELA", "privado", "segredo", "fence", "attempt", "executionRef", "sessionRef"] {
        assert!(!wire.contains(secret));
    }
    // O registro sobrevive à reabertura; a recuperação canônica interrompe o pedido,
    // e uma fotografia posterior não promete retomada.
    {
        let reopened = Store::open(&f.dir.join("db")).unwrap();
        let db = reopened.db.locked();
        assert_eq!(
            db.query_row("SELECT count(*) FROM donna_delegated_permission_checkpoints", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            5
        );
        crate::fork::retornos::recuperar_na(&db).unwrap();
    }
    assert_eq!(
        f.snapshot(&Caller::Local, PERSONAL).await.unwrap()["delegatedPermissions"]["value"]["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn acesso_space_roster_limite_e_falha_indisponivel() {
    let f = Fixture::new();
    f.request("pedido", "a", "b");
    for _ in 0..22 {
        f.card("pedido", "b", "pending");
    }
    let snapshot = f.snapshot(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(snapshot["delegatedPermissions"]["value"]["items"].as_array().unwrap().len(), 20);
    assert_eq!(snapshot["delegatedPermissions"]["value"]["omitted"], 2);
    let created = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceCreate",
        json!({"hubId":f.hub.host_id,
        "expectedRevision":f.hub.store.spaces().unwrap().revision, "name":"Trabalho",
        "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}}),
    )
    .await
    .unwrap();
    let other = created["spaces"][1]["id"].as_str().unwrap();
    assert_eq!(
        f.snapshot(&Caller::Local, other).await.unwrap()["delegatedPermissions"]["value"]["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    // Um checkpoint com espaço divergente não revela o cartão nem no espaço de origem nem no outro.
    f.hub.store.db.locked().execute("UPDATE donna_delegated_permission_checkpoints SET space_id=?1", [other]).unwrap();
    assert_eq!(
        f.snapshot(&Caller::Local, PERSONAL).await.unwrap()["delegatedPermissions"]["value"]["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        f.snapshot(&Caller::Local, other).await.unwrap()["delegatedPermissions"]["value"]["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_delegated_permission_checkpoints SET space_id=?1", [PERSONAL])
        .unwrap();
    f.hub.store.db.locked().execute("UPDATE bots SET deleted=1 WHERE id='a'", []).unwrap();
    assert_eq!(
        f.snapshot(&Caller::Local, PERSONAL).await.unwrap()["delegatedPermissions"]["value"]["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    f.hub
        .store
        .put_device(&Device {
            key: "sem-controle".into(),
            name: "fixture".into(),
            platform: "fixture".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::OwnerDevices],
            lease_until: None,
            created_at: now_ms(),
            last_seen_at: None,
        })
        .unwrap();
    let caller = Caller::Device { key: "sem-controle".into(), scopes: vec![Scope::OwnerDevices], autoridade: None };
    assert!(f.snapshot(&caller, PERSONAL).await.is_err());
    f.hub.store.db.locked().execute("DROP TABLE donna_delegated_permission_checkpoints", []).unwrap();
    assert_eq!(
        f.snapshot(&Caller::Local, PERSONAL).await.unwrap()["delegatedPermissions"],
        json!({"availability":"unavailable"})
    );
}
