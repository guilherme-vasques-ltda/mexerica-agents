//! Preparação canônica de session/new/load; não emite `ExecutionRef`, ACP ou token novo.
use super::bindings::{self, BindingRef};
use crate::{
    LockExt,
    hub::{BotStatus, Runtime},
    store::Store,
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Admission {
    binding: String,
    turn_token: String,
}

pub(crate) struct Preparation {
    binding: BindingRef,
    admission: Admission,
}

fn key(bot: &str) -> String {
    format!("session.binding.preparing.{bot}")
}

fn read(c: &Connection, bot: &str) -> Result<Option<Admission>> {
    let raw: Option<String> = c.query_row("SELECT v FROM kv WHERE k=?1", [key(bot)], |r| r.get(0)).optional()?;
    raw.map(|raw| serde_json::from_str(&raw).map_err(Into::into)).transpose()
}

pub(crate) fn discovery(method: &str) -> bool {
    matches!(
        method,
        "initialize"
            | "ping"
            | "tools/list"
            | "resources/list"
            | "resources/templates/list"
            | "prompts/list"
            | "notifications/initialized"
    )
}

/// VG03: a identidade do turno é uma só (`memory_turn_token` do Hub). Antes do permit ela
/// está preparada (Waiting + `preparing_turn_token`); depois, ativa. A preparação de sessão
/// admite as duas, só para descoberta (D298); `ExecutionRef` exige a identidade ativa.
pub(crate) fn turn_identity(runtime: &Runtime) -> Option<&str> {
    match runtime.status {
        BotStatus::Working | BotStatus::NeedsInput => runtime.memory_turn_token.as_deref(),
        BotStatus::Waiting if runtime.memory_turn_token.is_none() => runtime.preparing_turn_token.as_deref(),
        _ => None,
    }
}

/// O Hub segura runtime; esta escrita usa apenas o token que ele já gerou para o turno.
pub(crate) fn prepare(store: &Store, binding: &BindingRef, runtime: &Runtime) -> Result<()> {
    let token = turn_identity(runtime).context("chamada vencida: preparação sem turno")?;
    ensure!(!token.is_empty() && runtime.session.is_none(), "chamada vencida");
    let mut c = store.db.locked();
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let origin = bindings::resolve(&tx, binding.id.as_str(), &binding.bot)?;
    ensure!(origin.session_ref == binding.session_ref, "chamada vencida");
    let admission = Admission { binding: origin.id.as_str().into(), turn_token: token.into() };
    let preparation = Preparation { binding: origin, admission };
    preparation.check_runtime(runtime)?;
    tx.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        params![key(&binding.bot), serde_json::to_string(&preparation.admission)?],
    )?;
    tx.commit()?;
    Ok(())
}

pub(crate) fn capture(c: &Connection, binding: &str, bot: &str, method: &str) -> Result<Preparation> {
    ensure!(discovery(method), "chamada vencida: preparação admite somente descoberta");
    let origin = bindings::resolve(c, binding, bot)?;
    let admission = read(c, bot)?.context("chamada vencida: preparação indisponível")?;
    ensure!(admission.binding == binding, "chamada vencida");
    Ok(Preparation { binding: origin, admission })
}

/// Apenas o catálogo exato do MCP memory pode usar a preparação já emitida pelo ator.
pub(crate) fn capture_memory_catalog(hub: &crate::hub::Hub, body: &serde_json::Value) -> Result<Option<Preparation>> {
    ensure!(body["name"] == "__tools", "chamada vencida");
    let bot = body["botId"].as_str().context("bot ausente")?;
    let runtime = hub.runtime(bot);
    if runtime.session.is_some() {
        return Ok(None);
    }
    let binding = body["sessionBinding"].as_str().context("chamada vencida: vínculo de sessão ausente")?;
    let preparation = capture(&hub.store.db.locked(), binding, bot, "tools/list")?;
    preparation.check_runtime(&runtime)?;
    Ok(Some(preparation))
}

impl Preparation {
    pub(crate) fn bot(&self) -> &str {
        &self.binding.bot
    }

    pub(crate) fn check(&self, c: &Connection) -> Result<()> {
        let binding = bindings::resolve(c, self.binding.id.as_str(), &self.binding.bot)?;
        ensure!(
            binding.session_ref == self.binding.session_ref
                && self.binding.acp_session.as_ref().is_none_or(|acp| binding.acp_session.as_ref() == Some(acp))
                && read(c, &binding.bot)?.as_ref() == Some(&self.admission),
            "chamada vencida: preparação substituída"
        );
        Ok(())
    }

    pub(crate) fn check_runtime(&self, runtime: &Runtime) -> Result<()> {
        ensure!(
            runtime.session.is_none()
                && turn_identity(runtime) == Some(self.admission.turn_token.as_str())
                && runtime.lane.as_ref().is_some_and(|lane| lane.chat == self.binding.session_ref.chat_id
                    && lane.session.as_deref().unwrap_or(&lane.chat) == self.binding.session_ref.id),
            "chamada vencida"
        );
        Ok(())
    }
}

pub(crate) fn clear(c: &Connection, bot: &str) -> Result<()> {
    c.execute("DELETE FROM kv WHERE k=?1", [key(bot)])?;
    Ok(())
}
