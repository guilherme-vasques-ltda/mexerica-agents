//! Identidade de ferramentas ACP: só definições internas entregues pelo Hub à sessão.
use serde_json::Value;
use std::{collections::HashSet, path::Path};

/// Variável que o Hub põe em cada servidor interno: o slot da sessão do motor que o recebeu (D229-6).
pub(crate) const SESSION_SLOT_ENV: &str = "DONNABOT_SESSION_SLOT";
/// O slot da sessão principal do bot no seu próprio chat.
pub(crate) const MAIN_SLOT: &str = "main";

/// O valor de [`SESSION_SLOT_ENV`] para um slot (`None` é a sessão principal).
pub(crate) fn slot_value(slot: Option<&str>) -> &str {
    slot.unwrap_or(MAIN_SLOT)
}

#[allow(clippy::too_many_arguments, reason = "Confere todos os campos da definição MCP emitida pelo Hub.")]
pub(crate) fn internal_servers(
    definitions: &Value,
    exe: &Path,
    bot: &str,
    destination: (u16, Option<&Path>),
    space: &str,
    slot: Option<&str>,
    tool_binding: Option<&str>,
    binding: Option<&str>,
) -> HashSet<String> {
    let Some(definitions) = definitions.as_array() else { return HashSet::new() };
    let (port, socket) = destination;
    let socket = match (port, socket) {
        (crate::fork::bridge_http::SOCKET_PORT, Some(socket)) => {
            let Some(socket) = socket.to_str() else { return HashSet::new() };
            Some(socket)
        }
        (port, None) if port != crate::fork::bridge_http::SOCKET_PORT => None,
        _ => return HashSet::new(),
    };
    crate::agent::engine_permissions::HUB_SERVERS
        .iter()
        .filter(|name| {
            let mut matches = definitions.iter().filter(|d| d["name"].as_str() == Some(**name));
            let mut expected = serde_json::json!({"name":name,"command":exe,
                "args":["mcp",name,"--bot",bot,"--port",port.to_string()],
                "env":[{"name":"DONNABOT_SPACE_ID","value":space},{"name":SESSION_SLOT_ENV,"value":slot_value(slot)}]});
            if let Some(binding) = tool_binding {
                let Some(env) = expected["env"].as_array_mut() else { return false };
                env.push(serde_json::json!({"name":crate::fork::spaces::tool_context::BINDING_ENV,"value":binding}));
            }
            let first = matches.next();
            if **name == "memory" {
                let capability = first
                    .and_then(|d| d["env"].as_array())
                    .and_then(|env| env.iter().find(|e| e["name"] == crate::fork::memory_auto::transport::ENV))
                    .and_then(|e| e["value"].as_str())
                    .filter(|s| uuid::Uuid::parse_str(s).is_ok());
                let Some(capability) = capability else { return false };
                if let Some(env) = expected["env"].as_array_mut() {
                    env.push(serde_json::json!({"name":crate::fork::memory_auto::transport::ENV,"value":capability}));
                }
            }
            if let Some(binding) = binding
                && let Some(env) = expected["env"].as_array_mut()
            {
                env.push(serde_json::json!({"name":crate::fork::sessoes::bindings::ENV,"value":binding}));
            }
            if let Some(socket) = socket
                && let Some(env) = expected["env"].as_array_mut()
            {
                env.push(serde_json::json!({"name":crate::fork::bridge_http::SOCKET_ENV,"value":socket}));
            }
            let first = first.cloned().map(|mut definition| {
                if let Some(env) = definition["env"].as_array_mut() {
                    let tokens: Vec<_> =
                        env.iter().filter(|e| e["name"] == crate::fork::ponte_credencial::ENV).collect();
                    if tokens.len() == 1 && tokens[0]["value"].as_str().is_some_and(|s| s.starts_with("ponte_")) {
                        env.retain(|e| e["name"] != crate::fork::ponte_credencial::ENV);
                    }
                }
                definition
            });
            first.as_ref() == Some(&expected) && matches.next().is_none()
        })
        .map(|name| (*name).to_owned())
        .collect()
}

/// O que o canal de identidade do adaptador afirma neste update.
enum IdentityClaim {
    /// O update não traz canal de identidade: nada a afirmar nem a desmentir.
    Absent,
    /// O update traz canal de identidade, mas ele é malformado ou divergente.
    Rejected,
    /// Servidor alegado pelo canal comprovado do adaptador.
    Server(String),
}

/// Versões do codex-acp cujo contrato de identidade foi verificado nas fontes
/// pinadas do npm (integridade sha512 do registro):
/// 2.2.2 `sha512-vNdeNV3jxbqaH7MYvVWzyB6cMOz+YGIV9W1xyBDsQfoii92DulZLM+ahCL2T2G6uohlaB1Yma347p2kLfAab2w==`,
/// 2.6.0 `sha512-/PBsd0H6URy4bhXhAeXqwb7RqctbXfNceRihAh6nDmn5HJ873uzoHvHxF45YmeWfgOKKRrNRLZjYsket2a7EsQ==`.
/// Qualquer outra versão não tem canal confiável e degrada para `unknown` —
/// nunca para `internal`.
const CODEX_CONTRACT_VERSIONS: &[&str] = &["2.2.2", "2.6.0"];

/// Identidade só vem do canal do adaptador, nunca de argumento de ferramenta.
/// No Codex o reporter MCP marca `_meta.is_mcp_tool_call` e escreve o envelope
/// `{server, tool, arguments}` em `rawInput` — `arguments` continua sendo dado
/// da ferramenta e nunca é lido como identidade. No Claude o `rawInput` são os
/// argumentos da ferramenta (um `server` ali é dado, como no
/// `ReadMcpResourceTool`), então a identidade vem de `_meta.claudeCode.toolName`.
/// Outros adaptadores não têm canal comprovado e ficam sem alegação.
fn identity_claim(update: &Value, adapter: crate::agent::bot::Adapter, version: Option<&str>) -> IdentityClaim {
    match adapter {
        crate::agent::bot::Adapter::Codex => {
            // Contrato verificado em @agentclientprotocol/codex-acp@2.2.2 e 2.6.0
            // (src/tool-calls/reporters/McpToolReporter.ts): o reporter MCP marca
            // `_meta.is_mcp_tool_call` só no start e escreve o envelope
            // `{server, tool, arguments}` em `rawInput`. Updates de conclusão
            // repetem o envelope sem o marcador — ausência de canal, não
            // contradição. Versão fora da lista verificada não tem canal.
            if !version.is_some_and(|v| CODEX_CONTRACT_VERSIONS.contains(&v)) {
                return IdentityClaim::Absent;
            }
            if update.pointer("/_meta/is_mcp_tool_call").and_then(Value::as_bool) != Some(true) {
                return IdentityClaim::Absent;
            }
            let Some(input) = update.get("rawInput") else {
                // Marcador sem payload não é alegação: não desmente prova gravada.
                return IdentityClaim::Absent;
            };
            let server = input["server"].as_str();
            let tool = input["tool"].as_str();
            let malformed = input.get("server").is_some_and(|v| !v.is_string())
                || input.get("tool").is_some_and(|v| !v.is_string())
                || input.get("serverName").is_some()
                || server.is_some_and(str::is_empty)
                || tool.is_some_and(str::is_empty);
            if malformed || server.is_none() || tool.is_none() {
                IdentityClaim::Rejected
            } else {
                IdentityClaim::Server(server.unwrap_or_default().to_owned())
            }
        }
        crate::agent::bot::Adapter::Claude => {
            let Some(raw) = update.pointer("/_meta/claudeCode/toolName") else {
                return IdentityClaim::Absent;
            };
            let Some(name) = raw.as_str() else { return IdentityClaim::Rejected };
            match name.strip_prefix("mcp__").and_then(|rest| rest.split_once("__")) {
                Some((server, method)) if !server.is_empty() && !method.is_empty() => {
                    IdentityClaim::Server(server.to_owned())
                }
                _ => IdentityClaim::Rejected,
            }
        }
        crate::agent::bot::Adapter::Mexerica => {
            // `_meta.mexericaTool = {server, tool}` é escrito pelo Hub (`McpHost`),
            // nunca copiado de campo do motor. A prova continua sendo o servidor
            // estar entre as definições internas exatas da sessão.
            let Some(raw) = update.pointer("/_meta/mexericaTool") else { return IdentityClaim::Absent };
            match (raw["server"].as_str(), raw["tool"].as_str()) {
                (Some(server), Some(tool)) if !server.is_empty() && !tool.is_empty() => {
                    IdentityClaim::Server(server.to_owned())
                }
                _ => IdentityClaim::Rejected,
            }
        }
        crate::agent::bot::Adapter::Other => IdentityClaim::Absent,
    }
}

/// Rótulo de exibição do servidor Codex: só no mesmo canal da prova — marcador
/// `_meta.is_mcp_tool_call` + envelope `server`/`tool` não vazios. `serverName`
/// e argumentos soltos não viram rótulo — são dados de elicitação/ferramenta.
fn codex_claimed_server(update: &Value) -> Option<String> {
    if update.pointer("/_meta/is_mcp_tool_call").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let input = &update["rawInput"];
    (input["server"].as_str().is_some_and(|s| !s.is_empty()) && input["tool"].as_str().is_some_and(|s| !s.is_empty()))
        .then(|| input["server"].as_str().unwrap_or_default().to_owned())
}

// Apenas criação devolve exclusivamente os dados fornecidos na própria chamada.
// Atualizações e outras operações podem devolver conteúdo anteriormente armazenado.
fn routine_creation_claim(update: &Value, adapter: crate::agent::bot::Adapter, version: Option<&str>) -> bool {
    let (method, args) = match adapter {
        crate::agent::bot::Adapter::Claude => (
            update
                .pointer("/_meta/claudeCode/toolName")
                .and_then(Value::as_str)
                .and_then(|name| name.strip_prefix("mcp__routines__")),
            update.get("rawInput"),
        ),
        crate::agent::bot::Adapter::Codex
            if version.is_some_and(|v| CODEX_CONTRACT_VERSIONS.contains(&v))
                && update.pointer("/_meta/is_mcp_tool_call").and_then(Value::as_bool) == Some(true) =>
        {
            (update.pointer("/rawInput/tool").and_then(Value::as_str), update.pointer("/rawInput/arguments"))
        }
        crate::agent::bot::Adapter::Mexerica => (
            update
                .pointer("/_meta/mexericaTool")
                .filter(|tool| tool["server"] == "routines")
                .and_then(|tool| tool["tool"].as_str()),
            update.get("rawInput"),
        ),
        _ => return false,
    };
    method == Some("save_routine") && args.and_then(Value::as_object).is_some_and(|args| !args.contains_key("id"))
}

/// rawInput é metadado do adaptador, não título, conteúdo ou argumento de ferramenta.
/// O marcador é escrito pelo Hub; alegações do motor no conteúdo não são copiadas.
pub(crate) fn record(
    data: &mut Value,
    update: &Value,
    servers: Option<&HashSet<String>>,
    adapter: crate::agent::bot::Adapter,
    version: Option<&str>,
) {
    let claim = identity_claim(update, adapter, version);
    let server = match &claim {
        IdentityClaim::Server(server) => Some(server.clone()),
        _ => None,
    };
    // `mcpServer` é identidade: só o nome que o canal do adaptador comprovou;
    // o porteiro e a classificação de memória o leem como prova. Uma alegação
    // rejeitada não deixa a prova anterior para trás. `mcpServerClaimed` é só
    // o rótulo de exibição — a mesma alegação mais o envelope MCP completo do
    // Codex (`server`+`tool`), mesmo rejeitado — para o cartão mostrar o servidor
    // sem autorizá-lo nem inferir origem.
    let claimed = server.clone().or_else(|| match adapter {
        crate::agent::bot::Adapter::Codex => codex_claimed_server(update),
        _ => None,
    });
    match &claim {
        IdentityClaim::Server(server) => data["mcpServer"] = server.as_str().into(),
        IdentityClaim::Rejected => {
            if let Some(object) = data.as_object_mut() {
                object.remove("mcpServer");
            }
        }
        IdentityClaim::Absent => {}
    }
    match &claimed {
        Some(claimed) => data["mcpServerClaimed"] = claimed.as_str().into(),
        None if matches!(claim, IdentityClaim::Rejected) => {
            if let Some(object) = data.as_object_mut() {
                object.remove("mcpServerClaimed");
            }
        }
        None => {}
    }
    data["hubToolOrigin"] = if server.is_some_and(|s| {
        servers.is_some_and(|known| known.contains(&s))
            && (s != "routines" || routine_creation_claim(update, adapter, version))
    }) && !matches!(data["toolKind"].as_str(), Some("fetch" | "search"))
    {
        "internal"
    } else {
        "unknown"
    }
    .into();
}

pub(crate) fn revise(
    data: &mut Value,
    update: &Value,
    servers: Option<&HashSet<String>>,
    adapter: crate::agent::bot::Adapter,
    version: Option<&str>,
) {
    // No Claude a identidade viaja em `_meta`, não em `rawInput`: um update que
    // só traz `_meta.claudeCode.toolName` também precisa passar pelo record.
    if !(update.get("rawInput").is_some()
        || update.pointer("/_meta/claudeCode/toolName").is_some()
        || update.pointer("/_meta/is_mcp_tool_call").is_some()
        || update.pointer("/_meta/mexericaTool").is_some()
        || matches!(data["toolKind"].as_str(), Some("fetch" | "search")))
    {
        return;
    }
    let internal = data["hubToolOrigin"] == "internal";
    let previous_server = data["mcpServer"].as_str().map(str::to_owned);
    let claim = identity_claim(update, adapter, version);
    record(data, update, servers, adapter, version);
    // O mesmo toolCallId não pode trocar de servidor provado: memória→manage ou
    // memória→servidor externo é contradição, não atualização. A prova anterior
    // sai junto com a origem; uma alegação posterior não reabilita.
    if let (IdentityClaim::Server(server), Some(previous)) = (&claim, previous_server.as_deref())
        && previous != server
    {
        data["hubToolOrigin"] = "unknown".into();
        if let Some(object) = data.as_object_mut() {
            object.remove("mcpServer");
        }
    }
    if !internal {
        // Uma notificação posterior não reabilita proveniência que já era incerta.
        data["hubToolOrigin"] = "unknown".into();
    } else if matches!(claim, IdentityClaim::Absent) && !matches!(data["toolKind"].as_str(), Some("fetch" | "search")) {
        // Update sem canal de identidade não desmente a prova já gravada: é a
        // mesma chamada (toolCallId) e ausência de alegação não é contradição.
        // Alegação rejeitada ou divergente deixa o unknown gravado por record.
        data["hubToolOrigin"] = "internal".into();
    }
}

/// Apenas ferramentas internas identificadas pelo Hub ficam fora da marca de memória.
pub(crate) fn exposes_memory(data: &Value) -> bool {
    data["hubToolOrigin"] != "internal" || data["mcpServer"] == "computers"
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn codex(server: &str) -> Value {
        json!({"_meta":{"is_mcp_tool_call":true},"rawInput":{"server":server,"tool":"call","arguments":{}}})
    }

    fn codex_raw(raw: &Value) -> Value {
        json!({"_meta":{"is_mcp_tool_call":true},"rawInput":raw})
    }

    #[test]
    fn private_hub_requires_its_exact_socket_in_internal_definitions() {
        let socket = "/private/fixture/s";
        let env = crate::fork::bridge_http::SOCKET_ENV;
        let definition = json!({"name":"manage","command":"/hub",
            "args":["mcp","manage","--bot","bot","--port","0"],
            "env":[{"name":"DONNABOT_SPACE_ID","value":"space"},
                {"name":SESSION_SLOT_ENV,"value":MAIN_SLOT},{"name":env,"value":socket}]});
        let known = |definition: Value, destination| {
            internal_servers(&json!([definition]), Path::new("/hub"), "bot", destination, "space", None, None, None)
                .contains("manage")
        };
        let private = (0, Some(Path::new(socket)));
        assert!(known(definition.clone(), private));
        assert!(!known(definition.clone(), (0, None)));
        assert!(!known(definition.clone(), (0, Some(Path::new("/private/other/s")))));
        assert!(!known(definition.clone(), (123, None)));
        let mut duplicate = definition.clone();
        duplicate["env"].as_array_mut().unwrap().push(json!({"name":env,"value":socket}));
        assert!(!known(duplicate, private));
        let mut absent = definition;
        absent["env"].as_array_mut().unwrap().pop();
        assert!(!known(absent, private));
    }

    #[test]
    fn only_adapter_identity_bound_to_unique_hub_definition_is_internal() {
        let definition = json!({"name":"manage","command":"/hub",
            "args":["mcp","manage","--bot","donna","--port","123"],"env":[{"name":"DONNABOT_SPACE_ID","value":"personal-fixture"},
            {"name":SESSION_SLOT_ENV,"value":MAIN_SLOT}]});
        let known = internal_servers(
            &json!([definition]),
            Path::new("/hub"),
            "donna",
            (123, None),
            "personal-fixture",
            None,
            None,
            None,
        );
        assert!(known.contains("manage"));
        let mut contextual = definition.clone();
        contextual["env"][1]["value"] = "conversation:adicional".into();
        contextual["env"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":crate::fork::spaces::tool_context::BINDING_ENV,"value":"vinculo-emitido"}));
        assert!(
            internal_servers(
                &json!([contextual]),
                Path::new("/hub"),
                "donna",
                (123, None),
                "personal-fixture",
                Some("conversation:adicional"),
                Some("vinculo-emitido"),
                None,
            )
            .contains("manage")
        );
        assert!(
            internal_servers(
                &json!([contextual]),
                Path::new("/hub"),
                "donna",
                (123, None),
                "personal-fixture",
                Some("conversation:adicional"),
                Some("outro-vinculo"),
                None,
            )
            .is_empty()
        );

        assert!(
            internal_servers(
                &json!([definition]),
                Path::new("/hub"),
                "donna",
                (123, None),
                "different-space",
                None,
                None,
                None
            )
            .is_empty()
        );
        let in_group = Some("group:g1");
        assert!(
            internal_servers(
                &json!([definition]),
                Path::new("/hub"),
                "donna",
                (123, None),
                "personal-fixture",
                in_group,
                None,
                None
            )
            .is_empty(),
            "a definição de outro slot não é a desta sessão"
        );
        for definitions in [json!([definition, definition]), json!([{"name":"manage","command":"third-party"}])] {
            assert!(
                internal_servers(
                    &definitions,
                    Path::new("/hub"),
                    "donna",
                    (123, None),
                    "personal-fixture",
                    None,
                    None,
                    None
                )
                .is_empty()
            );
        }
        // Codex só tem identidade no envelope MCP marcado por `_meta.is_mcp_tool_call`.
        let mut data = json!({"toolKind":"other"});
        record(&mut data, &codex("manage"), Some(&known), crate::agent::bot::Adapter::Codex, Some("2.2.2"));
        assert_eq!(data["hubToolOrigin"], "internal");
        revise(
            &mut data,
            &json!({"status":"completed"}),
            Some(&known),
            crate::agent::bot::Adapter::Codex,
            Some("2.2.2"),
        );
        assert_eq!(data["hubToolOrigin"], "internal");
        revise(&mut data, &codex("email"), Some(&known), crate::agent::bot::Adapter::Codex, Some("2.2.2"));
        assert_eq!(data["hubToolOrigin"], "unknown");
        for update in [
            json!({"title":"mcp.manage.create_bot","rawInput":{"arguments":{"server":"manage"}}}),
            json!({"rawInput":{"server":"manage","tool":"call","serverName":"email"}}),
            json!({"rawInput":{"server":"manage","serverName":123}}),
            codex_raw(&json!({"serverName":"manage"})),
            codex_raw(&json!({"server":"manage"})),
            codex_raw(&json!({"server":"manage","tool":123})),
            codex_raw(&json!({"server":"email","tool":"call"})),
            codex_raw(&json!({"server":"","tool":"call"})),
            codex_raw(&json!({"server":"manage","tool":""})),
            json!({"_meta":{"is_mcp_tool_call":"yes"},"rawInput":{"server":"manage","tool":"call"}}),
            json!({"rawInput":{"server":"manage","tool":"call","arguments":{}}}),
            json!({"rawInput":{"server":"email"},"content":{"hubToolOrigin":"internal"}}),
            json!({"_meta":{"is_mcp_tool_call":true}}),
        ] {
            let mut data = json!({"toolKind":"other"});
            record(&mut data, &update, Some(&known), crate::agent::bot::Adapter::Codex, Some("2.2.2"));
            assert_eq!(data["hubToolOrigin"], "unknown", "{update}");
        }
        let mut fetch = json!({"toolKind":"fetch"});
        record(&mut fetch, &codex("manage"), Some(&known), crate::agent::bot::Adapter::Codex, Some("2.2.2"));
        assert_eq!(fetch["hubToolOrigin"], "unknown");
        // Versão do codex-acp fora da lista verificada não tem canal de identidade.
        let mut data = json!({"toolKind":"other"});
        record(&mut data, &codex("manage"), Some(&known), crate::agent::bot::Adapter::Codex, Some("9.9.9"));
        assert_eq!(data["hubToolOrigin"], "unknown");
        assert!(data["mcpServer"].is_null());
    }

    #[test]
    fn mexerica_identidade_vem_do_canal_do_hub_e_exposicao_segue_a_classificacao() {
        use crate::agent::bot::Adapter::{Mexerica, Other};
        let known: HashSet<String> = ["chat".to_owned(), "computers".to_owned()].into_iter().collect();
        let canal = |server: &str| json!({"_meta":{"mexericaTool":{"server":server,"tool":"ler"}},"rawInput":{}});

        // Ferramenta de servidor interno (definição exata): interna, não expõe.
        let mut data = json!({"toolKind":"other"});
        record(&mut data, &canal("chat"), Some(&known), Mexerica, None);
        assert_eq!((&data["hubToolOrigin"], &data["mcpServer"]), (&json!("internal"), &json!("chat")));
        assert!(!exposes_memory(&data));
        // Tela do usuário (`computers`) e conector expõem, como nos outros motores.
        let mut computers = json!({"toolKind":"other"});
        record(&mut computers, &canal("computers"), Some(&known), Mexerica, None);
        assert_eq!(computers["hubToolOrigin"], "internal");
        assert!(exposes_memory(&computers));
        let mut conector = json!({"toolKind":"other"});
        record(&mut conector, &canal("email"), Some(&known), Mexerica, None);
        assert_eq!(conector["hubToolOrigin"], "unknown");
        assert!(exposes_memory(&conector));
        // Sem canal, ou com canal malformado, nada prova: desconhecida e exposta.
        for update in [
            json!({"rawInput":{"server":"chat"}}),
            json!({"_meta":{"mexericaTool":{"server":"chat"}}}),
            json!({"_meta":{"mexericaTool":{"server":7,"tool":"ler"}}}),
            json!({"_meta":{"mexericaTool":{"server":"","tool":"ler"}}}),
            json!({"content":{"hubToolOrigin":"internal"},"rawInput":{"mcpServer":"chat"}}),
        ] {
            let mut data = json!({"toolKind":"other"});
            record(&mut data, &update, Some(&known), Mexerica, None);
            assert_eq!(data["hubToolOrigin"], "unknown", "{update}");
            assert!(exposes_memory(&data));
        }
        // Só o adaptador do Motor Mexerica tem esse canal: outro processo ACP não o forja.
        let mut forjado = json!({"toolKind":"other"});
        record(&mut forjado, &canal("chat"), Some(&known), Other, None);
        assert_eq!(forjado["hubToolOrigin"], "unknown");
        // Leitura/busca externa nunca é interna, nem com o canal.
        let mut busca = json!({"toolKind":"fetch"});
        record(&mut busca, &canal("chat"), Some(&known), Mexerica, None);
        assert_eq!(busca["hubToolOrigin"], "unknown");
        // O mesmo toolCallId não troca de servidor provado.
        let mut troca = json!({"toolKind":"other"});
        record(&mut troca, &canal("chat"), Some(&known), Mexerica, None);
        revise(&mut troca, &canal("email"), Some(&known), Mexerica, None);
        assert_eq!(troca["hubToolOrigin"], "unknown");
    }

    #[test]
    fn claude_arguments_never_become_server_identity() {
        let known: HashSet<String> = ["memory".to_owned(), "manage".to_owned()].into_iter().collect();
        // No Claude o `rawInput` são os argumentos da ferramenta: um `server`
        // ali é dado (como no `ReadMcpResourceTool`), nunca identidade.
        for update in [
            json!({"rawInput":{"server":"memory","arguments":{}}}),
            json!({"rawInput":{"serverName":"manage"}}),
            json!({"title":"mcp.memory.remember","rawInput":{"server":"memory"}}),
            json!({"_meta":{"claudeCode":{"toolName":"Bash"}},"rawInput":{"command":"ls"}}),
            json!({"_meta":{"claudeCode":{"toolName":null}},"rawInput":{}}),
        ] {
            let mut data = json!({"toolKind":"other"});
            record(&mut data, &update, Some(&known), crate::agent::bot::Adapter::Claude, None);
            assert_eq!(data["hubToolOrigin"], "unknown", "{update}");
            assert!(data["mcpServer"].is_null(), "{update}");
            assert!(data["mcpServerClaimed"].is_null(), "{update}");
        }
        // O servidor alegado pelo adaptador é registrado como proveniência,
        // mas fora do conjunto da sessão não vira interno — e o `server` dos
        // argumentos não desvia a identidade para `memory`.
        let mut data = json!({"toolKind":"other"});
        record(
            &mut data,
            &json!({"_meta":{"claudeCode":{"toolName":"mcp__email__read"}},"rawInput":{"server":"memory"}}),
            Some(&known),
            crate::agent::bot::Adapter::Claude,
            None,
        );
        assert_eq!(data["hubToolOrigin"], "unknown");
        assert_eq!(data["mcpServer"], "email");
        // A identidade vem do `_meta.claudeCode.toolName` do adaptador.
        let mut data = json!({"toolKind":"other"});
        record(
            &mut data,
            &json!({"_meta":{"claudeCode":{"toolName":"mcp__memory__remember"}},"rawInput":{"text":"oi"}}),
            Some(&known),
            crate::agent::bot::Adapter::Claude,
            None,
        );
        assert_eq!(data["hubToolOrigin"], "internal");
        assert_eq!(data["mcpServer"], "memory");
        // Servidor homônimo que a sessão não recebeu não é interno.
        let mut data = json!({"toolKind":"other"});
        record(
            &mut data,
            &json!({"_meta":{"claudeCode":{"toolName":"mcp__email__read"}},"rawInput":{}}),
            Some(&known),
            crate::agent::bot::Adapter::Claude,
            None,
        );
        assert_eq!(data["hubToolOrigin"], "unknown");
        // Outros adaptadores não têm canal comprovado.
        let mut data = json!({"toolKind":"other"});
        record(
            &mut data,
            &json!({"_meta":{"claudeCode":{"toolName":"mcp__memory__remember"}}}),
            Some(&known),
            crate::agent::bot::Adapter::Other,
            None,
        );
        assert_eq!(data["hubToolOrigin"], "unknown");
    }

    #[test]
    fn revise_rebaixa_e_nunca_reabilita_proveniencia() {
        let known: HashSet<String> = ["memory".to_owned(), "manage".to_owned()].into_iter().collect();
        // internal → atualização com servidor divergente → unknown.
        let mut data = json!({"toolKind":"other"});
        record(&mut data, &codex("memory"), Some(&known), crate::agent::bot::Adapter::Codex, Some("2.2.2"));
        assert_eq!(data["hubToolOrigin"], "internal");
        revise(&mut data, &codex("email"), Some(&known), crate::agent::bot::Adapter::Codex, Some("2.2.2"));
        assert_eq!(data["hubToolOrigin"], "unknown");
        assert!(data["mcpServer"].is_null());
        // Troca entre nomes conhecidos no mesmo toolCallId é contradição → unknown.
        let mut data = json!({"toolKind":"other"});
        record(&mut data, &codex("memory"), Some(&known), crate::agent::bot::Adapter::Codex, Some("2.2.2"));
        revise(&mut data, &codex("manage"), Some(&known), crate::agent::bot::Adapter::Codex, Some("2.2.2"));
        assert_eq!(data["hubToolOrigin"], "unknown");
        assert!(data["mcpServer"].is_null());
        assert_eq!(data["mcpServerClaimed"], "manage");
        // unknown não volta a internal, mesmo com alegação válida depois.
        revise(&mut data, &codex("memory"), Some(&known), crate::agent::bot::Adapter::Codex, Some("2.2.2"));
        assert_eq!(data["hubToolOrigin"], "unknown");
        // Update sem rawInput e kind comum não regrava: estado preservado.
        revise(
            &mut data,
            &json!({"status":"completed"}),
            Some(&known),
            crate::agent::bot::Adapter::Codex,
            Some("2.2.2"),
        );
        assert_eq!(data["hubToolOrigin"], "unknown");
        // fetch/search sempre passam por record, mesmo sem rawInput — e nunca
        // são internal.
        let mut fetch = json!({"toolKind":"fetch","hubToolOrigin":"internal","mcpServer":"manage"});
        revise(
            &mut fetch,
            &json!({"status":"completed"}),
            Some(&known),
            crate::agent::bot::Adapter::Codex,
            Some("2.2.2"),
        );
        assert_eq!(fetch["hubToolOrigin"], "unknown");
        // Update com rawInput mas sem `_meta.is_mcp_tool_call` não tem canal de
        // identidade: ausência de alegação não desmente a prova já gravada.
        let mut data = json!({"toolKind":"other"});
        record(&mut data, &codex("memory"), Some(&known), crate::agent::bot::Adapter::Codex, Some("2.2.2"));
        assert_eq!(data["hubToolOrigin"], "internal");
        revise(
            &mut data,
            &json!({"rawInput":{"arguments":{}}}),
            Some(&known),
            crate::agent::bot::Adapter::Codex,
            Some("2.2.2"),
        );
        assert_eq!(data["hubToolOrigin"], "internal");
        // Marcador Codex sem rawInput também não é alegação: ausência de canal.
        revise(
            &mut data,
            &json!({"_meta":{"is_mcp_tool_call":true}}),
            Some(&known),
            crate::agent::bot::Adapter::Codex,
            Some("2.2.2"),
        );
        assert_eq!(data["hubToolOrigin"], "internal");
        // Envelope Codex marcado mas rejeitado (divergente ou malformado) é
        // contradição, não ausência: rebaixa, limpa a prova e guarda o rótulo.
        revise(
            &mut data,
            &codex_raw(&json!({"server":"memory","tool":"remember","serverName":"external"})),
            Some(&known),
            crate::agent::bot::Adapter::Codex,
            Some("2.2.2"),
        );
        assert_eq!(data["hubToolOrigin"], "unknown");
        assert!(data["mcpServer"].is_null());
        assert_eq!(data["mcpServerClaimed"], "memory");
        // E não volta: unknown nunca é reabilitado por update posterior.
        revise(&mut data, &codex("memory"), Some(&known), crate::agent::bot::Adapter::Codex, Some("2.2.2"));
        assert_eq!(data["hubToolOrigin"], "unknown");
        // Malformado também é rejeição, não ausência.
        let mut data = json!({"toolKind":"other"});
        record(&mut data, &codex("memory"), Some(&known), crate::agent::bot::Adapter::Codex, Some("2.2.2"));
        revise(
            &mut data,
            &codex_raw(&json!({"server":"memory","tool":"remember","serverName":123})),
            Some(&known),
            crate::agent::bot::Adapter::Codex,
            Some("2.2.2"),
        );
        assert_eq!(data["hubToolOrigin"], "unknown");
        assert!(data["mcpServer"].is_null());
        assert_eq!(data["mcpServerClaimed"], "memory");
        // Claude: internal pelo toolName; update posterior com argumentos em
        // rawInput mas sem _meta preserva internal — args nunca são identidade.
        let mut data = json!({"toolKind":"other"});
        record(
            &mut data,
            &json!({"_meta":{"claudeCode":{"toolName":"mcp__memory__remember"}}}),
            Some(&known),
            crate::agent::bot::Adapter::Claude,
            None,
        );
        assert_eq!(data["hubToolOrigin"], "internal");
        revise(
            &mut data,
            &json!({"rawInput":{"server":"memory"}}),
            Some(&known),
            crate::agent::bot::Adapter::Claude,
            None,
        );
        assert_eq!(data["hubToolOrigin"], "internal");
        // Alegação divergente comprovada pelo adaptador rebaixa para unknown e
        // limpa a prova anterior: o mesmo toolCallId não troca de servidor.
        revise(
            &mut data,
            &json!({"_meta":{"claudeCode":{"toolName":"mcp__email__read"}},"rawInput":{}}),
            Some(&known),
            crate::agent::bot::Adapter::Claude,
            None,
        );
        assert_eq!(data["hubToolOrigin"], "unknown");
        assert!(data["mcpServer"].is_null());
        assert_eq!(data["mcpServerClaimed"], "email");
        // Update que repete a identidade do adaptador (o tool_call_update real
        // do claude-agent-acp carrega `_meta.claudeCode.toolName`) mantém
        // internal — os argumentos em rawInput continuam sem virar identidade.
        let mut data = json!({"toolKind":"other"});
        record(
            &mut data,
            &json!({"_meta":{"claudeCode":{"toolName":"mcp__memory__remember"}}}),
            Some(&known),
            crate::agent::bot::Adapter::Claude,
            None,
        );
        revise(
            &mut data,
            &json!({"_meta":{"claudeCode":{"toolName":"mcp__memory__remember"}},"rawInput":{"server":"email"}}),
            Some(&known),
            crate::agent::bot::Adapter::Claude,
            None,
        );
        assert_eq!(data["hubToolOrigin"], "internal");
        assert_eq!(data["mcpServer"], "memory");
        assert_eq!(data["mcpServerClaimed"], "memory");
    }

    #[test]
    fn claimed_e_rotulo_de_exibicao_sem_prova() {
        let known: HashSet<String> = ["memory".to_owned(), "manage".to_owned()].into_iter().collect();
        // Envelope Codex marcado mas rejeitado: nada de prova em `mcpServer`, mas
        // o rótulo de exibição mostra o servidor que o adaptador alegou.
        for update in [
            codex_raw(&json!({"server":"manage","tool":"create_bot","serverName":"email"})),
            codex_raw(&json!({"server":"manage","tool":"create_bot","serverName":123})),
        ] {
            let mut data = json!({"toolKind":"other"});
            record(&mut data, &update, Some(&known), crate::agent::bot::Adapter::Codex, Some("2.2.2"));
            assert_eq!(data["hubToolOrigin"], "unknown", "{update}");
            assert!(data["mcpServer"].is_null(), "{update}");
            assert_eq!(data["mcpServerClaimed"], "manage", "{update}");
        }
        // `serverName` solto e `server` sem `tool` não viram nem prova nem rótulo.
        for update in [codex_raw(&json!({"serverName":"manage"})), json!({"rawInput":{"server":"manage"}})] {
            let mut data = json!({"toolKind":"other"});
            record(&mut data, &update, Some(&known), crate::agent::bot::Adapter::Codex, Some("2.2.2"));
            assert_eq!(data["hubToolOrigin"], "unknown", "{update}");
            assert!(data["mcpServer"].is_null(), "{update}");
            assert!(data["mcpServerClaimed"].is_null(), "{update}");
        }
        // Alegação válida preenche os dois campos com o mesmo nome.
        let mut data = json!({"toolKind":"other"});
        record(&mut data, &codex("manage"), Some(&known), crate::agent::bot::Adapter::Codex, Some("2.2.2"));
        assert_eq!(data["mcpServer"], "manage");
        assert_eq!(data["mcpServerClaimed"], "manage");
        // No Claude o rótulo também vem só do canal do adaptador.
        let mut data = json!({"toolKind":"other"});
        record(
            &mut data,
            &json!({"_meta":{"claudeCode":{"toolName":"mcp__memory__remember"}},"rawInput":{"server":"email"}}),
            Some(&known),
            crate::agent::bot::Adapter::Claude,
            None,
        );
        assert_eq!(data["hubToolOrigin"], "internal");
        assert_eq!(data["mcpServer"], "memory");
        assert_eq!(data["mcpServerClaimed"], "memory");
    }

    /// Só a criação de rotina com identidade comprovada fica fora da marca D125.
    #[test]
    fn routines_do_hub_sao_internas_e_nao_marcam_conteudo_de_fora() {
        let definition = json!({"name":"routines","command":"/hub",
            "args":["mcp","routines","--bot","donna","--port","123"],
            "env":[{"name":"DONNABOT_SPACE_ID","value":"personal-fixture"},
                {"name":SESSION_SLOT_ENV,"value":MAIN_SLOT},
                {"name":crate::fork::sessoes::bindings::ENV,"value":"binding-fixture"}]});
        let definitions = json!([definition]);
        let known = internal_servers(
            &definitions,
            Path::new("/hub"),
            "donna",
            (123, None),
            "personal-fixture",
            None,
            None,
            Some("binding-fixture"),
        );
        assert!(known.contains("routines"));
        assert!(
            !internal_servers(
                &definitions,
                Path::new("/hub"),
                "donna",
                (123, None),
                "personal-fixture",
                None,
                None,
                None
            )
            .contains("routines")
        );
        let mut forged = definitions.clone();
        forged[0]["command"] = "/not-the-hub".into();
        assert!(
            !internal_servers(
                &forged,
                Path::new("/hub"),
                "donna",
                (123, None),
                "personal-fixture",
                None,
                None,
                Some("binding-fixture")
            )
            .contains("routines")
        );
        let mut data = json!({"toolKind":"other"});
        record(
            &mut data,
            &json!({"_meta":{"claudeCode":{"toolName":"mcp__routines__save_routine"}},"rawInput":{}}),
            Some(&known),
            crate::agent::bot::Adapter::Claude,
            None,
        );
        assert_eq!(data["hubToolOrigin"], "internal");
        assert_eq!(data["mcpServer"], "routines");
        assert!(!exposes_memory(&data), "chamada de rotina não é conteúdo de fora");
        for method in ["list_routines", "routine_webhook", "set_routine_enabled", "delete_routine", "run_routine"] {
            let mut data = json!({"toolKind":"other"});
            let update = json!({"_meta":{"claudeCode":{"toolName":format!("mcp__routines__{method}")}},"rawInput":{}});
            record(&mut data, &update, Some(&known), crate::agent::bot::Adapter::Claude, None);
            assert!(exposes_memory(&data), "{method} deve marcar exposição");
            assert_eq!(data["hubToolOrigin"], "unknown");
        }
        let mut update =
            json!({"_meta":{"claudeCode":{"toolName":"mcp__routines__save_routine"}},"rawInput":{"id":"existente"}});
        let mut data = json!({"toolKind":"other"});
        record(&mut data, &update, Some(&known), crate::agent::bot::Adapter::Claude, None);
        assert!(exposes_memory(&data));
        update["rawInput"] = json!({});
        let mut data = json!({"toolKind":"other"});
        record(&mut data, &update, None, crate::agent::bot::Adapter::Claude, None);
        assert!(exposes_memory(&data));
        for version in ["2.2.2", "2.6.0", "unknown"] {
            let mut data = json!({"toolKind":"other"});
            let update = json!({"_meta":{"is_mcp_tool_call":true},"rawInput":{"server":"routines","tool":"save_routine","arguments":{}}});
            record(&mut data, &update, Some(&known), crate::agent::bot::Adapter::Codex, Some(version));
            assert_eq!(exposes_memory(&data), version == "unknown");
        }
    }
}

#[cfg(test)]
mod binding_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn vinculo_faz_parte_do_env_exato_e_nao_e_reconhecido_por_outro_caminho() {
        let definition = json!({"name":"memory","command":"/hub",
            "args":["mcp","memory","--bot","bot","--port","123"],
            "env":[{"name":"DONNABOT_SPACE_ID","value":"personal"},
                {"name":SESSION_SLOT_ENV,"value":MAIN_SLOT},
                {"name":crate::fork::memory_auto::transport::ENV,"value":uuid::Uuid::new_v4().to_string()},
                {"name":crate::fork::sessoes::bindings::ENV,"value":"binding-a"}]});
        let definitions = json!([definition]);
        assert!(
            internal_servers(
                &definitions,
                Path::new("/hub"),
                "bot",
                (123, None),
                "personal",
                None,
                None,
                Some("binding-a")
            )
            .contains("memory")
        );
        for binding in [None, Some("binding-b")] {
            assert!(
                internal_servers(&definitions, Path::new("/hub"), "bot", (123, None), "personal", None, None, binding)
                    .is_empty()
            );
        }
        let mut swapped = definitions.clone();
        swapped[0]["env"].as_array_mut().unwrap().swap(2, 3);
        let mut duplicate = definitions.clone();
        duplicate[0]["env"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":crate::fork::sessoes::bindings::ENV,"value":"binding-a"}));
        let mut extra = definitions.clone();
        extra[0]["env"].as_array_mut().unwrap().push(json!({"name":"extra","value":"x"}));
        let mut invalid = definitions.clone();
        invalid[0]["env"][2]["value"] = json!("not-a-capability");
        for malformed in [swapped, duplicate, extra, invalid] {
            assert!(
                internal_servers(
                    &malformed,
                    Path::new("/hub"),
                    "bot",
                    (123, None),
                    "personal",
                    None,
                    None,
                    Some("binding-a")
                )
                .is_empty()
            );
        }
    }
}

#[cfg(test)]
mod memory_transport_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn memoria_exige_env_unico_exato_sem_ampliar_outros_servidores() {
        let mut memory = json!({"name":"memory","command":"/hub",
            "args":["mcp","memory","--bot","bot","--port","123"],
            "env":[{"name":"DONNABOT_SPACE_ID","value":"space"},{"name":SESSION_SLOT_ENV,"value":"main"},
                {"name":crate::fork::memory_auto::transport::ENV,"value":uuid::Uuid::new_v4().to_string()}]});
        let recognized = |definition: Value| {
            internal_servers(&json!([definition]), Path::new("/hub"), "bot", (123, None), "space", None, None, None)
                .contains("memory")
        };
        assert!(recognized(memory.clone()));
        memory["env"][2]["name"] = json!("arbitrary");
        assert!(!recognized(memory.clone()));
        memory["env"][2]["name"] = json!(crate::fork::memory_auto::transport::ENV);
        memory["env"].as_array_mut().unwrap().push(json!({"name":"extra","value":"x"}));
        assert!(!recognized(memory));
    }

    #[test]
    fn memoria_contextual_exige_transporte_e_binding_emitidos_sem_env_extra() {
        let capability = uuid::Uuid::new_v4().to_string();
        let mut definition = json!({"name":"memory","command":"/hub",
            "args":["mcp","memory","--bot","bot","--port","123"],
            "env":[{"name":"DONNABOT_SPACE_ID","value":"space"},
                {"name":SESSION_SLOT_ENV,"value":"conversation:chat"},
                {"name":crate::fork::spaces::tool_context::BINDING_ENV,"value":"binding"},
                {"name":crate::fork::memory_auto::transport::ENV,"value":capability},
                {"name":crate::fork::sessoes::bindings::ENV,"value":"session-original"}]});
        let recognized = |definition: Value| {
            internal_servers(
                &json!([definition]),
                Path::new("/hub"),
                "bot",
                (123, None),
                "space",
                Some("conversation:chat"),
                Some("binding"),
                Some("session-original"),
            )
            .contains("memory")
        };
        assert!(recognized(definition.clone()));
        definition["env"][2]["value"] = "revogado".into();
        assert!(!recognized(definition.clone()));
        definition["env"][2]["value"] = "binding".into();
        definition["env"][3]["name"] = "outro-env".into();
        assert!(!recognized(definition.clone()));
        definition["env"][3]["name"] = crate::fork::memory_auto::transport::ENV.into();
        definition["env"].as_array_mut().unwrap().push(json!({"name":"extra","value":"x"}));
        assert!(!recognized(definition));
    }
}

#[cfg(test)]
mod dual_binding_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn env_contextual_conserva_ordem_e_tokens_distintos_sem_extras_ou_duplicados() {
        let capability = uuid::Uuid::new_v4().to_string();
        let definition = json!({"name":"memory","command":"/hub","args":["mcp","memory","--bot","bot","--port","123"],
            "env":[{"name":"DONNABOT_SPACE_ID","value":"space"},{"name":SESSION_SLOT_ENV,"value":"conversation:chat"},
                {"name":crate::fork::spaces::tool_context::BINDING_ENV,"value":"tool-original"},
                {"name":crate::fork::memory_auto::transport::ENV,"value":capability},
                {"name":crate::fork::sessoes::bindings::ENV,"value":"session-original"}]});
        let accepts = |value: Value| {
            internal_servers(
                &json!([value]),
                Path::new("/hub"),
                "bot",
                (123, None),
                "space",
                Some("conversation:chat"),
                Some("tool-original"),
                Some("session-original"),
            )
            .contains("memory")
        };
        assert!(accepts(definition.clone()));
        for mode in ["swap", "duplicate", "extra", "value"] {
            let mut changed = definition.clone();
            let env = changed["env"].as_array_mut().unwrap();
            match mode {
                "swap" => env.swap(2, 4),
                "duplicate" => env.push(env[4].clone()),
                "extra" => env.push(json!({"name":"EXTRA","value":"x"})),
                "value" => env[4]["value"] = "tool-original".into(),
                _ => unreachable!(),
            }
            assert!(!accepts(changed), "{mode}");
        }
    }
}
