//! O chat só recebe estado/ID. O perfil anterior e as rotinas ficam privados no SQLite.
use crate::agent::bot::Cmd;
use crate::hub::Hub;
use crate::routines::Routine;
use crate::store::{BotConfig, Permission, Store};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
struct Undo {
    target: String,
    before: Option<BotConfig>,
    after: BotConfig,
    routines_before: Vec<Routine>,
    routines_after: Vec<Routine>,
}

/// Write-ahead: antes da primeira mutação, registra informação suficiente para publicar
/// seu aviso/journal após interrupção. Na criação, a diferença da lista de IDs identifica
/// o único bot que esta operação pode criar, sob `team_lock`.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Pending {
    id: String,
    source: String,
    target: Option<String>,
    known: Vec<String>,
    before: Option<BotConfig>,
    routines_before: Vec<Routine>,
    auto: bool,
    text: String,
    turn: i64,
    #[serde(default)]
    saved: Option<Saved>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    config: BotConfig,
    routines: Vec<Routine>,
}

impl Pending {
    pub(super) fn is_creation(&self) -> bool {
        self.before.is_none()
    }
}

pub(super) fn prepare(
    hub: &Hub,
    source: &BotConfig,
    before: Option<BotConfig>,
    routines_before: Vec<Routine>,
    text: &str,
    execution: Option<&crate::fork::sessoes::ExecutionRef>,
) -> Result<Pending> {
    recover_live(hub);
    let pending = Pending {
        id: uuid::Uuid::new_v4().to_string(),
        source: source.id.clone(),
        target: before.as_ref().map(|b| b.id.clone()),
        known: if before.is_none() { hub.store.bots()?.into_iter().map(|b| b.config.id).collect() } else { vec![] },
        before,
        routines_before,
        text: text.into(),
        turn: hub.store.max_turn(&source.id),
        saved: None,
        auto: super::management_permission(source, &hub.store.team_settings()?) == Permission::Auto,
    };
    let raw = serde_json::to_string(&pending)?;
    crate::fork::sessoes::bindings::write(&hub.store, execution, |c| {
        c.execute(
            "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            rusqlite::params![format!("team.pending.{}", pending.id), raw],
        )?;
        Ok(())
    })?;
    Ok(pending)
}

/// Serializado dentro da transação que salva perfil e rotinas, antes de qualquer ator.
pub(super) fn saved(pending: &Pending, config: &BotConfig, routines: &[Routine]) -> Result<(String, String)> {
    let mut pending = pending.clone();
    pending.saved = Some(Saved { config: config.clone(), routines: routines.to_vec() });
    Ok((format!("team.pending.{}", pending.id), serde_json::to_string(&pending)?))
}

/// Recupera antes de aceitar uma nova edição: assim ela nunca entra no snapshot da operação anterior.
pub(crate) fn recover_live(hub: &Hub) {
    // Gravar e publicar sob a trava do stream: o marcador de sessão sai antes do aviso (`fork::sessoes`).
    if let Err(error) = hub.republish(|| Ok(recover(&hub.store))) {
        tracing::warn!(error = format!("{error:#}"), "couldn't publish recovered team changes");
    }
}

/// Apenas `serve` chama no início, antes de subir atores/rotinas; abrir o banco não recupera.
pub fn recover(store: &Store) -> Vec<String> {
    let mut notices = Vec::new();
    let records = match store.pending_team_changes() {
        Ok(records) => records,
        Err(error) => {
            tracing::warn!(%error, "couldn't enumerate pending team changes; recovery deferred");
            return notices;
        }
    };
    for (key, raw) in records {
        // Só dados inválidos vão para quarentena; falhas operacionais mantêm o retry.
        let parsed = (|| -> Result<Pending> {
            let pending: Pending = serde_json::from_str(&raw)?;
            if key != format!("team.pending.{}", pending.id) {
                bail!("pending team change key does not match its id");
            }
            Ok(pending)
        })();
        let pending = match parsed {
            Ok(pending) => pending,
            Err(error) => {
                tracing::warn!(%error, %key, "quarantining invalid pending team change");
                if let Err(error) = store.quarantine_team_change(&key, &raw, &format!("{error:#}")) {
                    tracing::warn!(%error, %key, "couldn't persist team change quarantine; recovery deferred");
                }
                continue;
            }
        };
        match finish_store(store, pending) {
            Ok(published) => notices.extend(published),
            Err(error) => tracing::warn!(%error, %key, "pending team change recovery deferred"),
        }
    }
    notices
}

pub(super) fn finish(hub: &Hub, pending: &Pending) -> Result<()> {
    let id = pending.id.clone();
    let pending: Pending = serde_json::from_str(
        &hub.store.kv_read(&format!("team.pending.{id}"))?.context("missing pending team change")?,
    )?;
    hub.republish(|| finish_store(&hub.store, pending))
}

pub(super) fn rollback_creation(hub: &Hub, pending: &Pending) -> Result<()> {
    if pending.before.is_some() {
        bail!("team update saved; notice publication pending recovery");
    }
    for bot in hub.store.bots()?.into_iter().filter(|b| {
        !b.deleted && b.config.created_by.as_deref() == Some(&pending.source) && !pending.known.contains(&b.config.id)
    }) {
        let id = &bot.config.id;
        let group_space = hub.store.bot_group_space(id)?;
        hub.routines
            .discard_team_creation(id, |routines| hub.store.discard_team_creation(id, &pending.id, routines))?;
        if let Some(space) = group_space {
            crate::hub::bot_groups::publish(hub, &space);
        }
        if let Err(error) = hub.delete_bot(id) {
            tracing::warn!(%error, bot = %id, "team creation rolled back; actor cleanup pending restart");
        }
    }
    hub.store.kv_delete(&format!("team.pending.{}", pending.id))?;
    bail!("team creation rolled back because its notice could not be saved")
}

fn snapshot(store: &Store, pending: &Pending) -> Result<Option<Saved>> {
    let target = if let Some(id) = &pending.target {
        store.bot(id)?.filter(|b| !b.deleted).map(|b| b.config)
    } else {
        let made: Vec<_> = store
            .bots()?
            .into_iter()
            .filter(|b| {
                !b.deleted
                    && b.config.created_by.as_deref() == Some(&pending.source)
                    && !pending.known.contains(&b.config.id)
            })
            .map(|b| b.config)
            .collect();
        if made.len() > 1 {
            bail!("ambiguous pending team creation");
        }
        made.into_iter().next()
    };
    let Some(after) = target else {
        return Ok(None);
    };
    let state: Value =
        store.kv_read("routines.v1")?.map(|s| serde_json::from_str(&s)).transpose()?.unwrap_or(json!({}));
    let routines_after: Vec<Routine> = state["routines"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["botId"] == after.id && r["deleted"] != true)
        .cloned()
        .map(serde_json::from_value)
        .collect::<std::result::Result<_, _>>()?;
    Ok(Some(Saved { config: after, routines: routines_after }))
}

/// As entradas a publicar, em ordem: o marcador de sessão, se a conversa vinha de outra sessão, e o aviso.
/// Vazio quando não havia o que concluir.
fn finish_store(store: &Store, mut pending: Pending) -> Result<Vec<String>> {
    let saved = if let Some(saved) = pending.saved.clone() {
        saved
    } else {
        let Some(saved) = snapshot(store, &pending)? else {
            store.kv_delete(&format!("team.pending.{}", pending.id))?;
            return Ok(Vec::new());
        };
        saved
    };
    let before = pending.before.as_ref().map(|before| crate::store::restore_team_profile(&saved.config, before));
    if before.as_ref().is_some_and(|before| crate::store::same_team_profile(before, &saved.config))
        && crate::routines::same_team_routines(&pending.routines_before, &saved.routines)
    {
        store.kv_delete(&format!("team.pending.{}", pending.id))?;
        return Ok(Vec::new());
    }
    // Compatibilidade com registros anteriores ao snapshot transacional. As novas
    // mutações sempre chegam aqui com `saved`, confirmado junto do perfil/rotinas.
    if pending.saved.is_none() {
        pending.saved = Some(saved.clone());
        store.kv_set(&format!("team.pending.{}", pending.id), &serde_json::to_string(&pending)?)?;
    }
    let Saved { config: after, routines: routines_after } = saved;
    let mut data = json!({"text":pending.text});
    let journal = if pending.auto {
        data["teamChange"] = json!({"status":"available", "targetBotId":after.id});
        Some(serde_json::to_string(&Undo {
            target: after.id.clone(),
            before,
            after,
            routines_before: pending.routines_before,
            routines_after,
        })?)
    } else {
        None
    };
    let marker = store.finish_team_change(&pending.id, &pending.source, pending.turn, &data, journal.as_deref())?;
    Ok(marker.map(|m| m.id).into_iter().chain([pending.id.clone()]).collect())
}

/// Intenção exclusiva do usuário; não consta no MCP manage.
pub async fn undo(hub: &Arc<Hub>, entry_id: &str) -> Result<Value> {
    let _guard = hub.store.team_lock.lock().await;
    recover_live(hub);
    let entry = hub.store.entry(entry_id).context("unknown team change")?;
    if entry.kind != "notice" || entry.data["teamChange"]["status"] != "available" {
        bail!("team change is not available to undo");
    }
    let key = format!("team.undo.{entry_id}");
    let undo: Undo = serde_json::from_str(&hub.store.kv_read(&key)?.context("missing team change journal")?)?;
    let group_space = hub.store.bot_group_space(&undo.target)?;
    let restored = hub.with_inactive_bot(&undo.target, || {
        hub.routines.restore_team(&undo.target, &undo.routines_after, &undo.routines_before, |routines| {
            hub.store.commit_team_undo(entry_id, &undo.after, undo.before.as_ref(), routines)
        })
    })?;
    if restored.is_none()
        && let Some(space) = group_space
    {
        crate::hub::bot_groups::publish(hub, &space);
    }
    // O SQLite já confirmou toda a restauração. Em caso de falha de um ator, o próximo início
    // lê esse estado; não repetimos a mutação nem perdemos o status persistente do aviso.
    if let Some(cfg) = restored {
        if let Err(error) = hub.send_cmd(&cfg.id, Cmd::Reconfigure(Box::new(cfg.clone()))) {
            tracing::warn!(%error, "team undo committed; actor reconfiguration pending restart");
        }
        hub.emit_bot(&cfg.id);
    } else if let Err(error) = hub.delete_bot(&undo.target) {
        tracing::warn!(%error, "team undo committed; bot cleanup pending restart");
    }
    let updated = hub.store.entry(entry_id).context("missing team notice after undo")?;
    // Repete apenas a publicação do estado já confirmado, usando a ordenação de eventos do Hub.
    let updated = hub.set_entry(entry_id, &updated.data).unwrap_or(updated);
    Ok(json!({"entry":updated}))
}

/// Uma recusa anterior ao commit não deixa journal nem aviso de uma mudança que não ocorreu.
pub(super) fn discard_unsaved(hub: &Hub, pending: &Pending) -> Result<bool> {
    crate::fork::sessoes::bindings::write(&hub.store, None, |c| {
        Ok(c.execute(
            "DELETE FROM kv WHERE k=?1 AND json_extract(v,'$.saved') IS NULL",
            [format!("team.pending.{}", pending.id)],
        )? == 1)
    })
}
