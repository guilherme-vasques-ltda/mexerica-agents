//! A fila de mensagens com o Hub de verdade: a chave antiduplicata vale em envios simultâneos e depois de um
//! reinício, intenção diferente com a mesma chave é recusada, a fila sai na ordem sem duplicar e, cheia, recusa sem
//! criar linha e sem perder o anexo já enviado.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::workspaces::Workspaces;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// O mesmo limite do Hub (`owner_question::QUEUE_MAX`).
const QUEUE_MAX: usize = 50;

struct Host {
    child: Option<Child>,
    base: String,
    token: String,
    home: PathBuf,
    workspaces: Workspaces,
}

impl Drop for Host {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            common::stop(child);
        }
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

impl Host {
    async fn start() -> Self {
        let home = std::env::temp_dir().join(format!("hub-fila-e2e-{}", uuid::Uuid::new_v4()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&home).unwrap();
        }
        #[cfg(not(unix))]
        std::fs::create_dir_all(&home).unwrap();
        let workspaces = Workspaces::new(&home);
        let (child, base) = common::serve(&home).await;
        let token = std::fs::read_to_string(home.join("token")).unwrap().trim().to_owned();
        Self { child: Some(child), base, token, home, workspaces }
    }

    /// Mata o processo do Hub (SIGKILL) e sobe outro sobre os mesmos dados.
    async fn kill_and_restart(&mut self) {
        let mut child = self.child.take().unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
        let (child, base) = common::serve(&self.home).await;
        self.child = Some(child);
        self.base = base;
        std::fs::read_to_string(self.home.join("token")).unwrap().trim().clone_into(&mut self.token);
    }

    async fn try_call(&self, method: &str, body: Value) -> (bool, Value) {
        let res = reqwest::Client::new()
            .post(format!("{}/api/{method}", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        let ok = res.status().is_success();
        (ok, res.json().await.unwrap())
    }

    async fn call(&self, method: &str, body: Value) -> Value {
        let (ok, v) = self.try_call(method, body).await;
        assert!(ok, "{method} failed: {v}");
        v
    }

    async fn bot(&self, log: &Path) -> String {
        let cwd = self.workspaces.root().join("fila");
        std::fs::create_dir_all(&cwd).unwrap();
        let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fila_agent.py");
        let command = format!("{} '{}'", common::python_agent(&agent), log.display());
        let bot = json!({"name": "Fila", "backend": "custom", "command": command, "cwd": cwd, "permission": "ask", "notify": false});
        self.call("createBot", bot).await["bot"]["id"].as_str().unwrap().to_owned()
    }

    async fn send(&self, bot: &str, text: &str, nonce: &str) -> (bool, Value) {
        self.try_call("send", json!({"botId": bot, "text": text, "clientNonce": nonce})).await
    }

    async fn users(&self, bot: &str) -> Vec<Value> {
        let all =
            self.call("history", json!({"botId": bot, "limit": 500})).await["entries"].as_array().unwrap().clone();
        all.into_iter().filter(|e| e["kind"] == "user").collect()
    }

    /// Deixa o bot ocupado num pedido que não termina: o que vier depois espera na fila.
    async fn hold(&self, bot: &str, log: &Path) {
        assert!(self.send(bot, "segura", "segura").await.0);
        let deadline = Instant::now() + Duration::from_secs(60);
        while !std::fs::read_to_string(log).is_ok_and(|t| t.contains("segura")) {
            assert!(Instant::now() < deadline, "o motor não recebeu o primeiro pedido");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

fn python_missing() -> bool {
    let missing = Command::new(common::python()).arg("--version").output().is_err();
    if missing {
        eprintln!("skipping: Python not available");
    }
    missing
}

fn error_of(reply: &Value) -> String {
    reply.to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mesma_chave_em_envios_simultaneos_da_uma_linha_e_intencao_diferente_e_recusada() {
    if python_missing() {
        return;
    }
    let host = Host::start().await;
    let log = host.home.join("pedidos.log");
    let bot = host.bot(&log).await;
    host.hold(&bot, &log).await;
    for i in 0..10 {
        let (nonce, text) = (format!("dupla-{i}"), format!("mensagem {i}"));
        let (a, b) = tokio::join!(host.send(&bot, &text, &nonce), host.send(&bot, &text, &nonce));
        assert!(a.0 && b.0, "{} / {}", a.1, b.1);
        assert_eq!(a.1["entry"]["id"], b.1["entry"]["id"], "os dois envios devolvem a mesma mensagem");
    }
    let users = host.users(&bot).await;
    for i in 0..10 {
        let nonce = format!("dupla-{i}");
        assert_eq!(users.iter().filter(|e| e["data"]["clientNonce"] == nonce.as_str()).count(), 1, "{nonce}");
    }
    let original = users.iter().find(|e| e["data"]["clientNonce"] == "dupla-0").unwrap()["id"].clone();
    let (ok, reply) = host.send(&bot, "outro texto", "dupla-0").await;
    assert!(!ok && error_of(&reply).contains("nonce_intent_conflict"), "{reply}");
    let (ok, reply) = host.send(&bot, "mensagem 0", "dupla-0").await;
    assert!(ok, "{reply}");
    assert_eq!(reply["entry"]["id"], original, "o reenvio idêntico devolve a linha original");
    assert_eq!(host.users(&bot).await.len(), users.len(), "nenhuma linha nova");
}

#[tokio::test]
async fn vinte_na_fila_saem_em_ordem_e_sem_duplicar_depois_de_matar_o_hub() {
    if python_missing() {
        return;
    }
    let mut host = Host::start().await;
    let log = host.home.join("pedidos.log");
    let bot = host.bot(&log).await;
    host.hold(&bot, &log).await;
    let mut ids = Vec::new();
    for i in 1..=20 {
        let (ok, reply) = host.send(&bot, &format!("msg-{i:02}"), &format!("fila-{i:02}")).await;
        assert!(ok, "{reply}");
        ids.push(reply["entry"]["id"].clone());
    }
    for i in [5, 12] {
        let (_, reply) = host.send(&bot, &format!("msg-{i:02}"), &format!("fila-{i:02}")).await;
        assert_eq!(reply["entry"]["id"], ids[i - 1]);
    }
    host.kill_and_restart().await;
    // A mesma mensagem enviada de novo depois do reinício continua sendo uma só.
    let (ok, reply) = host.send(&bot, "msg-07", "fila-07").await;
    assert!(ok, "{reply}");
    assert_eq!(reply["entry"]["id"], ids[6]);

    let deadline = Instant::now() + Duration::from_secs(120);
    while !std::fs::read_to_string(&log).unwrap_or_default().contains("msg-20") {
        assert!(Instant::now() < deadline, "a fila não andou: {}", std::fs::read_to_string(&log).unwrap_or_default());
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    let seen = std::fs::read_to_string(&log).unwrap();
    let mut last = 0;
    for i in 1..=20 {
        let marker = format!("msg-{i:02}");
        assert_eq!(seen.matches(&marker).count(), 1, "{marker} uma única vez: {seen}");
        let at = seen.find(&marker).unwrap();
        assert!(at > last, "{marker} fora de ordem: {seen}");
        last = at;
    }
    assert_eq!(seen.matches("segura").count(), 1, "o pedido cortado não volta ao motor");
    assert_eq!(host.users(&bot).await.len(), 21);
}

#[tokio::test]
async fn fila_cheia_recusa_sem_criar_linha_e_o_anexo_continua_para_tentar_de_novo() {
    use base64::Engine as _;
    if python_missing() {
        return;
    }
    let host = Host::start().await;
    let log = host.home.join("pedidos.log");
    let bot = host.bot(&log).await;
    host.hold(&bot, &log).await;
    for i in 0..QUEUE_MAX {
        let (ok, reply) = host.send(&bot, &format!("espera {i}"), &format!("cheia-{i}")).await;
        assert!(ok, "{i}: {reply}");
    }
    let data = base64::engine::general_purpose::STANDARD.encode(b"conteudo do anexo");
    let upload_id = uuid::Uuid::new_v4().to_string();
    let upload =
        json!({"botId": bot, "uploadId": upload_id, "name": "nota.txt", "data": data, "offset": 0, "done": true});
    host.call("upload", upload).await;
    let before = host.users(&bot).await.len();
    let body = json!({"botId": bot, "text": "uma a mais", "clientNonce": "cheia-extra", "attachments": [upload_id]});
    let (ok, reply) = host.try_call("send", body).await;
    assert!(!ok && error_of(&reply).contains("queue_full"), "{reply}");
    assert_eq!(host.users(&bot).await.len(), before, "nenhuma linha criada");
    let read = host.call("readUpload", json!({"botId": bot, "uploadId": upload_id, "offset": 0})).await;
    assert_eq!(read["data"], data, "o anexo continua lá para o reenvio");
}
