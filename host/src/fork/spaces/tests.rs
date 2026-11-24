use crate::{
    LockExt,
    api::{devices::Caller, dispatch as api_dispatch},
    hub::Hub,
    remote::identity::Identity,
    store::Store,
};
use serde_json::json;
#[path = "birth_tests.rs"]
mod birth_tests;
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;

/// D543, fatia 1: dados sintéticos, sem enviar turnos a motores reais.
mod d543 {
    use super::*;
    use crate::store::{BotConfig, EntryKind, Lane};

    async fn fixture() -> (std::sync::Arc<Hub>, std::path::PathBuf, String) {
        let dir =
            std::fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("spaces-d543-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Hub::new(
            Store::open(std::path::Path::new(":memory:")).unwrap(),
            "fixture".into(),
            Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        let first = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({})).await.unwrap();
        let spaces = dispatch(
            &hub,
            &Caller::Local,
            "donnaSpaceCreate",
            json!({"expectedRevision":first["revision"],"name":"Trabalho",
                "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}}),
        )
        .await
        .unwrap();
        let work = spaces["spaces"][1]["id"].as_str().unwrap().to_owned();
        // save_bot não valida o Registry: este ID prova despacho/contexto, não conta executável.
        // A criação via API com conta cadastrada tem cobertura própria abaixo; runtime real é gate externo.
        for (id, space) in [("donna", super::super::PERSONAL), ("work-bot", work.as_str())] {
            let bot: BotConfig = serde_json::from_value(json!({
                "id":id,"name":id,"spaceId":space,"backend":"fixture","cwd":dir,
                "forkEditor":{"engineAccountId":"conta-sintetica-do-bot"}
            }))
            .unwrap();
            hub.store.save_bot(&bot).unwrap();
        }
        hub.store.set_team_settings(&json!({"primaryBotId":"donna"})).unwrap();
        (hub, dir, work)
    }

    fn connected_registry(dir: &std::path::Path, id: &str) -> serde_json::Value {
        json!({
            "salt":uuid::Uuid::new_v4().to_string(),"accounts":[{
                "id":id,"backend":"codex","billing":"paid_api","state":"connected","scope":"owner",
                "launch":{"kind":"cli_profile","executable":dir.join("cli-inexistente"),
                    "directory":dir.join("perfil-sintetico")},
                "quotaIdentity":null,"verifiedAt":null,"subscriptionProof":null,"models":[]
            }]
        })
    }

    #[tokio::test]
    async fn create_bot_api_recusa_contexto_ou_conta_ausente_sem_efeitos() {
        use crate::fork::engine_accounts::{Registry, SLOT, managed};

        let (hub, dir, work) = fixture().await;
        let local_id = format!("ac_{}", "3".repeat(64));
        hub.store.kv_set(SLOT, &connected_registry(&dir, &local_id).to_string()).unwrap();
        let workspace = dir.join("workspace-sintetico");
        std::fs::create_dir_all(&workspace).unwrap();
        let body = json!({"hubId":hub.host_id,"spaceId":work,"name":"Bot sintético do Space",
            "backend":"codex","cwd":workspace,"forkEditor":{"engineAccountId":local_id}});
        // A mesma seleção válida do positivo: nenhuma recusa pode depender de conta inválida.
        let mut candidate = body.clone();
        candidate["id"] = json!("candidato-sintetico");
        let candidate: BotConfig = serde_json::from_value(candidate).unwrap();
        managed::validate_selection_in(&Registry::load(&hub.store).unwrap(), &candidate).unwrap();
        let mut failures = Vec::new();
        for (case, expected) in [
            ("hub_ausente", "hub_context_mismatch"),
            ("hub_errado", "hub_context_mismatch"),
            ("pessoal_hub_ausente", "hub_context_mismatch"),
            ("pessoal_hub_errado", "hub_context_mismatch"),
            ("space_inexistente", "space_not_found"),
            ("conta_ausente", "space_account_selection_required"),
        ] {
            let mut refused = body.clone();
            if case.starts_with("pessoal_") {
                refused["spaceId"] = json!(super::super::PERSONAL);
            }
            match case {
                "hub_ausente" | "pessoal_hub_ausente" => {
                    refused.as_object_mut().unwrap().remove("hubId");
                }
                "hub_errado" | "pessoal_hub_errado" => refused["hubId"] = json!("outro-hub-sintetico"),
                "space_inexistente" => refused["spaceId"] = json!("space-sintetico-inexistente"),
                "conta_ausente" => {
                    refused["forkEditor"].as_object_mut().unwrap().remove("engineAccountId");
                }
                _ => unreachable!(),
            }
            let before = hub.store.bots().unwrap().len();
            let revision = hub.store.current_rev();
            let mut events = hub.events.subscribe();
            let result = api_dispatch(&hub, &Caller::Local, "createBot", refused).await;
            if !result.as_ref().is_err_and(|error| format!("{error:#}").contains(expected)) {
                failures.push(format!("{case}: deveria recusar com {expected}; resultado={result:?}"));
            }
            if hub.store.bots().unwrap().len() != before || hub.store.current_rev() != revision {
                failures.push(format!("{case}: persistiu efeitos na recusa"));
            }
            if events.try_recv().is_ok() {
                failures.push(format!("{case}: publicou evento na recusa"));
            }
        }
        // As tentativas indevidamente aceitas também são encerradas antes de reportar o RED.
        hub.shutdown().await;
        drop(hub);
        std::fs::remove_dir_all(dir).unwrap();
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[tokio::test]
    async fn create_bot_no_space_vincula_legacy_local_e_recusa_conta_de_outro_registry() {
        use crate::fork::engine_accounts::{Registry, SLOT, managed};

        let (hub, dir, work) = fixture().await;
        let local_id = format!("ac_{}", "3".repeat(64));
        let foreign_id = format!("ac_{}", "4".repeat(64));
        hub.store.kv_set(SLOT, &connected_registry(&dir, &local_id).to_string()).unwrap();
        let foreign = Store::open(std::path::Path::new(":memory:")).unwrap();
        foreign.kv_set(SLOT, &connected_registry(&dir, &foreign_id).to_string()).unwrap();
        let workspace = dir.join("workspace-sintetico");
        std::fs::create_dir_all(&workspace).unwrap();
        let body = json!({"hubId":hub.host_id,"spaceId":work,"name":"Bot sintético do Space",
            "backend":"codex","cwd":workspace,
            "forkEditor":{"engineAccountId":local_id}});
        let mut refused = body.clone();
        refused["forkEditor"]["engineAccountId"] = json!(foreign_id);
        let candidate: BotConfig = serde_json::from_value(json!({"id":"candidato","name":"Candidato",
            "backend":"codex","forkEditor":{"engineAccountId":foreign_id}}))
        .unwrap();
        managed::validate_selection_in(&Registry::load(&foreign).unwrap(), &candidate).unwrap();
        let before = hub.store.bots().unwrap().len();
        let revision = hub.store.current_rev();
        let mut events = hub.events.subscribe();
        let error = api_dispatch(&hub, &Caller::Local, "createBot", refused).await.unwrap_err();
        assert!(format!("{error:#}").contains("account_not_found"), "{error:#}");
        assert_eq!(hub.store.bots().unwrap().len(), before, "nenhum bot parcial na recusa");
        assert_eq!(hub.store.current_rev(), revision);
        assert!(events.try_recv().is_err());

        let personal = hub.store.bot("donna").unwrap().unwrap().config;
        let created = api_dispatch(&hub, &Caller::Local, "createBot", body).await.unwrap();
        let id = created["bot"]["id"].as_str().unwrap();
        let bot = hub.store.bot(id).unwrap().unwrap().config;
        assert_eq!(bot.space_id, work);
        assert_eq!(bot.fork_editor.engine_account_id.as_deref(), Some(local_id.as_str()));
        let local_registry = Registry::load(&hub.store).unwrap();
        let account = &local_registry.accounts[0];
        assert!(account.managed.is_none(), "conta legacy, sem perfil gerenciado");
        assert_eq!(account.state, crate::fork::engine_accounts::State::Connected);
        managed::validate_selection(&hub.store, &bot).unwrap();
        let cfg = super::super::execution_config(&hub.store, &bot, id).unwrap();
        assert_eq!(cfg.fork_editor.engine_account_id, bot.fork_editor.engine_account_id);
        assert_ne!(cfg.memory_id(), personal.memory_id());
        assert!(!crate::chat::memory::shared(&cfg));
        assert!(!cfg.manage_team && !cfg.harness_memory);
        for (owner, text) in [
            (Some(personal.memory_id()), "PESSOAL_CREATE_SINTETICO"),
            (None, "COMPARTILHADA_CREATE_SINTETICA"),
            (Some(cfg.memory_id()), "SPACE_CREATE_SINTETICO"),
        ] {
            hub.store
                .memory_write(|c, now, rev| {
                    codync_mem::store::add(
                        c,
                        codync_mem::store::Writer::User,
                        codync_mem::store::New { bot: owner, text, room: None, evidence: &[] },
                        now,
                        rev,
                    )
                })
                .unwrap();
        }
        let opening = crate::chat::memory::opening_scored(&hub.store, &cfg, None);
        assert!(opening.contains("SPACE_CREATE_SINTETICO"));
        assert!(!opening.contains("PESSOAL_CREATE_SINTETICO"));
        assert!(!opening.contains("COMPARTILHADA_CREATE_SINTETICA"));
        assert!(!crate::chat::memory::opening_scored(&hub.store, &personal, None).contains("SPACE_CREATE_SINTETICO"));
        assert_eq!(hub.store.bot("donna").unwrap().unwrap().config.fork_editor, personal.fork_editor);
        // Valida seleção e isolamento; não prova login/CLI executável nem turno real do host.
        // createBot inicia um ator ocioso; encerrá-lo sem enviar mensagens não executa motor.
        hub.shutdown().await;
        drop(hub);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn send_canonico_confere_contexto_antes_de_replay_ou_efeito() {
        let (hub, dir, work) = fixture().await;
        let mut commands = hub.test_bot_commands("work-bot");
        let mut personal_commands = hub.test_bot_commands("donna");
        let personal = hub
            .store
            .insert_entry(&Lane::main("donna"), EntryKind::User, 1, &json!({"text":"HISTORICO_PESSOAL_SINTETICO"}))
            .unwrap();
        let request = json!({"hubId":hub.host_id,"spaceId":work,"conversationId":"work-bot",
            "botId":"work-bot","text":"ENVIO_TRABALHO_SINTETICO","clientNonce":"d543-canonica"});
        let sent = api_dispatch(&hub, &Caller::Local, "send", request.clone()).await.unwrap();
        let id = sent["entry"]["id"].as_str().unwrap();
        let entry = hub.store.entry(id).unwrap();
        assert_eq!(entry.bot_id, "work-bot");
        assert_eq!(entry.data["spaceContext"]["spaceId"], work);
        assert_eq!(entry.data["spaceContext"]["conversationId"], "work-bot");
        match commands.try_recv().unwrap() {
            crate::agent::bot::Cmd::Send { lane, entry_id, .. } => {
                assert_eq!(lane, Lane::main("work-bot"));
                assert_eq!(entry_id, id);
            }
            _ => panic!("comando inesperado"),
        }
        let replay = api_dispatch(&hub, &Caller::Local, "send", request.clone()).await.unwrap();
        assert_eq!(replay["entry"]["id"], id);

        // Mesmo nonce já persistido: a guarda precisa recusar antes do atalho de replay.
        let mut wrong_hub = request.clone();
        wrong_hub["hubId"] = json!("outro-hub-sintetico");
        let mut wrong_space = request.clone();
        wrong_space["spaceId"] = json!(super::super::PERSONAL);
        let mut omitted_space = request.clone();
        omitted_space.as_object_mut().unwrap().remove("spaceId");
        let mut wrong_bot = request.clone();
        wrong_bot["botId"] = json!("donna");
        let mut wrong_reply = request.clone();
        wrong_reply["replyTo"] = json!(personal.id);
        for (body, code) in [
            (wrong_hub, "hub_context_mismatch"),
            (wrong_space, "conversation_not_found"),
            (omitted_space, "conversation_not_found"),
            (wrong_bot, "cross_space_reference"),
            (wrong_reply, "cross_space_reference"),
        ] {
            // Cobrir também envio novo: nada pode ser alocado ou publicado na recusa.
            for nonce in ["d543-canonica", "d543-recusa-nova"] {
                let mut body = body.clone();
                body["clientNonce"] = json!(nonce);
                let revision = hub.store.current_rev();
                let turn = hub.store.max_turn("work-bot");
                let mut events = hub.events.subscribe();
                let error = api_dispatch(&hub, &Caller::Local, "send", body).await.unwrap_err();
                assert!(format!("{error:#}").contains(code), "{error:#}");
                assert_eq!(hub.store.current_rev(), revision);
                assert_eq!(hub.store.max_turn("work-bot"), turn);
                assert!(events.try_recv().is_err());
                assert!(commands.try_recv().is_err());
                assert!(personal_commands.try_recv().is_err());
                assert!(hub.store.find_by_nonce("work-bot", "d543-recusa-nova").is_none());
            }
        }
        let history = api_dispatch(&hub, &Caller::Local, "history", request.clone()).await.unwrap();
        assert_eq!(history["entries"].as_array().unwrap().len(), 1);
        assert_eq!(history["entries"][0]["id"], id);
        assert!(!history.to_string().contains("HISTORICO_PESSOAL_SINTETICO"));
        let bot = hub.store.bot("work-bot").unwrap().unwrap().config;
        let cfg = super::super::execution_config(&hub.store, &bot, "work-bot").unwrap();
        assert!(cfg.context.is_none());
        assert_eq!(cfg.memory_id(), bot.memory_id());
        assert_eq!(cfg.fork_editor.engine_account_id, bot.fork_editor.engine_account_id);
        assert!(!crate::chat::memory::shared(&cfg));
        assert_eq!(hub.store.history("donna", i64::MAX, 30).unwrap().len(), 1);
        drop(hub);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn reconstruir_historico_respeita_corte_e_libera_banco() {
        let (hub, dir, work) = fixture().await;
        let created =
            dispatch(&hub, &Caller::Local, "donnaConversationCreate", json!({"spaceId":work,"botId":"donna"}))
                .await
                .unwrap();
        let chat = created["conversation"]["id"].as_str().unwrap();
        let bot = hub.store.bot("donna").unwrap().unwrap().config;
        let mut cfg = super::super::execution_config(&hub.store, &bot, chat).unwrap();
        cfg.person = Some(crate::chat::memory::profile::OWNER.into());
        hub.store
            .insert_entry(
                &Lane::main(chat),
                EntryKind::User,
                1,
                &json!({"text":"ANTES_CORTE","contentOrigin":{"ingress":{"caller":"local"}}}),
            )
            .unwrap();
        let cut = hub.store.last_seq(chat);
        hub.store.kv_set(&crate::chat::memory::history_cut_key(chat), &cut.to_string()).unwrap();
        hub.store
            .insert_entry(
                &Lane::main(chat),
                EntryKind::User,
                2,
                &json!({"text":"APOS_CORTE","contentOrigin":{"ingress":{"caller":"local"}}}),
            )
            .unwrap();
        hub.store
            .insert_entry(
                &Lane::main(chat),
                EntryKind::User,
                3,
                &json!({"text":"OUTRA_PESSOA_NA_FILA","status":"queued"}),
            )
            .unwrap();
        hub.store.insert_entry(&Lane::main(chat), EntryKind::User, 4, &json!({"text":"AUTOR_DESCONHECIDO"})).unwrap();
        hub.store
            .insert_entry(&Lane::main(chat), EntryKind::Agent, 4, &json!({"text":"RESPOSTA_DESCONHECIDA","final":true}))
            .unwrap();
        let history = super::super::history_context(&hub.store, &cfg, &[]).unwrap().unwrap().0;
        assert!(!history.contains("ANTES_CORTE"));
        assert!(history.contains("APOS_CORTE"));
        assert!(!history.contains("OUTRA_PESSOA_NA_FILA"));
        assert!(!history.contains("AUTOR_DESCONHECIDO"));
        assert!(!history.contains("RESPOSTA_DESCONHECIDA"));
        assert!(hub.store.current_rev() > 0);
        // Persiste a cópia do banco e reabre como num reinício, sem carregar o corte em memória.
        let database = dir.join("restart.sqlite");
        hub.store.db.locked().execute("VACUUM INTO ?1", [database.to_str().unwrap()]).unwrap();
        let reopened = Store::open(&database).unwrap();
        let history = super::super::history_context(&reopened, &cfg, &[]).unwrap().unwrap().0;
        assert!(!history.contains("ANTES_CORTE"));
        assert!(history.contains("APOS_CORTE"));
        assert!(!history.contains("OUTRA_PESSOA_NA_FILA"));
        assert!(!history.contains("AUTOR_DESCONHECIDO"));
        assert!(!history.contains("RESPOSTA_DESCONHECIDA"));
        assert!(reopened.current_rev() > 0);
        for invalid in ["inválido", "-1", "9223372036854775807"] {
            reopened.kv_set(&crate::chat::memory::history_cut_key(chat), invalid).unwrap();
            assert!(super::super::history_context(&reopened, &cfg, &[]).is_err());
        }
        drop(reopened);
        drop(hub);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn adicional_exige_api_contextual_e_conta_sem_herdar_memoria_pessoal() {
        let (hub, dir, work) = fixture().await;
        let mut commands = hub.test_bot_commands("donna");
        let created =
            dispatch(&hub, &Caller::Local, "donnaConversationCreate", json!({"spaceId":work,"botId":"donna"}))
                .await
                .unwrap();
        let chat = created["conversation"]["id"].as_str().unwrap();
        let body = json!({"hubId":hub.host_id,"spaceId":work,"conversationId":chat,"botId":"donna",
            "expectedRevision":created["conversation"]["revision"],"text":"ADICIONAL_SINTETICA",
            "clientNonce":"d543-adicional"});
        let original = hub.store.bot("donna").unwrap().unwrap().config;
        let cfg = super::super::execution_config(&hub.store, &original, chat).unwrap();
        assert_ne!(cfg.memory_id(), original.memory_id());
        assert_eq!(cfg.fork_editor.engine_account_id, None, "não herda a conta canônica");
        assert!(cfg.context.as_ref().unwrap().foreign_identity);
        assert!(!crate::chat::memory::shared(&cfg));
        for (owner, text) in [
            (None, "MEMORIA_COMPARTILHADA_SINTETICA"),
            (Some(original.memory_id()), "MEMORIA_PESSOAL_SINTETICA"),
            (Some(cfg.memory_id()), "MEMORIA_TRABALHO_SINTETICA"),
        ] {
            hub.store
                .memory_write(|c, now, rev| {
                    codync_mem::store::add(
                        c,
                        codync_mem::store::Writer::User,
                        codync_mem::store::New { bot: owner, text, room: None, evidence: &[] },
                        now,
                        rev,
                    )
                })
                .unwrap();
        }
        let opening = crate::chat::memory::opening_scored(&hub.store, &cfg, None);
        assert!(opening.contains("MEMORIA_TRABALHO_SINTETICA"));
        assert!(!opening.contains("MEMORIA_PESSOAL_SINTETICA"));
        assert!(!opening.contains("MEMORIA_COMPARTILHADA_SINTETICA"));
        assert!(
            !crate::chat::memory::opening_scored(&hub.store, &original, None).contains("MEMORIA_TRABALHO_SINTETICA")
        );

        for (owner, text) in [("donna", "HISTORICO_PESSOAL_SINTETICO"), (chat, "HISTORICO_ADICIONAL_SINTETICO")] {
            hub.store
                .insert_entry(
                    &Lane::main(owner),
                    EntryKind::User,
                    1,
                    &json!({"text":text,"contentOrigin":{"ingress":{"caller":"local"}}}),
                )
                .unwrap();
        }
        let mut cfg_com_pessoa = cfg.clone();
        cfg_com_pessoa.person = Some(crate::chat::memory::profile::OWNER.into());
        let history = super::super::history_context(&hub.store, &cfg_com_pessoa, &[]).unwrap().unwrap().0;
        assert!(history.contains("HISTORICO_ADICIONAL_SINTETICO"));
        assert!(!history.contains("HISTORICO_PESSOAL_SINTETICO"));

        let mut wrong_hub = body.clone();
        wrong_hub["hubId"] = json!("outro-hub-sintetico");
        let mut wrong_space = body.clone();
        wrong_space["spaceId"] = json!(super::super::PERSONAL);
        for (method, request, code) in [
            ("send", body.clone(), "use_contextual_conversation_api"),
            ("donnaConversationSend", wrong_hub, "hub_context_mismatch"),
            ("donnaConversationSend", wrong_space, "conversation_not_found"),
            ("donnaConversationSend", body, "space_account_selection_required"),
        ] {
            let revision = hub.store.current_rev();
            let seq = hub.store.last_seq(chat);
            let mut events = hub.events.subscribe();
            let error = api_dispatch(&hub, &Caller::Local, method, request).await.unwrap_err();
            assert!(format!("{error:#}").contains(code), "{error:#}");
            assert_eq!(hub.store.current_rev(), revision);
            assert_eq!(hub.store.last_seq(chat), seq);
            assert!(hub.store.find_by_nonce(chat, "d543-adicional").is_none());
            assert!(events.try_recv().is_err());
            assert!(commands.try_recv().is_err());
        }
        assert_eq!(
            hub.store.bot("donna").unwrap().unwrap().config.fork_editor.engine_account_id,
            original.fork_editor.engine_account_id
        );
        drop(hub);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

async fn dispatch(
    hub: &std::sync::Arc<Hub>,
    caller: &Caller,
    method: &str,
    mut body: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    if (method.starts_with("donnaSpace")
        || method.starts_with("donnaConversation")
        || method == "donnaOwnerPresentation")
        && body.get("hubId").is_none()
    {
        body["hubId"] = hub.host_id.clone().into();
    }
    api_dispatch(hub, caller, method, body).await
}

#[tokio::test]
async fn spaces_start_with_one_persistent_personal_context() {
    let dir =
        std::fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("donnabot-spaces-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.db");
    let hub = Hub::new(
        Store::open(&path).unwrap(),
        "fixture".into(),
        Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    let first = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({})).await.expect("Spaces API is available");
    assert_eq!(first["spaces"].as_array().unwrap().len(), 1);
    assert_eq!(first["spaces"][0]["kind"], "personal");
    assert_eq!(first["spaces"][0]["name"], "Pessoal");
    assert_eq!(first["spaces"][0]["position"], 0);
    drop(hub);
    let hub = Hub::new(
        Store::open(&path).unwrap(),
        "fixture".into(),
        Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    let again = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({})).await.unwrap();
    assert_eq!(first, again);
    drop(hub);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn spaces_are_created_empty_and_personal_is_immutable_under_cas() {
    let dir = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("donnabot-spaces-cas-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
        "fixture".into(),
        Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    let first = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({})).await.unwrap();
    let request = json!({"expectedRevision":first["revision"],"name":"Trabalho","tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}});
    let created =
        dispatch(&hub, &Caller::Local, "donnaSpaceCreate", request.clone()).await.expect("creates a real Space");
    assert_eq!(created["spaces"].as_array().unwrap().len(), 2);
    assert!(hub.store.bots().unwrap().is_empty(), "creating a Space cannot clone Donna");
    assert!(dispatch(&hub, &Caller::Local, "donnaSpaceCreate", request).await.is_err(), "stale revision rejected");
    let personal = created["personalSpaceId"].clone();
    for patch in [
        json!({"name":"Outro"}),
        json!({"tile":created["spaces"][1]["tile"]}),
        json!({"position":2}),
        json!({"id":"another"}),
    ] {
        assert!(
            dispatch(
                &hub,
                &Caller::Local,
                "donnaSpaceUpdate",
                json!({"spaceId":personal,"expectedRevision":created["revision"],"patch":patch})
            )
            .await
            .is_err()
        );
    }
    assert!(
        dispatch(
            &hub,
            &Caller::Local,
            "donnaSpaceReorder",
            json!({"expectedRevision":created["revision"],"ids":[created["spaces"][1]["id"],personal]})
        )
        .await
        .is_err()
    );
    assert_eq!(dispatch(&hub, &Caller::Local, "donnaSpaces", json!({})).await.unwrap(), created);
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn explicit_space_is_validated_before_reading_or_mutating_ids() {
    let dir = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("donnabot-spaces-scope-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
        "fixture".into(),
        Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    let first = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({})).await.unwrap();
    let spaces=dispatch(&hub,&Caller::Local,"donnaSpaceCreate",json!({"expectedRevision":first["revision"],"name":"Trabalho","tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}})).await.unwrap();
    let work = spaces["spaces"][1]["id"].clone();
    assert!(
        dispatch(&hub, &Caller::Local, "donnaConversations", json!({"hubId":"different-hub","spaceId":work}))
            .await
            .is_err()
    );
    assert!(api_dispatch(&hub, &Caller::Local, "donnaConversations", json!({"spaceId":work})).await.is_err());

    let bot: crate::store::BotConfig =
        serde_json::from_value(json!({"id":"work-bot","name":"Teste","spaceId":work,"cwd":dir,"backend":"fixture"}))
            .unwrap();
    hub.store.save_bot(&bot).unwrap();
    let entry = hub
        .store
        .insert_entry(
            &crate::store::Lane::main("work-bot"),
            crate::store::EntryKind::User,
            1,
            &json!({"text":"somente Trabalho"}),
        )
        .unwrap();
    assert!(
        dispatch(&hub, &Caller::Local, "history", json!({"botId":"work-bot"})).await.is_err(),
        "legacy omission means only Personal"
    );
    assert!(
        dispatch(&hub, &Caller::Local, "history", json!({"spaceId":spaces["personalSpaceId"],"botId":"work-bot"}))
            .await
            .is_err(),
        "ID cannot override explicit context"
    );
    let own = dispatch(&hub, &Caller::Local, "history", json!({"spaceId":work,"botId":"work-bot"})).await.unwrap();
    assert_eq!(own["entries"][0]["id"], entry.id);
    assert!(dispatch(&hub, &Caller::Local, "react", json!({"entryId":entry.id,"emoji":"ok"})).await.is_err());
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn legacy_event_stream_is_personal_before_roster_counts_and_history_delivery() {
    use futures::StreamExt;
    let dir = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("donnabot-spaces-events-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
        "fixture".into(),
        Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    let first = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({})).await.unwrap();
    let spaces=dispatch(&hub,&Caller::Local,"donnaSpaceCreate",json!({"expectedRevision":first["revision"],"name":"Trabalho","tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}})).await.unwrap();
    let work = spaces["spaces"][1]["id"].clone();
    for (id, space) in [("personal", spaces["personalSpaceId"].clone()), ("work", work)] {
        let bot =
            serde_json::from_value(json!({"id":id,"name":id,"spaceId":space,"cwd":dir,"backend":"fixture"})).unwrap();
        hub.store.save_bot(&bot).unwrap();
        hub.store
            .insert_entry(&crate::store::Lane::main(id), crate::store::EntryKind::User, 1, &json!({"text":id}))
            .unwrap();
    }
    let stream = crate::api::events_stream(&hub, 0, None, &Caller::Local).unwrap();
    tokio::pin!(stream);
    let mut values = vec![];
    while let Ok(Some(v)) = tokio::time::timeout(std::time::Duration::from_millis(20), stream.next()).await {
        values.push(v);
    }
    assert!(
        !values.iter().any(|v| v["bot"]["id"] == "work" || v["entry"]["botId"] == "work"),
        "legacy stream cannot deliver another context"
    );
    assert!(values.iter().any(|v| v["bot"]["id"] == "personal"));
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn donna_has_one_identity_and_separate_persisted_conversations_without_implicit_account() {
    let dir = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("donnabot-spaces-donna-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
        "fixture".into(),
        Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    let donna = serde_json::from_value(json!({"id":"donna","name":"Donna","cwd":dir,"backend":"claude"})).unwrap();
    hub.store.save_bot(&donna).unwrap();
    hub.store.kv_set("team.settings.v1", r#"{"primaryBotId":"donna","permission":"ask"}"#).unwrap();
    let old = hub
        .store
        .insert_entry(
            &crate::store::Lane::main("donna"),
            crate::store::EntryKind::User,
            1,
            &json!({"text":"Pessoal privado"}),
        )
        .unwrap();
    let first = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({})).await.unwrap();
    let spaces=dispatch(&hub,&Caller::Local,"donnaSpaceCreate",json!({"expectedRevision":first["revision"],"name":"Trabalho","tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}})).await.unwrap();
    let work = spaces["spaces"][1]["id"].clone();
    let created = dispatch(&hub, &Caller::Local, "donnaConversationCreate", json!({"spaceId":work,"botId":"donna"}))
        .await
        .expect("Donna context conversation is persisted");
    let chat = created["conversation"]["id"].as_str().unwrap();
    assert_ne!(chat, "donna");
    assert_eq!(created["conversation"]["botId"], "donna");
    assert_eq!(created["conversation"]["engineAccountId"], serde_json::Value::Null);
    assert_eq!(hub.store.bots().unwrap().len(), 1, "conversation cannot clone Donna");
    let own = hub
        .store
        .insert_entry(
            &crate::store::Lane::main(chat),
            crate::store::EntryKind::User,
            1,
            &json!({"text":"Trabalho somente"}),
        )
        .unwrap();
    let page = dispatch(
        &hub,
        &Caller::Local,
        "donnaConversationHistory",
        json!({"spaceId":work,"conversationId":chat,"query":"Trabalho"}),
    )
    .await
    .unwrap();
    assert_eq!(page["entries"][0]["id"], own.id);
    assert!(!page.to_string().contains(&old.id));
    assert!(dispatch(&hub, &Caller::Local, "donnaConversationHistory", json!({"conversationId":chat})).await.is_err());
    assert!(
        dispatch(
            &hub,
            &Caller::Local,
            "donnaConversationSend",
            json!({"spaceId":work,"conversationId":chat,"text":"do not run","clientNonce":"fixture"})
        )
        .await
        .is_err()
    );
    let history = hub.store.history(chat, i64::MAX, 100).unwrap();
    assert_eq!(history.len(), 2, "somente mensagem existente e aviso foreign; sem execução enfileirada");
    assert_eq!(history.iter().filter(|e| e.kind == "notice" && e.data["text"] == super::notice::TEXT).count(), 1);
    let primary = dispatch(&hub, &Caller::Local, "donnaConversations", json!({})).await.unwrap();
    assert_eq!(primary["conversations"][0]["id"], "donna");
    assert!(
        dispatch(
            &hub,
            &Caller::Local,
            "donnaConversationUpdate",
            json!({"conversationId":"donna","expectedRevision":primary["conversations"][0]["revision"],"closed":true})
        )
        .await
        .is_err()
    );
    hub.store.sem_donna_fixa(); // D366: exercita a troca/exclusão legada, sem Donna fixa.
    hub.store.delete_bot("donna").unwrap();
    assert!(
        dispatch(&hub, &Caller::Local, "donnaConversationHistory", json!({"spaceId":work,"conversationId":chat}))
            .await
            .is_err()
    );
    assert_eq!(
        super::filter_event(
            &hub,
            work.as_str().unwrap(),
            json!({"type":"botDeleted","id":"donna","rev":hub.store.current_rev()})
        )
        .unwrap()["type"],
        "resync"
    );
    assert!(hub.store.space_conversations(work.as_str().unwrap()).unwrap().is_empty());
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn donna_context_never_reuses_personal_memory_workspace_or_session_slot() {
    let dir = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("donnabot-spaces-context-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir.join("fixture.db")).unwrap();
    let donna=serde_json::from_value(json!({"id":"donna","name":"Donna","cwd":dir,"backend":"claude","description":"PERSONAL_PROMPT_SENTINEL","sharedMemory":true,"skills":["personal-skill"],"connectors":["personal-connector"],"logins":["personal-login"],"computer":true})).unwrap();
    store.save_bot(&donna).unwrap();
    store.kv_set("team.settings.v1", r#"{"primaryBotId":"donna","permission":"ask"}"#).unwrap();
    let hub = Hub::new(store, "fixture".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
    let first = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({})).await.unwrap();
    let spaces=dispatch(&hub,&Caller::Local,"donnaSpaceCreate",json!({"expectedRevision":first["revision"],"name":"Trabalho","tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}})).await.unwrap();
    let work = spaces["spaces"][1]["id"].as_str().unwrap();
    let created = dispatch(&hub, &Caller::Local, "donnaConversationCreate", json!({"spaceId":work,"botId":"donna"}))
        .await
        .unwrap();
    let chat = created["conversation"]["id"].as_str().unwrap();
    let cfg = super::execution_config(&hub.store, &donna, chat)
        .expect("context configuration exists independently from execution");
    assert_eq!(cfg.id, "donna");
    assert_eq!(cfg.space_id, work);
    assert_ne!(cfg.cwd, donna.cwd);
    assert!(cfg.connectors.is_empty() && cfg.logins.is_empty() && cfg.skills.is_empty());
    assert!(!cfg.computer && !cfg.manage_team && !cfg.harness_memory);
    assert!(!crate::chat::memory::shared(&cfg));
    assert_ne!(cfg.memory_id(), "donna");
    let opening = crate::chat::context::render(&hub.store, &cfg, crate::agent::bot::Instructions::FirstMessage);
    assert!(!opening.contains("PERSONAL_PROMPT_SENTINEL"));
    assert_eq!(super::session_slot(&cfg), Some(format!("conversation:{chat}")));
    for _ in 0..2 {
        super::notice::ensure(&hub, &donna, chat, work).unwrap();
    }
    let notice_id = format!("space-foreign-{chat}");
    assert_eq!(hub.store.history(chat, i64::MAX, 30).unwrap().iter().filter(|e| e.id == notice_id).count(), 1);
    // Simula conversa de uma versão anterior ao aviso e faz o backfill duas vezes.
    hub.store.db.locked().execute("DELETE FROM entries WHERE id=?1", [&notice_id]).unwrap();
    super::notice::backfill(&hub).unwrap();
    super::notice::backfill(&hub).unwrap();
    assert_eq!(hub.store.history(chat, i64::MAX, 30).unwrap().iter().filter(|e| e.id == notice_id).count(), 1);

    assert_ne!(crate::chat::uploads::root(&cfg), crate::chat::uploads::root(&donna));
    // MCPs seguem ausentes; a nova guarda não desativa o protocolo nativo de permissão já existente.
    let permission = hub
        .add_entry(
            &crate::store::Lane::main(chat),
            crate::store::EntryKind::Permission,
            1,
            &json!({"author":"donna","status":"pending"}),
        )
        .unwrap();
    super::tool_context::record_card(&hub, &cfg, &permission).unwrap();
    assert!(hub.store.kv_read(&format!("donna.tools.card.{}", permission.id)).unwrap().is_none());

    assert_eq!(
        hub.store.bot("donna").unwrap().unwrap().config.cwd,
        donna.cwd,
        "effective context cannot rewrite Donna"
    );
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn non_personal_memory_screen_cannot_expose_or_edit_shared_personal_items() {
    let dir = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("donnabot-spaces-mem-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
        "fixture".into(),
        Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    let first = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({})).await.unwrap();
    let spaces=dispatch(&hub,&Caller::Local,"donnaSpaceCreate",json!({"expectedRevision":first["revision"],"name":"Trabalho","tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}})).await.unwrap();
    let work = spaces["spaces"][1]["id"].as_str().unwrap();
    let bot =
        serde_json::from_value(json!({"id":"work-bot","name":"Teste","spaceId":work,"cwd":dir,"backend":"fixture"}))
            .unwrap();
    hub.store.save_bot(&bot).unwrap();
    let private = hub
        .store
        .memory_write(|c, now, rev| {
            codync_mem::store::add(
                c,
                codync_mem::store::Writer::User,
                codync_mem::store::New { bot: None, text: "PERSONAL_MEMORY_SENTINEL", room: None, evidence: &[] },
                now,
                rev,
            )
        })
        .unwrap();
    let screen = dispatch(&hub, &Caller::Local, "memory", json!({"spaceId":work,"botId":"work-bot"})).await.unwrap();
    assert!(
        !screen.to_string().contains("PERSONAL_MEMORY_SENTINEL"),
        "sharedMemory false is not enough if the UI API still returns it"
    );
    assert!(
        dispatch(&hub, &Caller::Local, "archiveMemory", json!({"spaceId":work,"botId":"work-bot","id":private.id}))
            .await
            .is_err()
    );
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn collections_count_and_organize_only_bots_of_the_selected_space() {
    let dir = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("donnabot-spaces-collections-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
        "fixture".into(),
        Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    let first = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({})).await.unwrap();
    let spaces=dispatch(&hub,&Caller::Local,"donnaSpaceCreate",json!({"expectedRevision":first["revision"],"name":"Trabalho","tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}})).await.unwrap();
    let work = spaces["spaces"][1]["id"].as_str().unwrap();
    let personal = super::PERSONAL;
    for (id, space) in [("personal-bot", personal), ("work-bot", work)] {
        let bot =
            serde_json::from_value(json!({"id":id,"name":id,"spaceId":space,"cwd":dir,"backend":"fixture"})).unwrap();
        hub.store.save_bot(&bot).unwrap();
        dispatch(&hub,&Caller::Local,"saveRoutine",json!({"spaceId":space,"botId":id,"name":id,"instruction":"fixture only","enabled":false,"triggers":[{"type":"webhook"}]})).await.unwrap();
    }
    let now = crate::store::now_ms();
    let overview =
        dispatch(&hub, &Caller::Local, "routinesOverview", json!({"spaceId":work,"from":now,"to":now+3_600_000}))
            .await
            .unwrap();
    assert!(!overview.to_string().contains("personal-bot"), "filter before occurrences/counts");
    let p = dispatch(
        &hub,
        &Caller::Local,
        "botGroupsEdit",
        json!({"spaceId":personal,"op":"add","name":"Pessoal privado"}),
    )
    .await
    .unwrap();
    let modern = dispatch(&hub, &Caller::Local, "botGroups", json!({"spaceId":work})).await.unwrap();
    assert_eq!(modern["groups"].as_array().unwrap().len(), 0);
    let err = dispatch(
        &hub,
        &Caller::Local,
        "botGroupsEdit",
        json!({"spaceId":work,"op":"place","botId":"work-bot","groupId":p["created"]}),
    )
    .await
    .unwrap_err();
    assert_eq!(crate::api::error_code(&err), Some("group_missing"));
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn tile_and_owner_assets_reject_active_svg_external_paths_and_unbounded_inputs() {
    use base64::Engine as _;
    let dir = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("donnabot-spaces-assets-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
        "fixture".into(),
        Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    let initial = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({})).await.unwrap();
    let svg =
        |text: &str| format!("data:image/svg+xml;base64,{}", base64::engine::general_purpose::STANDARD.encode(text));
    for value in [
        svg("<svg><script>alert(1)</script></svg>"),
        svg("<svg onload=\"alert(1)\"/>"),
        svg("<svg><foreignObject/></svg>"),
        svg("<!DOCTYPE svg [<!ENTITY x SYSTEM 'file:///private'>]><svg>&x;</svg>"),
        svg("<svg><image href=\"https://external.invalid/x\"/></svg>"),
        svg("<svg><path fill=\"url(https://external.invalid)\"/></svg>"),
        "https://external.invalid/logo.png".into(),
        "/private/logo.png".into(),
        "data:image/png;base64,AAAA".into(),
    ] {
        let request = json!({"expectedRevision":initial["revision"],"name":"Unsafe","tile":{"color":"oceano","style":"soft","symbol":{"kind":"logo","value":value}}});
        assert!(dispatch(&hub, &Caller::Local, "donnaSpaceCreate", request).await.is_err(), "unsafe logo accepted");
        assert_eq!(dispatch(&hub, &Caller::Local, "donnaSpaces", json!({})).await.unwrap(), initial);
    }
    assert!(dispatch(&hub,&Caller::Local,"donnaOwnerPresentation",json!({"expectedRevision":initial["revision"],"owner":{"name":"Fixture","initials":"FX","photo":"https://external.invalid/photo.png"}})).await.is_err());
    let safe = svg(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><path d=\"M0 0h10v10z\" fill=\"#ffffff\"/></svg>",
    );
    assert!(dispatch(&hub,&Caller::Local,"donnaSpaceCreate",json!({"expectedRevision":initial["revision"],"name":"Seguro","tile":{"color":"oceano","style":"soft","symbol":{"kind":"logo","value":safe}}})).await.is_ok());
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn legacy_migration_preserves_ids_sessions_read_marks_memory_and_bindings_transactionally() {
    use crate::store::{BotConfig, EntryKind, Lane};
    let dir = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("donnabot-spaces-migrate-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.db");
    let store = Store::open(&path).unwrap();
    let bot:BotConfig=serde_json::from_value(json!({"id":"legacy","name":"Legado","backend":"fixture","members":[],"skills":["fixture-skill"],"connectors":["fixture-connector"],"forkEditor":{"engineAccountId":"fixture-account"}})).unwrap();
    store.save_bot(&bot).unwrap();
    store.set_session("legacy", Some("legacy-acp-session")).unwrap();
    let entry = store
        .insert_entry(
            &Lane::main("legacy"),
            EntryKind::User,
            7,
            &json!({"text":"fixture historical text","attachments":[{"id":"fixture-file"}]}),
        )
        .unwrap();
    let read = store.mark_read("legacy").unwrap();
    store.kv_set("routines.v1", "fixture-routines-preserved").unwrap();
    store.kv_set("agent-env:fixture", "mock-vault-reference-only").unwrap();
    let memory = store
        .memory_write(|c, now, rev| {
            codync_mem::store::add(
                c,
                codync_mem::store::Writer::User,
                codync_mem::store::New {
                    bot: Some("legacy"),
                    text: "fixture memory preserved",
                    room: None,
                    evidence: &[],
                },
                now,
                rev,
            )
        })
        .unwrap();
    drop(store);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("DROP TABLE donna_space_conversations; DELETE FROM kv WHERE k IN ('donna.spaces.v1','donna.spaces.schema'); UPDATE bots SET config=json_remove(config,'$.spaceId');").unwrap();
    drop(db);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.bots().unwrap().len(), 1);
    let row = store.bot("legacy").unwrap().unwrap();
    assert_eq!(row.config.space_id, super::PERSONAL);
    assert_eq!(row.session_id.as_deref(), Some("legacy-acp-session"));
    assert_eq!(row.read_rev, read);
    assert_eq!(row.config.skills, bot.skills);
    assert_eq!(row.config.connectors, bot.connectors);
    assert_eq!(row.config.fork_editor, bot.fork_editor);
    assert_eq!(store.entry(&entry.id).unwrap().data, entry.data);
    assert_eq!(store.entry(&entry.id).unwrap().turn, 7);
    assert_eq!(store.kv_get("routines.v1").as_deref(), Some("fixture-routines-preserved"));
    assert_eq!(store.kv_get("agent-env:fixture").as_deref(), Some("mock-vault-reference-only"));
    assert_eq!(store.space_conversation(super::PERSONAL, "legacy").unwrap().read_revision, read);
    assert_eq!(
        store.memory_read(|c| codync_mem::store::get(c, memory.id)).unwrap().unwrap().text,
        "fixture memory preserved"
    );
    let state = serde_json::to_value(store.spaces().unwrap()).unwrap();
    drop(store);
    let again = Store::open(&path).unwrap();
    assert_eq!(serde_json::to_value(again.spaces().unwrap()).unwrap(), state);
    assert_eq!(again.bots().unwrap().len(), 1);
    assert_eq!(again.history("legacy", i64::MAX, 100).unwrap().len(), 1);
    drop(again);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn context_memory_and_uploads_never_fall_back_to_personal_or_another_conversation() {
    use base64::Engine as _;
    let dir = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("donnabot-spaces-resources-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(
        Store::open(&dir.join("fixture.db")).unwrap(),
        "fixture".into(),
        Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    let bot = serde_json::from_value(json!({"id":"donna","name":"Donna","backend":"claude","cwd":dir})).unwrap();
    hub.store.save_bot(&bot).unwrap();
    hub.store.kv_set("team.settings.v1", r#"{"primaryBotId":"donna","permission":"ask"}"#).unwrap();
    dispatch(
        &hub,
        &Caller::Local,
        "saveMemory",
        json!({"botId":"donna","text":"PERSONAL_MEMORY_SENTINEL","shared":true}),
    )
    .await
    .unwrap();
    let first = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({})).await.unwrap();
    let spaces=dispatch(&hub,&Caller::Local,"donnaSpaceCreate",json!({"expectedRevision":first["revision"],"name":"Trabalho","tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}})).await.unwrap();
    let work = spaces["spaces"][1]["id"].clone();
    let a = dispatch(&hub, &Caller::Local, "donnaConversationCreate", json!({"spaceId":work,"botId":"donna"}))
        .await
        .unwrap()["conversation"]
        .clone();
    let b = dispatch(&hub, &Caller::Local, "donnaConversationCreate", json!({"spaceId":work,"botId":"donna"}))
        .await
        .unwrap()["conversation"]
        .clone();
    let mut memory_events = hub.events.subscribe();
    let saved=dispatch(&hub,&Caller::Local,"donnaConversationMemory",json!({"spaceId":work,"conversationId":a["id"],"expectedRevision":a["revision"],"method":"saveMemory","text":"WORK_MEMORY_SENTINEL","shared":false})).await.expect("memory has an explicit context namespace");
    assert_eq!(saved["context"]["conversationId"], a["id"]);
    let event = memory_events.try_recv().unwrap();
    assert_eq!(event["type"], "conversation");
    assert_eq!(event["spaceId"], work);
    assert_eq!(event["conversation"]["revision"], a["revision"]);
    assert!(event["conversation"]["stateRevision"].as_i64().unwrap() > a["revision"].as_i64().unwrap());
    let memory = dispatch(
        &hub,
        &Caller::Local,
        "donnaConversationMemory",
        json!({"spaceId":work,"conversationId":a["id"],"method":"memory"}),
    )
    .await
    .unwrap();
    assert!(memory.to_string().contains("WORK_MEMORY_SENTINEL"));
    assert!(!memory.to_string().contains("PERSONAL_MEMORY_SENTINEL"));
    assert!(
        dispatch(&hub, &Caller::Local, "donnaConversationMemory", json!({"conversationId":a["id"],"method":"memory"}))
            .await
            .is_err()
    );
    let personal = dispatch(&hub, &Caller::Local, "memory", json!({"botId":"donna"})).await.unwrap();
    assert!(!personal.to_string().contains("WORK_MEMORY_SENTINEL"));
    let upload = uuid::Uuid::new_v4().to_string();
    let uploaded=dispatch(&hub,&Caller::Local,"donnaConversationUpload",json!({"spaceId":work,"conversationId":a["id"],"expectedRevision":a["revision"],"uploadId":upload,"name":"fixture.txt","data":base64::engine::general_purpose::STANDARD.encode("ONLY_A"),"offset":0,"done":true})).await.unwrap();
    assert_eq!(uploaded["attachment"]["id"], upload);
    assert!(
        dispatch(
            &hub,
            &Caller::Local,
            "donnaConversationReadUpload",
            json!({"spaceId":work,"conversationId":b["id"],"uploadId":upload,"offset":0})
        )
        .await
        .is_err()
    );
    let own = dispatch(
        &hub,
        &Caller::Local,
        "donnaConversationReadUpload",
        json!({"spaceId":work,"conversationId":a["id"],"uploadId":upload,"offset":0}),
    )
    .await
    .unwrap();
    assert_eq!(own["data"], base64::engine::general_purpose::STANDARD.encode("ONLY_A"));
    let source = dir.join("published-fixture.txt");
    std::fs::write(&source, "PUBLISHED_A_ONLY").unwrap();
    let mut snapshot = crate::chat::files::snapshot(
        &crate::chat::files::root(a["id"].as_str().unwrap()),
        &dir,
        "published-fixture.txt",
        None,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    let published = hub
        .store
        .insert_entry(
            &crate::store::Lane::main(a["id"].as_str().unwrap()),
            crate::store::EntryKind::Agent,
            1,
            &json!({"author":"donna","final":true,"files":[snapshot.meta],"text":"fixture"}),
        )
        .unwrap();
    snapshot.commit();
    let own = dispatch(
        &hub,
        &Caller::Local,
        "donnaConversationReadFile",
        json!({"spaceId":work,"conversationId":a["id"],"entryId":published.id,"fileId":snapshot.meta.id,"offset":0}),
    )
    .await
    .unwrap();
    assert_eq!(own["data"], base64::engine::general_purpose::STANDARD.encode("PUBLISHED_A_ONLY"));
    assert!(
        dispatch(
            &hub,
            &Caller::Local,
            "donnaConversationReadFile",
            json!({"spaceId":work,"conversationId":b["id"],"entryId":published.id,"fileId":snapshot.meta.id,"offset":0})
        )
        .await
        .is_err()
    );
    assert!(
        dispatch(
            &hub,
            &Caller::Local,
            "readFile",
            json!({"spaceId":work,"entryId":published.id,"fileId":snapshot.meta.id,"offset":0})
        )
        .await
        .is_err()
    );
    #[cfg(unix)]
    {
        let cfg = super::resource_config(&hub.store, &bot, a["id"].as_str().unwrap()).unwrap();
        let path = std::path::Path::new(&cfg.cwd).join("uploads").join(&upload).join("fixture.txt");
        let alias = dir.join("hardlink-fixture.txt");
        std::fs::hard_link(&path, &alias).unwrap();
        assert!(
            dispatch(
                &hub,
                &Caller::Local,
                "donnaConversationReadUpload",
                json!({"spaceId":work,"conversationId":a["id"],"uploadId":upload,"offset":0})
            )
            .await
            .is_err()
        );
        std::fs::remove_file(&alias).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&alias, "PERSONAL_FILE_SENTINEL").unwrap();
        std::os::unix::fs::symlink(&alias, &path).unwrap();
        assert!(
            dispatch(
                &hub,
                &Caller::Local,
                "donnaConversationReadUpload",
                json!({"spaceId":work,"conversationId":a["id"],"uploadId":upload,"offset":0})
            )
            .await
            .is_err()
        );
        let id = uuid::Uuid::new_v4().to_string();
        let link = path.parent().unwrap().parent().unwrap().join(&id);
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        assert!(dispatch(&hub,&Caller::Local,"donnaConversationUpload",json!({"spaceId":work,"conversationId":a["id"],"expectedRevision":a["revision"],"uploadId":id,"name":"refused.txt","data":"QQ==","offset":0,"done":true})).await.is_err());
        assert!(!dir.join("refused.txt").exists());
    }
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn mcp_rosters_and_management_never_discover_or_mutate_another_space() {
    let dir = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("donnabot-spaces-mcp-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    store.kv_set("team.settings.v1", r#"{"primaryBotId":"donna","permission":"ask"}"#).unwrap();
    let donna =
        serde_json::from_value(json!({"id":"donna","name":"Donna","backend":"fixture","cwd":dir,"manageTeam":true}))
            .unwrap();
    store.save_bot(&donna).unwrap();
    let hub = Hub::new(store, "fixture".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
    let first = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({})).await.unwrap();
    let spaces=dispatch(&hub,&Caller::Local,"donnaSpaceCreate",json!({"expectedRevision":first["revision"],"name":"Trabalho","tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}})).await.unwrap();
    let work = spaces["spaces"][1]["id"].clone();
    let private=serde_json::from_value(json!({"id":"private-work-bot","name":"Trabalho","description":"PRIVATE_WORK_PROMPT_SENTINEL","spaceId":work,"backend":"fixture","cwd":dir})).unwrap();
    hub.store.save_bot(&private).unwrap();
    let binding = crate::fork::sessoes::bindings::fixture_binding(&hub.store, "donna");
    hub.set_runtime("donna", |r| {
        r.session = Some("fixture-acp".into());
        r.lane = Some(crate::store::Lane::main("donna"));
        r.status = crate::hub::BotStatus::Working;
        r.memory_turn_token = Some("fixture-turn".into());
    });
    for (method, name) in [("manageCall", "list_team"), ("teamCall", "list_bots")] {
        let response = dispatch(
            &hub,
            &Caller::Local,
            method,
            json!({"botId":"donna","name":name,"arguments":{},"sessionBinding":binding.id.as_str()}),
        )
        .await
        .unwrap();
        assert!(!response.to_string().contains("PRIVATE_WORK_PROMPT_SENTINEL"));
    }
    assert!(
        dispatch(
            &hub,
            &Caller::Local,
            "manageCall",
            json!({"botId":"donna","name":"update_bot","arguments":{"botId":"private-work-bot","name":"Changed"}})
        )
        .await
        .is_err()
    );
    assert_eq!(hub.store.bot("private-work-bot").unwrap().unwrap().config.name, "Trabalho");
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

/// D395-01 (Sol): a conversa adicional do Berga carimba o idioma guardado do aparelho que mandou,
/// como o envio legado; um `appLanguage` no corpo é ignorado; o turno do Berga responde nele.
#[tokio::test]
async fn conversa_adicional_carimba_o_idioma_do_aparelho() {
    use crate::store::{Device, DeviceSource, Scope};
    let dir = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("donna-idioma-conversa-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let donna: crate::store::BotConfig =
        serde_json::from_value(json!({"id":"donna","name":"Donna","cwd":dir,"backend":"codex"})).unwrap();
    store.save_bot(&donna).unwrap();
    store.set_team_settings(&json!({"primaryBotId":"donna"})).unwrap();
    let account = format!("ac_{}", "1".repeat(64));
    store
        .kv_set(
            "fork.engine-accounts.v1",
            &json!({"salt":uuid::Uuid::new_v4().to_string(),"accounts":[{
                "id":account,"backend":"codex","billing":"paid_api","state":"connected","scope":"owner",
                "launch":{"kind":"cli_profile","executable":"/fixture/cli","directory":"/fixture/config"},
                "quotaIdentity":null,"verifiedAt":null,"subscriptionProof":null,"models":[]
            }]})
            .to_string(),
        )
        .unwrap();
    let chat = uuid::Uuid::new_v4().to_string();
    let (account_revision, account_stamp) = super::binding::account(&store, &account).unwrap();
    let rev = {
        let db = store.db.locked();
        let rev = crate::store::next_rev(&db).unwrap();
        db.execute("INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary,engine_account_id,account_revision,account_stamp) VALUES(?1,?2,'donna','Adicional',?3,0,?4,?5,?6)",
            rusqlite::params![chat, super::PERSONAL, rev, account, i64::try_from(account_revision).unwrap(), account_stamp]).unwrap();
        crate::fork::sessoes::ensure_principal(&db, &chat).unwrap();
        rev
    };
    let hub = Hub::new(store, "fixture".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
    let mut cmds = hub.test_bot_commands("donna");
    let device = Device {
        key: "fone".into(),
        name: "fone".into(),
        platform: "ios".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: crate::store::now_ms(),
        last_seen_at: None,
    };
    hub.store.put_device(&device).unwrap();
    let fone = crate::fork::autoridade::autenticar(&hub, "fone").unwrap().1;
    dispatch(&hub, &Caller::Local, "donnaAppLanguageSave", json!({"language":"pt-BR"})).await.unwrap();
    dispatch(&hub, &fone, "donnaAppLanguageSave", json!({"language":"en"})).await.unwrap();
    let sent = dispatch(
        &hub,
        &fone,
        "donnaConversationSend",
        json!({"spaceId":super::PERSONAL,"conversationId":chat,"expectedRevision":rev,
            "text":"hello","clientNonce":"n-en","appLanguage":"ar-AE"}),
    )
    .await
    .unwrap();
    let id = sent["entry"]["id"].as_str().unwrap().to_owned();
    match cmds.try_recv().expect("envio contextual chega só ao canal sintético") {
        crate::agent::bot::Cmd::Send { lane, entry_id, .. } => {
            assert_eq!(lane, crate::store::Lane::main(&chat));
            assert_eq!(entry_id, id);
        }
        _ => panic!("comando inesperado"),
    }
    assert!(cmds.try_recv().is_err());
    assert!(hub.store.history("donna", i64::MAX, 30).unwrap().is_empty());
    assert_eq!(hub.store.entry(&id).unwrap().data["appLanguage"], "en", "o corpo forjado é ignorado");
    let cfg = super::execution_config(&hub.store, &donna, &chat).unwrap();
    let linha = crate::fork::idioma_app::turno(&hub.store, &cfg, std::slice::from_ref(&id), false);
    assert!(linha.contains("English (en)"), "{linha}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `donnaConversationSend` num Hub com vínculo espera o `team_lock`; o papel da sessão muda antes do
/// efeito, e a Tx da mensagem confere a guarda: nada é gravado nem enviado ao motor.
#[tokio::test]
async fn envio_admitido_nao_grava_depois_que_o_papel_muda() {
    use crate::fork::sessao_clerk::{prova, vinculo::Papel};
    use crate::store::{Device, DeviceSource, Scope};
    let dir = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("donna-envio-sessao-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let donna: crate::store::BotConfig =
        serde_json::from_value(json!({"id":"donna","name":"Donna","cwd":dir,"backend":"codex"})).unwrap();
    store.save_bot(&donna).unwrap();
    store.set_team_settings(&json!({"primaryBotId":"donna"})).unwrap();
    let account = format!("ac_{}", "1".repeat(64));
    store
        .kv_set(
            "fork.engine-accounts.v1",
            &json!({"salt":uuid::Uuid::new_v4().to_string(),"accounts":[{
                "id":account,"backend":"codex","billing":"paid_api","state":"connected","scope":"owner",
                "launch":{"kind":"cli_profile","executable":"/fixture/cli","directory":"/fixture/config"},
                "quotaIdentity":null,"verifiedAt":null,"subscriptionProof":null,"models":[]
            }]})
            .to_string(),
        )
        .unwrap();
    let chat = uuid::Uuid::new_v4().to_string();
    let (account_revision, account_stamp) = super::binding::account(&store, &account).unwrap();
    let rev = {
        let db = store.db.locked();
        let rev = crate::store::next_rev(&db).unwrap();
        db.execute("INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary,engine_account_id,account_revision,account_stamp) VALUES(?1,?2,'donna','Adicional',?3,0,?4,?5,?6)",
            rusqlite::params![chat, super::PERSONAL, rev, account, i64::try_from(account_revision).unwrap(), account_stamp]).unwrap();
        crate::fork::sessoes::ensure_principal(&db, &chat).unwrap();
        rev
    };
    let hub = Hub::new(store, "fixture".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
    let mut cmds = hub.test_bot_commands("donna");
    prova::vincular(&hub, &prova::vinculo_pessoal());
    let entradas = |hub: &Hub| -> i64 {
        hub.store.db.locked().query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0)).unwrap()
    };
    let turno = |hub: &Hub| hub.store.kv_get(&format!("turn.seq.{chat}"));
    for (sub, trocar) in [(prova::DONO, true), (prova::OUTRO, false)] {
        let device = Device {
            key: format!("fone-{sub}"),
            name: "Telefone fictício".into(),
            platform: "ios".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: crate::store::now_ms(),
            last_seen_at: None,
        };
        let caller = prova::aparelho_com_sessao(&hub, &device, sub, Papel::Dono);
        let (antes, turno_antes) = (entradas(&hub), turno(&hub));
        let bloqueio = hub.store.team_lock.lock().await;
        let pedido = {
            let (hub, chat) = (hub.clone(), chat.clone());
            let b = json!({"spaceId":super::PERSONAL,"conversationId":chat,"expectedRevision":rev,
                "text":"olá","clientNonce":format!("n-{sub}")});
            tokio::spawn(async move { dispatch(&hub, &caller, "donnaConversationSend", b).await })
        };
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        if trocar {
            let iat = crate::store::now_ms() / 1000 + 1;
            prova::trocar_papel(&hub, sub, iat, Some(Papel::Admin));
        }
        drop(bloqueio);
        let resultado = pedido.await.unwrap();
        if trocar {
            assert!(format!("{:#}", resultado.unwrap_err()).contains("not_allowed"));
            assert_eq!(entradas(&hub), antes, "a mensagem não é gravada");
            assert_eq!(turno(&hub), turno_antes, "o turno é reservado na Tx da mensagem: recusada, nada é consumido");
            assert!(cmds.try_recv().is_err(), "nada chega ao motor");
        } else {
            assert!(resultado.unwrap()["entry"]["id"].is_string(), "sem troca, o mesmo caminho grava");
            assert!(entradas(&hub) > antes);
            assert!(matches!(cmds.try_recv(), Ok(crate::agent::bot::Cmd::Send { .. })));
        }
    }
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}
