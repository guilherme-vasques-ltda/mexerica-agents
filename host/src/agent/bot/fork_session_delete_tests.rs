//! D545: transporte, ator, transação e replay, só com fixtures em memória.
use super::*;
use crate::fork::sessoes::excluir::{self, Request};
use crate::fork::spaces::PERSONAL;

fn request(hub: &Hub, id: &str) -> Request {
    Request {
        hub_id: hub.host_id.clone(),
        space_id: PERSONAL.into(),
        bot_id: "b".into(),
        session_id: id.into(),
        expected_revision: revision(hub, id),
    }
}
fn body(r: &Request) -> Value {
    json!({"hubId":r.hub_id,"spaceId":r.space_id,"botId":r.bot_id,"sessionId":r.session_id,"expectedRevision":r.expected_revision})
}
async fn rpc(actor: &mut Actor, caller: &crate::api::devices::Caller, r: &Request) -> Result<Value> {
    let hub = actor.hub.clone();
    let mut commands = hub.test_bot_commands("b");
    let (done, _rx) = mpsc::unbounded_channel();
    let b = body(r);
    let request = crate::api::dispatch(&hub, caller, "sessionDelete", b);
    tokio::pin!(request);
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::select! {
            result = &mut request => result,
            command = commands.recv() => {
                actor.on_cmd(command.unwrap(), &done).await;
                request.await
            }
        }
    })
    .await
    .expect("o dispatcher e o ator devem responder sem espera")
}

#[tokio::test]
async fn excluir_extra_apaga_mensagens_atomicamente_preserva_memoria_rotinas_e_repete() {
    let (hub, dir) = fixture();
    let (id, sister) = (extra(&hub), extra(&hub));
    let own =
        hub.store.insert_entry(&lane(&id), EntryKind::User, 1, &json!({"text":"Apagar","status":"sent"})).unwrap();
    let trace = hub
        .store
        .insert_entry(&lane(&id), EntryKind::Tool, 1, &json!({"title":"Leitura","status":"completed"}))
        .unwrap();
    let other = hub
        .store
        .insert_entry(&lane(&sister), EntryKind::User, 2, &json!({"text":"Preservar","status":"sent"}))
        .unwrap();
    let main = hub
        .store
        .insert_entry(&Lane::main("b"), EntryKind::User, 3, &json!({"text":"Principal","status":"sent"}))
        .unwrap();
    let memory = hub
        .store
        .memory_write(|c, now, rev| {
            codync_mem::store::add(
                c,
                codync_mem::store::Writer::User,
                codync_mem::store::New { bot: Some("b"), text: "Memória preservada", room: None, evidence: &[] },
                now,
                rev,
            )
        })
        .unwrap();
    hub.routines.restore(&hub.store).unwrap();
    hub.routines
        .save(&hub, "b", &json!({"name":"Outra rotina","instruction":"Preservar","triggers":[{"type":"webhook"}]}))
        .unwrap();
    let routines = hub.store.kv_get("routines.v1");
    let memory_rev = hub.store.memory_rev();
    hub.store
        .kv_set(
            "turn.waiting.b",
            &json!({"phase":"finished","input":{"visibleSession":id,"text":"Apagar checkpoint"}}).to_string(),
        )
        .unwrap();
    let r = request(&hub, &id);
    let mut actor = actor_of(&hub);
    actor.lane = Lane::main("b");
    actor.turn = Some(42);
    let result = rpc(&mut actor, &crate::api::devices::Caller::Local, &r).await.unwrap();
    assert_eq!(result["sessionId"], id);
    assert_eq!(actor.turn, Some(42), "o turno de outra sessão continua");
    assert!(sessions::get(&hub.store.db.locked(), &id).unwrap().is_none());
    assert!(hub.store.entry(&own.id).is_none() && hub.store.entry(&trace.id).is_none());
    assert!(hub.store.entry(&other.id).is_some() && hub.store.entry(&main.id).is_some());
    assert_eq!(hub.store.memory_rev(), memory_rev);
    assert!(hub.store.kv_get("turn.waiting.b").is_none());
    let text = hub.store.memory_read(|c| codync_mem::store::get(c, memory.id)).unwrap().unwrap().text;
    assert_eq!(text, "Memória preservada");
    assert_eq!(hub.store.kv_get("routines.v1"), routines);
    assert_eq!(rpc(&mut actor, &crate::api::devices::Caller::Local, &r).await.unwrap(), result);
    let (d, _) = device(&hub);
    let (_, caller) = crate::fork::autoridade::autenticar(&hub, &d.key).unwrap();
    assert_eq!(rpc(&mut actor, &caller, &r).await.unwrap(), result);
    // Um produtor atrasado não pode recriar mensagens da sessão apagada.
    assert!(hub.store.insert_entry(&lane(&id), EntryKind::User, 4, &json!({"text":"atrasada"})).is_err());
    // Mesmo um cliente cujo cursor já passou da exclusão precisa limpar cache antigo.
    let db = hub.store.db.locked();
    let replay = excluir::catch_up(&db, PERSONAL, i64::MAX).unwrap();
    assert_eq!(replay, vec![result.clone()]);
    assert_eq!(excluir::catch_up(&db, "outro-space", 0).unwrap(), Vec::<Value>::new());
    drop(db);
    assert!(crate::fork::spaces::filter_event(&hub, "outro-space", result).is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn excluir_extra_recusa_space_bot_id_principal_revisao_e_nao_altera_dados() {
    let (hub, dir) = fixture();
    let id = extra(&hub);
    let r = request(&hub, &id);
    let original =
        hub.store.insert_entry(&lane(&id), EntryKind::User, 1, &json!({"text":"Preservar","status":"sent"})).unwrap();
    let mut actor = actor_of(&hub);
    let mut cases = Vec::new();
    let mut bad = r.clone();
    bad.space_id = "outro-space".into();
    cases.push(bad);
    let mut bad = r.clone();
    bad.bot_id = "c".into();
    cases.push(bad);
    let mut bad = r.clone();
    bad.session_id = "id-trocado".into();
    cases.push(bad);
    let mut bad = r.clone();
    bad.session_id = "b".into();
    cases.push(bad);
    let mut bad = r.clone();
    bad.expected_revision += 1;
    cases.push(bad);
    let mut bad = r.clone();
    bad.hub_id = "outro-hub".into();
    cases.push(bad);
    for bad in cases {
        assert!(actor.session_delete(&bad, &Autoridade::local()).is_err());
        assert!(sessions::get(&hub.store.db.locked(), &id).unwrap().is_some());
        assert!(hub.store.entry(&original.id).is_some());
    }
    let mut other_actor = new_actor(hub.clone(), hub.store.bot("c").unwrap().unwrap().config);
    let mut cross_bot = r.clone();
    cross_bot.bot_id = "c".into();
    assert!(other_actor.session_delete(&cross_bot, &Autoridade::local()).is_err());
    let spaces = hub.store.spaces().unwrap();
    let created = crate::api::dispatch(
        &hub,
        &crate::api::devices::Caller::Local,
        "donnaSpaceCreate",
        json!({"hubId":hub.host_id,"expectedRevision":spaces.revision,"name":"Outro Space",
        "tile":{"color":"oceano","style":"filled","symbol":{"kind":"icon","value":"code"}}}),
    )
    .await
    .unwrap();
    let mut cross_space = body(&r);
    cross_space["spaceId"] = created["spaces"].as_array().unwrap().last().unwrap()["id"].clone();
    let denied = tokio::time::timeout(
        Duration::from_secs(1),
        crate::api::dispatch(&hub, &crate::api::devices::Caller::Local, "sessionDelete", cross_space),
    )
    .await
    .unwrap();
    assert!(denied.is_err(), "Space real diferente deve recusar no dispatcher");
    assert!(hub.store.entry(&original.id).is_some());
    // Não confundir outro bot/Space com repetição válida depois da exclusão.
    actor.session_delete(&r, &Autoridade::local()).unwrap();
    let mut bad = r.clone();
    bad.bot_id = "c".into();
    assert!(actor.session_delete(&bad, &Autoridade::local()).is_err());
    let mut bad = r.clone();
    bad.space_id = "outro-space".into();
    assert!(actor.session_delete(&bad, &Autoridade::local()).is_err());
    let mut bad = r.clone();
    bad.expected_revision += 1;
    assert!(actor.session_delete(&bad, &Autoridade::local()).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn excluir_extra_recusa_execucao_fila_e_checkpoint_sem_parar_ou_apagar() {
    let (hub, dir) = fixture();
    let id = extra(&hub);
    let r = request(&hub, &id);
    let mut actor = actor_of(&hub);
    actor.lane = lane(&id);
    actor.turn = Some(1);
    let error = rpc(&mut actor, &crate::api::devices::Caller::Local, &r).await.unwrap_err();
    assert!(format!("{error:#}").contains("session_busy"));
    assert_eq!(actor.turn, Some(1));
    actor.turn = None;
    let queued_id = queued(&mut actor, &lane(&id), "Pendente");
    assert!(actor.session_delete(&r, &Autoridade::local()).is_err());
    assert!(hub.store.entry(&queued_id).is_some());
    // A fila restaurável também precisa impedir a exclusão (o durável confere).
    actor.queue.clear();
    assert!(actor.session_delete(&r, &Autoridade::local()).is_err());
    hub.store
        .db
        .locked()
        .execute("UPDATE entries SET data=json_set(data,'$.status','sent') WHERE id=?1", [&queued_id])
        .unwrap();
    hub.store.kv_set("turn.waiting.b", &json!({"phase":"waiting","input":{"visibleSession":id}}).to_string()).unwrap();
    assert!(actor.session_delete(&r, &Autoridade::local()).is_err());
    assert!(sessions::get(&hub.store.db.locked(), &id).unwrap().is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn excluir_extra_reconfere_autoridade_original_e_nao_executa_pedido_expirado() {
    let (hub, dir) = fixture();
    let id = extra(&hub);
    let r = request(&hub, &id);
    let mut actor = actor_of(&hub);
    let (d, authority) = device(&hub);
    revoke(&hub, &d);
    assert!(actor.session_delete(&r, &authority).is_err());
    let (_, authority) = device(&hub);
    // Revogação no gancho imediatamente antes da Tx da escrita.
    let h = hub.clone();
    let d2 = d.clone();
    *hub.store.before_owner_write.locked() = Some(Box::new(move || {
        revoke(&h, &d2);
    }));
    assert!(actor.session_delete(&r, &authority).is_err());
    let (reply, answer) = tokio::sync::oneshot::channel();
    drop(answer);
    let (done, _rx) = mpsc::unbounded_channel();
    actor
        .on_cmd(Cmd::SessionDelete { request: Box::new(r), autoridade: Box::new(Autoridade::local()), reply }, &done)
        .await;
    assert!(sessions::get(&hub.store.db.locked(), &id).unwrap().is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn excluir_extra_rollback_inclui_catalogo_mensagens_e_tombstone() {
    let (hub, dir) = fixture();
    let id = extra(&hub);
    let r = request(&hub, &id);
    let own =
        hub.store.insert_entry(&lane(&id), EntryKind::User, 1, &json!({"text":"Preservar","status":"sent"})).unwrap();
    hub.store.db.locked().execute_batch("CREATE TEMP TRIGGER reject_session_delete BEFORE DELETE ON donna_bot_sessions BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let mut actor = actor_of(&hub);
    assert!(actor.session_delete(&r, &Autoridade::local()).is_err());
    assert!(hub.store.entry(&own.id).is_some());
    assert!(sessions::get(&hub.store.db.locked(), &id).unwrap().is_some());
    assert_eq!(excluir::catch_up(&hub.store.db.locked(), PERSONAL, 0).unwrap(), Vec::<Value>::new());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn excluir_extra_recusa_bot_e_aparelho_sem_autoridade_no_dispatch() {
    let (hub, dir) = fixture();
    let id = extra(&hub);
    let r = request(&hub, &id);
    let binding = bindings::issue(&hub.store, "b", "b", "b", Some("acp-ponte")).unwrap();
    bindings::activate(&hub.store, &binding, "acp-ponte").unwrap();
    let mut servers = vec![json!({"env":[],"args":["mcp","remote","--context","original"]})];
    crate::fork::ponte_credencial::provisionar(&hub, "b", None, &binding, &mut servers).unwrap();
    let token = zeroize::Zeroizing::new(servers[0]["env"][0]["value"].as_str().unwrap().to_owned());
    let bot = hub.pontes.autenticar(&hub, &token).unwrap();
    assert!(crate::api::dispatch(&hub, &bot, "sessionDelete", body(&r)).await.is_err());
    assert!(
        crate::api::dispatch(
            &hub,
            &crate::api::devices::Caller::Device {
                key: "sem-origem".into(),
                scopes: vec![Scope::Control],
                autoridade: None
            },
            "sessionDelete",
            body(&r)
        )
        .await
        .is_err()
    );
    assert!(sessions::get(&hub.store.db.locked(), &id).unwrap().is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn excluir_extra_preserva_checkpoint_finalizado_da_irma() {
    let (hub, dir) = fixture();
    let id = extra(&hub);
    let sister = extra(&hub);
    let checkpoint = json!({"phase":"finished","input":{"visibleSession":sister,"text":"Preservar irmã"}}).to_string();
    hub.store.kv_set("turn.waiting.b", &checkpoint).unwrap();
    let mut actor = actor_of(&hub);
    actor.session_delete(&request(&hub, &id), &Autoridade::local()).unwrap();
    assert_eq!(hub.store.kv_get("turn.waiting.b"), Some(checkpoint));
    assert!(sessions::get(&hub.store.db.locked(), &sister).unwrap().is_some());
    let _ = std::fs::remove_dir_all(dir);
}
