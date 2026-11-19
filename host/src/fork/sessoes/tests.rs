//! Fronteiras do PR 2 do plano de sessões (D70): catálogo, validação, migração, marcador e exposição.
use super::origin::{self, SessionOrigin};
use super::runs_on::{self, Place, RunsOn};
use super::store::{self, NewSession};
use super::{exposure, marker};
use crate::LockExt;
use crate::store::{BotConfig, EntryKind, Lane, Store};
use serde_json::{Value, json};
use std::path::Path;

fn bot(store: &Store, id: &str) {
    let cfg: BotConfig =
        serde_json::from_value(json!({"id": id, "name": id, "backend": "claude", "cwd": "/tmp"})).unwrap();
    store.save_bot(&cfg).unwrap();
}

fn extra(store: &Store, chat: &str, title: &str) -> String {
    let c = store.db.locked();
    let new = NewSession {
        chat: chat.into(),
        origin: SessionOrigin::Owner,
        source_session: None,
        parent_session: None,
        title: title.into(),
        runs_on: None,
    };
    store::create(&c, &new).unwrap().id
}

#[test]
fn origin_aceita_todos_os_valores_e_recusa_o_resto_na_gravacao_e_na_leitura() {
    let all = [
        json!({"kind":"owner"}),
        json!({"kind":"continuation"}),
        json!({"kind":"bot","botId":"b2","fromSession":"s1","fromExecution":"x1"}),
        json!({"kind":"bot","botId":"b2"}),
        json!({"kind":"routine","routineId":"r1","runId":"run1"}),
        json!({"kind":"notice"}),
        json!({"kind":"group"}),
        json!({"kind":"subsession"}),
        json!({"kind":"suggestion","entryIds":["e1"]}),
        json!({"kind":"routed","rule":"assunto"}),
    ];
    let store = Store::open(Path::new(":memory:")).unwrap();
    bot(&store, "b");
    for value in all {
        let parsed = origin::parse(&value.to_string()).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&parsed.column().unwrap()).unwrap(), value);
        // Times e o 2b gravam pelo produtor único, sem migrar.
        let c = store.db.locked();
        let new = NewSession {
            chat: "b".into(),
            origin: parsed.clone(),
            source_session: None,
            parent_session: None,
            title: String::new(),
            runs_on: None,
        };
        let id = store::create(&c, &new).unwrap().id;
        assert_eq!(store::get(&c, &id).unwrap().unwrap().origin, parsed);
    }
    for bad in [
        "não é json",
        r#"{"kind":"owner","extra":1}"#,
        r#"{"kind":"desconhecido"}"#,
        r#"{"kind":"bot"}"#,
        r#"{"kind":"bot","botId":""}"#,
        r#"{"kind":"bot","botId":"b2","fromSession":null}"#,
        r#"{"kind":"suggestion","entryIds":[]}"#,
    ] {
        assert!(origin::parse(bad).is_err(), "{bad}");
    }
    assert!(SessionOrigin::Bot { bot_id: " ".into(), from_session: None, from_execution: None }.column().is_err());
    // A leitura também falha: nada vira `owner`.
    let id = extra(&store, "b", "Plano");
    let c = store.db.locked();
    c.execute("UPDATE donna_bot_sessions SET origin=?2 WHERE id=?1", [id.as_str(), r#"{"kind":"owner","x":1}"#])
        .unwrap();
    assert!(store::get(&c, &id).is_err());
    assert!(store::list(&c, "b").is_err());
}

#[test]
fn runs_on_nulo_e_local_e_o_invalido_falha_na_gravacao_e_na_leitura() {
    assert_eq!(runs_on::parse(None).unwrap(), None);
    let cloud = RunsOn { kind: Place::Cloud, id: "lugar-1".into() };
    let column = runs_on::column(Some(&cloud)).unwrap().unwrap();
    assert_eq!(runs_on::parse(Some(&column)).unwrap(), Some(cloud));
    for bad in [
        r#"{"kind":"computer","id":"c1","extra":true}"#,
        "{",
        r#"{"kind":"marte","id":"c1"}"#,
        r#"{"kind":"computer","id":""}"#,
        "null",
    ] {
        assert!(runs_on::parse(Some(bad)).is_err(), "{bad}");
    }
    assert!(runs_on::column(Some(&RunsOn { kind: Place::Computer, id: " ".into() })).is_err());
    let store = Store::open(Path::new(":memory:")).unwrap();
    bot(&store, "b");
    let id = extra(&store, "b", "");
    let c = store.db.locked();
    assert_eq!(store::get(&c, &id).unwrap().unwrap().runs_on, None);
    c.execute("UPDATE donna_bot_sessions SET runs_on=?2 WHERE id=?1", [id.as_str(), r#"{"kind":"marte","id":"x"}"#])
        .unwrap();
    assert!(store::get(&c, &id).is_err(), "nunca vira local");
}

#[test]
fn extras_so_no_chat_proprio_do_pessoal_e_a_principal_nao_muda_de_nome() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    bot(&store, "b");
    let group: BotConfig =
        serde_json::from_value(json!({"id":"g","name":"G","kind":"group","members":["b"],"cwd":"/tmp"})).unwrap();
    store.save_bot(&group).unwrap();
    let c = store.db.locked();
    let new = |chat: &str| NewSession {
        chat: chat.into(),
        origin: SessionOrigin::Owner,
        source_session: None,
        parent_session: None,
        title: "x".into(),
        runs_on: None,
    };
    assert!(store::create(&c, &new("g")).is_err(), "grupo com sessões é do 2b");
    assert!(store::create(&c, &new("nenhum")).is_err());
    let mut child = new("b");
    child.parent_session = Some("b".into());
    let id = store::create(&c, &child).unwrap().id;
    assert_eq!(store::get(&c, &id).unwrap().unwrap().depth, 1);
    let mut stranger = new("b");
    stranger.source_session = Some("g".into());
    assert!(store::create(&c, &stranger).is_err(), "origem de outra conversa");
    let sessions = store::list(&c, "b").unwrap();
    assert!(sessions[0].principal && sessions[0].id == "b", "a principal vem primeiro");
    assert!(store::rename(&c, "b", "b", "Outro", sessions[0].revision).is_err());
    let session = store::get(&c, &id).unwrap().unwrap();
    assert!(store::rename(&c, "b", &id, "Novo", session.revision - 1).is_err(), "CAS");
    assert!(store::rename(&c, "g", &id, "Novo", session.revision).is_err(), "sessão de outro bot");
    let renamed = store::rename(&c, "b", &id, "  Plano   da viagem ", session.revision).unwrap();
    assert_eq!((renamed.title.as_str(), renamed.title_source.as_str()), ("Plano da viagem", "owner"));
    assert!(store::rename(&c, "b", &id, &"x".repeat(61), renamed.revision).is_err());
}

#[test]
fn migracao_de_um_banco_da_main_cria_as_principais_sem_mudar_nada_e_e_idempotente() {
    let dir = crate::store::private_fixture_dir("donnabot-sessoes");
    let path = dir.join("sessions.db");
    let snapshot = |c: &rusqlite::Connection| -> Vec<String> {
        let mut rows: Vec<String> = c
            .prepare("SELECT id||'|'||rev||'|'||seq||'|'||data FROM entries ORDER BY seq")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let kv = "SELECT k||'='||v FROM kv WHERE k LIKE 'compaction.%' OR k LIKE 'handoff.%' OR k LIKE 'lane.session.%'
                  UNION ALL SELECT id||':'||COALESCE(session_id,'') FROM bots ORDER BY 1";
        rows.extend(c.prepare(kv).unwrap().query_map([], |r| r.get::<_, String>(0)).unwrap().map(Result::unwrap));
        rows
    };
    let before = {
        let store = Store::open(&path).unwrap();
        bot(&store, "b");
        let group: BotConfig =
            serde_json::from_value(json!({"id":"g","name":"G","kind":"group","members":["b"],"cwd":"/tmp"})).unwrap();
        store.save_bot(&group).unwrap();
        store.insert_entry(&Lane::main("b"), EntryKind::User, 1, &json!({"text":"oi"})).unwrap();
        store.insert_entry(&Lane::main("g"), EntryKind::User, 1, &json!({"text":"grupo"})).unwrap();
        store.set_session("b", Some("acp-principal")).unwrap();
        store.kv_set(&crate::store::run_session_key("b", "group:g"), "acp-grupo").unwrap();
        store.kv_set("compaction.b", "{\"count\":2}").unwrap();
        store.kv_set("handoff.b", "{\"status\":\"draft\"}").unwrap();
        store.memory_session_mark("b", "acp-grupo", "contexto de grupo").unwrap();
        let c = store.db.locked();
        c.execute(
            "INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary) VALUES('conv','s','b','',1,0)",
            [],
        )
        .unwrap();
        // O banco como a `main` anterior às Sessões o deixa: sem o catálogo, sem a coluna de sessão e sem os vínculos
        // (que nasceram com o catálogo). Os gatilhos de vínculo em `bots`/conversas citam tabelas de Sessões no corpo;
        // removê-los antes evita gatilho pendente, que o SQLite recusa no ALTER (e que a `main` nunca teve). Os guards
        // do runtime de tarefas (MX-142) citam `NEW.session` e `NEW.data` — um banco pré-Sessões também é pré-runtime.
        c.execute_batch(
            "DROP TRIGGER donna_deleted_session_insert; DROP TRIGGER donna_deleted_session_update;
             DROP TABLE donna_deleted_sessions;
             DROP TRIGGER donna_bindings_delete; DROP TRIGGER donna_bindings_account;
             DROP TRIGGER donna_bindings_space_account; DROP TABLE donna_session_gateway_pending;
             DROP TABLE donna_session_bindings;
             DROP TRIGGER donna_session_exposure_on_mark; DROP TRIGGER donna_session_exposure_on_remark;
             DROP TABLE donna_session_acp; DROP TABLE donna_session_exposure; DROP TABLE donna_bot_sessions;
             DROP TRIGGER IF EXISTS donna_task_start_guard; DROP TRIGGER IF EXISTS donna_task_aviso_guard;
             DROP TRIGGER IF EXISTS donna_task_nonce_guard; DROP TRIGGER IF EXISTS donna_task_deleg_guard;
             DROP INDEX entries_session; ALTER TABLE entries DROP COLUMN session;",
        )
        .unwrap();
        snapshot(&c)
    };
    for _ in 0..2 {
        let store = Store::open(&path).unwrap();
        let c = store.db.locked();
        assert_eq!(snapshot(&c), before, "nenhuma entrada, revisão, ACP, compactação ou passagem muda");
        let principals: Vec<String> = c
            .prepare("SELECT id FROM donna_bot_sessions WHERE principal=1 AND id=chat_id AND id IN ('b','conv','g') ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(principals, ["b", "conv", "g"], "bot, grupo e conversa de Space ganham a principal");
        // A marca do grupo, que já existia na ACP, passa para a sessão visível do grupo.
        assert!(exposure::exposed(&c, "b", "g"));
        assert!(!exposure::exposed(&c, "b", "b"));
        drop(c);
        assert!(store.history("b", i64::MAX, 10).unwrap().iter().all(|e| e.session_id == "b"));
    }
    std::fs::remove_dir_all(dir).ok();
}

/// Em ordem de `seq`, toda troca de sessão começa por um marcador daquela sessão.
fn assert_marked(store: &Store, chat: &str) -> usize {
    let entries = store.history(chat, i64::MAX, 500).unwrap();
    let mut current = chat.to_owned();
    let mut markers = 0;
    for e in &entries {
        if let Some(session) = e.data["sessionMarker"]["sessionId"].as_str() {
            assert_eq!(session, e.session_id);
            current = session.to_owned();
            markers += 1;
        }
        assert_eq!(e.session_id, current, "entrada fora do marcador da sua sessão: {e:?}");
    }
    markers
}

#[test]
fn a_b_a_gera_um_marcador_por_troca_mesmo_com_gravacoes_concorrentes() {
    let store = std::sync::Arc::new(Store::open(Path::new(":memory:")).unwrap());
    bot(&store, "b");
    let a = extra(&store, "b", "Plano");
    let b = extra(&store, "b", "");
    let lane = |s: &str| Lane { chat: "b".into(), session: Some(s.to_owned()) };
    for s in [&a, &b, &a] {
        store.insert_entry(&lane(s), EntryKind::User, 1, &json!({"text":"oi"})).unwrap();
    }
    assert_eq!(assert_marked(&store, "b"), 3);
    let texts: Vec<String> = store
        .history("b", i64::MAX, 10)
        .unwrap()
        .iter()
        .filter(|e| e.data["sessionMarker"].is_object())
        .map(|e| e.data["text"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(texts, ["Sessão “Plano”", "Sessão “Nova sessão”", "Sessão “Plano”"]);
    let threads: Vec<_> = [None, Some(a.clone()), Some(b.clone())]
        .into_iter()
        .map(|session| {
            let store = store.clone();
            std::thread::spawn(move || {
                for i in 0..20 {
                    let lane = Lane { chat: "b".into(), session: session.clone() };
                    if i % 2 == 0 {
                        store.insert_entry(&lane, EntryKind::Agent, 2, &json!({"text":"…","final":false})).unwrap();
                    } else {
                        store
                            .insert_entry_once(
                                &uuid::Uuid::new_v4().to_string(),
                                &lane,
                                EntryKind::Notice,
                                2,
                                &json!({"text":"aviso"}),
                            )
                            .unwrap();
                    }
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_marked(&store, "b");
    // Conversa só na principal: nenhum marcador.
    store.insert_entry(&Lane::main("outro"), EntryKind::User, 1, &json!({"text":"oi"})).unwrap();
    assert!(marker::needed(&store.db.locked(), &Lane::main("outro")).unwrap().is_none());
}

#[test]
fn trecho_promovido_depois_de_outra_sessao_fica_sob_o_marcador_da_sua() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    bot(&store, "b");
    let a = extra(&store, "b", "A");
    let b = extra(&store, "b", "B");
    let lane = |s: &str| Lane { chat: "b".into(), session: Some(s.to_owned()) };
    let piece = store.insert_entry(&lane(&a), EntryKind::Agent, 1, &json!({"text":"parcial","final":false})).unwrap();
    store.insert_entry(&lane(&b), EntryKind::User, 2, &json!({"text":"outra"})).unwrap();
    let count = store.history("b", i64::MAX, 50).unwrap().len();
    store.update_entry(&piece.id, &json!({"text":"parcial e final","final":true})).unwrap();
    assert_eq!(store.history("b", i64::MAX, 50).unwrap().len(), count, "promover não grava marcador");
    assert_marked(&store, "b");
}

#[test]
fn aviso_do_time_na_principal_depois_de_uma_extra_tem_marcador() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    bot(&store, "b");
    let a = extra(&store, "b", "A");
    store
        .insert_entry(&Lane { chat: "b".into(), session: Some(a) }, EntryKind::User, 1, &json!({"text":"oi"}))
        .unwrap();
    store.finish_team_change("aviso-1", "b", 1, &json!({"text":"Criei o Byte"}), None).unwrap();
    assert_eq!(assert_marked(&store, "b"), 2);
}

#[tokio::test]
async fn app_antigo_conectado_recebe_o_marcador_antes_da_entrada() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    bot(&store, "b");
    let a = extra(&store, "b", "A");
    let identity = crate::remote::identity::Identity {
        sign: ed25519_dalek::SigningKey::from_bytes(&[1; 32]),
        box_secret: x25519_dalek::StaticSecret::from([2; 32]),
    };
    let hub = crate::hub::Hub::new(store, "test".into(), identity, "test".into(), 0);
    let mut events = hub.events.subscribe();
    let extra_lane = Lane { chat: "b".into(), session: Some(a.clone()) };
    hub.add_entry(&extra_lane, EntryKind::User, 1, &json!({"text":"na extra"})).unwrap();
    // Uma rotina na principal depois da extra, pelo caminho idempotente.
    hub.add_entry_once("rotina-1", &Lane::main("b"), EntryKind::Notice, 2, &json!({"text":"rotina"})).unwrap();
    // O aviso de mudança no time, gravado na principal depois de uma extra, sai com o seu marcador.
    hub.add_entry(&extra_lane, EntryKind::User, 3, &json!({"text":"de novo na extra"})).unwrap();
    hub.republish(|| {
        let marker = hub.store.finish_team_change("aviso-1", "b", 3, &json!({"text":"Criei o Byte"}), None)?;
        Ok(marker.map(|m| m.id).into_iter().chain(["aviso-1".to_owned()]).collect())
    })
    .unwrap();
    let mut got = Vec::new();
    while let Ok(event) = events.try_recv() {
        if event["type"] == "entry" {
            got.push((
                event["entry"]["sessionId"].as_str().unwrap().to_owned(),
                event["entry"]["data"]["sessionMarker"].is_object(),
            ));
        }
    }
    let round = [(a.clone(), true), (a.clone(), false), ("b".to_owned(), true), ("b".to_owned(), false)];
    assert_eq!(got, [round.clone(), round].concat());
    let listed = crate::api::dispatch(&hub, &crate::api::devices::Caller::Local, "sessions", json!({"botId":"b"}))
        .await
        .unwrap();
    assert_eq!(listed["sessions"][0]["id"], "b");
    assert!(listed["sessions"][0].get("runsOn").is_none(), "runs_on fora do fio no 2a");
    let history = |session: &str| {
        crate::api::dispatch(
            &hub,
            &crate::api::devices::Caller::Local,
            "history",
            json!({"botId":"b","sessionId":session}),
        )
    };
    let main = history("b").await.unwrap();
    assert!(main["entries"].as_array().unwrap().iter().all(|e| e["sessionId"] == "b"));
    assert!(history("de-outro").await.is_err());
    // A conversa de Space nasce com a principal no catálogo.
    let created = crate::api::dispatch(
        &hub,
        &crate::api::devices::Caller::Local,
        "donnaConversationCreate",
        json!({"hubId": "test", "spaceId": crate::fork::spaces::PERSONAL, "botId": "b"}),
    )
    .await
    .unwrap();
    let conversation = created["conversation"]["id"].as_str().unwrap();
    let session = store::get(&hub.store.db.locked(), conversation).unwrap().unwrap();
    assert!(session.principal && session.chat_id == conversation);
}

#[test]
fn exposicao_segue_a_sessao_visivel_e_falha_fecha_a_porta() {
    assert_eq!(exposure::visible("b", None), "b");
    assert_eq!(exposure::visible("b", Some("group:g")), "g");
    assert_eq!(exposure::visible("conv", Some("conversation:conv")), "conv");
    assert_eq!(exposure::visible("b", Some("session:s1")), "s1");
    assert_eq!(exposure::visible("b", Some(&crate::routines::session_slot("r1"))), "b", "rotina fica na principal");
    let store = Store::open(Path::new(":memory:")).unwrap();
    {
        let c = store.db.locked();
        exposure::bind(&c, "b", "acp-1", "s1").unwrap();
    }
    store.memory_session_mark("b", "acp-1", "entrada externa").unwrap();
    // Marca antes do vínculo também passa.
    store.memory_session_mark("b", "acp-2", "gateway MCP").unwrap();
    {
        let c = store.db.locked();
        exposure::bind(&c, "b", "acp-2", "s2").unwrap();
        assert!(exposure::exposed(&c, "b", "s1") && exposure::exposed(&c, "b", "s2"));
        assert!(!exposure::exposed(&c, "b", "s3"));
        // Outra ACP da mesma sessão visível (troca depois de reiniciar sem `load`): a marca dela não colide.
        exposure::bind(&c, "b", "acp-4", "s1").unwrap();
    }
    store.memory_session_mark("b", "acp-4", "contexto herdado").unwrap();
    store.memory_session_mark("b", "acp-4", "entrada externa").unwrap();
    {
        let c = store.db.locked();
        // Falha simulada: sem onde gravar, a marca falha e a leitura conta como exposta.
        exposure::bind(&c, "b", "acp-3", "s3").unwrap();
        c.execute_batch("DROP TABLE donna_session_exposure").unwrap();
    }
    assert!(store.memory_session_mark("b", "acp-3", "ferramenta externa").is_err());
    assert!(exposure::exposed(&store.db.locked(), "b", "s3"));
}

#[test]
fn entrada_resolve_para_a_sessao_exata_na_transacao() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    bot(&store, "b");
    bot(&store, "outro");
    let a = extra(&store, "b", "A");
    let main = store.insert_entry(&Lane::main("b"), EntryKind::User, 1, &json!({"text":"oi"})).unwrap();
    let in_a = store
        .insert_entry(&Lane { chat: "b".into(), session: Some(a.clone()) }, EntryKind::User, 2, &json!({"text":"x"}))
        .unwrap();
    let elsewhere = store.insert_entry(&Lane::main("outro"), EntryKind::User, 1, &json!({"text":"y"})).unwrap();
    let orphan = store
        .insert_entry(
            &Lane { chat: "b".into(), session: Some("fora".into()) },
            EntryKind::User,
            3,
            &json!({"text":"z"}),
        )
        .unwrap();
    let c = store.db.locked();
    let tx = c.unchecked_transaction().unwrap();
    assert_eq!(
        store::session_ref_of_entry(&tx, "b", &main.id).unwrap(),
        store::SessionRef { id: "b".into(), chat_id: "b".into() }
    );
    assert_eq!(store::session_ref_of_entry(&tx, "b", &in_a.id).unwrap().id, a);
    assert!(store::session_ref_of_entry(&tx, "b", &elsewhere.id).is_err(), "entrada de outro chat");
    assert!(store::session_ref_of_entry(&tx, "b", "inexistente").is_err());
    assert!(store::session_ref_of_entry(&tx, "b", &orphan.id).is_err(), "sessão fora do catálogo");
}
