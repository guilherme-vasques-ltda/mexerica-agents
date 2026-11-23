//! Only D70 boundaries and the changed flow. No real helper, shell, services or harness.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use crate::api;
use crate::api::devices::{Caller, permit};
use crate::hub::{BotStatus, Hub};
use crate::remote::{
    channel::{self, ChannelKind, Out, Transport},
    crypto::{self, Opener, Sealer},
    identity::Identity,
};
use crate::store::{BotConfig, Device, DeviceSource, EntryKind, Lane, Scope, Store, now_ms};
use ed25519_dalek::SigningKey;
use serde_json::{Value, json};
use std::{net::IpAddr, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use x25519_dalek::StaticSecret;

struct Executor {
    key: SigningKey,
    tx: mpsc::Sender<Value>,
    rx: mpsc::Receiver<Out>,
    sealer: Option<Sealer>,
    opener: Option<Opener>,
    run: tokio::task::JoinHandle<()>,
    seq: u32,
}
impl Drop for Executor {
    fn drop(&mut self) {
        self.run.abort();
    }
}
impl Executor {
    async fn connect(hub: &Arc<Hub>, key: SigningKey, pair: bool) -> Self {
        let (tx, incoming) = mpsc::channel(64);
        let (outgoing, rx) = mpsc::channel(64);
        let run = tokio::spawn(channel::run(
            hub.clone(),
            Transport { incoming, outgoing, porta: channel::Porta::default() },
            ChannelKind::Direct { peer: IpAddr::from([10, 0, 0, 2]) },
        ));
        let mut d = Self { key, tx, rx, sealer: None, opener: None, run, seq: 0 };
        let secret = StaticSecret::from(crypto::random::<32>());
        let ep = crypto::x25519_pub(&secret);
        let n = crypto::random::<32>();
        let dk = d.key.verifying_key().to_bytes();
        let cid = hub.identity.cid_raw();
        d.tx.send(json!({"t":"hello","v":1,"dk":d.dk(),"ek":crypto::b64(&ep),"n":crypto::b64(&n),"sig":crypto::b64(&crypto::sign(&d.key,&crypto::hs1_input(&cid,&dk,&ep,&n))),"pair":pair})).await.unwrap();
        let Out::Msg(w) = d.next().await else { panic!("welcome expected") };
        assert_eq!(w["t"], "welcome");
        let eh = crypto::unb64_n(w["ek"].as_str().unwrap()).unwrap();
        let th = crypto::transcript_hash(&cid, &dk, &ep, &n, &eh);
        crypto::verify(&hub.identity.sign_pub(), &th, &crypto::unb64_n(w["sig"].as_str().unwrap()).unwrap()).unwrap();
        let keys = crypto::channel_keys(&crypto::x25519(&secret, &eh).unwrap(), &th);
        d.sealer = Some(Sealer::new(keys.d2h));
        d.opener = Some(Opener::new(keys.h2d, 16 << 20));
        d
    }
    fn dk(&self) -> String {
        crypto::b64(self.key.verifying_key().as_bytes())
    }
    async fn next(&mut self) -> Out {
        tokio::time::timeout(Duration::from_secs(5), self.rx.recv()).await.unwrap().unwrap()
    }
    async fn send(&mut self, v: Value) {
        for f in self.sealer.as_mut().unwrap().seal(v.to_string().as_bytes()).unwrap() {
            self.tx.send(f).await.unwrap();
        }
    }
    async fn recv(&mut self) -> Value {
        loop {
            match self.next().await {
                Out::Msg(v) if v["t"] == "f" => {
                    if let Some(bytes) = self.opener.as_mut().unwrap().open(&v).unwrap() {
                        return serde_json::from_slice(&bytes).unwrap();
                    }
                }
                Out::Msg(_) => {}
                Out::Close(code, reason) => panic!("unexpected close {code}: {reason}"),
            }
        }
    }
    async fn call(&mut self, m: &str, b: Value) -> Value {
        self.seq += 1;
        let id = self.seq;
        self.send(json!({"id":id,"m":m,"b":b})).await;
        let v = self.recv().await;
        assert_eq!(v["id"], id);
        v
    }
    async fn enroll(hub: &Arc<Hub>) -> Self {
        let code = pairing::issue(hub)["code"].clone();
        let key = SigningKey::from_bytes(&crypto::random());
        let mut d = Self::connect(hub, key.clone(), true).await;
        assert!(
            d.call("pair", json!({"role":"executor","code":code,"name":"Computador de teste","platform":"macos"}))
                .await["ok"]
                .is_object()
        );
        assert!(matches!(d.next().await, Out::Close(4100, _)));
        let mut d = Self::connect(hub, key, false).await;
        let h = d.call("executorHello", hello()).await;
        assert!(h["ok"].is_object(), "{h}");
        assert_eq!(h["ok"]["limits"]["maxResult"], 40_960);
        d
    }
}
fn hello() -> Value {
    json!({"proto":1,"bootId":"boot-1","version":"test","platform":"macos","capabilities":{"terminal":true,"arquivos":true,"spark":true,"op":true},"presence":{"appConnected":true,"locked":false,"idleSeconds":0,"stopped":false,"sparkRunning":true,"opReady":true},"journal":[],"routines":[]})
}
fn hub() -> Arc<Hub> {
    let dir = std::env::temp_dir().join(format!("donna-computers-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
        "hub".into(),
        Identity::load_or_create(&dir).unwrap(),
        "test-only".into(),
        0,
    );
    std::fs::remove_dir_all(dir).unwrap();
    let cfg: BotConfig = serde_json::from_value(json!({"id":"donna","name":"Donna"})).unwrap();
    hub.store.save_bot(&cfg).unwrap();
    hub.store.set_team_settings(&json!({"primaryBotId":"donna"})).unwrap();
    user_turn(&hub, None);
    hub
}
fn user_turn(hub: &Hub, from: Option<&str>) -> String {
    let mut data = json!({"text":"pedido do dono"});
    if let Some(id) = from {
        data["fromComputer"] = json!(id);
    }
    let e = hub.store.insert_entry(&Lane::main("donna"), EntryKind::User, 1, &data).unwrap();
    crate::fork::sessoes::bindings::fixture_binding(&hub.store, "donna");
    hub.set_runtime("donna", |rt| {
        rt.session = Some("fixture-acp".into());
        rt.memory_turn_token = Some("fixture-turn".into());
        rt.status = BotStatus::Working;
        rt.lane = Some(Lane::main("donna"));
        rt.message = Some(e.id.clone());
    });
    e.id
}
async fn bound_call(hub: &Arc<Hub>, body: &Value) -> Value {
    let mut body = body.clone();
    let binding = crate::fork::sessoes::bindings::fixture_binding(&hub.store, "donna");
    body["sessionBinding"] = json!(binding.id.as_str());
    let execution = crate::fork::sessoes::bindings::capture(hub, "donna", binding.id.as_str()).ok();
    tools::call(hub, &body, execution.as_ref()).await
}
fn c(hub: &Hub, d: &Executor) -> registry::Computer {
    hub.store.computers().unwrap().into_iter().find(|c| c.device_key == d.dk()).unwrap()
}
fn body() -> Value {
    json!({"botId":"donna","name":"run_command","arguments":{"command":"printf ok","reason":"teste"}})
}
fn make_job(hub: &Hub, c: &registry::Computer) -> jobs::Job {
    make_job_kind(hub, c, "exec")
}
fn make_job_kind(hub: &Hub, c: &registry::Computer, kind: &str) -> jobs::Job {
    jobs::create(
        hub,
        "donna",
        &jobs::authorize(hub, "donna").unwrap(),
        c,
        jobs::Request {
            kind,
            spec: if kind == "program" {
                json!({"program":"spark","args":["--version"]})
            } else {
                json!({"command":"printf ok","shell":"login"})
            },
            timeout: 60_000,
            reason: "teste",
        },
    )
    .unwrap()
}
async fn delivered(hub: &Hub, d: &mut Executor) -> jobs::Job {
    let c = c(hub, d);
    let j = make_job(hub, &c);
    let p = d.call("executorPoll", json!({"bootId":"boot-1","waitMs":0,"presence":hello()["presence"]})).await;
    assert_eq!(p["ok"]["jobs"][0]["jobId"], j.id, "{p}");
    jobs::find(hub, &j.id).unwrap()
}
async fn update(d: &mut Executor, j: &jobs::Job, state: &str) -> Value {
    d.call("executorUpdate", json!({"jobId":j.id,"leaseToken":j.lease_token,"state":state})).await
}
async fn result(d: &mut Executor, j: &jobs::Job) -> Value {
    d.call("executorResult",json!({"jobId":j.id,"leaseToken":j.lease_token,"outcome":"done","exitCode":0,"stdout":"ok","decidedBy":"click","auditSeq":1})).await
}

#[tokio::test]
async fn d502_todos_exec_program_create_poll_start_e_cancel_result() {
    use crate::fork::atividades::revisao::estado::fixture;
    for kind in ["exec", "program"] {
        let hub = hub();
        let mut first = Executor::enroll(&hub).await;
        let mut second = Executor::enroll(&hub).await;
        let computer = c(&hub, &first);
        let mut j = make_job_kind(&hub, &computer, kind);
        let p = first.call("executorPoll", json!({"bootId":"boot-1","waitMs":0,"presence":hello()["presence"]})).await;
        assert_eq!(p["ok"]["jobs"][0]["kind"], kind);
        j = jobs::find(&hub, &j.id).unwrap();
        assert_eq!(update(&mut first, &j, "accepted").await["ok"]["go"], true);
        fixture::hold(&hub, "original");
        assert!(update(&mut first, &j, "start").await["err"].is_object());
        assert!(jobs::find(&hub, &j.id).unwrap().started_at.is_none());
        let mut attempted = j.clone();
        attempted.id = "synthetic-new-job".into();
        attempted.state = jobs::Phase::Queued;
        assert!(hub.store.save_computer_job(&attempted).is_err(), "Store recusa criação direta");
        attempted.id.clone_from(&j.id);
        assert!(hub.store.save_computer_job(&attempted).is_err());
        attempted.state = jobs::Phase::Running;
        attempted.started_at = Some(now_ms());
        assert!(hub.store.save_computer_job(&attempted).is_err(), "Store também fecha atalho direto");
        for target in [&computer, &c(&hub, &second)] {
            assert!(
                jobs::create(
                    &hub,
                    "donna",
                    &jobs::authorize(&hub, "donna").unwrap(),
                    target,
                    jobs::Request { kind, spec: json!({"command":"synthetic"}), timeout: 60_000, reason: "fixture" }
                )
                .is_err()
            );
        }
        assert_eq!(
            second.call("executorPoll", json!({"bootId":"boot-1","waitMs":0,"presence":hello()["presence"]})).await["ok"]
                ["jobs"],
            json!([])
        );
        let tool_name = if kind == "program" { "run_program" } else { "run_command" };
        let args = if kind == "program" {
            json!({"program":"spark","args":["--version"],"reason":"fixture","computer":c(&hub,&second).id})
        } else {
            json!({"command":"printf ok","reason":"fixture","computer":c(&hub,&second).id})
        };
        let refused = bound_call(&hub, &json!({"botId":"donna","name":tool_name,"arguments":args})).await;
        assert_eq!(refused["isError"], true);
        assert!(refused["content"][0]["text"].as_str().unwrap().starts_with("busy:"), "{refused}");
        jobs::cancel(&hub, &j.id).unwrap();
        assert_eq!(jobs::find(&hub, &j.id).unwrap().state, jobs::Phase::Cancelled);
        // Resultado negativo conclusivo ainda é reconciliado durante o hold.
        let r = first
            .call(
                "executorResult",
                json!({"jobId":j.id,"leaseToken":j.lease_token,"outcome":"cancelled","decidedBy":"none","auditSeq":1}),
            )
            .await;
        assert_eq!(r["ok"]["ack"], true, "{r}");
    }
}

#[tokio::test]
async fn d502_long_poll_ja_pendente_reconfere_hold_sem_reoferta() {
    use crate::fork::atividades::revisao::estado::fixture;
    for kind in ["exec", "program"] {
        let hub = hub();
        let mut d = Executor::enroll(&hub).await;
        let computer = c(&hub, &d);
        let queued = make_job_kind(&hub, &computer, kind);
        // Mantém o poll pendente atrás de um trabalho sintético ativo.
        hub.store
            .db
            .locked()
            .execute(
                "UPDATE donna_computer_jobs SET state='running',started_at=?1 WHERE id=?2",
                rusqlite::params![now_ms(), queued.id],
            )
            .unwrap();
        let previous = hub.computers.sessions.locked()[&computer.id].poll;
        d.seq += 1;
        d.send(
            json!({"id":d.seq,"m":"executorPoll","b":{"bootId":"boot-1","waitMs":1000,"presence":hello()["presence"]}}),
        )
        .await;
        tokio::time::timeout(Duration::from_secs(1), async {
            while hub.computers.sessions.locked()[&computer.id].poll == previous {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        // Só a fixture recoloca o job na fila depois de publicar hold.
        fixture::hold(&hub, "original");
        hub.store
            .db
            .locked()
            .execute("UPDATE donna_computer_jobs SET state='queued',started_at=NULL WHERE id=?1", [&queued.id])
            .unwrap();
        hub.computers.wake();
        let message = jobs::authorize(&hub, "donna").unwrap();
        assert!(
            jobs::create(
                &hub,
                "donna",
                &message,
                &computer,
                jobs::Request {
                    kind: "program",
                    spec: json!({"program":"synthetic"}),
                    timeout: 60_000,
                    reason: "fixture"
                }
            )
            .is_err()
        );
        assert_eq!(d.recv().await["ok"]["jobs"], json!([]));
        assert_eq!(jobs::find(&hub, &queued.id).unwrap().state, jobs::Phase::Queued);
    }
}

#[tokio::test]
async fn executor_scope_and_cross_computer_boundary() {
    let hub = hub();
    let mut d = Executor::enroll(&hub).await;
    let mut other = Executor::enroll(&hub).await;
    let device = hub.store.device(&d.dk()).unwrap();
    assert_eq!(device.scopes, [Scope::Executor]);
    let caller = Caller::Device { key: d.dk(), scopes: device.scopes, autoridade: None };
    for method in EXECUTOR_METHODS {
        assert!(permit(&caller, method).is_ok());
        assert!(permit(&Caller::Local, method).is_err());
    }
    for method in ["hello", "send", "devices", "computersCall", "computerRoutineSet", "screenPrepare", "bogus"] {
        assert!(permit(&caller, method).is_err());
    }
    for scopes in [vec![], vec![Scope::Screen], vec![Scope::Executor]] {
        let key = SigningKey::from_bytes(&crypto::random());
        let dk = crypto::b64(key.verifying_key().as_bytes());
        hub.store
            .put_device(&Device {
                key: dk,
                name: "restricted".into(),
                platform: "macos".into(),
                source: DeviceSource::Local,
                grant_id: None,
                scopes,
                lease_until: None,
                created_at: now_ms(),
                last_seen_at: None,
            })
            .unwrap();
        let mut restricted = Executor::connect(&hub, key, false).await;
        for kind in ["events", "term"] {
            restricted.seq += 1;
            restricted.send(json!({"id":restricted.seq,"sub":kind,"b":{}})).await;
            assert_eq!(restricted.recv().await["err"]["status"], 403);
        }
    }
    let j = delivered(&hub, &mut d).await;
    assert!(update(&mut other, &j, "accepted").await["err"]["message"].as_str().unwrap().starts_with("unknown_job:"));
    assert!(result(&mut other, &j).await["err"].is_object());
}

#[tokio::test]
async fn calls_require_owner_main_turn_and_allowed_bot() {
    let hub = hub();
    let msg = jobs::authorize(&hub, "donna").unwrap();
    for data in [json!({"runId":"run"}), json!({"from":"other-bot"}), json!({"sourceBotId":"other"})] {
        let e = hub.store.insert_entry(&Lane::main("donna"), EntryKind::User, 1, &data).unwrap();
        hub.set_runtime("donna", |r| r.message = Some(e.id));
        assert!(
            bound_call(&hub, &body()).await["content"][0]["text"].as_str().unwrap().starts_with("not_allowed_here:")
        );
    }
    hub.set_runtime("donna", |r| {
        r.message = Some(msg);
        r.lane = Some(Lane::main("group"));
    });
    assert_eq!(bound_call(&hub, &body()).await["isError"], true);
    user_turn(&hub, None);
    hub.store.kv_set("computers.bots", "[]").unwrap();
    assert_eq!(bound_call(&hub, &body()).await["isError"], true);
    assert!(hub.store.computer_jobs().unwrap().is_empty());
}

#[tokio::test]
async fn resolution_offline_revocation_never_queue_or_fallback() {
    let hub = hub();
    let d = Executor::enroll(&hub).await;
    let other = Executor::enroll(&hub).await;
    assert!(registry::resolve(&hub, None, None).unwrap_err().to_string().starts_with("ambiguous_computer:"));
    let mut b = body();
    b["arguments"]["computer"] = json!("missing");
    assert!(bound_call(&hub, &b).await["content"][0]["text"].as_str().unwrap().starts_with("unknown_computer:"));
    let mut first = c(&hub, &d);
    first.last_seen_at = Some(now_ms() - ONLINE - 1);
    hub.store.save_computer(&first).unwrap();
    b["arguments"]["computer"] = json!(first.id);
    assert!(bound_call(&hub, &b).await["content"][0]["text"].as_str().unwrap().starts_with("offline:"));
    assert!(hub.store.computer_jobs().unwrap().is_empty());
    hub.revoke_device(&d.dk()).unwrap();
    assert_eq!(registry::resolve(&hub, None, Some(&first.id)).unwrap().id, c(&hub, &other).id);
    let mut data = json!({});
    registry::remember_source(&hub.store, &json!({"fromComputer":first.id}), &mut data);
    assert!(data["fromComputer"].is_null());
}

#[tokio::test]
async fn start_deadline_unknown_late_result_and_no_repeat() {
    let hub = hub();
    let mut d = Executor::enroll(&hub).await;
    let mut j = delivered(&hub, &mut d).await;
    assert_eq!(update(&mut d, &j, "accepted").await["ok"]["go"], true);
    j.start_by = now_ms() - 1;
    hub.store.save_computer_job(&j).unwrap();
    // Deadlines are immutable in normal writes; advance the sweep's Hub time instead.
    jobs::sweep(&hub, jobs::find(&hub, &j.id).unwrap().start_by + 1).unwrap();
    assert!(update(&mut d, &j, "start").await["err"].is_object());
    let ack = d
        .call(
            "executorResult",
            json!({"jobId":j.id,"leaseToken":j.lease_token,"outcome":"cancelled","decidedBy":"none","auditSeq":1}),
        )
        .await;
    assert_eq!(ack["ok"]["ack"], true);
    user_turn(&hub, None);
    let j = delivered(&hub, &mut d).await;
    assert_eq!(update(&mut d, &j, "accepted").await["ok"]["go"], true);
    assert_eq!(update(&mut d, &j, "start").await["ok"]["go"], true);
    jobs::sweep(&hub, j.deadline_at + 1).unwrap();
    let unknown = jobs::wait(&hub, &j.id, 0).await.unwrap();
    assert_eq!(unknown["outcome"], "unknown_outcome");
    assert_eq!(result(&mut d, &j).await["ok"]["ack"], true);
    assert_eq!(unknown["outcome"], "unknown_outcome");
    assert_eq!(jobs::response(&hub, &jobs::find(&hub, &j.id).unwrap())["outcome"], "done");
    assert!(
        jobs::create(
            &hub,
            "donna",
            &jobs::authorize(&hub, "donna").unwrap(),
            &c(&hub, &d),
            jobs::Request { kind: "exec", spec: j.spec.clone(), timeout: 60_000, reason: "teste" }
        )
        .unwrap_err()
        .to_string()
        .starts_with("duplicate_pending:")
    );
}

#[tokio::test]
async fn remote_approval_status_and_end_of_origin_cancel() {
    let hub = hub();
    let mut d = Executor::enroll(&hub).await;
    let j = delivered(&hub, &mut d).await;
    assert_eq!(j.start_by - j.created_at, LONG_APPROVAL);
    assert_eq!(update(&mut d, &j, "accepted").await["ok"]["go"], true);
    assert_eq!(jobs::wait(&hub, &j.id, 1).await.unwrap()["outcome"], "awaiting_approval");
    assert_eq!(update(&mut d, &j, "awaiting_approval").await["ok"]["go"], true);
    let mut waiting = tokio::spawn({
        let hub = hub.clone();
        let id = j.id.clone();
        async move {
            bound_call(
                &hub,
                &json!({"botId":"donna","name":"computer_job_status","arguments":{"jobId":id,"waitSeconds":240}}),
            )
            .await
        }
    });
    tokio::task::yield_now().await;
    assert_eq!(update(&mut d, &j, "start").await["ok"]["go"], true);
    assert!(tokio::time::timeout(Duration::from_millis(20), &mut waiting).await.is_err());
    assert!(jobs::wait(&hub, &j.id, 0).await.unwrap()["message"].as_str().unwrap().contains("está rodando"));
    assert_eq!(result(&mut d, &j).await["ok"]["ack"], true);
    let waited = waiting.await.unwrap();
    assert!(waited["content"][0]["text"].as_str().unwrap().contains("\"outcome\":\"done\""));
    let status = bound_call(
        &hub,
        &json!({"botId":"donna","name":"computer_job_status","arguments":{"jobId":j.id,"waitSeconds":0}}),
    )
    .await;
    assert!(status["content"][0]["text"].as_str().unwrap().contains("\"outcome\":\"done\""));
    let j = delivered(&hub, &mut d).await;
    assert_eq!(update(&mut d, &j, "accepted").await["ok"]["go"], true);
    hub.set_runtime("donna", |r| r.status = BotStatus::Idle);
    jobs::sweep(&hub, now_ms()).unwrap();
    assert_eq!(jobs::find(&hub, &j.id).unwrap().state, jobs::Phase::Cancelled);
    assert!(update(&mut d, &j, "start").await["err"].is_object());
    let poll = d
        .call("executorPoll", json!({"bootId":"boot-1","waitMs":0,"activeJob":j.id,"presence":hello()["presence"]}))
        .await;
    assert_eq!(poll["ok"]["cancel"][0], j.id);
}

#[tokio::test]
async fn enrollment_codes_are_isolated_single_use_and_hmac_bound() {
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;
    let hub = hub();
    let qr = hub.pairing.locked().issue().code;
    let code = pairing::issue(&hub)["code"].as_str().unwrap().replace('-', "");
    let proof = pairing::hello(&hub, "hello-source", &code[..4]).unwrap();
    for (b, expected) in [(&code[4..], true), ("000000000000", false)] {
        let mut mac = Hmac::<Sha256>::new_from_slice(b.as_bytes()).unwrap();
        mac.update(&hub.identity.sign_pub());
        mac.update(hub.identity.computer_id().as_bytes());
        assert_eq!(mac.verify_slice(&crypto::unb64(proof["mac"].as_str().unwrap()).unwrap()).is_ok(), expected);
    }
    assert!(!hub.pairing.locked().redeem(&code));
    let key = SigningKey::from_bytes(&crypto::random());
    let dk = crypto::b64(key.verifying_key().as_bytes());
    assert!(pairing::redeem(&hub, &dk, &json!({"code":qr,"name":"test","platform":"macos"}), "qr-source").is_err());
    let b = json!({"code":code,"name":"test","platform":"windows"});
    pairing::redeem(&hub, &dk, &b, "valid-source").unwrap();
    assert_eq!(hub.store.device(&dk).unwrap().scopes, [Scope::Executor]);
    assert!(pairing::redeem(&hub, &crypto::b64(&crypto::random::<32>()), &b, "reuse-source").is_err());
}

#[tokio::test]
async fn command_flows_over_e2e_channel_to_tool_response() {
    let hub = hub();
    let mut d = Executor::enroll(&hub).await;
    let computer = c(&hub, &d);
    user_turn(&hub, Some(&computer.id));
    let task = tokio::spawn({
        let hub = hub.clone();
        async move { bound_call(&hub, &body()).await }
    });
    let p = d.call("executorPoll", json!({"bootId":"boot-1","waitMs":1000,"presence":hello()["presence"]})).await;
    let id = p["ok"]["jobs"][0]["jobId"].as_str().unwrap();
    let j = jobs::find(&hub, id).unwrap();
    let notice = hub.store.entry(j.notice_id.as_ref().unwrap()).unwrap();
    assert!(notice.data["text"].as_str().unwrap().contains("printf ok"));
    assert_eq!(j.start_by - j.created_at, APPROVAL_BUDGET);
    assert_eq!(update(&mut d, &j, "accepted").await["ok"]["go"], true);
    assert_eq!(update(&mut d, &j, "start").await["ok"]["go"], true);
    let mut b = json!({"jobId":j.id,"leaseToken":j.lease_token,"outcome":"done","stdout":"x".repeat(MAX_STDIO + 1),"decidedBy":"click","auditSeq":1});
    assert!(d.call("executorResult", b.clone()).await["err"]["message"].as_str().unwrap().starts_with("too_large:"));
    b["stdout"] = json!("ok");
    b["data"] = json!("x".repeat(MAX_RESULT));
    assert!(d.call("executorResult", b.clone()).await["err"]["message"].as_str().unwrap().starts_with("too_large:"));
    let data = "\"\\\n".repeat(6000);
    b["data"] = json!(data);
    assert_eq!(d.call("executorResult", b).await["ok"]["ack"], true);
    assert_eq!(result(&mut d, &j).await["ok"]["ack"], true);
    let notice = hub.store.entry(j.notice_id.as_ref().unwrap()).unwrap();
    assert!(notice.data["text"].as_str().unwrap().contains("concluído · Computador de teste · printf ok"));
    let answer = tokio::time::timeout(Duration::from_secs(5), task).await.unwrap().unwrap();
    assert_eq!(answer["isError"], false);
    let text = answer["content"][0]["text"].as_str().unwrap();
    assert!(text.len() <= MAX_RESULT);
    assert!(serde_json::to_vec(&answer).unwrap().len() > MAX_RESULT);
    assert_eq!(serde_json::from_str::<Value>(text).unwrap()["data"], data);
    assert!(text.contains("\"stdout\":\"ok\""));
    assert!(!text.contains("leaseToken"));
    assert_eq!(hub.store.computer_jobs().unwrap().len(), 1);
}

#[tokio::test]
async fn redelivery_changes_lease_and_cancellation_and_denial_do_not_repeat() {
    let hub = hub();
    let mut d = Executor::enroll(&hub).await;
    let first = delivered(&hub, &mut d).await;
    jobs::sweep(&hub, first.lease_until.unwrap() + 1).unwrap();
    assert_eq!(jobs::find(&hub, &first.id).unwrap().state, jobs::Phase::Queued);
    let poll = d.call("executorPoll", json!({"bootId":"boot-1","waitMs":0,"presence":hello()["presence"]})).await;
    assert_eq!(poll["ok"]["jobs"][0]["jobId"], first.id);
    let fresh = jobs::find(&hub, &first.id).unwrap();
    assert_ne!(fresh.lease_token, first.lease_token);
    assert!(update(&mut d, &first, "accepted").await["err"]["message"].as_str().unwrap().starts_with("lease_lost:"));
    assert_eq!(update(&mut d, &fresh, "accepted").await["ok"]["go"], true);
    let denied=d.call("executorResult",json!({"jobId":fresh.id,"leaseToken":fresh.lease_token,"outcome":"denied","decidedBy":"click","auditSeq":1})).await;
    assert_eq!(denied["ok"]["ack"], true);
    let mut changed = body();
    changed["arguments"]["command"] = json!("printf another");
    assert!(bound_call(&hub, &changed).await["content"][0]["text"].as_str().unwrap().starts_with("denied_before:"));
    user_turn(&hub, None);
    let task = tokio::spawn({
        let hub = hub.clone();
        async move { bound_call(&hub, &body()).await }
    });
    let poll = d.call("executorPoll", json!({"bootId":"boot-1","waitMs":1000,"presence":hello()["presence"]})).await;
    let id = poll["ok"]["jobs"][0]["jobId"].as_str().unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(jobs::find(&hub, id).unwrap().state, jobs::Phase::Cancelled);
}

#[tokio::test]
async fn latest_poll_wins_and_reboot_never_reoffers_started_job() {
    let hub = hub();
    let mut d = Executor::enroll(&hub).await;
    let id = c(&hub, &d).id;
    let previous = hub.computers.sessions.locked()[&id].poll;
    d.seq += 1;
    d.send(json!({"id":d.seq,"m":"executorPoll","b":{"bootId":"boot-1","waitMs":1000,"presence":hello()["presence"]}}))
        .await;
    tokio::time::timeout(Duration::from_secs(1), async {
        while hub.computers.sessions.locked()[&id].poll == previous {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut second = Executor::connect(&hub, d.key.clone(), false).await;
    assert_eq!(
        second.call("executorPoll", json!({"bootId":"boot-1","waitMs":0,"presence":hello()["presence"]})).await["ok"]["jobs"],
        json!([])
    );
    assert_eq!(d.recv().await["ok"]["jobs"], json!([]));
    let j = delivered(&hub, &mut second).await;
    assert_eq!(update(&mut second, &j, "accepted").await["ok"]["go"], true);
    assert_eq!(update(&mut second, &j, "start").await["ok"]["go"], true);
    jobs::recover(&hub).unwrap();
    assert_eq!(jobs::find(&hub, &j.id).unwrap().state, jobs::Phase::Unknown);
    let mut h = hello();
    h["bootId"] = json!("boot-2");
    h["journal"] = json!([{"jobId":j.id,"state":"unknown","at":now_ms()}]);
    assert_eq!(second.call("executorHello", h).await["ok"]["reconcile"][0]["action"], "resend");
    let p = second.call("executorPoll", json!({"bootId":"boot-2","waitMs":0,"presence":hello()["presence"]})).await;
    assert_eq!(p["ok"]["jobs"], json!([]));
    assert_eq!(result(&mut second, &j).await["ok"]["ack"], true);
}

#[tokio::test]
async fn active_poll_returns_before_approval_lease_expires() {
    let hub = hub();
    let mut d = Executor::enroll(&hub).await;
    let j = delivered(&hub, &mut d).await;
    assert_eq!(update(&mut d, &j, "accepted").await["ok"]["go"], true);
    // A real pending 25 s poll must return within 12 s while its 15 s lease
    // remains valid. Run its inner RPC directly to avoid the fixture's 5 s cap.
    let caller = Caller::Device { key: d.dk(), scopes: vec![Scope::Executor], autoridade: None };
    let started = tokio::time::Instant::now();
    let poll = tokio::time::timeout(
        Duration::from_secs(12),
        api::dispatch(
            &hub,
            &caller,
            "executorPoll",
            json!({"bootId":"boot-1","activeJob":j.id,"waitMs":25_000,"presence":hello()["presence"]}),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(poll["jobs"], json!([]));
    assert!(started.elapsed() < Duration::from_secs(12));
    jobs::sweep(&hub, now_ms()).unwrap();
    assert_eq!(jobs::find(&hub, &j.id).unwrap().state, jobs::Phase::Accepted);
    assert_eq!(update(&mut d, &j, "start").await["ok"]["go"], true);
}

#[tokio::test]
async fn expiry_and_lost_unstarted_delivery_never_wedge_the_computer() {
    let hub = hub();
    let mut d = Executor::enroll(&hub).await;
    let j = delivered(&hub, &mut d).await;
    assert_eq!(update(&mut d, &j, "accepted").await["ok"]["go"], true);
    jobs::sweep(&hub, j.start_by + 1).unwrap();
    let b = json!({"jobId":j.id,"leaseToken":j.lease_token,"outcome":"expired","code":"approval_expired","decidedBy":"none","auditSeq":1});
    assert_eq!(d.call("executorResult", b.clone()).await["ok"]["ack"], true);
    assert_eq!(d.call("executorResult", b).await["ok"]["ack"], true);
    assert!(!jobs::find(&hub, &j.id).unwrap().cancel_requested);
    assert!(update(&mut d, &j, "start").await["err"].is_object());
    user_turn(&hub, None);
    let denied = delivered(&hub, &mut d).await;
    assert_eq!(update(&mut d, &denied, "accepted").await["ok"]["go"], true);
    jobs::sweep(&hub, denied.start_by + 1).unwrap();
    assert_eq!(d.call("executorResult",json!({"jobId":denied.id,"leaseToken":denied.lease_token,"outcome":"denied","decidedBy":"click","auditSeq":2})).await["ok"]["ack"],true);
    assert_eq!(jobs::find(&hub, &denied.id).unwrap().state, jobs::Phase::Denied);
    assert_eq!(jobs::response(&hub, &jobs::find(&hub, &denied.id).unwrap())["outcome"], "denied");
    let mut retry = body();
    retry["arguments"]["command"] = json!("printf changed");
    assert!(bound_call(&hub, &retry).await["content"][0]["text"].as_str().unwrap().starts_with("denied_before:"));
    assert!(update(&mut d, &denied, "start").await["err"].is_object());
    user_turn(&hub, None);
    let lost = delivered(&hub, &mut d).await;
    // Treat that poll response as lost: the helper cannot acknowledge its token.
    hub.set_runtime("donna", |r| r.status = BotStatus::Idle);
    jobs::sweep(&hub, now_ms()).unwrap();
    user_turn(&hub, None);
    let fresh = make_job(&hub, &c(&hub, &d));
    let poll = d.call("executorPoll", json!({"bootId":"boot-1","waitMs":0,"presence":hello()["presence"]})).await;
    assert_eq!(poll["ok"]["cancel"][0], lost.id);
    assert_eq!(poll["ok"]["jobs"][0]["jobId"], fresh.id);
    assert!(!jobs::find(&hub, &lost.id).unwrap().cancel_requested);
    assert!(update(&mut d, &lost, "start").await["err"].is_object());
}

#[tokio::test]
async fn cancelling_status_stops_running_but_ending_origin_does_not() {
    let hub = hub();
    let mut d = Executor::enroll(&hub).await;
    let j = delivered(&hub, &mut d).await;
    assert_eq!(jobs::wait(&hub, &j.id, 0).await.unwrap()["outcome"], "awaiting_approval");
    assert_eq!(update(&mut d, &j, "accepted").await["ok"]["go"], true);
    assert_eq!(update(&mut d, &j, "start").await["ok"]["go"], true);
    let task = tokio::spawn({
        let hub = hub.clone();
        let id = j.id.clone();
        async move {
            bound_call(
                &hub,
                &json!({"botId":"donna","name":"computer_job_status","arguments":{"jobId":id,"waitSeconds":240}}),
            )
            .await
        }
    });
    tokio::task::yield_now().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(jobs::find(&hub, &j.id).unwrap().cancel_requested);
    assert_eq!(
        d.call(
            "executorResult",
            json!({"jobId":j.id,"leaseToken":j.lease_token,"outcome":"cancelled","decidedBy":"none","auditSeq":1})
        )
        .await["ok"]["ack"],
        true
    );
    let next = delivered(&hub, &mut d).await;
    assert_eq!(update(&mut d, &next, "accepted").await["ok"]["go"], true);
    assert_eq!(update(&mut d, &next, "start").await["ok"]["go"], true);
    hub.set_runtime("donna", |r| r.status = BotStatus::Idle);
    jobs::sweep(&hub, now_ms()).unwrap();
    let running = jobs::find(&hub, &next.id).unwrap();
    assert_eq!(running.state, jobs::Phase::Running);
    assert!(!running.cancel_requested);
    assert!(jobs::response(&hub, &running)["message"].as_str().unwrap().contains("computer_job_status"));
    assert_eq!(result(&mut d, &next).await["ok"]["ack"], true);
}

#[tokio::test]
async fn lost_running_cancellation_is_retried_without_a_hot_poll_loop() {
    let hub = hub();
    let mut d = Executor::enroll(&hub).await;
    let j = delivered(&hub, &mut d).await;
    assert_eq!(update(&mut d, &j, "accepted").await["ok"]["go"], true);
    assert_eq!(update(&mut d, &j, "start").await["ok"]["go"], true);
    jobs::cancel(&hub, &j.id).unwrap();
    let b = json!({"bootId":"boot-1","waitMs":30,"activeJob":j.id,"presence":hello()["presence"]});
    d.seq += 1;
    d.send(json!({"id":d.seq,"m":"executorPoll","b":b})).await;
    tokio::time::timeout(Duration::from_secs(1), async {
        while !hub.computers.sessions.locked()[&j.computer_id].cancel_sent_at.contains_key(&j.id) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    // Drop the channel without receiving the completed poll's response.
    let key = d.key.clone();
    drop(d);
    assert!(jobs::find(&hub, &j.id).unwrap().cancel_requested);
    let mut d = Executor::connect(&hub, key, false).await;
    let mut h = hello();
    h["journal"] = json!([{"jobId":j.id,"state":"running"}]);
    assert_eq!(d.call("executorHello", h).await["ok"]["reconcile"][0]["action"], "resend");
    // Reconnection resets only the retry throttle, not the durable Stop request.
    assert_eq!(d.call("executorPoll", b.clone()).await["ok"]["cancel"], json!([j.id]));
    let started = tokio::time::Instant::now();
    assert_eq!(d.call("executorPoll", b.clone()).await["ok"]["cancel"], json!([]));
    assert!(started.elapsed() >= Duration::from_millis(30));
    let pending = jobs::find(&hub, &j.id).unwrap();
    assert_eq!(pending.state, jobs::Phase::Running);
    assert!(pending.cancel_requested);
    // Move the delivery timestamp close to its retry deadline. The pending
    // poll must wake for that deadline without relying on another Hub event.
    hub.computers
        .sessions
        .locked()
        .get_mut(&j.computer_id)
        .unwrap()
        .cancel_sent_at
        .insert(j.id.clone(), tokio::time::Instant::now() - Duration::from_millis(CANCEL_RETRY - 100));
    let mut retry = b.clone();
    retry["waitMs"] = json!(500);
    assert_eq!(d.call("executorPoll", retry).await["ok"]["cancel"], json!([j.id]));
    assert_eq!(
        d.call("executorResult", json!({"jobId":j.id,"leaseToken":j.lease_token,"outcome":"cancelled","stdout":"saída parcial","decidedBy":"none","auditSeq":1})).await["ok"]["ack"],
        true
    );
    let finished = jobs::find(&hub, &j.id).unwrap();
    assert_eq!(finished.state, jobs::Phase::Cancelled);
    assert!(!finished.cancel_requested);
    assert_eq!(finished.result.unwrap()["stdout"], "saída parcial");
    assert_eq!(d.call("executorPoll", b).await["ok"]["cancel"], json!([]));
}

#[tokio::test]
async fn stop_during_sleep_survives_hello_only_for_same_running_boot() {
    for (boot, state, expected) in [
        ("boot-1", "running", jobs::Phase::Running),
        ("boot-2", "running", jobs::Phase::Unknown),
        ("boot-1", "unknown", jobs::Phase::Unknown),
    ] {
        let hub = hub();
        let mut d = Executor::enroll(&hub).await;
        let j = delivered(&hub, &mut d).await;
        assert_eq!(update(&mut d, &j, "accepted").await["ok"]["go"], true);
        assert_eq!(update(&mut d, &j, "start").await["ok"]["go"], true);
        assert!(d.call("executorBye", json!({"reason":"sleep"})).await["ok"].is_object());
        assert!(!hub.computers.sessions.locked().contains_key(&j.computer_id));
        assert_eq!(c(&hub, &d).last_boot_id.as_deref(), Some("boot-1"));
        jobs::cancel(&hub, &j.id).unwrap();
        assert!(jobs::find(&hub, &j.id).unwrap().cancel_requested);

        let mut h = hello();
        h["bootId"] = json!(boot);
        h["journal"] = json!([{"jobId":j.id,"state":state}]);
        assert_eq!(d.call("executorHello", h).await["ok"]["reconcile"][0]["action"], "resend");
        assert_eq!(c(&hub, &d).last_boot_id.as_deref(), Some(boot));
        let resumed = jobs::find(&hub, &j.id).unwrap();
        assert_eq!(resumed.state, expected, "{boot}/{state}");
        assert_eq!(resumed.cancel_requested, expected == jobs::Phase::Running);
        let poll = json!({"bootId":boot,"waitMs":0,"activeJob":j.id,"presence":hello()["presence"]});
        let cancel = if expected == jobs::Phase::Running { json!([j.id]) } else { json!([]) };
        assert_eq!(d.call("executorPoll", poll.clone()).await["ok"]["cancel"], cancel);
        assert_eq!(
            d.call(
                "executorResult",
                json!({"jobId":j.id,"leaseToken":j.lease_token,"outcome":"cancelled","decidedBy":"none","auditSeq":1})
            )
            .await["ok"]["ack"],
            true
        );
        assert_eq!(jobs::find(&hub, &j.id).unwrap().state, jobs::Phase::Cancelled);
        assert_eq!(d.call("executorPoll", poll).await["ok"]["cancel"], json!([]));
    }
}

#[tokio::test]
async fn reconnect_resends_started_results_even_after_stop() {
    for (boot, state) in [("boot-1", "done"), ("boot-1", "running"), ("boot-2", "done")] {
        let hub = hub();
        let mut d = Executor::enroll(&hub).await;
        let j = delivered(&hub, &mut d).await;
        assert_eq!(update(&mut d, &j, "accepted").await["ok"]["go"], true);
        assert_eq!(update(&mut d, &j, "start").await["ok"]["go"], true);
        jobs::cancel(&hub, &j.id).unwrap();
        let mut h = hello();
        h["bootId"] = json!(boot);
        h["journal"] = json!([{"jobId":j.id,"state":state}]);
        assert_eq!(d.call("executorHello", h.clone()).await["ok"]["reconcile"][0]["action"], "resend");
        if boot == "boot-1" {
            assert_eq!(jobs::find(&hub, &j.id).unwrap().state, jobs::Phase::Running);
            assert!(jobs::find(&hub, &j.id).unwrap().cancel_requested);
        }
        // An actual result already in the outbox takes precedence over Stop.
        assert_eq!(result(&mut d, &j).await["ok"]["ack"], true);
        let finished = jobs::find(&hub, &j.id).unwrap();
        assert_eq!(finished.state, jobs::Phase::Done);
        assert_eq!(finished.result.unwrap()["stdout"], "ok");
        assert!(!finished.cancel_requested);
        assert_eq!(d.call("executorHello", h).await["ok"]["reconcile"][0]["action"], "forget");
        user_turn(&hub, None);
        let _fresh = make_job(&hub, &c(&hub, &d));
    }
}

#[tokio::test]
async fn recovery_preserves_unknown_results_and_notices() {
    let hub = hub();
    let mut d = Executor::enroll(&hub).await;
    let j = delivered(&hub, &mut d).await;
    assert_eq!(update(&mut d, &j, "accepted").await["ok"]["go"], true);
    assert_eq!(update(&mut d, &j, "start").await["ok"]["go"], true);
    jobs::recover(&hub).unwrap();
    assert_eq!(
        d.call("executorResult", json!({"jobId":j.id,"leaseToken":j.lease_token,"outcome":"unknown","code":"executor_restarted","decidedBy":"none","auditSeq":1})).await["ok"]["ack"],
        true
    );
    let unknown = jobs::find(&hub, &j.id).unwrap();
    let notice_id = unknown.notice_id.as_ref().unwrap();
    let notice = hub.store.entry(notice_id).unwrap();
    for legacy in [false, true, false] {
        if legacy {
            let mut j = unknown.clone();
            j.finished_at = None;
            j.cancel_requested = true;
            hub.store.save_computer_job(&j).unwrap();
        }
        jobs::recover(&hub).unwrap();
        let recovered = jobs::find(&hub, &j.id).unwrap();
        assert!(recovered.finished_at.is_some());
        assert!(!recovered.cancel_requested);
        assert_eq!(recovered.result, unknown.result);
        assert_eq!(jobs::response(&hub, &recovered)["code"], "executor_restarted");
        let after = hub.store.entry(notice_id).unwrap();
        assert_eq!(after.rev, notice.rev);
        assert_eq!(after.updated_at, notice.updated_at);
        assert_eq!(after.data, notice.data);
        let mut h = hello();
        h["bootId"] = json!("boot-2");
        h["journal"] = json!([{"jobId":j.id,"state":"unknown"}]);
        assert_eq!(d.call("executorHello", h).await["ok"]["reconcile"][0]["action"], "forget");
        assert_eq!(hub.store.entry(notice_id).unwrap().rev, notice.rev);
        assert_eq!(jobs::response(&hub, &jobs::find(&hub, &j.id).unwrap())["code"], "executor_restarted");
    }
}

#[tokio::test]
async fn cancelled_running_jobs_become_unknown_without_wedging_or_retention_leaks() {
    for route in ["deadline", "recover", "hello"] {
        let hub = hub();
        let mut d = Executor::enroll(&hub).await;
        let j = delivered(&hub, &mut d).await;
        assert_eq!(update(&mut d, &j, "accepted").await["ok"]["go"], true);
        assert_eq!(update(&mut d, &j, "start").await["ok"]["go"], true);
        jobs::cancel(&hub, &j.id).unwrap();
        let boot = match route {
            "deadline" => {
                jobs::sweep(&hub, j.deadline_at + 1).unwrap();
                "boot-1"
            }
            "recover" => {
                jobs::recover(&hub).unwrap();
                "boot-1"
            }
            _ => {
                let mut b = hello();
                b["bootId"] = json!("boot-2");
                b["journal"] = json!([{"jobId":j.id,"state":"unknown"}]);
                assert_eq!(d.call("executorHello", b.clone()).await["ok"]["reconcile"][0]["action"], "resend");
                let finished = jobs::find(&hub, &j.id).unwrap().finished_at;
                d.call("executorHello", b).await;
                assert_eq!(jobs::find(&hub, &j.id).unwrap().finished_at, finished);
                "boot-2"
            }
        };
        jobs::cancel(&hub, &j.id).unwrap();
        let unknown = jobs::find(&hub, &j.id).unwrap();
        assert_eq!(unknown.state, jobs::Phase::Unknown, "{route}");
        assert!(!unknown.cancel_requested, "{route}");
        let finished = unknown.finished_at.unwrap();
        let p = d.call("executorPoll", json!({"bootId":boot,"waitMs":0,"presence":hello()["presence"]})).await;
        assert_eq!(p["ok"]["cancel"], json!([]), "{route}");
        user_turn(&hub, None);
        let _fresh = make_job(&hub, &c(&hub, &d));
        hub.store.clean_computer_jobs(finished + 86_400_001).unwrap();
        let cleaned = jobs::find(&hub, &j.id).unwrap();
        assert_eq!(cleaned.spec, json!({}), "{route}");
        assert_eq!(cleaned.origin, json!({}), "{route}");
        hub.store.clean_computer_jobs(finished + 30 * 86_400_000 + 1).unwrap();
        assert!(jobs::find(&hub, &j.id).is_err(), "{route}");
    }
}

#[tokio::test]
async fn a_late_result_after_retention_keeps_the_original_finish_time_and_no_result() {
    let hub = hub();
    let mut d = Executor::enroll(&hub).await;
    let j = delivered(&hub, &mut d).await;
    assert_eq!(update(&mut d, &j, "accepted").await["ok"]["go"], true);
    assert_eq!(update(&mut d, &j, "start").await["ok"]["go"], true);
    jobs::recover(&hub).unwrap();
    let unknown = jobs::find(&hub, &j.id).unwrap();
    assert_eq!(unknown.state, jobs::Phase::Unknown);
    let body = json!({"jobId":j.id,"leaseToken":j.lease_token,"outcome":"unknown","code":"executor_restarted","decidedBy":"none","auditSeq":1});
    assert_eq!(d.call("executorResult", body.clone()).await["ok"]["ack"], true);
    assert_eq!(jobs::find(&hub, &j.id).unwrap().finished_at, unknown.finished_at);
    let mut aged = unknown.clone();
    aged.finished_at = Some(now_ms() - 2 * 86_400_000);
    hub.store.save_computer_job(&aged).unwrap();
    hub.store.clean_computer_jobs(now_ms()).unwrap();
    assert!(jobs::find(&hub, &j.id).unwrap().result.is_none());
    let late = d.call("executorResult", body).await;
    assert_eq!(late["ok"]["ack"], true, "{late}");
    let after = jobs::find(&hub, &j.id).unwrap();
    assert_eq!(after.finished_at, aged.finished_at);
    assert!(after.result.is_none());
}

#[tokio::test]
async fn revogacao_antes_do_commit_do_start_nao_grava_running_nem_ultimo_uso() {
    let hub = hub();
    let d = Executor::enroll(&hub).await;
    let mut computer = c(&hub, &d);
    let previous = computer.last_used_at;
    let mut job = make_job(&hub, &computer);
    let binding = crate::fork::sessoes::bindings::fixture_binding(&hub.store, "donna");
    job.origin["sessionBinding"] = json!(binding.id.as_str());
    let mut public_origin = job.origin.clone();
    public_origin.as_object_mut().unwrap().remove("sessionBinding");
    assert_eq!(jobs::wire(&job, now_ms()).unwrap()["origin"], public_origin);
    assert_eq!(job.origin["sessionBinding"], binding.id.as_str());
    job.state = jobs::Phase::Running;
    job.started_at = Some(now_ms());
    computer.last_used_at = job.started_at;
    crate::fork::sessoes::bindings::revoke(&hub.store, "donna", Some("fixture-acp")).unwrap();
    assert!(hub.store.save_computer_job_in_execution(&job, None, Some(&computer)).is_err());
    assert_eq!(jobs::find(&hub, &job.id).unwrap().state, jobs::Phase::Queued);
    assert_eq!(c(&hub, &d).last_used_at, previous);
}

fn sheet_args(filename: &str) -> Value {
    json!({"location":"desktop","folder":"X","filename":filename,"reason":"teste","sheets":[{"name":"A","rows":[["a",1]]}]})
}
fn announce(capabilities: Value) -> Value {
    let mut h = hello();
    h["capabilities"] = capabilities;
    h
}

#[tokio::test]
async fn spreadsheet_needs_the_announced_capability_and_nothing_else_is_reachable_through_it() {
    let hub = hub();
    let mut d = Executor::enroll(&hub).await;
    let computer = c(&hub, &d);
    user_turn(&hub, Some(&computer.id));
    // Anunciou só terminal/arquivos: planilha não é enfileirada.
    let answer =
        bound_call(&hub, &json!({"botId":"donna","name":"create_spreadsheet","arguments":sheet_args("V.xlsx")})).await;
    assert!(
        answer["isError"] == true && answer["content"][0]["text"].as_str().unwrap().contains("capability_missing"),
        "{answer}"
    );
    // Anunciou só planilha: comando, leitura, listagem e gravação genérica ficam fora do alcance, sem fila.
    assert!(d.call("executorHello", announce(json!({"planilha":true,"locais":["desktop"]}))).await["ok"].is_object());
    for body in [
        json!({"botId":"donna","name":"run_command","arguments":{"command":"ls","reason":"t"}}),
        json!({"botId":"donna","name":"read_file","arguments":{"path":"~/x"}}),
        json!({"botId":"donna","name":"list_dir","arguments":{"path":"~/"}}),
        json!({"botId":"donna","name":"write_file","arguments":{"path":"~/x","mode":"overwrite","content":"x","reason":"t"}}),
        json!({"botId":"donna","name":"create_spreadsheet","arguments":{"location":"documents","filename":"V.xlsx","reason":"t","sheets":[{"name":"A","rows":[]}]}}),
        json!({"botId":"donna","name":"create_spreadsheet","arguments":{"location":"desktop","filename":"../V.xlsx","reason":"t","sheets":[{"name":"A","rows":[]}]}}),
        json!({"botId":"donna","name":"create_spreadsheet","arguments":{"location":"desktop","folder":"a/b","filename":"V.xlsx","reason":"t","sheets":[{"name":"A","rows":[]}]}}),
        json!({"botId":"donna","name":"create_spreadsheet","arguments":{"location":"desktop","path":"/etc/passwd","filename":"V.xlsx","reason":"t","sheets":[{"name":"A","rows":[]}]}}),
    ] {
        let answer = bound_call(&hub, &body).await;
        assert_eq!(answer["isError"], true, "{body}: {answer}");
    }
    assert!(hub.store.computer_jobs().unwrap().is_empty());
    // Local não anunciado também não passa.
    assert!(d.call("executorHello", announce(json!({"planilha":true,"locais":[]}))).await["ok"].is_object());
    let answer =
        bound_call(&hub, &json!({"botId":"donna","name":"create_spreadsheet","arguments":sheet_args("V.xlsx")})).await;
    assert!(answer["content"][0]["text"].as_str().unwrap().contains("capability_missing"), "{answer}");
    assert!(hub.store.computer_jobs().unwrap().is_empty());
}

#[tokio::test]
async fn spreadsheet_is_done_only_with_a_receipt_that_repeats_what_the_hub_sent() {
    let hub = hub();
    let mut d = Executor::enroll(&hub).await;
    assert!(d.call("executorHello", announce(json!({"planilha":true,"locais":["desktop"]}))).await["ok"].is_object());
    let computer = c(&hub, &d);
    user_turn(&hub, Some(&computer.id));
    for (filename, receipt_ok) in [("Falso.xlsx", false), ("Certo.xlsx", true)] {
        let task = tokio::spawn({
            let hub = hub.clone();
            async move {
                bound_call(&hub, &json!({"botId":"donna","name":"create_spreadsheet","arguments":sheet_args(filename)}))
                    .await
            }
        });
        let p = d.call("executorPoll", json!({"bootId":"boot-1","waitMs":1000,"presence":hello()["presence"]})).await;
        let j = jobs::find(&hub, p["ok"]["jobs"][0]["jobId"].as_str().unwrap()).unwrap();
        assert_eq!(
            (j.kind.as_str(), j.spec["purpose"].clone(), j.spec["mode"].clone(), j.spec["path"].clone()),
            ("fs.write", json!("spreadsheet"), json!("create"), json!(filename))
        );
        assert_eq!((j.spec["location"].clone(), j.spec["folder"].clone()), (json!("desktop"), json!("X")));
        let content =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, j.spec["content"].as_str().unwrap())
                .unwrap();
        assert_eq!(&content[..2], b"PK");
        assert_eq!(update(&mut d, &j, "accepted").await["ok"]["go"], true);
        assert_eq!(update(&mut d, &j, "start").await["ok"]["go"], true);
        let mut receipt = json!({"path":format!("/Users/user/Desktop/X/{filename}"),"location":"desktop","folder":"X","created":true,"reopened":true,
            "size":j.spec["size"],"sha256":j.spec["sha256"],"contentSha256":j.spec["contentSha256"],"sheets":j.spec["summary"]});
        if !receipt_ok {
            receipt["sha256"] = json!("0".repeat(64));
        }
        let r = d.call("executorResult", json!({"jobId":j.id,"leaseToken":j.lease_token,"outcome":"done","decidedBy":"policy","auditSeq":1,"receipt":receipt})).await;
        assert_eq!(r["ok"]["ack"], true);
        let answer = tokio::time::timeout(Duration::from_secs(5), task).await.unwrap().unwrap();
        let text = answer["content"][0]["text"].as_str().unwrap();
        if receipt_ok {
            assert_eq!(answer["isError"], false, "{answer}");
            assert!(
                text.contains("\"outcome\":\"done\"") && text.contains("pasta liberada") && text.contains("Certo.xlsx"),
                "{text}"
            );
        } else {
            assert_eq!(answer["isError"], true);
            assert!(text.starts_with("receipt_invalid") && text.contains("unknown_outcome"), "{text}");
            assert_eq!(jobs::find(&hub, &j.id).unwrap().state, jobs::Phase::Unknown);
        }
    }
}
