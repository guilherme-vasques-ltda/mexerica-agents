#![allow(clippy::unwrap_used)]
use super::UNAVAILABLE;
use crate::LockExt;
use crate::api::{devices::Caller, dispatch};
use crate::fork::sessoes::{ExecutionRef, bindings};
use crate::hub::Hub;
use crate::market::{logins, requests};
use crate::remote::identity::Identity;
use crate::store::{BotConfig, Lane, Store};
use serde_json::json;
use std::{path::PathBuf, sync::Arc};

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
    login: logins::Login,
    execution: ExecutionRef,
}

impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("donnabot-secret-refs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.kv_set("screen_enabled", "1").unwrap();
        let password = format!("senha-ficticia-{}", uuid::Uuid::new_v4());
        let login = logins::save(&store, "portal.invalid", "usuario-ficticio", &password, "Conta senha").unwrap();
        let cfg: BotConfig = serde_json::from_value(json!({
            "id":"bot", "name":"Bot fictício", "backend":"fixture", "cwd":dir,
            "computer":true, "logins":[login.id]
        }))
        .unwrap();
        store.save_bot(&cfg).unwrap();
        let binding = bindings::issue(&store, &cfg.id, &cfg.id, &cfg.id, None).unwrap();
        store.memory_session_start(&cfg.id, "secret-refs-acp", false).unwrap();
        bindings::activate_for_config(&store, &binding, "secret-refs-acp", &cfg).unwrap();
        let hub = Hub::new(store, "teste".into(), Identity::load_or_create(&dir).unwrap(), "teste".into(), 0);
        hub.start_bot_turn(&cfg.id, |runtime| {
            runtime.lane = Some(Lane::main(&cfg.id));
            runtime.session = Some("secret-refs-acp".into());
        })
        .unwrap();
        let execution = bindings::capture(&hub, &cfg.id, binding.id.as_str()).unwrap();
        Self { hub, dir, login, execution }
    }

    fn changes(&self) -> i64 {
        self.hub.store.db.locked().query_row("SELECT total_changes()", [], |row| row.get(0)).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn referencias_recusam_nas_rotas_reais_sem_cofre_pedido_ou_tela() {
    let f = Fixture::new();
    // Ler um registro legado tentaria migrá-lo: total_changes denuncia essa leitura.
    let mut legacy = f.login.clone();
    legacy.label = "{{senha:SENTINELA_REF}}".into();
    f.hub.store.kv_set("logins", &serde_json::to_string(&[legacy]).unwrap()).unwrap();
    let vault = f.hub.store.kv_read("logins").unwrap();
    let config = serde_json::to_value(f.hub.store.bot("bot").unwrap().unwrap().config).unwrap();
    let screen = f.hub.screen.state();
    let changes = f.changes();
    let mut events = f.hub.events.subscribe();
    // Inclui formas que normalize_site descartaria em path, query ou userinfo.
    for locked in [false, true] {
        if locked {
            *f.hub.store.secret_key.locked() = None;
            assert!(logins::all(&f.hub.store).is_err(), "controle: o cofre está trancado");
        }
        for reference in [
            "{{senha:SENTINELA_REF}}",
            "{{senha:SENTINELA_SEM_FIM",
            "{{senha:",
            "https://portal.invalid/{{senha:SENTINELA_PATH}}",
            "portal.invalid?{{senha:SENTINELA_QUERY}}",
            "{{senha:SENTINELA_USERINFO}}@portal.invalid",
        ] {
            for (method, name, args) in [
                ("connectorCall", "request_login", json!({"site":reference,"reason":"SENTINELA_MOTIVO"})),
                ("computerCall", "type_login", json!({"login":reference,"field":"password","pid":123})),
            ] {
                let body = json!({"botId":"bot","name":name,"arguments":args});
                let error = dispatch(&f.hub, &Caller::Local, method, body).await.unwrap_err();
                assert_eq!(crate::api::error_status(&error).1, UNAVAILABLE);
                let direct = if name == "request_login" {
                    requests::call_in_execution(&f.hub, "bot", name, &args, &f.execution)
                } else {
                    crate::screen::computer(&f.hub, "bot", name, args, None).await
                };
                assert_eq!(format!("{:#}", direct.unwrap_err()), UNAVAILABLE);
                assert_eq!(f.changes(), changes, "nenhuma escrita, nem migração do cofre");
                assert_eq!(f.hub.screen.state(), screen, "sem controle, helper ou driver");
                assert!(!f.hub.computers.external("bot"));
                assert!(events.try_recv().is_err(), "nenhum evento de efeito");
            }
        }
    }
    assert_eq!(f.hub.store.kv_read("logins").unwrap(), vault);
    assert_eq!(serde_json::to_value(f.hub.store.bot("bot").unwrap().unwrap().config).unwrap(), config);
    assert!(f.hub.store.history("bot", i64::MAX, 100).unwrap().is_empty());
    assert!(f.hub.store.kv_read("vault.id").unwrap().is_none());
    assert!(f.hub.store.kv_read("vault.initialized").unwrap().is_none());
    assert!(f.hub.store.secret_key.locked().is_none());
}

#[tokio::test]
async fn referencia_recusa_antes_de_validar_outros_argumentos_sem_eco() {
    let f = Fixture::new();
    *f.hub.store.secret_key.locked() = None;
    for (method, name, args) in [
        ("connectorCall", "request_login", json!({"site":"{{senha:SENTINELA_SITE"})),
        ("computerCall", "type_login", json!({"login":"{{senha:SENTINELA_LOGIN", "field":"SENTINELA_CAMPO"})),
    ] {
        let error = dispatch(&f.hub, &Caller::Local, method, json!({"botId":"bot","name":name,"arguments":args}))
            .await
            .unwrap_err();
        let text = crate::api::error_status(&error).1;
        assert_eq!(text, "Esta referência de senha exige um caminho protegido ainda indisponível.");
        for forbidden in ["{{senha:", "SENTINELA", "portal.invalid", "usuario-ficticio"] {
            assert!(!text.contains(forbidden));
        }
    }
}

#[tokio::test]
async fn logins_comuns_preservam_pedido_normalizacao_e_selecao_legados() {
    let f = Fixture::new();
    let grant = json!({"botId":"bot","target":crate::fork::tela_por_computador::host_target(&f.hub),
        "enabled":true});
    crate::fork::tela_por_computador::call(&f.hub, &Caller::Local, &grant).unwrap();
    // O roteador também aceita o pedido comum, com a normalização preexistente.
    let result = dispatch(
        &f.hub,
        &Caller::Local,
        "connectorCall",
        json!({"botId":"bot","sessionBinding":f.execution.acp_binding(),"name":"request_login","arguments":{
            "site":"https://www.Portal.invalid/login", "reason":"Entrar no portal fictício"
        }}),
    )
    .await
    .unwrap();
    let entry = f.hub.store.entry(result["requestId"].as_str().unwrap()).unwrap();
    assert_eq!(entry.data["connectionRequest"]["site"], "portal.invalid");
    assert_eq!(entry.data["connectionRequest"]["status"], "pending");
    // Sem helper conectado: chegar à consulta do foco prova seleção sem digitar.
    #[cfg(not(windows))]
    for wanted in [&f.login.id, &f.login.label, &f.login.site] {
        let error = dispatch(
            &f.hub,
            &Caller::Local,
            "computerCall",
            json!({"botId":"bot","sessionBinding":f.execution.acp_binding(),"name":"type_login","arguments":{"login":wanted,"field":"password"}}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "The screen helper isn't running on this computer.");
    }
    assert_eq!(logins::all(&f.hub.store).unwrap().as_slice(), std::slice::from_ref(&f.login));
    assert_eq!(f.hub.store.bot("bot").unwrap().unwrap().config.logins.as_slice(), std::slice::from_ref(&f.login.id));
}
