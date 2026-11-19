//! Procedência no JSON da própria entrada: mesma gravação, sync e retenção do chat.
use serde_json::Value;

#[path = "../fork/routine_save/store.rs"]
mod bound_save;

/// Mudanças de status preservam o registro. Editar/restaurar conteúdo o invalida;
/// quem atualiza uma entrada não pode substituí-lo por uma nova alegação humana.
pub(super) fn preserve(before: &Value, next: &Value) -> Value {
    let mut next = next.clone();
    if before.get("contentOrigin").is_some() {
        let unchanged = ["text", "attachments", "replyTo"].iter().all(|k| before[k] == next[k]);
        next["contentOrigin"] =
            if unchanged { before["contentOrigin"].clone() } else { crate::guard_hub::origem::invalidate(&next) };
    } else if let Some(object) = next.as_object_mut() {
        object.remove("contentOrigin"); // Legado não ganha uma evidência retroativa.
    }
    next
}

/// Migração aditiva do fork. Não altera o schema ou o mirror do upstream.
pub(super) fn migrate(db: &rusqlite::Connection) -> anyhow::Result<()> {
    let tx = db.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_guard_setup(
            id TEXT PRIMARY KEY, connector TEXT NOT NULL, expires_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS donna_guard_contexts(
            id TEXT PRIMARY KEY, bot TEXT NOT NULL, connector TEXT NOT NULL,
            lane TEXT NOT NULL, run TEXT NOT NULL, data TEXT NOT NULL,binding TEXT NOT NULL DEFAULT '',
            UNIQUE(bot, connector, lane, run,binding));
         CREATE TABLE IF NOT EXISTS donna_guard_events(
            id TEXT PRIMARY KEY, created_at INTEGER NOT NULL, data TEXT NOT NULL);
         CREATE INDEX IF NOT EXISTS donna_guard_events_time ON donna_guard_events(created_at);
         CREATE TABLE IF NOT EXISTS donna_guard_state(
            bot TEXT NOT NULL, lane TEXT NOT NULL, external_exposed INTEGER NOT NULL,
            PRIMARY KEY(bot, lane));
         INSERT INTO donna_guard_state(bot,lane,external_exposed)
            SELECT bot,'*',1 FROM donna_guard_state WHERE external_exposed=1 GROUP BY bot
            ON CONFLICT(bot,lane) DO UPDATE SET external_exposed=1;
         DELETE FROM donna_guard_state WHERE lane<>'*';",
    )?;
    let has_binding: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('donna_guard_contexts') WHERE name='binding')",
        [],
        |r| r.get(0),
    )?;
    if !has_binding {
        tx.execute_batch("ALTER TABLE donna_guard_contexts RENAME TO donna_guard_contexts_legacy;
        CREATE TABLE donna_guard_contexts(id TEXT PRIMARY KEY,bot TEXT NOT NULL,connector TEXT NOT NULL,
            lane TEXT NOT NULL,run TEXT NOT NULL,data TEXT NOT NULL,binding TEXT NOT NULL DEFAULT '',
            UNIQUE(bot,connector,lane,run,binding));
        INSERT INTO donna_guard_contexts(id,bot,connector,lane,run,data) SELECT id,bot,connector,lane,run,data FROM donna_guard_contexts_legacy;
        DROP TABLE donna_guard_contexts_legacy;")?;
    }
    tx.commit()?;
    Ok(())
}

/// Revogação de bot/conector e retenção não consultam o histórico de rotinas.
/// Runs encerrados são removidos no commit do scheduler, a partir do estado em memória.
pub(super) fn prune(db: &rusqlite::Connection) -> anyhow::Result<Vec<String>> {
    let removed = db
        .prepare(
            "DELETE FROM donna_guard_contexts AS c WHERE
            NOT EXISTS(SELECT 1 FROM bots b, json_each(b.config, '$.connectors') connector
                WHERE b.id=c.bot AND b.deleted=0 AND connector.value=c.connector)
            RETURNING id",
        )?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    db.execute("DELETE FROM donna_guard_events WHERE created_at < ?1", [super::now_ms() - 30 * 24 * 60 * 60 * 1000])?;
    Ok(removed)
}

/// Costura SQL interna da Store: não abre conexão/transação nem chama Store/Hub.
/// O chamador confirma o commit antes de descartar os contextos em memória.
pub(super) fn routines_commit_in<'a>(
    tx: &rusqlite::Connection,
    raw: &str,
    live_runs: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> anyhow::Result<Vec<String>> {
    let live_runs: std::collections::HashSet<_> = live_runs.into_iter().collect();
    tx.execute("INSERT INTO kv(k,v) VALUES('routines.v1',?1) ON CONFLICT(k) DO UPDATE SET v=excluded.v", [raw])?;
    crate::fork::routine_creation::external_grant::prune(tx, raw)?;
    crate::fork::cadeias::store::link_runs(tx, raw)?;
    let mut removed = prune(tx)?;
    let ended = tx
        .prepare("SELECT id,bot,run FROM donna_guard_contexts WHERE run<>''")?
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, bot, run) in ended {
        if !live_runs.contains(&(bot.as_str(), run.as_str())) {
            tx.execute("DELETE FROM donna_guard_contexts WHERE id=?1", [&id])?;
            removed.push(id);
        }
    }
    Ok(removed)
}

impl super::Store {
    /// Uma entrada marcada como externa foi entregue ao contexto. Não guarda texto ou metadados pessoais.
    pub(crate) fn guard_mark_external(&self, bot: &str) -> anyhow::Result<()> {
        use crate::LockExt;
        Self::guard_mark_external_on(&self.db.locked(), bot)
    }

    /// Mesma marca D125, na Connection do consumer antes de entregar bytes externos.
    pub(crate) fn guard_mark_external_on(db: &rusqlite::Connection, bot: &str) -> anyhow::Result<()> {
        db.execute(
            "INSERT INTO donna_guard_state(bot,lane,external_exposed) VALUES(?1,'*',1)
             ON CONFLICT(bot,lane) DO UPDATE SET external_exposed=1",
            [bot],
        )?;
        Ok(())
    }

    /// A exposição pertence ao contexto do bot, inclusive depois de trocar de conversa/sessão.
    #[cfg(test)]
    pub(crate) fn guard_external_exposed(&self, bot: &str) -> anyhow::Result<bool> {
        use crate::LockExt;
        Ok(self.db.locked().query_row(
            "SELECT EXISTS(SELECT 1 FROM donna_guard_state WHERE bot=?1 AND external_exposed=1)",
            [bot],
            |row| row.get(0),
        )?)
    }

    /// Namespace persistente por banco; hubs de teste com IDs iguais não compartilham segredo.
    pub(crate) fn guard_runtime_namespace(&self) -> anyhow::Result<String> {
        use crate::LockExt;
        use rusqlite::OptionalExtension;
        let db = self.db.locked();
        if let Some(namespace) =
            db.query_row("SELECT v FROM kv WHERE k='donna.guard.runtime.namespace'", [], |row| row.get(0)).optional()?
        {
            return Ok(namespace);
        }
        db.execute(
            "INSERT OR IGNORE INTO kv(k,v) VALUES('donna.guard.runtime.namespace',?1)",
            [uuid::Uuid::new_v4().to_string()],
        )?;
        Ok(db.query_row("SELECT v FROM kv WHERE k='donna.guard.runtime.namespace'", [], |row| row.get(0))?)
    }

    pub(crate) fn guard_context_remove(&self, id: &str) -> anyhow::Result<()> {
        use crate::LockExt;
        let namespace = self.guard_runtime_namespace()?;
        self.db.locked().execute("DELETE FROM donna_guard_contexts WHERE id=?1", [id])?;
        self.db.locked().execute("DELETE FROM donna_guard_setup WHERE id=?1", [id])?;
        crate::guard_hub::mcp::runtime::forget_contexts(&namespace, &[id.into()]);
        Ok(())
    }

    /// D82 conserva a seleção no perfil: remover o conector revoga pelo ID explícito.
    pub(crate) fn guard_connector_removed(&self, connector: &str) -> anyhow::Result<()> {
        use crate::LockExt;
        let namespace = self.guard_runtime_namespace()?;
        let mut db = self.db.locked();
        let tx = db.transaction()?;
        let mut removed = tx
            .prepare("DELETE FROM donna_guard_contexts WHERE connector=?1 RETURNING id")?
            .query_map([connector], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        removed.extend(
            tx.prepare("DELETE FROM donna_guard_setup WHERE connector=?1 RETURNING id")?
                .query_map([connector], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?,
        );
        removed.extend(prune(&tx)?);
        tx.commit()?;
        drop(db);
        crate::guard_hub::mcp::runtime::forget_contexts(&namespace, &removed);
        Ok(())
    }

    /// Gancho para revogação; mesma transação da retenção e sem depender de nova mensagem MCP.
    pub(crate) fn guard_cleanup(&self) -> anyhow::Result<()> {
        use crate::LockExt;
        let namespace = self.guard_runtime_namespace()?;
        let mut db = self.db.locked();
        let tx = db.transaction()?;
        let removed = prune(&tx)?;
        tx.commit()?;
        drop(db);
        crate::guard_hub::mcp::runtime::forget_contexts(&namespace, &removed);
        Ok(())
    }

    /// Estado do scheduler, contextos encerrados e retenção confirmam juntos.
    pub(crate) fn guard_routines_commit<'a>(
        &self,
        raw: &str,
        live_runs: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> anyhow::Result<()> {
        self.guard_routines_commit_bound(None, raw, live_runs)
    }

    pub(crate) fn guard_context_save(&self, context: &crate::guard_hub::mcp::contexto::Context) -> anyhow::Result<()> {
        use crate::LockExt;
        let mut db = self.db.locked();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if let Some(binding) = context.binding.as_deref() {
            crate::fork::sessoes::bindings::resolve(&tx, binding, &context.bot)?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO donna_guard_contexts(id, bot, connector, lane, run, data, binding) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![context.id, context.bot, context.connector, context.lane,
                context.run.as_deref().unwrap_or_default(), serde_json::to_string(context)?, context.binding.as_deref().unwrap_or_default()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn guard_context_for(
        &self,
        bot: &str,
        connector: &str,
        lane: &str,
        run: Option<&str>,
        binding: Option<&str>,
    ) -> anyhow::Result<Option<crate::guard_hub::mcp::contexto::Context>> {
        use crate::LockExt;
        use rusqlite::OptionalExtension;
        let id: Option<String> = self
            .db
            .locked()
            .query_row(
                "SELECT id FROM donna_guard_contexts WHERE bot=?1 AND connector=?2 AND lane=?3 AND run=?4 AND binding=?5",
                rusqlite::params![bot, connector, lane, run.unwrap_or_default(), binding.unwrap_or_default()],
                |r| r.get(0),
            )
            .optional()?;
        id.map(|id| self.guard_context(&id)).transpose().map(Option::flatten)
    }

    pub(crate) fn guard_context(&self, id: &str) -> anyhow::Result<Option<crate::guard_hub::mcp::contexto::Context>> {
        use crate::LockExt;
        use rusqlite::OptionalExtension;
        let data: Option<String> = self
            .db
            .locked()
            .query_row("SELECT data FROM donna_guard_contexts WHERE id=?1", [id], |r| r.get(0))
            .optional()?;
        data.map(|v| serde_json::from_str(&v).map_err(Into::into)).transpose()
    }

    /// Recibo e exposição são confirmados na mesma transação, antes de liberar o retorno.
    #[cfg(test)]
    pub(crate) fn guard_observe(
        &self,
        context: &crate::guard_hub::mcp::contexto::Context,
        event: &Value,
    ) -> anyhow::Result<()> {
        self.guard_record(Some(context), event)
    }

    #[cfg(test)]
    pub(crate) fn guard_record(
        &self,
        context: Option<&crate::guard_hub::mcp::contexto::Context>,
        event: &Value,
    ) -> anyhow::Result<()> {
        self.guard_record_checked(context, event, None)
    }

    pub(crate) fn guard_record_in_intent(
        &self,
        event: &Value,
        intent: &crate::fork::conectores::oauth_runtime::RuntimeIntent,
    ) -> anyhow::Result<()> {
        #[cfg(test)]
        crate::fork::sessoes::call_tests::before_effect(self, "observe_commit");
        self.guard_record_checked(intent.context(), event, Some(intent))
    }

    fn guard_record_checked(
        &self,
        context: Option<&crate::guard_hub::mcp::contexto::Context>,
        event: &Value,
        intent: Option<&crate::fork::conectores::oauth_runtime::RuntimeIntent>,
    ) -> anyhow::Result<()> {
        use crate::LockExt;
        let mut db = self.db.locked();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let late = intent.is_some_and(crate::fork::conectores::oauth_runtime::RuntimeIntent::late_notification);
        Self::guard_record_on(&tx, context, event, late, |c| {
            if let Some(intent) = intent {
                intent.validate_db(c)?;
            } else if let Some(context) = context {
                let binding = context.binding.as_deref().ok_or_else(|| anyhow::anyhow!("chamada vencida"))?;
                let origin = crate::fork::sessoes::bindings::resolve(c, binding, &context.bot)?;
                crate::fork::sessoes::bindings::assert_binding_live(c, &origin)?;
            }
            Ok(())
        })?;
        tx.commit()?;
        Ok(())
    }

    /// O produtor conserva suas cercas; valida na mesma transação antes do primeiro efeito.
    pub(crate) fn guard_record_on(
        tx: &rusqlite::Transaction<'_>,
        context: Option<&crate::guard_hub::mcp::contexto::Context>,
        event: &Value,
        late: bool,
        validate: impl FnOnce(&rusqlite::Connection) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        validate(tx)?;
        let c: &rusqlite::Connection = tx;
        if let Some(context) = context {
            let binding = context.binding.as_deref().ok_or_else(|| anyhow::anyhow!("chamada vencida"))?;
            crate::fork::sessoes::bindings::resolve(c, binding, &context.bot)?;
            if event["decision"] == "sanitized" || late {
                crate::fork::memory::gateway::observe_conn(c, &context.bot, binding)?;
            }
        }
        c.execute(
            "INSERT INTO donna_guard_events(id, created_at, data) VALUES(?1,?2,?3)",
            rusqlite::params![event["receipt"].as_str(), super::now_ms(), event.to_string()],
        )?;
        if let Some(context) = context.filter(|_| !late) {
            c.execute(
                "INSERT INTO donna_guard_state(bot,lane,external_exposed) VALUES(?1,?2,1)
                ON CONFLICT(bot,lane) DO UPDATE SET external_exposed=1",
                rusqlite::params![context.bot, "*"],
            )?;
        }
        // Cada mensagem entregue recebe recibo próprio. Agregar progress/log aqui
        // perderia essa ligação; fica para o contrato de contagens do painel 22.3.
        // RuntimeIntent revalida o run em memória; nunca varrer o KV do scheduler por frame.
        c.execute(
            "DELETE FROM donna_guard_events WHERE created_at < ?1",
            [super::now_ms() - 30 * 24 * 60 * 60 * 1000],
        )?;
        Ok(())
    }
}

#[cfg(test)]
impl super::Store {
    pub(crate) fn guard_test_snapshot(&self) -> anyhow::Result<(Vec<String>, i64)> {
        use crate::LockExt;
        let db = self.db.locked();
        let events = db
            .prepare("SELECT data FROM donna_guard_events")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        let exposed =
            db.query_row("SELECT COALESCE(MAX(external_exposed),0) FROM donna_guard_state", [], |r| r.get(0))?;
        Ok((events, exposed))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::store::{EntryKind, Lane, Store};
    use serde_json::json;

    fn gateway_store() -> Store {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let bot = serde_json::from_value(json!({"id":"bot","name":"Fixture","connectors":["connector"]})).unwrap();
        store.save_bot(&bot).unwrap();
        let binding = crate::fork::sessoes::bindings::issue(&store, "bot", "bot", "bot", None).unwrap();
        crate::fork::sessoes::bindings::activate(&store, &binding, "fixture-acp").unwrap();
        store
    }

    fn gateway_context(store: &Store, run: Option<&str>) -> crate::guard_hub::mcp::contexto::Context {
        let binding = crate::fork::sessoes::bindings::for_acp(store, "bot", "fixture-acp").unwrap();
        crate::guard_hub::mcp::contexto::Context::issue_bound(
            store,
            "bot",
            "connector",
            "bot",
            run,
            Some(binding.id.as_str()),
        )
        .unwrap()
    }

    #[test]
    fn snapshot_de_teste_detecta_qualquer_marca_global_e_propaga_erro_sql() {
        use crate::LockExt;
        let store = gateway_store();
        assert_eq!(store.guard_test_snapshot().unwrap(), (vec![], 0));
        store.db.locked().execute("INSERT INTO donna_guard_state VALUES('bot','*',0),('other','*',1)", []).unwrap();
        assert_eq!(store.guard_test_snapshot().unwrap(), (vec![], 1));
        store.db.locked().execute_batch("DROP TABLE donna_guard_state").unwrap();
        assert!(store.guard_test_snapshot().is_err());
    }

    #[test]
    fn observation_marks_the_whole_bot_and_retains_events_without_reading_routine_history() {
        use crate::{LockExt, guard_hub::mcp::runtime};
        use zeroize::Zeroizing;
        let store = gateway_store();
        // Se a observação ainda consultar json_each(routines.v1), falha com JSON inválido.
        store.kv_set("routines.v1", "not-json").unwrap();
        let main = gateway_context(&store, None);
        let live = gateway_context(&store, Some("live"));
        let finished = gateway_context(&store, Some("finished"));
        let foreign = gateway_context(&store, Some("foreign"));
        let namespace = store.guard_runtime_namespace().unwrap();
        for context in [&live, &finished, &foreign] {
            runtime::remember(&namespace, "connector", &context.id, "bridge", &[Zeroizing::new("synthetic123".into())]);
        }
        store.db.locked().execute("INSERT INTO donna_guard_events VALUES('old',0,'{}')", []).unwrap();
        store.guard_observe(&main, &json!({"receipt":"new"})).unwrap();
        let (events, exposed) = store.guard_test_snapshot().unwrap();
        assert_eq!(events, vec![r#"{"receipt":"new"}"#]);
        assert_eq!(exposed, 1);
        let db = store.db.locked();
        let lanes: Vec<String> = db
            .prepare("SELECT lane FROM donna_guard_state WHERE bot='bot'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(lanes, vec!["*"]);
        drop(db);
        for context in [&live, &finished, &foreign] {
            assert!(store.guard_context(&context.id).unwrap().is_some());
        }
        store.guard_routines_commit(
            r#"{"runs":[{"id":"live","botId":"bot","status":"running"},{"id":"foreign","botId":"other","status":"running"}]}"#,
            [("bot", "live"), ("other", "foreign")],
        ).unwrap();
        assert!(store.guard_context(&main.id).unwrap().is_some());
        assert!(store.guard_context(&live.id).unwrap().is_some());
        assert!(runtime::snapshot(&namespace, "connector", &live.id, Some("bridge")).is_some());
        for context in [&finished, &foreign] {
            assert!(store.guard_context(&context.id).unwrap().is_none());
            assert!(runtime::snapshot(&namespace, "connector", &context.id, Some("bridge")).is_none());
        }
        runtime::release(&namespace, "connector", &live.id, None);
    }

    #[test]
    fn routine_commit_releases_ended_run_and_migration_preserves_old_exposure() {
        use crate::{LockExt, guard_hub::mcp::runtime};
        use zeroize::Zeroizing;
        let store = gateway_store();
        let context = gateway_context(&store, Some("run"));
        let namespace = store.guard_runtime_namespace().unwrap();
        runtime::remember(&namespace, "connector", &context.id, "bridge", &[Zeroizing::new("synthetic123".into())]);
        store
            .guard_routines_commit(r#"{"runs":[{"id":"run","botId":"bot","status":"starting"}]}"#, [("bot", "run")])
            .unwrap();
        assert!(store.guard_context(&context.id).unwrap().is_some());
        store.guard_routines_commit(r#"{"runs":[{"id":"run","botId":"bot","status":"cancelled"}]}"#, []).unwrap();
        assert!(store.guard_context(&context.id).unwrap().is_none());
        assert!(runtime::snapshot(&namespace, "connector", &context.id, Some("bridge")).is_none());
        let db = store.db.locked();
        db.execute("INSERT INTO donna_guard_state VALUES('bot','group',1)", []).unwrap();
        migrate(&db).unwrap();
        let lane: String = db
            .query_row("SELECT lane FROM donna_guard_state WHERE bot='bot' AND external_exposed=1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(lane, "*");
    }

    #[test]
    fn connector_removal_revokes_context_and_cache_even_when_profile_keeps_selection() {
        use crate::guard_hub::mcp::runtime;
        use zeroize::Zeroizing;
        let store = gateway_store();
        let context = gateway_context(&store, None);
        let namespace = store.guard_runtime_namespace().unwrap();
        runtime::remember(&namespace, "connector", &context.id, "bridge", &[Zeroizing::new("synthetic123".into())]);
        store.guard_connector_removed("connector").unwrap();
        assert!(store.bot("bot").unwrap().unwrap().config.connectors.contains(&"connector".into()));
        assert!(store.guard_context(&context.id).unwrap().is_none());
        assert!(runtime::snapshot(&namespace, "connector", &context.id, Some("bridge")).is_none());
    }

    #[test]
    fn origin_survives_restart_sync_and_status_but_not_content_revision() {
        let dir = std::env::temp_dir().join(format!("guard-origin-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("chat.db");
        let store = Store::open(&path).unwrap();
        let mut data = json!({"text":"pedido", "attachments":[], "status":"queued"});
        let body = json!({"contentOrigin": {"schema":1, "draftId":uuid::Uuid::new_v4().to_string(), "revision":1,
            "textLength":6, "deviceId":uuid::Uuid::new_v4().to_string(), "channel":"desktopComposer", "interaction":"send",
            "segments":[{"start":0,"end":6,"kind":"typed"}]}});
        data["contentOrigin"] = crate::guard_hub::origem::capture(&crate::api::devices::Caller::Local, &body, &data);
        let entry = store.insert_entry(&Lane::main("b"), EntryKind::User, 1, &data).unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        assert_eq!(store.entries_since(0, 20).unwrap()[0].data, data);
        data["status"] = json!("sent");
        assert_eq!(
            store.update_entry(&entry.id, &data).unwrap().unwrap().data["contentOrigin"]["status"],
            "appReported"
        );
        assert_eq!(
            store.history("b", i64::MAX, 20).unwrap()[0].data["contentOrigin"]["ingress"],
            json!({"caller":"local"})
        );
        for (key, changed) in [("attachments", json!([{"id":"another"}])), ("replyTo", json!({"id":"another"}))] {
            let mut edited = data.clone();
            edited[key] = changed;
            assert_eq!(preserve(&data, &edited)["contentOrigin"]["status"], "unknown");
        }
        data["text"] = json!("outro!");
        let revised = store.update_entry(&entry.id, &data).unwrap().unwrap();
        assert_eq!(revised.data["contentOrigin"]["status"], "unknown");
        assert!(revised.data["contentOrigin"]["ingress"].is_null());
        data["text"] = json!("pedido");
        assert_eq!(store.update_entry(&entry.id, &data).unwrap().unwrap().data["contentOrigin"]["status"], "unknown");
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
        assert!(preserve(&json!({"text":"old"}), &data).get("contentOrigin").is_none());
    }
}
