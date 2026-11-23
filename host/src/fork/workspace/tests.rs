#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Provas focais: epoch persistente, 409, corte de stream com evento em voo (stream, SSE e
//! Web local), bloqueio durável da revogação e Hub antigo sem vínculo gravado.
use super::*;
use crate::api::{self, devices::RejectCode};
use crate::remote::cloud::{self, Owner};
use crate::store::{Device, DeviceSource};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const MARCA: &str = "evento-em-voo-de-prova";

struct Pasta(PathBuf);
impl Pasta {
    fn nova() -> Self {
        let dir = std::env::temp_dir().join(format!("workspace-prova-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn hub(&self) -> Arc<Hub> {
        let store = Store::open(&self.0.join("hub.db")).unwrap();
        let identity = crate::remote::identity::Identity::load_or_create(&self.0).unwrap();
        Hub::new(store, "hub-prova".into(), identity, "token-prova".into(), 0)
    }
}
impl Drop for Pasta {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dono(hub: &Hub, user: Option<&str>) {
    cloud::update_status(hub, |s| s.owner = user.map(|u| Owner { user_id: u.into(), email: None }));
}

fn aparelho_conta(key: &str, grant: &str) -> Device {
    Device {
        key: key.into(),
        name: "Telefone de prova".into(),
        platform: "ios".into(),
        source: DeviceSource::Account,
        grant_id: Some(grant.into()),
        scopes: vec![Scope::Control],
        lease_until: Some(now_ms() + 600_000),
        created_at: now_ms(),
        last_seen_at: None,
    }
}

fn evento_em_voo(hub: &Hub) {
    let _ = hub.events.send(json!({"type": "usage", "usage": {"marca": MARCA}}));
}

#[test]
fn epoch_monotonico_e_persistente_apos_reinicio() {
    let pasta = Pasta::nova();
    let hub = pasta.hub();
    assert_eq!(hub.workspace.atual(), 0, "Hub antigo sem epoch gravado começa em 0");
    assert_eq!(bump(&hub, Motivo::RoleChanged).unwrap(), 1);
    assert_eq!(bump(&hub, Motivo::DeviceRevoked).unwrap(), 2);
    assert_eq!(hub.workspace.atual(), 2);
    drop(hub);

    let hub = pasta.hub();
    assert_eq!(hub.workspace.atual(), 2, "o epoch sobrevive ao reinício");
    assert_eq!(bump(&hub, Motivo::OwnerChanged).unwrap(), 3, "e nunca volta atrás");
}

#[tokio::test]
async fn pedido_com_epoch_divergente_recebe_409_estavel() {
    let pasta = Pasta::nova();
    let hub = pasta.hub();
    bump(&hub, Motivo::RoleChanged).unwrap();

    let ok = api::dispatch(&hub, &Caller::Local, METHOD, json!({"workspaceEpoch": 1})).await.unwrap();
    assert_eq!(ok["workspaceEpoch"], 1);
    let compat = api::dispatch(&hub, &Caller::Local, METHOD, json!({})).await.unwrap();
    assert_eq!(compat["workspaceEpoch"], 1, "sem epoch: aceito por compatibilidade");

    for velho in [json!(0), json!(7), json!("1"), Value::Null] {
        let erro = api::dispatch(&hub, &Caller::Local, METHOD, json!({"workspaceEpoch": velho})).await.unwrap_err();
        assert_eq!(api::error_status(&erro).0, axum::http::StatusCode::CONFLICT, "{velho}");
        assert_eq!(api::error_code(&erro), Some(EPOCH_MISMATCH));
    }
    let erro = api::dispatch(&hub, &Caller::Local, "hello", json!({"workspaceEpoch": 0})).await.unwrap_err();
    assert_eq!(api::error_code(&erro), Some(EPOCH_MISMATCH), "vale para qualquer método");

    let erro = api::scoped_events_stream_at(&hub, 0, None, &Caller::Local, None, Some(0)).err().unwrap();
    assert!(is_mismatch(&erro), "assinatura com epoch velho é recusada");
}

#[tokio::test]
async fn contexto_sem_dono_e_dono_que_fica_conhecido_nao_sobem_o_epoch() {
    let pasta = Pasta::nova();
    let hub = pasta.hub();
    let ctx = api::dispatch(&hub, &Caller::Local, METHOD, json!({})).await.unwrap();
    assert_eq!(
        ctx,
        json!({
            "schema": 1,
            "hubId": "hub-prova",
            "kind": "personal",
            "workspaceId": "personal:unbound",
            "userId": null,
            "role": "owner",
            "workspaceEpoch": 0,
            "capabilities": ["chat", "bots.manage", "devices.manage"],
        })
    );

    dono(&hub, Some("user_a.example.invalid"));
    assert_eq!(hub.workspace.atual(), 0, "o dono só ficou conhecido");
    let ctx = api::dispatch(&hub, &Caller::Local, METHOD, json!({})).await.unwrap();
    assert_eq!(
        (ctx["workspaceId"].as_str(), ctx["userId"].as_str()),
        (Some("personal:user_a.example.invalid"), Some("user_a.example.invalid"))
    );
    dono(&hub, None);
    dono(&hub, Some("user_a.example.invalid"));
    assert_eq!(hub.workspace.atual(), 0, "sumir e voltar o mesmo dono não é troca");

    dono(&hub, Some("user_b.example.invalid"));
    assert_eq!(hub.workspace.atual(), 1, "outro userId sobe o epoch");
    drop(hub);

    let hub = pasta.hub();
    dono(&hub, Some("user_b.example.invalid"));
    assert_eq!(hub.workspace.atual(), 1, "o mesmo dono depois do reinício não sobe");
    dono(&hub, Some("user_c.example.invalid"));
    assert_eq!(hub.workspace.atual(), 2);
}

#[tokio::test]
async fn stream_corta_no_bump_e_descarta_evento_em_voo() {
    use futures::StreamExt;
    let pasta = Pasta::nova();
    let hub = pasta.hub();
    let mut stream = Box::pin(
        api::scoped_events_stream_at(&hub, hub.store.current_rev(), None, &Caller::Local, None, Some(0)).unwrap(),
    );
    let hello = stream.next().await.unwrap();
    assert_eq!(hello["type"], "hello");
    assert_eq!(hello["workspace"]["workspaceEpoch"], 0);
    assert_eq!(hello["workspace"]["workspaceId"], "personal:unbound");

    evento_em_voo(&hub);
    bump(&hub, Motivo::RoleChanged).unwrap();
    evento_em_voo(&hub);
    let terminal = stream.next().await.unwrap();
    assert_eq!(terminal, json!({"type": "role_changed", "workspaceId": "personal:unbound", "workspaceEpoch": 1}));
    assert!(stream.next().await.is_none(), "nada sai depois do evento terminal");

    let mut parado =
        Box::pin(api::scoped_events_stream(&hub, hub.store.current_rev(), None, &Caller::Local, None).unwrap());
    assert_eq!(parado.next().await.unwrap()["type"], "hello");
    bump(&hub, Motivo::DeviceRevoked).unwrap();
    let terminal = tokio::time::timeout(Duration::from_secs(5), parado.next()).await.unwrap().unwrap();
    assert_eq!(terminal["type"], "workspace_revoked", "sem evento novo, o stream parado também termina");
    assert!(parado.next().await.is_none());
}

#[test]
fn revogacao_bloqueia_a_mesma_concessao_mesmo_apos_reinicio() {
    let pasta = Pasta::nova();
    let hub = pasta.hub();
    let preparar = |hub: &Hub| {
        dono(hub, Some("user_a.example.invalid"));
        hub.cloud_synced.store(true, std::sync::atomic::Ordering::Release);
    };
    preparar(&hub);
    hub.store.put_device(&aparelho_conta("chave-a", "grt_prova_a")).unwrap();
    let (_, antes) = crate::fork::autoridade::autenticar(&hub, "chave-a").unwrap();

    assert!(hub.revoke_device("chave-a").unwrap());
    assert_eq!(hub.workspace.atual(), 1, "revogar sobe o epoch");
    // Uma releitura atrasada (sincronização, corrida) devolve a mesma linha.
    hub.store.put_device(&aparelho_conta("chave-a", "grt_prova_a")).unwrap();
    assert_eq!(crate::fork::autoridade::autenticar(&hub, "chave-a").err(), Some(RejectCode::Revoked));
    assert_eq!(api::devices::authorize(&hub, "chave-a").err(), Some(RejectCode::Revoked));
    assert!(crate::fork::autoridade::conferir_caller(&hub, &antes).is_err(), "o caller capturado antes não volta");
    drop(antes);
    drop(hub);

    let hub = pasta.hub();
    preparar(&hub);
    assert_eq!(hub.workspace.atual(), 1);
    assert_eq!(crate::fork::autoridade::autenticar(&hub, "chave-a").err(), Some(RejectCode::Revoked));
    assert_eq!(api::devices::authorize(&hub, "chave-a").err(), Some(RejectCode::Revoked));

    hub.store.put_device(&aparelho_conta("chave-a", "grt_prova_nova")).unwrap();
    assert!(crate::fork::autoridade::autenticar(&hub, "chave-a").is_ok(), "uma concessão nova readmite");
}

#[test]
fn aparelho_local_revogado_so_volta_por_novo_pareamento() {
    let pasta = Pasta::nova();
    let hub = pasta.hub();
    let local =
        Device { source: DeviceSource::Local, grant_id: None, lease_until: None, ..aparelho_conta("chave-local", "x") };
    hub.store.put_device(&local).unwrap();
    assert!(crate::fork::autoridade::autenticar(&hub, "chave-local").is_ok());
    assert!(hub.revoke_device("chave-local").unwrap());
    hub.store.put_device(&local).unwrap();
    assert_eq!(crate::fork::autoridade::autenticar(&hub, "chave-local").err(), Some(RejectCode::Revoked));

    liberar_pareamento_local(&hub.store, "chave-local").unwrap();
    assert!(crate::fork::autoridade::autenticar(&hub, "chave-local").is_ok());
}

#[test]
fn codigos_estaveis_do_reject() {
    assert_eq!(codigo_estavel(RejectCode::Revoked), Some("device_revoked"));
    assert_eq!(codigo_estavel(RejectCode::Unauthorized), Some("device_unpaired"));
    assert_eq!(codigo_estavel(RejectCode::BadSignature), Some("device_unpaired"));
    assert_eq!(codigo_estavel(RejectCode::LeaseExpired), Some("lease_expired"));
    assert_eq!(codigo_estavel(RejectCode::RateLimited), None);
    assert_eq!(RejectCode::Revoked.close_code(), 4003);
}

async fn servir(hub: &Arc<Hub>) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = api::router(hub.clone()).into_make_service_with_connect_info::<SocketAddr>();
    (
        addr,
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        }),
    )
}

/// Lê o SSE até o fim: o `hello`, então evento em voo + bump, então o que sobrar.
async fn sse_corta(hub: &Hub, response: reqwest::Response) {
    use futures::StreamExt;
    assert_eq!(response.status(), 200);
    let mut body = response.bytes_stream();
    let mut texto = String::new();
    while !texto.contains("\n\n") {
        let chunk = tokio::time::timeout(Duration::from_secs(5), body.next()).await.unwrap().unwrap().unwrap();
        texto.push_str(std::str::from_utf8(&chunk).unwrap());
    }
    assert!(texto.contains("\"workspaceEpoch\":0"), "o hello traz o WorkspaceContext: {texto}");
    let epoch = hub.workspace.atual();
    evento_em_voo(hub);
    bump(hub, Motivo::DeviceRevoked).unwrap();
    evento_em_voo(hub);
    let mut resto = String::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(chunk) = body.next().await {
            resto.push_str(std::str::from_utf8(&chunk.unwrap()).unwrap());
        }
    })
    .await
    .expect("o stream fecha logo depois do evento terminal");
    assert!(!resto.contains(MARCA), "nenhum evento depois do bump: {resto}");
    let terminal = format!(
        "data:{}",
        json!({"type": "workspace_revoked", "workspaceId": "personal:unbound", "workspaceEpoch": epoch + 1})
    );
    assert!(resto.replace("data: ", "data:").contains(&terminal), "{resto}");
}

#[tokio::test]
async fn sse_local_corta_com_evento_em_voo_e_responde_409() {
    let pasta = Pasta::nova();
    let hub = pasta.hub();
    let (addr, servidor) = servir(&hub).await;
    let client = reqwest::Client::new();
    let r = client
        .get(format!("http://{addr}/events?since=0&workspaceEpoch=5"))
        .bearer_auth("token-prova")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    assert_eq!(r.json::<Value>().await.unwrap()["code"], EPOCH_MISMATCH);
    let r = client
        .post(format!("http://{addr}/api/{METHOD}"))
        .bearer_auth("token-prova")
        .json(&json!({"workspaceEpoch": 5}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    assert_eq!(r.json::<Value>().await.unwrap()["code"], EPOCH_MISMATCH);

    let r = client
        .get(format!("http://{addr}/events?since={}&workspaceEpoch=0", hub.store.current_rev()))
        .bearer_auth("token-prova")
        .send()
        .await
        .unwrap();
    sse_corta(&hub, r).await;
    servidor.abort();
}

#[tokio::test]
async fn web_local_corta_com_evento_em_voo() {
    let pasta = Pasta::nova();
    let site = pasta.0.join("site");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("index.html"), b"<!doctype html><html></html>").unwrap();
    let hub = pasta.hub();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let root = crate::fork::web_local::WebConfig::validate_dir(&site).unwrap();
    hub.set_web_local(Arc::new(crate::fork::web_local::WebState::new(root, addr.port()))).unwrap();
    let app = api::router(hub.clone()).into_make_service_with_connect_info::<SocketAddr>();
    let servidor = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let client = reqwest::Client::new();
    let login = client
        .post(format!("http://{addr}/web/auth/login"))
        .header("x-mexerica-web", "1")
        .header("origin", format!("http://{addr}"))
        .json(&json!({"token": "token-prova"}))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), 200);
    let cookie = login.headers().get("set-cookie").unwrap().to_str().unwrap().split(';').next().unwrap().to_owned();
    let csrf = login.json::<Value>().await.unwrap()["csrfToken"].as_str().unwrap().to_owned();
    let r = client
        .get(format!("http://{addr}/events?since={}&client=web&workspaceEpoch=0", hub.store.current_rev()))
        .header("cookie", &cookie)
        .header("x-mexerica-csrf", &csrf)
        .send()
        .await
        .unwrap();
    sse_corta(&hub, r).await;
    servidor.abort();
}

#[test]
fn migracao_idempotente_em_hub_antigo() {
    let pasta = Pasta::nova();
    drop(pasta.hub());
    let store = Store::open(Path::new(&pasta.0.join("hub.db"))).unwrap();
    migrate(&store.db.locked()).unwrap();
    assert_eq!(epoch_on(&store.db.locked()).unwrap(), 0);
    let spaces = store.spaces().unwrap();
    assert_eq!(spaces.spaces[0].id, crate::fork::spaces::PERSONAL, "Hub personal continua com a raiz de hoje");
}
