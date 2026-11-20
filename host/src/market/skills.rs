// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Habilidades instaladas e comandos. Catálogo/importação ficam no módulo próprio D166.
use anyhow::Result;
use serde_json::Value;

pub async fn browse_skills(store: &crate::store::Store, revalidate: impl FnOnce() -> Result<()>) -> Result<Value> {
    crate::fork::skill_catalog::browse(store, revalidate)
}

pub async fn skill_details(
    store: &crate::store::Store,
    b: &Value,
    revalidate: impl FnOnce() -> Result<()>,
) -> Result<Value> {
    crate::fork::skill_catalog::details(store, b, revalidate)
}

pub fn list_skills(store: &crate::store::Store, revalidate: impl FnOnce() -> Result<()>) -> Result<Value> {
    crate::fork::skill_catalog::list(store, revalidate)
}

/// D166: preserva seleções salvas; a migração antiga não ativa mais nada.
pub fn turn_on_installed_once(store: &crate::store::Store) -> Result<()> {
    crate::fork::skill_catalog::seed(store)
}

/// `/name rest` → `name` and the rest; the rest is `None` while the name is still being typed.
pub fn slash_command(text: &str) -> Option<(&str, Option<&str>)> {
    let body = text.strip_prefix('/')?;
    Some(match body.split_once(char::is_whitespace) {
        Some((name, rest)) => (name, Some(rest)),
        None => (body, None),
    })
}

/// A message that starts with `/<skill id>`, for a skill the bot has on, asks for that skill:
/// the agent gets told to read it first. `None` sends the message as written.
/// D306/R1: só a versão conferida agora entrega caminho (`skill_catalog::comando`).
pub fn expand_command(store: &crate::store::Store, enabled: &[String], text: &str) -> Option<String> {
    let (id, rest) = slash_command(text)?;
    if !enabled.iter().any(|e| e == id) {
        return None;
    }
    let note = crate::fork::skill_catalog::comando(store, id)?;
    Some(match rest.map(str::trim).filter(|r| !r.is_empty()) {
        Some(task) => format!("{note}\n\n{task}"),
        None => note,
    })
}

/// The skills block of a bot's profile: what each is for and where to read it.
/// D306/R1: alterada fora do app ou ilegível não entra (`skill_catalog::brief`).
pub fn skills_brief(store: &crate::store::Store, enabled: &[String]) -> Option<String> {
    crate::fork::skill_catalog::brief(store, enabled)
}
