//! The marketplace behind the phone's Plugins screen:
//!
//! - **Connectors** are MCP servers configured by URL, command or pasted config.
//!   Historical Registry provenance is passive. They are
//!   installed once per computer and handed to the agent as `mcpServers` when a
//!   bot that has them turned on starts or resumes its session.
//! - **Skills** are instruction folders (a `SKILL.md` plus any files it uses),
//!   from the pinned passive Mexerica catalog or written by hand, kept in
//!   `~/.codync/skills/<id>`. A bot that has one turned on is told where it is
//!   and reads it when the task calls for it.
//! - **Agents** come from the ACP registry; see `backends`.
//!
//! Imported configuration and GitHub JSON are untrusted input: names become ids and paths, so
//! they're reduced to a safe slug first. Secret values (API keys, tokens) stay
//! on this computer; listings only say which keys are set.

pub(crate) mod contas;
pub(crate) mod contas_api;
pub(crate) mod contas_escolha;
pub(crate) mod contas_migracao;
mod contas_recibos;
pub(crate) mod google_app;
pub mod logins;
pub mod oauth;
pub mod passwords;
pub mod requests;
pub mod vault;
pub mod verify;

#[cfg(test)]
mod contas_tests;
#[cfg(test)]
mod google_app_tests;
mod install;
pub(crate) mod login_codes;
mod registry;
mod skills;

pub(crate) use contas_recibos::migrate as migrate_recibos;
pub use install::{add_custom_connector, import_connectors, list_connectors, remove_connector};
pub use registry::{REGISTRY_UNAVAILABLE, reject_registry_request};
pub use skills::{
    browse_skills, expand_command, list_skills, skill_details, skills_brief, slash_command, turn_on_installed_once,
};

use crate::LockExt;
use crate::fork::conectores::oauth_binding;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

// MARK: stored items

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Connector {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Passive historical MCP Registry provenance; never used for discovery or updates.
    #[serde(default)]
    pub registry_name: Option<String>,
    /// stdio servers: the command the agent spawns.
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Remote (streamable HTTP) servers.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Remote servers that want sign-in; see `oauth`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<oauth::OAuth>,
    /// Metadados privados emitidos pelo Hub; não aparecem no DTO público.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) oauth_binding: Option<oauth_binding::Binding>,
    /// Marketplace: id da ficha do catálogo que originou a conta; `None` = MCP do próprio usuário.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub servico: Option<String>,
    /// Marketplace: id do jeito de conectar usado (`linear.oficial_oauth`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tipo: Option<String>,
    /// Marketplace: grupos de liberação da ficha ligados nesta conta. Só o Hub os grava, conferidos na ficha.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub liberacoes: Vec<String>,
    /// Space dono da conta, validado pelo Hub. Vazio só em conta anterior ao campo: `contas_migracao` lhe dá o Space.
    #[serde(default)]
    pub space: String,
    /// E-mail ou login legível do fornecedor, só para a tela; recibos usam um identificador opaco.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identidade: Option<String>,
}

impl Connector {
    /// Only materialization of a saved target, never a health/authentication claim.
    pub fn configured(&self) -> bool {
        match self.command.as_deref() {
            Some(command) => !command.trim().is_empty(),
            None => self.url.as_deref().is_some_and(|url| !url.trim().is_empty() && !url.contains(['{', '}'])),
        }
    }

    /// ACP `McpServer`. Remote servers run through this host's `remote` proxy (`codync-host mcp remote`),
    /// which keeps sign-in tokens fresh and works with agents that only speak stdio.
    /// A remote server still waiting for sign-in is left out.
    pub fn acp(&self, exe: &std::path::Path, port: u16) -> Option<Value> {
        if !self.configured() {
            return None;
        }
        if self.command.is_some() {
            return Some(
                json!({"name":self.id,"command":exe,"args":["mcp","local","--connector",self.id,"--port",port.to_string()],"env":[]}),
            );
        }
        self.url.as_ref()?;
        // Conta de ficha por chave ainda sem o valor guardado: nada de chamar o fornecedor sem credencial.
        if self.servico.is_some() && self.oauth.is_none() && self.headers.values().any(String::is_empty) {
            return None;
        }
        if self.oauth_binding.as_ref().is_some_and(oauth_binding::Binding::scopes_retained)
            || self.oauth.as_ref().is_some_and(|o| !o.signed_in())
        {
            return None;
        }
        Some(json!({
            "name": self.id, "command": exe,
            "args": ["mcp", "remote", "--connector", self.id, "--port", port.to_string()],
            "env": [],
        }))
    }

    /// What clients see: everything but the secret values.
    pub fn public(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "description": self.description,
            "registryName": self.registry_name,
            "configuration": if self.configured() { "configured" } else { "manualRequired" },
            "kind": if self.command.is_some() { "local" } else { "remote" },
            "command": self.command.clone(),
            "url": self.url.as_deref().and_then(|raw| reqwest::Url::parse(raw).ok()).map(|mut url| {
                let _ = url.set_username(""); let _ = url.set_password(None);
                url.set_query(None); url.set_fragment(None); url.to_string()
            }),
            "keys": self.env.keys().chain(self.headers.keys()).collect::<Vec<_>>(),
            // MCP do próprio usuário (decisão de 15/11): fora do catálogo, "seu, não verificado".
            "origem": if self.servico.is_some() { "catalogo" } else { "usuario" },
            "naoVerificado": self.servico.is_none(),
            "servico": self.servico,
            // A logo do serviço do catálogo (a chave do arquivo no app); sem serviço ou fora do catálogo, nada.
            "marca": self.servico.as_deref().and_then(crate::fork::conectores::catalogo_servicos::servico).map(|s| s.marca),
            "tipo": self.tipo,
            "liberacoes": self.liberacoes,
            "space": self.space,
            "identidade": self.identidade,
            "auth": match &self.oauth {
                None => "none",
                Some(o) if o.signed_in() && !self.oauth_binding.as_ref().is_some_and(oauth_binding::Binding::scopes_retained) => "signedIn",
                Some(_) => "signedOut",
            },
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// "catalog:<id>", "custom" ou procedência legada preservada.
    #[serde(default)]
    pub source: String,
}

/// Lowercase ASCII letters, digits and dashes; never empty, never a path.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').chars().take(48).collect::<String>();
    if out.is_empty() { "item".into() } else { out }
}

// D306/R1: skills_brief/`/id` leem pelo catálogo conferido; esta leitura crua ficou só para testes.
#[cfg(test)]
fn load<T: for<'de> Deserialize<'de>>(store: &crate::store::Store, key: &str) -> Vec<T> {
    store.kv_get(key).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub(crate) fn load_connectors(store: &crate::store::Store) -> Result<(Vec<Connector>, bool)> {
    let (raw, plaintext) = vault::read_unmigrated(store, "connectors")?;
    let mut all: Vec<Connector> = raw.map_or_else(|| Ok(Vec::new()), |s| serde_json::from_str(&s))?;
    // Conta anterior ao campo `space`: ganha o Space pela regra da migração e o cofre é regravado.
    let migrated = contas_migracao::normalizar(store, &mut all)?;
    Ok((all, plaintext || migrated))
}

pub fn connectors(store: &crate::store::Store) -> Result<Vec<Connector>> {
    let _guard = store.connector_lock.locked();
    connectors_locked(store)
}

/// Variante para quem já segura `connector_lock` (mesma fronteira das escritas);
/// nunca reentrar o lock público.
pub(crate) fn connectors_locked(store: &crate::store::Store) -> Result<Vec<Connector>> {
    let (mut all, plaintext) = load_connectors(store)?;
    if oauth_binding::initialize(&mut all) || plaintext {
        persist_connectors(store, &all)?;
    }
    Ok(all)
}

#[cfg(test)]
pub fn skills(store: &crate::store::Store) -> Vec<Skill> {
    load(store, "skills")
}

fn persist_connectors(store: &crate::store::Store, items: &[Connector]) -> Result<()> {
    vault::write(store, "connectors", &serde_json::to_string(items)?)
}

#[cfg(test)]
pub fn save_connectors(store: &crate::store::Store, items: &[Connector]) -> Result<()> {
    update_connectors(store, |all| {
        *all = items.to_vec();
        Ok(())
    })
}

/// Serialize read/modify/write so OAuth refreshes and installations cannot replace each other.
pub fn update_connectors<T>(
    store: &crate::store::Store,
    change: impl FnOnce(&mut Vec<Connector>) -> Result<T>,
) -> Result<T> {
    update_connectors_com(store, change, |_, _| Ok(()))
}

/// Como `update_connectors`, e `extra` (com o resultado da mudança) roda na MESMA transação que sela o cofre.
pub(crate) fn update_connectors_com<T>(
    store: &crate::store::Store,
    change: impl FnOnce(&mut Vec<Connector>) -> Result<T>,
    extra: impl FnOnce(&rusqlite::Connection, &T) -> Result<()>,
) -> Result<T> {
    let _guard = store.connector_lock.locked();
    let (mut all, _) = load_connectors(store)?;
    oauth_binding::initialize(&mut all);
    let before = all.clone();
    let result = change(&mut all)?;
    oauth_binding::lifecycle(&before, &mut all)?;
    oauth_binding::persist_locked(store, &before, &all, |tx| extra(tx, &result))?;
    for old in &before {
        if !all.iter().any(|c| oauth_binding::same_target(c, old)) {
            oauth::invalidate(old);
        }
    }
    Ok(result)
}

fn add_connector(store: &crate::store::Store, mut c: Connector) -> Result<Value> {
    update_connectors(store, |all| {
        if let Some(name) = c.registry_name.as_deref()
            && let Some(existing) = all.iter().find(|x| x.registry_name.as_deref() == Some(name))
        {
            return Ok(existing.public());
        }
        if c.registry_name.is_none() {
            contas::nome_livre(all, c.servico.as_deref(), &c.space, &c.name, None)?;
        }
        c.id = unique_id(&c.name, &reserved_connector_ids(store, all)?);
        c.oauth_binding = Some(oauth_binding::Binding::new());
        let public = c.public();
        all.push(c);
        Ok(public)
    })
}

/// A removed connector may still be selected in a bot. Reusing its id would grant a new account access.
fn reserved_connector_ids(store: &crate::store::Store, all: &[Connector]) -> Result<Vec<String>> {
    let mut ids: Vec<String> = all.iter().map(|c| c.id.clone()).collect();
    for row in store.bots()? {
        ids.extend(row.config.connectors);
    }
    Ok(ids)
}

fn unique_id(base: &str, taken: &[String]) -> String {
    let base = slug(base);
    let mut id = base.clone();
    let mut n = 2;
    while taken.contains(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_safe() {
        assert_eq!(slug("GitHub MCP Server"), "github-mcp-server");
        assert_eq!(slug("../../etc/passwd"), "etc-passwd");
        assert_eq!(slug("中文"), "item");
    }

    #[test]
    fn connectors_become_acp_servers() {
        let mut c = Connector {
            id: "gh".into(),
            name: "GitHub".into(),
            description: String::new(),
            registry_name: None,
            command: Some("npx".into()),
            args: vec!["-y".into(), "pkg@1".into()],
            env: BTreeMap::from([("TOKEN".into(), "x".into())]),
            url: None,
            headers: BTreeMap::new(),
            oauth: None,
            oauth_binding: None,
            servico: None,
            tipo: None,
            liberacoes: vec![],
            space: crate::fork::spaces::personal_id(),
            identidade: None,
        };
        let exe = std::path::Path::new("/bin/codync-host");
        let config = c.acp(exe, 1).unwrap();
        assert_eq!(config["args"][1], "local");
        assert_eq!(config["env"], json!([]));
        assert!(!config.to_string().contains("secret"));
        assert!(c.public().get("env").is_none(), "secrets never leave the host");
        c.command = None;
        c.url = Some("https://example.com/mcp".into());
        assert_eq!(c.acp(exe, 1).unwrap()["args"][1], "remote", "remote servers go through the proxy");
        c.oauth = Some(oauth::OAuth::default());
        assert!(c.acp(exe, 1).is_none(), "not signed in yet");
        assert_eq!(c.public()["auth"], "signedOut");
    }

    #[tokio::test]
    async fn custom_connector_keeps_the_ordinary_path() {
        let store = crate::store::Store::open(std::path::Path::new(":memory:")).unwrap();
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/credential_mcp.py");
        let python = if cfg!(windows) { "python" } else { "python3" };
        let c = Connector {
            id: "exemplo-mcp".into(),
            name: "Exemplo MCP".into(),
            description: String::new(),
            registry_name: None,
            command: Some(python.into()),
            args: vec![fixture.to_string_lossy().into_owned()],
            env: BTreeMap::from([("API_KEY".into(), "test-private-key".into())]),
            url: None,
            headers: BTreeMap::new(),
            oauth: None,
            oauth_binding: None,
            servico: None,
            tipo: None,
            liberacoes: vec![],
            space: crate::fork::spaces::personal_id(),
            identidade: None,
        };
        save_connectors(&store, &[c]).unwrap();
        assert_eq!(verify::verify(&store, "exemplo-mcp", 19222).await.unwrap()["status"], "ready");
        remove_connector(&store, "exemplo-mcp").unwrap();
        assert_eq!(connectors(&store).unwrap(), Vec::<Connector>::new());
    }
}
