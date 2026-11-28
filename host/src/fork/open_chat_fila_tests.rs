use super::{PENDING, cancel_pending, finish, submit};
use crate::LockExt;
use crate::fork::open_chat::{RUNNING, Running};
use crate::store::{EntryKind, Lane};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::watch;

fn hub_de_teste(bot: &str) -> Arc<crate::hub::Hub> {
    let dados = std::env::temp_dir().join(format!("open-fila-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dados).unwrap();
    let store = crate::store::Store::open(std::path::Path::new(":memory:")).unwrap();
    let cfg: crate::store::BotConfig =
        serde_json::from_value(json!({"id": bot, "name": "b", "backend": "claude", "cwd": "/tmp"})).unwrap();
    store.save_bot(&cfg).unwrap();
    let identidade = crate::remote::identity::Identity::load_or_create(&dados).unwrap();
    crate::hub::Hub::new(store, "fixture".into(), identidade, "fixture".into(), 0)
}

fn reservar(hub: &Arc<crate::hub::Hub>, bot: &str) -> (watch::Sender<()>, watch::Receiver<bool>) {
    let (cancel, cancel_rx) = watch::channel(false);
    let (done_tx, done_rx) = watch::channel(());
    RUNNING.locked().get_or_insert_with(Default::default).insert(bot.into(), Running::for_test(hub, cancel, done_rx));
    (done_tx, cancel_rx)
}

fn mensagem(hub: &Arc<crate::hub::Hub>, bot: &str, n: i64) -> (String, crate::store::BotConfig) {
    let lane = Lane::main(bot);
    let e = hub.add_entry(&lane, EntryKind::User, n, &json!({"text": format!("m{n}"), "status": "queued"})).unwrap();
    (e.id, hub.store.bot(bot).unwrap().unwrap().config)
}

fn status(hub: &Arc<crate::hub::Hub>, id: &str) -> String {
    hub.store.entry(id).unwrap().data["status"].as_str().unwrap().to_owned()
}

#[test]
fn mensagem_com_resposta_em_curso_espera_na_fila_em_ordem_sem_ser_recusada() {
    let bot = "bot-fila-ordem";
    let hub = hub_de_teste(bot);
    let _reserva = reservar(&hub, bot);
    let (a, cfg) = mensagem(&hub, bot, 1);
    let (b, _) = mensagem(&hub, bot, 2);
    submit(&hub, &cfg, Lane::main(bot), 1, (a.clone(), None), "m1".into());
    submit(&hub, &cfg, Lane::main(bot), 2, (b.clone(), None), "m2".into());
    let esperando: Vec<String> = PENDING.locked().as_ref().unwrap()[bot].iter().map(|q| q.entry.0.clone()).collect();
    assert_eq!(esperando, vec![a.clone(), b.clone()], "entregues na ordem do envio");
    assert_eq!((status(&hub, &a), status(&hub, &b)), ("queued".into(), "queued".into()), "visíveis como na fila");
    RUNNING.locked().as_mut().unwrap().remove(bot);
    PENDING.locked().as_mut().unwrap().remove(bot);
}

#[test]
fn ao_terminar_a_resposta_a_fila_segue_em_ordem_e_uma_recusa_nao_trava_as_outras() {
    let bot = "bot-fila-fim";
    let hub = hub_de_teste(bot);
    let _reserva = reservar(&hub, bot);
    let (a, cfg) = mensagem(&hub, bot, 1);
    let (b, _) = mensagem(&hub, bot, 2);
    submit(&hub, &cfg, Lane::main(bot), 1, (a.clone(), None), "m1".into());
    submit(&hub, &cfg, Lane::main(bot), 2, (b.clone(), None), "m2".into());
    // Sem conta da Open conectada a entrega falha com o motivo, e a seguinte ainda é tentada: a fila esvazia.
    finish(&hub, bot);
    assert_eq!((status(&hub, &a), status(&hub, &b)), ("failed".into(), "failed".into()));
    assert_eq!(hub.store.entry(&a).unwrap().data["reason"]["code"], "open_chat_account_required");
    assert!(RUNNING.locked().as_ref().is_none_or(|r| !r.contains_key(bot)), "a reserva não fica presa");
    assert!(PENDING.locked().as_ref().unwrap().get(bot).is_none_or(std::collections::VecDeque::is_empty));
}

#[test]
fn bot_livre_marca_a_mensagem_como_enviando_e_nao_como_fila() {
    let bot = "bot-fila-livre";
    let hub = hub_de_teste(bot);
    let (a, cfg) = mensagem(&hub, bot, 1);
    submit(&hub, &cfg, Lane::main(bot), 1, (a.clone(), None), "m1".into());
    assert_eq!(status(&hub, &a), "failed", "sem conta a entrega falha: passou por sending, não ficou na fila");
    let (b, _) = mensagem(&hub, bot, 2);
    super::mark_sending(&hub, &b);
    assert_eq!(status(&hub, &b), "sending");
    super::recover(&hub.store).unwrap();
    assert_eq!(status(&hub, &b), "failed", "o que ficou enviando numa execução anterior não fica preso");
}

#[test]
fn na_partida_a_mensagem_que_ficou_na_fila_de_uma_execucao_anterior_vira_falha() {
    let bot = "bot-fila-boot";
    let hub = hub_de_teste(bot);
    let (a, _) = mensagem(&hub, bot, 1);
    let enviada =
        hub.add_entry(&Lane::main(bot), EntryKind::User, 2, &json!({"text": "ok", "status": "sent"})).unwrap();
    super::recover(&hub.store).unwrap();
    assert_eq!(status(&hub, &a), "failed");
    assert_eq!(hub.store.entry(&a).unwrap().data["reason"]["code"], "open_chat_failed");
    assert_eq!(status(&hub, &enviada.id), "sent", "o que já foi aceito não muda");
}

#[test]
fn envios_concorrentes_entram_na_fila_na_ordem_em_que_foram_gravados() {
    let bot = "bot-fila-concorrente";
    let hub = hub_de_teste(bot);
    let _reserva = reservar(&hub, bot);
    let cfg = hub.store.bot(bot).unwrap().unwrap().config;
    let gravadas = Arc::new(std::sync::Mutex::new(Vec::new()));
    let threads: Vec<_> = (0..8)
        .map(|n| {
            let (hub, cfg, gravadas) = (hub.clone(), cfg.clone(), gravadas.clone());
            std::thread::spawn(move || {
                let _ordem = super::ordem();
                let e = hub
                    .add_entry(&Lane::main(bot), EntryKind::User, n, &json!({"text": "x", "status": "queued"}))
                    .unwrap();
                std::thread::sleep(std::time::Duration::from_millis(5));
                gravadas.locked().push(e.id.clone());
                submit(&hub, &cfg, Lane::main(bot), n, (e.id, None), "x".into());
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let na_fila: Vec<String> = PENDING.locked().as_ref().unwrap()[bot].iter().map(|q| q.entry.0.clone()).collect();
    assert_eq!(na_fila, *gravadas.locked(), "a fila segue a ordem de gravação");
    RUNNING.locked().as_mut().unwrap().remove(bot);
    PENDING.locked().as_mut().unwrap().remove(bot);
}

#[test]
fn stop_cancela_as_mensagens_que_esperavam_a_resposta() {
    let bot = "bot-fila-stop";
    let hub = hub_de_teste(bot);
    let _reserva = reservar(&hub, bot);
    let (a, cfg) = mensagem(&hub, bot, 1);
    submit(&hub, &cfg, Lane::main(bot), 1, (a.clone(), None), "m1".into());
    cancel_pending(bot);
    assert_eq!(status(&hub, &a), "cancelled");
    RUNNING.locked().as_mut().unwrap().remove(bot);
}
