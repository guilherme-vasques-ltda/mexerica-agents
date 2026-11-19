//! Runtime remoto usa o mesmo cofre, leases e observação do gateway stdio.
//! DONNA Bot, 2026-11-04: prazo privado do Bearer real na última entrega Hub,
//! depois da guarda completa; sem mudar autoridade, cache ou contrato público.
use super::runtime;
use crate::fork::conectores::{oauth_binding, oauth_runtime::RuntimeIntent};
use crate::{LockExt, hub::Hub};
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) async fn call(
    hub: &Arc<Hub>,
    action: &str,
    body: &Value,
    id: &str,
    connector: &str,
    bridge: Option<&str>,
    namespace: &str,
) -> Result<Value> {
    let method = body["method"].as_str().ok_or_else(|| anyhow!("Método MCP obrigatório"))?;
    let intent = if action == "donnaGuardRuntime" {
        RuntimeIntent::capture(hub, id, connector, method)?
    } else {
        RuntimeIntent::capture_observation(hub, id, connector, body)?
    };
    let context = intent.context().cloned();
    let bridge = bridge.ok_or_else(|| anyhow!("Ponte remota obrigatória"))?;
    if let Some(context) = &context {
        for (key, expected) in [("botId", Some(context.bot.as_str())), ("runId", context.run.as_deref())] {
            if let Some(value) = body.get(key)
                && value.as_str() != expected
            {
                bail!("Vínculo remoto incorreto");
            }
        }
    }
    // Captura03b antes do primeiro await, sobre a mesma execução do RuntimeIntent.
    crate::fork::spaces::connector_context::check(hub, id, body, intent.execution())?;
    if action == "donnaGuardRuntime" {
        if context.is_none()
            && !matches!(
                method,
                "initialize"
                    | "ping"
                    | "tools/list"
                    | "resources/list"
                    | "resources/templates/list"
                    | "prompts/list"
                    | "notifications/initialized"
            )
        {
            bail!("Runtime de setup não admite este método");
        }
        #[cfg(test)]
        crate::market::oauth::upstream02_tests::runtime_window(
            hub,
            crate::market::oauth::upstream02_tests::RuntimeWindow::BeforeUnlock,
        )
        .await;
        crate::market::vault::unlock(hub.clone()).await?;
        crate::fork::spaces::connector_context::check(hub, id, body, intent.execution())?;
        let saved = intent.snapshot(hub)?;
        let fingerprint = runtime::remote_fingerprint(&saved);
        if body["refreshLease"] == true && !runtime::remote_matches(namespace, connector, id, bridge, &fingerprint) {
            bail!("Lease remota perdida ou configuração alterada");
        }
        // Registrar também o token anterior ao refresh: resultados em voo podem contê-lo.
        let mut secrets = Vec::new();
        for value in saved.headers.values() {
            runtime::remote_secret(value, &mut secrets);
        }
        if let Some(oauth) = &saved.oauth {
            runtime::remote_secret(oauth.client_secret.as_deref().unwrap_or_default(), &mut secrets);
            runtime::remote_secret(&oauth.access_token, &mut secrets);
            runtime::remote_secret(oauth.refresh_token.as_deref().unwrap_or_default(), &mut secrets);
        }
        #[cfg(test)]
        crate::market::oauth::upstream02_tests::runtime_window(
            hub,
            crate::market::oauth::upstream02_tests::RuntimeWindow::BeforeTargetCapture,
        )
        .await;
        let target =
            crate::market::oauth::target(hub, &saved, &intent, body["stale"].as_bool().unwrap_or(false)).await?;
        #[cfg(test)]
        crate::market::oauth::upstream02_tests::runtime_window(
            hub,
            crate::market::oauth::upstream02_tests::RuntimeWindow::AfterTargetResolved,
        )
        .await;
        let target_payload = target.payload();
        if let Some(url) = target_payload["url"].as_str() {
            let url = reqwest::Url::parse(url).map_err(|_| anyhow!("URL remota inválida"))?;
            if let Some(password) = url.password() {
                runtime::remote_secret(password, &mut secrets);
            }
            for (_, value) in url.query_pairs() {
                runtime::remote_secret(&value, &mut secrets);
            }
        }
        // Cabeçalhos configurados são material privado efetivo; não classificar por nome/dígitos.
        for value in
            target_payload["headers"].as_object().into_iter().flat_map(|h| h.values()).filter_map(Value::as_str)
        {
            runtime::remote_secret(value, &mut secrets);
        }
        #[cfg(test)]
        crate::fork::sessoes::call_tests::before_effect(&hub.store, "gateway_credentials");
        crate::fork::spaces::connector_context::check(hub, id, body, intent.execution())?;
        let current = intent.current(hub, &saved)?;
        // Rotação de tokens é admitida; troca do alvo/header não herda o contexto em voo.
        if runtime::remote_fingerprint(&current) != fingerprint {
            bail!("Configuração remota mudou durante a resolução");
        }
        if let Some(oauth) = &current.oauth {
            runtime::remote_secret(oauth.client_secret.as_deref().unwrap_or_default(), &mut secrets);
            runtime::remote_secret(&oauth.access_token, &mut secrets);
            runtime::remote_secret(oauth.refresh_token.as_deref().unwrap_or_default(), &mut secrets);
        }
        runtime::remember(namespace, connector, id, bridge, &secrets);
        if runtime::snapshot(namespace, connector, id, Some(bridge)).is_none() {
            bail!("Cache transitório remoto indisponível");
        }
        runtime::bind_remote(namespace, connector, id, bridge, &fingerprint);
        let response = oauth_binding::check_runtime(hub, &current, &intent, || {
            Ok(
                json!({"url":target_payload["url"],"headers":target_payload["headers"],"bridgeId":bridge,"setup":context.is_none()}),
            )
        })?;
        #[cfg(test)]
        crate::market::oauth::upstream02_tests::runtime_window(
            hub,
            crate::market::oauth::upstream02_tests::RuntimeWindow::AfterDeliveryGuard,
        )
        .await;
        // A guarda inteira pode esperar locks APÓS sua closure. Validar o target
        // real por último, nunca o prazo de outro par rotativo em current.
        crate::fork::spaces::connector_context::check(hub, id, body, intent.execution())?;
        target.validate_delivery()?;
        if let Some(context) = &context {
            let _boundary = hub.store.memory_boundary.locked();
            intent.validate_live(hub)?;
            intent.validate_db(&hub.store.db.locked())?;
            match crate::fork::rules::mcp::check(&hub.store, &context.bot, &context.connector, method, &body["params"])
            {
                Ok(false) => {}
                Ok(true) => return Ok(json!({"ruleDenied":true})),
                Err(error) => {
                    tracing::error!(%error, "política MCP indisponível; chamada recusada");
                    return Ok(json!({"ruleDenied":true}));
                }
            }
        }
        return Ok(response);
    }
    if intent.late_notification() {
        let secrets = runtime::snapshot(namespace, connector, id, Some(bridge));
        return super::observe(hub, &intent, body, secrets.as_deref().map(Vec::as_slice), "http");
    }
    let saved = intent.snapshot(hub)?;
    let fingerprint = runtime::remote_fingerprint(&saved);
    if !runtime::remote_matches(namespace, connector, id, bridge, &fingerprint) {
        bail!("Retorno de configuração remota antiga retido");
    }
    let secrets = runtime::snapshot(namespace, connector, id, Some(bridge));
    let result = super::observe(hub, &intent, body, secrets.as_deref().map(Vec::as_slice), "http")?;
    let current = intent.current(hub, &saved)?;
    if runtime::remote_fingerprint(&current) != fingerprint {
        bail!("Configuração remota mudou antes da entrega");
    }
    crate::fork::spaces::connector_context::check(hub, id, body, intent.execution())?;
    oauth_binding::check_runtime(hub, &current, &intent, || Ok(result))
}
