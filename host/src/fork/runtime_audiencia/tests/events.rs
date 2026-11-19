use super::*;

#[tokio::test]
async fn evento_enfileirado_perde_runtime_ao_mudar_space_com_conversa_antiga_aberta() {
    let f = Fixture::new().await;
    f.runtime(&f.donna, Some(&f.personal_chat), BotStatus::Working);
    visible(&f.bot(&f.donna), &f.personal_chat, "working");
    let stream = scoped_events_stream(&f.hub, f.hub.store.current_rev(), None, &Caller::Local, Some(PERSONAL)).unwrap();
    tokio::pin!(stream);
    assert_eq!(stream.next().await.unwrap()["type"], "hello");
    f.hub.emit_bot(&f.donna);
    let queued_rev = f.bot(&f.donna)["rev"].as_i64().unwrap();

    // change_runtime real depois da fila, antes de qualquer consumo de bot.
    f.runtime(&f.donna, Some(&f.loja_chat), BotStatus::Working);
    assert!(!f.hub.store.space_conversation(PERSONAL, &f.personal_chat).unwrap().closed);
    let internal = format!("{:?}", f.hub.runtime(&f.donna));
    f.marker();
    let mut events = until_marker(stream.as_mut()).await;
    expect_tower_change(&mut events, 1, &f.hub, PERSONAL);
    assert_eq!(events.len(), 2, "tipos na ordem recebida: {:?}", event_types(&events));
    assert_eq!(events[0]["rev"], queued_rev);
    assert!(events[1]["rev"].as_i64().unwrap() > queued_rev);
    for event in events {
        assert_eq!(event["type"], "bot");
        assert_eq!(event["bot"]["id"], f.donna);
        unavailable(&event["bot"]);
        assert!(!event.to_string().contains("ATIVIDADE_PRIVADA"));
        assert!(!event.to_string().contains(&f.personal_chat));
        assert!(!event.to_string().contains(&f.loja_chat));
    }
    unavailable(&f.bot(&f.donna));
    assert_eq!(format!("{:?}", f.hub.runtime(&f.donna)), internal);
}

#[tokio::test]
async fn evento_antigo_na_mesma_lane_nao_preserva_atividade_e_horarios_obsoletos() {
    let f = Fixture::new().await;
    f.runtime(&f.donna, Some(&f.personal_chat), BotStatus::Working);
    let stream = scoped_events_stream(&f.hub, f.hub.store.current_rev(), None, &Caller::Local, Some(PERSONAL)).unwrap();
    tokio::pin!(stream);
    assert_eq!(stream.next().await.unwrap()["type"], "hello");
    f.hub.emit_bot(&f.donna);
    let mut queued = f.bot(&f.donna);
    f.hub.set_runtime(&f.donna, |rt| {
        rt.set_activity("ATIVIDADE_ATUAL", Some(ActivityKind::Continuing));
        rt.started_at = Some(1_730_000_001_123);
        rt.resume_at = Some(1_730_000_001_999);
    });
    let current = f.bot(&f.donna);
    assert_eq!(current["status"], "working");
    assert_eq!(current["activity"], "ATIVIDADE_ATUAL");
    assert_eq!(current["startedAt"], 1_730_000_001_123_i64);
    assert_eq!(current["resumeAt"], 1_730_000_001_999_i64);
    assert_eq!(current["workingChat"], f.personal_chat);
    let internal = format!("{:?}", f.hub.runtime(&f.donna));
    f.marker();
    let mut events = until_marker(stream.as_mut()).await;
    expect_tower_change(&mut events, 1, &f.hub, PERSONAL);
    assert_eq!(events.len(), 2, "tipos na ordem recebida: {:?}", event_types(&events));
    unavailable(&events[0]["bot"]);
    assert_eq!(events[0]["rev"], queued["rev"]);
    let mut stale = events[0]["bot"].clone();
    for field in FIELDS {
        assert_eq!(events[1]["bot"][field], current[field]);
        stale.as_object_mut().unwrap().remove(field);
        queued.as_object_mut().unwrap().remove(field);
    }
    assert_eq!(stale, queued, "não troca configuração, revisão ou outros campos do evento");
    assert_eq!(format!("{:?}", f.hub.runtime(&f.donna)), internal);
}

#[tokio::test]
async fn hello_atrasado_e_catch_up_completo_incremental_revalidam_no_consumo() {
    let f = Fixture::new().await;
    let before = f.hub.store.current_rev();
    f.runtime(&f.donna, Some(&f.personal_chat), BotStatus::Working);
    f.hub.screen.agent_for_test(Some(&f.donna));
    let full = scoped_events_stream(&f.hub, 0, None, &Caller::Local, Some(PERSONAL)).unwrap();
    let incremental = scoped_events_stream(&f.hub, before, None, &Caller::Local, Some(PERSONAL)).unwrap();
    tokio::pin!(full, incremental);
    let hello = full.next().await.unwrap();
    assert_eq!(hello["type"], "hello");
    assert_eq!(hello["screen"]["agentUse"], "using");
    assert_eq!(hello["screen"]["agentBot"], f.donna);

    // Ambos os vetores já estão prontos; só o hello do completo foi entregue.
    f.runtime(&f.donna, Some(&f.loja_chat), BotStatus::Working);
    assert!(!f.hub.store.space_conversation(PERSONAL, &f.personal_chat).unwrap().closed);
    let internal = format!("{:?}", f.hub.runtime(&f.donna));
    let hello = incremental.next().await.unwrap();
    assert_eq!(hello["type"], "hello");
    assert_eq!(hello["screen"]["agentUse"], "unavailable");
    assert!(hello["screen"]["agentBot"].is_null());
    f.marker();
    for events in [until_marker(full.as_mut()).await, until_marker(incremental.as_mut()).await] {
        let bots: Vec<_> = events.iter().filter(|e| e["type"] == "bot" && e["bot"]["id"] == f.donna).collect();
        assert_eq!(bots.len(), 2, "snapshot antigo e atualização real da cauda");
        for event in bots {
            unavailable(&event["bot"]);
        }
        for event in events.iter().filter(|e| e["type"] == "screen") {
            assert_eq!(event["screen"]["agentUse"], "unavailable");
            assert!(event["screen"]["agentBot"].is_null());
        }
    }
    assert_eq!(f.hub.screen.state()["agentBot"], f.donna, "broker preservado");
    assert_eq!(format!("{:?}", f.hub.runtime(&f.donna)), internal);
}
