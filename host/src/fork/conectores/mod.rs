//! PR04: consumidor estrito; fonte 05 e apresentação 06 ausentes fecham produção.
//! Login vinculado e runtime OAuth integrados; catálogo 05 somente preparatório.
pub(crate) mod api;
mod catalogo;
mod catalogo_fichas;
mod catalogo_google;
mod catalogo_json;
pub(crate) mod catalogo_local;
pub(crate) mod catalogo_servicos;
#[cfg(test)]
mod catalogo_servicos_tests;
#[cfg(test)]
mod catalogo_tests;
mod contexto;
mod lifecycle;
pub(crate) mod mcp_version;
pub(crate) mod whatsapp_human;
pub(crate) use lifecycle::{Definitions, Pool};
pub(crate) mod oauth_binding;
pub(crate) mod oauth_desktop;
pub(crate) mod oauth_iss;
pub(crate) mod oauth_refresh;
pub(crate) mod oauth_registration;
pub(crate) mod oauth_resource;
pub(crate) mod oauth_runtime;
pub(crate) mod oauth_scopes;
pub(crate) mod oauth_tentativa;
pub(crate) mod oauth_token_scopes;
pub(crate) mod oauth_urls;
pub(crate) mod operacao;
mod proxy;
#[cfg(test)]
mod tests;

use crate::{LockExt, hub::Hub};
use anyhow::{Result, anyhow, bail, ensure};
use operacao::{ContaSaida, SnapshotAcesso};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub(crate) struct State {
    channels: Mutex<HashMap<String, Arc<proxy::Channel>>>,
    drafts: whatsapp_human::Custody,
    #[cfg(test)]
    sources: Mutex<HashMap<String, Source>>,
}

/// Fonte só pode vir do contrato canônico Hub. Nenhum JSON de RPC desserializa Source.
#[derive(Clone)]
struct Source {
    access: SnapshotAcesso,
    /// Conta de saída legível do produtor canônico do serviço (`WhatsApp`: frente própria,
    /// ainda ausente). `None` fecha antes de cofre, cartão e rede.
    conta: Option<ContaSaida>,
    presentation: Presentation,
    /// Alvo permanece exclusivamente no Hub; não faz parte da ponte ou do ledger.
    target: String,
    headers: reqwest::header::HeaderMap,
    motor: Motor,
}
#[derive(Clone, Copy)]
enum Motor {
    Unavailable,
    #[cfg(test)]
    Fixture300,
}
impl Source {
    /// Mesma concessão e mesma conta legível capturadas no canal. Conta A nunca vale por B.
    fn same(&self, access: &SnapshotAcesso, conta: &ContaSaida) -> bool {
        self.access.same_grant(access) && self.conta.as_ref() == Some(conta)
    }
    fn verify_engine(&self, c: &rusqlite::Connection) -> Result<()> {
        let backend: String =
            c.query_row("SELECT json_extract(config,'$.backend') FROM bots WHERE id=?1", [&self.access.bot], |r| {
                r.get(0)
            })?;
        #[cfg(test)]
        if matches!(self.motor, Motor::Fixture300) && backend == "fixture-mcp-300" {
            return Ok(());
        }
        let _ = backend;
        bail!("Contrato de timeout deste backend/versão não comprovado; motor recusado")
    }
}
#[derive(Clone, PartialEq, Eq)]
enum Presentation {
    Unavailable,
    #[cfg(test)]
    V1,
}

fn source<T>(hub: &Hub, instance: &str, action: impl FnOnce(&Source) -> Result<T>) -> Result<T> {
    #[cfg(test)]
    {
        let sources = hub.conectores.sources.locked();
        if let Some(source) = sources.get(instance) {
            return action(source);
        }
    }
    // A futura fábrica deve ler Connector+binding, descriptor e conector_acessos
    // sob a cerca; registry_name, URL, connectors e parâmetros não são autoridade.
    let _ = (hub, instance, action, Presentation::Unavailable, Motor::Unavailable);
    bail!("conector_fonte_indisponivel: fonte canônica 05 ausente; decisionProtocol:0")
}

impl State {
    fn channel(&self, id: &str) -> Result<Arc<proxy::Channel>> {
        self.channels.locked().get(id).cloned().ok_or_else(|| anyhow!("Canal indisponível"))
    }
}

fn available(source: &Source) -> Result<()> {
    source.access.validate()?;
    let conta = source
        .conta
        .as_ref()
        .ok_or_else(|| anyhow!("conta_saida_nao_identificada: fonte sem identidade canônica da conta"))?;
    conta.validate()?;
    ensure!(source.access.conta.as_ref() == Some(&conta.vinculo), "Conta de saída divergente da concessão");
    ensure!(source.presentation != Presentation::Unavailable, "conector_apresentacao_indisponivel: decisionProtocol:0");
    match source.motor {
        #[cfg(test)]
        Motor::Fixture300 => Ok(()),
        Motor::Unavailable => Err(anyhow!("Motor sem orçamento MCP comprovado")),
    }
}

/// Ponto de encaixe do ator. O 04 não inventa uma seleção quando a fonte está ausente.
pub(crate) fn definitions(
    hub: &Arc<Hub>,
    bot: &str,
    exe: &Path,
    lane: &crate::store::Lane,
    pool: &Pool,
) -> Result<Definitions> {
    Definitions::new(hub, bot, exe, lane, pool)
}

fn selected(hub: &Hub, bot: &str) -> Vec<String> {
    #[cfg(test)]
    {
        hub.conectores
            .sources
            .locked()
            .values()
            .filter(|s| s.access.bot == bot)
            .map(|s| s.access.instance.clone())
            .collect()
    }
    #[cfg(not(test))]
    {
        let _ = (hub, bot);
        Vec::new()
    }
}
fn register(hub: &Arc<Hub>, bot: &str, instance: &str, lane: &crate::store::Lane) -> Result<String> {
    source(hub, instance, |source| {
        available(source)?;
        ensure!(source.access.bot == bot && lane.chat == bot, "Seleção curada incorreta");
        let mut channels = hub.conectores.channels.locked();
        let id = operacao::id();
        channels.insert(id.clone(), Arc::new(proxy::Channel::new(hub, bot, source, &id, lane)?));
        Ok(id)
    })
}

pub(crate) use proxy::serve;
