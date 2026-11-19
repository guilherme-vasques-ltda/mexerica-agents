//! Sono 2b1 (D384, seção 4 do desenho): prova por mensagem de que ela passou pela memória, o cursor só com `seq` e
//! o predicado único `valid_for_cleanup`, que a limpeza C7 chama na MESMA transação do DELETE.
//!
//! Tudo aqui é Connection-only: recebe a transação do chamador, não reentra no Store, não publica nada.
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    // A checagem é só de acréscimo: uma checagem invalidada fica de histórico e a rodada seguinte grava outra, com
    // `checked_at_rev` maior. Por isso a chave inclui `checked_at_rev`; a válida é sempre a mais nova.
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_memory_sleep_checks(
          bot TEXT NOT NULL,chat TEXT NOT NULL,message TEXT NOT NULL,rev INTEGER NOT NULL,run TEXT NOT NULL,
          status TEXT NOT NULL CHECK(status IN ('covered','excluded')),reason TEXT,checked_at_rev INTEGER NOT NULL,
          PRIMARY KEY(bot,chat,message,rev,checked_at_rev)) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS donna_memory_sleep_checks_message ON donna_memory_sleep_checks(message,rev);
         CREATE TABLE IF NOT EXISTS donna_memory_sleep_cursor(
          bot TEXT NOT NULL,chat TEXT NOT NULL,reviewed_through_seq INTEGER NOT NULL,schema_version INTEGER NOT NULL,
          run_id TEXT NOT NULL,updated_at INTEGER NOT NULL,PRIMARY KEY(bot,chat));",
    )?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Covered,
    Excluded,
}

/// Grava a checagem de `message` na revisão `rev`, com o contador global lido nesta transação.
pub(crate) fn record(
    db: &Connection,
    (bot, chat, run): (&str, &str, &str),
    (message, rev): (&str, i64),
    status: Status,
    reason: &str,
) -> Result<()> {
    let checked_at_rev = global_rev(db)?;
    db.execute(
        "INSERT OR IGNORE INTO donna_memory_sleep_checks(bot,chat,message,rev,run,status,reason,checked_at_rev)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            bot,
            chat,
            message,
            rev,
            run,
            if status == Status::Covered { "covered" } else { "excluded" },
            reason,
            checked_at_rev
        ],
    )?;
    Ok(())
}

fn global_rev(db: &Connection) -> Result<i64> {
    Ok(db.query_row("SELECT CAST(v AS INTEGER) FROM kv WHERE k='rev'", [], |r| r.get(0)).optional()?.unwrap_or(0))
}

/// Fatos que já se apoiaram em `message`: a linhagem só de acréscimo e, por segurança, a evidência atual.
const FACTS_OF: &str = "SELECT item FROM donna_memory_lineage WHERE kind='fact' AND message=?1
    UNION SELECT CAST(id AS TEXT) FROM memory_entries WHERE ','||evidence||',' LIKE '%,'||?1||',%'";

/// A checagem mais nova de `message` na sua revisão ATUAL ainda vale (R2-01, D409)? Inválida se a mensagem sumiu ou
/// mudou, se um trabalho não terminal ou uma sugestão pendente a cita, ou se um fato da sua linhagem mudou depois.
pub(crate) fn check_valid(db: &Connection, message: &str) -> Result<Option<Status>> {
    let Some(rev) =
        db.query_row("SELECT rev FROM entries WHERE id=?1", [message], |r| r.get::<_, i64>(0)).optional()?
    else {
        return Ok(None);
    };
    let row: Option<(String, i64)> = db
        .query_row(
            "SELECT status,checked_at_rev FROM donna_memory_sleep_checks WHERE message=?1 AND rev=?2
             ORDER BY checked_at_rev DESC LIMIT 1",
            params![message, rev],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((status, checked_at)) = row else { return Ok(None) };
    let invalid: bool = db.query_row(
        &format!(
            "SELECT EXISTS(SELECT 1 FROM donna_memory_auto_jobs j WHERE (j.status IN ('waiting','reserved','owner')
                OR j.outcome='running')
              AND (EXISTS(SELECT 1 FROM json_each(j.context,'$.messages') r WHERE json_extract(r.value,'$.id')=?1)
                OR EXISTS(SELECT 1 FROM json_each(j.context,'$.responses') r WHERE json_extract(r.value,'$.id')=?1)))
            OR EXISTS(SELECT 1 FROM donna_memory_suggestions s,json_each(s.operation,'$.evidence') e
              WHERE s.status='pending' AND e.value=?1)
            OR EXISTS(SELECT 1 FROM memory_entries m WHERE CAST(m.id AS TEXT) IN ({FACTS_OF}) AND m.rev>?2)"
        ),
        params![message, checked_at],
        |r| r.get(0),
    )?;
    Ok((!invalid).then_some(if status == "covered" { Status::Covered } else { Status::Excluded }))
}

/// O predicado da C7, na transação do DELETE. Pronto a partir da 2b1c, com a validação da linhagem inteira (D409)
/// na rodada do sono; a rodada também o usa para a prévia (contagem, sem efeito). Só `true` se `message` é humana deste chat, está até o cursor, tem
/// `covered` válida no `rev` atual e, para TODO fato da sua linhagem, todas as mensagens da linhagem desse fato têm
/// checagem válida (D409). Nota na linhagem, ou fato/nota que já não existe, retém (D412).
pub(crate) fn valid_for_cleanup(db: &Connection, bot: &str, chat: &str, message: &str) -> Result<bool> {
    let seq: Option<i64> = db
        .query_row("SELECT seq FROM entries WHERE id=?1 AND bot_id=?2", params![message, chat], |r| r.get(0))
        .optional()?;
    let cursor: Option<i64> = db
        .query_row(
            "SELECT reviewed_through_seq FROM donna_memory_sleep_cursor WHERE bot=?1 AND chat=?2",
            params![bot, chat],
            |r| r.get(0),
        )
        .optional()?;
    let (Some(seq), Some(cursor)) = (seq, cursor) else { return Ok(false) };
    if seq > cursor || check_valid(db, message)? != Some(Status::Covered) || pending_choice(db, message)? {
        return Ok(false);
    }
    let retained: bool = db.query_row(
        &format!(
            "SELECT EXISTS(SELECT 1 FROM donna_memory_lineage WHERE kind='note' AND message=?1)
             OR EXISTS(SELECT 1 FROM ({FACTS_OF}) f WHERE NOT EXISTS(SELECT 1 FROM memory_entries m
                WHERE CAST(m.id AS TEXT)=f.item))"
        ),
        [message],
        |r| r.get(0),
    )?;
    if retained {
        return Ok(false);
    }
    let mut stmt = db.prepare(&format!(
        "SELECT DISTINCT l.message FROM donna_memory_lineage l WHERE l.kind='fact' AND l.item IN ({FACTS_OF})
         UNION SELECT ?1"
    ))?;
    let others = stmt.query_map([message], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for other in others {
        if check_valid(db, &other)?.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// X = o maior `seq` tal que toda mensagem humana do chat até X tem checagem válida no `rev` atual. Só avança
/// (UPSERT com MAX), na transação que gravou as checagens. Devolve o cursor gravado.
pub(crate) fn advance_cursor(
    db: &Connection,
    (bot, chat, run): (&str, &str, &str),
    human: &[(i64, String)],
    now: i64,
) -> Result<i64> {
    let current = cursor(db, bot, chat)?;
    let mut through = current;
    for (seq, id) in human.iter().filter(|(seq, _)| *seq > current) {
        if check_valid(db, id)?.is_none() {
            break;
        }
        through = *seq;
    }
    if through > current || !exists(db, bot, chat)? {
        db.execute(
            "INSERT INTO donna_memory_sleep_cursor(bot,chat,reviewed_through_seq,schema_version,run_id,updated_at)
             VALUES(?1,?2,?3,1,?4,?5) ON CONFLICT(bot,chat) DO UPDATE SET
             reviewed_through_seq=MAX(reviewed_through_seq,excluded.reviewed_through_seq),run_id=excluded.run_id,
             updated_at=excluded.updated_at",
            params![bot, chat, through, run, now],
        )?;
    } else {
        db.execute(
            "UPDATE donna_memory_sleep_cursor SET run_id=?3,updated_at=?4 WHERE bot=?1 AND chat=?2",
            params![bot, chat, run, now],
        )?;
    }
    cursor(db, bot, chat)
}

pub(crate) fn cursor(db: &Connection, bot: &str, chat: &str) -> Result<i64> {
    Ok(db
        .query_row(
            "SELECT reviewed_through_seq FROM donna_memory_sleep_cursor WHERE bot=?1 AND chat=?2",
            params![bot, chat],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

fn exists(db: &Connection, bot: &str, chat: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_memory_sleep_cursor WHERE bot=?1 AND chat=?2)",
        params![bot, chat],
        |r| r.get(0),
    )?)
}

/// C7 para as entradas que NÃO são do dono (respostas do bot, cartões, avisos), na MESMA transação do DELETE, só
/// pela Connection recebida e só leitura. A mensagem do dono continua decidida por [`valid_for_cleanup`]. Só `true`
/// se: (1) o turno dela tem pelo menos uma mensagem do dono e TODAS passam em `valid_for_cleanup` (turno de rotina,
/// entre bots, só do bot ou só com o cartão D406 → `false`); (2) ela não é um cartão de memória nem um recibo
/// (`donna_memory_auto_receipts.id`, que é o id do cartão "Memória salva · Desfazer"), e o id dela não aparece na
/// linhagem, na evidência ou
/// na origem de nenhuma lembrança, versão ou nota, em nenhum trabalho de memória aberto, em nenhum recibo nem em
/// "não salvei", nem em sugestão; (3) o turno está encerrado (nenhum trabalho `running`, sessão do turno assentada)
/// e sem pergunta ao dono pendente (D406) nem cartão de escolha genUI pendente (SEG-518-02). Na dúvida, `false`.
pub(crate) fn turn_valid_for_cleanup(db: &Connection, entry: &str) -> Result<bool> {
    let Some(e) = db
        .query_row(
            &format!("SELECT {} FROM entries WHERE id=?1", crate::store::ENTRY_COLS),
            [entry],
            crate::store::row_entry,
        )
        .optional()?
    else {
        return Ok(false);
    };
    if e.is_user_authored() {
        return Ok(false);
    }
    // R1-518-01: o cartão de memória é o próprio recibo (mesmo id) e é dele que o Desfazer depende; ele também
    // guarda os `responseIds` que protegem as respostas. Cartão da memória automática ou legado (`memory` de fato,
    // `memoryFile` de nota), ou id de recibo em qualquer status: fica.
    let receipt: bool =
        db.query_row("SELECT EXISTS(SELECT 1 FROM donna_memory_auto_receipts WHERE id=?1)", [entry], |r| r.get(0))?;
    if receipt || pending_choice(db, entry)? || e.data["memory"].is_object() || e.data["memoryFile"].is_object() {
        return Ok(false);
    }
    let chat = e.bot_id.as_str();
    let bot: String = db
        .query_row("SELECT bot_id FROM donna_space_conversations WHERE id=?1", [chat], |r| r.get(0))
        .optional()?
        .unwrap_or_else(|| chat.to_owned());
    // (1) As mensagens do dono do turno, todas com a prova da C7.
    let owners = db
        .prepare(&format!(
            "SELECT {} FROM entries WHERE bot_id=?1 AND turn=?2 AND kind='user'",
            crate::store::ENTRY_COLS
        ))?
        .query_map(params![chat, e.turn], crate::store::row_entry)?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .filter(crate::store::Entry::is_user_authored)
        .collect::<Vec<_>>();
    if owners.is_empty() {
        return Ok(false);
    }
    for owner in &owners {
        if !valid_for_cleanup(db, &bot, chat, &owner.id)? {
            return Ok(false);
        }
    }
    // (2) Nada da memória cita esta entrada. Por texto, para não depender do formato de cada campo: um id é um UUID.
    let cited: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_memory_lineage WHERE message=?1)
         OR EXISTS(SELECT 1 FROM memory_entries WHERE ','||evidence||',' LIKE '%,'||?1||',%' OR instr(origin,?1)>0)
         OR EXISTS(SELECT 1 FROM donna_memory_versions WHERE ','||evidence||',' LIKE '%,'||?1||',%' OR instr(origin,?1)>0)
         OR EXISTS(SELECT 1 FROM memory_file_versions WHERE instr(origin,?1)>0)
         OR EXISTS(SELECT 1 FROM donna_memory_auto_jobs WHERE instr(context,?1)>0
            AND (status IN ('waiting','reserved','owner') OR outcome='running'))
         OR EXISTS(SELECT 1 FROM entries WHERE kind='notice' AND id<>?1 AND instr(data,?1)>0)
         OR EXISTS(SELECT 1 FROM donna_memory_auto_non_saved WHERE instr(refs,?1)>0)
         OR EXISTS(SELECT 1 FROM donna_memory_suggestions WHERE instr(operation,?1)>0 OR instr(COALESCE(receipt,''),?1)>0)",
        [entry],
        |r| r.get(0),
    )?;
    if cited {
        return Ok(false);
    }
    // (3) Turno encerrado e sem pergunta ao dono pendente. Sem o registro da sessão do turno, não se sabe: `false`.
    let session: Option<String> = db
        .query_row(
            "SELECT session FROM donna_memory_turns WHERE bot=?1 AND chat=?2 AND turn=?3",
            params![bot, chat, e.turn],
            |r| r.get(0),
        )
        .optional()?;
    let Some(session) = session else { return Ok(false) };
    let open: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_memory_auto_jobs WHERE chat=?1 AND outcome='running')
         OR EXISTS(SELECT 1 FROM donna_memory_unsettled WHERE bot=?2 AND session=?3)
         OR EXISTS(SELECT 1 FROM entries WHERE bot_id=?1 AND turn=?4 AND kind='notice'
            AND json_extract(data,'$.card')='ownerQuestion' AND json_extract(data,'$.ownerQuestion.status')='pending')",
        params![chat, bot, session, e.turn],
        |r| r.get(0),
    )?;
    Ok(!open)
}

/// SEG-518-02: a entrada é o cartão, o marcador ou a ORIGEM (inclusive a mensagem humana) de uma pergunta de escolha
/// genUI ainda pendente: é o vínculo canônico da pergunta, e apagá-la quebra o snapshot e a continuidade (a origem
/// apaga a pergunta em cascata). Respondida, cancelada ou expirada, vale a regra normal. Só a Connection recebida.
fn pending_choice(db: &Connection, entry: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_genui_questions WHERE state='pending'
            AND (entry_id=?1 OR marker_entry=?1 OR origin_entry=?1))",
        [entry],
        |r| r.get(0),
    )?)
}
