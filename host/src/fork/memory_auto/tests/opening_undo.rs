use super::{Operation, apply, plan, queue};
use crate::{
    agent::bot::Instructions,
    api::{devices::Caller, dispatch},
    chat::context,
    fork::spaces,
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, EntryKind, Lane, Store},
};
use serde_json::json;

#[tokio::test]
async fn desfazer_remove_so_a_lembranca_do_novo_opening_do_space() {
    const FACT: &str = "Meu caderno de ensaio tem capa xadrez cor de ameixa.";
    const CONTROL: &str = "O marcador do caderno de ensaio é de papel lilás.";
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("fixture.db")).unwrap();
    let bot: BotConfig = serde_json::from_value(json!({
        "id":"fixture-bot", "name":"Bot da fixture", "backend":"claude", "cwd":dir.path()
    }))
    .unwrap();
    store.save_bot(&bot).unwrap();
    store.kv_set("team.settings.v1", r#"{"primaryBotId":"fixture-bot","permission":"ask"}"#).unwrap();
    let hub = Hub::new(store, "fixture".into(), Identity::load_or_create(dir.path()).unwrap(), "fixture".into(), 0);
    let call = |method, body| dispatch(&hub, &Caller::Local, method, body);

    let mut workspaces = Vec::new();
    for (name, control) in [
        ("Ateliê de ensaio", CONTROL),
        ("Oficina de ensaio", "O estojo da oficina de ensaio é de cartolina."),
        ("Pessoal", "A pasta pessoal de ensaio é de papel azul."),
    ] {
        let space_id = if name == "Pessoal" {
            spaces::PERSONAL.to_owned()
        } else {
            let state = hub.store.spaces().unwrap();
            let created = call(
                "donnaSpaceCreate",
                json!({"hubId":hub.host_id,"expectedRevision":state.revision,"name":name,
                    "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"EN"}}}),
            )
            .await
            .unwrap();
            created["spaces"].as_array().unwrap().last().unwrap()["id"].as_str().unwrap().to_owned()
        };
        let created = call("donnaConversationCreate", json!({"hubId":hub.host_id,"spaceId":space_id,"botId":bot.id}))
            .await
            .unwrap();
        let conversation = &created["conversation"];
        let chat = conversation["id"].as_str().unwrap().to_owned();
        let cfg = spaces::execution_config(&hub.store, &bot, &chat).unwrap();
        assert_eq!(cfg.space_id, space_id);
        call(
            "donnaConversationMemory",
            json!({"hubId":hub.host_id,"spaceId":space_id,"conversationId":chat,
                "expectedRevision":conversation["revision"],"method":"saveMemory","text":control,"shared":false}),
        )
        .await
        .unwrap();
        workspaces.push((cfg, chat, control));
    }

    let (cfg, chat, _) = &workspaces[0];
    let lane = Lane::main(chat);
    let sid = "sessao-de-evidencia";
    let message = hub
        .add_entry(
            &lane,
            EntryKind::User,
            1,
            &json!({"text":FACT,"contentOrigin":{"status":"appReported","ingress":{"caller":"local"},
                "report":{"channel":"desktopComposer"},
                "segments":[{"kind":"typed","start":0,"end":FACT.encode_utf16().count()}]}}),
        )
        .unwrap();
    hub.store.memory_session_start(&cfg.id, sid, false).unwrap();
    hub.store.memory_session_turn(&cfg.id, chat, sid, 1).unwrap();
    hub.store.memory_turn_begin(&cfg.id, chat, sid, &[1], "owner").unwrap();
    hub.set_runtime(&cfg.id, |r| {
        r.memory_turn_token = Some(uuid::Uuid::new_v4().to_string());
        r.lane = Some(lane.clone());
        r.message = Some(message.id.clone());
        r.session = Some(sid.into());
    });
    queue::capture_engine(&hub, cfg, &lane, &cfg.backend).unwrap();
    queue::begin(&hub, cfg, &lane, sid, std::slice::from_ref(&message.id)).unwrap();
    let job = queue::active_job(&hub, &cfg.id).unwrap().unwrap();
    queue::checkpoint(&hub.store, "fixture-completion", r#"{"completion":{"turn":1}}"#, Some(&job), true, false)
        .unwrap();
    let reservation = queue::reserve_job(&hub.store, &job).unwrap().unwrap();
    let receipts = apply::complete(
        &hub,
        &reservation,
        plan(Operation::Remember { text: FACT.into(), shared: false, evidence: vec![message.id], nature: None }),
        |_, _| Ok(()),
    )
    .unwrap();
    let [receipt] = receipts.as_slice() else { panic!("esperava um único recibo de memória") };

    let opening = |cfg, session| {
        context::resolve_with_skills(
            &hub.store,
            cfg,
            session,
            Instructions::FirstMessage,
            || Ok(None),
            Some("Quais materiais de ensaio temos?"),
        )
        .unwrap()
        .system
    };
    for (index, (cfg, _, control)) in workspaces.iter().enumerate() {
        let system = opening(cfg, "abertura-antes");
        assert!(system.contains(control), "faltou a lembrança de controle em {}", cfg.space_id);
        assert_eq!(system.contains(FACT), index == 0, "escopo incorreto da lembrança em {}", cfg.space_id);
    }

    let undone =
        call("undoMemory", json!({"hubId":hub.host_id,"spaceId":cfg.space_id,"conversationId":chat,"entryId":receipt}))
            .await
            .unwrap();
    assert_eq!(undone["entry"]["data"]["memory"]["status"], "undone");
    let next = opening(cfg, "abertura-depois");
    assert!(!next.contains(FACT), "lembrança desfeita voltou na nova abertura");
    assert!(next.contains(CONTROL), "Desfazer removeu também a lembrança de controle");
}
