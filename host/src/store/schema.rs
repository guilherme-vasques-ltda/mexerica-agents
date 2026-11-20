// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Opening the database: tables, pragmas and schema migrations.

use super::Store;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Current schema version (kv `schema`).
const SCHEMA: i64 = 7;

/// 3: authorized devices (keys, not push tickets) with their push and Live Activity
/// tickets. The old ticket-only `devices` table is dropped; phones re-register on connect.
/// 4: threads (`entries.thread_id`); instruction snapshots are kept per session, so the
/// per-bot ones go.
/// 5: threads merge into their chats (quote replies took their place, see [`migrate_5`]).
/// 6: the dividers that announced a fresh agent session go; the chat no longer shows session resets.
/// 7: a `clientNonce` is unique per conversation and queued messages are indexed (see [`migrate_7`]).
fn migrate(c: &Connection) -> Result<()> {
    let version: i64 =
        c.query_row("SELECT CAST(v AS INTEGER) FROM kv WHERE k = 'schema'", [], |r| r.get(0)).optional()?.unwrap_or(0);
    if version >= SCHEMA {
        return Ok(());
    }
    if version < 3 {
        migrate_3(c)?;
    }
    if version < 4 {
        migrate_4(c)?;
    }
    if version < 5 {
        migrate_5(c)?;
    }
    if version < 6 {
        migrate_6(c)?;
    }
    migrate_7(c)
}

fn migrate_3(c: &Connection) -> Result<()> {
    c.execute_batch(
        "BEGIN;
         DROP TABLE IF EXISTS devices;
         CREATE TABLE devices(
            key TEXT PRIMARY KEY, name TEXT NOT NULL, platform TEXT NOT NULL,
            source TEXT NOT NULL, grant_id TEXT, scopes TEXT NOT NULL,
            lease_until INTEGER, created_at INTEGER NOT NULL, last_seen_at INTEGER);
         CREATE TABLE IF NOT EXISTS push_tickets(
            ticket TEXT PRIMARY KEY, device_key TEXT NOT NULL, push_key TEXT, ctx TEXT, name TEXT,
            created_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS activity_tickets(
            ticket TEXT PRIMARY KEY, bot_id TEXT NOT NULL, device_key TEXT NOT NULL, created_at INTEGER NOT NULL);
         INSERT INTO kv(k, v) VALUES('schema', '3') ON CONFLICT(k) DO UPDATE SET v = '3';
         COMMIT;",
    )?;
    Ok(())
}

fn migrate_4(c: &Connection) -> Result<()> {
    c.execute_batch(
        "BEGIN;
         ALTER TABLE entries ADD COLUMN thread_id TEXT;
         CREATE INDEX IF NOT EXISTS entries_thread ON entries(bot_id, thread_id, seq);
         DELETE FROM kv WHERE k GLOB 'context.*';
         INSERT INTO kv(k, v) VALUES('schema', '4') ON CONFLICT(k) DO UPDATE SET v = '4';
         COMMIT;",
    )?;
    Ok(())
}

/// Thread replies join their chat in time order. Each thread's first user reply quotes the message the thread was
/// on; a routine run's own reply stays in its trace, since its result is already in the chat, and the result shows
/// the run's trace. Thread sessions and read marks go (a routine run's session stays for its recovery). Rows whose
/// chat changes get new revs so synced clients pick them up, and keep their old one as `contentRev`. Thread trace
/// (thoughts, tool calls) keeps its rev: only the dropped column changed, and resending every run's trace would
/// flood each client's first catch-up.
fn migrate_5(c: &Connection) -> Result<()> {
    let tx = c.unchecked_transaction()?;
    let changed: Vec<String> = tx
        .prepare(
            "SELECT id FROM entries
             WHERE (thread_id IS NOT NULL AND (kind IN ('user', 'permission', 'notice')
                                               OR (kind = 'agent' AND json_extract(data, '$.final') = 1)))
               OR json_extract(data, '$.thread') IS NOT NULL OR id GLOB 'routine-result-*' ORDER BY seq",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    tx.execute_batch(
        // Without it each subquery below scans the whole table, for every row it is asked about.
        "CREATE INDEX migrate5_thread ON entries(thread_id, turn, kind);
         UPDATE entries SET data = json_set(data, '$.final', json('false'))
           WHERE thread_id GLOB 'routine-*' AND kind = 'agent' AND json_extract(data, '$.final') = 1
             AND NOT EXISTS (SELECT 1 FROM entries u WHERE u.thread_id = entries.thread_id AND u.turn = entries.turn
                               AND u.kind = 'user');
         UPDATE entries SET turn = COALESCE((SELECT MIN(t.turn) FROM entries t
                                             WHERE t.thread_id = 'routine-' || substr(entries.id, 16) AND t.turn > 0), turn)
           WHERE id GLOB 'routine-result-*';
         UPDATE entries SET data = json_set(data, '$.replyTo', json((SELECT json_object(
               'id', r.id,
               'author', CASE WHEN r.kind = 'user' THEN 'user' ELSE COALESCE(json_extract(r.data, '$.author'), r.bot_id) END,
               -- `replies::quote`'s text: the message's, or its files' names.
               'text', substr(COALESCE(NULLIF(json_extract(r.data, '$.text'), ''),
                                       (SELECT group_concat(json_extract(a.value, '$.name'), ', ')
                                          FROM json_each(r.data, '$.attachments') a), ''), 1, 500))
             FROM entries r WHERE r.id = entries.thread_id)))
           WHERE kind = 'user' AND EXISTS (SELECT 1 FROM entries r WHERE r.id = entries.thread_id)
             AND seq = (SELECT MIN(f.seq) FROM entries f WHERE f.thread_id = entries.thread_id AND f.kind = 'user');
         UPDATE entries SET data = json_remove(data, '$.thread') WHERE json_extract(data, '$.thread') IS NOT NULL;
         DROP INDEX migrate5_thread;
         DROP INDEX IF EXISTS entries_thread;
         ALTER TABLE entries DROP COLUMN thread_id;
         DELETE FROM kv WHERE k GLOB 'read.*' OR (k GLOB 'lane.*' AND NOT k GLOB 'lane.session.*/routine-*');
         INSERT INTO kv(k, v) VALUES('schema', '5') ON CONFLICT(k) DO UPDATE SET v = '5';",
    )?;
    for id in changed {
        let rev = super::next_rev(&tx)?;
        // A message the user read stays read (`contentRev`, as for a bot's reaction).
        tx.execute(
            "UPDATE entries SET rev = ?2, data = CASE WHEN json_extract(data, '$.contentRev') IS NULL
               THEN json_set(data, '$.contentRev', rev) ELSE data END WHERE id = ?1",
            rusqlite::params![id, rev],
        )?;
    }
    tx.commit()?;
    Ok(())
}

fn migrate_6(c: &Connection) -> Result<()> {
    c.execute_batch(
        "BEGIN;
         DELETE FROM entries WHERE kind = 'notice' AND json_extract(data, '$.style') = 'divider';
         INSERT INTO kv(k, v) VALUES('schema', '6') ON CONFLICT(k) DO UPDATE SET v = '6';
         COMMIT;",
    )?;
    Ok(())
}

/// A chave antiduplicata do envio passa a ser única por conversa. Base antiga com a mesma chave repetida conserva
/// todas as linhas: a mais antiga fica com a chave e as seguintes a guardam em `duplicateClientNonce`, sem apagar
/// nada nem supor que não tiveram efeito.
fn migrate_7(c: &Connection) -> Result<()> {
    c.execute_batch(
        "BEGIN;
         UPDATE entries SET data = json_set(data, '$.clientNonce', '',
                '$.duplicateClientNonce', json_extract(data, '$.clientNonce'))
          WHERE seq IN (SELECT seq FROM (
                SELECT seq, ROW_NUMBER() OVER (PARTITION BY bot_id, json_extract(data, '$.clientNonce') ORDER BY seq) AS n
                  FROM entries WHERE kind = 'user' AND json_valid(data)
                   AND COALESCE(json_extract(data, '$.clientNonce'), '') <> '')
              WHERE n > 1);
         CREATE UNIQUE INDEX IF NOT EXISTS entries_client_nonce
          ON entries(bot_id, (CASE WHEN json_valid(data) THEN json_extract(data, '$.clientNonce') END))
          WHERE kind = 'user' AND COALESCE(CASE WHEN json_valid(data) THEN json_extract(data, '$.clientNonce') END, '') <> '';
         CREATE INDEX IF NOT EXISTS entries_user_queued ON entries(bot_id)
          WHERE kind = 'user' AND (CASE WHEN json_valid(data) THEN json_extract(data, '$.status') END) = 'queued';
         INSERT INTO kv(k, v) VALUES('schema', '7') ON CONFLICT(k) DO UPDATE SET v = '7';
         COMMIT;",
    )?;
    Ok(())
}

/// Perfil fechado do inicializador; legado conserva pragmas/ordem anteriores.
#[derive(Clone, Copy)]
enum Abertura {
    Legado { novo: bool },
    D430,
}

impl Store {
    #[cfg(any(test, not(target_os = "linux")))]
    pub fn open(path: &Path) -> Result<Self> {
        let novo = super::personal_bootstrap::create_database(path)?;
        let c = Connection::open(path)?;
        Self::from_connection(path, c, Abertura::Legado { novo })
    }

    pub(crate) fn open_personal(database: super::personal_database::CapturedDatabase) -> Result<Self> {
        let (path, c) = database.into_verified()?;
        Self::from_connection(&path, c, Abertura::Legado { novo: false })
    }

    /// Nascimento D430: criação exclusiva SEM `personal_local_birth` (marca própria D430); FULL antes da 1ª DDL.
    pub(crate) fn open_d430_created(database: super::personal_database::CreatedDatabase) -> Result<Self> {
        let (path, c) = database.into_verified()?;
        Self::from_connection(&path, c, Abertura::D430)
    }

    /// Reabertura D430: só depois da validação positiva na mesma Connection (SEG-A-R1-01).
    pub(crate) fn open_d430(database: super::personal_database::ValidadaD430) -> Result<Self> {
        let (path, c) = database.into_verified()?;
        Self::from_connection(&path, c, Abertura::D430)
    }

    pub(crate) fn open_created(database: super::personal_database::CreatedDatabase) -> Result<Self> {
        let (path, c) = database.into_verified()?;
        Self::from_connection(&path, c, Abertura::Legado { novo: true })
    }

    fn from_connection(path: &Path, c: Connection, abertura: Abertura) -> Result<Self> {
        super::personal_database::configure(&c)?;
        let new_database = match abertura {
            Abertura::Legado { novo } => {
                c.execute_batch(
                    "PRAGMA journal_mode=WAL;
                     PRAGMA synchronous=NORMAL;
                     PRAGMA secure_delete=ON;",
                )?;
                novo
            }
            Abertura::D430 => {
                // FULL antes do WAL e de qualquer DDL/escrita, sem passar por NORMAL.
                c.execute_batch(
                    "PRAGMA synchronous=FULL;
                     PRAGMA journal_mode=WAL;
                     PRAGMA synchronous=FULL;
                     PRAGMA secure_delete=ON;",
                )?;
                anyhow::ensure!(
                    c.pragma_query_value(None, "synchronous", |r| r.get::<_, i64>(0))? == 2,
                    "D430 requires synchronous=FULL before the first write"
                );
                false
            }
        };
        #[cfg(test)]
        super::personal_database::observador::antes_da_primeira_ddl(&c);
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS kv(k TEXT PRIMARY KEY, v TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS bots(
                id TEXT PRIMARY KEY, rev INTEGER NOT NULL, deleted INTEGER NOT NULL DEFAULT 0,
                config TEXT NOT NULL, session_id TEXT, read_rev INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS entries(
                seq INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT UNIQUE NOT NULL, bot_id TEXT NOT NULL,
                rev INTEGER NOT NULL, kind TEXT NOT NULL, turn INTEGER NOT NULL, data TEXT NOT NULL,
                created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS entries_rev ON entries(rev);
             CREATE INDEX IF NOT EXISTS entries_bot ON entries(bot_id, seq);",
        )?;
        migrate(&c)?;
        codync_mem::schema::migrate(&c)?;
        super::computers::migrate(&c)?;
        super::guard_hub::migrate(&c)?;
        super::memory_session::migrate(&c)?;
        crate::fork::memory_auto::migrate(&c)?;
        crate::chat::memory::omitted::migrate(&c)?;
        super::bootstrap::migrate(&c)?;
        super::personal_bootstrap::migrate(&c, new_database)?;
        crate::fork::skill_selection::migrate(&c)?;
        crate::fork_avatars::migrate(&c)?;
        crate::fork::spaces::migrate(&c)?;
        super::bot_groups::legado::migrate(&c)?;
        crate::fork::tela_por_computador::migrate(&c)?;
        crate::fork::atividades::revisao::estado::migrate(&c)?;
        crate::fork::sessoes::migrate(&c)?;
        super::conectores::migrate(&c)?;
        crate::market::migrate_recibos(&c)?;
        crate::fork::background_space_selection::migrate(&c)?;
        let secret_key = if path == Path::new(":memory:") {
            let mut key = vec![0; 32];
            getrandom::fill(&mut key).map_err(|_| anyhow::anyhow!("random key unavailable"))?;
            Some(zeroize::Zeroizing::new(key))
        } else {
            None
        };
        let store = Self {
            skills_root: if path == Path::new(":memory:") {
                std::env::temp_dir().join(format!("donnabot-skills-{}", uuid::Uuid::new_v4()))
            } else {
                path.parent().unwrap_or_else(|| Path::new(".")).join("skills")
            },
            engine_setup_root: if path == Path::new(":memory:") {
                std::env::temp_dir().join(format!("donnabot-setup-{}", uuid::Uuid::new_v4()))
            } else {
                path.parent().unwrap_or_else(|| Path::new(".")).join("engine-setup")
            },
            gerados_root: if path == Path::new(":memory:") {
                std::env::temp_dir().join(format!("donnabot-gerados-{}", uuid::Uuid::new_v4()))
            } else {
                path.parent().unwrap_or_else(|| Path::new(".")).join("mexeriquinhas").join("gerados")
            },
            db: super::mutex_observado::ObservedMutex::new(c),
            team_lock: Arc::new(tokio::sync::Mutex::new(())),
            connector_lock: Mutex::new(()),
            memory_boundary: Mutex::new(()),
            #[cfg(test)]
            before_team_settings_write: Mutex::default(),
            #[cfg(test)]
            before_memory_maintain: Mutex::default(),
            #[cfg(test)]
            before_memory_scan: Mutex::default(),
            #[cfg(test)]
            before_owner_write: Mutex::default(),
            #[cfg(test)]
            vault_unlock_attempts: std::sync::atomic::AtomicUsize::new(0),
            secret_key: Mutex::new(secret_key),
            uso_historico: Mutex::default(),
            vault_unlock: Arc::default(),
        };
        store.restrict_legacy_managers()?;
        // Inclui época privada de devices para OAuth; migra antes da primeira captura de caller.
        crate::fork::migrate(&store)?;
        Ok(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("codync-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("t.db")
    }

    #[test]
    fn migrations_replace_the_old_ticket_table() {
        let db = temp_db();
        {
            let c = Connection::open(&db).unwrap();
            c.execute_batch(
                "CREATE TABLE kv(k TEXT PRIMARY KEY, v TEXT NOT NULL);
                 CREATE TABLE devices(ticket TEXT PRIMARY KEY, name TEXT, created_at INTEGER NOT NULL);
                 INSERT INTO devices VALUES('old', 'iPhone', 1);",
            )
            .unwrap();
        }
        let s = Store::open(&db).unwrap();
        assert!(s.devices().unwrap().is_empty());
        assert_eq!(s.kv_get("schema").as_deref(), Some("7"));
        drop(s);
        let s = Store::open(&db).unwrap();
        assert!(s.devices().unwrap().is_empty(), "reopening keeps the new table");
    }

    #[test]
    fn base_antiga_com_chave_repetida_conserva_todas_as_linhas_e_ganha_indice_unico() {
        let db = temp_db();
        Store::open(&db).unwrap();
        {
            let c = Connection::open(&db).unwrap();
            c.execute_batch(
                "DROP INDEX entries_client_nonce;
                 UPDATE kv SET v = '6' WHERE k = 'schema';
                 INSERT INTO entries(id, bot_id, rev, kind, turn, data, created_at, updated_at) VALUES
                   ('a1', 'a', 1, 'user', 1, '{\"text\":\"oi\",\"clientNonce\":\"n\"}', 0, 0),
                   ('a2', 'a', 2, 'user', 2, '{\"text\":\"oi\",\"clientNonce\":\"n\"}', 0, 0),
                   ('a3', 'a', 3, 'user', 3, '{\"text\":\"oi de novo\",\"clientNonce\":\"n\"}', 0, 0),
                   ('b1', 'b', 4, 'user', 1, '{\"text\":\"oi\",\"clientNonce\":\"n\"}', 0, 0),
                   ('s1', 'a', 5, 'user', 4, '{\"text\":\"sem chave\",\"clientNonce\":\"\"}', 0, 0),
                   ('s2', 'a', 6, 'user', 5, '{\"text\":\"sem chave\",\"clientNonce\":\"\"}', 0, 0),
                   ('x1', 'a', 7, 'user', 6, 'nao e json', 0, 0);",
            )
            .unwrap();
        }
        let s = Store::open(&db).unwrap();
        assert_eq!(s.kv_get("schema").as_deref(), Some("7"));
        let nonce = |id: &str| {
            let data = s.entry(id).unwrap().data;
            (data["clientNonce"].as_str().unwrap().to_owned(), data["duplicateClientNonce"].as_str().map(str::to_owned))
        };
        assert_eq!(nonce("a1"), ("n".into(), None), "a mais antiga fica com a chave");
        assert_eq!(nonce("a2"), (String::new(), Some("n".into())));
        assert_eq!(nonce("a3"), (String::new(), Some("n".into())));
        assert_eq!(nonce("b1"), ("n".into(), None), "outra conversa pode usar a mesma chave");
        assert_eq!((nonce("s1").0, nonce("s2").0), (String::new(), String::new()));
        assert_eq!(s.find_by_nonce("a", "n").unwrap().id, "a1");
        let again = s.insert_entry(
            &crate::store::Lane::main("a"),
            crate::store::EntryKind::User,
            6,
            &serde_json::json!({"text":"oi","clientNonce":"n"}),
        );
        assert!(again.is_err(), "o índice único recusa a segunda linha com a mesma chave");
        let c = Connection::open(&db).unwrap();
        let rows: i64 = c
            .query_row("SELECT COUNT(*) FROM entries WHERE id IN ('a1','a2','a3','b1','s1','s2','x1')", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(rows, 7, "nenhuma linha apagada, nem a que não é JSON");
        let plan: String = c
            .query_row(
                "EXPLAIN QUERY PLAN SELECT COUNT(*) FROM entries WHERE bot_id = 'a' AND kind = 'user'
                  AND (CASE WHEN json_valid(data) THEN json_extract(data, '$.status') END) = 'queued'",
                [],
                |r| r.get(3),
            )
            .unwrap();
        assert!(plan.contains("entries_user_queued"), "a contagem da fila usa o índice: {plan}");
    }

    #[test]
    fn session_reset_dividers_leave_the_chat() {
        let db = temp_db();
        {
            let c = Connection::open(&db).unwrap();
            Store::open(&db).unwrap();
            c.execute_batch(
                "UPDATE kv SET v = '5' WHERE k = 'schema';
                 INSERT INTO entries(id, bot_id, rev, kind, turn, data, created_at, updated_at) VALUES
                   ('reset', 'b', 1, 'notice', 0, '{\"text\":\"New session\",\"style\":\"divider\"}', 0, 0),
                   ('info', 'b', 2, 'notice', 0, '{\"text\":\"Routine · News\"}', 0, 0);",
            )
            .unwrap();
        }
        let s = Store::open(&db).unwrap();
        let chat: Vec<String> = s.history("b", i64::MAX, 10).unwrap().into_iter().map(|e| e.id).collect();
        assert_eq!(chat, ["info"]);
        assert_eq!(s.kv_get("schema").as_deref(), Some("7"));
    }

    #[test]
    fn threads_merge_into_their_chat() {
        let db = temp_db();
        {
            let c = Connection::open(&db).unwrap();
            Store::open(&db).unwrap();
            c.execute_batch(
                "UPDATE kv SET v = '4' WHERE k = 'schema';
                 ALTER TABLE entries ADD COLUMN thread_id TEXT;
                 INSERT INTO kv(k,v) VALUES('rev','10') ON CONFLICT(k) DO UPDATE SET v='10';
                 INSERT INTO kv(k, v) VALUES('read.root', '9'), ('lane.session.b@b/root', 's1'),
                   ('lane.session.b@b/routine-r1', 's2');
                 INSERT INTO entries(id, bot_id, thread_id, rev, kind, turn, data, created_at, updated_at) VALUES
                   ('root', 'b', NULL, 1, 'agent', 1, '{\"text\":\"Plan ready\",\"final\":true,\"author\":\"b\",\"thread\":{\"count\":2}}', 0, 0),
                   ('q', 'b', 'root', 2, 'user', 2, '{\"text\":\"why?\"}', 0, 0),
                   ('a', 'b', 'root', 3, 'agent', 2, '{\"text\":\"because\",\"final\":true}', 0, 0),
                   ('routine-r1', 'b', NULL, 4, 'notice', 0, '{\"text\":\"Routine · News\"}', 0, 0),
                   ('run', 'b', 'routine-r1', 5, 'agent', 3, '{\"text\":\"Today…\",\"final\":true}', 0, 0),
                   ('routine-result-r1', 'b', NULL, 6, 'agent', 0, '{\"text\":\"Today…\",\"final\":true}', 0, 0),
                   ('tool', 'b', 'routine-r1', 7, 'tool', 3, '{\"title\":\"Read\"}', 0, 0),
                   ('shot', 'b', NULL, 8, 'user', 4, '{\"text\":\"\",\"attachments\":[{\"name\":\"a.png\"}],\"thread\":{\"count\":1}}', 0, 0),
                   ('see', 'b', 'shot', 9, 'user', 5, '{\"text\":\"see?\"}', 0, 0);",
            )
            .unwrap();
        }
        let s = Store::open(&db).unwrap();
        let chat: Vec<String> = s.history("b", i64::MAX, 10).unwrap().into_iter().map(|e| e.id).collect();
        assert_eq!(
            chat,
            ["root", "q", "a", "routine-r1", "run", "routine-result-r1", "tool", "shot", "see"],
            "one chat, in time order"
        );
        let quote = s.entry("q").unwrap().data["replyTo"].clone();
        assert_eq!(quote, serde_json::json!({"id": "root", "author": "b", "text": "Plan ready"}));
        assert!(s.entry("root").unwrap().data.get("thread").is_none());
        assert_eq!(s.entry("run").unwrap().data["final"], false, "the run's reply was already copied into the chat");
        assert_eq!(s.entry("routine-result-r1").unwrap().turn, 3, "the result shows the run's trace");
        assert_eq!(s.entry("see").unwrap().data["replyTo"]["text"], "a.png", "a files-only message quotes its files");
        assert!(s.entry("q").unwrap().rev > 10, "synced clients get the changes");
        assert_eq!(s.entry("tool").unwrap().rev, 7, "thread trace isn't sent again");
        assert_eq!(s.unread("b", 10), 0, "a read chat stays read");
        assert_eq!(s.kv_get("read.root"), None);
        assert_eq!(s.kv_get("lane.session.b@b/root"), None);
        assert_eq!(s.kv_get("lane.session.b@b/routine-r1").as_deref(), Some("s2"), "a run's session stays");
    }

    #[test]
    fn reopens_a_database_with_a_pending_wal_and_migrates_it() {
        use crate::store::model::{EntryKind, Lane};
        let dir = crate::store::private_fixture_dir("codync-wal");
        let (db, copy) = (dir.join("t.db"), dir.join("copy.db"));
        let s = Store::open(&db).unwrap();
        s.insert_entry(&Lane::main("b"), EntryKind::User, 1, &serde_json::json!({"text": "Swift 6 strict mode"}))
            .unwrap();
        let c = Connection::open(&db).unwrap();
        c.execute_batch(
            "UPDATE kv SET v = '5' WHERE k = 'schema';
             INSERT INTO entries(id, bot_id, rev, kind, turn, data, created_at, updated_at) VALUES
               ('reset', 'b', 99, 'notice', 0, '{\"text\":\"New session\",\"style\":\"divider\"}', 0, 0);",
        )
        .unwrap();
        let wal = dir.join("t.db-wal");
        assert!(std::fs::metadata(&wal).unwrap().len() > 0, "the writes are still in the WAL, not checkpointed");
        std::fs::copy(&db, &copy).unwrap();
        std::fs::copy(&wal, dir.join("copy.db-wal")).unwrap();

        let reopened = Store::open(&copy).unwrap();
        assert_eq!(reopened.kv_get("schema").as_deref(), Some("7"), "migrated from the WAL's schema 5");
        let ids: Vec<String> = reopened.history("b", i64::MAX, 10).unwrap().into_iter().map(|e| e.id).collect();
        assert_eq!(ids.len(), 1, "the divider left the chat, the message stayed: {ids:?}");
        let found = reopened.search_messages("b", &["SWIFT".to_owned(), "strict".to_owned()], None, 10).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].data["text"], "Swift 6 strict mode");
    }

    #[test]
    fn two_connections_contend_for_the_write_lock_with_a_busy_timeout() {
        use std::time::Duration;
        let dir = crate::store::private_fixture_dir("codync-busy");
        let db = dir.join("t.db");
        Store::open(&db).unwrap();
        let count = |c: &Connection| -> i64 {
            c.query_row("SELECT count(*) FROM kv WHERE k LIKE 'busy.%'", [], |r| r.get(0)).unwrap()
        };

        let holder = Connection::open(&db).unwrap();
        holder.execute_batch("BEGIN IMMEDIATE").unwrap();
        let writer = Connection::open(&db).unwrap();
        writer.busy_timeout(Duration::from_millis(50)).unwrap();
        let err = writer.execute("INSERT INTO kv(k, v) VALUES('busy.timeout', '1')", []).unwrap_err();
        assert_eq!(err.sqlite_error_code(), Some(rusqlite::ErrorCode::DatabaseBusy));
        holder.execute_batch("COMMIT").unwrap();
        assert_eq!(count(&writer), 0, "the refused write left nothing behind");

        holder.execute_batch("BEGIN IMMEDIATE").unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            holder.execute_batch("COMMIT").unwrap();
        });
        writer.busy_timeout(Duration::from_secs(10)).unwrap();
        writer.execute("INSERT INTO kv(k, v) VALUES('busy.waited', '1')", []).unwrap();
        release.join().unwrap();
        assert_eq!(count(&writer), 1, "the write went through once the lock was released");
    }
}
