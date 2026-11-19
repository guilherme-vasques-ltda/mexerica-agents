//! D70: handler real com execução e número sintéticos dos produtores existentes.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::{
    number_tests::{DONNA, Fixture},
    send,
};
use crate::{
    LockExt,
    api::devices::Caller,
    fork::{
        bot_admission::{self, Origin},
        conectores::{api, whatsapp_human},
        sessoes::{ExecutionRef, bindings},
    },
    hub::BotStatus,
    store::{EntryKind, Lane, Scope},
};
use rusqlite::{params, types::Value as SqlValue};
use serde_json::json;

const CHAT: &str = "5511988887777@s.whatsapp.net";

pub(super) async fn ready() -> (Fixture, ExecutionRef) {
    let f = Fixture::new(true);
    *f.hub.store.secret_key.locked() = Some(zeroize::Zeroizing::new(vec![42; 32]));
    f.call("donnaWhatsappSave", json!({"enabled":true})).await.unwrap();
    f.call("donnaWhatsappBotSave", json!({"botId":DONNA,"settings":{"readChats":true}})).await.unwrap();
    f.connect(DONNA).await;
    f.until(DONNA, |n| n["phone"].is_string()).await;
    {
        let db = f.hub.store.db.locked();
        db.execute(
            "INSERT INTO donna_whatsapp_messages(bot_id,chat_id,message_id,account_id,is_group,
            sender_name,sent_at,received_at,body,offline,read,expires_at)
            SELECT ?1,?2,'M1',account_id,0,'Ana externa',1,1,'Mensagem externa literal',0,0,?3
            FROM donna_whatsapp_numbers WHERE bot_id=?1",
            params![DONNA, CHAT, i64::MAX],
        )
        .unwrap();
    }
    let entry = f
        .hub
        .store
        .insert_entry(
            &Lane::main(DONNA),
            EntryKind::User,
            1,
            &json!({"text":"Prepare a resposta à Ana","status":"queued"}),
        )
        .unwrap();
    let execution = bindings::fixture_execution(&f.hub, DONNA);
    f.hub.store.memory_session_start(DONNA, "fixture-acp", false).unwrap();
    let config = f.hub.store.bot(DONNA).unwrap().unwrap().config;
    let ids = vec![entry.id.clone()];
    let ticket =
        bot_admission::reserve_for_entries(&f.hub.store, &config, DONNA, execution.turn_token(), &Origin::Human, &ids)
            .unwrap();
    bot_admission::permit(&f.hub.store, &config, &ticket, None, &ids, Some(DONNA)).unwrap().unwrap();
    f.hub.set_runtime(DONNA, |r| {
        r.message = Some(entry.id);
    });
    (f, execution)
}

fn request() -> send::WhatsappHumanRequest {
    serde_json::from_value(json!({"chat_id":CHAT,"message_id":"M1","text":"Amanhã eu ligo."})).unwrap()
}

fn id(receipt: &send::WhatsappPreparedReceipt) -> String {
    serde_json::to_value(receipt).unwrap()["draftId"].as_str().unwrap().into()
}

fn private_count(f: &Fixture) -> i64 {
    f.hub
        .store
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM kv WHERE k LIKE 'donna.whatsapp.draft.private.%'", [], |r| r.get(0))
        .unwrap()
}

#[tokio::test]
async fn handler_real_rascunho_local_sobrevive_turno_sem_fila_ou_decisao() {
    let (f, execution) = ready().await;
    // O roteador real mantém a referência original, não o helper fechado de envio.
    let result = crate::market::requests::call_in_execution(
        &f.hub,
        DONNA,
        send::PREPARE_HUMAN,
        &serde_json::to_value(request()).unwrap(),
        &execution,
    )
    .unwrap();
    let draft = result["draftId"].as_str().unwrap();
    f.hub.set_runtime(DONNA, |r| {
        r.status = BotStatus::Idle;
        r.memory_turn_token = None;
        r.message = None;
    });
    let read = whatsapp_human::read(&f.hub, draft).await.unwrap();
    assert_eq!(read["presentation"]["text"], "Amanhã eu ligo.");
    assert_eq!(read["presentation"]["replyTo"]["messageId"], "M1");
    assert_eq!(read["presentation"]["replyTo"]["excerpt"], "Mensagem externa literal");
    assert_eq!(read["presentation"]["replyTo"]["origin"], "external");
    assert_eq!(read["capability"]["approve"], false);
    let public = f.hub.store.entry(draft).unwrap().data.to_string();
    assert!(!public.contains("Amanhã") && !public.contains("Mensagem externa") && !public.contains("5511"));
    {
        let db = f.hub.store.db.locked();
        let raw: String = db
            .query_row("SELECT v FROM kv WHERE k=?1", [format!("donna.whatsapp.draft.private.{draft}")], |r| r.get(0))
            .unwrap();
        assert!(raw.starts_with("vault:v1:") && !raw.contains("Amanhã"));
        for table in ["donna_whatsapp_pace", "donna_conector_tentativas"] {
            let count: i64 = db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap();
            assert_eq!(count, 0);
        }
    }
    assert_eq!(f.lines("sends"), 0);
    assert!(api::intercept(&f.hub, &json!({"entryId":draft})).is_err());
    let device = Caller::Device { key: "synthetic".into(), scopes: vec![Scope::Control], autoridade: None };
    assert!(crate::api::devices::permit(&device, "donnaConectorRascunho").is_err());
    assert!(api::call(&f.hub, &device, "donnaConectorRascunho", json!({"draftId":draft})).await.is_err());
    let capability = api::call(&f.hub, &Caller::Local, "donnaConectorCapability", json!({})).await.unwrap();
    let std::ops::ControlFlow::Break(capability) = capability else { panic!("capability") };
    assert_eq!(capability["decisionProtocol"], 0);
    f.done().await;
}

#[tokio::test]
async fn origem_falsa_mensagem_incompativel_e_cancelamento_nao_preparam() {
    let (f, execution) = ready().await;
    assert!(
        serde_json::from_value::<send::WhatsappHumanRequest>(
            json!({"chat_id":CHAT,"message_id":"M1","text":"x","reply_to":"M2"})
        )
        .is_err()
    );
    let mut req = request();
    req.message_id = "outra".into();
    assert!(send::prepare_human_in_execution(&f.hub, &execution, req).is_err());
    let mut req = request();
    req.text = "x\0y".into();
    assert!(send::prepare_human_in_execution(&f.hub, &execution, req).is_err());
    f.hub
        .store
        .db
        .locked()
        .execute(
            "UPDATE kv SET v=json_set(v,'$.origin',json('{\"kind\":\"routine\",\"id\":\"fake\"}')) WHERE k=?1",
            [bot_admission::key(DONNA)],
        )
        .unwrap();
    assert!(send::prepare_human_in_execution(&f.hub, &execution, request()).is_err());
    f.hub
        .store
        .db
        .locked()
        .execute(
            "UPDATE kv SET v=json_set(v,'$.origin',json('{\"kind\":\"human\"}')) WHERE k=?1",
            [bot_admission::key(DONNA)],
        )
        .unwrap();
    let ticket = bot_admission::current(&f.hub.store, DONNA).unwrap().unwrap().ticket();
    bot_admission::request_stop(&f.hub.store, DONNA, &ticket).unwrap();
    assert!(send::prepare_human_in_execution(&f.hub, &execution, request()).is_err());
    // Token de outro turno também recusa; isto não simula stop_requested.
    f.hub.set_runtime(DONNA, |r| {
        r.memory_turn_token = Some("outro-turno".into());
    });
    assert!(send::prepare_human_in_execution(&f.hub, &execution, request()).is_err());
    assert_eq!(private_count(&f), 0);
    f.done().await;
}

#[tokio::test]
async fn falha_sql_desfaz_privado_recibo_e_projecao_juntos() {
    let (f, execution) = ready().await;
    f.hub
        .store
        .db
        .locked()
        .execute_batch(
            "CREATE TEMP TRIGGER fail_draft BEFORE INSERT ON entries
        WHEN json_type(NEW.data,'$.conectorDraft') IS NOT NULL BEGIN SELECT RAISE(ABORT,'fixture'); END;",
        )
        .unwrap();
    assert!(send::prepare_human_in_execution(&f.hub, &execution, request()).is_err());
    assert_eq!(private_count(&f), 0);
    let count: i64 = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM kv WHERE k LIKE 'donna.whatsapp.draft.receipt.%'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    f.done().await;
}

#[tokio::test]
async fn revogar_e_reconceder_leitura_nao_salva_captura_antiga() {
    let (f, execution) = ready().await;
    let draft = id(&send::prepare_human_in_execution(&f.hub, &execution, request()).unwrap());
    f.call("donnaWhatsappBotSave", json!({"botId":DONNA,"settings":{"readChats":false}})).await.unwrap();
    f.call("donnaWhatsappBotSave", json!({"botId":DONNA,"settings":{"readChats":true}})).await.unwrap();
    assert!(whatsapp_human::read(&f.hub, &draft).await.unwrap()["presentation"].is_null());
    assert_eq!(private_count(&f), 0);
    f.done().await;
}

#[tokio::test]
async fn repareamento_nao_adota_a_nova_conta() {
    let (f, execution) = ready().await;
    let draft = id(&send::prepare_human_in_execution(&f.hub, &execution, request()).unwrap());
    f.call("donnaWhatsappDisconnect", json!({"botId":DONNA,"erase":false})).await.unwrap();
    f.connect(DONNA).await;
    assert!(whatsapp_human::read(&f.hub, &draft).await.unwrap()["presentation"].is_null());
    assert_eq!(private_count(&f), 0);
    assert_eq!(f.lines("sends"), 0);
    f.done().await;
}

#[tokio::test]
async fn prazo_original_e_reinicio_apagam_privado_sem_reconstituir_autoridade() {
    let (f, execution) = ready().await;
    let draft = id(&send::prepare_human_in_execution(&f.hub, &execution, request()).unwrap());
    tokio::time::pause();
    tokio::time::advance(std::time::Duration::from_secs(91)).await;
    assert!(whatsapp_human::read(&f.hub, &draft).await.unwrap()["presentation"].is_null());
    assert_eq!(private_count(&f), 0);
    tokio::time::resume();
    let draft = id(&send::prepare_human_in_execution(&f.hub, &execution, request()).unwrap());
    whatsapp_human::recover(&f.hub).unwrap();
    assert_eq!(private_count(&f), 0);
    assert!(whatsapp_human::read(&f.hub, &draft).await.unwrap()["presentation"].is_null());
    f.done().await;
}

#[tokio::test]
async fn startup_contem_so_recibo_corrompido_e_preserva_cerca_sem_privado() {
    let (f, execution) = ready().await;
    for kind in ["json", "extra", "id", "revision", "blob"] {
        let receipt = send::prepare_human_in_execution(&f.hub, &execution, request()).unwrap();
        let draft = id(&receipt);
        let mut value = serde_json::to_value(&receipt).unwrap();
        if kind == "extra" {
            value["unexpected"] = true.into();
        }
        if kind == "id" {
            value["draftId"] = "outro".into();
        }
        if kind == "revision" {
            value["revision"] = i64::MAX.into();
        }
        let raw = if kind == "json" { "{".to_owned() } else { value.to_string() };
        let raw = if kind == "blob" { SqlValue::Blob(vec![0xff]) } else { SqlValue::Text(raw) };
        f.hub
            .store
            .db
            .locked()
            .execute("UPDATE kv SET v=?2 WHERE k=?1", params![format!("donna.whatsapp.draft.receipt.{draft}"), raw])
            .unwrap();
        whatsapp_human::recover(&f.hub).unwrap();
        assert_eq!(private_count(&f), 0);
        let closed = whatsapp_human::read(&f.hub, &draft).await.unwrap();
        assert_eq!(closed["receipt"]["state"], "closed");
        assert_eq!(closed["receipt"]["deadline"], 0);
        assert!(closed["presentation"].is_null());
        assert!(whatsapp_human::is_draft(&f.hub.store, &draft).unwrap());
        assert!(api::intercept(&f.hub, &json!({"entryId":draft})).is_err());
        assert_eq!(f.hub.store.entry(&draft).unwrap().data["conectorDraft"]["state"], "closed");
    }
    // Uma falha SQL real não é corrupção de JSON e continua sendo propagada.
    send::prepare_human_in_execution(&f.hub, &execution, request()).unwrap();
    f.hub
        .store
        .db
        .locked()
        .execute_batch(
            "CREATE TEMP TRIGGER fail_receipt_cleanup BEFORE UPDATE ON kv
            WHEN NEW.k LIKE 'donna.whatsapp.draft.receipt.%'
            BEGIN SELECT RAISE(ABORT,'fixture SQL'); END;",
        )
        .unwrap();
    assert!(whatsapp_human::recover(&f.hub).is_err());
    assert_eq!(private_count(&f), 1, "rollback preserva o estado anterior, sem meia limpeza");
    f.hub.store.db.locked().execute_batch("DROP TRIGGER fail_receipt_cleanup").unwrap();
    whatsapp_human::recover(&f.hub).unwrap();
    assert_eq!(private_count(&f), 0);
    assert_eq!(f.lines("sends"), 0);
    f.done().await;
}
