// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Automatic bot names, as in Grok Bot: a bot created without a name is called
//! [`PLACEHOLDER`] until its first few conversations show what it's for. Then
//! a one-shot agent of the bot's harness (see `oneshot`) is asked for a short
//! name, at the end of a turn. The user renaming it, or asking the bot to rename itself
//! (`profile`), ends this. The description stays the user's: it holds standing instructions.

use crate::LockExt;
use crate::hub::Hub;
use crate::store::Store;
use anyhow::Result;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

/// A finished user ↔ bot exchange.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Turn {
    pub user: String,
    pub agent: String,
    /// MX-280 (revisão R3): instante do turno em ms. Duas filas drenam para a
    /// mesma janela no desligamento e a ordem de gravação não é cronológica —
    /// sem o carimbo, o turno mais recente podia ser o primeiro a cair.
    /// `default` trata registros antigos como os mais velhos da janela.
    #[serde(default)]
    pub at: i64,
}

pub const PLACEHOLDER: &str = "New Bot";
/// Conversations before the first naming attempt.
const NAME_AFTER: usize = 3;
/// The most recent conversations shown to the namer (it retries after each one).
const WINDOW: usize = 6;
pub const MAX_NAME_CHARS: usize = 40;

fn key(bot_id: &str) -> String {
    format!("naming.turns.{bot_id}")
}

fn pending(store: &Store, bot_id: &str) -> Vec<Turn> {
    store.kv_get(&key(bot_id)).and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
}

fn push_pending(store: &Store, bot_id: &str, turn: Turn) -> Result<Vec<Turn>> {
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let raw: Option<String> = tx.query_row("SELECT v FROM kv WHERE k=?1", [key(bot_id)], |r| r.get(0)).optional()?;
    let mut turns: Vec<Turn> = raw.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
    turns.push(turn);
    // Ordem cronológica, não de gravação: drenagens concorrentes do desligamento
    // podem persistir o turno mais novo antes dos antigos (sort estável preserva
    // a fila entre carimbos iguais ou ausentes).
    turns.sort_by_key(|t| t.at);
    let skip = turns.len().saturating_sub(WINDOW);
    turns.drain(..skip);
    tx.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        (key(bot_id), serde_json::to_string(&turns)?),
    )?;
    tx.commit()?;
    Ok(turns)
}

/// MX-280: grava o turno na janela de nomeação sem chamar o modelo — o caminho
/// de drenagem do desligamento, para um turno concluído não sumir da janela.
pub(crate) fn record_pending(store: &Store, bot_id: &str, turn: Turn) -> Result<()> {
    if !store.bot(bot_id)?.is_some_and(|b| !b.deleted && b.config.auto_name) {
        return Ok(());
    }
    push_pending(store, bot_id, turn).map(|_| ())
}

/// Records a finished conversation and, once there are enough, names the bot.
pub async fn observe(hub: &Arc<Hub>, bot_id: &str, turn: Turn) -> Result<()> {
    let Some(cfg) = hub.store.bot(bot_id)?.filter(|b| !b.deleted && b.config.auto_name).map(|b| b.config) else {
        return Ok(());
    };
    let turns = push_pending(&hub.store, bot_id, turn)?;
    if turns.len() < NAME_AFTER {
        return Ok(());
    }
    try_pending_with(hub, &cfg, &turns).await
}

pub(crate) async fn retry_pending(hub: &Arc<Hub>, bot_id: &str) -> Result<()> {
    let Some(cfg) = hub.store.bot(bot_id)?.filter(|b| !b.deleted && b.config.auto_name).map(|b| b.config) else {
        return Ok(());
    };
    let turns = pending(&hub.store, bot_id);
    if turns.len() < NAME_AFTER {
        return Ok(());
    }
    try_pending_with(hub, &cfg, &turns).await
}

async fn try_pending_with(hub: &Arc<Hub>, cfg: &crate::store::BotConfig, turns: &[Turn]) -> Result<()> {
    let bot_id = &cfg.id;
    if hub.routines.stopping() {
        return Ok(());
    }
    if !crate::fork::background_notice::supported(hub, cfg, "nome") {
        return Ok(());
    }
    let mut raw = crate::chat::oneshot::one_shot(hub, cfg, SYSTEM_PROMPT, &user_prompt(turns)).await?;
    crate::fork::background_notice::recovered(hub, cfg, "nome");
    let Some(name) = parse(&raw.text) else {
        tracing::info!(bot = %bot_id, "purpose not clear yet; naming later");
        raw.finish(crate::fork::background_history::Status::Unchanged);
        return Ok(());
    };
    let changed = match apply_name_with_selection(hub, bot_id, &name, Some(&raw.selection)).await {
        Ok(changed) => changed,
        Err(error) => {
            if error
                .downcast_ref::<crate::fork::background_notice::Problem>()
                .is_some_and(|reason| matches!(reason, crate::fork::background_notice::Problem::AccountChanged))
            {
                raw.fail_revalidation();
            }
            return Err(error);
        }
    };
    raw.finish(if changed {
        crate::fork::background_history::Status::Applied
    } else {
        crate::fork::background_history::Status::Unchanged
    });
    Ok(())
}

/// Aplica o resultado tardio do nomeador sob a mesma exclusão da promoção da principal.
#[cfg(test)]
pub(crate) async fn apply_name(hub: &Arc<Hub>, bot_id: &str, name: &str) -> Result<()> {
    apply_name_with_selection(hub, bot_id, name, None).await.map(|_| ())
}

async fn apply_name_with_selection(
    hub: &Arc<Hub>,
    bot_id: &str,
    name: &str,
    selection: Option<&crate::fork::background_selection::Resolved>,
) -> Result<bool> {
    let name = crate::fork::bot_editor::checked_name(name)?;
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let _accounts = crate::fork::engine_accounts::MUTATION.lock().await;
    if let Some(selection) = selection {
        crate::fork::background_selection::revalidate(&hub.store, selection)?;
    }
    crate::chat::manage::recover_live(hub);
    if hub.store.team_settings()?.primary_bot_id.as_deref() == Some(bot_id) {
        return Ok(false);
    }
    // The user may have renamed it while the namer ran.
    if !hub.store.bot(bot_id)?.is_some_and(|b| !b.deleted && b.config.auto_name) {
        return Ok(false);
    }
    let patch = json!({"id": bot_id, "name": name, "autoName": false});
    let hub2 = hub.clone();
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        // MX-280: closure destacada não pode gravar depois do sinal de parada.
        let _write = hub2.memory_write()?;
        hub2.update_bot(&patch)
    })
    .await??;
    hub.store.kv_set(&key(bot_id), "[]")?;
    tracing::info!(bot = %bot_id, "named from its conversations");
    Ok(true)
}

const SYSTEM_PROMPT: &str = "\
You name a personal assistant bot after its first conversations with the user.
Read the conversations and work out what the user relies on this bot for.
Output exactly one line:
name: <a short name for the bot, 1-4 words, saying what it does, like a job title or a team nickname>
Write it in the language and script the user writes in (Traditional and Simplified Chinese are different). No quotes, no emoji, no trailing punctuation.
Output exactly NONE if the conversations don't show a clear purpose yet (only greetings, tests or small talk).";

fn user_prompt(turns: &[Turn]) -> String {
    let body = turns
        .iter()
        .map(|t| format!("User: {}\nBot: {}", t.user.trim(), t.agent.trim()))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("Conversations so far, oldest first:\n\n{body}")
}

fn parse(raw: &str) -> Option<String> {
    let name = raw.lines().find_map(|line| {
        let (tag, rest) = line.trim().trim_start_matches(['-', '*', ' ']).split_once(':')?;
        tag.trim().eq_ignore_ascii_case("name").then(|| rest.trim().trim_matches(['"', '“', '”', '「', '」']))
    })?;
    let name = name.trim_end_matches(['.', '。']).trim();
    (!name.is_empty() && !name.eq_ignore_ascii_case("none") && name.chars().count() <= MAX_NAME_CHARS)
        .then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn revalidacao_impede_nome_se_a_selecao_mudou_apos_a_resposta() {
        use crate::fork::modelos_funcao::{catalogo::FunctionId, config};
        use crate::remote::identity::Identity;
        let dir = std::env::temp_dir().join(format!("fundo-nome-revisao-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let id = format!("ac_{}", "a".repeat(64));
        store
            .kv_set(
                "fork.engine-accounts.v1",
                &json!({"salt":uuid::Uuid::new_v4().to_string(), "accounts":[{
                    "id":id,"backend":"claude","billing":"subscription","state":"connected",
                    "launch":{"kind":"cli_profile","executable":"/bin/true","directory":"/tmp"},
                    "scope":"owner","quotaIdentity":id,"verifiedAt":1,
                    "subscriptionProof":{"method":"claude_auth_status","plan":"max"},
                    "models":[{"id":"haiku","options":[]}]
                }]})
                .to_string(),
            )
            .unwrap();
        let bot: crate::store::BotConfig = serde_json::from_value(json!({"id":"b", "name":"Original",
            "backend":"claude", "autoName":true}))
        .unwrap();
        store.save_bot(&bot).unwrap();
        config::initialize(&store).unwrap();
        let selection = crate::fork::background_selection::resolve(&store, &bot, FunctionId::BotNaming).unwrap();
        let revision = config::load(&store).unwrap().revision;
        config::save(&store, revision, Some(FunctionId::BotNaming), None).unwrap();
        let hub = Hub::new(store, "teste".into(), Identity::load_or_create(&dir).unwrap(), "teste".into(), 0);
        assert!(apply_name_with_selection(&hub, "b", "Nome indevido", Some(&selection)).await.is_err());
        assert_eq!(hub.store.bot("b").unwrap().unwrap().config.name, "Original");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn produtores_concorrentes_preservam_ambos_os_turnos() {
        let store = Arc::new(Store::open(std::path::Path::new(":memory:")).unwrap());
        let bot: crate::store::BotConfig =
            serde_json::from_value(json!({"id":"b", "name":"Bot", "autoName":true})).unwrap();
        store.save_bot(&bot).unwrap();
        let gate = Arc::new(std::sync::Barrier::new(3));
        let threads: Vec<_> = (0..2)
            .map(|i| {
                let store = store.clone();
                let gate = gate.clone();
                std::thread::spawn(move || {
                    gate.wait();
                    record_pending(&store, "b", Turn { user: i.to_string(), agent: "ok".into(), at: i64::from(i) })
                        .unwrap();
                })
            })
            .collect();
        gate.wait();
        for thread in threads {
            thread.join().unwrap();
        }
        let mut users: Vec<_> = pending(&store, "b").into_iter().map(|turn| turn.user).collect();
        users.sort();
        assert_eq!(users, ["0", "1"]);
    }

    #[test]
    fn drenagem_concorrente_nao_derruba_o_turno_mais_recente() {
        // Cenário da revisão R3: duas filas drenam para a mesma janela e o
        // turno novo (G) é gravado antes dos antigos (A–F) — a ordem cronológica
        // tem que vencer a ordem de gravação, senão G seria o primeiro a cair.
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let bot: crate::store::BotConfig =
            serde_json::from_value(json!({"id":"b", "name":"Bot", "backend":"claude", "autoName":true})).unwrap();
        store.save_bot(&bot).unwrap();
        record_pending(&store, "b", Turn { user: "g".into(), agent: "g".into(), at: 7 }).unwrap();
        for i in 1..=6 {
            record_pending(&store, "b", Turn { user: format!("u{i}"), agent: "a".into(), at: i }).unwrap();
        }
        let turns = pending(&store, "b");
        assert_eq!(turns.len(), WINDOW);
        assert_eq!(turns.last().map(|t| t.user.as_str()), Some("g"), "o mais recente não pode cair");
        assert_eq!(
            turns.first().map(|t| t.user.as_str()),
            Some("u2"),
            "quem cai é o mais antigo, não o gravado primeiro"
        );
        assert!(turns.windows(2).all(|w| w[0].at <= w[1].at), "janela fica em ordem cronológica");
    }

    #[test]
    fn names_are_parsed() {
        assert_eq!(parse("name: 「記帳助手」").as_deref(), Some("記帳助手"));
        assert_eq!(parse("Sure!\n- Name: Release Captain.").as_deref(), Some("Release Captain"));
    }

    #[test]
    fn unclear_or_bad_names_are_rejected() {
        assert_eq!(parse("NONE"), None);
        assert_eq!(parse("name: NONE"), None);
        assert_eq!(parse("name: "), None);
        assert_eq!(parse(&format!("name: {}", "x".repeat(41))), None);
    }
}
