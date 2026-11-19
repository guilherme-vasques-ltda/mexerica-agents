// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Native MCP configuration: pasted configs, custom servers, listing and removal.

use super::{
    Connector, add_connector, connectors, contas, oauth, reserved_connector_ids, unique_id, update_connectors,
};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// A connector you describe yourself: a command line or an https URL.
pub async fn add_custom_connector(store: &crate::store::Store, b: &Value) -> Result<Value> {
    let name =
        b["name"].as_str().map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("`name` is required"))?;
    let space = contas::space_do_pedido(store, b)?;
    let all = connectors(store)?;
    // Before reaching the server; `add_connector` checks again as it saves.
    contas::nome_livre(&all, None, &space, name, None)?;
    let mut c = custom(name, b, &all).await?;
    c.space = space;
    add_connector(store, c)
}

/// Adds every server in a pasted MCP config: the `mcpServers` / `servers` JSON that
/// Claude, Cursor, VS Code and most READMEs use, or one server's own entry.
pub async fn import_connectors(store: &crate::store::Store, config: &str, space: &str) -> Result<Value> {
    store.require_space(space)?;
    let v: Value = serde_json::from_str(config.trim()).context("that isn't JSON")?;
    let servers = config_servers(&v);
    if servers.is_empty() {
        bail!("no MCP servers in that config");
    }
    let all = connectors(store)?;
    let mut pending = vec![];
    for (name, entry) in servers {
        pending.push(custom(&name, &entry, &all).await.with_context(|| name.clone())?);
    }
    update_connectors(store, |all| {
        let mut added = vec![];
        for mut c in pending {
            space.clone_into(&mut c.space);
            contas::nome_livre(all, None, space, &c.name, None)?;
            c.id = unique_id(&c.name, &reserved_connector_ids(store, all)?);
            c.oauth_binding = Some(crate::fork::conectores::oauth_binding::Binding::new());
            added.push(c.public());
            all.push(c);
        }
        Ok(json!({"items":added}))
    })
}

/// The named server entries in a pasted config.
fn config_servers(v: &Value) -> Vec<(String, Value)> {
    match ["mcpServers", "servers", "context_servers"].iter().find_map(|k| v[*k].as_object()) {
        Some(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        None if v["command"].is_string() || v["url"].is_string() => {
            vec![(v["name"].as_str().unwrap_or("Custom").to_owned(), v.clone())]
        }
        // `{"github": {"command": …}}`: the map without its wrapper.
        None => v
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, e)| e.is_object())
            .map(|(k, e)| (k.clone(), e.clone()))
            .collect(),
    }
}

/// A connector from `{command, args?, env?}` or `{url, headers?}`. A command without
/// `args` is split like a shell would (quotes work).
async fn custom(name: &str, b: &Value, all: &[Connector]) -> Result<Connector> {
    let map = |v: &Value| -> BTreeMap<String, String> {
        v.as_object().into_iter().flatten().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned()))).collect()
    };
    let mut c = Connector {
        id: unique_id(name, &all.iter().map(|c| c.id.clone()).collect::<Vec<_>>()),
        name: name.to_owned(),
        description: b["description"].as_str().unwrap_or_default().to_owned(),
        registry_name: None,
        command: None,
        args: vec![],
        env: map(&b["env"]),
        url: None,
        headers: map(&b["headers"]),
        oauth: None,
        oauth_binding: None,
        servico: None,
        tipo: None,
        liberacoes: vec![],
        space: crate::fork::spaces::personal_id(),
        identidade: None,
    };
    let url =
        ["url", "serverUrl", "httpUrl"].iter().find_map(|k| b[*k].as_str()).map(str::trim).filter(|s| !s.is_empty());
    if let Some(url) = url {
        // DONNA M2-A: https, ou http só em loopback por nome/literal; sem prefixo falso.
        crate::fork::conectores::oauth_urls::conector(url)?;
        if oauth::required(url, &c.headers).await {
            c.oauth = Some(oauth::OAuth::default());
        }
        c.url = Some(url.to_owned());
        return Ok(c);
    }
    let line = b["command"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("give a command or a URL"))?;
    let mut words = match b["args"].as_array() {
        Some(args) => {
            std::iter::once(line.to_owned()).chain(args.iter().filter_map(|a| a.as_str().map(str::to_owned))).collect()
        }
        None => shlex::split(line).ok_or_else(|| anyhow!("the command has an unclosed quote"))?,
    }
    .into_iter();
    c.command = words.next();
    c.args = words.collect();
    Ok(c)
}

/// Installed MCP servers.
pub fn list_connectors(store: &crate::store::Store) -> Result<Value> {
    let items: Vec<Value> = connectors(store)?.iter().map(Connector::public).collect();
    Ok(json!({"items": items}))
}

pub fn remove_connector(store: &crate::store::Store, id: &str) -> Result<()> {
    update_connectors(store, |all| {
        all.retain(|c| c.id != id);
        Ok(())
    })?;
    store.guard_connector_removed(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn two_accounts_of_one_service_need_different_names() {
        let store = crate::store::Store::open(std::path::Path::new(":memory:")).unwrap();
        let body = |name: &str| json!({"name": name, "command": "github-mcp --stdio"});
        assert_eq!(add_custom_connector(&store, &body("GitHub pessoal")).await.unwrap()["id"], "github-pessoal");
        assert_eq!(add_custom_connector(&store, &body("GitHub Casa")).await.unwrap()["id"], "github-casa");
        let taken = add_custom_connector(&store, &body(" github PESSOAL ")).await.unwrap_err().to_string();
        assert!(taken.contains("already a connector named"), "{taken}");
        let personal = crate::fork::spaces::PERSONAL;
        assert!(
            import_connectors(&store, r#"{"mcpServers":{"GitHub Casa":{"command":"x"}}}"#, personal).await.is_err()
        );
        assert_eq!(connectors(&store).unwrap().len(), 2);
    }

    #[test]
    fn pasted_configs_list_their_servers() {
        let claude = json!({"mcpServers": {"github": {"command": "npx", "args": ["-y", "x"]}, "linear": {"url": "https://l/mcp"}}});
        assert_eq!(config_servers(&claude).len(), 2);
        let vscode = json!({"servers": {"a": {"type": "sse", "url": "https://a/sse"}}});
        assert_eq!(config_servers(&vscode)[0].0, "a");
        let one = json!({"command": "uvx", "args": ["y"]});
        assert_eq!(config_servers(&one)[0].0, "Custom");
        let bare = json!({"fs": {"command": "npx"}});
        assert_eq!(config_servers(&bare)[0].0, "fs");
    }
}
