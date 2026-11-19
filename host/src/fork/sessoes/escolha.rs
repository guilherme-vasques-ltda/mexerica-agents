//! G1b em sessões (contrato v1 `sessionChoiceAnswer`, SHA256 `d883e822…`): o dono responde, pelo app, a uma
//! pergunta de escolha de uma EXTRA aberta de bot agente do Pessoal (inclusive o Berga). Fora: principal, Space,
//! grupo, canal, Times e rotina.
//!
//! Ordem fixa: decoder fechado → `questionId` → captura da autoridade ORIGINAL (antes de qualquer `await`) → bot vivo
//! → pré-checagem de admissão só de leitura, fora da guarda (resultado guardado, nunca prova) → `team_lock` →
//! `antes_de_gravar` → `emit_lock` → `Autoridade::with_db(None)` (sem `engine.accounts`), callback só com a
//! `Connection`: sessão (CAS) → pergunta (escopo, snapshot) → replay ou (`admissivel?` → CAS da pergunta). Depois do
//! commit, só a resposta nova publica, ainda sob `emit_lock`, e entrega `Cmd::Send` depois de soltar as travas.
//!
//! O produtor da pergunta numa extra (selo/ponte MCP, C6) fica em `contexto_ferramenta`, não aqui: esta RPC só
//! responde perguntas que o storage aceitou pelo ramo estrito de sessão (`genui::storage::Domain::PersonalSession`).
use super::store;
use crate::fork::autoridade::Autoridade;
use crate::fork::genui::{
    contract::Answer,
    flow,
    storage::{self, Context, Domain, State},
};
use crate::store::{Entry, Lane};
use crate::{LockExt, api::devices::Caller, hub::Hub};
use anyhow::{Context as _, Result, anyhow, ensure};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

/// `sessionActions`: `choice` = `sessionChoiceAnswer`, anunciado só em chat de bot agente vivo do Pessoal.
pub(crate) const ACTION: &str = "choice";
const INVALID: &str = "invalid_field: pedido de resposta inválido.";
/// Pergunta inexistente, de outra sessão ou de outro bot: um só código, sem eco (C3).
const SCOPE: &str = "genui_question_scope_mismatch";

/// O corpo de domínio que sobra depois do envelope comum (que já removeu `hubId`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    bot_id: String,
    session_id: String,
    expected_revision: i64,
    question_id: String,
    revision: i64,
    answer: Answer,
}

/// O que a Tx decidiu.
struct Outcome {
    session: store::SessionRow,
    /// Marcador (se houver), resposta e projeção da pergunta, nesta ordem.
    entries: Vec<Entry>,
    fresh: bool,
}

/// Pré-checagem de admissão da extra (C1), na ordem de `send_lane`. Só leitura, fora de Tx da guarda: o resultado só
/// é consultado no ramo novo, depois do CAS da sessão e da decisão de replay, e não é captura, prova nem fallback.
fn admissible(c: &Connection, lane: &Lane) -> Result<()> {
    super::engine_choice::read(c, lane.session_id()).context("engine_unavailable")?;
    super::api::extra_admissible(c, lane)
}

pub(crate) async fn answer(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    let request: Request = serde_json::from_value(body).map_err(|_| anyhow!(INVALID))?;
    ensure!(!request.question_id.is_empty() && request.question_id.len() <= 64, "genui_question_required");
    // C1: antes de qualquer espera e da pré-checagem; Device sem retrato original recusa aqui.
    let autoridade = Autoridade::capturar(hub, caller)?;
    super::encerrar::live_bot(hub, &request.bot_id)?;
    let lane = Lane { chat: request.bot_id.clone(), session: Some(request.session_id.clone()) };
    let admissivel = admissible(&hub.store.db.locked(), &lane);
    // D395: o idioma de quem responde, lido do Hub (nunca do corpo), fora da guarda.
    let app_language = crate::fork::idioma_app::do_chamador(&hub.store, caller);
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let worker = hub.clone();
    let outcome = tokio::task::spawn_blocking(move || -> Result<Outcome> {
        let _guard = guard;
        let hub = worker;
        crate::fork::donna_fixa::pedido::antes_de_gravar(&hub.store);
        let _emit = hub.emit_lock.locked();
        let outcome = autoridade.with_db(&hub, None, |c| decide(c, &request, &lane, admissivel, app_language))?;
        if outcome.fresh {
            flow::publish(&hub, outcome.entries.clone());
        }
        Ok(outcome)
    })
    .await??;
    let [.., response, question] = outcome.entries.as_slice() else { anyhow::bail!("genui_missing_projection") };
    // C2: replay nunca reencaminha. A resposta nova já é durável (`queued`): sem ator agora, a fila canônica a
    // retoma no início; o ator ignora entrada já admitida.
    if outcome.fresh && response.data["status"] == "queued" {
        let cfg = hub.store.bot(&response.bot_id)?.filter(|b| !b.deleted).map(|b| b.config);
        if let Some(cfg) = cfg {
            let text = crate::chat::prompt::of_entry(hub, &cfg, &response.data);
            let lane = Lane { chat: response.bot_id.clone(), session: Some(response.session_id.clone()) };
            let command = crate::agent::bot::Cmd::Send { lane, entry_id: response.id.clone(), text };
            if let Err(error) = hub.send_cmd(&response.bot_id, command) {
                tracing::warn!(%error, bot = %response.bot_id, "resposta de escolha aguarda a retomada da fila");
            }
        } else {
            tracing::warn!(bot = %response.bot_id, "resposta de escolha sem bot vivo para entregar");
        }
    }
    Ok(json!({
        "session": outcome.session,
        "question": question.data["genuiChoice"],
        "entry": response,
        "replayed": !outcome.fresh,
    }))
}

/// Callback da guarda: síncrono, só a `Connection` recebida, sem Store/Cloud/Tx nova. Primeiro erro vence.
fn decide(
    c: &Connection,
    request: &Request,
    lane: &Lane,
    admissivel: Result<()>,
    app_language: Option<&'static str>,
) -> Result<Outcome> {
    let (bot, id) = (request.bot_id.as_str(), request.session_id.as_str());
    let session = store::get(c, id)?.filter(|s| s.chat_id == bot).ok_or_else(|| anyhow!("unknown_session"))?;
    ensure!(!session.principal, "genui_conversation_required");
    ensure!(store::personal_on(c, bot)?, "session_chat_unsupported");
    ensure!(session.state == "open", "session_closed");
    if session.revision != request.expected_revision {
        return Err(crate::remote::cloud::Conflict("session_revision_conflict").into());
    }
    let personal = crate::fork::spaces::PERSONAL;
    let at = storage::find(c, &request.question_id)?.ok_or_else(|| anyhow!(SCOPE))?;
    ensure!(at.chat == bot && at.session == id && at.space == personal, SCOPE);
    let scope = Context { lane, space: personal, origin_entry: &at.origin_entry, domain: Domain::PersonalSession };
    let current = storage::snapshot(c, &scope, &request.question_id)?;
    let replay = current.receipt.state == State::Answered
        && current.answer.as_ref() == Some(&request.answer)
        && request.revision.checked_add(1) == Some(current.receipt.revision);
    let (receipt, fresh) = if replay {
        (current.receipt, false)
    } else {
        // Nova resposta: falha fechado com o erro guardado da pré-checagem, antes do CAS da pergunta.
        admissivel?;
        let turn = crate::store::alloc_turn_on(c, bot)?;
        // Sem `executionBinding` (selo de conversa de Space) nem `contentOrigin`; autoria nenhuma.
        let mut extra = json!({"status":"queued","attachments":[]});
        if let Some(idioma) = app_language {
            extra["appLanguage"] = json!(idioma);
        }
        let receipt =
            storage::respond(c, &scope, &request.question_id, request.revision, &request.answer, turn, &extra)?;
        (receipt, true)
    };
    let response = receipt.response_entry.as_deref().ok_or_else(|| anyhow!("genui_missing_projection"))?;
    let entries =
        flow::collect(c, [receipt.response_marker.as_deref(), Some(response), Some(receipt.entry_id.as_str())])?;
    let session = store::get(c, id)?.ok_or_else(|| anyhow!("unknown_session"))?;
    Ok(Outcome { session, entries, fresh })
}

#[cfg(test)]
#[path = "escolha_tests.rs"]
mod tests;
