use super::{authorization, errors, validation};
use crate::LockExt;
use crate::agent::bot::Cmd;
use crate::api::devices::Caller;
use crate::hub::Hub;
use crate::store::BotConfig;
use anyhow::{Result, bail};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{fmt::Write as _, ops::ControlFlow, sync::Arc};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Read {
    bot_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Save {
    bot_id: String,
    expected_version: String,
    patch: Value,
}

pub async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<ControlFlow<Value, Value>> {
    if !matches!(method, "donnaBotEditor" | "donnaBotEditorSave") {
        return Ok(ControlFlow::Continue(body));
    }
    authorization::require_owner(caller)?;
    let mut body = body;
    if let Some(object) = body.as_object_mut() {
        object.remove("spaceId");
    }
    let engine_authority = if body["patch"].get("forkEditor").is_some() {
        Some(crate::fork::engine_setup::access::resolve(hub, caller)?)
    } else {
        None
    };
    let viewer_authority = crate::fork::engine_setup::access::resolve(hub, caller).ok();
    // D368: autoridade ORIGINAL do salvamento antes da fila; a escrita a reconfere na transação.
    let owner = if method == "donnaBotEditorSave" {
        Some(crate::fork::autoridade::Autoridade::capturar(hub, caller)?)
    } else {
        None
    };
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let hub = hub.clone();
    let method = method.to_owned();
    // O worker conserva o lock mesmo se o cliente fechar a conexão após persistir.
    let answer = tokio::task::spawn_blocking(move || {
        let _guard = guard;
        if engine_authority.as_ref().is_some_and(|authority| !authority.current(&hub)) {
            bail!("remote_setup_unavailable: autorização de contas de IA encerrada.");
        }
        crate::chat::manage::recover_live(&hub);
        let mut answer = if method == "donnaBotEditor" {
            let request: Read = parse(body)?;
            let cfg = agent(&hub, &request.bot_id)?;
            response(&hub, &cfg, reconcile(&hub, &cfg))?
        } else {
            let owner = owner.ok_or_else(|| anyhow::anyhow!("not_allowed: autoridade original ausente"))?;
            save(&hub, &parse(body)?, engine_authority.as_ref(), &owner)?
        };
        if viewer_authority.is_none_or(|authority| !authority.current(&hub)) {
            answer["engineAccounts"] = json!([]);
            answer["engineRuntime"] = Value::Null;
        }
        Ok::<_, anyhow::Error>(answer)
    })
    .await??;
    Ok(ControlFlow::Break(answer))
}

fn parse<T: serde::de::DeserializeOwned>(body: Value) -> Result<T> {
    serde_json::from_value(body).map_err(|_| anyhow::anyhow!("invalid_field: solicitação do editor inválida."))
}

fn agent(hub: &Hub, id: &str) -> Result<BotConfig> {
    let Some(row) = hub.store.bot(id)? else { bail!("bot_not_found: bot não encontrado.") };
    if row.deleted {
        bail!("bot_deleted: este bot foi apagado.");
    }
    if row.config.is_group() {
        bail!("not_allowed: o editor aceita apenas bots agentes.");
    }
    Ok(row.config)
}

/// Inclui configurações legadas e de outras áreas; não inclui revisão/progresso do runtime.
fn version(hub: &Hub, cfg: &BotConfig) -> Result<String> {
    let mut config = json!({"profile":cfg, "primaryBotId":hub.store.team_settings()?.primary_bot_id});
    config.sort_all_objects();
    let bytes = serde_json::to_vec(&config)?;
    Ok(Sha256::digest(bytes).iter().fold(String::from("v1:"), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    }))
}

fn applied(cfg: &BotConfig, patch: &Map<String, Value>) -> Result<bool> {
    // Reescolher o legado explicitamente também remove o v2, mesmo com a mesma projeção.
    if patch.contains_key("avatarConfig") && !patch.contains_key("mexeriquinha") && cfg.mexeriquinha.is_some() {
        return Ok(false);
    }
    let config = serde_json::to_value(cfg)?;
    Ok(patch.iter().all(|(key, value)| config.get(key) == Some(value)))
}

fn save(
    hub: &Arc<Hub>,
    request: &Save,
    authority: Option<&crate::fork::engine_setup::access::Authority>,
    owner: &crate::fork::autoridade::Autoridade,
) -> Result<Value> {
    let before = agent(hub, &request.bot_id)?;
    let before_version = version(hub, &before)?;
    if request.expected_version != before_version {
        bail!("settings_conflict: as configurações mudaram; releia antes de salvar.");
    }
    let patch = validation::patch(&request.patch, &before)?;
    // Retry de um save já persistido apenas reconcilia o ator, sem nova escrita/revisão.
    if patch.is_empty() || applied(&before, &patch)? {
        return response(hub, &before, reconcile(hub, &before));
    }
    let mut intent = patch.clone();
    intent.insert("id".into(), request.bot_id.clone().into());
    if let Err(error) = hub.update_bot_by_user_authorized(&Value::Object(intent), authority, Some(owner)) {
        let saved = agent(hub, &request.bot_id)?;
        if version(hub, &saved)? == before_version || !applied(&saved, &patch)? {
            return Err(errors::save(&error));
        }
        // O Hub pode persistir e falhar ao enviar Reconfigure. Não desfazer nem reportar perda do save.
        hub.emit_bot(&saved.id);
        return response(hub, &saved, reconcile(hub, &saved));
    }
    response(hub, &agent(hub, &request.bot_id)?, "next_turn")
}

/// Reenvia somente a configuração canônica. Um ator indisponível será restaurado pelo Hub ao reiniciar.
/// Read/retry reconcilia antes do próximo envio; não há outra cópia persistente do perfil.
fn reconcile(hub: &Hub, cfg: &BotConfig) -> &'static str {
    if hub.send_cmd(&cfg.id, Cmd::Reconfigure(Box::new(cfg.clone()))).is_ok() { "next_turn" } else { "apply_pending" }
}

fn response(hub: &Hub, cfg: &BotConfig, application: &str) -> Result<Value> {
    // D366: para o Berga, o Hub só oferece aparência e motor; a tela apenas reflete.
    let fixed = hub.store.team_settings()?.primary_bot_id.as_deref() == Some(cfg.id.as_str());
    let can_delete = !fixed && !crate::fork::spaces::is_space_principal(&hub.store.db.locked(), &cfg.id)?;
    let editable = if fixed {
        crate::fork::donna_fixa::editable(&validation::EDITABLE_FIELDS)
    } else {
        validation::EDITABLE_FIELDS.to_vec()
    };
    Ok(json!({
        "botId":cfg.id,
        "settingsVersion":version(hub, cfg)?,
        "profile":{"browserPreference":cfg.browser_preference,"name":cfg.name,"description":if fixed {crate::fork::donna_fixa::BASE} else {cfg.description.as_str()},"avatarConfig":cfg.avatar_config,"mexeriquinha":cfg.mexeriquinha,"title":cfg.title,"backend":cfg.backend,"model":cfg.model,"modelOptions":cfg.model_options,"fastMode":cfg.fast_mode,"forkEditor":cfg.fork_editor},
        "capabilities":{
            "editableFields":editable,
            "unavailableSections":["autonomy","folder","tools","memory","voice","routines","export"]
        },
        "permissions":{"canEdit":true,"canDelete":can_delete},
        "costInfo":null,"speedStatus":null,"engineAccounts":super::super::engine_accounts::managed::editor_accounts(&hub.store)?,"engineRuntime":super::super::engine_accounts::managed::editor_runtime(&hub.store,cfg)?,
        "application":application,
        "workspaceIssue":crate::fork::pasta::aviso(cfg),
        "message":if application == "apply_pending" {"Salvo, aplicação pendente."} else {"Aplica no próximo turno."}
    }))
}
