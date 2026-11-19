//! Fixtures locais, sem modelo: revalidação determinística entre consulta e entrega.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::*;
use crate::guard_hub::tool_origin::MAIN_SLOT;
use crate::remote::identity::Identity;
use crate::store::{BotConfig, EntryKind, Lane, Store};
use base64::Engine as _;

struct Fixture {
    hub: Arc<Hub>,
    dir: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("donna-history-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Hub::new(
            Store::open(std::path::Path::new(":memory:")).unwrap(),
            "history-test".into(),
            Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        for id in ["a", "b"] {
            let cfg: BotConfig = serde_json::from_value(json!({"id":id,"name":id,"cwd":dir})).unwrap();
            hub.store.save_bot(&cfg).unwrap();
        }
        let fixture = Self { hub, dir };
        fixture.hub.store.memory_session_start("a", "fixture-session", false).unwrap();
        let user = fixture.add("a", EntryKind::User, &json!({"text":"Consultar fonte", "contentOrigin":{"status":"appReported", "segments":[{"kind":"typed","start":0,"end":15}]}}));
        fixture.hub.set_runtime("a", |r| {
            r.status = BotStatus::Working;
            r.message = Some(user.id);
            r.session = Some("fixture-session".into());
            r.lane = Some(Lane::main("a"));
        });
        fixture
    }
    fn add(&self, bot: &str, kind: EntryKind, data: &Value) -> crate::store::Entry {
        self.hub.store.insert_entry(&Lane::main(bot), kind, 1, data).unwrap()
    }
    fn page(&self, args: &Value) -> Page {
        prepare(&self.hub, "a", "search_history", args, Turn::capture(&self.hub, "a").unwrap()).unwrap()
    }
    async fn search(&self, args: Value) -> Value {
        call(&self.hub, "a", "search_history", &args, Some(MAIN_SLOT)).await
    }
    async fn open(&self, reference: &Value) -> Value {
        call(&self.hub, "a", "open_history", &json!({"sourceRef":reference,"before":0,"after":0}), Some(MAIN_SLOT))
            .await
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn atividade_entre_consulta_e_entrega_nao_invalida_a_fonte() {
    let f = Fixture::new();
    let entry = f.add("a", EntryKind::User, &json!({"text":"SENTINELA privada do dono"}));
    let page = f.page(&json!({"query":"SENTINELA"}));
    let before = f.hub.store.bot("a").unwrap().unwrap().rev;
    // Intercala exatamente a notificação de atividade que o ator publica ao iniciar a ferramenta.
    f.hub.set_runtime("a", |r| r.activity = "mcp.memory.search_history".into());
    f.hub.store.mark_read("a").unwrap();
    assert!(f.hub.store.bot("a").unwrap().unwrap().rev > before);
    let answer = deliver(&f.hub, "a", page).unwrap();
    assert_eq!(answer["status"], "ok");
    assert_eq!(answer["results"][0]["id"], entry.id);
}

#[tokio::test]
async fn permissao_entre_consulta_e_entrega_invalida_a_fonte() {
    for restore in [false, true] {
        let f = Fixture::new();
        let page = f.page(&json!({}));
        let mut cfg = f.hub.store.bot("a").unwrap().unwrap().config;
        let original = cfg.permission;
        cfg.permission = crate::store::Permission::Ask;
        f.hub.store.save_bot(&cfg).unwrap();
        if restore {
            cfg.permission = original;
            f.hub.store.save_bot(&cfg).unwrap();
        }
        assert_eq!(failure(&deliver(&f.hub, "a", page).unwrap_err())["status"], "stale_reference");
    }
}

#[tokio::test]
async fn revalidates_revision_bot_scope_namespace_and_storage_before_delivery() {
    let f = Fixture::new();
    let a = f.add("a", EntryKind::User, &json!({"text":"frase idêntica A"}));
    let b = f.add("b", EntryKind::User, &json!({"text":"frase idêntica B"}));
    let found = f.search(json!({"query":"frase"})).await;
    let source = found["results"][0]["sourceRef"].clone();
    assert_eq!(f.open(&source).await["messages"][0]["id"], a.id);
    let mut forged = Source::decode(source.as_str().unwrap()).unwrap();
    forged.entry = b.id;
    let foreign = f.open(&json!(forged.encode().unwrap())).await;
    forged.entry = "missing".into();
    assert_eq!(foreign, f.open(&json!(forged.encode().unwrap())).await);
    forged.entry = a.id.clone();
    forged.namespace = hash("different database");
    assert_eq!(foreign, f.open(&json!(forged.encode().unwrap())).await);
    let other = Fixture::new();
    assert_eq!(foreign, other.open(&source).await, "namespace de banco temporário distinto");
    assert_eq!(foreign, f.open(&json!(format!("{}=", source.as_str().unwrap()))).await);
    let mut bytes = serde_json::to_value(Source::decode(source.as_str().unwrap()).unwrap()).unwrap();
    bytes["extra"] = true.into();
    let unknown = format!("hs1.{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes.to_string()));
    assert_eq!(foreign, f.open(&json!(unknown)).await);
    let page = f.page(&json!({"query":"frase"}));
    f.hub.store.update_entry(&a.id, &json!({"text":"frase idêntica editada"})).unwrap();
    assert_eq!(failure(&deliver(&f.hub, "a", page).unwrap_err())["status"], "stale_reference");
    assert_eq!(f.open(&source).await["status"], "stale_reference");
    let page = f.page(&json!({}));
    let mut cfg = f.hub.store.bot("a").unwrap().unwrap().config;
    cfg.kind = crate::store::BotKind::Group;
    f.hub.store.save_bot(&cfg).unwrap();
    assert_eq!(failure(&deliver(&f.hub, "a", page).unwrap_err())["status"], "denied");
    cfg.kind = crate::store::BotKind::Agent;
    f.hub.store.save_bot(&cfg).unwrap();
    let page = f.page(&json!({}));
    f.hub.store.db.locked().execute("UPDATE bots SET deleted=1 WHERE id='a'", []).unwrap();
    assert_eq!(failure(&deliver(&f.hub, "a", page).unwrap_err())["status"], "denied");

    let f = Fixture::new();
    assert_eq!(f.search(json!({"query":"absent"})).await["status"], "ok");
    let found = f.search(json!({})).await;
    let source = &found["results"][0]["sourceRef"];
    let page = f.page(&json!({}));
    f.hub.store.db.locked().execute("ALTER TABLE entries RENAME TO broken_entries", []).unwrap();
    assert_eq!(failure(&deliver(&f.hub, "a", page).unwrap_err())["status"], "unavailable");
    assert_eq!(f.search(json!({})).await["status"], "unavailable");
    assert_eq!(f.open(source).await["status"], "unavailable");
}

#[tokio::test]
async fn new_capability_requires_the_same_own_human_turn_through_delivery() {
    let f = Fixture::new();
    let found = f.search(json!({})).await;
    let source = &found["results"][0]["sourceRef"];
    let original = f.hub.runtime("a");
    for changed in ["message", "session", "lane", "status"] {
        let page = f.page(&json!({}));
        f.hub.set_runtime("a", |r| match changed {
            "message" => r.message = None,
            "session" => r.session = Some("other-session".into()),
            "lane" => r.lane = Some(Lane::main("b")),
            _ => r.status = BotStatus::Idle,
        });
        assert_eq!(failure(&deliver(&f.hub, "a", page).unwrap_err())["status"], "denied", "{changed}");
        if changed != "session" {
            assert_eq!(f.open(source).await["status"], "denied");
        }
        // Sem o turno humano próprio, a busca também é recusada (D229-6), sem resultado algum.
        let refused = f.search(json!({})).await;
        if changed != "session" {
            assert_eq!(refused["status"], "denied", "{changed}");
            assert!(refused.get("results").is_none());
        }
        f.hub.set_runtime("a", |r| *r = original.clone());
    }
    // Host/outro bot/rotina não ganham autoria humana pelo kind=user.
    for data in [
        json!({"text":"x","from":"host"}),
        json!({"text":"x","sourceBotId":"b"}),
        json!({"text":"x","runId":"routine"}),
    ] {
        let e = f.add("a", EntryKind::User, &data);
        f.hub.set_runtime("a", |r| r.message = Some(e.id));
        assert_eq!(f.open(source).await["status"], "denied");
        assert_eq!(f.search(json!({})).await["status"], "denied");
    }
}

#[tokio::test]
async fn grupos_nao_leem_o_historico_privado_do_membro() {
    let f = Fixture::new();
    f.add("a", EntryKind::User, &json!({"text":"SENTINELA privada do dono"}));
    let original = f.hub.runtime("a");
    for group in ["g1", "g2"] {
        let cfg: BotConfig =
            serde_json::from_value(json!({"id":group,"name":group,"cwd":f.dir,"kind":"group","members":["a","b"]}))
                .unwrap();
        f.hub.store.save_bot(&cfg).unwrap();
        let asked = f.add(group, EntryKind::User, &json!({"text":"Alguém lembra da SENTINELA?"}));
        f.hub.set_runtime("a", |r| {
            r.lane = Some(Lane::main(group));
            r.message = Some(asked.id.clone());
        });
        for (name, args) in
            [("search_history", json!({"query":"SENTINELA"})), ("open_history", json!({"sourceRef":"hs1.qualquer"}))]
        {
            let answer = call(&f.hub, "a", name, &args, Some(MAIN_SLOT)).await;
            assert_eq!(answer["status"], "denied", "{group} {name}");
            assert!(!answer.to_string().contains("do dono"), "{group} {name}: {answer}");
        }
        f.hub.set_runtime("a", |r| *r = original.clone());
    }
    // No turno do dono no chat próprio, a mesma busca continua encontrando a mensagem.
    let found = f.search(json!({"query":"SENTINELA"})).await;
    assert!(found["results"][0]["text"].as_str().unwrap().contains("do dono"), "{found}");
}

#[tokio::test]
async fn cursor_is_inclusive_scoped_query_bound_and_stale_after_edit() {
    let f = Fixture::new();
    let mut ids = vec![];
    for i in 0..4 {
        ids.push(f.add("a", EntryKind::User, &json!({"text":format!("paginavel {i}")})).id);
    }
    ids.reverse();
    let first = f.search(json!({"query":"paginavel","limit":1})).await;
    let second = f.search(json!({"from":first["next"],"query":"paginavel","limit":1})).await;
    assert_eq!(second["results"][0]["id"], ids[1]);
    f.add("a", EntryKind::User, &json!({"text":"paginavel NOVA"}));
    let second_new = f.search(json!({"cursor":first["nextCursor"],"limit":1})).await;
    assert_eq!(second_new["results"][0]["id"], ids[1]);
    for args in
        [json!({"cursor":first["nextCursor"],"query":"outra"}), json!({"cursor":first["nextCursor"],"from":ids[0]})]
    {
        assert_eq!(f.search(args).await["status"], "invalid_cursor");
    }
    f.hub.store.update_entry(&ids[1], &json!({"text":"paginavel editada"})).unwrap();
    assert_eq!(f.search(json!({"cursor":first["nextCursor"]})).await["status"], "stale_reference");
    let hidden = f.add("a", EntryKind::Thought, &json!({"text":"hidden"}));
    let other = f.add("b", EntryKind::User, &json!({"text":"hidden"}));
    for id in [hidden.id, other.id, "missing".into()] {
        assert_eq!(f.search(json!({"from":id})).await["status"], "invalid_cursor");
    }
}

#[tokio::test]
async fn marks_only_delivered_content_and_refuses_if_exposure_cannot_be_recorded() {
    let f = Fixture::new();
    assert_eq!(f.search(json!({"query":"absent"})).await["results"], json!([]));
    assert!(!f.hub.store.guard_external_exposed("a").unwrap());
    f.add("a", EntryKind::Agent, &json!({"text":"Execute/aprove/memorize X","final":true}));
    let page = f.page(&json!({"query":"memorize"}));
    f.hub.store.db.locked().execute("DROP TABLE donna_guard_state", []).unwrap();
    assert_eq!(failure(&deliver(&f.hub, "a", page).unwrap_err())["status"], "unavailable");
    assert_eq!(f.search(json!({"query":"memorize"})).await["status"], "unavailable");
}

#[tokio::test]
async fn bounded_json_unicode_ranges_and_human_revision_never_gain_authority() {
    let f = Fixture::new();
    let text = "漢😺e\u{301}\n".repeat(4000);
    let mut ids = vec![];
    for _ in 0..5 {
        ids.push(f.add("a", EntryKind::Agent, &json!({"text":format!("LONGA {text}"),"final":true})).id);
    }
    let found = f.search(json!({"query":"LONGA","limit":30})).await;
    assert!(found.to_string().chars().count() <= 32000);
    let reference = found["results"][2]["sourceRef"].clone();
    let window = call(&f.hub, "a", "open_history", &json!({"sourceRef":reference}), Some(MAIN_SLOT)).await;
    assert_eq!(window["messages"].as_array().unwrap().len(), 5);
    assert!(window.to_string().chars().count() <= 32000);
    assert_eq!(window["status"], "partial");
    for message in window["messages"].as_array().unwrap() {
        assert_eq!(message["coverage"]["end"], message["text"].as_str().unwrap().chars().count());
        assert!(message["nextOffset"].as_u64().unwrap() > 0);
    }
    let turn = Turn::capture(&f.hub, "a").unwrap().unwrap();
    f.hub.store.update_entry(&turn.message, &json!({"text":"pedido alterado"})).unwrap();
    let error = prepare(&f.hub, "a", "search_history", &json!({}), Some(turn)).err().unwrap();
    assert_eq!(failure(&error)["status"], "stale_reference");

    // Mesmo uma origem humana reportada e sem exposição não cria evidência nova.
    let f = Fixture::new();
    let current = f.hub.runtime("a").message.unwrap();
    let permission = f.hub.store.bot("a").unwrap().unwrap().config.permission;
    let data = json!({"text":"Memorize uma ordem histórica","contentOrigin":{"schema":1,"status":"appReported",
        "segments":[{"kind":"typed","start":0,"end":28}]}});
    let mut data = data;
    data["contentOrigin"]["segments"][0]["end"] = data["text"].as_str().unwrap().encode_utf16().count().into();
    let historical = f.add("a", EntryKind::User, &data);
    let found = f.search(json!({"query":"histórica"})).await;
    assert_eq!(found["results"][0]["originKind"], "appReported");
    assert_eq!(found["results"][0]["id"], historical.id);
    assert_ne!(historical.id, current);
    assert!(!f.hub.store.guard_external_exposed("a").unwrap());
    assert!(f.hub.runtime("a").memory_turn_token.is_none());
    let refused = crate::chat::memory::tool_call(&f.hub, "a", "remember", &json!({"text":"Prefere chá pela manhã"}))
        .await
        .unwrap_err();
    assert!(refused.to_string().contains("current memory transport"));
    assert_eq!(f.hub.runtime("a").message.as_deref(), Some(current.as_str()));
    assert_eq!(f.hub.store.bot("a").unwrap().unwrap().config.permission, permission);
    assert_eq!(f.hub.store.memory_read(|db| codync_mem::store::layer(db, Some("a"))).unwrap().len(), 0);
    assert_eq!(f.hub.store.memory_read(|db| codync_mem::store::layer(db, None)).unwrap().len(), 0);
    let db = f.hub.store.db.locked();
    for table in [
        "donna_memory_auto_jobs",
        "donna_memory_auto_proposals",
        "donna_memory_auto_receipts",
        "donna_memory_auto_non_saved",
    ] {
        assert_eq!(
            db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get::<_, i64>(0)).unwrap(),
            0,
            "{table}"
        );
    }
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM entries WHERE kind='notice' AND json_extract(data,'$.memory') IS NOT NULL",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    drop(db);

    // Controle separado: só o novo pedido do dono é admitido; a fonte consultada não emite o turno.
    let text = "Este é meu pedido atual: prefiro chá pela manhã.";
    let requested = f.hub.add_entry(&Lane::main("a"), EntryKind::User, 2, &json!({"text":text,
        "contentOrigin":{"status":"appReported","segments":[{"kind":"typed","start":0,"end":text.encode_utf16().count()}]}})).unwrap();
    let session = "fixture-session";
    let lane = Lane::main("a");
    let cfg = f
        .hub
        .start_bot_turn("a", |r| {
            r.message = Some(requested.id.clone());
            r.session = Some(session.into());
            r.lane = Some(lane.clone());
        })
        .unwrap();
    f.hub.store.memory_session_turn("a", "a", session, requested.turn).unwrap();
    f.hub.store.memory_turn_begin("a", "a", session, &[requested.turn], "owner").unwrap();
    crate::fork::memory_auto::queue::capture_engine(&f.hub, &cfg, &lane, &cfg.backend).unwrap();
    crate::fork::memory_auto::queue::begin(&f.hub, &cfg, &lane, session, std::slice::from_ref(&requested.id)).unwrap();
    let job = crate::fork::memory_auto::queue::active_job(&f.hub, "a").unwrap().unwrap();
    assert!(!f.hub.store.memory_turn_eligible("a", "a", requested.turn).unwrap(), "admissão não presume sucesso");
    assert_eq!(f.hub.store.bot("a").unwrap().unwrap().config.permission, permission);
    assert_eq!(f.hub.store.memory_read(|db| codync_mem::store::layer(db, Some("a"))).unwrap().len(), 0);
    assert_eq!(f.hub.store.memory_read(|db| codync_mem::store::layer(db, None)).unwrap().len(), 0);
    let db = f.hub.store.db.locked();
    let (raw, outcome, status): (String, String, String) = db
        .query_row(
            "SELECT context,outcome,status FROM donna_memory_auto_jobs WHERE id=?1 AND bot='a' AND chat='a'",
            [&job],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let context: crate::fork::memory_auto::queue::Context = serde_json::from_str(&raw).unwrap();
    crate::fork::memory_auto::worker::validate_context(&db, &context).unwrap();
    assert_eq!((outcome.as_str(), status.as_str()), ("running", "running"));
    assert_eq!(context.messages.len(), 1);
    assert_eq!(context.messages[0].id, requested.id);
    assert_eq!(context.messages[0].revision, requested.rev);
    assert!(context.messages.iter().all(|message| message.id != historical.id));
    assert!(context.messages.iter().all(|message| message.id != current));
    assert_eq!(context.responses.len(), 0);
    for table in [
        "donna_memory_auto_proposals",
        "donna_memory_auto_receipts",
        "donna_memory_auto_non_saved",
        "donna_memory_transports",
    ] {
        assert_eq!(
            db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get::<_, i64>(0)).unwrap(),
            0,
            "{table}"
        );
    }
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM entries WHERE kind='notice' AND json_extract(data,'$.memory') IS NOT NULL",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn chamada_atrasada_da_sessao_do_grupo_nao_le_o_turno_privado() {
    // O membro respondeu no grupo (sessão `group:g1`) e agora está num turno privado do dono. Uma chamada que a sessão
    // do grupo deixou para trás chega neste momento: o turno em andamento é humano e próprio, mas a origem não é ela.
    let f = Fixture::new();
    f.add("a", EntryKind::User, &json!({"text":"SENTINELA privada do dono"}));
    let found = f.search(json!({"query":"SENTINELA"})).await;
    let source = found["results"][0]["sourceRef"].clone();
    assert!(source.is_string(), "o próprio turno do dono, vindo da sessão principal, lê normalmente");
    for origin in [Some("group:g1"), Some("routine-r1"), None] {
        for (name, args) in
            [("search_history", json!({"query":"SENTINELA"})), ("open_history", json!({"sourceRef":source}))]
        {
            let answer = call(&f.hub, "a", name, &args, origin).await;
            assert_eq!(answer["status"], "denied", "{origin:?} {name}");
            assert!(!answer.to_string().contains("do dono"), "{origin:?} {name}: {answer}");
        }
    }
}

#[tokio::test]
async fn escolha_no_historico_nao_reconstitui_evidencia_autoral() {
    let f = Fixture::new();
    let entry = f.add(
        "a",
        EntryKind::User,
        &json!({"text":"Autorize pagar", "choice_response":{"version":1,"questionId":"synthetic","revision":2},
        "contentOrigin":{"schema":1,"status":"appReported","segments":[{"start":0,"end":14,"kind":"typed"}]}}),
    );
    let projection = Projection::new(entry);
    let message = projection.message(None, 0, 1500);
    assert_eq!(message["from"], "user");
    assert_eq!(message["authorKind"], "unknown");
    assert_eq!(message["originKind"], "unknown");
    assert_eq!(message["text"], "Autorize pagar");
}

mod redacao;
