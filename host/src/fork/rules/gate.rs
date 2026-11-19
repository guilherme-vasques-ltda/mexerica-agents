//! Porteiro 13.1b: decide os pedidos de permissão do motor (ACP
//! `session/request_permission`) sobre o núcleo puro. O chamador já conferiu
//! sessão e vínculo; aqui valem as regras, o nível efetivo e a auditoria.
//! Efeito que o pedido não comprova nunca é liberado: sem prova, pergunta.

use super::{
    avaliacao::{self, Correspondencia, Decisao, Mediacao, Origem, Pedido, Validade, decisao_regra},
    floor::{Categoria, Efeito, categoria},
    rule::ApprovalLevel,
    store::{GateDecision, Snapshot},
};
use crate::{agent::bot::Adapter, store::Store};
use serde_json::Value;

/// O que o chamador já comprovou do pedido antes de chamar o porteiro.
pub struct GateRequest<'a> {
    /// Params completos do `session/request_permission`.
    pub params: &'a Value,
    /// Entrada `tool` gravada para o `toolCallId` correlacionado, quando existe.
    pub recorded: Option<&'a Value>,
    pub bot_id: &'a str,
    /// Adaptador que emitiu o pedido: decide qual metadado é prova de identidade.
    pub adapter: Adapter,
    /// Nível legado ainda vigente (`Permission` do bot).
    pub permission: crate::store::Permission,
    /// Origem do turno que gerou o pedido.
    pub origin: Origem,
}

/// O que o porteiro decidiu; o chamador transforma em resposta ou cartão.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateOutcome {
    /// Regra ou nível cobrem o pedido com efeito comum comprovado: `allow_once`.
    Allow,
    /// Regra Recusar casou: `reject_once` ou cancelamento, nunca cartão.
    Deny,
    /// Tudo o mais pergunta no cartão — inclusive efeito não comprovado.
    Ask,
}

/// Decide e registra os vereditos automáticos. `Ask` já aparece como cartão
/// no chat; o evento não substitui a interface. Política ilegível fecha para
/// `Deny`: sem leitura da política, nenhuma aprovação é segura.
pub fn decide(store: &Store, req: &GateRequest) -> GateOutcome {
    let snapshot = match store.donna_rules() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            tracing::error!(bot = %req.bot_id, %error, "gate: política ilegível; pedido recusado. Verifique e recupere o banco de regras");
            return GateOutcome::Deny;
        }
    };
    let level = effective_level(&snapshot, req.bot_id, req.permission);
    let fields = pedido_fields(req.params, req.recorded, req.adapter);
    let pedido = Pedido {
        bot: req.bot_id,
        ferramenta: &fields.ferramenta,
        acao: &fields.acao,
        alvo: &fields.alvo,
        // Nenhum pedido ACP comprova efeitos estruturados nesta etapa: o
        // `toolCall` descreve a chamada, não o efeito. Efeitos provados são
        // obrigação de um adaptador futuro; até lá `Opaco` mantém tudo no
        // cartão — menos a Recusar, que vence antes dos efeitos.
        efeitos: &[Efeito::Opaco],
        origem: req.origin,
        mediacao: Mediacao::Comprovada,
        // Sessão e vínculo já foram conferidos pelo chamador neste pedido.
        politica_e_autoridade: Validade::Atual,
        nivel: level,
    };
    let decisao = avaliacao::avaliar(&pedido, &snapshot.rules);
    let decided = decisao_regra(&pedido, &snapshot.rules, decisao);
    let outcome = match decisao {
        Decisao::Permitir => GateOutcome::Allow,
        // Só a Recusar causada por regra prova proibição. Vínculo ou campo
        // insuficiente para avaliar pergunta: o cartão ainda é ponto seguro.
        Decisao::Recusar if decided.is_some() => GateOutcome::Deny,
        _ => GateOutcome::Ask,
    };
    if outcome == GateOutcome::Ask {
        return outcome;
    }
    let (rule, certeza) = decided.unzip();
    let (verdict, reason) = match outcome {
        GateOutcome::Allow => ("allow", if rule.is_some() { "allow_rule" } else { "full_access" }),
        GateOutcome::Deny => ("deny", if certeza == Some(Correspondencia::Sim) { "deny_rule" } else { "unknown_deny" }),
        GateOutcome::Ask => unreachable!(),
    };
    let floor_category = pedido.efeitos.iter().find_map(|efeito| categoria(*efeito)).map(category_name);
    let origin = match req.origin {
        Origem::DonoDigitadoAutenticado => "owner_typed",
        Origem::Derivada => "derived",
    };
    let decision = GateDecision {
        bot_id: req.bot_id,
        origin,
        tool: (!fields.ferramenta.is_empty()).then_some(fields.ferramenta.as_str()),
        verdict,
        reason,
        rule_id: rule.map(|rule| rule.id.as_str()),
        rule_sentence: rule.map(super::rule::Rule::sentence),
        policy_revision: snapshot.revision,
        floor_category,
    };
    if let Err(error) = store.donna_rule_gate_decision(&decision) {
        match outcome {
            // Allow sem trilha não se sustenta: o pedido vira cartão.
            GateOutcome::Allow => {
                tracing::warn!(bot = %req.bot_id, %error, "gate: auditoria falhou; pedido segue para cartão");
                return GateOutcome::Ask;
            }
            // Um Deny sem auditoria continua Deny — rebaixá-lo a cartão
            // deixaria o dono aprovar o que a regra proíbe.
            GateOutcome::Deny => {
                tracing::warn!(bot = %req.bot_id, %error, "gate: auditoria falhou num deny; mantendo a recusa");
            }
            GateOutcome::Ask => unreachable!(),
        }
    }
    outcome
}

/// O mais restritivo entre a projeção preparada (geral e do bot) e o legado,
/// até a troca explícita de autoridade. `auto` legado equivale a Auto aprovar
/// com chão; nenhum caminho prepara Acesso total.
fn effective_level(snapshot: &Snapshot, bot_id: &str, legacy: crate::store::Permission) -> ApprovalLevel {
    let rank = |level: ApprovalLevel| match level {
        ApprovalLevel::Ask => 0,
        ApprovalLevel::AutoApprove => 1,
        ApprovalLevel::FullAccess => 2,
    };
    let legacy_level = match legacy {
        crate::store::Permission::Ask => ApprovalLevel::Ask,
        crate::store::Permission::Auto => ApprovalLevel::AutoApprove,
    };
    let prepared = snapshot
        .approvals
        .iter()
        .filter(|approval| approval.bot_id.is_none() || approval.bot_id.as_deref() == Some(bot_id))
        .map(|approval| approval.level)
        .min_by_key(|level| rank(*level));
    match prepared {
        Some(level) if rank(level) < rank(legacy_level) => level,
        _ => legacy_level,
    }
}

struct Fields {
    ferramenta: String,
    acao: String,
    alvo: String,
}

/// Identidade e alvo literais do pedido, só com prova do adaptador. Nada é
/// normalizado: a regra nova compara texto exato, e argumento de ferramenta
/// nunca vira identidade (mesma prova do `tool_origin`). Quando o pedido não
/// traz o envelope — caso correlacionado — a entrada gravada do `tool_call`
/// supre a identidade que o Hub já validou contra os servidores da sessão.
fn pedido_fields(params: &Value, recorded: Option<&Value>, adapter: Adapter) -> Fields {
    let tool = &params["toolCall"];
    let raw = &tool["rawInput"];
    let kind = tool["kind"].as_str().unwrap_or_default();

    // MCP comprovado, a mesma prova do `tool_origin`: Claude pelo `toolName`
    // do `_meta`; Codex pelo envelope `server`/`serverName` + `tool` do
    // `rawInput` (no Claude esses campos são argumentos da ferramenta).
    let claude_name =
        tool.pointer("/_meta/claudeCode/toolName").and_then(Value::as_str).filter(|name| !name.is_empty());
    let mut acao_mcp = None;
    let mut codex_envelope = false;
    let mut ferramenta = match adapter {
        Adapter::Claude => match claude_name {
            Some(name) => {
                acao_mcp = name
                    .strip_prefix("mcp__")
                    .and_then(|rest| rest.split_once("__"))
                    .map(|(_, method)| method)
                    .filter(|method| !method.is_empty())
                    .map(str::to_owned);
                name.to_owned()
            }
            None => kind.to_owned(),
        },
        Adapter::Codex => {
            let a = raw["server"].as_str();
            let b = raw["serverName"].as_str();
            let malformed = ["server", "serverName"].iter().any(|key| raw.get(key).is_some_and(|v| !v.is_string()));
            let server = if malformed || (a.is_some() && b.is_some() && a != b) { None } else { a.or(b) };
            match server {
                Some(server) => {
                    codex_envelope = true;
                    let method = raw["tool"].as_str().filter(|tool| !tool.is_empty());
                    acao_mcp = method.map(str::to_owned);
                    match method {
                        Some(method) => format!("mcp__{server}__{method}"),
                        None => format!("mcp__{server}"),
                    }
                }
                None => kind.to_owned(),
            }
        }
        Adapter::Mexerica => {
            // O Hub escreve `_meta.mexericaTool` a partir dos servidores que abriu: o
            // mesmo nome `mcp__<servidor>__<ferramenta>` que as regras comparam no Claude.
            let server = tool.pointer("/_meta/mexericaTool/server").and_then(Value::as_str).filter(|s| !s.is_empty());
            let method = tool.pointer("/_meta/mexericaTool/tool").and_then(Value::as_str).filter(|s| !s.is_empty());
            match (server, method) {
                (Some(server), Some(method)) => {
                    acao_mcp = Some(method.to_owned());
                    format!("mcp__{server}__{method}")
                }
                _ => kind.to_owned(),
            }
        }
        Adapter::Other => kind.to_owned(),
    };
    // Pedido correlacionado sem identidade própria: `ferramenta == kind` marca
    // que nenhum metadado do adaptador provou nome (nomes provados são
    // `mcp__*` ou o toolName do Claude, nunca o kind ACP). A entrada gravada
    // supre a identidade MCP que o Hub já validou contra os servidores da
    // sessão — a mesma prova que `tool_origin` usou para gravá-la.
    if (ferramenta.is_empty() || ferramenta == kind)
        && let Some(name) = recorded_mcp_name(recorded)
    {
        ferramenta = name;
    }
    if ferramenta.is_empty() {
        tool["title"].as_str().unwrap_or_default().clone_into(&mut ferramenta);
    }
    let acao = acao_mcp.unwrap_or_else(|| kind.to_owned());

    // O alvo é o argumento primário, literal: no envelope Codex os argumentos
    // vêm em `arguments`; nos demais, o próprio `rawInput` são os argumentos.
    let args = if codex_envelope && raw["arguments"].is_object() { &raw["arguments"] } else { raw };
    let alvo =
        primary_target(args).or_else(|| tool["locations"][0]["path"].as_str().map(str::to_owned)).unwrap_or_default();

    Fields { ferramenta, acao, alvo }
}

/// `mcp__<servidor>__<método>` a partir da entrada gravada: `mcpServer` só é
/// preenchido quando a identidade foi comprovada na gravação, e o título
/// `mcp.<servidor>.<método>` traz o método da notificação do motor.
fn recorded_mcp_name(recorded: Option<&Value>) -> Option<String> {
    let data = recorded?;
    let server = data["mcpServer"].as_str().filter(|server| !server.is_empty())?;
    if let Some(title) = data["title"].as_str()
        && let Some(rest) = title.strip_prefix("mcp.")
        && let Some((title_server, method)) = rest.split_once('.')
        && title_server == server
        && !method.is_empty()
    {
        return Some(format!("mcp__{server}__{method}"));
    }
    Some(format!("mcp__{server}"))
}

/// Primeiro alvo textual relevante, sem interpretar nem normalizar.
fn primary_target(args: &Value) -> Option<String> {
    let object = args.as_object()?;
    for key in ["command", "path", "file_path", "filePath", "notebook_path", "target_file", "url"] {
        let Some(value) = object.get(key) else { continue };
        let text = value.as_str().map(str::to_owned).or_else(|| {
            value.as_array().map(|parts| parts.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "))
        });
        if let Some(text) = text.filter(|text| !text.is_empty()) {
            return Some(text);
        }
    }
    None
}

fn category_name(category: Categoria) -> &'static str {
    match category {
        Categoria::Dinheiro => "dinheiro",
        Categoria::Destruicao => "destruicao",
        Categoria::Elevacao => "elevacao",
        Categoria::Segredos => "segredos",
        Categoria::PastaHubMotores => "pasta_hub_motores",
        Categoria::ApiHubMotores => "api_hub_motores",
        Categoria::ConfiguracaoHubMotores => "configuracao_hub_motores",
        Categoria::ExecucaoFutura => "execucao_futura",
        Categoria::ConfirmacaoLocal => "confirmacao_local",
        Categoria::Desconhecida => "desconhecida",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::LockExt;
    use crate::fork::rules::rule::{Effect, Rule};
    use crate::store::{Permission, Store};
    use serde_json::json;
    use std::path::Path;

    fn regra(effect: Effect, field: &str, value: &str) -> Rule {
        Rule {
            id: "regra-teste".into(),
            effect,
            field: field.into(),
            op: "igual".into(),
            value: value.into(),
            bot_id: None,
            tool: None,
            scope: None,
            enabled: true,
            revision: 1,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn pedido_params(tool_call: &Value) -> Value {
        json!({
            "sessionId": "sessao-1",
            "toolCall": tool_call,
            "options": [
                {"optionId": "aceitar", "kind": "allow_once"},
                {"optionId": "recusar", "kind": "reject_once"},
                {"optionId": "sempre", "kind": "allow_always"},
            ],
        })
    }

    fn request<'a>(params: &'a Value, recorded: Option<&'a Value>, permission: Permission) -> GateRequest<'a> {
        GateRequest {
            params,
            recorded,
            bot_id: "bot-1",
            adapter: Adapter::Codex,
            permission,
            origin: Origem::DonoDigitadoAutenticado,
        }
    }

    fn codex_mcp() -> Value {
        pedido_params(&json!({
            "toolCallId": "chamada-1",
            "kind": "execute",
            "title": "mcp.external.test",
            "rawInput": {"server": "external", "tool": "test", "arguments": {"command": "ls"}},
        }))
    }

    #[test]
    fn identidade_mcp_codex_vem_do_envelope_e_nao_dos_argumentos() {
        let fields = pedido_fields(&codex_mcp(), None, Adapter::Codex);
        assert_eq!(fields.ferramenta, "mcp__external__test");
        assert_eq!(fields.acao, "test");
        assert_eq!(fields.alvo, "ls");
    }

    #[test]
    fn mexerica_usa_o_mesmo_nome_mcp_das_regras_so_pelo_canal_do_hub() {
        let params = pedido_params(&json!({
            "toolCallId": "mxt-1",
            "kind": "other",
            "title": "mcp__chat__ler",
            "_meta": {"mexericaTool": {"server": "chat", "tool": "ler"}},
            "rawInput": {"server": "memory", "path": "/tmp/alvo"},
        }));
        let fields = pedido_fields(&params, None, Adapter::Mexerica);
        assert_eq!(fields.ferramenta, "mcp__chat__ler");
        assert_eq!(fields.acao, "ler");
        assert_eq!(fields.alvo, "/tmp/alvo");
        // Sem o canal do Hub, o `server` do rawInput é só dado: cai no kind.
        let sem_canal = pedido_params(&json!({
            "toolCallId": "mxt-2", "kind": "execute", "rawInput": {"server": "memory", "command": "ls"},
        }));
        assert_eq!(pedido_fields(&sem_canal, None, Adapter::Mexerica).ferramenta, "execute");
        // Outro adaptador não ganha identidade só por mandar `_meta.mexericaTool`.
        assert_eq!(pedido_fields(&params, None, Adapter::Other).ferramenta, "other");
    }

    #[test]
    fn claude_nunca_promove_argumentos_a_identidade_mcp() {
        // `rawInput` do Claude são os argumentos da ferramenta: `server` ali é dado.
        let params = pedido_params(&json!({
            "toolCallId": "chamada-1",
            "kind": "other",
            "title": "ferramenta externa",
            "_meta": {"claudeCode": {"toolName": "mcp__files__act"}},
            "rawInput": {"server": "memory", "path": "/tmp/alvo"},
        }));
        let fields = pedido_fields(&params, None, Adapter::Claude);
        assert_eq!(fields.ferramenta, "mcp__files__act");
        assert_eq!(fields.acao, "act");
        assert_eq!(fields.alvo, "/tmp/alvo");

        let forjado = pedido_params(&json!({
            "toolCallId": "chamada-2",
            "kind": "execute",
            "rawInput": {"server": "memory", "command": "rm -rf /"},
        }));
        let fields = pedido_fields(&forjado, None, Adapter::Claude);
        assert_eq!(fields.ferramenta, "execute");
        assert_eq!(fields.acao, "execute");
    }

    #[test]
    fn nome_do_cartao_codex_e_claude_casa_na_ponte() {
        let codex = codex_mcp();
        let claude = pedido_params(
            &json!({"kind":"other", "_meta":{"claudeCode":{"toolName":"mcp__external__test"}}, "rawInput":{"command":"ls"}}),
        );
        for (adapter, params) in [(Adapter::Codex, codex), (Adapter::Claude, claude)] {
            let name = pedido_fields(&params, None, adapter).ferramenta;
            assert_eq!(name, "mcp__external__test");
            let store = Store::open(Path::new(":memory:")).unwrap();
            let revision = store.donna_rules().unwrap().revision;
            store.donna_rule_save(regra(Effect::Deny, "ferramenta", &name), revision, true).unwrap();
            assert!(
                super::super::mcp::check(
                    &store,
                    "bot-1",
                    "external",
                    "tools/call",
                    &json!({"name":"test","arguments":{"command":"ls"}})
                )
                .unwrap()
            );
        }
    }

    #[test]
    fn envelope_codex_conflitante_ou_malformado_cai_para_o_kind() {
        for raw in [
            json!({"server": "a", "serverName": "b", "tool": "x"}),
            json!({"server": 7, "tool": "x"}),
            json!({"server": "a", "serverName": 9}),
        ] {
            let params = pedido_params(&json!({
                "toolCallId": "chamada-1", "kind": "execute", "rawInput": raw,
            }));
            let fields = pedido_fields(&params, None, Adapter::Codex);
            assert_eq!(fields.ferramenta, "execute", "{raw}");
        }
    }

    #[test]
    fn pedido_correlacionado_usa_a_identidade_gravada_pelo_hub() {
        let params = pedido_params(&json!({"toolCallId": "chamada-1", "kind": "execute"}));
        let recorded = json!({"mcpServer": "external", "title": "mcp.external.test", "toolKind": "execute"});
        let fields = pedido_fields(&params, Some(&recorded), Adapter::Codex);
        assert_eq!(fields.ferramenta, "mcp__external__test");
        // Sem título no padrão, fica o servidor comprovado.
        let recorded = json!({"mcpServer": "external", "title": "leitura", "toolKind": "execute"});
        let fields = pedido_fields(&params, Some(&recorded), Adapter::Codex);
        assert_eq!(fields.ferramenta, "mcp__external");
        // Entrada sem servidor comprovado não inventa identidade.
        let recorded = json!({"title": "mcp.external.test", "toolKind": "execute"});
        let fields = pedido_fields(&params, Some(&recorded), Adapter::Codex);
        assert_eq!(fields.ferramenta, "execute");
    }

    #[test]
    fn alvo_literal_vem_de_command_path_ou_url_sem_normalizar() {
        let params = pedido_params(&json!({
            "toolCallId": "chamada-1", "kind": "execute",
            "rawInput": {"command": ["git", "status"], "path": "/tmp/x"},
        }));
        assert_eq!(pedido_fields(&params, None, Adapter::Codex).alvo, "git status");
        let params = pedido_params(&json!({
            "toolCallId": "chamada-1", "kind": "edit",
            "rawInput": {"file_path": "notas/../segredo.txt"},
        }));
        assert_eq!(pedido_fields(&params, None, Adapter::Codex).alvo, "notas/../segredo.txt");
        let params = pedido_params(&json!({
            "toolCallId": "chamada-1", "kind": "fetch",
            "locations": [{"path": "/tmp/loc"}],
            "rawInput": {"url": "https://exemplo.com/x"},
        }));
        assert_eq!(pedido_fields(&params, None, Adapter::Codex).alvo, "https://exemplo.com/x");
    }

    #[test]
    fn politica_ilegivel_recusa() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        store.db.locked().execute("DROP TABLE donna_rules", []).unwrap();
        let params = codex_mcp();
        assert_eq!(decide(&store, &request(&params, None, Permission::Ask)), GateOutcome::Deny);
    }

    #[test]
    fn regra_recusar_automatica_nega_e_grava_auditoria() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let revision = store.donna_rules().unwrap().revision;
        let salva =
            store.donna_rule_save(regra(Effect::Deny, "ferramenta", "mcp__external__test"), revision, true).unwrap();
        let params = codex_mcp();
        let outcome = decide(&store, &request(&params, None, Permission::Ask));
        assert_eq!(outcome, GateOutcome::Deny);
        let c = store.db.locked();
        let (verdict, rule_id, tool): (String, Option<String>, Option<String>) = c
            .query_row("SELECT verdict,rule_id,tool FROM donna_rule_events WHERE kind='gate_decision'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(
            (verdict.as_str(), rule_id.as_deref(), tool.as_deref()),
            ("deny", Some(salva.id.as_str()), Some("mcp__external__test"))
        );
    }

    #[test]
    fn regra_de_rotinas_continua_recusando_leitura_com_origem_externa() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let revision = store.donna_rules().unwrap().revision;
        store
            .donna_rule_save(regra(Effect::Deny, "ferramenta", "mcp__routines__list_routines"), revision, true)
            .unwrap();
        let params = pedido_params(&json!({"toolCallId":"rotina-1", "kind":"other",
            "title":"mcp.routines.list_routines", "rawInput":{"server":"routines","tool":"list_routines","arguments":{}}}));
        let recorded = json!({"mcpServer":"routines","hubToolOrigin":"unknown","title":"mcp.routines.list_routines"});
        assert_eq!(decide(&store, &request(&params, Some(&recorded), Permission::Ask)), GateOutcome::Deny);
    }

    #[test]
    fn sem_efeito_comprovado_nada_e_liberado_automaticamente() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let revision = store.donna_rules().unwrap().revision;
        // Nem regra Permitir nem nível legado `auto` liberam: o pedido não
        // comprova efeito comum, então cai no cartão como qualquer outro.
        store.donna_rule_save(regra(Effect::Allow, "ferramenta", "mcp__external__test"), revision, true).unwrap();
        let params = codex_mcp();
        for permission in [Permission::Ask, Permission::Auto] {
            assert_eq!(decide(&store, &request(&params, None, permission)), GateOutcome::Ask);
        }
        let eventos: i64 = store
            .db
            .locked()
            .query_row("SELECT count(*) FROM donna_rule_events WHERE kind='gate_decision'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(eventos, 0, "Ask não gera evento: o cartão já é a interface");
    }

    #[test]
    fn pedido_sem_identidade_comprovada_nao_casa_regra_e_pergunta() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let revision = store.donna_rules().unwrap().revision;
        store.donna_rule_save(regra(Effect::Deny, "ferramenta", "mcp__external__test"), revision, true).unwrap();
        // Ferramenta forjada por argumento no Claude não casa a regra.
        let params = pedido_params(&json!({
            "toolCallId": "chamada-1", "kind": "execute",
            "rawInput": {"server": "external", "tool": "test"},
        }));
        let mut req = request(&params, None, Permission::Auto);
        req.adapter = Adapter::Claude;
        assert_eq!(decide(&store, &req), GateOutcome::Ask);
        // O mesmo pedido como Codex casa a regra e nega.
        assert_eq!(decide(&store, &request(&params, None, Permission::Auto)), GateOutcome::Deny);
    }

    #[test]
    fn nivel_efetivo_e_o_mais_restritivo_entre_projecao_e_legado() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let revision = store.donna_rules().unwrap().revision;
        store.donna_approval_set(None, ApprovalLevel::Ask, revision).unwrap();
        let snapshot = store.donna_rules().unwrap();
        assert_eq!(effective_level(&snapshot, "bot-1", Permission::Auto), ApprovalLevel::Ask);
        assert_eq!(effective_level(&snapshot, "bot-1", Permission::Ask), ApprovalLevel::Ask);
    }
}
