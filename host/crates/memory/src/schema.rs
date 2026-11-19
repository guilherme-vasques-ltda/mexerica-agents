//! The crate's own tables and their migrations, versioned apart from the caller's schema.

use rusqlite::{Connection, OptionalExtension};

const MIGRATIONS: &[&str] = &[
    // 1: entries, tombstones and the dream log.
    "CREATE TABLE memory_entries (
        id INTEGER PRIMARY KEY,
        bot TEXT,
        state TEXT NOT NULL,
        text TEXT NOT NULL,
        author TEXT NOT NULL,
        author_bot TEXT,
        pinned INTEGER NOT NULL DEFAULT 0,
        superseded_by INTEGER,
        room TEXT,
        evidence TEXT NOT NULL DEFAULT '',
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        rev INTEGER NOT NULL
    );
    CREATE INDEX memory_entries_bot ON memory_entries (bot, state);
    CREATE TABLE memory_tombstones (
        scope TEXT NOT NULL,
        hash TEXT NOT NULL,
        PRIMARY KEY (scope, hash)
    ) WITHOUT ROWID;
    CREATE TABLE memory_dreams (
        id INTEGER PRIMARY KEY,
        bot TEXT NOT NULL,
        at INTEGER NOT NULL,
        undone INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX memory_dreams_bot ON memory_dreams (bot, at);
    CREATE TABLE memory_dream_changes (
        dream INTEGER NOT NULL,
        entry INTEGER NOT NULL,
        change TEXT NOT NULL
    );
    CREATE INDEX memory_dream_changes_dream ON memory_dream_changes (dream);",
    // 2: long-form files (the layer's key is '' for the shared one), their versions, and the ones dreaming wrote.
    "CREATE TABLE memory_files (
        bot TEXT NOT NULL,
        name TEXT NOT NULL,
        hash TEXT NOT NULL,
        size INTEGER NOT NULL,
        mtime INTEGER NOT NULL,
        description TEXT,
        tokens INTEGER NOT NULL,
        version INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        rev INTEGER NOT NULL,
        PRIMARY KEY (bot, name)
    ) WITHOUT ROWID;
    CREATE TABLE memory_file_versions (
        id INTEGER PRIMARY KEY,
        bot TEXT NOT NULL,
        name TEXT NOT NULL,
        content TEXT,
        author TEXT NOT NULL,
        author_bot TEXT,
        cause TEXT NOT NULL,
        at INTEGER NOT NULL,
        rev INTEGER NOT NULL
    );
    CREATE INDEX memory_file_versions_file ON memory_file_versions (bot, name, id);
    CREATE TABLE memory_dream_files (
        dream INTEGER NOT NULL,
        version INTEGER NOT NULL
    );
    CREATE INDEX memory_dream_files_dream ON memory_dream_files (dream);",
    // 3: entry ids are never reused, so an old undo line, a dream's undo or a bot's stale id can't reach a newer
    // entry. Ids the dream log still names count as used.
    "CREATE TABLE memory_entries_new (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        bot TEXT,
        state TEXT NOT NULL,
        text TEXT NOT NULL,
        author TEXT NOT NULL,
        author_bot TEXT,
        pinned INTEGER NOT NULL DEFAULT 0,
        superseded_by INTEGER,
        room TEXT,
        evidence TEXT NOT NULL DEFAULT '',
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        rev INTEGER NOT NULL
    );
    INSERT INTO memory_entries_new SELECT * FROM memory_entries;
    DROP TABLE memory_entries;
    ALTER TABLE memory_entries_new RENAME TO memory_entries;
    CREATE INDEX memory_entries_bot ON memory_entries (bot, state);
    DELETE FROM sqlite_sequence WHERE name = 'memory_entries';
    INSERT INTO sqlite_sequence (name, seq) SELECT 'memory_entries', max(
        coalesce((SELECT max(id) FROM memory_entries), 0),
        coalesce((SELECT max(entry) FROM memory_dream_changes), 0));",
];

/// The Donnabot fork's own migrations, counted apart (`memory_meta` key `donnabot`) so Codync's keep their numbers
/// and port as they come. Each can run again safely.
const DONNABOT: &[&str] = &[
    // 1: search that ignores case and accents, over active and archived entries.
    SEARCH_INDEX,
    // 2: origem por versão; registros sem prova continuam desconhecidos.
    "ALTER TABLE memory_entries ADD COLUMN origin TEXT NOT NULL DEFAULT '{}';
     ALTER TABLE memory_file_versions ADD COLUMN origin TEXT NOT NULL DEFAULT '{}';
     CREATE TABLE donna_memory_versions(entry INTEGER NOT NULL,rev INTEGER NOT NULL,text TEXT NOT NULL,
        state TEXT NOT NULL,origin TEXT NOT NULL,evidence TEXT NOT NULL,PRIMARY KEY(entry,rev));
     CREATE TRIGGER donna_memory_version_insert AFTER INSERT ON memory_entries BEGIN
        INSERT OR IGNORE INTO donna_memory_versions VALUES(new.id,new.rev,new.text,new.state,new.origin,new.evidence);
     END;
     CREATE TRIGGER donna_memory_version_update BEFORE UPDATE OF rev ON memory_entries BEGIN
        INSERT OR IGNORE INTO donna_memory_versions VALUES(old.id,old.rev,old.text,old.state,old.origin,old.evidence);
     END;
     CREATE TABLE donna_memory_fact_blocks(bot TEXT NOT NULL,fact TEXT NOT NULL,message TEXT NOT NULL,
        PRIMARY KEY(bot,fact,message));",
    // 3: natureza independente da origem, conservada em cada versão.
    "ALTER TABLE memory_entries ADD COLUMN nature TEXT NOT NULL DEFAULT 'unknown';
     ALTER TABLE memory_entries ADD COLUMN nature_bot TEXT;
     ALTER TABLE memory_entries ADD COLUMN conflict_text TEXT;
     ALTER TABLE memory_file_versions ADD COLUMN nature TEXT NOT NULL DEFAULT 'unknown';
     ALTER TABLE memory_file_versions ADD COLUMN nature_bot TEXT;
     ALTER TABLE memory_file_versions ADD COLUMN conflict_text TEXT;
     ALTER TABLE donna_memory_versions ADD COLUMN nature TEXT NOT NULL DEFAULT 'unknown';
     ALTER TABLE donna_memory_versions ADD COLUMN nature_bot TEXT;
     ALTER TABLE donna_memory_versions ADD COLUMN conflict_text TEXT;
     ALTER TABLE donna_memory_versions ADD COLUMN at INTEGER NOT NULL DEFAULT 0;
     DROP TRIGGER donna_memory_version_insert;
     DROP TRIGGER donna_memory_version_update;
     CREATE TRIGGER donna_memory_version_insert AFTER INSERT ON memory_entries BEGIN
       INSERT OR IGNORE INTO donna_memory_versions(entry,rev,text,state,origin,evidence,nature,nature_bot,conflict_text,at)
       VALUES(new.id,new.rev,new.text,new.state,new.origin,new.evidence,new.nature,new.nature_bot,new.conflict_text,new.updated_at);
     END;
     CREATE TRIGGER donna_memory_version_update BEFORE UPDATE OF rev ON memory_entries BEGIN
       INSERT OR IGNORE INTO donna_memory_versions(entry,rev,text,state,origin,evidence,nature,nature_bot,conflict_text,at)
       VALUES(old.id,old.rev,old.text,old.state,old.origin,old.evidence,old.nature,old.nature_bot,old.conflict_text,old.updated_at);
     END;",
    // 4: linhagem fato/nota → mensagem (sono 2b1); o SQL mora em `lineage.rs`, os gatilhos em `lineage::ensure`.
    crate::lineage::BACKFILL,
    // 5: perfil derivado e reforços/arquivo sem apagar fatos ou versões.
    crate::lifecycle::SCHEMA,
];

/// An FTS5 trigram index of the entries' text, kept by triggers.
const SEARCH_INDEX: &str = "CREATE VIRTUAL TABLE IF NOT EXISTS memory_entries_fts USING fts5(
        text, content = 'memory_entries', content_rowid = 'id', tokenize = 'trigram remove_diacritics 1'
    );
    CREATE TRIGGER IF NOT EXISTS memory_entries_fts_insert AFTER INSERT ON memory_entries BEGIN
        INSERT INTO memory_entries_fts (rowid, text) VALUES (new.id, new.text);
    END;
    CREATE TRIGGER IF NOT EXISTS memory_entries_fts_delete AFTER DELETE ON memory_entries BEGIN
        INSERT INTO memory_entries_fts (memory_entries_fts, rowid, text) VALUES ('delete', old.id, old.text);
    END;
    CREATE TRIGGER IF NOT EXISTS memory_entries_fts_update AFTER UPDATE OF text ON memory_entries BEGIN
        INSERT INTO memory_entries_fts (memory_entries_fts, rowid, text) VALUES ('delete', old.id, old.text);
        INSERT INTO memory_entries_fts (rowid, text) VALUES (new.id, new.text);
    END;
    INSERT INTO memory_entries_fts (memory_entries_fts) VALUES ('rebuild');";

/// Brings the crate's tables up to date. Safe to call on every start.
pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("CREATE TABLE IF NOT EXISTS memory_meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL)", [])?;
    let done: i64 = conn
        .query_row("SELECT value FROM memory_meta WHERE key = 'schema'", [], |row| row.get(0))
        .optional()?
        .unwrap_or(0);
    for (version, sql) in (1_i64..).zip(MIGRATIONS).filter(|(version, _)| *version > done) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.execute(
            "INSERT INTO memory_meta (key, value) VALUES ('schema', ?1)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [version],
        )?;
        tx.commit()?;
    }
    donnabot(conn)
}

/// The fork's migrations, after Codync's. A Codync migration that rebuilds `memory_entries` drops the search index's
/// triggers with it, so the index is set up again whenever one is missing.
fn donnabot(conn: &Connection) -> rusqlite::Result<()> {
    let done: i64 = conn
        .query_row("SELECT value FROM memory_meta WHERE key = 'donnabot'", [], |row| row.get(0))
        .optional()?
        .unwrap_or(0);
    for (version, sql) in (1_i64..).zip(DONNABOT).filter(|(version, _)| *version > done) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.execute(
            "INSERT INTO memory_meta (key, value) VALUES ('donnabot', ?1)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [version],
        )?;
        tx.commit()?;
    }
    let triggers: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'memory_entries_fts_%'",
        [],
        |row| row.get(0),
    )?;
    crate::lineage::ensure(conn)?;
    if triggers < 3 {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(SEARCH_INDEX)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_once() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        let version: i64 =
            conn.query_row("SELECT value FROM memory_meta WHERE key = 'schema'", [], |row| row.get(0)).unwrap();
        assert_eq!(version, i64::try_from(MIGRATIONS.len()).unwrap());
        let ours: i64 =
            conn.query_row("SELECT value FROM memory_meta WHERE key = 'donnabot'", [], |row| row.get(0)).unwrap();
        assert_eq!(ours, i64::try_from(DONNABOT.len()).unwrap());
    }

    #[test]
    fn d539_upgrade_is_atomic_and_preserves_existing_facts() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE memory_meta(key TEXT PRIMARY KEY,value INTEGER NOT NULL)").unwrap();
        conn.execute_batch(&MIGRATIONS.concat()).unwrap();
        conn.execute("INSERT INTO memory_meta VALUES('schema',?1)", [i64::try_from(MIGRATIONS.len()).unwrap()])
            .unwrap();
        conn.execute_batch(&DONNABOT[..4].concat()).unwrap();
        conn.execute("INSERT INTO memory_meta VALUES('donnabot',4)", []).unwrap();
        let id = insert(&conn);
        // Falha ao confirmar a versão faz a migração inteira voltar, incluindo cache e gatilhos.
        conn.execute_batch("CREATE TRIGGER fail_d539 BEFORE UPDATE ON memory_meta WHEN new.key='donnabot' AND new.value=5 BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        assert!(migrate(&conn).is_err());
        let cache_exists: bool = conn
            .query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='donna_memory_profiles')", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(!cache_exists);
        conn.execute("DROP TRIGGER fail_d539", []).unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        assert_eq!(crate::store::get(&conn, id).unwrap().unwrap().text, "x");
    }

    #[test]
    fn the_fork_indexes_what_is_there_and_sets_the_index_up_again() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE memory_meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL)").unwrap();
        conn.execute_batch(&MIGRATIONS.concat()).unwrap();
        conn.execute("INSERT INTO memory_meta (key, value) VALUES ('schema', ?1)", [MIGRATIONS.len().to_string()])
            .unwrap();
        let add = |text: &str| {
            conn.execute(
                "INSERT INTO memory_entries (state, text, author, created_at, updated_at, rev)
                 VALUES ('active', ?1, 'user', 0, 0, 0)",
                [text],
            )
            .unwrap();
            conn.last_insert_rowid()
        };
        let found = |query: &str| -> Vec<i64> {
            let mut stmt =
                conn.prepare("SELECT rowid FROM memory_entries_fts WHERE memory_entries_fts MATCH ?1").unwrap();
            stmt.query_map([query], |row| row.get(0)).unwrap().map(Result::unwrap).collect()
        };
        let before = add("Mora em São Paulo");
        migrate(&conn).unwrap();
        assert_eq!(found("\"sao paulo\""), [before], "entries from before the fork's migration");
        // As if a Codync migration rebuilt the table: the triggers went with it.
        conn.execute_batch("DROP TRIGGER memory_entries_fts_insert").unwrap();
        migrate(&conn).unwrap();
        let after = add("Trabalha em Curitiba");
        assert_eq!(found("curitiba"), [after]);
    }

    fn insert(conn: &Connection) -> i64 {
        conn.execute(
            "INSERT INTO memory_entries (state, text, author, created_at, updated_at, rev)
             VALUES ('active', 'x', 'user', 0, 0, 0)",
            [],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn entry_ids_are_not_reused() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let first = insert(&conn);
        conn.execute("DELETE FROM memory_entries WHERE id = ?1", [first]).unwrap();
        assert!(insert(&conn) > first);
    }

    #[test]
    fn upgrading_keeps_entries_and_skips_ids_the_dream_log_names() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE memory_meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL)").unwrap();
        conn.execute_batch(&MIGRATIONS[..2].concat()).unwrap();
        conn.execute("INSERT INTO memory_meta (key, value) VALUES ('schema', 2)", []).unwrap();
        let kept = insert(&conn);
        conn.execute("INSERT INTO memory_dream_changes (dream, entry, change) VALUES (1, 7, 'added')", []).unwrap();
        migrate(&conn).unwrap();
        let text: String =
            conn.query_row("SELECT text FROM memory_entries WHERE id = ?1", [kept], |row| row.get(0)).unwrap();
        assert_eq!(text, "x");
        assert_eq!(insert(&conn), 8);
    }
}
