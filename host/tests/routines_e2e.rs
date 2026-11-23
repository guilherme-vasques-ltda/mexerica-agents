// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Local routine lifecycle tests with real host processes and persistent fake ACP
//! sessions. No paid provider is involved.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(dead_code)]
mod common;

use common::workspaces::Workspaces;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct Host {
    child: Child,
    home: PathBuf,
    workspaces: Workspaces,
    base: String,
    token: String,
}

impl Drop for Host {
    fn drop(&mut self) {
        common::stop(&mut self.child);
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

impl Host {
    async fn start() -> Self {
        let home = std::env::temp_dir().join(format!("codync-routines-e2e-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        let workspaces = Workspaces::new(&home);
        let (child, base) = common::serve(&home).await;
        let mut host = Self { child, home, workspaces, base, token: String::new() };
        std::fs::read_to_string(host.home.join("token")).unwrap().trim().clone_into(&mut host.token);
        host
    }

    async fn restart(&mut self) {
        common::stop(&mut self.child);
        (self.child, self.base) = common::serve(&self.home).await;
    }

    async fn call(&self, method: &str, body: Value) -> Value {
        let response = reqwest::Client::new()
            .post(format!("{}/api/{method}", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let value: Value = response.json().await.unwrap();
        assert!(status.is_success(), "{method}: {value}");
        value
    }

    async fn bot(&self, name: &str) -> String {
        self.bot_fixture(name, "team_agent.py").await
    }

    async fn stable_bot(&self, name: &str) -> String {
        self.bot_fixture(name, "routine_agent.py").await
    }

    async fn bot_fixture(&self, name: &str, fixture: &str) -> String {
        let cwd = self.workspaces.root().join(name);
        std::fs::create_dir_all(&cwd).unwrap();
        let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(fixture);
        self.call(
            "createBot",
            json!({
                "name": name, "backend": "custom", "cwd": cwd, "permission": "ask", "notify": false,
                "command": common::python_agent(&agent),
            }),
        )
        .await["bot"]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }
    async fn save(&self, bot: &str, name: &str, instruction: &str, triggers: Value, timeout: u64) -> String {
        self.call(
            "saveRoutine",
            json!({"botId":bot,"name":name,"instruction":instruction,"triggers":triggers,"timeoutSeconds":timeout}),
        )
        .await["routine"]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn wait_status(&self, bot: &str, routine: &str, status: &str) -> Value {
        self.wait_run_until(bot, routine, |run| run["status"] == status).await
    }

    async fn wait_run_until(&self, bot: &str, routine: &str, condition: impl Fn(&Value) -> bool) -> Value {
        tokio::time::timeout(Duration::from_secs(25), async {
            loop {
                let listing = self.call("routines", json!({"botId":bot})).await;
                if let Some(run) =
                    listing["runs"].as_array().unwrap().iter().find(|r| r["routineId"] == routine && condition(r))
                {
                    return run.clone();
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("a rotina {routine} não chegou ao estado pedido"))
    }

    async fn wait_history_until(&self, bot: &str, condition: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        // Mesmo polling de publicação antes local ao teste de crash: 10 s/50 ms.
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let history = self.call("history", json!({"botId":bot})).await;
                let entries = history["entries"].as_array().unwrap();
                if condition(entries) {
                    return entries.clone();
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("o histórico não publicou a evidência pedida")
    }

    fn agent_events(&self, name: &str) -> Vec<Value> {
        std::fs::read_to_string(self.workspaces.root().join(name).join("events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    async fn wait_event(&self, name: &str, event: &str, contains: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(25), async {
            loop {
                if let Some(e) = self
                    .agent_events(name)
                    .into_iter()
                    .find(|e| e["event"] == event && e["text"].as_str().unwrap().contains(contains))
                {
                    return e;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("missing agent event {event} containing {contains}"))
    }

    fn permitted_dispatch(&self, bot: &str) -> Value {
        let db = rusqlite::Connection::open(self.home.join("codync.db")).unwrap();
        let raw: String =
            db.query_row("SELECT v FROM kv WHERE k=?1", [format!("turn.inflight.{bot}")], |r| r.get(0)).unwrap();
        let dispatch: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(dispatch["admissionVersion"], 1);
        assert_eq!(dispatch["phase"], "permitted");
        assert_ne!(dispatch["executionToken"].as_str().unwrap(), "");
        assert_ne!(dispatch["attempt"].as_str().unwrap(), "");
        let waiting: bool = db
            .query_row("SELECT EXISTS(SELECT 1 FROM kv WHERE k=?1)", [format!("turn.waiting.{bot}")], |r| r.get(0))
            .unwrap();
        assert!(!waiting, "este crash não tem checkpoint D72 para autorizar retomada");
        dispatch
    }

    fn assert_no_replay(&self, name: &str, text: &str) {
        let events = self.agent_events(name);
        let deliveries: Vec<_> = events.iter().filter(|e| e["text"].as_str().unwrap().contains(text)).collect();
        assert_eq!(deliveries.len(), 1, "só o início original pode existir: {events:?}");
        assert_eq!(deliveries[0]["event"], "started");
    }

    async fn assert_interruption_notice(&self, bot: &str, id: &str) {
        let entries = self.wait_history_until(bot, |entries| entries.iter().any(|e| e["id"] == id)).await;
        let notices: Vec<_> = entries
            .iter()
            .filter(|e| {
                e["kind"] == "notice"
                    && e["data"]["text"].as_str().is_some_and(|text| text.contains("efeito já realizado"))
            })
            .collect();
        assert_eq!(notices.len(), 1, "o aviso durável precisa ser único: {entries:?}");
        assert_eq!(notices[0]["id"], id);
    }

    /// Contrato da pausa: depois de um reinício com efeito incerto, nada novo sai desta conversa (a mensagem
    /// `held`, quando existe, fica `queued`) até o dono liberá-la; liberar não repete nada.
    async fn assert_paused_then_release(&self, bot: &str, name: &str, held: &str, message: bool) {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert!(
            !self.agent_events(name).iter().any(|e| e["text"].as_str().unwrap().contains(held)),
            "{held} saiu da conversa pausada"
        );
        if message {
            let history = self.call("history", json!({"botId":bot})).await;
            let entry = history["entries"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["kind"] == "user" && e["data"]["text"] == held)
                .cloned()
                .unwrap();
            assert_eq!(entry["data"]["status"], "queued", "a mensagem nova espera a liberação");
        }
        let released = self.call("releaseConversation", json!({"botId":bot})).await;
        assert_eq!(released["released"], json!([bot]));
    }

    async fn send(&self, bot: &str, text: &str) {
        // Como o app: toda mensagem traz a origem digitada; sem ela o Hub trata o texto como desconhecido.
        let body = json!({"botId":bot,"text":text,"contentOrigin":message_origin(text, false),"clientNonce":uuid::Uuid::new_v4().to_string()});
        self.call("send", body).await;
    }
}

#[tokio::test]
async fn routines_execute_pause_and_refuse_webhooks_without_chain_authority() {
    let mut host = Host::start().await;
    let bot = host.bot("routine-bot").await;
    let result = host.call("saveRoutine", json!({"botId":bot,"name":"Check status","instruction":"Report a short status","triggers":[{"type":"webhook"}]})).await;
    let id = result["routine"]["id"].as_str().unwrap();
    assert!(result["routine"].get("webhookKey").is_none());
    let credentials = host.call("routineWebhook", json!({"botId":bot,"id":id})).await;
    assert!(credentials["url"].is_null(), "no public URL while the cloud is off");
    let url = credentials["localUrl"].as_str().unwrap();
    let client = reqwest::Client::new();
    assert!(!client.post(url).bearer_auth("wrong").json(&json!({})).send().await.unwrap().status().is_success());
    for _ in 0..2 {
        let blocked = client
            .post(url)
            .bearer_auth(credentials["key"].as_str().unwrap())
            .header("x-delivery-id", "delivery-1")
            .json(&json!({"text":"hello", "chainId":"forged"}))
            .send()
            .await
            .unwrap();
        assert_eq!(blocked.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(blocked.json::<Value>().await.unwrap()["error"], "webhook blocked: Hub chain control unavailable");
    }
    assert_eq!(host.call("routines", json!({"botId":bot})).await["runs"].as_array().unwrap().len(), 0);
    let history = host.call("history", json!({"botId":bot})).await;
    assert_eq!(
        history["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["data"]["text"] == "Webhook bloqueado: controle de cadeia do Hub indisponível.")
            .count(),
        1
    );
    // Explicit owner-requested execution remains available independent of webhook admission.
    host.call("runRoutine", json!({"botId":bot,"id":id})).await;
    let db = rusqlite::Connection::open(host.home.join("codync.db")).unwrap();
    let run_sessions = || -> i64 {
        db.query_row("SELECT COUNT(*) FROM kv WHERE k GLOB 'lane.session.*/routine-*'", [], |r| r.get(0)).unwrap()
    };
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let listing = host.call("routines", json!({"botId":bot})).await;
            // A finished run's session is forgotten.
            if listing["runs"][0]["status"] == "succeeded" && run_sessions() == 0 {
                break;
            }
            assert_ne!(listing["runs"][0]["status"], "failed", "{listing}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    let servers: Value = serde_json::from_str(
        &std::fs::read_to_string(host.workspaces.root().join("routine-bot/servers.json")).unwrap(),
    )
    .unwrap();
    assert!(servers.as_array().unwrap().iter().any(|s| s["name"] == "routines" || s["name"] == "codync-routines"));
    host.call("setRoutineEnabled", json!({"botId":bot,"id":id,"enabled":false})).await;
    assert!(
        !client
            .post(url)
            .bearer_auth(credentials["key"].as_str().unwrap())
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    host.restart().await;
    let restored = host.call("routines", json!({"botId":bot})).await;
    assert_eq!(restored["routines"][0]["enabled"], false);
    assert_eq!(restored["runs"][0]["status"], "succeeded");
    host.call("deleteRoutine", json!({"botId":bot,"id":id})).await;
    assert_eq!(host.call("routines", json!({"botId":bot})).await["routines"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn interval_fires_without_a_client_and_invalid_schedules_fail() {
    let host = Host::start().await;
    let bot = host.bot("scheduled-bot").await;
    let response = reqwest::Client::new()
        .post(format!("{}/api/saveRoutine", host.base))
        .bearer_auth(&host.token)
        .json(&json!({"botId":bot,"name":"Bad","instruction":"test","triggers":[{"type":"interval","seconds":0}]}))
        .send()
        .await
        .unwrap();
    assert!(!response.status().is_success());
    let saved = host
        .call(
            "saveRoutine",
            json!({"botId":bot,"name":"Scheduled","instruction":"EMPTY","triggers":[{"type":"interval","seconds":1}]}),
        )
        .await;
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let listing = host.call("routines", json!({"botId":bot})).await;
            if listing["runs"].as_array().unwrap().iter().any(|r| r["status"] == "succeeded") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    host.call("setRoutineEnabled", json!({"botId":bot,"id":saved["routine"]["id"],"enabled":false})).await;
    let history = host.call("history", json!({"botId":bot})).await;
    assert!(!history["entries"].as_array().unwrap().iter().any(|e| e["kind"] == "agent" && e["data"]["final"] == true));
}

#[tokio::test]
async fn crash_apos_permissao_interrompe_sem_replay_e_publica_um_aviso() {
    let mut host = Host::start().await;
    let bot = host.stable_bot("resume").await;
    let id = host.save(&bot, "resume-routine", "BLOCK[resume]", json!([{"type":"webhook"}]), 30).await;
    host.call("runRoutine", json!({"botId":bot,"id":id})).await;
    let started = host.wait_event("resume", "started", "BLOCK[resume]").await;
    let running = host.wait_status(&bot, &id, "running").await;
    let dispatch = host.permitted_dispatch(&bot);
    assert_eq!(dispatch["origin"]["kind"], "routine");
    assert_eq!(dispatch["origin"]["id"], running["id"]);
    assert!(running.get("_chain").is_none(), "metadados internos não saem pelo RPC");
    let run_id = running["id"].as_str().unwrap();
    let db = rusqlite::Connection::open(host.home.join("codync.db")).unwrap();
    let original_turn: i64 =
        db.query_row("SELECT turn FROM entries WHERE id=?1", [format!("routine-{run_id}")], |r| r.get(0)).unwrap();
    let original_chain: String =
        db.query_row("SELECT id FROM donna_chain_roots WHERE run_id=?1 AND used=1", [run_id], |r| r.get(0)).unwrap();
    drop(db);
    host.restart().await;
    let interrupted =
        host.wait_run_until(&bot, &id, |run| run["status"] == "interrupted" && run["reportPending"] == false).await;
    assert_eq!(interrupted["id"], running["id"]);
    let db = rusqlite::Connection::open(host.home.join("codync.db")).unwrap();
    let used: i64 =
        db.query_row("SELECT used FROM donna_chain_roots WHERE id=?1", [&original_chain], |r| r.get(0)).unwrap();
    let same_turn: i64 =
        db.query_row("SELECT turn FROM entries WHERE id=?1", [format!("routine-{run_id}")], |r| r.get(0)).unwrap();
    assert_eq!(same_turn, original_turn, "interromper conserva o turno do notice");
    assert_eq!(used, 1, "interromper sem IA não consome outra tentativa da cadeia");
    drop(db);
    assert!(interrupted["finishedAt"].is_i64());
    assert!(interrupted["event"].is_null());
    let notice = format!("routine-result-{}", running["id"].as_str().unwrap());
    host.assert_interruption_notice(&bot, &notice).await;
    host.assert_no_replay("resume", "BLOCK[resume]");
    let history = host.call("history", json!({"botId":bot})).await;
    assert!(!history["entries"].as_array().unwrap().iter().any(|e| e["kind"] == "agent" && e["data"]["final"] == true));
    // Não soltar o BLOCK: replay indevido bloquearia a próxima entrada.
    host.send(&bot, "BARRIER").await;
    host.assert_paused_then_release(&bot, "resume", "BARRIER", true).await;
    let barrier = host.wait_event("resume", "finished", "BARRIER").await;
    assert_ne!(started["session"], barrier["session"], "a rotina não vira sessão principal");
    let entries = host
        .wait_history_until(&bot, |entries| entries.iter().any(|e| e["kind"] == "agent" && e["data"]["final"] == true))
        .await;
    assert_eq!(entries.iter().filter(|e| e["kind"] == "agent" && e["data"]["final"] == true).count(), 1);
    host.restart().await;
    host.send(&bot, "BARREIRA_FINAL").await;
    host.wait_event("resume", "finished", "BARREIRA_FINAL").await;
    host.assert_interruption_notice(&bot, &notice).await;
    host.assert_no_replay("resume", "BLOCK[resume]");
    let listing = host.call("routines", json!({"botId":bot})).await;
    assert_eq!(listing["runs"].as_array().unwrap().len(), 1);
    assert_eq!(listing["runs"][0]["status"], "interrupted");
    let entries = host
        .wait_history_until(&bot, |entries| {
            entries.iter().filter(|e| e["kind"] == "agent" && e["data"]["final"] == true).count() >= 2
        })
        .await;
    assert_eq!(
        entries.iter().filter(|e| e["kind"] == "agent" && e["data"]["final"] == true).count(),
        2,
        "só os dois pedidos posteriores publicam resultado"
    );
}

#[tokio::test]
async fn rotina_no_limite_nao_envia_outro_prompt_apos_reinicio() {
    let mut host = Host::start().await;
    let bot = host.stable_bot("limite-cadeia").await;
    let id = host.save(&bot, "limite", "BLOCK[limite]", json!([{"type":"webhook"}]), 30).await;
    host.call("runRoutine", json!({"botId":bot,"id":id})).await;
    host.wait_event("limite-cadeia", "started", "BLOCK[limite]").await;
    let run = host.wait_status(&bot, &id, "running").await;
    // Dados fictícios: orçamento já esgotado. Permitted incerto não pode consumir
    // continuação nem repetir IA; o limite numérico segue coberto pelo produtor #245.
    common::stop(&mut host.child);
    let db = rusqlite::Connection::open(host.home.join("codync.db")).unwrap();
    db.execute("UPDATE donna_chain_roots SET used=50 WHERE run_id=?1", [run["id"].as_str().unwrap()]).unwrap();
    drop(db);
    (host.child, host.base) = common::serve(&host.home).await;
    let interrupted = host.wait_status(&bot, &id, "interrupted").await;
    assert_eq!(interrupted["id"], run["id"]);
    host.assert_no_replay("limite-cadeia", "BLOCK[limite]");
    assert_eq!(host.agent_events("limite-cadeia").iter().filter(|e| e["event"] == "resumed").count(), 0);
    let db = rusqlite::Connection::open(host.home.join("codync.db")).unwrap();
    let used: i64 = db
        .query_row("SELECT used FROM donna_chain_roots WHERE run_id=?1", [run["id"].as_str().unwrap()], |r| r.get(0))
        .unwrap();
    assert_eq!(used, 50);
}

#[tokio::test]
async fn a_message_sent_during_a_run_gets_a_turn_of_its_own() {
    let host = Host::start().await;
    let bot = host.stable_bot("turns").await;
    let id = host.save(&bot, "slow", "BLOCK[run]", json!([{"type":"webhook"}]), 30).await;
    host.call("runRoutine", json!({"botId":bot,"id":id})).await;
    // Nothing of the run is in the chat yet but its notice.
    host.wait_event("turns", "started", "BLOCK[run]").await;
    host.send(&bot, "MEANWHILE").await;
    std::fs::write(host.workspaces.root().join("turns/release-run"), "").unwrap();
    let entries = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let history = host.call("history", json!({"botId":bot})).await;
            let entries = history["entries"].as_array().unwrap().clone();
            if entries.iter().filter(|e| e["kind"] == "agent" && e["data"]["final"] == true).count() == 2 {
                return entries;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let turn =
        |f: &dyn Fn(&Value) -> bool| entries.iter().filter(|e| f(e)).map(|e| e["turn"].clone()).collect::<Vec<_>>();
    let notice = turn(&|e| e["kind"] == "notice" && e["data"]["runId"].is_string());
    let user = turn(&|e| e["kind"] == "user");
    assert_ne!(notice, user);
    let finals = turn(&|e| e["kind"] == "agent" && e["data"]["final"] == true);
    assert_eq!(finals, [notice[0].clone(), user[0].clone()], "each reply stays in its own turn");
}

#[tokio::test]
async fn an_agent_started_for_a_run_stops_when_the_run_ends() {
    let host = Host::start().await;
    let bot = host.stable_bot("only-runs").await;
    let id = host.save(&bot, "one-off", "ONE_OFF", json!([{"type":"webhook"}]), 30).await;
    host.call("runRoutine", json!({"botId":bot,"id":id})).await;
    // The run's outcome, not the agent's own "finished": a message sent before the host took the reply would wait in
    // the queue and keep the agent running.
    host.wait_status(&bot, &id, "succeeded").await;
    host.send(&bot, "LATER").await;
    host.wait_event("only-runs", "finished", "LATER").await;
    let events = host.agent_events("only-runs");
    let count = |event: &str| events.iter().filter(|e| e["event"] == event).count();
    // The message started the agent again: the run's agent stopped with the run. Windows never stops an agent (it
    // can't tell a dev server from an MCP server, see `lifecycle`), so there the run's session closes instead.
    assert_eq!((count("initialize"), count("closed")), if cfg!(windows) { (1, 1) } else { (2, 0) });
}

#[tokio::test]
async fn a_finished_run_closes_its_session_and_the_main_one_stays() {
    let host = Host::start().await;
    let bot = host.stable_bot("spent").await;
    host.send(&bot, "MAIN_CHAT").await;
    let main = host.wait_event("spent", "finished", "MAIN_CHAT").await;
    let id = host.save(&bot, "one-off", "ONE_OFF", json!([{"type":"webhook"}]), 30).await;
    host.call("runRoutine", json!({"botId":bot,"id":id})).await;
    let run = host.wait_event("spent", "finished", "ONE_OFF").await;
    let closed = host.wait_event("spent", "closed", "").await;
    assert_eq!(closed["session"], run["session"]);
    assert_ne!(run["session"], main["session"]);
    host.send(&bot, "MAIN_AGAIN").await;
    assert_eq!(host.wait_event("spent", "finished", "MAIN_AGAIN").await["session"], main["session"]);
    assert_eq!(host.agent_events("spent").iter().filter(|e| e["event"] == "closed").count(), 1);
}

#[tokio::test]
async fn busy_bot_keeps_work_pending_and_honors_pause_delete_and_edit_across_restart() {
    let mut host = Host::start().await;
    let bot = host.stable_bot("queue").await;
    host.send(&bot, "BLOCK[user]").await;
    host.wait_event("queue", "started", "BLOCK[user]").await;
    let paused = host.save(&bot, "paused", "SHOULD_NOT_RUN_PAUSED", json!([{"type":"webhook"}]), 30).await;
    let deleted = host.save(&bot, "deleted", "SHOULD_NOT_RUN_DELETED", json!([{"type":"webhook"}]), 30).await;
    let kept = host.save(&bot, "kept", "OLD_INSTRUCTION", json!([{"type":"webhook"}]), 30).await;
    for id in [&paused, &deleted, &kept] {
        host.call("runRoutine", json!({"botId":bot,"id":id})).await;
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let listing = host.call("routines", json!({"botId":bot})).await;
    assert!(listing["runs"].as_array().unwrap().iter().all(|r| r["status"] == "pending"));
    host.call("setRoutineEnabled", json!({"botId":bot,"id":paused,"enabled":false})).await;
    host.call("deleteRoutine", json!({"botId":bot,"id":deleted})).await;
    host.call(
        "saveRoutine",
        json!({"botId":bot,"id":kept,"name":"kept","instruction":"NEW_INSTRUCTION","triggers":[{"type":"webhook"}]}),
    )
    .await;
    let dispatch = host.permitted_dispatch(&bot);
    host.restart().await;
    host.wait_status(&bot, &kept, "pending").await;
    host.assert_paused_then_release(&bot, "queue", "NEW_INSTRUCTION", false).await;
    host.wait_status(&bot, &kept, "succeeded").await;
    host.wait_event("queue", "finished", "NEW_INSTRUCTION").await;
    host.assert_no_replay("queue", "BLOCK[user]");
    let notice = format!("dispatch-interrupted-{}", dispatch["attempt"].as_str().unwrap());
    host.assert_interruption_notice(&bot, &notice).await;
    let listing = host.call("routines", json!({"botId":bot})).await;
    let runs = listing["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 3);
    for id in [&paused, &deleted] {
        assert_eq!(runs.iter().find(|r| r["routineId"] == *id).unwrap()["status"], "cancelled");
    }
    let events = host.agent_events("queue");
    let starts: Vec<_> = events.iter().filter(|e| e["event"] == "started").collect();
    assert_eq!(starts.len(), 2, "uma entrada humana original e uma rotina válida");
    assert!(starts[0]["text"].as_str().unwrap().contains("BLOCK[user]"));
    assert!(starts[1]["text"].as_str().unwrap().contains("NEW_INSTRUCTION"));
    let history = host.call("history", json!({"botId":bot})).await;
    let entries = history["entries"].as_array().unwrap();
    let user = entries.iter().find(|e| e["kind"] == "user" && e["data"]["text"] == "BLOCK[user]").unwrap();
    assert_eq!(user["data"]["status"], "sent");
    let results: Vec<_> = entries.iter().filter(|e| e["kind"] == "agent" && e["data"]["final"] == true).collect();
    assert_eq!(results.len(), 1, "só a rotina válida publica resultado");
    assert_ne!(results[0]["turn"], user["turn"]);
    assert!(!host.agent_events("queue").iter().any(|e| {
        let text = e["text"].as_str().unwrap();
        text.contains("SHOULD_NOT_RUN") || text.contains("OLD_INSTRUCTION")
    }));
}

#[tokio::test]
async fn a_hung_routine_times_out_and_the_next_task_still_runs() {
    let host = Host::start().await;
    let bot = host.stable_bot("timeout").await;
    let hung = host.save(&bot, "hung", "BLOCK[forever] IGNORE_CANCEL", json!([{"type":"webhook"}]), 2).await;
    let next = host.save(&bot, "next", "AFTER_TIMEOUT", json!([{"type":"webhook"}]), 30).await;
    host.call("runRoutine", json!({"botId":bot,"id":hung})).await;
    host.wait_event("timeout", "started", "BLOCK[forever]").await;
    host.call("runRoutine", json!({"botId":bot,"id":next})).await;
    let failed = host.wait_status(&bot, &hung, "failed").await;
    assert!(failed["detail"].as_str().unwrap().contains("time limit"));
    host.wait_status(&bot, &next, "succeeded").await;
    host.wait_event("timeout", "finished", "AFTER_TIMEOUT").await;
}

#[tokio::test]
async fn unavailable_resume_never_replays_work_in_a_fresh_session() {
    let mut host = Host::start().await;
    let bot = host.stable_bot("no-resume").await;
    let id = host.save(&bot, "unloadable", "BLOCK[original]", json!([{"type":"webhook"}]), 30).await;
    host.call("runRoutine", json!({"botId":bot,"id":id})).await;
    host.wait_event("no-resume", "started", "BLOCK[original]").await;
    std::fs::write(host.workspaces.root().join("no-resume/no-load"), "").unwrap();
    host.restart().await;
    host.wait_status(&bot, &id, "interrupted").await;
    let next = host.save(&bot, "next", "AFTER_INTERRUPTION", json!([{"type":"webhook"}]), 30).await;
    host.call("runRoutine", json!({"botId":bot,"id":next})).await;
    host.wait_status(&bot, &next, "pending").await;
    host.assert_paused_then_release(&bot, "no-resume", "AFTER_INTERRUPTION", false).await;
    host.wait_status(&bot, &next, "succeeded").await;
    assert_eq!(
        host.agent_events("no-resume")
            .iter()
            .filter(|e| e["text"].as_str().unwrap().contains("BLOCK[original]"))
            .count(),
        1
    );
}

#[tokio::test]
async fn recurring_work_continues_after_a_failed_run() {
    let host = Host::start().await;
    let bot = host.stable_bot("failure").await;
    let id = host
        .save(&bot, "retry-next-deadline", "FAIL_ONCE[transient]", json!([{"type":"interval","seconds":2}]), 30)
        .await;
    host.wait_status(&bot, &id, "failed").await;
    host.wait_status(&bot, &id, "succeeded").await;
    host.call("setRoutineEnabled", json!({"botId":bot,"id":id,"enabled":false})).await;
}

#[tokio::test]
async fn only_one_host_can_own_a_data_directory_even_on_another_port() {
    let host = Host::start().await;
    let mut other = Command::new(env!("CARGO_BIN_EXE_donnabot-host"))
        .args(["serve", "--bind", "127.0.0.1", "--port", "0"])
        .env("CODYNC_CLOUD", "off")
        .env("MEXERICA_HOME", &host.home)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let status = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(status) = other.try_wait().unwrap() {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    if status.is_err() {
        let _ = other.kill();
        let _ = other.wait();
    }
    assert!(!status.expect("second host did not exit").success());
}

#[tokio::test]
async fn agent_discovers_and_calls_routine_tools_over_real_mcp_stdio() {
    let host = Host::start().await;
    let bot = host.stable_bot("mcp-setup").await;
    host.send(&bot, "VERIFY_ROUTINE_TOOLS").await;
    host.wait_event("mcp-setup", "tools_verified", "Routine MCP lifecycle").await;
    let listing = host.call("routines", json!({"botId":bot})).await;
    let routine = &listing["routines"][0];
    assert_eq!(routine["name"], "Edited through MCP");
    assert_eq!(routine["enabled"], false);
    host.wait_status(&bot, routine["id"].as_str().unwrap(), "succeeded").await;
    host.wait_event("mcp-setup", "finished", "MCP_TASK_RESULT").await;
}

/// I-023: a sessão da execução declara o desfecho pelo MCP real; falha declarada avisa, sem novidade fica quieta,
/// conflito e ausência de declaração ficam sem confirmação (com resposta visível, sem aviso extra).
#[tokio::test]
async fn rotina_declara_o_desfecho_pela_propria_sessao() {
    let host = Host::start().await;
    let bot = host.stable_bot("desfecho").await;
    let mut seen = Vec::new();
    for (instruction, status, kind, notice) in [
        ("DECLARE[no_news] SILENT", "succeeded", "noNews", None),
        ("DECLARE[failed]", "failed", "failed", Some("não conseguiu terminar: Fixture failed")),
        ("DECLARE[reported] DECLARE[failed]", "succeeded", "unconfirmed", None),
        ("PLAIN", "succeeded", "unconfirmed", None),
    ] {
        let id = host.save(&bot, instruction, instruction, json!([{"type":"webhook"}]), 30).await;
        host.call("runRoutine", json!({"botId":bot,"id":id})).await;
        let run = host
            .wait_run_until(&bot, &id, |r| {
                r["status"] != "pending"
                    && r["status"] != "starting"
                    && r["status"] != "running"
                    && r["reportPending"] == false
            })
            .await;
        assert_eq!((run["status"].as_str(), run["outcome"]["kind"].as_str()), (Some(status), Some(kind)), "{run}");
        seen.push((run["id"].as_str().unwrap().to_owned(), run["outcome"].clone(), notice));
    }
    assert_eq!(seen[1].1["source"], "declared");
    assert_eq!(seen[2].1["reason"], "conflict");
    assert_eq!(
        (seen[3].1["reason"].as_str(), seen[3].1["visibleReply"].as_bool()),
        (Some("noDeclaration"), Some(true))
    );
    let history = host.call("history", json!({"botId":bot})).await;
    let entries = history["entries"].as_array().unwrap();
    for (run, _, notice) in &seen {
        let published: Vec<_> = entries
            .iter()
            .filter(|e| e["id"] == format!("routine-result-{run}"))
            .map(|e| e["data"]["text"].as_str().unwrap().to_owned())
            .collect();
        match notice {
            Some(text) => assert!(published.len() == 1 && published[0].contains(text), "{published:?}"),
            None => assert!(published.is_empty(), "{published:?}"),
        }
    }
}

/// I-023 (D70, caminho real MCP → API → ator): a sessão principal e o vínculo da execução anterior (B1, revogado)
/// não declaram por esta execução; a própria sessão declara (controle positivo). Essas duas recusas ocorrem na captura
/// da API (E1): vínculo inativo ou revogado.
#[tokio::test]
async fn outra_sessao_nao_declara_pela_execucao() {
    let host = Host::start().await;
    let bot = host.stable_bot("origem").await;
    host.send(&bot, "MAIN_CHAT").await;
    host.wait_event("origem", "finished", "MAIN_CHAT").await;
    let first = host.save(&bot, "first", "DECLARE[reported]", json!([{"type":"webhook"}]), 30).await;
    host.call("runRoutine", json!({"botId":bot,"id":first})).await;
    let run = host.wait_run_until(&bot, &first, |r| r["status"] == "succeeded" && r["reportPending"] == false).await;
    assert_eq!(run["outcome"]["kind"], "reported", "{run}");
    let second = host
        .save(
            &bot,
            "second",
            "DECLARE_VIA[main][failed] DECLARE_VIA[previous][failed] DECLARE[no_news] SILENT",
            json!([{"type":"webhook"}]),
            30,
        )
        .await;
    host.call("runRoutine", json!({"botId":bot,"id":second})).await;
    let run = host
        .wait_run_until(&bot, &second, |r| {
            r["status"] != "pending"
                && r["status"] != "starting"
                && r["status"] != "running"
                && r["reportPending"] == false
        })
        .await;
    assert_eq!((run["status"].as_str(), run["outcome"]["kind"].as_str()), (Some("succeeded"), Some("noNews")), "{run}");
    let events = host.agent_events("origem");
    for via in ["declared_via_main", "declared_via_previous"] {
        let result: Value = serde_json::from_str(
            events.iter().find(|e| e["event"] == via).unwrap_or_else(|| panic!("{via}"))["text"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(result["isError"], true, "{via}: {result}");
    }
}

/// I-023 (D70, E2 no ator): o motor devolve a mesma ACP para a principal e para a rotina, ambas guardadas. A declaração
/// é barrada antes do prompt: o identificador já pertence à sessão de outra pessoa.
#[tokio::test]
async fn acp_repetida_em_dois_slots_fica_sem_confirmacao() {
    let host = Host::start().await;
    let bot = host.stable_bot("ambigua").await;
    std::fs::write(host.workspaces.root().join("ambigua").join("same-acp"), "").unwrap();
    host.send(&bot, "MAIN_CHAT").await;
    host.wait_event("ambigua", "finished", "MAIN_CHAT").await;
    let id = host.save(&bot, "ambigua", "DECLARE[reported]", json!([{"type":"webhook"}]), 30).await;
    host.call("runRoutine", json!({"botId":bot,"id":id})).await;
    let run = host
        .wait_run_until(&bot, &id, |r| {
            r["status"] != "pending"
                && r["status"] != "starting"
                && r["status"] != "running"
                && r["reportPending"] == false
        })
        .await;
    assert_eq!(
        (run["outcome"]["kind"].as_str(), run["outcome"]["reason"].as_str()),
        (Some("failed"), Some("startFailed")),
        "{run}"
    );
    assert!(run["detail"].as_str().unwrap_or_default().contains("profile_person_mismatch"));
    assert!(!host.agent_events("ambigua").iter().any(|event| event["event"] == "declared"));
}

#[tokio::test]
async fn schedule_editor_preview_and_save_share_host_rules() {
    let host = Host::start().await;
    let bot = host.stable_bot("schedule-editor").await;
    let preview = host
        .call(
            "routineSchedule",
            json!({"draft": {
                "calendarStyle":"days", "hour":23, "minute":47,
                "selectedDays":[5,1,3], "zone":"Asia/Taipei"
            }}),
        )
        .await;
    assert_eq!(preview["triggers"][0]["expression"], "47 23 * * 1,3,5");
    let saved = host
        .call(
            "saveRoutine",
            json!({
                "botId":bot,"name":"Calendar editor","instruction":"Do not execute this paused test.",
                "enabled":false,"schedule":preview["draft"],"timeoutSeconds":"60"
            }),
        )
        .await;
    assert_eq!(saved["routine"]["triggers"], preview["triggers"]);
    assert_eq!(saved["routine"]["timeoutSeconds"], 60);
    let loaded = host.call("routineSchedule", json!({"triggers":saved["routine"]["triggers"]})).await;
    assert_eq!(loaded["draft"]["calendarStyle"], "days");
    assert_eq!(loaded["draft"]["zone"], "Asia/Taipei");
    // Saving directly must validate even when a caller bypasses the preview UI.
    for schedule in [
        json!({"calendarStyle":"days","selectedDays":[]}),
        json!({"calendarStyle":"custom","expression":"61 9 * * *"}),
        json!({"zone":"invalid/zone"}),
    ] {
        let response = reqwest::Client::new()
            .post(format!("{}/api/saveRoutine", host.base))
            .bearer_auth(&host.token)
            .json(&json!({"botId":bot,"name":"Invalid","instruction":"Never run", "schedule":schedule}))
            .send()
            .await
            .unwrap();
        assert!(!response.status().is_success());
    }
    assert_eq!(host.call("routines", json!({"botId":bot})).await["runs"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn pausa_global_persiste_e_nao_reexecuta_trabalho_aceito() {
    let mut host = Host::start().await;
    let bot = host.stable_bot("global-pause").await;
    host.send(&bot, "BLOCK[user]").await;
    host.wait_event("global-pause", "started", "BLOCK[user]").await;
    let id = host.save(&bot, "pending", "PENDING_ONCE", json!([{"type":"webhook"}]), 30).await;
    let credentials = host.call("routineWebhook", json!({"botId":bot,"id":id})).await;
    host.call("runRoutine", json!({"botId":bot,"id":id})).await;
    host.wait_status(&bot, &id, "pending").await;
    assert_eq!(host.call("setRoutinesPaused", json!({"paused":true})).await["paused"], true);
    let client = reqwest::Client::new();
    let refused = client
        .post(format!("{}/api/runRoutine", host.base))
        .bearer_auth(&host.token)
        .json(&json!({"botId":bot,"id":id}))
        .send()
        .await
        .unwrap();
    assert!(!refused.status().is_success());
    let webhook = client
        .post(credentials["localUrl"].as_str().unwrap())
        .bearer_auth(credentials["key"].as_str().unwrap())
        .json(&json!({"text":"event"}))
        .send()
        .await
        .unwrap();
    assert_eq!(webhook.status(), reqwest::StatusCode::CONFLICT);
    let dispatch = host.permitted_dispatch(&bot);
    host.restart().await;
    assert_eq!(host.call("routinesPause", json!({})).await["paused"], true);
    host.send(&bot, "BARREIRA_PAUSADA").await;
    host.assert_paused_then_release(&bot, "global-pause", "BARREIRA_PAUSADA", true).await;
    host.wait_event("global-pause", "finished", "BARREIRA_PAUSADA").await;
    let entries = host
        .wait_history_until(&bot, |entries| entries.iter().any(|e| e["kind"] == "agent" && e["data"]["final"] == true))
        .await;
    assert_eq!(entries.iter().filter(|e| e["kind"] == "agent" && e["data"]["final"] == true).count(), 1);
    assert!(!host.agent_events("global-pause").iter().any(|e| e["text"].as_str().unwrap().contains("PENDING_ONCE")));
    host.assert_no_replay("global-pause", "BLOCK[user]");
    let notice = format!("dispatch-interrupted-{}", dispatch["attempt"].as_str().unwrap());
    host.assert_interruption_notice(&bot, &notice).await;
    host.wait_status(&bot, &id, "pending").await;
    assert_eq!(host.call("setRoutinesPaused", json!({"paused":false})).await["paused"], false);
    host.call("setRoutinesPaused", json!({"paused":false})).await;
    host.wait_status(&bot, &id, "succeeded").await;
    host.assert_no_replay("global-pause", "BLOCK[user]");
    host.assert_interruption_notice(&bot, &notice).await;
    let listing = host.call("routines", json!({"botId":bot})).await;
    assert_eq!(listing["runs"].as_array().unwrap().len(), 1);
    let history = host.call("history", json!({"botId":bot})).await;
    let entries = history["entries"].as_array().unwrap();
    let user = entries.iter().find(|e| e["kind"] == "user" && e["data"]["text"] == "BLOCK[user]").unwrap();
    let results: Vec<_> = entries.iter().filter(|e| e["kind"] == "agent" && e["data"]["final"] == true).collect();
    assert_eq!(results.len(), 2, "um resultado da conversa nova e um da rotina");
    assert!(!results.iter().any(|e| e["turn"] == user["turn"]), "o turno incerto não publica sucesso");
    assert_eq!(
        host.agent_events("global-pause")
            .iter()
            .filter(|e| e["event"] == "started" && e["text"].as_str().unwrap().contains("PENDING_ONCE"))
            .count(),
        1
    );
}

/// Origem de mensagem como o app reporta: `typed` é fala digitada; `pasted` é conteúdo de fora.
fn message_origin(text: &str, pasted: bool) -> Value {
    let length = text.encode_utf16().count();
    let segments = if pasted {
        json!([{"start":0,"end":length,"kind":"pasted"}])
    } else {
        json!([{"start":0,"end":length,"kind":"typed"}])
    };
    json!({"schema":1,"draftId":uuid::Uuid::new_v4().to_string(),"revision":1,"textLength":length,"deviceId":uuid::Uuid::new_v4().to_string(),"channel":"desktopComposer","interaction":"send","segments":segments})
}

/// `HUB_FLOW_QUIET`: o adaptador fictício é `custom`, sem canal de identidade; anunciar o `tool_call` faria o
/// Hub tratá-lo como ferramenta externa (fail-closed) e toda criação nasceria marcada. Sem o anúncio, o MCP
/// `routines` se comporta como o do Hub identificado como interno, e só o conteúdo de fora marca a sessão.
fn routine_flow(name: &str) -> String {
    let step = json!(["routines","save_routine",{"name":name,"instruction":"Somente dado sintético","triggers":[{"type":"cron","expression":"0 9 * * 1","timeZone":"America/Sao_Paulo"}]}]);
    format!("HUB_FLOW_QUIET [{step}]")
}

const PERSONAL_SPACE: &str = "00000000-0000-4000-8000-000000000001";

impl Host {
    async fn send_message(&self, bot: &str, text: &str, pasted: bool) {
        let body = json!({"botId":bot,"text":text,"contentOrigin":message_origin(text, pasted),"clientNonce":uuid::Uuid::new_v4().to_string()});
        self.call("send", body).await;
    }

    async fn wait_routine(&self, bot: &str, name: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let listing = self.call("routines", json!({"botId":bot})).await;
                if let Some(routine) = listing["routines"].as_array().unwrap().iter().find(|r| r["name"] == name) {
                    return routine.clone();
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("a rotina criada não apareceu")
    }

    async fn wait_card(&self, routine: &Value) -> Value {
        let id = routine["id"].as_str().unwrap();
        let hub_id = self.call("hello", json!({})).await["hostId"].as_str().unwrap().to_owned();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let board = self.call("donnaControlTower", json!({"spaceId":PERSONAL_SPACE,"hubId":hub_id})).await;
                let cards = board["routineCreations"]["value"]["items"].as_array().unwrap();
                if let Some(card) = cards.iter().find(|card| card["routineId"] == id) {
                    return card.clone();
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("o cartão não apareceu no Mission Board")
    }
}

// MX-106 (decisão do dono em MX-89): rotina criada a partir de conteúdo de fora nasce pausada, avisa o
// dono, aparece no Mission Board esperando o OK e nada roda antes de o dono liberar no app. Sem a marca,
// a política D544 continua: nasce ativa e o silêncio a mantém.
#[tokio::test]
async fn routine_from_external_content_is_born_paused_and_waits_for_the_owner() {
    let host = Host::start().await;
    // O dono cola um e-mail e pede a rotina: a mensagem carrega conteúdo de fora (D125).
    let external = host.bot("external-routine").await;
    let pasted = format!("Responda os e-mails assim.\n\n{}", routine_flow("Resumo do e-mail"));
    host.send_message(&external, &pasted, true).await;
    let routine = host.wait_routine(&external, "Resumo do e-mail").await;
    assert_eq!(routine["enabled"], false, "MX-106: criação de conteúdo de fora nasce pausada: {routine}");
    assert_eq!(routine["nextRunAt"], Value::Null, "pausada não agenda: {routine}");
    assert_eq!(
        host.call("routines", json!({"botId":external})).await["runs"].as_array().unwrap().len(),
        0,
        "nada roda antes do OK do dono"
    );
    // Aviso na conversa principal: por que está pausada e onde liberar.
    let id = routine["id"].as_str().unwrap().to_owned();
    let history = host
        .wait_history_until(&external, |entries| {
            entries.iter().any(|e| e["data"]["routineCreation"]["routineId"] == id.as_str())
        })
        .await;
    let notice = history.iter().find(|e| e["data"]["routineCreation"]["routineId"] == id.as_str()).unwrap();
    let text = notice["data"]["text"].as_str().unwrap();
    assert!(text.contains("pausada"), "{text}");
    assert!(text.contains("conteúdo de fora"), "{text}");
    assert!(text.contains("ligar no Mission Board"), "{text}");
    // A marca D125 é da conversa: a mensagem digitada seguinte, nessa mesma conversa, também nasce pausada.
    host.send_message(&external, &routine_flow("Segunda rotina"), false).await;
    let second = host.wait_routine(&external, "Segunda rotina").await;
    assert_eq!(second["enabled"], false, "a marca da conversa persiste (D125): {second}");
    // Mission Board: cartões pendentes, com as rotinas pausadas.
    let card = host.wait_card(&routine).await;
    assert_eq!(card["status"], "paused");
    assert_eq!(card["nextRunAt"], Value::Null);
    assert_eq!(host.wait_card(&second).await["status"], "paused");
    // A liberação é gesto do dono no app: liga e volta a agendar.
    let released = host.call("setRoutineEnabled", json!({"botId":external,"id":id,"enabled":true})).await;
    assert_eq!(released["routine"]["enabled"], true);
    assert!(released["routine"]["nextRunAt"].is_number(), "{released}");
    // Controle causal: o mesmo fluxo, em conversa sem a marca, segue nascendo ativo (D544).
    let clean = host.bot("clean-routine").await;
    host.send_message(&clean, &routine_flow("Lembrete do dentista"), false).await;
    let routine = host.wait_routine(&clean, "Lembrete do dentista").await;
    assert_eq!(routine["enabled"], true, "sem a marca a política D544 continua: {routine}");
    assert!(routine["nextRunAt"].is_number());
    assert_eq!(host.wait_card(&routine).await["status"], "active");
}
