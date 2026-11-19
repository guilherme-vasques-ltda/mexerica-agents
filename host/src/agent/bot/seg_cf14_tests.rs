//! CF-14: Device autenticado no dispatcher; cartão produzido pela ACP canônica.
//! O corte é a fila real do Hub, antes de entregar o Cmd ao consumidor real.
//! A prova observa somente o commit do cartão, não execução de ferramenta.
use super::*;
use crate::store::{Device, DeviceSource, Scope};

fn control(hub: &Hub) -> anyhow::Result<Caller> {
    hub.store.put_device(&Device {
        key: "cf14-control-sintetico".into(),
        name: "Fixture CF-14".into(),
        platform: "fixture".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: crate::store::now_ms(),
        last_seen_at: None,
    })?;
    crate::fork::autoridade::autenticar(hub, "cf14-control-sintetico")
        .map(|(_, caller)| caller)
        .map_err(|_| anyhow::anyhow!("autenticação da fixture CF-14 recusada"))
}

#[tokio::test]
async fn cf14_device_revogado_na_fila_nao_confirma_cartao_acp_original() -> anyhow::Result<()> {
    for case in ["valido", "cancelamento", "revogado"] {
        let mut f = Fixture::new(false).await;
        *f.prompt_action.locked() = Some(json!({"readSkill":f.skill}));
        let (mut actor, prompt) = reading_actor_pending(&mut f).await;
        prompt.resume.send(()).map_err(|()| anyhow::anyhow!("prompt fechado"))?;
        let _blocked = f.request("candidate-blocked").await;
        let incoming = tokio::time::timeout(
            Duration::from_secs(20),
            actor.conn.as_mut().ok_or_else(|| anyhow::anyhow!("ACP ausente"))?.rx.recv(),
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("ACP fechou antes do cartão"))?;
        assert!(
            matches!(&incoming, crate::agent::acp::Incoming::Request { method, .. } if method == "session/request_permission")
        );
        actor.on_incoming(Some(incoming)).await;
        let cards: Vec<_> = f
            .hub
            .store
            .history_in_session("bot", Some("bot"), i64::MAX, 50)?
            .into_iter()
            .filter(|entry| entry.kind == "permission")
            .collect();
        assert_eq!(cards.len(), 1, "cartão deve vir do pedido ACP real");
        let card = &cards[0];
        assert_eq!(card.data["status"], "pending");
        assert_eq!(card.data["options"][0]["kind"], "allow_once");
        assert!(actor.perms.contains_key(&card.id));
        assert!(actor.turn.is_some());
        assert_eq!(actor.turn_session.as_deref(), Some("skills-session"));
        let original_session = f.hub.store.bot("bot")?.ok_or_else(|| anyhow::anyhow!("bot ausente"))?.session_id;
        let isolated = f.hub.store.insert_entry(
            &crate::store::Lane::main("bot"),
            crate::store::EntryKind::Permission,
            card.turn,
            &json!({"status":"pending","author":"bot","options":[{"optionId":"isolada","kind":"allow_once"}]}),
        )?;
        let caller = control(&f.hub)?;
        let mut commands = f.hub.test_bot_commands("bot");
        let option = if case == "cancelamento" { Value::Null } else { json!("allow") };
        let body = json!({"botId":"bot","entryId":card.id,"entryRev":card.rev,"optionId":option});
        let mut stale = body.clone();
        stale["entryRev"] = json!(card.rev + 1);
        assert!(api::dispatch(&f.hub, &caller, "respondPermission", stale).await.is_err());
        assert!(commands.try_recv().is_err(), "revisão errada não enfileira");
        let mut invalid = body.clone();
        invalid["optionId"] = json!("opcao-inexistente");
        let mut request = Box::pin(api::dispatch(&f.hub, &caller, "respondPermission", invalid));
        assert!(futures::poll!(request.as_mut()).is_pending());
        let rejected = commands.try_recv()?;
        let (done, _results) = mpsc::unbounded_channel();
        actor.on_cmd(rejected, &done).await;
        assert!(request.await.is_err(), "opção alheia deve ser recusada pelo ator");
        assert_eq!(f.hub.store.entry(&card.id).ok_or_else(|| anyhow::anyhow!("cartão ausente"))?.rev, card.rev);
        let mut request = Box::pin(api::dispatch(&f.hub, &caller, "respondPermission", body.clone()));
        assert!(futures::poll!(request.as_mut()).is_pending());
        let command = commands.try_recv()?;
        assert!(matches!(&command, super::super::Cmd::Permission { entry_id, .. } if entry_id == &card.id));
        assert_eq!(f.hub.store.entry(&card.id).ok_or_else(|| anyhow::anyhow!("cartão ausente"))?.rev, card.rev);
        if case == "revogado" {
            assert!(f.hub.revoke_device("cf14-control-sintetico")?);
            assert!(crate::fork::autoridade::conferir_caller(&f.hub, &caller).is_err());
        } else {
            crate::fork::autoridade::conferir_caller(&f.hub, &caller)?;
        }
        assert_eq!(f.hub.store.bot("bot")?.ok_or_else(|| anyhow::anyhow!("bot ausente"))?.session_id, original_session);
        assert_eq!(actor.turn_session.as_deref(), Some("skills-session"));
        actor.on_cmd(command, &done).await;
        let result = request.await;
        let saved = f.hub.store.entry(&card.id).ok_or_else(|| anyhow::anyhow!("cartão ausente"))?;
        let untouched = f.hub.store.entry(&isolated.id).ok_or_else(|| anyhow::anyhow!("isolamento ausente"))?;
        assert_eq!(untouched.rev, isolated.rev);
        assert_eq!(untouched.data, isolated.data);
        actor.conn.take().ok_or_else(|| anyhow::anyhow!("ACP perdida"))?.kill().await;
        if case == "revogado" {
            assert_eq!(
                saved.data["status"], "pending",
                "CF-14: Device revogado antes do consumo não pode confirmar cartão ACP"
            );
            assert_eq!(saved.rev, card.rev);
            assert!(result.is_err());
        } else {
            result?;
            assert_eq!(saved.data["status"], if case == "valido" { "answered" } else { "cancelled" });
            assert_eq!(saved.data["selected"], option);
            api::dispatch(&f.hub, &caller, "respondPermission", body).await?;
            assert!(commands.try_recv().is_err(), "replay não entrega outro comando");
            assert_eq!(f.hub.store.entry(&card.id).ok_or_else(|| anyhow::anyhow!("cartão ausente"))?.rev, saved.rev);
        }
        eprintln!("CF-14 controle={case}: dispatcher/Device/ACP/fila/isolamento concluídos; evidência limitada ao DB");
    }
    Ok(())
}

/// R1-01: os atalhos sem ator (autor excluído; `send_cmd` sem destinatário) gravam o cancelamento na guarda da
/// origem ORIGINAL. Device revogado entre a captura e a escrita: erro, cartão e revisão intactos, nenhum evento.
#[tokio::test]
async fn cf14_cancelamento_sem_ator_usa_origem_original() -> anyhow::Result<()> {
    for (author, revoked) in [("excluido", true), ("excluido", false), ("parado", true), ("parado", false)] {
        let f = Fixture::new(false).await;
        // Salvo depois do Hub: "parado" existe sem ator e `send_cmd` falha; "excluido" cai antes, sem autor.
        let cfg = serde_json::from_value(json!({"id":author, "name":"Autor", "backend":"custom"}))?;
        f.hub.store.save_bot(&cfg)?;
        if author == "excluido" {
            f.hub.store.delete_bot(author)?;
        }
        let card = f.hub.store.insert_entry(
            &crate::store::Lane::main("bot"),
            crate::store::EntryKind::Permission,
            1,
            &json!({"status":"pending","author":author,"options":[{"optionId":"deny","kind":"reject_once"}]}),
        )?;
        let caller = control(&f.hub)?;
        if revoked {
            let hub = f.hub.clone();
            *f.hub.store.before_owner_write.locked() = Some(Box::new(move || {
                assert!(hub.revoke_device("cf14-control-sintetico").unwrap());
            }));
        }
        let mut events = f.hub.events.subscribe();
        let body = json!({"botId":"bot","entryId":card.id,"entryRev":card.rev,"optionId":"deny"});
        let result = api::dispatch(&f.hub, &caller, "respondPermission", body).await;
        let saved = f.hub.store.entry(&card.id).ok_or_else(|| anyhow::anyhow!("cartão ausente"))?;
        if revoked {
            let error = result.err().ok_or_else(|| anyhow::anyhow!("{author}: Device revogado encerrou o cartão"))?;
            assert!(error.is::<crate::fork::autoridade::AutorizacaoEncerrada>(), "{author}: {error:#}");
            assert_eq!(saved.data["status"], "pending", "{author}");
            assert_eq!(saved.rev, card.rev, "{author}");
            assert!(std::iter::from_fn(|| events.try_recv().ok()).all(|event| event["type"] != "entry"), "{author}");
        } else {
            result?;
            assert_eq!(saved.data["status"], "cancelled", "{author}");
            assert_eq!(saved.data["selected"], "deny", "{author}");
            let published = std::iter::from_fn(|| events.try_recv().ok()).find(|event| event["type"] == "entry");
            assert_eq!(published.map(|event| event["rev"].clone()), Some(json!(saved.rev)), "{author}");
        }
    }
    Ok(())
}
