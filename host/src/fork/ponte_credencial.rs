//! D330/F1: identidade de transporte das pontes. Não emite Authority nem prova humana.
use crate::{
    LockExt,
    api::devices::{Caller, Forbidden},
    hub::Hub,
    remote::crypto,
};
use anyhow::{Result, ensure};
use axum::http::HeaderMap;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

pub(crate) const ENV: &str = "DONNABOT_PONTE_TOKEN";
pub(crate) const MODO_ENV: &str = "DONNABOT_PONTE_MODE";
pub(crate) const SETUP_ENV: &str = "DONNABOT_MCP_SETUP_LOCAL";
// Inventário dos leitores: mcp, guard stdio/remoto e proxy dos conectores curados.
pub(crate) const PERMITIDOS: &[&str] = &[
    "connectorCall",
    "routineCall",
    "computerCall",
    "computersCall",
    "chatCall",
    "teamCall",
    "taskCall",
    "manageCall",
    "memoryCall",
    "computerTools",
    "donnaGuardRuntime",
    "donnaGuardObserve",
    "donnaGuardRelease",
    "donnaConectorFrame",
    "donnaConectorRelease",
];

// Sem Debug/Serialize: somente o hash e identificadores ficam no Hub.
struct Credencial {
    hash: [u8; 32],
    bot: String,
    slot: String,
    binding: String,
    contexts: Vec<String>,
}
#[derive(Default)]
pub(crate) struct Credenciais(Mutex<Vec<Arc<Credencial>>>);

/// A mesma credencial emitida pela Ponte, privada e não serializável. Não é Control.
#[derive(Clone)]
pub struct Origem(Arc<Credencial>);
impl Origem {
    pub(crate) fn corresponde(&self, bot: &str, binding: &str) -> bool {
        self.0.bot == bot && self.0.binding == binding
    }
    /// Retém somente a coleção canônica; nenhuma captura de sucessora pelo bot/slot.
    pub(crate) fn com_original<T>(&self, hub: &Hub, action: impl FnOnce() -> Result<T>) -> Result<T> {
        let saved = hub.pontes.0.locked();
        ensure!(saved.iter().any(|c| Arc::ptr_eq(c, &self.0)), Forbidden);
        action()
    }
    pub(crate) fn conferir_db(&self, db: &rusqlite::Connection) -> Result<()> {
        crate::fork::sessoes::bindings::resolve(db, &self.0.binding, &self.0.bot)?;
        Ok(())
    }
}

impl Credenciais {
    pub(crate) fn revogar_todas(&self) {
        self.0.locked().clear();
    }
    pub(crate) fn revogar_bot(&self, bot: &str) {
        self.0.locked().retain(|c| c.bot != bot);
    }
    pub(crate) fn revogar_slot(&self, bot: &str, slot: Option<&str>) {
        let slot = crate::guard_hub::tool_origin::slot_value(slot);
        self.0.locked().retain(|c| c.bot != bot || c.slot != slot);
    }
    pub(crate) fn tem_slot(&self, bot: &str, slot: Option<&str>) -> bool {
        let slot = crate::guard_hub::tool_origin::slot_value(slot);
        self.0.locked().iter().any(|c| c.bot == bot && c.slot == slot)
    }
    pub(crate) fn autenticar(&self, hub: &Hub, token: &str) -> Option<Caller> {
        let hash = crypto::sha256(&[token.as_bytes()]);
        let caller = self.0.locked().iter().find_map(|c| {
            crypto::ct_eq(&c.hash, &hash).then(|| Caller::Ponte {
                bot: c.bot.clone(),
                sessao: c.binding.clone(),
                origem: Origem(c.clone()),
            })
        })?;
        let Caller::Ponte { bot, sessao, .. } = &caller else { return None };
        // Fechamento da sessão, troca de conta e exclusão já revogam o binding no DB.
        crate::fork::sessoes::bindings::resolve(&hub.store.db.locked(), sessao, bot).ok()?;
        Some(caller)
    }
}

pub(crate) fn ler_token() -> Result<String> {
    let token = std::env::var(ENV).map_err(|_| anyhow::anyhow!("Credencial da ponte ausente"))?;
    ensure!(!token.trim().is_empty(), "Credencial da ponte ausente");
    Ok(token)
}

pub(crate) fn provisionar(
    hub: &Hub,
    bot: &str,
    slot: Option<&str>,
    binding: &crate::fork::sessoes::bindings::BindingRef,
    servers: &mut [Value],
) -> Result<()> {
    ensure!(binding.bot == bot, "Bot da ponte incorreto");
    let token = zeroize::Zeroizing::new(format!("ponte_{}", crypto::b64(&crypto::random::<32>())));
    let mut contexts = Vec::new();
    for server in servers {
        if let Some(args) = server["args"].as_array() {
            for pair in args.windows(2) {
                if pair[0] == "--context"
                    && let Some(id) = pair[1].as_str()
                {
                    contexts.push(id.to_owned());
                }
            }
        }
        if server.get("env").is_none() {
            server["env"] = json!([]);
        }
        let env = server["env"].as_array_mut().ok_or_else(|| anyhow::anyhow!("Ambiente MCP inválido"))?;
        env.retain(|item| item["name"] != ENV && item["name"] != SETUP_ENV);
        env.push(json!({"name":ENV,"value":token.as_str()}));
    }
    let mut saved = hub.pontes.0.locked();
    let slot = crate::guard_hub::tool_origin::slot_value(slot);
    saved.retain(|c| c.bot != bot || c.slot != slot);
    saved.push(Arc::new(Credencial {
        hash: crypto::sha256(&[token.as_bytes()]),
        bot: bot.into(),
        slot: slot.into(),
        binding: binding.id.as_str().into(),
        contexts,
    }));
    Ok(())
}

/// Antes de qualquer despacho, inclusive atalhos de autoridade. O corpo não escolhe a identidade.
pub(crate) fn conferir(hub: &Hub, caller: &Caller, method: &str, body: &Value) -> Result<()> {
    let Caller::Ponte { bot, sessao, origem } = caller else { return Ok(()) };
    crate::api::devices::permit(caller, method)?;
    ensure!(body.get("botId").is_none_or(|id| id.as_str() == Some(bot)), Forbidden);
    ensure!(body.get("sessionBinding").is_none_or(|id| id.as_str() == Some(sessao)), Forbidden);
    let scoped = {
        let saved = hub.pontes.0.locked();
        saved.iter().any(|c| {
            Arc::ptr_eq(c, &origem.0)
                && c.bot == *bot
                && c.binding == *sessao
                && body.get("sessionSlot").is_none_or(|slot| slot.as_str() == Some(c.slot.as_str()))
                && (if matches!(
                    method,
                    "donnaGuardRuntime"
                        | "donnaGuardObserve"
                        | "donnaGuardRelease"
                        | "donnaConectorFrame"
                        | "donnaConectorRelease"
                ) {
                    body["context"].as_str().is_some_and(|id| c.contexts.iter().any(|context| context == id))
                } else {
                    true
                })
        })
    };
    ensure!(scoped, Forbidden);
    let live = crate::fork::sessoes::bindings::resolve(&hub.store.db.locked(), sessao, bot);
    ensure!(live.is_ok(), Forbidden);
    // Chamadas builtin exigem o binding de execução já existente; não substitui suas guardas.
    if method.ends_with("Call") {
        ensure!(body["botId"].as_str() == Some(bot) && body["sessionBinding"].as_str() == Some(sessao), Forbidden);
    }
    Ok(())
}

pub(crate) fn revogar_acp(hub: &Hub, bot: &str, acp: &str) {
    if let Ok(binding) = crate::fork::sessoes::bindings::for_acp(&hub.store, bot, acp) {
        hub.pontes.0.locked().retain(|c| c.binding != binding.id.as_str());
    }
}

/// A conexão é dona das credenciais do processo, inclusive em erro/EOF e shutdown.
pub(crate) struct Revogador {
    hub: std::sync::Weak<Hub>,
    bot: String,
}
impl Revogador {
    pub(crate) fn new(hub: &Arc<Hub>, bot: &str) -> Self {
        Self { hub: Arc::downgrade(hub), bot: bot.into() }
    }
}
impl Drop for Revogador {
    fn drop(&mut self) {
        if let Some(hub) = self.hub.upgrade() {
            hub.pontes.revogar_bot(&self.bot);
        }
    }
}

#[path = "ponte_http.rs"]
mod http;
pub(crate) use http::{autenticar, autenticar_com_origin, limitar_bearer};

/// Autenticação dos streams SSE (`/events`, `/term/{id}`): `Bearer` local segue como
/// antes, sem cair na sessão Web; a sessão Web devolve a guarda que encerra o stream
/// no logout, no despejo por limite de sessões ou na expiração.
#[allow(clippy::option_option)]
pub(crate) fn autenticar_stream(
    hub: &Hub,
    headers: &HeaderMap,
    peer: std::net::SocketAddr,
) -> Option<Option<crate::fork::web_local::SessaoStreamGuard>> {
    if !peer.ip().to_canonical().is_loopback()
        || (crate::environment::Environment::is_open() && peer != crate::open_hub::SOCKET_PEER)
    {
        return None;
    }
    if headers.contains_key("authorization") {
        let given = headers.get("authorization")?.to_str().ok()?.strip_prefix("Bearer ")?;
        if given.is_empty() {
            return None;
        }
        if hub.pontes.autenticar(hub, given).is_some() {
            return None;
        }
        if !crypto::ct_eq(given.as_bytes(), hub.token.as_bytes()) {
            return None;
        }
        return Some(None);
    }
    hub.web_local()?.autenticar_stream(headers, peer).map(Some)
}

#[cfg(test)]
#[path = "ponte_credencial_tests.rs"]
mod tests;
