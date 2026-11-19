//! CF-27: leitor e handlers produtivos; apenas wacli/cleanup são adaptadores locais.
use super::*;

fn cleanup_duas_conversas(f: &Fixture) {
    let script = format!(
        r"import sqlite3, sys
assert sys.argv[1] == {dir:?}
assert sys.argv[2] == '--jid' and sys.argv[3] in ({ana:?}, {bia:?})
assert sys.argv[4:] == ['--confirm']
c = sqlite3.connect(sys.argv[1] + '/wacli.db')
c.execute('DELETE FROM messages WHERE chat_jid=?', (sys.argv[3],))
c.execute('DELETE FROM chats WHERE jid=?', (sys.argv[3],))
c.commit()
c.close()
",
        dir = f.dir(DONNA).to_str().unwrap(),
        ana = ANA,
        bia = BIA,
    );
    std::fs::write(f.temp.join("fake/cleanup.py"), script).unwrap();
    let binary = f.temp.join("fake/wacli");
    let original = std::fs::read_to_string(&binary).unwrap();
    let needle = "shift 2; echo \"$*\" >> \"$here/cleanups\"; exit 0";
    assert_eq!(original.matches(needle).count(), 1);
    std::fs::write(
        &binary,
        original.replace(
            needle,
            "shift 2; echo \"$*\" >> \"$here/cleanups\"; python3 \"$here/cleanup.py\" \"$store\" \"$@\"; exit $?",
        ),
    )
    .unwrap();
}

fn apagar(f: &Fixture, chat: &'static str) -> tokio::task::JoinHandle<anyhow::Result<Value>> {
    let hub = f.hub.clone();
    tokio::spawn(async move {
        crate::api::dispatch(
            &hub,
            &crate::api::devices::Caller::Local,
            super::super::api::CHAT_ERASE,
            json!({"hubId":HUB_ID,"botId":DONNA,"whatsappChatId":chat}),
        )
        .await
    })
}

#[tokio::test]
async fn apagar_conversa_drena_lote_capturado_sem_restaurar_mensagem() {
    let f = ready(json!({"readChats":true}), Duration::from_secs(3600)).await;
    // Preparo: "outro bot" exige o modo por bot (o padrão libera só o bot principal).
    f.call("donnaWhatsappSave", json!({"enabled":true,"mode":"per_bot"})).await.unwrap();
    f.until(DONNA, |n| n["state"] == "connected").await;
    f.call("donnaWhatsappBotSave", json!({"botId":LIA,"settings":{"readChats":true}})).await.unwrap();
    f.connect(LIA).await;
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_whatsapp_numbers SET linked_at=?1", [crate::store::now_ms() - 3_600_000])
        .unwrap();
    let c = wacli_db(&f);
    insert(&c, &Msg { id: "OLD", ..Msg::default() });
    insert(&c, &Msg { chat: BIA, sender: BIA, id: "OTHER_CHAT", ..Msg::default() });
    pass(&f);
    let other = Connection::open(f.dir(LIA).join("wacli.db")).unwrap();
    other.execute_batch(include_str!("wacli_fixture.sql")).unwrap();
    insert(&other, &Msg { id: "OTHER_BOT", ..Msg::default() });
    inbox::read(&f.hub, LIA, &f.dir(LIA)).unwrap();
    assert_eq!(stored(&f, DONNA), 2);
    assert_eq!(stored(&f, LIA), 1);
    let account: String = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT account_id FROM donna_whatsapp_numbers WHERE bot_id=?1", [DONNA], |r| r.get(0))
        .unwrap();
    // O comando fake já exige que seu próprio sync tenha saído. Este adaptador
    // acrescenta limpeza SQLite real, restrita à pasta e à conversa desta fixture.
    let script = format!(
        r"import sqlite3, sys
assert sys.argv[1:] == [{dir:?}, '--jid', {chat:?}, '--confirm']
c = sqlite3.connect(sys.argv[1] + '/wacli.db')
c.execute('DELETE FROM messages WHERE chat_jid=?', (sys.argv[3],))
c.execute('DELETE FROM chats WHERE jid=?', (sys.argv[3],))
c.commit()
c.close()
",
        dir = f.dir(DONNA).to_str().unwrap(),
        chat = ANA
    );
    std::fs::write(f.temp.join("fake/cleanup.py"), script).unwrap();
    let binary = f.temp.join("fake/wacli");
    let original = std::fs::read_to_string(&binary).unwrap();
    let needle = "shift 2; echo \"$*\" >> \"$here/cleanups\"; exit 0";
    assert_eq!(original.matches(needle).count(), 1);
    std::fs::write(
        &binary,
        original.replace(
            needle,
            "shift 2; echo \"$*\" >> \"$here/cleanups\"; python3 \"$here/cleanup.py\" \"$store\" \"$@\"; exit $?",
        ),
    )
    .unwrap();
    let (_, secret) = hook(&f).await;
    let (captured, resume) = f.hub.whatsapp.inbox().pause_after_fetch(DONNA);
    insert(&c, &Msg { id: "INFLIGHT", text: "lote anterior à exclusão", ..Msg::default() });
    insert(&c, &Msg { chat: BIA, id: "CURSOR_KEEP", from_me: true, ..Msg::default() });
    assert!(f.hub.whatsapp.inbox().accept(
        "127.0.0.1:12345".parse().unwrap(),
        DONNA,
        Some(&sign(&secret, b"{}")),
        b"{}",
    ));
    tokio::time::timeout(Duration::from_secs(2), captured.notified()).await.unwrap();
    assert!(!shown(&f).iter().any(|m| m["messageId"] == "INFLIGHT"));
    let hub = f.hub.clone();
    let erase = tokio::spawn(async move {
        crate::api::dispatch(
            &hub,
            &crate::api::devices::Caller::Local,
            super::super::api::CHAT_ERASE,
            json!({"hubId":HUB_ID,"botId":DONNA,"whatsappChatId":ANA}),
        )
        .await
    });
    // A espera observada é do supervisor real, retido pelo mesmo leitor pós-fetch.
    tokio::time::timeout(Duration::from_secs(2), async {
        while !f.hub.whatsapp.erase_waiting(DONNA) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(!erase.is_finished());
    assert!(!shown(&f).iter().any(|m| m["chatId"] == ANA), "DELETE confirmado antes de liberar o lote");
    assert_eq!(f.lines("cleanups"), 0, "cleanup aguarda o leitor");
    resume.send(()).unwrap();
    let out = tokio::time::timeout(Duration::from_secs(5), erase).await.unwrap().unwrap().unwrap();
    assert_eq!(out["wacli"], "done", "erro/timeout de cleanup não confirma CF-27");
    let left: i64 = c.query_row("SELECT COUNT(*) FROM messages WHERE chat_jid=?1", [ANA], |r| r.get(0)).unwrap();
    assert_eq!(left, 0, "cleanup realmente apagou a cópia SQLite");
    assert_eq!(f.lines("cleanups"), 1);
    let page = api(&f, "donnaWhatsappMessages", json!({"botId":DONNA,"whatsappChatId":ANA})).await.unwrap();
    let restored = page["messages"].as_array().unwrap().iter().any(|m| m["messageId"] == "INFLIGHT");
    let other_chat = api(&f, "donnaWhatsappMessages", json!({"botId":DONNA,"whatsappChatId":BIA})).await.unwrap();
    assert_eq!(other_chat["messages"][0]["messageId"], "OTHER_CHAT");
    let other_bot = api(&f, "donnaWhatsappMessages", json!({"botId":LIA,"whatsappChatId":ANA})).await.unwrap();
    assert_eq!(other_bot["messages"][0]["messageId"], "OTHER_BOT");
    let current: String = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT account_id FROM donna_whatsapp_numbers WHERE bot_id=?1", [DONNA], |r| r.get(0))
        .unwrap();
    assert_eq!(current, account, "a mesma conta continua válida");
    f.until(DONNA, |n| n["state"] == "connected").await;
    // Evita reutilizar rowid removido pelo cleanup; o cursor produtivo continua válido.
    insert(&c, &Msg { id: "NEW", ts: now() + 1, ..Msg::default() });
    pass(&f);
    let positive = api(&f, "donnaWhatsappMessages", json!({"botId":DONNA,"whatsappChatId":ANA})).await.unwrap();
    assert!(
        positive["messages"].as_array().unwrap().iter().any(|m| m["messageId"] == "NEW"),
        "mensagem legítima nova deve continuar aceita"
    );
    f.done().await;
    assert!(!restored, "CF-27: lote capturado restaurou INFLIGHT após wacli=done e cleanup confirmado");
}

#[tokio::test]
async fn apagar_outra_conversa_aguarda_apagar_em_curso_e_reapaga_seu_lote() {
    let f = ready(json!({"readChats":true}), Duration::from_secs(3600)).await;
    cleanup_duas_conversas(&f);
    let c = wacli_db(&f);
    insert(&c, &Msg { id: "OLD_A", ..Msg::default() });
    insert(&c, &Msg { chat: BIA, sender: BIA, id: "OLD_B", ..Msg::default() });
    pass(&f);
    assert_eq!(shown(&f).len(), 2);
    let (_, secret) = hook(&f).await;
    let (captured, resume) = f.hub.whatsapp.inbox().pause_after_fetch(DONNA);
    insert(&c, &Msg { id: "INFLIGHT_A", ..Msg::default() });
    insert(&c, &Msg { chat: BIA, sender: BIA, id: "INFLIGHT_B", ..Msg::default() });
    assert!(f.hub.whatsapp.inbox().accept(
        "127.0.0.1:12345".parse().unwrap(),
        DONNA,
        Some(&sign(&secret, b"{}")),
        b"{}",
    ));
    tokio::time::timeout(Duration::from_secs(2), captured.notified()).await.unwrap();
    let first = apagar(&f, ANA);
    tokio::time::timeout(Duration::from_secs(2), async {
        while !f.hub.whatsapp.erase_waiting(DONNA) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let second = apagar(&f, BIA);
    tokio::time::timeout(Duration::from_secs(2), async {
        while shown(&f).iter().any(|m| m["chatId"] == BIA) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    // A exclusão inicial de B prova que o segundo handler já chegou ao hold.
    let second_finished_before_drain = second.is_finished();
    let cleanup_before_drain = f.lines("cleanups");
    resume.send(()).unwrap();
    let out_a = tokio::time::timeout(Duration::from_secs(5), first).await.unwrap().unwrap().unwrap();
    let out_b = tokio::time::timeout(Duration::from_secs(5), second).await.unwrap().unwrap().unwrap();
    let restored = shown(&f).iter().any(|m| m["chatId"] == ANA || m["chatId"] == BIA);
    let left: i64 = c.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0)).unwrap();
    let cleanups = f.lines("cleanups");
    f.done().await;
    assert!(!second_finished_before_drain, "segundo apagar recusou sem aguardar o leitor do mesmo bot");
    assert_eq!(cleanup_before_drain, 0);
    assert_eq!(out_a["wacli"], "done");
    assert_eq!(out_b["wacli"], "done");
    assert!(!restored, "o lote retido restaurou uma das duas conversas apagadas");
    assert_eq!(left, 0);
    assert_eq!(cleanups, 2);
}

#[tokio::test]
async fn apagar_timeout_nao_limpa_copia_e_avisa_exclusao_nao_confirmada() {
    let f = ready(json!({"readChats":true}), Duration::from_secs(3600)).await;
    // O mesmo cleanup do teste vizinho apaga a linha no SQLite fictício se for chamado.
    // O controle positivo é `apagar_outra_conversa_aguarda_apagar_em_curso_e_reapaga_seu_lote`.
    cleanup_duas_conversas(&f);
    let c = wacli_db(&f);
    insert(&c, &Msg { id: "OLD", ..Msg::default() });
    let antes: i64 = c.query_row("SELECT COUNT(*) FROM messages WHERE chat_jid=?1", [ANA], |r| r.get(0)).unwrap();
    assert!(antes > 0, "a fixture de timeout começa com a cópia presente");
    pass(&f);
    let (_, secret) = hook(&f).await;
    // Só esta fixture retém o lote além dos 10 s reais do wait_idle produtivo.
    // O gancho original e os outros testes continuam com o teto de 5 s.
    let (captured, resume) = f.hub.whatsapp.inbox().pause_after_fetch_for(DONNA, Duration::from_secs(20));
    insert(&c, &Msg { id: "INFLIGHT_TIMEOUT", ..Msg::default() });
    assert!(f.hub.whatsapp.inbox().accept(
        "127.0.0.1:12345".parse().unwrap(),
        DONNA,
        Some(&sign(&secret, b"{}")),
        b"{}",
    ));
    tokio::time::timeout(Duration::from_secs(2), captured.notified()).await.unwrap();
    let erase = apagar(&f, ANA);
    let out = tokio::time::timeout(Duration::from_secs(12), erase).await.unwrap().unwrap().unwrap();
    let cleanups = f.lines("cleanups");
    let left: i64 = c.query_row("SELECT COUNT(*) FROM messages WHERE chat_jid=?1", [ANA], |r| r.get(0)).unwrap();
    resume.send(()).unwrap();
    f.done().await;
    assert_eq!(out["wacli"], "failed");
    assert_eq!(cleanups, 0, "timeout não pode limpar a cópia antes de drenar o leitor");
    assert!(left > 0, "a cópia da fixture precisa continuar disponível para repetir a exclusão");
    assert_eq!(out["warning"], "Não foi possível confirmar que toda a conversa foi apagada. Tente novamente.");
}
