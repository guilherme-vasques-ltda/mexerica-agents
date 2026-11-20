//! MB-1b D70: transições do cartão `ownerQuestion` pela mensagem humana real (`send`), no canal e na conversa.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::{api::devices::Caller, store::Store};
use std::{path::PathBuf, sync::Arc};

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
    group: String,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("donna-pergunta-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        for id in ["a", "b"] {
            let cfg = serde_json::from_value(json!({"id":id,"name":id,"backend":"fixture","cwd":dir,"notify":false}));
            store.save_bot(&cfg.unwrap()).unwrap();
        }
        let hub = Hub::new(
            store,
            "pergunta".into(),
            crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        let group = hub
            .create_bot(
                serde_json::from_value(json!({"id":"","kind":"group","name":"Crew","members":["a","b"]})).unwrap(),
            )
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        Self { hub, dir, group }
    }
    /// Cartão gravado como o ator grava, sem a cerca de execução (coberta nos testes do ator).
    fn ask(&self, bot: &str, lane: &Lane, text: &str) -> Entry {
        let card = commit(&self.hub, |c| {
            let (withdrawn, marker, card) = ask_on(c, bot, lane, 1, text)?;
            let mut entries = withdrawn;
            entries.extend(marker);
            entries.push(card.clone());
            Ok((card, entries))
        })
        .unwrap();
        assert_eq!(card.data["card"], CARD);
        assert_eq!(card.data["ownerQuestion"]["status"], "pending");
        card
    }
    fn status(&self, id: &str) -> Value {
        self.hub.store.entry(id).unwrap().data["ownerQuestion"].clone()
    }
    async fn send(&self, chat: &str) -> String {
        let sent = crate::api::dispatch(&self.hub, &Caller::Local, "send", json!({"botId":chat,"text":"Pode seguir"}))
            .await
            .unwrap();
        sent["entry"]["id"].as_str().unwrap().to_owned()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn uma_mensagem_no_canal_responde_todas_as_perguntas_pendentes_dali() {
    let f = Fixture::new();
    let (_a, _b) = (f.hub.test_bot_commands("a"), f.hub.test_bot_commands("b"));
    let room = Lane::main(&f.group);
    let from_a = f.ask("a", &room, "Qual ambiente?");
    let from_b = f.ask("b", &room, "Posso apagar o rascunho?");
    // Outra conversa não é respondida pelo canal.
    let elsewhere = f.ask("a", &Lane::main("a"), "E na sua conversa?");
    assert_eq!(f.hub.runtime("a").pending_questions.len(), 2);
    assert_eq!(f.hub.runtime("b").pending_questions.len(), 1);

    let message = f.send(&f.group).await;
    for card in [&from_a, &from_b] {
        let state = f.status(&card.id);
        assert_eq!(state["status"], "answered", "uma mensagem humana responde todas as perguntas do canal");
        assert_eq!(state["answeredBy"], message.as_str());
    }
    assert_eq!(f.status(&elsewhere.id)["status"], "pending");
    assert_eq!(f.hub.runtime("b").pending_questions, Vec::new());
    assert_eq!(
        f.hub.runtime("a").pending_questions.iter().map(|q| q.entry_id.as_str()).collect::<Vec<_>>(),
        [elsewhere.id.as_str()]
    );
}

/// MB-1b fix (06/11): pergunta no canal, turno encerrado. Nada mostra trabalho: o canal fica Livre (não herda a
/// pergunta) e o membro mostra Precisa de você sem conversa de trabalho, o que o app lê como pergunta, não turno.
#[tokio::test]
async fn pergunta_no_canal_sem_turno_nao_vira_trabalho_no_canal_nem_no_membro() {
    let f = Fixture::new();
    f.hub.set_runtime("b", |_| {});
    let question = f.ask("a", &Lane::main(&f.group), "Qual ambiente?");
    let shown = |id: &str| f.hub.bot_json(&f.hub.store.bot(id).unwrap().unwrap());
    let group = shown(&f.group);
    assert_eq!(group["status"], "idle", "o canal não herda a pergunta do membro nem vira trabalho");
    assert_eq!(group["workingChat"], Value::Null);
    assert_eq!(group["pendingQuestions"], json!([]));
    let member = shown("a");
    assert_eq!(member["status"], "needsInput");
    assert_eq!(member["workingChat"], Value::Null, "sem turno, nenhuma conversa de trabalho");
    assert_eq!(member["pendingQuestions"][0]["entryId"], question.id.as_str());
    assert_eq!(member["pendingQuestions"][0]["chatId"], f.group.as_str());
}

#[tokio::test]
async fn mensagem_na_conversa_responde_so_a_sessao_dela_e_exclusao_retira() {
    let f = Fixture::new();
    let _a = f.hub.test_bot_commands("a");
    let main = f.ask("a", &Lane::main("a"), "Qual pasta?");
    let other = f.ask("a", &Lane { chat: "a".into(), session: Some("s2".into()) }, "E nesta sessão?");
    assert_eq!(f.hub.runtime("a").pending_questions[0].session_id.as_deref(), Some("s2"), "mais nova primeiro");

    let message = f.send("a").await;
    assert_eq!(f.status(&main.id)["status"], "answered");
    assert_eq!(f.status(&main.id)["answeredBy"], message.as_str());
    assert_eq!(f.status(&other.id)["status"], "pending", "outra sessão espera a própria mensagem");

    deleted(&f.hub, "a");
    assert_eq!(f.status(&other.id)["status"], "withdrawn");
    assert_eq!(f.status(&main.id)["status"], "answered", "estado final não volta");
}

#[test]
fn pergunta_vazia_ou_longa_e_recusada_antes_do_cartao() {
    assert!(question(&json!({"question":"   "})).is_err());
    assert!(question(&json!({"question":"x".repeat(MAX_CHARS + 1)})).is_err());
    assert_eq!(
        question(&json!({"question":format!(" {} ", "é".repeat(MAX_CHARS))})).unwrap().chars().count(),
        MAX_CHARS
    );
}

#[tokio::test]
async fn s1_refresh_atrasado_nao_ressuscita_pergunta_ja_respondida() {
    let f = Fixture::new();
    let _a = f.hub.test_bot_commands("a");
    f.hub.set_runtime("a", |_| {}); // o ator nasce com runtime (spawn_bot)
    let hub = f.hub.clone();
    // Entre ler [Q] e aplicar: o dono responde, e o refresh dessa Tx aplica [] primeiro.
    let reached = crate::fork::sessoes::call_tests::interleave(&f.hub.store, "owner_question_refresh", move || {
        add_human_message(&hub, &Lane::main("a"), 2, &json!({"text":"Pode seguir"})).unwrap();
    });
    let card = f.ask("a", &Lane::main("a"), "Qual pasta?");
    assert!(reached.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(f.status(&card.id)["status"], "answered");
    assert_eq!(f.hub.runtime("a").pending_questions, Vec::new(), "a leitura antiga [Q] foi descartada");
}

#[test]
fn s2_orientacao_so_onde_o_servidor_chat_e_oferecido() {
    assert!(!crate::agent::harness::compile().contains(TOOL), "o prompt geral não promete ask_owner");
    assert!(CHAT_INSTRUCTIONS.contains(TOOL));
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let mut cfg: crate::store::BotConfig = serde_json::from_value(json!({"id":"bot","name":"Bot"})).unwrap();
    let chat = |cfg: &crate::store::BotConfig| {
        crate::fork::spaces::tool_servers::decide(&store, cfg, false, false).builtin.contains(&"chat")
    };
    assert!(chat(&cfg));
    cfg.space_id = "loja".into();
    assert!(!chat(&cfg), "bot de outro Space não recebe o servidor chat, nem a ferramenta, nem a orientação");
}

#[tokio::test]
async fn s3_snapshot_e_evento_dao_as_mesmas_perguntas_com_runtime_redigido() {
    let f = Fixture::new();
    let question = f.ask("a", &Lane::main("a"), "Qual pasta?");
    // Atividade atual fora da audiência: runtime redigido, pergunta autorizada pela própria conversa.
    f.hub.set_runtime("a", |r| {
        r.status = crate::hub::BotStatus::Working;
        r.lane = Some(Lane::main("fantasma"));
    });
    let row = f.hub.store.bot("a").unwrap().unwrap();
    let snapshot = f.hub.bot_json(&row);
    assert_eq!(snapshot["status"], "unavailable");
    assert_eq!(snapshot["workingChat"], Value::Null);
    assert_eq!(snapshot["pendingQuestions"][0]["entryId"], question.id.as_str());
    let mut event = snapshot.clone();
    crate::fork::runtime_audiencia::revalidate_event(&f.hub, &row.config.space_id, &mut event);
    assert_eq!(event, snapshot);
}

#[tokio::test]
async fn retry_com_o_mesmo_nonce_nao_duplica_a_mensagem_nem_a_resposta() {
    let f = Fixture::new();
    let _a = f.hub.test_bot_commands("a");
    let question = f.ask("a", &Lane::main("a"), "Qual pasta?");
    let send = || {
        crate::api::dispatch(
            &f.hub,
            &Caller::Local,
            "send",
            json!({"botId":"a","text":"Pode seguir","clientNonce":"nonce-do-board"}),
        )
    };
    let first = send().await.unwrap();
    let answered = f.hub.store.entry(&question.id).unwrap();
    // O Board repete o envio que falhou com o MESMO nonce: a dedup existente do `send` devolve a mesma mensagem.
    let second = send().await.unwrap();
    assert_eq!(second["entry"]["id"], first["entry"]["id"]);
    let users = f.hub.store.history("a", i64::MAX, 50).unwrap().into_iter().filter(|e| e.kind == "user").count();
    assert_eq!(users, 1, "uma única entrada do dono");
    let after = f.hub.store.entry(&question.id).unwrap();
    assert_eq!(after.data["ownerQuestion"]["status"], "answered");
    assert_eq!(after.data["ownerQuestion"]["answeredBy"], first["entry"]["id"]);
    assert_eq!(after.rev, answered.rev, "a segunda chegada não grava nem responde de novo");
}

#[tokio::test]
async fn fila_cheia_conta_so_mensagens_novas_do_dono() {
    let f = Fixture::new();
    let lane = Lane::main("a");
    for (i, extra) in [json!({"choice_response": {"question": "q"}}), json!({"from": "host"})].into_iter().enumerate() {
        let mut data = json!({"text": format!("cartão {i}"), "status": "queued"});
        data.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        f.hub.store.insert_entry(&lane, EntryKind::User, 1, &data).unwrap();
    }
    f.hub
        .store
        .db
        .locked()
        .execute(
            "INSERT INTO entries(id, bot_id, rev, kind, turn, data, created_at, updated_at) VALUES('x', 'a', 999, 'user', 1, 'nao e json', 0, 0)",
            [],
        )
        .unwrap();
    for i in 0..QUEUE_MAX {
        let data = json!({"text": format!("m{i}"), "status": "queued", "clientNonce": format!("n{i}")});
        assert!(matches!(add_human_message(&f.hub, &lane, 2, &data).unwrap(), Human::New(_)), "{i}");
    }
    let extra = json!({"text": "uma a mais", "status": "queued", "clientNonce": "n-extra"});
    let error = add_human_message(&f.hub, &lane, 2, &extra).err().unwrap();
    assert!(error.to_string().starts_with("queue_full"), "{error}");
    let _ = std::fs::remove_dir_all(&f.dir);
}
