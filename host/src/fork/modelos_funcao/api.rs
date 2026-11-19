//! Contrato local para a tela do Bot de fundo.
//!
//! `functionModels` recebe `{}` e retorna documento, contas e funções com
//! seleção, resolução e consumidor. `checkFunctionModel` recebe `functionId`.
//! `saveFunctionModel` recebe `expectedRevision`, `functionId` e `selection`;
//! `restoreFunctionModel` recebe os dois primeiros, e `restoreFunctionModels`
//! apenas `expectedRevision`. Escritas usam revisão para detectar conflitos e
//! emitem `functionModelsChanged`. `backgroundHistory`, na API do fork, recebe
//! `{}` e retorna até 50 entradas em ordem de gravação; `backgroundHistoryChanged`
//! sinaliza releitura. Cada entrada tem `id`, `function` (`memory` ou `titles`),
//! `botId`, `status`, `at` em milissegundos, `effectiveModels`, `usage` opcional
//! e `failureStage` opcional. O histórico informa etapa da falha, não seu motivo
//! detalhado; `response_received` pode preceder o resultado aplicado.
//! Todos esses métodos exigem `Caller::Local`.
//!
//! Em `memory_consolidation`, `resolution.unavailable.reason` é
//! `account_disconnected` quando a conta escolhida está desconectada (rótulo em
//! `accounts[].label` pelo `selection.accountId`) e `unavailable.message` traz o
//! texto pronto; sem modelo, o motivo é `default_model_unknown` com o texto de
//! escolher um modelo (MEX-1355).
//!
//! `available` descreve a seleção, não garante execução: identidade, política e
//! uso são conferidos novamente por tarefa. `execution: partial` e `consumer`
//! identificam as funções já conectadas. A seleção é global para o Pessoal;
//! esta API não autoriza conteúdo nem seleção de outros Spaces.

use super::{
    catalogo::{FUNCTIONS, FunctionId},
    config::{self, SelectionInput},
    resolver::{self, TrustedContext},
};
use crate::{
    api::devices::{Caller, Forbidden},
    hub::Hub,
};
use anyhow::{Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Save {
    expected_revision: u64,
    function_id: FunctionId,
    selection: SelectionInput,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Restore {
    expected_revision: u64,
    function_id: FunctionId,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RestoreAll {
    expected_revision: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Check {
    function_id: String,
}

fn parse<T: serde::de::DeserializeOwned>(body: Value) -> Result<T> {
    serde_json::from_value(body).map_err(|_| anyhow!("invalid_field: solicitação de modelos por função inválida."))
}
fn resolution(hub: &Hub, id: &str, caller: &Caller, revision: Option<u64>) -> Value {
    if matches!(id, "memory_consolidation" | "bot_naming")
        && let Ok(document) = config::load(&hub.store)
        && let Ok(function) = serde_json::from_value::<FunctionId>(id.into())
        && let Some(selection) = document.functions.get(&function)
    {
        let binding = match selection {
            config::Selection::Custom { binding } => Some(binding),
            config::Selection::Automatic { binding, .. } => binding.as_ref(),
            config::Selection::ManagedInVoice => None,
        };
        if let Some(binding) = binding
            && let Ok(accounts) = resolver::accounts(&hub.store)
            && let Some(account) = accounts.iter().find(|account| account.id == binding.account_id)
            && matches!(account.billing.as_str(), "paid_api" | "api")
        {
            return json!({"status":"unavailable", "unavailable":{
                "functionId":id, "reason":"api_coming_soon", "revision":document.revision,
                "message":"Em breve: contas com chave de API poderão ser usadas no Bot de fundo. Ainda estamos preparando a confirmação dos modelos disponíveis para cada conta."
            }});
        }
        if revision.is_none_or(|value| value == document.revision)
            && let Ok(bot) = serde_json::from_value::<crate::store::BotConfig>(json!({
                "id":"background-worker", "name":"Bot de fundo", "backend":"claude"
            }))
            && let Ok(resolved) = crate::fork::background_selection::resolve(&hub.store, &bot, function)
        {
            return json!({"status":"available", "resolved":{
                "functionId":id, "requested":selection, "owner":"owner", "billing":"subscription",
                "revision":document.revision, "defaultsRevision":document.defaults_revision,
                "effective":{"accountId":resolved.config.fork_editor.engine_account_id,
                    "modelId":resolved.config.model, "modelOptions":resolved.config.model_options},
                "execution":"verified_before_each_job",
                "message":"A conta e o limite de uso serão conferidos antes de cada tarefa."
            }});
        }
        if let Some(binding) = binding
            && let Ok(accounts) = resolver::accounts(&hub.store)
            && let Some(account) = accounts.iter().find(|account| account.id == binding.account_id)
            && account.backend == "codex"
            && account.billing == "subscription"
        {
            return json!({"status":"unavailable", "unavailable":{
                "functionId":id, "reason":"codex_subscription_unavailable", "revision":document.revision,
                "message":"A assinatura Codex ainda não pode ser usada pelo Bot de fundo com a proteção necessária. Escolha outra conta para esta função."
            }});
        }
    }
    let mut context = TrustedContext::administration(caller);
    context.revision = revision;
    match resolver::resolve_function(&hub.store, id, &context) {
        Ok(resolved) => json!({"status":"available", "resolved":resolved}),
        Err(unavailable) => {
            let message = (id == "memory_consolidation").then(|| memory_message(hub, unavailable.reason)).flatten();
            let mut value = json!({"status":"unavailable", "unavailable":unavailable});
            if let Some(message) = message {
                value["unavailable"]["message"] = message.into();
            }
            value
        }
    }
}

/// MEX-1355: o mesmo texto que o Hub usa para "Organizar memória", no campo
/// `message` que a resolução já usa em outros motivos.
fn memory_message(hub: &Hub, reason: resolver::Reason) -> Option<String> {
    use crate::fork::background_notice::{Problem, memory_selection_text};
    let problem = match reason {
        resolver::Reason::AccountDisconnected => {
            let document = config::load(&hub.store).ok()?;
            let account_id = match document.functions.get(&FunctionId::MemoryConsolidation)? {
                config::Selection::Custom { binding } => Some(binding.account_id.clone()),
                config::Selection::Automatic { account_id, .. } => account_id.clone(),
                config::Selection::ManagedInVoice => None,
            }?;
            let label = resolver::accounts(&hub.store).ok()?.into_iter().find(|a| a.id == account_id)?.label;
            Problem::AccountDisconnected { label }
        }
        resolver::Reason::DefaultModelUnknown | resolver::Reason::ModelMissing => Problem::Model,
        _ => return None,
    };
    memory_selection_text(&problem)
}
fn response(hub: &Hub, caller: &Caller) -> Result<Value> {
    // Repetir a inicialização adiada quando o cadastro voltar a ser legível.
    let previous = config::load(&hub.store).ok().map(|document| document.revision);
    config::initialize(&hub.store)?;
    let document = hub.store.kv_read(config::SLOT)?.map(|_| config::load(&hub.store)).transpose()?;
    let revision = document.as_ref().map(|document| document.revision);
    if previous != revision
        && let Some(revision) = revision
    {
        let _ = hub.events.send(json!({"type":"functionModelsChanged", "revision":revision}));
    }
    let (accounts, accounts_issue) = match resolver::accounts(&hub.store) {
        Ok(accounts) => (accounts, None),
        Err(_) => (Vec::new(), Some(resolver::Reason::AccountsUnavailable)),
    };
    let functions = FUNCTIONS
        .into_iter()
        .map(|id| -> Result<Value> {
            let name = serde_json::to_value(id)?;
            let function_id = name.as_str().ok_or_else(|| anyhow!("invalid function ID"))?;
            let resolved = match accounts_issue {
                Some(reason) => json!({"status":"unavailable", "unavailable":resolver::FunctionUnavailable {
                    function_id: function_id.into(), reason, revision,
                }}),
                None => resolution(hub, function_id, caller, revision),
            };
            let consumer = if matches!(id, FunctionId::MemoryConsolidation | FunctionId::BotNaming) {
                "background_worker"
            } else {
                "awaiting_implementation"
            };
            Ok(json!({"definition":id.definition(), "selection":document.as_ref().map(|doc| &doc.functions[&id]),
            "resolution":resolved, "consumer":consumer}))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"document":document,"accounts":accounts,"accountsIssue":accounts_issue,"functions":functions,
        "execution":"partial", "approvalPolicy":"managed_by_rules_starts_observing"}))
}

pub async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    // Nenhum parâmetro do payload concede autoria. D62: Local ainda não prova pessoa.
    if !matches!(caller, Caller::Local) {
        return Err(Forbidden.into());
    }
    if method == "functionModels" {
        let _: Read = parse(body)?;
        let _guard = hub.store.team_lock.lock().await;
        return response(hub, caller);
    }
    if method == "checkFunctionModel" {
        let request: Check = parse(body)?;
        return Ok(resolution(hub, &request.function_id, caller, None));
    }
    // Serializa também evento/resposta, além do CAS SQLite entre stores/processos.
    let _guard = hub.store.team_lock.lock().await;
    let document = match method {
        "saveFunctionModel" => {
            let r: Save = parse(body)?;
            config::save(&hub.store, r.expected_revision, Some(r.function_id), Some(r.selection))?
        }
        "restoreFunctionModel" => {
            let r: Restore = parse(body)?;
            config::save(&hub.store, r.expected_revision, Some(r.function_id), None)?
        }
        "restoreFunctionModels" => {
            let r: RestoreAll = parse(body)?;
            config::save(&hub.store, r.expected_revision, None, None)?
        }
        _ => return Err(anyhow!("unknown method")),
    };
    let _ = hub.events.send(json!({"type":"functionModelsChanged", "revision":document.revision}));
    // A gravação confirmada não depende de uma leitura subsequente do cadastro.
    Ok(json!({"document":document, "execution":"partial"}))
}
