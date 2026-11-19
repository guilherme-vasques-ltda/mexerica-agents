//! D543: exclusão definitiva de Space, só no computador. Uma única Tx confirma
//! Space, bots, conversas e dados do Space; só depois atores encerram e eventos saem.
//! A recusa `space_busy` traz só `{botId, name}` por bot ocupado — nunca atividades.
use super::{PERSONAL, Snapshot};
use crate::{LockExt, fork::engine_setup::access, hub::Hub};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    space_id: String,
    expected_revision: i64,
    confirm_name: String,
}

/// O que a Tx única respondeu: recusa só com nomes, ou o Snapshot sem o Space e a
/// lista `(botId, rev)` para os efeitos pós-commit.
#[derive(Debug)]
pub(crate) enum SpaceDelete {
    Busy(Vec<(String, String)>),
    Done { snapshot: Snapshot, deleted: Vec<(String, i64)>, removed: Vec<String>, layers: Vec<String> },
}

/// `donnaSpaceDelete`: autoridade do Birth (Local ou aparelho do dono com Control e
/// vínculo), reconfirmada dentro da transação; nunca recaptura concessão nova.
pub(crate) async fn call(hub: &Arc<Hub>, authority: &access::Authority, body: Value) -> Result<Value> {
    let authority = authority.clone();
    let request: Request = serde_json::from_value(body)?;
    ensure!(request.space_id != PERSONAL, "personal_space_immutable");
    ensure!(request.expected_revision >= 0, "invalid_space_revision");
    // Nome exato como gravado: sem trim — " Cliente " não é "Cliente" (4.2).
    let confirm = request.confirm_name.clone();
    ensure!(!confirm.is_empty(), "invalid_field: confirmação vazia");
    let space_id = request.space_id.clone();
    let device_key = match &authority {
        access::Authority::Local => None,
        access::Authority::Owner { key, .. } => Some(key.clone()),
    };
    let guard = hub.store.team_lock.clone().lock_owned().await;
    authority.require_current(hub)?;
    // Congela apenas os atores que participam do Space. O Berga do Pessoal é
    // conferido só nas suas conversas deste Space; nunca é encerrado na exclusão.
    let pre_bots = hub.store.space_bot_ids(&space_id)?;
    let conversations = hub.store.space_delete_conversations(&space_id)?;
    let mut owners = pre_bots.clone();
    owners.extend(conversations.iter().map(|(_, owner)| owner.clone()));
    owners.sort();
    owners.dedup();
    let mut reservations = Vec::new();
    let mut actor_busy = Vec::new();
    for owner in owners {
        if !hub.has_bot_actor(&owner) {
            continue;
        }
        let chats = conversations.iter().filter(|(_, bot)| *bot == owner).map(|(chat, _)| chat.clone()).collect();
        let (reply, result) = tokio::sync::oneshot::channel();
        let (release, held) = tokio::sync::oneshot::channel();
        hub.send_cmd(
            &owner,
            crate::agent::bot::Cmd::SpaceDeleteGuard { chats, all: pre_bots.contains(&owner), reply, release: held },
        )?;
        reservations.push(release);
        let busy = tokio::time::timeout(std::time::Duration::from_secs(10), result)
            .await
            .map_err(|_| anyhow::anyhow!("space_delete_pending"))??;
        if busy {
            actor_busy.push(owner);
        }
    }
    let hub = hub.clone();
    tokio::task::spawn_blocking(move || -> Result<Value> {
        let _guard = guard;
        let _reservations = reservations;
        authority.require_current(&hub)?;
        // Bots do Space estáveis sob team_lock; a Tx relê e confere o mesmo conjunto.
        // Pastas geradas pelo Hub de cada bot: workspace alocado e, quando gerenciado,
        // o próprio cwd. Pasta de projeto do dono (não gerenciada) nunca entra aqui.
        let mut workspaces = Vec::new();
        for id in &pre_bots {
            if let Some(row) = hub.store.bot(id)?
                && crate::agent::workspace::is_managed(&row.config)
            {
                workspaces.push(std::path::PathBuf::from(&row.config.cwd));
            }
            workspaces.push(crate::agent::workspace::path(id));
        }
        workspaces.sort();
        workspaces.dedup();
        // Conversas extras do Space (inclusive as foreign do Pessoal): workspaces
        // de uploads/arquivos vivem em `spaces/{space}/conversations/{chat}` — a
        // raiz inteira do Space no disco é coletada aqui, motor ou não.
        let space_files = if crate::fork::motor_exec::configured() {
            Some(crate::fork::motor_roots::work().join("spaces").join(&space_id))
        } else {
            hub.store.engine_setup_root.parent().map(|root| root.join("spaces").join(&space_id))
        };
        if let Some(dir) = space_files {
            workspaces.push(dir);
        }
        // Capturas pré-commit da torre e dos canais liderados, como no delete_bot.
        let tower =
            pre_bots.iter().map(|id| crate::fork::control_tower::deletion(&hub, id)).collect::<Result<Vec<_>>>()?;
        let led =
            pre_bots.iter().map(|id| crate::fork::lider_canal::led_by(&hub.store, id)).collect::<Result<Vec<_>>>()?;
        // `Busy` não escreve nada na Tx: os nomes voltam por este canal e o estado
        // de rotinas não troca — os runs vivos continuam como estavam.
        let busy_names = std::cell::RefCell::new(Vec::new());
        let expected = request.expected_revision;
        let confirm = confirm.as_str();
        let device_key = device_key.as_deref();
        let host_id = hub.host_id.clone();
        let outcome = hub.routines.delete_bots_committed(&pre_bots, |raw, live, live_bots| {
            match authority.with_db(&hub, |db| {
                crate::store::Store::delete_space_on(
                    db,
                    &space_id,
                    expected,
                    confirm,
                    device_key,
                    &host_id,
                    &pre_bots,
                    &conversations,
                    raw,
                    live,
                    &live_bots.iter().chain(actor_busy.iter()).cloned().collect::<Vec<_>>(),
                )
            })? {
                SpaceDelete::Busy(busy) => {
                    *busy_names.borrow_mut() = busy;
                    Ok(None)
                }
                done @ SpaceDelete::Done { .. } => Ok(Some(done)),
            }
        })?;
        let Some(outcome) = outcome else {
            let busy = std::mem::take(&mut *busy_names.borrow_mut());
            // Recusa `space_busy`: código explícito + só `{botId, name}` por bot.
            return Ok(json!({"code":"space_busy","busy": busy.into_iter().map(|(bot_id, name)| {
                json!({"botId": bot_id, "name": name})
            }).collect::<Vec<_>>()}));
        };
        let SpaceDelete::Done { snapshot, deleted, removed, layers } = outcome else {
            return Err(anyhow::anyhow!("space_delete_aborted"));
        };
        // A partir daqui o commit venceu: nada aqui devolve erro ao app — falhas
        // viram aviso no log e a exclusão gravada se mantém (commit → efeitos).
        tracing::info!(space = %space_id, authority = device_key.unwrap_or("local"),
            bots = deleted.len(), "space deleted");
        let _boundary = hub.store.memory_boundary.locked();
        match hub.store.guard_runtime_namespace() {
            Ok(namespace) => crate::guard_hub::mcp::runtime::forget_contexts(&namespace, &removed),
            Err(error) => tracing::warn!(%error, "space delete: runtime namespace unavailable after commit"),
        }
        if let Err(error) = hub.finish_deleted_bots(&deleted) {
            tracing::warn!(%error, "space delete: post-commit bot cleanup failed");
        }
        // Pastas de trabalho no disco: melhor esforço, fora do caminho crítico.
        tokio::task::spawn_blocking(move || {
            // Camadas de memória do Space: a pasta de arquivos de cada camada sai
            // junto; falha vira aviso, nunca desfaz a exclusão já gravada.
            for layer in &layers {
                crate::chat::memory::files::remove_bot(layer);
            }
            for dir in workspaces {
                if let Err(error) = std::fs::remove_dir_all(&dir)
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    tracing::warn!(error = %error, dir = %dir.display(), "space delete: workspace cleanup failed");
                }
            }
        });
        for change in tower {
            change.publish(&hub);
        }
        for group in led.iter().flatten() {
            hub.publish_bot(group);
        }
        let value = serde_json::to_value(&snapshot)?;
        let _ = hub.events.send(json!({"type":"spaces","spaceId":PERSONAL,
            "rev":snapshot.revision,"snapshot":value}));
        Ok(value)
    })
    .await?
}
