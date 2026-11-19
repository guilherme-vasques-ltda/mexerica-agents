//! What a bot is told about itself, frozen per context window: Grok Bot's
//! "prompt snapshots".
//!
//! The bot's instructions (name, description, skills, memory) are rendered once
//! and kept as a [`Snapshot`] keyed by the ACP session and its *compaction
//! epoch*. How it reaches the agent depends on the harness (`agent::bot::Instructions`).
//! The snapshot stays byte-identical, so the prompt cache stays warm, until the
//! agent compacts its context (a `compaction_update`) or a new session starts.
//! Profile edits in between are announced once as an `<agent_profile_update>`
//! block on the next message and folded into the snapshot after the next
//! compaction. Memory changes in between are told on the next message the same way
//! (see `memory::change_notice`).

use crate::agent::bot::Instructions;
use crate::store::{BotConfig, Store};
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// The snapshot envelope's version. A snapshot without it, or with an earlier one, never told the agent the profile as
/// it reads now, so a session carrying one gets the profile again when it loads (see `predates`).
const VERSION: u32 = 5;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[serde(default)]
    memory_policy: Option<crate::fork::bot_admission::MemoryPolicy>,
    /// Versão do envelope (`VERSION`); descarta o manual duplicado dos snapshots anteriores sem apagar a sessão.
    #[serde(default)]
    version: u32,
    #[serde(default)]
    native_manual: bool,
    pub session: String,
    pub epoch: u32,
    /// The rendered instructions, exactly as the agent got them.
    pub system: String,
    /// The profile rendered into `system`, as the model reads it (see `profile_text`).
    pub system_profile: String,
    /// The profile the agent last heard (`system` or a later update block), as the model reads it.
    pub announced: String,
    /// The memory revision `system` holds (`memory::current_revision` before it was rendered).
    #[serde(default)]
    pub memory: String,
    /// D6: a pessoa da conversa dona deste snapshot — `system` pode conter a nota "Sobre você" dela.
    #[serde(default)]
    pub person: Option<String>,
}

// A bot talks in several sessions (its chat and groups, routine runs), each with its own: snapshots
// and epochs are kept per session.
fn key(bot_id: &str, session: &str) -> String {
    format!("context.{bot_id}.{session}")
}

pub(crate) fn epoch_key(bot_id: &str, session: &str) -> String {
    format!("context.epoch.{bot_id}.{session}")
}

/// `(epoch, last compaction id)` stored as `session \t epoch \t id`.
fn epoch_state(store: &Store, bot_id: &str, session: &str) -> (u32, String) {
    store
        .kv_get(&epoch_key(bot_id, session))
        .and_then(|v| {
            let mut parts = v.splitn(3, '\t');
            (parts.next()? == session).then(|| {
                (parts.next().and_then(|n| n.parse().ok()).unwrap_or(0), parts.next().unwrap_or("").to_owned())
            })
        })
        .unwrap_or_default()
}

/// How many times the agent has compacted `session`.
pub fn epoch(store: &Store, bot_id: &str, session: &str) -> u32 {
    epoch_state(store, bot_id, session).0
}

/// Counts a finished compaction once (adapters may repeat its terminal update).
/// Returns whether it was new.
pub fn bump_epoch(store: &Store, bot_id: &str, session: &str, compaction_id: &str) -> Result<bool> {
    let (n, last) = epoch_state(store, bot_id, session);
    if !compaction_id.is_empty() && compaction_id == last {
        return Ok(false);
    }
    store.kv_set(&epoch_key(bot_id, session), &format!("{session}\t{}\t{compaction_id}", n + 1))?;
    Ok(true)
}

fn load(store: &Store, bot_id: &str, session: &str) -> Option<Snapshot> {
    store.kv_get(&key(bot_id, session)).and_then(|v| serde_json::from_str(&v).ok())
}

fn save(store: &Store, bot_id: &str, s: &Snapshot) -> Result<()> {
    store.kv_set(&key(bot_id, &s.session), &serde_json::to_string(s)?)
}

/// The snapshot for `session` at its current epoch, rendering a fresh one when
/// the stored one belongs to another session or an earlier epoch.
#[cfg(test)]
pub fn resolve(store: &Store, cfg: &BotConfig, session: &str, instructions: Instructions) -> Result<Snapshot> {
    resolve_with_skills(store, cfg, session, instructions, || Ok(crate::market::skills_brief(store, &cfg.skills)), None)
}

/// `query` is the turn's request: it only decides which memories fit when the opening is over its cap.
pub(crate) fn resolve_with_skills(
    store: &Store,
    cfg: &BotConfig,
    session: &str,
    instructions: Instructions,
    skills: impl FnOnce() -> Result<Option<String>>,
    query: Option<&str>,
) -> Result<Snapshot> {
    let epoch = epoch(store, cfg.session_owner(), session);
    let stored = load(store, cfg.session_owner(), session);
    if let Some(s) = stored.as_ref().filter(|s| {
        s.version == VERSION
            && s.memory_policy.as_ref() == Some(&crate::fork::bot_admission::MemoryPolicy::of_config(cfg))
            && s.native_manual == instructions.native_manual()
            && s.epoch == epoch
            && s.person == cfg.person
            && !s.system.is_empty()
    }) {
        return Ok(s.clone());
    }
    let skills = skills()?;
    let memory = crate::chat::memory::current_revision(store, cfg.memory_id());
    let rendered = render_with_skills(store, cfg, instructions, skills, query);
    let profile = rendered.system_profile;
    let s = Snapshot {
        memory_policy: Some(crate::fork::bot_admission::MemoryPolicy::of_config(cfg)),
        version: VERSION,
        native_manual: instructions.native_manual(),
        session: session.to_owned(),
        epoch,
        system: rendered.system,
        system_profile: profile.clone(),
        // A re-render reaches the agent only as Claude's system prompt or a re-sent profile (see `mark_announced`):
        // until then it knows the profile it last heard, so an update it missed is still announced.
        announced: stored.map_or(profile, |old| old.announced),
        memory,
        person: cfg.person.clone(),
    };
    save(store, cfg.session_owner(), &s)?;
    Ok(s)
}

/// One session's instructions are rendered again on its next turn: before a compaction the host starts, so the
/// compaction keeps them as they are now.
pub fn refresh(store: &Store, bot_id: &str, session: &str) -> Result<()> {
    match load(store, bot_id, session) {
        Some(mut s) => {
            s.system.clear();
            save(store, bot_id, &s)
        }
        None => Ok(()),
    }
}

/// Every session of the bot renders its instructions again on its next turn (after the move from the old memory).
pub fn invalidate(store: &Store, bot_id: &str) -> Result<()> {
    for k in store.kv_prefix(&format!("context.{bot_id}.")) {
        match store.kv_get(&k).and_then(|v| serde_json::from_str::<Snapshot>(&v).ok()) {
            Some(mut s) => {
                s.system.clear();
                save(store, bot_id, &s)?;
            }
            None => store.kv_set(&k, "")?,
        }
    }
    Ok(())
}

/// Records `system` (already handed to a new session), rendered at memory revision `memory`, as that session's
/// snapshot.
pub fn adopt(
    store: &Store,
    cfg: &BotConfig,
    session: &str,
    rendered: RenderedProfile,
    memory: String,
    instructions: Instructions,
) -> Result<Snapshot> {
    let profile = rendered.system_profile;
    let s = Snapshot {
        memory_policy: Some(crate::fork::bot_admission::MemoryPolicy::of_config(cfg)),
        version: VERSION,
        native_manual: instructions.native_manual(),
        session: session.to_owned(),
        epoch: epoch(store, cfg.session_owner(), session),
        system: rendered.system,
        system_profile: profile.clone(),
        announced: profile,
        memory,
        person: cfg.person.clone(),
    };
    save(store, cfg.session_owner(), &s)?;
    Ok(s)
}

/// Whether `session`'s stored snapshot predates the current envelope, or none is stored: the agent never heard this
/// profile in its current form, so the next message carries it again.
pub fn predates(store: &Store, bot_id: &str, session: &str) -> bool {
    store
        .kv_get(&key(bot_id, session))
        .and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok())
        .and_then(|s| s["version"].as_u64())
        .is_none_or(|v| v < u64::from(VERSION))
}

/// The bot's instructions, rendered live.
#[cfg(test)]
pub fn render(store: &Store, cfg: &BotConfig, instructions: Instructions) -> String {
    render_with_skills(store, cfg, instructions, crate::market::skills_brief(store, &cfg.skills), None).system
}

/// Par inseparável de uma montagem; não contém corpos de documentos de Skills.
pub(crate) struct RenderedProfile {
    pub system: String,
    pub system_profile: String,
}

pub(crate) fn render_with_skills(
    store: &Store,
    cfg: &BotConfig,
    instructions: Instructions,
    skills: Option<String>,
    query: Option<&str>,
) -> RenderedProfile {
    let profile_lines = profile(store, cfg);
    let system_profile = profile_with_lines(profile_lines.clone(), skills.as_deref());
    let mut lines = Vec::new();
    if !instructions.native_manual() {
        lines.push(crate::agent::harness::compile());
    }
    lines.extend(profile_lines);
    lines.push(String::new());
    if let Some(skills) = skills {
        lines.push(String::new());
        lines.push(skills);
    }
    let mut out = lines.join("\n");
    let memory = crate::chat::memory::opening_frozen(store, cfg, query);
    if !memory.is_empty() {
        out.push_str("\n\n");
        out.push_str(&memory);
    }
    RenderedProfile { system: out, system_profile }
}

/// O mesmo perfil é usado na abertura e nas atualizações, sem repetir o manual estável.
fn profile(store: &Store, cfg: &BotConfig) -> Vec<String> {
    let name = if cfg.name.trim().is_empty() { "Bot" } else { cfg.name.trim() };
    let mut lines = vec![
        "Agent profile:".to_owned(),
        crate::fork::bot_editor::authorization::INSTRUCTIONS.to_owned(),
        format!("Current name: {name}"),
        format!("Description: {}", crate::fork::donna_fixa::description(store, cfg)),
    ];
    lines.extend(crate::fork::donna_fixa::preferences_line(store, cfg));
    lines.extend(crate::fork::nunca_fazer::prompt_line(store, cfg));
    lines.extend([
        format!(
            "Workspace: your default working directory is {}. {} Keep your own files and outputs there. For another project, use explicit paths or change directory for that command; don't change your bot configuration to work elsewhere. This directory is not a sandbox or a grant of access: follow the user's instructions and your permissions, and don't assume other bots' files are yours.",
            cfg.cwd,
            if crate::agent::workspace::is_managed(cfg) {
                "Mexerica allocated it for you alone."
            } else {
                "The user chose this project folder."
            }
        ),
        format!(
            "Configuração do bot: gestão {}. Uso do computador {}. Conectores selecionados: {}. Essas opções não anunciam ferramentas por si só; confira os MCPs efetivamente disponíveis e autenticados nesta conversa.",
            crate::chat::manage::enabled(store, cfg),
            cfg.computer,
            cfg.connectors.len()
        ),
    ]);
    lines
}

/// Configuração do Hub em <dados>/harness-context.json, carregada no início do host.
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HarnessContext {
    #[serde(default)]
    owner_name: String,
    time_zone: Option<String>,
}

static HARNESS_CONTEXT: std::sync::OnceLock<HarnessContext> = std::sync::OnceLock::new();

pub async fn load_harness_context() {
    let file = crate::service::data_dir().join("harness-context.json");
    let cfg = match tokio::fs::read(file).await {
        Ok(bytes) => match serde_json::from_slice::<HarnessContext>(&bytes) {
            Ok(cfg) => cfg,
            Err(error) => {
                tracing::warn!(%error, "invalid harness-context.json; using neutral defaults");
                HarnessContext::default()
            }
        },
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(%error, "couldn't read harness-context.json; using neutral defaults");
            }
            HarnessContext::default()
        }
    };
    if let Some(zone) = cfg.time_zone.as_deref()
        && zone.parse::<chrono_tz::Tz>().is_err()
    {
        tracing::warn!("invalid time zone in harness-context.json; using Hub local time");
    }
    let _ = HARNESS_CONTEXT.set(cfg);
}

/// Fuso do dono (`harness-context.json`), quando configurado. O silêncio do `WhatsApp` usa este.
pub(crate) fn owner_time_zone() -> Option<chrono_tz::Tz> {
    HARNESS_CONTEXT
        .get_or_init(HarnessContext::default)
        .time_zone
        .as_deref()
        .and_then(|s| s.parse::<chrono_tz::Tz>().ok())
}

pub fn turn_context() -> String {
    context_at(HARNESS_CONTEXT.get_or_init(HarnessContext::default), chrono::Utc::now())
}

fn context_at(cfg: &HarnessContext, now: chrono::DateTime<chrono::Utc>) -> String {
    let owner = cfg.owner_name.trim();
    let owner = if owner.is_empty() { "usuário desta instalação" } else { owner };
    let zone = cfg.time_zone.as_deref().and_then(|s| s.parse::<chrono_tz::Tz>().ok());
    let time = zone.map_or_else(
        || now.with_timezone(&chrono::Local).to_rfc3339(),
        |tz| format!("{} ({tz})", now.with_timezone(&tz).to_rfc3339()),
    );
    format!(
        "<system-reminder>\nDono da instalação: {owner}.\nData e hora atuais: {time}.\nPlataforma do Hub: {} / {}. Computador dos bots da Mexerica (o Hub).\n</system-reminder>",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// The profile as the model reads it: the profile lines and the skills, as text. A change is announced only when this
/// text changes, so one the model can't see (a connector swapped for another, which the profile only counts) isn't.
#[cfg(test)]
fn profile_text(store: &Store, cfg: &BotConfig) -> String {
    let skills = crate::market::skills_brief(store, &cfg.skills);
    profile_with_skills(store, cfg, skills.as_deref())
}

fn profile_with_skills(store: &Store, cfg: &BotConfig, skills: Option<&str>) -> String {
    profile_with_lines(profile(store, cfg), skills)
}

fn profile_with_lines(mut lines: Vec<String>, skills: Option<&str>) -> String {
    lines.push(skills.unwrap_or("Skills: none.").into());
    lines.join("\n")
}

/// The `<agent_profile_update>` block to append to the next message when the profile the model reads changed since
/// the agent last heard it. Returns the block and the profile text it announces.
#[cfg(test)]
pub fn profile_update(store: &Store, snapshot: &Snapshot, cfg: &BotConfig) -> Option<(String, String)> {
    profile_update_text(snapshot, profile_text(store, cfg))
}

pub(crate) fn profile_update_with_skills(
    store: &Store,
    snapshot: &Snapshot,
    cfg: &BotConfig,
    skills: Option<&str>,
) -> Option<(String, String)> {
    profile_update_text(snapshot, profile_with_skills(store, cfg, skills))
}

fn profile_update_text(snapshot: &Snapshot, text: String) -> Option<(String, String)> {
    if text == snapshot.announced {
        return None;
    }
    let update = [
        "<agent_profile_update>",
        "Your agent profile changed. This full update is authoritative and supersedes the Agent profile section and every earlier profile update in this conversation.",
        text.as_str(),
        "</agent_profile_update>",
    ]
    .join("\n");
    Some((update, text))
}

/// After the turn that carried an update reached the agent: remember it heard `announced`, unless the snapshot moved
/// on meanwhile (a compaction re-rendered it).
pub fn mark_announced(store: &Store, bot_id: &str, turn_snapshot: &Snapshot, announced: String) -> Result<()> {
    let Some(mut current) = load(store, bot_id, &turn_snapshot.session) else { return Ok(()) };
    if current.session != turn_snapshot.session
        || current.epoch != turn_snapshot.epoch
        || current.system != turn_snapshot.system
        || current.announced == announced
    {
        return Ok(());
    }
    current.announced = announced;
    save(store, bot_id, &current)
}

#[cfg(test)]
mod tests {
    fn adopt(
        store: &Store,
        cfg: &BotConfig,
        session: &str,
        system: String,
        memory: String,
        instructions: Instructions,
    ) -> Result<Snapshot> {
        super::adopt(
            store,
            cfg,
            session,
            RenderedProfile { system, system_profile: profile_text(store, cfg) },
            memory,
            instructions,
        )
    }

    use super::*;

    fn cfg(name: &str, description: &str) -> BotConfig {
        serde_json::from_value(serde_json::json!({
            "id": "5f0c7a52-0000-4000-8000-000000000001", "name": name, "description": description,
            "backend": "claude", "cwd": "/tmp",
        }))
        .expect("valid bot")
    }

    fn store() -> (Store, std::path::PathBuf) {
        let dir = crate::store::private_fixture_dir("codync-context");
        let store = Store::open(&dir.join("context.db")).expect("temp store");
        (store, dir)
    }

    #[test]
    fn contexto_dinamico_respeita_fuso_dono_e_virada_do_dia() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-21T01:30:00Z").unwrap().to_utc();
        let cfg = HarnessContext { owner_name: "Pessoa".into(), time_zone: Some("America/Sao_Paulo".into()) };
        let text = context_at(&cfg, now);
        assert!(text.contains("2026-10-20T22:30:00-03:00 (America/Sao_Paulo)"));
        assert!(text.contains("Dono da instalação: Pessoa"));
        assert!(text.contains(std::env::consts::OS));
        assert!(context_at(&HarnessContext::default(), now).contains("usuário desta instalação"));
    }

    #[test]
    fn perfil_de_capacidades_muda_sem_alterar_manual_nativo_ou_snapshot() {
        let (store, dir) = store();
        let a = cfg("Teste", "Teste");
        let first = adopt(
            &store,
            &a,
            "s1",
            render(&store, &a, Instructions::ClaudeMeta),
            String::new(),
            Instructions::ClaudeMeta,
        )
        .unwrap();
        assert!(!first.system.contains("Manual do harness v0"));
        let mut b = a.clone();
        b.connectors.push("mcp".into());
        b.manage_team = true;
        store.kv_set("team.settings.v1", &serde_json::json!({"primaryBotId":b.id}).to_string()).unwrap();
        b.computer = true;
        let (update, text) = profile_update(&store, &first, &b).unwrap();
        assert!(
            update.contains("Configuração do bot: gestão true. Uso do computador true. Conectores selecionados: 1")
        );
        assert!(update.contains("Essas opções não anunciam ferramentas por si só"));
        mark_announced(&store, &a.id, &first, text).unwrap();
        let current = resolve(&store, &b, "s1", Instructions::ClaudeMeta).unwrap();
        assert_eq!(first.system, current.system);
        assert!(profile_update(&store, &current, &b).is_none());
        drop(store);
        std::fs::remove_dir_all(dir).expect("limpeza da fixture privada");
    }

    #[test]
    fn perfil_nao_anuncia_manage_por_flag_legado_sem_principal() {
        let (store, dir) = store();
        let mut bot = cfg("Teste", "Teste");
        bot.manage_team = true;
        assert!(profile_text(&store, &bot).contains("Configuração do bot: gestão false"));
        store.kv_set("team.settings.v1", &serde_json::json!({"primaryBotId":bot.id}).to_string()).unwrap();
        assert!(profile_text(&store, &bot).contains("Configuração do bot: gestão true"));
        drop(store);
        std::fs::remove_dir_all(dir).expect("limpeza da fixture privada");
    }

    #[test]
    fn sessao_de_envelope_antigo_recebe_o_perfil_de_novo() {
        let (store, dir) = store();
        let a = cfg("Fixer", "Fix bugs");
        assert!(predates(&store, &a.id, "s1"), "sem snapshot, a sessão nunca ouviu este perfil");
        adopt(&store, &a, "s1", "SYSTEM v4".into(), String::new(), Instructions::ClaudeMeta).unwrap();
        assert!(!predates(&store, &a.id, "s1"));
        // Snapshot anterior ao envelope atual, do jeito que a Mexerica gravava antes.
        store
            .kv_set(&key(&a.id, "s1"), r#"{"session":"s1","epoch":0,"system":"SYSTEM v3","announced":{},"memory":""}"#)
            .unwrap();
        assert!(predates(&store, &a.id, "s1"));
        drop(store);
        std::fs::remove_dir_all(dir).expect("limpeza da fixture privada");
    }

    #[test]
    fn trocar_um_conector_por_outro_nao_anuncia_perfil_que_o_modelo_nao_ve() {
        let (store, dir) = store();
        let mut a = cfg("Teste", "Teste");
        a.connectors = vec!["gmail".into()];
        let first = adopt(&store, &a, "s1", String::new(), String::new(), Instructions::ClaudeMeta).unwrap();
        let mut b = a.clone();
        b.connectors = vec!["slack".into()];
        assert!(profile_update(&store, &first, &b).is_none(), "o perfil mostra só a contagem de conectores");
        drop(store);
        std::fs::remove_dir_all(dir).expect("limpeza da fixture privada");
    }

    #[test]
    fn instructions_distinguish_ask_replies_from_independent_user_reports() {
        let system = crate::agent::harness::compile();
        assert!(system.contains("ask_bot"));
        assert!(system.contains("message_bot"));
        assert!(system.contains("send_message is available"));
        assert!(system.contains("These rules apply to the current turn"));
    }

    #[test]
    fn snapshot_is_frozen_until_compaction_and_updates_are_announced_once() {
        let (store, dir) = store();
        let a = cfg("Fixer", "Fix bugs");
        let first =
            adopt(&store, &a, "s1", "SYSTEM v1".into(), String::new(), Instructions::ClaudeMeta).expect("adopt");
        // Frozen: resolving again returns the same text, even though a live render differs.
        assert_eq!(resolve(&store, &a, "s1", Instructions::ClaudeMeta).expect("resolve").system, "SYSTEM v1");
        assert!(profile_update(&store, &first, &a).is_none());

        let b = cfg("Fixer", "Fix bugs and write tests");
        let (text, announced) = profile_update(&store, &first, &b).expect("changed");
        assert!(text.contains("Description: Fix bugs and write tests"));
        mark_announced(&store, &a.id, &first, announced).expect("mark");
        let snap = resolve(&store, &b, "s1", Instructions::ClaudeMeta).expect("resolve");
        assert_eq!(snap.system, "SYSTEM v1");
        assert!(profile_update(&store, &snap, &b).is_none());

        // A compaction folds the new identity into a fresh render.
        assert!(bump_epoch(&store, &a.id, "s1", "c1").expect("bump"));
        assert!(!bump_epoch(&store, &a.id, "s1", "c1").expect("bump"));
        let folded = resolve(&store, &b, "s1", Instructions::ClaudeMeta).expect("resolve");
        assert_eq!(folded.epoch, 1);
        assert!(folded.system.contains("Description: Fix bugs and write tests"));
        // A new session starts over at epoch 0.
        assert_eq!(epoch(&store, &a.id, "s2"), 0);
        drop(store);
        std::fs::remove_dir_all(dir).expect("limpeza da fixture privada");
    }

    #[test]
    fn a_memory_change_re_renders_but_keeps_what_the_agent_heard() {
        let (store, dir) = store();
        let a = cfg("Fixer", "Fix bugs");
        adopt(&store, &a, "s1", "SYSTEM v1".into(), String::new(), Instructions::ClaudeMeta).expect("adopt");
        let b = cfg("Fixer", "Fix bugs and write tests");
        invalidate(&store, &a.id).expect("invalidate");
        let snap = resolve(&store, &b, "s1", Instructions::ClaudeMeta).expect("resolve");
        assert!(snap.system.contains("Description: Fix bugs and write tests"), "re-rendered");
        assert!(profile_update(&store, &snap, &b).is_some(), "the agent never heard the new description");
        drop(store);
        std::fs::remove_dir_all(dir).expect("limpeza da fixture privada");
    }

    #[test]
    fn um_pedido_novo_nao_mexe_no_snapshot_congelado() {
        let (store, dir) = store();
        let a = cfg("Fixer", "Fix bugs");
        // The bot layer is over its cap; only the request decides which memories fit.
        store
            .memory_write(|c, now, rev| {
                codync_mem::store::add(
                    c,
                    codync_mem::store::Writer::User,
                    codync_mem::store::New {
                        bot: Some(a.memory_id()),
                        text: "supplier invoices always arrive on the 10th",
                        room: None,
                        evidence: &[],
                    },
                    now,
                    rev,
                )?;
                for i in 0..130 {
                    codync_mem::store::add(
                        c,
                        codync_mem::store::Writer::User,
                        codync_mem::store::New {
                            bot: Some(a.memory_id()),
                            text: &format!("unrelated note number {i} about the weather"),
                            room: None,
                            evidence: &[],
                        },
                        now,
                        rev,
                    )?;
                }
                Ok(())
            })
            .expect("memory");
        let first = resolve_with_skills(
            &store,
            &a,
            "s1",
            Instructions::ClaudeMeta,
            || Ok(None),
            Some("when do supplier invoices arrive?"),
        )
        .expect("resolve");
        assert!(first.system.contains("supplier invoices"), "the request's fact is in");
        // The next turn asks something unrelated: the frozen snapshot does not re-render.
        let second = resolve_with_skills(
            &store,
            &a,
            "s1",
            Instructions::ClaudeMeta,
            || Ok(None),
            Some("qual a senha do wifi da sala?"),
        )
        .expect("resolve");
        assert_eq!(second.system, first.system, "the snapshot stays frozen across requests");
        drop(store);
        std::fs::remove_dir_all(dir).expect("limpeza da fixture privada");
    }
}
