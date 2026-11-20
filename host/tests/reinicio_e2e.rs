//! Hub de verdade cortado no meio de uma resposta (SIGTERM e SIGKILL): ao reiniciar, a conversa mostra o texto até o
//! corte marcado como interrompido, nunca como resposta final; a ferramenta aberta aparece como interrompida; o aviso
//! de reinício sai uma única vez e o pedido não é reenviado ao motor.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::workspaces::Workspaces;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

const AVISO: &str = "O Hub reiniciou após permitir este turno";

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
        let home = std::env::temp_dir().join(format!("hub-reinicio-e2e-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        let workspaces = Workspaces::new(&home);
        let (child, base) = common::serve(&home).await;
        let token = std::fs::read_to_string(home.join("token")).unwrap().trim().to_owned();
        Self { child: Some(child), base, token, home, workspaces }
    }

    /// Derruba o processo do Hub (`kill` = SIGKILL, senão SIGTERM) e sobe outro sobre os mesmos dados.
    async fn restart(&mut self, kill: bool) {
        let mut child = self.child.take().unwrap();
        if kill {
            child.kill().unwrap();
            child.wait().unwrap();
        } else {
            common::stop(&mut child);
        }
        let (child, base) = common::serve(&self.home).await;
        self.child = Some(child);
        self.base = base;
        std::fs::read_to_string(self.home.join("token")).unwrap().trim().clone_into(&mut self.token);
    }

    async fn call(&self, method: &str, body: Value) -> Value {
        let res = reqwest::Client::new()
            .post(format!("{}/api/{method}", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = res.status();
        let v: Value = res.json().await.unwrap();
        assert!(status.is_success(), "{method} failed: {v}");
        v
    }

    async fn bot(&self, log: &std::path::Path) -> String {
        let cwd = self.workspaces.root().join("cortado");
        std::fs::create_dir_all(&cwd).unwrap();
        let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cut_agent.py");
        let command = format!("{} '{}'", common::python_agent(&agent), log.display());
        let bot = json!({"name": "Cortado", "backend": "custom", "command": command, "cwd": cwd, "permission": "ask", "notify": false});
        self.call("createBot", bot).await["bot"]["id"].as_str().unwrap().to_owned()
    }

    async fn entries(&self, chat: &str) -> Vec<Value> {
        self.call("history", json!({"botId": chat})).await["entries"].as_array().unwrap().clone()
    }

    async fn wait_for(&self, chat: &str, pred: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let entries = self.entries(chat).await;
            if pred(&entries) {
                return entries;
            }
            assert!(Instant::now() < deadline, "timed out; history: {entries:?}");
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

fn of_kind<'a>(entries: &'a [Value], kind: &str) -> Vec<&'a Value> {
    entries.iter().filter(|e| e["kind"] == kind).collect()
}

fn reply(entries: &[Value]) -> String {
    let agents = of_kind(entries, "agent");
    assert_eq!(agents.len(), 1, "um único trecho de resposta: {agents:?}");
    agents[0]["data"]["text"].as_str().unwrap_or_default().to_owned()
}

async fn cut_mid_reply(kill: bool) {
    let mut host = Host::start().await;
    let log = host.home.join("pedidos.log");
    let bot = host.bot(&log).await;
    host.call("send", json!({"botId": bot, "text": "escreva sem parar"})).await;
    let before = host
        .wait_for(&bot, |entries| {
            entries.iter().any(|e| e["kind"] == "tool" && e["data"]["status"] == "in_progress")
                && entries
                    .iter()
                    .any(|e| e["kind"] == "agent" && e["data"]["text"].as_str().is_some_and(|t| t.contains("p20 ")))
        })
        .await;
    let seen = reply(&before);
    host.restart(kill).await;

    host.wait_for(&bot, |entries| entries.iter().any(|e| e["kind"] == "notice")).await;
    // O motor do Hub novo não recebe o pedido de novo, nem depois de uns instantes.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "prompt\n", "nenhum pedido repetido");
    let after = host.entries(&bot).await;

    let agent = of_kind(&after, "agent")[0].clone();
    let text = reply(&after);
    assert!(text.starts_with(&seen), "o texto visto antes do corte continua inteiro: {seen:?} -> {text:?}");
    assert!(text.ends_with(' '), "o texto termina num trecho completo: {text:?}");
    assert_eq!(agent["data"]["interrupted"], true, "{agent}");
    assert_ne!(agent["data"]["final"], true, "texto cortado não vira resposta final: {agent}");
    assert!(agent["data"].get("streaming").is_none(), "{agent}");

    let tools = of_kind(&after, "tool");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["data"]["status"], "failed", "{}", tools[0]);
    assert_eq!(tools[0]["data"]["interrupted"], true, "{}", tools[0]);

    let notices: Vec<_> = of_kind(&after, "notice")
        .into_iter()
        .filter(|e| e["data"]["text"].as_str().is_some_and(|t| t.contains(AVISO)))
        .collect();
    assert_eq!(notices.len(), 1, "aviso de reinício uma única vez: {after:?}");

    // Um segundo reinício não repete o aviso nem muda o que já foi marcado.
    host.restart(false).await;
    let again = host.entries(&bot).await;
    assert_eq!(
        of_kind(&again, "notice")
            .iter()
            .filter(|e| e["data"]["text"].as_str().is_some_and(|t| t.contains(AVISO)))
            .count(),
        1
    );
    assert_eq!(reply(&again), text);
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "prompt\n");
}

#[tokio::test]
async fn desligar_no_meio_da_resposta_marca_o_corte_e_nao_repete_nada() {
    if python_missing() {
        return;
    }
    cut_mid_reply(false).await;
}

#[tokio::test]
async fn matar_o_processo_no_meio_da_resposta_guarda_o_ultimo_trecho_gravado() {
    if python_missing() {
        return;
    }
    cut_mid_reply(true).await;
}
