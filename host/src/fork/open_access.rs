//! Pedido de acesso do agente no turno do dono local: computador, navegador ou pasta. Nada é concedido sem o
//! Aprovar do dono no app, nem com o bot em aprovação automática; a pasta é conferida de novo ao aplicar e cada
//! concessão guarda o estado anterior para o Desfazer.
use super::{autoridade::Autoridade, browser::BrowserPreference, donna_fixa::como_dono, open_command_settings, pasta};
use crate::{
    LockExt,
    agent::bot::Cmd,
    api::devices::Caller,
    hub::Hub,
    store::{BotConfig, Entry, Lane, Store},
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) const TOOL: &str = "request_access";
const GRANT: &str = "open_access_grant.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Access {
    Computer,
    Browser,
    Folder,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
enum State {
    Computer(bool),
    Browser(BrowserPreference),
    Folder(String),
}

impl State {
    fn access(&self) -> Access {
        match self {
            Self::Computer(_) => Access::Computer,
            Self::Browser(_) => Access::Browser,
            Self::Folder(_) => Access::Folder,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Grant {
    bot: String,
    before: State,
    after: State,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    access: Access,
    reason: String,
    #[serde(default)]
    path: Option<String>,
}

pub(super) fn catalog() -> Value {
    json!({
        "type":"function", "name":TOOL,
        "description":"Pede ao dono, num card no app, um acesso que falta: computer (comandos locais sem isolamento), browser (navegador integrado) ou folder (trocar sua pasta de trabalho; informe path absoluto). Explique o motivo em reason. Nada muda sem o Aprovar do dono.",
        "inputSchema":{
            "type":"object", "additionalProperties":false, "required":["access","reason"],
            "properties":{
                "access":{"enum":["computer","browser","folder"]},
                "reason":{"type":"string","maxLength":300},
                "path":{"type":"string"}
            }
        }
    })
}

pub(super) async fn call(
    hub: &Arc<Hub>,
    lane: &Lane,
    turn: i64,
    args: &Value,
    revalidate: impl Fn() -> Result<BotConfig>,
) -> Result<Value> {
    ensure!(crate::environment::Environment::is_open() && lane.session.is_none(), "open_tools_local_turn_required");
    let cfg = revalidate()?;
    ensure!(cfg.id == lane.chat && !cfg.is_group() && !cfg.hidden, "open_tools_local_turn_required");
    let Request { access, reason, path } =
        serde_json::from_value(args.clone()).context("open_tools_invalid_arguments")?;
    let reason = reason.trim();
    ensure!(
        !reason.is_empty()
            && reason.chars().count() <= 300
            && !reason.chars().any(char::is_control)
            && (access == Access::Folder) == path.is_some(),
        "open_tools_invalid_arguments"
    );
    let after = match access {
        Access::Computer => {
            open_command_settings::project(hub, &cfg).context("open_tools_command_unavailable")?;
            State::Computer(true)
        }
        Access::Browser => State::Browser(BrowserPreference::Integrated),
        Access::Folder => State::Folder(folder(path.as_deref().unwrap_or_default(), &cfg.id)?),
    };
    if read(&hub.store.db.locked(), &cfg, access)? == after {
        return Ok(json!({"access":access,"granted":true}));
    }
    let (title, allows) = match access {
        Access::Computer => (
            "Liberar comandos no computador",
            "Permite pedir comandos locais neste computador, cada um com aprovação no app. Os comandos rodam como você, com os seus arquivos e permissões, sem isolamento.",
        ),
        Access::Browser => (
            "Liberar o navegador integrado",
            "Permite usar o navegador integrado da Mexerica. No Mac, o navegador também precisa estar ligado nos ajustes.",
        ),
        Access::Folder => {
            ("Trocar a pasta de trabalho", "Permite ler e trabalhar nos arquivos desta pasta nas próximas respostas.")
        }
    };
    let path = match &after {
        State::Folder(path) => Some(path.as_str()),
        _ => None,
    };
    let detail = match path {
        Some(path) => format!("{allows}\nPasta: {path}\nMotivo: {reason}"),
        None => format!("{allows}\nMotivo: {reason}"),
    };
    let card = json!({"kind":access,"path":path,"asUser":access == Access::Computer,"state":"pending"});
    let approved = super::open_tool_actions::approve_access(hub, lane, turn, title, &detail, card).await?;
    let _team = hub.store.travar_equipe().await;
    let current = revalidate()?;
    ensure!(
        current.id == cfg.id && current.space_id == cfg.space_id && !current.hidden && !current.is_group(),
        "open_tools_local_turn_required"
    );
    {
        let _emit = hub.emit_lock.locked();
        let mut db = hub.store.db.locked();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        super::open_tool_actions::check_card(&tx, &approved)?;
        let row = Store::bot_on(&tx, &cfg.id)?
            .filter(|row| {
                !row.deleted && !row.config.hidden && !row.config.is_group() && row.config.space_id == cfg.space_id
            })
            .context("open_tools_local_turn_required")?;
        let before = read(&tx, &row.config, access)?;
        write(&tx, &row.config, &after)?;
        let grant = Grant { bot: cfg.id.clone(), before, after };
        tx.execute(
            "INSERT INTO kv(k,v) VALUES(?1,?2)",
            params![format!("{GRANT}{}", approved.id), serde_json::to_string(&grant)?],
        )?;
        tx.commit()?;
    }
    publish(hub, &cfg.id, &approved, "granted");
    Ok(json!({"access":access,"granted":true}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Undo {
    bot_id: String,
    entry_id: String,
}

/// RPC `undoAccessGrant`: só o app deste computador, com a autoridade do dono reconferida na transação. Volta ao
/// estado anterior apenas se o acesso ainda é o concedido pelo card.
pub(crate) fn undo(hub: &Hub, caller: &Caller, body: &Value) -> Result<Value> {
    ensure!(matches!(caller, Caller::Local), "open_tools_local_app_required");
    let Undo { bot_id, entry_id } = serde_json::from_value(body.clone()).context("invalid_field")?;
    let owner = Autoridade::capturar(hub, caller)?;
    {
        let _emit = hub.emit_lock.locked();
        owner.with_db(hub, None, |db| {
            let key = format!("{GRANT}{entry_id}");
            let raw: String = db
                .query_row("SELECT v FROM kv WHERE k=?1", [&key], |r| r.get(0))
                .optional()?
                .context("open_access_nothing_to_undo")?;
            let grant: Grant = serde_json::from_str(&raw)?;
            ensure!(grant.bot == bot_id, "open_access_nothing_to_undo");
            let row = Store::bot_on(db, &bot_id)?.filter(|row| !row.deleted).context("open_access_nothing_to_undo")?;
            ensure!(read(db, &row.config, grant.after.access())? == grant.after, "open_access_changed");
            write(db, &row.config, &grant.before)?;
            db.execute("DELETE FROM kv WHERE k=?1", [&key])?;
            Ok(())
        })?;
    }
    let card = hub.store.entry(&entry_id).context("open_access_nothing_to_undo")?;
    publish(hub, &bot_id, &card, "undone");
    Ok(json!({"entry": hub.store.entry(&entry_id)}))
}

fn folder(path: &str, bot: &str) -> Result<String> {
    pasta::validar(path, Some(bot))?
        .into_os_string()
        .into_string()
        .map_err(|_| anyhow::anyhow!("O caminho da pasta precisa ser texto UTF-8 válido."))
}

fn read(db: &Connection, cfg: &BotConfig, access: Access) -> Result<State> {
    Ok(match access {
        Access::Computer => State::Computer(open_command_settings::revision(db, &cfg.id)?.is_some()),
        Access::Browser => State::Browser(cfg.browser_preference),
        Access::Folder => State::Folder(cfg.cwd.clone()),
    })
}

fn write(db: &Connection, cfg: &BotConfig, state: &State) -> Result<()> {
    let mut cfg = cfg.clone();
    match state {
        State::Computer(enabled) => {
            open_command_settings::set(db, &cfg.id, *enabled)?;
            return Ok(());
        }
        State::Browser(preference) => cfg.browser_preference = *preference,
        State::Folder(path) => {
            cfg.cwd = if path.is_empty() { String::new() } else { folder(path, &cfg.id)? };
        }
    }
    como_dono(db, |c| crate::store::bots::save_bot(c, &cfg)).map(drop)
}

fn publish(hub: &Hub, bot: &str, card: &Entry, state: &str) {
    let mut data = card.data.clone();
    data["access"]["state"] = json!(state);
    hub.set_entry(&card.id, &data);
    if let Some(row) = hub.store.bot(bot).ok().flatten()
        && let Err(error) = hub.send_cmd(bot, Cmd::Reconfigure(Box::new(row.config)))
    {
        tracing::warn!(%error, bot, "acesso gravado; reconfiguração do bot fica para o próximo início");
    }
    hub.emit_bot(bot);
}

#[cfg(test)]
#[path = "open_access_tests.rs"]
mod tests;
