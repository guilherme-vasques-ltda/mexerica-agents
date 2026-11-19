#![allow(clippy::unwrap_used)]
//! MX-18: fila de saída gravada, dedup, limites, silêncio, incerto, revogação e reinício.
//! Wacli falso do R2; nada de rede, número ou credencial real.
use super::{
    number_tests::{DONNA, Fixture},
    outbox::{self, Enqueue, Enqueued, Origin},
};
use crate::{LockExt, store::now_ms};
use chrono::Timelike;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::time::Duration;

const ANA: &str = "5511988887777@s.whatsapp.net";

fn quiet_settings() -> Value {
    use chrono::Timelike;
    let now = chrono::Utc::now();
    let minute = now.hour() * 60 + now.minute();
    json!({"quietEnabled":true,"quietStart":minute,"quietEnd":(minute + 30) % 1440,"quietTimeZone":"UTC"})
}

async fn ready(settings: Value) -> Fixture {
    let f = Fixture::new(true);
    f.call("donnaWhatsappSave", json!({"enabled":true})).await.unwrap();
    // O silêncio padrão (22h–8h) seguraria os envios destes testes de madrugada;
    // quem quer silêncio passa `quietEnabled` ou usa `quiet_ready`.
    let mut settings = settings;
    if let Some(object) = settings.as_object_mut()
        && !object.contains_key("quietEnabled")
    {
        object.insert("quietEnabled".into(), json!(false));
    }
    f.call("donnaWhatsappBotSave", json!({"botId":DONNA,"settings":settings})).await.unwrap();
    f.connect(DONNA).await;
    f
}

async fn quiet_ready(settings: Value) -> Fixture {
    let f = ready(settings).await;
    f.call("donnaWhatsappBotSave", json!({"botId":DONNA,"settings":quiet_settings()})).await.unwrap();
    f
}

fn wrote(f: &Fixture, chat: &str, message: &str) {
    let c = f.hub.store.db.locked();
    let account: String =
        c.query_row("SELECT account_id FROM donna_whatsapp_numbers WHERE bot_id=?1", [DONNA], |r| r.get(0)).unwrap();
    c.execute(
        "INSERT INTO donna_whatsapp_messages(bot_id,chat_id,message_id,account_id,is_group,sender_name,sent_at,
         received_at,body,offline,read,expires_at) VALUES(?1,?2,?3,?4,?5,'Ana',1,1,'oi',0,0,?6)",
        params![DONNA, chat, message, account, chat.ends_with("@g.us"), i64::MAX],
    )
    .unwrap();
}

fn enqueue(f: &Fixture, chat: &str, reply_to: &str, text: &str, origin: Origin) -> Enqueued {
    outbox::enqueue(&f.hub, &Enqueue { bot: DONNA, chat, reply_to, text, origin }, now_ms()).unwrap()
}

fn card(f: &Fixture, chat: &str, reply_to: &str, text: &str) -> Enqueued {
    enqueue(f, chat, reply_to, text, Origin::OwnerCard)
}

fn rows(f: &Fixture) -> Vec<(String, String, String, i64)> {
    let c = f.hub.store.db.locked();
    let mut stmt = c
        .prepare("SELECT id,state,COALESCE(reason,''),not_before FROM donna_whatsapp_outbox WHERE bot_id=?1 ORDER BY created_at")
        .unwrap();
    stmt.query_map([DONNA], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

async fn wait_rows(f: &Fixture, want: usize) -> Vec<(String, String, String, i64)> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let rows = rows(f);
        if rows.len() >= want {
            return rows;
        }
        assert!(tokio::time::Instant::now() < deadline, "fila não chegou em {want}: {rows:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_state(f: &Fixture, state: &str) -> Vec<(String, String, String, i64)> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let rows = rows(f);
        if rows.iter().any(|(_, s, _, _)| s == state) {
            return rows;
        }
        assert!(tokio::time::Instant::now() < deadline, "estado {state} não chegou: {rows:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_sends(f: &Fixture, n: usize) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while f.lines("sends") < n {
        assert!(tokio::time::Instant::now() < deadline, "envios: {} < {n}", f.lines("sends"));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_reason(f: &Fixture, state: &str, reason: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let rows = rows(f);
        if rows.iter().any(|(_, s, r, _)| s == state && r == reason) {
            return;
        }
        assert!(tokio::time::Instant::now() < deadline, "{state}/{reason} não chegou: {rows:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn wake(f: &Fixture) {
    f.hub.whatsapp.wake_outbox();
}

fn sql(f: &Fixture, q: &str) {
    f.hub.store.db.locked().execute_batch(q).unwrap();
}

#[tokio::test]
async fn same_message_twice_sends_once_and_survives_restart() {
    let f = Fixture::on_disk(true);
    f.call("donnaWhatsappSave", json!({"enabled":true})).await.unwrap();
    f.call(
        "donnaWhatsappBotSave",
        json!({"botId":DONNA,"settings":{"readChats":true,"replyChats":true,"delaySeconds":8,"quietEnabled":false}}),
    )
    .await
    .unwrap();
    f.connect(DONNA).await;
    wrote(&f, ANA, "M1");
    assert_eq!(card(&f, ANA, "M1", "Amanhã eu ligo."), Enqueued::Inserted);
    // Segunda vez (ainda na fila ou já enviada): dedup, nunca segunda linha.
    assert!(matches!(card(&f, ANA, "M1", "Amanhã eu ligo."), Enqueued::Existing(_)));
    wait_sends(&f, 1).await;
    let rows = wait_state(&f, "sent").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(f.lines("sends"), 1);
    // Reinício depois do envio: nada reenvia, nada some da auditoria.
    let f = f.restart().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(f.lines("sends"), 1);
    f.done().await;
}

#[tokio::test]
async fn held_item_survives_restart_and_leased_in_flight_becomes_uncertain() {
    let f = Fixture::on_disk(true);
    f.call("donnaWhatsappSave", json!({"enabled":true})).await.unwrap();
    f.call(
        "donnaWhatsappBotSave",
        json!({"botId":DONNA,"settings":{"readChats":true,"replyChats":true,"delaySeconds":8}}),
    )
    .await
    .unwrap();
    f.connect(DONNA).await;
    f.call("donnaWhatsappBotSave", json!({"botId":DONNA,"settings":quiet_settings()})).await.unwrap();
    wrote(&f, ANA, "M1");
    wrote(&f, ANA, "M2");
    card(&f, ANA, "M1", "primeira");
    card(&f, ANA, "M2", "segunda");
    wait_state(&f, "held").await;
    // Simula queda no meio de um envio: lease vencido vira incerto na recuperação.
    sql(&f, "UPDATE donna_whatsapp_outbox SET lease_until=1,attempts=1 WHERE reply_to='M2'");
    let f = f.restart().await;
    let rows = wait_rows(&f, 2).await;
    let states: Vec<&str> = rows.iter().map(|r| r.1.as_str()).collect();
    assert!(states.contains(&"held") || states.contains(&"sent"), "{states:?}");
    // A linha M2 ficou incerta (pode ter saído); nada reenvia sozinho.
    let uncertain = rows.iter().filter(|r| r.1 == "uncertain").count();
    assert!(uncertain >= 1, "{rows:?}");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(f.lines("sends"), 0, "incerto não pode reenviar");
    f.done().await;
}

#[tokio::test]
async fn daily_and_hourly_limits_hold_and_release() {
    let f = ready(json!({"readChats":true,"replyChats":true,"delaySeconds":8,"dailyLimit":1,"hourlyLimit":1})).await;
    wrote(&f, ANA, "M1");
    wrote(&f, ANA, "M2");
    card(&f, ANA, "M1", "um");
    card(&f, ANA, "M2", "dois");
    wait_sends(&f, 1).await;
    wait_reason(&f, "held", "limit").await;
    // Libera os dois limites envelhecendo o envio e o ritmo, e acorda a fila.
    sql(&f, "UPDATE donna_whatsapp_outbox SET last_attempt_at=last_attempt_at-90000000 WHERE state='sent'");
    sql(&f, "UPDATE donna_whatsapp_pace SET at_ms=at_ms-3700000");
    sql(&f, "UPDATE donna_whatsapp_outbox SET not_before=0 WHERE state='held'");
    wake(&f);
    wait_sends(&f, 2).await;
    f.done().await;
}

#[tokio::test]
async fn quiet_hours_hold_and_release_at_opening() {
    let f = ready(json!({"readChats":true,"replyChats":true,"delaySeconds":8,"quietEnabled":true,
            "quietStart":chrono::Utc::now().hour() * 60 + chrono::Utc::now().minute(),
            "quietEnd":(chrono::Utc::now().hour() * 60 + chrono::Utc::now().minute() + 30) % 1440,
            "quietTimeZone":"UTC"}))
    .await;
    wrote(&f, ANA, "M1");
    card(&f, ANA, "M1", "oi");
    wait_reason(&f, "held", "quiet").await;
    assert_eq!(f.lines("sends"), 0);
    // Dono desliga o silêncio (ou o horário abre): a mensagem sai.
    f.call("donnaWhatsappBotSave", json!({"botId":DONNA,"settings":{"quietEnabled":false}})).await.unwrap();
    sql(&f, "UPDATE donna_whatsapp_outbox SET not_before=0 WHERE state='held'");
    wake(&f);
    wait_sends(&f, 1).await;
    f.done().await;
}

#[tokio::test]
async fn uncertain_never_resends_and_owner_decides() {
    let f = ready(json!({"readChats":true,"replyChats":true,"delaySeconds":8})).await;
    std::fs::write(f.temp.join("fake/send-mode"), "fail").unwrap();
    wrote(&f, ANA, "M1");
    card(&f, ANA, "M1", "talvez");
    wait_state(&f, "uncertain").await;
    assert_eq!(f.lines("sends"), 1);
    wake(&f);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(f.lines("sends"), 1);
    let id = rows(&f)[0].0.clone();
    outbox::decide(&f.hub.store, DONNA, &id, "cancelled").unwrap();
    assert_eq!(rows(&f)[0].1, "cancelled");
    f.done().await;
}

#[tokio::test]
async fn not_started_retries_with_growing_wait_and_cap() {
    let f = ready(json!({"readChats":true,"replyChats":true,"delaySeconds":8})).await;
    wrote(&f, ANA, "-bad");
    card(&f, ANA, "-bad", "retry");
    wait_reason(&f, "pending", "retry").await;
    let first = rows(&f);
    let (_, _, reason, not_before) = &first[0];
    assert_eq!(reason, "retry");
    assert!(*not_before > now_ms(), "{first:?}");
    // Segunda tentativa: espera maior (5 s → 10 s), ainda sem enviar.
    sql(&f, "UPDATE donna_whatsapp_outbox SET not_before=0");
    wake(&f);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let first_wait = *not_before;
    loop {
        let cur = rows(&f);
        if cur[0].2 == "retry" && cur[0].3 > first_wait + 4_000 {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "retry não cresceu: {cur:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(f.lines("sends"), 0, "reply_to inválido não chega ao wacli");
    f.done().await;
}

#[tokio::test]
async fn revoking_reply_or_disabling_empties_the_queue() {
    let f = quiet_ready(json!({"readChats":true,"replyChats":true,"delaySeconds":8})).await;
    wrote(&f, ANA, "M1");
    wrote(&f, ANA, "M2");
    card(&f, ANA, "M1", "segurada");
    wait_reason(&f, "held", "quiet").await;
    // Desligar "Responder" cancela na mesma hora, mesmo segurada.
    f.call("donnaWhatsappBotSave", json!({"botId":DONNA,"settings":{"replyChats":false}})).await.unwrap();
    wait_reason(&f, "cancelled", "reply_revoked").await;
    // Outra segurada; desligar o plugin esvazia tudo.
    f.call("donnaWhatsappBotSave", json!({"botId":DONNA,"settings":{"replyChats":true}})).await.unwrap();
    card(&f, ANA, "M2", "segunda");
    wait_reason(&f, "held", "quiet").await;
    f.call("donnaWhatsappSave", json!({"enabled":false})).await.unwrap();
    wait_reason(&f, "cancelled", "plugin_disabled").await;
    f.done().await;
}

#[tokio::test]
async fn disconnect_cancels_the_queue() {
    let f = quiet_ready(json!({"readChats":true,"replyChats":true,"delaySeconds":8})).await;
    wrote(&f, ANA, "M1");
    card(&f, ANA, "M1", "oi");
    wait_reason(&f, "held", "quiet").await;
    f.call("donnaWhatsappDisconnect", json!({"botId":DONNA,"erase":false})).await.unwrap();
    wait_reason(&f, "cancelled", "number_gone").await;
    f.done().await;
}

#[tokio::test]
async fn routine_and_direct_are_refused_in_approval_mode() {
    let f = ready(json!({"readChats":true,"replyChats":true,"delaySeconds":8})).await;
    wrote(&f, ANA, "M1");
    // Envio direto nem entra na fila.
    assert!(
        outbox::enqueue(
            &f.hub,
            &Enqueue { bot: DONNA, chat: ANA, reply_to: "M1", text: "direto", origin: Origin::OwnerDirect },
            now_ms(),
        )
        .is_err()
    );
    // Rotina entra, mas a conferência da vez recusa no modo "peço antes".
    enqueue(&f, ANA, "M1", "rotina", Origin::BotRoutine);
    wait_reason(&f, "cancelled", "not_allowed").await;
    assert_eq!(f.lines("sends"), 0);
    f.done().await;
}

// MARK: revisão P2/TEC: efeito externo nunca volta à fila — 'sending' é durável.

#[tokio::test]
async fn record_failures_past_lease_go_uncertain_for_the_owner() {
    let f = ready(json!({"readChats":true,"replyChats":true,"delaySeconds":8})).await;
    // Sent::Yes: o 'sent' não grava E o 'uncertain' de fallback também não.
    f.hub.whatsapp.fail_next_records(2);
    wrote(&f, ANA, "M1");
    assert_eq!(card(&f, ANA, "M1", "oi"), Enqueued::Inserted);
    wait_sends(&f, 1).await;
    // O item ficou 'sending' (gravado antes do envio): o dreno não o pega.
    // Lease vencido → a varredura marca incerto; o dono decide. Nunca reenvio.
    sql(&f, "UPDATE donna_whatsapp_outbox SET lease_until=0,not_before=0");
    wake(&f);
    wait_reason(&f, "uncertain", "uncertain").await;
    assert_eq!(f.lines("sends"), 1, "envio confirmado não pode reenviar");
    let id = rows(&f)[0].0.clone();
    f.call("donnaWhatsappQueueDecide", json!({"botId":DONNA,"itemId":id,"decision":"sent"})).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(f.lines("sends"), 1);
    f.done().await;
}

#[tokio::test]
async fn record_failure_after_send_falls_back_to_uncertain() {
    let f = ready(json!({"readChats":true,"replyChats":true,"delaySeconds":8})).await;
    f.hub.whatsapp.fail_next_records(1);
    wrote(&f, ANA, "M1");
    assert_eq!(card(&f, ANA, "M1", "oi"), Enqueued::Inserted);
    wait_sends(&f, 1).await;
    // O 'sent' não grava: o item cai para 'incerta' — nunca pending, nunca reenvio.
    // O dono decide pela fila.
    wait_reason(&f, "uncertain", "uncertain").await;
    assert_eq!(f.lines("sends"), 1);
    let id = rows(&f)[0].0.clone();
    f.call("donnaWhatsappQueueDecide", json!({"botId":DONNA,"itemId":id,"decision":"sent"})).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(f.lines("sends"), 1);
    f.done().await;
}

#[tokio::test]
async fn record_failure_past_restart_goes_uncertain_for_the_owner() {
    let f = Fixture::on_disk(true);
    f.call("donnaWhatsappSave", json!({"enabled":true})).await.unwrap();
    f.call(
        "donnaWhatsappBotSave",
        json!({"botId":DONNA,"settings":{"readChats":true,"replyChats":true,"delaySeconds":8,"quietEnabled":false}}),
    )
    .await
    .unwrap();
    f.connect(DONNA).await;
    f.hub.whatsapp.fail_next_records(usize::MAX);
    wrote(&f, ANA, "M1");
    assert_eq!(card(&f, ANA, "M1", "oi"), Enqueued::Inserted);
    wait_sends(&f, 1).await;
    // A gravação nunca passa: o item fica 'sending' — o dreno não o pega e nada
    // reenvia. No reinício a recuperação o marca incerto para o dono decidir
    // (o lease vencido em processo já é coberto pelo teste da varredura acima).
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(f.lines("sends"), 1);
    let f = f.restart().await;
    wait_reason(&f, "uncertain", "restart").await;
    assert_eq!(f.lines("sends"), 1);
    // O dono decide pela fila (QueueDecide): fecha sem reenvio.
    let id = rows(&f)[0].0.clone();
    f.call("donnaWhatsappQueueDecide", json!({"botId":DONNA,"itemId":id,"decision":"sent"})).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(f.lines("sends"), 1);
    f.done().await;
}

#[tokio::test]
async fn record_failure_after_uncertain_send_never_resends() {
    let f = ready(json!({"readChats":true,"replyChats":true,"delaySeconds":8})).await;
    std::fs::write(f.temp.join("fake/send-mode"), "fail").unwrap();
    f.hub.whatsapp.fail_next_records(1);
    wrote(&f, ANA, "M1");
    assert_eq!(card(&f, ANA, "M1", "talvez"), Enqueued::Inserted);
    wait_sends(&f, 1).await;
    // A gravação do resultado ambíguo falhou: o item fica 'sending'. Lease
    // vencido → incerto para o dono — nunca reenvia.
    sql(&f, "UPDATE donna_whatsapp_outbox SET lease_until=0,not_before=0 WHERE state='sending'");
    wake(&f);
    wait_state(&f, "uncertain").await;
    assert_eq!(f.lines("sends"), 1);
    f.done().await;
}

#[tokio::test]
async fn queue_rpc_lists_and_owner_decides() {
    let f = ready(json!({"readChats":true,"replyChats":true,"delaySeconds":8})).await;
    wrote(&f, ANA, "M1");
    card(&f, ANA, "M1", "ver fila");
    wait_sends(&f, 1).await;
    let view = f.call("donnaWhatsappQueue", json!({"botId":DONNA})).await.unwrap();
    assert!(view["items"].as_array().unwrap().iter().any(|i| i["text"] == "ver fila"));
    let id = rows(&f)[0].0.clone();
    let view = f.call("donnaWhatsappQueueDecide", json!({"botId":DONNA,"itemId":id,"decision":"cancelled"})).await;
    // Já enviada: decisão inválida, sem efeito.
    assert!(view.is_err());
    f.done().await;
}

#[tokio::test]
async fn autonomous_mode_sends_only_to_marked_chats() {
    let f = ready(json!({"readChats":true,"replyChats":true,"replyMode":"autonomous","delaySeconds":8})).await;
    f.call("donnaWhatsappChatMark", json!({"botId":DONNA,"whatsappChatId":ANA,"marked":true})).await.unwrap();
    wrote(&f, ANA, "M1");
    enqueue(&f, ANA, "M1", "sozinho", Origin::BotRoutine);
    wait_sends(&f, 1).await;
    // Conversa não marcada: a conferência da vez recusa, nada sai.
    let other = "5511977776666@s.whatsapp.net";
    wrote(&f, other, "M1");
    enqueue(&f, other, "M1", "sozinho", Origin::BotRoutine);
    wait_reason(&f, "cancelled", "not_allowed").await;
    assert_eq!(f.lines("sends"), 1);
    f.done().await;
}

#[tokio::test]
async fn unmarking_cancels_the_held_autonomous_reply() {
    let f = ready(json!({"readChats":true,"replyChats":true,"replyMode":"autonomous","delaySeconds":8})).await;
    f.call("donnaWhatsappChatMark", json!({"botId":DONNA,"whatsappChatId":ANA,"marked":true})).await.unwrap();
    f.call("donnaWhatsappBotSave", json!({"botId":DONNA,"settings":quiet_settings()})).await.unwrap();
    wrote(&f, ANA, "M1");
    enqueue(&f, ANA, "M1", "sozinho", Origin::BotRoutine);
    wait_reason(&f, "held", "quiet").await;
    f.call("donnaWhatsappChatMark", json!({"botId":DONNA,"whatsappChatId":ANA,"marked":false})).await.unwrap();
    wait_reason(&f, "cancelled", "unmarked").await;
    f.done().await;
}

#[tokio::test]
async fn leaving_autonomous_mode_cancels_bot_queue() {
    let f = ready(json!({"readChats":true,"replyChats":true,"replyMode":"autonomous","delaySeconds":8})).await;
    f.call("donnaWhatsappChatMark", json!({"botId":DONNA,"whatsappChatId":ANA,"marked":true})).await.unwrap();
    f.call("donnaWhatsappBotSave", json!({"botId":DONNA,"settings":quiet_settings()})).await.unwrap();
    wrote(&f, ANA, "M1");
    enqueue(&f, ANA, "M1", "sozinho", Origin::BotRoutine);
    wait_reason(&f, "held", "quiet").await;
    f.call("donnaWhatsappBotSave", json!({"botId":DONNA,"settings":{"replyMode":"approval"}})).await.unwrap();
    wait_reason(&f, "cancelled", "mode").await;
    f.done().await;
}

// MARK: MX-18/D379: o dono escreve no mini `WhatsApp` — direto, fora da fila.

#[tokio::test]
async fn owner_send_goes_out_directly_without_queue() {
    let f = ready(json!({"readChats":true,"replyChats":true,"delaySeconds":8})).await;
    wrote(&f, ANA, "M1");
    let out =
        f.call("donnaWhatsappSend", json!({"botId":DONNA,"whatsappChatId":ANA,"text":"oi, tudo bem?"})).await.unwrap();
    assert_eq!(out["state"], "sent");
    assert_eq!(f.lines("sends"), 1);
    let sends = std::fs::read_to_string(f.temp.join("fake/sends")).unwrap();
    assert!(sends.contains("--reply-to=M1"), "{sends}");
    assert!(rows(&f).is_empty(), "envio do dono não entra na fila do bot");
    f.done().await;
}

#[tokio::test]
async fn owner_send_needs_number_and_a_chat_that_wrote() {
    let f = Fixture::new(true);
    let err = f.call("donnaWhatsappSend", json!({"botId":DONNA,"whatsappChatId":ANA,"text":"oi"})).await.unwrap_err();
    assert!(err.to_string().contains("not_connected"), "{err}");
    f.call("donnaWhatsappSave", json!({"enabled":true})).await.unwrap();
    f.call("donnaWhatsappBotSave", json!({"botId":DONNA,"settings":{"readChats":true}})).await.unwrap();
    f.connect(DONNA).await;
    // Conversa que nunca escreveu para este número: nada sai.
    let err = f.call("donnaWhatsappSend", json!({"botId":DONNA,"whatsappChatId":ANA,"text":"oi"})).await.unwrap_err();
    assert!(err.to_string().contains("not_allowed"), "{err}");
    // Texto fora do formato seguro: nada sai.
    wrote(&f, ANA, "M1");
    let err = f.call("donnaWhatsappSend", json!({"botId":DONNA,"whatsappChatId":ANA,"text":"  "})).await.unwrap_err();
    assert!(err.to_string().contains("invalid_field"), "{err}");
    assert_eq!(f.lines("sends"), 0);
    f.done().await;
}

#[tokio::test]
async fn chats_list_marks_only_chats_the_owner_marked() {
    let f = ready(json!({"readChats":true,"replyChats":true,"replyMode":"autonomous","delaySeconds":8})).await;
    let other = "5511977776666@s.whatsapp.net";
    wrote(&f, ANA, "M1");
    wrote(&f, other, "M1");
    f.call("donnaWhatsappChatMark", json!({"botId":DONNA,"whatsappChatId":ANA,"marked":true})).await.unwrap();
    let view = f.call("donnaWhatsappChats", json!({"botId":DONNA})).await.unwrap();
    let chats = view["chats"].as_array().unwrap();
    let marked = |id: &str| chats.iter().find(|c| c["whatsappChatId"] == id).unwrap()["marked"] == true;
    assert!(marked(ANA));
    assert!(!marked(other));
    f.done().await;
}

#[test]
fn migration_adds_sending_to_the_outbox_states() {
    // Banco como está instalado: CHECK de state sem 'sending' e os dois índices.
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch(
        "CREATE TABLE donna_whatsapp_outbox(
            id TEXT PRIMARY KEY NOT NULL,
            bot_id TEXT NOT NULL,
            account_id TEXT NOT NULL,
            chat_id TEXT NOT NULL,
            is_group INTEGER NOT NULL CHECK(is_group IN (0,1)),
            reply_to TEXT NOT NULL,
            body TEXT NOT NULL,
            origin TEXT NOT NULL CHECK(origin IN ('owner_card','bot_human','bot_routine','owner_direct')),
            state TEXT NOT NULL CHECK(state IN ('pending','held','sent','uncertain','expired','cancelled')),
            reason TEXT,
            not_before INTEGER NOT NULL,
            attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts>=0),
            lease_until INTEGER,
            last_attempt_at INTEGER,
            sent_at INTEGER,
            sent_id TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            expires_at INTEGER NOT NULL
        ) STRICT;
        CREATE INDEX donna_whatsapp_outbox_due ON donna_whatsapp_outbox(state,not_before);
        CREATE INDEX donna_whatsapp_outbox_bot ON donna_whatsapp_outbox(bot_id,state);
        INSERT INTO donna_whatsapp_outbox(id,bot_id,account_id,chat_id,is_group,reply_to,body,origin,state,
            not_before,created_at,updated_at,expires_at)
        VALUES('i1','b','a','c',0,'m','oi','owner_card','pending',0,1,1,9999999999999);",
    )
    .unwrap();
    super::migrate(&c).unwrap();
    // 'sending' passa no CHECK novo, a linha antiga sobreviveu e os índices voltaram.
    c.execute("UPDATE donna_whatsapp_outbox SET state='sending' WHERE id='i1'", []).unwrap();
    let state: String = c.query_row("SELECT state FROM donna_whatsapp_outbox WHERE id='i1'", [], |r| r.get(0)).unwrap();
    assert_eq!(state, "sending");
    let indexes: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index'
             AND name IN ('donna_whatsapp_outbox_due','donna_whatsapp_outbox_bot')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(indexes, 2);
    // Rodar de novo não refaz nada.
    super::migrate(&c).unwrap();
    assert!(c.execute("UPDATE donna_whatsapp_outbox SET state='bogus' WHERE id='i1'", []).is_err());
}

#[tokio::test]
async fn claim_runs_durable_and_restores_normal_sync() {
    let f = ready(json!({"readChats":true,"replyChats":true,"delaySeconds":8})).await;
    wrote(&f, ANA, "M1");
    card(&f, ANA, "M1", "olá");
    wait_sends(&f, 1).await;
    // O claim 'sending' foi gravado com synchronous=FULL (fsync) antes do wacli.
    assert_eq!(outbox::CLAIM_SYNCHRONOUS.load(std::sync::atomic::Ordering::Relaxed), 2);
    // Depois do commit a conexão volta a NORMAL — o FULL não vaza para o resto do banco.
    let sync = {
        let db = f.hub.store.db.locked();
        db.pragma_query_value(None, "synchronous", |r| r.get::<_, i64>(0)).unwrap()
    };
    assert_eq!(sync, 1);
    f.done().await;
}

#[tokio::test]
async fn claim_preserves_a_connection_already_at_full() {
    let f = ready(json!({"readChats":true,"replyChats":true,"delaySeconds":8})).await;
    // Conexão já em FULL (bootstrap/D430): o claim não pode rebaixar para NORMAL.
    sql(&f, "PRAGMA synchronous=FULL");
    wrote(&f, ANA, "M1");
    card(&f, ANA, "M1", "olá");
    wait_sends(&f, 1).await;
    assert_eq!(outbox::CLAIM_SYNCHRONOUS.load(std::sync::atomic::Ordering::Relaxed), 2);
    let sync = {
        let db = f.hub.store.db.locked();
        db.pragma_query_value(None, "synchronous", |r| r.get::<_, i64>(0)).unwrap()
    };
    assert_eq!(sync, 2);
    f.done().await;
}

#[tokio::test]
async fn marked_group_requires_explicit_autonomous_permission() {
    let f = ready(json!({"readGroups":true,"replyGroups":true,"replyMode":"autonomous","delaySeconds":8})).await;
    let group = "12345@g.us";
    let remote = crate::api::devices::Caller::Pairing { key: "pairing".into() };
    assert!(
        super::api::call(
            &f.hub,
            &remote,
            "donnaWhatsappGroupAuto",
            json!({"botId":DONNA,"whatsappChatId":group,"auto":true})
        )
        .await
        .is_err()
    );
    wrote(&f, group, "M1");
    f.call("donnaWhatsappChatMark", json!({"botId":DONNA,"whatsappChatId":group,"marked":true})).await.unwrap();
    let req = Enqueue { bot: DONNA, chat: group, reply_to: "M1", text: "oi", origin: Origin::BotRoutine };
    assert!(outbox::enqueue(&f.hub, &req, now_ms()).is_err());
    assert_eq!(rows(&f).len(), 0);
    assert_eq!(f.lines("sends"), 0);
    f.call("donnaWhatsappGroupAuto", json!({"botId":DONNA,"whatsappChatId":group,"auto":true})).await.unwrap();
    assert_eq!(outbox::enqueue(&f.hub, &req, now_ms()).unwrap(), Enqueued::Inserted);
    wait_sends(&f, 1).await;
    f.done().await;
}

#[tokio::test]
async fn owner_card_sends_to_group_with_autonomous_disabled() {
    let f = ready(json!({"readGroups":true,"replyGroups":true,"replyMode":"autonomous","delaySeconds":8})).await;
    let group = "12345@g.us";
    wrote(&f, group, "M1");
    f.call("donnaWhatsappChatMark", json!({"botId":DONNA,"whatsappChatId":group,"marked":true})).await.unwrap();
    assert_eq!(card(&f, group, "M1", "aprovado"), Enqueued::Inserted);
    wait_sends(&f, 1).await;
    wait_state(&f, "sent").await;
    f.done().await;
}

#[tokio::test]
async fn disabling_group_auto_cancels_held_reply_and_remarking_does_not_enable_it() {
    let f = quiet_ready(json!({"readGroups":true,"replyGroups":true,"replyMode":"autonomous"})).await;
    let group = "12345@g.us";
    wrote(&f, group, "M1");
    f.call("donnaWhatsappChatMark", json!({"botId":DONNA,"whatsappChatId":group,"marked":true})).await.unwrap();
    f.call("donnaWhatsappGroupAuto", json!({"botId":DONNA,"whatsappChatId":group,"auto":true})).await.unwrap();
    enqueue(&f, group, "M1", "oi", Origin::BotRoutine);
    wait_reason(&f, "held", "quiet").await;
    f.call("donnaWhatsappGroupAuto", json!({"botId":DONNA,"whatsappChatId":group,"auto":false})).await.unwrap();
    wait_reason(&f, "cancelled", "not_allowed").await;
    f.call("donnaWhatsappChatMark", json!({"botId":DONNA,"whatsappChatId":group,"marked":false})).await.unwrap();
    f.call("donnaWhatsappChatMark", json!({"botId":DONNA,"whatsappChatId":group,"marked":true})).await.unwrap();
    let req = Enqueue { bot: DONNA, chat: group, reply_to: "M1", text: "outro", origin: Origin::BotRoutine };
    assert!(outbox::enqueue(&f.hub, &req, now_ms()).is_err());
    assert_eq!(f.lines("sends"), 0);
    f.done().await;
}

#[tokio::test]
async fn drain_rechecks_group_permission_before_sending() {
    let f = quiet_ready(json!({"readGroups":true,"replyGroups":true,"replyMode":"autonomous"})).await;
    let group = "12345@g.us";
    wrote(&f, group, "M1");
    f.call("donnaWhatsappChatMark", json!({"botId":DONNA,"whatsappChatId":group,"marked":true})).await.unwrap();
    f.call("donnaWhatsappGroupAuto", json!({"botId":DONNA,"whatsappChatId":group,"auto":true})).await.unwrap();
    enqueue(&f, group, "M1", "oi", Origin::BotRoutine);
    wait_reason(&f, "held", "quiet").await;
    // Simula uma revogação depois da entrada, sem o cancelamento antecipado da RPC.
    sql(&f, "UPDATE donna_whatsapp_marked SET auto=0 WHERE chat_id='12345@g.us'");
    sql(&f, "UPDATE donna_whatsapp_outbox SET not_before=0 WHERE state='held'");
    wake(&f);
    wait_reason(&f, "cancelled", "not_allowed").await;
    assert_eq!(f.lines("sends"), 0);
    f.done().await;
}

#[test]
fn marked_group_migration_is_idempotent_and_fails_closed() {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch("CREATE TABLE donna_whatsapp_marked(bot_id TEXT NOT NULL, chat_id TEXT NOT NULL, account_id TEXT NOT NULL, marked_at INTEGER NOT NULL, PRIMARY KEY(bot_id,chat_id)) STRICT; INSERT INTO donna_whatsapp_marked VALUES('bot','123@g.us','account',1);").unwrap();
    super::migrate(&c).unwrap();
    super::migrate(&c).unwrap();
    assert!(!super::store::group_auto(&c, "bot", "123@g.us", "account").unwrap());
    assert!(super::store::chat_marked(&c, "bot", "123@g.us", "account").unwrap());
}
