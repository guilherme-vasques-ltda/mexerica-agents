//! Preparação pela fábrica MAIN, efeito pela guarda original, ator após commit.
use super::{catalog, model::Add, store, workspace::PreparedWorkspace};
use crate::{LockExt, api::devices::Caller, fork::engine_setup::access, hub::Hub, store::Permission};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fmt::Write as _, sync::Arc};

pub(crate) async fn add(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    with_backend_check(hub, caller, body, crate::agent::backends::is_known).await
}

/// Predicado injetável para provar mudança do registro sem alterar cache global nos testes.
pub(super) async fn with_backend_check(
    hub: &Arc<Hub>,
    caller: &Caller,
    body: Value,
    backend_known: impl Fn(&str) -> bool,
) -> Result<Value> {
    super::super::bot_editor::authorization::require_owner(caller)?;
    let request: Add = serde_json::from_value(body)
        .map_err(|_| anyhow::anyhow!("invalid_field: inclusão inválida ou campo desconhecido."))?;
    super::super::engine_accounts::managed::request_id(&request.client_request_key)?;
    ensure!(request.expected_generation >= 0 && request.expected_revision >= 0, "invalid_field: expectativa inválida.");
    // Nenhuma flag de JSON, origem de turno, backend ou perfil concede autoridade.
    // Conservada ANTES de locks/awaits. Control isolado não substitui posse/lease/ativação.
    let authority = access::resolve(hub, caller)?;
    let digest = Sha256::digest(serde_json::to_vec(&request)?).iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    });
    let guard = hub.store.team_lock.clone().lock_owned().await;
    authority.require_current(hub)?;
    // Recibo imutável antes de consultar qualquer validade dinâmica do registro.
    let replay = authority.with_db(hub, |db| store::replay(db, &hub.host_id, &request, &digest))?;
    if let Some(answer) = replay {
        return Ok(serde_json::to_value(answer)?);
    }
    if let Some(backend) = &request.backend {
        ensure!(backend_known(backend), "invalid_field: motor desconhecido.");
    }
    ensure!(
        request.space_id == super::super::spaces::PERSONAL || request.backend.is_some(),
        "space_backend_selection_required"
    );
    crate::chat::manage::recover_live(hub);
    let existing = authority.with_db(hub, |db| store::apply(db, &hub.host_id, &request, &digest, None))?;
    if let Some(answer) = existing {
        return Ok(serde_json::to_value(answer)?);
    }
    let profile = catalog::profiles()?
        .into_iter()
        .find(|p| p.catalog_key == request.catalog_key)
        .context("invalid_field: perfil desconhecido.")?;
    // Somente campos allowlist. Não passa pelo ramo createBot que herdaria Auto.
    let mut cfg = crate::agent::new_bot::with_defaults(
        hub,
        json!({
            "spaceId":request.space_id,"kind":"agent","name":profile.name,
            "description":profile.description,"title":profile.title,"avatarConfig":profile.avatar_config,
            "backend":request.backend,"permission":"ask","permissionChosenByUser":false,"createdBy":null,
            "computer":false,"manageTeam":false,"connectors":[],"logins":[],"skills":[],
            "sharedMemory":false,"harnessMemory":false
        }),
    )
    .await?;
    // Defesa explícita contra alterações futuras de defaults (sem fingir escolha humana).
    cfg.permission = Permission::Ask;
    cfg.permission_chosen_by_user = false;
    let hub = hub.clone();
    let answer = tokio::task::spawn_blocking(move || -> Result<Value> {
        let _guard = guard;
        authority.require_current(&hub)?;
        let mut workspace = PreparedWorkspace::default();
        let cfg = hub.prepare_new_bot_with_workspace(cfg, |cfg| workspace.prepare(cfg))?;
        let result = {
            let _emit = hub.emit_lock.locked();
            authority.with_db(&hub, |db| store::apply(db, &hub.host_id, &request, &digest, Some(&cfg)))
        };
        let answer = result?.context("native_link_unavailable")?;
        if answer.bot_id == cfg.id {
            // Commit confirmado: nenhuma falha posterior pode limpar o bot persistido.
            workspace.keep();
            if let Err(error) = hub.store.guard_cleanup() {
                tracing::warn!(%error, "inclusão persistida; limpeza de guardas pendente");
            }
            let saved = hub.store.bot(&cfg.id)?.context("native_link_unavailable")?.config;
            hub.start_created_bot(&saved);
        }
        // Um recibo persistido não concede audiência ao titular revogado.
        authority.require_current(&hub)?;
        serde_json::to_value(answer).map_err(Into::into)
    })
    .await??;
    Ok(answer)
}
