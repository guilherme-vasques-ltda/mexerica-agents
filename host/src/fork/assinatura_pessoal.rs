use crate::store::BotConfig;
use anyhow::Result;

pub(crate) const CHAT_ONLY: &str = "personal_subscription_chat_only: A assinatura Claude só funciona no chat. Use uma chave de API da Anthropic ou outro motor para automações.";
pub(crate) const META: &str = "meta_standard_unverified: Não foi possível comprovar uma chave da Meta Model API Standard. O modo Contributor vem ligado por padrão na assinatura Muse.";

#[derive(Debug)]
pub(crate) enum Refusal {
    Automation,
    Fallback,
    Meta,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Automation => CHAT_ONLY,
            Self::Fallback => "personal_subscription_no_fallback: A assinatura Claude não troca de conta automaticamente. Escolha a conta manualmente para continuar.",
            Self::Meta => META,
        })
    }
}

impl std::error::Error for Refusal {}

pub(crate) fn claude(backend: &str) -> bool {
    matches!(
        backend.to_ascii_lowercase().as_str(),
        "claude" | "claude-code" | "claude-acp" | "claude-agent-acp" | "claude-code-acp"
    )
}

pub(crate) fn muse(backend: &str) -> bool {
    let name = backend.to_ascii_lowercase();
    name == "muse" || name.starts_with("muse-") || name.starts_with("meta-muse")
}

pub(crate) fn known_meta_credential(name: &str, value: &str) -> bool {
    matches!(name.to_ascii_uppercase().as_str(), "META_API_KEY" | "MODEL_API_KEY" | "MUSE_API_KEY" | "MUSE_AUTH_TOKEN")
        || value.trim().starts_with("LLM_")
}

pub(crate) fn automation(cfg: &BotConfig) -> Result<()> {
    anyhow::ensure!(!claude(&cfg.backend), Refusal::Automation);
    anyhow::ensure!(!muse(&cfg.backend), Refusal::Meta);
    Ok(())
}

pub(crate) fn human_entries(store: &crate::store::Store, lane: &crate::store::Lane, ids: &[String]) -> bool {
    !ids.is_empty()
        && ids.iter().all(|id| {
            store.entry(id).is_some_and(|entry| {
                entry.bot_id == lane.chat
                    && entry.is_user_authored()
                    && entry.data["sourceBotId"].is_null()
                    && entry.data["runId"].is_null()
            })
        })
}

pub(crate) fn turn(cfg: &BotConfig, direct_chat: bool, owner_compaction: bool) -> Result<()> {
    let personal_action = (direct_chat && cfg.context.is_none())
        || (owner_compaction && cfg.context.as_ref().is_none_or(|context| !context.foreign_identity));
    if !personal_action || cfg.is_group() || cfg.space_id != crate::fork::spaces::PERSONAL {
        return automation(cfg);
    }
    anyhow::ensure!(!muse(&cfg.backend), Refusal::Meta);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn assinatura_so_no_chat_e_api_separada() {
        for backend in ["claude", "claude-acp", "muse", "muse-code", "mexerica", "codex"] {
            let mut cfg: BotConfig =
                serde_json::from_value(json!({"id":"b", "name":"Bot", "backend":backend})).unwrap();
            assert_eq!(turn(&cfg, true, false).is_ok(), !muse(backend), "{backend}");
            assert_eq!(turn(&cfg, false, true).is_ok(), !muse(backend), "{backend}: compactação manual");
            assert_eq!(turn(&cfg, false, false).is_ok(), matches!(backend, "mexerica" | "codex"), "{backend}");
            cfg.space_id = "outside".into();
            assert_eq!(turn(&cfg, true, true).is_ok(), matches!(backend, "mexerica" | "codex"), "{backend}");
        }
    }

    #[test]
    fn assinatura_pessoal_credencial_meta_nao_pode_ser_recadastrada_com_outro_nome() {
        assert!(known_meta_credential("MODEL_API_KEY", "opaque-fixture"));
        assert!(known_meta_credential("GENERIC_TOKEN", " LLM_fixture "));
        assert!(!known_meta_credential("ANTHROPIC_API_KEY", "opaque-fixture"));
    }

    #[test]
    fn assinatura_pessoal_recusa_entrada_sintetica() {
        use crate::store::{EntryKind, Lane, Store};
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let lane = Lane::main("bot");
        let own = store.insert_entry(&lane, EntryKind::User, 1, &json!({"text":"oi"})).unwrap();
        let other = store.insert_entry(&lane, EntryKind::User, 2, &json!({"text":"oi","runId":"r"})).unwrap();
        assert!(human_entries(&store, &lane, &[own.id]));
        assert!(!human_entries(&store, &lane, &[other.id]));
        assert!(!human_entries(&store, &lane, &["missing".into()]));
    }
}
