//! Recibo de recusa do scanner de injeção nos portões de escrita da memória pessoal.
//!
//! O recibo nunca guarda texto nem hash do texto recusado: só identificadores, a revisão exata
//! conferida, os ids das regras (`Refusal::id`), a versão do scanner e o horário. O candidato fica
//! serializado em `pendentes` para a liberação manual do dono (mesmo critério de origem do
//! `write_gate`), que gera um recibo separado em `liberacoes` e nunca contorna segredo.
use super::{
    audit::{HUB_CAP, SCOPE_CAP},
    queue::{Context, LIFE_MS},
    worker::Operation,
};
use crate::{store::BotConfig, store::now_ms};
use anyhow::{Result, bail, ensure};
use codync_mem::{check, store as mem};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

/// O evento interno de cada recusa, num lugar só para os dois portões.
pub(crate) const RECUSADA: &str = "memoria.promocao.recusada";
/// O evento da liberação manual do dono, separado da recusa.
pub(crate) const LIBERADA: &str = "memoria.promocao.liberada";
/// O aviso visível ao dono, sem texto nem hash do material recusado.
pub(crate) const AVISO: &str = "nota não salva: parece instrução vinda de conteúdo externo";

/// Qual portão recusou a escrita.
#[derive(Clone, Copy)]
pub(crate) enum Gate {
    /// A promoção automática do `memory_auto`.
    Promocao,
    /// A escrita direta do bot pelas ferramentas de memória.
    Ferramenta,
}
impl Gate {
    fn name(self) -> &'static str {
        match self {
            Self::Promocao => "promocao",
            Self::Ferramenta => "ferramenta",
        }
    }
}

pub(super) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_memory_injecao_recusas(
            id TEXT PRIMARY KEY,
            gate TEXT NOT NULL CHECK(gate IN('promocao','ferramenta')),
            job TEXT,
            bot TEXT NOT NULL,
            chat TEXT NOT NULL,
            space TEXT NOT NULL,
            candidate TEXT NOT NULL,
            entry INTEGER,
            note TEXT,
            revision INTEGER,
            rules TEXT NOT NULL,
            scanner INTEGER NOT NULL,
            at INTEGER NOT NULL,
            released_at INTEGER,
            shared INTEGER NOT NULL DEFAULT 0);
        CREATE INDEX IF NOT EXISTS donna_memory_injecao_recusas_at ON donna_memory_injecao_recusas(at);
        CREATE TABLE IF NOT EXISTS donna_memory_injecao_pendentes(
            recusa TEXT PRIMARY KEY REFERENCES donna_memory_injecao_recusas(id),
            operation TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS donna_memory_injecao_liberacoes(
            id TEXT PRIMARY KEY,
            recusa TEXT NOT NULL REFERENCES donna_memory_injecao_recusas(id),
            candidate TEXT NOT NULL,
            revision INTEGER,
            rules TEXT NOT NULL,
            scanner INTEGER NOT NULL,
            space TEXT NOT NULL,
            bot TEXT NOT NULL,
            at INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS donna_memory_injecao_tumbas(
            id TEXT PRIMARY KEY,
            bot TEXT NOT NULL,
            space TEXT NOT NULL DEFAULT '',
            shared INTEGER NOT NULL DEFAULT 0,
            at INTEGER NOT NULL);",
    )?;
    // Bancos já migrados ganham as colunas novas sem recriar a tabela. Criar a coluna e
    // preenchê-la é uma unidade só (SAVEPOINT): uma falha entre os dois passos desfaz a coluna e a
    // próxima abertura refaz tudo, sem deixar registro antigo com o padrão `shared=0`.
    for (table, column, decl) in [
        ("donna_memory_injecao_recusas", "shared", "INTEGER NOT NULL DEFAULT 0"),
        ("donna_memory_injecao_tumbas", "space", "TEXT NOT NULL DEFAULT ''"),
        ("donna_memory_injecao_tumbas", "shared", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        let exists: bool = db.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name='{column}')"),
            [],
            |r| r.get(0),
        )?;
        if exists {
            continue;
        }
        db.execute_batch("SAVEPOINT injecao_coluna")?;
        let step = (|| -> Result<()> {
            db.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"))?;
            // Nos testes, simula a interrupção entre criar a coluna e preenchê-la.
            #[cfg(test)]
            tests_hook::run(table, column)?;
            if column == "shared" {
                backfill_shared(db, table)?;
            }
            Ok(())
        })();
        match step {
            Ok(()) => db.execute_batch("RELEASE injecao_coluna")?,
            Err(e) => {
                db.execute_batch("ROLLBACK TO injecao_coluna; RELEASE injecao_coluna")?;
                return Err(e);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) mod tests_hook {
    use anyhow::{Result, bail};
    use std::cell::Cell;
    thread_local!(pub(in crate::fork::memory_auto) static FAIL_ON: Cell<Option<&'static str>> = const { Cell::new(None) });
    pub(super) fn run(table: &str, column: &str) -> Result<()> {
        if FAIL_ON.with(Cell::get).is_some_and(|t| table == t && column == "shared") {
            bail!("falha simulada entre ALTER e preenchimento");
        }
        Ok(())
    }
}

/// Banco no formato antigo: a marca `shared` nasce 0 em tudo, mas o escopo original se perdeu.
/// Deriva-se dos dados que sobraram (pendência com a operação, ou entrada existente); o que não
/// puder ser provado pessoal vira compartilhado — fail-closed: o `purge_shared` apaga a mais, nunca
/// a menos. Tumbas antigas não guardam nada que as identifique, então todas viram compartilhadas.
fn backfill_shared(db: &Connection, table: &str) -> Result<()> {
    if table == "donna_memory_injecao_tumbas" {
        db.execute("UPDATE donna_memory_injecao_tumbas SET shared=1", [])?;
    } else {
        // Só prova pessoal: (a) a ENTRADA da recusa existe e é pessoal — e a pendência, se houver,
        // não diz o contrário —, ou (b) recusa sem entrada (escrita nova) cuja pendência traz
        // `shared` explícito. Pendência sem `shared` (Revise etc.) com entrada compartilhada ou
        // sumida, e recusa sem rastro nenhum, são compartilhadas.
        db.execute(
            "UPDATE donna_memory_injecao_recusas SET shared=CASE
               WHEN entry IS NOT NULL THEN
                 CASE WHEN EXISTS(SELECT 1 FROM memory_entries e WHERE e.id=donna_memory_injecao_recusas.entry AND e.bot IS NOT NULL)
                       AND COALESCE((SELECT json_extract(p.operation,'$.shared') FROM donna_memory_injecao_pendentes p
                                     WHERE p.recusa=donna_memory_injecao_recusas.id),0)=0
                      THEN 0 ELSE 1 END
               WHEN EXISTS(SELECT 1 FROM donna_memory_injecao_pendentes p WHERE p.recusa=donna_memory_injecao_recusas.id
                           AND json_extract(p.operation,'$.shared') IS NOT NULL)
                 THEN (SELECT json_extract(p.operation,'$.shared') FROM donna_memory_injecao_pendentes p
                       WHERE p.recusa=donna_memory_injecao_recusas.id)
               ELSE 1 END",
            [],
        )?;
    }
    Ok(())
}

/// A linha de recusa, sem texto nem hash do candidato.
#[derive(Clone)]
pub(crate) struct Recusa {
    pub id: String,
    pub bot: String,
    pub chat: String,
    pub space: String,
    pub candidate: String,
    pub entry: Option<i64>,
    pub note: Option<String>,
    pub revision: Option<i64>,
    pub rules: Vec<String>,
    pub scanner: u32,
    pub at: i64,
    pub released_at: Option<i64>,
}

pub(crate) fn recusa(db: &Connection, id: &str) -> Result<Option<Recusa>> {
    db.query_row(
        "SELECT gate,job,bot,chat,space,candidate,entry,note,revision,rules,scanner,at,released_at FROM donna_memory_injecao_recusas WHERE id=?1",
        [id],
        |r| {
            let rules: String = r.get(9)?;
            Ok((
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, Option<i64>>(8)?,
                rules,
                r.get::<_, u32>(10)?,
                r.get::<_, i64>(11)?,
                r.get::<_, Option<i64>>(12)?,
            ))
        },
    )
    .optional()?
    .map(|(bot, chat, space, candidate, entry, note, revision, rules, scanner, at, released_at)| {
        // Regras corrompidas fecham a liberação: a lista é quem decide o que é liberável.
        Ok(Recusa {
            id: id.into(), bot, chat, space, candidate, entry, note, revision,
            rules: serde_json::from_str(&rules)?, scanner, at, released_at,
        })
    })
    .transpose()
}

/// A operação pendente guardada na recusa, para a liberação manual.
pub(crate) fn pendente(db: &Connection, recusa: &str) -> Result<Option<Operation>> {
    let raw: Option<String> = db
        .query_row("SELECT operation FROM donna_memory_injecao_pendentes WHERE recusa=?1", [recusa], |r| r.get(0))
        .optional()?;
    raw.map(|raw| Ok(serde_json::from_str(&raw)?)).transpose()
}

/// A revisão atual do alvo da operação, lida na mesma transação da checagem e da escrita.
fn revision(db: &Connection, cfg: &BotConfig, op: &Operation) -> Result<Option<i64>> {
    Ok(match op {
        Operation::Revise { entry, .. } | Operation::Archive { entry, .. } => mem::get(db, *entry)?.map(|e| e.rev),
        Operation::Note { file, shared, .. } => {
            let layer = if *shared { None } else { Some(cfg.memory_id().to_owned()) };
            codync_mem::files::get(db, layer.as_deref(), file)?.map(|f| f.rev)
        }
        _ => None,
    })
}

fn candidate_of(op: &Operation) -> (String, Option<i64>, Option<String>) {
    match op {
        Operation::Remember { .. } => ("entry".into(), None, None),
        Operation::Revise { entry, .. }
        | Operation::Archive { entry, .. }
        | Operation::Confirm { entry, .. }
        | Operation::Reclassify { entry, .. } => ("entry".into(), Some(*entry), None),
        Operation::Note { .. } => ("note".into(), None, None),
        Operation::NotDurable { .. } => ("job".into(), None, None),
    }
}

/// O alvo da recusa, comum aos dois portões de escrita.
pub(crate) struct Alvo<'a> {
    pub job: Option<&'a str>,
    pub context: &'a Context,
    pub cfg: &'a BotConfig,
    pub op: &'a Operation,
}

/// O evento `memoria.promocao.recusada` com os mesmos campos do recibo.
fn event(gate: Gate, job: Option<&str>, c: &Context, r: &Recusa) -> Value {
    json!({
        "type": RECUSADA,
        "id": r.id,
        "gate": gate.name(),
        "job": job,
        "candidate": r.candidate,
        "entry": r.entry,
        "note": r.note,
        "revision": r.revision,
        "rules": r.rules,
        "scanner": r.scanner,
        "spaceId": c.space_id,
        "botId": c.bot_id,
        "at": r.at,
    })
}

/// O aviso visível ao dono: uma nota da conversa do bot, sem texto nem hash do candidato.
fn aviso(db: &Connection, c: &Context, turn: i64, recusa: &str, now: i64) -> Result<String> {
    let id = format!("memoria-injecao-{recusa}");
    let data = crate::fork::spaces::stamp_entry(
        db,
        &c.chat_id,
        &json!({"text":AVISO,"style":"info","author":c.bot_id,"memoryRefusal":{"id":recusa}}),
    )?;
    db.execute(
        "INSERT INTO entries(id,bot_id,rev,kind,turn,data,created_at,updated_at) VALUES(?1,?2,?3,'notice',?4,?5,?6,?6)",
        params![id, c.chat_id, crate::store::next_rev(db)?, turn, serde_json::to_string(&data)?, now],
    )?;
    Ok(id)
}

/// Grava, na mesma transação que abortou a escrita, o recibo da recusa, o candidato pendente e o
/// aviso ao dono. Devolve o evento a publicar depois do commit e o id do aviso.
pub(crate) fn refuse(
    db: &Connection,
    gate: Gate,
    alvo: &Alvo<'_>,
    refusal: check::Refusal,
    turn: i64,
    now: i64,
) -> Result<(Value, String)> {
    let c = alvo.context;
    // A expectativa ORIGINAL da operação vale antes de criar a pendência: um alvo que já mudou
    // fecha a recusa como mudança de revisão, como `apply_one` faria — sem pendência que
    // sobrescreveria a revisão nova na liberação.
    match alvo.op {
        Operation::Revise { entry, expected_rev, .. } | Operation::Archive { entry, expected_rev, .. } => {
            ensure!(mem::get(db, *entry)?.map(|e| e.rev) == Some(*expected_rev), "memory_revision_changed");
        }
        Operation::Note { file, shared, expected_hash, .. } => {
            let layer = if *shared { None } else { Some(alvo.cfg.memory_id().to_owned()) };
            let current = codync_mem::files::current(db, layer.as_deref(), file)?;
            ensure!(current.as_deref().map(codync_mem::files::hash) == *expected_hash, "memory_note_changed");
        }
        _ => {}
    }
    let id = uuid::Uuid::new_v4().to_string();
    let revision = revision(db, alvo.cfg, alvo.op)?;
    let (candidate, entry, note) = candidate_of(alvo.op);
    // O escopo fica no recibo (só a marca `shared`, sem texto nem hash): a `pendência` que a
    // identificava some na liberação e a entrada compartilhada pode ser apagada, mas o
    // `purge_shared` precisa continuar achando o recibo e a tumba.
    let shared = match alvo.op {
        Operation::Remember { shared, .. } | Operation::Note { shared, .. } => *shared,
        Operation::Revise { entry, .. }
        | Operation::Archive { entry, .. }
        | Operation::Confirm { entry, .. }
        | Operation::Reclassify { entry, .. } => db
            .query_row("SELECT bot IS NULL FROM memory_entries WHERE id=?1", [entry], |r| r.get(0))
            .optional()?
            .unwrap_or(false),
        Operation::NotDurable { .. } => false,
    };
    db.execute(
        "INSERT INTO donna_memory_injecao_recusas(id,gate,job,bot,chat,space,candidate,entry,note,revision,rules,scanner,at,shared)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        params![
            id,
            gate.name(),
            alvo.job,
            c.bot_id,
            c.chat_id,
            c.space_id,
            candidate,
            entry,
            note,
            revision,
            serde_json::to_string(&json!([refusal.id()]))?,
            check::VERSION,
            now,
            shared,
        ],
    )?;
    // Segredo nunca é persistido: a recusa sai com recibo e aviso, mas sem candidato no banco —
    // nada de texto para a prévia nem de liberação possível (`liberavel` recusa `secret`).
    if !matches!(refusal, check::Refusal::Secret) {
        db.execute(
            "INSERT INTO donna_memory_injecao_pendentes(recusa,operation) VALUES(?1,?2)",
            params![id, serde_json::to_string(alvo.op)?],
        )?;
    }
    // A poda roda DEPOIS da inserção, na ordem da auditoria `non_saved`: prazo, teto do escopo do
    // recibo novo e teto do Hub — assim o limite nunca fica excedido até a próxima poda.
    prune(db, now, Some((c.bot_id.as_str(), c.space_id.as_str())))?;
    let r = recusa(db, &id)?.expect("recibo recém-gravado");
    let notice = aviso(db, c, turn, &id, now)?;
    Ok((event(gate, alvo.job, c, &r), notice))
}

/// O recibo separado da liberação manual; segredo nunca chega aqui porque a recusa com `secret`
/// não é liberável e a escrita liberada continua passando pelas outras guardas (`Writer::Override`).
pub(crate) fn released(db: &Connection, r: &Recusa, now: i64) -> Result<Value> {
    ensure!(r.released_at.is_none(), "memory_refusal_already_released");
    let id = uuid::Uuid::new_v4().to_string();
    db.execute(
        "INSERT INTO donna_memory_injecao_liberacoes(id,recusa,candidate,revision,rules,scanner,space,bot,at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![id, r.id, r.candidate, r.revision, serde_json::to_string(&r.rules)?, r.scanner, r.space, r.bot, now,],
    )?;
    let changed = db.execute(
        "UPDATE donna_memory_injecao_recusas SET released_at=?1 WHERE id=?2 AND released_at IS NULL",
        params![now, r.id],
    )?;
    ensure!(changed == 1, "memory_refusal_already_released");
    db.execute("DELETE FROM donna_memory_injecao_pendentes WHERE recusa=?1", [&r.id])?;
    // Idem à recusa: a poda das liberações roda depois desta inserção, no escopo do recibo.
    prune(db, now, Some((r.bot.as_str(), r.space.as_str())))?;
    Ok(json!({
        "type": LIBERADA,
        "id": id,
        "refusal": r.id,
        "candidate": r.candidate,
        "entry": r.entry,
        "note": r.note,
        "revision": r.revision,
        "rules": r.rules,
        "scanner": r.scanner,
        "spaceId": r.space,
        "botId": r.bot,
        "at": now,
    }))
}

/// A recusa já teve recibo e saiu pelo prazo de vida: a consulta responde vencida, não desconhecida.
pub(crate) fn vencida(db: &Connection, id: &str) -> Result<bool> {
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM donna_memory_injecao_tumbas WHERE id=?1)", [id], |r| r.get(0))?)
}

/// Segredo nunca é liberável: a liberação manual só cobre instrução plantada, texto oculto e links.
pub(crate) fn liberavel(r: &Recusa) -> Result<()> {
    ensure!(r.space == crate::fork::spaces::PERSONAL, "memory_refusal_out_of_scope");
    ensure!(r.released_at.is_none(), "memory_refusal_already_released");
    // O candidato guardado expira: depois do prazo não há mais o que liberar.
    ensure!(r.at + LIFE_MS > now_ms(), "memory_refusal_expired");
    if r.rules.iter().any(|rule| rule == check::Refusal::Secret.id()) {
        bail!(check::Refusal::Secret);
    }
    Ok(())
}

/// Quantas recusas um job já acumulou: o limite do portão de ferramentas conta também as recusadas,
/// para um bot em laço não encher a conversa de avisos nem o banco de pendências.
pub(crate) fn recusas_do_job(db: &Connection, job: &str) -> Result<i64> {
    Ok(db.query_row("SELECT COUNT(*) FROM donna_memory_injecao_recusas WHERE job=?1", [job], |r| r.get(0))?)
}

/// Apaga recusas, pendências, liberações e tumbas de um bot — na exclusão do bot e no
/// `clearMemory`.
pub(crate) fn purge(db: &Connection, bot: &str) -> Result<()> {
    db.execute(
        "DELETE FROM donna_memory_injecao_pendentes WHERE recusa IN(SELECT id FROM donna_memory_injecao_recusas WHERE bot=?1)",
        [bot],
    )?;
    db.execute(
        "DELETE FROM donna_memory_injecao_liberacoes WHERE recusa IN(SELECT id FROM donna_memory_injecao_recusas WHERE bot=?1) OR bot=?1",
        [bot],
    )?;
    db.execute("DELETE FROM donna_memory_injecao_recusas WHERE bot=?1", [bot])?;
    db.execute("DELETE FROM donna_memory_injecao_tumbas WHERE bot=?1", [bot])?;
    Ok(())
}

/// `clearMemory` da camada compartilhada: apaga recusas, pendências, liberações e tumbas cujo alvo
/// é a camada apagada — a operação `shared` de QUALQUER bot (marca gravada no recibo, que sobrevive
/// à pendência já liberada) ou a entrada compartilhada (bot NULL). Roda ANTES de a camada ser
/// esquecida, enquanto os alvos ainda existem; sem isso uma recusa compartilhada sobreviveria à
/// limpeza e poderia recriar conteúdo nela.
pub(crate) fn purge_shared(db: &Connection) -> Result<()> {
    let ids: Vec<String> = db
        .prepare(
            "SELECT r.id FROM donna_memory_injecao_recusas r
             WHERE r.shared=1
                OR COALESCE(json_extract(
                       (SELECT p.operation FROM donna_memory_injecao_pendentes p WHERE p.recusa=r.id),
                       '$.shared'),0)=1
                OR EXISTS(SELECT 1 FROM memory_entries e WHERE e.id=r.entry AND e.bot IS NULL)",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for id in ids {
        db.execute("DELETE FROM donna_memory_injecao_pendentes WHERE recusa=?1", [&id])?;
        db.execute("DELETE FROM donna_memory_injecao_liberacoes WHERE recusa=?1", [&id])?;
        db.execute("DELETE FROM donna_memory_injecao_recusas WHERE id=?1", [&id])?;
        db.execute("DELETE FROM donna_memory_injecao_tumbas WHERE id=?1", [id])?;
    }
    // Tumba de recusa compartilhada já vencida não tem mais recibo: sai pela marca gravada nela.
    db.execute("DELETE FROM donna_memory_injecao_tumbas WHERE shared=1", [])?;
    Ok(())
}

/// Ids acima do teto por bot + Space, no desempate da auditoria (`at DESC, rowid DESC`). Com
/// `scope` só o par do recibo recém-inserido é podado, como `audit.rs` faz; sem ele a varredura
/// cobre todos os pares excedentes.
fn over_scope(db: &Connection, table: &str, scope: Option<(&str, &str)>) -> Result<Vec<String>> {
    let rows = match scope {
        Some((bot, space)) => db
            .prepare(&format!(
                "SELECT id FROM {table} WHERE bot=?1 AND space=?2
                 ORDER BY at DESC,rowid DESC LIMIT -1 OFFSET {SCOPE_CAP}"
            ))?
            .query_map(params![bot, space], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?,
        None => db
            .prepare(&format!(
                "SELECT id FROM (SELECT id,ROW_NUMBER() OVER(
                    PARTITION BY bot,space ORDER BY at DESC,rowid DESC) rn
                 FROM {table}) WHERE rn>{SCOPE_CAP}"
            ))?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?,
    };
    Ok(rows)
}

/// Ids acima do teto do Hub, mesmo desempate.
fn over_hub(db: &Connection, table: &str) -> Result<Vec<String>> {
    Ok(db
        .prepare(&format!("SELECT id FROM {table} ORDER BY at DESC,rowid DESC LIMIT -1 OFFSET {HUB_CAP}"))?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

fn drop_ids(db: &Connection, table: &str, ids: &[String]) -> Result<()> {
    for id in ids {
        db.execute(&format!("DELETE FROM {table} WHERE id=?1"), [id])?;
    }
    Ok(())
}

/// A recusa que sai leva junto a pendência e as liberações dela, para nunca sobrar texto sem
/// recibo.
fn drop_recusas(db: &Connection, ids: &[String]) -> Result<()> {
    for id in ids {
        db.execute("DELETE FROM donna_memory_injecao_pendentes WHERE recusa=?1", [id])?;
        db.execute("DELETE FROM donna_memory_injecao_liberacoes WHERE recusa=?1", [id])?;
        db.execute("DELETE FROM donna_memory_injecao_recusas WHERE id=?1", [id])?;
    }
    Ok(())
}

/// Retenção dos recibos do scanner, igual à da auditoria `non_saved` (`audit.rs`): vida de
/// `LIFE_MS`, teto de `SCOPE_CAP` por bot + Space e de `HUB_CAP` por Hub, saindo os mais antigos
/// primeiro (`at DESC, rowid DESC`) e aplicados EM SEQUÊNCIA — prazo, depois escopo, depois Hub —
/// para um excesso já resolvido não derrubar registro de outro escopo. Chamada DEPOIS da inserção
/// (da recusa em `refuse`, da liberação em `released`), com o `scope` do recibo novo; com `None`
/// poda todos os escopos (varredura de manutenção). Liberações e tumbas obedecem à mesma regra
/// pelos próprios `bot`/`space`/`at`. Memória, notas e chat do dono não são tocados. A recusa que
/// sai pelo prazo de vida deixa tumba em `donna_memory_injecao_tumbas` (só id/bot/space/shared/at,
/// sem texto nem hash), para a consulta responder `memory_refusal_expired` em vez de
/// "desconhecida". Devolve os ids de recusa removidos.
pub(crate) fn prune(db: &Connection, now: i64, scope: Option<(&str, &str)>) -> Result<Vec<String>> {
    let mut gone = Vec::new();
    // Prazo: tumba de id/bot/space/shared, sem texto nem hash — a consulta de uma recusa vencida
    // continua respondendo `memory_refusal_expired` depois que a poda rodou.
    db.execute(
        "INSERT OR IGNORE INTO donna_memory_injecao_tumbas(id,bot,space,shared,at)
         SELECT id,bot,space,shared,?2 FROM donna_memory_injecao_recusas WHERE at<=?1",
        params![now - LIFE_MS, now],
    )?;
    let expired: Vec<String> = db
        .prepare("SELECT id FROM donna_memory_injecao_recusas WHERE at<=?1")?
        .query_map([now - LIFE_MS], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    drop_recusas(db, &expired)?;
    gone.extend(expired);
    let extra = over_scope(db, "donna_memory_injecao_recusas", scope)?;
    drop_recusas(db, &extra)?;
    gone.extend(extra);
    let extra = over_hub(db, "donna_memory_injecao_recusas")?;
    drop_recusas(db, &extra)?;
    gone.extend(extra);
    // Liberações: mesma retenção, mesma sequência prazo → escopo → Hub.
    let table = "donna_memory_injecao_liberacoes";
    db.execute(&format!("DELETE FROM {table} WHERE at<=?1"), [now - LIFE_MS])?;
    drop_ids(db, table, &over_scope(db, table, scope)?)?;
    drop_ids(db, table, &over_hub(db, table)?)?;
    // Tumbas: a expiração em lote cria tumbas de QUALQUER escopo, então depois do prazo vale
    // primeiro o teto de TODOS os pares bot + Space e só então o do Hub (ordem do `audit.rs`),
    // para o excesso de um par não derrubar tumba válida de outro.
    let table = "donna_memory_injecao_tumbas";
    db.execute(&format!("DELETE FROM {table} WHERE at<=?1"), [now - LIFE_MS])?;
    drop_ids(db, table, &over_scope(db, table, None)?)?;
    drop_ids(db, table, &over_hub(db, table)?)?;
    Ok(gone)
}
