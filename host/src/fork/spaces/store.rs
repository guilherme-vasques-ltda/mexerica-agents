use super::{Appearance, PERSONAL, Snapshot, Space};
use crate::{LockExt, store::Store};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension};
pub(crate) const KEY: &str = "donna.spaces.v1";

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    let tx = c.unchecked_transaction()?;
    let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM kv WHERE k=?1)", [KEY], |r| r.get(0))?;
    if !exists {
        let revision = crate::store::next_rev(&tx)?;
        let snapshot = Snapshot {
            version: 1,
            personal_space_id: PERSONAL.into(),
            revision,
            spaces: vec![Space {
                id: PERSONAL.into(),
                principal_bot_id: None,
                kind: "personal".into(),
                name: "Pessoal".into(),
                position: 0,
                revision,
                tile: None,
                appearance: Appearance::default(),
                appearance_configured: false,
                owner: None,
            }],
        };
        tx.execute("INSERT INTO kv(k,v) VALUES(?1,?2)", [KEY, &serde_json::to_string(&snapshot)?])?;
    }
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_space_conversations(
        id TEXT PRIMARY KEY, space_id TEXT NOT NULL, bot_id TEXT NOT NULL,
        title TEXT NOT NULL, revision INTEGER NOT NULL, is_primary INTEGER NOT NULL,
        closed INTEGER NOT NULL DEFAULT 0, read_revision INTEGER NOT NULL DEFAULT 0,
        engine_account_id TEXT, session_id TEXT);
        CREATE INDEX IF NOT EXISTS donna_space_conversations_scope ON donna_space_conversations(space_id,bot_id);",
    )?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS donna_space_uploads(conversation_id TEXT NOT NULL,space_id TEXT NOT NULL,id TEXT NOT NULL,name TEXT NOT NULL,size INTEGER NOT NULL,complete INTEGER NOT NULL,sha256 TEXT,revision INTEGER NOT NULL,PRIMARY KEY(conversation_id,id));")?;
    let columns = tx
        .prepare("PRAGMA table_info(donna_space_conversations)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (name, kind) in [
        ("engine_selection", "TEXT"),
        ("account_revision", "INTEGER"),
        ("account_stamp", "TEXT"),
        ("context_start_seq", "INTEGER NOT NULL DEFAULT 0"),
        ("continuity", "TEXT"),
        ("state_revision", "INTEGER NOT NULL DEFAULT 0"),
        ("startup_hub_id", "TEXT"),
        ("startup_launch_id", "TEXT"),
        ("startup_request_digest", "TEXT"),
    ] {
        if !columns.iter().any(|c| c == name) {
            tx.execute(&format!("ALTER TABLE donna_space_conversations ADD COLUMN {name} {kind}"), [])?;
        }
    }
    tx.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS donna_space_conversations_startup ON donna_space_conversations(startup_hub_id,startup_launch_id) WHERE startup_hub_id IS NOT NULL AND startup_launch_id IS NOT NULL;")?;
    // Relação aditiva: histórico, contas, grants, rotinas, arquivos e sessões ficam em seus lugares.
    tx.execute("INSERT OR IGNORE INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary,read_revision)
        SELECT id,COALESCE(json_extract(config,'$.spaceId'),?1),id,COALESCE(json_extract(config,'$.name'),''),rev,1,read_rev FROM bots WHERE json_valid(config) AND deleted=0", [PERSONAL])?;
    tx.execute(
        "INSERT OR IGNORE INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary)
        SELECT bot_id,?1,bot_id,'',MAX(rev),1 FROM entries
        WHERE bot_id IN (SELECT id FROM bots WHERE deleted=0) GROUP BY bot_id",
        [PERSONAL],
    )?;
    tx.execute(
        "UPDATE bots SET config=json_set(config,'$.spaceId',?1) WHERE CASE WHEN json_valid(config) THEN json_extract(config,'$.spaceId') IS NULL ELSE 0 END",
        [PERSONAL],
    )?;
    tx.execute("INSERT INTO kv(k,v) VALUES('donna.spaces.schema','1') ON CONFLICT(k) DO NOTHING", [])?;
    super::config_generation::migrate(&tx)?;
    super::birth::migrate(&tx)?;
    tx.commit()?;
    Ok(())
}
impl Store {
    pub fn spaces(&self) -> Result<Snapshot> {
        read(&self.db.locked())
    }
    pub(super) fn change_spaces(
        &self,
        expected: i64,
        change: impl FnOnce(&mut Snapshot, i64) -> Result<()>,
    ) -> Result<Snapshot> {
        let mut c = self.db.locked();
        let tx = c.transaction()?;
        let mut state = read(&tx)?;
        if state.revision != expected {
            return Err(crate::remote::cloud::Conflict("spaces_revision_conflict").into());
        }
        let rev = crate::store::next_rev(&tx)?;
        change(&mut state, rev)?;
        ensure!(
            state.spaces.first().is_some_and(|s| s.id == PERSONAL
                && s.kind == "personal"
                && s.name == "Pessoal"
                && s.position == 0
                && s.tile.is_none()),
            "personal_space_immutable"
        );
        state.revision = rev;
        tx.execute("UPDATE kv SET v=?2 WHERE k=?1", [KEY, &serde_json::to_string(&state)?])?;
        tx.commit()?;
        Ok(state)
    }
    pub fn require_space(&self, id: &str) -> Result<()> {
        require_space(&self.db.locked(), id)
    }

    /// D543: ids dos bots do Space para a leitura prévia da exclusão (a Tx relê e
    /// confere o mesmo conjunto dentro da autoridade).
    pub(crate) fn space_bot_ids(&self, space_id: &str) -> Result<Vec<String>> {
        let c = self.db.locked();
        let mut st = c.prepare(
            "SELECT id FROM bots WHERE deleted=0 AND json_valid(config)
             AND json_extract(config,'$.spaceId')=?1 ORDER BY id",
        )?;
        Ok(st.query_map([space_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
    }
}
pub(crate) fn require_space(c: &Connection, id: &str) -> Result<()> {
    ensure!(read(c)?.spaces.iter().any(|s| s.id == id), "space_not_found");
    Ok(())
}

pub(crate) fn read(c: &Connection) -> Result<Snapshot> {
    let raw: String = c
        .query_row("SELECT v FROM kv WHERE k=?1", [KEY], |r| r.get(0))
        .optional()?
        .ok_or_else(|| anyhow!("spaces_unavailable"))?;
    Ok(serde_json::from_str(&raw)?)
}

pub(crate) fn is_space_principal(c: &Connection, id: &str) -> Result<bool> {
    Ok(read(c)?.spaces.iter().any(|s| s.id != PERSONAL && s.principal_bot_id.as_deref() == Some(id)))
}

pub(crate) fn register_primary(c: &Connection, cfg: &crate::store::BotConfig, rev: i64) -> Result<()> {
    c.execute(
        "INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary) VALUES(?1,?2,?1,?3,?4,1)
        ON CONFLICT(id) DO UPDATE SET title=?3,revision=?4",
        rusqlite::params![cfg.id, cfg.space_id, cfg.name, rev],
    )?;
    Ok(())
}
