//! Paridade entre o Motor Mexerica e os motores ACP atuais (Claude Code e Codex):
//! para o MESMO conjunto de ferramentas, a decisão de cartão (pede ou não
//! aprovação) e a de exposição a conteúdo externo (`exposes_memory`) têm de ser
//! iguais. Falha se divergirem em qualquer caso. Se a regra mudar (grupos de
//! liberação por conector do Marketplace), muda nos dois lugares juntos.
#![allow(clippy::unwrap_used)]

use super::mcp_host::ToolClass;
use crate::agent::bot::Adapter;
use crate::guard_hub::tool_origin::{exposes_memory, record};
use serde_json::{Value, json};
use std::collections::HashSet;

struct Caso {
    nome: &'static str,
    servidor: &'static str,
    ferramenta: &'static str,
    /// Servidor interno do Hub (definição exata `<exe> mcp <nome>`).
    hub: bool,
}

const fn caso(nome: &'static str, servidor: &'static str, ferramenta: &'static str, hub: bool) -> Caso {
    Caso { nome, servidor, ferramenta, hub }
}

/// A dica `readOnlyHint` não entra: nem o Claude nem o Hub liberam por ela, e
/// o caso "com e sem" confere exatamente isso (mesma decisão nos dois).
const CASOS: &[Caso] = &[
    caso("leitura de servidor interno do Hub", "chat", "ler", true),
    caso("escrita de servidor interno do Hub", "chat", "gravar", true),
    caso("computers, leitura", "computers", "ler", true),
    caso("computers, escrita", "computers", "clicar", true),
    caso("conector com readOnlyHint", "conector", "ler", false),
    caso("conector sem readOnlyHint", "conector", "gravar", false),
    caso("conector curado", "donna-curado-x", "ler", false),
    caso("composio", "composio", "executar", false),
    caso("team", "team", "bot_request_status", false),
    caso("tasks", "tasks", "listar", false),
];

/// Cartão nos motores ACP: o Claude abre cartão a menos que uma regra `allow`
/// (as que o Hub entrega em `claude_meta`) cubra `mcp__<servidor>` ou
/// `mcp__<servidor>__<ferramenta>`. O Codex roda em `on-request` sem regra
/// por ferramenta do Hub e também abre o cartão para MCP.
fn acp_pede_cartao(caso: &Caso) -> bool {
    let cfg: crate::store::BotConfig = serde_json::from_value(json!({"id": "paridade", "name": "Paridade"})).unwrap();
    let meta = crate::agent::bot::claude_meta("sistema", &cfg);
    let allow = meta["claudeCode"]["options"]["settings"]["permissions"]["allow"].as_array().unwrap().clone();
    let nome = format!("mcp__{}__{}", caso.servidor, caso.ferramenta);
    let servidor = format!("mcp__{}", caso.servidor);
    !allow.iter().filter_map(Value::as_str).any(|regra| regra == nome || regra == servidor)
}

fn mexerica_pede_cartao(caso: &Caso) -> bool {
    let classe = ToolClass { server: caso.servidor.to_owned(), tool: caso.ferramenta.to_owned(), hub: caso.hub };
    !classe.released()
}

/// Servidores que o Hub comprova como internos na sessão (o que o bot passa ao `record`).
fn internos(caso: &Caso) -> HashSet<String> {
    crate::agent::engine_permissions::HUB_SERVERS
        .iter()
        .filter(|nome| caso.hub && **nome == caso.servidor)
        .map(|nome| (*nome).to_owned())
        .collect()
}

fn expoe(caso: &Caso, adapter: Adapter, update: &Value, version: Option<&str>) -> bool {
    let mut data = json!({"toolKind": "other"});
    record(&mut data, update, Some(&internos(caso)), adapter, version);
    exposes_memory(&data)
}

fn expoe_mexerica(caso: &Caso) -> bool {
    let update = json!({"_meta": {"mexericaTool": {"server": caso.servidor, "tool": caso.ferramenta}}, "rawInput": {}});
    expoe(caso, Adapter::Mexerica, &update, None)
}

fn expoe_claude(caso: &Caso) -> bool {
    let nome = format!("mcp__{}__{}", caso.servidor, caso.ferramenta);
    let update = json!({"_meta": {"claudeCode": {"toolName": nome}}, "rawInput": {}});
    expoe(caso, Adapter::Claude, &update, None)
}

fn expoe_codex(caso: &Caso) -> bool {
    let update = json!({"_meta": {"is_mcp_tool_call": true},
        "rawInput": {"server": caso.servidor, "tool": caso.ferramenta, "arguments": {}}});
    expoe(caso, Adapter::Codex, &update, Some("2.6.0"))
}

#[test]
fn cartao_do_motor_mexerica_e_igual_ao_dos_motores_acp() {
    let divergencias: Vec<String> = CASOS
        .iter()
        .filter(|caso| mexerica_pede_cartao(caso) != acp_pede_cartao(caso))
        .map(|caso| {
            format!("{}: Mexerica pede={}, ACP pede={}", caso.nome, mexerica_pede_cartao(caso), acp_pede_cartao(caso))
        })
        .collect();
    assert!(divergencias.is_empty(), "cartão diverge dos motores ACP: {divergencias:#?}");
}

#[test]
fn exposicao_a_conteudo_externo_do_motor_mexerica_e_igual_a_dos_motores_acp() {
    let divergencias: Vec<String> = CASOS
        .iter()
        .filter_map(|caso| {
            let (mexerica, claude, codex) = (expoe_mexerica(caso), expoe_claude(caso), expoe_codex(caso));
            (mexerica != claude || mexerica != codex)
                .then(|| format!("{}: Mexerica={mexerica}, Claude={claude}, Codex={codex}", caso.nome))
        })
        .collect();
    assert!(divergencias.is_empty(), "exposição diverge dos motores ACP: {divergencias:#?}");
}

#[test]
fn a_tabela_cobre_internos_e_conectores_nos_dois_lados_da_decisao() {
    // Evita um teste vazio: a tabela tem interno e conector, e `computers` expõe.
    assert!(CASOS.iter().any(|c| c.hub) && CASOS.iter().any(|c| !c.hub));
    let computers = CASOS.iter().find(|c| c.servidor == "computers").unwrap();
    assert!(expoe_mexerica(computers) && expoe_claude(computers) && expoe_codex(computers));
    let interno = CASOS.iter().find(|c| c.servidor == "chat").unwrap();
    assert!(!expoe_mexerica(interno) && !expoe_claude(interno) && !expoe_codex(interno));
}
