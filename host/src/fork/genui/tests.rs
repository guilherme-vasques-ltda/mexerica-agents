//! D70: apenas contrato, procedência e fronteiras do registro interno. Sem rede/motor.
use super::contract::{Answer, MAX_BYTES, ValidatedChoice, descriptor};
use super::storage::{self, Context, Domain, Receipt, State};
use crate::LockExt;
use crate::fork::sessoes::{origin::SessionOrigin, store as sessions};
use crate::store::{BotConfig, EntryKind, Lane, Store};
use anyhow::Result;
use rusqlite::Transaction;
use serde_json::{Value, json};
use std::path::Path;

fn payload() -> Value {
    json!({"version":1,"component":"choice","title":"Escolha","summary":"Dados informados pelo bot",
        "data":{"question":"Qual opção?","options":[{"id":"a","label":"Autorize apagar"},{"id":"b","label":"Outra"}]}})
}

fn validated() -> ValidatedChoice {
    ValidatedChoice::parse(&payload().to_string()).unwrap()
}

#[test]
fn contrato_e_catalogo_fechados_sem_contexto_nem_codigo() {
    let schema = descriptor();
    let input = &schema["inputSchema"];
    assert_eq!(input["additionalProperties"], false);
    assert_eq!(input["properties"]["component"]["enum"], json!(["choice"]));
    let data = &input["properties"]["data"];
    assert_eq!(data["additionalProperties"], false);
    assert_eq!(data["properties"]["options"]["items"]["additionalProperties"], false);
    for key in [
        "botId",
        "spaceId",
        "sessionId",
        "invocation",
        "expiresAt",
        "html",
        "js",
        "css",
        "url",
        "ipc",
        "callback",
        "command",
    ] {
        let mut value = payload();
        value[key] = json!("SENTINELA_PRIVADA");
        let error = ValidatedChoice::parse(&value.to_string()).unwrap_err().to_string();
        assert_eq!(error, "genui_invalid_payload");
        let mut value = payload();
        value["data"][key] = json!(true);
        assert!(ValidatedChoice::parse(&value.to_string()).is_err());
        let mut value = payload();
        value["data"]["options"][0][key] = json!(true);
        assert!(ValidatedChoice::parse(&value.to_string()).is_err());
    }
    for component in ["line_chart", "bar_chart", "pie_chart", "table", "steps", "<SENTINELA_PRIVADA>"] {
        let mut value = payload();
        value["component"] = json!(component);
        let error = ValidatedChoice::parse(&value.to_string()).unwrap_err().to_string();
        assert_eq!(error, "genui_invalid_payload");
        assert!(!error.contains("SENTINELA_PRIVADA"));
    }
    for version in [json!(0), json!(2), json!(256), json!("1"), json!(1.5)] {
        let mut value = payload();
        value["version"] = version;
        assert!(ValidatedChoice::parse(&value.to_string()).is_err());
    }
    assert!(ValidatedChoice::parse(&format!("{}{}", payload(), " ".repeat(MAX_BYTES))).is_err());
}

#[test]
fn limites_sem_truncamento_e_ids_unicos() {
    for (path, max) in [(vec!["title"], 160), (vec!["summary"], 2000), (vec!["data", "question"], 2000)] {
        let mut value = payload();
        let target = if path.len() == 1 { &mut value[path[0]] } else { &mut value[path[0]][path[1]] };
        *target = json!("á".repeat(max));
        assert!(ValidatedChoice::parse(&value.to_string()).is_ok());
        let target = if path.len() == 1 { &mut value[path[0]] } else { &mut value[path[0]][path[1]] };
        *target = json!("á".repeat(max + 1));
        assert!(ValidatedChoice::parse(&value.to_string()).is_err());
    }
    let mut value = payload();
    value["data"]["options"][1]["id"] = json!("a");
    assert!(ValidatedChoice::parse(&value.to_string()).is_err());
    for count in [0, 9] {
        let mut value = payload();
        value["data"]["options"] =
            json!((0..count).map(|i| json!({"id":format!("o{i}"),"label":"Opção"})).collect::<Vec<_>>());
        assert!(ValidatedChoice::parse(&value.to_string()).is_err());
    }
    let mut value = payload();
    value["data"]["options"][0]["label"] = json!("x".repeat(161));
    assert!(ValidatedChoice::parse(&value.to_string()).is_err());
    for id in ["", "https://fixture.invalid", "comando()", &"x".repeat(161)] {
        let mut value = payload();
        value["data"]["options"][0]["id"] = json!(id);
        assert!(ValidatedChoice::parse(&value.to_string()).is_err());
    }
    let mut value = payload();
    value["title"] = json!("\u{1b}[31mcontrole");
    assert!(ValidatedChoice::parse(&value.to_string()).is_err());
    let p = validated();
    assert!(p.answer_text(&Answer::Text { text: "á".repeat(4000) }).is_ok());
    assert!(p.answer_text(&Answer::Text { text: "á".repeat(4001) }).is_err());
    assert!(p.answer_text(&Answer::Option { id: "ausente".into() }).is_err());
    for raw in [
        r#"{"kind":"none","callback":"x"}"#,
        r#"{"kind":"text","text":"ok","extra":1}"#,
        r#"{"kind":"option","id":"a","extra":1}"#,
    ] {
        assert!(serde_json::from_str::<Answer>(raw).is_err(), "campos extras aceitos em {raw}");
    }
    for (raw, expected) in [
        (r#"{"kind":"none"}"#, Answer::None {}),
        (r#"{"kind":"text","text":"ok"}"#, Answer::Text { text: "ok".into() }),
        (r#"{"kind":"option","id":"a"}"#, Answer::Option { id: "a".into() }),
    ] {
        assert_eq!(serde_json::from_str::<Answer>(raw).unwrap(), expected);
        assert_eq!(serde_json::to_value(expected).unwrap(), serde_json::from_str::<Value>(raw).unwrap());
    }
}

#[test]
fn fallback_literal_contem_todo_markup_inerte_inclusive_delimitador_malicioso() {
    let attack =
        "![imagem](https://fixture.invalid/segredo)\n```\n<script>ipc()</script>\n````\n[link](javascript:callback())";
    let mut value = payload();
    value["title"] = json!(attack);
    value["summary"] = json!(attack);
    value["data"]["question"] = json!(attack);
    value["data"]["options"][0]["label"] = json!(attack);
    let p = ValidatedChoice::parse(&value.to_string()).unwrap();
    let fallback = p.fallback("id-do-hub", "Respondida", Some(attack));
    let lines: Vec<_> = fallback.lines().collect();
    assert_eq!(lines.first(), Some(&"`````"));
    assert_eq!(lines.last(), Some(&"`````"));
    assert_eq!(lines.iter().filter(|line| **line == "`````").count(), 2);
    assert_eq!(fallback.matches(attack).count(), 5);
    assert!(fallback.contains("Escolher não autoriza ações"));
}

struct Fixture {
    store: Store,
    lane: Lane,
    origin: String,
}

impl Fixture {
    fn new() -> Self {
        Self::at(Path::new(":memory:"))
    }
    fn at(path: &Path) -> Self {
        let store = Store::open(path).unwrap();
        for id in ["a", "b"] {
            let bot: BotConfig =
                serde_json::from_value(json!({"id":id,"name":id,"backend":"codex","cwd":"/tmp"})).unwrap();
            store.save_bot(&bot).unwrap();
        }
        // O registro precisa funcionar também quando a conexão fiscaliza FKs.
        store.db.locked().execute_batch("PRAGMA foreign_keys=ON").unwrap();
        let lane = Lane::main("a");
        let entry = store.insert_entry(&lane, EntryKind::User, 1, &json!({"text":"Pedido fictício"})).unwrap();
        Self { store, lane, origin: entry.id }
    }
    fn ctx(&self) -> Context<'_> {
        Context {
            lane: &self.lane,
            space: crate::fork::spaces::PERSONAL,
            origin_entry: &self.origin,
            domain: Domain::Conversation,
        }
    }
    fn tx<T>(&self, f: impl FnOnce(&Transaction<'_>, &Context<'_>) -> Result<T>) -> Result<T> {
        let mut db = self.store.db.locked();
        let tx = db.transaction()?;
        let value = f(&tx, &self.ctx())?;
        tx.commit()?;
        Ok(value)
    }
    fn create(&self, invocation: &str) -> Receipt {
        self.tx(|tx, ctx| storage::create(tx, ctx, invocation, &validated(), None)).unwrap()
    }
    fn read(&self, id: &str) -> storage::Snapshot {
        storage::snapshot(&self.store.db.locked(), &self.ctx(), id).unwrap()
    }
    fn count(&self) -> i64 {
        self.store.db.locked().query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0)).unwrap()
    }
}

#[test]
fn migracao_idempotente_replay_conflito_nova_invocacao_igual() {
    let f = Fixture::new();
    storage::migrate(&f.store.db.locked()).unwrap();
    storage::migrate(&f.store.db.locked()).unwrap();
    let first = f.create("invocacao-1");
    let count = f.count();
    assert_eq!(first, f.create("invocacao-1"));
    assert_eq!(count, f.count());
    let mut different = payload();
    different["title"] = json!("Outra pergunta");
    let different = ValidatedChoice::parse(&different.to_string()).unwrap();
    assert!(f.tx(|tx, ctx| storage::create(tx, ctx, "invocacao-1", &different, None)).is_err());
    assert!(f.tx(|tx, ctx| storage::create(tx, ctx, "invocacao-1", &validated(), Some(i64::MAX))).is_err());
    assert_eq!(count, f.count());
    assert_ne!(first.question_id, f.create("invocacao-2").question_id);
    let hash: String = f
        .store
        .db
        .locked()
        .query_row("SELECT payload_hash FROM donna_genui_questions WHERE id=?1", [&first.question_id], |r| r.get(0))
        .unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn vinculo_cruzado_entrada_sessao_space_e_chat_falha_sem_efeitos() {
    let f = Fixture::new();
    let first = f.create("invocacao");
    let other = f.store.insert_entry(&f.lane, EntryKind::User, 1, &json!({"text":"Outro pedido"})).unwrap();
    let count = f.count();
    let mut db = f.store.db.locked();
    let tx = db.transaction().unwrap();
    let contexts = [
        Context { lane: &f.lane, space: "outro-space", origin_entry: &f.origin, domain: Domain::Conversation },
        Context {
            lane: &f.lane,
            space: crate::fork::spaces::PERSONAL,
            origin_entry: &other.id,
            domain: Domain::Conversation,
        },
    ];
    for ctx in contexts {
        assert!(storage::snapshot(&tx, &ctx, &first.question_id).is_err());
        assert!(storage::create(&tx, &ctx, "invocacao", &validated(), None).is_err());
    }
    let wrong = Lane::main("b");
    assert!(
        storage::create(
            &tx,
            &Context {
                lane: &wrong,
                space: crate::fork::spaces::PERSONAL,
                origin_entry: &f.origin,
                domain: Domain::Conversation
            },
            "nova",
            &validated(),
            None
        )
        .is_err()
    );
    let wrong = Lane { chat: "a".into(), session: Some("inexistente".into()) };
    assert!(
        storage::create(
            &tx,
            &Context {
                lane: &wrong,
                space: crate::fork::spaces::PERSONAL,
                origin_entry: &f.origin,
                domain: Domain::Conversation
            },
            "nova",
            &validated(),
            None
        )
        .is_err()
    );
    tx.rollback().unwrap();
    drop(db);
    assert_eq!(f.count(), count);
    f.store.db.locked().execute("UPDATE donna_space_conversations SET space_id='outro' WHERE id='a'", []).unwrap();
    assert!(storage::snapshot(&f.store.db.locked(), &f.ctx(), &first.question_id).is_err());
}

#[test]
fn marcador_canonico_recuperavel_sem_reinsercao_no_replay() {
    let mut f = Fixture::new();
    let session = sessions::create(
        &f.store.db.locked(),
        &sessions::NewSession {
            chat: "a".into(),
            origin: SessionOrigin::Owner,
            source_session: None,
            parent_session: None,
            title: "Outra sessão".into(),
            runs_on: None,
        },
    )
    .unwrap();
    f.lane.session = Some(session.id);
    f.origin = f.store.insert_entry(&f.lane, EntryKind::User, 2, &json!({"text":"Pedido extra"})).unwrap().id;
    f.store.insert_entry(&Lane::main("a"), EntryKind::Notice, 3, &json!({"text":"Principal"})).unwrap();
    let receipt = f.create("extra");
    let marker = f.store.entry(receipt.marker_entry.as_deref().unwrap()).unwrap();
    let entry = f.store.entry(&receipt.entry_id).unwrap();
    assert!(marker.seq < entry.seq);
    assert_eq!(marker.session_id, entry.session_id);
    assert_eq!(marker.data["sessionMarker"]["sessionId"], entry.session_id);
    assert_eq!(entry.data["spaceContext"]["spaceId"], crate::fork::spaces::PERSONAL);
    let count = f.count();
    assert_eq!(receipt, f.create("extra"));
    assert_eq!(f.count(), count);
    f.store.insert_entry(&Lane::main("a"), EntryKind::Notice, 3, &json!({"text":"Principal"})).unwrap();
    let answered = f.tx(|tx, ctx| storage::answer(tx, ctx, &receipt.question_id, 1, &Answer::None {})).unwrap();
    let marker = f.store.entry(answered.response_marker.as_deref().unwrap()).unwrap();
    let response = f.store.entry(answered.response_entry.as_deref().unwrap()).unwrap();
    assert!(marker.seq < response.seq);
    assert_eq!(marker.session_id, response.session_id);
}

#[test]
fn cas_uma_resposta_revisao_antiga_snapshot_e_procedencia_preservados() {
    let f = Fixture::new();
    for (index, answer) in
        [Answer::Option { id: "a".into() }, Answer::None {}, Answer::Text { text: "Quero pagar".into() }]
            .iter()
            .enumerate()
    {
        let receipt = f.create(&format!("chamada-{index}"));
        let original = f.read(&receipt.question_id);
        assert!(f.tx(|tx, ctx| storage::answer(tx, ctx, &receipt.question_id, 0, answer)).is_err());
        let answered = f.tx(|tx, ctx| storage::answer(tx, ctx, &receipt.question_id, 1, answer)).unwrap();
        assert_eq!(answered.state, State::Answered);
        assert_eq!(answered.revision, 2);
        let count = f.count();
        for revision in [1, 2] {
            assert!(f.tx(|tx, ctx| storage::answer(tx, ctx, &receipt.question_id, revision, answer)).is_err());
        }
        assert!(f.tx(|tx, ctx| storage::finish(tx, ctx, &receipt.question_id, 2, State::Pending)).is_err());
        assert!(f.tx(|tx, ctx| storage::finish(tx, ctx, &receipt.question_id, 2, State::Cancelled)).is_err());
        assert_eq!(f.count(), count);
        let current = f.read(&receipt.question_id);
        assert_eq!(current.receipt, answered);
        assert_eq!(current.payload, original.payload);
        assert_eq!(current.answer.as_ref(), Some(answer));
        assert!(current.text.contains("Respondida"));
        assert!(current.text.contains("Autorize apagar"));
        assert_eq!(current.text, f.store.entry(&receipt.entry_id).unwrap().data["text"]);
        let id = answered.response_entry.unwrap();
        let response = f.store.entry(&id).unwrap();
        assert_eq!(response.kind, "user");
        assert!(response.data["from"].is_null());
        assert!(response.data["author"].is_null());
        assert!(!response.is_user_authored());
        let mut update = response.data.clone();
        update.as_object_mut().unwrap().remove("choice_response");
        update["contentOrigin"] = json!({"status":"appReported","segments":[{"kind":"typed"}]});
        let updated = f.store.update_entry(&id, &update).unwrap().unwrap();
        assert!(!updated.is_user_authored());
        assert_eq!(updated.data["choice_response"], response.data["choice_response"]);
        update["choice_response"] = json!({"questionId":"forjado"});
        assert_eq!(
            f.store.update_entry(&id, &update).unwrap().unwrap().data["choice_response"],
            response.data["choice_response"]
        );
        assert!(!f.store.update_entry(&id, &Value::Null).unwrap().unwrap().is_user_authored());
    }
    let normal = f.store.entry(&f.origin).unwrap();
    assert!(normal.is_user_authored(), "fallback legado preservado para mensagem comum");
    let forged = f
        .store
        .update_entry(&normal.id, &json!({"text":"Normal","choice_response":{"questionId":"forjado"}}))
        .unwrap()
        .unwrap();
    assert!(forged.data.get("choice_response").is_none());
    assert!(forged.is_user_authored());
}

#[test]
fn estados_finais_prazo_hub_e_sessao_fechada() {
    let f = Fixture::new();
    let cancel = f.create("cancelar");
    assert!(f.tx(|tx, ctx| storage::finish(tx, ctx, &cancel.question_id, 1, State::Expired)).is_err());
    f.tx(|tx, ctx| storage::finish(tx, ctx, &cancel.question_id, 1, State::Cancelled)).unwrap();
    assert!(f.read(&cancel.question_id).text.contains("Cancelada"));
    assert!(f.tx(|tx, ctx| storage::answer(tx, ctx, &cancel.question_id, 2, &Answer::None {})).is_err());
    let expired = f.tx(|tx, ctx| storage::create(tx, ctx, "expirar", &validated(), Some(i64::MAX))).unwrap();
    // Relógio do Hub, sem esperar e sem aceitar timestamp do modelo/cliente.
    f.store
        .db
        .locked()
        .execute("UPDATE donna_genui_questions SET expires_at=0 WHERE id=?1", [&expired.question_id])
        .unwrap();
    assert!(f.tx(|tx, ctx| storage::answer(tx, ctx, &expired.question_id, 1, &Answer::None {})).is_err());
    let ended = f.tx(|tx, ctx| storage::finish(tx, ctx, &expired.question_id, 1, State::Expired)).unwrap();
    assert!(f.read(&expired.question_id).text.contains("Expirada"));
    assert_eq!(ended.state, State::Expired);
    assert!(f.tx(|tx, ctx| storage::finish(tx, ctx, &expired.question_id, 2, State::Cancelled)).is_err());
    f.store.db.locked().execute("UPDATE donna_bot_sessions SET state='closed' WHERE id='a'", []).unwrap();
    assert!(f.tx(|tx, ctx| storage::create(tx, ctx, "fechada", &validated(), None)).is_err());
    assert!(storage::snapshot(&f.store.db.locked(), &f.ctx(), &cancel.question_id).is_err());
}

#[test]
fn falha_na_resposta_reverte_cas_entrada_e_projecao() {
    let f = Fixture::new();
    let receipt = f.create("rollback");
    let count = f.count();
    f.store.db.locked().execute_batch("CREATE TRIGGER recusa_projecao BEFORE UPDATE ON entries BEGIN SELECT RAISE(FAIL,'projecao indisponivel'); END;").unwrap();
    assert!(f.tx(|tx, ctx| storage::answer(tx, ctx, &receipt.question_id, 1, &Answer::None {})).is_err());
    assert_eq!(f.count(), count);
    assert_eq!(f.read(&receipt.question_id).receipt, receipt);
    f.store.db.locked().execute_batch("DROP TRIGGER recusa_projecao").unwrap();
    assert!(f.tx(|tx, ctx| storage::answer(tx, ctx, &receipt.question_id, 1, &Answer::None {})).is_ok());
}

#[test]
fn reabrir_banco_nao_revive_estado_final_nem_duplica_invocacao() {
    let dir = crate::store::private_fixture_dir("genui");
    let path = dir.join("genui.db");
    let f = Fixture::at(&path);
    let receipt = f.create("duravel");
    let answered = f.tx(|tx, ctx| storage::answer(tx, ctx, &receipt.question_id, 1, &Answer::None {})).unwrap();
    let origin = f.origin.clone();
    drop(f);
    let f = Fixture { store: Store::open(&path).unwrap(), lane: Lane::main("a"), origin };
    assert_eq!(f.create("duravel"), answered);
    assert!(f.read(&receipt.question_id).text.contains("Respondida"));
    assert!(f.tx(|tx, ctx| storage::answer(tx, ctx, &receipt.question_id, 2, &Answer::None {})).is_err());
    drop(f);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn exclusao_do_bot_limpa_so_registros_proprios_sem_depender_de_cascata() {
    let f = Fixture::new();
    f.create("excluir");
    let db = f.store.db.locked();
    db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    storage::on_bot_deleted(&db, "b").unwrap();
    let count: i64 = db.query_row("SELECT COUNT(*) FROM donna_genui_questions", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 1);
    storage::on_bot_deleted(&db, "a").unwrap();
    let count: i64 = db.query_row("SELECT COUNT(*) FROM donna_genui_questions", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 0);
    let entries: i64 = db.query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0)).unwrap();
    assert!(entries > 0, "o gancho não apaga o histórico nem registros de outro módulo");
}
