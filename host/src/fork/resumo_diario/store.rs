//! Tabela durável do resumo do dia: só o texto curto de cada bot e metadados, uma linha por
//! bot e dia. Funções `*_na` usam somente a `Connection` recebida.
use crate::store::{Store, now_ms};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

const CONFIG: &str = "daily_summary.v1";

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    let tx = c.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_daily_summaries(
           bot_id TEXT NOT NULL, day TEXT NOT NULL, up_bot_id TEXT,
           state TEXT NOT NULL CHECK(state IN ('running','done','silent','skipped','failed','interrupted')),
           reason TEXT, bot_line TEXT, team_line TEXT,
           attempts INTEGER NOT NULL CHECK(attempts BETWEEN 0 AND 2),
           since INTEGER NOT NULL, consumed_at INTEGER, chain_id TEXT,
           created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
           PRIMARY KEY(bot_id, day),
           CHECK(state<>'done' OR bot_line IS NOT NULL OR team_line IS NOT NULL));
         CREATE INDEX IF NOT EXISTS donna_daily_summaries_up ON donna_daily_summaries(up_bot_id, consumed_at);
         UPDATE donna_daily_summaries SET state='interrupted' WHERE state='running';",
    )?;
    tx.commit()?;
    Ok(())
}

pub(crate) fn on_bot_deleted(c: &Connection, id: &str) -> Result<()> {
    c.execute("DELETE FROM donna_daily_summaries WHERE bot_id=?1", [id])?;
    c.execute("UPDATE donna_daily_summaries SET up_bot_id=NULL WHERE up_bot_id=?1", [id])?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Estado {
    Rodando,
    Feito,
    /// A IA disse que não há nada relevante.
    Silencio,
    /// Sem chamada: sem atividade, pausa ou fora do resumo.
    Pulado,
    Falhou,
    Interrompido,
}

impl Estado {
    fn sql(self) -> &'static str {
        match self {
            Self::Rodando => "running",
            Self::Feito => "done",
            Self::Silencio => "silent",
            Self::Pulado => "skipped",
            Self::Falhou => "failed",
            Self::Interrompido => "interrupted",
        }
    }
    fn of(sql: &str) -> Option<Self> {
        [Self::Rodando, Self::Feito, Self::Silencio, Self::Pulado, Self::Falhou, Self::Interrompido]
            .into_iter()
            .find(|e| e.sql() == sql)
    }
    pub(super) fn terminal(self) -> bool {
        !matches!(self, Self::Rodando | Self::Interrompido)
    }
}

pub(super) struct Linha {
    pub bot_id: String,
    pub estado: Estado,
    pub reason: Option<String>,
    pub bot_line: Option<String>,
    pub team_line: Option<String>,
}

fn linha(r: &rusqlite::Row<'_>) -> rusqlite::Result<Option<Linha>> {
    let estado: String = r.get(1)?;
    Ok(Estado::of(&estado).map(|estado| Linha {
        bot_id: r.get(0).unwrap_or_default(),
        estado,
        reason: r.get(2).ok().flatten(),
        bot_line: r.get(3).ok().flatten(),
        team_line: r.get(4).ok().flatten(),
    }))
}

const COLS: &str = "bot_id,state,reason,bot_line,team_line";

pub(super) fn linha_na(c: &Connection, bot: &str, day: &str) -> Result<Option<Linha>> {
    Ok(c.query_row(
        &format!("SELECT {COLS} FROM donna_daily_summaries WHERE bot_id=?1 AND day=?2"),
        params![bot, day],
        linha,
    )
    .optional()?
    .flatten())
}

pub(super) fn dia_na(c: &Connection, day: &str) -> Result<Vec<Linha>> {
    Ok(c.prepare(&format!("SELECT {COLS} FROM donna_daily_summaries WHERE day=?1 ORDER BY bot_id"))?
        .query_map([day], linha)?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

/// Início do último resumo feito (ou silencioso): a atividade nova conta a partir daí.
pub(super) fn ultimo_corte_na(c: &Connection, bot: &str) -> Result<Option<i64>> {
    Ok(c.query_row(
        "SELECT MAX(created_at) FROM donna_daily_summaries WHERE bot_id=?1 AND state IN ('done','silent')",
        [bot],
        |r| r.get(0),
    )?)
}

/// Resumos do mesmo dia de quem reporta direto a `bot`, ainda não consumidos. Dia anterior
/// nunca volta: o que não subiu no dia dele fica só na linha de quem escreveu.
pub(super) fn recebidos_na(c: &Connection, bot: &str, day: &str) -> Result<Vec<(String, String)>> {
    Ok(c.prepare(
        "SELECT bot_id,COALESCE(team_line,bot_line) FROM donna_daily_summaries
         WHERE up_bot_id=?1 AND day=?2 AND state='done' AND consumed_at IS NULL ORDER BY bot_id",
    )?
    .query_map(params![bot, day], |r| Ok((r.get(0)?, r.get(1)?)))?
    .collect::<rusqlite::Result<_>>()?)
}

/// Grava a linha `running`; recomeça só uma interrompida. Devolve a tentativa ou `None`.
pub(super) fn comecar_na(c: &Connection, bot: &str, day: &str, up: Option<&str>, since: i64) -> Result<Option<i64>> {
    let now = now_ms();
    let changed = c.execute(
        "INSERT INTO donna_daily_summaries(bot_id,day,up_bot_id,state,attempts,since,created_at,updated_at)
         VALUES(?1,?2,?3,'running',1,?4,?5,?5)
         ON CONFLICT(bot_id,day) DO UPDATE SET state='running',attempts=attempts+1,up_bot_id=?3,since=?4,
           reason=NULL,updated_at=?5 WHERE state='interrupted' AND attempts<2",
        params![bot, day, up, since, now],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    Ok(Some(c.query_row(
        "SELECT attempts FROM donna_daily_summaries WHERE bot_id=?1 AND day=?2",
        params![bot, day],
        |r| r.get(0),
    )?))
}

pub(super) fn cadeia_na(c: &Connection, bot: &str, day: &str, chain: &str) -> Result<()> {
    c.execute("UPDATE donna_daily_summaries SET chain_id=?3 WHERE bot_id=?1 AND day=?2", params![bot, day, chain])?;
    Ok(())
}

/// Linha sem chamada (`skipped`), gravada uma vez por bot e dia.
pub(super) fn pular_na(c: &Connection, bot: &str, day: &str, up: Option<&str>, reason: &str) -> Result<()> {
    let now = now_ms();
    c.execute(
        "INSERT INTO donna_daily_summaries(bot_id,day,up_bot_id,state,reason,attempts,since,created_at,updated_at)
         VALUES(?1,?2,?3,'skipped',?4,0,?5,?5,?5)
         ON CONFLICT(bot_id,day) DO UPDATE SET state='skipped',reason=?4,updated_at=?5 WHERE state='interrupted'",
        params![bot, day, up, reason, now],
    )?;
    Ok(())
}

pub(super) struct Fim<'a> {
    pub estado: Estado,
    pub reason: Option<&'a str>,
    pub bot_line: Option<&'a str>,
    pub team_line: Option<&'a str>,
    /// Marca como consumidos os resumos recebidos usados nesta linha.
    pub consome: bool,
}

pub(super) fn terminar(store: &Store, bot: &str, day: &str, fim: &Fim<'_>) -> Result<()> {
    use crate::LockExt;
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let now = now_ms();
    let attempts = if fim.estado == Estado::Interrompido { "MAX(attempts-1,0)" } else { "attempts" };
    tx.execute(
        &format!(
            "UPDATE donna_daily_summaries SET state=?3,reason=?4,bot_line=?5,team_line=?6,attempts={attempts},
             updated_at=?7 WHERE bot_id=?1 AND day=?2 AND state='running'"
        ),
        params![bot, day, fim.estado.sql(), fim.reason, fim.bot_line, fim.team_line, now],
    )?;
    if fim.consome {
        tx.execute(
            "UPDATE donna_daily_summaries SET consumed_at=?2 WHERE up_bot_id=?1 AND day=?3 AND state='done' AND consumed_at IS NULL",
            params![bot, now, day],
        )?;
    }
    tx.commit()?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Config {
    pub enabled: bool,
    /// "HH:MM", em passos de 15 min, de 02:00 a 23:45.
    pub time: String,
    pub time_zone: String,
    pub revision: u64,
}

impl Config {
    pub(super) fn zona(&self) -> chrono_tz::Tz {
        self.time_zone.parse().unwrap_or(chrono_tz::Tz::UTC)
    }
    /// Minutos desde a meia-noite.
    pub(super) fn minutos(&self) -> u32 {
        hora(&self.time).unwrap_or(19 * 60)
    }
}

fn hora(text: &str) -> Option<u32> {
    let (h, m) = text.split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    // A partir das 02:00: o nível mais fundo (T − 2 h) não cruza a meia-noite.
    (h < 24 && m < 60 && m % 15 == 0 && text.len() == 5 && h >= 2).then_some(h * 60 + m)
}

/// Fuso IANA deste computador (mesma regra das Rotinas no terminal).
pub(crate) fn fuso_local() -> String {
    use std::str::FromStr as _;
    let valid = |z: &str| chrono_tz::Tz::from_str(z).is_ok();
    if let Ok(tz) = std::env::var("TZ")
        && valid(tz.trim_start_matches(':'))
    {
        return tz.trim_start_matches(':').to_owned();
    }
    std::fs::read_link("/etc/localtime")
        .ok()
        .and_then(|p| p.to_string_lossy().split_once("zoneinfo/").map(|(_, z)| z.to_owned()))
        .filter(|z| valid(z))
        .unwrap_or_else(|| "UTC".into())
}

pub(crate) fn config(store: &Store) -> Result<Config> {
    match store.kv_read(CONFIG)? {
        Some(raw) => Ok(serde_json::from_str(&raw)?),
        None => Ok(Config { enabled: true, time: "19:00".into(), time_zone: fuso_local(), revision: 0 }),
    }
}

pub(super) fn salvar(store: &Store, enabled: bool, time: &str, time_zone: &str) -> Result<Config> {
    ensure!(hora(time).is_some(), "invalid_field: horário em HH:MM, de 15 em 15 minutos, entre 02:00 e 23:45.");
    time_zone.parse::<chrono_tz::Tz>().ok().context("invalid_field: fuso horário desconhecido.")?;
    let current = config(store)?;
    let next = Config {
        enabled,
        time: time.into(),
        time_zone: time_zone.into(),
        revision: current.revision.checked_add(1).context("revisão esgotada")?,
    };
    // D368: num pedido do app pareado, grava sob a autoridade original reconferida (`pedido::gravar`).
    let raw = serde_json::to_string(&next)?;
    crate::fork::donna_fixa::pedido::gravar(store, |c| {
        c.execute("INSERT INTO kv(k, v) VALUES(?1, ?2) ON CONFLICT(k) DO UPDATE SET v = ?2", [CONFIG, raw.as_str()])?;
        Ok(())
    })?;
    Ok(next)
}
