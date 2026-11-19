//! Sono 2b1b/2b1c (D384/D413, `(arquivo de design privado do projeto de origem, fora deste repositório)`): uma rodada por bot por dia de calendário, com
//! o bot parado há 30 min, puxada pelo dreamer (sem timer novo). Não chama modelo. Prova, por mensagem, o que já
//! passou pela memória (`sleep_checks`) e avança o cursor da conversa na mesma transação. Lembrança duvidosa
//! (conflito, inferência, prazo, fixada) segura as mensagens que a sustentam. A 2b1c acrescenta o atalho literal:
//! a inferência sem conflito, com a linhagem inteira comprovada nesta conversa, cujo texto está numa mensagem limpa
//! do dono vira "Você disse" (`auto`, com recibo e Desfazer) ou sugestão (`suggest`).
//!
//! Fila própria (`donna_memory_sleep_jobs`), separada da extração: as consultas da extração (reserva, limites,
//! expiração, recuperação, auditoria) não mudam e nunca veem um trabalho de sono, então o sono nunca passa à frente
//! nem bloqueia uma extração (R2-02). Na direção contrária, o sono só é reservado sem extração esperando no Hub.
use super::{
    queue::{self, Context},
    sleep_checks::{self, Status},
    worker::Operation,
};
use crate::{
    LockExt,
    fork::{
        background_space_selection,
        memory_learning::{self, Mode},
    },
    hub::{BotStatus, Hub},
    store::{Entry, Store},
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::json;
use std::{collections::HashSet, sync::Arc};

/// O bot está parado se a última mensagem de qualquer conversa dele tem pelo menos isto.
pub(crate) const IDLE_MS: i64 = 30 * 60 * 1000;
/// Mensagens provadas (checagem gravada) por rodada; o resto fica para a próxima (o cursor só passa do provado).
const MESSAGES: usize = 500;
/// Teto de mensagens lidas por rodada (R2-D1): as seguradas não gastam a cota acima, mas o lote tem um teto.
const SCAN: i64 = 5_000;
/// Lembranças conferidas pelo atalho literal por rodada (seção 1 do desenho).
const CANDIDATES: i64 = 20;
/// Falhas técnicas seguidas antes de a rodada do dia desistir (nada gravado em nenhuma delas).
const ATTEMPTS: i64 = 3;

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_memory_sleep_jobs(
          id TEXT PRIMARY KEY,bot TEXT NOT NULL,chat TEXT NOT NULL,space TEXT NOT NULL,day TEXT NOT NULL,
          context TEXT NOT NULL,source TEXT NOT NULL,status TEXT NOT NULL DEFAULT 'waiting',reason TEXT,
          created_at INTEGER NOT NULL,reserved_at INTEGER,reservation TEXT,attempts INTEGER NOT NULL DEFAULT 0,
          summary TEXT,UNIQUE(bot,day));
         CREATE INDEX IF NOT EXISTS donna_memory_sleep_jobs_status ON donna_memory_sleep_jobs(status,bot);
         CREATE TABLE IF NOT EXISTS donna_memory_sleep_facts(
          entry INTEGER NOT NULL,rev INTEGER NOT NULL,run TEXT NOT NULL,verdict TEXT NOT NULL,reason TEXT NOT NULL,
          PRIMARY KEY(entry,rev,run)) WITHOUT ROWID;
         CREATE TABLE IF NOT EXISTS donna_memory_sleep_kept(
          entry INTEGER NOT NULL,rev INTEGER NOT NULL,run TEXT NOT NULL,sources TEXT NOT NULL,
          PRIMARY KEY(entry,rev,run)) WITHOUT ROWID;",
    )?;
    // 2b1d-1: recibo de perdas da rodada do modelo (só ids, revisões e motivos do enum).
    let has_receipt: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('donna_memory_sleep_jobs') WHERE name='receipt')",
        [],
        |r| r.get(0),
    )?;
    if !has_receipt {
        db.execute("ALTER TABLE donna_memory_sleep_jobs ADD COLUMN receipt TEXT", [])?;
    }
    sleep_checks::migrate(db)
}

/// Gancho do dreamer, quando o bot não tem extração nem revisão pendentes. Nunca derruba o dreamer: a falha da
/// rodada não grava nada e fica para a próxima checagem.
pub(crate) async fn tick(hub: &Arc<Hub>, bot: &str) {
    if let Err(error) = try_tick_with_model(hub, bot, crate::store::now_ms()).await {
        tracing::warn!(bot = %bot, error = format!("{error:#}"), "sono da memória adiado");
    }
}

/// A rodada sem o modelo (checagens e atalho literal): o mesmo caminho, para os testes do Hub.
#[cfg(test)]
pub(crate) fn try_tick(hub: &Hub, bot: &str, now: i64) -> Result<()> {
    if let Some(r) = begin(hub, bot, now)? {
        run(hub, &r, now, None)?;
    }
    Ok(())
}

/// A rodada inteira (2b1d-1): primeiro a pergunta ao modelo (sem gravar nada), depois uma Tx só com as checagens,
/// o atalho literal, o resultado do modelo, o recibo e o cursor. Extração chegando, sem vaga ou limite da
/// assinatura devolvem a reserva sem gravar nada.
async fn try_tick_with_model(hub: &Arc<Hub>, bot: &str, now: i64) -> Result<()> {
    let Some(r) = begin(hub, bot, now)? else { return Ok(()) };
    if hub.routines.stopping() {
        return release(&hub.store, &r);
    }
    let outcome = super::sleep_model::ask(hub, &r).await?;
    if matches!(outcome, Some(super::sleep_model::Outcome::Deferred)) {
        return release(&hub.store, &r);
    }
    let applied = run(hub, &r, now, outcome.as_ref());
    // O histórico da chamada só fecha como sucesso depois do commit que gravou o resultado (SEG-543-03).
    if let Some(super::sleep_model::Outcome::Answered(mut reply)) = outcome {
        match applied {
            Ok(true) => reply.response.finish(crate::fork::background_history::Status::Unchanged),
            _ => reply.response.fail_revalidation(),
        }
    }
    applied.map(|_| ())
}

fn begin(hub: &Hub, bot: &str, now: i64) -> Result<Option<Reservation>> {
    // MX-280: depois do sinal de parada a rodada não reserva nem grava; o dia
    // não é consumido e a próxima abertura tenta de novo.
    if hub.routines.stopping() {
        return Ok(None);
    }
    // D389 `off`: não agenda, não reserva e não roda (e a rodada do modelo também não consulta cota).
    if memory_learning::mode(&hub.store)? == Mode::Off || !parked(hub, bot)? {
        return Ok(None);
    }
    schedule(&hub.store, bot, now, &day(now))?;
    reserve(&hub.store, bot, now)
}

/// Devolve a reserva sem contar tentativa e sem gravar nada (R7: a extração tem prioridade).
fn release(store: &Store, r: &Reservation) -> Result<()> {
    store.db.locked().execute(
        "UPDATE donna_memory_sleep_jobs SET status='waiting',reservation=NULL,reason='deferred',attempts=attempts-1
         WHERE id=?1 AND reservation=?2 AND status='reserved'",
        params![r.id, r.token],
    )?;
    Ok(())
}

/// O dia de calendário local de `now` (como a revisão diária): uma rodada por bot por dia, não janela de 24 h.
fn day(now: i64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_millis_opt(now).single().unwrap_or_else(chrono::Local::now).format("%Y-%m-%d").to_string()
}

/// Nenhum turno vivo: o `active_job` confirma só o trabalho `running` do turno atual; a fila é olhada no `schedule`.
fn parked(hub: &Hub, bot: &str) -> Result<bool> {
    Ok(hub.runtime(bot).status == BotStatus::Idle && queue::active_job(hub, bot)?.is_none())
}

/// Agenda a rodada do dia, se o bot está parado e há conversa com mensagem por provar. Nada a fazer não consome
/// o dia: a próxima checagem tenta de novo.
pub(crate) fn schedule(store: &Store, bot: &str, now: i64, day: &str) -> Result<bool> {
    let _boundary = store.memory_boundary.locked();
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let busy: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_memory_sleep_jobs WHERE bot=?1 AND (day=?2 OR status IN ('waiting','reserved')))
         OR EXISTS(SELECT 1 FROM donna_memory_auto_jobs WHERE bot=?1
            AND (status IN ('waiting','reserved','owner') OR outcome='running'))
         OR COALESCE((SELECT MAX(created_at) FROM entries WHERE bot_id=?1
            OR bot_id IN (SELECT id FROM donna_space_conversations WHERE bot_id=?1)),0)>?3",
        params![bot, day, now - IDLE_MS],
        |r| r.get(0),
    )?;
    if busy {
        return Ok(false);
    }
    // A conversa com o cursor mais antigo primeiro, para nenhuma ficar para trás.
    let chats = tx
        .prepare(
            "SELECT DISTINCT j.chat FROM donna_memory_auto_jobs j
             LEFT JOIN donna_memory_sleep_cursor c ON c.bot=j.bot AND c.chat=j.chat
             WHERE j.bot=?1 AND j.outcome='success' AND j.scope='allowed'
             AND ((j.status='applied' AND COALESCE(j.proof,'')<>'unproven') OR (j.status='noChange' AND j.proof='learningOn'))
             ORDER BY COALESCE(c.updated_at,0),j.chat",
        )?
        .query_map([bot], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for chat in chats {
        if pending(&tx, bot, &chat)?.is_empty() {
            continue;
        }
        let row: Option<(String, String, String)> = tx
            .query_row(
                "SELECT id,context,space FROM donna_memory_auto_jobs WHERE bot=?1 AND chat=?2 AND outcome='success'
                 AND scope='allowed' AND ((status='applied' AND COALESCE(proof,'')<>'unproven') OR (status='noChange' AND proof='learningOn'))
                 ORDER BY admitted_at DESC,admitted_order DESC,rowid DESC LIMIT 1",
                params![bot, chat],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((source, raw, space)) = row else { continue };
        let context: Context = serde_json::from_str(&raw)?;
        if super::worker::validate_context(&tx, &context).is_err() {
            continue;
        }
        tx.execute(
            "INSERT OR IGNORE INTO donna_memory_sleep_jobs(id,bot,chat,space,day,context,source,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![queue::hash(&format!("sleep:{bot}:{day}")), bot, chat, space, day, raw, source, now],
        )?;
        tx.commit()?;
        return Ok(true);
    }
    Ok(false)
}

/// Há extração reservável esperando no Hub (as vagas do executor são globais)? Não conta a já tentada e devolvida
/// por falta de seleção. O mesmo predicado na reserva do sono e antes do envio ao modelo (R7).
const EXTRACTION_WAITING: &str = "EXISTS(SELECT 1 FROM donna_memory_auto_jobs e
    WHERE e.status='waiting' AND e.outcome='success' AND e.scope='allowed'
    AND NOT (e.attempts>0 AND COALESCE(e.reason,'')='noSelection'))";

pub(crate) fn extraction_waiting(db: &Connection) -> Result<bool> {
    Ok(db.query_row(&format!("SELECT {EXTRACTION_WAITING}"), [], |r| r.get(0))?)
}

#[derive(Clone, Debug)]
pub(crate) struct Reservation {
    pub id: String,
    pub token: String,
    pub context: Context,
}

/// Reserva do sono (R7/R2-02). Um só sono reservado por bot; e só sem nenhuma extração esperando no Hub inteiro,
/// porque as vagas do executor são globais. A reserva da extração (`queue::reserve`) não muda. Não conta a extração
/// que já foi tentada e voltou por falta de seleção (`attempts>0` e `noSelection`): parada esperando a seleção de um
/// Space, ela seguraria o sono de todos os bots por até 30 dias. A recém-chegada tem o mesmo motivo e conta.
pub(crate) fn reserve(store: &Store, bot: &str, now: i64) -> Result<Option<Reservation>> {
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Reserva abandonada (queda no meio da rodada) volta a esperar; a rodada não gravou nada.
    tx.execute(
        "UPDATE donna_memory_sleep_jobs SET status='waiting',reservation=NULL,reason='transient'
         WHERE status='reserved' AND reserved_at<=?1",
        [now - queue::RESERVATION_MS],
    )?;
    let row: Option<(String, String)> = tx
        .query_row(
            &format!(
                "SELECT s.id,s.context FROM donna_memory_sleep_jobs s WHERE s.bot=?1 AND s.status='waiting'
                 AND NOT ({EXTRACTION_WAITING})
                 AND NOT EXISTS(SELECT 1 FROM donna_memory_sleep_jobs o WHERE o.bot=s.bot AND o.status='reserved')
                 ORDER BY s.created_at,s.id LIMIT 1"
            ),
            [bot],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((id, raw)) = row else { return Ok(None) };
    let token = uuid::Uuid::new_v4().to_string();
    tx.execute(
        "UPDATE donna_memory_sleep_jobs SET status='reserved',reserved_at=?2,reservation=?3,attempts=attempts+1
         WHERE id=?1",
        params![id, now, token],
    )?;
    tx.commit()?;
    Ok(Some(Reservation { id, token, context: serde_json::from_str(&raw)? }))
}

/// A rodada inteira numa transação: checagens, atalho literal, cursor e o fim do trabalho. Com o Space autorizado
/// para o fundo, roda sob MB → Owner/Cloud → DB (a mesma composição da extração, `with_memory_current`) e o
/// `authorize` original é conferido antes do commit; sem autorização, roda sob MB → DB e não aplica nem sugere o
/// atalho. Interrompida, não deixa nada; a reserva volta a esperar. Publica só depois de soltar as guardas.
/// `model`: o que a rodada do modelo deu (2b1d-1), gravado nesta mesma Tx e só sob a guarda do Space.
/// Devolve se o resultado do modelo foi gravado. Com resposta do modelo, a autoridade é a ORIGINAL da chamada (o
/// `Prepared` e a seleção do executor, reconferidos), nunca uma seleção resolvida de novo (SEG-543-03).
pub(crate) fn run(hub: &Hub, r: &Reservation, now: i64, model: Option<&super::sleep_model::Outcome>) -> Result<bool> {
    let store = &hub.store;
    let fresh;
    let prepared = if let Some(super::sleep_model::Outcome::Answered(reply)) = model {
        super::sleep_model::original(store, reply, &r.context).map(|()| &reply.prepared)
    } else {
        fresh = background_space_selection::resolve(hub, &r.context);
        fresh.as_ref().map_err(|error| anyhow::anyhow!("{error:#}"))
    };
    let result = match prepared {
        Ok(prepared) => {
            crate::fork::background_memory_auto::with_memory_current(hub, prepared, |section, authorize| {
                let mut db = section.hub().store.db.locked();
                let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                authorize(&tx, &r.context)?;
                let out = round(&tx, r, now, true, model)?;
                authorize(&tx, &r.context)?;
                tx.commit()?;
                Ok(out)
            })
        }
        Err(error) if super::worker::is_transient(&error) => Err(error),
        Err(_) => {
            let _boundary = store.memory_boundary.locked();
            let mut db = store.db.locked();
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            round(&tx, r, now, false, None).and_then(|out| Ok(tx.commit().map(|()| out)?))
        }
    };
    match result {
        Ok(Round { ids, shared, parked, model }) => {
            if !ids.is_empty() || parked {
                super::apply::Committed::sleep(ids, shared, parked, r.context.clone()).publish(hub);
            }
            Ok(model)
        }
        Err(error) => {
            let db = store.db.locked();
            let terminal = !super::worker::is_transient(&error);
            db.execute(
                "UPDATE donna_memory_sleep_jobs SET reservation=NULL,
                 status=CASE WHEN ?3 OR attempts>=?4 THEN 'failed' ELSE 'waiting' END,
                 reason=CASE WHEN ?3 THEN 'context' ELSE 'transient' END
                 WHERE id=?1 AND reservation=?2 AND status='reserved'",
                params![r.id, r.token, terminal, ATTEMPTS],
            )?;
            Err(error)
        }
    }
}

/// O que a rodada aplicou ou guardou, para publicar depois das guardas.
#[derive(Default)]
struct Round {
    /// Cartões do atalho aplicado (`auto`).
    ids: Vec<String>,
    /// Algum deles em "Sobre você".
    shared: bool,
    /// Guardou sugestão (`suggest`).
    parked: bool,
    /// Gravou o resultado do modelo.
    model: bool,
}

fn round(
    tx: &Connection,
    r: &Reservation,
    now: i64,
    authorized: bool,
    model: Option<&super::sleep_model::Outcome>,
) -> Result<Round> {
    let c = &r.context;
    let held: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_memory_sleep_jobs WHERE id=?1 AND reservation=?2 AND status='reserved')",
        params![r.id, r.token],
        |row| row.get(0),
    )?;
    ensure!(held, super::worker::InvalidSnapshot::Job);
    if memory_learning::mode_in(tx)? == Mode::Off {
        // Desligado no meio do caminho: devolve sem contar tentativa e sem gravar nada.
        tx.execute(
            "UPDATE donna_memory_sleep_jobs SET status='waiting',reservation=NULL,reason='learningOff',
             attempts=attempts-1 WHERE id=?1",
            [&r.id],
        )?;
        return Ok(Round::default());
    }
    // O mesmo `Context` de um trabalho aplicado, revalidado agora: bot vivo na mesma revisão, nunca grupo, conversa
    // aberta do mesmo Space e a mesma memória. Rotinas, webhooks, canais e bot→bot não têm trabalho de memória (D246)
    // e, mesmo que ganhem, não passam daqui: só a conversa principal ou uma conversa de Space.
    super::worker::validate_context(tx, c)?;
    let conversation: bool = tx.query_row(
        "SELECT ?1=?2 OR EXISTS(SELECT 1 FROM donna_space_conversations WHERE id=?1 AND bot_id=?2)",
        params![c.chat_id, c.bot_id],
        |row| row.get(0),
    )?;
    ensure!(conversation, super::worker::InvalidSnapshot::Context);
    let mut out = if authorized { literal(tx, c, &r.id, now)? } else { Round::default() };
    // 2b1d-1: o resultado do modelo, reconferido nesta Tx; os `undecided` válidos deixam de segurar as mensagens.
    if let Some(model) = model.filter(|_| authorized) {
        out.model = super::sleep_model::apply(tx, c, &r.id, model)?;
    }
    let doubtful = doubtful(tx, c)?;
    let pending = pending(tx, &c.bot_id, &c.chat_id)?;
    let ids = (c.bot_id.as_str(), c.chat_id.as_str(), r.id.as_str());
    let (mut covered, mut excluded, mut kept) = (0, 0, 0);
    // R2-D1: as mensagens seguradas (lembrança duvidosa, trabalho não terminado) não gastam a cota do lote; só as
    // provadas. Assim uma conversa com muitas seguradas antigas não deixa as novas sem checagem para sempre.
    let mut read = 0;
    for m in &pending {
        if covered + excluded >= MESSAGES {
            break;
        }
        read += 1;
        if passed(tx, c, m, "allowed")? {
            if backs_doubtful(tx, &m.id, &doubtful)? {
                kept += 1;
            } else {
                sleep_checks::record(tx, ids, (&m.id, m.rev), Status::Covered, "applied")?;
                covered += 1;
            }
        } else if passed(tx, c, m, "blocked")? {
            // Só `memory_blocked` (o dono decidiu): a C7 não apaga sozinha, vai para a lista à parte da prévia.
            sleep_checks::record(tx, ids, (&m.id, m.rev), Status::Excluded, "memoryBlocked")?;
            excluded += 1;
        } else {
            kept += 1;
        }
    }
    let messages = &pending[..read];
    let human: Vec<(i64, String)> = messages.iter().map(|m| (m.seq, m.id.clone())).collect();
    let cursor = sleep_checks::advance_cursor(tx, ids, &human, now)?;
    // Prévia da limpeza C7, pelo mesmo predicado que o DELETE dela vai chamar: quantas destas a C7 poderia apagar.
    let mut cleanable = 0;
    for m in messages.iter().filter(|m| m.seq <= cursor) {
        cleanable += i64::from(sleep_checks::valid_for_cleanup(tx, &c.bot_id, &c.chat_id, &m.id)?);
    }
    // E as entradas que não são do dono nesses turnos (respostas, cartões), pelo predicado de turno da C7.
    let turns: HashSet<i64> = messages.iter().filter(|m| m.seq <= cursor).map(|m| m.turn).collect();
    let mut replies = 0;
    for turn in turns {
        let others = tx
            .prepare("SELECT id FROM entries WHERE bot_id=?1 AND turn=?2 AND kind<>'user'")?
            .query_map(params![c.chat_id, turn], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for id in others {
            replies += i64::from(sleep_checks::turn_valid_for_cleanup(tx, &id)?);
        }
    }
    let summary = json!({"covered":covered,"excluded":excluded,"kept":kept,"doubtful":doubtful.len(),
        "cursor":cursor,"cleanable":cleanable,"cleanableReplies":replies,"confirmed":out.ids.len(),"suggested":out.parked});
    tx.execute(
        "UPDATE donna_memory_sleep_jobs SET status='done',reason=NULL,reservation=NULL,summary=?2 WHERE id=?1",
        params![r.id, summary.to_string()],
    )?;
    Ok(out)
}

/// O atalho literal (seção 3), sem modelo: só inferência sem conflito, com a linhagem inteira comprovada nesta
/// conversa, cujo texto o Hub acha numa mensagem limpa do dono (`origin::confirms`). `auto` aplica (recibo e
/// Desfazer, preso ao trabalho de origem); `suggest` guarda a sugestão e não toca `memory_entries`. Cada candidata
/// que não passou fica marcada "conferida pelo atalho nesta revisão" (`donna_memory_sleep_facts`, só ids e rev;
/// R1-03): não ocupa as 20 vagas de novo até mudar, e a seleção avança. A marca não tira a lembrança das duvidosas.
/// A marca vale só para a conversa conferida (R2-507-01): a rodada é ligada ao bot e à conversa pelo trabalho do
/// sono (`run`), então uma lembrança de "Sobre você" recusada pela linhagem de B continua candidata em A.
fn literal(tx: &Connection, c: &Context, run: &str, now: i64) -> Result<Round> {
    let mode = memory_learning::mode_in(tx)?;
    let rows = tx
        .prepare(
            "SELECT id,rev,text,evidence FROM memory_entries m WHERE state='active' AND author<>'user'
             AND nature='inferred' AND conflict_text IS NULL AND (bot=?1 OR bot IS NULL)
             AND NOT EXISTS(SELECT 1 FROM donna_memory_sleep_facts f JOIN donna_memory_sleep_jobs j ON j.id=f.run
                WHERE f.entry=m.id AND f.rev=m.rev AND f.verdict='literal' AND j.bot=?3 AND j.chat=?4)
             ORDER BY id LIMIT ?2",
        )?
        .query_map(params![c.memory_id, CANDIDATES, c.bot_id, c.chat_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Round::default();
    let skip = |entry: i64, rev: i64, reason: &str| -> Result<()> {
        tx.execute(
            "INSERT OR IGNORE INTO donna_memory_sleep_facts(entry,rev,run,verdict,reason) VALUES(?1,?2,?3,'literal',?4)",
            params![entry, rev, run, reason],
        )?;
        Ok(())
    };
    for (entry, rev, text, evidence) in rows {
        let Some((job, source)) = lineage(tx, c, entry)? else {
            skip(entry, rev, "lineage")?;
            continue;
        };
        let evidence: Vec<String> = evidence.split(',').filter(|id| !id.is_empty()).map(str::to_owned).collect();
        if evidence.is_empty() || !super::origin::confirms(tx, &source, &evidence, &text)? {
            skip(entry, rev, "notLiteral")?;
            continue;
        }
        let op = Operation::Confirm { entry, expected_rev: rev, evidence };
        match mode {
            Mode::Auto => match super::apply::confirm_in(tx, (&job, &source), &op)? {
                Some((id, shared)) => {
                    out.ids.push(id);
                    out.shared |= shared;
                }
                None => skip(entry, rev, "blocked")?,
            },
            Mode::Suggest => {
                out.parked |= super::suggestions::park(tx, &job, &source, &op, now)?;
                skip(entry, rev, "suggested")?;
            }
            Mode::Off => {}
        }
    }
    Ok(out)
}

/// A linhagem inteira (seção 2, R2; D409): toda mensagem que já sustentou o fato existe e é desta conversa; cada
/// versão do fato tem recibo de um trabalho do mesmo bot, conversa, Space e memória, e cada evidência dessa versão
/// está no `Context` desse trabalho na revisão ATUAL da mensagem (editada depois = fora). Versão sem recibo (legado,
/// edição do dono, fixar, Desfazer) ou fonte de outra conversa exclui o candidato inteiro, sem ler texto. Devolve o
/// trabalho e o `Context` da versão atual.
pub(super) fn lineage(tx: &Connection, c: &Context, entry: i64) -> Result<Option<(String, Context)>> {
    let foreign: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_memory_lineage l WHERE l.kind='fact' AND l.item=CAST(?1 AS TEXT)
         AND NOT EXISTS(SELECT 1 FROM entries e WHERE e.id=l.message AND e.bot_id=?2))",
        params![entry, c.chat_id],
        |r| r.get(0),
    )?;
    if foreign {
        return Ok(None);
    }
    let versions = tx
        .prepare(
            "SELECT rev,evidence FROM donna_memory_versions WHERE entry=?1
             UNION SELECT rev,evidence FROM memory_entries WHERE id=?1 ORDER BY 1",
        )?
        .query_map([entry], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut source = None;
    for (rev, evidence) in versions {
        let row: Option<(String, String)> = tx
            .query_row(
                "SELECT r.job,j.context FROM donna_memory_auto_receipts r JOIN donna_memory_auto_jobs j ON j.id=r.job
                 WHERE r.entry=?1 AND r.rev=?2 AND j.bot=?3 AND j.chat=?4 AND j.space=?5
                 AND json_extract(j.context,'$.memoryId')=?6 ORDER BY r.rowid LIMIT 1",
                params![entry, rev, c.bot_id, c.chat_id, c.space_id, c.memory_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((job, raw)) = row else { return Ok(None) };
        let context: Context = serde_json::from_str(&raw)?;
        for id in evidence.split(',').filter(|id| !id.is_empty()) {
            let current: Option<i64> = tx
                .query_row(
                    "SELECT rev FROM entries WHERE id=?1 AND bot_id=?2 AND kind='user'",
                    params![id, c.chat_id],
                    |r| r.get(0),
                )
                .optional()?;
            if !current.is_some_and(|rev| context.messages.iter().any(|m| m.id == id && m.revision == rev)) {
                return Ok(None);
            }
        }
        source = Some((job, context));
    }
    Ok(source)
}

/// Mensagens humanas da conversa por provar, em ordem de `seq`: as depois do cursor e as de antes sem checagem no
/// `rev` atual (editadas depois da checagem: revistas primeiro, sem o cursor voltar) ou cuja checagem mais nova
/// perdeu a validade porque um fato da linhagem mudou depois dela (R1-02). Trabalho na fila e sugestão pendente
/// só seguram enquanto existem; quando viram fato, a mudança do fato cai nesta mesma regra.
/// A classificação de autoria vem antes do LIMIT (R1-03): respostas de cartão genUI e mensagens de bot ou rotina
/// não ocupam o lote. É a régua de `Entry::is_user_authored`, que ainda filtra de novo depois.
fn pending(db: &Connection, bot: &str, chat: &str) -> Result<Vec<Entry>> {
    let cursor = sleep_checks::cursor(db, bot, chat)?;
    let rows = db
        .prepare(&format!(
            "SELECT {} FROM entries e WHERE e.bot_id=?1 AND e.kind='user' AND json_valid(e.data)
             AND json_extract(e.data,'$.from') IS NULL AND json_extract(e.data,'$.author') IS NULL
             AND json_extract(e.data,'$.sourceBotId') IS NULL AND json_extract(e.data,'$.botMessage') IS NULL
             AND json_extract(e.data,'$.runId') IS NULL AND json_type(e.data,'$.choice_response') IS NULL
             AND (e.seq>?2 OR NOT EXISTS(SELECT 1 FROM donna_memory_sleep_checks k
                WHERE k.bot=?3 AND k.chat=?1 AND k.message=e.id AND k.rev=e.rev)
              OR EXISTS(SELECT 1 FROM donna_memory_lineage l JOIN memory_entries m ON CAST(m.id AS TEXT)=l.item
                WHERE l.kind='fact' AND l.message=e.id AND m.rev>(SELECT MAX(k.checked_at_rev)
                  FROM donna_memory_sleep_checks k WHERE k.message=e.id AND k.rev=e.rev)))
             ORDER BY e.seq LIMIT ?4",
            crate::store::ENTRY_COLS
        ))?
        .query_map(params![chat, cursor, bot, SCAN], crate::store::row_entry)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows.into_iter().filter(Entry::is_user_authored).collect())
}

/// A mensagem, na revisão exata, está no `Context` de um trabalho deste bot e chat que terminou (`applied`, ou
/// `noChange` com a prova `learningOn` gravada no commit: o legado sem prova e o `off` no commit não contam; nenhum
/// job marcado `unproven`, nem `applied` de lote misto, SEG-507-02; escopo permitido) ou que o dono bloqueou (`memory_blocked`; pergunta D406 não conta).
fn passed(db: &Connection, c: &Context, m: &Entry, scope: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_memory_auto_jobs j,json_each(j.context,'$.messages') r
         WHERE j.bot=?1 AND j.chat=?2 AND j.outcome='success' AND j.scope=?5
         AND (?5='blocked' OR (j.status='applied' AND COALESCE(j.proof,'')<>'unproven')
            OR (j.status='noChange' AND j.proof='learningOn'))
         AND (?5='allowed' OR json_extract(j.context,'$.ownerQuestion') IS NULL)
         AND json_extract(r.value,'$.id')=?3 AND json_extract(r.value,'$.revision')=?4)",
        params![c.bot_id, c.chat_id, m.id, m.rev, scope],
        |row| row.get(0),
    )?)
}

/// Lembranças que o sono vai reler (seção 2): ativas, não escritas pelo dono, em conflito, inferidas, com data ou
/// prazo, ou fixadas, na camada desta memória ou em "Sobre você". Aqui só seguram as mensagens que as sustentam.
/// A conferida pelo modelo e mantida (`undecided` válido, 2b1d-1) nesta revisão e nesta conversa deixa de segurar.
fn doubtful(db: &Connection, c: &Context) -> Result<HashSet<String>> {
    let rows = db
        .prepare(&format!(
            "SELECT id,text,nature,pinned FROM memory_entries m WHERE state='active' AND author<>'user'
             AND (bot=?1 OR bot IS NULL) AND NOT {}",
            super::sleep_model::kept_sql("m.id", "m.rev", "?2", "?3")
        ))?
        .query_map(params![c.memory_id, c.bot_id, c.chat_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, bool>(3)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter(|(_, text, nature, pinned)| doubtful_fact(nature, *pinned, text))
        .map(|(id, ..)| id.to_string())
        .collect())
}

/// A lembrança é duvidosa: conflito, inferência, fixada ou com data/prazo.
pub(super) fn doubtful_fact(nature: &str, pinned: bool, text: &str) -> bool {
    matches!(nature, "conflict" | "inferred") || pinned || dated(text)
}

/// Data ou prazo no texto ("dia 12", "até sexta", "amanhã"). Larga de propósito: segurar a mais é o lado seguro.
fn dated(text: &str) -> bool {
    const WORDS: &[&str] = &[
        "hoje", "amanhã", "ontem", "semana", "mês", "prazo", "até ", "segunda", "terça", "quarta", "quinta", "sexta",
        "sábado", "domingo",
    ];
    let text = text.to_lowercase();
    text.chars().any(|c| c.is_ascii_digit()) || WORDS.iter().any(|w| text.contains(w))
}

fn backs_doubtful(db: &Connection, message: &str, doubtful: &HashSet<String>) -> Result<bool> {
    if doubtful.is_empty() {
        return Ok(false);
    }
    let mut stmt = db.prepare(
        "SELECT item FROM donna_memory_lineage WHERE kind='fact' AND message=?1
         UNION SELECT CAST(id AS TEXT) FROM memory_entries WHERE ','||evidence||',' LIKE '%,'||?1||',%'",
    )?;
    let facts = stmt.query_map([message], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(facts.iter().any(|f| doubtful.contains(f)))
}
