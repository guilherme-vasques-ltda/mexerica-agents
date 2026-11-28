// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Durable, non-secret connection requests. Only the client submits credentials;
//! tools receive status, never credential values.
use super::connectors;
use crate::{
    LockExt,
    agent::bot::Cmd,
    fork::spaces::tool_context::ToolContext,
    hub::Hub,
    store::{Entry, EntryKind, Lane, now_ms},
};
use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

pub const INSTRUCTIONS: &str = "A marca reservada {{senha: em site de request_login ou login de type_login, mesmo malformada, recusa antes do cofre, pedido ou preenchimento.\nO caminho protegido e a coleta privada ainda estão indisponíveis; não remova a marca nem contorne a recusa.\nNunca peça senhas/chaves no chat nem em argumentos de ferramentas.\nAntes de usar, chame ajuda_instrucoes para o guia.\nNão repita uma escrita com resultado incerto sem conferir, mesmo se a ponte continuar.\nO Hub conserva a execução original através do cofre e confere o vínculo na transação do cartão; chamada revogada ou de outra sessão é recusada, sem publicar na conversa atual nem recuar para a principal.\nNas conversas adicionais próprias, só list_connectors, request_connection e request_secret para conectores já ligados estão disponíveis; cartões conservam conversa, sessão, turno e permissão originais, sem fallback ou nova concessão.\nMCP Registry e search_connectors estão indisponíveis; não invente endpoint nem credenciais.\nPermissões inválidas ou conflitantes recusam login; informe e pare, só o provedor corrige.\nNão amplie permissões nem repita sozinho.\nOriente Entrar em Extensões, sem segredos, step-up ou repetição automática de efeitos.\nTroca de emissor ou cadastro legado sem vínculo exige DCR novo no próximo login; sem suporte, informe a recusa e pare, sem escolher outro servidor, contornar ou pedir segredos.\nLogin recusado por endereço inválido ou redirecionamento: o usuário corrige a URL no app; não sugira outro endereço nem repita.\nLogin recusado porque o servidor de login não confirmou a própria identidade: informe e pare; não escolha outro servidor, não relaxe a checagem nem peça segredos.\nLogin recusado porque o servidor de login informou um recurso adicional: informe e pare; só o provedor corrige a query dos endpoints.\nNão remova a proteção nem peça segredos.\nLogin recusado porque o serviço não confirmou a identidade do recurso: informe e pare; só o provedor corrige os metadados.\nNão troque de endpoint, não relaxe a checagem nem peça segredos.\nLogin recusado porque o serviço não declara a proteção PKCE exigida (S256): informe e pare; só o provedor corrige, não repita nem contorne.\nLogin recusado porque o retorno não confirmou o serviço esperado (emissor iss) ou veio malformado: informe e peça um novo login pelo app atualizado; não repita sozinho nem contorne.\nRevogação confirmada exige novo login seguro, sem repetir ferramentas ou efeitos automaticamente.\nIsso não concede autorização de efeitos nem isolamento e não cobre terminal ou motor nativo.\nO ledger específico do Conector não produz prova para outros domínios. decisionProtocol:0 não habilita chamadas nem pode ser contornado por JSON, voz, cartão do motor ou shell.\nO cartão do conector mostra a ação e os campos com rótulos curados do Hub e a conta de saída da fonte canônica; você não declara conta, número ou rótulo, e sem conta identificada não há aprovação.\nApós solicitar, finalize o turno e aguarde; a Mexerica retoma após o usuário concluir o setup.\nLogin MCP prioriza permissões do desafio Bearer 401 inicial; sem ele usa a metadata validada, ou omite sem confirmar o conjunto.\nOriente conexão com cabeçalho de token pelo fluxo seguro do app quando não houver cadastro automático; não repita sozinho.\nApp iPhone antigo não envia iss e é recusado quando o servidor o exige.\nFalha temporária de renovação MCP conserva login e permite tentar depois; token vencido ou recusado não é reenviado.\nO início do novo login pode já ter limpado tokens anteriores; a recusa não os restaura.\nO início da reconexão pode já ter invalidado o login anterior; a recusa não o restaura.\nNão declare que enviou nem contorne por shell.\nMudança durante espera exige nova intenção.\nNão anuncie Linear pronto.";
pub(crate) const INSTRUCTIONS_GUIDE: &str = "preparar_resposta_whatsapp prepara somente rascunho humano não decisível: use chat_id/message_id/text exatos.\nConta e Resposta a são fixas do Hub.\nO app Local lê por até 90 s sem renovação; aprovação, envio e fila indisponíveis.\nListagens revalidam antes de responder.\nLogins/computer nelas ficam para outros recortes.\nUse list_connectors para conexões ligadas para você e request_connection com connectorId existente.\nO usuário adiciona MCP por URL, comando ou configuração no app.\nO login e refresh validam a declaração OAuth de scope ou a omissão permitida; isso não introspecta token nem comprova poderes reais, perfil ou aprovação humana.\nPedido explícito exige igualdade do conjunto no piloto Mexerica.\nPar com declaração inválida ou incompatível fica indisponível localmente até nova entrada pelo app; par anterior é preservado, sem revogação remota presumida.\nLegado/omissão continuam desconhecidos, sem selo falso.\nPerfis mínimos curados continuam pendentes.\nO cadastro OAuth só é reutilizado no issuer original exato vinculado pelo Hub.\nLogin ativo e refresh legado não ganham prova retroativa nem são deslogados em massa.\nProcedência antiga é passiva; salvo, autenticado, ligado e disponível são estados diferentes.\nO retorno das pontes MCP locais e remotas é inspecionado e marcado como exposição externa antes da entrega, inclusive descoberta; falhas retêm conteúdo.\nO caminho de efeitos/recibos de chamadas curadas está indisponível em produção até a fonte de acessos, apresentação V1 e integração de HumanReviewProof.\nTimes/Atividades 05b é seu único produtor, ainda não integrado e sem API positiva estável aceita; Local, source=app e scope não comprovam aprovação humana.\nOs 90 s originais não renovam nem herdam 120 s; confirmação nos computadores do dono só com vínculo exato comprovado.\nO catálogo Linear do Hub é apenas preparatório, sem conexão, perfil aplicado ao login ou acesso; URL manual não vira curadoria.\nSerialize pedidos MCP; cursor inseguro gera descoberta incompleta, não fim da lista.\nA inspeção é parcial, com limites de Basic/path e possível redação excessiva.\nPara atualizar a chave de conector instalado, use request_secret.\nVocê só vê conectores e logins ligados para seu bot.\nPara login em site/app na tela, consulte list_logins, peça request_login se nenhum servir e digite com type_login; com várias contas, escolha o rótulo indicado ou pergunte ao usuário.\nPara código de verificação de um login já ligado, peça request_login_code; o usuário digita no cartão e type_login field \"code\" consome uma vez, sem devolver o valor.\nPedidos legados aparecem em cartões seguros na conversa.\nAs pontes MCP aceitam as revisões 2025-06-18 (preferida), 2025-03-26 e 2024-11-05; versão incompatível encerra a conexão com erro técnico, sem repetir chamada nem trocar transporte por conta própria.";

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum RequestKind {
    Connection,
    Secret,
    Login,
    /// Código de dois fatores de um login já ligado; o valor fica só na memória do Hub.
    Code,
    /// The bot's own agent (Claude Code…) needs signing in; posted by the host, not a tool.
    Agent,
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum RequestStatus {
    Pending,
    Ready,
    Cancelled,
}

fn has_status(request: &Value, status: RequestStatus) -> bool {
    serde_json::from_value::<RequestStatus>(request["status"].clone()).ok() == Some(status)
}

const MAX_AGE: i64 = 24 * 60 * 60 * 1000;
/// Exclusão comum da publicação, do consumo e do cancelamento de códigos de login.
pub(crate) static MUTATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
/// Agent sign-in cards change from bot actors and API calls alike (no `.await` while held).
pub(crate) static SIGN_IN: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `bot`'s turn failed because agent `backend` (`name`) needs signing in: a card in `lane`
/// asks the user to, and `entries` (the user messages it couldn't answer) go out again once
/// they have. A bot keeps one such card; messages that fail later join it.
pub fn invite_agent_sign_in(
    hub: &Hub,
    bot: &str,
    lane: &Lane,
    backend: &str,
    name: &str,
    entries: &[String],
) -> Result<()> {
    let _guard = SIGN_IN.locked();
    if let Some(card) = hub.store.pending_sign_ins(backend)?.into_iter().find(|card| card.bot_id == bot) {
        let mut data = card.data;
        let mut ids = data["connectionRequest"]["entryIds"].as_array().cloned().unwrap_or_default();
        for id in entries {
            if !ids.iter().any(|v| v == id.as_str()) {
                ids.push(json!(id));
            }
        }
        data["connectionRequest"]["entryIds"] = Value::Array(ids);
        hub.set_entry(&card.id, &data).ok_or_else(|| anyhow!("Could not update the sign-in request"))?;
        return Ok(());
    }
    let request = json!({"kind": RequestKind::Agent, "status": RequestStatus::Pending, "title": name, "botId": bot,
        "backend": backend, "entryIds": entries});
    // The card's title names the agent.
    let text = "Needs you to sign in. Your message goes out again once you're signed in.";
    hub.add_entry(
        lane,
        EntryKind::Notice,
        hub.store.max_turn(bot),
        &json!({"text": text, "connectionRequest": request}),
    )
    .ok_or_else(|| anyhow!("Could not save the sign-in request"))?;
    Ok(())
}

/// Agent `backend` is signed in now: its sign-in cards complete, and the messages they hold go
/// out again (a card older than a day only completes).
pub fn agent_signed_in(hub: &Hub, backend: &str) -> Result<()> {
    let _guard = SIGN_IN.locked();
    for card in hub.store.pending_sign_ins(backend)? {
        let mut data = card.data.clone();
        data["connectionRequest"]["status"] = serde_json::to_value(RequestStatus::Ready)?;
        hub.set_entry(&card.id, &data).ok_or_else(|| anyhow!("Could not complete the sign-in request"))?;
        let Some(bot) = hub.store.bot(&card.bot_id)?.filter(|b| !b.deleted) else { continue };
        if now_ms() - card.created_at > MAX_AGE {
            continue;
        }
        let ids = data["connectionRequest"]["entryIds"].as_array().cloned().unwrap_or_default();
        for mut entry in ids.iter().filter_map(Value::as_str).filter_map(|id| hub.store.entry(id)) {
            let prompt = crate::chat::prompt::of_entry(hub, &bot.config, &entry.data);
            entry.data["status"] = "queued".into();
            hub.set_entry(&entry.id, &entry.data);
            let lane = Lane::main(&entry.bot_id);
            hub.send_cmd(&card.bot_id, Cmd::Send { lane, entry_id: entry.id, text: prompt })?;
        }
    }
    Ok(())
}

pub fn tools() -> Value {
    let mut tools = json!([
        {"name":"list_connectors","description":"Lista conexões ligadas para seu bot, sem credenciais, também na conversa adicional própria. Contas do mesmo serviço têm nomes e IDs separados. Disponibilidade e mediação do retorno não autorizam efeitos.","inputSchema":{"type":"object","properties":{}}},
        {"name":"request_connection","description":"Solicita ao usuário acesso a uma conexão MCP existente por connectorId. Na adicional, só aceita conector já ligado e mantém cartão/retomada na origem; contexto ou permissão alterados recusam. Novas conexões são adicionadas no app, sem MCP Registry. O catálogo Linear é apenas preparatório no Hub, sem conexão ou perfil aplicado ao login; URL manual não vira curadoria. O caminho curado de efeitos/recibos segue indisponível, inclusive a integração da prova humana única de Times/Atividades. Setup só verifica handshake, versão MCP e descoberta; não executa ferramenta nem liga outro bot. Versão incompatível não autoriza repetir chamada nem trocar transporte. Falha temporária de renovação conserva o login: tente depois, sem repetir efeitos automaticamente. Revogação confirmada pede novo login. Permissões do desafio Bearer 401 inicial têm prioridade sobre a metadata validada; ausência conserva omissão desconhecida. Permissões inválidas ou conflitantes recusam login: informe e pare, sem ampliar nem repetir sozinho. O login e refresh validam scope declarado ou omissão permitida; pedido explícito exige igualdade no piloto Mexerica. Isso não introspecta token nem comprova poderes reais, perfil ou aprovação humana. Par incompatível fica indisponível localmente até Entrar novamente em Extensões; não significa revogação remota. Legado/omissão seguem desconhecidos. Não peça segredos, amplie escopos nem repita efeitos. Perfis mínimos curados seguem pendentes. O cadastro OAuth só é reutilizado no issuer original exato vinculado pelo Hub. Troca de emissor ou cadastro legado sem vínculo exige DCR novo no próximo login; sem suporte, informe a recusa e pare, sem escolher outro servidor, contornar ou pedir segredos. Oriente conexão com cabeçalho de token pelo fluxo seguro do app quando não houver cadastro automático; não repita sozinho. Login ativo e refresh legado não ganham prova retroativa nem são deslogados em massa. O início do novo login pode já ter limpado tokens anteriores; a recusa não os restaura. Login recusado por endereço inválido ou redirecionamento: o usuário corrige a URL no app; não sugira outro endereço nem repita. Login recusado porque o servidor de login não confirmou a própria identidade: informe e pare; não escolha outro servidor, não relaxe a checagem nem peça segredos. Login recusado porque o servidor de login informou um recurso adicional: informe e pare; só o provedor corrige a query dos endpoints. Não remova a proteção nem peça segredos. Login recusado porque o serviço não confirmou a identidade do recurso: informe e pare; só o provedor corrige os metadados. Não troque de endpoint, não relaxe a checagem nem peça segredos. O início da reconexão pode já ter invalidado o login anterior; a recusa não o restaura. Login recusado porque o serviço não declara a proteção PKCE exigida (S256): informe e pare; só o provedor corrige, não repita nem contorne. Login recusado porque o retorno não confirmou o serviço esperado (emissor iss) ou veio malformado: informe e peça um novo login pelo app atualizado; não repita sozinho nem contorne. App iPhone antigo não envia iss e é recusado quando o servidor o exige. Login invalidado por remoção, reconexão, troca de autoridade ou mudança durante o desbloqueio exige nova intenção explícita; nunca peça código ou token no chat.","inputSchema":{"type":"object","properties":{"connectorId":{"type":"string"},"reason":{"type":"string"}},"required":["connectorId","reason"]}},
        {"name":"list_logins","description":"List the website and app logins turned on for you: id, label, site and username; never passwords. With several for the same site, use the one the task names, or ask the user which.","inputSchema":{"type":"object","properties":{}}},
        {"name":"request_login","description":"Pede um login legado para site/app quando nenhum ligado para você servir. No cartão, o usuário escolhe um salvo ou cadastra um novo, ligado só para você; você nunca vê a senha. Depois use type_login. A marca reservada {{senha: em site, mesmo malformada, recusa antes do cofre ou pedido. O caminho protegido e a coleta privada ainda estão indisponíveis; não contorne a recusa.","inputSchema":{"type":"object","properties":{"site":{"type":"string","description":"Domínio do site ou nome do app. Não aceita a marca reservada {{senha:, inclusive sem fechamento."},"reason":{"type":"string"}},"required":["site","reason"]}},
        {"name":"request_login_code","description":"Pede ao usuário o código de verificação (2FA) de um login já ligado para você, por exemplo quando o site pede um código por SMS ou app autenticador. No cartão o usuário digita o código; depois use type_login com field \"code\", que só digita com prova positiva de campo curto e numérico e envia (Enter). O valor não volta pelo resultado da ferramenta, mas depois de digitado pode aparecer na tela: vale uma vez só, por 5 minutos, e some se não for usado.","inputSchema":{"type":"object","properties":{"login":{"type":"string","description":"ID de um login de list_logins já ligado para você. Não aceita a marca reservada {{senha:, inclusive sem fechamento."},"reason":{"type":"string"}},"required":["login","reason"]}},
        {"name":"request_secret","description":"Pede ao usuário uma credencial em cartão seguro para conector instalado. Na adicional, só para conector já ligado, conservando origem/sessão/turno/permissão. O valor vai para o cofre e nunca retorna ao motor. Alterar o alvo ou headers retém retornos em voo da configuração anterior.","inputSchema":{"type":"object","properties":{"connectorId":{"type":"string"},"field":{"type":"string"},"location":{"type":"string","enum":["env","header"]},"reason":{"type":"string"}},"required":["connectorId","field","location","reason"]}}
    ]);
    if let Some(list) = tools.as_array_mut() {
        // R4a: ler o WhatsApp do número do bot (só leitura; conteúdo de fora, D125).
        list.extend(crate::fork::whatsapp::leitura::tools());
        list.push(crate::fork::whatsapp::send::draft_tool());
        list.push(crate::fork::whatsapp::send::reply_tool());
        for tool in list {
            if let Some(description) = tool["description"].as_str() {
                let description = format!(
                    "{description} O Hub confere a execução original até a transação do cartão ou a entrega da consulta; origem revogada ou de outra sessão é recusada."
                );
                tool["description"] = json!(description);
            }
        }
    }
    tools
}

pub(crate) fn context_tools() -> Value {
    let mut list = tools();
    if let Some(tools) = list.as_array_mut() {
        tools.retain(|t| {
            matches!(t["name"].as_str(), Some("list_connectors" | "request_connection" | "request_secret"))
        });
    }
    list
}

#[cfg(test)]
pub fn call(hub: &Arc<Hub>, bot: &str, name: &str, args: &Value) -> Result<Value> {
    let execution = crate::fork::sessoes::bindings::fixture_execution(hub, bot);
    call_in_execution(hub, bot, name, args, &execution)
}

pub(crate) fn call_in_execution(
    hub: &Arc<Hub>,
    bot: &str,
    name: &str,
    args: &Value,
    execution: &crate::fork::sessoes::ExecutionRef,
) -> Result<Value> {
    call_in_context(hub, bot, name, args, execution, None)
}

/// `context` só existe nas conversas adicionais próprias: guarda complementar03b sobre a
/// mesma execução, nunca outra fonte de lane ou autoridade.
pub(crate) fn call_in_context(
    hub: &Arc<Hub>,
    bot: &str,
    name: &str,
    args: &Value,
    execution: &crate::fork::sessoes::ExecutionRef,
    context: Option<&ToolContext>,
) -> Result<Value> {
    if let Some(context) = context {
        context.check(hub)?;
        anyhow::ensure!(
            matches!(name, "list_connectors" | "request_connection" | "request_secret"),
            "space_tool_not_bound"
        );
        if name != "list_connectors" {
            anyhow::ensure!(
                context.cfg.connectors.iter().any(|c| args["connectorId"] == c.as_str()),
                "connector_not_enabled"
            );
        }
    }
    if super::registry::registry_tool(name, args) {
        bail!(super::REGISTRY_UNAVAILABLE);
    }
    let session = crate::fork::sessoes::bindings::recheck(hub, execution)?;
    crate::fork::sessoes::bindings::resolve(
        &hub.store.db.locked(),
        execution.acp_binding().ok_or_else(|| anyhow!("chamada vencida"))?,
        bot,
    )?;
    let lane = Lane { chat: session.chat_id.clone(), session: (session.id != session.chat_id).then_some(session.id) };
    if let Some(context) = context {
        anyhow::ensure!(lane == context.lane(), "tool_actor_changed");
    }
    // Na adicional, o cartão conserva o turno da mensagem humana capturada.
    let turn = context.map_or_else(|| hub.store.max_turn(&lane.chat), ToolContext::number);
    let row =
        hub.store.bot(bot)?.filter(|b| !b.deleted && !b.config.is_group()).ok_or_else(|| anyhow!("unknown bot"))?;
    let result = match name {
        crate::fork::whatsapp::send::PREPARE_HUMAN => {
            let request = serde_json::from_value(args.clone()).map_err(|_| anyhow!("Pedido de rascunho inválido"))?;
            serde_json::to_value(crate::fork::whatsapp::send::prepare_human_in_execution(hub, execution, request)?)
                .map_err(Into::into)
        }
        // R4a: só leitura, turno do dono ou rotina, concessão "ver" da conta atual; a adicional já recusou acima.
        name if crate::fork::whatsapp::leitura::handles(name) => {
            crate::fork::whatsapp::leitura::call(hub, bot, name, args, execution)
        }
        // MX-18 "sozinho": enfileira a resposta; a fila reconfere tudo na vez.
        name if crate::fork::whatsapp::send::handles_reply(name) => {
            crate::fork::whatsapp::send::reply_call(hub, bot, args, execution)
        }
        // D82: a bot sees only the accounts (connectors, logins) turned on for it.
        "list_connectors" => {
            let mut listed = super::list_connectors(&hub.store)?;
            if let Some(items) = listed["items"].as_array_mut() {
                items.retain(|c| {
                    row.config.connectors.iter().any(|on| c["id"] == on.as_str())
                        && c["space"].as_str().is_none_or(|space| space == row.config.space_id)
                });
            }
            Ok(listed)
        }
        "list_logins" => super::logins::list_on(&hub.store, &row.config.logins),
        "request_login" => {
            crate::fork::secret_refs::reject_argument(args, "site")?;
            let site = super::logins::normalize_site(text(args, "site")?);
            if site.is_empty() || site.len() > 200 {
                bail!("Give the website's domain or the app's name");
            }
            let reason: String = text(args, "reason")?.chars().take(400).collect();
            let entry = post_request(
                hub,
                &lane,
                EntryKind::Notice,
                turn,
                &json!({"text":reason,"connectionRequest":{"kind":RequestKind::Login,"status":RequestStatus::Pending,"title":site,"site":site,"botId":bot}}),
                execution,
            )?;
            Ok(json!({"requestId":entry.id,"status":"waitingForUser"}))
        }
        "request_login_code" => {
            crate::fork::secret_refs::reject_argument(args, "login")?;
            // Só um login já ligado para este bot; site e título vêm do Hub, não do texto do bot.
            let login = super::logins::pick(&hub.store, &row.config.logins, text(args, "login")?)?;
            let reason: String = text(args, "reason")?.chars().take(400).collect();
            // O código só pode ser consumido pela sessão que pediu o cartão.
            let origin = execution
                .acp_binding()
                .ok_or_else(|| anyhow!("chamada vencida: execução sem vínculo de sessão"))?
                .to_owned();
            let entry = post_request(
                hub,
                &lane,
                EntryKind::Notice,
                turn,
                &json!({"text":reason,"connectionRequest":{"kind":RequestKind::Code,"status":RequestStatus::Pending,
                    "title":login.title(),"site":login.site,"loginId":login.id,"botId":bot,"origin":origin}}),
                execution,
            )?;
            Ok(json!({"requestId":entry.id,"status":"waitingForUser"}))
        }
        "request_connection" | "request_secret" => {
            let installed = text(args, "connectorId")?;
            let c = connectors(&hub.store)?
                .into_iter()
                .find(|c| c.id == installed)
                .ok_or_else(|| anyhow!("unknown connector"))?;
            let (title, registry) = (c.name, c.registry_name);
            let secret = name == "request_secret";
            if secret {
                let field = text(args, "field")?;
                if field.len() > 128 || !field.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
                    bail!("Invalid credential field");
                }
                if !matches!(text(args, "location")?, "env" | "header") {
                    bail!("Invalid credential destination");
                }
            }
            let request = json!({"kind":if secret {RequestKind::Secret} else {RequestKind::Connection}, "status":RequestStatus::Pending, "title":title,
                "registryName":registry, "connectorId":installed, "botId":bot,
                "field":if secret {args["field"].clone()} else {Value::Null},
                "location":if secret {args["location"].clone()} else {Value::Null}});
            let reason: String = text(args, "reason")?.chars().take(400).collect();
            let _boundary = hub.store.memory_boundary.locked();
            if let Some(context) = context {
                context.check(hub)?;
            }
            let entry = post_request(
                hub,
                &lane,
                EntryKind::Notice,
                turn,
                &json!({"text":reason,"connectionRequest":request}),
                execution,
            )?;
            if let Some(context) = context {
                crate::fork::spaces::connector_cards::record(hub, context, &entry)?;
            }
            Ok(json!({"requestId":entry.id,"status":"waitingForUser"}))
        }
        _ => bail!("Unknown connection tool"),
    }?;
    if matches!(name, "list_connectors" | "list_logins") {
        #[cfg(test)]
        crate::fork::sessoes::call_tests::before_effect(&hub.store, "connector_read");
        crate::fork::sessoes::bindings::recheck(hub, execution)?;
    }
    Ok(result)
}

fn post_request(
    hub: &Hub,
    lane: &Lane,
    kind: EntryKind,
    turn: i64,
    data: &Value,
    execution: &crate::fork::sessoes::ExecutionRef,
) -> Result<Entry> {
    #[cfg(test)]
    crate::fork::sessoes::call_tests::before_effect(&hub.store, "connector_commit");
    hub.add_entry_in_execution(lane, kind, turn, data, Some(execution))
        .ok_or_else(|| anyhow!("chamada vencida ou falha ao salvar pedido de conexão"))
}

/// 4 a 10 dígitos após aparar as pontas; qualquer espaço ou letra recusa.
fn login_code(raw: &str) -> Result<String> {
    let code = raw.trim();
    if !(4..=10).contains(&code.len()) || !code.bytes().all(|b| b.is_ascii_digit()) {
        bail!("O código de login deve ter de 4 a 10 dígitos");
    }
    Ok(code.to_owned())
}

fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key].as_str().filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("{key} is required"))
}

fn pending(hub: &Hub, id: &str, cancelling: bool) -> Result<Entry> {
    let e = hub.store.entry(id).ok_or_else(|| anyhow!("Unknown connection request"))?;
    if !has_status(&e.data["connectionRequest"], RequestStatus::Pending) {
        bail!("This request is already completed or cancelled");
    }
    if !cancelling && now_ms() - e.created_at > MAX_AGE {
        bail!("This request expired. Ask the bot for a new request.");
    }
    let bot = text(&e.data["connectionRequest"], "botId")?;
    hub.store.bot(bot)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("The requesting bot was removed"))?;
    if !cancelling {
        crate::fork::spaces::connector_cards::check(hub, &e, e.turn)?;
    }
    Ok(e)
}

/// Called after the route's Caller/Space checks, before any vault access.
/// This grants no completion: finish revalidates the card under MUTATION.
pub fn finish_requires_vault(hub: &Hub, args: &Value) -> Result<bool> {
    let id = text(args, "entryId")?;
    if let Some(entry) = hub.store.entry(id)
        && has_status(&entry.data["connectionRequest"], RequestStatus::Ready)
    {
        return Ok(false);
    }
    let cancelling = args["cancel"] == true;
    let entry = pending(hub, id, cancelling)?;
    let request = &entry.data["connectionRequest"];
    let kind: RequestKind = serde_json::from_value(request["kind"].clone())?;
    if cancelling || kind == RequestKind::Code {
        // O código de login mora só na memória: concluir ou cancelar não toca o cofre.
        return Ok(false);
    }
    super::registry::reject_registry_finish(request)?;
    Ok(true)
}

/// Leitura mínima do cartão para a janela segura (MX-209): o main resolve o
/// pedido pelo `entryId` e deriva dele todos os textos mostrados — o renderer
/// nunca descreve o pedido. Só login/secret/code (MX-286: o código de login usa a
/// mesma janela); a projeção é a mesma, sem `loginId` nem `origin` da sessão, e o
/// valor nunca existe na entrada (o código só mora na memória do Hub).
pub fn inspect(hub: &Hub, args: &Value) -> Result<Value> {
    let e = hub.store.entry(text(args, "entryId")?).ok_or_else(|| anyhow!("unknown connection request"))?;
    let request = &e.data["connectionRequest"];
    let kind: RequestKind = serde_json::from_value(request["kind"].clone())?;
    if !matches!(kind, RequestKind::Login | RequestKind::Secret | RequestKind::Code) {
        bail!("not a secure request");
    }
    serde_json::from_value::<RequestStatus>(request["status"].clone())?;
    // Mesma regra de idade do finish: cartão vencido não abre janela nem recebe segredo,
    // mesmo que o status continue pendente.
    if now_ms() - e.created_at > MAX_AGE {
        bail!("This request expired. Ask the bot for a new request.");
    }
    let bot_id = text(request, "botId")?;
    let row = hub.store.bot(bot_id)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("unknown bot"))?;
    Ok(json!({
        "entryId": e.id,
        "botId": bot_id,
        "botName": row.config.name,
        "kind": request["kind"],
        "status": request["status"],
        "title": request["title"],
        "site": request["site"],
        "field": request["field"],
        "location": request["location"],
        "spaceId": crate::fork::spaces::chat_space(hub, &e.bot_id)?,
        "conversationId": e.bot_id,
        "updatedAt": e.updated_at,
    }))
}

#[cfg(test)]
pub async fn finish(hub: &Arc<Hub>, args: &Value) -> Result<Value> {
    finish_as(hub, args, &crate::fork::autoridade::Autoridade::local()).await
}

/// D368: `owner` é a autoridade original de quem respondeu o cartão, reconferida na escrita do perfil.
pub(crate) async fn finish_as(
    hub: &Arc<Hub>,
    args: &Value,
    owner: &crate::fork::autoridade::Autoridade,
) -> Result<Value> {
    let _guard = MUTATION.lock().await;
    let id = text(args, "entryId")?;
    if let Some(entry) = hub.store.entry(id)
        && has_status(&entry.data["connectionRequest"], RequestStatus::Ready)
    {
        // Cancelar um cartão de código já pronto descarta o código guardado,
        // antes do retorno idempotente e sob a mesma exclusão do consumo.
        if args["cancel"] == true
            && serde_json::from_value::<RequestKind>(entry.data["connectionRequest"]["kind"].clone()).ok()
                == Some(RequestKind::Code)
        {
            hub.login_codes.drop(id);
        }
        return Ok(json!({"entry":entry}));
    }
    let e = pending(hub, id, args["cancel"] == true)?;
    let request = &e.data["connectionRequest"];
    let kind: RequestKind = serde_json::from_value(request["kind"].clone())?;
    if args["cancel"] == true {
        hub.login_codes.drop(id);
        let mut data = e.data.clone();
        data["connectionRequest"]["status"] = serde_json::to_value(RequestStatus::Cancelled)?;
        return Ok(json!({"entry":hub.set_entry(id, &data).ok_or_else(|| anyhow!("Could not cancel request"))?}));
    }
    super::registry::reject_registry_finish(request)?;
    if kind == RequestKind::Agent {
        let backend = text(request, "backend")?;
        // Motor retirado: o cartão salvo é recusado com a mensagem de lançar, sem abrir o CLI.
        anyhow::ensure!(
            !crate::fork::motores_retirados::retirado(backend),
            "{}",
            crate::fork::motores_retirados::msg_lancar(backend)
        );
        if crate::agent::auth::check(&hub.store, backend).await?["signedIn"] != true {
            bail!("{} isn't signed in yet.", request["title"].as_str().unwrap_or("The agent"));
        }
        agent_signed_in(hub, backend)?;
        return Ok(json!({"entry": hub.store.entry(id)}));
    }
    if kind == RequestKind::Login {
        let site = text(request, "site")?;
        let login = match args["loginId"].as_str().filter(|s| !s.is_empty()) {
            // A login saved before (maybe on for other bots): the user picks it instead of typing it again.
            Some(saved) => {
                let login = super::logins::get(&hub.store, saved)?;
                if login.site != super::logins::normalize_site(site) {
                    bail!("That login is for {}, not {site}", login.site);
                }
                login
            }
            None => super::logins::save(
                &hub.store,
                site,
                args["username"].as_str().unwrap_or_default(),
                text(args, "value")?,
                args["label"].as_str().unwrap_or_default(),
            )?,
        };
        // D82: the bot that asked gets this login; no other bot does.
        turn_on_login(hub, text(request, "botId")?, &login.id, owner).await?;
        let note = format!(
            "[Login “{}” for {} is on for you as `{}`. Type it with the computer tool type_login; the password is never shown to you. Continue the task.]",
            login.title(),
            login.site,
            login.id,
        );
        return complete(hub, &e, id, note);
    }
    if kind == RequestKind::Code {
        let login_id = text(request, "loginId")?;
        let bot = text(request, "botId")?;
        let code = login_code(text(args, "value")?)?;
        // O login pode ter sido desligado do bot ou removido depois do pedido.
        let row = hub.store.bot(bot)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("unknown bot"))?;
        if !row.config.logins.iter().any(|l| l == login_id) || super::logins::get(&hub.store, login_id).is_err() {
            bail!("Esse login não está mais ligado para este bot");
        }
        // O código só pode ser consumido pela sessão que pediu o cartão.
        let origin = request["origin"].as_str().filter(|s| !s.is_empty()).ok_or_else(|| {
            anyhow!("Este pedido de código é antigo e não tem origem de sessão. Peça um novo código ao bot.")
        })?;
        #[cfg(test)]
        crate::fork::sessoes::call_tests::before_effect(&hub.store, "login_code_staged");
        hub.login_codes.put(id, bot, login_id, origin, code)?;
        let note = format!(
            "[O código de login de {} está pronto. Digite com a ferramenta de computador type_login, field \"code\"; o valor não volta pelo resultado, mas depois de digitado pode aparecer na tela. Vale uma vez só, por 5 minutos. Recusa se não houver código pronto e válido. Continue a tarefa.]",
            request["title"].as_str().unwrap_or("o login"),
        );
        // O código só fica consumível depois do cartão gravado como pronto;
        // qualquer falha da conclusão o descarta.
        let done = complete(hub, &e, id, note);
        if done.is_err() {
            hub.login_codes.drop(id);
        }
        return done;
    }
    let connector_id = request["connectorId"]
        .as_str()
        .or(args["connectorId"].as_str())
        .ok_or_else(|| anyhow!("Choose a connector"))?;
    {
        let _boundary = hub.store.memory_boundary.locked();
        crate::fork::spaces::connector_cards::check(hub, &e, e.turn)?;
        super::update_connectors(&hub.store, |all| {
            let c = all.iter_mut().find(|c| c.id == connector_id).ok_or_else(|| anyhow!("Unknown connector"))?;
            if request["connectorId"].is_null() && request["registryName"].as_str() != c.registry_name.as_deref() {
                bail!("This connector does not match the request");
            }
            if kind == RequestKind::Secret {
                let value = text(args, "value")?;
                if value.len() > 64 * 1024 {
                    bail!("Credential is too large");
                }
                let field = text(request, "field")?.to_owned();
                match text(request, "location")? {
                    "env" if c.command.is_some() => {
                        c.env.insert(field, value.to_owned());
                    }
                    "header" if c.url.is_some() => {
                        c.headers.insert(field, value.to_owned());
                    }
                    _ => bail!("Credential destination does not match this connector"),
                }
            }
            Ok(())
        })?;
    }
    super::verify::verify(&hub.store, connector_id, hub.bridge_port()).await?;
    let bot = text(request, "botId")?;
    let cfg = crate::fork::spaces::connector_cards::check(hub, &e, e.turn)?;
    if cfg.context.is_some() {
        // Já ligado: setup não amplia o perfil nem troca a origem do cartão.
        hub.send_cmd(bot, Cmd::RefreshTools)?;
    } else {
        enable_connector(hub, bot, connector_id, owner).await?;
    }
    let note = format!(
        "[Connection setup completed: {}. Credentials were submitted securely and are not in this conversation. Continue the task.]",
        request["title"].as_str().unwrap_or("Connector")
    );
    complete(hub, &e, id, note)
}

/// A verificação externa termina antes desta reserva; leitura+escrita do perfil
/// participam da mesma exclusão do Desfazer e das outras edições de time.
async fn enable_connector(
    hub: &Arc<Hub>,
    bot: &str,
    connector_id: &str,
    owner: &crate::fork::autoridade::Autoridade,
) -> Result<()> {
    let _guard = hub.store.team_lock.lock().await;
    crate::chat::manage::recover_live(hub);
    let row = hub.store.bot(bot)?.ok_or_else(|| anyhow!("Unknown bot"))?;
    let mut enabled = row.config.connectors;
    if !enabled.iter().any(|s| s == connector_id) {
        enabled.push(connector_id.to_owned());
    }
    hub.update_bot_by_user_authorized(&json!({"id":bot,"connectors":enabled}), None, Some(owner))?;
    hub.send_cmd(bot, Cmd::RefreshTools)?;
    Ok(())
}

/// Turns a login on for the bot whose card the user answered, under the same exclusion as the
/// other team edits (see `enable_connector`).
async fn turn_on_login(
    hub: &Arc<Hub>,
    bot: &str,
    login: &str,
    owner: &crate::fork::autoridade::Autoridade,
) -> Result<()> {
    let _guard = hub.store.team_lock.lock().await;
    crate::chat::manage::recover_live(hub);
    let row = hub.store.bot(bot)?.ok_or_else(|| anyhow!("Unknown bot"))?;
    let mut on = row.config.logins;
    if !on.iter().any(|l| l == login) {
        on.push(login.to_owned());
        hub.update_bot_by_user_authorized(&json!({"id":bot,"logins":on}), None, Some(owner))?;
    }
    Ok(())
}

/// Marks a request ready and resumes its bot once with `note`.
fn complete(hub: &Arc<Hub>, e: &Entry, id: &str, note: String) -> Result<Value> {
    let _boundary = hub.store.memory_boundary.locked();
    let cfg = crate::fork::spaces::connector_cards::check(hub, e, e.turn)?;
    let bot = text(&e.data["connectionRequest"], "botId")?.to_owned();
    let mut data = e.data.clone();
    data["connectionRequest"]["status"] = serde_json::to_value(RequestStatus::Ready)?;
    let entry = hub.set_entry(id, &data).ok_or_else(|| anyhow!("Could not complete request"))?;
    let lane = Lane::main(&e.bot_id);
    // A confirmação abre um turno novo no chat: número reservado de forma atômica, nunca o 0 compartilhado (D229-6).
    // Repetir a confirmação devolve a mesma entrada; o número reservado a mais fica sem uso.
    let mut ack_data = json!({"text":note,"status":"queued","from":"host"});
    if cfg.context.is_some() {
        ack_data["executionBinding"] = crate::fork::spaces::execution_stamp(&hub.store, &cfg)?.into();
    }
    let (ack, added) = hub.add_entry_once(
        &format!("connection-ack-{id}"),
        &lane,
        EntryKind::User,
        hub.store.alloc_turn(&lane.chat)?,
        // Written for the user, not by them: memory doesn't take it as their words.
        &ack_data,
    )?;
    if added {
        hub.send_cmd(&bot, Cmd::Send { lane, entry_id: ack.id, text: note })?;
    }
    Ok(json!({"entry":entry}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{BotConfig, Store};
    use std::path::{Path, PathBuf};

    fn fixture() -> (Arc<Hub>, PathBuf) {
        std::fs::create_dir_all(crate::service::data_dir()).unwrap();
        let dir = std::env::temp_dir().join(format!("codync-credentials-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(Path::new(":memory:")).unwrap();
        let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/team_agent.py");
        let cfg: BotConfig = serde_json::from_value(json!({"id":"bot","name":"bot","backend":"fixture","cwd":dir,"command":crate::shell::python(&agent),"notify":false})).unwrap();
        store.save_bot(&cfg).unwrap();
        let python = if cfg!(windows) { "python" } else { "python3" };
        let c: super::super::Connector = serde_json::from_value(json!({"id":"fixture","name":"Fixture","space":crate::fork::spaces::PERSONAL,"command":python,"args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/credential_mcp.py")]})).unwrap();
        super::super::save_connectors(&store, &[c]).unwrap();
        let hub = Hub::new(
            store,
            "test".into(),
            crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
            "test".into(),
            19222,
        );
        hub.start().unwrap();
        (hub, dir)
    }

    #[tokio::test]
    async fn cartao_da_adicional_retoma_intencao_humana_original_e_fila_recusa_sessao_nova() {
        let (f, _, _) = crate::fork::spaces::connector_context::tests::fixture();
        let mut commands = f.hub.test_bot_commands(&f.cfg.id);
        let mut body = f.body.clone();
        body["name"] = "request_connection".into();
        body["arguments"] = json!({"connectorId":"fixture","reason":"Fixture"});
        let result = crate::fork::spaces::tool_context::route(&f.hub, "connectorCall", &body).await.unwrap().unwrap();
        let id = result["requestId"].as_str().unwrap();
        let card = f.hub.store.entry(id).unwrap();
        complete(&f.hub, &card, id, "Setup conferido".into()).unwrap();
        let command = commands.try_recv().unwrap();
        let Cmd::Send { lane, entry_id, .. } = command else { panic!("retomada tipada esperada") };
        assert_eq!(lane.chat, f.cfg.session_owner());
        let ack = f.hub.store.entry(&entry_id).unwrap();
        assert!(!ack.is_user_authored(), "confirmar não forja autoria humana");
        assert!(ack.data.get("connectionResume").is_none(), "continuação usa somente ID canônico e KV privado");
        f.hub.set_runtime(&f.cfg.id, |r| {
            r.message = Some(entry_id.clone());
            r.memory_turn_token = Some("retomada-A".into());
        });
        body["name"] = "list_connectors".into();
        body["arguments"] = json!({});
        assert!(
            crate::fork::spaces::tool_context::route(&f.hub, "connectorCall", &body).await.is_ok(),
            "a continuação conserva a mensagem humana do cartão"
        );
        let slot = crate::fork::spaces::session_slot(&f.cfg).unwrap();
        f.hub.store.kv_set(&crate::store::run_session_key(&f.cfg.id, &slot), "sessao-B").unwrap();
        assert!(
            crate::fork::spaces::validate_entries(&f.hub, &f.cfg, &[entry_id]).is_err(),
            "a fila não herda sessão nova"
        );
        assert_eq!(
            f.hub.store.history(&f.cfg.id, i64::MAX, 30).unwrap().into_iter().map(|e| e.id).collect::<Vec<_>>(),
            Vec::<String>::new(),
            "sem entrada na principal"
        );
    }

    #[tokio::test]
    async fn confirmacoes_de_conexao_reservam_turnos_distintos_e_repetir_e_idempotente() {
        let (hub, dir) = fixture();
        let lane = Lane::main("bot");
        let request = |connector: &str| {
            let data = json!({"connectionRequest":{"botId":"bot","connectorId":connector,"status":"pending"}});
            hub.store.insert_entry(&lane, EntryKind::Notice, 1, &data).unwrap()
        };
        let (a, b) = (request("um"), request("dois"));
        complete(&hub, &a, &a.id, "Conector um pronto".into()).unwrap();
        complete(&hub, &b, &b.id, "Conector dois pronto".into()).unwrap();
        let ack = |id: &str| hub.store.entry(&format!("connection-ack-{id}")).unwrap();
        let (first, second) = (ack(&a.id), ack(&b.id));
        assert!(first.turn > 1 && second.turn > 1, "nenhuma confirmação fica no turno 0");
        assert_ne!(first.turn, second.turn);
        complete(&hub, &a, &a.id, "Conector um pronto".into()).unwrap();
        // O id é fixo por pedido: repetir devolve a mesma entrada, com o mesmo turno.
        assert_eq!(ack(&a.id).turn, first.turn, "repetir não cria outra confirmação nem troca o turno");
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn enabling_a_connector_waits_for_undo_and_preserves_the_restored_profile() {
        let (hub, dir) = fixture();
        let before = hub.store.bot("bot").unwrap().unwrap().config;
        hub.update_bot(&json!({"id":"bot", "name":"Changed", "description":"changed instructions"})).unwrap();
        let after = hub.store.bot("bot").unwrap().unwrap().config;
        hub.store
            .finish_team_change(
                "undo-test",
                "bot",
                0,
                &json!({"teamChange":{"status":"available","targetBotId":"bot"}}),
                Some("{}"),
            )
            .unwrap();
        let guard = hub.store.team_lock.lock().await;
        let owner = crate::fork::autoridade::Autoridade::local();
        let enable = enable_connector(&hub, "bot", "fixture", &owner);
        tokio::pin!(enable);
        assert!(tokio::time::timeout(std::time::Duration::from_millis(20), &mut enable).await.is_err());
        let routines = hub.store.kv_read("routines.v1").unwrap().unwrap();
        hub.with_inactive_bot("bot", || hub.store.commit_team_undo("undo-test", &after, Some(&before), &routines))
            .unwrap();
        drop(guard);
        enable.await.unwrap();
        let cfg = hub.store.bot("bot").unwrap().unwrap().config;
        assert_eq!(cfg.name, before.name);
        assert_eq!(cfg.description, before.description);
        assert!(cfg.connectors.contains(&"fixture".to_owned()));
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn ligacao_depois_de_conectar_recusa_o_principal_com_o_codigo_estavel() {
        let (hub, dir) = fixture();
        hub.store.set_team_settings(&json!({"primaryBotId": "bot"})).unwrap();
        let pedido = crate::market::contas::Pedido {
            servico: "linear",
            tipo: "linear.oficial_oauth",
            space: crate::fork::spaces::PERSONAL,
            nome: None,
            nome_primeira: None,
            liberacoes: None,
            aceita_credencial_com_escrita: false,
        };
        crate::market::contas::conectar(&hub.store, &pedido).unwrap();
        let owner = crate::fork::autoridade::Autoridade::local();
        let e = format!("{:#}", enable_connector(&hub, "bot", "linear", &owner).await.unwrap_err());
        assert!(e.starts_with("principal_sem_contas"), "{e}");
        assert_eq!(hub.store.bot("bot").unwrap().unwrap().config.connectors, Vec::<String>::new());
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn secure_submission_enables_connector_and_resumes_only_once() {
        let (hub, dir) = fixture();
        let r = call(
            &hub,
            "bot",
            "request_secret",
            &json!({"connectorId":"fixture","field":"API_KEY","location":"env","reason":"Connect the test service"}),
        )
        .unwrap();
        let id = r["requestId"].as_str().unwrap();
        let submitted = json!({"entryId":id,"value":"test-private-key"});
        let first = finish(&hub, &submitted).await.unwrap();
        assert_eq!(first["entry"]["data"]["connectionRequest"]["status"], "ready");
        assert!(!first.to_string().contains("test-private-key"));
        assert!(hub.store.bot("bot").unwrap().unwrap().config.connectors.contains(&"fixture".to_owned()));
        assert!(!hub.store.kv_read("connectors").unwrap().unwrap().contains("test-private-key"));
        finish(&hub, &submitted).await.unwrap();
        let history = hub.store.history("bot", i64::MAX, 100).unwrap();
        assert_eq!(history.iter().filter(|e| e.id == format!("connection-ack-{id}")).count(), 1);
        assert!(!serde_json::to_string(&history).unwrap().contains("test-private-key"));
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn remote_control_devices_cannot_retrieve_credentials_or_forge_requests() {
        let caller = crate::api::devices::Caller::Device {
            key: "test".into(),
            scopes: vec![crate::store::Scope::Control],
            autoridade: None,
        };
        for method in
            ["donnaGuardRuntime", "donnaGuardObserve", "donnaGuardRelease", "connectorTarget", "connectorCall"]
        {
            assert!(crate::api::devices::permit(&caller, method).is_err());
        }
        assert!(crate::api::devices::permit(&caller, "connectorRequestFinish").is_ok());
    }

    #[tokio::test]
    async fn saved_login_resumes_the_bot_without_its_password() {
        let (hub, dir) = fixture();
        let r = call(
            &hub,
            "bot",
            "request_login",
            &json!({"site":"https://www.GitHub.com/login","reason":"Sign in to open the PR"}),
        )
        .unwrap();
        let id = r["requestId"].as_str().unwrap();
        // D82: saved before, but not on for this bot: it sees none of them, nor connectors it doesn't have.
        let other = super::super::logins::save(&hub.store, "github.com", "casa", "pw-other", "GitHub Casa").unwrap();
        assert_eq!(call(&hub, "bot", "list_logins", &json!({})).unwrap()["items"], json!([]));
        assert_eq!(call(&hub, "bot", "list_connectors", &json!({})).unwrap()["items"], json!([]));
        let done = finish(&hub, &json!({"entryId":id,"username":"kevin","value":"hunter2-private"})).await.unwrap();
        assert_eq!(done["entry"]["data"]["connectionRequest"]["status"], "ready");
        let listed = call(&hub, "bot", "list_logins", &json!({})).unwrap();
        assert_eq!(listed["items"].as_array().unwrap().len(), 1, "only the login it asked for is on");
        assert_eq!(listed["items"][0]["site"], "github.com");
        assert_eq!(listed["items"][0]["username"], "kevin");
        assert_eq!(hub.store.bot("bot").unwrap().unwrap().config.logins, [listed["items"][0]["id"].as_str().unwrap()]);
        // A second card: the user picks the login saved before instead of typing it again.
        let r =
            call(&hub, "bot", "request_login", &json!({"site":"github.com","reason":"Use the work account"})).unwrap();
        finish(&hub, &json!({"entryId":r["requestId"],"loginId":other.id})).await.unwrap();
        assert!(hub.store.bot("bot").unwrap().unwrap().config.logins.contains(&other.id));
        assert!(!listed.to_string().contains("hunter2-private"));
        assert!(!hub.store.kv_read("logins").unwrap().unwrap().contains("hunter2-private"));
        let history = hub.store.history("bot", i64::MAX, 100).unwrap();
        let history = serde_json::to_string(&history).unwrap();
        assert!(history.contains("type_login") && !history.contains("hunter2-private"));
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Um cartão de login legado salvo com motor retirado é recusado com a mensagem de lançar;
    /// o CLI do motor não é aberto e o cartão não fica pronto.
    #[tokio::test]
    async fn cartao_de_login_legado_de_motor_retirado_e_recusado_sem_executar_o_cli() {
        let (hub, dir) = fixture();
        let lane = Lane::main("bot");
        let user = hub.add_entry(&lane, EntryKind::User, 1, &json!({"text":"oi","status":"sent"})).unwrap().id;
        invite_agent_sign_in(&hub, "bot", &lane, "kilo", "Kilo", std::slice::from_ref(&user)).unwrap();
        let cards = hub.store.pending_sign_ins("kilo").unwrap();
        assert_eq!(cards.len(), 1, "um bot mantém um cartão de login por motor");
        let erro = finish(&hub, &json!({"entryId":cards[0].id})).await.unwrap_err().to_string();
        assert!(erro.starts_with("O motor Kilo não está disponível nesta versão da Mexerica"), "{erro}");
        let card = hub.store.entry(&cards[0].id).unwrap();
        assert_ne!(card.data["connectionRequest"]["status"], json!("ready"), "o cartão não fica pronto");
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn signing_in_completes_the_card_and_sends_its_messages_again() {
        let (hub, dir) = fixture();
        let lane = Lane::main("bot");
        let user =
            |text: &str| hub.add_entry(&lane, EntryKind::User, 1, &json!({"text":text,"status":"sent"})).unwrap().id;
        let (first, second) = (user("alpha-7"), user("bravo-9"));
        invite_agent_sign_in(&hub, "bot", &lane, "fixture", "Fixture", std::slice::from_ref(&first)).unwrap();
        invite_agent_sign_in(&hub, "bot", &lane, "fixture", "Fixture", &[second.clone(), first.clone()]).unwrap();
        let cards = hub.store.pending_sign_ins("fixture").unwrap();
        assert_eq!(cards.len(), 1, "a bot keeps one sign-in card");
        assert_eq!(cards[0].data["connectionRequest"]["entryIds"], json!([first, second]));
        agent_signed_in(&hub, "fixture").unwrap();
        assert_eq!(hub.store.entry(&cards[0].id).unwrap().data["connectionRequest"]["status"], "ready");
        let prompts = dir.join("prompts.jsonl");
        let sent = tokio::time::timeout(std::time::Duration::from_secs(20), async {
            while !std::fs::read_to_string(&prompts).is_ok_and(|p| p.contains("alpha-7") && p.contains("bravo-9")) {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        });
        sent.await.expect("both messages go out again");
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn cancellation_rejects_later_secret_submission() {
        let (hub, dir) = fixture();
        let r = call(
            &hub,
            "bot",
            "request_secret",
            &json!({"connectorId":"fixture","field":"API_KEY","location":"env","reason":"Test"}),
        )
        .unwrap();
        let id = &r["requestId"];
        finish(&hub, &json!({"entryId":id,"cancel":true})).await.unwrap();
        assert!(finish(&hub, &json!({"entryId":id,"value":"test-private-key"})).await.is_err());
        assert!(connectors(&hub.store).unwrap()[0].env.is_empty());
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn failed_verification_does_not_enable_connector_or_resume() {
        let (hub, dir) = fixture();
        let r = call(
            &hub,
            "bot",
            "request_secret",
            &json!({"connectorId":"fixture","field":"API_KEY","location":"env","reason":"Test"}),
        )
        .unwrap();
        let id = r["requestId"].as_str().unwrap();
        assert!(finish(&hub, &json!({"entryId":id,"value":"wrong"})).await.is_err());
        assert_eq!(hub.store.entry(id).unwrap().data["connectionRequest"]["status"], "pending");
        assert_eq!(hub.store.bot("bot").unwrap().unwrap().config.connectors.len(), 0);
        assert!(hub.store.entry(&format!("connection-ack-{id}")).is_none());
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    fn code_fixture() -> (Arc<Hub>, PathBuf, super::super::logins::Login) {
        let (hub, dir) = fixture();
        let login = super::super::logins::save(&hub.store, "github.com", "conta-a", "pw-x", "GitHub conta A").unwrap();
        let mut cfg = hub.store.bot("bot").unwrap().unwrap().config;
        cfg.logins = vec![login.id.clone()];
        hub.store.save_bot(&cfg).unwrap();
        (hub, dir, login)
    }

    /// O vínculo de sessão gravado no cartão na criação do pedido.
    fn code_origin(hub: &Arc<Hub>) -> String {
        crate::fork::sessoes::bindings::for_acp(&hub.store, "bot", "fixture-acp").unwrap().id.as_str().to_owned()
    }

    #[tokio::test]
    async fn login_code_card_uses_hub_data_and_keeps_the_value_out_of_sight() {
        let (hub, dir, login) = code_fixture();
        let r = call(&hub, "bot", "request_login_code", &json!({"login":login.id,"reason":"O site pediu um código"}))
            .unwrap();
        let id = r["requestId"].as_str().unwrap();
        let request = hub.store.entry(id).unwrap().data["connectionRequest"].clone();
        assert_eq!(request["kind"], "code");
        assert_eq!(request["status"], "pending");
        assert_eq!(request["site"], "github.com");
        assert_eq!(request["title"], "GitHub conta A");
        assert_eq!(request["loginId"], login.id.as_str());
        assert_eq!(request["botId"], "bot");
        assert!(!request["origin"].as_str().unwrap_or_default().is_empty(), "o cartão guarda a sessão de origem");
        let done = finish(&hub, &json!({"entryId":id,"value":" 123456 "})).await.unwrap();
        assert_eq!(done["entry"]["data"]["connectionRequest"]["status"], "ready");
        assert!(!done.to_string().contains("123456"), "a resposta não devolve o código");
        let history = serde_json::to_string(&hub.store.history("bot", i64::MAX, 100).unwrap()).unwrap();
        assert!(!history.contains("123456"), "nem a entrada nem a nota ao bot");
        assert!(!hub.store.kv_read("logins").unwrap().unwrap().contains("123456"), "nem o cofre");
        // Só na memória, ligado a bot + login + sessão de origem, uso único.
        let origin = code_origin(&hub);
        assert_eq!(
            hub.login_codes.take("bot", &login.id, &origin),
            crate::market::login_codes::Take::Ready("123456".into())
        );
        assert_eq!(hub.login_codes.take("bot", &login.id, &origin), crate::market::login_codes::Take::Missing);
        assert_eq!(hub.login_codes.take("outro-bot", &login.id, &origin), crate::market::login_codes::Take::Missing);
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn inspect_projects_a_login_code_card_for_the_secure_window_without_any_value() {
        let (hub, dir, login) = code_fixture();
        let r = call(&hub, "bot", "request_login_code", &json!({"login":login.id,"reason":"O site pediu um código"}))
            .unwrap();
        let id = r["requestId"].as_str().unwrap();
        // Ingresso pela API (Leitura): mesma projeção do login/secret, com o título e o site vindos do Hub.
        let got = crate::api::dispatch(
            &hub,
            &crate::api::devices::Caller::Local,
            "connectorRequestGet",
            json!({"entryId": id}),
        )
        .await
        .unwrap();
        assert_eq!((got["kind"].as_str(), got["status"].as_str()), (Some("code"), Some("pending")));
        assert_eq!((got["site"].as_str(), got["title"].as_str()), (Some("github.com"), Some("GitHub conta A")));
        assert_eq!((got["botId"].as_str(), got["botName"].as_str()), (Some("bot"), Some("bot")));
        assert_eq!(got["entryId"], id);
        for escondido in ["value", "code", "password", "loginId", "origin"] {
            assert!(got.get(escondido).is_none(), "a projeção nunca leva `{escondido}`");
        }
        // Entrega o código (o `finish` do código lê o login, então ainda com o cofre aberto).
        finish(&hub, &json!({"entryId":id,"value":"987654"})).await.unwrap();
        let pronto = inspect(&hub, &json!({"entryId": id})).unwrap();
        assert_eq!((pronto["kind"].as_str(), pronto["status"].as_str()), (Some("code"), Some("ready")));
        assert!(!pronto.to_string().contains("987654"), "o código nunca aparece na leitura");
        // Cartão vencido não abre a janela, como login e chave.
        let r = call(&hub, "bot", "request_login_code", &json!({"login":login.id,"reason":"Velho"})).unwrap();
        let velho = r["requestId"].as_str().unwrap();
        hub.store
            .db
            .locked()
            .execute("UPDATE entries SET created_at=?2 WHERE id=?1", rusqlite::params![velho, now_ms() - MAX_AGE - 1])
            .unwrap();
        assert!(inspect(&hub, &json!({"entryId":velho})).is_err(), "vencido não abre a janela");
        // A leitura não pede o cofre: com ele trancado, o cartão segue abrindo a projeção.
        *hub.store.secret_key.locked() = None;
        let unlocks = hub.store.vault_unlock_attempts.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(inspect(&hub, &json!({"entryId": id})).unwrap()["kind"], "code");
        assert_eq!(hub.store.vault_unlock_attempts.load(std::sync::atomic::Ordering::SeqCst), unlocks);
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn login_code_rejects_bad_values_and_stays_pending() {
        let (hub, dir, login) = code_fixture();
        let r = call(&hub, "bot", "request_login_code", &json!({"login":login.id,"reason":"Código"})).unwrap();
        let id = r["requestId"].as_str().unwrap();
        for bad in ["123", "12345678901", "12 34", "abcdef", ""] {
            assert!(finish(&hub, &json!({"entryId":id,"value":bad})).await.is_err(), "{bad}");
        }
        assert_eq!(hub.store.entry(id).unwrap().data["connectionRequest"]["status"], "pending");
        let origin = code_origin(&hub);
        assert_eq!(hub.login_codes.take("bot", &login.id, &origin), crate::market::login_codes::Take::Missing);
        finish(&hub, &json!({"entryId":id,"value":"654321"})).await.unwrap();
        assert_eq!(
            hub.login_codes.take("bot", &login.id, &origin),
            crate::market::login_codes::Take::Ready("654321".into())
        );
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn login_code_is_bound_to_its_login_and_dies_with_the_card() {
        let (hub, dir, login) = code_fixture();
        let other = super::super::logins::save(&hub.store, "gitlab.com", "conta-b", "pw-y", "").unwrap();
        let mut cfg = hub.store.bot("bot").unwrap().unwrap().config;
        cfg.logins = vec![login.id.clone(), other.id.clone()];
        hub.store.save_bot(&cfg).unwrap();
        let r = call(&hub, "bot", "request_login_code", &json!({"login":login.id,"reason":"Código"})).unwrap();
        let id = r["requestId"].as_str().unwrap();
        finish(&hub, &json!({"entryId":id,"value":"112233"})).await.unwrap();
        // Um segundo cartão guarda o próprio código, ainda preso ao login dele.
        let r = call(&hub, "bot", "request_login_code", &json!({"login":login.id,"reason":"Outro"})).unwrap();
        let second = r["requestId"].as_str().unwrap();
        finish(&hub, &json!({"entryId":second,"value":"445566"})).await.unwrap();
        let origin = code_origin(&hub);
        assert_eq!(
            hub.login_codes.take("bot", &other.id, &origin),
            crate::market::login_codes::Take::Missing,
            "outro login não consome"
        );
        // Dois cartões prontos da mesma sessão: ambíguo, nada é consumido.
        assert_eq!(hub.login_codes.take("bot", &login.id, &origin), crate::market::login_codes::Take::Several);
        // Um cartão saindo de cena deixa o outro pronto e consumível.
        hub.login_codes.drop(id);
        assert_eq!(
            hub.login_codes.take("bot", &login.id, &origin),
            crate::market::login_codes::Take::Ready("445566".into())
        );
        assert_eq!(hub.login_codes.take("bot", &login.id, &origin), crate::market::login_codes::Take::Missing);
        // Cancelar o cartão pendente não deixa código para trás.
        let r = call(&hub, "bot", "request_login_code", &json!({"login":login.id,"reason":"Cancelado"})).unwrap();
        let cancelled = r["requestId"].as_str().unwrap();
        finish(&hub, &json!({"entryId":cancelled,"cancel":true})).await.unwrap();
        assert_eq!(hub.store.entry(cancelled).unwrap().data["connectionRequest"]["status"], "cancelled");
        assert_eq!(hub.login_codes.take("bot", &login.id, &origin), crate::market::login_codes::Take::Missing);
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn login_code_ready_cancel_discards_and_wrong_session_cannot_consume() {
        let (hub, dir, login) = code_fixture();
        let r = call(&hub, "bot", "request_login_code", &json!({"login":login.id,"reason":"Código"})).unwrap();
        let id = r["requestId"].as_str().unwrap();
        finish(&hub, &json!({"entryId":id,"value":"778899"})).await.unwrap();
        let origin = code_origin(&hub);
        // Uma sessão nova do mesmo bot+login não consome o código da anterior.
        let other_session =
            crate::fork::sessoes::bindings::issue_in(&hub.store, "bot", "bot", "bot", None, Some("sessao-2")).unwrap();
        assert_eq!(
            hub.login_codes.take("bot", &login.id, other_session.id.as_str()),
            crate::market::login_codes::Take::Missing,
            "vínculo de sessão"
        );
        assert_eq!(
            hub.login_codes.take("bot", &login.id, &origin),
            crate::market::login_codes::Take::Ready("778899".into())
        );
        // Cancelar o cartão JÁ PRONTO descarta o código, antes do retorno idempotente.
        let r = call(&hub, "bot", "request_login_code", &json!({"login":login.id,"reason":"Segundo"})).unwrap();
        let second = r["requestId"].as_str().unwrap();
        finish(&hub, &json!({"entryId":second,"value":"665544"})).await.unwrap();
        finish(&hub, &json!({"entryId":second,"cancel":true})).await.unwrap();
        assert_eq!(
            hub.login_codes.take("bot", &login.id, &origin),
            crate::market::login_codes::Take::Missing,
            "cancelar depois de pronto descarta"
        );
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn login_code_is_dropped_when_completion_fails() {
        let (hub, dir, login) = code_fixture();
        let r = call(&hub, "bot", "request_login_code", &json!({"login":login.id,"reason":"Código"})).unwrap();
        let id = r["requestId"].as_str().unwrap().to_owned();
        let origin = code_origin(&hub);
        // Intercalação: o bot some entre a validação e a conclusão do cartão.
        let reached = crate::fork::sessoes::call_tests::interleave(&hub.store, "login_code_staged", {
            let hub = hub.clone();
            move || {
                hub.store.delete_bot("bot").unwrap();
            }
        });
        assert!(finish(&hub, &json!({"entryId":id,"value":"990011"})).await.is_err());
        assert!(reached.load(std::sync::atomic::Ordering::SeqCst));
        // Apagar o bot leva as entries junto: não há cartão para concluir.
        assert!(hub.store.entry(&id).is_none());
        assert_eq!(
            hub.login_codes.take("bot", &login.id, &origin),
            crate::market::login_codes::Take::Missing,
            "falha de conclusão não deixa código consumível"
        );
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn login_code_needs_a_login_on_for_the_bot_and_no_reserved_mark() {
        let (hub, dir) = fixture();
        let login = super::super::logins::save(&hub.store, "github.com", "conta-a", "pw-x", "GitHub conta A").unwrap();
        // Salvo, mas não ligado para o bot: recusa como em type_login.
        let err = call(&hub, "bot", "request_login_code", &json!({"login":login.id,"reason":"Código"})).unwrap_err();
        assert!(err.to_string().contains("is on for this bot"));
        let mut cfg = hub.store.bot("bot").unwrap().unwrap().config;
        cfg.logins = vec![login.id.clone()];
        hub.store.save_bot(&cfg).unwrap();
        // A marca reservada recusa antes de qualquer cartão.
        let err =
            call(&hub, "bot", "request_login_code", &json!({"login":"{{senha:github","reason":"Código"})).unwrap_err();
        assert!(err.to_string().contains("referência de senha"));
        assert_eq!(hub.store.history("bot", i64::MAX, 30).unwrap().len(), 0);
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn inspect_reads_only_login_and_secret_cards_with_their_context() {
        let (hub, dir) = fixture();
        // Pedido inexistente e tipo errado recusam.
        assert!(inspect(&hub, &json!({"entryId":"nao-existe"})).is_err());
        let lane = Lane::main("bot");
        let notice = hub.add_entry(&lane, EntryKind::Notice, 1, &json!({"text":"oi"})).unwrap();
        assert!(inspect(&hub, &json!({"entryId":notice.id})).is_err(), "sem connectionRequest");
        // Login pendente: o main deriva todos os textos e o contexto da entrada.
        let r = call(&hub, "bot", "request_login", &json!({"site":"github.com","reason":"Entre"})).unwrap();
        let id = r["requestId"].as_str().unwrap();
        let got = inspect(&hub, &json!({"entryId":id})).unwrap();
        assert_eq!(got["entryId"], id);
        assert_eq!(got["kind"], "login");
        assert_eq!(got["status"], "pending");
        assert_eq!(got["title"], "github.com");
        assert_eq!(got["site"], "github.com");
        assert_eq!(got["botId"], "bot");
        assert_eq!(got["botName"], "bot");
        assert_eq!(got["spaceId"], crate::fork::spaces::chat_space(&hub, "bot").unwrap());
        assert_eq!(got["conversationId"], "bot");
        assert!(got.get("value").is_none() && got.get("password").is_none(), "nunca devolve segredo");
        // Depois de concluído, o status muda e a janela não abre mais.
        finish(&hub, &json!({"entryId":id,"username":"kevin","value":"hunter2-private","label":"Conta"}))
            .await
            .unwrap();
        let done = inspect(&hub, &json!({"entryId":id})).unwrap();
        assert_eq!(done["status"], "ready");
        // Secret também é lido; connection/app/agent não.
        let r = call(
            &hub,
            "bot",
            "request_secret",
            &json!({"connectorId":"fixture","field":"API_KEY","location":"env","reason":"Teste"}),
        )
        .unwrap();
        let sid = r["requestId"].as_str().unwrap();
        let got = inspect(&hub, &json!({"entryId":sid})).unwrap();
        assert_eq!(got["kind"], "secret");
        assert_eq!(got["field"], "API_KEY");
        assert_eq!(got["location"], "env");
        assert_eq!(got["title"], "Fixture");
        // Pedido vencido (mesma regra de idade do finish) não abre janela nem recebe segredo,
        // mesmo com status ainda pendente.
        let r = call(&hub, "bot", "request_login", &json!({"site":"vencido.com","reason":"Entre"})).unwrap();
        let velho = r["requestId"].as_str().unwrap();
        hub.store
            .db
            .locked()
            .execute("UPDATE entries SET created_at=?2 WHERE id=?1", rusqlite::params![velho, now_ms() - MAX_AGE - 1])
            .unwrap();
        assert!(inspect(&hub, &json!({"entryId":velho})).is_err(), "vencido não abre a janela");
        assert!(finish(&hub, &json!({"entryId":velho,"value":"x"})).await.is_err(), "vencido não recebe segredo");
        // Ingresso pela API: a rota é Leitura e devolve a mesma projeção, sem valor —
        // e não pede o cofre (o finish continua exigindo).
        let unlocks = hub.store.vault_unlock_attempts.load(std::sync::atomic::Ordering::SeqCst);
        let got = crate::api::dispatch(
            &hub,
            &crate::api::devices::Caller::Local,
            "connectorRequestGet",
            json!({"entryId": sid}),
        )
        .await
        .unwrap();
        assert_eq!(got["kind"], "secret");
        assert!(got.get("value").is_none() && got.get("password").is_none());
        assert_eq!(
            hub.store.vault_unlock_attempts.load(std::sync::atomic::Ordering::SeqCst),
            unlocks,
            "a leitura não toca o cofre"
        );
        // Com o cofre trancado, a leitura segue abrindo o cartão; o finish é que exige o cofre.
        *hub.store.secret_key.locked() = None;
        let got = crate::api::dispatch(
            &hub,
            &crate::api::devices::Caller::Local,
            "connectorRequestGet",
            json!({"entryId": sid}),
        )
        .await
        .unwrap();
        assert_eq!(got["kind"], "secret");
        let unlocks = hub.store.vault_unlock_attempts.load(std::sync::atomic::Ordering::SeqCst);
        assert!(
            crate::api::dispatch(
                &hub,
                &crate::api::devices::Caller::Local,
                "connectorRequestFinish",
                json!({"entryId": sid, "value": "x"}),
            )
            .await
            .is_err(),
            "o finish continua exigindo o cofre"
        );
        assert!(
            hub.store.vault_unlock_attempts.load(std::sync::atomic::Ordering::SeqCst) > unlocks,
            "o finish tentou o cofre"
        );
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }
}
