#![allow(clippy::unwrap_used)]
use super::METHOD;
use crate::{
    LockExt,
    api::{devices::Caller, dispatch},
    fork::spaces::PERSONAL,
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, Scope, Store},
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("donna-native-catalog-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Hub::new(
            Store::open(std::path::Path::new(":memory:")).unwrap(),
            "catalog-test".into(),
            Identity::load_or_create(&dir).unwrap(),
            "test".into(),
            0,
        );
        Self { hub, dir }
    }
    fn request(&self, space: &str) -> Value {
        json!({"hubId":self.hub.host_id,"spaceId":space})
    }
    async fn read(&self, caller: &Caller, space: &str) -> Value {
        dispatch(&self.hub, caller, METHOD, self.request(space)).await.unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn cinco_previas_autorais_versionadas_repetem_sem_criar_ou_executar() {
    let f = Fixture::new();
    let mut events = f.hub.events.subscribe();
    let changes = f.hub.store.db.locked().total_changes();
    let revision = f.hub.store.current_rev();
    let first = f.read(&Caller::Local, PERSONAL).await;
    assert_eq!(first, f.read(&Caller::Local, PERSONAL).await);
    assert_eq!(first["hubId"], f.hub.host_id);
    assert_eq!(first["spaceId"], PERSONAL);
    assert_eq!(first["catalogVersion"], 2);
    assert_eq!(first["links"].as_array().unwrap().len(), 5);
    assert!(first["links"].as_array().unwrap().iter().all(|link| link["state"] == "available"));
    let profiles = first["profiles"].as_array().unwrap();
    assert_eq!(profiles.len(), 5);
    let donna_avatar = serde_json::to_value(crate::fork_avatars::preset("", "", true)).unwrap();
    for (profile, (key, name, avatar_id)) in profiles.iter().zip([
        ("pesquisador", "Pesquisador", "research"),
        ("redator", "Redator", "escritor"),
        ("financeiro", "Financeiro", "financeiro"),
        ("operador", "Operador", "estoque"),
        ("planejador", "Planejador", "pm"),
    ]) {
        assert_eq!(profile["catalogKey"], key);
        assert_eq!(profile["name"], name);
        assert_eq!(profile["profileVersion"], 2);
        assert_eq!(profile["origin"], "donnaOriginal");
        let avatar = serde_json::to_value(crate::fork_avatars::preset(avatar_id, "", false)).unwrap();
        assert_eq!(profile["avatarConfig"], avatar);
        assert_ne!(profile["avatarConfig"], donna_avatar);
        let alma = profile["description"].as_str().unwrap();
        assert_ne!(alma, "");
        assert!(alma.chars().count() <= 4000);
        assert!(crate::chat::profile::checked_instructions(alma).is_ok());
        for field in ["permission", "engineAccountId", "skills", "connectors", "computer", "manageTeam"] {
            assert!(profile.get(field).is_none(), "catálogo não concede {field}");
        }
    }
    assert!(f.hub.store.bots().unwrap().is_empty());
    assert_eq!(f.hub.store.current_rev(), revision);
    assert_eq!(f.hub.store.db.locked().total_changes(), changes, "nenhuma escrita, inclusive memória/rotina");
    assert!(!f.hub.busy());
    assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn autoridade_corpo_e_contexto_recusam_sem_vazar_roster_ou_configuracao() {
    let f = Fixture::new();
    let state = f.hub.store.spaces().unwrap();
    let spaces = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceCreate",
        json!({
            "hubId":f.hub.host_id,"expectedRevision":state.revision,"name":"Reservado",
            "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"RE"}}
        }),
    )
    .await
    .unwrap();
    let other = spaces["spaces"][1]["id"].as_str().unwrap();
    let cfg: BotConfig = serde_json::from_value(json!({
        "id":"privado","name":"Nome privado","spaceId":other,"backend":"fixture",
        "description":"Alma privada","cwd":f.dir,"manageTeam":false
    }))
    .unwrap();
    f.hub.store.save_bot(&cfg).unwrap();
    let changes = f.hub.store.db.locked().total_changes();
    let control = Caller::Device { key: "owner".into(), scopes: vec![Scope::Control], autoridade: None };
    let personal = f.read(&control, PERSONAL).await;
    let second = f.read(&control, other).await;
    assert_eq!(personal["profiles"], second["profiles"]);
    for answer in [&personal, &second] {
        assert!(!answer.to_string().contains("privado"));
        assert!(!answer.to_string().contains("Reservado"));
        assert!(!answer.to_string().contains("Alma privada"));
        assert_eq!(answer.as_object().unwrap().len(), 5);
    }
    for caller in [
        Caller::Device { key: "none".into(), scopes: vec![], autoridade: None },
        Caller::Device { key: "executor".into(), scopes: vec![Scope::Executor], autoridade: None },
        Caller::Device { key: "screen".into(), scopes: vec![Scope::Screen], autoridade: None },
        Caller::Device { key: "devices".into(), scopes: vec![Scope::OwnerDevices], autoridade: None },
        Caller::Pairing { key: "pairing".into() },
        Caller::Bootstrap { key: "bootstrap".into(), bootstrap_id: "bootstrap".into() },
    ] {
        assert!(dispatch(&f.hub, &caller, METHOD, f.request(PERSONAL)).await.is_err());
        assert!(super::read(&f.hub, &caller, json!({"spaceId":PERSONAL})).is_err());
    }
    for request in [
        json!({"hubId":f.hub.host_id,"spaceId":"inexistente"}),
        json!({"hubId":"outro-hub","spaceId":PERSONAL}),
        json!({"spaceId":PERSONAL}),
        json!({"hubId":f.hub.host_id}),
        json!({"hubId":f.hub.host_id,"spaceId":null}),
        json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"botId":"privado"}),
        json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"profileVersion":0}),
        json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"permission":"auto","manageTeam":true}),
        json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"engineAccountId":"conta","skills":["skill"]}),
    ] {
        assert!(dispatch(&f.hub, &control, METHOD, request).await.is_err());
    }
    assert_eq!(f.hub.store.db.locked().total_changes(), changes);
    assert_eq!(f.hub.store.bots().unwrap().len(), 1);
    assert_eq!(f.hub.store.bot("privado").unwrap().unwrap().config.description, "Alma privada");
    assert!(!f.hub.busy());
}

#[tokio::test]
async fn catalogo_nao_e_ferramenta_anunciada_nem_acao_do_chat() {
    let f = Fixture::new();
    let tools = [crate::chat::outbox::tools(), crate::chat::team::tools(), crate::chat::manage::tools()];
    for list in tools {
        assert!(list.as_array().unwrap().iter().all(|tool| tool["name"] != METHOD));
    }
    // O filtro usado pelo anúncio MCP também retira uma inclusão acidental.
    assert!(!crate::fork::bot_editor::authorization::retain_tool(&json!({"name":METHOD})));
    assert!(!crate::fork::bot_editor::authorization::retain_tool(&json!({"name":super::ADD_METHOD})));
    for list in [crate::chat::outbox::tools(), crate::chat::team::tools(), crate::chat::manage::tools()] {
        assert!(list.as_array().unwrap().iter().all(|tool| tool["name"] != super::ADD_METHOD));
    }
    let cfg: BotConfig = serde_json::from_value(json!({
        "id":"manual","name":"Bot manual","backend":"fixture","cwd":f.dir
    }))
    .unwrap();
    f.hub.store.save_bot(&cfg).unwrap();
    crate::fork::sessoes::bindings::fixture_execution(&f.hub, "manual");
    let binding = crate::fork::sessoes::bindings::for_acp(&f.hub.store, "manual", "fixture-acp").unwrap();
    let changes = f.hub.store.db.locked().total_changes();
    for method in [METHOD, super::ADD_METHOD] {
        let error = dispatch(
            &f.hub,
            &Caller::Local,
            "chatCall",
            json!({"botId":"manual","sessionBinding":binding.id.as_str(),"name":method,"arguments":f.request(PERSONAL)}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), format!("unknown chat tool: {method}"));
    }
    assert_eq!(f.hub.store.bots().unwrap().len(), 1);
    assert_eq!(f.hub.store.db.locked().total_changes(), changes);
}
