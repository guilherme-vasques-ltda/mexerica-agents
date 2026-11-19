#![allow(clippy::unwrap_used)]
//! D70: padrões, regras, modo Só o Berga, desligar/religar, autoridade e exclusão.
use super::{api::handle, number::State};
use crate::{
    LockExt,
    api::devices::Caller,
    store::{BotConfig, Scope, Store},
};
use serde_json::{Value, json};
use std::path::Path;

const HUB: &str = "00000000-0000-4000-8000-0000000000aa";

fn fixture() -> Store {
    let store = Store::open(Path::new(":memory:")).unwrap();
    for id in ["donna", "lia"] {
        let cfg: BotConfig =
            serde_json::from_value(json!({"id":id,"name":id,"backend":"fixture","cwd":"/synthetic"})).unwrap();
        store.save_bot(&cfg).unwrap();
    }
    store.set_team_settings(&json!({"primaryBotId":"donna"})).unwrap();
    store
}

fn call(store: &Store, method: &str, body: Value) -> anyhow::Result<Value> {
    handle(store, HUB, &State::default(), &Caller::Local, method, body)
}

fn bot<'a>(view: &'a Value, id: &str) -> &'a Value {
    view["bots"].as_array().unwrap().iter().find(|b| b["botId"] == id).unwrap()
}

fn rows(store: &Store) -> i64 {
    store.db.locked().query_row("SELECT COUNT(*) FROM donna_whatsapp_bots", [], |r| r.get(0)).unwrap()
}

#[test]
fn defaults_start_off_with_separate_states() {
    let store = fixture();
    let view = call(&store, "donnaWhatsapp", json!({})).unwrap();
    assert_eq!(view["plugin"], json!({"enabled": false, "mode": "donna_only", "wacli": {"installed": false}}));
    assert_eq!(view["transcription"]["source"], "none");
    let donna = bot(&view, "donna");
    assert_eq!(
        donna["settings"],
        json!({"readChats":false,"replyChats":false,"readGroups":false,"replyGroups":false,
            "replyMode":"approval","delaySeconds":15,"hourlyLimit":30,"dailyLimit":100,"quietEnabled":true,
        "quietStart":1320,"quietEnd":480,"quietTimeZone":null,"transcribeAudio":true,"retentionDays":30})
    );
    // Sem binário do wacli: estado claro, nada quebra.
    assert_eq!(donna["number"], json!({"state":"not_installed"}));
    assert_eq!(donna["sending"], json!({"state":"none"}));
    assert_eq!(donna["botCan"]["readChats"], false);
    // Plugin desligado: nenhuma opção do bot pode ser gravada.
    let err = call(&store, "donnaWhatsappBotSave", json!({"botId":"donna","settings":{"readChats":true}}));
    assert!(err.unwrap_err().to_string().starts_with("plugin_disabled"));
}

#[test]
fn reply_requires_read_and_ranges_hold() {
    let store = fixture();
    call(&store, "donnaWhatsappSave", json!({"enabled":true})).unwrap();
    let save = |settings: Value| call(&store, "donnaWhatsappBotSave", json!({"botId":"donna","settings":settings}));
    for bad in [
        json!({"replyChats":true}),
        json!({"replyGroups":true}),
        json!({"delaySeconds":7}),
        json!({"delaySeconds":301}),
        json!({"delaySeconds":-1}),
        json!({"hourlyLimit":0}),
        json!({"hourlyLimit":201}),
        json!({"startConversations":true}),
    ] {
        assert!(save(bad.clone()).is_err(), "{bad}");
    }
    // D357: a recusa diz a regra, seja quem for que chamou o RPC.
    let err = save(json!({"delaySeconds":3})).unwrap_err().to_string();
    assert!(err.contains("8 a 300") && err.starts_with("invalid_field"), "{err}");
    let err = save(json!({"hourlyLimit":500})).unwrap_err().to_string();
    assert!(err.contains("200") && err.starts_with("invalid_field"), "{err}");
    let view = save(json!({"readChats":true,"replyChats":true,"delaySeconds":8,"hourlyLimit":200})).unwrap();
    assert_eq!(bot(&view, "donna")["botCan"]["replyChats"], true);
    let view = save(json!({"delaySeconds":300,"hourlyLimit":1})).unwrap();
    assert_eq!(bot(&view, "donna")["settings"]["delaySeconds"], 300);
    // MX-18: "sozinho" é modo válido e persiste; voltar ao "peço antes" também.
    let view = save(json!({"replyMode":"autonomous"})).unwrap();
    assert_eq!(bot(&view, "donna")["settings"]["replyMode"], "autonomous");
    let view = save(json!({"replyMode":"approval"})).unwrap();
    assert_eq!(bot(&view, "donna")["settings"]["replyMode"], "approval");
    // Desligar "ver" leva junto "responder".
    let view = save(json!({"readChats":false})).unwrap();
    assert_eq!(bot(&view, "donna")["settings"]["replyChats"], false);
    assert!(save(json!({"readChats":false,"replyChats":true})).is_err());
}

#[test]
fn donna_only_blocks_and_pauses_other_bots_without_deleting() {
    let store = fixture();
    call(&store, "donnaWhatsappSave", json!({"enabled":true})).unwrap();
    let lia = json!({"botId":"lia","settings":{"readGroups":true}});
    let err = call(&store, "donnaWhatsappBotSave", lia.clone()).unwrap_err();
    assert!(err.to_string().starts_with("not_allowed"));
    call(&store, "donnaWhatsappSave", json!({"mode":"per_bot"})).unwrap();
    let view = call(&store, "donnaWhatsappBotSave", lia).unwrap();
    assert_eq!(bot(&view, "lia")["settings"]["readGroups"], true);
    // R3: "pode ver" é concessão do registro, que só existe com número conectado.
    assert_eq!(bot(&view, "lia")["botCan"]["readGroups"], false);
    let view = call(&store, "donnaWhatsappSave", json!({"mode":"donna_only"})).unwrap();
    let lia = bot(&view, "lia");
    assert_eq!(lia["paused"], true);
    assert_eq!(lia["botCan"]["readGroups"], false);
    assert_eq!(lia["settings"]["readGroups"], true);
    assert_eq!(bot(&view, "donna")["paused"], false);
}

#[test]
fn disabling_drops_access_and_reenabling_does_not_restore_it() {
    let store = fixture();
    call(&store, "donnaWhatsappSave", json!({"enabled":true})).unwrap();
    call(&store, "donnaWhatsappBotSave", json!({"botId":"donna","settings":{"readChats":true,"delaySeconds":20}}))
        .unwrap();
    let generation = |s: &Store| -> i64 {
        s.db.locked()
            .query_row("SELECT generation FROM donna_plugin_installations WHERE builtin='whatsapp'", [], |r| r.get(0))
            .unwrap()
    };
    let before = generation(&store);
    call(&store, "donnaWhatsappSave", json!({"enabled":false})).unwrap();
    let view = call(&store, "donnaWhatsappSave", json!({"enabled":true})).unwrap();
    assert_eq!(generation(&store), before + 2);
    let donna = bot(&view, "donna");
    assert_eq!(donna["settings"]["readChats"], false);
    assert_eq!(donna["settings"]["delaySeconds"], 20);
    assert_eq!(donna["botCan"]["readChats"], false);
}

#[test]
fn transcription_defaults_to_the_keyed_provider_and_accepts_a_choice() {
    let store = fixture();
    crate::market::vault::write(&store, "voice", r#"{"gemini":"FIXTURE"}"#).unwrap();
    let view = call(&store, "donnaWhatsapp", json!({})).unwrap();
    assert_eq!(view["transcription"]["provider"], "gemini");
    assert_eq!(view["transcription"]["source"], "default");
    let choice = json!({"transcription":{"provider":"openai","model":"whisper-1"}});
    let view = call(&store, "donnaWhatsappSave", choice).unwrap();
    assert_eq!(
        view["transcription"],
        json!({"provider":"openai","model":"whisper-1","source":"chosen","keyOnHub":false})
    );
    assert!(call(&store, "donnaWhatsappSave", json!({"transcription":{"provider":"x","model":"m"}})).is_err());
    assert!(call(&store, "donnaWhatsappSave", json!({"transcription":{"provider":"openai","model":"a b"}})).is_err());
    let view = call(&store, "donnaWhatsappSave", json!({"transcription":null})).unwrap();
    assert_eq!(view["transcription"]["source"], "default");
}

#[test]
fn only_the_owner_reads_or_writes() {
    let store = fixture();
    for caller in [
        Caller::Device { key: "executor".into(), scopes: vec![Scope::Executor], autoridade: None },
        Caller::Pairing { key: "pairing".into() },
    ] {
        for method in super::api::SETTINGS {
            let err = handle(&store, HUB, &State::default(), &caller, method, json!({"enabled":true})).unwrap_err();
            assert!(err.to_string().starts_with("not_allowed"), "{method}");
        }
    }
    let control = Caller::Device { key: "owner".into(), scopes: vec![Scope::Control], autoridade: None };
    assert_eq!(
        handle(&store, HUB, &State::default(), &control, "donnaWhatsappSave", json!({"enabled":true})).unwrap()["plugin"]
            ["enabled"],
        true
    );
}

#[test]
fn deleting_the_bot_deletes_its_settings() {
    let store = fixture();
    call(&store, "donnaWhatsappSave", json!({"enabled":true,"mode":"per_bot"})).unwrap();
    call(&store, "donnaWhatsappBotSave", json!({"botId":"lia","settings":{"hourlyLimit":5}})).unwrap();
    assert_eq!(rows(&store), 1);
    store.delete_bot("lia").unwrap();
    assert_eq!(rows(&store), 0);
}
