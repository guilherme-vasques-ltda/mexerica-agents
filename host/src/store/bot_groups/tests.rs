use super::*;
use crate::fork::spaces::PERSONAL;
use serde_json::json;

fn store() -> Store {
    Store::open(std::path::Path::new(":memory:")).unwrap()
}
fn bot(store: &Store, id: &str) {
    let config = serde_json::from_value(json!({"id":id,"name":id,"backend":"fixture","cwd":"/tmp"})).unwrap();
    store.save_bot(&config).unwrap();
}
#[test]
fn names_membership_and_revision() {
    let store = store();
    bot(&store, "a");
    bot(&store, "b");
    let (first, id, _) =
        store.edit_bot_groups(PERSONAL, Edit::Add { name: "  Pesquisa  ".into(), bot_id: Some("a".into()) }).unwrap();
    let id = id.unwrap();
    assert_eq!(first.groups[0].name, "Pesquisa");
    assert_eq!(first.groups[0].bots, ["a"]);
    let error = store.edit_bot_groups(PERSONAL, Edit::Add { name: "pésquisa".into(), bot_id: None }).unwrap_err();
    assert_eq!(crate::api::error_code(&error), Some("name_taken"));
    assert_eq!(
        crate::api::error_code(
            &store.edit_bot_groups(PERSONAL, Edit::Rename { id: id.clone(), name: "a\nb".into() }).unwrap_err()
        ),
        Some("name_invalid")
    );
    assert_eq!(
        crate::api::error_code(
            &store.edit_bot_groups(PERSONAL, Edit::Add { name: "a\u{034f}b".into(), bot_id: None }).unwrap_err()
        ),
        Some("name_invalid")
    );
    let (second, _, _) =
        store.edit_bot_groups(PERSONAL, Edit::Place { bot_id: "b".into(), group_id: Some(id.clone()) }).unwrap();
    assert!(second.rev > first.rev);
    let (last, _, affected) = store.edit_bot_groups(PERSONAL, Edit::Remove { id }).unwrap();
    assert_eq!(last.groups.len(), 0);
    assert_eq!(affected, ["a", "b"]);
    assert_ne!(store.bot("a").unwrap().unwrap().rev, store.bot("b").unwrap().unwrap().rev);
}
#[test]
fn upgrade_direto_copia_e_contrai_reabrindo_sem_efeito() {
    let store = store();
    bot(&store, "a");
    let db = store.db.locked();
    db.execute_batch("DELETE FROM kv WHERE k='bot_groups.migrated'; CREATE TABLE donna_bot_sections(id TEXT PRIMARY KEY,name TEXT NOT NULL,position INTEGER NOT NULL); CREATE TABLE donna_bot_section_assignments(bot_id TEXT PRIMARY KEY,section_id TEXT NOT NULL); INSERT INTO donna_bot_sections VALUES('g1','Pesquisa',0),('g2','Pésquisa',1),('g3','',2); INSERT INTO donna_bot_section_assignments VALUES('a','g1'); INSERT INTO kv(k,v) VALUES('donna.botSections.rev','42');").unwrap();
    // N−1 → N+1 direto: copia e apaga na mesma abertura.
    legado::migrate(&db).unwrap();
    // Reabrir duas vezes dá o mesmo resultado.
    legado::migrate(&db).unwrap();
    let snap = snapshot(&db, PERSONAL).unwrap();
    assert_eq!(snap.rev, 42);
    assert_eq!(snap.groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["Pesquisa", "Pésquisa (2)", "Grupo"]);
    assert_eq!(snap.groups[0].bots, ["a"]);
    assert!(
        !db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='donna_bot_sections')", [], |r| r
            .get::<_, bool>(0))
            .unwrap()
    );
    assert!(
        !db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='donna_bot_section_assignments')",
            [],
            |r| r.get::<_, bool>(0)
        )
        .unwrap()
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM kv WHERE k='donna.botSections.rev' OR k GLOB 'donna.botSections.rev.*'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    // N+1 → N → N+1: o binário N reabre (só copia, não apaga) sem recriar nada,
    // e a N+1 seguinte mantém as edições feitas na N.
    db.execute("UPDATE bot_groups SET name='Editado na N' WHERE id='g1'", []).unwrap();
    legado::copy(&db).unwrap();
    assert!(migrated(&db).unwrap());
    legado::migrate(&db).unwrap();
    assert_eq!(snapshot(&db, PERSONAL).unwrap().groups[0].name, "Editado na N");
    let ords = db
        .prepare("SELECT ord FROM bot_groups WHERE space_id=?1 ORDER BY ord")
        .unwrap()
        .query_map([PERSONAL], |r| r.get::<_, i64>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(ords, [0, 1, 2]);
    drop(db);
    let (edited, _, _) =
        store.edit_bot_groups(PERSONAL, Edit::Rename { id: "g1".into(), name: "Outro".into() }).unwrap();
    assert!(edited.rev > 42);
}

#[test]
fn edicoes_feitas_na_n_sobrevivem_a_contracao() {
    let store = store();
    bot(&store, "a");
    let db = store.db.locked();
    db.execute_batch("DELETE FROM kv WHERE k='bot_groups.migrated'; CREATE TABLE donna_bot_sections(id TEXT PRIMARY KEY,name TEXT NOT NULL,position INTEGER NOT NULL); INSERT INTO donna_bot_sections VALUES('g','Pesquisa',0);").unwrap();
    // Abertura da N: só copia, sem apagar as tabelas antigas.
    legado::copy(&db).unwrap();
    assert_eq!(db.query_row("SELECT COUNT(*) FROM donna_bot_sections", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    drop(db);
    // Edição feita com o binário N rodando.
    let (edited, created, _) =
        store.edit_bot_groups(PERSONAL, Edit::Add { name: "Novo da N".into(), bot_id: Some("a".into()) }).unwrap();
    assert!(created.is_some() && edited.groups.len() == 2);
    let db = store.db.locked();
    // Abertura da N+1: a cópia já vale, então só apaga.
    legado::migrate(&db).unwrap();
    assert_eq!(snapshot(&db, PERSONAL).unwrap().groups.len(), 2);
    assert!(
        !db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='donna_bot_sections')", [], |r| r
            .get::<_, bool>(0))
            .unwrap()
    );
}
#[test]
fn failed_copy_retries_after_deleted_bot() {
    let dir = crate::store::private_fixture_dir("groups-migration-retry");
    let path = dir.join("fixture.db");
    let store = Store::open(&path).unwrap();
    bot(&store, "a");
    bot(&store, "b");
    let db = store.db.locked();
    db.execute_batch("DELETE FROM kv WHERE k='bot_groups.migrated'; CREATE TABLE donna_bot_sections(id TEXT PRIMARY KEY,name TEXT NOT NULL,position INTEGER NOT NULL); CREATE TABLE donna_bot_section_assignments(bot_id TEXT PRIMARY KEY,section_id TEXT NOT NULL); INSERT INTO donna_bot_sections VALUES('g','Grupo',0); INSERT INTO donna_bot_section_assignments VALUES('a','g'),('b','g'); CREATE TRIGGER fail_group BEFORE INSERT ON bot_groups BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    legado::migrate(&db).unwrap();
    assert!(!migrated(&db).unwrap());
    assert_eq!(snapshot(&db, PERSONAL).unwrap().groups.len(), 0);
    // A cópia falhou: nada da versão anterior é apagado.
    assert_eq!(db.query_row("SELECT COUNT(*) FROM donna_bot_sections", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    drop(db);
    assert_eq!(
        crate::api::error_code(
            &store.edit_bot_groups(PERSONAL, Edit::Add { name: "Novo".into(), bot_id: None }).unwrap_err()
        ),
        Some("migrating")
    );
    store.delete_bot("a").unwrap();
    store.discard_team_creation("b", "pending", "{}").unwrap();
    store.db.locked().execute_batch("DROP TRIGGER fail_group;").unwrap();
    drop(store);
    let reopened = Store::open(&path).unwrap();
    let db = reopened.db.locked();
    assert!(migrated(&db).unwrap());
    assert_eq!(snapshot(&db, PERSONAL).unwrap().groups[0].bots.len(), 0);
    assert!(
        !db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='donna_bot_sections')", [], |r| r
            .get::<_, bool>(0))
            .unwrap()
    );
    drop(db);
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn legacy_duplicate_suffix_avoids_existing_forty_character_name() {
    let store = store();
    let db = store.db.locked();
    db.execute_batch("DELETE FROM kv WHERE k='bot_groups.migrated'; CREATE TABLE donna_bot_sections(id TEXT PRIMARY KEY,name TEXT NOT NULL,position INTEGER NOT NULL,space_id TEXT); CREATE TABLE donna_bot_section_assignments(bot_id TEXT PRIMARY KEY,section_id TEXT NOT NULL);").unwrap();
    let long = "a".repeat(40);
    let existing = format!("{} (2)", "a".repeat(36));
    for (id, name, position) in [("a", long.as_str(), 0), ("b", long.as_str(), 1), ("c", existing.as_str(), 2)] {
        db.execute("INSERT INTO donna_bot_sections VALUES(?1,?2,?3,?4)", params![id, name, position, PERSONAL])
            .unwrap();
    }
    db.execute("INSERT INTO donna_bot_sections VALUES('missing','Inacessível',3,'inexistente')", []).unwrap();
    legado::migrate(&db).unwrap();
    let state = snapshot(&db, PERSONAL).unwrap();
    assert_eq!(state.groups.len(), 3);
    assert_eq!(state.groups[0].name, long);
    assert_eq!(state.groups[1].name, format!("{} (3)", "a".repeat(36)));
    assert_eq!(state.groups[2].name, existing);
}

#[test]
fn fresh_database_marks_copy_without_creating_legacy_tables() {
    let store = store();
    let db = store.db.locked();
    assert!(migrated(&db).unwrap());
    assert!(
        !db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='donna_bot_sections')", [], |r| r
            .get::<_, bool>(0))
            .unwrap()
    );
    assert_eq!(snapshot(&db, PERSONAL).unwrap().rev, 0);
}

#[test]
fn migration_copies_only_live_members_in_their_space_and_max_revision() {
    let store = store();
    let mut spaces = store.spaces().unwrap();
    let mut work = spaces.spaces[0].clone();
    work.id = "work".into();
    work.kind = "space".into();
    work.name = "Trabalho".into();
    work.position = 1;
    spaces.spaces.push(work);
    store.kv_set("donna.spaces.v1", &serde_json::to_string(&spaces).unwrap()).unwrap();
    bot(&store, "personal");
    let config = serde_json::from_value(
        json!({"id":"worker","name":"worker","spaceId":"work","backend":"fixture","cwd":"/tmp"}),
    )
    .unwrap();
    store.save_bot(&config).unwrap();
    bot(&store, "deleted");
    store.delete_bot("deleted").unwrap();
    let db = store.db.locked();
    db.execute_batch("DELETE FROM kv WHERE k='bot_groups.migrated'; CREATE TABLE donna_bot_sections(id TEXT PRIMARY KEY,name TEXT NOT NULL,position INTEGER NOT NULL,space_id TEXT); CREATE TABLE donna_bot_section_assignments(bot_id TEXT PRIMARY KEY,section_id TEXT NOT NULL); INSERT INTO donna_bot_sections VALUES('p','P',0,'personal'),('w','W',0,'work'),('gone','X',0,'missing'); INSERT INTO donna_bot_section_assignments VALUES('personal','w'),('worker','w'),('deleted','p'),('ghost','p'); INSERT INTO kv(k,v) VALUES('donna.botSections.rev.work','41'),('bot_groups.rev.work','99');").unwrap();
    // O identificador pessoal não é presumido pela fixture.
    db.execute("UPDATE donna_bot_sections SET space_id=?1 WHERE id='p'", [PERSONAL]).unwrap();
    legado::migrate(&db).unwrap();
    let work = snapshot(&db, "work").unwrap();
    assert_eq!(work.rev, 99);
    assert_eq!(work.groups[0].bots, ["worker"]);
    assert_eq!(snapshot(&db, PERSONAL).unwrap().groups[0].bots.len(), 0);
    assert_eq!(db.query_row("SELECT COUNT(*) FROM bot_groups WHERE id='gone'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}
