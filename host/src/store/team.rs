//! Política local da D67. Identidade explícita, sem inferir autoridade pelo nome do bot.
use super::{BotConfig, ManagementOrigin, Permission, Store, next_rev};
use crate::LockExt;
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const KEY: &str = "team.settings.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TeamSettings {
    pub primary_bot_id: Option<String>,
    /// Raiz definida pelo usuário para pastas escolhidas por `create_bot`.
    #[serde(default)]
    pub workspace_root: Option<String>,
    pub permission: Permission,
}

impl Default for TeamSettings {
    fn default() -> Self {
        Self { primary_bot_id: None, workspace_root: None, permission: Permission::Ask }
    }
}

/// Apenas campos que as ferramentas do Berga podem alterar. O restante é preservado.
pub(crate) fn same_team_profile(a: &BotConfig, b: &BotConfig) -> bool {
    a.name == b.name
        && a.description == b.description
        && a.avatar_color == b.avatar_color
        && a.avatar_shape == b.avatar_shape
        && a.hidden == b.hidden
}

pub(crate) fn restore_team_profile(current: &BotConfig, before: &BotConfig) -> BotConfig {
    let mut cfg = current.clone();
    cfg.name.clone_from(&before.name);
    cfg.description.clone_from(&before.description);
    cfg.avatar_color.clone_from(&before.avatar_color);
    cfg.avatar_shape.clone_from(&before.avatar_shape);
    cfg.hidden = before.hidden;
    cfg
}

pub fn restrictive(a: Permission, b: Permission) -> Permission {
    if a == Permission::Ask || b == Permission::Ask { Permission::Ask } else { Permission::Auto }
}

pub(crate) fn read(c: &Connection) -> Result<TeamSettings> {
    c.query_row("SELECT v FROM kv WHERE k = ?", [KEY], |r| r.get::<_, String>(0)).optional()?.map_or_else(
        || Ok(TeamSettings::default()),
        |s| {
            Ok(serde_json::from_str(&s).unwrap_or_else(|error| {
                tracing::warn!(%error, "unreadable team settings; using safe defaults");
                TeamSettings::default()
            }))
        },
    )
}

fn save_team_state(c: &Connection, routines: &str, (key, pending): &(String, String)) -> Result<()> {
    c.execute("INSERT INTO kv(k, v) VALUES('routines.v1', ?1) ON CONFLICT(k) DO UPDATE SET v = ?1", [routines])?;
    crate::fork::routine_creation::external_grant::prune(c, routines)?;
    if c.execute("UPDATE kv SET v = ?2 WHERE k = ?1", params![key, pending])? != 1 {
        bail!("missing pending team change");
    }
    Ok(())
}

impl Store {
    pub(crate) fn restrict_legacy_managers(&self) -> Result<()> {
        let c = self.db.locked();
        let tx = c.unchecked_transaction()?;
        let settings = read(&tx)?;
        let rows = tx
            .prepare(&crate::fork_avatars::profile_query("SELECT id, {profile} FROM bots"))?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, raw) in rows {
            let Ok(mut cfg) = serde_json::from_str::<BotConfig>(&raw) else { continue };
            let before = cfg.manage_team;
            if cfg.manage_team && (cfg.is_group() || settings.primary_bot_id.as_deref() != Some(&id)) {
                cfg.manage_team = false;
            }
            if before != cfg.manage_team {
                tx.execute(
                    "UPDATE bots SET config = ?2, rev = ?3 WHERE id = ?1",
                    params![id, serde_json::to_string(&cfg)?, next_rev(&tx)?],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn team_settings(&self) -> Result<TeamSettings> {
        read(&self.db.locked())
    }

    #[cfg(test)]
    pub(crate) fn test_before_team_settings_write(&self, hook: impl FnOnce() + Send + 'static) {
        *self.before_team_settings_write.locked() = Some(Box::new(hook));
    }

    /// Chamado apenas pela intenção do usuário na API local. Uma transação troca a principal,
    /// retira permissões antigas e recalcula só a autonomia ainda herdada dos bots criados por bot.
    pub fn set_team_settings(&self, patch: &Value) -> Result<Vec<BotConfig>> {
        // Revogação herdada e admissão de efeitos usam a mesma ordem boundary -> DB.
        let _boundary = self.memory_boundary.locked();
        #[cfg(test)]
        {
            let before = self.before_team_settings_write.locked().take();
            if let Some(before) = before {
                before();
            }
        }
        let c = self.db.locked();
        let tx = c.unchecked_transaction()?;
        let previous = read(&tx)?;
        let mut value = serde_json::to_value(&previous)?;
        for (key, val) in patch.as_object().context("team settings must be an object")? {
            if !matches!(key.as_str(), "primaryBotId" | "permission" | "workspaceRoot") {
                bail!("unknown team setting: {key}");
            }
            value[key] = val.clone();
        }
        let mut settings: TeamSettings = serde_json::from_value(value)?;
        let bots: Vec<BotConfig> = tx
            .prepare(&crate::fork_avatars::profile_query("SELECT {profile} FROM bots WHERE deleted = 0"))?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .collect::<Result<_>>()?;
        if let Some(id) = &settings.primary_bot_id
            && !bots.iter().any(|b| b.id == *id && !b.is_group())
        {
            if patch.get("primaryBotId").is_some() {
                bail!("primaryBotId must name an existing agent bot");
            }
            settings.primary_bot_id = None;
        }
        let mut changed = Vec::new();
        for mut cfg in bots.iter().cloned() {
            let before = serde_json::to_value(&cfg)?;
            let primary = settings.primary_bot_id.as_deref() == Some(&cfg.id);
            cfg.manage_team = primary && (cfg.manage_team || patch.get("primaryBotId").is_some());
            if primary {
                cfg.auto_name = false;
            }
            if let Some(creator) = &cfg.created_by
                && patch.get("permission").is_some()
                && !cfg.permission_chosen_by_user
            {
                let creator_permission =
                    bots.iter().find(|b| b.id == *creator).map_or(Permission::Ask, |b| b.permission);
                cfg.permission = restrictive(creator_permission, settings.permission);
            }
            if before != serde_json::to_value(&cfg)? {
                tx.execute(
                    "UPDATE bots SET config = ?2, rev = ?3 WHERE id = ?1",
                    params![cfg.id, serde_json::to_string(&cfg)?, next_rev(&tx)?],
                )?;
            }
            if before != serde_json::to_value(&cfg)?
                || (previous.primary_bot_id != settings.primary_bot_id
                    && (primary || previous.primary_bot_id.as_deref() == Some(&cfg.id)))
            {
                changed.push(cfg);
            }
        }
        tx.execute(
            "INSERT INTO kv(k, v) VALUES(?1, ?2) ON CONFLICT(k) DO UPDATE SET v = ?2",
            params![KEY, serde_json::to_string(&settings)?],
        )?;
        // D368: o Berga escolhido num Hub preservado leva suas instruções antigas uma única vez.
        crate::fork::donna_fixa::migrar(&tx)?;
        for cfg in &mut changed {
            if let Some(row) = Store::bot_on(&tx, &cfg.id)? {
                *cfg = row.config;
            }
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Perfil e lote de rotinas são confirmados antes de qualquer aviso ao ator.
    pub(crate) fn save_team_update_in_execution(
        &self,
        cfg: &BotConfig,
        routines: &str,
        freeze: impl FnOnce(&BotConfig) -> Result<(String, String)>,
        execution: Option<&crate::fork::sessoes::ExecutionRef>,
        origin: ManagementOrigin<'_, impl FnOnce() -> bool>,
    ) -> Result<BotConfig> {
        let mut c = self.db.locked();
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        crate::fork::sessoes::bindings::check_optional(&tx, execution)?;
        let tx = origin.guard(tx)?;
        let raw: String = tx.query_row(
            &crate::fork_avatars::profile_query("SELECT {profile} FROM bots WHERE id = ? AND deleted = 0"),
            [&cfg.id],
            |r| r.get(0),
        )?;
        let current: BotConfig = serde_json::from_str(&raw)?;
        // Conectores/skills também mudam fora de team_lock. A ferramenta só grava seu escopo.
        let mut saved = restore_team_profile(&current, cfg);
        if saved.name != current.name {
            saved.auto_name = false;
        }
        super::bots::save_bot(&tx, &saved)?;
        save_team_state(&tx, routines, &freeze(&saved)?)?;
        tx.commit()?;
        Ok(saved)
    }

    pub(crate) fn save_team_creation_in_execution(
        &self,
        cfg: &BotConfig,
        routines: &str,
        pending: &(String, String),
        creator_skills: &str,
        execution: Option<&crate::fork::sessoes::ExecutionRef>,
        origin: ManagementOrigin<'_, impl FnOnce() -> bool>,
    ) -> Result<()> {
        let mut c = self.db.locked();
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        crate::fork::sessoes::bindings::check_optional(&tx, execution)?;
        let tx = origin.guard(tx)?;
        if let Some(creator) = &cfg.created_by {
            crate::fork::skill_selection::confirm_creator(&tx, creator, creator_skills)?;
        }
        super::bots::save_bot(&tx, cfg)?;
        save_team_state(&tx, routines, pending)?;
        tx.commit()?;
        Ok(())
    }

    /// Estado de controle durável: falha de leitura não equivale a lista vazia.
    pub(crate) fn pending_team_changes(&self) -> Result<Vec<(String, String)>> {
        Ok(self
            .db
            .locked()
            .prepare("SELECT k, v FROM kv WHERE k GLOB 'team.pending.*' ORDER BY k")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn quarantine_team_change(&self, key: &str, raw: &str, error: &str) -> Result<()> {
        let c = self.db.locked();
        let tx = c.unchecked_transaction()?;
        let quarantine = key.replacen("team.pending.", "team.quarantine.", 1);
        let value = serde_json::json!({"pending": raw, "error": error, "at": super::now_ms()});
        tx.execute(
            "INSERT INTO kv(k, v) VALUES(?1, ?2) ON CONFLICT(k) DO UPDATE SET v = ?2",
            params![quarantine, value.to_string()],
        )?;
        tx.execute("DELETE FROM kv WHERE k = ?", [key])?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn discard_team_creation(&self, id: &str, pending_id: &str, routines: &str) -> Result<()> {
        let c = self.db.locked();
        let tx = c.unchecked_transaction()?;
        let rev = next_rev(&tx)?;
        tx.execute("UPDATE bots SET deleted = 1, session_id = NULL, rev = ?2 WHERE id = ?1", params![id, rev])?;
        super::bot_groups::forget_bot(&tx, id, rev)?;
        crate::fork::on_bot_deleted(&tx, id)?;
        tx.execute("INSERT INTO kv(k, v) VALUES('routines.v1', ?1) ON CONFLICT(k) DO UPDATE SET v = ?1", [routines])?;
        crate::fork::routine_creation::external_grant::prune(&tx, routines)?;
        tx.execute("DELETE FROM kv WHERE k = ?", [format!("team.pending.{pending_id}")])?;
        tx.commit()?;
        Ok(())
    }

    /// Finaliza journal e aviso na mesma transação; o pending só sai após ambos existirem.
    pub(crate) fn finish_team_change(
        &self,
        id: &str,
        source: &str,
        turn: i64,
        data: &Value,
        journal: Option<&str>,
    ) -> Result<Option<super::Entry>> {
        let c = self.db.locked();
        let tx = c.unchecked_transaction()?;
        if let Some(journal) = journal {
            tx.execute(
                "INSERT INTO kv(k, v) VALUES(?1, ?2) ON CONFLICT(k) DO UPDATE SET v = ?2",
                params![format!("team.undo.{id}"), journal],
            )?;
        }
        // O aviso fica na principal: o marcador de sessão entra antes dele, se a conversa vinha de outra sessão.
        let marker = super::entries::insert_marker(&tx, &super::Lane::main(source), turn)?;
        let now = super::now_ms();
        let added = tx.execute(
            "INSERT INTO entries(id, bot_id, rev, kind, turn, data, created_at, updated_at)
            SELECT ?1, ?2, ?3, 'notice', ?4, ?5, ?6, ?6
            WHERE EXISTS(SELECT 1 FROM bots WHERE id = ?2 AND deleted = 0)
            ON CONFLICT(id) DO NOTHING",
            params![id, source, next_rev(&tx)?, turn, data.to_string(), now],
        )?;
        if added != 1 {
            bail!("couldn't publish team change notice");
        }
        tx.execute("DELETE FROM kv WHERE k = ?", [format!("team.pending.{id}")])?;
        tx.commit()?;
        Ok(marker)
    }

    /// Confere o snapshot e consome o Desfazer na mesma transação que restaura o estado.
    /// A criação é marcada como apagada antes de soltar o lock: uma mensagem concorrente
    /// não pode entrar entre a checagem de conversa e a exclusão.
    pub(crate) fn commit_team_undo(
        &self,
        entry_id: &str,
        expected: &BotConfig,
        before: Option<&BotConfig>,
        routines: &str,
    ) -> Result<Option<BotConfig>> {
        let c = self.db.locked();
        let tx = c.unchecked_transaction()?;
        let settings = read(&tx)?;
        if settings.primary_bot_id.as_deref() == Some(&expected.id) {
            bail!("only the user may change the primary bot in settings");
        }
        let (raw, session): (String, Option<String>) = tx.query_row(
            &crate::fork_avatars::profile_query("SELECT {profile}, session_id FROM bots WHERE id = ? AND deleted = 0"),
            [&expected.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let current: BotConfig = serde_json::from_str(&raw)?;
        if !same_team_profile(&current, expected) {
            bail!("bot changed after this action; later edits were preserved");
        }
        let data: String =
            tx.query_row("SELECT data FROM entries WHERE id = ? AND kind = 'notice'", [entry_id], |r| r.get(0))?;
        let data: Value = serde_json::from_str(&data)?;
        if data["teamChange"]["status"] != "available" || data["teamChange"]["targetBotId"] != expected.id {
            bail!("team change is not available to undo");
        }
        let restored = if let Some(before) = before {
            let mut cfg = restore_team_profile(&current, before);
            cfg.manage_team = false;
            if cfg.created_by.is_some() && !cfg.permission_chosen_by_user {
                cfg.permission = restrictive(cfg.permission, settings.permission);
            }
            // D366: o Desfazer não devolve o nome reservado a outro bot enquanto o Berga existir.
            crate::fork::donna_fixa::check_save(&tx, &cfg, settings.primary_bot_id.as_deref())?;
            tx.execute(
                "UPDATE bots SET config = ?2, rev = ?3 WHERE id = ?1",
                params![cfg.id, serde_json::to_string(&cfg)?, next_rev(&tx)?],
            )?;
            Some(cfg)
        } else {
            if session.is_some() || has_conversations(&tx, &expected.id)? {
                bail!("this bot already has conversations; only the user may delete it in settings");
            }
            let rev = next_rev(&tx)?;
            tx.execute(
                "UPDATE bots SET deleted = 1, session_id = NULL, rev = ?2 WHERE id = ?1",
                params![expected.id, rev],
            )?;
            super::bot_groups::forget_bot(&tx, &expected.id, rev)?;
            crate::fork::on_bot_deleted(&tx, &expected.id)?;
            None
        };
        tx.execute("INSERT INTO kv(k, v) VALUES('routines.v1', ?1) ON CONFLICT(k) DO UPDATE SET v = ?1", [routines])?;
        crate::fork::routine_creation::external_grant::prune(&tx, routines)?;
        tx.execute("UPDATE entries SET data = json_set(data, '$.teamChange.status', 'undone'), rev = ?2, updated_at = ?3 WHERE id = ?1",
            params![entry_id, next_rev(&tx)?, super::now_ms()])?;
        tx.execute("DELETE FROM kv WHERE k = ?", [format!("team.undo.{entry_id}")])?;
        tx.commit()?;
        Ok(restored)
    }
}

/// Inclui conversas de grupo e pedidos entre bots; avisos de configuração não são conversas.
fn has_conversations(c: &Connection, bot: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM entries WHERE
            (bot_id = ?1 AND kind != 'notice') OR
            json_extract(data, '$.author') = ?1 OR
            json_extract(data, '$.sourceBotId') = ?1 OR
            json_extract(data, '$.targetBotId') = ?1 OR
            (bot_id = ?1 AND (json_extract(data, '$.runId') IS NOT NULL OR
                              json_extract(data, '$.botMessage') IS NOT NULL)))",
        [bot],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exposicao_antes_da_transacao_recusa_criacao_e_edicao_sem_gravar() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let mut cfg: BotConfig = serde_json::from_value(json!({"id":"dev", "name":"Dev"})).unwrap();
        store.save_bot(&cfg).unwrap();
        store.kv_set("routines.v1", "original").unwrap();
        store.kv_set("team.pending.test", "original").unwrap();
        assert!(!store.guard_management_exposed("lead", false).unwrap());
        assert!(store.guard_management_exposed("lead", true).unwrap());
        cfg.name = "Changed".into();
        assert!(
            store
                .save_team_update_in_execution(
                    &cfg,
                    "changed",
                    |_| { Ok(("team.pending.test".into(), "changed".into())) },
                    None,
                    ManagementOrigin { manager: "lead", observed_external: || false },
                )
                .is_err()
        );
        let created: BotConfig = serde_json::from_value(json!({"id":"new", "name":"New"})).unwrap();
        assert!(
            store
                .save_team_creation_in_execution(
                    &created,
                    "changed",
                    &("team.pending.test".into(), "changed".into()),
                    "",
                    None,
                    ManagementOrigin { manager: "lead", observed_external: || false },
                )
                .is_err()
        );
        assert_eq!(store.bot("dev").unwrap().unwrap().config.name, "Dev");
        assert!(store.bot("new").unwrap().is_none());
        assert_eq!(store.kv_read("routines.v1").unwrap().as_deref(), Some("original"));
        assert_eq!(store.kv_read("team.pending.test").unwrap().as_deref(), Some("original"));
    }

    #[test]
    fn exposicao_observada_na_transacao_recusa_criacao_e_edicao_sem_gravar() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let mut cfg: BotConfig = serde_json::from_value(json!({"id":"dev", "name":"Dev"})).unwrap();
        store.save_bot(&cfg).unwrap();
        store.kv_set("routines.v1", "original").unwrap();
        store.kv_set("team.pending.test", "original").unwrap();
        cfg.name = "Changed".into();
        let error = store
            .save_team_update_in_execution(
                &cfg,
                "changed",
                |_| panic!("não deve preparar escrita"),
                None,
                ManagementOrigin { manager: "editor", observed_external: || true },
            )
            .unwrap_err();
        assert!(error.to_string().contains("conteúdo externo"));
        let created: BotConfig = serde_json::from_value(json!({"id":"new", "name":"New"})).unwrap();
        let error = store
            .save_team_creation_in_execution(
                &created,
                "changed",
                &("team.pending.test".into(), "changed".into()),
                "",
                None,
                ManagementOrigin { manager: "creator", observed_external: || true },
            )
            .unwrap_err();
        assert!(error.to_string().contains("conteúdo externo"));
        assert!(store.guard_management_exposed("editor", false).unwrap());
        assert!(store.guard_management_exposed("creator", false).unwrap());
        assert_eq!(store.bot("dev").unwrap().unwrap().config.name, "Dev");
        assert!(store.bot("new").unwrap().is_none());
        assert_eq!(store.kv_read("routines.v1").unwrap().as_deref(), Some("original"));
        assert_eq!(store.kv_read("team.pending.test").unwrap().as_deref(), Some("original"));
    }

    #[test]
    fn general_autonomy_recalculates_children_and_survives_reopening() {
        let dir = std::env::temp_dir().join(format!("d67-autonomy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.db");
        let store = Store::open(&path).unwrap();
        for value in [
            json!({"id":"lead", "name":"Donna", "permission":"auto"}),
            json!({"id":"careful", "name":"Careful", "permission":"ask"}),
            json!({"id":"child", "name":"Child", "createdBy":"lead", "permission":"ask"}),
            json!({"id":"restricted", "name":"Restricted", "createdBy":"careful", "permission":"ask"}),
            json!({"id":"orphan", "name":"Orphan", "createdBy":"missing", "permission":"ask"}),
        ] {
            store.save_bot(&serde_json::from_value(value).unwrap()).unwrap();
        }
        for (general, child) in [("auto", Permission::Auto), ("ask", Permission::Ask), ("auto", Permission::Auto)] {
            store.set_team_settings(&json!({"permission":general})).unwrap();
            assert_eq!(store.bot("child").unwrap().unwrap().config.permission, child);
            for id in ["careful", "restricted", "orphan"] {
                assert_eq!(store.bot(id).unwrap().unwrap().config.permission, Permission::Ask);
            }
            assert_eq!(store.bot("lead").unwrap().unwrap().config.permission, Permission::Auto);
        }
        drop(store);
        let store = Store::open(&path).unwrap();
        assert_eq!(store.team_settings().unwrap().permission, Permission::Auto);
        assert_eq!(store.bot("child").unwrap().unwrap().config.permission, Permission::Auto);
        assert_eq!(store.bot("restricted").unwrap().unwrap().config.permission, Permission::Ask);
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn deleting_primary_clears_its_id_and_unrelated_patches_repair_stale_ids() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let cfg: BotConfig = serde_json::from_value(json!({"id":"lead", "name":"Donna"})).unwrap();
        store.save_bot(&cfg).unwrap();
        store.set_team_settings(&json!({"primaryBotId":"lead"})).unwrap();
        assert!(store.delete_bot("lead").is_err(), "D366: a Donna viva não sai");
        store.sem_donna_fixa(); // D366: mecânica legada, sem Donna fixa.
        store.delete_bot("lead").unwrap();
        assert_eq!(store.team_settings().unwrap().primary_bot_id, None);
        store.kv_set(KEY, r#"{"primaryBotId":"deleted","permission":"auto"}"#).unwrap();
        store.set_team_settings(&json!({"permission":"ask"})).unwrap();
        assert_eq!(store.team_settings().unwrap().primary_bot_id, None);
        assert_eq!(store.team_settings().unwrap().permission, Permission::Ask);
        assert!(store.set_team_settings(&json!({"primaryBotId":"missing"})).is_err());
    }

    #[test]
    fn unreadable_settings_use_safe_defaults_and_do_not_block_bot_saves() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        for raw in ["invalid JSON", r#"{"primaryBotId":7}"#, r#"{"permission":"invalid"}"#] {
            store.kv_set(KEY, raw).unwrap();
            assert_eq!(store.team_settings().unwrap(), TeamSettings::default());
            let cfg: BotConfig = serde_json::from_value(json!({"id":"dev", "name":"Dev"})).unwrap();
            store.save_bot(&cfg).unwrap();
            store.restrict_legacy_managers().unwrap();
        }
        store.kv_set(KEY, r#"{"primaryBotId":"dev","permission":"ask","futureField":true}"#).unwrap();
        assert_eq!(store.team_settings().unwrap().primary_bot_id.as_deref(), Some("dev"));
        assert!(store.set_team_settings(&json!({"futureField":true})).is_err());
    }

    #[test]
    fn team_update_preserves_concurrent_changes_outside_its_editable_scope() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let mut cfg: BotConfig = serde_json::from_value(json!({"id":"dev", "name":"Dev", "autoName":true})).unwrap();
        store.save_bot(&cfg).unwrap();
        let mut current = cfg.clone();
        current.connectors = vec!["new-connector".into()];
        current.model = Some("user-model".into());
        store.save_bot(&current).unwrap();
        cfg.name = "Changed".into();
        store.kv_set("team.pending.test", "prepared").unwrap();
        let saved = store
            .save_team_update_in_execution(
                &cfg,
                r#"{"routines":[],"runs":[]}"#,
                |cfg| Ok(("team.pending.test".into(), serde_json::to_string(cfg)?)),
                None,
                ManagementOrigin { manager: "lead", observed_external: || false },
            )
            .unwrap();
        assert_eq!(saved.name, "Changed");
        assert_eq!(saved.connectors, ["new-connector"]);
        assert_eq!(saved.model.as_deref(), Some("user-model"));
        assert!(!saved.auto_name);
    }

    #[test]
    fn unreadable_settings_do_not_block_reopening_or_saving_the_old_primary() {
        let dir = std::env::temp_dir().join(format!("d67-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.db");
        let store = Store::open(&path).unwrap();
        let cfg: BotConfig = serde_json::from_value(json!({"id":"lead", "name":"Donna"})).unwrap();
        store.save_bot(&cfg).unwrap();
        store.set_team_settings(&json!({"primaryBotId":"lead"})).unwrap();
        store.set_team_settings(&json!({"permission":"auto"})).unwrap();
        let created: BotConfig =
            serde_json::from_value(json!({"id":"child", "name":"Child", "createdBy":"lead"})).unwrap();
        store.save_bot(&created).unwrap();
        assert!(store.kv_set(KEY, "invalid JSON").is_err(), "D366: a Donna viva não perde a principal");
        store.sem_donna_fixa(); // D366: banco corrompido por fora, sem a trava.
        store.kv_set(KEY, "invalid JSON").unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        let mut cfg = store.bot("lead").unwrap().unwrap().config;
        assert!(!cfg.manage_team);
        cfg.name = "Renamed".into();
        store.save_bot(&cfg).unwrap();
        let mut created = store.bot("child").unwrap().unwrap().config;
        assert_eq!(created.permission, Permission::Auto, "opening the store must preserve the user's choice");
        created.name = "Edited child".into();
        created.permission = Permission::Ask;
        store.save_bot(&created).unwrap();
        assert_eq!(store.team_settings().unwrap(), TeamSettings::default());
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
