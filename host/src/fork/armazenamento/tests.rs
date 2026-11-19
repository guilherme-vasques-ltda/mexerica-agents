#![allow(clippy::unwrap_used)]
use super::*;
use crate::{
    api::devices,
    store::{Scope, Store},
};
use serde_json::json;

#[test]
fn owner_boundary_rejects_before_reading() {
    let body = json!({});
    for scopes in [vec![], vec![Scope::Executor], vec![Scope::Screen], vec![Scope::OwnerDevices]] {
        let caller = Caller::Device { key: "test".into(), scopes, autoridade: None };
        assert!(authorize(&caller, &body).is_err());
        assert!(devices::permit(&caller, METHOD).is_err());
    }
    let pairing = Caller::Pairing { key: "test".into() };
    assert!(authorize(&pairing, &body).is_err());
    let control = Caller::Device { key: "test".into(), scopes: vec![Scope::Control], autoridade: None };
    assert!(authorize(&control, &body).is_ok());
    assert!(devices::permit(&control, METHOD).is_ok());
    assert!(authorize(&Caller::Local, &body).is_ok());
    assert!(authorize(&Caller::Local, &json!({"delete":true})).is_err());
    assert!(!crate::fork::ponte_credencial::PERMITIDOS.contains(&METHOD));
}

#[test]
fn two_bots_utf8_sessions_files_and_partial_failure() {
    let root = std::env::temp_dir().join(format!("storage-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("hub.db");
    let store = Store::open(&path).unwrap();
    super::super::whatsapp::migrate(&store.db.locked()).unwrap();
    let a = uuid::Uuid::new_v4().to_string();
    let b = uuid::Uuid::new_v4().to_string();
    for (id, name) in [(&a, "Ágata"), (&b, "Bia")] {
        let cfg = serde_json::from_value(json!({"id":id,"name":name})).unwrap();
        store.save_bot(&cfg).unwrap();
    }
    {
        let c = store.db.locked();
        c.execute("INSERT INTO bots(id,rev,deleted,config) VALUES('deleted',1,1,'{}')", []).unwrap();
        for (id, bot, kind, data, session, at) in [
            ("1", &a, "user", "á", None, 10),
            ("2", &a, "tool", "abc", None, 5),
            ("3", &a, "agent", "oi", Some("extra"), 20),
            ("4", &b, "user", "xyz", None, 30),
        ] {
            c.execute("INSERT INTO entries(id,bot_id,rev,kind,turn,data,created_at,updated_at,session) VALUES(?1,?2,1,?3,1,?4,?6,?6,?5)",
                rusqlite::params![id, bot, kind, data, session, at]).unwrap();
        }
        c.execute("INSERT INTO memory_entries(bot,state,text,author,created_at,updated_at,rev) VALUES(?1,'active','olá','user',1,1,1)", [&a]).unwrap();
        c.execute("INSERT INTO donna_whatsapp_messages(bot_id,chat_id,message_id,account_id,is_group,sender_name,sent_at,received_at,body,offline,read,expires_at) VALUES(?1,'chat','msg','account',0,'Ana',1,1,'oi',0,0,999)", [&b]).unwrap();
    }
    // Um bot legado sem spaceId pertence ao Pessoal; o outro pertence a B.
    store
        .db
        .locked()
        .execute("UPDATE bots SET config=json_set(config,'$.spaceId','space-b') WHERE id=?1", [&b])
        .unwrap();
    store.db.locked().execute("INSERT INTO bots(id,rev,deleted,config) VALUES('invalid',1,0,'{')", []).unwrap();
    let invalid = report(Some(&path), crate::fork::spaces::PERSONAL);
    assert!(invalid.bots.unwrap().iter().any(|b| b.id == "invalid" && b.name.contains("inválido")));
    assert!(!report(Some(&path), "space-b").bots.unwrap().iter().any(|b| b.id == "invalid"));
    store.db.locked().execute("DELETE FROM bots WHERE id='invalid'", []).unwrap();
    let personal = report(Some(&path), crate::fork::spaces::PERSONAL);
    let work = report(Some(&path), "space-b");
    assert_eq!(personal.bots.as_ref().unwrap().iter().map(|b| &b.id).collect::<Vec<_>>(), vec![&a]);
    assert_eq!(work.bots.as_ref().unwrap().iter().map(|b| &b.id).collect::<Vec<_>>(), vec![&b]);
    assert_eq!(personal.total.database_bytes, work.total.database_bytes);
    assert_eq!(work.total.context, "global");
    store.db.locked().execute("UPDATE bots SET config=json_remove(config,'$.spaceId') WHERE id=?1", [&b]).unwrap();
    let reader = open(&path).unwrap();
    assert!(reader.execute("INSERT INTO kv VALUES('no','write')", []).is_err());
    let before = store.current_rev();
    let view = report(Some(&path), crate::fork::spaces::PERSONAL);
    let bots = view.bots.unwrap();
    assert_eq!(bots.len(), 2);
    let first = bots.iter().find(|bot| bot.id == a).unwrap();
    assert_eq!(first.name, "Ágata");
    let main = first.messages.main.as_ref().unwrap();
    assert_eq!((main.bytes, main.count, main.oldest_at), (5, 1, Some(10)));
    let extra = first.messages.extras.as_ref().unwrap();
    assert_eq!((extra.bytes, extra.count, extra.oldest_at), (2, 1, Some(20)));
    // Texto UTF-8 (4) + origin '{}' (2), atual e versão gravada pelo crate.
    assert_eq!(first.memory.rows_bytes, Some(12));
    assert_eq!(first.bytes, Some(19));
    let second = bots.iter().find(|bot| bot.id == b).unwrap();
    assert_eq!(second.messages.bytes, Some(3));
    assert_eq!(second.whatsapp.hub_bytes, Some(5));
    assert_eq!(second.bytes, Some(8));
    assert_eq!(view.total.database_bytes, Some(std::fs::metadata(&path).unwrap().len()));
    assert_eq!(view.total.wal_bytes, Some(std::fs::metadata(suffix(&path, "-wal")).unwrap().len()));
    assert_eq!(view.total.database_free_bytes, Some(free_bytes(&reader).unwrap()));
    assert_eq!(store.current_rev(), before);
    let files = root.join("uploads");
    std::fs::create_dir_all(files.join("nested")).unwrap();
    std::fs::write(files.join("a"), b"123").unwrap();
    std::fs::write(files.join("nested/b"), b"45").unwrap();
    assert_eq!(directory_bytes(&files).unwrap(), 5);
    let wacli = root.join("wacli.db");
    std::fs::write(&wacli, b"not a database").unwrap();
    assert_eq!(file_bytes(&wacli, true).unwrap(), 14);
    // Falha de um componente não apaga as demais medições nem produz um total falso.
    store.db.locked().execute("DROP TABLE donna_whatsapp_messages", []).unwrap();
    let partial = report(Some(&path), crate::fork::spaces::PERSONAL);
    let bot = partial.bots.unwrap().into_iter().find(|bot| bot.id == a).unwrap();
    assert!(bot.whatsapp.hub_bytes.is_none());
    assert!(bot.bytes.is_none());
    assert_eq!(bot.messages.bytes, Some(7));
    assert!(partial.total.database_bytes.is_some());
    drop(reader);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
