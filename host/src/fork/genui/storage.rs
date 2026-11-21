//! Registro de pergunta, não ledger de trabalho. Nenhuma função agenda/publica/retoma.
//! O chamador (`super::flow`, G1b; `sessoes::escolha` na resposta da extra) autentica origem e
//! execução, abre a transação e a propaga até o commit da guarda final. Em qualquer erro,
//! descarta a transação inteira. Usamos apenas a mesma conexão recebida.
//!
//! Dois domínios que não se misturam ([`Domain`]), escolhidos pelo chamador e nunca pela presença
//! do carimbo: conversa de Space (JOIN em `donna_space_conversations` + `spaceContext`) e extra de
//! sessão do Pessoal (G1b sessões, errata E1: `donna_bot_sessions` extra aberta deste bot agente vivo
//! do Pessoal; origem e projeção com o `spaceContext` canônico da PRINCIPAL do mesmo bot).
use super::contract::{Answer, ValidatedChoice, literal};
use crate::fork::sessoes;
use crate::store::{EntryKind, Lane, insert_visible_on, next_rev, now_ms};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_genui_questions(
            id TEXT PRIMARY KEY, invocation TEXT NOT NULL UNIQUE, payload_hash TEXT NOT NULL,
            origin_entry TEXT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
            chat TEXT NOT NULL, session TEXT NOT NULL, space TEXT NOT NULL,
            entry_id TEXT NOT NULL UNIQUE REFERENCES entries(id) ON DELETE CASCADE,
            payload TEXT NOT NULL, snapshot_text TEXT NOT NULL,
            marker_entry TEXT, response_marker TEXT,
            state TEXT NOT NULL CHECK(state IN ('pending','answered','cancelled','expired')),
            revision INTEGER NOT NULL CHECK(revision>0), expires_at INTEGER,
            answer TEXT, response_entry TEXT UNIQUE REFERENCES entries(id) ON DELETE CASCADE,
            CHECK((state='answered' AND answer IS NOT NULL AND response_entry IS NOT NULL)
                OR (state!='answered' AND answer IS NULL AND response_entry IS NULL))
        );",
    )?;
    Ok(())
}

/// Limpa somente nossos registros na transação de exclusão do bot. A conexão
/// produtiva pode manter `foreign_keys=OFF`; a integridade não depende de cascatas.
pub(crate) fn on_bot_deleted(c: &Connection, bot: &str) -> Result<()> {
    c.execute(
        "DELETE FROM donna_genui_questions WHERE chat=?1
            OR chat IN (SELECT id FROM donna_space_conversations WHERE bot_id=?1)",
        [bot],
    )?;
    Ok(())
}

/// Referência interna, nunca desserializada do modelo. É vínculo de armazenamento,
/// NÃO prova de autoridade. Lane/Space são conferidos contra a entrada canônica.
pub(crate) struct Context<'a> {
    pub lane: &'a Lane,
    pub space: &'a str,
    pub origin_entry: &'a str,
    /// Escolhido pelo chamador interno, nunca inferido da Lane: cada domínio tem seu ramo estrito.
    pub domain: Domain,
}

/// Onde a pergunta mora. Não há ramo que aceite qualquer Lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Domain {
    /// Conversa de Space capturada (caminho G1b existente).
    Conversation,
    /// Extra aberta de um bot agente do Pessoal (`Lane{chat: bot, session: Some(extra)}`).
    PersonalSession,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Pending,
    Answered,
    Cancelled,
    Expired,
}

impl State {
    #[cfg(test)]
    fn column(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Answered => "answered",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pendente",
            Self::Answered => "Respondida",
            Self::Cancelled => "Cancelada",
            Self::Expired => "Expirada",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Receipt {
    pub question_id: String,
    pub entry_id: String,
    pub revision: i64,
    pub state: State,
    pub response_entry: Option<String>,
    /// IDs duráveis dos marcadores já inseridos. G1b deverá publicar marcador → entrada
    /// somente após commit, inclusive no replay; nunca reinseri-los.
    pub marker_entry: Option<String>,
    pub response_marker: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Snapshot {
    pub receipt: Receipt,
    pub payload: String,
    #[cfg_attr(not(test), expect(dead_code, reason = "texto atual lido só pelos testes de projeção"))]
    pub text: String,
    pub answer: Option<Answer>,
}

struct Record {
    snapshot: Snapshot,
    origin_entry: String,
    chat: String,
    session: String,
    space: String,
    payload_hash: String,
    expires_at: Option<i64>,
}

fn read(c: &Connection, id: &str) -> Result<Record> {
    let row = c.query_row(
        "SELECT q.entry_id,q.revision,q.state,q.response_entry,q.payload,json_extract(e.data,'$.text'),q.answer,
            q.origin_entry,q.chat,q.session,q.space,q.payload_hash,q.expires_at,q.marker_entry,q.response_marker
            FROM donna_genui_questions q JOIN entries e ON e.id=q.entry_id WHERE q.id=?1",
        [id],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, String>(10)?,
                r.get::<_, String>(11)?,
                r.get::<_, Option<i64>>(12)?,
                r.get::<_, Option<String>>(13)?,
                r.get::<_, Option<String>>(14)?,
            ))
        },
    )?;
    Ok(Record {
        snapshot: Snapshot {
            receipt: Receipt {
                question_id: id.into(),
                entry_id: row.0,
                revision: row.1,
                state: serde_json::from_value(json!(row.2)).map_err(|_| anyhow!("genui_invalid_state"))?,
                response_entry: row.3,
                marker_entry: row.13,
                response_marker: row.14,
            },
            payload: row.4,
            text: row.5,
            answer: row.6.map(|s| serde_json::from_str(&s)).transpose().map_err(|_| anyhow!("genui_invalid_answer"))?,
        },
        origin_entry: row.7,
        chat: row.8,
        session: row.9,
        space: row.10,
        payload_hash: row.11,
        expires_at: row.12,
    })
}

/// Revalida vínculo e estado canônicos; não consulta Store/Cloud nem aceita carimbo do payload.
pub(super) fn context(c: &Connection, ctx: &Context<'_>) -> Result<i64> {
    ensure!(ctx.lane.session.as_deref() != Some(ctx.lane.chat.as_str()), "genui_noncanonical_lane");
    let session = sessoes::store::session_ref_of_entry(c, &ctx.lane.chat, ctx.origin_entry)?;
    ensure!(session.id == ctx.lane.session_id(), "genui_session_mismatch");
    let session = sessoes::store::get(c, &session.id)?.ok_or_else(|| anyhow!("genui_unknown_session"))?;
    ensure!(session.state == "open", "genui_session_closed");
    if ctx.domain == Domain::PersonalSession {
        personal_session(c, ctx, &session)?;
        return Ok(c.query_row("SELECT turn FROM entries WHERE id=?1", [ctx.origin_entry], |r| r.get(0))?);
    }
    let valid: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_space_conversations c JOIN bots b ON b.id=c.bot_id
            JOIN entries e ON e.id=?1 WHERE c.id=?2 AND c.space_id=?3 AND c.closed=0 AND b.deleted=0
            AND json_extract(e.data,'$.spaceContext.spaceId')=c.space_id
            AND json_extract(e.data,'$.spaceContext.conversationId')=c.id
            AND json_extract(e.data,'$.spaceContext.botId')=c.bot_id)",
        params![ctx.origin_entry, ctx.lane.chat, ctx.space],
        |r| r.get(0),
    )?;
    ensure!(valid, "genui_scope_mismatch");
    Ok(c.query_row("SELECT turn FROM entries WHERE id=?1", [ctx.origin_entry], |r| r.get(0))?)
}

/// Ramo estrito da extra do Pessoal, na mesma Connection: a sessão da Lane é extra (não principal) deste
/// chat e está aberta (conferido acima; a sessão vem só de `donna_bot_sessions` e da coluna `session` da
/// entrada, nunca do carimbo), o Space é o Pessoal e a origem tem o carimbo canônico da principal (E1).
fn personal_session(c: &Connection, ctx: &Context<'_>, session: &sessoes::store::SessionRow) -> Result<()> {
    ensure!(ctx.lane.session.is_some() && !session.principal, "genui_session_mismatch");
    ensure!(session.chat_id == ctx.lane.chat, "genui_session_mismatch");
    ensure!(ctx.space == crate::fork::spaces::PERSONAL, "genui_scope_mismatch");
    personal_stamp(c, &ctx.lane.chat, ctx.origin_entry)
}

/// E1: entrada de extra do Pessoal leva o `spaceContext` que `spaces::stamp_entry` grava pelo chat, isto é, o da
/// PRINCIPAL do mesmo bot. Aceita só: `version` inteiro 1; `botId` e `conversationId` strings iguais ao chat;
/// `spaceId` string igual ao Pessoal; `conversationRevision` inteiro em 1..=revisão atual da principal (sanidade:
/// salvar o bot sobe a revisão sem invalidar a pergunta, e ela não concede nem tira domínio). A linha principal
/// (`id`=`bot_id`=chat, `is_primary`=1, Space Pessoal) tem de existir e o bot tem de ser agente vivo do Pessoal.
/// Carimbo ausente, JSON `null`, campo ausente ou de outro tipo, outro bot/chat/Space ou conversa adicional recusam
/// com `genui_scope_mismatch`; sem linha principal, falha fechada (sem fallback).
fn personal_stamp(c: &Connection, chat: &str, entry: &str) -> Result<()> {
    const MISMATCH: &str = "genui_scope_mismatch";
    let personal = crate::fork::spaces::PERSONAL;
    ensure!(sessoes::store::personal_on(c, chat)?, MISMATCH);
    let current: Option<i64> = c
        .query_row(
            "SELECT revision FROM donna_space_conversations WHERE id=?1 AND bot_id=?1 AND is_primary=1 AND space_id=?2",
            params![chat, personal],
            |r| r.get(0),
        )
        .optional()?;
    let current = current.ok_or_else(|| anyhow!(MISMATCH))?;
    let data = crate::store::entry_on(c, entry)?.ok_or_else(|| anyhow!("unknown_entry"))?.data;
    let stamp = &data["spaceContext"];
    let exact = |key: &str, want: &str| stamp[key].as_str() == Some(want);
    ensure!(
        stamp["version"].as_i64() == Some(1)
            && exact("botId", chat)
            && exact("conversationId", chat)
            && exact("spaceId", personal)
            && stamp["conversationRevision"].as_i64().is_some_and(|r| (1..=current).contains(&r)),
        MISMATCH
    );
    Ok(())
}

fn bound(c: &Connection, ctx: &Context<'_>, id: &str) -> Result<Record> {
    context(c, ctx)?;
    let record = read(c, id)?;
    ensure!(
        record.origin_entry == ctx.origin_entry
            && record.chat == ctx.lane.chat
            && record.session == ctx.lane.session_id()
            && record.space == ctx.space,
        "genui_question_scope_mismatch"
    );
    check_projection(c, ctx, &record.snapshot.receipt.entry_id)?;
    Ok(record)
}

/// A entrada de projeção (pergunta ou gráfico) tem de continuar na mesma sessão e Space do contexto.
pub(super) fn check_projection(c: &Connection, ctx: &Context<'_>, entry_id: &str) -> Result<()> {
    let entry_session = sessoes::store::session_ref_of_entry(c, &ctx.lane.chat, entry_id)?;
    ensure!(entry_session.id == ctx.lane.session_id(), "genui_entry_session_mismatch");
    if ctx.domain == Domain::PersonalSession {
        // A projeção também com o carimbo canônico da principal (E1). Sem corte `context_start_seq` (é de conversa
        // de Space); o equivalente da extra é o fechamento.
        return personal_stamp(c, &ctx.lane.chat, entry_id);
    }
    let stamped: bool = c.query_row(
        "SELECT json_extract(e.data,'$.spaceContext.spaceId')=c.space_id
            AND json_extract(e.data,'$.spaceContext.conversationId')=c.id
            AND json_extract(e.data,'$.spaceContext.botId')=c.bot_id
            FROM entries e JOIN donna_space_conversations c ON c.id=e.bot_id
            WHERE e.id=?1 AND c.space_id=?2",
        params![entry_id, ctx.space],
        |r| r.get(0),
    )?;
    ensure!(stamped, "genui_entry_space_mismatch");
    // Defesa em profundidade: pergunta de antes da limpeza nunca entra no contexto novo.
    let current: bool = c.query_row(
        "SELECT e.seq>=s.context_start_seq FROM entries e JOIN donna_space_conversations s ON s.id=e.bot_id
            WHERE e.id=?1",
        [entry_id],
        |r| r.get(0),
    )?;
    ensure!(current, "genui_context_reset");
    Ok(())
}

/// Identidade de invocação é parâmetro interno, não campo do payload nem grant.
/// G1b a deriva do envelope da ponte e da execução autenticada (`super::flow::invocation_key`). Hash é do JSON
/// tipado; replay igual retorna o mesmo recibo, outra origem/payload/prazo conflita.
/// Prazo opcional pertence ao Hub; nenhum default é criado aqui.
pub(crate) fn create(
    tx: &Transaction<'_>,
    ctx: &Context<'_>,
    invocation: &str,
    payload: &ValidatedChoice,
    expires_at: Option<i64>,
) -> Result<Receipt> {
    let turn = context(tx, ctx)?;
    ensure!(!invocation.is_empty() && invocation.len() <= 256, "genui_invalid_invocation");
    let raw = payload.snapshot()?;
    let hash = crate::agent::registry::sha256_hex(raw.as_bytes());
    let used_by_chart: bool =
        tx.query_row("SELECT EXISTS(SELECT 1 FROM donna_genui_charts WHERE invocation=?1)", [invocation], |r| {
            r.get(0)
        })?;
    if used_by_chart {
        tracing::warn!("escolha recusada: a invocação já pertence a um gráfico");
        anyhow::bail!("genui_invocation_conflict");
    }
    let existing: Option<String> = tx
        .query_row("SELECT id FROM donna_genui_questions WHERE invocation=?1", [invocation], |r| r.get(0))
        .optional()?;
    if let Some(id) = existing {
        let record = bound(tx, ctx, &id)?;
        ensure!(record.payload_hash == hash && record.expires_at == expires_at, "genui_invocation_conflict");
        return Ok(record.snapshot.receipt);
    }
    ensure!(expires_at.is_none_or(|deadline| deadline > now_ms()), "genui_deadline_elapsed");
    let id = uuid::Uuid::new_v4().to_string();
    let text = payload.fallback(&id, State::Pending.label(), None);
    // Texto literal para app antigo; `genuiChoice` é a projeção server-owned para o cartão.
    let (marker, entry) = insert_visible_on(
        tx,
        &uuid::Uuid::new_v4().to_string(),
        ctx.lane,
        EntryKind::Notice,
        turn,
        &json!({"text":text,"genuiChoice":wire(&id, 1, State::Pending, &raw, None, None)}),
    )?;
    tx.execute(
        "INSERT INTO donna_genui_questions(id,invocation,payload_hash,origin_entry,chat,session,space,
            entry_id,payload,snapshot_text,state,revision,expires_at,marker_entry)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'pending',1,?11,?12)",
        params![
            id,
            invocation,
            hash,
            ctx.origin_entry,
            ctx.lane.chat,
            ctx.lane.session_id(),
            ctx.space,
            entry.id,
            raw,
            text,
            expires_at,
            marker.map(|e| e.id)
        ],
    )?;
    Ok(read(tx, &id)?.snapshot.receipt)
}

/// Leitura interna com revalidação. Reabrir é somente ler; não muda estado/revisão.
pub(crate) fn snapshot(c: &Connection, ctx: &Context<'_>, id: &str) -> Result<Snapshot> {
    Ok(bound(c, ctx, id)?.snapshot)
}

fn pending(record: &Record, revision: i64) -> Result<()> {
    ensure!(record.snapshot.receipt.revision == revision, "genui_revision_conflict");
    ensure!(record.snapshot.receipt.state == State::Pending, "genui_final_state");
    Ok(())
}

/// CAS grava resposta conversacional uma vez, no turno da origem (base G1a, só testes).
#[cfg(test)]
pub(crate) fn answer(
    tx: &Transaction<'_>,
    ctx: &Context<'_>,
    id: &str,
    revision: i64,
    answer: &Answer,
) -> Result<Receipt> {
    let turn = context(tx, ctx)?;
    respond(tx, ctx, id, revision, answer, turn, &json!({}))
}

/// CAS grava resposta conversacional uma vez. Não entrega turno nem agenda bot: o chamador
/// passa o turno novo alocado nesta transação e os campos da fila canônica (`extra`), e
/// entrega a entrada ao produtor de Sessões depois do commit.
pub(crate) fn respond(
    tx: &Connection,
    ctx: &Context<'_>,
    id: &str,
    revision: i64,
    answer: &Answer,
    turn: i64,
    extra: &serde_json::Value,
) -> Result<Receipt> {
    let record = bound(tx, ctx, id)?;
    pending(&record, revision)?;
    ensure!(record.expires_at.is_none_or(|deadline| deadline > now_ms()), "genui_deadline_elapsed");
    let payload = ValidatedChoice::parse(&record.snapshot.payload)?;
    let answer_text = payload.answer_text(answer)?;
    let response_id = uuid::Uuid::new_v4().to_string();
    let next = revision.checked_add(1).ok_or_else(|| anyhow!("genui_revision_overflow"))?;
    context(tx, ctx)?;
    // Não falsifica from/author; procedência fica numa marca própria server-owned.
    let mut data = json!({"text":literal(&format!("Resposta à pergunta {id}: {answer_text}")),
        "choice_response":{"version":1,"questionId":id,"revision":next}});
    for (key, value) in extra.as_object().into_iter().flatten() {
        ensure!(!matches!(key.as_str(), "text" | "choice_response" | "genuiChoice"), "genui_invalid_response");
        data[key] = value.clone();
    }
    let (marker, _) = insert_visible_on(tx, &response_id, ctx.lane, EntryKind::User, turn, &data)?;
    // A FK é imediata: a entrada existe antes do CAS, na mesma transação.
    // Em erro o chamador descarta tudo, inclusive entrada/marcador provisórios.
    let changed = tx.execute(
        "UPDATE donna_genui_questions SET state='answered',revision=?3,answer=?4,response_entry=?5,response_marker=?6
            WHERE id=?1 AND revision=?2 AND state='pending'",
        params![id, revision, next, serde_json::to_string(answer)?, response_id, marker.map(|e| e.id)],
    )?;
    ensure!(changed == 1, "genui_cas_conflict");
    let at = now_ms();
    let wire = wire(id, next, State::Answered, &record.snapshot.payload, Some(answer), Some(at));
    project(tx, &record, &payload.fallback(id, State::Answered.label(), Some(&answer_text)), &wire)?;
    Ok(read(tx, id)?.snapshot.receipt)
}

/// Apenas cancelamento/expiração pendentes. Não há operação de reabrir/revisar.
#[cfg(test)]
pub(crate) fn finish(
    tx: &Transaction<'_>,
    ctx: &Context<'_>,
    id: &str,
    revision: i64,
    state: State,
) -> Result<Receipt> {
    ensure!(matches!(state, State::Cancelled | State::Expired), "genui_invalid_transition");
    let record = bound(tx, ctx, id)?;
    pending(&record, revision)?;
    if state == State::Expired {
        ensure!(record.expires_at.is_some_and(|deadline| deadline <= now_ms()), "genui_not_expired");
    }
    let next = revision.checked_add(1).ok_or_else(|| anyhow!("genui_revision_overflow"))?;
    let changed = tx.execute(
        "UPDATE donna_genui_questions SET state=?3,revision=?4 WHERE id=?1 AND revision=?2 AND state='pending'",
        params![id, revision, state.column(), next],
    )?;
    ensure!(changed == 1, "genui_cas_conflict");
    let payload = ValidatedChoice::parse(&record.snapshot.payload)?;
    let wire = wire(id, next, state, &record.snapshot.payload, None, Some(now_ms()));
    project(tx, &record, &payload.fallback(id, state.label(), None), &wire)?;
    Ok(read(tx, id)?.snapshot.receipt)
}

/// Encerramento pelo Hub (conversa fechada/limpa ou sessão trocada): cancela as pendentes
/// do chat, ou só da sessão. Produtor interno, sem contexto vindo de fora; a sessão pode
/// já estar fechada nesta mesma transação. Devolve as entradas a publicar após o commit.
pub(crate) fn cancel_chat_on(c: &Connection, chat: &str, session: Option<&str>) -> Result<Vec<String>> {
    let ids = query_ids(
        c,
        "SELECT id FROM donna_genui_questions
            WHERE chat=?1 AND (?2 IS NULL OR session=?2) AND state='pending'",
        params![chat, session],
    )?;
    cancel_ids(c, &ids)
}

/// Sessões PR5 (P4): escolha pendente na sessão.
pub(crate) fn pending_on(c: &Connection, chat: &str, session: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_genui_questions WHERE chat=?1 AND session=?2 AND state='pending')",
        params![chat, session],
        |r| r.get(0),
    )?)
}

/// Varredura do início: pendentes cuja entrada sumiu; na conversa (`session` = `chat`), conversa fechada ou de antes
/// do corte de contexto (`context_start_seq`, limpeza interrompida por queda); na extra do Pessoal (C6, `session` ≠
/// `chat`), extra fechada, ausente ou de bot excluído, pela relação `donna_bot_sessions`/`bots` e nunca pelo carimbo
/// ou pelo corte da principal. Não recria nem repara sessão.
pub(crate) fn cancel_stale_on(c: &Connection) -> Result<Vec<String>> {
    let ids = query_ids(
        c,
        "SELECT q.id FROM donna_genui_questions q LEFT JOIN entries e ON e.id=q.entry_id
            LEFT JOIN donna_space_conversations s ON s.id=q.chat
            WHERE q.state='pending' AND (e.id IS NULL
                OR (q.session=q.chat AND (s.closed<>0 OR e.seq<s.context_start_seq))
                OR (q.session<>q.chat AND NOT EXISTS(SELECT 1 FROM donna_bot_sessions x
                    JOIN bots b ON b.id=x.chat_id AND b.deleted=0
                    WHERE x.id=q.session AND x.chat_id=q.chat AND x.principal=0 AND x.state='open')))",
        [],
    )?;
    cancel_ids(c, &ids)
}

fn query_ids(c: &Connection, sql: &str, args: impl rusqlite::Params) -> Result<Vec<String>> {
    Ok(c.prepare(sql)?.query_map(args, |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
}

fn cancel_ids(c: &Connection, ids: &[String]) -> Result<Vec<String>> {
    let mut entries = Vec::new();
    for id in ids {
        let orphan: bool = c.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM entries e JOIN donna_genui_questions q ON e.id=q.entry_id
                WHERE q.id=?1)",
            [id],
            |r| r.get(0),
        )?;
        if orphan {
            // Sem projeção (entrada já apagada): só o estado muda, para não travar o encerramento comum.
            c.execute(
                "UPDATE donna_genui_questions SET state='cancelled',revision=revision+1
                    WHERE id=?1 AND state='pending'",
                [id],
            )?;
            continue;
        }
        let record = read(c, id)?;
        let revision = record.snapshot.receipt.revision;
        let next = revision.checked_add(1).ok_or_else(|| anyhow!("genui_revision_overflow"))?;
        let changed = c.execute(
            "UPDATE donna_genui_questions SET state='cancelled',revision=?3
                WHERE id=?1 AND revision=?2 AND state='pending'",
            params![id, revision, next],
        )?;
        ensure!(changed == 1, "genui_cas_conflict");
        let payload = ValidatedChoice::parse(&record.snapshot.payload)?;
        let wire = wire(id, next, State::Cancelled, &record.snapshot.payload, None, Some(now_ms()));
        project(c, &record, &payload.fallback(id, State::Cancelled.label(), None), &wire)?;
        entries.push(record.snapshot.receipt.entry_id);
    }
    Ok(entries)
}

/// Onde a pergunta mora, sem prova de autoridade: o chamador confere conversa/Space do pedido
/// e só então monta o `Context` a partir destes valores guardados.
pub(crate) struct Location {
    pub chat: String,
    pub session: String,
    pub space: String,
    pub origin_entry: String,
}

pub(crate) fn locate(c: &Connection, id: &str) -> Result<Location> {
    find(c, id)?.ok_or_else(|| anyhow!("genui_unknown_question"))
}

/// Como [`locate`], mas a ausência é `None` e só erro técnico é `Err` (nunca confundidos).
pub(crate) fn find(c: &Connection, id: &str) -> Result<Option<Location>> {
    Ok(c.query_row("SELECT chat,session,space,origin_entry FROM donna_genui_questions WHERE id=?1", [id], |r| {
        Ok(Location { chat: r.get(0)?, session: r.get(1)?, space: r.get(2)?, origin_entry: r.get(3)? })
    })
    .optional()?)
}

/// Projeção server-owned que o app consome (`entry.data.genuiChoice`). Campos iguais ao
/// estado do Hub do `ChoiceCard`; `snapshot` é o JSON tipado canônico, já validado.
fn wire(
    id: &str,
    revision: i64,
    state: State,
    snapshot: &str,
    answer: Option<&Answer>,
    at: Option<i64>,
) -> serde_json::Value {
    let mut value = json!({"version":1,"questionId":id,"revision":revision,"state":state,"snapshot":snapshot});
    match (state, at) {
        (State::Answered, Some(at)) => {
            value["answer"] = json!(answer);
            value["answeredAt"] = json!(at);
        }
        (State::Cancelled | State::Expired, Some(at)) => value["at"] = json!(at),
        _ => {}
    }
    value
}

fn project(c: &Connection, record: &Record, text: &str, wire: &serde_json::Value) -> Result<()> {
    // Só o texto derivado e a projeção mudam; snapshot e valores originais são imutáveis.
    let changed = c.execute(
        "UPDATE entries SET data=json_set(data,'$.text',?2,'$.genuiChoice',json(?5)),rev=?3,updated_at=?4 WHERE id=?1",
        params![record.snapshot.receipt.entry_id, text, next_rev(c)?, now_ms(), wire.to_string()],
    )?;
    ensure!(changed == 1, "genui_missing_projection");
    Ok(())
}
