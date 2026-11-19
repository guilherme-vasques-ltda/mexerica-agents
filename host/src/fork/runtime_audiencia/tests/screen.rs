use super::*;
use crate::fork::runtime_audiencia::screen::state;
use crate::store::Scope;

fn usage(screen: &Value, expected: &str, id: Option<&str>) {
    assert_eq!(screen["agentUse"], expected);
    assert_eq!(screen["agentBot"], json!(id));
    assert!(screen.get("workingChat").is_none());
    assert!(screen.get("startedAt").is_none());
    assert!(screen.get("resumeAt").is_none());
}

#[tokio::test]
async fn screen_hello_rpc_e_stream_projetam_a_principal_no_space_solicitado() {
    let f = Fixture::new().await;
    f.runtime(&f.donna, Some(&f.loja_chat), BotStatus::Working);
    f.hub.screen.agent_for_test(Some(&f.donna));
    let original = f.hub.store.bot(&f.donna).unwrap().unwrap().config;
    let internal = format!("{:?}", f.hub.runtime(&f.donna));
    let raw = f.hub.screen.state();
    let caller = Caller::Device { key: "controle-fixture".into(), scopes: vec![Scope::Control], autoridade: None };
    for (space, expected, id) in [(PERSONAL, "unavailable", None), (f.loja.as_str(), "using", Some(f.donna.as_str()))] {
        let hello = dispatch(&f.hub, &caller, "hello", json!({"spaceId":space})).await.unwrap();
        usage(&hello["screen"], expected, id);
        let status = dispatch(&f.hub, &caller, "screenStatus", json!({"spaceId":space})).await.unwrap();
        usage(&status, expected, id);
        let stream =
            scoped_events_stream(&f.hub, f.hub.store.current_rev(), None, &Caller::Local, Some(space)).unwrap();
        tokio::pin!(stream);
        let hello = stream.next().await.unwrap();
        usage(&hello["screen"], expected, id);
        // Emissão real do broker de fixture; o evento global bruto não vai ao fio.
        f.hub.screen.agent_for_test(Some(&f.donna));
        f.marker();
        let events = until_marker(stream.as_mut()).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "screen");
        assert_eq!(events[0]["spaceId"], space);
        usage(&events[0]["screen"], expected, id);
        let mut projected = status;
        let mut broker = raw.clone();
        projected.as_object_mut().unwrap().remove("agentUse");
        projected.as_object_mut().unwrap().remove("agentBot");
        broker.as_object_mut().unwrap().remove("agentBot");
        assert_eq!(projected, broker, "broker, permissões e displays não são reescritos");
    }
    // O BotJSON continua nativo: não clonou cfg.spaceId nem criou um vínculo de acesso.
    unavailable(&f.bot(&f.donna));
    assert_eq!(f.hub.store.bot(&f.donna).unwrap().unwrap().config.space_id, original.space_id);
    assert_eq!(original.space_id, PERSONAL);
    assert_eq!(format!("{:?}", f.hub.runtime(&f.donna)), internal);
    assert!(dispatch(&f.hub, &caller, "screenTakeover", json!({"on":true})).await.is_err());
    let screen_caller = Caller::Device { key: "tela-fixture".into(), scopes: vec![Scope::Screen], autoridade: None };
    let response =
        dispatch(&f.hub, &screen_caller, "screenTakeover", json!({"spaceId":f.loja,"on":true})).await.unwrap();
    usage(&response, "using", Some(&f.donna));
    assert_eq!(response["userControl"], true);
    let disabled =
        dispatch(&f.hub, &Caller::Local, "setScreenEnabled", json!({"spaceId":f.loja,"enabled":false})).await.unwrap();
    usage(&disabled, "idle", None);
    assert_eq!(disabled["enabled"], false);
}

#[tokio::test]
async fn mudanca_de_lane_e_principal_atualizam_screen_e_revalidam_evento_em_fila() {
    let f = Fixture::new().await;
    f.runtime(&f.donna, Some(&f.personal_chat), BotStatus::Working);
    f.hub.screen.agent_for_test(Some(&f.donna));
    let since = f.hub.store.current_rev();
    let personal = scoped_events_stream(&f.hub, since, None, &Caller::Local, Some(PERSONAL)).unwrap();
    let loja = scoped_events_stream(&f.hub, since, None, &Caller::Local, Some(&f.loja)).unwrap();
    tokio::pin!(personal, loja);
    usage(&personal.next().await.unwrap()["screen"], "using", Some(&f.donna));
    usage(&loja.next().await.unwrap()["screen"], "unavailable", None);
    // Mesmo status e mesma posse; a mudança de lane por si atualiza o evento existente.
    f.runtime(&f.donna, Some(&f.loja_chat), BotStatus::Working);
    f.marker();
    for (events, expected, id) in [
        (until_marker(personal.as_mut()).await, "unavailable", None),
        (until_marker(loja.as_mut()).await, "using", Some(f.donna.as_str())),
    ] {
        let screens: Vec<_> = events.iter().filter(|e| e["type"] == "screen").collect();
        assert_ne!(screens, Vec::<&Value>::new());
        for event in screens {
            usage(&event["screen"], expected, id);
        }
    }
    f.hub.screen.agent_for_test(Some(&f.donna));
    f.hub.store.sem_donna_fixa(); // D366: exercita a troca/exclusão legada, sem Donna fixa.
    dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"primaryBotId":f.observer})).await.unwrap();
    f.marker();
    for events in [until_marker(personal.as_mut()).await, until_marker(loja.as_mut()).await] {
        let screens: Vec<_> = events.iter().filter(|e| e["type"] == "screen").collect();
        assert_ne!(screens, Vec::<&Value>::new());
        for event in screens {
            usage(&event["screen"], "unavailable", None);
        }
    }
    assert_eq!(f.hub.runtime(&f.donna).status, BotStatus::Working);
}

#[tokio::test]
async fn screen_sem_contexto_fechamento_rebinding_e_exclusao_nao_afirmam_livre() {
    let f = Fixture::new().await;
    usage(&state(&f.hub, PERSONAL), "idle", None);
    f.hub.screen.agent_for_test(Some("identidade-desconhecida"));
    usage(&state(&f.hub, PERSONAL), "unavailable", None);
    f.hub.screen.agent_for_test(Some(&f.donna));
    usage(&state(&f.hub, PERSONAL), "unavailable", None);
    f.hub.set_runtime(&f.donna, |rt| *rt = Runtime::default());
    usage(&state(&f.hub, PERSONAL), "unavailable", None);
    f.runtime(&f.donna, Some(&f.observer), BotStatus::Working);
    usage(&state(&f.hub, PERSONAL), "unavailable", None);
    let group = f.group();
    f.runtime(&f.donna, Some(&group), BotStatus::Working);
    usage(&state(&f.hub, PERSONAL), "using", Some(&f.donna));
    let mut cfg = f.hub.store.bot(&group).unwrap().unwrap().config;
    cfg.members = vec![f.observer.clone()];
    f.hub.store.save_bot(&cfg).unwrap();
    f.hub.emit_bot(&group);
    usage(&state(&f.hub, PERSONAL), "unavailable", None);
    f.hub.screen.agent_for_test(None);
    usage(&state(&f.hub, PERSONAL), "idle", None);

    for invalidation in ["fechamento", "rebinding", "exclusao"] {
        let f = Fixture::new().await;
        // Segunda conversa da Loja: fechamento é permitido pela API original.
        let created = dispatch(
            &f.hub,
            &Caller::Local,
            "donnaConversationCreate",
            json!({
                "hubId":f.hub.host_id,"spaceId":f.loja,"botId":f.donna,"title":"Conversa fechável",
            }),
        )
        .await
        .unwrap();
        let chat = created["conversation"]["id"].as_str().unwrap();
        f.runtime(&f.donna, Some(chat), BotStatus::Error);
        f.hub.screen.agent_for_test(Some(&f.donna));
        usage(&state(&f.hub, &f.loja), "using", Some(&f.donna));
        let stream =
            scoped_events_stream(&f.hub, f.hub.store.current_rev(), None, &Caller::Local, Some(&f.loja)).unwrap();
        tokio::pin!(stream);
        usage(&stream.next().await.unwrap()["screen"], "using", Some(&f.donna));
        f.hub.screen.agent_for_test(Some(&f.donna));
        match invalidation {
            "fechamento" => {
                dispatch(
                    &f.hub,
                    &Caller::Local,
                    "donnaConversationUpdate",
                    json!({
                        "hubId":f.hub.host_id,"spaceId":f.loja,"conversationId":chat,
                        "expectedRevision":created["conversation"]["revision"],"closed":true,
                    }),
                )
                .await
                .unwrap();
            }
            "rebinding" => {
                // Estado persistido inválido: só fixture, sem oferecer mutação de vínculo.
                f.hub
                    .store
                    .db
                    .locked()
                    .execute("UPDATE donna_space_conversations SET bot_id=?2 WHERE id=?1", [chat, &f.loja_bot])
                    .unwrap();
                f.hub.emit_bot(&f.donna);
            }
            "exclusao" => {
                f.hub.store.sem_donna_fixa(); // D366: exclusão legada.
                f.hub.delete_bot(&f.donna).unwrap();
            }
            _ => unreachable!(),
        }
        f.marker();
        let events = until_marker(stream.as_mut()).await;
        let screens: Vec<_> = events.iter().filter(|e| e["type"] == "screen").collect();
        assert_ne!(screens, Vec::<&Value>::new());
        for event in screens {
            usage(&event["screen"], "unavailable", None);
        }
        usage(&state(&f.hub, &f.loja), "unavailable", None);
        assert_eq!(f.hub.screen.state()["agentBot"], f.donna, "broker não é alterado pela projeção");
    }
}
