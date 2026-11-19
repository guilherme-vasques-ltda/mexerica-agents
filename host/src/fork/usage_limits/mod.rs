//! C4: preferências do Hub e barreira durável de desistência por espera.
use crate::{LockExt, api::devices::Caller, hub::Hub, store::Store};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

const SETTINGS: &str = "donna.usage-limits.v1";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Settings {
    pub auto_resume: bool,
    pub snooze: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self { auto_resume: true, snooze: false }
    }
}

pub(crate) fn handles(method: &str) -> bool {
    matches!(method, "donnaUsageLimits" | "donnaUsageLimitsSave" | "donnaUsageWaitCancel" | "donnaUsageWaitResume")
}
fn read(c: &Connection, key: &str) -> Result<Option<String>> {
    Ok(c.query_row("SELECT v FROM kv WHERE k=?1", [key], |r| r.get(0)).optional()?)
}
fn write(c: &Connection, key: &str, value: &Value) -> Result<()> {
    c.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        [key, &value.to_string()],
    )?;
    Ok(())
}
fn cancel_key(id: &str) -> String {
    format!("donna.usage-wait.cancelled.{id}")
}
pub(crate) fn settings_on(c: &Connection) -> Result<Settings> {
    Ok(read(c, SETTINGS)?.map(|raw| serde_json::from_str(&raw)).transpose()?.unwrap_or_default())
}
pub(crate) fn settings(store: &Store) -> Result<Settings> {
    settings_on(&store.db.locked())
}
pub(crate) fn cancellation(store: &Store, id: &str) -> Result<Option<Value>> {
    read(&store.db.locked(), &cancel_key(id))?.map(|raw| serde_json::from_str(&raw).map_err(Into::into)).transpose()
}

/// Dentro da mesma Tx do permit, depois de todos os awaits do preparo. Erro fecha o envio.
pub(crate) fn allow_on(c: &Connection, checkpoint: Option<&str>) -> Result<bool> {
    let Some(raw) = checkpoint else { return Ok(true) };
    let w: Value = serde_json::from_str(raw)?;
    if let Some(id) = w["usageControl"]["id"].as_str()
        && read(c, &cancel_key(id))?.is_some()
    {
        return Ok(false);
    }
    let policy = settings_on(c)?;
    Ok((policy.auto_resume || w["usageControl"]["manual"] == true)
        && !(policy.snooze && w["resumeAt"].as_i64().is_some_and(|at| at > crate::store::now_ms())))
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[expect(clippy::struct_field_names, reason = "DTO distingue os cinco IDs exigidos no contrato RPC.")]
pub struct Target {
    pub bot_id: String,
    pub chat_id: String,
    pub session_id: Option<String>,
    pub entry_id: String,
    pub wait_id: String,
}
impl Target {
    pub(crate) fn matches(&self, w: &Value) -> bool {
        w["usageControl"]["id"] == self.wait_id
            && w["notice"] == self.entry_id
            && w["input"]["chat"] == self.chat_id
            && w["input"]["visibleSession"].as_str() == self.session_id.as_deref()
    }
}

/// A Tx compete com o permit; nunca confirma cancelamento somente na memória do app.
/// D368: num pedido do app pareado, a autoridade original é reconferida nesta Tx (`pedido::gravar`).
pub(crate) fn cancel(store: &Store, target: &Target) -> Result<Value> {
    crate::fork::donna_fixa::pedido::gravar(store, |tx| cancel_on(tx, target))
}

fn cancel_on(tx: &Connection, target: &Target) -> Result<Value> {
    if let Some(raw) = read(tx, &cancel_key(&target.wait_id))? {
        let saved: Value = serde_json::from_str(&raw)?;
        ensure!(saved["botId"] == target.bot_id && target.matches(&saved["checkpoint"]), "usage_wait_stale");
        ensure!(saved["resumed"] != true, "usage_wait_stale");
        return Ok(json!({"cancelled":true,"alreadyStarted":saved["alreadyStarted"]}));
    }
    let raw = read(tx, &format!("turn.waiting.{}", target.bot_id))?.ok_or_else(|| anyhow!("usage_wait_stale"))?;
    let w: Value = serde_json::from_str(&raw)?;
    ensure!(
        target.matches(&w) && matches!(w["phase"].as_str(), Some("waiting" | "paused" | "retrying")),
        "usage_wait_stale"
    );
    ensure!(w["completion"].is_null(), "usage_wait_already_completed");
    let dispatch = read(tx, &format!("turn.inflight.{}", target.bot_id))?
        .map(|raw| serde_json::from_str::<Value>(&raw))
        .transpose()?;
    let started = w["usageControl"]["uncertainAt"].as_i64().is_some()
        || dispatch.is_some_and(|d| d["executionToken"] == w["executionToken"] && d["phase"] == "permitted");
    write(
        tx,
        &cancel_key(&target.wait_id),
        &json!({"botId":target.bot_id,"checkpoint":w,"alreadyStarted":started,"resumed":false}),
    )?;
    Ok(json!({"cancelled":true,"alreadyStarted":started}))
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    super::bot_editor::authorization::require_owner(caller)?;
    match method {
        "donnaUsageLimits" => {
            ensure!(body.as_object().is_some_and(serde_json::Map::is_empty), "invalid_usage_limits");
            Ok(serde_json::to_value(settings(&hub.store)?)?)
        }
        "donnaUsageLimitsSave" => {
            let value: Settings = serde_json::from_value(body)?;
            let answer = serde_json::to_value(value)?;
            // D368: reconferida na gravação (`pedido::gravar`).
            crate::fork::donna_fixa::pedido::gravar(&hub.store, |c| write(c, SETTINGS, &answer))?;
            Ok(answer)
        }
        _ => {
            let target: Target = serde_json::from_value(body)?;
            // O destino vem do checkpoint e da entrada, nunca do bot selecionado no app.
            let entry = hub.store.entry(&target.entry_id).ok_or_else(|| anyhow!("usage_wait_stale"))?;
            ensure!(
                entry.bot_id == target.chat_id
                    && entry.session_id == target.session_id.clone().unwrap_or_else(|| target.chat_id.clone()),
                "usage_wait_destination_mismatch"
            );
            if method == "donnaUsageWaitCancel" {
                let answer = cancel(&hub.store, &target)?;
                if let Err(error) =
                    hub.send_cmd(&target.bot_id, crate::agent::bot::Cmd::UsageWaitChanged { wait_id: target.wait_id })
                {
                    tracing::warn!(%error, "desistência gravada; ator reconciliará no reinício");
                }
                Ok(answer)
            } else {
                let (reply, receive) = tokio::sync::oneshot::channel();
                // D368: a origem do pedido vai até o ator, que a reconfere antes de retomar.
                let origem = crate::fork::donna_fixa::pedido::atual();
                hub.send_cmd(
                    &target.bot_id.clone(),
                    crate::agent::bot::Cmd::UsageWaitResume { target, reply, origem },
                )?;
                receive.await?
            }
        }
    }
}

/// Consome a ação manual e instala o checkpoint na mesma gravação. A desistência antiga nunca é apagada.
pub(crate) fn resume_archive(
    store: &Store,
    target: &Target,
    checkpoint: &Value,
    origem: Option<&crate::fork::donna_fixa::pedido::Origem>,
) -> Result<()> {
    crate::fork::donna_fixa::pedido::gravar_com(store, origem, |tx| resume_archive_on(tx, target, checkpoint))
}

/// Retomar agora da espera em curso: autoridade e gravação na mesma transação (D368/D393), sem
/// janela entre a conferência e o `manual=true` gravado.
pub(crate) fn save_resumed(
    store: &Store,
    bot: &str,
    checkpoint: &Value,
    origem: Option<&crate::fork::donna_fixa::pedido::Origem>,
) -> Result<()> {
    crate::fork::donna_fixa::pedido::gravar_com(store, origem, |tx| {
        write(tx, &format!("turn.waiting.{bot}"), checkpoint)
    })
}

fn resume_archive_on(tx: &Connection, target: &Target, checkpoint: &Value) -> Result<()> {
    let raw = read(tx, &cancel_key(&target.wait_id))?.ok_or_else(|| anyhow!("usage_wait_stale"))?;
    let mut saved: Value = serde_json::from_str(&raw)?;
    ensure!(
        saved["botId"] == target.bot_id
            && target.matches(&saved["checkpoint"])
            && saved["resumed"] != true
            && saved["alreadyStarted"] != true,
        "usage_wait_stale_or_uncertain"
    );
    saved["resumed"] = json!(true);
    write(tx, &cancel_key(&target.wait_id), &saved)?;
    write(tx, &format!("turn.waiting.{}", target.bot_id), checkpoint)?;
    Ok(())
}

/// O estado esperado do aviso é derivado da barreira; nenhuma confirmação em memória é necessária.
pub(crate) fn pending_notices(store: &Store, bot: &str) -> Result<Vec<Value>> {
    let db = store.db.locked();
    let mut stmt = db.prepare(
        "WITH cancelled AS MATERIALIZED (SELECT v FROM kv WHERE k GLOB 'donna.usage-wait.cancelled.*')
         SELECT k.v FROM cancelled k JOIN entries e ON e.id=json_extract(k.v,'$.checkpoint.notice')
         WHERE json_extract(k.v,'$.botId')=?1
         AND (json_extract(e.data,'$.subscriptionWait.cancelled') IS NOT 1
           OR json_extract(e.data,'$.subscriptionWait.waitId') IS NOT json_extract(k.v,'$.checkpoint.usageControl.id')
           OR json_extract(e.data,'$.subscriptionWait.retry') IS NOT
              (json_extract(k.v,'$.alreadyStarted')=0 AND json_extract(k.v,'$.resumed')=0))",
    )?;
    let rows = stmt.query_map([bot], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(|raw| serde_json::from_str(&raw).map_err(Into::into)).collect()
}

pub(crate) fn on_bot_deleted(c: &Connection, bot: &str) -> Result<()> {
    c.execute("DELETE FROM kv WHERE k GLOB 'donna.usage-wait.cancelled.*' AND json_extract(v,'$.botId')=?1", [bot])?;
    Ok(())
}
