use crate::LockExt;
use crate::hub::Hub;
use crate::store::{Store, now_ms};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Computer {
    pub id: String,
    pub device_key: String,
    pub label: String,
    pub platform: String,
    pub os_version: Option<String>,
    pub arch: Option<String>,
    pub app_version: Option<String>,
    pub capabilities: Value,
    pub presence: Value,
    pub enrolled_at: i64,
    pub last_seen_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub state_since: Option<i64>,
    pub removed_at: Option<i64>,
    pub last_boot_id: Option<String>,
}

pub fn bots(store: &Store) -> Result<Vec<String>> {
    match store.kv_read("computers.bots")? {
        Some(v) => Ok(serde_json::from_str(&v)?),
        None => Ok(store.team_settings()?.primary_bot_id.into_iter().collect()),
    }
}

pub fn state(hub: &Hub, c: &Computer, now: i64) -> &'static str {
    if c.presence["stopped"] == true {
        return "stopped";
    }
    if c.presence["asleep"] == true {
        return "asleep";
    }
    if c.presence["appConnected"] != true {
        return "app_closed";
    }
    if !hub.connected.locked().contains_key(&c.device_key) || c.last_seen_at.is_none_or(|at| now - at > super::ONLINE) {
        return "offline";
    }
    if c.presence["locked"] == true {
        return "locked";
    }
    "online"
}

pub fn list(hub: &Hub, from: Option<&str>) -> Result<Value> {
    Ok(json!({"computers": hub.store.computers()?.iter().map(|c| json!({
        "id": c.id, "name": c.label, "os": if c.platform == "macos" { "macOS" } else { "Windows" },
        "state": state(hub, c, now_ms()), "since": c.state_since.or(c.last_seen_at),
        "lastUsed": c.last_used_at, "current": from == Some(c.id.as_str()),
        "capabilities": c.capabilities, "readFolders": c.presence["readFolders"], "approval": "perguntar",
    })).collect::<Vec<_>>()}))
}

pub fn resolve(hub: &Hub, requested: Option<&str>, from: Option<&str>) -> Result<Computer> {
    if requested.is_some_and(|s| {
        s.eq_ignore_ascii_case("hub")
            || s.eq_ignore_ascii_case(&crate::service::host_name())
            || s == hub.identity.computer_id()
            || s == hub.host_id
    }) {
        bail!("use_hub_tools: para o Hub use o shell e os arquivos do próprio motor");
    }
    let computers = hub.store.computers()?;
    if computers.is_empty() {
        bail!("no_computers: o acesso aos seus computadores ainda está sendo instalado; nenhum computador cadastrado");
    }
    let valid = computers.iter().map(|c| format!("{} ({})", c.label, c.id)).collect::<Vec<_>>().join(", ");
    if let Some(name) = requested {
        let matches: Vec<_> =
            computers.iter().filter(|c| c.id == name || c.label.to_lowercase() == name.to_lowercase()).collect();
        return match matches.as_slice() {
            [c] => Ok((*c).clone()),
            [] => bail!("unknown_computer: {name:?} não é um computador do dono. Válidos: {valid}"),
            _ => bail!("ambiguous_computer: rótulo repetido; use o id. Válidos: {valid}"),
        };
    }
    if let Some(c) = computers.iter().find(|c| from == Some(c.id.as_str())) {
        return Ok(c.clone());
    }
    if computers.len() == 1 {
        return Ok(computers[0].clone());
    }
    bail!("ambiguous_computer: há vários computadores; pergunte ao dono qual. Válidos: {valid}")
}

pub fn remember_source(store: &Store, body: &Value, data: &mut Value) {
    if let Some(id) = body["fromComputer"].as_str()
        && store.computers().is_ok_and(|cs| cs.iter().any(|c| c.id == id))
    {
        data["fromComputer"] = json!(id);
    }
}
