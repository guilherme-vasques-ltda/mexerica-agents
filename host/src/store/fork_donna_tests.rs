use super::*;
use std::path::{Path, PathBuf};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("donna-default-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn open(&self) -> Store {
        Store::open(&self.0.join("test.db")).unwrap()
    }
    fn initialize(&self, store: &Store) -> Result<()> {
        store.initialize_donna(|cfg| {
            let dir = self.0.join(&cfg.id);
            std::fs::create_dir_all(&dir)?;
            cfg.cwd = dir.to_string_lossy().into_owned();
            Ok(())
        })
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn default_donna_is_primary_safe_and_durable_without_an_ai_account() {
    let f = Fixture::new();
    let store = f.open();
    f.initialize(&store).unwrap();
    let rows = store.bots().unwrap();
    assert_eq!(rows.len(), 1);
    let cfg = &rows[0].config;
    let id = cfg.id.clone();
    assert_eq!(cfg.name, "Berga");
    assert_eq!(cfg.description, PROFILE);
    assert_eq!(cfg.backend, "codex");
    assert!(cfg.model.is_none() && cfg.command.is_none() && cfg.model_options.is_empty());
    assert_eq!(cfg.space_id, crate::fork::spaces::PERSONAL);
    assert!(cfg.manage_team && !cfg.hidden && !cfg.auto_name);
    assert_eq!(cfg.permission, super::super::Permission::Ask);
    assert!(!cfg.permission_chosen_by_user && !cfg.computer && !cfg.harness_memory && !cfg.fast_mode);
    assert!(cfg.connectors.is_empty() && cfg.logins.is_empty() && cfg.skills.is_empty());
    assert!(cfg.fork_editor.engine_account_id.is_none());
    assert_eq!(cfg.mexeriquinha, Some(crate::fork_avatars::principal_mexeriquinha()));
    assert!(Path::new(&cfg.cwd).is_dir());
    assert!(rows[0].session_id.is_none());
    assert_eq!(store.team_settings().unwrap().primary_bot_id.as_deref(), Some(id.as_str()));
    assert!(crate::chat::manage::enabled(&store, cfg));
    let notice: (String, String) = store
        .db
        .locked()
        .query_row("SELECT kind,data FROM entries WHERE bot_id=?1", [&id], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!(notice.0, "notice", "orientação do Hub, sem turno humano nem fala fabricada da IA");
    let data = serde_json::from_str::<serde_json::Value>(&notice.1).unwrap();
    let text = data["text"].as_str().unwrap().to_owned();
    if crate::environment::Environment::is_open() {
        assert_eq!(text, OPEN_SETUP_NOTICE);
        assert_eq!(data["code"], "open_setup", "a janela traduz o aviso pelo código");
        assert!(!text.contains("Motor"), "a Open não manda escolher conta em Motor");
    } else {
        assert_eq!(text, SETUP_NOTICE);
    }
    let rev = store.current_rev();
    drop(store);
    let store = f.open();
    f.initialize(&store).unwrap();
    assert_eq!(store.bots().unwrap().len(), 1);
    assert_eq!(store.kv_get(KEY).as_deref(), Some(id.as_str()));
    assert_eq!(store.current_rev(), rev);
}

#[test]
fn default_donna_never_undoes_rename_deselection_or_deletion() {
    let f = Fixture::new();
    let store = f.open();
    f.initialize(&store).unwrap();
    let mut cfg = store.bots().unwrap()[0].config.clone();
    cfg.name = "Nome escolhido".into();
    // D366: o Berga criado pelo Hub é fixo.
    assert!(store.save_bot(&cfg).is_err());
    assert!(store.set_team_settings(&json!({"primaryBotId": null})).is_err());
    assert!(store.delete_bot(&cfg.id).is_err());
    // Mecânica legada, sem a trava: o marcador D190 continua impedindo a recriação.
    store.sem_donna_fixa();
    store.save_bot(&cfg).unwrap();
    store.set_team_settings(&json!({"primaryBotId": null})).unwrap();
    f.initialize(&store).unwrap();
    assert_eq!(store.bot(&cfg.id).unwrap().unwrap().config.name, "Nome escolhido");
    assert!(store.team_settings().unwrap().primary_bot_id.is_none());
    store.delete_bot(&cfg.id).unwrap();
    drop(store);
    let store = f.open();
    f.initialize(&store).unwrap();
    assert!(store.bots().unwrap().iter().all(|b| b.deleted));
    assert!(store.team_settings().unwrap().primary_bot_id.is_none());
}

#[test]
fn existing_hubs_including_deleted_or_unreadable_bots_are_preserved() {
    for (config, deleted) in [(r#"{"id":"old","name":"Original","backend":"codex"}"#, 0), ("{}", 0), ("{}", 1)] {
        let f = Fixture::new();
        let store = f.open();
        store
            .db
            .locked()
            .execute("INSERT INTO bots(id,rev,deleted,config) VALUES('old',7,?1,?2)", params![deleted, config])
            .unwrap();
        let rev = store.current_rev();
        f.initialize(&store).unwrap();
        let after: (String, i64, i64) = store
            .db
            .locked()
            .query_row("SELECT config,deleted,rev FROM bots", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        assert_eq!(after, (config.to_owned(), deleted, 7));
        assert_eq!(store.kv_get(KEY).as_deref(), Some("preserved"));
        assert!(store.kv_get("team.settings.v1").is_none());
        assert_eq!(store.current_rev(), rev);
    }
    for prior_settings in ["{}", r#"{"primaryBotId":"deleted","permission":"auto"}"#, "unreadable"] {
        let f = Fixture::new();
        let store = f.open();
        store.kv_set("team.settings.v1", prior_settings).unwrap();
        f.initialize(&store).unwrap();
        assert!(store.bots().unwrap().is_empty());
        assert_eq!(store.kv_get("team.settings.v1").as_deref(), Some(prior_settings));
    }
}

#[test]
fn a_failed_initialization_rolls_back_bot_principal_notice_marker_and_revision() {
    let f = Fixture::new();
    let store = f.open();
    let rev = store.current_rev();
    store
        .db
        .locked()
        .execute_batch("CREATE TRIGGER fail_notice BEFORE INSERT ON entries BEGIN SELECT RAISE(ABORT,'fixture'); END;")
        .unwrap();
    assert!(f.initialize(&store).is_err());
    assert!(store.bots().unwrap().is_empty());
    assert!(store.kv_get(KEY).is_none());
    assert!(store.kv_get("team.settings.v1").is_none());
    assert_eq!(store.current_rev(), rev);
    store.db.locked().execute_batch("DROP TRIGGER fail_notice").unwrap();
    f.initialize(&store).unwrap();
    assert_eq!(store.bots().unwrap().len(), 1);
}

#[test]
fn concurrent_initializers_commit_only_one_donna() {
    let f = Fixture::new();
    let first = f.open();
    let second = f.open();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            barrier.wait();
            f.initialize(&first).unwrap();
        });
        let b = scope.spawn(|| {
            barrier.wait();
            f.initialize(&second).unwrap();
        });
        a.join().unwrap();
        b.join().unwrap();
    });
    assert_eq!(first.bots().unwrap().len(), 1);
    let count: i64 = first.db.locked().query_row("SELECT count(*) FROM entries", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 1);
}
