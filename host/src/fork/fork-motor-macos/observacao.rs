use std::{collections::BTreeMap, num::NonZeroU32};

use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, TransactionBehavior};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Instancia {
    pub token: TokenAuditado,
    pub inicio_micros: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TokenAuditado {
    pub pid: NonZeroU32,
    pub versao: NonZeroU32,
    pub uid_real: NonZeroU32,
    pub uid_efetivo: NonZeroU32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ParAutenticado {
    pub instancia: Instancia,
    pub titular: NonZeroU32,
    pub geracao: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Processo {
    pub pid: NonZeroU32,
    pub versao: Option<NonZeroU32>,
    pub inicio_micros: u64,
    pub uid_real: u32,
    pub uid_efetivo: u32,
}

pub(super) struct Censo {
    pub uid_real_observador: u32,
    pub uid_efetivo_observador: u32,
    pub por_uid_real: Vec<Processo>,
    pub por_uid_efetivo: Vec<Processo>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Observacao {
    Bloqueada,
    Trabalhadores(usize),
    ZeroObservado,
}

pub(super) struct Geracao {
    valor: i64,
}

impl Geracao {
    pub(super) fn inicializar(db: &Connection) -> Result<()> {
        configurar_durabilidade(db)?;
        let tx = db.unchecked_transaction().context("iniciar registro da geração")?;
        tx.execute_batch(
            "CREATE TABLE cerca_macos (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 geracao INTEGER NOT NULL CHECK (geracao > 0)
             ) STRICT;
             INSERT INTO cerca_macos (id, geracao) VALUES (1, 1);",
        )
        .context("criar registro da geração")?;
        tx.commit().context("persistir registro inicial da geração")?;
        Ok(())
    }

    pub(super) fn recuperar(db: &mut Connection) -> Result<Self> {
        configurar_durabilidade(db)?;
        let tx =
            db.transaction_with_behavior(TransactionBehavior::Immediate).context("iniciar recuperação da geração")?;
        let valor = tx
            .query_row("UPDATE cerca_macos SET geracao = geracao + 1 WHERE id = 1 RETURNING geracao", [], |row| {
                row.get(0)
            })
            .context("avançar geração na recuperação")?;
        tx.commit().context("persistir geração recuperada")?;
        Ok(Self { valor })
    }

    pub(super) fn valor(&self) -> i64 {
        self.valor
    }

    pub(super) fn observar(
        self,
        db: &mut Connection,
        uid_motor: NonZeroU32,
        titular: NonZeroU32,
        antes: Option<ParAutenticado>,
        ler: impl FnOnce() -> Result<(Censo, Option<ParAutenticado>)>,
    ) -> Result<Observacao> {
        configurar_durabilidade(db)?;
        let tx =
            db.transaction_with_behavior(TransactionBehavior::Immediate).context("iniciar invalidação da geração")?;
        ensure!(
            tx.execute("UPDATE cerca_macos SET geracao = geracao + 1 WHERE id = 1 AND geracao = ?1", [self.valor])
                .context("atualizar geração para invalidar a observação")?
                == 1,
            "geração do observador não está vigente"
        );
        tx.commit().context("persistir invalidação da geração")?;
        let (censo, depois) = ler().context("ler censo após invalidar a geração")?;
        let vigente = db
            .query_row("SELECT geracao FROM cerca_macos WHERE id = 1", [], |row| row.get::<_, i64>(0))
            .context("reconferir geração após o censo")?;
        ensure!(self.valor.checked_add(1) == Some(vigente), "geração mudou durante a observação");
        let uid = uid_motor.get();
        if censo.uid_real_observador != titular.get()
            || censo.uid_efetivo_observador != titular.get()
            || titular == uid_motor
        {
            return Ok(Observacao::Bloqueada);
        }
        let Some(par) = antes.filter(|par| {
            Some(*par) == depois
                && par.geracao == self.valor
                && par.titular == titular
                && par.instancia.token.uid_real == uid_motor
                && par.instancia.token.uid_efetivo == uid_motor
                && par.instancia.inicio_micros > 0
        }) else {
            return Ok(Observacao::Bloqueada);
        };

        let mut uniao = BTreeMap::new();
        for (real, processos) in [(true, censo.por_uid_real), (false, censo.por_uid_efetivo)] {
            for p in processos {
                let uid_filtrado = if real { p.uid_real } else { p.uid_efetivo };
                let (anterior, viu_real, viu_efetivo) = uniao.entry(p.pid).or_insert((p, false, false));
                let visto = if real { viu_real } else { viu_efetivo };
                if uid_filtrado != uid || p.inicio_micros == 0 || *anterior != p || *visto {
                    return Ok(Observacao::Bloqueada);
                }
                *visto = true;
            }
        }
        if uniao.values().any(|(p, real, efetivo)| *real != (p.uid_real == uid) || *efetivo != (p.uid_efetivo == uid)) {
            return Ok(Observacao::Bloqueada);
        }
        let Some((supervisor, _, _)) = uniao.remove(&par.instancia.token.pid) else {
            return Ok(Observacao::Bloqueada);
        };
        if supervisor.versao != Some(par.instancia.token.versao)
            || supervisor.inicio_micros != par.instancia.inicio_micros
            || supervisor.uid_real != uid
            || supervisor.uid_efetivo != uid
        {
            return Ok(Observacao::Bloqueada);
        }
        Ok(if uniao.is_empty() { Observacao::ZeroObservado } else { Observacao::Trabalhadores(uniao.len()) })
    }
}

fn configurar_durabilidade(db: &Connection) -> Result<()> {
    let journal: String =
        db.pragma_query_value(None, "journal_mode", |row| row.get(0)).context("consultar journal da geração")?;
    ensure!(journal == "delete", "journal da geração não é durável no modo previsto");
    db.pragma_update(None, "synchronous", "FULL").context("configurar sincronização da geração")?;
    db.pragma_update(None, "fullfsync", true).context("configurar flush da geração")?;
    Ok(())
}
