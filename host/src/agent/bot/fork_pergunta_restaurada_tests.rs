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

/// Sem recusa única no cartão original, a «recusar sempre» volta como recusa única; a restaurada nunca oferece
/// «sempre».
#[test]
fn recusar_sempre_volta_como_recusa_unica() {
    let dir = std::env::temp_dir().join(format!("pergunta-recusa-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir.join("test.db")).unwrap();
    let cfg: BotConfig = serde_json::from_value(json!({"id":"bot","name":"Bot","backend":"custom"})).unwrap();
    store.save_bot(&cfg).unwrap();
    let card = store
        .insert_entry(
            &Lane::main("bot"),
            EntryKind::Permission,
            1,
            &json!({"status":"pending","author":"bot","options":[
                {"optionId":"allow","kind":"allow_once"},
                {"optionId":"never","name":"Recusar sempre","kind":"reject_always"}]}),
        )
        .unwrap();
    store.expire_pending().unwrap();
    let old = store.entry(&card.id).unwrap();
    let restored = store.entry(old.data["restoredAs"].as_str().unwrap()).unwrap();
    let options = restored.data["options"].as_array().unwrap();
    let kinds: Vec<&str> = options.iter().filter_map(|o| o["kind"].as_str()).collect();
    assert_eq!(kinds, ["allow_once", "reject_once"]);
    assert_eq!(options[1]["name"], "Recusar", "rótulo neutro: o «sempre» não acompanha, e o cliente exige um nome");
    let _ = std::fs::remove_dir_all(dir);
}

/// Falha ao restaurar um cartão não interrompe a varredura do boot: cada cartão só expira, como antes.
#[test]
fn falha_ao_restaurar_um_cartao_expira_todos_sem_travar_o_boot() {
    let dir = std::env::temp_dir().join(format!("pergunta-falha-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir.join("test.db")).unwrap();
    let cfg: BotConfig = serde_json::from_value(json!({"id":"bot","name":"Bot","backend":"custom"})).unwrap();
    store.save_bot(&cfg).unwrap();
    let options = json!([{"optionId":"allow","kind":"allow_once"}]);
    let ids: Vec<String> = (0..2)
        .map(|_| {
            store
                .insert_entry(
                    &Lane::main("bot"),
                    EntryKind::Permission,
                    1,
                    &json!({"status":"pending","author":"bot","options":options}),
                )
                .unwrap()
                .id
        })
        .collect();
    store
        .db
        .locked()
        .execute_batch(
            "CREATE TRIGGER reject_restore BEFORE INSERT ON entries WHEN NEW.id LIKE '%-restaurada'
             BEGIN SELECT RAISE(ABORT, 'falha simulada'); END",
        )
        .unwrap();
    store.expire_pending().unwrap();
    for id in &ids {
        let card = store.entry(id).unwrap();
        assert_eq!(card.data["status"], "expired", "{id}");
        assert!(card.data.get("restoredAs").is_none(), "{id}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn falha_ao_expirar_um_cartao_nao_trava_o_boot_nem_aceita_resposta() {
    let f = Fixture::new();
    let target = crate::fork::tela_por_computador::host_target(&f.hub);
    let generation = crate::fork::spaces::config_generation_on(&f.hub.store.db.locked(), "bot").unwrap();
    f.hub.shutdown().await;
    let (blocked, others) = {
        let store = Store::open(&f.dir.join("test.db")).unwrap();
        let lane = Lane::main("bot");
        let blocked = store
            .insert_entry(
                &lane,
                EntryKind::Permission,
                1,
                &json!({"status":"pending","author":"bot","toolKind":"screen_access",
                    "screenAccess":{"target":target,"generation":generation,"expiresAt":crate::store::now_ms()+1_200_000}}),
            )
            .unwrap();
        let others: Vec<_> = (0..2)
            .map(|_| store.insert_entry(&lane, EntryKind::Permission, 1, &json!({"status":"pending"})).unwrap())
            .collect();
        store
            .db
            .locked()
            .execute_batch(
                "CREATE TRIGGER reject_expiration BEFORE UPDATE OF data ON entries
                 WHEN json_extract(OLD.data,'$.toolKind') = 'screen_access'
                  AND json_extract(NEW.data,'$.status') = 'expired'
                 BEGIN SELECT RAISE(ABORT, 'falha simulada'); END",
            )
            .unwrap();
        (blocked, others)
    };

    let hub = reopen(&f.dir);
    assert_eq!(hub.store.entry(&blocked.id).unwrap().data["status"], "pending");
    for other in others {
        assert_eq!(hub.store.entry(&other.id).unwrap().data["status"], "expired");
    }
    for option in ["always", "now", "deny"] {
        let error = answer(&hub, &blocked, json!(option)).await.unwrap_err();
        assert!(error.to_string().contains("não está mais pendente"), "{error:#}");
    }
    let grants: i64 =
        hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_screen_access", [], |row| row.get(0)).unwrap();
    assert_eq!(grants, 0);
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}
