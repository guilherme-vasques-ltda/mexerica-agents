#![allow(clippy::unwrap_used)]
//! D70 (R4b, D357): migração das faixas do R1 e a fila por conexão (janela de 1 hora, intervalo).
use super::pace::{JITTER_MAX, Slot, gap_ms, random_jitter_pct, reserve};
use rusqlite::{Connection, params};

const MIN: i64 = 60_000;

/// Banco com a tabela do R1 (3–60 s, 1–60/h), como está nos Hubs instalados.
fn r1_db() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch(
        "CREATE TABLE donna_whatsapp_bots(
            bot_id TEXT PRIMARY KEY NOT NULL,
            read_chats INTEGER NOT NULL CHECK(read_chats IN (0,1)),
            reply_chats INTEGER NOT NULL CHECK(reply_chats IN (0,1)),
            read_groups INTEGER NOT NULL CHECK(read_groups IN (0,1)),
            reply_groups INTEGER NOT NULL CHECK(reply_groups IN (0,1)),
            reply_mode TEXT NOT NULL CHECK(reply_mode='approval'),
            delay_seconds INTEGER NOT NULL CHECK(delay_seconds BETWEEN 3 AND 60),
            hourly_limit INTEGER NOT NULL CHECK(hourly_limit BETWEEN 1 AND 60),
            transcribe_audio INTEGER NOT NULL CHECK(transcribe_audio IN (0,1)),
            CHECK(reply_chats<=read_chats),
            CHECK(reply_groups<=read_groups)
        ) STRICT;",
    )
    .unwrap();
    for (bot, delay, limit) in [("a", 3, 60), ("b", 7, 1), ("c", 8, 30), ("d", 60, 12)] {
        c.execute("INSERT INTO donna_whatsapp_bots VALUES(?1,1,1,0,0,'approval',?2,?3,1)", params![bot, delay, limit])
            .unwrap();
    }
    c
}

fn settings(c: &Connection, bot: &str) -> (u32, u32, bool) {
    c.query_row("SELECT delay_seconds,hourly_limit,reply_chats FROM donna_whatsapp_bots WHERE bot_id=?1", [bot], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    })
    .unwrap()
}

#[test]
fn migration_raises_3_to_7_seconds_to_8_and_keeps_the_rest() {
    let c = r1_db();
    super::migrate(&c).unwrap();
    assert_eq!(settings(&c, "a"), (8, 60, true));
    assert_eq!(settings(&c, "b"), (8, 1, true));
    assert_eq!(settings(&c, "c"), (8, 30, true));
    assert_eq!(settings(&c, "d"), (60, 12, true));
    // Faixas novas no próprio banco: 300 s e 200/h entram; 7 s e 201/h não.
    let set = |delay: u32, limit: u32| {
        c.execute(
            "UPDATE donna_whatsapp_bots SET delay_seconds=?1,hourly_limit=?2 WHERE bot_id='a'",
            params![delay, limit],
        )
    };
    set(300, 200).unwrap();
    assert!(set(7, 30).is_err());
    assert!(set(301, 30).is_err());
    assert!(set(8, 201).is_err());
    // Rodar de novo não mexe em nada.
    super::migrate(&c).unwrap();
    assert_eq!(settings(&c, "a"), (300, 200, true));
}

fn fresh() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    super::migrate(&c).unwrap();
    c
}

fn save(c: &Connection, bot: &str, delay: u32, limit: u32) {
    c.execute(
        "INSERT INTO donna_whatsapp_bots(bot_id,read_chats,reply_chats,read_groups,reply_groups,reply_mode,
         delay_seconds,hourly_limit,transcribe_audio)
         VALUES(?1,1,1,0,0,'approval',?2,?3,1)
         ON CONFLICT(bot_id) DO UPDATE SET delay_seconds=excluded.delay_seconds,hourly_limit=excluded.hourly_limit",
        params![bot, delay, limit],
    )
    .unwrap();
}

#[test]
fn variation_is_only_above_x() {
    assert_eq!(gap_ms(8, 0), 8_000);
    assert_eq!(gap_ms(8, 30), 10_400);
    // Acima de 30 % fica em 30 %; nunca abaixo de X.
    assert_eq!(gap_ms(10, 99), 13_000);
    for _ in 0..200 {
        assert!(random_jitter_pct() <= JITTER_MAX);
    }
}

#[test]
fn interval_holds_between_messages_of_the_same_connection() {
    let c = fresh();
    save(&c, "donna", 15, 200);
    let t = 1_000_000_000;
    assert_eq!(reserve(&c, "donna", "acc", t, 0).unwrap(), Slot::Now);
    assert_eq!(reserve(&c, "donna", "acc", t + 14_999, 0).unwrap(), Slot::WaitUntil(t + 15_000));
    assert_eq!(reserve(&c, "donna", "acc", t + 15_000, 20).unwrap(), Slot::Now);
    // A variação sorteada no envio anterior (20 %) vale para o próximo.
    assert_eq!(reserve(&c, "donna", "acc", t + 30_000, 0).unwrap(), Slot::WaitUntil(t + 33_000));
    // Sem linha de ajustes, vale o padrão novo (15 s).
    assert_eq!(reserve(&c, "lia", "acc", t, 0).unwrap(), Slot::Now);
    assert_eq!(reserve(&c, "lia", "acc", t + 14_000, 0).unwrap(), Slot::WaitUntil(t + 15_000));
}

#[test]
fn two_routines_share_the_hourly_limit_of_the_connection() {
    let c = fresh();
    save(&c, "donna", 8, 3);
    save(&c, "lia", 8, 3);
    let t = 1_000_000_000;
    // "Rotina A" e "rotina B" no mesmo número: a soma das duas é que conta.
    let mut at = t;
    for _ in 0..3 {
        assert_eq!(reserve(&c, "donna", "acc", at, 0).unwrap(), Slot::Now);
        at += 10 * MIN;
    }
    // Quarta mensagem (de qualquer rotina): espera sair da janela a primeira.
    assert_eq!(reserve(&c, "donna", "acc", at, 0).unwrap(), Slot::WaitUntil(t + 60 * MIN));
    // Outra conexão não divide a fila.
    assert_eq!(reserve(&c, "lia", "acc", at, 0).unwrap(), Slot::Now);
    // Uma hora depois da primeira, abre uma vaga, e só uma.
    assert_eq!(reserve(&c, "donna", "acc", t + 60 * MIN, 0).unwrap(), Slot::Now);
    assert_eq!(reserve(&c, "donna", "acc", t + 60 * MIN + 8_000, 0).unwrap(), Slot::WaitUntil(t + 70 * MIN));
    // O dono baixou o limite para 1: espera até sobrar só uma na janela.
    save(&c, "donna", 8, 1);
    assert_eq!(reserve(&c, "donna", "acc", t + 61 * MIN, 0).unwrap(), Slot::WaitUntil(t + 120 * MIN));
    assert_eq!(reserve(&c, "donna", "acc", t + 120 * MIN, 0).unwrap(), Slot::Now);
}

#[test]
fn changing_the_interval_applies_to_the_next_message() {
    let c = fresh();
    save(&c, "donna", 8, 200);
    let t = 1_000_000_000;
    assert_eq!(reserve(&c, "donna", "acc", t, 0).unwrap(), Slot::Now);
    // Dono sobe de 8 para 300 s: a próxima espera 300 s, não 8.
    save(&c, "donna", 300, 200);
    assert_eq!(reserve(&c, "donna", "acc", t + 8_000, 0).unwrap(), Slot::WaitUntil(t + 300_000));
    // Com a variação sorteada no envio anterior (10 %), sobre o X atual.
    assert_eq!(reserve(&c, "donna", "acc", t + 300_000, 10).unwrap(), Slot::Now);
    assert_eq!(reserve(&c, "donna", "acc", t + 600_000, 0).unwrap(), Slot::WaitUntil(t + 300_000 + 330_000));
    // Dono baixa para 8 s: vale na hora, e nunca abaixo do piso de 8 s (+ variação).
    save(&c, "donna", 8, 200);
    let after = t + 300_000;
    assert_eq!(reserve(&c, "donna", "acc", after + 8_799, 0).unwrap(), Slot::WaitUntil(after + 8_800));
    assert_eq!(reserve(&c, "donna", "acc", after + 8_800, 0).unwrap(), Slot::Now);
    // O banco não aceita intervalo abaixo de 8 s.
    assert!(c.execute("UPDATE donna_whatsapp_bots SET delay_seconds=7 WHERE bot_id='donna'", []).is_err());
}
