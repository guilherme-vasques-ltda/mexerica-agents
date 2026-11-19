//! D541/PR1 e D543: somente negociação negativa, sem executor ou armazenamento de navegador.
//! Escopo vem de Autoridade/Sessões/ToolContext existentes; argumentos nunca concedem acesso.
pub(crate) mod bridge;
mod cdp;
mod nav;
mod net_proxy;
mod network;
mod preference;
#[cfg(target_os = "linux")]
mod proxy_socket;
pub(crate) mod stream;
mod supervisor;
pub use preference::BrowserPreference;
pub(crate) use preference::check_save;

use crate::{
    LockExt,
    api::devices::Caller,
    fork::{
        autoridade::Autoridade,
        sessoes::{ExecutionRef, bindings},
        spaces::tool_context::ToolContext,
    },
    hub::Hub,
    store::Store,
};
use anyhow::{Result, anyhow, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) const METHOD: &str = "donnaBrowserStatus";
const TOOL: &str = "browser_status";

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Target {
    Desktop,
    Host,
}
impl Target {
    fn name(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Host => "host",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Negotiation {
    version: Option<u32>,
    target: Option<Target>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
// O dispatch já conferiu hubId em spaces::guard e o removeu antes deste decoder.
struct Read {
    space_id: String,
    bot_id: String,
    version: Option<u32>,
    target: Option<Target>,
}

pub(crate) fn definition() -> Value {
    json!({
        "name":TOOL,
        "description":"Consulta apenas a indisponibilidade do navegador integrado e a preferência canônica do dono no Space original. Não navega, lista perfis, lê cookies ou usa senhas. Preferência não libera o PC; CUA e a liberação D536 seguem seu fluxo existente. Não troque de navegador automaticamente quando o integrado estiver indisponível.",
        "inputSchema":{"type":"object","additionalProperties":false,"properties":{
            "version":{"type":"integer","enum":[2]},
            "target":{"type":"string","enum":["desktop","host"]}
        },"required":["version","target"]},
        "annotations":{"readOnlyHint":true}
    })
}

fn unavailable(hub: &Hub, bot: &str, space: &str, preference: BrowserPreference, request: &Negotiation) -> Value {
    let compatible = request.version == Some(2) && request.target.is_some();
    json!({
        "version":2,
        "browserPreference":preference,
        "scope":{"hubId":hub.host_id,"spaceId":space,"botId":bot},
        "target":request.target.map(Target::name),
        "status":"unavailable",
        "reason":if compatible { "backend_absent" } else { "client_unsupported" },
        "profiles":{"status":"unavailable"},
        "capabilities":{
            "persistentProfile":false,"ephemeralProfile":false,"observe":false,
            "control":false,"files":false,"privateLogin":false
        },
        "message":"Navegador integrado indisponível. Não há backend nem acesso a perfis, cookies ou senhas. Preferência não autoriza o PC. Não troque de navegador, tente outro Space ou repita automaticamente."
    })
}

/// Consulta do app no transporte atual. Não cria Authority, concessão, perfil ou campo de cache.
pub(crate) fn status(hub: &Hub, caller: &Caller, body: Value) -> Result<Value> {
    let original = Autoridade::capturar(hub, caller)?;
    let request: Read =
        serde_json::from_value(body).map_err(|_| anyhow!("invalid_field: consulta de navegador inválida"))?;
    original.with_db(hub, None, |db| {
        crate::fork::spaces::require_space(db, &request.space_id)?;
        let row = Store::bot_on(db, &request.bot_id)?.ok_or_else(|| anyhow!("browser_scope_unavailable"))?;
        ensure!(
            !row.deleted && !row.config.is_group() && row.config.space_id == request.space_id,
            "browser_scope_unavailable"
        );
        Ok(unavailable(
            hub,
            &row.config.id,
            &row.config.space_id,
            row.config.browser_preference,
            &Negotiation { version: request.version, target: request.target },
        ))
    })
}

/// O servidor memory já alcança todos os Spaces; somente metadado, sem ferramenta positiva.
/// Não recebe executor, cofre, URL, caminho de perfil ou cliente de rede.
pub(crate) fn call(
    hub: &Hub,
    bot: &str,
    name: &str,
    args: &Value,
    context: Option<&ToolContext>,
    execution: Option<&ExecutionRef>,
) -> Result<Value> {
    ensure!(name == TOOL, "browser_unavailable: operação de navegador indisponível");
    let request: Negotiation =
        serde_json::from_value(args.clone()).map_err(|_| anyhow!("invalid_field: consulta de navegador inválida"))?;
    let execution = execution.ok_or_else(|| anyhow!("browser_context_required: origem original ausente"))?;
    let session = verify(hub, bot, execution)?;
    if let Some(context) = context {
        context.check(hub)?;
        ensure!(context.cfg.id == bot && context.turn.execution.as_ref() == Some(execution), "browser_context_changed");
    }
    let (cfg, generation) = {
        let db = hub.store.db.locked();
        bindings::resolve(&db, execution.acp_binding().ok_or_else(|| anyhow!("browser_context_required"))?, bot)?;
        let row = Store::bot_on(&db, bot)?.ok_or_else(|| anyhow!("browser_scope_unavailable"))?;
        ensure!(!row.deleted && !row.config.is_group(), "browser_scope_unavailable");
        crate::fork::spaces::require_space(&db, &row.config.space_id)?;
        (row.config, crate::fork::spaces::config_generation_on(&db, bot)?)
    };
    // Conversas adicionais só passam pelo roteador que conserva o ToolContext original.
    ensure!(session.chat_id == bot || context.is_some(), "browser_context_required");
    if let Some(context) = context {
        ensure!(context.cfg.space_id == cfg.space_id, "browser_context_changed");
    }
    let answer = unavailable(hub, bot, &cfg.space_id, cfg.browser_preference, &request);
    #[cfg(test)]
    crate::fork::sessoes::call_tests::before_effect(&hub.store, "browser_response");
    verify(hub, bot, execution)?;
    if let Some(context) = context {
        context.check(hub)?;
    }
    ensure!(
        crate::fork::spaces::config_generation_on(&hub.store.db.locked(), bot)? == generation,
        "browser_context_changed"
    );
    Ok(answer)
}

fn verify(hub: &Hub, bot: &str, execution: &ExecutionRef) -> Result<crate::fork::sessoes::store::SessionRef> {
    let session = bindings::recheck(hub, execution)?;
    ensure!(hub.runtime(bot).memory_turn_token.as_deref() == Some(execution.turn_token()), "browser_context_changed");
    Ok(session)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod preference_tests;
