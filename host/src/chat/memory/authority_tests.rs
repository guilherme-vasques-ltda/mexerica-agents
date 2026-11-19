//! D424 / DON-925: a concessão capturada não sobrevive à revogação na fila.
#![allow(clippy::unwrap_used, clippy::panic)]

use super::api::dispatch;
use crate::{
    api::devices::Caller,
    fork::autoridade::{Autoridade, autenticar},
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, Device, DeviceSource, Scope, Store},
};
use serde_json::{Value, json};
use std::{fmt::Write as _, path::PathBuf, sync::Arc};

struct Fixture {
    hub: Arc<Hub>,
    root: PathBuf,
    bot: String,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("d424-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(crate::service::data_dir()).unwrap();
        let store = Store::open(&root.join("state.db")).unwrap();
        let bot = uuid::Uuid::new_v4().to_string();
        let cfg: BotConfig = serde_json::from_value(json!({"id":bot,"name":"Memória D424","backend":"codex"})).unwrap();
        store.save_bot(&cfg).unwrap();
        let hub = Hub::new(store, "d424".into(), Identity::load_or_create(&root).unwrap(), "test".into(), 0);
        Self { hub, root, bot }
    }

    fn device(&self) -> Caller {
        self.hub
            .store
            .put_device(&Device {
                key: "d424-device".into(),
                name: "Teste".into(),
                platform: "ios".into(),
                source: DeviceSource::Local,
                grant_id: None,
                scopes: vec![Scope::Control],
                lease_until: None,
                created_at: crate::store::now_ms(),
                last_seen_at: None,
            })
            .unwrap();
        autenticar(&self.hub, "d424-device").unwrap().1
    }

    fn body(&self, extra: Value) -> Value {
        let mut body = extra;
        body["botId"] = json!(self.bot);
        body
    }

    fn local(&self, method: &str, extra: Value) -> Value {
        let authority = Autoridade::capturar(&self.hub, &Caller::Local).unwrap();
        dispatch(&self.hub, &authority, method, &self.body(extra)).unwrap()
    }

    fn snapshot(&self) -> String {
        use crate::LockExt;
        let db = self.hub.store.db.locked();
        let mut snapshot = String::new();
        let tables = db.prepare("SELECT name FROM sqlite_master WHERE type='table' AND (name LIKE 'memory_%' OR name IN ('donna_memory_auto_receipts','donna_memory_lineage','entries','kv','sqlite_sequence')) ORDER BY name")
            .unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap()
            .collect::<rusqlite::Result<Vec<_>>>().unwrap();
        for table in tables {
            let mut statement = db.prepare(&format!("SELECT * FROM {table}")).unwrap();
            let columns = statement.column_count();
            let mut rows = statement
                .query_map([], |row| {
                    let values = (0..columns)
                        .map(|i| row.get::<_, rusqlite::types::Value>(i))
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    Ok(format!("{values:?}"))
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            rows.sort();
            writeln!(snapshot, "{table}: {rows:?}").unwrap();
        }
        snapshot
    }

    fn revoked(&self, methods: &[(&str, Value)]) {
        let caller = self.device();
        let authority = Autoridade::capturar(&self.hub, &caller).unwrap();
        assert!(self.hub.revoke_device("d424-device").unwrap());
        let before = self.snapshot();
        let mut events = self.hub.events.subscribe();
        let file = super::files::folder(Some(&self.bot)).join("people.md");
        let disk = std::fs::read(&file).ok();
        for (method, body) in methods {
            let error = dispatch(&self.hub, &authority, method, &self.body(body.clone())).unwrap_err();
            assert!(format!("{error:#}").contains("not_allowed"), "{method}: {error:#}");
            assert_eq!(self.snapshot(), before, "{method} não altera DB/recibos");
            assert_eq!(std::fs::read(&file).ok(), disk, "{method} não altera bytes no disco");
            assert!(
                matches!(events.try_recv(), Err(tokio::sync::broadcast::error::TryRecvError::Empty)),
                "{method} não publica eventos"
            );
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
        let _ = std::fs::remove_dir_all(super::files::folder(Some(&self.bot)).parent().unwrap());
    }
}

#[test]
fn revoked_device_cannot_save_or_edit_memory() {
    let f = Fixture::new();
    let id = f.local("saveMemory", json!({"text":"Prefiro chá de manhã."}))["entry"]["id"].clone();
    f.revoked(&[
        ("saveMemory", json!({"text":"Prefiro café à tarde."})),
        ("saveMemory", json!({"id":id,"text":"Prefiro café à tarde."})),
        ("pinMemory", json!({"id":id,"pinned":true})),
        ("archiveMemory", json!({"id":id,"archived":true})),
        ("archiveMemory", json!({"id":id,"archived":false})),
    ]);
}

#[test]
fn revoked_device_cannot_forget_memory() {
    let f = Fixture::new();
    let id = f.local("saveMemory", json!({"text":"Prefiro chá de manhã."}))["entry"]["id"].clone();
    f.revoked(&[("forgetMemory", json!({"id":id}))]);
}

#[tokio::test]
async fn valid_device_saves_through_real_dispatch() {
    let f = Fixture::new();
    let caller = f.device();
    let saved = crate::api::dispatch(&f.hub, &caller, "saveMemory", f.body(json!({"text":"Prefiro chá de manhã."})))
        .await
        .unwrap();
    assert_eq!(saved["entry"]["text"], "Prefiro chá de manhã.");
    let id = saved["entry"]["id"].as_i64().unwrap();
    assert!(f.hub.store.memory_read(|db| codync_mem::store::get(db, id)).unwrap().is_some());
}

const NOTE: &str = "---\ndescription: Preferências\n---\n- Prefiro chá de manhã.\n";

#[test]
fn revoked_device_cannot_clear_facts_and_files() {
    let f = Fixture::new();
    f.local("saveMemory", json!({"text":"Prefiro chá de manhã."}));
    f.local("saveMemoryFile", json!({"file":"people.md","content":NOTE}));
    f.revoked(&[("clearMemory", json!({}))]);
}

#[test]
fn revoked_device_cannot_save_delete_or_restore_memory_file() {
    let f = Fixture::new();
    let saved = f.local("saveMemoryFile", json!({"file":"people.md","content":NOTE}));
    let hash = codync_mem::files::hash(NOTE);
    f.revoked(&[
        ("saveMemoryFile", json!({"file":"people.md","content":"Outra nota pessoal.","base":hash})),
        ("deleteMemoryFile", json!({"file":"people.md","base":hash})),
        ("restoreMemoryFile", json!({"file":"people.md","version":saved["version"]["id"],"base":hash})),
    ]);
}

#[test]
fn revoked_device_cannot_undo_dream_facts_or_files() {
    use codync_mem::dream;
    let f = Fixture::new();
    let file = f.local("saveMemoryFile", json!({"file":"people.md","content":NOTE}));
    let id = f
        .hub
        .store
        .memory_write(|db, now, rev| {
            let snapshot = dream::snapshot(db, &f.bot, false, "2026-11-07")?;
            let plan = dream::parse(
                r#"{"add":[{"text":"Prefiro chá de manhã.","quote":"Prefiro chá de manhã.","message":"m1"}]}"#,
            )
            .unwrap();
            let messages = vec![dream::Message {
                id: "human-d424".into(),
                from_user: true,
                text: vec!["Prefiro chá de manhã.".into()],
                date: "2026-11-07".into(),
            }];
            let dream::Outcome::Changed(dream) = dream::apply(db, &snapshot, &plan, &messages, now, rev)? else {
                panic!("o produtor deve criar o sonho");
            };
            dream::record_file(db, dream.id, file["version"]["id"].as_i64().unwrap())?;
            Ok(dream.id)
        })
        .unwrap();
    f.revoked(&[("undoDream", json!({"id":id}))]);
    f.local("undoDream", json!({"id":id}));
    assert!(!super::files::folder(Some(&f.bot)).join("people.md").exists());
    assert_eq!(f.hub.store.memory_read(|db| codync_mem::store::layer(db, Some(&f.bot))).unwrap(), []);
}

#[test]
fn revoked_device_cannot_undo_legacy_memory_card() {
    use crate::store::{EntryKind, Lane};
    let f = Fixture::new();
    let memory = f.local("saveMemory", json!({"text":"Prefiro chá de manhã."}))["entry"].clone();
    let card = f
        .hub
        .add_entry(
            &Lane::main(&f.bot),
            EntryKind::Notice,
            0,
            &json!({
                "author":f.bot,"style":"info","text":"Lembrei",
                "memory":{"action":"remember","text":"Prefiro chá de manhã.","status":"done",
                    "entry":memory["id"],"rev":memory["rev"]}
            }),
        )
        .unwrap();
    f.revoked(&[("undoMemory", json!({"entryId":card.id}))]);
    let undone = f.local("undoMemory", json!({"entryId":card.id}));
    assert_eq!(undone["entry"]["data"]["memory"]["status"], "undone");
}

#[test]
fn revoked_device_cannot_undo_legacy_file_card() {
    use crate::store::{EntryKind, Lane};
    let f = Fixture::new();
    let file = f.local("saveMemoryFile", json!({"file":"people.md","content":NOTE}));
    let card = f
        .hub
        .add_entry(
            &Lane::main(&f.bot),
            EntryKind::Notice,
            0,
            &json!({
                "author":f.bot,"style":"info","text":"Updated people.md",
                "memoryFile":{"file":"people.md","shared":false,"status":"done","diff":"+ Nota",
                    "version":file["version"]["id"]}
            }),
        )
        .unwrap();
    f.revoked(&[("undoMemory", json!({"entryId":card.id}))]);
    let undone = f.local("undoMemory", json!({"entryId":card.id}));
    assert_eq!(undone["entry"]["data"]["memoryFile"]["status"], "undone");
    assert!(!super::files::folder(Some(&f.bot)).join("people.md").exists());
}

#[test]
fn user_journal_survives_projection_failure_and_scan_restores_it() {
    let f = Fixture::new();
    let folder = super::files::folder(Some(&f.bot));
    let path = folder.join("people.md");
    std::fs::create_dir_all(&path).unwrap();
    let saved = f.local("saveMemoryFile", json!({"file":"people.md","content":NOTE}));
    assert!(saved["version"]["id"].is_i64(), "falha na projeção não transforma commit em erro");
    assert!(path.is_dir(), "colisão bloqueou a projeção");
    assert_eq!(
        f.hub.store.memory_read(|db| codync_mem::files::current(db, Some(&f.bot), "people.md")).unwrap().as_deref(),
        Some(NOTE)
    );
    std::fs::remove_dir(&path).unwrap();
    let cfg = f.hub.store.bot(&f.bot).unwrap().unwrap().config;
    super::files::scan(&f.hub, &cfg);
    assert_eq!(std::fs::read(&path).unwrap(), NOTE.as_bytes());
    let versions =
        f.hub.store.memory_read(|db| codync_mem::files::versions(db, Some(&f.bot), "people.md", 20)).unwrap();
    assert_eq!(versions.len(), 1, "reconciliação não cria versão nova");
    assert_eq!(versions[0].author, codync_mem::files::Author::User);
    assert_eq!(versions[0].id, saved["version"]["id"].as_i64().unwrap());
}

#[tokio::test]
async fn legacy_memory_answers_remain_refused_without_effects() {
    use crate::store::{EntryKind, Lane};
    let f = Fixture::new();
    let caller = f.device();
    for data in [
        json!({"memory":{"confirmation":false}}),
        json!({"memory":{"confirmation":true}}),
        json!({"memoryFile":{"file":"people.md"}}),
    ] {
        let card = f.hub.add_entry(&Lane::main(&f.bot), EntryKind::Notice, 0, &data).unwrap();
        let before = f.snapshot();
        for accept in [true, false] {
            let error = crate::api::dispatch(
                &f.hub,
                &caller,
                "answerMemory",
                f.body(json!({
                    "entryId":card.id,"entryRev":card.rev,"accept":accept
                })),
            )
            .await
            .unwrap_err();
            assert!(format!("{error:#}").contains("Confirmações foram substituídas"));
            assert_eq!(f.snapshot(), before);
        }
    }
}
