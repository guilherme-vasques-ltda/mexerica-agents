use super::*;
use crate::fork::runtime_audiencia::live_activity::send;
use crate::store::Scope;

fn captured(f: &Fixture, id: &str) -> Vec<Value> {
    let mut bodies = Vec::new();
    send(
        &f.hub,
        id,
        || Some("fixture-sem-rede".into()),
        |url, body| {
            assert_eq!(url, "fixture-sem-rede/push");
            bodies.push(body);
        },
    );
    bodies
}

#[tokio::test]
async fn registro_e_emissor_indisponiveis_nao_terminam_nem_consumem_ticket() {
    let f = Fixture::new().await;
    let caller = Caller::Device { key: "aparelho-fixture".into(), scopes: vec![Scope::Control], autoridade: None };
    // Ausência de observação continua distinta de Idle, inclusive no registro real.
    let reply = dispatch(&f.hub, &caller, "registerActivity", json!({"botId":f.donna,"ticket":"fixture-ticket"}))
        .await
        .unwrap();
    assert_eq!(reply, json!({}));
    assert_eq!(f.hub.store.activity_tickets(&f.donna), ["fixture-ticket"]);
    for status in [BotStatus::Working, BotStatus::NeedsInput, BotStatus::Waiting, BotStatus::Error] {
        f.runtime(&f.donna, Some(&f.loja_chat), status);
        let internal = format!("{:?}", f.hub.runtime(&f.donna));
        send(&f.hub, &f.donna, || panic!("indisponibilidade não prepara transporte"), |_, _| panic!("não envia"));
        assert_eq!(f.hub.store.activity_tickets(&f.donna), ["fixture-ticket"]);
        assert_eq!(format!("{:?}", f.hub.runtime(&f.donna)), internal);
    }
    // O término real do ator limpa a lane: não inventa um end pessoal do turno da Loja.
    f.hub.set_runtime(&f.donna, |rt| *rt = Runtime::default());
    assert_eq!(captured(&f, &f.donna), Vec::<Value>::new());
    assert_eq!(f.hub.store.activity_tickets(&f.donna), ["fixture-ticket"]);
    f.runtime(&f.donna, None, BotStatus::Error);
    assert_eq!(captured(&f, &f.donna), Vec::<Value>::new());
    assert_eq!(f.hub.store.activity_tickets(&f.donna), ["fixture-ticket"]);
    assert!(
        dispatch(&f.hub, &caller, "registerActivity", json!({"spaceId":f.loja,"botId":f.donna,"ticket":"cruzado"}))
            .await
            .is_err()
    );
    assert_eq!(f.hub.store.activity_tickets(&f.donna), ["fixture-ticket"]);
    let group = f.group();
    f.hub.store.add_activity_ticket("grupo-ticket", &group, caller.device_key()).unwrap();
    // O membro excluído não comprova runtime agregado nem término.
    f.hub.store.sem_donna_fixa(); // D366: exercita a troca/exclusão legada, sem Donna fixa.
    f.hub.store.delete_bot(&f.donna).unwrap();
    assert_eq!(captured(&f, &group), Vec::<Value>::new());
    assert_eq!(f.hub.store.activity_tickets(&group), ["grupo-ticket"]);

    let denied = Caller::Device { key: "sem-escopo".into(), scopes: vec![], autoridade: None };
    assert!(
        dispatch(&f.hub, &denied, "registerActivity", json!({"botId":f.observer,"ticket":"negado"})).await.is_err()
    );
    assert_eq!(f.hub.store.activity_tickets(&f.observer), Vec::<String>::new());
}

#[tokio::test]
async fn emissor_preserva_cinco_estados_autorizados_e_fio_movel_existente() {
    let f = Fixture::new().await;
    for (status, wire, terminal) in [
        (BotStatus::Working, "working", false),
        (BotStatus::NeedsInput, "needsInput", false),
        (BotStatus::Waiting, "waiting", false),
        (BotStatus::Error, "error", true),
        (BotStatus::Idle, "idle", true),
    ] {
        if status == BotStatus::Idle {
            f.hub.set_runtime(&f.donna, |rt| {
                *rt = Runtime { lane: Some(Lane::main(&f.personal_chat)), ..Runtime::default() }
            });
        } else {
            f.runtime(&f.donna, Some(&f.personal_chat), status);
        }
        f.hub.store.add_activity_ticket("ticket-autorizado", &f.donna, "aparelho-fixture").unwrap();
        let internal = format!("{:?}", f.hub.runtime(&f.donna));
        let bodies = captured(&f, &f.donna);
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0]["ticket"], "ticket-autorizado");
        let payload = &bodies[0]["liveActivity"];
        assert_eq!(payload["event"], if terminal { "end" } else { "update" });
        assert_eq!(payload["contentState"]["status"], wire);
        assert_eq!(payload["contentState"]["activity"], "");
        assert_eq!(payload["staleDate"].as_i64().unwrap() - payload["timestamp"].as_i64().unwrap(), 900);
        if status == BotStatus::Idle {
            assert!(payload["contentState"]["startedAt"].is_null());
        } else {
            assert_eq!(payload["contentState"]["startedAt"], 1_730_000_000_123_f64 / 1000.0 - 978_307_200.0);
        }
        assert!(!payload.to_string().contains("ATIVIDADE_PRIVADA"));
        assert!(!payload.to_string().contains(&f.personal_chat));
        assert!(payload["contentState"].get("workingChat").is_none());
        assert!(payload["contentState"].get("agentUse").is_none());
        assert_eq!(f.hub.store.activity_tickets(&f.donna).is_empty(), terminal);
        assert_eq!(format!("{:?}", f.hub.runtime(&f.donna)), internal);
    }
    // O agregado autorizado também usa a projeção atual, sem receber Runtime fabricado.
    let group = f.group();
    f.runtime(&f.donna, Some(&group), BotStatus::NeedsInput);
    f.hub.store.add_activity_ticket("ticket-grupo", &group, "aparelho-fixture").unwrap();
    let bodies = captured(&f, &group);
    assert_eq!(bodies[0]["liveActivity"]["event"], "update");
    assert_eq!(bodies[0]["liveActivity"]["contentState"]["status"], "needsInput");
    assert_eq!(f.hub.store.activity_tickets(&group), ["ticket-grupo"]);
    // Um bot nativo da Loja conserva seu contexto permitido pelo guard existente.
    dispatch(
        &f.hub,
        &Caller::Local,
        "registerActivity",
        json!({"spaceId":f.loja,"botId":f.loja_bot,"ticket":"ticket-loja"}),
    )
    .await
    .unwrap();
    f.runtime(&f.loja_bot, Some(&f.loja_bot), BotStatus::Working);
    let bodies = captured(&f, &f.loja_bot);
    assert_eq!(bodies[0]["liveActivity"]["contentState"]["status"], "working");
    assert_eq!(f.hub.store.activity_tickets(&f.loja_bot), ["ticket-loja"]);
}

#[tokio::test]
async fn pendencia_humana_e_revogacao_antes_do_envio_preservam_ticket() {
    let f = Fixture::new().await;
    f.hub.set_runtime(&f.donna, |rt| *rt = Runtime { lane: Some(Lane::main(&f.personal_chat)), ..Runtime::default() });
    f.hub.store.insert_entry(&Lane::main(&f.donna), EntryKind::User, 1, &json!({"text":"Pedido pendente"})).unwrap();
    dispatch(&f.hub, &Caller::Local, "registerActivity", json!({"botId":f.donna,"ticket":"pendente"})).await.unwrap();
    assert_eq!(captured(&f, &f.donna), Vec::<Value>::new());
    assert_eq!(f.hub.store.activity_tickets(&f.donna), ["pendente"]);

    f.runtime(&f.donna, Some(&f.personal_chat), BotStatus::Error);
    // A revalidação ocorre depois do preparo síncrono do transporte, antes de take.
    send(
        &f.hub,
        &f.donna,
        || {
            f.hub
                .store
                .db
                .locked()
                .execute("UPDATE donna_space_conversations SET closed=1 WHERE id=?1", [&f.personal_chat])
                .unwrap();
            Some("fixture-sem-rede".into())
        },
        |_, _| panic!("conversa fechada não produz end"),
    );
    assert_eq!(f.hub.store.activity_tickets(&f.donna), ["pendente"]);
    assert_eq!(f.hub.runtime(&f.donna).status, BotStatus::Error);
}
