//! Request cancelada não pode tomar o slot de uma assinatura ou request sucessora.
use super::*;

async fn mensagem(canal: &mut Channel, valor: Value) {
    canal.handle(&serde_json::to_vec(&valor).unwrap()).await.unwrap();
}

fn tarefa(canal: &Channel, id: u64) -> AbortHandle {
    match canal.jobs.locked().get(&id) {
        Some(Job::Request(handle, _) | Job::Sub(handle)) => handle.clone(),
        None => panic!("o slot real deve conter a tarefa"),
    }
}

async fn concluida(handle: &AbortHandle) {
    timeout(Duration::from_secs(60), async {
        while !handle.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("a tarefa deve concluir sem abortar o trabalho");
}

async fn recebida(rx: &mut mpsc::Receiver<ToDevice>) -> Value {
    let pacote = timeout(Duration::from_secs(60), rx.recv()).await.unwrap().unwrap();
    let ToDevice::Inner(valor) = pacote else { panic!("não deve fechar o canal nem enviar pacote externo") };
    valor
}

#[tokio::test]
async fn efeitos_rpc_reconferem_epoch_no_ponto_da_escrita() {
    use crate::store::BotConfig;
    for (method, phase) in [
        ("botGroupsEdit", "bot_groups_write"),
        ("donnaAppLanguageSave", "app_language_write"),
        ("registerDevice", "registration_write"),
        ("registerActivity", "registration_write"),
        ("sessions", "sessions_write"),
        ("upload", "upload_write"),
    ] {
        for changed in [false, true] {
            let hub = temp_hub();
            let bot = format!("fixture-{}", uuid::Uuid::new_v4());
            let cfg: BotConfig = serde_json::from_value(json!({"id":bot,"name":"Fixture"})).unwrap();
            hub.store.save_bot(&cfg).unwrap();
            hub.store.kv_set("bot_groups.migrated", "1").unwrap();
            let key = paired_phone(&hub).await;
            let dk = crypto::b64(key.verifying_key().as_bytes());
            let upload_id = uuid::Uuid::new_v4().to_string();
            let body = match method {
                "botGroupsEdit" => json!({"op":"add","name":"Fixture"}),
                "donnaAppLanguageSave" => json!({"language":"fr"}),
                "registerDevice" => json!({"ticket":"fixture-push"}),
                "registerActivity" => json!({"botId":bot,"ticket":"fixture-activity"}),
                "sessions" => {
                    hub.store.db.locked().execute("DELETE FROM donna_bot_sessions WHERE id=?1", [&bot]).unwrap();
                    json!({"botId":bot})
                }
                "upload" => json!({"botId":bot,"uploadId":upload_id,"name":"fixture.txt","data":"YWJj","done":true}),
                _ => unreachable!(),
            };
            let mut phone = Phone::direct(&hub, key);
            assert!(matches!(phone.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
            if changed {
                let bump_hub = hub.clone();
                let reached = crate::fork::sessoes::call_tests::interleave(&hub.store, phase, move || {
                    crate::fork::workspace::bump(&bump_hub, crate::fork::workspace::Motivo::RoleChanged).unwrap();
                });
                phone.send(json!({"id":1,"m":method,"b":body})).await;
                assert_eq!(phone.recv().await.unwrap_err().0, 4003, "{method}");
                timeout(Duration::from_secs(5), async {
                    while !reached.load(std::sync::atomic::Ordering::Acquire) {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
            } else {
                let response = phone.call(1, method, body).await;
                assert!(response["ok"].is_object(), "{method}: {response}");
            }
            let root = crate::chat::uploads::root(&cfg);
            let had_effect = match method {
                "botGroupsEdit" => {
                    !hub.store.bot_groups_in_space(crate::fork::spaces::PERSONAL).unwrap().groups.is_empty()
                }
                "donnaAppLanguageSave" => crate::fork::idioma_app::do_aparelho(&hub.store, &dk) == Some("fr"),
                "registerDevice" => !hub.store.push_tickets().is_empty(),
                "registerActivity" => !hub.store.activity_tickets(&bot).is_empty(),
                "sessions" => hub
                    .store
                    .db
                    .locked()
                    .query_row("SELECT EXISTS(SELECT 1 FROM donna_bot_sessions WHERE id=?1)", [&bot], |r| {
                        r.get::<_, bool>(0)
                    })
                    .unwrap(),
                "upload" => crate::chat::uploads::resolve(&root, &upload_id).is_ok(),
                _ => unreachable!(),
            };
            assert_eq!(had_effect, !changed, "{method}");
            if method == "upload" && had_effect {
                std::fs::remove_dir_all(root.join(upload_id)).unwrap();
            }
        }
    }
}

#[tokio::test]
async fn request_legada_admitida_antes_do_bump_nao_grava_na_transacao() {
    let hub = temp_hub();
    let phone = paired_phone(&hub).await;
    let key = crypto::b64(phone.verifying_key().as_bytes());
    let (_, caller) = autoridade::autenticar(&hub, &key).unwrap();
    let admissao = barreira_workspace::Admissao::do_caller(&hub, &caller).unwrap();
    let (tx, mut rx) = mpsc::channel(1);
    let (prazo, _) = tokio::sync::watch::channel(None);
    let mut canal = Channel {
        hub: hub.clone(),
        tx,
        jobs: Arc::default(),
        request_slots: Arc::new(tokio::sync::Semaphore::new(MAX_REQUESTS)),
        key,
        caller: Arc::new(caller),
        admissao: Some(admissao),
        lease: None,
        online: None,
        quadros: false,
        browser: None,
        sessao: None,
        prazo,
        th: [0; 32],
        epoch: admissao.epoch,
        nonces: crate::fork::sessao_clerk::posse::Nonces::default(),
    };
    let bump_hub = hub.clone();
    let chegou = crate::fork::sessoes::call_tests::interleave(&hub.store, "routine_commit", move || {
        use crate::fork::workspace::{self, Motivo};
        workspace::bump(&bump_hub, Motivo::RoleChanged).unwrap();
    });
    mensagem(&mut canal, json!({"id":1,"m":"setRoutinesPaused","b":{"paused":true}})).await;
    concluida(&tarefa(&canal, 1)).await;
    assert!(chegou.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(hub.routines.pause_state()["paused"], false);
    assert!(matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
}

#[tokio::test]
async fn send_legado_no_epoch_atual_entra_na_fila_e_passa_pelo_inicio_do_turno() {
    use crate::agent::bot::Cmd;
    let hub = temp_hub();
    let bot = "bot-legado";
    hub.store.save_bot(&serde_json::from_value(json!({"id":bot,"name":"Bot"})).unwrap()).unwrap();
    let mut commands = hub.test_bot_commands(bot);
    let key = paired_phone(&hub).await;
    let mut phone = Phone::direct(&hub, key);
    assert!(matches!(phone.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
    let response = phone.call(1, "send", json!({"botId":bot,"text":"Mensagem","clientNonce":"legado"})).await;
    assert_eq!(response["ok"]["entry"]["data"]["status"], "queued");
    let Some(Cmd::Send { entry_id, .. }) = timeout(Duration::from_secs(2), commands.recv()).await.unwrap() else {
        panic!("send deve entregar o comando ao ator");
    };
    assert!(hub.admit_remote_turn(&entry_id).is_ok());
    crate::fork::workspace::bump(&hub, crate::fork::workspace::Motivo::RoleChanged).unwrap();
    assert!(hub.admit_remote_turn(&entry_id).is_err());
    assert_eq!(hub.store.entry(&entry_id).unwrap().data["status"], "failed");
    assert!(hub.store.queued_user_entries_for_identity(bot).unwrap().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn send_legado_atual_chega_ao_prompt_do_ator() {
    let hub = temp_hub();
    std::fs::create_dir_all(crate::service::data_dir()).unwrap();
    let cwd = tempfile::tempdir().unwrap();
    std::fs::write(cwd.path().join("blocked"), "").unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/subscription_agent.py");
    let cfg = serde_json::from_value(json!({"id":"bot-fio","name":"Bot","backend":"codex",
        "cwd":cwd.path(),"command":crate::shell::python(&fixture),"notify":false}))
    .unwrap();
    hub.store.save_bot(&cfg).unwrap();
    hub.start_created_bot(&cfg);
    let key = paired_phone(&hub).await;
    let mut phone = Phone::direct(&hub, key);
    assert!(matches!(phone.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
    let response =
        phone.call(1, "send", json!({"botId":"bot-fio","text":"marcador-legado","clientNonce":"turno"})).await;
    assert!(response["ok"]["entry"].is_object(), "{response}");
    let prompts = cwd.path().join("prompts.jsonl");
    let result = timeout(Duration::from_secs(20), async {
        while !std::fs::read_to_string(&prompts).is_ok_and(|text| text.contains("marcador-legado")) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    hub.shutdown().await;
    assert!(
        result.is_ok(),
        "o ator deve entregar o prompt ao motor de teste; estado da entrada: {:?}",
        hub.store.find_by_nonce("bot-fio", "turno").map(|entry| entry.data["status"].clone())
    );
}

#[tokio::test]
async fn send_legado_cortado_depois_do_commit_preserva_mensagem_sem_marcar_leitura_ou_iniciar_turno() {
    use crate::store::{BotConfig, EntryKind, Lane};
    let hub = temp_hub();
    let bot = "bot-corte";
    let cfg: BotConfig = serde_json::from_value(json!({"id":bot,"name":"Bot"})).unwrap();
    hub.store.save_bot(&cfg).unwrap();
    let mut comandos = hub.test_bot_commands(bot);
    hub.store.insert_entry(&Lane::main(bot), EntryKind::Agent, 0, &json!({"final":true})).unwrap();
    let read_rev = hub.store.bot(bot).unwrap().unwrap().read_rev;
    assert_eq!(hub.store.unread(bot, read_rev), 1);
    let key = paired_phone(&hub).await;
    let mut phone = Phone::direct(&hub, key);
    assert!(matches!(phone.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
    let bump_hub = hub.clone();
    let chegou = crate::fork::sessoes::call_tests::interleave(&hub.store, "send_after_commit", move || {
        use crate::fork::workspace::{self, Motivo};
        workspace::bump(&bump_hub, Motivo::RoleChanged).unwrap();
    });
    phone.send(json!({"id":1,"m":"send","b":{"botId":bot,"text":"Mensagem","clientNonce":"send-corte"}})).await;
    assert_eq!(phone.recv().await.unwrap_err().0, 4003);
    timeout(Duration::from_secs(2), async {
        while hub.store.find_by_nonce(bot, "send-corte").is_none_or(|entry| entry.data["status"] == "queued") {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(chegou.load(std::sync::atomic::Ordering::Acquire));
    let entry = hub.store.find_by_nonce(bot, "send-corte").unwrap();
    assert_eq!(entry.data["status"], "failed");
    assert_eq!(hub.store.bot(bot).unwrap().unwrap().read_rev, read_rev);
    assert!(hub.store.queued_user_entries_for_identity(bot).unwrap().is_empty());
    assert!(matches!(comandos.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
}

#[tokio::test]
async fn request_cancelada_conclui_sem_tomar_sub_ou_request_do_mesmo_id() {
    let hub = temp_hub();
    let key = paired_phone(&hub).await;
    let chave = crypto::b64(key.verifying_key().as_bytes());
    let (_, caller) = autoridade::autenticar(&hub, &chave).unwrap();
    let admissao = barreira_workspace::Admissao::do_caller(&hub, &caller).unwrap();
    let (tx, mut rx) = mpsc::channel(1);
    // O mesmo handle/dispatch e a fila de saída usados após abrir o frame do canal.
    // A identidade vem da autenticação real, não de um Caller fabricado pelo teste.
    let mut canal = Channel {
        hub: hub.clone(),
        tx,
        jobs: Arc::default(),
        request_slots: Arc::new(tokio::sync::Semaphore::new(MAX_REQUESTS)),
        key: chave,
        caller: Arc::new(caller),
        admissao: Some(admissao),
        lease: None,
        online: None,
        quadros: false,
        browser: None,
        sessao: None,
        prazo: tokio::sync::watch::channel(None).0,
        th: [0; 32],
        epoch: hub.workspace.atual(),
        nonces: crate::fork::sessao_clerk::posse::Nonces::default(),
    };
    let bloqueio = hub.store.team_lock.lock().await;
    mensagem(&mut canal, json!({"id":7,"m":"setRoutinesPaused","b":{"paused":true}})).await;
    let request_a = tarefa(&canal, 7);
    assert!(!request_a.is_finished());
    mensagem(&mut canal, json!({"id":7,"cancel":true})).await;
    assert!(!request_a.is_finished(), "cancelar não aborta a request bloqueada");
    mensagem(&mut canal, json!({"id":7,"sub":"events","b":{"since":hub.store.current_rev(),"client":"ios"}})).await;
    let sub_b = tarefa(&canal, 7);
    assert_ne!(request_a.id(), sub_b.id());
    let hello = recebida(&mut rx).await;
    assert_eq!((hello["id"].as_u64(), hello["ev"]["type"].as_str()), (Some(7), Some("hello")));
    assert!(hub.phone_connected());
    drop(bloqueio);
    concluida(&request_a).await;
    assert_eq!(hub.routines.pause_state()["paused"], true, "o trabalho cancelado deve concluir");
    assert!(matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)), "sem resposta tardia de A");
    assert_eq!(tarefa(&canal, 7).id(), sub_b.id());
    assert!(!sub_b.is_finished(), "B não pode desaparecer sem end/fechamento");
    hub.events.send(json!({"type":"resync","marcador":"evento-legitimo-de-B"})).unwrap();
    let evento = recebida(&mut rx).await;
    assert_eq!((evento["id"].as_u64(), evento["ev"]["marcador"].as_str()), (Some(7), Some("evento-legitimo-de-B")));
    mensagem(&mut canal, json!({"id":7,"cancel":true})).await;
    assert_eq!(recebida(&mut rx).await, json!({"id":7,"end":true}));
    concluida(&sub_b).await;
    assert!(!hub.phone_connected());

    // Mesma variante também exige a identidade: a fila cheia conserva a vaga em espera.
    assert!(canal.tx.try_send(ToDevice::Inner(json!({"sentinela":true}))).is_ok());
    mensagem(&mut canal, json!({"id":7,"m":"routinesPause","b":{}})).await;
    let request_c = tarefa(&canal, 7);
    tokio::task::yield_now().await;
    assert!(!request_c.is_finished(), "a resposta aguarda vaga real na fila");
    mensagem(&mut canal, json!({"id":7,"cancel":true})).await;
    mensagem(&mut canal, json!({"id":7,"m":"setRoutinesPaused","b":{"paused":false}})).await;
    let request_d = tarefa(&canal, 7);
    assert_ne!(request_c.id(), request_d.id());
    assert_eq!(recebida(&mut rx).await, json!({"sentinela":true}));
    assert_eq!(recebida(&mut rx).await, json!({"id":7,"ok":{"paused":false}}));
    concluida(&request_c).await;
    concluida(&request_d).await;
    assert!(!canal.jobs.locked().contains_key(&7));
    assert!(matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)), "C não responde por D");
}
