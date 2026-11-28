//! Autoridade transitória do proxy: a mesma linha Context/Setup emitida pelo Hub.
//! Não é Caller/Origin de Control, Source do04, DTO ou registro de credenciais.
use super::{catalogo_servicos::Auth, oauth_binding};
use crate::{
    LockExt,
    fork::open_turn::LocalTurn,
    fork::sessoes::{
        bindings::{self, ExecutionRef},
        gateway::{self, Preparation},
    },
    guard_hub::mcp::{contexto::Context, setup},
    hub::{BotStatus, Hub},
    market::{self, Connector},
    store::now_ms,
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension};

pub(crate) struct RuntimeIntent {
    original: Original,
    method: String,
    execution: Option<ExecutionRef>,
    preparation: Option<Preparation>,
    late_notification: bool,
}

enum Original {
    Context(Context),
    Setup { id: String, connector: String, expires: i64 },
    Open { turn: LocalTurn, connector: String, vinculo: i64 },
}

fn denied() -> anyhow::Error {
    anyhow!("Contexto do gateway indisponível ou revogado")
}

fn context_row(db: &Connection, id: &str) -> Result<Option<Context>> {
    let row: Option<(String, String, String, String, String, String)> = db
        .query_row("SELECT bot,connector,lane,run,data,binding FROM donna_guard_contexts WHERE id=?1", [id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
        })
        .optional()?;
    let Some((bot, connector, lane, run, data, binding)) = row else { return Ok(None) };
    let context: Context = serde_json::from_str(&data).map_err(|_| denied())?;
    ensure!(
        context.id == id
            && context.bot == bot
            && context.connector == connector
            && context.lane == lane
            && context.run.as_deref().unwrap_or_default() == run
            && context.binding.as_deref() == Some(binding.as_str())
            && !binding.is_empty(),
        "{}",
        denied()
    );
    Ok(Some(context))
}

fn same_context(a: &Context, b: &Context) -> bool {
    a.id == b.id
        && a.bot == b.bot
        && a.connector == b.connector
        && a.lane == b.lane
        && a.run == b.run
        && a.binding == b.binding
}

impl RuntimeIntent {
    pub(crate) fn capture_open(hub: &Hub, turn: &LocalTurn, connector: &str) -> Result<Self> {
        turn.check_live(hub)?;
        let db = hub.store.db.locked();
        let vinculo = market::ferramentas::epoca_em(&db, &turn.lane().chat, connector)?;
        let intent = Self {
            original: Original::Open { turn: turn.clone(), connector: connector.into(), vinculo },
            method: String::new(),
            execution: None,
            preparation: None,
            late_notification: false,
        };
        intent.validate_db(&db)?;
        Ok(intent)
    }

    pub(crate) fn with_open_current<T>(
        &self,
        hub: &Hub,
        expected: &Connector,
        action: impl FnOnce(&mut Connection, &Connector) -> Result<T>,
    ) -> Result<T> {
        self.with_open_preflight(hub, expected, || Ok(()), action)
    }

    pub(crate) fn with_open_preflight<T, G>(
        &self,
        hub: &Hub,
        expected: &Connector,
        preflight: impl FnOnce() -> Result<G>,
        action: impl FnOnce(&mut Connection, &Connector) -> Result<T>,
    ) -> Result<T> {
        let Original::Open { turn, .. } = &self.original else { return Err(denied()) };
        self.validate_connector(expected)?;
        let _reservation = turn.reserve(hub)?;
        let _policy = preflight()?;
        let _guard = hub.store.connector_lock.locked();
        let (all, _) = market::load_connectors(&hub.store)?;
        let current = all.iter().find(|c| self.matches_target(c, expected)).ok_or_else(denied)?;
        let mut db = hub.store.db.locked();
        let bot = turn.check_db(&db)?;
        Self::validate_open_account(current, &bot)?;
        self.validate_db(&db)?;
        action(&mut db, current)
    }

    fn validate_open_account(saved: &Connector, bot: &crate::store::BotConfig) -> Result<()> {
        market::contas::exigir_mesmo_space(saved, &bot.space_id)?;
        let tipo = market::contas::tipo_da_conta(saved)?.ok_or_else(denied)?;
        let connected = match &tipo.auth {
            Auth::Oauth { .. } | Auth::AppProprio { .. } => {
                saved.oauth.as_ref().is_some_and(market::oauth::OAuth::signed_in)
            }
            Auth::Chave { destino: "header", nome, .. } => {
                saved.oauth.is_none() && saved.headers.get(*nome).is_some_and(|value| !value.trim().is_empty())
            }
            Auth::Chave { .. } => false,
        };
        ensure!(
            connected
                && saved.command.is_none()
                && saved.configured()
                && !saved.missing_catalog_header()
                && saved.headers.values().all(|value| !value.starts_with("op://"))
                && !saved.oauth_binding.as_ref().is_some_and(oauth_binding::Binding::scopes_retained)
                && market::ferramentas::politica(saved)?.is_some(),
            "Conector remoto indisponível"
        );
        Ok(())
    }

    pub(super) fn matches_target(&self, current: &Connector, expected: &Connector) -> bool {
        oauth_binding::same_target(current, expected)
            && (!matches!(self.original, Original::Open { .. })
                || (current.space == expected.space
                    && current.servico == expected.servico
                    && current.tipo == expected.tipo
                    && current.liberacoes == expected.liberacoes
                    && current.identidade == expected.identidade))
    }

    /// Captura antes do primeiro await; os campos vêm da linha Hub, não do JSON do cliente.
    pub(crate) fn capture(hub: &Hub, id: &str, connector: &str, method: &str) -> Result<Self> {
        Self::capture_mode(hub, id, connector, method, false)
    }

    pub(crate) fn capture_observation(hub: &Hub, id: &str, connector: &str, body: &serde_json::Value) -> Result<Self> {
        let late =
            body["method"] == "notification" && crate::guard_hub::mcp::notification_method(&body["frame"]).is_some();
        Self::capture_mode(hub, id, connector, body["method"].as_str().unwrap_or("unknown"), late)
    }

    fn capture_mode(hub: &Hub, id: &str, connector: &str, method: &str, late: bool) -> Result<Self> {
        let original = {
            let db = hub.store.db.locked();
            if let Some(expires) = setup::snapshot(&db, id, connector, method)? {
                Original::Setup { id: id.into(), connector: connector.into(), expires }
            } else {
                let context = context_row(&db, id)?.ok_or_else(denied)?;
                ensure!(context.connector == connector, "Conector não pertence ao contexto do gateway");
                Original::Context(context)
            }
        };
        let preparation = match &original {
            Original::Context(context) if !late && hub.runtime(&context.bot).session.is_none() => {
                Some(gateway::capture(
                    &hub.store.db.locked(),
                    context.binding.as_deref().ok_or_else(denied)?,
                    &context.bot,
                    method,
                )?)
            }
            _ => None,
        };
        let execution = match &original {
            Original::Context(context) if !late && preparation.is_none() => {
                Some(bindings::capture(hub, &context.bot, context.binding.as_deref().ok_or_else(denied)?)?)
            }
            _ => None,
        };
        let intent = Self { original, method: method.into(), execution, preparation, late_notification: late };
        {
            let db = hub.store.db.locked();
            let _ = intent.validate_db(&db)?;
        }
        intent.validate_live(hub)?;
        Ok(intent)
    }

    pub(crate) fn context(&self) -> Option<&Context> {
        match &self.original {
            Original::Context(context) => Some(context),
            Original::Setup { .. } | Original::Open { .. } => None,
        }
    }

    /// Execução Sessões3 desta chamada, para guardas complementares sem recaptura.
    pub(crate) fn execution(&self) -> Option<&ExecutionRef> {
        self.execution.as_ref()
    }

    pub(crate) fn late_notification(&self) -> bool {
        self.late_notification && self.context().is_some()
    }

    pub(crate) fn require_execution(&self, hub: &Hub) -> Result<()> {
        let execution = self.execution.as_ref().ok_or_else(denied)?;
        bindings::recheck(hub, execution)?;
        Ok(())
    }

    fn connector(&self) -> &str {
        match &self.original {
            Original::Context(context) => &context.connector,
            Original::Setup { connector, .. } | Original::Open { connector, .. } => connector,
        }
    }

    pub(crate) fn validate_connector(&self, expected: &Connector) -> Result<()> {
        ensure!(!self.late_notification(), "Notificação tardia não autoriza credenciais");
        ensure!(expected.id == self.connector(), "Conector não pertence ao contexto original");
        Ok(())
    }

    /// Só Connection/JSON/tempo. Retorna o limite original/atual do setup, sem renová-lo.
    pub(crate) fn validate_db(&self, db: &Connection) -> Result<Option<i64>> {
        match &self.original {
            Original::Context(expected) => {
                let current = context_row(db, &expected.id)?.ok_or_else(denied)?;
                ensure!(same_context(&current, expected), "{}", denied());
                bindings::resolve(db, expected.binding.as_deref().ok_or_else(denied)?, &expected.bot)?;
                bindings::check_optional(db, self.execution.as_ref())?;
                if let Some(preparation) = &self.preparation {
                    preparation.check(db)?;
                }
                ensure!(
                    self.late_notification || self.preparation.is_some() || self.execution.is_some(),
                    "{}",
                    denied()
                );
                let bot: Option<(String, Option<String>)> = db
                    .query_row(
                        "SELECT config,avatar_config FROM bots WHERE id=?1 AND deleted=0",
                        [&expected.bot],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?;
                let (profile, avatar) = bot.ok_or_else(denied)?;
                // Mesmo parser puro usado pelo Store; escalar/objeto não vira seleção.
                ensure!(
                    serde_json::from_str::<serde_json::Value>(&profile).is_ok_and(|value| value.is_object()),
                    "{}",
                    denied()
                );
                let bot = crate::fork_avatars::restored(&profile, avatar.as_deref()).map_err(|_| denied())?;
                ensure!(bot.id == expected.bot && bot.connectors.contains(&expected.connector), "{}", denied());
                Ok(None)
            }
            Original::Setup { id, connector, expires } => {
                let current = setup::snapshot(db, id, connector, &self.method)?.ok_or_else(denied)?;
                let deadline = (*expires).min(current);
                ensure!(deadline > now_ms(), "Contexto de setup indisponível");
                Ok(Some(deadline))
            }
            Original::Open { turn, connector, vinculo } => {
                let bot = turn.check_db(db)?;
                ensure!(bot.connectors.contains(connector), "{}", denied());
                market::ferramentas::conferir_vinculo(db, &bot.id, connector, *vinculo)?;
                Ok(None)
            }
        }
    }

    /// Revalida a execução original sem cleanup; nunca chamar segurando DB ou `connector_lock`.
    pub(crate) fn validate_live(&self, hub: &Hub) -> Result<()> {
        if let Original::Open { turn, .. } = &self.original {
            return turn.check_live(hub);
        }
        bindings::recheck_optional(hub, self.execution.as_ref())?;
        if let Some(context) = self.context() {
            if self.late_notification() {
                return Ok(());
            }
            if let Some(preparation) = &self.preparation {
                preparation.check_runtime(&hub.runtime(&context.bot))?;
            }
            if let Some(run) = context.run.as_deref() {
                ensure!(hub.routines.guard_run_live(&context.bot, run), "Execução do gateway encerrada ou incorreta");
            }
            let runtime = hub.runtime(&context.bot);
            ensure!(
                runtime.lane.as_ref().is_some_and(|lane| lane.chat == context.lane)
                    && !matches!(runtime.status, BotStatus::Idle | BotStatus::Error),
                "Conversa do gateway remoto encerrada ou incorreta"
            );
        }
        Ok(())
    }

    /// Primeira fotografia autorizada após unlock, já posterior à migração legada.
    pub(crate) fn snapshot(&self, hub: &Hub) -> Result<Connector> {
        self.canonical(hub, None)
    }

    /// Somente rotação do mesmo binding/config; nunca captura substituto pelo ID.
    pub(crate) fn current(&self, hub: &Hub, expected: &Connector) -> Result<Connector> {
        self.canonical(hub, Some(expected))
    }

    fn canonical(&self, hub: &Hub, expected: Option<&Connector>) -> Result<Connector> {
        ensure!(!self.late_notification(), "Notificação tardia não autoriza credenciais");
        if let Some(expected) = expected {
            self.validate_connector(expected)?;
        }
        self.validate_live(hub)?;
        let saved = {
            let store = &hub.store;
            let _guard = store.connector_lock.locked();
            {
                let db = store.db.locked();
                let _ = self.validate_db(&db)?;
            }
            // Loader interno: abre a chave fora de DB e não reentra connector_lock.
            let (mut all, plaintext) = market::load_connectors(store)?;
            let migrated = oauth_binding::initialize(&mut all) || plaintext;
            let saved = all
                .iter()
                .find(|c| c.id == self.connector() && c.command.is_none() && c.configured())
                .ok_or_else(|| anyhow!("Conector remoto indisponível"))?;
            if let Some(expected) = expected {
                ensure!(self.matches_target(saved, expected), "Login antigo: o vínculo do conector mudou");
            }
            // Marketplace: a credencial de uma conta só é entregue a bot do Space dela (execução, não só seleção).
            if let Some(context) = self.context() {
                let bot = store.bot(&context.bot)?.filter(|b| !b.deleted).ok_or_else(denied)?;
                market::contas::exigir_mesmo_space(saved, &bot.config.space_id)?;
            }
            if let Original::Open { turn, .. } = &self.original {
                Self::validate_open_account(saved, &turn.check_db(&store.db.locked())?)?;
            }
            let saved = saved.clone();
            if migrated {
                let sealed = market::vault::seal(store, "connectors", &serde_json::to_string(&all)?)?;
                let mut db = store.db.locked();
                let tx = db.transaction()?;
                let _ = self.validate_db(&tx)?;
                market::vault::write_sealed(&tx, "connectors", &sealed)?;
                // Migração não é reconexão: preserva os contextos/setup de todos os legados.
                let _ = self.validate_db(&tx)?;
                tx.commit()?;
            } else {
                let db = store.db.locked();
                let _ = self.validate_db(&db)?;
            }
            saved
        };
        self.validate_live(hub)?;
        Ok(saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{guard_hub::mcp::runtime, store::Lane};
    use serde_json::json;
    use std::sync::Arc;
    use zeroize::Zeroizing;

    fn fixture() -> (Arc<Hub>, Connector, Context) {
        let hub = oauth_binding::tests::hub();
        let connector = serde_json::from_value(json!({"id":"notes","name":"Notes","url":"https://fixture.invalid",
            "oauth":{"accessToken":"old-access","refreshToken":"old-refresh"}}))
        .unwrap();
        market::save_connectors(&hub.store, &[connector]).unwrap();
        let connector = market::connectors(&hub.store).unwrap().remove(0);
        let bot = serde_json::from_value(json!({"id":"runtime-bot","name":"Runtime","connectors":["notes"]})).unwrap();
        hub.store.save_bot(&bot).unwrap();
        let execution = bindings::fixture_execution(&hub, "runtime-bot");
        let context = oauth_binding::issue_context(
            &hub.store,
            &connector,
            "runtime-bot",
            "runtime-bot",
            None,
            execution.acp_binding().unwrap(),
        )
        .unwrap();
        hub.set_runtime("runtime-bot", |runtime| {
            runtime.status = BotStatus::Working;
            runtime.lane = Some(Lane::main("runtime-bot"));
        });
        (hub, connector, context)
    }

    #[test]
    fn same_context_row_still_requires_bot_selection_without_cleanup_or_cache_loss() {
        for change in ["unselected", "deleted", "scalar"] {
            let (hub, _, context) = fixture();
            let intent = RuntimeIntent::capture(&hub, &context.id, "notes", "initialize").unwrap();
            let namespace = hub.store.guard_runtime_namespace().unwrap();
            runtime::remember(
                &namespace,
                "notes",
                &context.id,
                "bridge",
                &[Zeroizing::new("retained-old-secret".into())],
            );
            let raw = hub.store.kv_read("connectors").unwrap();
            // A escrita direta de fixture isola a validação SQL do cleanup posterior do Store.
            if change == "deleted" {
                hub.store.db.locked().execute("UPDATE bots SET deleted=1 WHERE id='runtime-bot'", []).unwrap();
            } else if change == "scalar" {
                hub.store
                    .db
                    .locked()
                    .execute(
                        "UPDATE bots SET config=json_set(config,'$.connectors','notes') WHERE id='runtime-bot'",
                        [],
                    )
                    .unwrap();
            } else {
                hub.store
                    .db
                    .locked()
                    .execute(
                        "UPDATE bots SET config=json_set(config,'$.connectors',json('[]')) WHERE id='runtime-bot'",
                        [],
                    )
                    .unwrap();
            }
            assert!(intent.snapshot(&hub).is_err());
            assert!(hub.store.guard_context(&context.id).unwrap().is_some(), "guarda não faz cleanup");
            assert!(runtime::snapshot(&namespace, "notes", &context.id, Some("bridge")).is_some());
            assert!(hub.store.kv_read("connectors").unwrap().eq(&raw));
            runtime::release(&namespace, "notes", &context.id, Some("bridge"));
        }
    }

    /// Marketplace 1a: a credencial de uma conta só é entregue a bot do Space dela, mesmo com a conta na lista do bot.
    #[test]
    fn credencial_de_conta_de_outro_space_nao_e_entregue_ao_proxy() {
        let (hub, _, context) = fixture();
        let intent = RuntimeIntent::capture(&hub, &context.id, "notes", "tools/call").unwrap();
        assert!(intent.snapshot(&hub).is_ok(), "mesmo Space: segue o fluxo de antes");
        market::update_connectors(&hub.store, |all| {
            all[0].space = "00000000-0000-4000-8000-0000000000ff".into();
            Ok(())
        })
        .unwrap();
        let error = intent.snapshot(&hub).unwrap_err().to_string();
        assert!(error.starts_with("conta_de_outro_space"), "{error}");
        // O contexto e o cache não são apagados pela recusa; só a entrega é negada.
        assert!(hub.store.guard_context(&context.id).unwrap().is_some());
    }

    #[test]
    fn original_setup_deadline_cannot_be_extended_by_current_row() {
        let (hub, connector, _) = fixture();
        let setup = oauth_binding::issue_setup(&hub.store, &connector).unwrap();
        let mut intent = RuntimeIntent::capture(&hub, &setup.id, "notes", "initialize").unwrap();
        // Simula deterministicamente prazo ORIGINAL vencido, com prazo atual ainda futuro.
        if let Original::Setup { expires, .. } = &mut intent.original {
            *expires = now_ms() - 1;
        }
        let raw = hub.store.kv_read("connectors").unwrap();
        assert!(intent.snapshot(&hub).is_err());
        assert!(hub.store.kv_read("connectors").unwrap().eq(&raw));
        let exists: bool = hub
            .store
            .db
            .locked()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM donna_guard_setup WHERE id=?1 AND expires_at>?2)",
                rusqlite::params![setup.id, now_ms()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists, "intenção antiga não adota validade renovada nem apaga a linha atual");
    }

    #[test]
    fn original_context_tuple_cannot_adopt_another_lane_at_same_id() {
        let (hub, _, context) = fixture();
        let intent = RuntimeIntent::capture(&hub, &context.id, "notes", "initialize").unwrap();
        let mut changed = context.clone();
        changed.lane = "other-lane".into();
        hub.store.guard_context_save(&changed).unwrap();
        assert!(intent.snapshot(&hub).is_err());
        assert_eq!(hub.store.guard_context(&context.id).unwrap().unwrap().lane, "other-lane");
    }

    #[test]
    fn contexto_oauth_original_nao_adota_outro_binding_no_mesmo_id() {
        let (hub, _, context) = fixture();
        let intent = RuntimeIntent::capture(&hub, &context.id, "notes", "initialize").unwrap();
        let replacement =
            bindings::issue(&hub.store, "runtime-bot", "runtime-bot", "runtime-bot", Some("other-acp")).unwrap();
        let mut changed = context.clone();
        changed.binding = Some(replacement.id.as_str().into());
        hub.store.guard_context_save(&changed).unwrap();
        assert!(intent.snapshot(&hub).is_err());
        assert_eq!(hub.store.guard_context(&context.id).unwrap().unwrap().binding, changed.binding);
    }

    #[test]
    fn revogar_sessao_impede_refresh_oauth_sem_gravacao_parcial() {
        let (hub, connector, context) = fixture();
        let intent = RuntimeIntent::capture(&hub, &context.id, "notes", "tools/call").unwrap();
        let raw = hub.store.kv_read("connectors").unwrap();
        bindings::revoke(&hub.store, "runtime-bot", None).unwrap();
        assert!(
            oauth_binding::change_runtime(&hub, &connector, &intent, |connector| {
                connector.oauth.as_mut().unwrap().access_token = "must-not-persist".into();
                Ok(())
            })
            .is_err()
        );
        assert!(hub.store.kv_read("connectors").unwrap().eq(&raw));
        assert!(hub.store.guard_context(&context.id).unwrap().is_some());
    }

    #[test]
    fn validacao_oauth_conserva_execution_ref_original_na_mesma_sessao() {
        let (hub, connector, context) = fixture();
        let intent = RuntimeIntent::capture(&hub, &context.id, "notes", "tools/call").unwrap();
        let original = intent.execution.clone().unwrap();
        hub.set_runtime("runtime-bot", |runtime| runtime.memory_turn_token = Some("next-turn".into()));
        intent.current(&hub, &connector).unwrap();
        assert_eq!(intent.execution.as_ref(), Some(&original));
        assert_eq!(original.turn_token(), "fixture-turn", "D229 opção A não recaptura a identidade");
    }
}
