//! Contexto emitido pelo ator, nunca pelo texto ou pelos parâmetros do MCP.
use crate::store::Store;
use anyhow::Result;
#[cfg(test)]
use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    pub id: String,
    pub bot: String,
    pub connector: String,
    pub lane: String,
    pub run: Option<String>,
    #[serde(default)]
    pub binding: Option<String>,
}

impl Context {
    pub fn issue_bound(
        store: &Store,
        bot: &str,
        connector: &str,
        lane: &str,
        run: Option<&str>,
        binding: Option<&str>,
    ) -> Result<Self> {
        store.guard_cleanup()?;
        if let Some(context) = store.guard_context_for(bot, connector, lane, run, binding)? {
            return Ok(context);
        }
        let context = Self {
            id: uuid::Uuid::new_v4().to_string(),
            bot: bot.into(),
            connector: connector.into(),
            lane: lane.into(),
            run: run.map(str::to_owned),
            binding: binding.map(str::to_owned),
        };
        store.guard_context_save(&context)?;
        Ok(context)
    }

    #[cfg(test)]
    pub fn issue(store: &Store, bot: &str, connector: &str, lane: &str, run: Option<&str>) -> Result<Self> {
        let binding = crate::fork::sessoes::bindings::for_acp(store, bot, "fixture-acp")
            .or_else(|_| crate::fork::sessoes::bindings::issue(store, bot, bot, bot, Some("fixture-acp")))?;
        crate::fork::sessoes::bindings::activate(store, &binding, "fixture-acp")?;
        Self::issue_bound(store, bot, connector, lane, run, Some(binding.id.as_str()))
    }

    #[cfg(test)]
    pub fn resolve(store: &Store, id: &str, connector: &str) -> Result<Self> {
        let denied = || anyhow!("Contexto do gateway indisponível ou revogado");
        let context = store.guard_context(id)?.ok_or_else(denied)?;
        if context.connector != connector {
            bail!("Conector não pertence ao contexto do gateway");
        }
        let bot = store.bot(&context.bot)?.filter(|b| !b.deleted);
        if bot.is_none_or(|b| !b.config.connectors.contains(&context.connector)) {
            store.guard_context_remove(id)?;
            return Err(denied());
        }
        Ok(context)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::guard_hub::mcp::runtime;
    use serde_json::json;
    use zeroize::Zeroizing;

    #[test]
    fn revocation_erases_context_and_its_lease_without_affecting_a_wrong_connector_request() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let mut bot: crate::store::BotConfig =
            serde_json::from_value(json!({"id":"bot","name":"Fixture","connectors":["connector"]})).unwrap();
        store.save_bot(&bot).unwrap();
        let context = Context::issue(&store, "bot", "connector", "lane", None).unwrap();
        let namespace = store.guard_runtime_namespace().unwrap();
        runtime::remember(&namespace, "connector", &context.id, "bridge", &[Zeroizing::new("synthetic123".into())]);
        assert!(Context::resolve(&store, &context.id, "wrong").is_err());
        assert!(runtime::snapshot(&namespace, "connector", &context.id, Some("bridge")).is_some());
        bot.connectors.clear();
        store.save_bot(&bot).unwrap();
        assert!(Context::resolve(&store, &context.id, "connector").is_err());
        assert!(store.guard_context(&context.id).unwrap().is_none());
        assert!(runtime::snapshot(&namespace, "connector", &context.id, Some("bridge")).is_none());
    }
}

#[cfg(test)]
mod binding_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::{LockExt, fork::sessoes::bindings};
    use serde_json::json;
    #[test]
    fn contextos_do_mesmo_bot_conector_lane_e_run_nao_substituem_outro_vinculo() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store
            .save_bot(&serde_json::from_value(json!({"id":"bot","name":"Bot","connectors":["connector"]})).unwrap())
            .unwrap();
        let a = bindings::issue(&store, "bot", "bot", "bot", Some("acp-a")).unwrap();
        let b = bindings::issue(&store, "bot", "bot", "bot", Some("acp-b")).unwrap();
        let first = Context::issue_bound(&store, "bot", "connector", "bot", None, Some(a.id.as_str())).unwrap();
        let second = Context::issue_bound(&store, "bot", "connector", "bot", None, Some(b.id.as_str())).unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(Context::resolve(&store, &first.id, "connector").unwrap().binding.as_deref(), Some(a.id.as_str()));
        bindings::revoke(&store, "bot", Some("acp-a")).unwrap();
        assert!(bindings::resolve(&store.db.locked(), a.id.as_str(), "bot").is_err());
        assert!(bindings::resolve(&store.db.locked(), b.id.as_str(), "bot").is_ok());
    }
}
