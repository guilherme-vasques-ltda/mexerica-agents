//! Origem de ferramentas do Hub e opções de aprovação temporárias.

use serde_json::Value;

/// Servidores MCP que o próprio Hub entrega e cuja identidade pode ser conferida na definição exata.
/// `routines` entra por ser dado do Hub (instruções do dono e resultados do próprio bot): sem ele, cada
/// chamada de rotina contaria como ferramenta externa (D125) e toda criação de rotina nasceria marcada.
pub const HUB_SERVERS: &[&str] = &["chat", "memory", "manage", "computers", "routines"];

/// Regras `allow` de ferramenta MCP que o Hub entrega ao Claude (`claude_meta`). Hoje NENHUMA: toda
/// ferramenta MCP, interna ou de conector, pede cartão. É a fonte única da liberação sem cartão: o Motor
/// Mexerica (`ToolClass::released`) lê a mesma lista, e `motor_mexerica::tests_paridade` falha se os dois
/// divergirem. Quando o Marketplace trouxer os grupos de liberação por conector, a regra muda aqui.
pub const MCP_ALLOW_RULES: &[&str] = &[];

/// `true` se alguma regra de [`MCP_ALLOW_RULES`] cobre o servidor inteiro (`mcp__<servidor>`) ou a
/// ferramenta (`mcp__<servidor>__<ferramenta>`), a sintaxe de regra do Claude.
pub fn mcp_released(server: &str, tool: &str) -> bool {
    MCP_ALLOW_RULES.iter().any(|rule| *rule == format!("mcp__{server}") || *rule == format!("mcp__{server}__{tool}"))
}

/// Reenvio do cliente após perder a resposta do Hub: só confirma o que já foi aplicado.
pub fn repeated_answer(entry: &crate::store::Entry, option_id: Option<&str>) -> bool {
    entry.kind == "permission"
        && entry.data["status"] == "answered"
        && option_id.is_some_and(|id| entry.data["selected"].as_str() == Some(id))
}

/// O Codex usa `allow_always` também para permissões de sessão. Só essas podem continuar:
/// regras persistentes pertencem ao Hub (D106), nunca ao `CODEX_HOME` compartilhado.
pub fn codex_options(mut options: Vec<Value>) -> Vec<Value> {
    options.retain_mut(|option| {
        let id = option["optionId"].as_str().unwrap_or_default();
        let kind = option["kind"].as_str().unwrap_or_default();
        let session = matches!(id, "allow_for_session" | "allow_session" | "allow_permissions_session");
        if id == "accept_execpolicy_amendment"
            || id.starts_with("apply_network_policy_amendment:")
            || (kind == "allow_always" && !session)
        {
            return false;
        }
        if session && kind == "allow_always" {
            option["name"] = "Permitir nesta sessão".into();
        }
        true
    });
    options
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reenvio_so_confirma_a_mesma_opcao_de_um_cartao_respondido() {
        use crate::store::{EntryKind, Lane, Store};
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        for status in ["answered", "pending", "expired", "cancelled"] {
            let mut entry = store
                .insert_entry(
                    &Lane::main("bot"),
                    EntryKind::Permission,
                    1,
                    &json!({"status":status, "selected":"allow"}),
                )
                .unwrap();
            assert_eq!(repeated_answer(&entry, Some("allow")), status == "answered");
            assert!(!repeated_answer(&entry, Some("deny")));
            assert!(!repeated_answer(&entry, None));
            entry.kind = "notice".into();
            assert!(!repeated_answer(&entry, Some("allow")));
        }
    }

    #[test]
    fn codex_so_oferece_decisoes_temporarias() {
        let options = codex_options(vec![
            json!({"optionId":"allow_once", "kind":"allow_once"}),
            json!({"optionId":"allow_for_session", "kind":"allow_always"}),
            json!({"optionId":"allow_session", "kind":"allow_always"}),
            json!({"optionId":"allow_permissions_session", "kind":"allow_always"}),
            json!({"optionId":"accept_execpolicy_amendment", "kind":"allow_always"}),
            json!({"optionId":"apply_network_policy_amendment:0", "kind":"allow_always"}),
            json!({"optionId":"allow_always", "kind":"allow_always"}),
            json!({"optionId":"persistencia_futura", "kind":"allow_always"}),
            json!({"optionId":"apply_network_policy_amendment:1", "kind":"reject_always"}),
            json!({"optionId":"apply_network_policy_amendment:2", "kind":"reject_once"}),
            json!({"optionId":"apply_network_policy_amendment:3"}),
            json!({"optionId":"decline", "kind":"reject_once"}),
            json!({"optionId":"cancel", "kind":"reject_once"}),
        ]);
        let ids: Vec<_> = options.iter().map(|o| o["optionId"].as_str().unwrap()).collect();
        assert_eq!(
            ids,
            ["allow_once", "allow_for_session", "allow_session", "allow_permissions_session", "decline", "cancel"]
        );
        assert!(options[1..4].iter().all(|o| o["name"] == "Permitir nesta sessão"));
    }
}
