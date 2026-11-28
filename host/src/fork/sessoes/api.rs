//! RPCs de sessões do 2a: `sessions {botId}`, `sessionCreate {botId, title?}` e
//! `sessionRename {botId, sessionId, title, expectedRevision}`, e o filtro `history.sessionId`. Criar não executa nada.
//! Encerrar (`sessionClose`), continuar (`sessionContinue`) e o arquivamento automático ficam em `encerrar` (PR5).
//!
//! D372: `sessionEngineSet {botId, sessionId, expectedRevision, engine}` troca a seleção de motor de UMA conversa do
//! Pessoal (`sessionId: null` = principal; nunca edita o padrão do bot). Cada sessão projeta `engine` (ou `null`).
//!
//! `sessions` responde também `sessionActions`: o anúncio, só de leitura, do que este Hub faz com extras. O app só
//! cria e envia numa extra quando o Hub diz `create` e `send` (UI-01): o `sessionCreate` já existia antes de o `send`
//! respeitar `sessionId`, e um Hub assim gravaria a mensagem da extra na principal. Não autoriza nada: cada RPC
//! continua validando bot, sessão, estado e conta como antes.
use super::engine_choice::{self, EngineRequest};
use super::{origin::SessionOrigin, store};
use crate::{LockExt, api::devices::Caller, hub::Hub};
use anyhow::{Context, Result, anyhow, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

/// O anúncio de `sessions`: `create` = `sessionCreate`; `send` = `send.sessionId` grava na extra indicada.
pub(crate) const SESSION_ACTIONS: [&str; 2] = ["create", "send"];
/// D372: `engine` = `sessionEngineSet`, anunciado só onde a conversa tem seleção própria (bot do Pessoal).
pub(crate) const ENGINE_ACTION: &str = "engine";

fn actions(c: &rusqlite::Connection, chat: &str) -> Result<Vec<&'static str>> {
    if crate::environment::Environment::is_open() {
        return Ok(if store::personal_on(c, chat)? { SESSION_ACTIONS.to_vec() } else { Vec::new() });
    }
    let mut actions = SESSION_ACTIONS.to_vec();
    // Sessões PR5: encerrar e continuar, implementados por este Hub (Hub antigo não os anuncia; sem fallback).
    actions.extend(super::encerrar::ACTIONS);
    if engine_choice::personal_bot(c, chat)?.is_some() {
        actions.push(ENGINE_ACTION);
    }
    // G1b: `sessionChoiceAnswer`, com o mesmo escopo que a RPC confere (bot agente vivo do Pessoal). Não anuncia
    // `mostrar_componente`: a ferramenta MCP depende do selo do produtor C6 (`contexto_ferramenta`).
    if store::personal_on(c, chat)? {
        actions.push(super::escolha::ACTION);
        actions.push(super::excluir::ACTION);
    }
    Ok(actions)
}

pub(crate) fn call(hub: &Hub, method: &str, body: &Value) -> Result<Value> {
    let chat = body["botId"].as_str().ok_or_else(|| anyhow!("`botId` is required"))?;
    ensure!(hub.store.bot(chat)?.is_some_and(|b| !b.deleted), "unknown bot");
    if method == "sessions" {
        #[cfg(test)]
        crate::fork::sessoes::call_tests::before_effect(&hub.store, "sessions_write");
        return crate::fork::donna_fixa::pedido::gravar(&hub.store, |tx| {
            store::ensure_principal(tx, chat)?;
            let sessions = store::list(tx, chat)?;
            let actions = actions(tx, chat)?;
            Ok(json!({"sessions": sessions, "sessionActions": actions}))
        });
    }
    let reconnecting: Vec<String> = if method == "sessionCreate" && crate::environment::Environment::is_open() {
        crate::fork::engine_accounts::Registry::load(&hub.store)?
            .accounts
            .into_iter()
            .filter(|a| crate::fork::engine_setup::in_setup(&hub.store, &a.id))
            .map(|a| a.id)
            .collect()
    } else {
        Vec::new()
    };
    let db = hub.store.db.locked();
    let tx = db.unchecked_transaction()?;
    if method == "sessionCreate" {
        let title = match body.get("title").filter(|v| !v.is_null()) {
            Some(title) => title.as_str().ok_or_else(|| anyhow!("invalid_title"))?,
            None => "",
        };
        let created = store::create_in(
            &tx,
            &store::NewSession {
                chat: chat.to_owned(),
                origin: SessionOrigin::Owner,
                source_session: None,
                parent_session: None,
                title: title.to_owned(),
                runs_on: None,
            },
            &reconnecting,
        )?;
        // Título dado na criação é do dono: a 1ª mensagem não o troca.
        if !title.trim().is_empty() {
            tx.execute("UPDATE donna_bot_sessions SET title_source='owner' WHERE id=?1", [&created.id])?;
        }
        let session = store::get(&tx, &created.id)?.ok_or_else(|| anyhow!("unknown_session"))?;
        tx.commit()?;
        drop(db);
        let _ = hub.events.send(json!({"type": "session", "session": session}));
        return Ok(json!({"session": session}));
    }
    let id = body["sessionId"].as_str().ok_or_else(|| anyhow!("`sessionId` is required"))?;
    let title = body["title"].as_str().ok_or_else(|| anyhow!("`title` is required"))?;
    let expected = body["expectedRevision"].as_i64().ok_or_else(|| anyhow!("`expectedRevision` is required"))?;
    let session = store::rename(&tx, chat, id, title, expected)?;
    tx.commit()?;
    drop(db);
    let _ = hub.events.send(json!({"type": "session", "session": session}));
    Ok(json!({"session": session}))
}

/// O pedido `sessionEngineSet`. `sessionId` é obrigatório no corpo (`null` = principal).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EngineSet {
    bot_id: String,
    session_id: Option<String>,
    expected_revision: i64,
    engine: EngineRequest,
}

/// `sessionEngineSet`: guarda do dono (`engine.accounts`, SAAS-04) resolvida na entrada e conservada até o commit; a
/// gravação roda na transação da guarda (`Authority::with_db`), com o cadastro lido na mesma `Connection`. Sem
/// fallback: Hub sem esta RPC recusa, e o app nunca troca o padrão do bot no lugar. Revisão divergente devolve
/// `session_revision_conflict`; o app relê e pede ação nova.
///
/// D422 (método `Dono` em `donna_fixa::metodos`): vale para todo bot do Pessoal, inclusive a Donna. A guarda já é a do
/// dono pelo app: Local, ou o Device com Control, a autoridade ORIGINAL do transporte (`Autoridade::capturar`) e o
/// vínculo `engine.accounts`. `Authority::Owner::with_db` é `Autoridade::with_db(hub, Some(self), ..)`, que reconfere
/// as duas antes e depois da escrita na mesma `TxImmediate`, sem guarda aninhada nem Store/Cloud no callback. Device
/// sem Control, Ponte, Pairing, Bootstrap, bot e rotina não chegam a essa guarda e recusam sem efeito.
pub(crate) async fn engine_set(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    ensure!(body.get("sessionId").is_some(), "invalid_field: sessionId é obrigatório.");
    let complete =
        body["engine"].as_object().is_some_and(|e| engine_choice::ENGINE_KEYS.iter().all(|k| e.contains_key(*k)));
    ensure!(complete, "invalid_field: engine incompleto.");
    let request: EngineSet =
        serde_json::from_value(body).map_err(|_| anyhow!("invalid_field: pedido de modelo inválido."))?;
    let authority =
        crate::fork::engine_setup::access::resolve(hub, caller).map_err(|_| anyhow!("engine_owner_only"))?;
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let hub = hub.clone();
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        let chat = request.bot_id;
        let id = request.session_id.unwrap_or_else(|| chat.clone());
        // D422: ponto de gravação do dono (gancho de teste `before_owner_write`); a autoridade original capturada na
        // entrada é reconferida dentro da Tx abaixo, junto do CAS.
        crate::fork::donna_fixa::pedido::antes_de_gravar(&hub.store);
        let (session, from, to) = authority.with_db(&hub, |db| {
            let (from, to) = engine_choice::set(db, &chat, &id, request.expected_revision, &request.engine)?;
            let session = store::get(db, &id)?.ok_or_else(|| anyhow!("unknown_session"))?;
            Ok((session, from, to))
        })?;
        let _ = hub.events.send(json!({"type": "session", "session": session}));
        // Turno desta conversa em andamento (rodando, pedindo você ou em espera): ele termina com a configuração
        // capturada; a seleção nova vale no próximo. Só informativo: a admissão decide pela captura do turno.
        let kind = engine_choice::switch_kind(from.as_ref(), &to);
        let runtime = hub.runtime(&chat);
        let busy = runtime.status != crate::hub::BotStatus::Idle
            && runtime.lane.as_ref().is_some_and(|lane| lane.chat == chat && lane.session_id() == id);
        Ok(json!({
            "session": session,
            "switch": {
                "from": from.map(|from| from.view(request.expected_revision)),
                "to": to.view(session.revision),
                "kind": kind,
                "appliesAt": if busy { "nextTurn" } else { "now" },
            },
        }))
    })
    .await?
}

/// O `sessionId` opcional de `history`: tem de ser uma sessão desta conversa.
pub(crate) fn history_session<'a>(hub: &Hub, chat: &str, body: &'a Value) -> Result<Option<&'a str>> {
    let Some(id) = body.get("sessionId").filter(|v| !v.is_null()) else { return Ok(None) };
    let id = id.as_str().ok_or_else(|| anyhow!("invalid_session"))?;
    if id != chat {
        let session = store::get(&hub.store.db.locked(), id)?;
        ensure!(session.is_some_and(|s| s.chat_id == chat), "unknown_session");
    }
    Ok(Some(id))
}

/// A `Lane` de `send.sessionId`. Ausente, `null` ou o id da conversa = principal (clientes antigos iguais).
/// Sessão de outra conversa → `unknown_session`; encerrada → `session_closed`. Seleção própria ilegível falha fechado
/// (principal ou extra). A extra sem seleção (NULL) também falha fechado na pré-checagem: nunca cai no padrão do bot.
pub(crate) fn send_lane(c: &rusqlite::Connection, chat: &str, body: &Value) -> Result<crate::store::Lane> {
    let id = match body.get("sessionId").filter(|v| !v.is_null()) {
        None => chat,
        Some(id) => id.as_str().ok_or_else(|| anyhow!("invalid_session"))?,
    };
    let session = store::get(c, id)?.filter(|s| s.chat_id == chat);
    let Some(session) = session else {
        // Conversa sem linha principal ainda (bot recém-migrado): a principal continua aceita como antes.
        ensure!(id == chat, "unknown_session");
        return Ok(crate::store::Lane::main(chat));
    };
    ensure!(session.state == "open", "session_closed");
    engine_choice::read(c, id).context("engine_unavailable")?;
    if session.principal {
        return Ok(crate::store::Lane::main(chat));
    }
    // A extra roda em série no próprio slot; a admissão (recibo + CAS da escolha) acontece no `queued` → `sent`.
    let lane = crate::store::Lane { chat: chat.to_owned(), session: Some(session.id) };
    if crate::environment::Environment::is_open() {
        ensure!(
            store::personal_on(c, chat)? && session.runs_on.is_none() && session.origin == SessionOrigin::Owner,
            "session_chat_unsupported"
        );
    } else {
        extra_admissible(c, &lane)?;
    }
    Ok(lane)
}

const EXTRA_UNAVAILABLE: &str = "session_selection_unavailable: esta sessão precisa de conta gerenciada.";

/// Recusa antecipada, só de leitura, de uma extra que o permit nunca admitiria: sem ela, o pedido ficaria pendente
/// e travaria a fila do bot inteiro. Usa a classificação canônica do Roteamento (`admission_class`) sobre a escolha
/// fixada ou o Fixo original do bot, numa transação descartada. A revisão da conta é a do cadastro agora, só para
/// recusar cedo; não é captura nem prova. A admissão e todas as validações do permit continuam obrigatórias e
/// valem contra troca ou revogação depois do envio. `NotRouted` ou erro recusam antes de gravar a entrada.
pub(crate) fn extra_admissible(c: &rusqlite::Connection, lane: &crate::store::Lane) -> Result<()> {
    let snapshot = c.is_autocommit().then(|| c.unchecked_transaction()).transpose()?;
    let c = snapshot.as_deref().unwrap_or(c);
    let check = || -> Result<bool> {
        let mut cfg = crate::store::Store::bot_on(c, &lane.chat)?
            .filter(|b| !b.deleted)
            .ok_or_else(|| anyhow!("unknown bot"))?
            .config;
        // A seleção do próximo turno (a próxima do dono, se houver), que é a que este envio vai executar.
        super::admissao::apply_configured(c, lane, &mut cfg)?;
        let account = match cfg.fork_editor.engine_account_id.as_deref() {
            None => None,
            Some(id) => {
                let registry = crate::fork::engine_accounts::Registry::load_db(c)?;
                Some((id, registry.account(id)?.managed.as_ref().map_or(1, |m| m.revision)))
            }
        };
        let selection = crate::fork::spaces::EngineSelection {
            backend: cfg.backend.clone(),
            model: cfg.model.clone(),
            model_options: cfg.model_options.clone(),
        };
        let class = crate::fork::roteamento::receipt::admission_class(c, &cfg, true, false, account, &selection)?;
        Ok(matches!(class, crate::fork::roteamento::receipt::RoutingClass::Routed {}))
    };
    ensure!(check().context(EXTRA_UNAVAILABLE)?, EXTRA_UNAVAILABLE);
    Ok(())
}

/// Título da 1ª mensagem do dono numa extra, publicado com o evento `session`. Falha não desfaz o envio já gravado.
#[cfg(test)]
pub(crate) fn title_first_message(hub: &Hub, lane: &crate::store::Lane, text: &str) {
    title_first_message_authorized(hub, lane, text, None);
}

pub(crate) fn title_first_message_authorized(
    hub: &Hub,
    lane: &crate::store::Lane,
    text: &str,
    autoridade: Option<&crate::fork::autoridade::Autoridade>,
) {
    let Some(session) = lane.session.as_deref() else { return };
    let titled = if let Some(autoridade) = autoridade {
        autoridade.with_db(hub, None, |tx| super::titulo::first_message(tx, session, text))
    } else {
        (|| -> Result<Option<store::SessionRow>> {
            let db = hub.store.db.locked();
            let tx = db.unchecked_transaction()?;
            let row = super::titulo::first_message(&tx, session, text)?;
            crate::fork::sessao_clerk::conferir_no_efeito(&tx)?;
            tx.commit()?;
            Ok(row)
        })()
    };
    match titled {
        Ok(Some(session)) => {
            let _ = hub.events.send(json!({"type": "session", "session": session}));
        }
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "título da sessão não gravado"),
    }
}
