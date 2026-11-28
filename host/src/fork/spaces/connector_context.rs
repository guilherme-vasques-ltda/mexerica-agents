//! Costura03b: a definição fixa a geração; cada pedido conserva a captura03a,
//! composta sobre o `ExecutionRef` emitido pela Sessões3 (sem identidade paralela).
use super::tool_context::ToolContext;
use crate::{LockExt, fork::sessoes::ExecutionRef, hub::Hub, store::BotConfig};
use anyhow::{Result, anyhow, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

#[derive(Serialize, Deserialize)]
struct Definition {
    bot: String,
    space: String,
    chat: String,
    process: String,
    stamp: String,
    token: String,
}
#[derive(Clone)]
struct Request {
    context: Option<ToolContext>,
    started: Instant,
    method: String,
    bootstrap: crate::hub::Runtime,
    sequence: u64,
}
#[derive(Default)]
struct BridgeRequests {
    last: u64,
    requests: HashMap<String, Request>,
}
static REQUESTS: LazyLock<Mutex<HashMap<String, BridgeRequests>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
fn definition_key(id: &str) -> String {
    format!("donna.tools.connector.{id}")
}
fn request_key(hub: &Hub, id: &str, bridge: &str) -> Result<String> {
    Ok(format!("{}:{id}:{bridge}:", hub.store.guard_runtime_namespace()?))
}

pub(crate) fn bind(hub: &Hub, id: &str, cfg: &BotConfig, token: &str) -> Result<()> {
    let saved = Definition {
        bot: cfg.id.clone(),
        space: cfg.space_id.clone(),
        chat: cfg.session_owner().into(),
        process: super::tool_context::process().into(),
        stamp: super::execution_stamp(&hub.store, cfg)?,
        token: token.into(),
    };
    hub.store.kv_set(&definition_key(id), &serde_json::to_string(&saved)?)
}
fn load_definition(hub: &Hub, id: &str) -> Result<Option<Definition>> {
    hub.store.kv_read(&definition_key(id))?.map(|s| serde_json::from_str(&s).map_err(Into::into)).transpose()
}
fn check_definition(hub: &Hub, saved: &Definition) -> Result<()> {
    ensure!(saved.process == super::tool_context::process(), "connector_process_changed");
    let original = hub.store.bot(&saved.bot)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("bot_not_found"))?.config;
    let cfg = super::execution_config(&hub.store, &original, &saved.chat)?;
    ensure!(super::execution_stamp(&hub.store, &cfg)? == saved.stamp, "connector_binding_changed");
    Ok(())
}

/// Só handshake/descoberta podem anteceder a ativação ACP; nunca leitura/efeito.
fn discovery(method: &str) -> bool {
    matches!(
        method,
        "initialize"
            | "ping"
            | "tools/list"
            | "resources/list"
            | "resources/templates/list"
            | "prompts/list"
            | "notifications/initialized"
            | "notifications/cancelled"
    )
}

/// `execution` é o `ExecutionRef` da Sessões3 capturado pelo `RuntimeIntent` desta chamada;
/// só a primeira chamada do pedido o usa, as seguintes revalidam a captura original.
pub(crate) fn check(hub: &Hub, id: &str, body: &Value, execution: Option<&ExecutionRef>) -> Result<()> {
    let Some(saved) = load_definition(hub, id)? else {
        if let Some(context) = hub.store.guard_context(id)? {
            ensure!(
                context.lane == context.bot
                    || hub.store.bot(&context.lane)?.is_some_and(|b| !b.deleted && b.config.is_group()),
                "tool_binding_required"
            );
        }
        return Ok(());
    };
    check_definition(hub, &saved)?;
    // Aquisição inicial stdio não encaminha ferramenta; a geração já foi conferida.
    if body["method"].is_null() && body["validateOnly"] != true {
        return Ok(());
    }
    let bridge = body["bridgeId"].as_str().ok_or_else(|| anyhow!("connector_bridge_required"))?;
    let request = body["requestId"]
        .as_str()
        .filter(|s| uuid::Uuid::parse_str(s).is_ok())
        .ok_or_else(|| anyhow!("connector_request_required"))?;
    let sequence =
        body["requestSeq"].as_u64().filter(|s| *s > 0).ok_or_else(|| anyhow!("connector_sequence_required"))?;
    let key = request_key(hub, id, bridge)?;
    let captured = REQUESTS.locked().get(&key).and_then(|b| b.requests.get(request)).cloned();
    let captured = if let Some(captured) = captured {
        captured
    } else {
        ensure!(body["beginRequest"] == true, "connector_request_expired");
        let method = body["method"].as_str().ok_or_else(|| anyhow!("tool_name_required"))?;
        let captured = ToolContext::capture_in_execution(
            hub,
            &json!({"botId":saved.bot,"spaceId":saved.space,"toolBinding":saved.token}),
            &saved.chat,
            execution,
        );
        let context = match captured {
            Ok(context) => Some(context),
            Err(_) if discovery(method) => None,
            Err(error) => return Err(error),
        };
        let captured = Request {
            context,
            started: Instant::now(),
            method: method.into(),
            bootstrap: hub.runtime(&saved.bot),
            sequence,
        };
        let mut requests = REQUESTS.locked();
        ensure!(requests.contains_key(&key) || requests.len() < 1024, "connector_bridge_limit");
        let bridge = requests.entry(key).or_default();
        ensure!(
            sequence == bridge.last.checked_add(1).ok_or_else(|| anyhow!("connector_sequence_limit"))?,
            "connector_request_expired"
        );
        bridge.requests.retain(|_, r| r.started.elapsed() < Duration::from_secs(90));
        ensure!(bridge.requests.len() < 1024, "connector_request_limit");
        bridge.last = sequence;
        bridge.requests.insert(request.into(), captured.clone());
        captured
    };
    ensure!(captured.sequence == sequence, "connector_request_sequence_changed");
    ensure!(captured.started.elapsed() < Duration::from_secs(90), "connector_request_expired");
    let method = body["method"].as_str().unwrap_or_default();
    ensure!(
        method == captured.method
            || matches!(method, "notification" | "server-request" | "invalid" | "unmatched" | "ping"),
        "connector_request_method_changed"
    );
    if let Some(context) = &captured.context {
        context.check(hub)?;
    } else {
        let current = hub.runtime(&saved.bot);
        let original = &captured.bootstrap;
        ensure!(
            current.lane == original.lane
                && current.session == original.session
                && current.message == original.message
                && current.started_at == original.started_at
                && current.memory_turn_token == original.memory_turn_token,
            "connector_bootstrap_changed"
        );
    }
    Ok(())
}

pub(crate) fn release(hub: &Hub, id: &str, bridge: Option<&str>) -> Result<()> {
    let prefix = format!("{}:{id}:", hub.store.guard_runtime_namespace()?);
    let prefix = bridge.map_or(prefix.clone(), |b| format!("{prefix}{b}:"));
    REQUESTS.locked().retain(|key, _| !key.starts_with(&prefix));
    Ok(())
}

/// Contextos antigos não recebem o novo token: nova definição usa novo ID Guard.
#[allow(clippy::too_many_arguments)]
pub(crate) fn definition(
    hub: &Hub,
    cfg: &BotConfig,
    token: Option<&str>,
    connector: &crate::market::Connector,
    exe: &std::path::Path,
    port: u16,
    bot: &str,
    lane: &str,
    run: Option<&str>,
    binding: &str,
) -> Result<Option<Value>> {
    if cfg.context.is_some()
        && let Some(old) = hub.store.guard_context_for(bot, &connector.id, lane, run, Some(binding))?
    {
        hub.store.guard_context_remove(&old.id)?;
    }
    let result = crate::guard_hub::mcp::definition(&hub.store, connector, exe, port, bot, lane, run, binding)?;
    if cfg.context.is_some()
        && let Some(definition) = &result
    {
        let args = definition["args"].as_array().ok_or_else(|| anyhow!("invalid_mcp_definition"))?;
        let id = args
            .windows(2)
            .find(|pair| pair[0] == "--context")
            .and_then(|pair| pair[1].as_str())
            .ok_or_else(|| anyhow!("connector_context_required"))?;
        bind(hub, id, cfg, token.ok_or_else(|| anyhow!("tool_binding_required"))?)?;
    }
    Ok(result)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        api::devices::Caller,
        fork::spaces::tool_context::tests::Fixture,
        guard_hub::mcp::{self, contexto::Context},
    };

    pub(crate) fn fixture() -> (Fixture, Context, Value) {
        let f = Fixture::with_memory_profile(|store, cfg| {
            cfg.connectors = vec!["fixture".into()];
            let connector = serde_json::from_value(json!({"id":"fixture","name":"Fixture","command":"fixture-unused"}))
                .expect("fixture03b válida");
            crate::market::save_connectors(store, &[connector]).expect("fixture03b válida");
        });
        // Mesmo vínculo Sessões3 emitido pela fixture03a; nenhum binding paralelo de teste.
        let context = Context::issue_bound(
            &f.hub.store,
            &f.cfg.id,
            "fixture",
            f.cfg.session_owner(),
            None,
            f.body["sessionBinding"].as_str(),
        )
        .expect("fixture03b válida");
        bind(&f.hub, &context.id, &f.cfg, "vinculo-A").expect("fixture03b válida");
        let body = json!({"context":context.id,"connector":"fixture","method":"tools/call", "validateOnly":true,
            "bridgeId":uuid::Uuid::new_v4().to_string(),"requestId":uuid::Uuid::new_v4().to_string(),"requestSeq":1,"beginRequest":true});
        let slot = super::super::session_slot(&f.cfg).expect("fixture03b válida");
        f.hub.store.kv_set(&crate::store::run_session_key(&f.cfg.id, &slot), "sessao-A").expect("fixture03b válida");
        (f, context, body)
    }

    #[tokio::test]
    async fn adicional_lista_conector_e_cria_cartao_somente_na_origem() {
        let (f, _, body) = fixture();
        assert_eq!(
            mcp::call(&f.hub, &Caller::Local, "donnaGuardRuntime", body).await.expect("fixture03b válida")["valid"],
            true
        );
        let mut request = f.body.clone();
        request["name"] = "request_connection".into();
        request["arguments"] = json!({"connectorId":"fixture","reason":"Fixture sem credencial"});
        let result = super::super::tool_context::route(&f.hub, "connectorCall", &request)
            .await
            .expect("fixture03b válida")
            .expect("fixture03b válida");
        let entry =
            f.hub.store.entry(result["requestId"].as_str().expect("fixture03b válida")).expect("fixture03b válida");
        assert_eq!(entry.bot_id, f.cfg.session_owner());
        assert_eq!(entry.turn, 1);
        assert!(
            crate::market::requests::finish_requires_vault(&f.hub, &json!({"entryId":entry.id}))
                .expect("fixture03b válida")
        );
        assert_eq!(
            f.hub
                .store
                .history(&f.cfg.id, i64::MAX, 30)
                .expect("fixture03b válida")
                .into_iter()
                .map(|e| e.id)
                .collect::<Vec<_>>(),
            Vec::<String>::new(),
            "nenhuma entrada na principal"
        );
    }

    #[tokio::test]
    async fn pedido_nao_recaptura_turno_ou_permissao_apos_espera() {
        for permission in [false, true] {
            let (f, _, mut body) = fixture();
            mcp::call(&f.hub, &Caller::Local, "donnaGuardRuntime", body.clone()).await.expect("fixture03b válida");
            body["beginRequest"] = false.into();
            if permission {
                let mut commands = f.hub.test_bot_commands(&f.cfg.id);
                assert_eq!(
                    f.hub.store.bot(&f.cfg.id).expect("perfil fixture").expect("bot fixture").config.permission,
                    crate::store::Permission::Auto,
                    "pré-condição permissiva"
                );
                f.hub
                    .update_bot_by_user(&json!({"id":f.cfg.id,"permission":"ask","permissionChosenByUser":true}))
                    .expect("fixture03b válida");
                assert_eq!(
                    f.hub.store.bot(&f.cfg.id).expect("perfil fixture").expect("bot fixture").config.permission,
                    crate::store::Permission::Ask,
                    "revogação humana persistida antes de liberar a espera"
                );
                assert!(
                    matches!(commands.try_recv(), Ok(crate::agent::bot::Cmd::Reconfigure(cfg)) if cfg.id == f.cfg.id && cfg.permission == crate::store::Permission::Ask),
                    "Reconfigure Ask do mesmo bot recebido antes de liberar a espera"
                );
            } else {
                f.hub.set_runtime(&f.cfg.id, |r| r.memory_turn_token = Some("turno-B".into()));
            }
            assert!(mcp::call(&f.hub, &Caller::Local, "donnaGuardRuntime", body).await.is_err());
            assert_eq!(
                f.hub
                    .store
                    .history(&f.cfg.id, i64::MAX, 30)
                    .expect("fixture03b válida")
                    .into_iter()
                    .map(|e| e.id)
                    .collect::<Vec<_>>(),
                Vec::<String>::new(),
                "nenhuma entrada na principal"
            );
        }
    }

    #[tokio::test]
    async fn cartao_antigo_recusa_antes_do_cofre_sem_retornar_a_principal() {
        for change in ["permission", "session", "turn"] {
            let (f, _, _) = fixture();
            let mut request = f.body.clone();
            request["name"] = "request_secret".into();
            request["arguments"] =
                json!({"connectorId":"fixture","field":"API_KEY","location":"env","reason":"Fixture"});
            let result = super::super::tool_context::route(&f.hub, "connectorCall", &request)
                .await
                .expect("fixture03b válida")
                .expect("fixture03b válida");
            let id = result["requestId"].as_str().expect("fixture03b válida");
            match change {
                "permission" => {
                    let mut commands = f.hub.test_bot_commands(&f.cfg.id);
                    assert_eq!(
                        f.hub.store.bot(&f.cfg.id).expect("perfil fixture").expect("bot fixture").config.permission,
                        crate::store::Permission::Auto,
                        "pré-condição permissiva"
                    );
                    f.hub
                        .update_bot_by_user(&json!({"id":f.cfg.id,"permission":"ask","permissionChosenByUser":true}))
                        .expect("fixture03b válida");
                    assert_eq!(
                        f.hub.store.bot(&f.cfg.id).expect("perfil fixture").expect("bot fixture").config.permission,
                        crate::store::Permission::Ask,
                        "revogação humana persistida antes de liberar a espera"
                    );
                    assert!(
                        matches!(commands.try_recv(), Ok(crate::agent::bot::Cmd::Reconfigure(cfg)) if cfg.id == f.cfg.id && cfg.permission == crate::store::Permission::Ask),
                        "Reconfigure Ask do mesmo bot recebido antes de liberar a espera"
                    );
                }
                "session" => {
                    let slot = super::super::session_slot(&f.cfg).expect("fixture03b válida");
                    f.hub
                        .store
                        .kv_set(&crate::store::run_session_key(&f.cfg.id, &slot), "sessao-B")
                        .expect("fixture03b válida");
                }
                _ => {
                    f.hub
                        .add_entry(
                            &crate::store::Lane::main(f.cfg.session_owner()),
                            crate::store::EntryKind::User,
                            2,
                            &json!({"text":"Novo turno"}),
                        )
                        .expect("fixture03b válida");
                }
            }
            let args = json!({"entryId":id,"value":"fixture-unused"});
            assert!(crate::market::requests::finish_requires_vault(&f.hub, &args).is_err());
            assert!(crate::market::requests::finish(&f.hub, &args).await.is_err());
            assert_eq!(
                f.hub.store.entry(id).expect("fixture03b válida").data["connectionRequest"]["status"],
                "pending"
            );
            assert!(f.hub.store.entry(&format!("connection-ack-{id}")).is_none());
            assert_eq!(
                f.hub
                    .store
                    .history(&f.cfg.id, i64::MAX, 30)
                    .expect("fixture03b válida")
                    .into_iter()
                    .map(|e| e.id)
                    .collect::<Vec<_>>(),
                Vec::<String>::new(),
                "nenhuma entrada na principal"
            );
        }
    }
}
