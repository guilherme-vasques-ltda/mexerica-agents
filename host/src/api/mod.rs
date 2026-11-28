// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! HTTP API: `POST /api/<method>` commands + `GET /events` SSE for this computer's own
//! apps and helpers (loopback + `Authorization: Bearer <token>`), and `GET /channel`, the
//! direct end-to-end encrypted channel phones use (see `channel`).

mod bots;
pub mod devices;
mod events;
pub(crate) mod files;
mod host;
mod http;
mod marketplace;
mod remote;

#[cfg(test)]
pub use events::events_stream;
#[cfg(test)]
pub use events::scoped_events_stream;
pub use events::{scoped_events_stream_at, term_events};
#[cfg(unix)]
pub(crate) use http::open_socket_router;
pub use http::router;

use crate::LockExt;
use crate::api::devices::{Caller, Forbidden};
use crate::hub::Hub;
use crate::market;
use anyhow::{Result, anyhow};
use axum::http::StatusCode;
use serde_json::Value;
use std::ops::ControlFlow;
use std::sync::Arc;

/// `POST /api/<method>` with a method this host doesn't know (404, not 400).
#[derive(Debug)]
struct UnknownMethod;

impl std::fmt::Display for UnknownMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unknown method")
    }
}

impl std::error::Error for UnknownMethod {}

/// Origem do chamador para log: só o tipo, sem chave, sessão ou identificador.
pub(crate) fn origem(caller: &Caller) -> &'static str {
    match caller {
        Caller::Local => "local",
        Caller::Ponte { .. } => "ponte",
        Caller::Device { .. } => "aparelho",
        Caller::Pairing { .. } => "pareamento",
        Caller::Bootstrap { .. } => "bootstrap",
    }
}

/// Recusa de nome sem classe em `fork::acesso_rpc`, antes dos argumentos: desconhecido (404) para todos; na Open, a
/// lista fechada responde a todo nome fora dela com `Forbidden` (403), e o desconhecido segue essa regra. O log leva
/// motivo fixo, a origem e o tamanho do nome; nunca o nome que veio do cliente.
pub(crate) fn recusa_sem_classe(origem: &str, method: &str) -> Option<anyhow::Error> {
    if !crate::fork::acesso_rpc::desconhecido(method) {
        return None;
    }
    tracing::warn!(reason = "unknown_rpc_method", origin = origem, method_len = method.len(), "rpc refused");
    Some(if crate::environment::Environment::is_open() { Forbidden.into() } else { UnknownMethod.into() })
}

/// Stable codes are additive; the displayed message remains independently useful.
#[derive(Debug)]
pub(crate) struct GroupError {
    pub code: &'static str,
    pub message: &'static str,
}

impl std::fmt::Display for GroupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message)
    }
}

impl std::error::Error for GroupError {}

pub(crate) fn error_code(e: &anyhow::Error) -> Option<&'static str> {
    e.downcast_ref::<GroupError>().map(|error| error.code)
}

/// HTTP status and message for a failed call; channels send the same pair in `err`.
pub fn error_status(e: &anyhow::Error) -> (StatusCode, String) {
    if let Some(message) = crate::fork::donna_fixa::wire(e) {
        (StatusCode::BAD_REQUEST, message.to_owned())
    } else if e.is::<UnknownMethod>() {
        (StatusCode::NOT_FOUND, e.to_string())
    } else if e.is::<Forbidden>() {
        (StatusCode::FORBIDDEN, e.to_string())
    } else if e.is::<crate::remote::cloud::Conflict>() || crate::fork::workspace::is_mismatch(e) {
        (StatusCode::CONFLICT, e.to_string())
    } else {
        // `{:#}` keeps the context chain ("starting `npx …`: No such file or directory").
        (StatusCode::BAD_REQUEST, format!("{e:#}"))
    }
}

fn str_arg<'a>(b: &'a Value, k: &str) -> Result<&'a str> {
    b[k].as_str().ok_or_else(|| anyhow!("`{k}` is required"))
}

tokio::task_local! {
    /// Marca Hub-side de turno automático: só o código do próprio Hub entra por aqui (acordar de tarefa agendada,
    /// aviso de comentário). O corpo do pedido não define nem apaga esta marca — ela nunca sai do Hub.
    pub(crate) static AUTOMATICO: ();
}

/// Turno automático do próprio Hub (acordar de tarefa, aviso de comentário): entra como chamador local, e o `send`
/// carimba a entrada com a origem automática do Hub (`contentOrigin.hubOrigin`), que nenhum corpo define sozinho.
pub(crate) async fn dispatch_do_hub(hub: &Arc<Hub>, method: &str, b: Value) -> Result<Value> {
    AUTOMATICO.scope((), dispatch(hub, &Caller::Local, method, b)).await
}

/// Runs one API method for `caller` (permissions per spec §6.6).
pub async fn dispatch(hub: &Arc<Hub>, caller: &Caller, method: &str, b: Value) -> Result<Value> {
    // PL1 (PR3): a origem pessoal durável recusa a rota genérica ANTES da captura D368 e de olhar o nome.
    crate::fork::personal_bootstrap::guard_generic(hub, caller)?;
    // Fail-closed: método sem classe em `fork::acesso_rpc` não existe para ninguém, nem para este computador.
    if let Some(recusa) = recusa_sem_classe(origem(caller), method) {
        return Err(recusa);
    }
    crate::open_hub::check_request(caller, method, &b)?;
    let b = crate::fork::workspace::conferir_pedido(hub, origem(caller), b)?;
    // D368: a autoridade original do app do dono acompanha o pedido inteiro (`donna_fixa::pedido`).
    crate::fork::donna_fixa::pedido::com_pedido(hub, caller, Box::pin(dispatch_pedido(hub, caller, method, b))).await
}

async fn dispatch_pedido(hub: &Arc<Hub>, caller: &Caller, method: &str, b: Value) -> Result<Value> {
    crate::fork::sessao_clerk::negar_sem_cerca(hub, caller, method)?;
    crate::fork::ponte_credencial::conferir(hub, caller, method, &b)?;
    crate::fork::autoridade::conferir_caller(hub, caller)?;
    if method.starts_with("donnaWorkplace") {
        crate::fork::onde_roda::autoridade::require(caller, method)?;
        devices::permit(caller, method)?;
        let mut answer = crate::fork::onde_roda::api::call(hub, caller, method, b).await?;
        answer["hubId"] = hub.host_id.clone().into();
        return Ok(answer);
    }
    if method == "engineGoKeySave" {
        devices::permit(caller, method)?;
        return crate::fork::conexao_motores::save_go(hub, caller, &b).await;
    }
    if method == "setAgentEnv" || method == "agentAuthenticate" {
        crate::fork::engine_setup::require_local(caller)?;
    }
    if crate::fork::engine_setup::methods(method) || (method == "agentSetup" && b["step"] == "login") {
        crate::fork::engine_setup::access::require(hub, caller)?;
    }
    let engine_authority = if (matches!(method, "updateBot" | "createBot") && b.get("forkEditor").is_some())
        || matches!(method, "teamSettings" | "donnaSpaceBirth" | "donnaSpacePrincipal" | "donnaSpaceDelete")
    {
        Some(crate::fork::engine_setup::access::resolve(hub, caller)?)
    } else {
        None
    };
    if matches!(method, "agentSetup" | "agentAuthenticate") && !(method == "agentSetup" && b["step"] == "login") {
        crate::fork::engine_setup::require_legacy_setup(caller, b["backend"].as_str().unwrap_or_default())?;
    }
    devices::permit(caller, method)?;
    if method == "teamSettings" && !matches!(caller, Caller::Local) {
        crate::fork::engine_setup::access::require(hub, caller)?;
    }
    if matches!(method, crate::fork::control_tower::METHOD | crate::fork::control_tower::ATTENTION_METHOD) {
        crate::fork::control_tower::require(hub, caller)?;
    }
    if method == crate::fork::control_tower::ATTENTION_METHOD {
        return crate::fork::control_tower::attention_call(hub, caller, b).await;
    }
    if method == crate::fork::torre_visoes::METHOD {
        return crate::fork::torre_visoes::call(hub, caller, b).await;
    }
    if crate::fork::governanca::METHODS.contains(&method) {
        return crate::fork::governanca::call(hub, caller, method, b).await;
    }
    // Projetos conserva hubId no DTO e recusa outros contextos sem oráculo de Space.
    if crate::fork::projetos::METHODS.contains(&method) {
        return crate::fork::projetos::call(hub, caller, method, b).await;
    }
    if crate::fork::tarefas_runtime::METHODS.contains(&method) {
        return crate::fork::tarefas_runtime::call(hub, caller, method, b).await;
    }
    if crate::fork::propostas::METHODS.contains(&method) {
        return crate::fork::propostas::call(hub, caller, method, b).await;
    }
    crate::fork::secret_refs::reject_api(method, &b)?;
    crate::fork::spaces::guard(hub, method, &b)?;
    let mut b = b;
    // D545: a exclusão conserva o seletor obrigatório até a validação própria do pedido.
    if method != "sessionDelete"
        && let Some(body) = b.as_object_mut()
    {
        body.remove("hubId");
    }
    // Chaves de API do Motor Mexerica: só este computador, nunca devolvem a chave.
    if crate::fork::motor_mexerica::CHAVE_METHODS.contains(&method) {
        return crate::fork::motor_mexerica::chaves_call(hub, caller, method, b).await;
    }
    // Entrar com OpenRouter (OAuth PKCE): só este computador; a chave vai direto ao cofre, nunca ao app.
    if crate::fork::motor_mexerica::ENTRAR_METHODS.contains(&method) {
        return crate::fork::motor_mexerica::entrar_openrouter_call(hub, caller, method, b).await;
    }
    // Entrar com ChatGPT (OAuth PKCE): só este computador; os tokens vão direto ao cofre, nunca ao app.
    if crate::fork::motor_mexerica::CHATGPT_METHODS.contains(&method) {
        return crate::fork::motor_mexerica::entrar_chatgpt_call(hub, caller, method, b).await;
    }
    // Planos de código (chave colada, OpenCode como motor): só este computador, nunca devolvem a chave.
    if crate::fork::planos_codigo::METHODS.contains(&method) {
        return crate::fork::planos_codigo::call(hub, caller, method, b).await;
    }
    // C6: catálogo interno da ponte `chat` da extra selada; só lê, nunca executa (exceção fechada).
    if crate::fork::sessoes::contexto_ferramenta::is_catalog(method, &b) {
        return crate::fork::sessoes::contexto_ferramenta::catalog(hub, caller, &b);
    }
    let preparation = if method == "memoryCall" && b["name"] == "__tools" {
        crate::fork::sessoes::gateway::capture_memory_catalog(hub, &b)?
    } else {
        None
    };
    let execution = if preparation.is_none()
        && matches!(
            method,
            "chatCall"
                | "memoryCall"
                | "manageCall"
                | "teamCall"
                | "taskCall"
                | "routineCall"
                | "computersCall"
                | "computerCall"
                | "connectorCall"
        ) {
        Some(crate::fork::sessoes::bindings::capture(
            hub,
            str_arg(&b, "botId")?,
            b["sessionBinding"].as_str().ok_or_else(|| anyhow!("chamada vencida: vínculo de sessão ausente"))?,
        )?)
    } else {
        None
    };
    let observation = if method == "computerCall" {
        Some(crate::screen::ObservationAttempt::capture_original(
            hub,
            caller,
            str_arg(&b, "botId")?,
            execution.as_ref().ok_or_else(|| anyhow!("chamada vencida"))?,
            crate::screen::observation_read_only(str_arg(&b, "name")?),
        )?)
    } else {
        None
    };
    let catalog_context =
        if preparation.is_some() { crate::fork::spaces::tool_context::capture_catalog(hub, &b)? } else { None };
    crate::fork::sessoes::contexto_ferramenta::require_ponte(caller, method, &b)?;
    // C6: só `mostrar_componente` numa extra (`session:`) toma o ramo pessoal, com a Ponte original (SEG-C6-01).
    if let Some(answer) =
        crate::fork::sessoes::contexto_ferramenta::route(hub, caller, method, &b, execution.as_ref()).await?
    {
        return Ok(answer);
    }
    if preparation.is_none()
        && let Some(answer) =
            crate::fork::spaces::tool_context::route_in_execution(hub, method, &b, execution.as_ref()).await?
    {
        return Ok(answer);
    }
    let observation = if let Some(original) = observation { Some(original.authorize(None).await?) } else { None };
    let observed = crate::analytics::observe_call(hub, caller, method, &b);
    let mut result =
        route(hub, caller, method, b, engine_authority.as_ref(), execution.as_ref(), observation.as_ref()).await;
    if let Some(catalog) = catalog_context {
        result = result.and_then(|answer| {
            catalog.check(hub)?;
            Ok(answer)
        });
    }
    if let Some(preparation) = preparation {
        result = result.and_then(|answer| {
            preparation.check_runtime(&hub.runtime(preparation.bot()))?;
            preparation.check(&hub.store.db.locked())?;
            Ok(answer)
        });
    }
    if (method.starts_with("donnaSpace")
        || method.starts_with("donnaConversation")
        || method == "donnaOwnerPresentation")
        && let Ok(answer) = &mut result
    {
        answer["hubId"] = hub.host_id.clone().into();
    }
    if let (Ok(answer), Some((event, properties))) = (&result, observed)
        && let Some(properties) = crate::analytics::answered(event, properties, answer)
    {
        crate::analytics::capture(hub, event, properties);
    }
    result
}

async fn route(
    hub: &Arc<Hub>,
    caller: &Caller,
    method: &str,
    b: Value,
    engine_authority: Option<&crate::fork::engine_setup::access::Authority>,
    execution: Option<&crate::fork::sessoes::ExecutionRef>,
    observation: Option<&Arc<crate::screen::ObservationAttempt>>,
) -> Result<Value> {
    // Retired discovery must not unlock the vault or mutate runtime/configuration.
    market::reject_registry_request(method, &b)?;
    if matches!(method, "agentAuthenticate" | "setAgentEnv") {
        crate::agent::backends::require_supported_execution(str_arg(&b, "backend")?)?;
    }
    #[cfg(test)]
    if matches!(method, "connectorCall" | "routineCall" | "teamCall") {
        crate::fork::sessoes::call_tests::before_effect(&hub.store, "api_origin");
    }
    if method == "connectorSignIn"
        && crate::environment::Environment::is_open()
        && crate::fork::conectores::oauth_desktop::do_pedido(&b)?.is_none()
    {
        anyhow::bail!("login_sem_ouvinte: o retorno do login só chega pelo ouvinte do app neste computador.");
    }
    let oauth_intent = if method == "connectorSignIn" {
        Some(crate::fork::conectores::oauth_binding::StartIntent::capture(hub, caller, str_arg(&b, "id")?)?)
    } else {
        None
    };
    #[cfg(test)]
    if oauth_intent.is_some() {
        market::oauth::upstream02_tests::before_unlock(hub).await;
    }
    // connectorRequestGet é leitura da entrada (projeção sem valor) para a janela segura abrir
    // com o cofre trancado; o finish continua exigindo o cofre pelo finish_requires_vault.
    let connector_needs_vault = match method {
        "connectorRequestGet" => false,
        "connectorRequestFinish" => market::requests::finish_requires_vault(hub, &b)?,
        _ => true,
    };
    if (method.contains("onnector") && connector_needs_vault)
        || method.starts_with("credential")
        // Only signing in reads the vault; other computer tools work without a keyring.
        // type_login with field "code" também não: o código vive só na memória do Hub.
        || (method == "computerCall" && b["name"] == "type_login" && b["arguments"]["field"] != "code")
        || method.starts_with("voice")
        || method == "setVoiceKey"
        // Só chave/geração tocam o cofre: estado e transporte de quadros não
        // devem pedir desbloqueio do chaveiro ao exibir avatar.
        || matches!(method, "mexeriquinhaGeradorChave" | "mexeriquinhaGerar")
        || matches!(
            method,
            "marketConectar" | "marketGoogleAppSalvar" | "marketAppProprioSalvar" | "marketPersonalizado" | "installConnector"
        )
        || (matches!(method, "agentAuth" | "agentAuthenticate" | "setAgentEnv")
            && !b["backend"].as_str().is_some_and(crate::agent::backends::is_antigravity))
    {
        market::vault::unlock_para(hub.clone(), caller).await?;
    }
    if let Some(attempt) = observation {
        let args = b.get("arguments").filter(|a| a.is_object()).cloned().unwrap_or_else(|| serde_json::json!({}));
        return crate::screen::computer(hub, str_arg(&b, "botId")?, str_arg(&b, "name")?, args, Some(attempt)).await;
    }
    if let Some(intent) = oauth_intent {
        // Hub em outro computador (túnel): o app diz a porta do ouvinte dele; só o chamador local pode pedir, e só as
        // portas do app valem. Quem não é o app local segue pela página da nuvem, e a porta de um aparelho é recusa.
        let desktop = crate::fork::conectores::oauth_desktop::do_pedido(&b)?;
        let callback = match (caller, desktop) {
            (Caller::Local, Some(port)) => market::oauth::Callback::Desktop(port),
            (Caller::Local, None) => market::oauth::Callback::Host,
            (_, None) => market::oauth::Callback::App,
            (_, Some(_)) => anyhow::bail!(crate::fork::conectores::oauth_desktop::RECUSA),
        };
        return market::oauth::start(hub, intent, callback).await;
    }
    if method == "readFile" {
        return files::read(hub, &b).await;
    }
    if method == "donnaSpaceBirth" {
        return crate::fork::spaces::birth_authorized(
            hub,
            engine_authority.ok_or_else(|| anyhow!("space_birth_authority_required"))?,
            b,
        )
        .await;
    }
    if method == "donnaSpacePrincipal" {
        return crate::fork::spaces::principal_authorized(
            hub,
            engine_authority.ok_or_else(|| anyhow!("space_principal_authority_required"))?,
            b,
        )
        .await;
    }
    if method == "donnaSpaceDelete" {
        return crate::fork::spaces::delete_authorized(
            hub,
            engine_authority.ok_or_else(|| anyhow!("space_delete_authority_required"))?,
            b,
        )
        .await;
    }
    // Each method group answers its own methods and hands the body back otherwise.
    let b = match crate::fork::api::call(hub, caller, method, b).await? {
        ControlFlow::Break(v) => return Ok(v),
        ControlFlow::Continue(b) => b,
    };
    let b = match crate::hub::bot_groups::call(hub, method, b).await? {
        ControlFlow::Break(v) => return Ok(v),
        ControlFlow::Continue(b) => b,
    };
    let b = match crate::computers::executor_api::call(hub, caller, method, b, execution).await? {
        ControlFlow::Break(v) => return Ok(v),
        ControlFlow::Continue(b) => b,
    };
    let b = match host::call(hub, caller, method, b).await? {
        ControlFlow::Break(v) => return Ok(v),
        ControlFlow::Continue(b) => b,
    };
    let b = match marketplace::call(hub, caller, method, b, execution).await? {
        ControlFlow::Break(v) => return Ok(v),
        ControlFlow::Continue(b) => b,
    };
    let b = match bots::call(hub, caller, method, b, engine_authority, execution).await? {
        ControlFlow::Break(v) => return Ok(v),
        ControlFlow::Continue(b) => b,
    };
    match remote::call(hub, caller, method, b).await? {
        ControlFlow::Break(v) => Ok(v),
        ControlFlow::Continue(_) => Err(UnknownMethod.into()),
    }
}
