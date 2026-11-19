//! Exposição externa da linhagem de sessão usada pela memória, separada da guarda de gestão.
use super::{Store, now_ms};
use crate::LockExt;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

/// Linhas antigas de turno de grupo: o número era do chat do grupo e podia ter sobrescrito o turno de mesmo número do
/// chat próprio. Ficam fora de qualquer chat real, e o turno próprio afetado continua inelegível, como antes.
const LEGACY_GROUP_CHAT: &str = "legado:grupo";

pub(super) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_memory_sessions(
            bot TEXT NOT NULL, session TEXT NOT NULL, marked_at INTEGER, reason TEXT,
            PRIMARY KEY(bot, session));
         CREATE TABLE IF NOT EXISTS donna_memory_gateway_pending(bot TEXT PRIMARY KEY, at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS donna_memory_unsettled(bot TEXT NOT NULL, session TEXT NOT NULL,
            PRIMARY KEY(bot, session));",
    )?;
    migrate_turn_chat(db)
}

fn has_chat(db: &Connection, table: &str) -> Result<Option<bool>> {
    let mut st = db.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns: Vec<String> = st.query_map([], |r| r.get(1))?.collect::<rusqlite::Result<_>>()?;
    Ok((!columns.is_empty()).then(|| columns.iter().any(|c| c == "chat")))
}

/// O turno é numerado por chat; a memória passa a indexar por `(bot, chat, turn)` (D229-6). Idempotente.
fn migrate_turn_chat(db: &Connection) -> Result<()> {
    let turns = has_chat(db, "donna_memory_turns")?;
    let origin = has_chat(db, "donna_memory_turn_origin")?;
    let tx = db.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_memory_turns_chat(
            bot TEXT NOT NULL, chat TEXT NOT NULL, turn INTEGER NOT NULL, session TEXT NOT NULL,
            PRIMARY KEY(bot, chat, turn));
         CREATE TABLE IF NOT EXISTS donna_memory_turn_origin_chat(bot TEXT NOT NULL, chat TEXT NOT NULL,
            turn INTEGER NOT NULL, origin TEXT NOT NULL, PRIMARY KEY(bot, chat, turn));",
    )?;
    if turns == Some(false) {
        // A origem ainda está na tabela antiga: decide o chat legado antes de qualquer mudança.
        let source = if origin == Some(false) { "donna_memory_turn_origin" } else { "donna_memory_turn_origin_chat" };
        tx.execute(
            &format!(
                "INSERT OR IGNORE INTO donna_memory_turns_chat(bot,chat,turn,session)
                 SELECT t.bot, CASE WHEN EXISTS(SELECT 1 FROM {source} o WHERE o.bot=t.bot AND o.turn=t.turn
                    AND o.origin='group') THEN ?1 ELSE t.bot END, t.turn, t.session FROM donna_memory_turns t"
            ),
            [LEGACY_GROUP_CHAT],
        )?;
    }
    if origin == Some(false) {
        tx.execute(
            "INSERT OR IGNORE INTO donna_memory_turn_origin_chat(bot,chat,turn,origin)
             SELECT bot, CASE WHEN origin='group' THEN ?1 ELSE bot END, turn, origin FROM donna_memory_turn_origin",
            [LEGACY_GROUP_CHAT],
        )?;
    }
    let tables = [
        ("donna_memory_turns", "donna_memory_turns_chat", turns),
        ("donna_memory_turn_origin", "donna_memory_turn_origin_chat", origin),
    ];
    for (old, new, present) in tables {
        match present {
            // Já migrada: a tabela nova criada acima fica vazia e sai.
            Some(true) => tx.execute_batch(&format!("DROP TABLE {new};"))?,
            Some(false) => tx.execute_batch(&format!("DROP TABLE {old}; ALTER TABLE {new} RENAME TO {old};"))?,
            None => tx.execute_batch(&format!("ALTER TABLE {new} RENAME TO {old};"))?,
        }
    }
    tx.commit()?;
    Ok(())
}

impl Store {
    /// Uma sessão recém-criada só nasce limpa se não receber contexto anterior.
    pub(crate) fn memory_session_start(&self, bot: &str, session: &str, inherited: bool) -> Result<()> {
        let _boundary = self.memory_boundary.locked();
        let marked = inherited.then(now_ms);
        let reason = inherited.then_some("contexto herdado");
        self.db.locked().execute(
            "INSERT INTO donna_memory_sessions(bot,session,marked_at,reason) VALUES(?1,?2,?3,?4)
             ON CONFLICT(bot,session) DO UPDATE SET marked_at=COALESCE(marked_at,excluded.marked_at),
                reason=COALESCE(reason,excluded.reason)",
            params![bot, session, marked, reason],
        )?;
        Ok(())
    }

    /// Associa todos os registros de um turno à sessão que recebeu seu contexto. O turno é do `chat` onde aconteceu.
    pub(crate) fn memory_session_turn(&self, bot: &str, chat: &str, session: &str, turn: i64) -> Result<()> {
        let _boundary = self.memory_boundary.locked();
        let db = self.db.locked();
        let existing: Option<String> = db
            .query_row(
                "SELECT session FROM donna_memory_turns WHERE bot=?1 AND chat=?2 AND turn=?3",
                params![bot, chat, turn],
                |r| r.get(0),
            )
            .optional()?;
        if existing.as_deref().is_some_and(|old| old != session) {
            db.execute("INSERT INTO donna_memory_sessions(bot,session,marked_at,reason) VALUES(?1,?2,?3,'turno retomado')
                ON CONFLICT(bot,session) DO UPDATE SET marked_at=COALESCE(marked_at,excluded.marked_at),reason=COALESCE(reason,excluded.reason)", params![bot, session, now_ms()])?;
        }
        db.execute(
            "INSERT INTO donna_memory_turns(bot,chat,turn,session) VALUES(?1,?2,?3,?4)
            ON CONFLICT(bot,chat,turn) DO UPDATE SET session=excluded.session",
            params![bot, chat, turn, session],
        )?;
        Ok(())
    }

    /// Turnos sem procedência persistida são externos, inclusive os anteriores à migração.
    pub(crate) fn memory_turn_marked(&self, bot: &str, chat: &str, turn: i64) -> Result<bool> {
        let marked: Option<Option<i64>> = self.db.locked().query_row(
            "SELECT s.marked_at FROM donna_memory_turns t JOIN donna_memory_sessions s ON s.bot=t.bot AND s.session=t.session WHERE t.bot=?1 AND t.chat=?2 AND t.turn=?3",
            params![bot,chat,turn], |row| row.get(0),
        ).optional()?;
        Ok(marked.is_none_or(|value| value.is_some()))
    }

    /// Proteção persistente é armada antes de entregar o contexto ao motor.
    pub(crate) fn memory_turn_begin(
        &self,
        bot: &str,
        chat: &str,
        session: &str,
        turns: &[i64],
        origin: &str,
    ) -> Result<()> {
        let _boundary = self.memory_boundary.locked();
        let mut db = self.db.locked();
        let tx = db.transaction()?;
        let unresolved: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM donna_memory_unsettled WHERE bot=?1 AND session=?2)",
            params![bot, session],
            |row| row.get(0),
        )?;
        if unresolved || origin == "group" {
            let reason = if unresolved { "exposição não confirmada" } else { "contexto de grupo" };
            tx.execute("INSERT INTO donna_memory_sessions(bot,session,marked_at,reason) VALUES(?1,?2,?3,?4)
                ON CONFLICT(bot,session) DO UPDATE SET marked_at=COALESCE(marked_at,excluded.marked_at),reason=COALESCE(reason,excluded.reason)", params![bot, session, now_ms(), reason])?;
        }
        tx.execute("INSERT OR IGNORE INTO donna_memory_unsettled(bot,session) VALUES(?1,?2)", params![bot, session])?;
        for turn in turns {
            tx.execute(
                "INSERT INTO donna_memory_turn_origin(bot,chat,turn,origin) VALUES(?1,?2,?3,?4)
                ON CONFLICT(bot,chat,turn) DO UPDATE SET origin=excluded.origin",
                params![bot, chat, turn, origin],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Só o fechamento sem falha remove a proteção; erro e interrupção deixam a linhagem fechada.
    pub(crate) fn memory_turn_finish(&self, bot: &str, session: &str) -> Result<()> {
        self.db
            .locked()
            .execute("DELETE FROM donna_memory_unsettled WHERE bot=?1 AND session=?2", params![bot, session])?;
        Ok(())
    }

    /// Turnos sem origem conhecida ou com exposição não confirmada são descartados pelo dream.
    pub(crate) fn memory_turn_eligible(&self, bot: &str, chat: &str, turn: i64) -> Result<bool> {
        Ok(self.db.locked().query_row(
            "SELECT EXISTS(SELECT 1 FROM donna_memory_auto_jobs j,json_each(j.context,'$.messages') r
             JOIN entries e ON e.id=json_extract(r.value,'$.id') AND e.rev=json_extract(r.value,'$.revision')
             WHERE j.bot=?1 AND j.chat=?2 AND e.bot_id=?2 AND e.turn=?3
             AND j.outcome='success' AND j.scope='allowed')",
            params![bot, chat, turn],
            |row| row.get(0),
        )?)
    }

    /// Diagnóstico sem conteúdo externo ou identificadores de mensagens.
    pub(crate) fn memory_session_info(&self, bot: &str, session: &str) -> Result<serde_json::Value> {
        let state: Option<(Option<i64>, Option<String>)> = self
            .db
            .locked()
            .query_row(
                "SELECT marked_at,reason FROM donna_memory_sessions WHERE bot=?1 AND session=?2",
                params![bot, session],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(match state {
            Some((at, reason)) => serde_json::json!({"marked":at.is_some(),"at":at,"reason":reason}),
            None => serde_json::json!({"marked":true,"at":null,"reason":"sessão anterior ou origem desconhecida"}),
        })
    }

    /// Uma sessão desconhecida ou anterior à migração é tratada como marcada.
    pub(crate) fn memory_session_marked(&self, bot: &str, session: &str) -> Result<bool> {
        let state: Option<Option<i64>> = self
            .db
            .locked()
            .query_row(
                "SELECT marked_at FROM donna_memory_sessions WHERE bot=?1 AND session=?2",
                params![bot, session],
                |row| row.get(0),
            )
            .optional()?;
        Ok(state.is_none_or(|marked| marked.is_some()))
    }

    pub(crate) fn memory_session_mark(&self, bot: &str, session: &str, reason: &str) -> Result<()> {
        self.memory_session_mark_with_failure(bot, session, reason, || {})
    }

    /// A falha fecha a porta ainda sob a cerca; um escritor nunca observa o estado antigo como seguro.
    pub(crate) fn memory_session_mark_with_failure(
        &self,
        bot: &str,
        session: &str,
        reason: &str,
        on_failure: impl FnOnce(),
    ) -> Result<()> {
        let _boundary = self.memory_boundary.locked();
        let result = self.db.locked().execute(
            "INSERT INTO donna_memory_sessions(bot,session,marked_at,reason) VALUES(?1,?2,?3,?4)
             ON CONFLICT(bot,session) DO UPDATE SET
                marked_at=COALESCE(marked_at,excluded.marked_at), reason=COALESCE(reason,excluded.reason)",
            params![bot, session, now_ms(), reason],
        );
        if result.is_err() {
            on_failure();
        }
        result?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn turno_de_grupo_nao_sobrescreve_o_turno_de_mesmo_numero_do_chat_proprio() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.memory_session_start("bot", "privada", false).unwrap();
        store.memory_session_turn("bot", "bot", "privada", 7).unwrap();
        store.memory_turn_begin("bot", "bot", "privada", &[7], "owner").unwrap();
        store.memory_turn_finish("bot", "privada").unwrap();
        // O grupo também está no turno 7 do chat dele, numa sessão própria do membro.
        store.memory_session_start("bot", "grupo-sessao", false).unwrap();
        store.memory_session_turn("bot", "grupo", "grupo-sessao", 7).unwrap();
        store.memory_turn_begin("bot", "grupo", "grupo-sessao", &[7], "group").unwrap();
        store.memory_turn_finish("bot", "grupo-sessao").unwrap();
        assert!(!store.memory_turn_eligible("bot", "bot", 7).unwrap(), "linhagem não prova sucesso durável");
        assert!(!store.memory_turn_marked("bot", "bot", 7).unwrap(), "e não herda a marca do grupo");
        assert!(!store.memory_session_marked("bot", "privada").unwrap());
        assert!(!store.memory_turn_eligible("bot", "grupo", 7).unwrap());
        assert!(store.memory_turn_marked("bot", "grupo", 7).unwrap());
    }

    #[test]
    fn migracao_preserva_linhas_e_turno_de_grupo_antigo_continua_inelegivel() {
        let dir = std::env::temp_dir().join(format!("memoria-turno-chat-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE donna_memory_turns(bot TEXT NOT NULL, turn INTEGER NOT NULL, session TEXT NOT NULL,
                    PRIMARY KEY(bot, turn));
                 CREATE TABLE donna_memory_turn_origin(bot TEXT NOT NULL, turn INTEGER NOT NULL, origin TEXT NOT NULL,
                    PRIMARY KEY(bot, turn));
                 CREATE TABLE donna_memory_sessions(bot TEXT NOT NULL, session TEXT NOT NULL, marked_at INTEGER,
                    reason TEXT, PRIMARY KEY(bot, session));
                 INSERT INTO donna_memory_sessions VALUES('bot','s1',NULL,NULL),('bot','s2',5,'contexto de grupo');
                 INSERT INTO donna_memory_turns VALUES('bot',1,'s1'),('bot',2,'s2');
                 INSERT INTO donna_memory_turn_origin VALUES('bot',1,'owner'),('bot',2,'group');",
            )
            .unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert!(!store.memory_turn_eligible("bot", "bot", 1).unwrap(), "legado preservado sem presumir sucesso");
        // O turno 2 do chat próprio foi sobrescrito por um grupo: continua inelegível, como antes da migração.
        assert!(!store.memory_turn_eligible("bot", "bot", 2).unwrap());
        assert!(store.memory_turn_marked("bot", "bot", 2).unwrap());
        let legacy: i64 = store
            .db
            .locked()
            .query_row("SELECT COUNT(*) FROM donna_memory_turn_origin WHERE chat=?1", [LEGACY_GROUP_CHAT], |r| r.get(0))
            .unwrap();
        assert_eq!(legacy, 1);
        drop(store);
        let reopened = Store::open(&path).unwrap();
        assert!(!reopened.memory_turn_eligible("bot", "bot", 1).unwrap(), "reabrir não inventa sucesso");
        drop(reopened);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sessao_nova_nao_herda_marca_e_falha_fecha_sob_cerca() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.memory_session_start("bot", "antiga", false).unwrap();
        store.memory_session_mark("bot", "antiga", "anexo").unwrap();
        store.memory_session_start("bot", "nova", false).unwrap();
        assert!(store.memory_session_marked("bot", "antiga").unwrap());
        assert!(!store.memory_session_marked("bot", "nova").unwrap());
        store.memory_session_start("bot", "nova", true).unwrap();
        assert!(
            store.memory_session_marked("bot", "nova").unwrap(),
            "um resumo herdado não perde a marca quando o ID é reutilizado"
        );
        store.db.locked().execute("DROP TABLE donna_memory_sessions", []).unwrap();
        let blocked = AtomicBool::new(false);
        assert!(
            store
                .memory_session_mark_with_failure("bot", "nova", "rede", || {
                    assert!(store.memory_boundary.try_lock().is_err());
                    blocked.store(true, Ordering::SeqCst);
                })
                .is_err()
        );
        assert!(blocked.load(Ordering::SeqCst));
    }
}
