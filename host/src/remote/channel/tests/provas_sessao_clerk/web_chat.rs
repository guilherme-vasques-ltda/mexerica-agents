use super::*;
use crate::fork::spaces::PERSONAL;
use crate::store::{BotConfig, EntryKind, Lane, bot_groups::Edit};

#[tokio::test]
async fn navegador_confirma_envio_gravado_quando_ator_nao_esta_disponivel() {
    let hub = temp_hub();
    let bot: BotConfig = serde_json::from_value(json!({"id":"chat","name":"Chat"})).unwrap();
    hub.store.save_bot(&bot).unwrap();
    let actor = hub.test_bot_commands("chat");
    drop(actor);

    vincular(&hub, &vinculo_pessoal());
    let key = parear_em(&hub, &afirmacoes(DONO), "web").await.unwrap();
    let mut p = conectado_via(&hub, key, Via::Relay).await;
    assert_eq!(autenticar(&mut p, &hub, 1, &token(&afirmacoes(DONO))).await["ok"]["userId"], DONO);

    let send = json!({"botId":"chat","text":"mensagem fictícia","clientNonce":"nonce-teste"});
    let first = com_posse(&mut p, &hub, 2, "send", send.clone()).await;
    let id = first["ok"]["entry"]["id"].as_str().expect("mensagem gravada tem recibo");
    assert_eq!(first["ok"]["entry"]["data"]["status"], "sent");
    assert_eq!(hub.store.entry(id).unwrap().data["status"], "failed");
    let replay = com_posse(&mut p, &hub, 3, "send", send).await;
    assert_eq!(replay["ok"]["entry"]["id"], id);
    assert_eq!(hub.store.history("chat", i64::MAX, 10).unwrap().len(), 1);

    for (request, text, expected) in [(4, "mensagem fictícia", 1), (5, "outra mensagem", 0)] {
        let read = com_posse(
            &mut p,
            &hub,
            request,
            "history",
            json!({
                "botId":"chat","sendReceipt":{"nonce":"nonce-teste","text":text}
            }),
        )
        .await;
        let entries = read["ok"]["entries"].as_array().unwrap();
        assert_eq!(entries.len(), expected);
        if let Some(entry) = entries.first() {
            assert_eq!(entry["id"], id);
        }
    }

    let refused = com_posse(&mut p, &hub, 6, "history", json!({"botId":"chat","sessionId":"outra-sessao"})).await;
    assert_eq!(refused["err"], json!({"status":400,"message":"request_failed","code":"request_failed"}));
    let b = com_epoch(&hub, "sub:events", json!({"spaceId":"nao-existe"}));
    let pop = pop(&p.key, &hub.host_id, &p.th, "sub:events");
    let stream = pedir(&mut p, 7, json!({"sub":"events","b":b,"pop":pop})).await;
    assert_eq!(stream["err"], json!({"status":500,"message":"request_failed","code":"request_failed"}));
}

#[tokio::test]
async fn navegador_recebe_so_dados_de_chat_por_rpc_e_eventos() {
    let hub = temp_hub();
    let local_hub = crate::environment::Environment::is_open();
    let mut spaces = hub.store.spaces().unwrap();
    spaces.spaces[0].owner = Some(crate::fork::spaces::OwnerPresentation {
        name: "omitir-perfil".into(),
        initials: "OP".into(),
        photo: None,
    });
    let mut other = spaces.spaces[0].clone();
    other.id = "outro-espaco".into();
    other.name = "Outro Space".into();
    other.kind = "space".into();
    other.position = 1;
    spaces.spaces.push(other);
    hub.store.kv_set("donna.spaces.v1", &serde_json::to_string(&spaces).unwrap()).unwrap();
    for (id, space) in [("chat", PERSONAL), ("outro-chat", "outro-espaco")] {
        let bot: BotConfig =
            serde_json::from_value(json!({"id": id, "name": id, "spaceId": space, "cwd":"."})).unwrap();
        hub.store.save_bot(&bot).unwrap();
    }
    hub.store
        .edit_bot_groups(PERSONAL, Edit::Add { name: "Grupo de chat".into(), bot_id: Some("chat".into()) })
        .unwrap();
    hub.store
        .db
        .locked()
        .execute(
            "UPDATE donna_space_conversations SET engine_account_id='omitir-conta', continuity=?1 WHERE id='chat'",
            [json!({"interno":"omitir-continuity"}).to_string()],
        )
        .unwrap();

    let lane = Lane::main("chat");
    let ask = hub.store.insert_entry(&lane, EntryKind::User, 1,
        &json!({"text":"mensagem visível","status":"sent","attachments":[{"path":"omitir-arquivo"}],"extension":{"x":"omitir-extensao"}})).unwrap();
    let answer = hub
        .store
        .insert_entry(
            &lane,
            EntryKind::Agent,
            1,
            &json!({"text":"resposta visível","final":true,"executionBinding":"omitir-vinculo"}),
        )
        .unwrap();
    let mut host_entry = None;
    for (kind, data) in [
        (EntryKind::Tool, json!({"text":"omitir-ferramenta"})),
        (EntryKind::Thought, json!({"text":"omitir-raciocinio"})),
        (EntryKind::Permission, json!({"text":"omitir-aprovacao"})),
        (EntryKind::Agent, json!({"text":"omitir-parcial","final":false})),
        (EntryKind::User, json!({"text":"omitir-origem","from":"host"})),
        (EntryKind::User, json!({"text":"omitir-escolha","choice_response":null})),
    ] {
        let stored = hub.store.insert_entry(&lane, kind, 1, &data).unwrap();
        if data["from"] == "host" {
            host_entry = Some(stored.id);
        }
    }
    hub.store
        .insert_entry(&Lane::main("outro-chat"), EntryKind::User, 1, &json!({"text":"somente outro Space"}))
        .unwrap();
    let private = if local_hub {
        hub.store
            .db
            .locked()
            .execute(
                "INSERT INTO donna_bot_sessions(id,chat_id,principal,origin,title,title_source,created_at,last_used_at,revision)
                 SELECT 'omitir-aba',chat_id,0,origin,'omitir-titulo','owner',created_at,last_used_at,revision
                 FROM donna_bot_sessions WHERE id='chat'",
                [],
            )
            .unwrap();
        Some(
            hub.store
                .insert_entry(
                    &Lane { chat: "chat".into(), session: Some("omitir-aba".into()) },
                    EntryKind::User,
                    2,
                    &json!({
                        "text":"omitir-mensagem","clientNonce":"nonce-local",
                        "intent":{"text":"omitir-mensagem","attachments":[],"session":"omitir-aba"}
                    }),
                )
                .unwrap(),
        )
    } else {
        None
    };

    vincular(&hub, &vinculo_pessoal());
    let key = parear_em(&hub, &afirmacoes(DONO), "web").await.unwrap();
    let mut peer = conectado_via(&hub, key, Via::Relay).await;
    assert_eq!(autenticar(&mut peer, &hub, 1, &token(&afirmacoes(DONO))).await["ok"]["userId"], DONO);

    let fixture = json!({"hubId":hub.host_id,"spaceId":PERSONAL});
    for (id, method, body, field, expected) in [
        (2, "hello", json!({}), "/spaces/spaces/1/name", json!("Outro Space")),
        (3, "donnaSpaces", fixture.clone(), "/spaces/1/name", json!("Outro Space")),
        (4, "donnaConversations", fixture.clone(), "/conversations/0/id", json!("chat")),
        (
            5,
            "sessions",
            json!({"botId":"chat"}),
            "/sessionActions",
            if local_hub { json!([]) } else { json!(["send"]) },
        ),
        (6, "botGroups", json!({}), "/groups/0/bots", json!(["chat"])),
        (7, "history", json!({"botId":"chat","limit":1}), "/entries/0/id", json!(answer.id)),
        (8, "history", json!({"botId":"chat","beforeSeq":answer.seq,"limit":1}), "/entries/0/id", json!(ask.id)),
        (
            9,
            "donnaConversationHistory",
            json!({"hubId":hub.host_id,"conversationId":"chat","limit":1}),
            "/hasMore",
            json!(true),
        ),
        (
            10,
            "donnaConversationHistory",
            json!({"hubId":hub.host_id,"conversationId":"chat","query":"omitir"}),
            "/entries",
            json!([]),
        ),
    ] {
        let reply = com_posse(&mut peer, &hub, id, method, body).await;
        assert_eq!(reply["ok"].pointer(field), Some(&expected), "{method}");
        if method == "sessions" && local_hub {
            let sessions = reply["ok"]["sessions"].as_array().unwrap();
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0]["id"], "chat");
        }
        for field in
            ["omitir-", "\"engine\"", "\"origin\"", "\"titleSource\"", "\"engineAccountId\"", "\"attachments\""]
        {
            assert!(!reply["ok"].to_string().contains(field), "{method}: {field}");
        }
    }
    let forbidden =
        com_posse(&mut peer, &hub, 11, "send", json!({"botId":"chat","text":"texto","attachments":["arquivo"]})).await;
    assert_eq!(forbidden["err"]["status"], 403);
    let forbidden = com_posse(
        &mut peer,
        &hub,
        12,
        "send",
        json!({"botId":"chat","text":"texto","replyTo":host_entry.unwrap(),"clientNonce":"nonce-citacao"}),
    )
    .await;
    assert_eq!(forbidden["err"]["status"], 403);
    for (id, method, body) in [
        (13, "history", json!({"botId":"outro-chat","spaceId":PERSONAL})),
        (14, "history", json!({"botId":"chat","sessionId":"outro-chat"})),
        (15, "donnaConversationHistory", json!({"hubId":hub.host_id,"conversationId":"outro-chat","spaceId":PERSONAL})),
    ] {
        let reply = com_posse(&mut peer, &hub, id, method, body).await;
        assert!(reply.get("err").is_some(), "{method}");
    }
    let other_history =
        com_posse(&mut peer, &hub, 16, "history", json!({"botId":"outro-chat","spaceId":"outro-espaco"})).await;
    assert_eq!(other_history["ok"]["entries"][0]["data"]["text"], "somente outro Space");
    if let Some(private) = &private {
        for (id, method, body) in [
            (18, "history", json!({"botId":"chat","sessionId":"omitir-aba"})),
            (19, "send", json!({"botId":"chat","sessionId":"omitir-aba","text":"texto","clientNonce":"nonce-envio"})),
            (20, "send", json!({"botId":"chat","replyTo":private.id,"text":"texto","clientNonce":"nonce-citacao"})),
        ] {
            let reply = com_posse(&mut peer, &hub, id, method, body).await;
            assert_eq!(reply["err"]["status"], 403, "{method}");
        }
        let receipt = json!({"nonce":"nonce-local","text":"omitir-mensagem"});
        let result =
            com_posse(&mut peer, &hub, 21, "history", json!({"botId":"chat","sessionId":"chat","sendReceipt":receipt}))
                .await;
        assert_eq!(result["ok"]["entries"], json!([]));
        let result = com_posse(
            &mut peer,
            &hub,
            22,
            "history",
            json!({"botId":"chat","sessionId":"omitir-aba","sendReceipt":receipt}),
        )
        .await;
        assert_eq!(result["err"]["status"], 403);
    }

    let b = com_epoch(&hub, "sub:events", json!({"since":0}));
    peer.send(json!({"id":17,"sub":"events","b":b,"pop":pop(&peer.key,&hub.host_id,&peer.th,"sub:events")})).await;
    let hello = peer.recv().await.unwrap();
    assert_eq!(hello["ev"]["type"], "hello");
    assert_eq!(hello["ev"]["botGroups"]["groups"][0]["name"], "Grupo de chat");
    assert_eq!(hello["ev"]["spaces"]["spaces"][0]["owner"], Value::Null);
    assert!(hello["ev"]["conversations"][0].get("engineAccountId").is_none());

    if let Some(private) = &private {
        for event in [
            json!({"type":"session","session":{"id":"omitir-aba","chatId":"chat","title":"omitir-titulo"}}),
            json!({"type":"entry","entry":private}),
            json!({"type":"sessionDeleted","chatId":"chat","sessionId":"omitir-aba"}),
        ] {
            hub.events.send(event).unwrap();
        }
    }
    hub.events.send(json!({"type":"session","session":{"id":"chat","chatId":"chat","state":"open","engine":{"account":"omitir-conta"}}})).unwrap();
    let mut ids = Vec::new();
    loop {
        let frame = timeout(Duration::from_secs(2), peer.recv()).await.unwrap().unwrap();
        assert!(!frame.to_string().contains("omitir-"));
        assert!(!frame.to_string().contains("somente outro Space"));
        if frame["ev"]["type"] == "session" {
            assert!(frame["ev"]["session"].get("engine").is_none());
            break;
        }
        if frame["ev"]["type"] == "entry" {
            ids.push(frame["ev"]["entry"]["id"].clone());
        }
    }
    assert_eq!(ids, [json!(ask.id), json!(answer.id)]);

    let local = api::dispatch(&hub, &Caller::Local, "history", json!({"botId":"chat","limit":1})).await.unwrap();
    assert_eq!(local["entries"][0]["data"]["text"], "omitir-escolha");
}
