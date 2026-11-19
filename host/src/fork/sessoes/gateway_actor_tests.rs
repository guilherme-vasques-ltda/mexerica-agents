//! D70: ciclo de preparação/ativação pelo ator e ACP sintética, com barreiras sem sleep.
#![allow(clippy::unwrap_used)]
use crate::{
    LockExt,
    api::{self, devices::Caller},
    fork::conectores::oauth_runtime::RuntimeIntent,
    hub::{BotStatus, Hub},
    remote::identity::Identity,
    store::Store,
};
use anyhow::{Context, Result, anyhow};
use axum::{Json, Router, http::StatusCode, routing};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

struct Prepared {
    context: String,
    intent: RuntimeIntent,
    catalog: Value,
    ponte: String,
    resume: oneshot::Sender<()>,
}

type Hooks = HashMap<usize, mpsc::UnboundedSender<Prepared>>;
static HOOKS: LazyLock<Mutex<Hooks>> = LazyLock::new(Mutex::default);
type Issued = HashMap<(usize, String), Value>;
static ISSUED: LazyLock<Mutex<Issued>> = LazyLock::new(Mutex::default);

/// Somente o teste registrado observa o ponto real entre `mcp_servers` e session/new/load/reuso.
pub(crate) async fn prepared(hub: &Arc<Hub>, servers: &Value) -> Result<()> {
    let hook = HOOKS.locked().get(&(Arc::as_ptr(hub) as usize)).cloned();
    let Some(hook) = hook else { return Ok(()) };
    let connector = servers
        .as_array()
        .context("servidores MCP ausentes")?
        .iter()
        .find(|s| s["name"] == "connector")
        .context("conector fixture ausente")?;
    let args = connector["args"].as_array().context("argv ausente")?;
    let index = args.iter().position(|a| a == "--context").context("contexto ausente")?;
    let context = args.get(index + 1).and_then(Value::as_str).context("contexto inválido")?.to_owned();
    let intent = RuntimeIntent::capture(hub, &context, "connector", "initialize")?;
    intent.snapshot(hub)?;
    let binding = intent.context().and_then(|c| c.binding.as_ref()).context("binding ausente")?;
    ISSUED.locked().insert((Arc::as_ptr(hub) as usize, binding.clone()), servers.clone());
    let memory = servers
        .as_array()
        .context("servidores ausentes")?
        .iter()
        .find(|server| server["name"] == "memory")
        .context("memory ausente")?;
    let env = memory["env"].as_array().context("env ausente")?;
    let value = |name: &str| {
        env.iter()
            .find(|entry| entry["name"] == name)
            .map(|entry| entry["value"].clone())
            .context("env canônico ausente")
    };
    let catalog = json!({"botId":"bot","name":"__tools",
        "sessionSlot":value(crate::guard_hub::tool_origin::SESSION_SLOT_ENV)?,
        "sessionBinding":value(super::bindings::ENV)?,
        "arguments":{(crate::fork::memory_auto::transport::ARG):value(crate::fork::memory_auto::transport::ENV)?}});
    let (resume, wait) = oneshot::channel();
    let ponte = value(crate::fork::ponte_credencial::ENV)?.as_str().context("credencial ausente")?.to_owned();
    hook.send(Prepared { context, intent, catalog, ponte, resume }).map_err(|_| anyhow!("teste encerrado"))?;
    wait.await.context("barreira de preparação encerrada")
}

/// No reuso, observa somente as definições que o produtor real já entregou à ACP.
pub(crate) async fn reused(hub: &Arc<Hub>, binding: &super::bindings::BindingRef) -> Result<()> {
    let key = Arc::as_ptr(hub) as usize;
    if !HOOKS.locked().contains_key(&key) {
        return Ok(());
    }
    let servers = ISSUED
        .locked()
        .get(&(key, binding.id.as_str().to_owned()))
        .cloned()
        .context("definição original ausente no reuso")?;
    prepared(hub, &servers).await
}

struct Prompt {
    session: String,
    resume: oneshot::Sender<()>,
}

struct Fixture {
    hub: Arc<Hub>,
    prepared: mpsc::UnboundedReceiver<Prepared>,
    prompts: mpsc::UnboundedReceiver<Prompt>,
    phases: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
    dir: std::path::PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let key = Arc::as_ptr(&self.hub) as usize;
        HOOKS.locked().remove(&key);
        ISSUED.locked().retain(|(hub, _), _| *hub != key);
        self.task.abort();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Fixture {
    async fn new() -> Self {
        std::fs::create_dir_all(crate::service::data_dir()).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let dir = std::env::temp_dir().join(format!("sessoes-actor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/oauth_context_agent.py");
        let command = format!(
            "{} {} --gateway-lifecycle",
            crate::shell::python(&script),
            crate::shell::quote(&format!("http://{address}/session-phase"))
        );
        let cfg = serde_json::from_value(json!({"id":"bot","name":"Fixture","backend":"fixture",
            "command":command,"cwd":dir,"connectors":["connector"],"notify":false}))
        .unwrap();
        store.save_bot(&cfg).unwrap();
        *store.secret_key.locked() = Some(zeroize::Zeroizing::new(vec![42; 32]));
        let connector = serde_json::from_value(json!({"id":"connector","name":"Fixture",
            "url":"https://fixture.invalid/mcp","headers":{"Authorization":"Bearer synthetic"}}))
        .unwrap();
        crate::market::save_connectors(&store, &[connector]).unwrap();
        let hub = Hub::new(
            store,
            "fixture".into(),
            Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            address.port(),
        );
        let (tx, prepared) = mpsc::unbounded_channel();
        HOOKS.locked().insert(Arc::as_ptr(&hub) as usize, tx);
        let (prompt_tx, prompts) = mpsc::unbounded_channel();
        let phases = Arc::new(Mutex::new(Vec::new()));
        let observed = phases.clone();
        let phase = routing::post(move |Json(body): Json<Value>| {
            let phases = observed.clone();
            let tx = prompt_tx.clone();
            async move {
                let method = body["method"].as_str().unwrap().to_owned();
                phases.locked().push(method.clone());
                if method == "session/prompt" {
                    let (resume, wait) = oneshot::channel();
                    assert!(tx.send(Prompt { session: body["sessionId"].as_str().unwrap().into(), resume }).is_ok());
                    wait.await.unwrap();
                }
                StatusCode::OK
            }
        });
        let router = Router::new().route("/session-phase", phase).merge(api::router(hub.clone()));
        let task = tokio::spawn(async move {
            axum::serve(listener, router.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
        });
        hub.start().unwrap();
        Self { hub, prepared, prompts, phases, task, dir }
    }
}

#[tokio::test]
async fn ator_consume_preparacao_em_new_load_e_reuso_e_aceita_nova_intencao_ativa() {
    let mut f = Fixture::new().await;
    let mut events = f.hub.events.subscribe();
    let mut anterior: Option<String> = None;
    for stage in ["new", "reuse", "load"] {
        if stage == "load" {
            f.hub.send_cmd("bot", crate::agent::bot::Cmd::RefreshTools).unwrap();
        }
        api::dispatch(&f.hub, &Caller::Local, "send", json!({"botId":"bot","text":stage})).await.unwrap();
        let pending = tokio::time::timeout(Duration::from_secs(20), f.prepared.recv()).await.unwrap().unwrap();
        let caller = f.hub.pontes.autenticar(&f.hub, &pending.ponte).expect("ponte nova válida");
        if let Some(old) = &anterior {
            if stage == "reuse" {
                assert!(crate::remote::crypto::ct_eq(old.as_bytes(), pending.ponte.as_bytes()));
            } else {
                assert!(!crate::remote::crypto::ct_eq(old.as_bytes(), pending.ponte.as_bytes()));
                assert!(f.hub.pontes.autenticar(&f.hub, old).is_none());
            }
        }
        anterior = Some(pending.ponte.clone());
        pending.intent.snapshot(&f.hub).unwrap();
        let catalog = api::dispatch(&f.hub, &caller, "memoryCall", pending.catalog.clone()).await.unwrap();
        let has_mutation = catalog["tools"].as_array().unwrap().iter().any(|tool| tool["name"] == "remember");
        assert_eq!(has_mutation, stage != "reuse", "{stage}: preparação não substitui transporte vinculado");
        assert!(pending.intent.require_execution(&f.hub).is_err(), "preparação não fabrica ExecutionRef");
        let bootstrap = api::dispatch(
            &f.hub,
            &caller,
            "donnaGuardRuntime",
            json!({
                "context":pending.context,"connector":"connector","transport":"http",
                "bridgeId":uuid::Uuid::new_v4().to_string(),"method":"initialize"
            }),
        )
        .await
        .unwrap();
        assert!(bootstrap["url"].is_string(), "{stage}: bootstrap canônico ainda é válido");
        pending.resume.send(()).unwrap();
        let prompt = tokio::time::timeout(Duration::from_secs(20), f.prompts.recv()).await.unwrap().unwrap();
        assert_eq!(f.hub.runtime("bot").session.as_deref(), Some(prompt.session.as_str()));
        assert!(
            pending.intent.validate_db(&f.hub.store.db.locked()).is_err(),
            "{stage}: ativação remove admissão no commit"
        );
        assert!(pending.intent.snapshot(&f.hub).is_err(), "{stage}: captura anterior permanece recusada");
        let active = RuntimeIntent::capture(&f.hub, &pending.context, "connector", "initialize").unwrap();
        active.require_execution(&f.hub).unwrap();
        active.snapshot(&f.hub).unwrap();
        let catalog = api::dispatch(&f.hub, &caller, "memoryCall", pending.catalog.clone()).await.unwrap();
        assert!(catalog["tools"].as_array().unwrap().iter().any(|tool| tool["name"] == "remember"));
        let answer = api::dispatch(
            &f.hub,
            &caller,
            "donnaGuardRuntime",
            json!({
                "context":pending.context,"connector":"connector","transport":"http",
                "bridgeId":uuid::Uuid::new_v4().to_string(),"method":"initialize"
            }),
        )
        .await
        .unwrap();
        assert!(answer["url"].is_string(), "{stage}: nova intenção ativa legítima continua funcionando");
        prompt.resume.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let status = f.hub.runtime("bot").status;
                assert!(!matches!(status, BotStatus::Error));
                if status == BotStatus::Idle {
                    break;
                }
                events.recv().await.unwrap();
            }
        })
        .await
        .unwrap();
    }
    assert_eq!(
        *f.phases.locked(),
        vec!["session/new", "session/prompt", "session/prompt", "session/load", "session/prompt"]
    );
    f.hub.shutdown().await;
}
