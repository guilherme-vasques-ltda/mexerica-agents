#![allow(clippy::unwrap_used)]
use super::*;
use crate::api::{devices::Caller, dispatch};
use std::time::Duration;

fn fixture() -> (Arc<Hub>, tempfile::TempDir) {
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let directory = builder.tempdir().unwrap();
    let store = Store::open(&directory.path().join("hub.db")).unwrap();
    let bot: BotConfig = serde_json::from_value(json!({"id":"bot","name":"Bot","backend":"codex"})).unwrap();
    store.save_bot(&bot).unwrap();
    let hub = Hub::new(
        store,
        "open-access".into(),
        crate::remote::identity::Identity::load_or_create(directory.path()).unwrap(),
        "test".into(),
        0,
    );
    (hub, directory)
}

async fn next_card(hub: &Hub, events: &mut tokio::sync::broadcast::Receiver<Value>) -> Entry {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), events.recv()).await.unwrap().unwrap();
        if event["entry"]["kind"] == "permission"
            && let Some(card) = event["entry"]["id"].as_str().and_then(|id| hub.store.entry(id))
            && card.data["status"] == "pending"
        {
            return card;
        }
    }
}

async fn answer(hub: &Arc<Hub>, card: &Entry, option: &str) {
    dispatch(
        hub,
        &Caller::Local,
        "respondPermission",
        json!({"source":"app","botId":card.bot_id,"entryId":card.id,"entryRev":card.rev,"optionId":option}),
    )
    .await
    .unwrap();
}

fn commands(hub: &Hub) -> bool {
    open_command_settings::revision(&hub.store.db.locked(), "bot").unwrap().is_some()
}

fn cwd(hub: &Hub) -> String {
    hub.store.bot("bot").unwrap().unwrap().config.cwd
}

fn grants(hub: &Hub) -> i64 {
    hub.store
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM kv WHERE k LIKE 'open_access_grant.%'", [], |r| r.get(0))
        .unwrap()
}

#[tokio::test]
async fn sem_o_aprovar_do_dono_nada_e_concedido_nem_em_aprovacao_automatica() {
    if !crate::environment::Environment::is_open() {
        return;
    }
    let (hub, _directory) = fixture();
    hub.store
        .db
        .locked()
        .execute(
            "INSERT OR REPLACE INTO donna_approvals(subject,level,chosen_by_user,legacy_permission) \
             VALUES('','full_access',1,'auto'),('bot','full_access',1,'auto')",
            [],
        )
        .unwrap();
    let lane = Lane::main("bot");
    let revalidate = || Ok(hub.store.bot("bot")?.unwrap().config);
    let args = json!({"access":"computer","reason":"rodar os testes"});
    let mut events = hub.events.subscribe();
    let mut waiting = Box::pin(call(&hub, &lane, 1, &args, revalidate));
    let card = tokio::select! {
        _ = &mut waiting => panic!("acesso concedido sem resposta do dono"),
        card = next_card(&hub, &mut events) => card,
    };
    assert_eq!(card.data["access"]["asUser"], true);
    assert!(card.data["detail"].as_str().unwrap().contains("sem isolamento"));
    drop(waiting);
    assert_eq!(hub.store.entry(&card.id).unwrap().data["status"], "cancelled");
    assert!(!commands(&hub));

    let (refused, ()) = tokio::join!(call(&hub, &lane, 2, &args, revalidate), async {
        let card = next_card(&hub, &mut events).await;
        answer(&hub, &card, "deny").await;
    });
    assert_eq!(refused.unwrap_err().to_string(), "open_tools_approval_denied");
    assert!(!commands(&hub));
    assert_eq!(grants(&hub), 0);
}

#[tokio::test]
async fn desfazer_tira_o_acesso_concedido_pelo_card() {
    if !crate::environment::Environment::is_open() {
        return;
    }
    let (hub, _directory) = fixture();
    let lane = Lane::main("bot");
    let revalidate = || Ok(hub.store.bot("bot")?.unwrap().config);
    let mut events = hub.events.subscribe();
    let args = json!({"access":"computer","reason":"rodar os testes"});
    let (granted, card) = tokio::join!(call(&hub, &lane, 1, &args, revalidate), async {
        let card = next_card(&hub, &mut events).await;
        answer(&hub, &card, "allow").await;
        card
    });
    assert_eq!(granted.unwrap()["granted"], true);
    assert!(commands(&hub));
    assert_eq!(hub.store.entry(&card.id).unwrap().data["access"]["state"], "granted");

    let undo = json!({"botId":"bot","entryId":card.id});
    let device = Caller::Device { key: "aparelho".into(), scopes: vec![], autoridade: None };
    assert!(dispatch(&hub, &device, "undoAccessGrant", undo.clone()).await.is_err());
    assert!(commands(&hub));
    dispatch(&hub, &Caller::Local, "undoAccessGrant", undo.clone()).await.unwrap();
    assert!(!commands(&hub));
    assert_eq!(hub.store.entry(&card.id).unwrap().data["access"]["state"], "undone");
    assert!(dispatch(&hub, &Caller::Local, "undoAccessGrant", undo).await.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn pasta_segue_as_regras_e_e_conferida_de_novo_ao_aplicar() {
    if !crate::environment::Environment::is_open() {
        return;
    }
    let (hub, _directory) = fixture();
    let lane = Lane::main("bot");
    let revalidate = || Ok(hub.store.bot("bot")?.unwrap().config);
    let home = crate::service::home();
    for path in [home.join(".ssh"), home.clone(), "/".into(), "/etc".into()] {
        let args = json!({"access":"folder","reason":"ler o projeto","path":path});
        assert!(call(&hub, &lane, 1, &args, revalidate).await.is_err(), "{path:?}");
    }
    assert!(call(&hub, &lane, 1, &json!({"access":"folder","reason":"ler"}), revalidate).await.is_err());

    let work = tempfile::tempdir().unwrap();
    let trocada = work.path().join("projeto");
    std::fs::create_dir(&trocada).unwrap();
    let mut events = hub.events.subscribe();
    let args = json!({"access":"folder","reason":"ler o projeto","path":trocada});
    let (refused, ()) = tokio::join!(call(&hub, &lane, 1, &args, revalidate), async {
        let card = next_card(&hub, &mut events).await;
        std::fs::remove_dir(&trocada).unwrap();
        std::os::unix::fs::symlink("/etc", &trocada).unwrap();
        answer(&hub, &card, "allow").await;
    });
    assert!(refused.is_err());
    assert_eq!(cwd(&hub), "");
    assert_eq!(grants(&hub), 0);

    let projeto = work.path().join("outro");
    std::fs::create_dir(&projeto).unwrap();
    let args = json!({"access":"folder","reason":"ler o projeto","path":projeto});
    let (granted, card) = tokio::join!(call(&hub, &lane, 2, &args, revalidate), async {
        let card = next_card(&hub, &mut events).await;
        answer(&hub, &card, "allow").await;
        card
    });
    granted.unwrap();
    assert_eq!(cwd(&hub), std::fs::canonicalize(&projeto).unwrap().to_str().unwrap());
    dispatch(&hub, &Caller::Local, "undoAccessGrant", json!({"botId":"bot","entryId":card.id})).await.unwrap();
    assert_eq!(cwd(&hub), "");
}
