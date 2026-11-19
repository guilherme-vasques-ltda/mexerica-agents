//! D406: fluxo do cartão real e a fronteira entre o turno da pergunta e o da resposta.
use super::*;
use crate::fork::owner_question;

fn ask(f: &Fixture) -> crate::store::Entry {
    owner_question::ask_on(&f.hub.store.db.locked(), "bot", &Lane::main("bot"), 1, "Onde você mora?").unwrap().2
}

fn checkpoint(f: &Fixture) {
    queue::checkpoint(&f.hub.store, "completion", r#"{"completion":{"turn":1}}"#, Some(&f.job), true, false).unwrap();
}

fn status(f: &Fixture) -> String {
    f.hub
        .store
        .db
        .locked()
        .query_row("SELECT status FROM donna_memory_auto_jobs WHERE id=?1", [&f.job], |r| r.get(0))
        .unwrap()
}

fn assert_original_closed(f: &Fixture, expected: &str) {
    assert_eq!(status(f), expected);
    assert!(queue::reserve_job(&f.hub.store, &f.job).unwrap().is_none());
    assert!(!f.hub.store.memory_turn_eligible("bot", "bot", 1).unwrap());
    let revision = f.hub.store.entry(&f.message).unwrap().rev;
    let db = f.hub.store.db.locked();
    assert!(!queue::eligible(&db, "bot", "bot", &f.message, revision).unwrap());
    assert_eq!(mem::layer(&db, Some("bot")).unwrap().len(), 0);
    let scope_revision: i64 =
        db.query_row("SELECT scope_revision FROM donna_memory_auto_jobs WHERE id=?1", [&f.job], |r| r.get(0)).unwrap();
    assert!(!queue::set_scope(&db, &f.job, scope_revision, true).unwrap());
}

fn answer(f: &Fixture) -> crate::store::Entry {
    let text = "Moro em Recife.";
    owner_question::add_human_message(
        &f.hub,
        &Lane::main("bot"),
        2,
        &json!({"text":text,"status":"queued",
            "contentOrigin":{"status":"appReported","report":{"channel":"desktopComposer"},
            "segments":[{"kind":"typed","start":0,"end":text.encode_utf16().count()}]}}),
    )
    .unwrap()
}

/// Mesma admissão usada por agent/bot/turn.rs; não reaproveita o token da pergunta.
fn answer_turn(f: &Fixture, entry: &crate::store::Entry) -> queue::Reservation {
    f.hub.set_runtime("bot", |r| {
        r.message = Some(entry.id.clone());
        r.memory_turn_token = Some(uuid::Uuid::new_v4().to_string());
    });
    f.hub.store.memory_session_turn("bot", "bot", "session", 2).unwrap();
    f.hub.store.memory_turn_begin("bot", "bot", "session", &[2], "owner").unwrap();
    queue::capture_engine(&f.hub, &f.cfg, &Lane::main("bot"), &f.cfg.backend).unwrap();
    queue::begin(&f.hub, &f.cfg, &Lane::main("bot"), "session", std::slice::from_ref(&entry.id)).unwrap();
    let job = queue::active_job(&f.hub, "bot").unwrap().unwrap();
    assert_ne!(job, f.job);
    let reply =
        f.hub.add_entry(&Lane::main("bot"), EntryKind::Agent, 2, &json!({"text":"Entendi.","author":"bot"})).unwrap();
    queue::checkpoint(
        &f.hub.store,
        "completion",
        &json!({"completion":{"turn":2,"last_text":reply.id}}).to_string(),
        Some(&job),
        true,
        false,
    )
    .unwrap();
    queue::reserve_job(&f.hub.store, &job).unwrap().unwrap()
}

#[test]
fn d406_sem_resposta_espera_dono_sem_gravar_nem_expirar() {
    let f = fixture(false);
    let card = ask(&f);
    checkpoint(&f);
    owner_question::sweep(&f.hub);
    assert_original_closed(&f, "owner");
    let db = f.hub.store.db.locked();
    queue::expire(&db, crate::store::now_ms() + 2 * queue::LIFE_MS).unwrap();
    let saved: String =
        db.query_row("SELECT context FROM donna_memory_auto_jobs WHERE id=?1", [&f.job], |r| r.get(0)).unwrap();
    let context: queue::Context = serde_json::from_str(&saved).unwrap();
    assert_eq!(context.owner_question.as_deref(), Some(card.id.as_str()));
    assert!(!context.contains(&card.id));
    drop(db);
    assert_eq!(status(&f), "owner");
    assert!(queue::reserve(&f.hub.store, None).unwrap().is_none());
}

#[test]
fn d406_resposta_antes_ou_depois_checkpoint_so_alimenta_o_proprio_turno() {
    for before in [true, false] {
        let f = fixture(false);
        let card = ask(&f);
        if !before {
            checkpoint(&f);
            assert_eq!(status(&f), "owner");
            // Mesmo depois de 30 dias, a resposta terá uma admissão nova no próprio turno.
            f.hub
                .store
                .db
                .locked()
                .execute("UPDATE donna_memory_auto_jobs SET admitted_at=0 WHERE id=?1", [&f.job])
                .unwrap();
        }
        let response = answer(&f);
        if before {
            assert_eq!(status(&f), "running");
            checkpoint(&f);
        }
        assert_eq!(f.hub.store.entry(&card.id).unwrap().data["ownerQuestion"]["status"], "answered");
        assert_original_closed(&f, "ineligible");
        let r = answer_turn(&f, &response);
        let input = worker::prepare(&f.hub, &r, |_, _| Ok(())).unwrap();
        assert_eq!(r.context.messages.len(), 1);
        assert_eq!(r.context.messages[0].id, response.id);
        assert_eq!(input.messages.len(), 2);
        assert_eq!(input.messages[0].text, "Moro em Recife.");
        assert!(input.messages[0].from_user);
        assert!(!input.messages[1].from_user);
        assert!(input.messages.iter().all(|m| m.id != card.id && m.id != f.message));
        // Evidência do turno original não serve para gravar no trabalho da resposta.
        assert!(apply::complete(&f.hub, &r, plan(remember(&f, "Mora em Curitiba")), |_, _| Ok(())).is_err());
        let op = Operation::Remember {
            text: "Moro em Recife.".into(),
            shared: false,
            evidence: vec![response.id],
            nature: None,
        };
        apply::complete(&f.hub, &r, plan(op), |_, _| Ok(())).unwrap();
        let db = f.hub.store.db.locked();
        let saved = mem::layer(&db, Some("bot")).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].text, "Moro em Recife.");
    }
}

#[test]
fn d406_retirada_antes_ou_depois_checkpoint_descarta_original() {
    for before in [true, false] {
        let f = fixture(false);
        ask(&f);
        if !before {
            checkpoint(&f);
        }
        owner_question::withdraw_chat(&f.hub, "bot", None);
        if before {
            assert_eq!(status(&f), "running");
            checkpoint(&f);
        }
        assert_original_closed(&f, "ineligible");
    }
}

#[tokio::test]
async fn d406_bot_apagado_reinicio_sweep_nao_deixa_trabalho_preso() {
    for channel in [false, true] {
        let bot = format!("d406-delete-{}", uuid::Uuid::new_v4());
        let f = fixture_for_bot(false, &bot);
        let chat = if channel {
            let chat = uuid::Uuid::new_v4().to_string();
            let cfg =
                serde_json::from_value(json!({"id":chat,"kind":"group","name":"Canal D406","members":[bot]})).unwrap();
            f.hub.store.save_bot(&cfg).unwrap();
            // Fronteira de recuperação: vínculo persistido de um trabalho do bot no canal.
            let db = f.hub.store.db.locked();
            db.execute("UPDATE entries SET bot_id=?2 WHERE id=?1", params![f.message, chat]).unwrap();
            db.execute(
                "UPDATE donna_memory_auto_jobs SET chat=?2,
                 context=json_set(context,'$.chatId',?2,'$.visibleSession',?2) WHERE id=?1",
                params![f.job, chat],
            )
            .unwrap();
            chat
        } else {
            bot.clone()
        };
        let lane = Lane::main(&chat);
        let card = owner_question::ask_on(&f.hub.store.db.locked(), &bot, &lane, 1, "Pode seguir?").unwrap().2;
        checkpoint(&f);
        assert_eq!(status(&f), "owner");
        f.hub.delete_bot(&chat).unwrap();
        assert!(f.hub.store.bot(&chat).unwrap().unwrap().deleted);
        assert!(f.hub.store.entry(&card.id).is_none());
        assert!(f.hub.store.entry(&f.message).is_none());
        queue::migrate(&f.hub.store.db.locked()).unwrap();
        owner_question::sweep(&f.hub);
        owner_question::sweep(&f.hub);
        assert_eq!(status(&f), "ineligible", "canal={channel}");
        assert!(queue::reserve_job(&f.hub.store, &f.job).unwrap().is_none());
        assert_eq!(f.hub.store.memory_read(|c| mem::layer(c, Some(&bot))).unwrap().len(), 0);
    }
}

#[test]
fn d406_resposta_e_descarte_revertem_na_mesma_transacao() {
    let f = fixture(false);
    let card = ask(&f);
    checkpoint(&f);
    f.hub
        .store
        .db
        .locked()
        .execute_batch(
            "CREATE TRIGGER fail_owner_close BEFORE UPDATE OF status ON donna_memory_auto_jobs
        WHEN OLD.status='owner' BEGIN SELECT RAISE(ABORT,'fixture'); END;",
        )
        .unwrap();
    assert!(owner_question::add_human_message(&f.hub, &Lane::main("bot"), 2, &json!({"text":"Resposta"})).is_none());
    assert_eq!(status(&f), "owner");
    assert_eq!(f.hub.store.entry(&card.id).unwrap().data["ownerQuestion"]["status"], "pending");
    assert_eq!(
        f.hub
            .store
            .db
            .locked()
            .query_row("SELECT COUNT(*) FROM entries WHERE kind='user'", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn d406_sem_cartao_ou_cartao_de_outro_turno_sessao_autor_nao_muda_fluxo() {
    for (bot, lane, turn) in [
        ("bot", Lane::main("bot"), 9),
        ("bot", Lane { chat: "bot".into(), session: Some("outra".into()) }, 1),
        ("outro", Lane::main("bot"), 1),
    ] {
        let f = fixture(false);
        owner_question::ask_on(&f.hub.store.db.locked(), bot, &lane, turn, "Outra pergunta?").unwrap();
        let r = success(&f);
        assert_eq!(r.context.messages[0].id, f.message);
        assert!(r.context.owner_question.is_none());
        assert_eq!(worker::prepare(&f.hub, &r, |_, _| Ok(())).unwrap().messages[0].id, f.message);
    }
    let f = fixture(false);
    let r = success(&f);
    assert_eq!(r.context.messages[0].id, f.message);
}

fn suggestions(f: &Fixture) -> i64 {
    f.hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_memory_suggestions", [], |r| r.get(0)).unwrap()
}

/// D389 × D406: no modo sugestão, o turno que espera o dono (`owner`, escopo bloqueado) não gera sugestão,
/// nem por uma reserva forjada no commit; só o trabalho da resposta, com escopo permitido, gera.
#[test]
fn d389_turno_owner_nao_gera_sugestao_e_a_resposta_gera() {
    let f = fixture(false);
    f.hub.store.kv_set(crate::fork::memory_learning::KEY, "suggest").unwrap();
    ask(&f);
    checkpoint(&f);
    assert_eq!(status(&f), "owner");
    assert!(queue::reserve(&f.hub.store, None).unwrap().is_none());
    let (raw, scope_revision): (String, i64) = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT context,scope_revision FROM donna_memory_auto_jobs WHERE id=?1", [&f.job], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    let forged = queue::Reservation {
        id: f.job.clone(),
        token: "forjada".into(),
        scope_revision,
        context: serde_json::from_str(&raw).unwrap(),
    };
    assert!(apply::complete(&f.hub, &forged, plan(remember(&f, "Mora em Curitiba")), |_, _| Ok(())).is_err());
    assert_eq!(suggestions(&f), 0);
    let response = answer(&f);
    let r = answer_turn(&f, &response);
    let op = Operation::Remember {
        text: "Moro em Recife.".into(),
        shared: false,
        evidence: vec![response.id],
        nature: None,
    };
    assert_eq!(apply::complete(&f.hub, &r, plan(op), |_, _| Ok(())).unwrap().len(), 0);
    assert_eq!(suggestions(&f), 1, "a resposta do dono vira sugestão pendente");
    assert_eq!(f.hub.store.memory_read(|c| mem::layer(c, Some("bot"))).unwrap().len(), 0);
}
