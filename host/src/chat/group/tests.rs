// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
use super::*;
use serde_json::json;

fn bot(id: &str, name: &str) -> BotConfig {
    serde_json::from_value(json!({"id": id, "name": name, "backend": "claude", "cwd": "/tmp"})).expect("valid bot")
}

fn said(text: &str) -> Entry {
    Entry {
        id: "e".into(),
        seq: 1,
        bot_id: "g".into(),
        rev: 1,
        kind: "user".into(),
        turn: 1,
        data: json!({"text": text}),
        created_at: 0,
        updated_at: 0,
        session_id: "g".into(),
    }
}

#[test]
fn mentions_pick_responders_and_no_mention_means_everyone() {
    let members = [bot("a", "Alice Chen"), bot("b", "Bob"), bot("c", "尼采課")];
    let ids = |m: &[BotConfig], text: &str| -> Vec<String> {
        responders(m, &[said(text)]).into_iter().map(|b| b.id.clone()).collect()
    };
    assert_eq!(ids(&members, "hi all"), ["a", "b", "c"]);
    assert_eq!(ids(&members, "@bob can you look"), ["b"]);
    assert_eq!(ids(&members, "@alicechen and @Alice"), ["a"]);
    assert_eq!(ids(&members, "@尼采課幫我整理"), ["c"], "CJK names can run into the message");
    assert_eq!(ids(&members, "@bobby?"), ["a", "b", "c"], "a longer word isn't a mention");
    assert_eq!(ids(&members, "@all go"), ["a", "b", "c"]);
    let mut reply = said("why?");
    reply.data["replyTo"] = json!({"id": "x", "author": "b", "text": "done"});
    let replied: Vec<String> = responders(&members, &[reply]).into_iter().map(|b| b.id.clone()).collect();
    assert_eq!(replied, ["b"], "a reply asks the bot it quotes");
}

/// D327 (dono): menção manda. Quem é chamado responde; o líder vai primeiro só se estiver entre eles, e sem nome
/// ele abre a rodada. Cada rodada recalcula com o que foi dito desde a mensagem do dono, como em `run`.
#[test]
fn mentions_decide_who_answers_and_the_leader_goes_first_among_them() {
    let members = [bot("a", "Ana"), bot("b", "Bia"), bot("c", "Caio")];
    let mut group = bot("g", "Casa");
    group.leader = Some("c".into());
    let round = |messages: &[Entry]| -> Vec<String> {
        let mut speakers = responders(&members, messages);
        crate::fork::lider_canal::leader_first(&group, &mut speakers);
        speakers.into_iter().map(|b| b.id.clone()).collect()
    };
    assert_eq!(round(&[said("@bia pode ver?")]), ["b"], "menção sem o líder: só quem foi chamado");
    assert_eq!(round(&[said("@ana e @caio, vejam")]), ["c", "a"], "o líder entre os chamados vai primeiro");
    assert_eq!(round(&[said("bom dia")]), ["c", "a", "b"], "mensagem geral: o líder abre");
    let mut delegated = said("@ana, faça a primeira parte");
    delegated.kind = "agent".into();
    delegated.data["author"] = json!("c");
    assert_eq!(round(&[said("bom dia"), delegated]), ["a"], "na rodada seguinte, responde quem o líder chamou");
}

use crate::api::devices::Caller;
use crate::hub::BotStatus;
use std::path::PathBuf;
use std::time::Duration;

/// A host with agent bots `a` and `b` (the scripted fixture agent) and a group of both.
struct Room {
    hub: Arc<Hub>,
    dir: PathBuf,
    group: String,
}

impl Room {
    fn new() -> Self {
        std::fs::create_dir_all(crate::service::data_dir()).unwrap();
        let dir = std::env::temp_dir().join(format!("codync-group-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("test.db")).unwrap();
        let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/team_agent.py");
        for id in ["a", "b"] {
            let cwd = dir.join(id);
            std::fs::create_dir_all(&cwd).unwrap();
            let cfg: BotConfig = serde_json::from_value(json!({
                "id": id, "name": id, "backend": "fixture", "cwd": cwd,
                "command": crate::shell::python(&agent), "notify": false,
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
        let group = hub
            .create_bot(
                serde_json::from_value(
                    json!({"id": "", "kind": "group", "name": "Crew", "description": "Ship the release", "members": ["a", "b", "a"]}),
                )
                .unwrap(),
            )
            .unwrap();
        let group = group["id"].as_str().unwrap().to_owned();
        Self { hub, dir, group }
    }

    async fn send(&self, text: &str, reply_to: Option<&str>) -> Entry {
        let sent = crate::api::dispatch(
            &self.hub,
            &Caller::Local,
            "send",
            json!({"botId": self.group, "text": text, "replyTo": reply_to}),
        )
        .await
        .unwrap();
        serde_json::from_value::<serde_json::Value>(sent["entry"].clone())
            .ok()
            .and_then(|e| self.hub.store.entry(e["id"].as_str()?))
            .unwrap()
    }

    fn replies(&self, lane: &Lane) -> Vec<(String, String)> {
        self.hub
            .store
            .messages_after(lane, 0, 100)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == "agent")
            .map(|e| (e.data["author"].as_str().unwrap().to_owned(), e.data["text"].as_str().unwrap().to_owned()))
            .collect()
    }

    async fn settle(&self) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                tokio::time::sleep(Duration::from_millis(300)).await;
                let idle = ["a", "b"].iter().all(|b| self.hub.runtime(b).status == BotStatus::Idle);
                if idle && self.hub.bots_json(0).unwrap().iter().all(|b| b["status"] == "idle") {
                    break;
                }
            }
        })
        .await
        .expect("room settles");
    }

    async fn shutdown(self) {
        self.hub.shutdown().await;
        let _ = std::fs::remove_dir_all(self.dir);
    }
}

#[tokio::test]
async fn room_turns_follow_mentions_and_end_when_everyone_passes() {
    let room = Room::new();
    let row = room.hub.store.bot(&room.group).unwrap().unwrap();
    assert_eq!(row.config.members, ["a", "b"], "members are deduplicated");
    let again = room
        .hub
        .create_bot(
            serde_json::from_value(json!({"id": "", "kind": "group", "name": "Dup", "members": ["b", "a"]})).unwrap(),
        )
        .unwrap();
    assert_eq!(again["id"], room.group, "the same bots reopen their group");

    let main = Lane::main(&room.group);
    room.send("hello everyone", None).await;
    room.settle().await;
    // `a` answers the user; `b` then sees a bot spoke last and passes; round two is all passes.
    assert_eq!(room.replies(&main), [("a".to_owned(), "reply: group".to_owned())]);

    room.send("@b your turn", None).await;
    room.settle().await;
    assert_eq!(room.replies(&main).last().unwrap().0, "b", "only the mentioned bot answers");
    assert_eq!(room.replies(&main).len(), 2);
    let listed = room.hub.bot_value_for_test(&room.group);
    assert_eq!(listed["lastMessage"], "b: reply: group");
    let prompts = std::fs::read_to_string(room.dir.join("a/prompts.jsonl")).unwrap();
    assert!(prompts.contains("Ship the release) with the user"), "members are told what the room is for: {prompts}");
    // The members' own chats stay private: the room's entries live in the group.
    assert!(room.hub.store.messages_after(&Lane::main("a"), 0, 100).unwrap().is_empty());
    room.shutdown().await;
}

#[tokio::test]
async fn a_skill_command_reaches_the_members_that_have_the_skill_on() {
    let room = Room::new();
    // D306/R1: só uma instalação conferida entrega a skill; nada de registro cru sem arquivo.
    let pdf = json!({"name": "PDF", "description": "Merge PDFs", "instructions": "Merge them."});
    let pdf = crate::fork::skill_catalog::custom(&room.hub.store, &pdf, || Ok(())).unwrap();
    assert_eq!(pdf["skill"]["id"], "pdf");
    let mut a = room.hub.store.bot("a").unwrap().unwrap().config;
    a.skills = vec!["pdf".into()];
    crate::fork::skill_selection::set_for_test(&room.hub.store, "a", &["pdf"]);
    room.hub.store.save_bot(&a).unwrap();

    room.send("/pdf merge these", None).await;
    room.settle().await;
    let prompts = |id: &str| std::fs::read_to_string(room.dir.join(id).join("prompts.jsonl")).unwrap();
    assert!(prompts("a").contains(r#"User: [Donnabot: the user picked the \"PDF\" skill"#), "{}", prompts("a"));
    assert!(
        prompts("b").contains("User: /pdf merge these"),
        "a member without it reads what was typed: {}",
        prompts("b")
    );
    room.shutdown().await;
}

#[tokio::test]
async fn replies_quote_the_message_they_answer() {
    let room = Room::new();
    let main = Lane::main(&room.group);
    room.send("@a start here", None).await;
    room.settle().await;
    let answer = room.hub.store.messages_after(&main, 0, 100).unwrap();
    let answer = answer.iter().find(|e| e.kind == "agent" && e.data["final"] == true).unwrap();
    let reply = room.send("@a about that", Some(&answer.id)).await;
    assert_eq!(reply.data["replyTo"], json!({"id": answer.id, "author": "a", "text": "reply: group"}));
    room.settle().await;
    let prompts = |id: &str| std::fs::read_to_string(room.dir.join(id).join("prompts.jsonl")).unwrap();
    assert!(
        prompts("a").contains(r#"User: [Donnabot: this replies to your message: \"reply: group\"] @a about that"#),
        "the room line carries the quote: {}",
        prompts("a")
    );
    let send = |b: serde_json::Value| crate::api::dispatch(&room.hub, &Caller::Local, "send", b);
    assert!(send(json!({"botId": "a", "text": "x", "replyTo": answer.id})).await.is_err(), "another chat's message");

    // Sending read the chat; only the answer to the reply is new.
    let unread = || room.hub.bot_value_for_test(&room.group)["unread"].as_i64().unwrap();
    assert_eq!(unread(), 1);
    crate::api::dispatch(&room.hub, &Caller::Local, "markRead", json!({"botId": room.group})).await.unwrap();
    assert_eq!(unread(), 0);

    // Reactions toggle, and reacting to a read message doesn't make it unread.
    let react = |emoji: &str| {
        crate::api::dispatch(&room.hub, &Caller::Local, "react", json!({"entryId": answer.id, "emoji": emoji}))
    };
    react("👍").await.unwrap();
    let reacted = react("✅").await.unwrap();
    assert_eq!(reacted["entry"]["data"]["reactions"], json!(["👍", "✅"]));
    assert_eq!(unread(), 0);
    let undone = react("👍").await.unwrap();
    assert_eq!(undone["entry"]["data"]["reactions"], json!(["✅"]));
    assert!(react("hi").await.is_err(), "only emoji");

    // In a bot's own chat the agent reads the quote before the text.
    let dm = send(json!({"botId": "b", "text": "main"})).await.unwrap();
    room.settle().await;
    send(json!({"botId": "b", "text": "aside", "replyTo": dm["entry"]["id"]})).await.unwrap();
    room.settle().await;
    assert!(
        prompts("b")
            .lines()
            .last()
            .unwrap()
            .contains(r#"[Donnabot: this replies to the user's earlier message: \"main\"]\naside"#),
        "{}",
        prompts("b")
    );
    room.shutdown().await;
}

#[tokio::test]
async fn stop_ends_the_room_turn_before_the_next_speaker() {
    let room = Room::new();
    room.send("BLOCK everyone look", None).await;
    let hub = room.hub.clone();
    tokio::time::timeout(Duration::from_secs(10), async {
        while hub.runtime("a").status != BotStatus::Working {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("a starts");
    crate::api::dispatch(&room.hub, &Caller::Local, "stop", json!({"botId": room.group})).await.unwrap();
    room.settle().await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!room.dir.join("b/prompts.jsonl").exists(), "b never got a turn after Stop");
    assert!(room.replies(&Lane::main(&room.group)).iter().all(|(_, t)| t != "reply: group"));
    room.shutdown().await;
}

#[tokio::test]
async fn the_channel_leader_answers_first_and_cant_be_removed_before_a_swap() {
    let room = Room::new();
    let update = |patch: serde_json::Value| crate::api::dispatch(&room.hub, &Caller::Local, "updateBot", patch);
    assert_eq!(room.hub.bot_value_for_test(&room.group)["leader"], serde_json::Value::Null, "no leader by default");
    update(json!({"id": room.group, "leader": "b"})).await.unwrap();
    assert_eq!(room.hub.bot_value_for_test(&room.group)["leader"], "b");
    room.send("hello everyone", None).await;
    room.settle().await;
    assert_eq!(room.replies(&Lane::main(&room.group)), [("b".to_owned(), "reply: group".to_owned())]);
    let prompts = |id: &str| std::fs::read_to_string(room.dir.join(format!("{id}/prompts.jsonl"))).unwrap();
    assert!(prompts("b").contains("leader: you") && prompts("b").contains("You lead this channel"), "{}", prompts("b"));
    assert!(prompts("a").contains("b leads this channel"), "{}", prompts("a"));
    let refused = update(json!({"id": room.group, "members": ["a"]})).await.unwrap_err();
    assert!(format!("{refused:#}").contains(crate::fork::lider_canal::SWAP_FIRST), "{refused:#}");
    update(json!({"id": room.group, "members": ["a"], "leader": null})).await.unwrap();
    assert_eq!(room.hub.bot_value_for_test(&room.group)["leader"], serde_json::Value::Null);
    room.shutdown().await;
}

#[test]
fn pass_is_recognized_loosely() {
    for t in ["(pass)", " (pass) ", "\"(pass)\"", "pass.", ""] {
        assert!(is_pass(t), "{t:?}");
    }
    assert!(!is_pass("I'll pass this to Bob"));
}

/// O alvo da reação sai do mesmo recorte do prompt: nem o que veio antes da última fala do membro, nem a mensagem em
/// conferência e, na retomada, só a mensagem identificada.
#[test]
fn reactions_target_exactly_what_the_prompt_shows() {
    let dir = std::env::temp_dir().join(format!("codync-group-shown-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let members = [bot("a", "Ann"), bot("b", "Ben"), bot("c", "Cid")];
    let group: BotConfig =
        serde_json::from_value(json!({"id": "g", "kind": "group", "name": "Crew", "members": ["a", "b", "c"]}))
            .unwrap();
    for cfg in members.iter().chain([&group]) {
        store.save_bot(cfg).unwrap();
    }
    let identity = crate::remote::identity::Identity::load_or_create(&dir).unwrap();
    let hub = Hub::new(store, "shown-test".into(), identity, "test".into(), 0);
    let lane = Lane::main("g");
    let said = |kind, data: serde_json::Value| hub.add_entry(&lane, kind, 1, &data).unwrap();
    let old = said(EntryKind::User, json!({"text": "antes de Ben falar"}));
    said(EntryKind::Agent, json!({"text": "Ben falou", "final": true, "author": "b"}));
    let ask = said(EntryKind::User, json!({"text": "depois de Ben falar"}));
    said(EntryKind::Agent, json!({"text": "em conferência", "final": true, "author": "a", "groupReview": true}));

    let (prompt, shown) = member_prompt(&hub, &group, &members, &members[1], &lane, None).unwrap();
    assert_eq!(shown, [ask.id.as_str()]);
    assert!(prompt.contains("depois de Ben falar"));
    assert!(!prompt.contains("antes de Ben falar") && !prompt.contains("em conferência"));
    let react = |to, shown: &[String]| crate::chat::reactions::in_room(&hub, "b", to, shown);
    assert_eq!(react(None, &shown).unwrap(), ask.id);
    assert!(react(Some("Ann"), &shown).is_err(), "a mensagem em conferência não foi mostrada");

    let (prompt, shown) = member_prompt(&hub, &group, &members, &members[2], &lane, Some(old.seq)).unwrap();
    assert_eq!(shown, [old.id.as_str()], "a retomada mostra só a mensagem identificada");
    assert!(prompt.contains("antes de Ben falar") && !prompt.contains("depois de Ben falar"));
    let _ = std::fs::remove_dir_all(&dir);
}
