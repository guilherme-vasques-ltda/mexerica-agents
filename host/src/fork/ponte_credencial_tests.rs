//! D70: fronteira de transporte e revogação; nenhum segredo real.
#![allow(clippy::unwrap_used)]
use super::*;
use std::sync::Arc;

struct Fixture {
    hub: Arc<Hub>,
    binding: crate::fork::sessoes::bindings::BindingRef,
    dir: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let store = crate::store::Store::open(std::path::Path::new(":memory:")).unwrap();
        store.save_bot(&serde_json::from_value(json!({"id":"bot","name":"Bot"})).unwrap()).unwrap();
        let binding = crate::fork::sessoes::bindings::issue(&store, "bot", "bot", "bot", Some("acp")).unwrap();
        crate::fork::sessoes::bindings::activate(&store, &binding, "acp").unwrap();
        let dir = std::env::temp_dir().join(format!("ponte-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Hub::new(
            store,
            "hub".into(),
            crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
            "local-test".into(),
            0,
        );
        Self { hub, binding, dir }
    }
    fn issue(&self) -> String {
        let mut servers = vec![json!({"env":[],"args":["mcp","remote","--context","original"]})];
        provisionar(&self.hub, "bot", None, &self.binding, &mut servers).unwrap();
        servers[0]["env"][0]["value"].as_str().unwrap().into()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn http_recusa_dono_rotas_externas_e_bearer_invalido_sem_promover_a_local() {
    let f = Fixture::new();
    f.hub.start().unwrap();
    let token = f.issue();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = crate::api::router(f.hub.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
    });
    let client = reqwest::Client::new();
    for method in [
        "nuncaFazer",
        "nuncaFazerAdicionar",
        "nuncaFazerEditar",
        "nuncaFazerRemover",
        "nuncaFazerReordenar",
        "instrucaoGeral",
        "instrucaoGeralSalvar",
        "pairing",
        "decideAccessRequest",
        "setApproval",
        "devices",
        "claimSign",
        "donnaConectorDecidir",
        "donnaApprovalSet",
        "setCloud",
        "bootstrapInstallGrant",
        "unclaim",
        "donnaWorkplaceStatus",
    ] {
        let r = client
            .post(format!("http://{addr}/api/{method}"))
            .bearer_auth(&token)
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 403, "{method}");
    }
    for route in ["/events", "/term/test", "/channel?v=1", "/health"] {
        let r = client.get(format!("http://{addr}{route}")).bearer_auth(&token).send().await.unwrap();
        assert_eq!(r.status(), 403, "{route}");
    }
    let r = client
        .post(format!("http://{addr}/ingest/statusline"))
        .bearer_auth(&token)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    let r = client
        .post(format!("http://{addr}/api/devices"))
        .bearer_auth("invalido")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    // Método permitido chega ao consumidor e lista as ferramentas reais, sem autoridade Local.
    let r = client
        .post(format!("http://{addr}/api/computerTools"))
        .bearer_auth(&token)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let r = client
        .post(format!("http://{addr}/api/chatCall"))
        .bearer_auth(&token)
        .json(&json!({"botId":"outro","sessionBinding":f.binding.id.as_str()}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    crate::api::dispatch(&f.hub, &Caller::Local, "stop", json!({"botId":"bot"})).await.unwrap();
    let r = client
        .post(format!("http://{addr}/api/computerTools"))
        .bearer_auth(&token)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    f.hub.shutdown().await;
    server.abort();
}

#[test]
fn rotacao_revogacao_e_contexto_nao_adotam_outra_sessao() {
    let f = Fixture::new();
    let old = f.issue();
    let new = f.issue(); // Mesmo caminho de provisionamento usado por new e load.
    assert!(!crypto::ct_eq(old.as_bytes(), new.as_bytes()));
    assert!(f.hub.pontes.autenticar(&f.hub, &old).is_none());
    let caller = f.hub.pontes.autenticar(&f.hub, &new).unwrap();
    assert!(matches!(&caller, Caller::Ponte { bot, sessao, .. } if bot == "bot" && sessao == f.binding.id.as_str()));
    for method in PERMITIDOS {
        assert!(crate::api::devices::permit(&caller, method).is_ok(), "{method}");
    }
    assert!(conferir(&f.hub, &caller, "donnaGuardRuntime", &json!({"context":"original"})).is_ok());
    assert!(conferir(&f.hub, &caller, "donnaGuardRuntime", &json!({"context":"roubado"})).is_err());
    assert!(conferir(&f.hub, &caller, "chatCall", &json!({"botId":"bot","sessionBinding":"outra"})).is_err());
    crate::fork::sessoes::bindings::revoke(&f.hub.store, "bot", Some("acp")).unwrap();
    assert!(f.hub.pontes.autenticar(&f.hub, &new).is_none());
}
