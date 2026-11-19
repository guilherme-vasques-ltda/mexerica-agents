//! SQL fixo, somente Connection recebida. Não adquire locks, Store, Cloud ou I/O externo.
use super::modelo::{ContextoPersistido, Escopo, Fase, Origem, Pedido, Recibo, Recusa, Tipo};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};

struct Existente {
    id: String,
    intent: String,
    parent: Option<String>,
    kind: String,
    source: String,
    routine: Option<String>,
    scope: String,
}
fn row_existing(row: &rusqlite::Row<'_>) -> rusqlite::Result<Existente> {
    Ok(Existente {
        id: row.get(0)?,
        intent: row.get(1)?,
        parent: row.get(2)?,
        kind: row.get(3)?,
        source: row.get(4)?,
        routine: row.get(5)?,
        scope: row.get(6)?,
    })
}

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    let tx = c.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_chain_roots(
           id TEXT PRIMARY KEY, version INTEGER NOT NULL CHECK(version=1),
           occurrence TEXT NOT NULL UNIQUE, intent TEXT NOT NULL CHECK(length(intent)=64),
           source TEXT NOT NULL CHECK(source IN ('schedule','manual','legacy-pending','internal')),
           scope TEXT NOT NULL,
           routine_id TEXT, bot_id TEXT NOT NULL, run_id TEXT NOT NULL UNIQUE,
           used INTEGER NOT NULL CHECK(used BETWEEN 1 AND 50),
           linked INTEGER NOT NULL DEFAULT 0 CHECK(linked IN (0,1)));
         CREATE TABLE IF NOT EXISTS donna_chain_admissions(
           id TEXT PRIMARY KEY, chain_id TEXT NOT NULL REFERENCES donna_chain_roots(id),
           run_id TEXT NOT NULL, bot_id TEXT NOT NULL, event_key TEXT NOT NULL,
           intent TEXT NOT NULL CHECK(length(intent)=64),
           source TEXT NOT NULL CHECK(source IN ('schedule','manual','legacy-pending','internal')),
           routine_id TEXT, scope TEXT NOT NULL,
           predecessor TEXT REFERENCES donna_chain_admissions(id),
           kind TEXT NOT NULL CHECK(kind IN ('root','continuation','delegation')),
           level INTEGER NOT NULL CHECK(level BETWEEN 1 AND 5),
           ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 1 AND 50),
           phase TEXT NOT NULL CHECK(phase IN ('reserved','sending','finished')),
           UNIQUE(chain_id,event_key), UNIQUE(chain_id,ordinal), UNIQUE(run_id,ordinal));
         CREATE INDEX IF NOT EXISTS donna_chain_orphans ON donna_chain_roots(linked,routine_id);",
    )?;
    let linked: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('donna_chain_admissions') WHERE name='linked')",
        [],
        |r| r.get(0),
    )?;
    if !linked {
        tx.execute(
            "ALTER TABLE donna_chain_admissions ADD COLUMN linked INTEGER NOT NULL DEFAULT 0 CHECK(linked IN (0,1))",
            [],
        )?;
    }
    tx.commit()?;
    Ok(())
}

fn row_receipt(row: &rusqlite::Row<'_>) -> rusqlite::Result<Recibo> {
    Ok(Recibo {
        chain_id: row.get(0)?,
        admission_id: row.get(1)?,
        run_id: row.get(2)?,
        bot_id: row.get(3)?,
        level: row.get(4)?,
        ordinal: row.get(5)?,
    })
}

pub(crate) fn receipt(c: &Connection, id: &str) -> Result<Option<Recibo>> {
    Ok(c.query_row(
        "SELECT a.chain_id,a.id,a.run_id,a.bot_id,a.level,a.ordinal FROM donna_chain_admissions a
         JOIN donna_chain_roots r ON r.id=a.chain_id AND r.version=1 WHERE a.id=?1",
        [id],
        row_receipt,
    )
    .optional()?)
}

/// Uma consulta valida identidade e contexto do predecessor contra a raiz, sem nova autoridade.
/// Tipos futuros não concedem outro bot, Run ou contexto por possuírem um predecessor.
pub(crate) fn contexto(c: &Connection, r: &Recibo) -> Result<Option<ContextoPersistido>> {
    let stored: Option<(String, Option<String>)> = c
        .query_row(
            "SELECT a.scope,a.routine_id FROM donna_chain_admissions a
         JOIN donna_chain_roots root ON root.id=a.chain_id AND root.version=1
         WHERE a.id=?1 AND a.chain_id=?2 AND a.run_id=?3 AND a.bot_id=?4 AND a.level=?5 AND a.ordinal=?6
           AND json_extract(a.scope,'$.bot_id')=a.bot_id
           AND ((a.scope=root.scope AND a.bot_id=root.bot_id AND a.run_id=root.run_id
                 AND a.routine_id IS root.routine_id)
                OR json_extract(a.scope,'$.kind')='rotina_descendente')",
            params![r.admission_id, r.chain_id, r.run_id, r.bot_id, r.level, r.ordinal],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match stored {
        Some((scope, rotina)) => {
            let escopo = serde_json::from_str(&scope)?;
            // A alternativa SQL só classifica; nunca autoriza sem validar a âncora.
            if matches!(escopo, Escopo::RotinaDescendente { .. }) {
                return super::descendencia_rotina::contexto_na(c, r);
            }
            Ok(Some(ContextoPersistido { escopo, rotina }))
        }
        None => Ok(None),
    }
}

pub(crate) fn phase(c: &Connection, r: &Recibo) -> Result<Fase> {
    ensure!(receipt(c, &r.admission_id)?.as_ref() == Some(r), Recusa::ReciboInvalido);
    let phase: String =
        c.query_row("SELECT phase FROM donna_chain_admissions WHERE id=?1", [&r.admission_id], |row| row.get(0))?;
    Ok(match phase.as_str() {
        "reserved" => Fase::Reservada,
        "sending" => Fase::EnvioPossivel,
        _ => Fase::Concluida,
    })
}

/// Consumidor original: até cinco instruções (predecessor, evento, contador, UPDATE e INSERT).
/// Descendência de Rotinas valida até cinco ancestrais e recebe orçamento próprio de 32 no seam.
pub(crate) fn admit(c: &Connection, p: &Pedido) -> Result<Result<Recibo, Recusa>> {
    let scope = serde_json::to_string(&p.escopo)?;
    let previous = match (&p.anterior, p.tipo) {
        (None, Tipo::Raiz) if matches!(p.escopo, Escopo::RotinaDescendente { .. }) => {
            return Ok(Err(Recusa::ReciboInvalido));
        }
        (None, Tipo::Raiz) => None,
        (Some(r), Tipo::Delegacao) if matches!(p.escopo, Escopo::RotinaDescendente { .. }) => {
            if !super::descendencia_rotina::predecessor_valido(c, r, p)? {
                return Ok(Err(Recusa::ReciboInvalido));
            }
            Some(r)
        }
        // Repasse MCP entre bots: ramo próprio; as guardas de Rotinas abaixo não mudam.
        (Some(r), Tipo::Delegacao) if matches!(p.escopo, Escopo::PedidoEntreBots { .. }) => {
            if !super::equipe::predecessor_valido(c, r, p)? {
                return Ok(Err(Recusa::ReciboInvalido));
            }
            Some(r)
        }
        (Some(r), Tipo::Continuacao | Tipo::Delegacao) => {
            let Some(contexto) = contexto(c, r)? else {
                return Ok(Err(Recusa::ReciboInvalido));
            };
            // Contar uma filha não autoriza atravessar bot/Run/contexto. Novo consumidor exige regra própria.
            if r.bot_id != p.candidato.bot_id
                || r.run_id != p.candidato.run_id
                || contexto.escopo != p.escopo
                || contexto.rotina != p.rotina
                || p.origem != Origem::Interna
            {
                return Ok(Err(Recusa::ReciboInvalido));
            }
            Some(r)
        }
        _ => return Ok(Err(Recusa::ReciboInvalido)),
    };
    let chain = previous.map_or(p.candidato.chain_id.as_str(), |r| r.chain_id.as_str());
    let existing: Option<Existente> = if previous.is_none() {
        c.query_row(
            "SELECT a.id,a.intent,a.predecessor,a.kind,r.source,r.routine_id,r.scope FROM donna_chain_roots r
             JOIN donna_chain_admissions a ON a.chain_id=r.id AND a.kind='root' WHERE r.occurrence=?1",
            [&p.evento],
            row_existing,
        )
        .optional()?
    } else {
        c.query_row(
            "SELECT id,intent,predecessor,kind,source,routine_id,scope FROM donna_chain_admissions WHERE chain_id=?1 AND event_key=?2",
            params![chain, p.evento],
            row_existing,
        ).optional()?
    };
    if let Some(Existente { id, intent, parent, kind, source, routine, scope }) = existing {
        let r = receipt(c, &id)?.ok_or(Recusa::ReciboInvalido)?;
        if intent != p.intencao
            || parent.as_deref() != previous.map(|r| r.admission_id.as_str())
            || kind != p.tipo.sql()
            || r.bot_id != p.candidato.bot_id
            || source != p.origem.sql()
            || routine != p.rotina
            || scope != serde_json::to_string(&p.escopo)?
            || (previous.is_some() && r.run_id != p.candidato.run_id)
        {
            return Ok(Err(Recusa::Conflito));
        }
        return Ok(Ok(r));
    }
    let level = previous.map_or(1, |r| r.level + i64::from(p.tipo == Tipo::Delegacao));
    if level > 5 {
        return Ok(Err(Recusa::Nivel));
    }
    let ordinal = if previous.is_some() {
        let used: i64 = c.query_row("SELECT used FROM donna_chain_roots WHERE id=?1", [chain], |r| r.get(0))?;
        if used >= 50 {
            return Ok(Err(Recusa::Execucoes));
        }
        ensure!(
            c.execute(
                "UPDATE donna_chain_roots SET used=used+1 WHERE id=?1 AND used=?2 AND used<50",
                params![chain, used]
            )? == 1,
            Recusa::Conflito
        );
        used + 1
    } else {
        c.execute(
            "INSERT INTO donna_chain_roots(id,version,occurrence,intent,source,routine_id,bot_id,run_id,used,scope)
             VALUES(?1,1,?2,?3,?4,?5,?6,?7,1,?8)",
            params![
                chain,
                p.evento,
                p.intencao,
                p.origem.sql(),
                p.rotina,
                p.candidato.bot_id,
                p.candidato.run_id,
                &scope
            ],
        )?;
        1
    };
    let r = Recibo { chain_id: chain.into(), level, ordinal, ..p.candidato.clone() };
    c.execute(
        "INSERT INTO donna_chain_admissions(id,chain_id,run_id,bot_id,event_key,intent,predecessor,kind,level,ordinal,phase,source,routine_id,scope)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'reserved',?11,?12,?13)",
        params![r.admission_id,r.chain_id,r.run_id,r.bot_id,p.evento,p.intencao,
            previous.map(|r| &r.admission_id),p.tipo.sql(),level,ordinal,p.origem.sql(),p.rotina,&scope],
    )?;
    Ok(Ok(r))
}

pub(crate) fn dispatch(c: &Connection, r: &Recibo) -> Result<Result<(), Recusa>> {
    let Some(context) = contexto(c, r)? else {
        return Ok(Err(Recusa::ReciboInvalido));
    };
    if matches!(context.escopo, Escopo::RotinaDescendente { .. }) {
        let changed = c.execute(
            "UPDATE donna_chain_admissions SET phase='sending' WHERE id=?1 AND phase='reserved' AND linked=1",
            [&r.admission_id],
        )?;
        return Ok(if changed == 1 { Ok(()) } else { Err(Recusa::EnvioPossivel) });
    }
    let changed = c.execute(
        "UPDATE donna_chain_admissions SET phase='sending' WHERE id=?1 AND phase='reserved'
         AND EXISTS(SELECT 1 FROM donna_chain_roots r WHERE r.id=chain_id AND r.linked=1)",
        [&r.admission_id],
    )?;
    Ok(if changed == 1 { Ok(()) } else { Err(Recusa::EnvioPossivel) })
}

pub(crate) fn finish(c: &Connection, r: &Recibo) -> Result<()> {
    ensure!(receipt(c, &r.admission_id)?.as_ref() == Some(r), Recusa::ReciboInvalido);
    c.execute("UPDATE donna_chain_admissions SET phase='finished' WHERE id=?1", [&r.admission_id])?;
    Ok(())
}

pub(crate) fn vinculada(c: &Connection, r: &Recibo) -> Result<bool> {
    let descendant: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM donna_chain_admissions WHERE id=?1 AND json_extract(scope,'$.kind')='rotina_descendente')", [&r.admission_id], |row| row.get(0))?;
    if descendant {
        ensure!(super::descendencia_rotina::contexto_na(c, r)?.is_some(), Recusa::ReciboInvalido);
        return Ok(c.query_row("SELECT linked FROM donna_chain_admissions WHERE id=?1", [&r.admission_id], |row| {
            row.get(0)
        })?);
    }
    Ok(c.query_row(
        "SELECT linked FROM donna_chain_roots WHERE id=?1 AND run_id=?2",
        params![r.chain_id, r.run_id],
        |row| row.get(0),
    )?)
}

/// Marcador junto do JSON: segundo commit legado ou a mesma Tx da admissão MCP.
/// Sem Tx nova ou dependência de memória.
pub(crate) fn link_runs(c: &Connection, raw: &str) -> Result<()> {
    c.execute(
        "UPDATE donna_chain_roots SET linked=1 WHERE linked=0 AND run_id IN
         (SELECT json_extract(value,'$.id') FROM json_each(?1,'$.runs')
          WHERE json_extract(value,'$._chain.run_id')=json_extract(value,'$.id')
            AND json_extract(value,'$._chain.chain_id')=donna_chain_roots.id)",
        [raw],
    )?;
    c.execute("UPDATE donna_chain_admissions SET linked=1 WHERE linked=0 AND json_extract(scope,'$.kind')='rotina_descendente' AND EXISTS(SELECT 1 FROM json_each(?1,'$.runs') run WHERE json_extract(run.value,'$.id')=donna_chain_admissions.run_id AND json_extract(run.value,'$.botId')=donna_chain_admissions.bot_id AND json_extract(run.value,'$.routineId')=donna_chain_admissions.routine_id AND json_extract(run.value,'$._chain.admission_id')=donna_chain_admissions.id AND json_extract(run.value,'$._chain.chain_id')=donna_chain_admissions.chain_id AND json_extract(run.value,'$._chain.run_id')=donna_chain_admissions.run_id AND json_extract(run.value,'$._chain.bot_id')=donna_chain_admissions.bot_id AND json_extract(run.value,'$._chain.level')=donna_chain_admissions.level AND json_extract(run.value,'$._chain.ordinal')=donna_chain_admissions.ordinal)", [raw])?;
    Ok(())
}
