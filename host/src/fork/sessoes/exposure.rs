//! Exposição externa por sessão visível (R4 #4). A linhagem do #125 marca a sessão ACP; aqui a marca também vai para
//! a sessão visível dona daquela ACP, e sobrevive à troca de ACP e à continuação (PR 4 e 5 do plano).
//! Os gatilhos copiam toda marca de `donna_memory_sessions` na mesma instrução: falhar ao gravar aqui faz a marca
//! falhar, e o chamador já trata isso como exposição (`memory_exposure_error`). Eles conferem a linha existente com
//! `NOT EXISTS`, e não com `OR IGNORE`: dentro de um gatilho, a política de conflito da instrução de fora (os UPSERTs
//! da linhagem) prevalece.
use anyhow::Result;
use rusqlite::{Connection, params};

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_session_acp(bot TEXT NOT NULL, acp_session TEXT NOT NULL,
            session TEXT NOT NULL, PRIMARY KEY(bot, acp_session));
         CREATE TABLE IF NOT EXISTS donna_session_exposure(bot TEXT NOT NULL, session TEXT NOT NULL,
            at INTEGER NOT NULL, reason TEXT, PRIMARY KEY(bot, session));
         CREATE TRIGGER IF NOT EXISTS donna_session_exposure_on_mark AFTER INSERT ON donna_memory_sessions
            WHEN NEW.marked_at IS NOT NULL BEGIN
            INSERT INTO donna_session_exposure(bot,session,at,reason)
                SELECT NEW.bot, a.session, NEW.marked_at, NEW.reason FROM donna_session_acp a
                WHERE a.bot=NEW.bot AND a.acp_session=NEW.session AND NOT EXISTS(
                    SELECT 1 FROM donna_session_exposure x WHERE x.bot=NEW.bot AND x.session=a.session);
         END;
         CREATE TRIGGER IF NOT EXISTS donna_session_exposure_on_remark AFTER UPDATE OF marked_at ON donna_memory_sessions
            WHEN NEW.marked_at IS NOT NULL BEGIN
            INSERT INTO donna_session_exposure(bot,session,at,reason)
                SELECT NEW.bot, a.session, NEW.marked_at, NEW.reason FROM donna_session_acp a
                WHERE a.bot=NEW.bot AND a.acp_session=NEW.session AND NOT EXISTS(
                    SELECT 1 FROM donna_session_exposure x WHERE x.bot=NEW.bot AND x.session=a.session);
         END;
         CREATE TRIGGER IF NOT EXISTS donna_session_exposure_on_bind AFTER INSERT ON donna_session_acp BEGIN
            INSERT INTO donna_session_exposure(bot,session,at,reason)
                SELECT NEW.bot, NEW.session, m.marked_at, m.reason FROM donna_memory_sessions m
                WHERE m.bot=NEW.bot AND m.session=NEW.acp_session AND m.marked_at IS NOT NULL AND NOT EXISTS(
                    SELECT 1 FROM donna_session_exposure x WHERE x.bot=NEW.bot AND x.session=NEW.session);
         END;",
    )?;
    // As ACPs que já existem: a principal em `bots.session_id`, os slots em `kv lane.session.<bot>@<bot>/<slot>`.
    c.execute(
        "INSERT OR IGNORE INTO donna_session_acp(bot,acp_session,session)
         SELECT id, session_id, id FROM bots WHERE session_id IS NOT NULL AND session_id<>''",
        [],
    )?;
    let slots: Vec<(String, String)> = c
        .prepare("SELECT k, v FROM kv WHERE k LIKE 'lane.session.%' AND v<>''")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (key, acp) in slots {
        let Some((bot, slot)) = key.strip_prefix("lane.session.").and_then(|rest| rest.split_once('/')) else {
            continue;
        };
        let Some((bot, _)) = bot.split_once('@') else { continue };
        bind(c, bot, &acp, &visible(bot, Some(slot)))?;
    }
    Ok(())
}

/// A sessão visível dona de um slot ACP do ator: `group:<g>` é a do grupo, `conversation:<c>` a da conversa de
/// Space e `session:<s>` a extra; a principal e as rotinas ficam na principal de `owner`.
pub(crate) fn visible(owner: &str, slot: Option<&str>) -> String {
    slot.and_then(|slot| ["group:", "conversation:", "session:"].iter().find_map(|prefix| slot.strip_prefix(prefix)))
        .unwrap_or(owner)
        .to_owned()
}

/// Liga uma ACP à sessão visível. Uma marca anterior da ACP passa para a sessão no mesmo comando.
pub(crate) fn bind(c: &Connection, bot: &str, acp: &str, session: &str) -> Result<()> {
    c.execute(
        "INSERT OR REPLACE INTO donna_session_acp(bot,acp_session,session) VALUES(?1,?2,?3)",
        params![bot, acp, session],
    )?;
    Ok(())
}

/// MX-106/D125: o contexto desta execução já recebeu conteúdo de fora. A marca da sessão visível cobre
/// entrada colada, anexo e resposta; a da ACP cobre resultado MCP, skill/histórico externo e a herança da
/// troca de ACP. A marca é persistente; falha de leitura conta como exposta.
pub(crate) fn context_marked(c: &Connection, bot: &str, session: &str, acp: &str) -> bool {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_memory_sessions WHERE bot=?1 AND session=?2 AND marked_at IS NOT NULL)
            OR EXISTS(SELECT 1 FROM donna_memory_sessions WHERE bot=?1 AND session=?3 AND marked_at IS NOT NULL)",
        params![bot, session, acp],
        |r| r.get(0),
    )
    .unwrap_or(true)
}

/// Incerteza fecha a porta: falha de leitura conta como exposta. Consumida pela troca de ACP e pela continuação.
pub(crate) fn exposed(c: &Connection, bot: &str, session: &str) -> bool {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_session_exposure WHERE bot=?1 AND session=?2)",
        params![bot, session],
        |r| r.get(0),
    )
    .unwrap_or(true)
}

/// Sessões PR5 (R4 #4, D125): a ACP nova de uma extra cuja sessão visível está exposta (continuação que herdou a
/// marca, ou troca de ACP) nasce marcada como externa. A principal segue a linhagem própria.
pub(crate) fn carry(store: &crate::store::Store, bot: &str, lane: &crate::store::Lane, acp: &str) -> Result<()> {
    if lane.session.is_none() {
        return Ok(());
    }
    // Mesma ordem de `memory_session_start`: fronteira de memória → DB.
    let _boundary = crate::LockExt::locked(&store.memory_boundary);
    let c = crate::LockExt::locked(&store.db);
    if exposed(&c, bot, lane.session_id()) {
        super::bindings::mark(&c, bot, acp, "session_exposure")?;
    }
    Ok(())
}

/// Sessões PR5: a última ACP ligada à sessão visível (`donna_session_acp`, gravada na ativação e mantida depois de
/// soltar o slot). É dela o resumo do motor que a continuação pode levar.
pub(crate) fn last_acp(store: &crate::store::Store, bot: &str, session: &str) -> Result<Option<String>> {
    use rusqlite::OptionalExtension;
    let c = crate::LockExt::locked(&store.db);
    Ok(c.query_row(
        "SELECT acp_session FROM donna_session_acp WHERE bot=?1 AND session=?2 ORDER BY rowid DESC LIMIT 1",
        params![bot, session],
        |r| r.get(0),
    )
    .optional()?)
}
