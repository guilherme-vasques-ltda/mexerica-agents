//! D70 do PR 4 nas fronteiras de Sessões: `engine_choice` estrito (D372: retrato na criação e migração única),
//! `sessionCreate` sem execução,
//! `send.sessionId` com destino conferido, título pela 1ª mensagem e resumo por sessão. Admissão: `admissao_tests`.
use super::engine_choice::{self, EngineSource, SessionEngine};
use super::{api as sessions_api, store, titulo};
use crate::{
    LockExt,
    api::{self, devices::Caller},
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, EntryKind, Lane, Store},
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

fn hub() -> (Arc<Hub>, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("sessoes-pr4-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(Path::new(":memory:")).unwrap();
    for id in ["b", "c"] {
        let cfg: BotConfig =
            serde_json::from_value(json!({"id": id, "name": id, "backend": "claude", "cwd": "/tmp"})).unwrap();
        store.save_bot(&cfg).unwrap();
    }
    let hub = Hub::new(store, "fixture".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
    (hub, dir)
}

fn own() -> SessionEngine {
    SessionEngine {
        schema_version: 2,
        source: EngineSource::Own,
        account_id: Some(format!("ac_{}", "a".repeat(64))),
        backend: "claude".into(),
        model: Some("sonnet".into()),
        model_options: [].into(),
        fast_mode: false,
    }
}

async fn create(hub: &Arc<Hub>, body: Value) -> anyhow::Result<Value> {
    api::dispatch(hub, &Caller::Local, "sessionCreate", body).await
}

#[test]
fn engine_choice_nasce_com_retrato_e_le_estrito() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let cfg: BotConfig =
        serde_json::from_value(json!({"id":"b","name":"b","backend":"claude","fastMode":true})).unwrap();
    store.save_bot(&cfg).unwrap();
    let c = store.db.locked();
    // A principal nasce com o retrato do padrão: conta e modelo ausentes continuam ausentes (nada inventado).
    let principal = engine_choice::read(&c, "b").unwrap().unwrap();
    assert_eq!(
        (principal.source, principal.account_id.as_deref(), principal.backend.as_str(), principal.model.as_deref()),
        (EngineSource::BotDefault, None, "claude", None)
    );
    assert!(principal.fast_mode);
    // Repetir a migração não altera nada.
    super::migrate(&c).unwrap();
    super::migrate(&c).unwrap();
    assert_eq!(engine_choice::read(&c, "b").unwrap().unwrap(), principal);
    assert!(engine_choice::read(&c, "inexistente").is_err());
    let good = serde_json::to_string(&own()).unwrap();
    assert!(engine_choice::parse(&good).is_ok());
    let account = format!("ac_{}", "a".repeat(64));
    for bad in [
        "não é json".to_owned(),
        good.replace("\"schemaVersion\":2", "\"schemaVersion\":3"),
        good.replace('}', ",\"extra\":1}"),
        json!({"schemaVersion":1,"accountId":account,"selection":{"backend":"claude","model":"m","modelOptions":{}}})
            .to_string(),
        json!({"schemaVersion":2,"source":"own","accountId":account,"backend":"outro","model":"m","modelOptions":{},"fastMode":false})
            .to_string(),
        json!({"schemaVersion":2,"source":"own","accountId":"ac_curta","backend":"claude","model":"m","modelOptions":{},"fastMode":false})
            .to_string(),
        json!({"schemaVersion":2,"source":"own","accountId":account,"backend":"claude","model":"m","modelOptions":{}})
            .to_string(),
        json!({"schemaVersion":2,"source":"talvez","accountId":null,"backend":"claude","model":null,"modelOptions":{},"fastMode":false})
            .to_string(),
    ] {
        assert!(engine_choice::parse(&bad).is_err(), "{bad}");
    }
    // Coluna adulterada falha fechado na leitura, sem virar NULL/padrão; a lista continua legível com `engine: null`.
    c.execute("UPDATE donna_bot_sessions SET engine_choice='{\"schemaVersion\":2}' WHERE id='b'", []).unwrap();
    assert!(engine_choice::read(&c, "b").is_err());
    assert_eq!(store::list(&c, "b").unwrap()[0].engine, None);
}

#[test]
fn migracao_unica_preserva_escolha_fixada_e_estado_legado_sem_inventar() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let cfg: BotConfig =
        serde_json::from_value(json!({"id":"b","name":"b","backend":"codex","model":"m-padrao","fastMode":true}))
            .unwrap();
    store.save_bot(&cfg).unwrap();
    let mut c = store.db.locked();
    let new = store::NewSession {
        chat: "b".into(),
        origin: super::origin::SessionOrigin::Owner,
        source_session: None,
        parent_session: None,
        title: String::new(),
        runs_on: None,
    };
    let (fixed, unsent, broken) =
        (store::create(&c, &new).unwrap().id, store::create(&c, &new).unwrap().id, store::create(&c, &new).unwrap().id);
    // Estado anterior à D372: principal e extra sem envio NULL, extra fixada no formato v1, uma ilegível.
    let account = format!("ac_{}", "c".repeat(64));
    let v1 = json!({"schemaVersion":1,"accountId":account,"selection":{"backend":"codex","model":"m-fixado","modelOptions":{"effort":"high"}}});
    c.execute("UPDATE donna_bot_sessions SET engine_choice=NULL WHERE id IN ('b',?1)", [&unsent]).unwrap();
    c.execute("UPDATE donna_bot_sessions SET engine_choice=?2 WHERE id=?1", [&fixed, &v1.to_string()]).unwrap();
    c.execute("UPDATE donna_bot_sessions SET engine_choice='{}' WHERE id=?1", [&broken]).unwrap();
    c.execute("DELETE FROM kv WHERE k='sessoes.engine.v2'", []).unwrap();
    let revisions: Vec<i64> = store::list(&c, "b").unwrap().iter().map(|s| s.revision).collect();
    let tx = c.transaction().unwrap();
    engine_choice::migrate(&tx).unwrap();
    tx.commit().unwrap();
    let principal = engine_choice::read(&c, "b").unwrap().unwrap();
    assert_eq!(
        (principal.source, principal.account_id.as_deref(), principal.model.as_deref(), principal.fast_mode),
        (EngineSource::BotDefault, None, Some("m-padrao"), true)
    );
    assert_eq!(engine_choice::read(&c, &unsent).unwrap().unwrap(), principal, "extra sem envio: padrão vigente");
    let migrated = engine_choice::read(&c, &fixed).unwrap().unwrap();
    let effort = migrated.model_options.get("effort").map(String::as_str);
    assert_eq!(
        (migrated.account_id.as_deref(), migrated.model.as_deref(), effort),
        (Some(account.as_str()), Some("m-fixado"), Some("high")),
        "escolha fixada preservada"
    );
    assert!(engine_choice::read(&c, &broken).is_err(), "erro não vira padrão nem legado");
    let after: Vec<i64> = store::list(&c, "b").unwrap().iter().map(|s| s.revision).collect();
    assert_eq!(after, revisions, "migração não republica sessões");
    // Uma única vez: depois do marcador, editar o padrão e migrar de novo não mexe em nada.
    c.execute("UPDATE donna_bot_sessions SET engine_choice=NULL WHERE id=?1", [&unsent]).unwrap();
    let tx = c.transaction().unwrap();
    engine_choice::migrate(&tx).unwrap();
    tx.commit().unwrap();
    assert!(engine_choice::read(&c, &unsent).unwrap().is_none());
}

#[tokio::test]
async fn session_create_so_cria_e_send_para_sessao_confere_destino_sem_gravar_recusas() {
    let (hub, dir) = hub();
    let mut events = hub.events.subscribe();
    // UI-01: o anúncio que o app exige antes de criar ou enviar numa extra, na resposta real do catálogo.
    let listed = api::dispatch(&hub, &Caller::Local, "sessions", json!({"botId":"b"})).await.unwrap();
    assert_eq!(listed["sessionActions"], json!(["create", "send", "close", "continue", "engine", "choice", "delete"]));
    assert_eq!(listed["sessions"][0]["engine"]["source"], "botDefault");
    let created = create(&hub, json!({"botId":"b"})).await.unwrap();
    let id = created["session"]["id"].as_str().unwrap().to_owned();
    assert_eq!(created["session"]["titleSource"], "first_message");
    assert_eq!(events.recv().await.unwrap()["type"], "session");
    let titled = create(&hub, json!({"botId":"b","title":"  Plano   de viagem "})).await.unwrap();
    assert_eq!(titled["session"]["title"], "Plano de viagem");
    assert_eq!(titled["session"]["titleSource"], "owner");
    assert!(create(&hub, json!({"botId":"b","title":"x".repeat(61)})).await.is_err());
    assert!(create(&hub, json!({"botId":"nada"})).await.is_err());
    // Criar não executa nem grava entrada.
    assert_eq!(hub.store.last_seq("b"), 0);
    assert_eq!(hub.runtime("b").memory_turn_token, None);

    // Extra de bot sem conta gerenciada: recusada no envio pela classificação canônica, antes de gravar.
    let send = |body: Value| api::dispatch(&hub, &Caller::Local, "send", body);
    let refused = sessions_api::send_lane(&hub.store.db.locked(), "b", &json!({"sessionId":id})).unwrap_err();
    assert!(format!("{refused:#}").contains("session_selection_unavailable"), "{refused:#}");
    // Escolha ilegível numa extra falha fechado antes de gravar.
    hub.store.db.locked().execute("UPDATE donna_bot_sessions SET engine_choice='{}' WHERE id=?1", [&id]).unwrap();
    assert!(send(json!({"botId":"b","text":"oi","sessionId":id})).await.is_err());
    // Extra sem seleção (NULL): falha fechado, nunca cai no padrão do bot nem na principal.
    hub.store.db.locked().execute("UPDATE donna_bot_sessions SET engine_choice=NULL WHERE id=?1", [&id]).unwrap();
    assert!(send(json!({"botId":"b","text":"oi","sessionId":id})).await.is_err());
    // Sessão de outro bot, inexistente ou tipo inválido.
    let other = create(&hub, json!({"botId":"c"})).await.unwrap()["session"]["id"].as_str().unwrap().to_owned();
    for (body, code) in [
        (json!({"botId":"b","text":"oi","sessionId":other}), "unknown_session"),
        (json!({"botId":"b","text":"oi","sessionId":"inexistente"}), "unknown_session"),
        (json!({"botId":"b","text":"oi","sessionId":7}), "invalid_session"),
    ] {
        assert!(format!("{:#}", send(body).await.unwrap_err()).contains(code));
    }
    // Encerrada.
    hub.store.db.locked().execute("UPDATE donna_bot_sessions SET state='closed' WHERE id=?1", [&id]).unwrap();
    let closed = send(json!({"botId":"b","text":"oi","sessionId":id})).await.unwrap_err();
    assert!(format!("{closed:#}").contains("session_closed"));
    assert_eq!(hub.store.last_seq("b"), 0, "nenhuma recusa grava entrada");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn send_lane_principal_aceita_selecao_propria_e_ilegivel_falha_fechado() {
    let (hub, dir) = hub();
    let c = hub.store.db.locked();
    for body in [json!({}), json!({"sessionId":null}), json!({"sessionId":"b"})] {
        assert_eq!(sessions_api::send_lane(&c, "b", &body).unwrap(), Lane::main("b"));
    }
    // D372: seleção própria na principal é válida; ilegível falha fechado, sem cair no padrão do bot.
    let raw = serde_json::to_string(&own()).unwrap();
    c.execute("UPDATE donna_bot_sessions SET engine_choice=?2 WHERE id=?1", ["b", raw.as_str()]).unwrap();
    assert_eq!(sessions_api::send_lane(&c, "b", &json!({})).unwrap(), Lane::main("b"));
    c.execute("UPDATE donna_bot_sessions SET engine_choice='{}' WHERE id='b'", []).unwrap();
    assert!(sessions_api::send_lane(&c, "b", &json!({})).is_err());
    drop(c);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn titulo_pela_primeira_mensagem_sem_trocar_o_do_dono() {
    assert_eq!(titulo::from_message("\n\n## **Plano** de `viagem`\nresto"), "Plano de viagem");
    assert_eq!(titulo::from_message("- item _um_"), "item um");
    assert_eq!(titulo::from_message("   "), "");
    let long = titulo::from_message(&"palavra ".repeat(20));
    assert_eq!(long.chars().count(), store::TITLE_MAX);
    assert!(long.ends_with('…'));

    let store = Store::open(Path::new(":memory:")).unwrap();
    let cfg: BotConfig = serde_json::from_value(json!({"id":"b","name":"b","backend":"claude"})).unwrap();
    store.save_bot(&cfg).unwrap();
    let c = store.db.locked();
    let new = |title: &str| store::NewSession {
        chat: "b".into(),
        origin: super::origin::SessionOrigin::Owner,
        source_session: None,
        parent_session: None,
        title: title.into(),
        runs_on: None,
    };
    let id = store::create(&c, &new("")).unwrap().id;
    assert_eq!(titulo::first_message(&c, &id, "Primeira ideia").unwrap().unwrap().title, "Primeira ideia");
    assert!(titulo::first_message(&c, &id, "Segunda").unwrap().is_none(), "só a primeira");
    let owner = store::create(&c, &new("")).unwrap().id;
    let rev = store::get(&c, &owner).unwrap().unwrap().revision;
    store::rename(&c, "b", &owner, "Do dono", rev).unwrap();
    assert!(titulo::first_message(&c, &owner, "Outra").unwrap().is_none());
    assert!(titulo::first_message(&c, "b", "Principal").unwrap().is_none(), "a principal não ganha título");
}

#[test]
fn resumo_da_troca_de_acp_le_so_a_propria_sessao() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let cfg: BotConfig = serde_json::from_value(json!({"id":"b","name":"b","backend":"claude"})).unwrap();
    store.save_bot(&cfg).unwrap();
    let id = {
        let c = store.db.locked();
        let new = store::NewSession {
            chat: "b".into(),
            origin: super::origin::SessionOrigin::Owner,
            source_session: None,
            parent_session: None,
            title: String::new(),
            runs_on: None,
        };
        store::create(&c, &new).unwrap().id
    };
    let extra = Lane { chat: "b".into(), session: Some(id.clone()) };
    let main_text = json!({"text":"segredo da principal","status":"sent"});
    store.insert_entry(&Lane::main("b"), EntryKind::User, 1, &main_text).unwrap();
    store.insert_entry(&extra, EntryKind::User, 2, &json!({"text":"pedido da extra","status":"sent"})).unwrap();
    let handoff = crate::chat::handoff::build_in(&store, "b", &id, "acp-1").unwrap();
    assert!(handoff.contains("pedido da extra"));
    assert!(!handoff.contains("segredo da principal"));
    let main = crate::chat::handoff::build(&store, "b", "acp-0").unwrap();
    assert!(main.contains("segredo da principal") && !main.contains("pedido da extra"));
}
