#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use serde_json::json;

#[tokio::test]
async fn perfil_global_revisao_projecao_e_autoridade() {
    let host = common::host::Host::start().await;
    let bot = host.bot("Perfil", "ask").await;
    let initial = host.call("memoryProfile", json!({})).await;
    assert_eq!(
        initial,
        json!({"spaceId":"00000000-0000-4000-8000-000000000001","content":null,"revision":null,"loose":0})
    );
    host.call("saveMemory", json!({"botId":bot,"shared":true,"text":"Gosta de chá."})).await;
    let note = "<!-- mexerica:sobre-voce v1 -->\n# Sobre você\n\n## O que não fazer\nEvitar pressa.\n";
    let saved = host.call("saveMemoryProfile", json!({"content":note,"base":null})).await;
    assert_eq!(saved["content"], note);
    assert_eq!(saved["loose"], 1);
    assert_eq!(host.call("memoryProfile", json!({})).await, saved);
    let err = host.refused("saveMemoryProfile", json!({"content":"Outra nota", "base":null})).await;
    assert!(err.contains("memory_profile_conflict"));
    assert_eq!(host.call("memoryProfile", json!({})).await, saved);
    let err = host
        .refused("saveMemoryProfile", json!({"content":"Wi-Fi password: hunter22", "base":saved["revision"]}))
        .await;
    assert!(err.contains("secret_detected"));
    assert!(!err.contains("hunter22"));
    let file = host.call("memoryFile", json!({"botId":bot,"shared":true,"file":"mexerica-sobre-voce.md"})).await;
    assert!(file["content"].as_str().unwrap().contains("O que não fazer"));
    assert!(!file.to_string().contains("mexerica:sobre-voce"));
    assert!(!host.home.join("shared-memory/mexerica-sobre-voce.md").exists());
    let err = host.tool_refused(&bot, "saveMemoryProfile", json!({"content":"Bot", "base":saved["revision"]})).await;
    assert_ne!(err, "");
    let deleted = host.call("saveMemoryProfile", json!({"content":null,"base":saved["revision"]})).await;
    assert!(deleted["content"].is_null());
    assert_ne!(deleted["revision"], saved["revision"]);
}
