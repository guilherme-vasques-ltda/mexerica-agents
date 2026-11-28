//! Runtime remoto usa o mesmo cofre, leases e observação do gateway stdio.
//! DONNA Bot, 2026-11-04: prazo privado do Bearer real na última entrega Hub,
//! depois da guarda completa; sem mudar autoridade, cache ou contrato público.
use super::runtime;
use crate::fork::conectores::{catalogo_local, oauth_binding, oauth_runtime::RuntimeIntent};
use crate::{LockExt, hub::Hub};
use anyhow::{Result, anyhow, bail, ensure};
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
    let local = super::catalog_local(body);
    let acquire = local && action == "donnaGuardRuntime" && body["method"].is_null();
    let method = if acquire {
        "initialize"
    } else {
        body["method"].as_str().ok_or_else(|| anyhow!("Método MCP obrigatório"))?
    };
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
    if local && intent.late_notification() && runtime::snapshot(namespace, connector, id, Some(bridge)).is_none() {
        bail!("Lease MCP local indisponível");
    }
    let mut local_snapshot = None;
    if local && !intent.late_notification() {
        crate::market::vault::unlock(hub.clone()).await?;
        let saved = intent.snapshot(hub)?;
        let artifact = catalogo_local::da_conta(&saved).ok_or_else(|| anyhow!("Artefato MCP não fixado"))?;
        ensure!(crate::market::ferramentas::politica(&saved)?.is_some(), "Ficha MCP local indisponível");
        ensure!(
            saved.oauth.as_ref().is_none_or(crate::market::oauth::OAuth::signed_in),
            "Login MCP local indisponível"
        );
        let fingerprint = runtime::remote_fingerprint(&saved);
        if acquire {
            oauth_binding::check_runtime(hub, &saved, &intent, || Ok(()))?;
            runtime::remember(namespace, connector, id, bridge, &[]);
            runtime::bind_remote(namespace, connector, id, bridge, &fingerprint);
            return Ok(json!({"artefato":artifact.id(),"bridgeId":bridge}));
        }
        ensure!(
            runtime::remote_matches(namespace, connector, id, bridge, &fingerprint),
            "Lease MCP local indisponível ou alterada"
        );
        local_snapshot = Some((saved, fingerprint));
    }
    // Marketplace, fatia 2: o fornecedor avisou que a lista de ferramentas mudou. A decisão antiga deixa de valer
    // antes de qualquer outra coisa, mesmo que a observação seguinte seja retida: nada passa até uma lista nova.
    if action == "donnaGuardObserve"
        && super::notification_method(&body["frame"]) == Some("notifications/tools/list_changed")
    {
        crate::market::ferramentas::invalidar(&hub.store, connector)?;
        if let Some(context) = &context
            && let Err(error) = hub.send_cmd(&context.bot, crate::agent::bot::Cmd::RefreshTools)
        {
            tracing::warn!(%error, "lista de ferramentas mudou; atualização do bot pendente");
        }
    }
    if action == "donnaGuardRuntime" {
        if let Some((saved, _)) = local_snapshot {
            if !matches!(
                method,
                "initialize"
                    | "ping"
                    | "tools/list"
                    | "tools/call"
                    | "notifications/initialized"
                    | "notifications/cancelled"
            ) {
                return Ok(json!({"ruleDenied":true}));
            }
            if let Some(context) = &context {
                let _boundary = hub.store.memory_boundary.locked();
                intent.validate_live(hub)?;
                intent.validate_db(&hub.store.db.locked())?;
                if super::chamada_recusada(hub, context, Some(&saved), method, &body["params"]) {
                    return Ok(json!({"ruleDenied":true}));
                }
            }
            oauth_binding::check_runtime(hub, &saved, &intent, || Ok(()))?;
            if method == "tools/list" {
                let request = body["requestId"]
                    .as_str()
                    .filter(|id| uuid::Uuid::parse_str(id).is_ok())
                    .ok_or_else(|| anyhow!("Pedido de descoberta local inválido"))?;
                let generation = crate::market::ferramentas::geracao(&hub.store, connector)?;
                if runtime::local_discovery(
                    namespace,
                    connector,
                    id,
                    bridge,
                    request,
                    runtime::LocalDiscovery::Start { generation, paginated: !body["params"]["cursor"].is_null() },
                )?
                .is_none()
                {
                    return Ok(json!({"ruleDenied":true}));
                }
            } else if method == "notifications/cancelled" {
                let request = body["params"]["requestId"]
                    .as_str()
                    .filter(|id| uuid::Uuid::parse_str(id).is_ok())
                    .ok_or_else(|| anyhow!("Cancelamento MCP local inválido"))?;
                runtime::local_discovery(namespace, connector, id, bridge, request, runtime::LocalDiscovery::Cancel)?;
            }
            return Ok(json!({"valid":true}));
        }
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
        ensure!(!catalogo_local::reservado(&saved), "Conta MCP local não usa transporte HTTP");
        if !matches!(method, "initialize" | "ping" | "tools/list" | "tools/call" | "notifications/initialized")
            && crate::market::contas::tipo_da_conta(&saved)?.is_some_and(|tipo| tipo.exige_filtro(&saved.liberacoes))
        {
            return Ok(json!({"ruleDenied": true}));
        }
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
        // Marketplace, fatia 2: a geração da conta é anotada antes de o pedido sair; só uma resposta de geração
        // ainda vigente instala inventário.
        if method == "tools/list" && saved.servico.is_some() {
            crate::market::ferramentas::iniciar_descoberta(&hub.store, bridge, connector)?;
        }
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
            if super::chamada_recusada(hub, context, Some(&saved), method, &body["params"]) {
                return Ok(json!({"ruleDenied":true}));
            }
        }
        return Ok(response);
    }
    if intent.late_notification() {
        let secrets = runtime::snapshot(namespace, connector, id, Some(bridge));
        return super::observe(
            hub,
            &intent,
            body,
            secrets.as_deref().map(Vec::as_slice),
            if local { "catalog-local" } else { "http" },
        );
    }
    let (saved, fingerprint) = if let Some(local) = local_snapshot {
        local
    } else {
        let saved = intent.snapshot(hub)?;
        ensure!(!catalogo_local::reservado(&saved), "Conta MCP local não usa transporte HTTP");
        let fingerprint = runtime::remote_fingerprint(&saved);
        (saved, fingerprint)
    };
    if !runtime::remote_matches(namespace, connector, id, bridge, &fingerprint) {
        bail!("Retorno de configuração remota antiga retido");
    }
    let secrets = runtime::snapshot(namespace, connector, id, Some(bridge));
    let mut result = super::observe(
        hub,
        &intent,
        body,
        secrets.as_deref().map(Vec::as_slice),
        if local { "catalog-local" } else { "http" },
    )?;
    if method == "tools/list" {
        // Marketplace, fatia 2: o digest sai do frame original; o que a conta não pode usar não chega ao bot.
        let (geracao, params) = if local {
            let request = body["requestId"]
                .as_str()
                .filter(|id| body["frame"]["id"] == *id)
                .ok_or_else(|| anyhow!("Resposta de descoberta local inválida"))?;
            let (generation, paginated) =
                runtime::local_discovery(namespace, connector, id, bridge, request, runtime::LocalDiscovery::Finish)?
                    .ok_or_else(|| anyhow!("Resposta de descoberta local sem geração"))?;
            (Some(generation), json!({"cursor":paginated.then_some(true)}))
        } else {
            (crate::market::ferramentas::encerrar_descoberta(bridge, connector), body["params"].clone())
        };
        match &context {
            Some(context) => crate::market::ferramentas::filtrar_lista(
                &hub.store,
                &context.bot,
                &saved,
                &params,
                &body["frame"],
                &mut result,
                geracao,
            )?,
            None => {
                if saved.servico.is_some() {
                    crate::market::ferramentas::registrar_descoberta(
                        &hub.store,
                        &saved,
                        &params,
                        &body["frame"],
                        &result,
                        geracao,
                    )?;
                    if local {
                        ensure!(
                            crate::market::ferramentas::lista_completa(&params, &body["frame"])
                                && geracao == Some(crate::market::ferramentas::geracao(&hub.store, connector)?),
                            "Descoberta MCP local incompleta ou alterada"
                        );
                        let permitidas = crate::market::contas::ferramentas_da_conta(&saved)?
                            .ok_or_else(|| anyhow!("Lista de ferramentas MCP local ausente"))?;
                        let tools = result["message"]["result"]["tools"]
                            .as_array_mut()
                            .ok_or_else(|| anyhow!("Inventário MCP local indisponível"))?;
                        tools.retain(|tool| tool["name"].as_str().is_some_and(|name| permitidas.contains(name)));
                    }
                }
            }
        }
    }
    let current = intent.current(hub, &saved)?;
    if runtime::remote_fingerprint(&current) != fingerprint {
        bail!("Configuração remota mudou antes da entrega");
    }
    crate::fork::spaces::connector_context::check(hub, id, body, intent.execution())?;
    oauth_binding::check_runtime(hub, &current, &intent, || Ok(result))
}
