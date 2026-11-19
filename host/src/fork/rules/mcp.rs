//! Recusas aplicáveis aos encaminhamentos MCP. O chamador mantém `memory_boundary`.
use super::{
    rule::{Campo, Effect},
    store::GateDecision,
};
use crate::store::Store;
use anyhow::Result;
use serde_json::Value;

/// A ausência de alvo reconhecível não prova que uma regra de alvo não casa.
pub(crate) fn check(store: &Store, bot: &str, server: &str, method: &str, params: &Value) -> Result<bool> {
    if !matches!(method, "tools/call" | "resources/read" | "prompts/get") {
        return Ok(false);
    }
    let snapshot = store.donna_rules()?;
    let connector = format!("mcp__{server}");
    let name = params["name"].as_str().filter(|s| !s.is_empty());
    let tool = name.map(|name| format!("{connector}__{name}"));
    // Argumentos MCP são alegações da ferramenta, sem contrato que prove
    // qual campo determina o efeito. Um alvo arbitrário permanece desconhecido.
    for rule in &snapshot.rules {
        if !rule.enabled || rule.effect != Effect::Deny || rule.bot_id.as_deref().is_some_and(|b| b != bot) {
            continue;
        }
        if method == "tools/call" {
            if rule.tool.as_deref().is_some_and(|t| t != connector && tool.as_deref().is_some_and(|v| v != t)) {
                continue;
            }
            let possible = match rule.interpretacao() {
                Some((Campo::Ferramenta, _)) => {
                    rule.value == connector || tool.as_deref().is_none_or(|t| t == rule.value)
                }
                Some((Campo::Acao, _)) => name.is_none_or(|n| n == rule.value),
                Some((Campo::Alvo, _)) | None => true,
            };
            if !possible {
                continue;
            }
        } else if rule.interpretacao().is_none_or(|(field, _)| field != Campo::Ferramenta)
            || rule.value != connector
            || rule.tool.as_deref().is_some_and(|t| t != connector)
        {
            continue;
        }
        let decision = GateDecision {
            bot_id: bot,
            origin: "derived",
            tool: tool.as_deref(),
            verdict: "deny",
            reason: "mcp_deny_rule",
            rule_id: Some(&rule.id),
            rule_sentence: Some(rule.sentence()),
            policy_revision: snapshot.revision,
            floor_category: None,
        };
        if let Err(error) = store.donna_rule_gate_decision(&decision) {
            tracing::warn!(%error, "auditoria de recusa MCP falhou");
        }
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::{LockExt, fork::rules::rule::Rule};
    use serde_json::json;

    fn rule(field: &str, value: &str) -> Rule {
        Rule {
            id: String::new(),
            effect: Effect::Deny,
            field: field.into(),
            op: "igual".into(),
            value: value.into(),
            bot_id: None,
            tool: None,
            scope: None,
            enabled: true,
            revision: 0,
            created_at: 0,
            updated_at: 0,
        }
    }
    #[test]
    fn regras_mcp_recusam_alvo_incerto_e_conector_inteiro() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let params = json!({"name":"ler","arguments":{}});
        assert!(!check(&store, "bot", "files", "tools/call", &params).unwrap());
        let rev = store.donna_rules().unwrap().revision;
        let saved = store.donna_rule_save(rule("alvo", "/segredo"), rev, true).unwrap();
        assert!(check(&store, "bot", "files", "tools/call", &params).unwrap());
        assert!(
            check(&store, "bot", "files", "tools/call", &json!({"name":"ler","arguments":{"path":"/outro"}})).unwrap()
        );
        assert!(!check(&store, "bot", "files", "resources/read", &json!({})).unwrap());
        store.donna_rule_delete(&saved.id, saved.revision).unwrap();
        let rev = store.donna_rules().unwrap().revision;
        let saved = store.donna_rule_save(rule("ferramenta", "mcp__files__ler"), rev, true).unwrap();
        assert!(check(&store, "bot", "files", "tools/call", &params).unwrap());
        assert!(!check(&store, "bot", "files", "resources/read", &json!({})).unwrap());
        store.donna_rule_delete(&saved.id, saved.revision).unwrap();
        let rev = store.donna_rules().unwrap().revision;
        store.donna_rule_save(rule("ferramenta", "mcp__files"), rev, true).unwrap();
        assert!(check(&store, "bot", "files", "resources/read", &json!({})).unwrap());
        assert!(check(&store, "bot", "files", "prompts/get", &json!({})).unwrap());
        store.db.locked().execute("DROP TABLE donna_rules", []).unwrap();
        assert!(check(&store, "bot", "files", "tools/call", &params).is_err());
    }
}
