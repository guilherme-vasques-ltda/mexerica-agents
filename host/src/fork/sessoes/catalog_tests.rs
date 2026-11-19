//! D70 do catálogo no consumidor real: preparação e transporte não se promovem a execução.
use super::{bindings, call_tests, gateway};
use crate::{
    LockExt,
    api::{self, devices::Caller},
    fork::memory_auto::{queue, transport},
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, Lane, Store},
};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::sync::{Arc, atomic::Ordering};

struct Fixture {
    hub: Arc<Hub>,
    cfg: BotConfig,
    binding: bindings::BindingRef,
    body: Value,
    dir: std::path::PathBuf,
}

impl Fixture {
    fn new(backend: &str) -> Result<Self> {
        let dir = std::env::temp_dir().join(format!("memory-catalog-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir)?;
        let store = Store::open(std::path::Path::new(":memory:"))?;
        let cfg: BotConfig = serde_json::from_value(json!({"id":"bot","name":"Catálogo","backend":backend}))?;
        store.save_bot(&cfg)?;
        let hub = Hub::new(store, "fixture".into(), Identity::load_or_create(&dir)?, "fixture".into(), 0);
        hub.start_bot_turn("bot", |r| r.lane = Some(Lane::main("bot")))?;
        transport::new_generation(&hub, &cfg, "bot", backend)?;
        let binding = bindings::issue(&hub.store, "bot", "bot", "bot", None)?;
        hub.prepare_session_binding(&binding)?;
        let capability = transport::issue(&hub, &cfg, "bot", None, None)?;
        let body = json!({"botId":"bot","sessionSlot":"main","sessionBinding":binding.id.as_str(),
            "name":"__tools","arguments":{(transport::ARG):capability}});
        Ok(Self { hub, cfg, binding, body, dir })
    }

    async fn call(&self, body: Value) -> Result<Value> {
        api::dispatch(&self.hub, &Caller::Local, "memoryCall", body).await
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn mutating(catalog: &Value) -> Result<bool> {
    Ok(catalog["tools"].as_array().context("catálogo ausente")?.iter().any(|tool| {
        matches!(tool["name"].as_str(), Some("remember" | "revise_memory" | "archive_memory" | "edit_memory"))
    }))
}

#[tokio::test]
async fn catalogo_exato_preserva_transporte_filtros_e_consumo_da_mesma_preparacao() {
    for backend in ["claude", "codex"] {
        let f = Fixture::new(backend).unwrap();
        let captured = gateway::capture(&f.hub.store.db.locked(), f.binding.id.as_str(), "bot", "tools/list").unwrap();
        let catalog = f.call(f.body.clone()).await.unwrap();
        assert_eq!(mutating(&catalog).unwrap(), backend != "codex");
        let mut absent = f.body.clone();
        absent["arguments"] = json!({});
        assert!(!mutating(&f.call(absent).await.unwrap()).unwrap(), "preparação não substitui transporte");
        for name in ["__tools ", "__Tools", "tools/list", "search_memory", "search_history", "remember", "edit_memory"]
        {
            let mut body = f.body.clone();
            body["name"] = name.into();
            assert!(f.call(body).await.unwrap_err().to_string().contains("chamada vencida"), "{name}");
        }
        let servers = json!([{"name":"memory","env":[{"name":transport::ENV,
            "value":f.body["arguments"][transport::ARG]}]}]);
        transport::bind(&f.hub, &f.cfg, "bot", None, &servers, "acp-catalog").unwrap();
        bindings::activate(&f.hub.store, &f.binding, "acp-catalog").unwrap();
        assert!(captured.check(&f.hub.store.db.locked()).is_err(), "o mesmo commit consome a captura original");
        f.hub.set_runtime("bot", |r| r.session = Some("acp-catalog".into()));
        assert!(captured.check_runtime(&f.hub.runtime("bot")).is_err());
        assert_eq!(mutating(&f.call(f.body.clone()).await.unwrap()).unwrap(), backend != "codex");
        f.hub.set_runtime("bot", |r| r.session = Some("outra-acp".into()));
        assert!(f.call(f.body.clone()).await.is_err(), "ACP presente nunca tenta fallback de preparação");
    }
}

#[tokio::test]
async fn catalogo_nao_adota_preparacao_transporte_ou_turno_posteriores_ao_check() {
    for scenario in ["swap", "revoke", "consume", "generation"] {
        let f = Fixture::new("claude").unwrap();
        let hub = f.hub.clone();
        let cfg = f.cfg.clone();
        let binding = f.binding.clone();
        let reached = call_tests::interleave(&f.hub.store, "memory_catalog", move || match scenario {
            "swap" => {
                let extra = super::store::create(
                    &hub.store.db.locked(),
                    &super::store::NewSession {
                        chat: "bot".into(),
                        origin: super::origin::SessionOrigin::Owner,
                        source_session: None,
                        parent_session: None,
                        title: "B".into(),
                        runs_on: None,
                    },
                )
                .unwrap();
                let b = bindings::issue(&hub.store, "bot", "bot", &extra.id, None).unwrap();
                let lane = Lane { chat: "bot".into(), session: Some(extra.id) };
                hub.start_bot_turn("bot", |r| {
                    r.lane = Some(lane.clone());
                    r.session = None;
                })
                .unwrap();
                queue::capture_engine(&hub, &cfg, &lane, "claude").unwrap();
                hub.prepare_session_binding(&b).unwrap();
            }
            "revoke" => bindings::revoke(&hub.store, "bot", None).unwrap(),
            "consume" => bindings::activate(&hub.store, &binding, "acp-catalog").unwrap(),
            "generation" => transport::new_generation(&hub, &cfg, "bot", "codex").unwrap(),
            _ => unreachable!(),
        });
        assert!(f.call(f.body.clone()).await.is_err(), "{scenario}");
        assert!(reached.load(Ordering::SeqCst), "{scenario}");
        assert_eq!(f.hub.store.guard_test_snapshot().unwrap(), (vec![], 0));
        let proposals: i64 = f
            .hub
            .store
            .db
            .locked()
            .query_row("SELECT COUNT(*) FROM donna_memory_auto_proposals", [], |r| r.get(0))
            .unwrap();
        assert_eq!(proposals, 0, "catálogo recusado não prepara efeitos em B");
    }
}
