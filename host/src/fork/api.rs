//! Ponto único de despacho das intenções próprias do fork.
use crate::{api::devices::Caller, hub::Hub};
use anyhow::Result;
use serde_json::Value;
use std::{ops::ControlFlow, sync::Arc};

pub async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<ControlFlow<Value, Value>> {
    if matches!(method, "memoryProfile" | "saveMemoryProfile") {
        return crate::chat::memory::profile::call(hub, caller, method, body).await.map(ControlFlow::Break);
    }
    if super::nunca_fazer::METHODS.contains(&method) {
        return super::nunca_fazer::call(hub, caller, method, body).map(ControlFlow::Break);
    }
    if super::instrucao_geral::METHODS.contains(&method) {
        return super::instrucao_geral::call(hub, caller, method, body).map(ControlFlow::Break);
    }
    if method == super::browser::METHOD {
        return super::browser::status(hub, caller, body).map(ControlFlow::Break);
    }
    if matches!(method, super::browser::local::STATE | super::browser::local::CONSENT | super::browser::local::FRAME) {
        return super::browser::local::call(hub, caller, method, body).await.map(ControlFlow::Break);
    }
    if method == super::workspace::METHOD {
        return super::workspace::call(hub, caller).map(ControlFlow::Break);
    }
    if method == super::tela_por_computador::METHOD {
        return super::tela_por_computador::call(hub, caller, &body).map(ControlFlow::Break);
    }
    if method == super::personal_bootstrap::STATUS_METHOD {
        return super::personal_bootstrap::status(caller, &body).map(ControlFlow::Break);
    }
    if method == super::open_modelo::SET_METHOD {
        return super::open_modelo::call(hub, caller, body).await.map(ControlFlow::Break);
    }
    if super::onboarding::METHODS.contains(&method) {
        let body = super::onboarding::call(hub, caller, method, body).await?;
        return Ok(ControlFlow::Break(body));
    }
    if crate::chat::memory::WRITES.contains(&method)
        || matches!(method, "approveMemorySuggestion" | "discardMemorySuggestion")
    {
        let original = super::autoridade::Autoridade::capturar(hub, caller)?;
        return crate::chat::memory::suggestion_call(hub, method, &body, original).await.map(ControlFlow::Break);
    }
    if super::routine_creation::handles(method) {
        return super::routine_creation::call(hub, caller, method, body).await.map(ControlFlow::Break);
    }
    if super::extensoes_em_todos::METHODS.contains(&method) {
        return super::extensoes_em_todos::call(hub.clone(), caller.clone(), method, body)
            .await
            .map(ControlFlow::Break);
    }
    if super::usage_limits::handles(method) {
        return super::usage_limits::call(hub, caller, method, body).await.map(ControlFlow::Break);
    }
    if super::teto::handles(method) {
        return super::teto::call(hub, caller, method, body).await.map(ControlFlow::Break);
    }
    let body = match super::conectores::api::call(hub, caller, method, body).await? {
        ControlFlow::Break(value) => return Ok(ControlFlow::Break(value)),
        ControlFlow::Continue(body) => body,
    };
    if super::rules::api::METHODS.contains(&method) {
        return super::rules::api::call(hub, caller, method, body).await;
    }
    if method.starts_with("mexeriquinha") {
        return crate::fork_avatars::gerador::call(hub, caller, method, body).await.map(ControlFlow::Break);
    }
    if method == "donnaEngineAccess" {
        return Ok(ControlFlow::Break(super::engine_setup::access::capability(hub, caller)));
    }
    if super::engine_setup::methods(method) {
        return super::engine_setup::call(hub, caller, method, body).await.map(ControlFlow::Break);
    }
    if method == "setAgentEnv" && body.get("engineAccountId").is_some() {
        return super::engine_setup::set_api_env(hub, caller, body).await.map(ControlFlow::Break);
    }
    let body = match super::spaces::call(hub, caller, method, body).await? {
        ControlFlow::Break(v) => return Ok(ControlFlow::Break(v)),
        ControlFlow::Continue(body) => body,
    };
    if super::compressao::METHODS.contains(&method) {
        return super::compressao::call(hub, caller, method, body).map(ControlFlow::Break);
    }
    match method {
        super::idioma_app::METHOD => super::idioma_app::call(hub, caller, &body).map(ControlFlow::Break),
        super::custo::METHOD => super::custo::call(hub, caller, body).await.map(ControlFlow::Break),
        super::uso_historico::METHOD => super::uso_historico::call(hub, caller, body).map(ControlFlow::Break),
        super::armazenamento::METHOD => super::armazenamento::call(hub, caller, body).await.map(ControlFlow::Break),
        m if super::contexto::METHODS.contains(&m) => {
            super::contexto::call(hub, caller, method, body).await.map(ControlFlow::Break)
        }
        super::control_tower::METHOD => super::control_tower::call(hub, caller, body).await.map(ControlFlow::Break),
        "donnaHubDoctor" => super::doctor::call(hub, caller, body).await.map(ControlFlow::Break),
        super::resumo_diario::api::METHOD => {
            super::resumo_diario::api::call(hub, caller, body).await.map(ControlFlow::Break)
        }
        "backgroundHistory" => {
            if !matches!(caller, Caller::Local) {
                return Err(crate::api::devices::Forbidden.into());
            }
            anyhow::ensure!(
                body.as_object().is_some_and(serde_json::Map::is_empty),
                "invalid_field: consulta inválida"
            );
            Ok(ControlFlow::Break(serde_json::to_value(super::background_history::read_para_tela(&hub.store)?)?))
        }
        super::native_catalog::METHOD => super::native_catalog::read(hub, caller, body).map(ControlFlow::Break),
        super::native_catalog::ADD_METHOD => {
            super::native_catalog::add(hub, caller, body).await.map(ControlFlow::Break)
        }
        "donnaGuardRuntime" | "donnaGuardObserve" | "donnaGuardRelease" => {
            crate::guard_hub::mcp::call(hub, caller, method, body).await.map(ControlFlow::Break)
        }
        m if super::roteamento::api::METHODS.contains(&m) => {
            super::roteamento::api::call(hub, caller, method, body).await.map(ControlFlow::Break)
        }
        m if super::whatsapp::api::handles(m) => {
            super::whatsapp::api::call(hub, caller, method, body).await.map(ControlFlow::Break)
        }
        "donnaBotEditor" | "donnaBotEditorSave" => super::bot_editor::api::call(hub, caller, method, body).await,
        m if super::delegacao::api::METHODS.contains(&m) => {
            super::delegacao::api::call(hub, caller, method, body).await.map(ControlFlow::Break)
        }
        super::pedidos_time::METHOD => super::pedidos_time::call(hub, caller, body).await.map(ControlFlow::Break),
        "donnaTimes" | "donnaTimesPreview" | "donnaTimesSave" => {
            super::times::api::call(hub, caller, method, body).await
        }
        "donnaEngineAccounts" => super::engine_accounts::call(hub, caller, body).await.map(ControlFlow::Break),
        "donnaSkillsDoutor" => super::skills_doutor::call(hub, caller, &body).map(ControlFlow::Break),
        "sessions" | "sessionCreate" | "sessionRename" => {
            super::sessoes::api::call(hub, method, &body).map(ControlFlow::Break)
        }
        "sessionEngineSet" => super::sessoes::api::engine_set(hub, caller, body).await.map(ControlFlow::Break),
        "sessionDelete" => super::sessoes::excluir::delete(hub, caller, body).await.map(ControlFlow::Break),
        "sessionClose" => super::sessoes::encerrar::close(hub, caller, body).await.map(ControlFlow::Break),
        "sessionContinue" => super::sessoes::encerrar::continue_(hub, caller, body).await.map(ControlFlow::Break),
        "sessionChoiceAnswer" => super::sessoes::escolha::answer(hub, caller, body).await.map(ControlFlow::Break),
        "sessionAutoCloseConfig" | "sessionAutoCloseSave" => {
            super::sessoes::encerrar::config(hub, caller, method, body).await.map(ControlFlow::Break)
        }
        "backgroundSpaceSelections" | "saveBackgroundSpaceSelection" => {
            super::background_space_selection::call(hub, caller, method, body).await.map(ControlFlow::Break)
        }
        m if super::memory_learning::METHODS.contains(&m) => {
            super::memory_learning::call(hub, caller, method, body).map(ControlFlow::Break)
        }
        m if super::mail_manager::api::METHODS.contains(&m) => {
            super::mail_manager::api::call(hub, caller, method, body).map(ControlFlow::Break)
        }
        "functionModels"
        | "saveFunctionModel"
        | "restoreFunctionModel"
        | "restoreFunctionModels"
        | "checkFunctionModel" => {
            super::modelos_funcao::api::call(hub, caller, method, body).await.map(ControlFlow::Break)
        }
        _ => Ok(ControlFlow::Continue(body)),
    }
}
