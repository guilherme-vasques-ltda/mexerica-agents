//! CF-17: consumidor Ponte canônico; somente histórico e credencial efêmeros da fixture.
use super::*;
use crate::api::devices::Caller;
use crate::fork::sessoes::bindings;
use anyhow::Context as _;

async fn search(f: &Fixture, caller: &Caller, binding: &str, args: Value) -> Result<Value> {
    crate::api::dispatch(
        &f.hub,
        caller,
        "memoryCall",
        json!({"botId":"a","sessionBinding":binding,"sessionSlot":"main",
            "name":"search_history","arguments":args}),
    )
    .await
}

#[tokio::test]
async fn ponte_busca_e_pagina_somente_a_projecao_redigida() -> Result<()> {
    let f = Fixture::new();
    let old = f.add("a", EntryKind::User, &json!({"text":"marcador Token do banco: 482913"}));
    let middle = f.add("a", EntryKind::User, &json!({"text":"marcador Token do banco: 482914"}));
    let new = f.add("a", EntryKind::User, &json!({"text":"marcador Token do banco: 482913"}));
    f.add("b", EntryKind::User, &json!({"text":"marcador de outro bot"}));
    f.add("a", EntryKind::Thought, &json!({"text":"marcador interno"}));
    let execution = bindings::fixture_execution(&f.hub, "a");
    bindings::recheck(&f.hub, &execution)?;
    let binding = bindings::for_acp(&f.hub.store, "a", "fixture-acp")?;
    let mut servers = vec![json!({"args":["mcp","memory"],"env":[]})];
    crate::fork::ponte_credencial::provisionar(&f.hub, "a", None, &binding, &mut servers)?;
    let token = servers[0]["env"][0]["value"].as_str().context("credencial sintética ausente")?;
    let caller = f.hub.pontes.autenticar(&f.hub, token).context("Ponte da fixture recusada")?;

    let first = search(&f, &caller, binding.id.as_str(), json!({"query":"marcador","limit":1})).await?;
    assert_eq!(first["status"], "partial");
    assert_eq!(first["results"][0]["id"], new.id);
    assert_eq!(first["results"][0]["text"], "marcador Token do banco: [removido]");
    assert_eq!(first["next"], middle.id);
    assert!(first["nextCursor"].is_string());
    let legacy =
        search(&f, &caller, binding.id.as_str(), json!({"query":"marcador","from":first["next"],"limit":1})).await?;
    let second = search(&f, &caller, binding.id.as_str(), json!({"cursor":first["nextCursor"],"limit":1})).await?;
    assert_eq!(legacy, second);
    assert_eq!(second["results"][0]["id"], middle.id);
    assert_eq!(second["next"], old.id);
    let last = search(&f, &caller, binding.id.as_str(), json!({"cursor":second["nextCursor"],"limit":1})).await?;
    assert_eq!(last["results"][0]["id"], old.id);
    assert!(last["next"].is_null());
    assert!(last.get("nextCursor").is_none());

    // Todas as chamadas atravessam o mesmo dispatcher e a mesma execução original.
    // O controle ausente e os dois valores removidos precisam ser indistinguíveis na seleção.
    let mut observations = vec![];
    for query in ["482915", "482913", "482914"] {
        let answer = search(&f, &caller, binding.id.as_str(), json!({"query":query,"limit":1})).await?;
        assert!(matches!(answer["status"].as_str(), Some("ok" | "partial")));
        let results = answer["results"].as_array().context("resposta sem results")?;
        for result in results {
            assert_eq!(result["text"], "marcador Token do banco: [removido]");
        }
        observations.push((results.len(), answer["next"].is_null(), answer.get("nextCursor").is_none()));
    }
    assert_eq!(observations, vec![(0, true, true); 3], "seleção/continuação não podem depender de valor removido");
    Ok(())
}

#[tokio::test]
async fn pagina_limitada_continua_sem_perder_correspondencia_redigida() -> Result<()> {
    let f = Fixture::new();
    let target = f.add("a", EntryKind::User, &json!({"text":"alvo Token do banco: 482913"}));
    for _ in 0..consulta::SEARCH_SCAN_LIMIT {
        f.add("a", EntryKind::User, &json!({"text":"ruido Token do banco: 482913"}));
    }
    let first = f.search(json!({"query":"alvo","limit":1})).await;
    assert_eq!(first["status"], "partial");
    assert_eq!(first["results"], json!([]));
    assert_eq!(first["next"], target.id);
    assert!(first["nextCursor"].is_string());
    let next = f.search(json!({"cursor":first["nextCursor"],"limit":1})).await;
    let legacy = f.search(json!({"query":"alvo","from":first["next"],"limit":1})).await;
    assert_eq!(next, legacy);
    assert_eq!(next["results"][0]["id"], target.id);
    assert!(next["next"].is_null());
    assert!(next.get("nextCursor").is_none());

    // O orçamento/continuação tampouco pode distinguir termos removidos de termos ausentes.
    let hidden = f.search(json!({"query":"482913","limit":1})).await;
    let absent = f.search(json!({"query":"482915","limit":1})).await;
    for answer in [&hidden, &absent] {
        assert_eq!(answer["results"], json!([]));
        assert_eq!(answer["next"], target.id);
        assert!(answer["nextCursor"].is_string());
    }
    for answer in [hidden, absent] {
        let tail = f.search(json!({"cursor":answer["nextCursor"],"limit":1})).await;
        assert_eq!(tail["results"], json!([]));
        assert!(tail["next"].is_null());
        assert!(tail.get("nextCursor").is_none());
    }
    // A redação cria este termo: pré-filtrar o bruto também perderia correspondências permitidas.
    let redacted = f.search(json!({"query":"[removido]","limit":1})).await;
    assert_eq!(redacted["results"].as_array().context("results ausente")?.len(), 1);
    assert_eq!(redacted["results"][0]["text"], "ruido Token do banco: [removido]");
    Ok(())
}
