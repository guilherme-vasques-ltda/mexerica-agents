use super::{
    conectores::{mcp_version, oauth_refresh, oauth_runtime::RuntimeIntent},
    open_tool_actions,
    open_turn::LocalTurn,
};
use crate::{
    LockExt,
    guard_hub::mcp::{
        self, Observation,
        http::{Mediation, PolicyDenied, Session, Target},
    },
    hub::Hub,
    market::{self, Connector, ferramentas},
    store::{Entry, Store},
};
use anyhow::{Context, Result, anyhow, ensure};
use reqwest::{Url, header::HeaderMap};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::time::Instant;
use zeroize::Zeroizing;

const LIST: &str = "list_mcp_tools";
const CALL: &str = "call_mcp_tool";

pub(super) fn catalog() -> Vec<Value> {
    [
        (LIST, "Lista ferramentas permitidas ou ligadas de uma conta MCP vinculada.", json!(["account"])),
        (CALL, "Pede autorização e chama uma ferramenta MCP de uma conta vinculada. Não repita chamadas sem retorno confirmado.", json!(["account", "tool", "arguments"])),
    ]
    .into_iter()
    .map(|(name, description, required)| {
        let mut properties = json!({"account":{"type":"string"}});
        if name == CALL {
            properties["tool"] = json!({"type":"string"});
            properties["arguments"] = json!({"type":"object"});
        }
        json!({"type":"function", "name":name,"description":description,
            "inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
    })
    .collect()
}

pub(super) fn offered(name: &str) -> bool {
    matches!(name, LIST | CALL)
}

#[derive(Debug)]
pub(super) struct EffectUnknown;

impl std::fmt::Display for EffectUnknown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("O efeito MCP pode ter ocorrido, mas o Hub não confirmou o resultado. Não repita automaticamente.")
    }
}

impl std::error::Error for EffectUnknown {}

fn rpc(id: i64, method: &str, params: Value) -> Value {
    let mut frame = json!({"jsonrpc":"2.0","id":id,"method":method});
    frame["params"] = params;
    frame
}

pub(super) async fn call(hub: &Arc<Hub>, turn: &LocalTurn, name: &str, args: &Value) -> Result<Value> {
    run(
        hub,
        turn,
        name,
        args,
        #[cfg(test)]
        None,
    )
    .await
}

async fn run(
    hub: &Arc<Hub>,
    turn: &LocalTurn,
    name: &str,
    args: &Value,
    #[cfg(test)] endpoint: Option<Url>,
) -> Result<Value> {
    let props = args.as_object().context("Argumentos MCP inválidos")?;
    let account = args["account"].as_str().filter(|s| !s.is_empty() && s.len() <= 128).context("Conta MCP inválida")?;
    let params = match name {
        LIST if props.len() == 1 => None,
        CALL if props.len() == 3 && args["arguments"].is_object() => {
            let name =
                args["tool"].as_str().filter(|s| !s.is_empty() && s.len() <= 128).context("Ferramenta MCP inválida")?;
            Some(json!({"name":name,"arguments":args["arguments"]}))
        }
        _ => return Err(anyhow!("Argumentos MCP inválidos")),
    };
    let intent = RuntimeIntent::capture_open(hub, turn, account)?;
    let saved = intent.snapshot(hub)?;
    let mut session = Session::new(OpenMediation {
        hub,
        turn,
        intent,
        saved,
        target: None,
        secrets: Zeroizing::new(Vec::new()),
        generation: None,
        invocation: None,
        delivers_list: params.is_none(),
        #[cfg(test)]
        endpoint,
    })?;
    if let Some(params) = &params {
        session.mediation_mut().policy(params)?;
        ensure!(params.to_string().encode_utf16().count() <= 1600, "Pedido MCP grande demais para autorização");
    }
    let initialized = exchange(
        &mut session,
        rpc(
            1,
            "initialize",
            json!({
                "protocolVersion":mcp_version::PREFERIDA.as_str(),"capabilities":{},
                "clientInfo":{"name":"mexerica-hub","version":"1"}
            }),
        ),
    )
    .await?;
    ensure!(initialized["result"]["capabilities"].get("tools").is_some(), "Servidor sem ferramentas MCP");
    exchange(&mut session, json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}})).await?;
    let listed = exchange(&mut session, rpc(2, "tools/list", json!({}))).await?;
    let Some(params) = params else {
        let mut tools = listed["result"]["tools"].clone();
        for tool in tools.as_array_mut().into_iter().flatten() {
            if let Some(fields) = tool.as_object_mut() {
                fields.remove("digest");
            }
        }
        turn.check_live(hub)?;
        return Ok(json!({"account":account,"origin":"external","transport":"http","tools":tools,
            "provenance":listed["result"]["_meta"]["donnabot/guard"]}));
    };
    let mediation = session.mediation_mut();
    let tool = params["name"].as_str().context("Ferramenta MCP inválida")?;
    let digest = mediation.intent.with_open_current(hub, &mediation.saved, |db, current| {
        let digest = ferramentas::digest_autorizado(db, &turn.lane().chat, current, tool)?;
        ensure!(
            listed["result"]["tools"]
                .as_array()
                .is_some_and(|tools| { tools.iter().any(|t| t["name"] == tool && t["digest"] == digest) }),
            "Ferramenta MCP não descoberta ou alterada"
        );
        Ok(digest)
    })?;
    mediation.policy(&params)?;
    let detail = json!({"account":account,"tool":tool,"arguments":params["arguments"]});
    ensure!(mcp::redigir_json(&detail, &mediation.secrets) == detail, "Pedido contém material privado");
    let card = if crate::agent::engine_permissions::mcp_released(account, tool) {
        None
    } else {
        Some(
            open_tool_actions::approve(hub, turn.lane(), turn.turn(), "Chamar ferramenta MCP", &detail.to_string())
                .await?,
        )
    };
    mediation.invocation = Some(Invocation {
        params: params.clone(),
        digest,
        card,
        deadline: Instant::now() + Duration::from_secs(60),
        sent: false,
    });
    let response = exchange(&mut session, rpc(3, "tools/call", params)).await;
    let mediation = session.mediation_mut();
    let confirmed = response.and_then(|frame| {
        mediation.check_call()?;
        ensure!(frame.get("result").is_some(), "Retorno MCP não confirmado");
        Ok(json!({"origin":"external","transport":"http","result":frame["result"]}))
    });
    if confirmed.is_err() && mediation.invocation.as_ref().is_some_and(|i| i.sent) {
        return Err(EffectUnknown.into());
    }
    confirmed
}

async fn exchange(session: &mut Session<OpenMediation<'_>>, frame: Value) -> Result<Value> {
    session.exchange(frame).await.map(Option::unwrap_or_default).map_err(|_| anyhow!("Sessão MCP indisponível"))
}

struct Invocation {
    params: Value,
    digest: String,
    card: Option<Entry>,
    deadline: Instant,
    sent: bool,
}

struct OpenMediation<'a> {
    hub: &'a Hub,
    turn: &'a LocalTurn,
    intent: RuntimeIntent,
    saved: Connector,
    target: Option<oauth_refresh::Target>,
    secrets: Zeroizing<Vec<String>>,
    generation: Option<i64>,
    invocation: Option<Invocation>,
    delivers_list: bool,
    #[cfg(test)]
    endpoint: Option<Url>,
}

impl OpenMediation<'_> {
    fn secret(&mut self, secret: &str) {
        if !secret.is_empty() && !self.secrets.iter().any(|s| s == secret) {
            self.secrets.push(secret.to_owned());
        }
        if let Some(token) = secret.strip_prefix("Bearer ") {
            self.secret(token);
        }
    }

    fn account_secrets(&mut self, saved: &Connector) {
        for value in saved.headers.values() {
            self.secret(value);
        }
        if let Some(oauth) = &saved.oauth {
            self.secret(&oauth.access_token);
            self.secret(oauth.refresh_token.as_deref().unwrap_or_default());
            self.secret(oauth.client_secret.as_deref().unwrap_or_default());
            self.secret(&oauth.token_endpoint);
        }
    }

    fn policy(&self, params: &Value) -> Result<()> {
        match super::rules::mcp::check(&self.hub.store, &self.turn.lane().chat, &self.saved.id, "tools/call", params) {
            Ok(false) => Ok(()),
            _ => Err(PolicyDenied.into()),
        }
    }

    fn check_invocation(&self, db: &rusqlite::Connection, current: &Connector, invocation: &Invocation) -> Result<()> {
        let tool = invocation.params["name"].as_str().context("Ferramenta MCP inválida")?;
        ensure!(
            ferramentas::digest_autorizado(db, &self.turn.lane().chat, current, tool)? == invocation.digest,
            "Ferramenta MCP alterada"
        );
        if let Some(card) = &invocation.card {
            open_tool_actions::check_card(db, card)?;
        } else {
            ensure!(crate::agent::engine_permissions::mcp_released(&current.id, tool), "Autorização MCP ausente");
        }
        Ok(())
    }

    fn check_call(&self) -> Result<()> {
        let invocation = self.invocation.as_ref().context("Autorização MCP ausente")?;
        ensure!(invocation.sent, "Chamada MCP não admitida");
        self.intent.with_open_preflight(
            self.hub,
            &self.saved,
            || self.policy_boundary(Some(&invocation.params)),
            |db, current| self.check_invocation(db, current, invocation),
        )?;
        self.intent.validate_live(self.hub)
    }

    fn policy_boundary(&self, params: Option<&Value>) -> Result<std::sync::MutexGuard<'_, ()>> {
        let boundary = self.hub.store.memory_boundary.locked();
        if let Some(params) = params {
            self.policy(params)?;
        }
        Ok(boundary)
    }
}

impl Mediation for OpenMediation<'_> {
    fn session_id(&mut self, id: &str) {
        self.secret(id);
    }

    fn private_url(&mut self, url: &Url) {
        self.secret(url.as_str());
        self.secret(&url.origin().ascii_serialization());
        self.secret(url.host_str().unwrap_or_default());
        for (_, value) in url.query_pairs() {
            self.secret(&value);
        }
    }

    async fn target(&mut self, method: &str, stale: bool) -> Result<Target> {
        if method == "tools/call" && self.invocation.as_ref().is_some_and(|i| i.sent) {
            self.check_call()?;
        }
        let current = self.intent.current(self.hub, &self.saved)?;
        self.account_secrets(&current);
        let target = market::oauth::target(self.hub, &current, &self.intent, stale).await?;
        let updated = self.intent.current(self.hub, &self.saved)?;
        self.account_secrets(&updated);
        let payload = target.payload();
        let url = Url::parse(payload["url"].as_str().context("Alvo MCP indisponível")?)?;
        #[cfg(test)]
        let url = self.endpoint.clone().unwrap_or(url);
        self.private_url(&url);
        let mut headers = HeaderMap::new();
        for (name, val) in payload["headers"].as_object().context("Cabeçalhos MCP indisponíveis")? {
            let val = val.as_str().context("Cabeçalho MCP inválido")?;
            self.secret(val);
            headers.insert(reqwest::header::HeaderName::from_bytes(name.as_bytes())?, val.parse()?);
        }
        self.secrets.sort_unstable_by_key(|s| std::cmp::Reverse(s.len()));
        self.intent.with_open_current(self.hub, &self.saved, |_, _| Ok(()))?;
        target.validate_delivery()?;
        self.target = Some(target);
        Ok(Target { url, headers })
    }

    fn before_send(&mut self, frame: &Value, method: &str) -> Result<()> {
        let generation = self.intent.with_open_preflight(
            self.hub,
            &self.saved,
            || self.policy_boundary((method == "tools/call").then_some(&frame["params"])),
            |db, current| {
                if method == "tools/call" {
                    let invocation = self.invocation.as_ref().context("Autorização MCP ausente")?;
                    ensure!(
                        !invocation.sent
                            && Instant::now() < invocation.deadline
                            && frame["params"] == invocation.params,
                        "Autorização MCP vencida, usada ou divergente"
                    );
                    self.check_invocation(db, current, invocation)?;
                    ensure!(
                        mcp::redigir_json(&frame["params"]["arguments"], &self.secrets) == frame["params"]["arguments"],
                        "Pedido contém material privado"
                    );
                }
                if method == "tools/list" {
                    return Ok(Some(ferramentas::iniciar_descoberta_open(db, &current.id)?));
                }
                Ok(None)
            },
        )?;
        self.intent.validate_live(self.hub)?;
        self.target.as_ref().context("Alvo MCP indisponível")?.validate_delivery()?;
        if let Some(generation) = generation {
            self.generation = Some(generation);
        }
        if method == "tools/call"
            && let Some(invocation) = &mut self.invocation
        {
            invocation.sent = true;
        }
        Ok(())
    }

    #[allow(clippy::unused_async_trait_impl)]
    async fn observe(&mut self, frame: Value, method: &str) -> Result<Option<Value>> {
        self.secrets.sort_unstable_by_key(|s| std::cmp::Reverse(s.len()));
        let mut body = json!({"method":method,"connector":self.saved.id});
        body["frame"] = frame;
        let mut observed = mcp::observe_checked(
            Observation::Open(self.turn),
            &body,
            Some(&self.secrets),
            || {
                if method == "tools/call" {
                    self.check_call()
                } else {
                    self.intent.with_open_current(self.hub, &self.saved, |_, _| Ok(()))
                }
            },
            |event| {
                let params = if method == "tools/call" {
                    Some(&self.invocation.as_ref().context("Autorização MCP ausente")?.params)
                } else {
                    None
                };
                self.intent.with_open_preflight(
                    self.hub,
                    &self.saved,
                    || self.policy_boundary(params),
                    |db, current| {
                        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                        Store::guard_record_on(&tx, None, event, false, |db| {
                            self.intent.validate_db(db)?;
                            if method == "tools/call" {
                                let invocation = self.invocation.as_ref().context("Autorização MCP ausente")?;
                                ensure!(invocation.sent, "Chamada MCP não admitida");
                                self.check_invocation(db, current, invocation)?;
                            }
                            Ok(())
                        })?;
                        if (method == "tools/call" || (method == "tools/list" && self.delivers_list))
                            && event["decision"] == "sanitized"
                        {
                            Store::guard_mark_external_on(&tx, &self.turn.lane().chat)?;
                        }
                        tx.commit()?;
                        Ok(())
                    },
                )
            },
        )?;
        if method == "tools/list" {
            let generation = self.generation.take().context("Descoberta MCP ausente")?;
            self.intent.with_open_current(self.hub, &self.saved, |db, current| {
                ferramentas::filtrar_lista_open(
                    db,
                    &self.turn.lane().chat,
                    current,
                    &body["frame"],
                    &mut observed,
                    generation,
                    |db| self.intent.validate_db(db).map(|_| ()),
                )
            })?;
        }
        Ok(observed.get_mut("message").filter(|value| !value.is_null()).map(Value::take))
    }
}

#[cfg(test)]
#[path = "open_mcp_tests.rs"]
mod tests;
