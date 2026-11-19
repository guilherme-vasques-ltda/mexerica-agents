#![allow(clippy::unwrap_used)]
use super::*;

#[test]
fn internal_instructions_fit_and_preserve_guidance() {
    let instructions = [
        ("chat", crate::fork::owner_question::CHAT_INSTRUCTIONS.as_str()),
        ("chat-genui", CHAT_GENUI_INSTRUCTIONS.as_str()),
        ("connectors", crate::market::requests::INSTRUCTIONS),
        ("routines", crate::routines::INSTRUCTIONS),
        ("computer", INSTRUCTIONS),
        ("computers", crate::computers::tools::INSTRUCTIONS),
        ("team", crate::chat::team::INSTRUCTIONS),
        ("manage", crate::chat::manage::INSTRUCTIONS),
        ("memory", crate::chat::memory::INSTRUCTIONS),
    ];
    for (name, text) in instructions {
        let chars = text.chars().count();
        let utf16 = text.encode_utf16().count();
        eprintln!("{name}: {chars} caracteres, {utf16} unidades UTF-16");
        assert!(utf16 <= 3800, "{name}: {utf16}");
    }
    let security = regex::Regex::new(r"(?i)senha|segred|\bsecrets?\b|permiss|recus|nunca|never").unwrap();
    for (server, text, original, keys) in [
        (
            Server::Connectors,
            crate::market::requests::INSTRUCTIONS,
            include_str!("connectors_instructions_original.txt"),
            &[
                "Nunca peça senhas/chaves",
                "Permissões inválidas ou conflitantes",
                "Não repita uma escrita com resultado incerto",
                "não remova a marca nem contorne a recusa",
            ][..],
        ),
        (
            Server::Routines,
            crate::routines::INSTRUCTIONS,
            include_str!("routines_instructions_original.txt"),
            &["Keep secrets out of routine instructions", "nunca contorne", "Nunca use agendamento nativo"][..],
        ),
    ] {
        let hint = "Antes de usar, chame ajuda_instrucoes para o guia.";
        let end = text.find(hint).expect("falta orientação para o guia") + hint.len();
        assert!(text[..end].encode_utf16().count() <= 2048);
        let mut tools = json!([]);
        add_instruction_guide(server, &mut tools);
        assert_eq!(tools[0]["name"], "ajuda_instrucoes");
        assert!(!security.is_match(instruction_guide(server).unwrap()), "regra de segurança no guia");
        for key in keys {
            let position = text.find(key).unwrap_or_else(|| panic!("faltou regra: {key}"));
            assert!(text[..position + key.len()].encode_utf16().count() <= 3800, "regra fora do limite: {key}");
        }
        let mut actual: Vec<_> = text
            .lines()
            .filter(|line| *line != "Antes de usar, chame ajuda_instrucoes para o guia.")
            .chain(instruction_guide(server).unwrap().lines())
            .collect();
        let mut expected: Vec<_> = original.lines().collect();
        actual.sort_unstable();
        expected.sort_unstable();
        assert_eq!(actual, expected);
    }
    let mut other_tools = json!([]);
    add_instruction_guide(Server::Chat, &mut other_tools);
    assert_eq!(other_tools, json!([]));
}

#[tokio::test]
async fn guard_gateway_keeps_internal_mcp_result_shapes() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = axum::Router::new().route(
        "/api/{method}",
        axum::routing::post(|| async {
            axum::Json(json!({"fixture":true,"content":[{"type":"text","text":"internal fixture"}]}))
        }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    for kind in [Server::Chat, Server::Memory, Server::Manage] {
        let result = call(port, "synthetic-token", "synthetic-bot", &json!({"name":"fixture"}), kind).await;
        assert_eq!(
            result,
            json!({"content":[{"type":"text","text":"{\"fixture\":true,\"content\":[{\"type\":\"text\",\"text\":\"internal fixture\"}]}"}]})
        );
        assert!(result.get("_meta").is_none());
    }
    let result = call(port, "synthetic-token", "synthetic-bot", &json!({"name":"fixture"}), Server::Computers).await;
    assert_eq!(result, json!({"content":[{"type":"text","text":"internal fixture"}]}));
    server.abort();
}

#[test]
fn memory_transport_injeta_e_limpa_outros_browser_sem_excecao_por_prefixo() {
    for name in ["browser_open", "browser_status_extra", "search_memory"] {
        let params = json!({"name":name,"arguments":{"query":"fixture","__memoryTransport":"forjado"}});
        assert_eq!(
            memory_arguments(Server::Memory, &params, Some("transporte-sintetico")),
            json!({"query":"fixture","__memoryTransport":"transporte-sintetico"})
        );
        assert_eq!(memory_arguments(Server::Memory, &params, None), json!({"query":"fixture"}));
        assert_eq!(params["arguments"]["__memoryTransport"], "forjado", "não modifica a entrada");
    }
}

#[test]
fn browser_status_nao_injeta_nem_oculta_argumento_de_transporte_forjado() {
    let clean = json!({"name":"browser_status","arguments":{"version":1,"target":"desktop"}});
    let forged =
        json!({"name":"browser_status","arguments":{"version":1,"target":"desktop","__memoryTransport":"forjado"}});
    for capability in [None, Some("transporte-sintetico")] {
        assert_eq!(memory_arguments(Server::Memory, &clean, capability), clean["arguments"]);
        // Preserva o campo extra para a recusa do decoder; não transforma entrada inválida em válida.
        assert_eq!(memory_arguments(Server::Memory, &forged, capability), forged["arguments"]);
    }
    let other = json!({"name":"browser_open","arguments":{"__memoryTransport":"outro-servidor"}});
    assert_eq!(memory_arguments(Server::Chat, &other, None), other["arguments"]);
    assert_eq!(memory_arguments(Server::Chat, &other, Some("transporte-sintetico")), other["arguments"]);
}
