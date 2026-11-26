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
