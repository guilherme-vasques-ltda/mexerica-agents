//! G1b: percurso ligado da pergunta de escolha no Hub, na ordem fixada pela passagem D273:
//! envelope autenticado da ponte → guarda do contexto (`ToolContext` + execução viva) →
//! storage/entrada canônica → commit → publicação em ordem → resposta autenticada por CAS →
//! entrega à fila canônica de Sessões. Sem fila, ledger, timer ou checkpoint próprios.
//!
//! Só conversa capturada (adicional, com `ToolContext`) ou extra selada do Pessoal (C6, variante pessoal do
//! `ToolContext`). Na conversa principal do bot a ferramenta não aparece: falta o vínculo de sessão da principal.
use super::chart_store;
use super::charts::ValidatedChart;
use super::contract::{Answer, ValidatedChoice};
use super::storage::{self, Context, Domain, State};
use crate::{
    LockExt,
    fork::sessoes::ExecutionRef,
    fork::spaces::tool_context::ToolContext,
    hub::Hub,
    store::{Entry, Lane, entry_on},
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, TransactionBehavior};
use serde_json::{Value, json};

pub(crate) const TOOL: &str = "mostrar_componente";
/// Campo do envelope que a ponte MCP do Hub põe no topo do corpo, fora de `arguments`.
pub(crate) const INVOCATION: &str = "invocation";
const MAX_INVOCATION: usize = 256;

/// Orientação ao motor: vai nas instruções do servidor `chat` somente quando a ponte
/// também anuncia a ferramenta (conversa capturada ou extra com sim atual do Hub).
pub(crate) const INSTRUCTIONS: &str = "Quando o dono precisar escolher entre poucas opções para você seguir, \
use mostrar_componente com component \"choice\": o Hub mostra um cartão de escolha nesta conversa ou sessão. Depois, encerre o \
turno e espere: a resposta chega como uma nova mensagem (opção, \"Nenhuma destas\" ou texto livre). Escolher não \
autoriza nenhuma ação nem vale como aprovação; no turno da resposta as ferramentas desta conversa ficam recusadas, \
então responda pela mensagem final. \
Para mostrar números ao dono (evolução, comparação, partes de um todo), use mostrar_componente com component \"line_chart\", \"bar_chart\" ou \"pie_chart\", só com dados reais que você já tem e sem inventar valores; o dono vê o gráfico e pode abrir Ver dados para a tabela. O gráfico só mostra: não espera resposta, então siga o turno e diga o essencial em texto.";

/// Recusa clara, sem eco, fora da conversa capturada.
pub(crate) const MAIN_REFUSAL: &str = "genui_conversation_required: mostrar_componente só existe numa conversa \
adicional ou numa sessão extra aberta com o selo do Hub; na conversa principal do bot ainda não aparece (falta o \
vínculo de sessão da principal).";

/// Identidade de invocação vinda do envelope da ponte. Sem ela, recusa: nunca cai para hash do payload.
pub(crate) fn invocation(body: &Value) -> Result<String> {
    let raw = body[INVOCATION].as_str().ok_or_else(|| anyhow!("genui_invocation_required"))?;
    ensure!(
        !raw.is_empty() && raw.len() <= MAX_INVOCATION && raw.bytes().all(|b| b.is_ascii_graphic()),
        "genui_invocation_required"
    );
    Ok(raw.to_owned())
}

/// Chave durável: a invocação da ponte vale só dentro da execução autenticada que a recebeu.
/// Outra execução, turno ou bot nunca colide nem reaproveita o recibo.
pub(crate) fn invocation_key(execution: &ExecutionRef, bot: &str, invocation: &str) -> String {
    let binding = execution.acp_binding().unwrap_or_default();
    let material = format!("genui-v1\n{binding}\n{}\n{bot}\n{invocation}", execution.turn_token());
    crate::agent::registry::sha256_hex(material.as_bytes())
}

/// Publicação depois do commit de quem cancelou na própria transação (limpeza/fechamento).
pub(crate) fn publish_committed(hub: &Hub, entries: Vec<Entry>) {
    let _emit = hub.emit_lock.locked();
    publish(hub, entries);
}

/// Publica em ordem de `rev`; o chamador já segura `emit_lock` e já comitou.
pub(crate) fn publish(hub: &Hub, mut entries: Vec<Entry>) {
    entries.sort_by_key(|e| e.rev);
    for entry in &entries {
        let _ = hub.events.send(json!({"type":"entry","rev":entry.rev,"entry":entry}));
    }
}

pub(crate) fn collect<'a>(c: &Connection, ids: impl IntoIterator<Item = Option<&'a str>>) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for id in ids.into_iter().flatten() {
        entries.push(entry_on(c, id)?.ok_or_else(|| anyhow!("genui_missing_projection"))?);
    }
    Ok(entries)
}

/// Grava (ou devolve no replay) a pergunta do turno vivo. O ator já conferiu o turno e segura
/// a barreira comum de update/delete; aqui a execução é rechecada na mesma transação.
pub(crate) fn show(hub: &Hub, ctx: &ToolContext, invocation: &str, payload: &ValidatedChoice) -> Result<Value> {
    let execution = ctx.turn.execution.as_ref().ok_or_else(|| anyhow!("chamada vencida: execução ausente"))?;
    let origin = ctx.turn.message.clone().ok_or_else(|| anyhow!("human_turn_required"))?;
    let key = invocation_key(execution, &ctx.cfg.id, invocation);
    let lane = ctx.lane();
    let _emit = hub.emit_lock.locked();
    // SEG-C6-01: na extra pessoal, a Ponte original segue emitida até o commit (coleção antes do DB).
    let (receipt, entries) = ctx.com_origem(hub, || {
        let mut db = hub.store.db.locked();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ctx.conferir_origem_on(&tx)?;
        crate::store::check_execution_on(&tx, &lane, Some(execution))?;
        // Domínio pela variante capturada (C6), nunca pelo prefixo, pela Lane ou pelo carimbo.
        let domain = ctx.domain();
        let scope = Context { lane: &lane, space: &ctx.cfg.space_id, origin_entry: &origin, domain };
        let receipt = storage::create(&tx, &scope, &key, payload, None)?;
        if domain == Domain::PersonalSession {
            // Origem E projeção (E1), inclusive a recém-inserida e o replay, antes do commit.
            storage::snapshot(&tx, &scope, &receipt.question_id)?;
        }
        // Replay republica as mesmas entradas, sem reinserir.
        let entries = collect(&tx, [receipt.marker_entry.as_deref(), Some(receipt.entry_id.as_str())])?;
        tx.commit()?;
        Ok((receipt, entries))
    })?;
    publish(hub, entries);
    Ok(json!({
        "shown": true,
        "questionId": receipt.question_id,
        "revision": receipt.revision,
        "state": receipt.state,
        "status": "waitingForAnswer",
    }))
}

/// Grava (ou devolve no replay) o gráfico de leitura do turno vivo. Mesma guarda da pergunta: o ator já conferiu o
/// turno e segura a barreira comum; aqui a execução e o vínculo são rechecados na mesma transação.
pub(crate) fn show_chart(hub: &Hub, ctx: &ToolContext, invocation: &str, chart: &ValidatedChart) -> Result<Value> {
    let execution = ctx.turn.execution.as_ref().ok_or_else(|| anyhow!("chamada vencida: execução ausente"))?;
    let origin = ctx.turn.message.clone().ok_or_else(|| anyhow!("human_turn_required"))?;
    let key = invocation_key(execution, &ctx.cfg.id, invocation);
    let lane = ctx.lane();
    let _emit = hub.emit_lock.locked();
    let (receipt, entries) = ctx.com_origem(hub, || {
        let mut db = hub.store.db.locked();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ctx.conferir_origem_on(&tx)?;
        crate::store::check_execution_on(&tx, &lane, Some(execution))?;
        let scope = Context { lane: &lane, space: &ctx.cfg.space_id, origin_entry: &origin, domain: ctx.domain() };
        let receipt = chart_store::create(&tx, &scope, &key, chart)?;
        // Replay republica as mesmas entradas, sem reinserir.
        let entries = collect(&tx, [receipt.marker_entry.as_deref(), Some(receipt.entry_id.as_str())])?;
        tx.commit()?;
        Ok((receipt, entries))
    })?;
    publish(hub, entries);
    Ok(json!({"shown": true, "chartId": receipt.chart_id, "status": "shown"}))
}

/// Resultado da resposta, para a RPC devolver ao app.
pub(crate) struct Answered {
    pub question: Entry,
    pub response: Entry,
    pub replayed: bool,
}

/// Conversa já resolvida pela RPC (audiência/Space conferidos pelo guard comum).
pub(crate) struct Target<'a> {
    pub chat: &'a str,
    pub space: &'a str,
    pub bot: &'a str,
    pub primary: bool,
    pub closed: bool,
    /// D395: o idioma guardado de quem responde (`idioma_app::do_chamador`), nunca do corpo.
    pub app_language: Option<&'static str>,
}

/// Intenção do app: `{questionId, revision, answer}`. CAS pending+revisão; replay idêntico devolve o
/// mesmo recibo. A resposta entra como `user` com `choice_response` e `status: queued`, e segue pela
/// fila canônica do ator (admissão de Sessões), inclusive após reinício.
pub(crate) fn answer(hub: &Hub, target: &Target<'_>, body: &Value) -> Result<Answered> {
    ensure!(!target.primary, "genui_conversation_required");
    ensure!(!target.closed, "conversation_closed");
    let id = body["questionId"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 64)
        .ok_or_else(|| anyhow!("genui_question_required"))?;
    let revision = body["revision"].as_i64().ok_or_else(|| anyhow!("genui_revision_required"))?;
    let answer: Answer = serde_json::from_value(body["answer"].clone()).map_err(|_| anyhow!("genui_invalid_answer"))?;
    {
        // Recusa cedo, sem eco, antes de resolver a configuração; a transação confere de novo.
        let at = storage::locate(&hub.store.db.locked(), id)?;
        ensure!(at.chat == target.chat && at.space == target.space, "genui_question_scope_mismatch");
    }
    let original = hub
        .store
        .bot(target.bot)?
        .filter(|row| !row.deleted && !row.config.is_group())
        .ok_or_else(|| anyhow!("bot_not_found"))?
        .config;
    let cfg = crate::fork::spaces::execution_config(&hub.store, &original, target.chat)?;
    let (lane, answered) = {
        let _emit = hub.emit_lock.locked();
        let mut db = hub.store.db.locked();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let at = storage::locate(&tx, id)?;
        // Sem eco: o pedido de outra conversa/Space recebe o mesmo código que vínculo trocado.
        ensure!(at.chat == target.chat && at.space == target.space, "genui_question_scope_mismatch");
        let lane = Lane { chat: at.chat.clone(), session: (at.session != at.chat).then(|| at.session.clone()) };
        let scope =
            Context { lane: &lane, space: &at.space, origin_entry: &at.origin_entry, domain: Domain::Conversation };
        let current = storage::snapshot(&tx, &scope, id)?;
        let replay = current.receipt.state == State::Answered
            && current.answer.as_ref() == Some(&answer)
            && revision.checked_add(1) == Some(current.receipt.revision);
        let (receipt, fresh) = if replay {
            (current.receipt, false)
        } else {
            let turn = crate::store::alloc_turn_on(&tx, &lane.chat)?;
            let stamp = crate::fork::spaces::execution_stamp_on(&tx, &cfg)?;
            let mut extra = json!({"status":"queued","attachments":[],"executionBinding":stamp});
            if let Some(idioma) = target.app_language {
                extra["appLanguage"] = json!(idioma);
            }
            (storage::respond(&tx, &scope, id, revision, &answer, turn, &extra)?, true)
        };
        let response = receipt.response_entry.as_deref().ok_or_else(|| anyhow!("genui_missing_projection"))?;
        let mut entries =
            collect(&tx, [receipt.response_marker.as_deref(), Some(response), Some(receipt.entry_id.as_str())])?;
        if fresh {
            tx.commit()?;
        }
        let question = entries.pop().ok_or_else(|| anyhow!("genui_missing_projection"))?;
        let response = entries.last().cloned().ok_or_else(|| anyhow!("genui_missing_projection"))?;
        if fresh {
            entries.push(question.clone());
            publish(hub, entries);
        }
        (lane, Answered { question, response, replayed: !fresh })
    };
    // Depois do commit: a mesma entrega da mensagem humana. O ator ignora entrada já admitida.
    // A entrada `queued` já é durável: sem ator agora, o início do Hub a retoma pela fila canônica.
    if answered.response.data["status"] == "queued" {
        let prompt = crate::chat::prompt::of_entry(hub, &cfg, &answered.response.data);
        let command = crate::agent::bot::Cmd::Send { lane, entry_id: answered.response.id.clone(), text: prompt };
        if let Err(error) = hub.send_cmd(target.bot, command) {
            tracing::warn!(%error, bot = target.bot, "resposta de escolha aguarda a retomada da fila");
        }
    }
    Ok(answered)
}

/// Encerramento da conversa/sessão: cancela as pendentes na transação do chamador.
pub(crate) fn cancel_on(c: &Connection, chat: &str, session: Option<&str>) -> Result<Vec<Entry>> {
    let ids = storage::cancel_chat_on(c, chat, session)?;
    collect(c, ids.iter().map(|id| Some(id.as_str())))
}

/// Sessões PR5 (P4): há escolha pendente nesta sessão? Leitura na transação do chamador.
pub(crate) fn pending_on(c: &Connection, chat: &str, session: &str) -> Result<bool> {
    storage::pending_on(c, chat, session)
}

/// Varredura do início: conversa fechada, corte de contexto anterior à pergunta ou entrada sumida.
pub(crate) fn sweep_on(c: &Connection) -> Result<Vec<Entry>> {
    let ids = storage::cancel_stale_on(c)?;
    collect(c, ids.iter().map(|id| Some(id.as_str())))
}

#[cfg(test)]
#[path = "flow_tests.rs"]
mod tests;
