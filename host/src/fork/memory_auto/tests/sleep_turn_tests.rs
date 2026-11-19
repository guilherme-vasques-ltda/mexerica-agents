//! C7 por turno (`sleep_checks::turn_valid_for_cleanup`): a resposta do bot só sai com o turno inteiro provado.
use super::*;
use crate::fork::memory_auto::{sleep, sleep_checks};

fn later() -> i64 {
    crate::store::now_ms() + sleep::IDLE_MS + 60_000
}

/// Turno 1 com M (dono, fato literal aplicado) e a resposta A do bot. O turno encerra e o sono cobre M.
fn covered_turn(f: &Fixture) -> String {
    covered_turn_with_cards(f).0
}

/// O mesmo, devolvendo também os ids que o produtor (`apply::complete`) devolveu: os cartões "Memória salva".
fn covered_turn_with_cards(f: &Fixture) -> (String, Vec<String>) {
    let text = "Moro em Curitiba. Prefiro café sem açúcar.";
    let data = json!({"text":text,"contentOrigin":{"status":"appReported","report":{"channel":"desktopComposer"},
        "segments":[{"kind":"typed","start":0,"end":text.encode_utf16().count()}]}});
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE entries SET data=?2 WHERE id=?1", params![f.message, data.to_string()])
        .unwrap();
    let reply = f.hub.add_entry(&Lane::main("bot"), EntryKind::Agent, 1, &json!({"text":"Anotado.","author":"bot"}));
    let reply = reply.unwrap().id;
    let r = success(f);
    let cards = apply::complete(&f.hub, &r, plan(remember(f, "Moro em Curitiba")), |_, _| Ok(())).unwrap();
    f.hub.store.memory_turn_finish("bot", "session").unwrap();
    sleep::try_tick(&f.hub, "bot", later()).unwrap();
    assert!(sleep_checks::valid_for_cleanup(&f.hub.store.db.locked(), "bot", "bot", &f.message).unwrap());
    (reply, cards)
}

fn out(f: &Fixture, id: &str) -> bool {
    sleep_checks::turn_valid_for_cleanup(&f.hub.store.db.locked(), id).unwrap()
}

#[test]
fn turno_completo_coberto_libera_a_resposta() {
    let f = fixture(false);
    let reply = covered_turn(&f);
    assert!(out(&f, &reply));
    assert!(!out(&f, &f.message), "a mensagem do dono é decidida por valid_for_cleanup");
}

#[test]
fn mensagem_do_dono_sem_prova_ou_turno_vivo_segura_a_resposta() {
    let f = fixture(false);
    let reply =
        f.hub.add_entry(&Lane::main("bot"), EntryKind::Agent, 1, &json!({"text":"Oi","author":"bot"})).unwrap().id;
    assert!(!out(&f, &reply), "M sem prova da C7 (o sono não rodou)");
    let f = fixture(false);
    let reply = covered_turn(&f);
    f.hub
        .store
        .db
        .locked()
        .execute("INSERT INTO donna_memory_unsettled(bot,session) VALUES('bot','session')", [])
        .unwrap();
    assert!(!out(&f, &reply), "turno vivo");
}

#[test]
fn resposta_citada_num_recibo_ou_na_linhagem_nao_sai() {
    let f = fixture(false);
    let reply = covered_turn(&f);
    let card = json!({"text":"Memória salva","style":"info","author":"bot","memory":{"responseIds":[reply]}});
    let notice = f.hub.add_entry(&Lane::main("bot"), EntryKind::Notice, 1, &card).unwrap().id;
    assert!(!out(&f, &reply), "responseIds de um recibo");
    f.hub.store.db.locked().execute("DELETE FROM entries WHERE id=?1", [&notice]).unwrap();
    assert!(out(&f, &reply));
    let fact = f.hub.store.memory_read(|c| mem::layer(c, Some("bot"))).unwrap().remove(0);
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE memory_entries SET evidence=evidence||','||?2 WHERE id=?1", params![fact.id, reply])
        .unwrap();
    assert!(!out(&f, &reply), "evidência e linhagem");
}

#[test]
fn turno_sem_mensagem_do_dono_nao_sai() {
    let f = fixture(false);
    let _ = covered_turn(&f);
    let alone =
        f.hub.add_entry(&Lane::main("bot"), EntryKind::Agent, 7, &json!({"text":"Rotina","author":"bot"})).unwrap().id;
    assert!(!out(&f, &alone));
}

#[test]
fn trabalho_aberto_ou_pergunta_pendente_segura_a_resposta() {
    let f = fixture(false);
    let reply = covered_turn(&f);
    let context = json!({"botId":"bot","chatId":"bot","spaceId":"personal","memoryId":"bot","botRevision":1,
        "conversationRevision":null,"session":"session","token":"t2","messages":[],
        "responses":[{"id":reply,"hash":"x"}]});
    f.hub
        .store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_memory_auto_jobs(id,bot,chat,space,context,outcome,status) VALUES('j2','bot','bot','personal',?1,'success','waiting')",
            [context.to_string()],
        )
        .unwrap();
    assert!(!out(&f, &reply), "trabalho aberto cita a resposta");
    f.hub.store.db.locked().execute("UPDATE donna_memory_auto_jobs SET status='applied' WHERE id='j2'", []).unwrap();
    assert!(out(&f, &reply));
    let card = json!({"text":"Onde você mora?","card":"ownerQuestion","ownerQuestion":{"status":"pending"}});
    f.hub.add_entry(&Lane::main("bot"), EntryKind::Notice, 1, &card).unwrap();
    assert!(!out(&f, &reply), "pergunta D406 pendente");
}

/// R1-518-01 = SEG-518-01: o cartão "Memória salva" (o id que o `complete` devolve é o do recibo durável) não sai;
/// a resposta citada nos `responseIds` dele fica enquanto ele existir; a leitura e o Desfazer seguem funcionando.
/// Cartões legados de fato e de nota também ficam.
#[test]
fn cartao_de_memoria_com_recibo_nao_sai_e_protege_a_resposta() {
    let f = fixture(false);
    let (reply, cards) = covered_turn_with_cards(&f);
    let card = cards[0].clone();
    let durable: bool = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT EXISTS(SELECT 1 FROM donna_memory_auto_receipts WHERE id=?1)", [&card], |r| r.get(0))
        .unwrap();
    assert!(durable, "o id devolvido pelo produtor é o do recibo");
    assert!(!out(&f, &card), "cartão com recibo durável não sai");
    let mut data = f.hub.store.entry(&card).unwrap().data;
    data["memory"]["responseIds"] = json!([reply]);
    f.hub.store.db.locked().execute("UPDATE entries SET data=?2 WHERE id=?1", params![card, data.to_string()]).unwrap();
    assert!(!out(&f, &reply), "A citada no cartão fica enquanto ele existir");
    assert_eq!(f.hub.store.entry(&card).unwrap().data["memory"]["status"], "done", "a leitura segue");
    let undone = apply::undo(&f.hub, &card, &crate::fork::autoridade::Autoridade::local()).unwrap();
    assert_eq!(undone["entry"]["data"]["memory"]["status"], "undone", "o Desfazer segue");
    for legacy in
        [json!({"text":"Lembrei","memory":{"status":"done"}}), json!({"text":"Nota","memoryFile":{"status":"done"}})]
    {
        let id = f.hub.add_entry(&Lane::main("bot"), EntryKind::Notice, 1, &legacy).unwrap().id;
        assert!(!out(&f, &id), "cartão legado: {legacy}");
    }
}
