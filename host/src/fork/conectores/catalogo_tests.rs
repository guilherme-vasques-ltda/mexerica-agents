//! D70: contrato preparatório pelo dispatch real, sem provedor, cofre ou Source.
use crate::{LockExt, api, api::devices::Caller, hub::Hub, store::Scope};
use serde_json::{Value, json};

fn estado(hub: &Hub) -> Value {
    let db = hub.store.db.locked();
    let kv = db
        .prepare("SELECT k,v FROM kv ORDER BY k")
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let bots = db
        .prepare("SELECT id,config FROM bots ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let contagens: (i64, i64, i64, i64) = db
        .query_row(
            "SELECT (SELECT COUNT(*) FROM donna_conector_tentativas),
            (SELECT COUNT(*) FROM donna_conector_cartoes), (SELECT COUNT(*) FROM entries), total_changes()",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    json!({"kv":kv,"bots":bots,"contagens":contagens})
}

#[tokio::test]
async fn catalogo_preparatorio_local_no_dispatch_nao_abre_cofre_nem_habilita_operacao() {
    let hub = super::oauth_binding::tests::hub();
    *hub.store.secret_key.locked() = None; // Store em memória começa com chave sintética.
    // Sentinela deliberadamente não cifrada/ilegível: esta consulta não abre o registro.
    hub.store.kv_set("connectors", "registro-sintetico-nao-abrir").unwrap();
    hub.store
        .save_bot(
            &serde_json::from_value(json!({"id":"bot", "name":"Fixture",
        "backend":"codex", "connectors":["manual"]}))
            .unwrap(),
        )
        .unwrap();
    assert!(hub.store.secret_key.locked().is_none());
    let antes = estado(&hub);
    let result = api::dispatch(&hub, &Caller::Local, "donnaConectorCapability", json!({})).await.unwrap();
    assert_eq!(
        result,
        json!({
            "decisionProtocol":0, "available":false, "reason":"fonte_05_e_apresentacao_06_indisponiveis",
            "catalogo":{"versao":1,"servicos":[{
                "id":"linear", "nome":"Linear", "estado":"preparatorio",
                "conectarDisponivel":false, "acessosDisponiveis":[], "perfis":[{
                    "id":"leitura", "nome":"Leitura", "url":"https://mcp.linear.app/mcp/readonly",
                    "resource":"https://mcp.linear.app/mcp/readonly", "issuer":"https://mcp.linear.app",
                    "scopesPrevistos":["read"]
                },{
                    "id":"leitura_criacao", "nome":"Leitura e criação", "url":"https://mcp.linear.app/mcp",
                    "resource":"https://mcp.linear.app/mcp", "issuer":"https://mcp.linear.app",
                    "scopesPrevistos":["read","write"]
                }]
            }]}
        })
    );
    let device = Caller::Device { key: "fixture-device".into(), scopes: vec![Scope::Control], autoridade: None };
    let error = api::dispatch(&hub, &device, "donnaConectorCapability", json!({})).await.unwrap_err();
    assert!(error.is::<api::devices::Forbidden>());
    for body in [
        json!({"url":"https://fixture.invalid", "scopes":["write"], "bot":"bot",
        "conta":"presumida", "available":true}),
        Value::Null,
    ] {
        let error = api::dispatch(&hub, &Caller::Local, "donnaConectorCapability", body).await.unwrap_err();
        assert_eq!(error.to_string(), "Campos desconhecidos");
    }
    assert_eq!(super::selected(&hub, "bot"), [] as [String; 0]);
    assert!(super::register(&hub, "bot", "linear", &crate::store::Lane::main("bot")).is_err());
    assert!(hub.conectores.channels.locked().is_empty());
    assert!(hub.store.secret_key.locked().is_none());
    assert_eq!(estado(&hub), antes);
}
