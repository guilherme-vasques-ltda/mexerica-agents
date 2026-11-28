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

pub fn codex_options(options: Vec<Value>) -> Vec<Value> {
    options
        .into_iter()
        .filter(|option| {
            option["optionId"].as_str().is_some_and(|id| {
                !id.is_empty()
                    && !matches!(
                        id,
                        "allow_always"
                            | "allow_for_session"
                            | "allow_session"
                            | "allow_permissions_session"
                            | "accept_execpolicy_amendment"
                    )
                    && !id.starts_with("apply_network_policy_amendment:")
            }) && matches!(option["kind"].as_str(), Some("allow_once" | "reject_once" | "reject_always"))
        })
        .collect()
}

/// O Grok persiste `allow_always` no próprio perfil, fora do Hub. Aprovação persistente pertence ao Hub
/// (D106): para o motor Grok só chegam ao cartão os tipos seguros conhecidos; qualquer outro — inclusive
/// `kind` ausente ou desconhecido — é descartado, e cada chamada volta a perguntar.
pub fn grok_options(options: Vec<Value>) -> Vec<Value> {
    options
        .into_iter()
        .filter(|option| matches!(option["kind"].as_str(), Some("allow_once" | "reject_once" | "reject_always")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn grok_so_recebe_tipos_seguros_conhecidos_e_descarta_o_resto() {
        let options = vec![
            json!({"optionId":"once","name":"Permitir","kind":"allow_once"}),
            json!({"optionId":"always","name":"Permitir sempre","kind":"allow_always"}),
            json!({"optionId":"allow_session","kind":"allow_always"}),
            json!({"optionId":"deny","kind":"reject_once"}),
            json!({"optionId":"never","kind":"reject_always"}),
            json!({"optionId":"futuro","kind":"allow_for_life"}),
            json!({"optionId":"sem_tipo"}),
        ];
        let kept = grok_options(options);
        assert_eq!(kept.len(), 3);
        assert!(
            kept.iter()
                .all(|option| matches!(option["kind"].as_str(), Some("allow_once" | "reject_once" | "reject_always")))
        );
    }

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
    fn codex_so_oferece_aprovacao_da_chamada_atual() {
        let options = codex_options(vec![
            json!({"optionId":"allow_once", "kind":"allow_once"}),
            json!({"optionId":"allow_for_session", "kind":"allow_always"}),
            json!({"optionId":"allow_session", "kind":"allow_always"}),
            json!({"optionId":"allow_permissions_session", "kind":"allow_always"}),
            json!({"optionId":"allow_permissions_session", "kind":"allow_once"}),
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
        assert_eq!(ids, ["allow_once", "decline", "cancel"]);
    }
}
