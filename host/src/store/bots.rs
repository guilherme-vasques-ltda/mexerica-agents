//! The `bots` table: roster rows, sessions, read marks and unread counts.

use super::model::{BotConfig, BotRow, EntryKind, LastMessage, message_text};
use super::{Store, logged, next_rev};
use crate::LockExt;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

/// Raw `bots` row before the config JSON is parsed.
type RawBot = (String, String, i64, bool, Option<String>, i64, Option<String>, Option<String>);

fn row_bot(r: &rusqlite::Row) -> rusqlite::Result<RawBot> {
    // Metadado inválido vira ausência de condição, nunca some com o bot.
    let version = r.get::<_, Option<String>>(7).ok().flatten();
    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0, r.get(4)?, r.get(5)?, r.get(6)?, version))
}

/// A stored config that no longer parses is skipped (and logged) rather than failing every listing.
fn parse_bot((id, cfg, rev, deleted, session_id, read_rev, avatar, version): RawBot) -> Option<BotRow> {
    match crate::fork_avatars::restored(&cfg, avatar.as_deref()) {
        Ok(config) => Some(BotRow {
            config,
            skills_version: crate::fork::skill_selection::valid_version(version),
            rev,
            deleted,
            session_id,
            read_rev,
        }),
        Err(error) => {
            tracing::warn!(bot = %id, %error, "skipping unreadable bot config");
            None
        }
    }
}

#[path = "../fork/bot_deletion/store.rs"]
mod deletion;

const BOT_COLS: &str = "id, config, rev, deleted, session_id, read_rev, avatar_config, donna_skills_version";

pub(crate) fn save_bot(c: &Connection, cfg: &BotConfig) -> Result<i64> {
    crate::fork::spaces::validate_bot(c, cfg)?;
    crate::fork::browser::check_save(c, cfg)?;
    let settings = super::team::read(c)?;
    let mut cfg = cfg.clone();
    crate::fork::skill_selection::preserve(c, &mut cfg)?;
    // Antes do settle: a aparência crua ainda traz estilo 3D; depois dele um par
    // avatar_config/mexeriquinha inconsistente já teria o v2 derrubado para None.
    crate::fork::spaces::check_principal_save(c, &cfg)?;
    crate::fork_avatars::settle(&mut cfg);
    if cfg.manage_team && (cfg.is_group() || settings.primary_bot_id.as_deref() != Some(&cfg.id)) {
        tracing::warn!(bot = %cfg.id, "ignoring manageTeam for a bot that is not the configured primary");
        cfg.manage_team = false;
    }
    crate::fork::donna_fixa::check_save(c, &cfg, settings.primary_bot_id.as_deref())?;
    let (profile, avatar) = crate::fork_avatars::stored(&cfg)?;
    // Conta que sai do bot leva junto o que a pessoa tinha ligado nela (ações perigosas), na mesma transação.
    if let Some(antes) = read_on(c, &cfg.id)?.filter(|b| !b.deleted) {
        let tiradas: Vec<String> =
            antes.config.connectors.iter().filter(|id| !cfg.connectors.contains(id)).cloned().collect();
        crate::market::ferramentas::esquecer_liberacoes_do_bot(c, &cfg.id, &tiradas)?;
    }
    let rev = next_rev(c)?;
    c.execute(
        "INSERT INTO bots(id, rev, config, avatar_config) VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET rev = ?2, config = ?3, avatar_config = ?4 WHERE deleted = 0",
        params![cfg.id, rev, profile, avatar],
    )?;
    crate::fork::spaces::register_primary(c, &cfg, rev)?;
    crate::fork::sessoes::ensure_principal(c, &cfg.id)?;
    Ok(rev)
}

/// Leitura canônica na Connection do chamador; não adquire Store nem abre transação.
pub(super) fn read_on(c: &Connection, id: &str) -> Result<Option<BotRow>> {
    let row = c.query_row(&format!("SELECT {BOT_COLS} FROM bots WHERE id = ?1"), [id], row_bot).optional()?;
    Ok(row.and_then(parse_bot))
}

impl Store {
    pub fn bots(&self) -> Result<Vec<BotRow>> {
        let c = self.db.locked();
        let mut st = c.prepare(&format!("SELECT {BOT_COLS} FROM bots"))?;
        let rows = st.query_map([], row_bot)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.into_iter().filter_map(parse_bot).collect())
    }

    pub fn bot(&self, id: &str) -> Result<Option<BotRow>> {
        read_on(&self.db.locked(), id)
    }

    /// Mesma leitura na `Connection` recebida (validadores Connection-only do VG03).
    pub(crate) fn bot_on(c: &Connection, id: &str) -> Result<Option<BotRow>> {
        read_on(c, id)
    }

    pub fn save_bot(&self, cfg: &BotConfig) -> Result<i64> {
        let mut c = self.db.locked();
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let rev = save_bot(&tx, cfg)?;
        tx.commit()?;
        drop(c);
        self.guard_cleanup()?;
        Ok(rev)
    }

    /// D368: fixture de testes do caminho do dono pelo app (ver `fork::donna_fixa::como_dono`).
    #[cfg(test)]
    pub(crate) fn save_bot_as_owner(&self, cfg: &BotConfig) -> Result<i64> {
        let mut c = self.db.locked();
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let rev = crate::fork::donna_fixa::como_dono(&tx, |c| save_bot(c, cfg))?;
        tx.commit()?;
        drop(c);
        self.guard_cleanup()?;
        Ok(rev)
    }

    pub fn touch_bot(&self, id: &str) -> Result<i64> {
        let c = self.db.locked();
        let rev = next_rev(&c)?;
        c.execute("UPDATE bots SET rev = ?2 WHERE id = ?1", params![id, rev])?;
        Ok(rev)
    }

    #[cfg(test)]
    pub fn delete_bot(&self, id: &str) -> Result<i64> {
        self.delete_bot_committed(id, None, std::iter::empty())
    }

    pub fn set_session(&self, id: &str, session: Option<&str>) -> Result<()> {
        let c = self.db.locked();
        c.execute("UPDATE bots SET session_id = ?2 WHERE id = ?1", params![id, session])?;
        Ok(())
    }

    /// The chat read: bumps the bot's `read_rev`.
    pub fn mark_read(&self, id: &str) -> Result<i64> {
        let mut c = self.db.locked();
        let tx = c.transaction()?;
        let rev = next_rev(&tx)?;
        tx.execute("UPDATE bots SET read_rev = ?2, rev = ?2 WHERE id = ?1", params![id, rev])?;
        tx.execute("UPDATE donna_space_conversations SET read_revision=?2 WHERE id=?1", params![id, rev])?;
        tx.commit()?;
        Ok(rev)
    }

    /// Final agent messages + pending permission cards newer than what the user has read. A reacted-to message
    /// doesn't count again when it changes: the user reacted, so they have seen it. A bot's reaction isn't a change
    /// either (`contentRev`, see `Hub::bot_react`).
    pub fn unread(&self, id: &str, read_rev: i64) -> i64 {
        let c = self.db.locked();
        let n = c.query_row(
            "SELECT COUNT(*) FROM entries WHERE bot_id = ?1 AND
               ((kind = 'agent' AND json_extract(data, '$.final') = 1) OR
                (kind = 'permission' AND json_extract(data, '$.status') = 'pending')) AND
               json_extract(data, '$.reactions') IS NULL AND
               COALESCE(json_extract(data, '$.contentRev'), rev) > ?2",
            params![id, read_rev],
            |r| r.get(0),
        );
        logged("unread", n).unwrap_or(0)
    }

    pub fn last_message(&self, id: &str) -> Option<LastMessage> {
        self.last_message_filtered(id, false)
    }

    /// Newest chat-visible line for the roster preview, with its author.
    pub fn last_visible_message(&self, id: &str) -> Option<LastMessage> {
        self.last_message_filtered(id, true)
    }

    fn last_message_filtered(&self, id: &str, visible_only: bool) -> Option<LastMessage> {
        let c = self.db.locked();
        let row = c.query_row(
            "SELECT kind, data, updated_at FROM entries WHERE bot_id = ?1 AND
               (kind = 'user' OR (kind = 'agent' AND json_extract(data, '$.final') = 1)) AND
               (?2 = 0 OR json_extract(data, '$.from') IS NOT 'host')
             ORDER BY seq DESC LIMIT 1",
            params![id, visible_only],
            |r| {
                let kind: String = r.get(0)?;
                let data: String = r.get(1)?;
                let at: i64 = r.get(2)?;
                let data = serde_json::from_str::<Value>(&data).unwrap_or_default();
                // A files-only message previews as its file names.
                let text = message_text(&data);
                let author =
                    (kind != EntryKind::User.as_str()).then(|| data["author"].as_str().unwrap_or(id).to_owned());
                Ok(LastMessage { text, at, author })
            },
        );
        logged("last message", row.optional()).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::model::{BotKind, EntryKind, Lane, Permission};
    use crate::store::tests::temp_store;

    #[test]
    fn last_visible_message_skips_host_entries() {
        let s = temp_store();
        let cfg: BotConfig = serde_json::from_value(serde_json::json!({"id":"b1", "name":"Bot"})).unwrap();
        s.save_bot(&cfg).unwrap();
        let lane = Lane::main("b1");
        s.insert_entry(&lane, EntryKind::User, 1, &serde_json::json!({"text":"nota", "from":"host"})).unwrap();
        assert!(s.last_visible_message("b1").is_none());
        s.insert_entry(&lane, EntryKind::User, 2, &serde_json::json!({"text":"pergunta"})).unwrap();
        s.insert_entry(&lane, EntryKind::Agent, 2, &serde_json::json!({"text":"resposta", "final":true})).unwrap();
        s.insert_entry(&lane, EntryKind::User, 3, &serde_json::json!({"text":"outra nota", "from":"host"})).unwrap();
        assert_eq!(s.last_visible_message("b1").unwrap().text, "resposta");
        assert_eq!(s.last_message("b1").unwrap().text, "outra nota");
        s.insert_entry(&lane, EntryKind::User, 4, &serde_json::json!({"text":"nova pergunta"})).unwrap();
        assert_eq!(s.last_visible_message("b1").unwrap().text, "nova pergunta");
    }

    #[test]
    fn saves_ignore_non_primary_manager_flags_without_losing_other_changes() {
        let store = temp_store();
        let mut cfg: BotConfig =
            serde_json::from_value(serde_json::json!({"id":"dev", "name":"Dev", "manageTeam":true})).unwrap();
        store.save_bot(&cfg).unwrap();
        assert!(!store.bot("dev").unwrap().unwrap().config.manage_team);
        cfg.name = "Renamed".into();
        store.save_bot(&cfg).unwrap();
        let saved = store.bot("dev").unwrap().unwrap().config;
        assert_eq!(saved.name, "Renamed");
        assert!(!saved.manage_team);
        store.set_team_settings(&serde_json::json!({"primaryBotId":"dev"})).unwrap();
        store.save_bot(&cfg).unwrap();
        assert!(store.bot("dev").unwrap().unwrap().config.manage_team);
        cfg.id = "group".into();
        cfg.kind = BotKind::Group;
        store.save_bot(&cfg).unwrap();
        assert!(!store.bot("group").unwrap().unwrap().config.manage_team);
    }

    #[test]
    fn rev_sync_and_unread() {
        let s = temp_store();
        let cfg = BotConfig {
            space_id: crate::fork::spaces::personal_id(),
            context: None,
            person: None,
            fork_editor: crate::fork::engine_setup::BotEngineSelection::default(),
            id: "b1".into(),
            kind: BotKind::Agent,
            members: vec![],
            leader: None,
            name: "Rev".into(),
            description: String::new(),
            owner_preferences: String::new(),
            avatar_color: "blue".into(),
            avatar_config: None,
            mexeriquinha: None,
            title: String::new(),
            avatar_shape: "blob".into(),
            backend: "claude".into(),
            command: None,
            cwd: "/tmp".into(),
            permission: Permission::Ask,
            permission_chosen_by_user: false,
            model: None,
            model_options: std::collections::BTreeMap::new(),
            fast_mode: false,
            pinned: false,
            position: 0,
            hidden: false,
            notify: None,
            connectors: vec![],
            conta_padrao: std::collections::BTreeMap::new(),
            perguntar_conta: std::collections::BTreeMap::new(),
            logins: vec![],
            skills: vec![],
            computer: false,
            browser_preference: crate::fork::browser::BrowserPreference::default(),
            manage_team: false,
            created_by: None,
            shared_memory: None,
            harness_memory: false,
            auto_name: false,
            created_at: 0,
        };
        s.save_bot(&cfg).unwrap();
        let u = s.insert_entry(&Lane::main("b1"), EntryKind::User, 1, &serde_json::json!({"text": "hi"})).unwrap();
        let a = s
            .insert_entry(&Lane::main("b1"), EntryKind::Agent, 1, &serde_json::json!({"text": "yo", "final": false}))
            .unwrap();
        assert!(a.rev > u.rev);
        assert_eq!(s.unread("b1", 0), 0, "narration is not unread");
        let a2 = s.update_entry(&a.id, &serde_json::json!({"text": "yo!", "final": true})).unwrap().unwrap();
        assert_eq!(s.unread("b1", 0), 1);
        assert_eq!(s.entries_since(a2.rev - 1, 500).unwrap().len(), 1);
        assert_eq!(s.entries_since(0, 1).unwrap().len(), 1, "fresh sync is capped per bot");
        assert_eq!(s.last_message("b1").unwrap().text, "yo!");
        let r = s.mark_read("b1").unwrap();
        assert_eq!(s.unread("b1", r), 0);
        assert_eq!(s.history("b1", a.seq, 10).unwrap().len(), 1);
        assert_eq!(s.bot("b1").unwrap().unwrap().config.permission, Permission::Ask);
        assert!(s.bot("missing").unwrap().is_none());

        let reply = serde_json::json!({"text": "done", "final": true});
        // A deleted bot stays deleted, and its winding-down actor leaves no lines behind.
        s.delete_bot("b1").unwrap();
        assert!(s.insert_entry(&Lane::main("b1"), EntryKind::Notice, 3, &reply).is_err());
        s.save_bot(&cfg).unwrap();
        assert!(s.bot("b1").unwrap().unwrap().deleted);
    }
}
