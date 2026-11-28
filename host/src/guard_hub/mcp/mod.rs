//! Gateway stdio em observação. Não emite grants, não usa score nem baixa pesos.
pub mod contexto;
mod entrada;
pub(crate) use entrada::notification_method;
#[cfg(unix)]
pub(crate) use entrada::redigir_json;
pub(crate) use entrada::sem_envelope;
pub(crate) mod http;
mod ponte_remota;
mod remoto;
pub(crate) mod runtime;
pub(crate) mod setup;
pub mod stdio;

use crate::LockExt;
use crate::{
    api::devices::Caller, fork::conectores::oauth_runtime::RuntimeIntent, hub::Hub, market::Connector, store::Store,
};
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
use zeroize::Zeroizing;

fn catalog_local(body: &Value) -> bool {
    body["transport"] == "catalog-local"
}

fn chamada_recusada(
    hub: &Hub,
    context: &contexto::Context,
    conta: Option<&Connector>,
    method: &str,
    params: &Value,
) -> bool {
    match crate::fork::rules::mcp::check(&hub.store, &context.bot, &context.connector, method, params) {
        Ok(false) => {}
        Ok(true) => return true,
        Err(error) => {
            tracing::error!(%error, "política MCP indisponível; chamada recusada");
            return true;
        }
    }
    conta.is_some_and(|conta| {
        crate::market::ferramentas::recusar_chamada(&hub.store, &context.bot, conta, method, params)
    })
}

#[allow(clippy::too_many_arguments)] // Encaixe curto do vínculo no construtor MCP existente.
pub fn definition(
    store: &Store,
    connector: &Connector,
    exe: &Path,
    port: u16,
    bot: &str,
    lane: &str,
    run: Option<&str>,
    binding: &str,
) -> Result<Option<Value>> {
    let Some(mut definition) = connector.acp(exe, port) else { return Ok(None) };
    let context = crate::fork::conectores::oauth_binding::issue_context(store, connector, bot, lane, run, binding)?;
    definition["args"]
        .as_array_mut()
        .ok_or_else(|| anyhow!("Definição MCP inválida"))?
        .extend([json!("--context"), json!(context.id)]);
    Ok(Some(definition))
}

pub async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    if !matches!(caller, Caller::Local | Caller::Ponte { .. }) {
        bail!("Gateway disponível somente na ponte local");
    }
    let id = body["context"].as_str().ok_or_else(|| anyhow!("Contexto obrigatório"))?;
    let connector = body["connector"].as_str().ok_or_else(|| anyhow!("Conector obrigatório"))?;
    let namespace = hub.store.guard_runtime_namespace()?;
    let bridge = body
        .get("bridgeId")
        .map(|value| {
            value
                .as_str()
                .filter(|id| uuid::Uuid::parse_str(id).is_ok())
                .ok_or_else(|| anyhow!("Ponte do gateway inválida"))
        })
        .transpose()?;
    if method == "donnaGuardRelease" {
        // A ponte precisa liberar sua lease mesmo depois de perder o vínculo ou o run.
        runtime::release(&namespace, connector, id, bridge);
        crate::fork::spaces::connector_context::release(hub, id, bridge)?;
        return Ok(json!({"released":true}));
    }
    if !matches!(method, "donnaGuardRuntime" | "donnaGuardObserve") {
        bail!("Método do gateway desconhecido");
    }
    if body["validateOnly"] == true {
        // Guarda03b antes de encaminhar: a execução vem do RuntimeIntent da Sessões3.
        let mcp_method = body["method"].as_str().ok_or_else(|| anyhow!("Método MCP obrigatório"))?;
        let intent = RuntimeIntent::capture(hub, id, connector, mcp_method)?;
        // Marketplace, fatia 2: a conta é lida antes do lock de memória (o cofre pode esperar).
        let conta_final = if catalog_local(&body) || (body["transport"] == "http" && mcp_method == "tools/call") {
            crate::market::vault::unlock(hub.clone()).await?;
            let saved = intent.snapshot(hub)?;
            if catalog_local(&body) {
                let bridge = bridge.ok_or_else(|| anyhow!("Ponte MCP local obrigatória"))?;
                if crate::fork::conectores::catalogo_local::da_conta(&saved).is_none()
                    || !runtime::remote_matches(&namespace, connector, id, bridge, &runtime::remote_fingerprint(&saved))
                {
                    bail!("Lease MCP local indisponível ou alterada");
                }
                crate::market::ferramentas::politica(&saved)?.ok_or_else(|| anyhow!("Ficha MCP local indisponível"))?;
            }
            Some(saved)
        } else {
            None
        };
        let _boundary = hub.store.memory_boundary.locked();
        crate::fork::spaces::connector_context::check(hub, id, &body, intent.execution())?;
        intent.validate_live(hub)?;
        intent.validate_db(&hub.store.db.locked())?;
        if let Some(context) = intent.context()
            && chamada_recusada(hub, context, conta_final.as_ref(), mcp_method, &body["params"])
        {
            return Ok(json!({"valid":false,"ruleDenied":true}));
        }
        return Ok(json!({"valid":true}));
    }
    if body["transport"] == "http" || catalog_local(&body) {
        return remoto::call(hub, method, &body, id, connector, bridge, &namespace).await;
    }
    let intent = if method == "donnaGuardRuntime" {
        RuntimeIntent::capture(hub, id, connector, body["method"].as_str().unwrap_or("initialize"))?
    } else {
        RuntimeIntent::capture_observation(hub, id, connector, &body)?
    };
    let context = intent.context().ok_or_else(|| anyhow!("Contexto de bot obrigatório no stdio"))?;
    for (key, expected) in [("botId", Some(context.bot.as_str())), ("runId", context.run.as_deref())] {
        if let Some(value) = body.get(key)
            && value.as_str() != expected
        {
            bail!("Vínculo do gateway incorreto");
        }
    }
    if method == "donnaGuardRuntime" {
        crate::market::vault::unlock(hub.clone()).await?;
        let connector = crate::market::connectors(&hub.store)?
            .into_iter()
            .find(|c| c.id == context.connector && c.command.is_some() && c.configured())
            .ok_or_else(|| anyhow!("Conector local indisponível"))?;
        if connector.servico.is_some() {
            bail!("Conta do catálogo não roda como comando local");
        }
        let mut env = serde_json::Map::new();
        let mut secrets = Vec::new();
        for (key, value) in &connector.env {
            let resolved = Zeroizing::new(
                crate::market::passwords::resolve(&hub.store, value)
                    .await
                    .map_err(|_| anyhow!("Credencial do conector indisponível"))?,
            );
            runtime::collect(&resolved, &mut secrets);
            env.insert(key.clone(), json!(resolved.as_str()));
        }
        #[cfg(test)]
        crate::fork::sessoes::call_tests::before_effect(&hub.store, "gateway_credentials");
        intent.validate_live(hub)?;
        crate::fork::spaces::connector_context::check(hub, id, &body, intent.execution())?;
        let bridge = bridge.map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_owned);
        let db = hub.store.db.locked();
        intent.validate_db(&db)?;
        runtime::remember(&namespace, &context.connector, id, &bridge, &secrets);
        return Ok(json!({"command":connector.command,"args":connector.args,"env":env,"bridgeId":bridge}));
    }
    let secrets = runtime::snapshot(&namespace, &context.connector, id, bridge);
    observe(hub, &intent, &body, secrets.as_deref().map(Vec::as_slice), "stdio")
}

pub(super) fn observe(
    hub: &Hub,
    intent: &RuntimeIntent,
    body: &Value,
    secrets: Option<&[String]>,
    transport: &str,
) -> Result<Value> {
    if body["method"] == "binding/check" {
        intent.require_execution(hub)?;
        return Ok(json!({}));
    }
    observe_checked(
        Observation::Legacy { intent, transport },
        body,
        secrets,
        || {
            intent.validate_live(hub)?;
            if let Some(context) = intent.context() {
                crate::fork::spaces::connector_context::check(hub, &context.id, body, intent.execution())?;
            }
            intent.validate_db(&hub.store.db.locked()).map(|_| ())
        },
        |event| {
            let _boundary = hub.store.memory_boundary.locked();
            hub.store.guard_record_in_intent(event, intent)
        },
    )
}

#[derive(Clone, Copy)]
pub(crate) enum Observation<'a> {
    Legacy { intent: &'a RuntimeIntent, transport: &'a str },
    Curated(&'a contexto::Context),
}

/// Inspetor e recibo comuns; cada produtor mantém autoridade e gravação transacional próprias.
pub(crate) fn observe_checked(
    origin: Observation<'_>,
    body: &Value,
    secrets: Option<&[String]>,
    validate: impl Fn() -> Result<()>,
    record: impl Fn(&Value) -> Result<()>,
) -> Result<Value> {
    let (context, transport, late) = match origin {
        Observation::Legacy { intent, transport } => (intent.context(), transport, intent.late_notification()),
        Observation::Curated(context) => (Some(context), "http", false),
    };
    let frame = body.get("frame").ok_or_else(|| anyhow!("Mensagem obrigatória"))?;
    let method = body["method"].as_str().unwrap_or("unknown");
    let mut guarded = if let Some(secrets) = secrets {
        entrada::inspect(frame, method, secrets)
    } else {
        // Cache ausente/descartado: não inspeciona com uma lista vazia e libera raw.
        // Só preserva o id de correlação; o corpo nunca entra na resposta nem no evento.
        let retained = if let Some(id) = frame.get("id") {
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,
                "message":"[Conteúdo retido: cache transitório do gateway indisponível]"}})
        } else {
            json!({"method":"invalid"})
        };
        let mut guarded = entrada::inspect(&retained, method, &[]);
        guarded.bytes = frame.to_string().len();
        guarded.coverage = "unsupported";
        guarded.instructions_discarded = method == "initialize" && frame["result"].get("instructions").is_some();
        if let Some(message) = guarded.message.as_mut() {
            message["error"]["data"]["_meta"]["donnabot/guard"]["coverage"] = json!("unsupported");
        }
        guarded
    };
    if late
        || (matches!(transport, "http" | "catalog-local")
            && matches!(method, "notification" | "server-request" | "invalid"))
    {
        // A sessão remota não anuncia canais reversos nem entrega ruído não correlacionado.
        guarded.message = None;
    }
    // Revalida depois do I/O do cofre. Falha ao gravar nunca libera o corpo original.
    validate()?;
    let event = json!({
        "receipt":guarded.receipt, "sourceId":guarded.source_id, "origin":"external", "transport":transport,
        "bot":context.map(|c| &c.bot), "connector":body["connector"], "lane":context.map(|c| &c.lane), "run":context.and_then(|c| c.run.as_deref()),
        "gatewayContext":body["context"], "session":null, "turn":null, "mode":"observe", "method":match method {
        "initialize" | "tools/list" | "tools/call" | "resources/read" | "resources/list" | "resources/templates/list" | "prompts/get" | "prompts/list" | "notification" | "invalid" | "server-request" => method, _ => "other"
    }, "coverage":guarded.coverage,
        "bytes":guarded.bytes, "redactions":guarded.redactions, "wouldWarn":guarded.would_warn,
        "instructionsDiscarded":guarded.instructions_discarded,
        "schemaKeywordsDiscarded":guarded.schema_keywords_discarded, "runtimeAvailable":secrets.is_some(),
        "decision":if secrets.is_some() && guarded.message.is_some() { "sanitized" } else { "retained" }, "version":"mediated-return-2", "model":null,
    });
    record(&event)?;
    if guarded.message.is_some() {
        validate()?;
    }
    Ok(json!({"message":guarded.message}))
}

/// A resposta mediada que o Guard entregaria ao bot, sem gravar evento. Só para testes de quem consome a mediação.
#[cfg(test)]
pub(crate) fn mediar_para_teste(frame: &Value, method: &str, secrets: &[String]) -> Value {
    json!({"message": entrada::inspect(frame, method, secrets).message})
}

#[cfg(test)]
mod tests;

pub use ponte_remota::serve as serve_remote;
