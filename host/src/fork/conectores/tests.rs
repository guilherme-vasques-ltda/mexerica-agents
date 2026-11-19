//! Fronteiras PR04. Tudo sintético; não toca keyring, contas ou provedores.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use crate::hub::BotStatus;
use crate::{
    api::devices::Caller,
    fork::conectores::operacao::{Campo, Estado, Evidencia, Regra},
    hub::Hub,
    remote::identity::Identity,
    store::{EntryKind, Lane, Scope, Store},
};
use axum::{Json, Router, http::StatusCode, routing};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

// Metadados sintéticos no formato do contrato canônico, sem Binding/produtor real.
const FIXTURE_GENERATION: &str = "d7260f34-9e92-4c10-8101-5b2438cfc009";
const REPLACEMENT_GENERATION: &str = "32d17e4f-4e6d-42a3-a1b8-694ab5a6f421";
// Conta de saída sintética: número reservado de exemplo, sem pareamento/serviço real.
const FIXTURE_IDENTITY: &str = "+55 11 90000-0001";
fn conta(id: &str, encarnacao: &str, identidade: &str) -> ContaSaida {
    ContaSaida {
        vinculo: operacao::VinculoConta { conta: id.into(), encarnacao: encarnacao.into() },
        rotulo: "WhatsApp da Donna (exemplo)".into(),
        identidade: identidade.into(),
    }
}

struct Server {
    url: String,
    calls: Arc<AtomicUsize>,
    requests: Arc<AtomicUsize>,
    bodies: Arc<Mutex<Vec<Value>>>,
    mode: Arc<Mutex<&'static str>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn rule() -> Regra {
    let fields: BTreeMap<String, Campo> = BTreeMap::from([
        (
            "team".into(),
            Campo {
                max_bytes: 64,
                editable: false,
                allowed: Some(vec!["fixture-team".into()]),
                rotulo: "Equipe".into(),
            },
        ),
        ("title".into(), Campo { max_bytes: 2048, editable: true, allowed: None, rotulo: "Título".into() }),
    ]);
    let properties: BTreeMap<_, _> = fields
        .iter()
        .map(|(name, f)| {
            let mut schema = json!({"type":"string","maxLength":f.max_bytes});
            if let Some(values) = &f.allowed {
                schema["enum"] = json!(values);
            }
            (name.clone(), schema)
        })
        .collect();
    let schema = json!({"type":"object","properties":properties,"required":fields.keys().collect::<Vec<_>>(),"additionalProperties":false});
    Regra {
        schema_digest: operacao::digest(&schema),
        schema,
        fields,
        presentation: "Criar registro sintético".into(),
        evidence: Evidencia::Fixture,
    }
}
async fn server() -> Server {
    let requests = Arc::new(AtomicUsize::new(0));
    let http_requests = requests.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let mode = Arc::new(Mutex::new("success"));
    let (count, received, setting) = (calls.clone(), bodies.clone(), mode.clone());
    let router=Router::new().route("/mcp",routing::post(move |Json(frame):Json<Value>| {
        let (count,received,setting,requests)=(count.clone(),received.clone(),setting.clone(),http_requests.clone());
        async move {
            requests.fetch_add(1, Ordering::SeqCst);
            let mode=*setting.locked();
            let response=match frame["method"].as_str().unwrap_or_default() {
                "initialize" => json!({"protocolVersion":if mode=="bad-protocol" {"1999-01-01"} else {"2024-11-05"},"capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}),
                "tools/list" => json!({"tools":[{"name":"fixture_create","description":"Fixture","inputSchema":rule().schema}]}),
                "tools/call" => {
                    count.fetch_add(1,Ordering::SeqCst); received.locked().push(frame.clone());
                    if mode=="503" { return (StatusCode::SERVICE_UNAVAILABLE,Json(json!({"retained":"not a conclusion"}))); }
                    if mode=="hold" { std::future::pending::<()>().await; }
                    if mode=="generic" { json!({"content":[{"type":"text","text":"ok"}]}) }
                    else { json!({"structuredContent":{"fixtureEffect":"created"},"content":[{"type":"text","text":"Registro sintético criado"}]}) }
                }
                _=>json!({}),
            };
            let mut response=json!({"jsonrpc":"2.0","id":frame["id"],"result":response});
            if mode=="wrong-version" { response["jsonrpc"]=json!("1.0"); }
            if mode=="wrong-id" { response["id"]=json!("forged-id"); }
            if mode=="both" { response["error"]=json!({"code":-32603,"message":"synthetic"}); }
            (StatusCode::OK,Json(response))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Server { url, calls, requests, bodies, mode, task }
}
struct Fixture {
    hub: Arc<Hub>,
    handle: String,
    dir: std::path::PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
impl Fixture {
    fn new(server: &Server) -> Self {
        let dir = std::env::temp_dir().join(format!("donna-conectores04-{}", operacao::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("ledger.db")).unwrap();
        *store.secret_key.locked() = Some(zeroize::Zeroizing::new(vec![42; 32]));
        let bot =
            serde_json::from_value(json!({"id":"bot","name":"Fixture","backend":"fixture-mcp-300","connectors":[]}))
                .unwrap();
        store.save_bot(&bot).unwrap();
        let entry = store
            .insert_entry(&Lane::main("bot"), EntryKind::User, 1, &json!({"text":"Pedido sintético","status":"sent"}))
            .unwrap();
        let binding = crate::fork::sessoes::bindings::issue(&store, "bot", "bot", "bot", None).unwrap();
        store.memory_session_start("bot", "acp-fixture", false).unwrap();
        crate::fork::sessoes::bindings::activate(&store, &binding, "acp-fixture").unwrap();
        let hub =
            Hub::new(store, "fixture-hub".into(), Identity::load_or_create(&dir).unwrap(), "synthetic-local".into(), 0);
        hub.start_bot_turn("bot", |r| {
            r.lane = Some(Lane::main("bot"));
            r.session = Some("acp-fixture".into());
            r.message = Some(entry.id.clone());
        })
        .unwrap();
        let source = Source {
            access: SnapshotAcesso {
                instance: "instance-fixture".into(),
                generation: FIXTURE_GENERATION.into(),
                profile: "fixture-write".into(),
                config_revision: 0,
                curation_revision: 1,
                access_revision: 1,
                bot: "bot".into(),
                rules: BTreeMap::from([("fixture_create".into(), rule())]),
                conta: Some(conta("conta-a", "pareamento-a", FIXTURE_IDENTITY).vinculo),
            },
            conta: Some(conta("conta-a", "pareamento-a", FIXTURE_IDENTITY)),
            presentation: Presentation::V1,
            target: server.url.clone(),
            headers: reqwest::header::HeaderMap::new(),
            motor: Motor::Fixture300,
        };
        hub.conectores.sources.locked().insert(source.access.instance.clone(), source);
        let handle = register(&hub, "bot", "instance-fixture", &Lane::main("bot")).unwrap();
        hub.conectores.channel(&handle).unwrap().bind(&hub, "acp-fixture").unwrap();
        Self { hub, handle, dir }
    }
    async fn call(&self, caller: &Caller, method: &str, body: Value) -> Result<Value> {
        rpc(&self.hub, caller, method, body).await
    }
    async fn initialize(&self) {
        self.call(&Caller::Local,"donnaConectorFrame",json!({"context":self.handle,"sequence":1,
            "frame":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"fixture-mcp-300","version":"1"}}}})).await.unwrap();
        self.call(
            &Caller::Local,
            "donnaConectorFrame",
            json!({"context":self.handle,"sequence":2,
            "frame":{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}}),
        )
        .await
        .unwrap();
    }
    fn start(&self, sequence: u64, frame: Value) -> tokio::task::JoinHandle<Result<Value>> {
        let (hub, handle) = (self.hub.clone(), self.handle.clone());
        tokio::spawn(async move {
            // Este é o transporte do motor sintético realmente admitido na fixture.
            // Não atribui o contrato ao Codex ou a outro motor real.
            tokio::time::timeout(
                Duration::from_secs(300),
                rpc(
                    &hub,
                    &Caller::Local,
                    "donnaConectorFrame",
                    json!({"context":handle,"sequence":sequence,"frame":frame}),
                ),
            )
            .await
            .map_err(|_| anyhow!("Timeout do motor sintético"))?
        })
    }
    async fn pending(&self) -> operacao::Tentativa {
        for _ in 0..1000 {
            let id: Option<String> = self
                .hub
                .store
                .db
                .locked()
                .query_row("SELECT id FROM donna_conector_tentativas ORDER BY rowid DESC LIMIT 1", [], |r| r.get(0))
                .optional()
                .unwrap();
            if let Some(id) = id {
                return self.hub.store.conector_attempt(&id).unwrap().unwrap();
            }
            tokio::task::yield_now().await;
        }
        panic!("Nenhuma tentativa preparada")
    }
    fn count(&self) -> i64 {
        self.hub
            .store
            .db
            .locked()
            .query_row("SELECT COUNT(*) FROM donna_conector_tentativas", [], |r| r.get(0))
            .unwrap()
    }
}
impl Fixture {
    fn turn(&self, turn: i64, sid: &str) -> String {
        let entry = self
            .hub
            .store
            .insert_entry(
                &Lane::main("bot"),
                EntryKind::User,
                turn,
                &json!({"text":"Pedido sintético novo","status":"queued"}),
            )
            .unwrap();
        self.hub
            .prepare_bot_turn("bot", None, |r| {
                r.lane = Some(Lane::main("bot"));
                r.message = Some(entry.id.clone());
                r.session = Some(sid.into());
            })
            .unwrap();
        let runtime = self.hub.runtime("bot");
        assert_eq!(runtime.status, BotStatus::Waiting);
        assert!(runtime.memory_turn_token.is_none());
        assert!(runtime.preparing_turn_token.is_some());
        entry.id
    }
    async fn sequence(&self, sequence: u64) -> operacao::Tentativa {
        let ingress = format!("{}:{sequence}", self.handle);
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let found = crate::store::conectores::ingress(&self.hub.store.db.locked(), &ingress).unwrap();
                if let Some(op) = found {
                    return op;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Ingresso da fixture deve chegar ao ledger")
    }
    fn actor_config(&self) -> crate::store::BotConfig {
        let mut cfg = self.hub.store.bot("bot").unwrap().unwrap().config;
        cfg.cwd = self.dir.to_string_lossy().into_owned();
        self.hub.store.save_bot(&cfg).unwrap();
        cfg
    }
    fn actor_command() -> String {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/conectores_acp.py");
        format!("python3 -u '{}'", script.display())
    }
}

use rusqlite::OptionalExtension;
async fn rpc(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    crate::api::devices::permit(caller, method)?;
    match crate::fork::api::call(hub, caller, method, body).await? {
        std::ops::ControlFlow::Break(value) => Ok(value),
        std::ops::ControlFlow::Continue(_) => bail!("RPC não interceptado"),
    }
}
fn frame(id: i64, title: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"fixture_create","arguments":{"team":"fixture-team","title":title}}})
}
fn decision(op: &operacao::Tentativa) -> Value {
    json!({"protocol":2,"tentativaId":op.id,"snapshotId":op.snapshot_id,"cardId":op.card_id,"cardRev":op.rev,
        "requestDigest":op.request_digest,"decisao":"aprovar_uma_vez","source":"app"})
}

#[tokio::test]
async fn pipeline_real_aguarda_local_envia_exato_e_replay_nao_repete() {
    let server = server().await;
    let f = Fixture::new(&server);
    f.initialize().await;
    let work = f.start(3, frame(3, "conteudo-privado-fixture"));
    let op = f.pending().await;
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
    let public = f.hub.store.entry(&op.card_id).unwrap();
    assert!(public.data.get("options").is_none());
    for secret in ["conteudo-privado-fixture", "fixture-team", "fixture_create", "instance-fixture", FIXTURE_IDENTITY] {
        assert!(!serde_json::to_string(&public).unwrap().contains(secret));
    }
    assert_eq!(public.data["conectorOperation"]["decisionProtocol"], 2);
    let private = f.call(&Caller::Local, "donnaConectorTentativa", json!({"tentativaId":op.id})).await.unwrap();
    assert!(private.to_string().contains("conteudo-privado-fixture"));
    // Conta A do snapshot A e rótulos curados no privado; nome técnico/args intactos no protocolo.
    let presentation = &private["presentation"];
    assert_eq!(presentation["conta"], json!({"rotulo":"WhatsApp da Donna (exemplo)","identidade":FIXTURE_IDENTITY}));
    assert_eq!(presentation["efeito"], "Criar registro sintético");
    assert_eq!(presentation["rotulos"], json!({"team":"Equipe","title":"Título"}));
    assert_eq!(presentation["ferramenta"], "fixture_create");
    assert_eq!(presentation["argumentos"], frame(3, "conteudo-privado-fixture")["params"]["arguments"]);
    // Ledger durável só guarda o vínculo opaco; o número fica no privado cifrado.
    let raw: String = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT data FROM donna_conector_tentativas WHERE id=?1", [&op.id], |r| r.get(0))
        .unwrap();
    assert!(!raw.contains(FIXTURE_IDENTITY) && !raw.contains("WhatsApp da Donna"));
    assert_eq!(op.source.conta.as_ref().unwrap().encarnacao, "pareamento-a");
    assert!(!f.hub.store.kv_read(&op.slot()).unwrap().unwrap().contains(FIXTURE_IDENTITY));
    let body = decision(&op);
    let (first, second) = tokio::join!(
        f.call(&Caller::Local, "donnaConectorDecidir", body.clone()),
        f.call(&Caller::Local, "donnaConectorDecidir", body.clone())
    );
    assert!(first.is_ok() && second.is_ok());
    work.await.unwrap().unwrap();
    assert_eq!(server.calls.load(Ordering::SeqCst), 1);
    {
        let bodies = server.bodies.locked();
        assert_eq!(bodies[0]["params"], frame(999, "conteudo-privado-fixture")["params"]);
    }
    let receipt = f.call(&Caller::Local, "donnaConectorDecidir", body).await.unwrap();
    assert_eq!(receipt["state"], "concluida");
    assert!(!receipt.to_string().contains(FIXTURE_IDENTITY));
    let retry=f.start(3,serde_json::from_str(r#"{"id":999,"method":"tools/call","jsonrpc":"2.0","params":{"arguments":{"title":"conteudo-privado-fixture","team":"fixture-team"},"name":"fixture_create"}}"#).unwrap());
    assert!(retry.await.unwrap().unwrap()["error"].is_object());
    assert!(f.start(3, frame(1000, "corpo conflitante")).await.unwrap().is_err());
    assert_eq!(f.count(), 1);
    assert_eq!(server.calls.load(Ordering::SeqCst), 1);
    let saved = f.hub.store.conector_attempt(&op.id).unwrap().unwrap();
    assert_eq!(saved.state, Estado::Concluida);
    assert_eq!(saved.source.generation, FIXTURE_GENERATION);
    assert_eq!(saved.source.config_revision, 0);
    let reopened = Store::open(&f.dir.join("ledger.db")).unwrap();
    let persisted = reopened.conector_attempt(&op.id).unwrap().unwrap();
    assert_eq!(persisted.source.generation, FIXTURE_GENERATION);
    assert_eq!(persisted.source.config_revision, 0);
    assert!(f.hub.store.kv_read(&saved.slot()).unwrap().is_none());
    let (events, _) = f.hub.store.guard_test_snapshot().unwrap();
    assert!(!events.join("").contains("conteudo-privado-fixture") && !events.join("").contains(FIXTURE_IDENTITY));
    assert!(crate::fork::sessoes::exposure::exposed(&f.hub.store.db.locked(), "bot", "bot"));
}

#[tokio::test]
async fn versao_incompativel_fecha_canal_curado_depois_do_recibo_sem_descoberta() {
    let server = server().await;
    *server.mode.locked() = "bad-protocol";
    let f = Fixture::new(&server);
    let reply = f.call(&Caller::Local,"donnaConectorFrame",json!({"context":f.handle,"sequence":1,
        "frame":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{}}}})).await.unwrap();
    assert_eq!(
        reply,
        json!({"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"Versão MCP não suportada nesta conexão"}})
    );
    let (events, _) = f.hub.store.guard_test_snapshot().unwrap();
    assert!(events.iter().any(|e| e.contains("\"initialize\"")));
    for (sequence, frame) in [
        (2, json!({"jsonrpc":"2.0","method":"notifications/initialized"})),
        (3, json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}})),
    ] {
        let next =
            f.call(&Caller::Local, "donnaConectorFrame", json!({"context":f.handle,"sequence":sequence,"frame":frame}));
        assert!(next.await.is_err());
    }
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn edicao_gera_outro_card_e_cas_recusa_antigo_sem_trocar_canal() {
    let server = server().await;
    let f = Fixture::new(&server);
    f.initialize().await;
    let work = f.start(3, frame(3, "antes"));
    let op = f.pending().await;
    let edit = |patch: Value| {
        json!({"protocol":2,"tentativaId":op.id,"snapshotId":op.snapshot_id,"cardId":op.card_id,"cardRev":op.rev,
        "requestDigest":op.request_digest,"source":"app","patch":patch})
    };
    assert!(f.call(&Caller::Local, "donnaConectorEditar", edit(json!({"team":"outra-equipe"}))).await.is_err());
    let edited = f.call(&Caller::Local, "donnaConectorEditar", edit(json!({"title":"depois"}))).await.unwrap();
    assert_ne!(edited["cardId"], op.card_id);
    assert_ne!(edited["snapshotId"], op.snapshot_id);
    assert_eq!(edited["cardDeadline"], op.card_deadline);
    assert!(f.call(&Caller::Local, "donnaConectorDecidir", decision(&op)).await.is_err());
    f.hub.store.db.locked().execute("DELETE FROM entries WHERE id=?1", [&op.card_id]).unwrap();
    assert!(api::intercept(&f.hub, &json!({"entryId":op.card_id})).is_err());
    let current = f.hub.store.conector_attempt(&op.id).unwrap().unwrap();
    f.call(&Caller::Local, "donnaConectorDecidir", decision(&current)).await.unwrap();
    work.await.unwrap().unwrap();
    assert_eq!(server.bodies.locked()[0]["params"]["arguments"]["title"], "depois");
    assert_eq!(server.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn device_voz_body_inventado_e_prod_gate0_nao_concedem() {
    let server = server().await;
    let f = Fixture::new(&server);
    f.initialize().await;
    let work = f.start(3, frame(3, "privado"));
    let op = f.pending().await;
    let device = Caller::Device {
        key: "synthetic-device".into(),
        scopes: vec![Scope::Control, Scope::OwnerDevices],
        autoridade: None,
    };
    for (method, body) in
        [("donnaConectorTentativa", json!({"tentativaId":op.id})), ("donnaConectorDecidir", decision(&op))]
    {
        assert!(f.call(&device, method, body.clone()).await.is_err());
        assert!(api::call(&f.hub, &device, method, body).await.is_err());
    }
    let mut voice = decision(&op);
    voice["source"] = json!("voice");
    assert!(f.call(&Caller::Local, "donnaConectorDecidir", voice).await.is_err());
    let mut forged = decision(&op);
    forged["caller"] = json!("local");
    assert!(f.call(&Caller::Local, "donnaConectorDecidir", forged).await.is_err());
    f.call(&Caller::Local, "donnaConectorRelease", json!({"context":f.handle})).await.unwrap();
    assert!(work.await.unwrap().is_err());
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
    f.hub.conectores.sources.locked().clear();
    assert!(register(&f.hub, "bot", "instance-fixture", &Lane::main("bot")).is_err());
    let capability = f.call(&Caller::Local, "donnaConectorCapability", json!({})).await.unwrap();
    assert_eq!(capability["decisionProtocol"], 0);
    assert!(
        f.call(
            &Caller::Local,
            "donnaConectorFrame",
            json!({"context":f.handle,"sequence":4,"frame":frame(4,"x"),"protocol":1,"source":"app"})
        )
        .await
        .is_err()
    );
    assert_eq!(f.count(), 1);
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn fora_allowlist_schema_argumentos_ou_motor_desconhecido_zero_tentativa() {
    let server = server().await;
    let f = Fixture::new(&server);
    f.initialize().await;
    let mut denied = frame(3, "x");
    denied["params"]["name"] = json!("not-curated");
    assert!(f.start(3, denied).await.unwrap().is_err());
    let mut denied = frame(4, "x");
    denied["params"]["arguments"]["extra"] = json!(true);
    assert!(f.start(4, denied).await.unwrap().is_err());
    f.hub
        .conectores
        .sources
        .locked()
        .get_mut("instance-fixture")
        .unwrap()
        .access
        .rules
        .get_mut("fixture_create")
        .unwrap()
        .schema_digest = "forged".into();
    assert!(f.start(5, frame(5, "x")).await.unwrap().is_err());
    assert_eq!(f.count(), 0);
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
    let other = Fixture::new(&server);
    let mut bot = other.hub.store.bot("bot").unwrap().unwrap().config;
    bot.backend = "codex".into();
    other.hub.store.save_bot(&bot).unwrap();
    assert!(other.start(1, frame(1, "x")).await.unwrap().is_err());
    assert_eq!(other.count(), 0);
}

#[tokio::test]
async fn http503_e_resultado_generico_sao_incertos_sem_retry() {
    for mode in ["503", "generic", "wrong-version", "wrong-id", "both"] {
        let server = server().await;
        let f = Fixture::new(&server);
        f.initialize().await;
        *server.mode.locked() = mode;
        let work = f.start(3, frame(3, "x"));
        let op = f.pending().await;
        f.call(&Caller::Local, "donnaConectorDecidir", decision(&op)).await.unwrap();
        let _ = work.await.unwrap();
        assert_eq!(server.calls.load(Ordering::SeqCst), 1);
        assert_eq!(f.hub.store.conector_attempt(&op.id).unwrap().unwrap().state, Estado::Incerta);
    }
}

#[tokio::test]
async fn sucesso_salvo_sobrevive_falha_guard_e_restart_sem_privado() {
    let server = server().await;
    let f = Fixture::new(&server);
    f.initialize().await;
    let work = f.start(3, frame(3, "segredo-pendente"));
    let op = f.pending().await;
    f.hub.store.db.locked().execute_batch("CREATE TRIGGER reject_guard BEFORE INSERT ON donna_guard_events BEGIN SELECT RAISE(FAIL,'falha sintética'); END;").unwrap();
    f.call(&Caller::Local, "donnaConectorDecidir", decision(&op)).await.unwrap();
    assert!(work.await.unwrap().is_err());
    let saved = f.hub.store.conector_attempt(&op.id).unwrap().unwrap();
    assert_eq!(saved.state, Estado::Concluida);
    assert_eq!(saved.delivery, "retida");
    assert!(f.hub.store.kv_read(&saved.slot()).unwrap().is_none());
    let reopen = Store::open(&f.dir.join("ledger.db")).unwrap();
    reopen.conector_recover().unwrap();
    assert_eq!(reopen.conector_attempt(&op.id).unwrap().unwrap().state, Estado::Concluida);
    assert_eq!(server.calls.load(Ordering::SeqCst), 1);
}

// O instante de corte da autorização não depende do tick da roda de timers.
// Estes helpers só controlam/observam tempo e eventos; não expiram o ledger.
async fn advance_card_clock_to(instant: tokio::time::Instant) {
    let now = tokio::time::Instant::now();
    assert!(now <= instant, "O relógio passou do ponto de observação");
    tokio::time::advance(instant - now).await;
    assert_eq!(tokio::time::Instant::now(), instant);
}

async fn expired_projection_after_tick(
    events: &mut tokio::sync::broadcast::Receiver<Value>,
    card: &str,
    deadline: tokio::time::Instant,
) -> Value {
    // Dar um tick de 1 ms à projeção, sem acrescentá-lo à janela de aprovação.
    // O limite de observação faz a fixture falhar em vez de aguardar indefinidamente.
    let tick = Duration::from_millis(1);
    advance_card_clock_to(deadline + tick).await;
    let entry = tokio::time::timeout_at(deadline + tick + tick, async {
        loop {
            let event = events.recv().await.unwrap();
            if event["type"] == "entry"
                && event["entry"]["id"] == card
                && event["entry"]["data"]["status"] == "expirada"
            {
                return event["entry"].clone();
            }
        }
    })
    .await
    .expect("A projeção deve expirar no primeiro tick após o deadline");
    assert_eq!(tokio::time::Instant::now(), deadline + tick);
    entry
}

#[tokio::test]
async fn prazo90_nao_renova_na_edicao_e_expira_sem_clique() {
    let server = server().await;
    let f = Fixture::new(&server);
    f.initialize().await;
    let mut events = f.hub.events.subscribe();
    tokio::time::pause();
    let origin = tokio::time::Instant::now();
    let deadline = origin + Duration::from_secs(90);
    let work = f.start(3, frame(3, "antes"));
    let op = f.pending().await;
    advance_card_clock_to(origin + Duration::from_secs(89)).await;
    let channel = f.hub.conectores.channel(&f.handle).unwrap();
    assert!(channel.within_card(&op.id).is_ok());
    let edit = json!({"protocol":2,"tentativaId":op.id,"snapshotId":op.snapshot_id,"cardId":op.card_id,"cardRev":op.rev,
        "requestDigest":op.request_digest,"source":"app","patch":{"title":"depois"}});
    f.call(&Caller::Local, "donnaConectorEditar", edit).await.unwrap();
    let edited = f.hub.store.conector_attempt(&op.id).unwrap().unwrap();
    assert_eq!(edited.state, Estado::Aguardando);
    assert_eq!(edited.t0, op.t0);
    assert_eq!(edited.card_deadline, op.card_deadline);
    advance_card_clock_to(deadline).await;
    // Aprovação já é recusada em 90, mesmo se a projeção esperar o tick seguinte.
    assert!(channel.within_card(&op.id).is_err());
    assert!(f.call(&Caller::Local, "donnaConectorDecidir", decision(&edited)).await.is_err());
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
    let projection = expired_projection_after_tick(&mut events, &edited.card_id, deadline).await;
    let current = f.hub.store.conector_attempt(&op.id).unwrap().unwrap();
    assert_eq!(current.state, Estado::Expirada);
    assert_eq!(current.t0, op.t0);
    assert_eq!(current.card_deadline, op.card_deadline);
    assert_eq!(projection["data"]["status"], "expirada");
    assert!(projection["rev"].as_i64().unwrap() > op.rev);
    assert!(work.await.unwrap().is_err());
    advance_card_clock_to(origin + Duration::from_secs(91)).await;
    assert!(f.call(&Caller::Local, "donnaConectorDecidir", decision(&current)).await.is_err());
    assert_eq!(f.hub.store.conector_attempt(&op.id).unwrap().unwrap().state, Estado::Expirada);
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn revogacao_antes_admissao_fecha_e_depois_nao_apaga_prova() {
    let server = server().await;
    let f = Fixture::new(&server);
    f.initialize().await;
    let work = f.start(3, frame(3, "x"));
    let op = f.pending().await;
    f.hub.set_runtime("bot", |r| r.memory_turn_token = Some("outro-turno".into()));
    assert!(f.call(&Caller::Local, "donnaConectorDecidir", decision(&op)).await.is_err());
    f.call(&Caller::Local, "donnaConectorRelease", json!({"context":f.handle})).await.unwrap();
    let _ = work.await;
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
    let other = Fixture::new(&server);
    other.initialize().await;
    let work = other.start(3, frame(3, "y"));
    let op = other.pending().await;
    other.hub.conectores.sources.locked().get_mut("instance-fixture").unwrap().access.access_revision += 1;
    assert!(other.call(&Caller::Local, "donnaConectorDecidir", decision(&op)).await.is_err());
    work.abort();
    let _ = work.await;
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);

    for decided in [false, true] {
        let current = Fixture::new(&server);
        current.initialize().await;
        let work = current.start(3, frame(3, "geracao-original"));
        let original = current.pending().await;
        if decided {
            // Handler síncrono neste runtime: não ceder ao driver entre decisão e troca.
            current.call(&Caller::Local, "donnaConectorDecidir", decision(&original)).await.unwrap();
        }
        current.hub.conectores.sources.locked().get_mut("instance-fixture").unwrap().access.generation =
            REPLACEMENT_GENERATION.into();
        if !decided {
            assert!(current.call(&Caller::Local, "donnaConectorDecidir", decision(&original)).await.is_err());
            current.call(&Caller::Local, "donnaConectorRelease", json!({"context":current.handle})).await.unwrap();
        }
        assert!(work.await.unwrap().is_err());
        let saved = current.hub.store.conector_attempt(&original.id).unwrap().unwrap();
        assert_eq!(saved.source.generation, FIXTURE_GENERATION);
        assert_eq!(saved.source.config_revision, 0);
        assert_eq!(saved.state, Estado::Cancelada);
        assert!(saved.admitted_at.is_none());
        assert_eq!(server.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn cipher_aad_rollback_reopen_e_recuperacao_dos_estados() {
    let server = server().await;
    let f = Fixture::new(&server);
    f.initialize().await;
    f.hub.store.db.locked().execute_batch("CREATE TRIGGER reject_card BEFORE INSERT ON donna_conector_cartoes BEGIN SELECT RAISE(FAIL,'falha cartão'); END;").unwrap();
    assert!(f.start(3, frame(3, "segredo-aad")).await.unwrap().is_err());
    assert_eq!(f.count(), 0);
    assert_eq!(f.hub.store.kv_prefix("donna.conectores.tentativa."), Vec::<String>::new());
    f.hub.store.db.locked().execute_batch("DROP TRIGGER reject_card;").unwrap();
    let work = f.start(4, frame(4, "segredo-aad"));
    let op = f.pending().await;
    let raw = f.hub.store.kv_read(&op.slot()).unwrap().unwrap();
    assert!(raw.starts_with("vault:v1:"));
    assert!(!raw.contains("segredo-aad"));
    assert!(crate::market::vault::open(&f.hub.store, "wrong-aad", &raw).is_err());
    assert!(crate::market::vault::open(&f.hub.store, &op.slot(), "plaintext").is_err());
    // As constraints persistidas recusam ausência/tipo inválido sem converter
    // geração em número nem transformar revisão zero em outra revisão.
    for (field, value) in [
        ("generation", None),
        ("generation", Some(json!(1))),
        ("generation", Some(json!("not-a-uuid"))),
        ("configRevision", None),
        ("configRevision", Some(json!("0"))),
        ("configRevision", Some(json!(-1))),
        ("configRevision", Some(json!(0.5))),
    ] {
        let mut malformed = serde_json::to_value(&op).unwrap();
        let source = malformed["source"].as_object_mut().unwrap();
        if let Some(value) = value {
            source.insert(field.into(), value);
        } else {
            source.remove(field);
        }
        assert!(
            f.hub
                .store
                .db
                .locked()
                .execute(
                    "UPDATE donna_conector_tentativas SET data=?2 WHERE id=?1",
                    rusqlite::params![op.id, malformed.to_string()],
                )
                .is_err()
        );
    }
    // Reabertura de cada estado com Store real, sem reexecutar a Session.
    for (state, expected) in [
        (Estado::Aguardando, Estado::Expirada),
        (Estado::Autorizada, Estado::Cancelada),
        (Estado::EmEnvio, Estado::Incerta),
        (Estado::Concluida, Estado::Concluida),
    ] {
        f.hub
            .conector_publish(|| {
                crate::store::conectores::change(&f.hub.store.db.locked(), &op.id, |_, op| {
                    op.state = state;
                    Ok(None)
                })
            })
            .unwrap();
        let reopened = Store::open(&f.dir.join("ledger.db")).unwrap();
        reopened.conector_recover().unwrap();
        let persisted = reopened.conector_attempt(&op.id).unwrap().unwrap();
        assert_eq!(persisted.state, expected);
        let projection = reopened.entry(&persisted.card_id).unwrap();
        assert_eq!(persisted.rev, projection.rev);
        assert_eq!(persisted.receipt()["cardRev"], projection.rev);
        let stable_rev = persisted.rev;
        reopened.conector_recover().unwrap();
        assert_eq!(reopened.conector_attempt(&op.id).unwrap().unwrap().rev, stable_rev);
        assert_eq!(persisted.source.generation, FIXTURE_GENERATION);
        assert_eq!(persisted.source.config_revision, 0);
    }
    f.hub.store.db.locked().execute("DELETE FROM entries WHERE id=?1", [&op.card_id]).unwrap();
    crate::store::conectores::change(&f.hub.store.db.locked(), &op.id, |_, op| {
        op.state = Estado::Aguardando;
        Ok(None)
    })
    .unwrap();
    let reopened = Store::open(&f.dir.join("ledger.db")).unwrap();
    reopened.conector_recover().unwrap();
    let absent = reopened.conector_attempt(&op.id).unwrap().unwrap();
    assert_eq!(absent.state, Estado::Expirada);
    assert!(reopened.entry(&absent.card_id).is_none());
    assert!(reopened.kv_read(&absent.slot()).unwrap().is_none());
    work.abort();
    let _ = work.await;
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
}

struct LegacyServer {
    server: Server,
    outbound: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>>,
    reverses: Arc<AtomicUsize>,
    reverse_entered: Arc<tokio::sync::Notify>,
    ack: Arc<tokio::sync::Notify>,
}
async fn legacy_server(hold_ack: bool, hold_ping: bool) -> LegacyServer {
    use axum::{body::Body, response::Response};
    let calls = Arc::new(AtomicUsize::new(0));
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let outbound: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>> = Arc::new(Mutex::new(None));
    let ack = Arc::new(tokio::sync::Notify::new());
    let reverses = Arc::new(AtomicUsize::new(0));
    let reverse_entered = Arc::new(tokio::sync::Notify::new());
    let ping_entered = reverse_entered.clone();
    let stream_out = outbound.clone();
    let (send_out, count, received, allow_ack, pings) =
        (outbound.clone(), calls.clone(), bodies.clone(), ack.clone(), reverses.clone());
    let router=Router::new().route("/mcp",routing::post(|| async {StatusCode::METHOD_NOT_ALLOWED}).get(move || {
        let stream_out=stream_out.clone();async move {
            let (tx,rx)=tokio::sync::mpsc::unbounded_channel::<String>();
            *stream_out.locked()=Some(tx.clone());tx.send("event: endpoint\ndata: /messages\n\n".into()).unwrap();
            let stream=futures::stream::unfold(rx,|mut rx| async {
                rx.recv().await.map(|text| (Ok::<_,std::io::Error>(text),rx))
            });
            Response::builder().header("content-type","text/event-stream").body(Body::from_stream(stream)).unwrap()
        }
    })).route("/messages",routing::post(move |Json(frame):Json<Value>| {
        let (send_out,count,received,allow_ack,pings,ping_entered)=(send_out.clone(),count.clone(),received.clone(),allow_ack.clone(),pings.clone(),ping_entered.clone());
        async move {
            if frame.get("method").is_none() {
                pings.fetch_add(1,Ordering::SeqCst);
                ping_entered.notify_one();
                if hold_ping { std::future::pending::<()>().await; }
                return StatusCode::ACCEPTED;
            }
            let result=match frame["method"].as_str().unwrap_or_default() {
                "initialize"=>json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}),
                "tools/list"=>json!({"tools":[{"name":"fixture_create","inputSchema":rule().schema}]}),
                "tools/call"=>{
                    count.fetch_add(1,Ordering::SeqCst);received.locked().push(frame.clone());
                    json!({"structuredContent":{"fixtureEffect":"created"},"content":[{"type":"text","text":"Sintético"}]})
                }
                _=>json!({}),
            };
            send_out.locked().as_ref().unwrap().send(format!("data: {}\n\n",json!({"jsonrpc":"2.0","id":frame["id"],"result":result}))).unwrap();
            if hold_ack && frame["method"]=="tools/call" { allow_ack.notified().await; }
            StatusCode::ACCEPTED
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    LegacyServer {
        server: Server {
            url,
            calls,
            requests: Arc::new(AtomicUsize::new(0)),
            bodies,
            mode: Arc::new(Mutex::new("success")),
            task,
        },
        outbound,
        reverses,
        reverse_entered,
        ack,
    }
}
async fn spins_until(mut condition: impl FnMut() -> bool) {
    for _ in 0..10_000 {
        if condition() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("Condição da fronteira não observada")
}

#[tokio::test]
async fn legado_espera_humano_processa_ping_e_salva_get_antes_ack() {
    let server = legacy_server(true, false).await;
    let f = Fixture::new(&server.server);
    f.initialize().await;
    let work = f.start(3, frame(3, "segredo-no-get"));
    let op = f.pending().await;
    server
        .outbound
        .locked()
        .as_ref()
        .unwrap()
        .send(format!(
            "data: {}\n\n",
            json!({"jsonrpc":"2.0","id":"forged-id","result":{"structuredContent":{"fixtureEffect":"created"}}})
        ))
        .unwrap();
    server
        .outbound
        .locked()
        .as_ref()
        .unwrap()
        .send(format!("data: {}\n\n", json!({"jsonrpc":"2.0","id":"ping-fixture","method":"ping"})))
        .unwrap();
    spins_until(|| server.reverses.load(Ordering::SeqCst) == 1).await;
    assert_eq!(server.server.calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.hub.store.conector_attempt(&op.id).unwrap().unwrap().state, Estado::Aguardando);
    f.call(&Caller::Local, "donnaConectorDecidir", decision(&op)).await.unwrap();
    spins_until(|| f.hub.store.conector_attempt(&op.id).unwrap().unwrap().state == Estado::Concluida).await;
    assert!(!work.is_finished()); // POST ainda não devolveu o ACK.
    f.hub.set_runtime("bot", |r| r.memory_turn_token = Some("revogado-depois-do-efeito".into()));
    server.ack.notify_one();
    assert!(work.await.unwrap().is_err());
    let receipt = f.hub.store.conector_attempt(&op.id).unwrap().unwrap();
    assert_eq!(receipt.state, Estado::Concluida);
    assert_eq!(receipt.delivery, "retida");
    assert_eq!(server.server.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn timer_expira_projecao_mesmo_com_reverse_idle_em_await() {
    let server = legacy_server(false, true).await;
    let f = Fixture::new(&server.server);
    f.initialize().await;
    let mut events = f.hub.events.subscribe();
    tokio::time::pause();
    let origin = tokio::time::Instant::now();
    let deadline = origin + Duration::from_secs(90);
    let work = f.start(3, frame(3, "x"));
    let op = f.pending().await;
    advance_card_clock_to(origin + Duration::from_secs(88)).await;
    let channel = f.hub.conectores.channel(&f.handle).unwrap();
    // O handshake de rede corre com relógio retomado: o auto-advance do runtime
    // pausado não pode saltar ao deadline enquanto aguarda o socket loopback.
    tokio::time::resume();
    server
        .outbound
        .locked()
        .as_ref()
        .unwrap()
        .send(format!("data: {}\n\n", json!({"jsonrpc":"2.0","id":"ping-bloqueado","method":"ping"})))
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), server.reverse_entered.notified())
        .await
        .expect("O reverse deve entrar durante a preparação anterior a 89 segundos");
    tokio::time::pause();
    assert!(tokio::time::Instant::now() < origin + Duration::from_secs(89));
    advance_card_clock_to(origin + Duration::from_secs(89)).await;
    assert!(channel.within_card(&op.id).is_ok());
    assert_eq!(server.reverses.load(Ordering::SeqCst), 1);
    assert_eq!(f.hub.store.conector_attempt(&op.id).unwrap().unwrap().state, Estado::Aguardando);
    advance_card_clock_to(deadline).await;
    assert!(channel.within_card(&op.id).is_err());
    assert!(f.call(&Caller::Local, "donnaConectorDecidir", decision(&op)).await.is_err());
    let projection = expired_projection_after_tick(&mut events, &op.card_id, deadline).await;
    assert_eq!(projection["data"]["status"], "expirada");
    assert_eq!(f.hub.store.conector_attempt(&op.id).unwrap().unwrap().state, Estado::Expirada);
    assert!(!work.is_finished(), "O reverse ainda deve estar em await; só o timer independente projetou");
    assert_eq!(server.server.calls.load(Ordering::SeqCst), 0);
    advance_card_clock_to(origin + Duration::from_secs(91)).await;
    assert!(f.call(&Caller::Local, "donnaConectorDecidir", decision(&op)).await.is_err());
    assert_eq!(server.server.calls.load(Ordering::SeqCst), 0);
    work.abort();
    let _ = work.await;
}

#[tokio::test]
async fn cancelamento_e_eof_drop_antes_e_depois_admissao_sao_conservadores() {
    let server = server().await;
    *server.mode.locked() = "hold";
    let f = Fixture::new(&server);
    f.initialize().await;
    let work = f.start(3, frame(3, "x"));
    let op = f.pending().await;
    f.call(&Caller::Local, "donnaConectorDecidir", decision(&op)).await.unwrap();
    spins_until(|| server.calls.load(Ordering::SeqCst) == 1).await;
    f.call(&Caller::Local, "donnaConectorRelease", json!({"context":f.handle})).await.unwrap();
    assert!(work.await.unwrap().is_err());
    assert_eq!(f.hub.store.conector_attempt(&op.id).unwrap().unwrap().state, Estado::Incerta);
    let before = Fixture::new(&server);
    before.initialize().await;
    let work = before.start(3, frame(3, "y"));
    let op = before.pending().await;
    work.abort();
    let _ = work.await;
    assert_eq!(before.hub.store.conector_attempt(&op.id).unwrap().unwrap().state, Estado::Cancelada);
    assert_eq!(server.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn erro_db_no_mapa_nativo_nunca_cai_no_motor() {
    let server = server().await;
    let f = Fixture::new(&server);
    f.hub.store.db.locked().execute_batch("DROP TABLE donna_conector_cartoes;").unwrap();
    assert!(api::intercept(&f.hub, &json!({"entryId":"qualquer"})).is_err());
    assert!(crate::fork::permission_card::respond(&f.hub, &json!({"entryId":"qualquer"})).await.is_err());
}

#[tokio::test]
async fn ponte_real_eof_durante_http_em_envio_libera_hub_sem_retry() {
    use tokio::io::AsyncWriteExt;
    let server = server().await;
    *server.mode.locked() = "hold";
    let f = Fixture::new(&server);
    f.initialize().await;
    let hub = f.hub.clone();
    let router = Router::new().route(
        "/api/{method}",
        routing::post(move |axum::extract::Path(method): axum::extract::Path<String>, Json(body): Json<Value>| {
            let hub = hub.clone();
            async move {
                match rpc(&hub, &Caller::Local, &method, body).await {
                    Ok(value) => (StatusCode::OK, Json(value)),
                    Err(_) => (StatusCode::BAD_REQUEST, Json(json!({"error":"retido"}))),
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let api_task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let (mut motor, input) = tokio::io::duplex(64 * 1024);
    let handle = f.handle.clone();
    let bridge = tokio::spawn(async move {
        proxy::relay(input, tokio::io::sink(), port, handle, zeroize::Zeroizing::new("synthetic-local".into())).await
    });
    let mut bytes = serde_json::to_vec(&frame(3, "eof-privado")).unwrap();
    bytes.push(b'\n');
    motor.write_all(&bytes).await.unwrap();
    let op = f.pending().await;
    f.call(&Caller::Local, "donnaConectorDecidir", decision(&op)).await.unwrap();
    spins_until(|| server.calls.load(Ordering::SeqCst) == 1).await;
    motor.shutdown().await.unwrap();
    bridge.await.unwrap().unwrap();
    spins_until(|| f.hub.store.conector_attempt(&op.id).unwrap().unwrap().state == Estado::Incerta).await;
    assert_eq!(server.calls.load(Ordering::SeqCst), 1);
    api_task.abort();
    let _ = api_task.await;
}

#[tokio::test]
async fn fonte_ou_apresentacao_ausente_fecha_antes_cofre_e_qualquer_tentativa() {
    let server = server().await;
    let f = Fixture::new(&server);
    let source = f.hub.conectores.sources.locked().get("instance-fixture").unwrap().clone();
    *f.hub.store.secret_key.locked() = None;
    f.hub.conectores.sources.locked().clear();
    assert!(register(&f.hub, "bot", "instance-fixture", &Lane::main("bot")).is_err());
    let mut unavailable = source;
    unavailable.presentation = Presentation::Unavailable;
    f.hub.conectores.sources.locked().insert("instance-fixture".into(), unavailable);
    assert!(register(&f.hub, "bot", "instance-fixture", &Lane::main("bot")).is_err());
    assert!(f.start(1, frame(1, "privado")).await.unwrap().is_err());
    assert_eq!(f.count(), 0);
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
    assert!(f.hub.store.kv_read("vault.id").unwrap().is_none());
    assert_eq!(f.hub.store.kv_prefix("donna.conectores.tentativa."), Vec::<String>::new());
}

#[tokio::test]
async fn rotacao_legitima_e_outra_conta_nao_revogam_snapshot_logico() {
    let server = server().await;
    let f = Fixture::new(&server);
    f.initialize().await;
    let work = f.start(3, frame(3, "x"));
    let op = f.pending().await;
    {
        let mut sources = f.hub.conectores.sources.locked();
        let source = sources.get_mut("instance-fixture").unwrap();
        source.headers.insert("authorization", reqwest::header::HeaderValue::from_static("Bearer synthetic-rotated"));
        let mut other = source.clone();
        other.access.instance = "outra-instancia".into();
        sources.insert("outra-instancia".into(), other);
    }
    // Slot legado muda; não é geração/revisão da conta curada capturada.
    crate::market::vault::write(&f.hub.store, "connectors", "[]").unwrap();
    f.call(&Caller::Local, "donnaConectorDecidir", decision(&op)).await.unwrap();
    work.await.unwrap().unwrap();
    assert_eq!(server.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn digest_exclui_wire_id_ordena_objetos_e_nao_relaxa_tipos() {
    let source = SnapshotAcesso {
        instance: "fixture".into(),
        generation: FIXTURE_GENERATION.into(),
        profile: "fixture".into(),
        config_revision: 0,
        curation_revision: 1,
        access_revision: 1,
        bot: "bot".into(),
        rules: BTreeMap::from([("fixture_create".into(), rule())]),
        conta: Some(conta("conta-a", "pareamento-a", FIXTURE_IDENTITY).vinculo),
    };
    let account = conta("conta-a", "pareamento-a", FIXTURE_IDENTITY);
    let a = frame(1, "x");
    let b:Value=serde_json::from_str(r#"{"id":"wire-novo","jsonrpc":"2.0","method":"tools/call","params":{"arguments":{"title":"x","team":"fixture-team"},"name":"fixture_create"}}"#).unwrap();
    assert_eq!(
        source.request(&account, &a).unwrap().request_digest,
        source.request(&account, &b).unwrap().request_digest
    );
    // Conta B não monta pedido sobre a concessão da conta A.
    assert!(source.request(&conta("conta-b", "pareamento-b", "+55 11 90000-0002"), &a).is_err());
    let mut numeric = a;
    numeric["params"]["arguments"]["title"] = json!(1.0);
    assert!(source.request(&account, &numeric).is_err());
    assert_eq!(
        operacao::digest(&json!({"a":{"b":2,"c":1.0}})),
        operacao::digest(&serde_json::from_str::<Value>(r#"{"a":{"c":1e0,"b":2}}"#).unwrap())
    );
}

#[tokio::test]
async fn ator_dezessete_turnos_reusa_sessao_e_contextos_de_pedidos_sao_distintos() {
    if crate::agent::bot::conectores_fixture::isolated(concat!(
        module_path!(),
        "::ator_dezessete_turnos_reusa_sessao_e_contextos_de_pedidos_sao_distintos"
    ))
    .await
    .unwrap()
    {
        return;
    }
    let server = server().await;
    let mut f = Fixture::new(&server);
    f.hub.conectores.release(&f.hub, &f.handle).unwrap();
    let cfg = f.actor_config();
    let mut actor = crate::agent::bot::conectores_fixture::Fixture::new(f.hub.clone(), cfg, &Fixture::actor_command())
        .await
        .unwrap();
    let mut first_handle = None;
    let mut contexts = Vec::new();
    for turn in 1..=17 {
        let entry = f.turn(turn, "fixture-acp");
        actor.prompt(&entry).await.unwrap();
        let channels = lifecycle::handles(&f.hub);
        assert_eq!(channels.len(), 1);
        f.handle.clone_from(&channels[0].id);
        if let Some(first) = &first_handle {
            assert_eq!(&f.handle, first);
        } else {
            first_handle = Some(f.handle.clone());
            f.initialize().await;
        }
        if turn == 1 || turn == 17 {
            let seq = 10 + u64::try_from(turn).unwrap();
            let work = f.start(seq, frame(100 + turn, "turno novo"));
            let op = f.sequence(seq).await;
            assert_eq!(op.context.turn, turn);
            contexts.push(op.context.clone());
            f.call(&Caller::Local, "donnaConectorDecidir", decision(&op)).await.unwrap();
            work.await.unwrap().unwrap();
        }
        actor.finish().await.unwrap();
    }
    assert_ne!(contexts[0].run, contexts[1].run);
    assert_ne!(contexts[0].message, contexts[1].message);
    assert_eq!(server.calls.load(Ordering::SeqCst), 2);
    let requests = std::fs::read_to_string(f.dir.join("session-requests.jsonl")).unwrap();
    let requests: Vec<Value> = requests.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["method"], "session/new");
    let tools = requests[0]["params"]["mcpServers"].as_array().unwrap();
    assert_eq!(tools.iter().filter(|d| d["name"] == "donna-curado-instance-fixture").count(), 1);
    let old = f.handle.clone();
    actor.close().await;
    assert!(lifecycle::handles(&f.hub).is_empty());
    assert!(f.start(80, frame(80, "fechado")).await.unwrap().is_err());
    actor.ensure().await.unwrap();
    let channels = lifecycle::handles(&f.hub);
    assert_eq!(channels.len(), 1);
    assert_ne!(channels[0].id, old);
    actor.replace().await;
    assert!(lifecycle::handles(&f.hub).is_empty());
}

#[tokio::test]
async fn registro_prebinding_e_lote_parcial_fecham_sem_rede_ou_handles_orfaos() {
    let server = server().await;
    let mut f = Fixture::new(&server);
    f.hub.conectores.release(&f.hub, &f.handle).unwrap();
    f.handle = register(&f.hub, "bot", "instance-fixture", &Lane::main("bot")).unwrap();
    let before = server.requests.load(Ordering::SeqCst);
    for method in ["initialize", "tools/list"] {
        assert!(
            f.call(
                &Caller::Local,
                "donnaConectorFrame",
                json!({"context":f.handle,"sequence":1,
            "frame":{"jsonrpc":"2.0","id":1,"method":method,"params":{}}})
            )
            .await
            .is_err()
        );
    }
    assert_eq!(server.requests.load(Ordering::SeqCst), before);
    assert_eq!(f.count(), 0);
    assert!(f.hub.store.kv_read("vault.id").unwrap().is_none());
    assert_eq!(f.hub.store.kv_prefix("donna.conectores.tentativa."), Vec::<String>::new());
    f.hub.conectores.channel(&f.handle).unwrap().bind(&f.hub, "acp-fixture").unwrap();
    f.initialize().await;
    assert!(server.requests.load(Ordering::SeqCst) > before);
    f.hub.conectores.release(&f.hub, &f.handle).unwrap();
    {
        let mut sources = f.hub.conectores.sources.locked();
        let mut other = sources["instance-fixture"].clone();
        other.access.instance = "instance-second".into();
        sources.insert(other.access.instance.clone(), other);
    }
    let pool = Pool::default();
    let batch = definitions(&f.hub, "bot", &std::env::current_exe().unwrap(), &Lane::main("bot"), &pool).unwrap();
    assert_eq!(batch.servers.len(), 2);
    let retired = batch.servers[1]["args"][3].as_str().unwrap();
    f.hub.conectores.release(&f.hub, retired).unwrap();
    assert!(batch.bind(&f.hub, "acp-fixture").is_err());
    assert!(pool.failed());
    assert!(lifecycle::handles(&f.hub).is_empty());
}

#[tokio::test]
async fn ator_erro_new_modelo_manual_binding_load_e_close_faz_cleanup_do_lote() {
    if crate::agent::bot::conectores_fixture::isolated(concat!(
        module_path!(),
        "::ator_erro_new_modelo_manual_binding_load_e_close_faz_cleanup_do_lote"
    ))
    .await
    .unwrap()
    {
        return;
    }
    let server = server().await;
    for mode in ["new_error", "model_error", "manual_error", "binding_error", "load_error", "close_error"] {
        let f = Fixture::new(&server);
        f.hub.conectores.release(&f.hub, &f.handle).unwrap();
        let mut cfg = f.actor_config();
        if mode == "model_error" {
            cfg.model = Some("fixture-model".into());
            f.hub.store.save_bot(&cfg).unwrap();
        }
        if mode == "load_error" {
            f.hub.store.set_session("bot", Some("saved-fixture")).unwrap();
        }
        std::fs::write(f.dir.join("scenario"), mode).unwrap();
        if mode == "binding_error" {
            f.hub.store.db.locked().execute_batch("CREATE TRIGGER reject_binding BEFORE INSERT ON donna_session_acp BEGIN SELECT RAISE(FAIL,'fixture'); END;").unwrap();
        }
        let mut actor =
            crate::agent::bot::conectores_fixture::Fixture::new(f.hub.clone(), cfg, &Fixture::actor_command())
                .await
                .unwrap();
        let result = actor.ensure().await;
        if mode == "close_error" {
            result.unwrap();
            assert_eq!(lifecycle::handles(&f.hub).len(), 1);
            actor.close().await;
            assert!(lifecycle::handles(&f.hub).is_empty());
        } else if mode == "load_error" {
            result.unwrap();
            let lines = std::fs::read_to_string(f.dir.join("session-requests.jsonl")).unwrap();
            let calls: Vec<Value> = lines.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[0]["method"], "session/load");
            assert_eq!(calls[1]["method"], "session/new");
            let handle = |request: &Value| {
                request["params"]["mcpServers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|d| d["name"] == "donna-curado-instance-fixture")
                    .unwrap()["args"][3]
                    .clone()
            };
            assert_ne!(handle(&calls[0]), handle(&calls[1]));
            assert_eq!(lifecycle::handles(&f.hub).len(), 1);
        } else {
            assert!(result.is_err());
            assert!(lifecycle::handles(&f.hub).is_empty());
        }
        actor.replace().await;
        assert!(lifecycle::handles(&f.hub).is_empty());
        assert_eq!(f.count(), 0);
    }
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn pending_antigo_nao_adota_contexto_do_novo_turno_no_mesmo_canal() {
    if crate::agent::bot::conectores_fixture::isolated(concat!(
        module_path!(),
        "::pending_antigo_nao_adota_contexto_do_novo_turno_no_mesmo_canal"
    ))
    .await
    .unwrap()
    {
        return;
    }
    let server = server().await;
    let mut f = Fixture::new(&server);
    f.hub.conectores.release(&f.hub, &f.handle).unwrap();
    let cfg = f.actor_config();
    let mut actor = crate::agent::bot::conectores_fixture::Fixture::new(f.hub.clone(), cfg, &Fixture::actor_command())
        .await
        .unwrap();
    let entry = f.turn(1, "fixture-acp");
    actor.prompt(&entry).await.unwrap();
    f.handle.clone_from(&lifecycle::handles(&f.hub)[0].id);
    f.initialize().await;
    let first = f.start(3, frame(3, "antigo"));
    let a = f.sequence(3).await;
    let approval = decision(&a);
    f.call(&Caller::Local, "donnaConectorDecidir", approval.clone()).await.unwrap();
    // Handler síncrono: revogar antes de ceder ao driver autorizado.
    let next_entry = f.turn(2, "fixture-acp");
    assert!(
        f.call(
            &Caller::Local,
            "donnaConectorFrame",
            json!({"context":f.handle,"sequence":4,
        "frame":frame(4,"concorrente")})
        )
        .await
        .is_err()
    );
    assert!(f.call(&Caller::Local, "donnaConectorTentativa", json!({"tentativaId":a.id})).await.is_err());
    let replay = f.call(&Caller::Local, "donnaConectorDecidir", approval.clone()).await.unwrap();
    assert_eq!(replay, f.hub.store.conector_attempt(&a.id).unwrap().unwrap().receipt());
    let mut conflict = approval;
    conflict["decisao"] = json!("recusar");
    assert!(f.call(&Caller::Local, "donnaConectorDecidir", conflict.clone()).await.is_err());
    let edit = json!({"protocol":2,"tentativaId":a.id,"snapshotId":a.snapshot_id,"cardId":a.card_id,
        "cardRev":a.rev,"requestDigest":a.request_digest,"source":"app","patch":{"title":"novo"}});
    assert!(f.call(&Caller::Local, "donnaConectorEditar", edit).await.is_err());
    assert_eq!(f.hub.store.conector_attempt(&a.id).unwrap().unwrap().context, a.context);
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
    assert!(first.await.unwrap().is_err());
    actor.finish().await.unwrap();
    actor.prompt(&next_entry).await.unwrap();
    let second = f.start(5, frame(5, "novo"));
    let b = f.sequence(5).await;
    assert_eq!(b.context.turn, 2);
    assert_ne!(a.context.run, b.context.run);
    assert!(f.call(&Caller::Local, "donnaConectorDecidir", conflict).await.is_err());
    f.call(&Caller::Local, "donnaConectorDecidir", decision(&b)).await.unwrap();
    second.await.unwrap().unwrap();
    let old = f.hub.store.conector_attempt(&a.id).unwrap().unwrap();
    assert_eq!(old.context, a.context);
    assert_eq!(old.state, Estado::Cancelada);
    assert!(old.admitted_at.is_none());
    assert_eq!(server.calls.load(Ordering::SeqCst), 1);
    actor.finish().await.unwrap();
    actor.replace().await;
}

#[tokio::test]
async fn dezesseis_pendentes_sql_recusam_dezessetimo_atomicamente_sem_limitar_handles() {
    let server = server().await;
    let mut f = Fixture::new(&server);
    let original = f.hub.conectores.sources.locked()["instance-fixture"].clone();
    let mut pending = Vec::new();
    for index in 0..17 {
        if index > 0 {
            let mut source = original.clone();
            source.access.instance = format!("instance-{index}");
            let instance = source.access.instance.clone();
            f.hub.conectores.sources.locked().insert(instance.clone(), source);
            f.handle = register(&f.hub, "bot", &instance, &Lane::main("bot")).unwrap();
            f.hub.conectores.channel(&f.handle).unwrap().bind(&f.hub, "acp-fixture").unwrap();
        }
        f.initialize().await;
        let work = f.start(3, frame(3, "limite SQL"));
        if index == 16 {
            assert!(work.await.unwrap().is_err());
        } else {
            let op = f.sequence(3).await;
            pending.push((work, op));
        }
    }
    assert_eq!(lifecycle::handles(&f.hub).len(), 17);
    assert_eq!(f.count(), 16);
    assert_eq!(f.hub.store.kv_prefix("donna.conectores.tentativa.").len(), 16);
    let cards: i64 =
        f.hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_conector_cartoes", [], |r| r.get(0)).unwrap();
    assert_eq!(cards, 16);
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
    let (first, op) = pending.remove(0);
    let mut refusal = decision(&op);
    refusal["decisao"] = json!("recusar");
    f.call(&Caller::Local, "donnaConectorDecidir", refusal).await.unwrap();
    assert!(first.await.unwrap().is_err());
    let next = f.start(4, frame(4, "vaga liberada"));
    let op = f.sequence(4).await;
    assert_eq!(op.state, Estado::Aguardando);
    assert_eq!(f.count(), 17);
    next.abort();
    let _ = next.await;
    for (work, _) in pending {
        work.abort();
        let _ = work.await;
    }
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
    let count: i64 = f
        .hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT COUNT(*) FROM donna_conector_tentativas WHERE state IN ('aguardando','autorizada','em_envio')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

struct FrameDone(Arc<tokio::sync::Notify>);
impl Drop for FrameDone {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}
struct LocalApi {
    port: u16,
    frame_done: Arc<tokio::sync::Notify>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for LocalApi {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl LocalApi {
    async fn new(hub: Arc<Hub>) -> Self {
        let frame_done = Arc::new(tokio::sync::Notify::new());
        let completed = frame_done.clone();
        let router = Router::new().route(
            "/api/{method}",
            routing::post(move |axum::extract::Path(method): axum::extract::Path<String>, Json(body): Json<Value>| {
                let hub = hub.clone();
                let completed = completed.clone();
                async move {
                    // Declarado antes do futuro: notifica após conclusão ou drop do handler.
                    let _completed = (method == "donnaConectorFrame").then(|| FrameDone(completed));
                    let result = rpc(&hub, &Caller::Local, &method, body).await;
                    match result {
                        Ok(value) => (StatusCode::OK, Json(value)),
                        Err(_) => (StatusCode::BAD_REQUEST, Json(json!({"error":"retido"}))),
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self { port, frame_done, task }
    }
    fn bridge(&self, context: String) -> (tokio::io::DuplexStream, tokio::task::JoinHandle<Result<()>>) {
        let (motor, input) = tokio::io::duplex(64 * 1024);
        let port = self.port;
        let bridge = tokio::spawn(async move {
            proxy::relay(input, tokio::io::sink(), port, context, zeroize::Zeroizing::new("synthetic-local".into()))
                .await
        });
        (motor, bridge)
    }
}
fn session_requests(directory: &Path) -> Vec<Value> {
    std::fs::read_to_string(directory.join("session-requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[tokio::test]
async fn relay_release_invalida_dono_sem_reprovisionar_ou_adotar_pedido_antigo() {
    use tokio::io::AsyncWriteExt;
    if crate::agent::bot::conectores_fixture::isolated(concat!(
        module_path!(),
        "::relay_release_invalida_dono_sem_reprovisionar_ou_adotar_pedido_antigo"
    ))
    .await
    .unwrap()
    {
        return;
    }
    for cancel in [false, true] {
        let server = server().await;
        *server.mode.locked() = "hold";
        let mut f = Fixture::new(&server);
        f.hub.conectores.release(&f.hub, &f.handle).unwrap();
        let mut cfg = f.actor_config();
        let command = Fixture::actor_command();
        cfg.command = Some(command.clone());
        f.hub.store.save_bot(&cfg).unwrap();
        let mut actor =
            crate::agent::bot::conectores_fixture::Fixture::new(f.hub.clone(), cfg, &command).await.unwrap();
        let entry = f.turn(1, "fixture-acp");
        actor.prompt(&entry).await.unwrap();
        f.handle.clone_from(&lifecycle::handles(&f.hub)[0].id);
        let old_handle = f.handle.clone();
        f.initialize().await;
        let api = LocalApi::new(f.hub.clone()).await;
        let (mut motor, bridge) = api.bridge(old_handle.clone());
        let mut bytes = serde_json::to_vec(&frame(3, "pedido original")).unwrap();
        bytes.push(b'\n');
        motor.write_all(&bytes).await.unwrap();
        let original = f.sequence(1).await;
        let approval = decision(&original);
        f.call(&Caller::Local, "donnaConectorDecidir", approval.clone()).await.unwrap();
        spins_until(|| server.calls.load(Ordering::SeqCst) == 1).await;
        if cancel {
            let mut bytes = serde_json::to_vec(&json!({"jsonrpc":"2.0","method":"notifications/cancelled",
                "params":{"requestId":3}}))
            .unwrap();
            bytes.push(b'\n');
            motor.write_all(&bytes).await.unwrap();
        } else {
            motor.shutdown().await.unwrap();
        }
        bridge.await.unwrap().unwrap();
        // O handler real terminou, incluindo Pending::drop; não comparar uma rev intermediária.
        tokio::time::timeout(Duration::from_secs(2), api.frame_done.notified()).await.unwrap();
        let saved = f.hub.store.conector_attempt(&original.id).unwrap().unwrap();
        assert_eq!(saved.state, Estado::Incerta);
        assert_eq!(saved.context, original.context);
        assert_eq!(saved.t0, original.t0);
        assert_eq!(saved.card_deadline, original.card_deadline);
        assert_eq!(saved.card_id, original.card_id);
        assert!(f.hub.store.kv_read(&saved.slot()).unwrap().is_none());
        assert!(lifecycle::handles(&f.hub).is_empty());
        assert!(actor.connected());
        assert!(actor.loaded());
        let requests_before = session_requests(&f.dir);
        assert_eq!(requests_before.len(), 1);
        let remote_before = server.requests.load(Ordering::SeqCst);
        actor.finish().await.unwrap();
        let rejected_entry = f.turn(2, "fixture-acp");
        f.hub.set_runtime("bot", |r| r.session = None);
        let error = actor.ensure().await.unwrap_err();
        assert!(error.to_string().contains("Confira o recibo antes de outra ação"));
        assert!(!actor.connected());
        assert_eq!(session_requests(&f.dir), requests_before);
        assert_eq!(server.requests.load(Ordering::SeqCst), remote_before);
        assert_eq!(f.count(), 1);
        assert_eq!(f.hub.store.conector_attempt(&saved.id).unwrap().unwrap().receipt(), saved.receipt());
        assert_eq!(f.hub.store.conector_attempt(&saved.id).unwrap().unwrap().context, saved.context);
        assert!(f.hub.store.kv_read(&saved.slot()).unwrap().is_none());
        let (mut restarted, old_bridge) = api.bridge(old_handle.clone());
        let mut bytes = serde_json::to_vec(&frame(4, "mesma definição encerrada")).unwrap();
        bytes.push(b'\n');
        restarted.write_all(&bytes).await.unwrap();
        assert!(old_bridge.await.unwrap().is_err());
        assert_eq!(session_requests(&f.dir), requests_before);
        assert_eq!(server.requests.load(Ordering::SeqCst), remote_before);
        assert_eq!(f.count(), 1);
        // Só um turno futuro independente percorre o fluxo normal de criação/load.
        let next_entry = f.turn(3, "fixture-acp");
        actor.prompt(&next_entry).await.unwrap();
        assert_eq!(f.hub.store.entry(&rejected_entry).unwrap().data["status"], "queued");
        f.handle.clone_from(&lifecycle::handles(&f.hub)[0].id);
        assert_ne!(f.handle, old_handle);
        assert_eq!(session_requests(&f.dir).len(), requests_before.len() + 1);
        assert_eq!(server.calls.load(Ordering::SeqCst), 1);
        f.initialize().await;
        let work = f.start(10, frame(10, "outra ação independente"));
        let next = f.sequence(10).await;
        assert_eq!(next.context.turn, 3);
        assert_ne!(next.id, original.id);
        assert_ne!(next.context.run, original.context.run);
        assert_ne!(next.context.message, original.context.message);
        assert_eq!(f.call(&Caller::Local, "donnaConectorDecidir", approval).await.unwrap(), saved.receipt());
        assert_eq!(f.hub.store.conector_attempt(&next.id).unwrap().unwrap().state, Estado::Aguardando);
        let mut refusal = decision(&next);
        refusal["decisao"] = json!("recusar");
        f.call(&Caller::Local, "donnaConectorDecidir", refusal).await.unwrap();
        assert!(work.await.unwrap().is_err());
        let unchanged = f.hub.store.conector_attempt(&saved.id).unwrap().unwrap();
        assert_eq!(unchanged.receipt(), saved.receipt());
        assert_eq!(unchanged.context, original.context);
        assert!(f.hub.store.kv_read(&unchanged.slot()).unwrap().is_none());
        assert_eq!(server.calls.load(Ordering::SeqCst), 1);
        actor.finish().await.unwrap();
        actor.replace().await;
        assert!(lifecycle::handles(&f.hub).is_empty());
    }
}

// PR #202/#4462360470: fixture positiva deliberadamente RED antes da correção da relay.
struct ReleaseInitializedAck(Arc<tokio::sync::Notify>);
impl Drop for ReleaseInitializedAck {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}
struct RelayTask(tokio::task::JoinHandle<Result<()>>);
impl Drop for RelayTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Observa bytes entregues ao parser real, sem interpretar ou fabricar frames.
struct HandshakeRead<R> {
    input: R,
    bytes: Arc<AtomicUsize>,
    through_list: usize,
    consumed: Arc<tokio::sync::Notify>,
}
impl<R: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for HandshakeRead<R> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let filled = buffer.filled().len();
        let result = tokio::io::AsyncRead::poll_read(std::pin::Pin::new(&mut this.input), cx, buffer);
        if let std::task::Poll::Ready(Ok(())) = &result {
            let count = buffer.filled().len() - filled;
            let before = this.bytes.fetch_add(count, Ordering::SeqCst);
            if before < this.through_list && before + count >= this.through_list {
                this.consumed.notify_one();
            }
        }
        result
    }
}

async fn handshake_server(
    entered: Arc<tokio::sync::Notify>,
    ack: Arc<tokio::sync::Notify>,
    methods: Arc<Mutex<Vec<String>>>,
) -> Server {
    handshake_server_control(entered, ack, methods, "notifications/initialized", StatusCode::ACCEPTED).await
}
async fn handshake_server_control(
    entered: Arc<tokio::sync::Notify>,
    ack: Arc<tokio::sync::Notify>,
    methods: Arc<Mutex<Vec<String>>>,
    hold: &'static str,
    status: StatusCode,
) -> Server {
    use axum::{body::Body, response::Response};
    let requests = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let (counted, effects, received) = (requests.clone(), calls.clone(), bodies.clone());
    let router = Router::new().route(
        "/mcp",
        routing::post(move |Json(frame): Json<Value>| {
            let (entered, ack, methods) = (entered.clone(), ack.clone(), methods.clone());
            let (counted, effects, received) = (counted.clone(), effects.clone(), received.clone());
            async move {
                counted.fetch_add(1, Ordering::SeqCst);
                let method = frame["method"].as_str().unwrap().to_owned();
                methods.locked().push(method.clone());
                if method == hold {
                    entered.notify_one();
                    ack.notified().await;
                    if !status.is_success() {
                        return Response::builder().status(status).body(Body::empty()).unwrap();
                    }
                }
                let result = match method.as_str() {
                    "initialize" => json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},
                    "serverInfo":{"name":"fixture-handshake","version":"1"}}),
                    "notifications/initialized" => {
                        assert!(frame.get("id").is_none());
                        return Response::builder().status(202).body(Body::empty()).unwrap();
                    }
                    "tools/list" => json!({"tools":[{"name":"fixture_create","inputSchema":rule().schema}]}),
                    _ => {
                        if method == "tools/call" {
                            effects.fetch_add(1, Ordering::SeqCst);
                            received.locked().push(frame);
                        }
                        return Response::builder().status(400).body(Body::empty()).unwrap();
                    }
                };
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"jsonrpc":"2.0","id":frame["id"],"result":result}).to_string()))
                    .unwrap()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Server { url, calls, requests, bodies, mode: Arc::new(Mutex::new("handshake")), task }
}

#[tokio::test]
async fn relay_handshake_initialized_preserva_tools_list_com_ack_bloqueado() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let entered = Arc::new(tokio::sync::Notify::new());
    let ack = Arc::new(tokio::sync::Notify::new());
    let methods = Arc::new(Mutex::new(Vec::new()));
    let server = handshake_server(entered.clone(), ack.clone(), methods.clone()).await;
    let f = Fixture::new(&server);
    let api = LocalApi::new(f.hub.clone()).await;
    let release_ack = ReleaseInitializedAck(ack);
    let init = serde_json::to_string(&json!({"jsonrpc":"2.0","id":1,"method":"initialize",
        "params":{"protocolVersion":"2024-11-05","capabilities":{},
            "clientInfo":{"name":"fixture-mcp-300","version":"1"}}}))
    .unwrap()
        + "\n";
    let initialized =
        serde_json::to_string(&json!({"jsonrpc":"2.0","method":"notifications/initialized"})).unwrap() + "\n";
    let list =
        serde_json::to_string(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}})).unwrap() + "\n";
    let through_list = init.len() + initialized.len() + list.len();
    let consumed = Arc::new(tokio::sync::Notify::new());
    let bytes = Arc::new(AtomicUsize::new(0));
    let (engine, proxy) = tokio::io::duplex(8192);
    let (input, output) = tokio::io::split(proxy);
    let (engine_in, mut engine_out) = tokio::io::split(engine);
    let input = HandshakeRead { input, bytes: bytes.clone(), through_list, consumed: consumed.clone() };
    let (port, context) = (api.port, f.handle.clone());
    let mut relay = RelayTask(tokio::spawn(async move {
        proxy::relay(input, output, port, context, zeroize::Zeroizing::new("synthetic-local".into())).await
    }));
    let mut lines = BufReader::new(engine_in).lines();
    engine_out.write_all(init.as_bytes()).await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(3), lines.next_line()).await.unwrap().unwrap().unwrap();
    let response: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
    engine_out.write_all(initialized.as_bytes()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), entered.notified()).await.unwrap();
    engine_out.write_all(list.as_bytes()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), consumed.notified()).await.unwrap();
    assert_eq!(bytes.load(Ordering::SeqCst), through_list);
    assert_eq!(*methods.locked(), vec!["initialize".to_owned(), "notifications/initialized".to_owned()]);
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
    // O ACK não pode vencer o select antes da leitura da descoberta no stdin real.
    drop(release_ack);
    let discovery = tokio::time::timeout(Duration::from_secs(3), lines.next_line())
        .await
        .expect("A relay deve devolver a descoberta após o ACK")
        .expect("Saída da relay deve ser legível")
        .expect("tools/list ID 2 foi perdido após notifications/initialized");
    let discovery: Value = serde_json::from_str(&discovery).unwrap();
    assert_eq!(discovery["jsonrpc"], "2.0");
    assert_eq!(discovery["id"], 2);
    assert!(discovery.get("error").is_none());
    let tools = discovery["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "fixture_create");
    assert_eq!(tools[0]["inputSchema"], rule().schema);
    assert_eq!(
        *methods.locked(),
        vec!["initialize".to_owned(), "notifications/initialized".to_owned(), "tools/list".to_owned()]
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 3);
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.count(), 0);
    assert_eq!(f.hub.store.kv_prefix("donna.conectores.tentativa."), Vec::<String>::new());
    let cards: i64 =
        f.hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_conector_cartoes", [], |r| r.get(0)).unwrap();
    assert_eq!(cards, 0);
    assert!(f.hub.conectores.channel(&f.handle).is_ok());
    assert!(!relay.0.is_finished());
    engine_out.shutdown().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), &mut relay.0).await.unwrap().unwrap().unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(3), lines.next_line()).await.unwrap().unwrap().is_none());
    assert!(f.hub.conectores.channel(&f.handle).is_err());
}

fn handshake_lines() -> [String; 3] {
    let line = |value: &Value| serde_json::to_string(value).unwrap() + "\n";
    [
        line(&json!({"jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2024-11-05","capabilities":{},
                "clientInfo":{"name":"fixture-mcp-300","version":"1"}}})),
        line(&json!({"jsonrpc":"2.0","method":"notifications/initialized"})),
        line(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}})),
    ]
}
struct HandshakePipe {
    writer: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    reader: tokio::io::Lines<tokio::io::BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
    relay: RelayTask,
    consumed: Arc<tokio::sync::Notify>,
}
impl HandshakePipe {
    fn new(api: &LocalApi, context: String, through_list: usize) -> Self {
        use tokio::io::AsyncBufReadExt;
        let (engine, proxy) = tokio::io::duplex(8192);
        let (input, output) = tokio::io::split(proxy);
        let (reader, writer) = tokio::io::split(engine);
        let consumed = Arc::new(tokio::sync::Notify::new());
        let input =
            HandshakeRead { input, bytes: Arc::new(AtomicUsize::new(0)), through_list, consumed: consumed.clone() };
        let port = api.port;
        let relay = RelayTask(tokio::spawn(async move {
            proxy::relay(input, output, port, context, zeroize::Zeroizing::new("synthetic-local".into())).await
        }));
        Self { writer, reader: tokio::io::BufReader::new(reader).lines(), relay, consumed }
    }
    async fn write(&mut self, line: &str) {
        use tokio::io::AsyncWriteExt;
        self.writer.write_all(line.as_bytes()).await.unwrap();
    }
    async fn response(&mut self) -> Value {
        let line =
            tokio::time::timeout(Duration::from_secs(3), self.reader.next_line()).await.unwrap().unwrap().unwrap();
        serde_json::from_str(&line).unwrap()
    }
    async fn finish(&mut self) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(3), &mut self.relay.0).await.unwrap().unwrap()
    }
}

#[derive(Clone, Copy)]
enum HandshakeStop {
    Eof,
    CancelQueued,
    RefusedAck,
}

#[tokio::test]
async fn relay_initialized_eof_cancel_retido_e_erro_http_descartam_slot_sem_retry() {
    use tokio::io::AsyncWriteExt;
    for stop in [HandshakeStop::Eof, HandshakeStop::CancelQueued, HandshakeStop::RefusedAck] {
        let entered = Arc::new(tokio::sync::Notify::new());
        let ack = Arc::new(tokio::sync::Notify::new());
        let methods = Arc::new(Mutex::new(Vec::new()));
        let status = if matches!(stop, HandshakeStop::RefusedAck) {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::ACCEPTED
        };
        let server = handshake_server_control(
            entered.clone(),
            ack.clone(),
            methods.clone(),
            "notifications/initialized",
            status,
        )
        .await;
        let f = Fixture::new(&server);
        let api = LocalApi::new(f.hub.clone()).await;
        let release_ack = ReleaseInitializedAck(ack);
        let [init, initialized, list] = handshake_lines();
        let mut pipe = HandshakePipe::new(&api, f.handle.clone(), init.len() + initialized.len() + list.len());
        pipe.write(&init).await;
        assert_eq!(pipe.response().await["id"], 1);
        pipe.write(&initialized).await;
        tokio::time::timeout(Duration::from_secs(3), entered.notified()).await.unwrap();
        pipe.write(&list).await;
        tokio::time::timeout(Duration::from_secs(3), pipe.consumed.notified()).await.unwrap();
        match stop {
            HandshakeStop::Eof => pipe.writer.shutdown().await.unwrap(),
            HandshakeStop::CancelQueued => {
                pipe.write(
                    "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\"params\":{\"requestId\":2}}\n",
                )
                .await;
            }
            HandshakeStop::RefusedAck => {}
        }
        if matches!(stop, HandshakeStop::RefusedAck) {
            drop(release_ack);
            assert!(pipe.finish().await.is_err());
        } else {
            pipe.finish().await.unwrap(); // ACK ainda bloqueado: só EOF/cancel encerrou.
            drop(release_ack);
        }
        assert_eq!(*methods.locked(), vec!["initialize".to_owned(), "notifications/initialized".to_owned()]);
        assert_eq!(server.requests.load(Ordering::SeqCst), 2);
        assert_eq!(server.calls.load(Ordering::SeqCst), 0);
        assert_eq!(f.count(), 0);
        assert_eq!(f.hub.store.kv_prefix("donna.conectores.tentativa."), Vec::<String>::new());
        let cards: i64 =
            f.hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_conector_cartoes", [], |r| r.get(0)).unwrap();
        assert_eq!(cards, 0);
        assert!(f.hub.conectores.channel(&f.handle).is_err());
        assert!(
            tokio::time::timeout(Duration::from_secs(3), pipe.reader.next_line()).await.unwrap().unwrap().is_none()
        );
    }
}

#[derive(Clone, Copy)]
enum HandshakeViolation {
    Effect,
    ExtraList,
    ActiveRequest,
}

#[tokio::test]
async fn relay_initialized_slot_recusa_efeitos_excesso_e_requests_com_id_concorrentes() {
    for violation in [HandshakeViolation::Effect, HandshakeViolation::ExtraList, HandshakeViolation::ActiveRequest] {
        let entered = Arc::new(tokio::sync::Notify::new());
        let ack = Arc::new(tokio::sync::Notify::new());
        let methods = Arc::new(Mutex::new(Vec::new()));
        let active_request = matches!(violation, HandshakeViolation::ActiveRequest);
        let hold = if active_request { "initialize" } else { "notifications/initialized" };
        let server =
            handshake_server_control(entered.clone(), ack.clone(), methods.clone(), hold, StatusCode::ACCEPTED).await;
        let f = Fixture::new(&server);
        let api = LocalApi::new(f.hub.clone()).await;
        let release_ack = ReleaseInitializedAck(ack);
        let [init, initialized, list] = handshake_lines();
        let mut pipe = HandshakePipe::new(&api, f.handle.clone(), init.len() + initialized.len() + list.len());
        pipe.write(&init).await;
        if !active_request {
            assert_eq!(pipe.response().await["id"], 1);
            pipe.write(&initialized).await;
        }
        tokio::time::timeout(Duration::from_secs(3), entered.notified()).await.unwrap();
        match violation {
            HandshakeViolation::Effect => {
                let effect = serde_json::to_string(&frame(2, "não enfileirar efeito")).unwrap() + "\n";
                pipe.write(&effect).await;
            }
            HandshakeViolation::ExtraList => {
                pipe.write(&list).await;
                tokio::time::timeout(Duration::from_secs(3), pipe.consumed.notified()).await.unwrap();
                pipe.write("{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/list\",\"params\":{}}\n").await;
            }
            HandshakeViolation::ActiveRequest => pipe.write(&list).await,
        }
        assert!(pipe.finish().await.is_err()); // Não soltar ACK para fazer concorrência parecer serial.
        drop(release_ack);
        let expected = if active_request {
            vec!["initialize".to_owned()]
        } else {
            vec!["initialize".to_owned(), "notifications/initialized".to_owned()]
        };
        assert_eq!(*methods.locked(), expected);
        assert_eq!(server.requests.load(Ordering::SeqCst), if active_request { 1 } else { 2 });
        assert_eq!(server.calls.load(Ordering::SeqCst), 0);
        assert_eq!(f.count(), 0);
        assert_eq!(f.hub.store.kv_prefix("donna.conectores.tentativa."), Vec::<String>::new());
        assert!(f.hub.conectores.channel(&f.handle).is_err());
    }
}

#[tokio::test]
async fn observacao_curada_valida_contexto_original_na_mesma_tx_antes_de_qualquer_marca() {
    use crate::fork::sessoes::{
        bindings, call_tests,
        origin::SessionOrigin,
        store::{self, NewSession},
    };
    for scenario in ["swap", "revoke", "reissue", "grant"] {
        let server = server().await;
        let f = Fixture::new(&server);
        // Linha conhecida e ainda não exposta: ausente é fail-closed em produção.
        f.hub.store.memory_session_start("bot", "acp-b", false).unwrap();
        assert!(!f.hub.store.memory_session_marked("bot", "acp-fixture").unwrap());
        assert!(!f.hub.store.memory_session_marked("bot", "acp-b").unwrap());
        let hub = f.hub.clone();
        let reached = call_tests::interleave(&f.hub.store, "curated_observe_commit", move || match scenario {
            "swap" => {
                let extra = store::create(
                    &hub.store.db.locked(),
                    &NewSession {
                        chat: "bot".into(),
                        origin: SessionOrigin::Owner,
                        source_session: None,
                        parent_session: None,
                        title: "B".into(),
                        runs_on: None,
                    },
                )
                .unwrap();
                let binding = bindings::issue(&hub.store, "bot", "bot", &extra.id, None).unwrap();
                bindings::activate(&hub.store, &binding, "acp-b").unwrap();
                let lane = Lane { chat: "bot".into(), session: Some(extra.id) };
                let entry = hub.store.insert_entry(&lane, EntryKind::User, 2, &json!({"text":"Pedido de B"})).unwrap();
                hub.start_bot_turn("bot", |r| {
                    r.lane = Some(lane);
                    r.session = Some("acp-b".into());
                    r.message = Some(entry.id);
                })
                .unwrap();
            }
            "revoke" => bindings::revoke(&hub.store, "bot", Some("acp-fixture")).unwrap(),
            "reissue" => {
                bindings::revoke(&hub.store, "bot", Some("acp-fixture")).unwrap();
                let next = bindings::issue(&hub.store, "bot", "bot", "bot", None).unwrap();
                bindings::activate(&hub.store, &next, "acp-fixture").unwrap();
            }
            "grant" => {
                let mut sources = hub.conectores.sources.locked();
                sources.get_mut("instance-fixture").unwrap().access.generation = REPLACEMENT_GENERATION.into();
            }
            _ => unreachable!(),
        });
        let result = f
            .call(
                &Caller::Local,
                "donnaConectorFrame",
                json!({"context":f.handle,"sequence":1,
            "frame":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05",
                "capabilities":{},"clientInfo":{"name":"fixture-mcp-300","version":"1"}}}}),
            )
            .await;
        assert!(result.is_err(), "{scenario}: {result:?}");
        assert!(reached.load(Ordering::SeqCst), "{scenario}");
        assert_eq!(f.hub.store.guard_test_snapshot().unwrap(), (vec![], 0));
        assert!(!f.hub.store.memory_session_marked("bot", "acp-fixture").unwrap());
        assert!(!f.hub.store.memory_session_marked("bot", "acp-b").unwrap());
        assert_eq!(f.count(), 0);
    }
    let denied_server = server().await;
    let denied = Fixture::new(&denied_server);
    bindings::revoke(&denied.hub.store, "bot", Some("acp-fixture")).unwrap();
    assert!(
        denied
            .call(
                &Caller::Local,
                "donnaConectorFrame",
                json!({"context":denied.handle,"sequence":1,
        "frame":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05",
            "capabilities":{},"clientInfo":{"name":"fixture-mcp-300","version":"1"}}}})
            )
            .await
            .is_err()
    );
    assert_eq!(denied_server.requests.load(Ordering::SeqCst), 0);
    assert_eq!(denied.hub.store.guard_test_snapshot().unwrap(), (vec![], 0));
    let server = server().await;
    let valid = Fixture::new(&server);
    valid.initialize().await;
    let (receipts, exposed) = valid.hub.store.guard_test_snapshot().unwrap();
    assert_eq!(receipts.len(), 2);
    assert_eq!(exposed, 1);
    assert!(valid.hub.store.memory_session_marked("bot", "acp-fixture").unwrap());
}

#[tokio::test]
async fn binding_reemitido_na_mesma_acp_nao_libera_alvo_headers_ou_recibo_da_captura_antiga() {
    use crate::fork::sessoes::{bindings, call_tests};
    let server = server().await;
    let f = Fixture::new(&server);
    let hub = f.hub.clone();
    let reached = call_tests::interleave(&f.hub.store, "curated_target", move || {
        let original = bindings::for_acp(&hub.store, "bot", "acp-fixture").unwrap();
        bindings::revoke(&hub.store, "bot", Some("acp-fixture")).unwrap();
        let next = bindings::issue(&hub.store, "bot", "bot", "bot", None).unwrap();
        bindings::activate(&hub.store, &next, "acp-fixture").unwrap();
        assert_ne!(next.id, original.id);
    });
    let result = f
        .call(
            &Caller::Local,
            "donnaConectorFrame",
            json!({"context":f.handle,"sequence":1,
        "frame":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05",
            "capabilities":{},"clientInfo":{"name":"fixture-mcp-300","version":"1"}}}}),
        )
        .await;
    assert!(result.is_err());
    assert!(reached.load(Ordering::SeqCst));
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.hub.store.guard_test_snapshot().unwrap(), (vec![], 0));
}

#[tokio::test]
async fn recibo_terminal_legado_sem_binding_permanece_legivel_sem_autorizar_execucao() {
    let server = server().await;
    let f = Fixture::new(&server);
    f.initialize().await;
    let work = f.start(3, frame(3, "Legado"));
    let op = f.pending().await;
    let mut refusal = decision(&op);
    refusal["decisao"] = "recusar".into();
    let receipt = f.call(&Caller::Local, "donnaConectorDecidir", refusal).await.unwrap();
    assert_eq!(receipt["state"], "recusada");
    assert!(work.await.unwrap().is_err());
    let receipt = f.hub.store.conector_attempt(&op.id).unwrap().unwrap().receipt();
    f.hub
        .store
        .db
        .locked()
        .execute(
            "UPDATE donna_conector_tentativas SET data=json_remove(data,'$.context.sessionBinding','$.source.conta',
                '$.source.rules.fixture_create.fields.team.rotulo','$.source.rules.fixture_create.fields.title.rotulo') WHERE id=?1",
            [&op.id],
        )
        .unwrap();
    let legacy = f.hub.store.conector_attempt(&op.id).unwrap().unwrap();
    assert!(legacy.context.session_binding.is_none());
    // Recibo anterior à conta de saída: legível, mas o default nunca admite pedido novo.
    assert!(legacy.source.conta.is_none() && legacy.source.validate().is_err());
    let channel = f.hub.conectores.channel(&f.handle).unwrap();
    assert!(channel.transaction(&f.hub, &legacy.context, |_| Ok(((), vec![]))).is_err());
    assert_eq!(f.call(&Caller::Local, "donnaConectorTentativa", json!({"tentativaId":op.id})).await.unwrap(), receipt);
    assert!(!receipt.to_string().contains("sessionBinding"));
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn conta_saida_ausente_trocada_ou_app_antigo_nao_autoriza_nem_envia() {
    let server = server().await;
    let f = Fixture::new(&server);
    // Sem identidade canônica, com vínculo divergente ou identidade vazia: sem canal nem cartão.
    let original = f.hub.conectores.sources.locked().get("instance-fixture").unwrap().clone();
    let mut sem_conta = original.clone();
    sem_conta.conta = None;
    let mut divergente = original.clone();
    divergente.conta = Some(conta("conta-b", "pareamento-b", "+55 11 90000-0002"));
    let mut vazia = original.clone();
    vazia.conta = Some(conta("conta-a", "pareamento-a", " "));
    let mut sem_vinculo = original.clone();
    sem_vinculo.access.conta = None;
    let mut sem_rotulo = original.clone();
    sem_rotulo.access.rules.get_mut("fixture_create").unwrap().fields.get_mut("title").unwrap().rotulo = String::new();
    for broken in [sem_conta, divergente, vazia, sem_vinculo, sem_rotulo] {
        f.hub.conectores.sources.locked().insert("instance-fixture".into(), broken);
        assert!(register(&f.hub, "bot", "instance-fixture", &Lane::main("bot")).is_err());
    }
    f.hub.conectores.sources.locked().insert("instance-fixture".into(), original);
    f.initialize().await;

    // Cartão A: app antigo (protocolo 1) não aprova; o app não declara conta pela edição.
    let work = f.start(3, frame(3, "conta-a"));
    let op = f.pending().await;
    let mut antigo = decision(&op);
    antigo["protocol"] = json!(1);
    assert!(f.call(&Caller::Local, "donnaConectorDecidir", antigo).await.is_err());
    let edit = json!({"protocol":2,"tentativaId":op.id,"snapshotId":op.snapshot_id,"cardId":op.card_id,
        "cardRev":op.rev,"requestDigest":op.request_digest,"source":"app","patch":{"conta":"+55 11 90000-0002"}});
    assert!(f.call(&Caller::Local, "donnaConectorEditar", edit).await.is_err());
    let edicao_v1 = json!({"protocol":1,"tentativaId":op.id,"snapshotId":op.snapshot_id,"cardId":op.card_id,
        "cardRev":op.rev,"requestDigest":op.request_digest,"source":"app","patch":{"title":"editado"}});
    assert!(f.call(&Caller::Local, "donnaConectorEditar", edicao_v1).await.is_err());
    assert_eq!(f.hub.store.conector_attempt(&op.id).unwrap().unwrap().card_id, op.card_id);
    // Repareamento na janela (mesma conta, nova encarnação/identidade): cartão A não autoriza B.
    f.hub.conectores.sources.locked().get_mut("instance-fixture").unwrap().conta =
        Some(conta("conta-a", "pareamento-a", "+55 11 90000-0002"));
    assert!(f.call(&Caller::Local, "donnaConectorDecidir", decision(&op)).await.is_err());
    assert!(f.call(&Caller::Local, "donnaConectorTentativa", json!({"tentativaId":op.id})).await.is_err());
    f.call(&Caller::Local, "donnaConectorRelease", json!({"context":f.handle})).await.unwrap();
    assert!(work.await.unwrap().is_err());
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);

    // Troca A→B depois da aprovação e antes da admissão: before_send recusa, nada sai.
    let current = Fixture::new(&server);
    current.initialize().await;
    let work = current.start(3, frame(3, "conta-a"));
    let op = current.pending().await;
    // Handler síncrono neste runtime: não ceder ao driver entre decisão e troca.
    current.call(&Caller::Local, "donnaConectorDecidir", decision(&op)).await.unwrap();
    {
        let mut sources = current.hub.conectores.sources.locked();
        let source = sources.get_mut("instance-fixture").unwrap();
        let b = conta("conta-b", "pareamento-b", "+55 11 90000-0002");
        source.access.conta = Some(b.vinculo.clone());
        source.conta = Some(b);
    }
    assert!(work.await.unwrap().is_err());
    let saved = current.hub.store.conector_attempt(&op.id).unwrap().unwrap();
    assert_eq!(saved.source.conta.as_ref().unwrap().conta, "conta-a");
    assert_eq!(saved.state, Estado::Cancelada);
    assert!(saved.admitted_at.is_none());
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn rotulo_curado_nao_vem_do_servidor_e_mudanca_invalida_cartao() {
    let server = server().await;
    let f = Fixture::new(&server);
    f.initialize().await;
    let work = f.start(3, frame(3, "rotulos"));
    let op = f.pending().await;
    let private = f.call(&Caller::Local, "donnaConectorTentativa", json!({"tentativaId":op.id})).await.unwrap();
    // A descrição remota ("Fixture") não entra; só a curadoria do snapshot.
    assert!(!private["presentation"].to_string().contains("\"Fixture\""));
    assert_eq!(private["presentation"]["rotulos"]["title"], "Título");
    // Mudar o significado exibido muda a concessão: a aprovação antiga não vale.
    f.hub
        .conectores
        .sources
        .locked()
        .get_mut("instance-fixture")
        .unwrap()
        .access
        .rules
        .get_mut("fixture_create")
        .unwrap()
        .presentation = "Apagar registro sintético".into();
    assert!(f.call(&Caller::Local, "donnaConectorDecidir", decision(&op)).await.is_err());
    f.call(&Caller::Local, "donnaConectorRelease", json!({"context":f.handle})).await.unwrap();
    assert!(work.await.unwrap().is_err());
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn porteiro_recusa_proxy_curado_apos_cartao_sem_enviar() {
    use crate::fork::rules::rule::{Effect, Rule};
    let server = server().await;
    let f = Fixture::new(&server);
    f.initialize().await;
    let work = f.start(3, frame(3, "teste"));
    let op = f.pending().await;
    let revision = f.hub.store.donna_rules().unwrap().revision;
    f.hub
        .store
        .donna_rule_save(
            Rule {
                id: String::new(),
                effect: Effect::Deny,
                field: "ferramenta".into(),
                op: "igual".into(),
                value: "mcp__donna-curado-instance-fixture__fixture_create".into(),
                bot_id: None,
                tool: None,
                scope: None,
                enabled: true,
                revision: 0,
                created_at: 0,
                updated_at: 0,
            },
            revision,
            true,
        )
        .unwrap();
    f.call(&Caller::Local, "donnaConectorDecidir", decision(&op)).await.unwrap();
    let response = work.await.unwrap().unwrap();
    assert!(response["error"]["message"].as_str().unwrap().contains("regra"));
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
}
