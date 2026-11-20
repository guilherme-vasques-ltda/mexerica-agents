// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Saved sign-ins for websites and apps. A bot asks for one with `request_login` (a
//! secure card the user fills in), then `type_login` types it into the focused field;
//! the password reaches the screen, never the bot. Stored encrypted in the vault.
//!
//! D82: each login is an account of its own with a readable label, turned on bot by bot
//! (`BotConfig::logins`). A bot lists and types only the logins on for it; with more than
//! one on for the same site, it has to say which.
use crate::LockExt;
use crate::remote::cloud::Conflict;
use crate::store::Store;
use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Mutex;

const SLOT: &str = "logins";
const MAX_LABEL: usize = 100;
/// Every read-modify-write of the logins slot holds this, so a creation-only save checks and writes
/// under the same exclusion (no check-then-save race, even against another save or a removal).
static LOGINS: Mutex<()> = Mutex::new(());
/// The 409 message of a creation-only save that found the same site and username.
pub const LOGIN_EXISTS: &str = "login_exists";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Login {
    pub id: String,
    /// A domain (`github.com`) or an app name (`Slack`).
    pub site: String,
    #[serde(default)]
    pub username: String,
    /// The user's name for it ("GitHub Casa"); empty: `site · username`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// The password, or an `op://` reference resolved when it's typed.
    pub password: String,
}

impl Login {
    /// The label bots and people see: the user's, or `site · username`.
    pub fn title(&self) -> String {
        if !self.label.is_empty() {
            self.label.clone()
        } else if self.username.is_empty() {
            self.site.clone()
        } else {
            format!("{} · {}", self.site, self.username)
        }
    }

    /// Everything but the password.
    pub fn public(&self) -> Value {
        json!({"id": self.id, "site": self.site, "username": self.username, "label": self.title()})
    }
}

pub fn all(store: &Store) -> Result<Vec<Login>> {
    Ok(match super::vault::read(store, SLOT)? {
        Some(s) if !s.is_empty() => serde_json::from_str(&s)?,
        _ => vec![],
    })
}

fn save_all(store: &Store, items: &[Login]) -> Result<()> {
    super::vault::write(store, SLOT, &serde_json::to_string(items)?)
}

pub fn get(store: &Store, id: &str) -> Result<Login> {
    all(store)?.into_iter().find(|l| l.id == id).ok_or_else(|| anyhow!("No saved login `{id}`"))
}

/// Every saved login, for the user's own screens.
pub fn list(store: &Store) -> Result<Value> {
    Ok(json!({"items": all(store)?.iter().map(Login::public).collect::<Vec<_>>()}))
}

/// The logins on for a bot (`on`: its `BotConfig::logins`).
fn on_for(store: &Store, on: &[String]) -> Result<Vec<Login>> {
    Ok(all(store)?.into_iter().filter(|l| on.contains(&l.id)).collect())
}

/// What `list_logins` shows a bot: only the logins on for it.
pub fn list_on(store: &Store, on: &[String]) -> Result<Value> {
    Ok(json!({"items": on_for(store, on)?.iter().map(Login::public).collect::<Vec<_>>()}))
}

/// The login a bot means by `wanted` (its id, its site or its label) among those on for it.
/// Logins that aren't on for it don't exist as far as it can tell. Several on for the same
/// site: it has to pick one by id (D82). The site comes before labels, so no label (a login
/// without a username is labeled with its site) can stand in for that choice.
pub fn pick(store: &Store, on: &[String], wanted: &str) -> Result<Login> {
    let wanted = wanted.trim();
    let logins = on_for(store, on)?;
    if let Some(l) = logins.iter().find(|l| l.id == wanted) {
        return Ok(l.clone());
    }
    let site = normalize_site(wanted);
    let (mut same, others): (Vec<Login>, Vec<Login>) = logins.into_iter().partition(|l| l.site == site);
    match same.len() {
        0 => others.into_iter().find(|l| same_label(&l.title(), wanted)).ok_or_else(|| {
            anyhow!("No login `{wanted}` is on for this bot. Check list_logins, or ask for one with request_login.")
        }),
        1 => Ok(same.remove(0)),
        n => bail!(
            "{n} logins for {site} are on for this bot: {}. Pass the id of the one to use as `login`; if the task doesn't say which account, ask the user.",
            same.iter().map(|l| format!("“{}” (id `{}`)", l.title(), l.id)).collect::<Vec<_>>().join(", ")
        ),
    }
}

fn same_label(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Labels tell logins apart, so no two share one. Two default labels can only clash by case
/// (`Kevin` and `kevin`, saved before D82); those stay as they are.
fn check_label(items: &[Login], index: usize) -> Result<()> {
    let title = items[index].title();
    let custom = !items[index].label.is_empty();
    if items
        .iter()
        .enumerate()
        .any(|(i, l)| i != index && (custom || !l.label.is_empty()) && same_label(&l.title(), &title))
    {
        bail!("Another login is already called “{title}”. Give this one a label that tells them apart.");
    }
    Ok(())
}

fn clean_label(label: &str) -> Result<&str> {
    let label = label.trim();
    if label.chars().count() > MAX_LABEL {
        bail!("Keep the label under {MAX_LABEL} characters");
    }
    Ok(label)
}

/// A new login's id. Never one still kept in a bot's settings (a removed login stays there),
/// so no bot gets a login saved later under that id.
fn new_id(site: &str, taken: &[String]) -> String {
    loop {
        let id = format!("{}-{}", super::slug(site), &uuid::Uuid::new_v4().simple().to_string()[..6]);
        if !taken.contains(&id) {
            return id;
        }
    }
}

/// `https://www.GitHub.com/login` → `github.com`; an app name stays as typed.
pub fn normalize_site(site: &str) -> String {
    let s = site.trim();
    let s = s.split_once("://").map_or(s, |(_, rest)| rest);
    let s = s.split(['/', '?', '#']).next().unwrap_or(s);
    let s = s.rsplit_once('@').map_or(s, |(_, host)| host);
    s.strip_prefix("www.").unwrap_or(s).to_lowercase()
}

/// Saves a login, replacing the password of the one for the same site and username. An empty
/// `label` keeps the label it had (none for a new one: `site · username`).
pub fn save(store: &Store, site: &str, username: &str, password: &str, label: &str) -> Result<Login> {
    save_as(store, site, username, password, label, false)
}

/// Creation only (MX-277): like `save`, but NEVER replaces. The same site (normalized) and username
/// refuse with `Conflict(LOGIN_EXISTS)` and nothing is modified: password, id and the bots it is on stay.
pub fn create(store: &Store, site: &str, username: &str, password: &str, label: &str) -> Result<Login> {
    save_as(store, site, username, password, label, true)
}

fn save_as(store: &Store, site: &str, username: &str, password: &str, label: &str, create_only: bool) -> Result<Login> {
    let _exclusive = LOGINS.locked();
    let site = normalize_site(site);
    if site.is_empty() || site.len() > 200 {
        bail!("Give the website or app this login is for");
    }
    if password.is_empty() || password.len() > 4096 || username.len() > 512 {
        bail!("Invalid password");
    }
    let label = clean_label(label)?;
    let mut items = all(store)?;
    let username = username.trim().to_owned();
    let index = if let Some(i) = items.iter().position(|l| l.site == site && l.username == username) {
        if create_only {
            return Err(Conflict(LOGIN_EXISTS).into());
        }
        password.clone_into(&mut items[i].password);
        i
    } else {
        let mut taken: Vec<String> = items.iter().map(|l| l.id.clone()).collect();
        for row in store.bots()? {
            taken.extend(row.config.logins);
        }
        let id = new_id(&site, &taken);
        items.push(Login { id, site, username, label: String::new(), password: password.to_owned() });
        items.len() - 1
    };
    if !label.is_empty() {
        label.clone_into(&mut items[index].label);
    }
    check_label(&items, index)?;
    save_all(store, &items)?;
    Ok(items.swap_remove(index))
}

/// Renames a login; an empty label goes back to `site · username`.
pub fn relabel(store: &Store, id: &str, label: &str) -> Result<Login> {
    let _exclusive = LOGINS.locked();
    let label = clean_label(label)?;
    let mut items = all(store)?;
    let index = items.iter().position(|l| l.id == id).ok_or_else(|| anyhow!("No saved login `{id}`"))?;
    label.clone_into(&mut items[index].label);
    check_label(&items, index)?;
    save_all(store, &items)?;
    Ok(items.swap_remove(index))
}

pub fn remove(store: &Store, id: &str) -> Result<()> {
    let _exclusive = LOGINS.locked();
    let mut items = all(store)?;
    items.retain(|l| l.id != id);
    save_all(store, &items)
}

/// Whether the focused window belongs to the login's site: the page's host is the
/// site or one of its subdomains; without a page, the app's name is the site.
pub fn matches(site: &str, url: Option<&str>, app: &str) -> bool {
    match url.map(normalize_site).filter(|h| !h.is_empty()) {
        Some(host) => {
            let host = host.split(':').next().unwrap_or(&host).to_owned();
            host == site || host.ends_with(&format!(".{site}"))
        }
        None => !app.is_empty() && app.eq_ignore_ascii_case(site),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_normalize() {
        assert_eq!(normalize_site("https://www.GitHub.com/login?x=1"), "github.com");
        assert_eq!(normalize_site("Slack"), "slack");
        assert_eq!(normalize_site("https://user@accounts.google.com"), "accounts.google.com");
    }

    #[test]
    fn only_the_right_site_matches() {
        assert!(matches("github.com", Some("https://github.com/login"), "Safari"));
        assert!(matches("google.com", Some("https://accounts.google.com/signin"), "Chrome"));
        assert!(!matches("github.com", Some("https://github.com.evil.io/login"), "Safari"));
        assert!(!matches("github.com", Some("https://notgithub.com/"), "Safari"));
        assert!(matches("slack", None, "Slack"));
        assert!(!matches("github.com", None, "Terminal"));
        // A page URL wins over the app name.
        assert!(!matches("safari", Some("https://evil.io"), "Safari"));
    }

    fn store() -> Store {
        Store::open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn a_bot_lists_and_types_only_the_logins_on_for_it() {
        let store = store();
        let x = save(&store, "github.com", "kevin", "pw-x", "GitHub pessoal").unwrap();
        let y = save(&store, "gitlab.com", "casa", "pw-y", "").unwrap();
        let on = [x.id.clone()];
        let listed = list_on(&store, &on).unwrap().to_string();
        assert!(listed.contains("GitHub pessoal") && !listed.contains("gitlab.com"));
        assert!(!listed.contains("pw-x"));
        for wanted in [x.id.as_str(), "github.com", "https://www.github.com/login", "github pessoal"] {
            assert_eq!(pick(&store, &on, wanted).unwrap().id, x.id, "{wanted}");
        }
        // Y exists, but not for this bot: same answer as a login that doesn't exist.
        let refused = pick(&store, &on, &y.id).unwrap_err().to_string();
        assert!(refused.contains("is on for this bot") && !refused.contains("casa"));
        assert!(pick(&store, &on, "gitlab.com").is_err());
        assert!(pick(&store, &[], &x.id).is_err(), "a new bot has none");
    }

    #[test]
    fn several_logins_for_one_site_need_a_choice() {
        let store = store();
        let a = save(&store, "github.com", "kevin", "pw-a", "GitHub pessoal").unwrap();
        let b = save(&store, "github.com", "casa", "pw-b", "").unwrap();
        assert_eq!(b.title(), "github.com · casa");
        let on = [a.id.clone(), b.id.clone()];
        let asked = pick(&store, &on, "github.com").unwrap_err().to_string();
        for part in ["2 logins for github.com", "“GitHub pessoal”", &a.id, "“github.com · casa”", &b.id] {
            assert!(asked.contains(part), "{asked}");
        }
        assert!(!asked.contains("pw-"));
        // A login without a username is labeled with its site: still a choice to make.
        let c = save(&store, "github.com", "", "pw-c", "").unwrap();
        let all = [a.id.clone(), b.id.clone(), c.id.clone()];
        assert!(pick(&store, &all, "github.com").unwrap_err().to_string().starts_with("3 logins"));
        assert_eq!(pick(&store, &on, &b.id).unwrap().id, b.id);
        assert_eq!(pick(&store, &on, "GitHub pessoal").unwrap().id, a.id);
        // Labels tell them apart, so two can't share one; saving the same account again keeps its id.
        assert!(save(&store, "github.com", "other", "pw-c", "github pessoal").is_err());
        assert!(relabel(&store, &b.id, "GitHub Pessoal").is_err());
        assert_eq!(save(&store, "github.com", "casa", "pw-new", "GitHub Casa").unwrap().id, b.id);
        assert_eq!(pick(&store, &on, "GitHub Casa").unwrap().id, b.id);
    }

    fn bot_with_login(store: &Store, id: &str, login: &str) {
        let cfg: crate::store::BotConfig = serde_json::from_value(json!({
            "id": id, "name": id, "backend": "fixture", "cwd": std::env::temp_dir(), "command": "unused", "notify": false,
            "logins": [login],
        }))
        .unwrap();
        store.save_bot(&cfg).unwrap();
    }

    #[test]
    fn creation_only_never_replaces_an_existing_login_nor_touches_its_bots() {
        let store = store();
        let old = save(&store, "https://www.Painel.exemplo.com/entrar", " ana ", "pw-antiga", "Painel da Ana").unwrap();
        bot_with_login(&store, "bot-b", &old.id);
        for (site, user) in [
            ("painel.exemplo.com", "ana"),
            ("HTTPS://painel.exemplo.com/outra", " ana "),
            ("www.painel.exemplo.com", "ana"),
        ] {
            let err = create(&store, site, user, "pw-nova", "Outro nome").unwrap_err();
            let conflict = err.downcast_ref::<Conflict>().expect("a collision is a 409 Conflict");
            assert_eq!(conflict.0, LOGIN_EXISTS, "{site}");
        }
        // Nothing changed: same id, same password, same label, still on for the other bot.
        let items = all(&store).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            (items[0].id.as_str(), items[0].password.as_str(), items[0].label.as_str()),
            (old.id.as_str(), "pw-antiga", "Painel da Ana")
        );
        assert_eq!(store.bot("bot-b").unwrap().unwrap().config.logins, std::slice::from_ref(&old.id));
        // A different username (or site) is a new login: new id, no bot gets it, the old one is intact.
        let novo = create(&store, "painel.exemplo.com", "bia", "pw-bia", "").unwrap();
        assert_ne!(novo.id, old.id);
        assert_eq!(get(&store, &old.id).unwrap().password, "pw-antiga");
        assert_eq!(store.bot("bot-b").unwrap().unwrap().config.logins, std::slice::from_ref(&old.id));
        // The normal path (Extensões → Credenciais) is unchanged: it still replaces, keeping the id and the bots.
        assert_eq!(save(&store, "painel.exemplo.com", "ana", "pw-trocada", "").unwrap().id, old.id);
        assert_eq!(get(&store, &old.id).unwrap().password, "pw-trocada");
        assert_eq!(store.bot("bot-b").unwrap().unwrap().config.logins, [old.id]);
    }

    #[test]
    fn creation_only_is_checked_and_written_under_one_exclusion() {
        // Many creators race for the same site and username: exactly one wins, the rest get the 409,
        // and the stored login is the winner's (never a mix, never a replacement).
        let store = std::sync::Arc::new(store());
        let results: Vec<Result<Login>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|n| {
                    let store = store.clone();
                    scope.spawn(move || create(&store, "corrida.exemplo.com", "ana", &format!("pw-{n}"), ""))
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let winners: Vec<&Login> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
        assert_eq!(winners.len(), 1, "exactly one creation wins");
        for r in results.iter().filter(|r| r.is_err()) {
            assert_eq!(r.as_ref().unwrap_err().downcast_ref::<Conflict>().unwrap().0, LOGIN_EXISTS);
        }
        let stored = all(&store).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].password, winners[0].password);
        assert_eq!(stored[0].id, winners[0].id);
    }
}
