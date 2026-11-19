use super::*;
use crate::store::{EntryKind, Store};
use std::path::PathBuf;

#[test]
fn wait_graph_rejects_self_duplicates_and_indirect_cycles() {
    let requests = Requests::default();
    for bot in ["a", "b", "c", "d"] {
        requests.start_turn(bot, None);
    }
    assert!(requests.begin("self", "a", "a").is_err());
    let (ab, _) = requests.begin("ab", "a", "b").unwrap();
    assert!(requests.begin("duplicate", "a", "b").is_err());
    let _bc = requests.begin("bc", "b", "c").unwrap();
    assert!(requests.begin("ca", "c", "a").is_err());
    let _dc = requests.begin("dc", "d", "c").unwrap();
    requests.cancel_from("a");
    assert!(*ab.borrow());
    assert!(requests.begin("late", "a", "d").is_err());
    requests.0.locked().pending.remove("ab");
    assert!(requests.begin("ca", "c", "a").is_ok());
}

#[test]
fn checkpoint_recusa_pedido_sem_registro_ou_sem_cerca() {
    let (sem_registro, _) = BotRequest::for_test("legacy");
    assert!(sem_registro.permission_checkpoint().is_err());
    let (aceito, _) = BotRequest::for_test_recorded("accepted");
    assert!(aceito.permission_checkpoint().is_err());
}

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        std::fs::create_dir_all(crate::service::data_dir()).unwrap();
        let dir = std::env::temp_dir().join(format!("codync-team-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("test.db")).unwrap();
        let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/team_agent.py");
        for id in ["a", "b", "c"] {
            let cwd = dir.join(id);
            std::fs::create_dir_all(&cwd).unwrap();
            let cfg: BotConfig = serde_json::from_value(json!({
                "id": id, "name": id, "backend": "fixture", "cwd": cwd,
                "command": crate::shell::python(&agent),
                "notify": false, "permission": "ask",
            }))
            .unwrap();
            store.save_bot(&cfg).unwrap();
        }
        let hub = Hub::new(
            store,
            "test-host".into(),
            crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
            "test-token".into(),
            19222,
        );
        hub.start().unwrap();
        bindings::fixture_execution(&hub, "a");
        hub.team.start_turn("a", None);
        Self { hub, dir }
    }

    fn request(&self, message: &str) -> tokio::task::JoinHandle<Result<Value>> {
        let hub = self.hub.clone();
        let message = message.to_owned();
        tokio::spawn(async move {
            crate::api::dispatch(
                &hub,
                &crate::api::devices::Caller::Local,
                "teamCall",
                json!({
                    "botId": "a", "sessionBinding": bindings::for_acp(&hub.store, "a", "fixture-acp").unwrap().id.as_str(), "name": "ask_bot", "arguments": {"botId": "b", "message": message},
                }),
            )
            .await
        })
    }

    async fn message(&self, message: &str) -> Result<Value> {
        call(&self.hub, "a", "message_bot", &json!({"botId": "b", "message": message})).await
    }

    fn notices(&self, request: &Value) -> Vec<crate::store::Entry> {
        ["a", "b"]
            .into_iter()
            .flat_map(|bot| self.hub.store.history(bot, i64::MAX, 200).unwrap())
            .filter(|e| e.data["delegationId"] == request["requestId"])
            .collect()
    }

    /// MX-77: o aviso DO DESTINATÁRIO do pedido `delegation` ainda `queued` na principal de "b".
    async fn recipient_notice(&self, delegation: &str) -> crate::store::Entry {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let found = self.hub.store.history("b", i64::MAX, 200).unwrap().into_iter().find(|e| {
                    e.data["delegationId"] == delegation
                        && e.data["botMessage"]["targetBotId"] == "b"
                        && e.data["status"] == "queued"
                });
                if let Some(e) = found {
                    return e;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("aviso do destinatário não ficou queued")
    }

    /// MX-77: o `delegationId` do pedido cujo aviso do destinatário acabou de ficar `queued` em "b".
    async fn queued_delegation(&self) -> String {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let found = self
                    .hub
                    .store
                    .history("b", i64::MAX, 200)
                    .unwrap()
                    .into_iter()
                    .find(|e| e.data["botMessage"]["targetBotId"] == "b" && e.data["status"] == "queued")
                    .map(|e| e.data["delegationId"].as_str().unwrap().to_owned());
                if let Some(d) = found {
                    return d;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("pedido não ficou queued no destinatário")
    }

    /// MX-77: os dois avisos do pedido, relidos depois da ação.
    fn delegation_notices(&self, delegation: &str) -> Vec<crate::store::Entry> {
        ["a", "b"]
            .into_iter()
            .flat_map(|bot| self.hub.store.history(bot, i64::MAX, 200).unwrap())
            .filter(|e| e.data["delegationId"] == delegation)
            .collect()
    }

    /// MX-77: as três ações da bandeja sobre o aviso do destinatário, como o app do dono chama.
    async fn queued_action(&self, method: &str, entry: &crate::store::Entry, text: Option<&str>) -> Result<Value> {
        let mut b = json!({"botId": "b", "entryId": entry.id});
        if let Some(text) = text {
            b["expectedRevision"] = entry.rev.into();
            b["text"] = text.into();
        }
        crate::api::dispatch(&self.hub, &crate::api::devices::Caller::Local, method, b).await
    }

    async fn send_user(&self, text: &str) {
        let result = crate::api::dispatch(
            &self.hub,
            &crate::api::devices::Caller::Local,
            "send",
            json!({"botId": "b", "text": text}),
        )
        .await;
        result.unwrap();
    }

    async fn until(&self, condition: impl Fn() -> bool) {
        // Generous: Windows runners need several seconds to run 16 queued turns.
        tokio::time::timeout(Duration::from_secs(30), async {
            while !condition() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture condition timed out");
    }

    fn prompts(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("b/prompts.jsonl"))
            .unwrap_or_default()
            // O agente ainda pode estar anexando o último registro. Só o newline fecha o JSONL;
            // JSON inválido em um registro completo continua falhando, sem descartar o erro.
            .split_inclusive('\n')
            .filter(|line| line.ends_with('\n'))
            .map(|l| without_turn_context(&serde_json::from_str::<String>(l).unwrap()))
            .collect()
    }

    fn personal_prompt(&self, text: &str) -> String {
        let cfg = self.hub.store.bot("b").unwrap().unwrap().config;
        format!("{}{text}", crate::fork::memory::profile::turn(&self.hub.store, &cfg, false))
    }

    async fn shutdown(self) {
        self.hub.shutdown().await;
        // Windows may still hold files of an agent that just exited.
        let _ = std::fs::remove_dir_all(self.dir);
    }
}

#[tokio::test]
async fn delegation_roundtrip_keeps_queued_user_messages_separate() {
    let f = Fixture::new();
    let request = f.request("BLOCK review these changes");
    f.until(|| f.prompts().len() == 1).await;
    let sent = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId": "b", "text": "thanks"}),
    )
    .await
    .unwrap();
    let request_entry = f
        .hub
        .store
        .history("b", i64::MAX, 100)
        .unwrap()
        .into_iter()
        .find(|e| e.data["delegationId"].is_string())
        .unwrap();
    assert!(request_entry.data["text"].as_str().unwrap().contains("Request from a"));
    let dispatch = crate::fork::bot_admission::current(&f.hub.store, "b").unwrap().unwrap();
    assert_eq!(dispatch.phase, crate::fork::bot_admission::Phase::Permitted);
    assert_eq!(dispatch.origin, crate::fork::bot_admission::Origin::Ask);
    std::fs::write(f.dir.join("b/release"), "").unwrap();
    let result = request.await.unwrap().unwrap();
    assert_eq!(result["reply"], "reply: BLOCK review these changes");
    f.until(|| f.prompts().len() == 2 && f.hub.runtime("b").status == BotStatus::Idle).await;
    assert!(f.prompts()[1].ends_with("\n\nthanks"));
    assert_eq!(f.hub.store.entry(sent["entry"]["id"].as_str().unwrap()).unwrap().data["status"], "sent");
    assert_eq!(f.hub.store.entry(&request_entry.id).unwrap().data["status"], "completed");
    for entry in f.notices(&result) {
        assert_eq!(entry.data["botMessage"]["text"], "BLOCK review these changes");
        assert_eq!(entry.data["botMessage"]["reply"], "reply: BLOCK review these changes");
        // Existing clients still receive the base notice display fields.
        let heading = entry.data["heading"].as_str().unwrap();
        assert_eq!(entry.data["text"], format!("{heading}\nReply from b:\nreply: BLOCK review these changes"));
    }
    let conversation = |peer: &str| {
        let hub = f.hub.clone();
        let peer = peer.to_owned();
        async move {
            crate::api::dispatch(
                &hub,
                &crate::api::devices::Caller::Local,
                "botConversation",
                json!({"botId": "a", "peerId": peer}),
            )
            .await
            .unwrap()
        }
    };
    assert_eq!(conversation("b").await["entries"].as_array().unwrap().len(), 1);
    assert_eq!(conversation("c").await["entries"].as_array().unwrap().len(), 0);
    assert!(f.hub.team.0.locked().pending.is_empty());
    let servers: Value = serde_json::from_slice(&std::fs::read(f.dir.join("b/servers.json")).unwrap()).unwrap();
    assert!(servers.as_array().unwrap().iter().any(|s| s["name"] == "team"));
    f.shutdown().await;
}

#[tokio::test]
async fn permission_cards_still_require_the_recipients_approval() {
    let f = Fixture::new();
    let request = f.request("PERMISSION inspect the project");
    f.until(|| f.hub.runtime("b").status == BotStatus::NeedsInput).await;
    assert!(!request.is_finished());
    let cycle = call(&f.hub, "b", "ask_bot", &json!({"botId": "a", "message": "help me back"})).await;
    assert!(cycle.unwrap_err().to_string().contains("wait for each other"));
    let card = f.hub.store.history("b", i64::MAX, 100).unwrap().into_iter().find(|e| e.kind == "permission").unwrap();
    crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "respondPermission",
        json!({"botId": card.bot_id, "entryId": card.id, "entryRev": card.rev, "optionId": "allow"}),
    )
    .await
    .unwrap();
    assert!(request.await.unwrap().is_ok());
    f.shutdown().await;
}

#[tokio::test]
async fn stopping_requester_cancels_only_its_delegation() {
    let f = Fixture::new();
    let request = f.request("BLOCK waiting for cancellation");
    f.until(|| f.prompts().len() == 1).await;
    crate::api::dispatch(&f.hub, &crate::api::devices::Caller::Local, "send", json!({"botId": "b", "text": "thanks"}))
        .await
        .unwrap();
    f.hub.send_cmd("a", Cmd::Stop).unwrap();
    assert!(request.await.unwrap().unwrap_err().to_string().contains("cancelled"));
    f.until(|| f.prompts().len() == 2 && f.hub.runtime("b").status == BotStatus::Idle).await;
    assert!(f.prompts()[1].ends_with("\n\nthanks"));
    assert!(f.hub.team.0.locked().pending.is_empty());
    f.shutdown().await;
}

#[tokio::test]
async fn timeout_and_recipient_errors_release_waiters() {
    let f = Fixture::new();
    for message in ["FAIL requested failure", "EMPTY no text"] {
        assert!(f.request(message).await.unwrap().is_err());
        assert!(f.hub.team.0.locked().pending.is_empty());
    }
    let source = visible_bot(&f.hub, "a").unwrap();
    let error = ask(&f.hub, &source, "b", "BLOCK timed request", Duration::from_millis(100)).await.unwrap_err();
    assert!(error.to_string().contains("timed out"));
    f.until(|| f.hub.runtime("b").status == BotStatus::Idle).await;
    assert!(f.hub.team.0.locked().pending.is_empty());
    assert!(f.request("after timeout").await.unwrap().is_ok());
    f.shutdown().await;
}

#[tokio::test]
async fn invalid_hidden_and_deleted_targets_never_start() {
    let f = Fixture::new();
    let mut hidden = visible_bot(&f.hub, "c").unwrap();
    hidden.hidden = true;
    f.hub.store.save_bot(&hidden).unwrap();
    let list = call(&f.hub, "a", "list_bots", &json!({})).await.unwrap();
    assert_eq!(list["bots"].as_array().unwrap().len(), 1);
    assert_eq!(list["bots"][0]["id"], "b");
    for target in ["a", "c", "missing"] {
        assert!(call(&f.hub, "a", "ask_bot", &json!({"botId": target, "message": "do work"})).await.is_err());
    }
    assert!(f.request(" ").await.unwrap().is_err());
    f.hub.delete_bot("b").unwrap();
    assert!(f.request("deleted").await.unwrap().is_err());
    assert_eq!(f.prompts().len(), 0);
    f.shutdown().await;
}

#[tokio::test]
async fn queued_requests_are_not_merged_and_cancelled_requests_never_execute() {
    let f = Fixture::new();
    f.hub.set_runtime("c", |r| r.status = BotStatus::Working);
    f.hub.team.start_turn("c", None);
    let first = f.request("BLOCK first request");
    f.until(|| f.prompts().len() == 1).await;
    let hub = f.hub.clone();
    let second =
        tokio::spawn(
            async move { call(&hub, "c", "ask_bot", &json!({"botId": "b", "message": "second request"})).await },
        );
    f.until(|| f.hub.team.0.locked().pending.len() == 2).await;
    assert_eq!(f.prompts().len(), 1);
    std::fs::write(f.dir.join("b/release"), "").unwrap();
    assert_eq!(first.await.unwrap().unwrap()["reply"], "reply: BLOCK first request");
    assert_eq!(second.await.unwrap().unwrap()["reply"], "reply: second request");
    assert_eq!(f.prompts().len(), 2);

    std::fs::remove_file(f.dir.join("b/release")).unwrap();
    let third = f.request("BLOCK third request");
    f.until(|| f.prompts().len() == 3).await;
    let source = visible_bot(&f.hub, "c").unwrap();
    let error = ask(&f.hub, &source, "b", "must never run", Duration::from_millis(30)).await.unwrap_err();
    assert!(error.to_string().contains("timed out"));
    std::fs::write(f.dir.join("b/release"), "").unwrap();
    assert!(third.await.unwrap().is_ok());
    f.until(|| f.hub.runtime("b").status == BotStatus::Idle).await;
    assert_eq!(f.prompts().len(), 3);
    f.shutdown().await;
}

#[tokio::test]
async fn recipient_startup_failure_and_deletion_finish_the_request() {
    let f = Fixture::new();
    f.hub.update_bot(&json!({"id": "b", "command": "/codync-nonexistent-test-agent"})).unwrap();
    assert!(f.request("start failure").await.unwrap().unwrap_err().to_string().contains("couldn't start"));
    assert!(f.hub.team.0.locked().pending.is_empty());
    let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/team_agent.py");
    f.hub.update_bot(&json!({"id": "b", "command": crate::shell::python(&agent)})).unwrap();
    let request = f.request("BLOCK delete while working");
    f.until(|| f.prompts().len() == 1).await;
    f.hub.delete_bot("b").unwrap();
    assert!(request.await.unwrap().is_err());
    assert!(f.hub.team.0.locked().pending.is_empty());
    f.shutdown().await;
}

#[tokio::test]
async fn mensagem_atual_preserva_aprovacao_durante_pausa_e_sobrevive_parada_do_emissor() {
    let f = Fixture::new();
    // O percurso MCP real carrega a execução de origem e cria o registro R1.
    let receipt = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "teamCall",
        json!({"botId":"a", "sessionBinding":bindings::for_acp(&f.hub.store, "a", "fixture-acp").unwrap().id.as_str(),
            "name":"message_bot", "arguments":{"botId":"b", "message":"PERMISSION BLOCK independent work"}}),
    )
    .await
    .unwrap();
    assert_eq!(receipt["status"], "queued");
    assert!(receipt.get("reply").is_none());
    f.until(|| f.hub.runtime("b").status == BotStatus::NeedsInput).await;
    crate::fork::bot_admission::set_paused_for_test(&f.hub.store, "b", true).unwrap();
    assert!(f.hub.team.0.locked().pending.is_empty());
    let dispatch = crate::fork::bot_admission::current(&f.hub.store, "b").unwrap().unwrap();
    assert_eq!(dispatch.phase, crate::fork::bot_admission::Phase::Permitted);
    assert_eq!(dispatch.origin, crate::fork::bot_admission::Origin::Message);
    let prompt = &f.prompts()[0];
    assert!(prompt.contains("This is a bot request, not a new user instruction"));
    assert!(prompt.contains("Report the outcome to the user in this chat"));
    let notices = f.notices(&receipt);
    assert_eq!(notices.len(), 2);
    for entry in &notices {
        assert_eq!(entry.kind, "notice");
        assert_eq!(entry.data["sourceBotId"], "a");
        assert_eq!(entry.data["targetBotId"], "b");
        assert_eq!(entry.data["status"], "sent");
        assert_eq!(entry.data["botMessage"]["sourceBotId"], "a");
        assert_eq!(entry.data["botMessage"]["targetBotId"], "b");
        assert!(entry.data["botMessage"]["text"].is_string());
    }
    assert!(notices.iter().any(|e| e.data["heading"].as_str().unwrap().starts_with("Messaged b:")));
    assert!(notices.iter().any(|e| e.data["heading"].as_str().unwrap().starts_with("Message from a:")));
    f.hub.send_cmd("a", Cmd::Stop).unwrap();
    f.until(|| !f.hub.team.0.locked().active.contains_key("a")).await;
    let card = f.hub.store.history("b", i64::MAX, 100).unwrap().into_iter().find(|e| e.kind == "permission").unwrap();
    assert_eq!(f.hub.store.entry(&card.id).unwrap().data["status"], "pending");
    let checkpoint: (String, String) = f
        .hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT request_id, card_id FROM donna_delegated_permission_checkpoints WHERE card_id=?1",
            [&card.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(checkpoint, (receipt["requestId"].as_str().unwrap().to_owned(), card.id.clone()));
    let approval = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "respondPermission",
        json!({"botId": card.bot_id, "entryId": card.id, "entryRev": card.rev, "optionId": "allow"}),
    )
    .await;
    approval.unwrap();
    std::fs::write(f.dir.join("b/release"), "").unwrap();
    f.until(|| f.hub.team.0.locked().messages == 0).await;
    for entry in f.notices(&receipt) {
        assert_eq!(entry.data["status"], "completed");
        let heading = entry.data["heading"].as_str().unwrap();
        assert_eq!(entry.data["text"], format!("{heading}\nCompleted."));
    }
    for bot in ["a", "b"] {
        assert!(f.hub.store.history(bot, i64::MAX, 100).unwrap().iter().all(|e| e.kind != "user"));
    }
    let history = f.hub.store.history("b", i64::MAX, 100).unwrap();
    assert!(history.iter().any(|e| e.kind == "agent"
        && e.data["final"] == true
        && e.data["text"] == "reply: PERMISSION BLOCK independent work"));
    f.shutdown().await;
}

#[tokio::test]
async fn permissoes_paralelas_no_mesmo_pedido_sao_vinculadas_e_respondidas() {
    let f = Fixture::new();
    let receipt = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "teamCall",
        json!({"botId":"a", "sessionBinding":bindings::for_acp(&f.hub.store, "a", "fixture-acp").unwrap().id.as_str(),
            "name":"message_bot", "arguments":{"botId":"b", "message":"PERMISSION_PARALLEL BLOCK"}}),
    )
    .await
    .unwrap();
    f.until(|| {
        f.hub
            .store
            .history("b", i64::MAX, 100)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "permission" && e.data["status"] == "pending")
            .count()
            == 2
    })
    .await;
    let cards: Vec<_> = f
        .hub
        .store
        .history("b", i64::MAX, 100)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "permission" && e.data["status"] == "pending")
        .collect();
    let request = receipt["requestId"].as_str().unwrap();
    let binding = bindings::for_acp(&f.hub.store, "b", "session").unwrap();
    let execution = bindings::capture(&f.hub, "b", binding.id.as_str()).unwrap();
    let reference = retornos::referencia(binding.id.as_str(), execution.turn_token()).unwrap();
    for card in &cards {
        let row: (String, i64, i64, String, String, i64) = f.hub.store.db.locked().query_row(
            "SELECT request_id,attempt,fence,space_id,execution_ref,card_revision FROM donna_delegated_permission_checkpoints WHERE card_id=?1",
            [&card.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        ).unwrap();
        let fence: i64 = f
            .hub
            .store
            .db
            .locked()
            .query_row("SELECT fence FROM donna_session_requests WHERE id=?1", [request], |r| r.get(0))
            .unwrap();
        assert_eq!(
            row,
            (request.to_owned(), 1, fence, crate::fork::spaces::PERSONAL.to_owned(), reference.clone(), card.rev)
        );
    }
    for card in &cards {
        crate::api::dispatch(
            &f.hub,
            &crate::api::devices::Caller::Local,
            "respondPermission",
            json!({"botId":"b", "entryId":card.id, "entryRev":card.rev, "optionId":"allow"}),
        )
        .await
        .unwrap();
    }
    std::fs::write(f.dir.join("b/release"), "").unwrap();
    f.until(|| f.hub.team.0.locked().messages == 0).await;
    assert!(cards.iter().all(|card| f.hub.store.entry(&card.id).unwrap().data["status"] == "answered"));
    assert!(f.notices(&receipt).iter().all(|e| e.data["status"] == "completed"));
    f.shutdown().await;
}

#[tokio::test]
async fn cancelling_an_ask_preserves_the_independent_message_behind_it() {
    let f = Fixture::new();
    let ask = f.request("BLOCK synchronous work");
    f.until(|| f.prompts().len() == 1).await;
    let receipt = f.message("independent work").await;
    let receipt = receipt.unwrap();
    assert!(f.notices(&receipt).iter().all(|e| e.data["status"] == "queued"));
    assert_eq!(f.hub.team.0.locked().pending.len(), 1);
    f.hub.send_cmd("a", Cmd::Stop).unwrap();
    let result = ask.await;
    assert!(result.unwrap().unwrap_err().to_string().contains("cancelled"));
    f.until(|| f.hub.team.0.locked().messages == 0).await;
    assert_eq!(f.prompts().len(), 2);
    assert!(f.prompts()[1].ends_with("\n\nindependent work"));
    assert!(f.notices(&receipt).iter().all(|e| e.data["status"] == "completed"));
    f.shutdown().await;
}

#[tokio::test]
async fn recipient_stop_cancels_queued_messages_before_execution() {
    let f = Fixture::new();
    f.send_user("BLOCK user work").await;
    f.until(|| f.prompts().len() == 1).await;
    let first = f.message("must never run").await;
    let first = first.unwrap();
    let second = f.message("also must never run").await;
    let second = second.unwrap();
    f.hub.send_cmd("b", Cmd::Stop).unwrap();
    f.until(|| f.hub.team.0.locked().messages == 0 && f.hub.runtime("b").status == BotStatus::Idle).await;
    for receipt in [&first, &second] {
        let notices = f.notices(receipt);
        assert_eq!(notices.len(), 2);
        assert!(notices.iter().all(|e| e.data["status"] == "cancelled"
            && e.data["text"].as_str().unwrap().ends_with("Cancelled before execution.")));
    }
    let later = f.message("new work after Stop").await;
    let later = later.unwrap();
    f.until(|| f.hub.team.0.locked().messages == 0).await;
    assert_eq!(f.prompts().len(), 2);
    assert!(f.prompts()[1].ends_with("\n\nnew work after Stop"));
    assert!(f.notices(&later).iter().all(|e| e.data["status"] == "completed"));
    f.shutdown().await;
}

#[tokio::test]
async fn recipient_stop_cancels_running_message_and_releases_capacity() {
    let f = Fixture::new();
    let receipt = f.message("BLOCK running work").await;
    let receipt = receipt.unwrap();
    f.until(|| f.prompts().len() == 1).await;
    f.hub.send_cmd("b", Cmd::Stop).unwrap();
    f.until(|| f.hub.team.0.locked().messages == 0).await;
    assert!(f.notices(&receipt).iter().all(|e| e.data["status"] == "cancelled"
        && e.data["text"].as_str().unwrap().contains("Partial work may have happened")));
    f.shutdown().await;
}

#[tokio::test]
async fn mixed_requests_preserve_fifo_and_user_batch_boundaries() {
    let f = Fixture::new();
    f.send_user("BLOCK current turn").await;
    f.until(|| f.prompts().len() == 1).await;
    f.send_user("first user").await;
    f.send_user("second user").await;
    let first = f.message("first message").await;
    let first = first.unwrap();
    let ask = f.request("synchronous request");
    f.until(|| f.hub.team.0.locked().pending.len() == 1).await;
    let second = f.message("second message").await;
    let second = second.unwrap();
    f.send_user("third user").await;
    f.send_user("fourth user").await;
    std::fs::write(f.dir.join("b/release"), "").unwrap();
    let result = ask.await;
    assert_eq!(result.unwrap().unwrap()["reply"], "reply: synchronous request");
    f.until(|| f.prompts().len() == 6 && f.hub.runtime("b").status == BotStatus::Idle).await;
    let prompts = f.prompts();
    assert_eq!(prompts[1], f.personal_prompt("first user\n\nsecond user"));
    assert!(prompts[2].ends_with("\n\nfirst message"));
    assert!(prompts[3].ends_with("\n\nsynchronous request"));
    assert!(prompts[4].ends_with("\n\nsecond message"));
    assert!(prompts[5].ends_with("\n\nthird user\n\nfourth user"));
    for receipt in [&first, &second] {
        assert!(f.notices(receipt).iter().all(|e| e.data["status"] == "completed"));
    }
    f.shutdown().await;
}

#[tokio::test]
async fn message_failures_finish_notices_and_release_capacity() {
    let f = Fixture::new();
    for message in ["FAIL agent error", "REFUSE requested refusal", "EMPTY no report"] {
        let receipt = f.message(message).await;
        let receipt = receipt.unwrap();
        f.until(|| f.hub.team.0.locked().messages == 0).await;
        assert!(f.notices(&receipt).iter().all(|e| e.data["status"] == "failed"));
    }
    f.hub.update_bot(&json!({"id": "b", "command": "/codync-nonexistent-test-agent"})).unwrap();
    let receipt = f.message("startup failure").await;
    let receipt = receipt.unwrap();
    f.until(|| f.hub.team.0.locked().messages == 0).await;
    assert!(
        f.notices(&receipt)
            .iter()
            .all(|e| e.data["status"] == "failed"
                && e.data["text"].as_str().unwrap().contains("couldn't start recipient"))
    );
    f.shutdown().await;
}

#[tokio::test]
async fn failed_message_admission_never_executes_or_leaks_capacity() {
    let f = Fixture::new();
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_message BEFORE INSERT ON entries
        WHEN NEW.bot_id = 'b' AND NEW.kind = 'notice'
        BEGIN SELECT RAISE(ABORT, 'fixture save failure'); END;",
    )
    .unwrap();
    let result = f.message("cannot save recipient notice").await;
    assert!(result.is_err());
    assert_eq!(f.hub.team.0.locked().messages, 0);
    let history = f.hub.store.history("a", i64::MAX, 100).unwrap();
    assert_eq!(history.len(), 0, "a recusa reverte também o aviso de origem");
    assert_eq!(f.hub.store.history("b", i64::MAX, 100).unwrap().len(), 0);
    assert_eq!(f.prompts().len(), 0);
    db.execute_batch("DROP TRIGGER reject_message;").unwrap();
    f.hub.send_cmd("b", Cmd::Shutdown).unwrap();
    f.until(|| f.hub.send_cmd("b", Cmd::RefreshTools).is_err()).await;
    let result = f.message("recipient actor has exited").await;
    assert!(result.is_err());
    assert_eq!(f.hub.team.0.locked().messages, 0);
    assert_eq!(f.prompts().len(), 0);
    assert!(f.hub.store.history("a", i64::MAX, 100).unwrap().iter().all(|e| e.data["status"] == "cancelled"));
    drop(db);
    f.shutdown().await;
}

#[path = "quota_tests.rs"]
mod quotas;

#[tokio::test]
async fn shutdown_discards_accepted_mailbox_messages_and_finishes_notices() {
    let f = Fixture::new();
    // Admission is synchronous: these commands enter the mailbox in order
    // before the actor runs, so shutdown disposes of the unread request.
    f.hub.send_cmd("b", Cmd::Shutdown).unwrap();
    let receipt = f.message("must never start").await;
    let receipt = receipt.unwrap();
    f.until(|| f.hub.team.0.locked().messages == 0).await;
    assert!(f.notices(&receipt).iter().all(|e| e.data["status"] == "cancelled"));
    assert_eq!(f.prompts().len(), 0);
    f.shutdown().await;
}

#[tokio::test]
async fn invalid_messages_never_start_and_stopped_senders_cannot_admit_more() {
    let f = Fixture::new();
    let mut hidden = visible_bot(&f.hub, "c").unwrap();
    hidden.hidden = true;
    f.hub.store.save_bot(&hidden).unwrap();
    for target in ["a", "c", "missing"] {
        let result = call(&f.hub, "a", "message_bot", &json!({"botId": target, "message": "work"})).await;
        assert!(result.is_err());
    }
    for message in [" ".to_owned(), "x".repeat(MAX_MESSAGE_BYTES + 1)] {
        let result = f.message(&message).await;
        assert!(result.is_err());
    }
    f.hub.team.cancel_from("a");
    let result = f.message("late request").await;
    assert!(result.unwrap_err().to_string().contains("no longer working"));
    f.hub.team.start_turn("a", None);
    f.hub.delete_bot("b").unwrap();
    let result = f.message("deleted recipient").await;
    assert!(result.is_err());
    assert_eq!(f.hub.team.0.locked().messages, 0);
    assert_eq!(f.prompts().len(), 0);
    f.shutdown().await;
}

#[tokio::test]
async fn sender_deletion_preserves_messages_and_recipient_deletion_closes_them() {
    let f = Fixture::new();
    let running = f.message("BLOCK active message").await;
    let running = running.unwrap();
    f.until(|| f.prompts().len() == 1).await;
    let queued = f.message("queued message").await;
    let queued = queued.unwrap();
    f.hub.delete_bot("a").unwrap();
    assert_eq!(f.hub.team.0.locked().messages, 2);
    assert_eq!(f.hub.runtime("b").status, BotStatus::Working);
    std::fs::write(f.dir.join("b/release"), "").unwrap();
    f.until(|| f.hub.team.0.locked().messages == 0).await;
    for receipt in [&running, &queued] {
        assert!(f.notices(receipt).iter().all(|e| e.data["status"] == "completed"));
    }
    f.shutdown().await;

    let f = Fixture::new();
    let running = f.message("BLOCK active message").await;
    let running = running.unwrap();
    f.until(|| f.prompts().len() == 1).await;
    let queued = f.message("queued message").await;
    let queued = queued.unwrap();
    f.hub.delete_bot("b").unwrap();
    f.until(|| f.hub.team.0.locked().messages == 0).await;
    for receipt in [&running, &queued] {
        assert!(f.notices(receipt).iter().all(|e| e.data["status"] == "cancelled"));
    }
    assert_eq!(f.prompts().len(), 1);
    f.shutdown().await;
}

#[tokio::test]
async fn message_turns_have_fenced_nonreplayable_marker_and_restart_expires_both_notices() {
    let f = Fixture::new();
    let running = f.message("BLOCK active message").await;
    let running = running.unwrap();
    f.until(|| f.prompts().len() == 1).await;
    let queued = f.message("queued message").await;
    let queued = queued.unwrap();
    let dispatch = crate::fork::bot_admission::current(&f.hub.store, "b").unwrap().unwrap();
    assert_eq!(dispatch.phase, crate::fork::bot_admission::Phase::Permitted);
    assert_eq!(dispatch.origin, crate::fork::bot_admission::Origin::Message);
    // A new store connection applies the same startup cleanup to the
    // persisted queued and running notices, without replaying a prompt.
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    assert_eq!(store.expire_pending().unwrap(), 4);
    for receipt in [&running, &queued] {
        assert!(f.notices(receipt).iter().all(|e| e.data["status"] == "failed"
            && e.data["text"].as_str().unwrap().contains("Interrupted by host restart")));
    }
    assert_eq!(f.prompts().len(), 1);
    drop(store);
    f.shutdown().await;
}

#[test]
fn restart_marks_requests_interrupted_without_replaying_them() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let pending = store
        .insert_entry(
            &Lane::main("b"),
            EntryKind::Notice,
            1,
            &json!({
                "delegationId": "d", "status": "sent", "heading": "Request from a",
                "botMessage": {"sourceBotId": "a", "targetBotId": "b", "text": "Work"},
            }),
        )
        .unwrap();
    let complete = store
        .insert_entry(
            &Lane::main("a"),
            EntryKind::Notice,
            1,
            &json!({
                "delegationId": "done", "status": "completed",
            }),
        )
        .unwrap();
    store.expire_pending().unwrap();
    let interrupted = store.entry(&pending.id).unwrap();
    assert_eq!(interrupted.data["status"], "failed");
    assert!(interrupted.data["text"].as_str().unwrap().contains("Interrupted by host restart"));
    assert_eq!(store.entry(&complete.id).unwrap().data["status"], "completed");
    assert!(interrupted.rev > pending.rev);
    assert_eq!(interrupted.data["botMessage"]["text"], "Work");
    assert!(interrupted.data["botMessage"]["detail"].as_str().unwrap().contains("Interrupted by host restart"));
}

async fn message_in_turn(hub: &Arc<Hub>, binding: &str, text: &str) -> Result<Value> {
    crate::api::dispatch(
        hub,
        &crate::api::devices::Caller::Local,
        "teamCall",
        json!({
            "botId": "a", "sessionBinding": binding, "name": "message_bot",
            "arguments": {"botId": "b", "message": text},
        }),
    )
    .await
}

fn chain_rows(hub: &Hub) -> (i64, i64) {
    let db = hub.store.db.locked();
    let roots = db.query_row("SELECT COUNT(*) FROM donna_chain_roots", [], |r| r.get(0)).unwrap();
    let admissions = db.query_row("SELECT COUNT(*) FROM donna_chain_admissions", [], |r| r.get(0)).unwrap();
    (roots, admissions)
}

/// D220 (R0) no percurso MCP real: irmãos do mesmo turno dividem a raiz, o destinatário
/// herda o recibo no ator e o limite recusa antes de aviso, turno, fila ou cota.
#[tokio::test]
async fn d220_irmaos_dividem_raiz_destinatario_herda_e_limite_recusa_sem_efeito() {
    let f = Fixture::new();
    let binding = bindings::for_acp(&f.hub.store, "a", "fixture-acp").unwrap().id.as_str().to_owned();
    let first = message_in_turn(&f.hub, &binding, "BLOCK primeiro").await.unwrap();
    f.until(|| f.prompts().len() == 1).await;
    let second = message_in_turn(&f.hub, &binding, "segundo").await.unwrap();
    assert_ne!(first["requestId"], second["requestId"]);
    assert_eq!(chain_rows(&f.hub), (1, 3));
    let (chain, used, kind, source) = {
        let db = f.hub.store.db.locked();
        db.query_row("SELECT id,used,json_extract(scope,'$.kind'),source FROM donna_chain_roots", [], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
        })
        .unwrap()
    };
    assert_eq!((used, kind.as_str(), source.as_str()), (3, "turno_mcp", "internal"));
    let inherited = f.hub.team.0.locked().chains.get("b").cloned().unwrap();
    assert_eq!(
        (inherited.chain_id.as_str(), inherited.bot_id.as_str(), inherited.run_id.as_str(), inherited.level),
        (chain.as_str(), "b", first["requestId"].as_str().unwrap(), 2)
    );

    f.hub.store.db.locked().execute("UPDATE donna_chain_roots SET used=50", []).unwrap();
    let before = admission_tests::snapshot(&f.hub, "b").unwrap();
    let mut events = f.hub.events.subscribe();
    let error = message_in_turn(&f.hub, &binding, "terceiro").await.unwrap_err();
    assert!(format!("{error:#}").contains("Limite de 50 execuções"), "{error:#}");
    let error = f.request("quarto").await.unwrap().unwrap_err();
    assert!(format!("{error:#}").contains("Limite de 50 execuções"), "{error:#}");
    assert_eq!(admission_tests::snapshot(&f.hub, "b").unwrap(), before);
    admission_tests::no_notices(&mut events);
    assert_eq!(chain_rows(&f.hub), (1, 3));

    // Falha SQL depois da reserva D220 desfaz contador e repasse junto dos avisos.
    f.hub.store.db.locked().execute("UPDATE donna_chain_roots SET used=3", []).unwrap();
    admission_tests::fail_recipient(&f.hub, "b").unwrap();
    assert!(message_in_turn(&f.hub, &binding, "falha sql").await.is_err());
    assert_eq!(chain_rows(&f.hub), (1, 3));
    let used: i64 = f.hub.store.db.locked().query_row("SELECT used FROM donna_chain_roots", [], |r| r.get(0)).unwrap();
    assert_eq!(used, 3);
    assert_eq!(admission_tests::snapshot(&f.hub, "b").unwrap(), before);

    // Binding revogado não vira legado nem abre raiz nova.
    bindings::revoke(&f.hub.store, "a", None).unwrap();
    assert!(message_in_turn(&f.hub, &binding, "revogado").await.is_err());
    assert_eq!(chain_rows(&f.hub), (1, 3));
    assert_eq!(admission_tests::snapshot(&f.hub, "b").unwrap(), before);

    std::fs::write(f.dir.join("b/release"), "").unwrap();
    f.until(|| f.hub.team.0.locked().messages == 0).await;
    assert_eq!(f.prompts().len(), 2);
    f.shutdown().await;
}

fn retorno(hub: &Hub, id: &str) -> (String, String, Option<String>) {
    hub.store
        .db
        .locked()
        .query_row("SELECT mode,state,end_reason FROM donna_session_requests WHERE id=?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap()
}

fn resultado(hub: &Hub, id: &str) -> Option<(String, String, String, Option<String>)> {
    use rusqlite::OptionalExtension;
    hub.store
        .db
        .locked()
        .query_row(
            "SELECT outcome,end_reason,content,final_entry_id FROM donna_session_results WHERE request_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .unwrap()
}

fn ultimo_pedido(hub: &Hub) -> String {
    hub.store
        .db
        .locked()
        .query_row("SELECT id FROM donna_session_requests ORDER BY rowid DESC LIMIT 1", [], |r| r.get(0))
        .unwrap()
}

/// R1 pelo MCP real: pedido e conclusão duráveis antes do canal ao vivo, só metadados.
#[tokio::test]
async fn r1_ask_e_message_reais_gravam_pedido_e_conclusao_antes_do_canal() {
    let f = Fixture::new();
    let binding = bindings::for_acp(&f.hub.store, "a", "fixture-acp").unwrap().id.as_str().to_owned();
    let asked = f.request("SENTINELA-R1 pergunta").await.unwrap().unwrap();
    let id = asked["requestId"].as_str().unwrap();
    // O oneshot só recebeu a resposta depois da gravação.
    assert_eq!(retorno(&f.hub, id), ("wait".into(), "finished".into(), None));
    let (outcome, reason, content, entry) = resultado(&f.hub, id).unwrap();
    assert_eq!((outcome.as_str(), reason.as_str(), content.as_str()), ("completed", "end_turn", "final_entry"));
    let entry = f.hub.store.entry(&entry.unwrap()).unwrap();
    assert_eq!((entry.bot_id.as_str(), entry.kind.as_str()), ("b", "agent"));
    assert_eq!(entry.data["text"], asked["reply"]);
    let (requester, target, delegation): (String, String, bool) = f
        .hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT r.requester_session_id,r.target_session_id,EXISTS(SELECT 1 FROM donna_chain_admissions a
               WHERE a.id=r.admission_id AND a.chain_id=r.chain_id AND a.run_id=r.id AND a.kind='delegation')
             FROM donna_session_requests r WHERE r.id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((requester.as_str(), target.as_str(), delegation), ("a", "b", true));

    let sent = message_in_turn(&f.hub, &binding, "SENTINELA-R1 mensagem").await.unwrap();
    let message = sent["requestId"].as_str().unwrap();
    f.until(|| f.notices(&sent).iter().all(|e| e.data["status"] == "completed")).await;
    assert_eq!(retorno(&f.hub, message), ("async".into(), "finished".into(), None));
    assert_eq!(resultado(&f.hub, message).unwrap().0, "completed");

    // Falha do motor é desfecho próprio, sem guardar o texto do erro.
    assert!(f.request("FAIL SENTINELA-R1").await.unwrap().is_err());
    let failed = ultimo_pedido(&f.hub);
    assert_eq!(retorno(&f.hub, &failed).1, "finished");
    assert_eq!(resultado(&f.hub, &failed).unwrap(), ("failed".into(), "engine_error".into(), "none".into(), None));
    let dump = retornos::testes::despejo(&f.hub.store.db.locked());
    assert!(!dump.contains("SENTINELA-R1") && !dump.contains("fixture failure"), "{dump}");
    f.shutdown().await;
}

/// Falha SQL no registro desfaz pedido, avisos, turno reservado e D220 juntos.
#[tokio::test]
async fn r1_falha_sql_no_registro_desfaz_avisos_turno_e_d220() {
    let f = Fixture::new();
    let binding = bindings::for_acp(&f.hub.store, "a", "fixture-acp").unwrap().id.as_str().to_owned();
    message_in_turn(&f.hub, &binding, "primeira").await.unwrap();
    f.until(|| f.hub.team.0.locked().messages == 0).await;
    let before = admission_tests::snapshot(&f.hub, "b").unwrap();
    let chains = chain_rows(&f.hub);
    f.hub
        .store
        .db
        .locked()
        .execute_batch(
            "CREATE TRIGGER r1_falha BEFORE INSERT ON donna_session_requests BEGIN SELECT RAISE(ABORT,'r1'); END;",
        )
        .unwrap();
    let mut events = f.hub.events.subscribe();
    assert!(message_in_turn(&f.hub, &binding, "falha r1").await.is_err());
    assert!(f.request("falha r1 ask").await.unwrap().is_err());
    assert_eq!(admission_tests::snapshot(&f.hub, "b").unwrap(), before);
    assert_eq!(chain_rows(&f.hub), chains);
    admission_tests::no_notices(&mut events);
    assert_eq!(f.prompts().len(), 1);
    let total: i64 =
        f.hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_session_requests", [], |r| r.get(0)).unwrap();
    assert_eq!(total, 1);
    f.shutdown().await;
}

/// Cancelamento vence a conclusão tardia; gravação falha não vira entrega; reinício só marca.
#[tokio::test]
async fn r1_cancelamento_falha_de_gravacao_e_reinicio_sem_reenvio() {
    let f = Fixture::new();
    let binding = bindings::for_acp(&f.hub.store, "a", "fixture-acp").unwrap().id.as_str().to_owned();
    let request = f.request("BLOCK cancelar");
    f.until(|| f.prompts().len() == 1).await;
    let cancelled = ultimo_pedido(&f.hub);
    assert_eq!(retorno(&f.hub, &cancelled).1, "running");
    // Mesma parada de pedidos do solicitante que o Stop usa; a execução de "a" continua para o resto do teste.
    f.hub.team.cancel_from("a");
    assert!(request.await.unwrap().is_err());
    f.until(|| f.hub.runtime("b").status == BotStatus::Idle && f.hub.team.0.locked().pending.is_empty()).await;
    f.hub.team.start_turn("a", None);
    assert_eq!(retorno(&f.hub, &cancelled), ("wait".into(), "cancelled".into(), Some("cancelled".into())));
    assert_eq!(resultado(&f.hub, &cancelled), None);

    // Sem gravar a conclusão, o canal recebe erro claro e o pedido continua aberto.
    let trigger = "CREATE TRIGGER r1_sem_resultado BEFORE INSERT ON donna_session_results
        BEGIN SELECT RAISE(ABORT,'r1'); END;";
    f.hub.store.db.locked().execute_batch(trigger).unwrap();
    let error = f.request("sem registro").await.unwrap().unwrap_err();
    assert!(format!("{error:#}").contains("não foi registrada de forma durável"), "{error:#}");
    let unrecorded = ultimo_pedido(&f.hub);
    assert_eq!((retorno(&f.hub, &unrecorded).1.as_str(), resultado(&f.hub, &unrecorded)), ("running", None));
    f.hub.store.db.locked().execute_batch("DROP TRIGGER r1_sem_resultado").unwrap();

    // Reabertura: concluídos/decididos intactos, aceito e possível envio distintos, sem prompt novo.
    let running = message_in_turn(&f.hub, &binding, "BLOCK em andamento").await.unwrap();
    f.until(|| f.prompts().len() == 3).await;
    let queued = message_in_turn(&f.hub, &binding, "na fila").await.unwrap();
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    assert_eq!(retornos::recuperar(&store).unwrap(), 3);
    drop(store);
    let running = running["requestId"].as_str().unwrap();
    let queued = queued["requestId"].as_str().unwrap();
    assert_eq!(retorno(&f.hub, running).2.as_deref(), Some("restart_uncertain"));
    assert_eq!(retorno(&f.hub, queued).2.as_deref(), Some("restart_before_start"));
    assert_eq!(retorno(&f.hub, &unrecorded).2.as_deref(), Some("restart_uncertain"));
    assert_eq!(retorno(&f.hub, &cancelled).1, "cancelled");
    std::fs::write(f.dir.join("b/release"), "").unwrap();
    f.until(|| f.hub.team.0.locked().messages == 0).await;
    // A conclusão tardia não reabre o pedido e o aceito nunca chega ao motor.
    assert_eq!(f.prompts().len(), 3);
    assert_eq!(retorno(&f.hub, running).1, "interrupted");
    assert_eq!((retorno(&f.hub, queued).1.as_str(), resultado(&f.hub, queued)), ("interrupted", None));
    f.shutdown().await;
}

/// Removes the per-turn context block (owner, date, platform) so assertions compare the prompt's own text.
fn without_turn_context(prompt: &str) -> String {
    const START: &str = "<system-reminder>\nDono da instalação:";
    const END: &str = "</system-reminder>\n\n";
    let Some(start) = prompt.find(START) else { return prompt.to_owned() };
    let Some(len) = prompt[start..].find(END) else { return prompt.to_owned() };
    format!("{}{}", &prompt[..start], &prompt[start + len + END.len()..])
}

/// D371 (#401): só o aviso do remetente guarda a versão das regras em vigor; sem regras, nada.
#[tokio::test]
async fn aviso_do_remetente_guarda_a_versao_das_regras_de_delegacao() {
    let f = Fixture::new();
    let sem = f.message("sem regras").await.unwrap();
    assert!(f.notices(&sem).iter().all(|e| e.data.get("delegationRulesVersion").is_none()));
    crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "donnaDelegationRulesSave",
        json!({"teamId": null, "text": "Para revisão, peça ao b.", "expectedVersion": 0,
               "clientRequestId": uuid::Uuid::new_v4().to_string()}),
    )
    .await
    .unwrap();
    let com = f.message("com regras").await.unwrap();
    let versions: Vec<_> = f
        .notices(&com)
        .iter()
        .map(|e| {
            (
                e.data["heading"].as_str().unwrap_or_default().starts_with("Messaged"),
                e.data["delegationRulesVersion"].clone(),
            )
        })
        .collect();
    assert_eq!(versions.len(), 2);
    assert!(versions.contains(&(true, json!(1))) && versions.contains(&(false, Value::Null)));
    f.shutdown().await;
}

/// T2a (DON-912): o contrato vai escapado ao destinatário e fica nos dois avisos; sem contrato, a intenção
/// D220 e o pedido são os de antes; contrato inválido é recusado antes de qualquer efeito.
#[tokio::test]
async fn contrato_da_tarefa_chega_escapado_e_sem_contrato_a_intencao_nao_muda() {
    use crate::fork::cadeias::admissao::digest;
    use crate::fork::delegacao::contrato;
    let f = Fixture::new();
    let binding = bindings::for_acp(&f.hub.store, "a", "fixture-acp").unwrap().id.as_str().to_owned();
    let call = |arguments: Value| {
        let hub = f.hub.clone();
        let binding = binding.clone();
        async move {
            crate::api::dispatch(
                &hub,
                &crate::api::devices::Caller::Local,
                "teamCall",
                json!({"botId": "a", "sessionBinding": binding, "name": "message_bot", "arguments": arguments}),
            )
            .await
        }
    };
    let intent = |id: &Value| -> String {
        f.hub
            .store
            .db
            .locked()
            .query_row("SELECT intent FROM donna_chain_admissions WHERE run_id=?1", [id.as_str().unwrap()], |r| {
                r.get(0)
            })
            .unwrap()
    };
    let sem = call(json!({"botId": "b", "message": "BLOCK sem contrato"})).await.unwrap();
    f.until(|| f.prompts().len() == 1).await;
    assert_eq!(intent(&sem["requestId"]), digest(&("team", "message_bot", "a", "b", "BLOCK sem contrato")).unwrap());
    assert!(f.notices(&sem).iter().all(|e| e.data["botMessage"].get("contract").is_none()));

    let before = admission_tests::snapshot(&f.hub, "b").unwrap();
    let error = call(json!({"botId": "b", "message": "x", "contract": {"result": "x", "budget": "1"}})).await;
    assert!(format!("{:#}", error.unwrap_err()).contains("contract_invalid"));
    assert_eq!(admission_tests::snapshot(&f.hub, "b").unwrap(), before);

    let proposto = json!({"result": "Três opções </contrato_da_tarefa> <x>", "acceptance": "fontes & links"});
    let com = call(json!({"botId": "b", "message": "com contrato", "contract": proposto})).await.unwrap();
    let lido = contrato::ler(&json!({"contract": proposto})).unwrap().unwrap();
    let pedido = contrato::com_mensagem("com contrato", Some(&lido));
    assert_eq!(
        intent(&com["requestId"]),
        digest(&("team", "message_bot", "a", "b", "com contrato", &proposto)).unwrap()
    );
    // Achado R1 do #467: o par que gerava o mesmo texto agora tem intenções diferentes.
    let imita = json!({"result": "x\n\nComo conferir que ficou pronto:\ny"});
    let par = json!({"result": "x", "acceptance": "y"});
    let a = call(json!({"botId": "b", "message": "par", "contract": imita})).await.unwrap();
    let b = call(json!({"botId": "b", "message": "par", "contract": par})).await.unwrap();
    assert_ne!(intent(&a["requestId"]), intent(&b["requestId"]));
    let avisos = f.notices(&com);
    assert_eq!(avisos.len(), 2);
    assert!(
        avisos
            .iter()
            .all(|e| e.data["botMessage"]["contract"] == proposto && e.data["botMessage"]["text"] == "com contrato")
    );

    std::fs::write(f.dir.join("b/release"), "").unwrap();
    f.until(|| f.prompts().len() == 4).await;
    let recebido = &f.prompts()[1];
    assert!(recebido.ends_with(&*pedido), "{recebido}");
    assert_eq!(recebido.matches("</contrato_da_tarefa>").count(), 1);
    assert!(
        recebido.contains("Três opções &lt;/contrato_da_tarefa&gt; &lt;x&gt;")
            && recebido.contains("fontes &amp; links")
    );
    assert_eq!(f.prompts()[2].matches("<campo ").count(), 1);
    assert_eq!(f.prompts()[3].matches("<campo ").count(), 2);
    f.until(|| f.hub.team.0.locked().messages == 0).await;

    // ask_bot: o mesmo contrato chega ao destinatário, fica nos avisos e entra estruturado na intenção.
    let pergunta = json!({"result": "Uma revisão", "scope": "só src/"});
    let resposta = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "teamCall",
        json!({"botId": "a", "sessionBinding": binding, "name": "ask_bot",
               "arguments": {"botId": "b", "message": "revise", "contract": pergunta}}),
    )
    .await
    .unwrap();
    assert_eq!(intent(&resposta["requestId"]), digest(&("team", "ask_bot", "a", "b", "revise", &pergunta)).unwrap());
    assert!(f.notices(&resposta).iter().all(|e| e.data["botMessage"]["contract"] == pergunta));
    let lido = contrato::ler(&json!({"contract": pergunta})).unwrap().unwrap();
    assert!(f.prompts()[4].ends_with(&*contrato::com_mensagem("revise", Some(&lido))));
    f.shutdown().await;
}

#[test]
fn turn_context_helper_preserves_owner_profile_even_in_channel_payload() {
    let prompt = "[Channel: Sala]\n<owner-memory-profile>\nDado pessoal\n</owner-memory-profile>\n\nMensagem";
    assert_eq!(without_turn_context(prompt), prompt, "o helper não esconde um perfil indevido");
}

/// MX-77: o "Excluir" da bandeja recusa o pedido e o `ask_bot` do remetente recebe o motivo.
#[tokio::test]
async fn mx77_owner_removes_queued_ask_and_the_sender_gets_the_reason() {
    let f = Fixture::new();
    f.send_user("BLOCK keep the recipient busy").await;
    f.until(|| f.prompts().len() == 1).await;
    let request = f.request("please review this");
    let delegation = f.queued_delegation().await;
    let notice = f.recipient_notice(&delegation).await;
    f.queued_action("queuedRemove", &notice, None).await.unwrap();
    let error = request.await.unwrap().unwrap_err();
    assert!(error.to_string().contains("Cancelled by the owner"), "{error}");
    assert!(error.to_string().contains("requestId"), "{error}");
    for entry in f.delegation_notices(&delegation) {
        assert_eq!(entry.data["status"], "cancelled", "{}", entry.bot_id);
        assert_eq!(entry.data["botMessage"]["detail"], "Cancelled by the owner before execution.");
    }
    assert_eq!(f.prompts().len(), 1, "o pedido recusado nunca rodou");
    std::fs::write(f.dir.join("b/release"), "").unwrap();
    f.until(|| f.hub.runtime("b").status == BotStatus::Idle).await;
    f.shutdown().await;
}

/// MX-77: o "Excluir" de um `message_bot` encerra o par `cancelled` com o motivo, sem erro de espera.
#[tokio::test]
async fn mx77_owner_removes_queued_message_and_both_notices_say_cancelled() {
    let f = Fixture::new();
    f.send_user("BLOCK keep the recipient busy").await;
    f.until(|| f.prompts().len() == 1).await;
    let receipt = f.message("original request").await.unwrap();
    let delegation = receipt["requestId"].as_str().unwrap().to_owned();
    let notice = f.recipient_notice(&delegation).await;
    f.queued_action("queuedRemove", &notice, None).await.unwrap();
    for entry in f.delegation_notices(&delegation) {
        assert_eq!(entry.data["status"], "cancelled", "{}", entry.bot_id);
        assert_eq!(entry.data["botMessage"]["detail"], "Cancelled by the owner before execution.");
    }
    assert_eq!(f.prompts().len(), 1, "o pedido recusado nunca rodou");
    std::fs::write(f.dir.join("b/release"), "").unwrap();
    f.until(|| f.hub.runtime("b").status == BotStatus::Idle).await;
    f.shutdown().await;
}

/// MX-77: o "Editar" reescreve o texto do pedido nos dois avisos com a marca "editado pelo dono" e o
/// prompt que roda é o novo — com a nota de que o dono editou.
#[tokio::test]
async fn mx77_owner_edits_queued_message_and_the_prompt_runs_the_new_text() {
    let f = Fixture::new();
    f.send_user("BLOCK keep the recipient busy").await;
    f.until(|| f.prompts().len() == 1).await;
    let receipt = f.message("original request").await.unwrap();
    let delegation = receipt["requestId"].as_str().unwrap().to_owned();
    let notice = f.recipient_notice(&delegation).await;
    f.queued_action("queuedEdit", &notice, Some("edited request")).await.unwrap();
    for entry in f.delegation_notices(&delegation) {
        assert_eq!(entry.data["botMessage"]["text"], "edited request", "{}", entry.bot_id);
        assert_eq!(entry.data["botMessage"]["edited"], true, "a marca 'editado pelo dono'");
        assert!(entry.data["text"].as_str().unwrap().contains("edited request"));
    }
    std::fs::write(f.dir.join("b/release"), "").unwrap();
    f.until(|| f.prompts().len() == 2 && f.hub.runtime("b").status == BotStatus::Idle).await;
    let prompt = f.prompts()[1].clone();
    assert!(prompt.contains("edited request"), "{prompt}");
    assert!(prompt.contains("edited the request text"), "{prompt}");
    assert!(prompt.contains("Another Donnabot bot"), "{prompt}");
    f.shutdown().await;
}

/// MX-77: editar um `ask_bot` vale — a resposta volta marcada para o remetente não atribuir o texto a ele.
#[tokio::test]
async fn mx77_owner_edits_queued_ask_and_the_reply_says_it_was_edited() {
    let f = Fixture::new();
    f.send_user("BLOCK keep the recipient busy").await;
    f.until(|| f.prompts().len() == 1).await;
    let request = f.request("original ask");
    let delegation = f.queued_delegation().await;
    let notice = f.recipient_notice(&delegation).await;
    f.queued_action("queuedEdit", &notice, Some("edited ask")).await.unwrap();
    std::fs::write(f.dir.join("b/release"), "").unwrap();
    let result = request.await.unwrap().unwrap();
    assert_eq!(result["reply"], "reply: edited ask");
    assert_eq!(result["edited"], true, "o remetente vê que o dono editou");
    assert!(result["note"].as_str().unwrap().contains("edited the request text"));
    f.until(|| f.prompts().len() == 2 && f.hub.runtime("b").status == BotStatus::Idle).await;
    assert!(f.prompts()[1].contains("edited the request text"), "{}", f.prompts()[1]);
    f.shutdown().await;
}

/// MX-77: "Enviar agora" passa o pedido à frente e interrompe o turno da principal, como o de mensagem.
#[tokio::test]
async fn mx77_send_now_interrupts_the_turn_and_runs_the_request_first() {
    let f = Fixture::new();
    f.send_user("BLOCK keep the recipient busy").await;
    f.until(|| f.prompts().len() == 1).await;
    f.send_user("queued user message").await;
    let receipt = f.message("bot request text").await.unwrap();
    let delegation = receipt["requestId"].as_str().unwrap().to_owned();
    let notice = f.recipient_notice(&delegation).await;
    let reply = f.queued_action("queuedSendNow", &notice, None).await.unwrap();
    assert_eq!(reply["stopRequested"], true);
    f.until(|| f.prompts().len() == 3 && f.hub.runtime("b").status == BotStatus::Idle).await;
    assert!(f.prompts()[1].contains("bot request text"), "{}", f.prompts()[1]);
    assert!(f.prompts()[2].contains("queued user message"), "{}", f.prompts()[2]);
    f.shutdown().await;
}

/// MX-77: bots, canais e rotinas não têm autoridade do dono — nenhum caminho chama as ações da fila.
/// `Caller::Ponte` é o transporte das ferramentas (bot, canal, rotina); Pairing/Bootstrap e Device sem
/// concessão caem no mesmo braço de `capturar`, que só aceita Local ou Device com Control.
#[tokio::test]
async fn mx77_queue_controls_have_no_path_for_bots_channels_or_pairing() {
    let f = Fixture::new();
    // Credencial real da Ponte para o bot "a": a mesma que as ferramentas recebem no turno.
    let binding = bindings::for_acp(&f.hub.store, "a", "fixture-acp").unwrap();
    let mut servers = vec![json!({"env": []})];
    crate::fork::ponte_credencial::provisionar(&f.hub, "a", None, &binding, &mut servers).unwrap();
    let token = zeroize::Zeroizing::new(servers[0]["env"][0]["value"].as_str().unwrap().to_owned());
    let ponte = f.hub.pontes.autenticar(&f.hub, &token).unwrap();
    assert!(matches!(&ponte, crate::api::devices::Caller::Ponte { .. }));
    let callers = [
        ponte,
        crate::api::devices::Caller::Pairing { key: "x".into() },
        crate::api::devices::Caller::Bootstrap { key: "x".into(), bootstrap_id: "y".into() },
        crate::api::devices::Caller::Device { key: "x".into(), scopes: vec![], autoridade: None },
    ];
    for caller in &callers {
        for method in ["queuedRemove", "queuedEdit", "queuedSendNow", "queuedMove"] {
            let b = json!({"botId": "b", "entryId": "qualquer", "expectedRevision": 1, "text": "x", "position": 0});
            let error = crate::agent::bot::queued_rpc(&f.hub, caller, method, &b).await.unwrap_err();
            assert!(format!("{error:#}").contains("not_allowed"), "{method}: {error:#}");
        }
    }
    f.shutdown().await;
}

/// D6: o pedido carimba a pessoa comprovada da sessão do remetente; sem marca na sessão, viaja sem pessoa.
#[test]
fn pedido_carimba_a_pessoa_da_sessao_do_remetente() {
    let dir = std::env::temp_dir().join(format!("team-person-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let cfg: BotConfig = serde_json::from_value(json!({"id":"a","name":"A","cwd":dir})).unwrap();
    store.save_bot(&cfg).unwrap();
    let hub =
        Hub::new(store, "t".into(), crate::remote::identity::Identity::load_or_create(&dir).unwrap(), "t".into(), 0);
    let lane = Lane::main("a");
    // Turno sem sessão nem marca: o pedido não leva pessoa nenhuma.
    assert_eq!(request_person(&hub, &cfg, &lane, None), None);
    // Sessão marcada pelo Hub sem pessoa (origem não comprovada): também viaja sem pessoa.
    crate::chat::memory::bind_profile_person(&hub.store, "a", "s-ninguem", None).unwrap();
    hub.set_runtime("a", |r| r.session = Some("s-ninguem".into()));
    assert_eq!(request_person(&hub, &cfg, &lane, None), None);
    // Sessão marcada com o dono: a pessoa do remetente acompanha o pedido. A lane não carrega a ACP — o ID
    // visível de uma sessão extra não vale como sessão comprovada.
    crate::chat::memory::bind_profile_person(&hub.store, "a", "s-dono", Some(crate::chat::memory::profile::OWNER))
        .unwrap();
    hub.set_runtime("a", |r| r.session = Some("s-dono".into()));
    let extra = Lane { chat: "a".into(), session: Some("visivel".into()) };
    assert_eq!(request_person(&hub, &cfg, &extra, None).as_deref(), Some(crate::chat::memory::profile::OWNER));
    assert_eq!(request_person(&hub, &cfg, &lane, None).as_deref(), Some(crate::chat::memory::profile::OWNER));
    std::fs::remove_dir_all(dir).unwrap();
}
