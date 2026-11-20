//! MB-1b (D326, opção B): `ask_owner`, a pergunta explícita do bot ao dono. O cartão é uma entrada
//! `notice` no transcript da própria conversa (`data.card = "ownerQuestion"`), sem tabela nova. Só o Hub
//! muda o estado: `pending` → `answered` (na mesma transação da próxima mensagem humana daquela
//! conversa/sessão) ou `withdrawn` (pergunta nova do mesmo bot ali, reinício/limpeza da conversa,
//! fechamento da conversa, exclusão do bot ou da conversa). Nada é adivinhado pelo texto do bot.
//!
//! As funções `*_on` recebem a Connection da transação do chamador e nunca voltam ao Store (sem deadlock);
//! a publicação e a atualização do runtime acontecem depois do commit, fora das travas de DB.
use crate::{
    LockExt,
    hub::Hub,
    store::{Entry, EntryKind, Lane, entry_on, insert_visible_on, next_rev, now_ms},
};
use anyhow::{Result, bail, ensure};
use rusqlite::{Connection, TransactionBehavior, params};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) const TOOL: &str = "ask_owner";
pub(crate) const CARD: &str = "ownerQuestion";
/// Limite da pergunta, em caracteres.
pub(crate) const MAX_CHARS: usize = 500;
/// O runtime leva no máximo as cinco perguntas pendentes mais novas.
pub(crate) const MAX_PENDING: usize = 5;

/// Orientação ao motor. S2: vai SÓ nas instruções do servidor `chat`, que `tool_servers::decide` oferece apenas no
/// Pessoal; bot de outro Space ou identidade estrangeira não recebe a ferramenta nem esta orientação.
pub(crate) const INSTRUCTIONS: &str = "Quando precisar de uma resposta do dono para seguir, use ask_owner(question) \
em vez de terminar a mensagem com \"?\". A pergunta vai só no ask_owner: não repita o texto em send_message nem na \
resposta final. Depois, encerre o turno e espere: a próxima mensagem do dono nesta conversa é a resposta. \
ask_owner não existe em pedidos de outro bot ou de uma equipe nem em rotinas; ali, devolva a dúvida na resposta. \
Nesta etapa ask_owner existe só no Pessoal, onde este servidor chat é oferecido.";

/// Instruções do servidor `chat`: as do outbox e as de `ask_owner`.
pub(crate) static CHAT_INSTRUCTIONS: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| format!("{} {INSTRUCTIONS}", crate::chat::outbox::INSTRUCTIONS));

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Status {
    Pending,
    Answered,
    Withdrawn,
}

/// Uma pergunta pendente, como o runtime e o app a veem.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingQuestion {
    pub entry_id: String,
    pub chat_id: String,
    /// Sessão adicional do catálogo; ausente na principal da conversa.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub text: String,
    pub asked_at: i64,
}

pub(crate) fn tool() -> Value {
    json!({
        "name": TOOL,
        "description": "Faz uma pergunta ao dono quando você precisa da resposta dele para seguir (até 500 caracteres). \
    O Hub mostra um cartão de pergunta na conversa de origem e marca você como Precisa de você. Depois de perguntar, \
    encerre o turno e espere: não repita a pergunta em send_message nem na resposta final. A próxima mensagem do dono \
    nesta conversa (pela conversa ou pelo Mission Board) é a resposta; uma pergunta nova sua aqui retira a anterior. \
    Recusada em pedido de outro bot ou de uma equipe e em rotina, onde não há dono na conversa. O Hub recusa como chamada \
    vencida se o vínculo foi revogado ou a sessão de origem não está em andamento.",
        "inputSchema": {
            "type": "object",
            "properties": {"question": {"type": "string", "maxLength": MAX_CHARS, "description": "A pergunta, como o dono deve lê-la."}},
            "required": ["question"],
        },
        "annotations": {"readOnlyHint": false, "idempotentHint": false},
    })
}

/// O texto da pergunta: sem vazio, até [`MAX_CHARS`] caracteres.
pub(crate) fn question(args: &Value) -> Result<String> {
    let text = args["question"].as_str().unwrap_or_default().trim();
    if text.is_empty() {
        bail!("question is required");
    }
    if text.chars().count() > MAX_CHARS {
        bail!("question must be at most {MAX_CHARS} characters");
    }
    Ok(text.to_owned())
}

const PENDING: &str = "kind='notice' AND json_extract(data,'$.card')='ownerQuestion' \
    AND json_extract(data,'$.ownerQuestion.status')='pending'";
const PENDING_E: &str = "e.kind='notice' AND json_extract(e.data,'$.card')='ownerQuestion' \
    AND json_extract(e.data,'$.ownerQuestion.status')='pending'";

fn ids(c: &Connection, sql: &str, args: impl rusqlite::Params) -> Result<Vec<String>> {
    Ok(c.prepare(sql)?.query_map(args, |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
}

/// Muda o estado dos cartões ainda pendentes; devolve os que mudaram, para publicar após o commit.
fn transition_on(c: &Connection, ids: &[String], status: Status, answered_by: Option<&str>) -> Result<Vec<Entry>> {
    let mut changed = Vec::new();
    for id in ids {
        let Some(mut entry) = entry_on(c, id)? else { continue };
        if entry.data["ownerQuestion"]["status"] != "pending" {
            continue;
        }
        entry.data["ownerQuestion"]["status"] = json!(status);
        entry.data["ownerQuestion"]["answeredBy"] = answered_by.map_or(Value::Null, |m| json!(m));
        entry.data["ownerQuestion"]["closedAt"] = json!(now_ms());
        c.execute(
            "UPDATE entries SET rev=?2,data=?3,updated_at=?4 WHERE id=?1",
            params![id, next_rev(c)?, entry.data.to_string(), now_ms()],
        )?;
        crate::fork::memory_auto::owner::close_on(c, id)?;
        if let Some(entry) = entry_on(c, id)? {
            changed.push(entry);
        }
    }
    Ok(changed)
}

/// Grava a pergunta nova e retira a anterior do mesmo bot nesta conversa/sessão, na mesma transação.
/// O chamador já conferiu a execução viva nesta Connection.
pub(crate) fn ask_on(
    c: &Connection,
    bot: &str,
    lane: &Lane,
    turn: i64,
    text: &str,
) -> Result<(Vec<Entry>, Option<Entry>, Entry)> {
    let previous = ids(
        c,
        &format!(
            "SELECT id FROM entries WHERE bot_id=?1 AND COALESCE(session,bot_id)=?2 \
             AND json_extract(data,'$.author')=?3 AND {PENDING}"
        ),
        params![lane.chat, lane.session_id(), bot],
    )?;
    let withdrawn = transition_on(c, &previous, Status::Withdrawn, None)?;
    let data = json!({
        "text": text,
        "author": bot,
        "card": CARD,
        "ownerQuestion": {
            "status": Status::Pending,
            "chatId": lane.chat,
            "sessionId": lane.session_id(),
            "askedAt": now_ms(),
            "answeredBy": Value::Null,
        },
    });
    let (marker, card) = insert_visible_on(c, &uuid::Uuid::new_v4().to_string(), lane, EntryKind::Notice, turn, &data)?;
    Ok((withdrawn, marker, card))
}

/// Ajuste B: UMA mensagem humana na conversa/sessão responde TODAS as perguntas pendentes ali, de qualquer
/// bot (num canal com vários bots, cada um recebe a mesma mensagem no seu turno de sala).
pub(crate) fn answer_on(c: &Connection, lane: &Lane, message_id: &str) -> Result<Vec<Entry>> {
    let pending = ids(
        c,
        &format!("SELECT id FROM entries WHERE bot_id=?1 AND COALESCE(session,bot_id)=?2 AND {PENDING}"),
        params![lane.chat, lane.session_id()],
    )?;
    transition_on(c, &pending, Status::Answered, Some(message_id))
}

/// Reinício/limpeza ou fechamento da conversa: todas as sessões, ou só uma.
pub(crate) fn withdraw_chat_on(c: &Connection, chat: &str, session: Option<&str>) -> Result<Vec<Entry>> {
    let pending = ids(
        c,
        &format!(
            "SELECT id FROM entries WHERE bot_id=?1 AND (?2 IS NULL OR COALESCE(session,bot_id)=?2) AND {PENDING}"
        ),
        params![chat, session],
    )?;
    // G1b: os cartões de escolha são cancelados na própria transação da limpeza/fechamento.
    transition_on(c, &pending, Status::Withdrawn, None)
}

/// Sessões PR5 (P4): há pergunta pendente nesta sessão? Leitura na transação do chamador.
pub(crate) fn pending_in_on(c: &Connection, chat: &str, session: &str) -> Result<bool> {
    Ok(c.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM entries WHERE bot_id=?1 AND COALESCE(session,bot_id)=?2 AND {PENDING})"),
        params![chat, session],
        |r| r.get(0),
    )?)
}

/// Exclusão de um bot ou de uma conversa de canal (o id do canal é o do chat): perguntas dele e nele.
pub(crate) fn withdraw_bot_on(c: &Connection, id: &str) -> Result<Vec<Entry>> {
    let pending = ids(
        c,
        &format!("SELECT id FROM entries WHERE (bot_id=?1 OR json_extract(data,'$.author')=?1) AND {PENDING}"),
        [id],
    )?;
    transition_on(c, &pending, Status::Withdrawn, None)
}

/// As pendentes do bot, as mais novas primeiro, no máximo [`MAX_PENDING`].
pub(crate) fn pending_on(c: &Connection, bot: &str) -> Result<Vec<PendingQuestion>> {
    let mut st = c.prepare(&format!(
        "SELECT id,bot_id,COALESCE(session,bot_id),json_extract(data,'$.text'),
            COALESCE(json_extract(data,'$.ownerQuestion.askedAt'),created_at)
         FROM entries WHERE json_extract(data,'$.author')=?1 AND {PENDING}
         ORDER BY seq DESC LIMIT ?2"
    ))?;
    let rows = st.query_map(params![bot, i64::try_from(MAX_PENDING)?], |r| {
        let chat: String = r.get(1)?;
        let session: String = r.get(2)?;
        Ok(PendingQuestion {
            entry_id: r.get(0)?,
            session_id: (session != chat).then_some(session),
            chat_id: chat,
            text: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            asked_at: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// As pendentes e a revisão global do banco lidas sob o MESMO lock: a lista é o estado daquela revisão.
/// Reinício do Hub não esquece perguntas pendentes (o ator nasce com elas).
pub(crate) fn load(store: &crate::store::Store, bot: &str) -> (Vec<PendingQuestion>, i64) {
    let c = store.db.locked();
    let read = (|| -> Result<_> {
        let rev: i64 =
            c.query_row("SELECT COALESCE((SELECT CAST(v AS INTEGER) FROM kv WHERE k='rev'),0)", [], |r| r.get(0))?;
        Ok((pending_on(&c, bot)?, rev))
    })();
    read.unwrap_or_else(|error| {
        tracing::warn!(%error, bot, "perguntas pendentes não lidas");
        (Vec::new(), 0)
    })
}

/// S1: aplica a leitura só se for mais nova que a já aplicada. Duas atualizações fora de ordem (ler [Q],
/// outra Tx responde e aplica [], a primeira retoma) não ressuscitam uma pergunta encerrada. O callback só
/// compara campos do runtime: nenhum DB dentro de `set_runtime`, ordem de locks inalterada.
fn apply(hub: &Hub, bot: &str, questions: Vec<PendingQuestion>, rev: i64) {
    hub.set_runtime(bot, |r| {
        if rev > r.pending_questions_rev {
            r.pending_questions = questions;
            r.pending_questions_rev = rev;
        }
    });
}

/// D1: varredura idempotente no início do Hub. Retira as pendentes cujo autor ou conversa foi excluído, cuja
/// conversa de Space está fechada ou cuja conversa foi limpa depois da pergunta (queda entre a operação e a
/// retirada). Não muda a transação da operação original.
pub(crate) fn sweep(hub: &Hub) {
    let result = commit(hub, |c| {
        let stale = ids(
            c,
            &format!(
                "SELECT e.id FROM entries e WHERE {PENDING_E} AND (
                    EXISTS(SELECT 1 FROM bots b WHERE b.id=e.bot_id AND b.deleted<>0)
                    OR EXISTS(SELECT 1 FROM bots b WHERE b.id=json_extract(e.data,'$.author') AND b.deleted<>0)
                    OR EXISTS(SELECT 1 FROM donna_space_conversations s WHERE s.id=e.bot_id
                        AND (s.closed<>0 OR s.context_start_seq>e.seq)))"
            ),
            [],
        )?;
        let mut changed = transition_on(c, &stale, Status::Withdrawn, None)?;
        crate::fork::memory_auto::owner::sweep_on(c)?;
        changed.extend(crate::fork::genui::flow::sweep_on(c)?);
        Ok(((), changed))
    });
    if let Err(error) = result {
        tracing::error!(%error, "varredura de perguntas pendentes falhou");
    }
}

fn authors(entries: &[Entry]) -> BTreeSet<String> {
    entries.iter().filter_map(|e| e.data["author"].as_str().map(str::to_owned)).collect()
}

/// Atualiza `Runtime.pending_questions` dos bots afetados; fora das travas de emissão e DB.
pub(crate) fn refresh<'a>(hub: &Hub, bots: impl IntoIterator<Item = &'a String>) {
    for bot in bots {
        let (questions, rev) = load(&hub.store, bot);
        // Não fabrica um runtime observado só para dizer "nenhuma". Todo ator nasce com runtime (`spawn_bot`),
        // então a revisão de S1 sempre fica registrada para quem pode perguntar.
        if questions.is_empty() && hub.observed_runtime(bot).is_none() {
            continue;
        }
        #[cfg(test)]
        crate::fork::sessoes::call_tests::before_effect(&hub.store, "owner_question_refresh");
        apply(hub, bot, questions, rev);
    }
}

/// Transação IMMEDIATE sob a trava do stream; publica na ordem de rev e depois atualiza os runtimes.
fn commit<T>(hub: &Hub, change: impl FnOnce(&Connection) -> Result<(T, Vec<Entry>)>) -> Result<T> {
    let (value, entries) = {
        let _emit = hub.emit_lock.locked();
        let (value, mut entries) = {
            let mut db = hub.store.db.locked();
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let out = change(&tx)?;
            tx.commit()?;
            out
        };
        entries.sort_by_key(|e| e.rev);
        for entry in &entries {
            let _ = hub.events.send(json!({"type":"entry","rev":entry.rev,"entry":entry}));
        }
        (value, entries)
    };
    refresh(hub, &authors(&entries));
    Ok(value)
}

/// A pergunta do turno vivo: execução conferida na mesma transação do cartão.
pub(crate) fn ask(
    hub: &Hub,
    bot: &str,
    lane: &Lane,
    turn: i64,
    text: &str,
    execution: &crate::fork::sessoes::ExecutionRef,
) -> Result<Entry> {
    #[cfg(test)]
    crate::fork::sessoes::call_tests::before_effect(&hub.store, "owner_question_commit");
    commit(hub, |c| {
        crate::store::check_execution_on(c, lane, Some(execution))?;
        let (withdrawn, marker, card) = ask_on(c, bot, lane, turn, text)?;
        let mut entries = withdrawn;
        entries.extend(marker);
        entries.push(card.clone());
        Ok((card, entries))
    })
}

/// Grava a mensagem HUMANA e, na mesma transação, responde as perguntas pendentes da conversa/sessão.
/// Substitui `Hub::add_entry` nos envios do dono (`send` da conversa/canal e da conversa de Space).
/// A chave antiduplicata (`clientNonce`) e o limite da fila são conferidos na mesma transação da gravação: dois
/// envios simultâneos com a mesma chave dão uma linha só, e a fila cheia recusa sem criar nada.
pub(crate) fn add_human_message(hub: &Hub, lane: &Lane, turn: i64, data: &Value) -> Result<Human> {
    commit(hub, |c| {
        if let Some(original) = crate::store::Store::replay_on(c, &lane.chat, data)? {
            return Ok((Human::Replayed(original), Vec::new()));
        }
        if data["status"] == "queued" {
            let waiting: i64 = c.query_row(
                "SELECT COUNT(*) FROM entries WHERE bot_id = ?1 AND kind = 'user'
                    AND (CASE WHEN json_valid(data) THEN json_extract(data, '$.status') END) = 'queued'
                    AND (CASE WHEN json_valid(data) THEN json_extract(data, '$.choice_response') END) IS NULL
                    AND COALESCE(CASE WHEN json_valid(data) THEN json_extract(data, '$.from') END, '') <> 'host'",
                [&lane.chat],
                |r| r.get(0),
            )?;
            ensure!(waiting < QUEUE_MAX, "queue_full: esta conversa já tem {QUEUE_MAX} mensagens na fila");
        }
        let (marker, entry) =
            insert_visible_on(c, &uuid::Uuid::new_v4().to_string(), lane, EntryKind::User, turn, data)?;
        let answered = answer_on(c, lane, &entry.id)?;
        let mut entries: Vec<Entry> = marker.into_iter().collect();
        entries.push(entry.clone());
        entries.extend(answered);
        Ok((Human::New(entry), entries))
    })
}

/// Recusa por fila cheia: quem envia sozinho (tarefas) espera a vez sem contar como falha.
pub(crate) fn queue_full(error: &anyhow::Error) -> bool {
    format!("{error:#}").contains("queue_full")
}

/// Mensagens novas do dono que uma conversa pode ter esperando a vez; a seguinte é recusada com `queue_full`.
/// Respostas a cartões do próprio bot e confirmações do Hub não passam por aqui, nunca são recusadas e não contam.
pub(crate) const QUEUE_MAX: i64 = 50;

/// O que o envio do dono gravou.
pub(crate) enum Human {
    New(Entry),
    /// Reenvio com a mesma chave e a mesma intenção: a mensagem original, sem linha nova.
    Replayed(Entry),
}

/// Reinício, limpeza ou fechamento da conversa (todas as sessões quando `session` é `None`).
pub(crate) fn withdraw_chat(hub: &Hub, chat: &str, session: Option<&str>) {
    if let Err(error) = commit(hub, |c| Ok(((), withdraw_chat_on(c, chat, session)?))) {
        tracing::error!(%error, chat, "perguntas pendentes não retiradas");
    }
}

/// Exclusão confirmada de um bot ou canal.
pub(crate) fn deleted(hub: &Hub, id: &str) {
    if let Err(error) = commit(hub, |c| Ok(((), withdraw_bot_on(c, id)?))) {
        tracing::error!(%error, id, "perguntas pendentes não retiradas na exclusão");
    }
    if hub.observed_runtime(id).is_some() {
        apply(hub, id, Vec::new(), i64::MAX);
    }
}

#[cfg(test)]
mod tests;
