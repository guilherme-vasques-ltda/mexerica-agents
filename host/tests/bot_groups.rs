//! Protocolo de grupos do Hub N+1: erros estáveis no HTTP e isolamento de Spaces.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;
use serde_json::json;

#[tokio::test]
async fn codigos_estaveis_chegam_no_http() {
    let hub = common::host::Host::start().await;
    let bot = hub.bot("Pesquisador", "ask").await;
    let added = hub.call("botGroupsEdit", json!({"op":"add","name":"Pesquisa"})).await;
    let id = added["created"].as_str().unwrap();
    let placed = hub.call("botGroupsEdit", json!({"op":"place","botId":bot,"groupId":id})).await;
    assert_eq!(placed["snapshot"]["groups"][0]["bots"], json!([bot]));
    let (ok, error) = hub.post("botGroupsEdit", &json!({"op":"add","name":"PÉSQUISA"})).await;
    assert!(!ok);
    assert_eq!(error["code"], "name_taken");
    assert!(error["error"].is_string());
    let (ok, missing) =
        hub.post("botGroupsEdit", &json!({"op":"place","botId":"bot-inexistente","groupId":null})).await;
    assert!(!ok);
    assert_eq!(missing["code"], "bot_missing");
    let (ok, gone) = hub.post("botGroupsEdit", &json!({"op":"rename","id":"grupo-inexistente","name":"x"})).await;
    assert!(!ok);
    assert_eq!(gone["code"], "group_missing");
    let renamed = hub.call("botGroupsEdit", json!({"op":"rename","id":id,"name":"Novo"})).await;
    assert_eq!(renamed["snapshot"]["groups"][0]["name"], "Novo");
    assert_eq!(hub.call("botGroups", json!({})).await["groups"][0]["name"], "Novo");
    let removed = hub.call("botGroupsEdit", json!({"op":"remove","id":id})).await;
    assert_eq!(removed["snapshot"]["groups"], json!([]));
}

#[tokio::test]
async fn grupos_nao_cruzam_spaces() {
    let hub = common::host::Host::start().await;
    let hub_id = hub.call("hello", json!({})).await["hostId"].clone();
    let initial = hub.call("donnaSpaces", json!({"hubId":hub_id})).await;
    let spaces=hub.call("donnaSpaceCreate",json!({"hubId":hub_id,"expectedRevision":initial["revision"],"name":"Trabalho","tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}})).await;
    let work = spaces["spaces"][1]["id"].as_str().unwrap();
    let personal = hub.call("botGroupsEdit", json!({"op":"add","name":"Pessoal"})).await;
    let work_added = hub.call("botGroupsEdit", json!({"spaceId":work,"op":"add","name":"Trabalho"})).await;
    let work_id = work_added["created"].as_str().unwrap();
    let personal_id = personal["created"].as_str().unwrap();
    assert_ne!(work_id, personal_id);
    let changed = hub.call("botGroupsEdit", json!({"spaceId":work,"op":"rename","id":work_id,"name":"Projeto"})).await;
    assert_eq!(changed["snapshot"]["groups"][0]["name"], "Projeto");
    assert_eq!(hub.call("botGroups", json!({"spaceId":work})).await["groups"][0]["name"], "Projeto");
    assert_eq!(hub.call("botGroups", json!({})).await["groups"][0]["name"], "Pessoal");
    let (ok, error) = hub.post("botGroupsEdit", &json!({"op":"remove","id":work_id})).await;
    assert!(!ok);
    assert_eq!(error["code"], "group_missing");
}
