//! Reinício com pergunta de permissão sem resposta: a pergunta volta uma vez, com outro cartão e escopo reduzido; a
//! resposta ao cartão antigo continua recusada, reinícios seguintes não duplicam e aprovar não executa nada.
use super::*;
use crate::api::devices::Caller;
use crate::store::Lane;

fn cards(store: &Store) -> Vec<crate::store::Entry> {
    store.history("bot", i64::MAX, 200).unwrap().into_iter().filter(|e| e.kind == "permission").collect()
}

async fn answer(hub: &Arc<Hub>, card: &crate::store::Entry, option: Value) -> anyhow::Result<Value> {
    crate::api::dispatch(
        hub,
        &Caller::Local,
        "respondPermission",
        json!({"botId":"bot","entryId":card.id,"entryRev":card.rev,"optionId":option}),
    )
    .await
}

fn reopen(dir: &std::path::Path) -> Arc<Hub> {
    Fixture::open(dir, Store::open(&dir.join("test.db")).unwrap())
}

#[tokio::test]
async fn pergunta_volta_uma_vez_exige_resposta_nova_e_nao_duplica_em_reinicio_duplo() {
    let f = Fixture::new();
    f.hub.shutdown().await;
    let original = {
        let store = Store::open(&f.dir.join("test.db")).unwrap();
        store
            .insert_entry(
                &Lane::main("bot"),
                EntryKind::Permission,
                1,
                &json!({"title":"Apagar a pasta","status":"pending","author":"bot","options":[
                    {"optionId":"allow","name":"Permitir","kind":"allow_once"},
                    {"optionId":"always","name":"Permitir sempre","kind":"allow_always"},
                    {"optionId":"deny","name":"Recusar","kind":"reject_once"}]}),
            )
            .unwrap()
    };

    let first = reopen(&f.dir);
    let old = first.store.entry(&original.id).unwrap();
    assert_eq!(old.data["status"], "expired", "o cartão antigo não volta a valer");
    let restored = first.store.entry(old.data["restoredAs"].as_str().unwrap()).unwrap();
    assert_ne!(restored.id, original.id);
    assert_eq!(restored.data["status"], "pending");
    assert_eq!(restored.data["restoredFrom"], json!(original.id));
    assert_eq!(restored.data["title"], "Apagar a pasta");
    let kinds: Vec<&str> =
        restored.data["options"].as_array().unwrap().iter().filter_map(|o| o["kind"].as_str()).collect();
    assert_eq!(kinds, ["allow_once", "reject_once"], "a pergunta restaurada nunca oferece «sempre»");
    assert!(answer(&first, &old, json!("allow")).await.is_err(), "a resposta ao cartão antigo continua recusada");
    assert!(answer(&first, &restored, json!("always")).await.is_err(), "escopo retirado não é aceito");
    first.shutdown().await;

    let second = reopen(&f.dir);
    assert_eq!(cards(&second.store).len(), 2, "o segundo reinício não cria outro cartão");
    let restored = second.store.entry(&restored.id).unwrap();
    assert_eq!(restored.data["status"], "pending", "a pergunta restaurada continua esperando a pessoa");
    assert!(answer(&second, &old, json!("allow")).await.is_err());
    answer(&second, &restored, json!("allow")).await.unwrap();
    let answered = second.store.entry(&restored.id).unwrap();
    assert_eq!(
        (answered.data["status"].clone(), answered.data["selected"].clone()),
        (json!("answered"), json!("allow"))
    );
    answer(&second, &restored, json!("allow")).await.unwrap();
    assert_eq!(second.store.entry(&restored.id).unwrap().rev, answered.rev, "repetir a resposta não regrava");
    let notes = second
        .store
        .history("bot", i64::MAX, 200)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "notice" && e.data["text"].as_str().is_some_and(|t| t.contains("Aprovação registrada")))
        .count();
    assert_eq!(notes, 1);
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(f.prompts().len(), 0, "aprovar a pergunta restaurada não executa nem envia nada");
    second.shutdown().await;

    let third = reopen(&f.dir);
    assert_eq!(cards(&third.store).len(), 2, "nem depois de respondida a pergunta volta outra vez");
    third.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn pergunta_de_autor_ausente_ou_de_conversa_alheia_so_expira() {
    let f = Fixture::new();
    f.hub.shutdown().await;
    let (missing, foreign) = {
        let store = Store::open(&f.dir.join("test.db")).unwrap();
        let options = json!([{"optionId":"allow","kind":"allow_once"}]);
        let other: BotConfig =
            serde_json::from_value(json!({"id":"alheio","name":"Alheio","backend":"custom"})).unwrap();
        store.save_bot(&other).unwrap();
        let missing = store
            .insert_entry(
                &Lane::main("bot"),
                EntryKind::Permission,
                1,
                &json!({"status":"pending","author":"inexistente","options":options}),
            )
            .unwrap();
        // O autor existe, mas a conversa é de outro bot, sem conversa de Space aberta nem sala: contexto alheio.
        let foreign = store
            .insert_entry(
                &Lane::main("alheio"),
                EntryKind::Permission,
                1,
                &json!({"status":"pending","author":"bot","options":options}),
            )
            .unwrap();
        (missing, foreign)
    };
    let hub = reopen(&f.dir);
    for id in [&missing.id, &foreign.id] {
        let card = hub.store.entry(id).unwrap();
        assert_eq!(card.data["status"], "expired");
        assert!(card.data.get("restoredAs").is_none(), "{id} não pode ganhar pergunta nova");
    }
    assert_eq!(cards(&hub.store).len(), 1);
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}
