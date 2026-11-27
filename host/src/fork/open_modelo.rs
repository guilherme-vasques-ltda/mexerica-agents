//! Na Open, a escolha de conta e modelo do Codex para os bots. Só aceita uma conta Open conectada e fora de setup e
//! um modelo que o `model/list` dessa conta comprovou. Sem `botId`, aplica a escolha a todos os bots que ainda não
//! escolheram conta nem modelo e nunca sobrescreve quem já escolheu; com `botId`, troca só aquele bot.
use crate::LockExt;
use crate::api::devices::{Caller, Forbidden};
use crate::fork::engine_accounts::{Registry, State};
use crate::fork::engine_setup;
use crate::hub::Hub;
use crate::store::{BotConfig, BotRow, Store};
use anyhow::{Result, anyhow, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) const SET_METHOD: &str = "donnaOpenModelSet";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Set {
    account_id: String,
    model: String,
    /// Raciocínio e velocidade do modelo (`reasoning_effort`, `service_tier`), só para um bot. Sem este campo, o bot
    /// mantém as escolhas se o modelo não mudou. Cada valor tem de constar no `model/list` da conta (`validate_selection`).
    #[serde(default)]
    options: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    bot_id: Option<String>,
    /// O Space do cliente vinculado. Com `botId`, o bot tem de ser desse Space (sem Space, do Pessoal); o lote sem
    /// `botId` é global e só vale sem Space ou no Pessoal.
    #[serde(default)]
    space_id: Option<String>,
    #[serde(default)]
    hub_id: Option<String>,
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    if !crate::environment::Environment::is_open() {
        return Err(Forbidden.into());
    }
    engine_setup::require_local(caller)?;
    let request: Set =
        serde_json::from_value(body).map_err(|_| anyhow!("invalid_field: escolha de conta e modelo inválida."))?;
    let space = check_scope(hub, &request)?;
    let authority = engine_setup::access::resolve(hub, caller)?;
    let owner = crate::fork::autoridade::Autoridade::capturar(hub, caller)?;
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let hub = hub.clone();
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        apply(&hub, &request, &space, &authority, &owner)
    })
    .await?
}

/// Contrato por Space: devolve o Space do pedido (sem Space, o Pessoal). O lote global só vale no Pessoal.
fn check_scope(hub: &Hub, request: &Set) -> Result<String> {
    ensure!(request.hub_id.as_deref().is_none_or(|id| id == hub.host_id), "cross_hub_reference: pedido de outro Hub.");
    let personal = hub.store.spaces()?.personal_space_id;
    let space = request.space_id.clone().unwrap_or_else(|| personal.clone());
    ensure!(
        request
            .options
            .as_ref()
            .is_none_or(|o| request.bot_id.is_some()
                && o.keys().all(|k| matches!(k.as_str(), "reasoning_effort" | "service_tier"))),
        "invalid_field: opções só valem para um bot, de raciocínio ou velocidade."
    );
    ensure!(
        request.bot_id.is_some() || space == personal,
        "space_not_personal: a escolha para todos os bots é feita no Pessoal."
    );
    Ok(space)
}

/// Bot sem escolha: agente do Codex sem conta e sem modelo gravados.
fn without_choice(cfg: &BotConfig) -> bool {
    !cfg.is_group()
        && cfg.backend == "codex"
        && cfg.fork_editor.engine_account_id.is_none()
        && cfg.model.is_none()
        && cfg.command.is_none()
}

/// O lote inteiro numa transação, sob a cerca das mutações de conta (`RUNNING`): a conta conferida é a que fica
/// gravada, e um bot mudado ou apagado no meio derruba o lote todo, sem gravação parcial.
fn apply(
    hub: &Arc<Hub>,
    request: &Set,
    space: &str,
    authority: &engine_setup::access::Authority,
    owner: &crate::fork::autoridade::Autoridade,
) -> Result<Value> {
    let (account, applied) = crate::fork::open_chat::with_account_fence(|| {
        let registry = Registry::load(&hub.store)?;
        let account = registry
            .accounts
            .iter()
            .find(|a| a.id == request.account_id && engine_setup::is_open_account(a) && a.state == State::Connected)
            .ok_or_else(|| anyhow!("account_unavailable: reconecte a conta."))?;
        ensure!(!engine_setup::in_setup(&hub.store, &account.id), "open_chat_reconnecting");
        ensure!(
            account.models.iter().any(|m| m.id == request.model),
            "model_unavailable: modelo não comprovado nesta conta."
        );
        let targets: Vec<BotRow> = match request.bot_id.as_deref() {
            Some(id) => vec![
                hub.store
                    .bot(id)?
                    .filter(|row| !row.deleted && !row.config.is_group() && row.config.space_id == space)
                    .ok_or_else(|| anyhow!("bot_not_found: bot não encontrado."))?,
            ],
            None => hub.store.bots()?.into_iter().filter(|row| !row.deleted && without_choice(&row.config)).collect(),
        };
        let mut planned = Vec::with_capacity(targets.len());
        for row in targets {
            let mut cfg = row.config.clone();
            let same_choice = row.config.model.as_deref() == Some(request.model.as_str())
                && row.config.fork_editor.engine_account_id.as_deref() == Some(account.id.as_str());
            cfg.model = Some(request.model.clone());
            cfg.fork_editor.engine_account_id = Some(account.id.clone());
            cfg.command = None;
            match &request.options {
                Some(options) => cfg.model_options.clone_from(options),
                None if !same_choice => cfg.model_options.clear(),
                None => {}
            }
            crate::fork::lider_canal::validate(&mut cfg, Some(&row.config))?;
            crate::hub::validate_update(&mut cfg, &row.config)?;
            crate::market::contas::validar_vinculo(&hub.store, Some(&row.config), &mut cfg)?;
            crate::fork::engine_accounts::managed::validate_selection(&hub.store, &cfg)?;
            planned.push((row.rev, cfg));
        }
        {
            let _boundary = hub.store.memory_boundary.locked();
            let _emit = hub.emit_lock.locked();
            crate::fork::donna_fixa::pedido::antes_de_gravar(&hub.store);
            let by_bot = request.bot_id.as_deref();
            owner.with_db(hub, Some(authority), |db| {
                for (rev, cfg) in &planned {
                    let current = Store::bot_on(db, &cfg.id)?.filter(|row| !row.deleted);
                    ensure!(
                        current
                            .is_some_and(|row| row.rev == *rev && (by_bot.is_none() || row.config.space_id == space)),
                        "bot_changed: o bot mudou enquanto a escolha era gravada; tente de novo."
                    );
                    crate::fork::donna_fixa::como_dono(db, |db| crate::store::bots::save_bot(db, cfg))?;
                }
                Ok(())
            })?;
            hub.store.guard_cleanup()?;
        }
        Ok((account.id.clone(), planned.into_iter().map(|(_, cfg)| cfg.id).collect::<Vec<_>>()))
    })?;
    for id in &applied {
        hub.emit_bot(id);
    }
    Ok(json!({"accountId": account, "model": request.model, "applied": applied}))
}

#[cfg(test)]
#[path = "open_modelo_tests.rs"]
mod tests;
