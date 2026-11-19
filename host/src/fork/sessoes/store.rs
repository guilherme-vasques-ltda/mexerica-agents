//! O catálogo `donna_bot_sessions`. Toda sessão nasce por [`create`] (produtor único); a principal, por
//! [`ensure_principal`]. Nenhum outro módulo escreve na tabela.
use super::origin::{self, SessionOrigin};
use super::runs_on::{self, RunsOn};
use crate::store::{next_rev, now_ms};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

/// Título mais longo aceito, em caracteres.
pub(crate) const TITLE_MAX: usize = 60;

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    let tx = c.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_bot_sessions(
            id TEXT PRIMARY KEY, chat_id TEXT NOT NULL, principal INTEGER NOT NULL DEFAULT 0,
            origin TEXT NOT NULL, source_session TEXT, parent_session TEXT, depth INTEGER NOT NULL DEFAULT 0,
            title TEXT NOT NULL DEFAULT '', title_source TEXT NOT NULL,
            state TEXT NOT NULL DEFAULT 'open', closed_reason TEXT, closed_at INTEGER,
            created_at INTEGER NOT NULL, last_used_at INTEGER NOT NULL, revision INTEGER NOT NULL, runs_on TEXT);
         CREATE UNIQUE INDEX IF NOT EXISTS donna_bot_sessions_principal ON donna_bot_sessions(chat_id) WHERE principal=1;
         CREATE INDEX IF NOT EXISTS donna_bot_sessions_chat ON donna_bot_sessions(chat_id, last_used_at);",
    )?;
    let has_session: bool =
        tx.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('entries') WHERE name='session')", [], |r| {
            r.get(0)
        })?;
    if !has_session {
        // NULL = principal: nenhuma entrada é reescrita.
        tx.execute("ALTER TABLE entries ADD COLUMN session TEXT", [])?;
    }
    tx.execute("CREATE INDEX IF NOT EXISTS entries_session ON entries(bot_id, session, seq)", [])?;
    // Antes de criar principais: elas já nascem com o retrato do padrão (D372).
    super::engine_choice::add_column(&tx)?;
    // Cada bot e grupo vivo, e cada conversa de Space, ganha a linha principal (id = id da conversa).
    let chats: Vec<String> = tx
        .prepare(
            "SELECT id FROM bots WHERE deleted=0 UNION SELECT id FROM donna_space_conversations
             EXCEPT SELECT chat_id FROM donna_bot_sessions WHERE principal=1",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for chat in chats {
        ensure_principal(&tx, &chat)?;
    }
    super::excluir::migrate(&tx)?;
    super::engine_choice::migrate(&tx)?;
    super::exposure::migrate(&tx)?;
    super::bindings::migrate(&tx)?;
    tx.commit()?;
    Ok(())
}

/// A linha principal da conversa, criada uma vez. Chamada ao salvar bot ou grupo e ao criar conversa de Space. A do
/// bot do Pessoal nasce com o retrato do padrão do bot neste instante (D372); salvar o bot de novo não a muda.
pub(crate) fn ensure_principal(c: &Connection, chat: &str) -> Result<()> {
    let exists: bool =
        c.query_row("SELECT EXISTS(SELECT 1 FROM donna_bot_sessions WHERE id=?1)", [chat], |r| r.get(0))?;
    if exists {
        return Ok(());
    }
    let now = now_ms();
    c.execute(
        "INSERT INTO donna_bot_sessions(id,chat_id,principal,origin,title,title_source,created_at,last_used_at,revision)
         VALUES(?1,?1,1,?2,'','principal',?3,?3,?4)",
        params![chat, SessionOrigin::Owner.column()?, now, next_rev(c)?],
    )?;
    super::engine_choice::snapshot_on_create(c, chat, chat, true)?;
    Ok(())
}

/// Pedido de sessão extra. `parent_session` e `runs_on` ficam sem uso no 2a, mas já são validados.
pub(crate) struct NewSession {
    pub chat: String,
    pub origin: SessionOrigin,
    pub source_session: Option<String>,
    pub parent_session: Option<String>,
    pub title: String,
    pub runs_on: Option<RunsOn>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionRef {
    pub id: String,
    pub chat_id: String,
}

/// O produtor único das sessões extras: valida tudo e grava na transação do chamador. Times e o 2b chamam esta
/// função, sem DDL própria. Sessões extras só existem no chat próprio de um bot do Pessoal.
pub(crate) fn create(c: &Connection, new: &NewSession) -> Result<SessionRef> {
    ensure!(personal_on(c, &new.chat)?, "session_chat_unsupported");
    ensure_principal(c, &new.chat)?;
    let same_chat = |id: &str| -> Result<Option<i64>> {
        Ok(c.query_row("SELECT depth FROM donna_bot_sessions WHERE id=?1 AND chat_id=?2", params![id, new.chat], |r| {
            r.get(0)
        })
        .optional()?)
    };
    if let Some(source) = &new.source_session {
        ensure!(same_chat(source)?.is_some(), "unknown_session");
    }
    let depth = match &new.parent_session {
        Some(parent) => same_chat(parent)?.ok_or_else(|| anyhow!("unknown_session"))? + 1,
        None => 0,
    };
    ensure!(depth <= 2, "session_too_deep");
    let title = clean_title(&new.title)?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    c.execute(
        "INSERT INTO donna_bot_sessions(id,chat_id,principal,origin,source_session,parent_session,depth,title,
            title_source,created_at,last_used_at,revision,runs_on)
         VALUES(?1,?2,0,?3,?4,?5,?6,?7,'first_message',?8,?8,?9,?10)",
        params![
            id,
            new.chat,
            new.origin.column()?,
            new.source_session,
            new.parent_session,
            depth,
            title,
            now,
            next_rev(c)?,
            runs_on::column(new.runs_on.as_ref())?
        ],
    )?;
    // D372: a extra nasce com o padrão do bot de agora, não do primeiro envio.
    super::engine_choice::snapshot_on_create(c, &id, &new.chat, false)?;
    Ok(SessionRef { id, chat_id: new.chat.clone() })
}

/// O escopo canônico das extras: `chat` é um bot agente vivo do Pessoal. Usado pelo produtor e reconferido pelo
/// fechamento automático na própria transação.
pub(crate) fn personal_on(c: &Connection, chat: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM bots WHERE id=?1 AND deleted=0
            AND COALESCE(json_extract(config,'$.kind'),'agent')='agent'
            AND COALESCE(json_extract(config,'$.spaceId'),?2)=?2)",
        params![chat, crate::fork::spaces::PERSONAL],
        |r| r.get(0),
    )?)
}

/// A sessão exata de uma entrada de `chat`, na transação do chamador (Times, D246): `entries.session` NULL é a
/// principal do chat; o resto, a linha de `donna_bot_sessions`. Usa só a `Connection` recebida. Entrada inexistente,
/// de outro chat ou de sessão fora do catálogo falha.
pub(crate) fn session_ref_of_entry(c: &Connection, chat: &str, entry_id: &str) -> Result<SessionRef> {
    let (owner, session): (String, Option<String>) = c
        .query_row("SELECT bot_id, session FROM entries WHERE id=?1", [entry_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?
        .ok_or_else(|| anyhow!("unknown_entry"))?;
    ensure!(owner == chat, "cross_conversation_entry");
    let id = session.unwrap_or_else(|| chat.to_owned());
    let catalog: Option<String> =
        c.query_row("SELECT chat_id FROM donna_bot_sessions WHERE id=?1", [&id], |r| r.get(0)).optional()?;
    ensure!(catalog.as_deref() == Some(chat), "unknown_session");
    Ok(SessionRef { id, chat_id: chat.to_owned() })
}

fn clean_title(title: &str) -> Result<String> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    ensure!(title.chars().count() <= TITLE_MAX, "invalid_title");
    Ok(title)
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionRow {
    pub id: String,
    pub chat_id: String,
    pub principal: bool,
    pub origin: SessionOrigin,
    pub source_session: Option<String>,
    pub parent_session: Option<String>,
    pub depth: i64,
    pub title: String,
    pub title_source: String,
    pub state: String,
    pub closed_reason: Option<String>,
    pub closed_at: Option<i64>,
    pub created_at: i64,
    pub last_used_at: i64,
    pub revision: i64,
    /// D372: a seleção própria da conversa; `null` fora do Pessoal ou se a gravada estiver ilegível (o app mostra
    /// indisponível e a execução falha fechado).
    pub engine: Option<super::engine_choice::EngineView>,
    /// Fora do fio no 2a.
    #[serde(skip)]
    pub runs_on: Option<RunsOn>,
}

const COLS: &str = "id,chat_id,principal,origin,source_session,parent_session,depth,title,title_source,state,
    closed_reason,closed_at,created_at,last_used_at,revision,runs_on,engine_choice,engine_next";

/// Linha bruta: sessão, origem, `runs_on`, seleção efetiva e próxima (D372).
type RawRow = (SessionRow, String, Option<String>, Option<String>, Option<String>);

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
    Ok((
        SessionRow {
            id: r.get(0)?,
            chat_id: r.get(1)?,
            principal: r.get(2)?,
            origin: SessionOrigin::Owner,
            source_session: r.get(4)?,
            parent_session: r.get(5)?,
            depth: r.get(6)?,
            title: r.get(7)?,
            title_source: r.get(8)?,
            state: r.get(9)?,
            closed_reason: r.get(10)?,
            closed_at: r.get(11)?,
            created_at: r.get(12)?,
            last_used_at: r.get(13)?,
            revision: r.get(14)?,
            engine: None,
            runs_on: None,
        },
        r.get(3)?,
        r.get(15)?,
        r.get(16)?,
        r.get(17)?,
    ))
}

/// A leitura falha com coluna inválida: nada vira `owner` nem local por engano.
/// A projeção mostra a seleção configurada para os próximos turnos (próxima ?? efetiva). Ilegível só some da
/// projeção (`engine: null`): a lista continua legível e a execução falha fechado.
fn checked((mut session, origin, runs_on, effective, next): RawRow) -> Result<SessionRow> {
    session.origin = origin::parse(&origin)?;
    session.runs_on = runs_on::parse(runs_on.as_deref())?;
    let parse = |raw: Option<String>| raw.as_deref().map(super::engine_choice::parse).transpose();
    session.engine = if let (Ok(effective), Ok(next)) = (parse(effective), parse(next)) {
        next.or(effective).map(|engine| engine.view(session.revision))
    } else {
        tracing::warn!(session = %session.id, "seleção da conversa ilegível");
        None
    };
    Ok(session)
}

pub(crate) fn get(c: &Connection, id: &str) -> Result<Option<SessionRow>> {
    c.query_row(&format!("SELECT {COLS} FROM donna_bot_sessions WHERE id=?1"), [id], row)
        .optional()?
        .map(checked)
        .transpose()
}

/// A principal primeiro; as outras, da usada mais recentemente para a mais antiga.
pub(crate) fn list(c: &Connection, chat: &str) -> Result<Vec<SessionRow>> {
    c.prepare(&format!(
        "SELECT {COLS} FROM donna_bot_sessions WHERE chat_id=?1 ORDER BY principal DESC, last_used_at DESC, id"
    ))?
    .query_map([chat], row)?
    .map(|r| checked(r?))
    .collect()
}

/// Renomeia com CAS na revisão. A principal não muda de nome.
pub(crate) fn rename(c: &Connection, chat: &str, id: &str, title: &str, expected: i64) -> Result<SessionRow> {
    let session = get(c, id)?.filter(|s| s.chat_id == chat).ok_or_else(|| anyhow!("unknown_session"))?;
    ensure!(!session.principal, "principal_title_fixed");
    if session.revision != expected {
        return Err(crate::remote::cloud::Conflict("session_revision_conflict").into());
    }
    let title = clean_title(title)?;
    ensure!(!title.is_empty(), "invalid_title");
    c.execute(
        "UPDATE donna_bot_sessions SET title=?2,title_source='owner',revision=?3 WHERE id=?1",
        params![id, title, next_rev(c)?],
    )?;
    get(c, id)?.ok_or_else(|| anyhow!("unknown_session"))
}

/// Sessões PR5: marca a extra aberta como encerrada, com motivo e revisão nova, na transação do chamador
/// (`encerrar`). A principal e uma sessão já encerrada não mudam.
pub(crate) fn mark_closed_on(c: &Connection, id: &str, reason: &str) -> Result<SessionRow> {
    let changed = c.execute(
        "UPDATE donna_bot_sessions SET state='closed',closed_reason=?2,closed_at=?3,revision=?4
         WHERE id=?1 AND principal=0 AND state='open'",
        params![id, reason, now_ms(), next_rev(c)?],
    )?;
    ensure!(changed == 1, "session_closed");
    get(c, id)?.ok_or_else(|| anyhow!("unknown_session"))
}

/// D545: ponto único de remoção do catálogo, na transação do controle do dono.
pub(super) fn remove_extra_on(c: &Connection, chat: &str, id: &str, revision: i64) -> Result<()> {
    let removed = c.execute(
        "DELETE FROM donna_bot_sessions WHERE id=?1 AND chat_id=?2 AND principal=0 AND revision=?3",
        params![id, chat, revision],
    )?;
    ensure!(removed == 1, "session_revision_conflict");
    Ok(())
}
