use super::*;

#[test]
fn exclusao_de_space_preserva_banco_sem_coluna_de_space() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE kv(k TEXT PRIMARY KEY,v TEXT NOT NULL); CREATE TABLE donna_bot_sections(id TEXT PRIMARY KEY,name TEXT NOT NULL,position INTEGER NOT NULL); CREATE TABLE donna_bot_section_assignments(bot_id TEXT PRIMARY KEY,section_id TEXT NOT NULL); INSERT INTO donna_bot_sections VALUES('p','Pessoal',0); INSERT INTO donna_bot_section_assignments VALUES('bot','p'); INSERT INTO kv VALUES('donna.botSections.rev','9'),('donna.botSections.rev.work','10');").unwrap();
    legado::delete_space(&db, "work").unwrap();
    assert_eq!(db.query_row("SELECT COUNT(*) FROM donna_bot_sections", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM donna_bot_section_assignments", [], |r| r.get::<_, i64>(0)).unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM kv WHERE k='donna.botSections.rev'", [], |r| r.get::<_, i64>(0)).unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM kv WHERE k='donna.botSections.rev.work'", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn exclusao_sem_tabelas_antigas_nao_falha() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE kv(k TEXT PRIMARY KEY,v TEXT NOT NULL);").unwrap();
    legado::delete_space(&db, "work").unwrap();
}
