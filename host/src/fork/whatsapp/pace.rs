//! R4b (D357): a fila de cada conexão (bot × número). No máximo N envios por hora e 1 a cada
//! X s (+0 a 30 % de variação, nunca abaixo de X), somando todas as rotinas e automações do
//! número. Só o registro: nada aqui envia. O envio (R4c) reserva a vez antes de cada mensagem.
#![cfg_attr(not(test), allow(dead_code, reason = "R4b entrega a fila; o envio do R4c é quem vai usá-la."))]
use super::store::bot_row;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;
/// Variação máxima acima de X, em %.
pub(super) const JITTER_MAX: u32 = 30;

/// A vez de uma mensagem nesta conexão.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Slot {
    /// Pode sair agora; o envio ficou registrado.
    Now,
    /// Espera na fila até este instante (ms). Nada foi registrado.
    WaitUntil(i64),
}

/// Intervalo até o próximo envio: X segundos mais `jitter_pct` % (limitado a 30 %). Nunca menos que X.
pub(super) fn gap_ms(delay_seconds: u32, jitter_pct: u32) -> i64 {
    i64::from(delay_seconds) * 10 * i64::from(100 + jitter_pct.min(JITTER_MAX))
}

/// Variação sorteada de 0 a 30 %. Sem sorteio disponível, a maior (mais devagar, nunca mais rápido).
pub(super) fn random_jitter_pct() -> u32 {
    getrandom::u32().map_or(JITTER_MAX, |n| n % (JITTER_MAX + 1))
}

/// Reserva a vez de uma mensagem da conexão `bot`, com os ajustes atuais do dono. Quem chama
/// segura a transação junto com o envio, para duas rotinas não pegarem a mesma vez.
pub(super) fn reserve(c: &Connection, bot: &str, account: &str, now_ms: i64, jitter_pct: u32) -> Result<Slot> {
    let settings = bot_row(c, bot)?.unwrap_or_default();
    c.execute("DELETE FROM donna_whatsapp_pace WHERE bot_id=?1 AND at_ms<=?2", params![bot, now_ms - HOUR_MS])?;
    let count: u32 = c.query_row("SELECT COUNT(*) FROM donna_whatsapp_pace WHERE bot_id=?1", [bot], |r| r.get(0))?;
    // Intervalo ATUAL do dono a partir do último envio (só a variação sorteada fica gravada):
    // subir o intervalo já vale para a próxima; baixar nunca fica abaixo do piso de 8 s.
    let last: Option<(i64, u32)> = c
        .query_row(
            "SELECT at_ms,jitter_pct FROM donna_whatsapp_pace WHERE bot_id=?1 ORDER BY at_ms DESC LIMIT 1",
            [bot],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let mut wait = last.map_or(i64::MIN, |(at, jitter)| at + gap_ms(settings.delay_seconds, jitter));
    if count >= settings.hourly_limit {
        // Limite cheio (ou baixado pelo dono): libera quando sair da janela o envio que devolve a vaga.
        let freed: Option<i64> = c
            .query_row(
                "SELECT at_ms FROM donna_whatsapp_pace WHERE bot_id=?1 ORDER BY at_ms LIMIT 1 OFFSET ?2",
                params![bot, count - settings.hourly_limit],
                |r| r.get(0),
            )
            .optional()?;
        wait = wait.max(freed.map_or(now_ms, |at| at + HOUR_MS));
    }
    // MX-18: além da hora, o dia por bot × número (janela móvel de 24 h sobre a fila).
    let day_ago = now_ms - DAY_MS;
    let mut sent = c.prepare(
        "SELECT last_attempt_at FROM donna_whatsapp_outbox
         WHERE bot_id=?1 AND account_id=?2 AND state IN ('sent','uncertain') AND last_attempt_at>?3
         ORDER BY last_attempt_at",
    )?;
    let times = sent
        .query_map(params![bot, account, day_ago], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if times.len() >= settings.daily_limit as usize {
        let free_at = times[times.len() - settings.daily_limit as usize] + DAY_MS;
        wait = wait.max(free_at);
    }
    if now_ms < wait {
        return Ok(Slot::WaitUntil(wait));
    }
    c.execute(
        "INSERT INTO donna_whatsapp_pace(bot_id,at_ms,jitter_pct) VALUES(?1,?2,?3)",
        params![bot, now_ms, jitter_pct.min(JITTER_MAX)],
    )?;
    Ok(Slot::Now)
}
