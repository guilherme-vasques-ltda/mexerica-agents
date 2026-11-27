#![allow(clippy::unwrap_used)]
//! Testes das RPCs administrativas do Mail Manager: callers, escopo, ciclo completo e Open.
use super::api::{METHODS, call};
use crate::{
    api::{
        devices::{Caller, Forbidden},
        dispatch, error_status,
    },
    fork::spaces::PERSONAL,
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, Device, DeviceSource, Scope, Store, now_ms},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
    work_space: String,
}

impl Fixture {
    async fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("mail-manager-api-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let donna: BotConfig = serde_json::from_value(json!({
            "id": "donna",
            "name": "Berga",
            "backend": "fixture",
            "cwd": dir,
            "manageTeam": true
        }))
        .unwrap();
        let zeta: BotConfig = serde_json::from_value(json!({
            "id": "bot-zeta",
            "name": "zeta",
            "backend": "fixture",
            "cwd": dir
        }))
        .unwrap();
        let alpha: BotConfig = serde_json::from_value(json!({
            "id": "bot-alpha",
            "name": "Alpha",
            "backend": "fixture",
            "cwd": dir
        }))
        .unwrap();
        let alpha_b: BotConfig = serde_json::from_value(json!({
            "id": "bot-alpha-a",
            "name": "alpha",
            "backend": "fixture",
            "cwd": dir
        }))
        .unwrap();
        let deleted: BotConfig = serde_json::from_value(json!({
            "id": "bot-deleted",
            "name": "Apagado",
            "backend": "fixture",
            "cwd": dir
        }))
        .unwrap();
        let group: BotConfig = serde_json::from_value(json!({
            "id": "grupo-pessoal",
            "name": "Grupo",
            "kind": "group",
            "members": ["donna", "bot-alpha"]
        }))
        .unwrap();
        for bot in [&donna, &zeta, &alpha, &alpha_b, &deleted, &group] {
            store.save_bot(bot).unwrap();
        }
        store.delete_bot("bot-deleted").unwrap();
        store.set_team_settings(&json!({ "primaryBotId": "donna" })).unwrap();

        let hub = Hub::new(store, "mail-test-hub".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0);
        let spaces = hub.store.spaces().unwrap();
        let updated = dispatch(
            &hub,
            &Caller::Local,
            "donnaSpaceCreate",
            json!({
                "hubId": hub.host_id,
                "expectedRevision": spaces.revision,
                "name": "Trabalho",
                "tile": {"color": "oceano", "style": "soft", "symbol": {"kind": "letters", "value": "TR"}}
            }),
        )
        .await
        .unwrap();
        let work_space = updated["spaces"].as_array().unwrap().iter().find(|s| s["id"] != PERSONAL).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();

        let work_bot: BotConfig = serde_json::from_value(json!({
            "id": "bot-trabalho",
            "name": "Trabalho Bot",
            "spaceId": work_space,
            "backend": "fixture",
            "cwd": dir
        }))
        .unwrap();
        hub.store.save_bot(&work_bot).unwrap();

        Self { hub, dir, work_space }
    }

    fn ponte_caller(&self) -> Caller {
        let binding =
            crate::fork::sessoes::bindings::issue(&self.hub.store, "donna", "donna", "donna", Some("acp")).unwrap();
        crate::fork::sessoes::bindings::activate(&self.hub.store, &binding, "acp").unwrap();
        let mut servers = vec![json!({"env": [], "args": ["mcp", "remote", "--context", "original"]})];
        crate::fork::ponte_credencial::provisionar(&self.hub, "donna", None, &binding, &mut servers).unwrap();
        let token = servers[0]["env"][0]["value"].as_str().unwrap();
        self.hub.pontes.autenticar(&self.hub, token).unwrap()
    }

    fn control_device_caller(&self) -> Caller {
        let device = Device {
            key: "dev-control".into(),
            name: "Aparelho".into(),
            platform: "macos".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: now_ms(),
            last_seen_at: None,
        };
        self.hub.store.put_device(&device).unwrap();
        crate::fork::autoridade::autenticar(&self.hub, &device.key).unwrap().1
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn sample_bodies() -> [(&'static str, Value); 5] {
    [
        ("donnaMailManager", json!({})),
        ("donnaMailManagerSave", json!({"enabled": true})),
        ("donnaMailManagerConnect", json!({"label": "Suporte", "addresses": ["suporte@loja.example.invalid"]})),
        ("donnaMailManagerDisconnect", json!({"mailboxId": "caixa-1"})),
        (
            "donnaMailManagerGrant",
            json!({
                "mailboxId": "caixa-1",
                "participantKind": "bot",
                "participantId": "donna",
                "permission": "view",
                "allowed": true
            }),
        ),
    ]
}

#[tokio::test]
async fn chamador_nao_local_e_recusado_em_todos_os_cinco_metodos() {
    let f = Fixture::new().await;
    let ponte = f.ponte_caller();
    let device_with_control = f.control_device_caller();
    let raw_device = Caller::Device { key: "raw-control".into(), scopes: vec![Scope::Control], autoridade: None };
    let pairing = Caller::Pairing { key: "pair-1".into() };

    for caller in [ponte, device_with_control, raw_device, pairing] {
        for (method, body) in sample_bodies() {
            let err_dispatch = dispatch(&f.hub, &caller, method, body.clone()).await.unwrap_err();
            assert_eq!(error_status(&err_dispatch).0, axum::http::StatusCode::FORBIDDEN, "{method} via dispatch");
            let err_direct = call(&f.hub, &caller, method, body).unwrap_err();
            assert!(err_direct.is::<Forbidden>(), "{method} direto no módulo");
        }
    }
}

#[tokio::test]
async fn space_id_e_validado_fail_closed() {
    let f = Fixture::new().await;

    // Ausente, null e PERSONAL passam.
    for body in [json!({}), json!({"spaceId": null}), json!({"spaceId": PERSONAL})] {
        let view = dispatch(&f.hub, &Caller::Local, "donnaMailManager", body).await.unwrap();
        assert_eq!(view["plugin"]["enabled"], false);
    }

    // Liga o plugin e cria uma caixa real antes de testar spaceId inválido com alvos válidos.
    dispatch(&f.hub, &Caller::Local, "donnaMailManagerSave", json!({"enabled": true})).await.unwrap();
    let connected = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaMailManagerConnect",
        json!({"label": "Suporte", "addresses": ["suporte@loja.example.invalid"]}),
    )
    .await
    .unwrap();
    let mailbox_id = connected["connectedMailboxId"].as_str().unwrap().to_owned();

    let valid_targets = [
        ("donnaMailManager", json!({})),
        ("donnaMailManagerSave", json!({"enabled": true})),
        ("donnaMailManagerConnect", json!({"label": "Nova", "addresses": ["nova@loja.example.invalid"]})),
        (
            "donnaMailManagerGrant",
            json!({
                "mailboxId": mailbox_id,
                "participantKind": "bot",
                "participantId": "donna",
                "permission": "view",
                "allowed": true
            }),
        ),
        ("donnaMailManagerDisconnect", json!({"mailboxId": mailbox_id})),
    ];

    // Outro Space existente, inexistente ou tipo não-string são recusados em todos os métodos.
    for (method, base) in &valid_targets {
        for bad_space in
            [json!(f.work_space), json!("space-inexistente"), json!(123), json!({}), json!(true), json!([])]
        {
            let mut body = base.clone();
            body["spaceId"] = bad_space.clone();
            let err = dispatch(&f.hub, &Caller::Local, method, body.clone()).await.unwrap_err();
            assert_ne!(
                error_status(&err).0,
                axum::http::StatusCode::OK,
                "{method} deveria recusar spaceId={bad_space}"
            );
            let direct = call(&f.hub, &Caller::Local, method, body).unwrap_err().to_string();
            assert!(
                direct.starts_with("mail_denied:") || direct.starts_with("mail_invalid:"),
                "{method} com spaceId={bad_space} devolveu erro inesperado: {direct}"
            );
        }
    }

    // No mesmo estado, os mesmos pedidos têm sucesso quando spaceId é PERSONAL.
    for (method, base) in valid_targets {
        let mut body = base;
        body["spaceId"] = json!(PERSONAL);
        dispatch(&f.hub, &Caller::Local, method, body)
            .await
            .unwrap_or_else(|e| panic!("{method} deveria aceitar spaceId=PERSONAL no mesmo estado: {e}"));
    }
}

#[tokio::test]
async fn scope_guard_recusa_space_id_nao_pessoal_nos_metodos_mail_manager() {
    let f = Fixture::new().await;
    for method in METHODS {
        for ok_body in [json!({}), json!({"spaceId": null}), json!({"spaceId": PERSONAL})] {
            crate::fork::spaces::guard(&f.hub, method, &ok_body)
                .unwrap_or_else(|e| panic!("scope::guard deveria aceitar {method} com {ok_body}: {e}"));
        }
        for bad_space in
            [json!(f.work_space), json!("space-inexistente"), json!(123), json!({}), json!(true), json!([])]
        {
            let err = crate::fork::spaces::guard(&f.hub, method, &json!({"spaceId": bad_space})).unwrap_err();
            assert_eq!(
                err.to_string(),
                "invalid_space",
                "scope::guard deveria recusar {method} com spaceId={bad_space} usando invalid_space"
            );
        }
    }
}

#[tokio::test]
async fn fluxo_completo_de_administracao_e_concessoes() {
    let f = Fixture::new().await;

    // Estado inicial: plugin desligado, sem transporte, sem pessoa, bots elegíveis ordenados.
    let initial = dispatch(&f.hub, &Caller::Local, "donnaMailManager", json!({})).await.unwrap();
    assert_eq!(initial["plugin"], json!({"enabled": false, "mode": "preview"}));
    assert_eq!(initial["transportAvailable"], false);
    assert_eq!(initial["personGrantsSupported"], false);
    assert_eq!(initial["mailboxes"], json!([]));

    let bots = initial["bots"].as_array().unwrap();
    let ids: Vec<&str> = bots.iter().map(|b| b["botId"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["donna", "bot-alpha", "bot-alpha-a", "bot-zeta"]);
    assert_eq!(bots[0], json!({"botId": "donna", "name": "Berga", "isPrimary": true}));
    assert_eq!(bots[1], json!({"botId": "bot-alpha", "name": "Alpha", "isPrimary": false}));
    assert_eq!(bots[2], json!({"botId": "bot-alpha-a", "name": "alpha", "isPrimary": false}));
    assert_eq!(bots[3], json!({"botId": "bot-zeta", "name": "zeta", "isPrimary": false}));

    // Conectar antes de ligar o plugin é negado.
    let before_enable = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaMailManagerConnect",
        json!({"label": "Suporte", "addresses": ["suporte@loja.example.invalid"]}),
    )
    .await
    .unwrap_err();
    assert!(before_enable.to_string().starts_with("mail_denied:"), "{before_enable}");

    // Ligar o plugin.
    let enabled = dispatch(&f.hub, &Caller::Local, "donnaMailManagerSave", json!({"enabled": true})).await.unwrap();
    assert_eq!(enabled["plugin"]["enabled"], true);

    // Conectar duas caixas fictícias; conectar não concede acesso a ninguém.
    let c1 = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaMailManagerConnect",
        json!({"label": "Suporte", "addresses": ["suporte@loja.example.invalid"]}),
    )
    .await
    .unwrap();
    let id1 = c1["connectedMailboxId"].as_str().unwrap().to_owned();
    assert_eq!(c1["mailboxes"].as_array().unwrap().len(), 1);
    assert_eq!(c1["mailboxes"][0]["grants"], json!([]));

    let c2 = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaMailManagerConnect",
        json!({"label": "Operações", "addresses": ["operacoes@loja.example.invalid"]}),
    )
    .await
    .unwrap();
    let id2 = c2["connectedMailboxId"].as_str().unwrap().to_owned();
    assert_ne!(id1, id2);
    assert_eq!(c2["mailboxes"].as_array().unwrap().len(), 2);
    for m in c2["mailboxes"].as_array().unwrap() {
        assert_eq!(m["grants"], json!([]));
    }

    // Conceder Ver/Enviar/Excluir independentemente para mais de um bot na mesma caixa.
    for (bot, perm) in
        [("bot-zeta", "send"), ("donna", "view"), ("donna", "delete"), ("bot-alpha", "view"), ("bot-alpha", "send")]
    {
        dispatch(
            &f.hub,
            &Caller::Local,
            "donnaMailManagerGrant",
            json!({
                "mailboxId": id1,
                "participantKind": "bot",
                "participantId": bot,
                "permission": perm,
                "allowed": true
            }),
        )
        .await
        .unwrap();
    }

    // Revogar uma permissão de um bot sem afetar as demais.
    let after_grants = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaMailManagerGrant",
        json!({
            "mailboxId": id1,
            "participantKind": "bot",
            "participantId": "bot-alpha",
            "permission": "view",
            "allowed": false
        }),
    )
    .await
    .unwrap();
    let box1 = after_grants["mailboxes"].as_array().unwrap().iter().find(|m| m["id"] == id1).unwrap();
    assert_eq!(
        box1["grants"],
        json!([
            {"participantKind": "bot", "participantId": "bot-alpha", "view": false, "send": true, "delete": false},
            {"participantKind": "bot", "participantId": "bot-zeta", "view": false, "send": true, "delete": false},
            {"participantKind": "bot", "participantId": "donna", "view": true, "send": false, "delete": true}
        ])
    );

    // Desligar revoga todas as concessões e religar não as devolve.
    let disabled = dispatch(&f.hub, &Caller::Local, "donnaMailManagerSave", json!({"enabled": false})).await.unwrap();
    assert_eq!(disabled["plugin"]["enabled"], false);
    for m in disabled["mailboxes"].as_array().unwrap() {
        assert_eq!(m["grants"], json!([]));
    }
    let reenabled = dispatch(&f.hub, &Caller::Local, "donnaMailManagerSave", json!({"enabled": true})).await.unwrap();
    assert_eq!(reenabled["plugin"]["enabled"], true);
    for m in reenabled["mailboxes"].as_array().unwrap() {
        assert_eq!(m["grants"], json!([]));
    }

    // Desconectar caixa limpa tudo.
    let after_disc1 =
        dispatch(&f.hub, &Caller::Local, "donnaMailManagerDisconnect", json!({"mailboxId": id1})).await.unwrap();
    let remaining: Vec<&str> =
        after_disc1["mailboxes"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(remaining, vec![id2.as_str()]);

    let after_disc2 =
        dispatch(&f.hub, &Caller::Local, "donnaMailManagerDisconnect", json!({"mailboxId": id2})).await.unwrap();
    assert_eq!(after_disc2["mailboxes"], json!([]));

    // Resposta não expõe nenhum campo além do contrato permitido em nenhum nível.
    let root_keys: BTreeSet<&str> = after_grants.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(
        root_keys,
        BTreeSet::from(["bots", "mailboxes", "personGrantsSupported", "plugin", "transportAvailable"])
    );
    let plugin_keys: BTreeSet<&str> = after_grants["plugin"].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(plugin_keys, BTreeSet::from(["enabled", "mode"]));

    let bots_arr = after_grants["bots"].as_array().unwrap();
    assert_ne!(bots_arr.as_slice(), &[] as &[Value]);
    for bot in bots_arr {
        let keys: BTreeSet<&str> = bot.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, BTreeSet::from(["botId", "isPrimary", "name"]));
    }

    let mailboxes_arr = after_grants["mailboxes"].as_array().unwrap();
    assert_ne!(mailboxes_arr.as_slice(), &[] as &[Value]);
    for mailbox in mailboxes_arr {
        let keys: BTreeSet<&str> = mailbox.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, BTreeSet::from(["addresses", "grants", "id", "label"]));
        for grant in mailbox["grants"].as_array().unwrap() {
            let grant_keys: BTreeSet<&str> = grant.as_object().unwrap().keys().map(String::as_str).collect();
            assert_eq!(grant_keys, BTreeSet::from(["delete", "participantId", "participantKind", "send", "view"]));
        }
    }
}

#[tokio::test]
async fn concessao_a_pessoa_e_recusada_com_mail_denied() {
    let f = Fixture::new().await;
    dispatch(&f.hub, &Caller::Local, "donnaMailManagerSave", json!({"enabled": true})).await.unwrap();
    let connected = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaMailManagerConnect",
        json!({"label": "Suporte", "addresses": ["suporte@loja.example.invalid"]}),
    )
    .await
    .unwrap();
    let mailbox_id = connected["connectedMailboxId"].as_str().unwrap();

    let err = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaMailManagerGrant",
        json!({
            "mailboxId": mailbox_id,
            "participantKind": "person",
            "participantId": "owner-1",
            "permission": "view",
            "allowed": true
        }),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().starts_with("mail_denied:"), "{err}");
}

#[tokio::test]
async fn enderecos_invalidos_duplicados_campos_desconhecidos_e_ids_vazios_sao_recusados() {
    let f = Fixture::new().await;
    dispatch(&f.hub, &Caller::Local, "donnaMailManagerSave", json!({"enabled": true})).await.unwrap();
    dispatch(
        &f.hub,
        &Caller::Local,
        "donnaMailManagerConnect",
        json!({"label": "Suporte", "addresses": ["suporte@loja.example.invalid"]}),
    )
    .await
    .unwrap();

    // Endereço duplicado entre caixas.
    let dup = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaMailManagerConnect",
        json!({"label": "Duplicada", "addresses": ["SUPORTE@loja.example.invalid"]}),
    )
    .await
    .unwrap_err();
    assert!(dup.to_string().starts_with("mail_invalid:"), "{dup}");

    // Endereço inválido.
    for bad in ["sem-arroba", "invalido@dominio", "a@b..example.invalid", "a <x@example.invalid>"] {
        let err =
            dispatch(&f.hub, &Caller::Local, "donnaMailManagerConnect", json!({"label": "Ruim", "addresses": [bad]}))
                .await
                .unwrap_err();
        assert!(err.to_string().starts_with("mail_invalid:"), "{bad}: {err}");
    }

    // Campos desconhecidos recusados em todos os 5 métodos.
    for (method, mut body) in sample_bodies() {
        body["extraField"] = json!(true);
        let err = dispatch(&f.hub, &Caller::Local, method, body).await.unwrap_err();
        assert!(err.to_string().starts_with("mail_invalid:"), "{method} com campo extra: {err}");
    }

    // mailboxId vazio e participantId vazio recusados.
    let empty_disc =
        dispatch(&f.hub, &Caller::Local, "donnaMailManagerDisconnect", json!({"mailboxId": "  "})).await.unwrap_err();
    assert!(empty_disc.to_string().starts_with("mail_invalid:"), "{empty_disc}");

    let empty_grant = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaMailManagerGrant",
        json!({
            "mailboxId": "",
            "participantKind": "bot",
            "participantId": "donna",
            "permission": "view",
            "allowed": true
        }),
    )
    .await
    .unwrap_err();
    assert!(empty_grant.to_string().starts_with("mail_invalid:"), "{empty_grant}");
}

#[test]
fn open_hub_libera_os_cinco_metodos_so_ao_dono_local() {
    let device = Caller::Device { key: "device".into(), scopes: vec![crate::store::Scope::Control], autoridade: None };
    for method in METHODS {
        assert!(crate::open_hub::open_allows(&Caller::Local, method, &json!({})), "{method} é do dono local na Open");
        assert!(!crate::open_hub::open_allows(&device, method, &json!({})), "{method} não é de aparelho na Open");
    }
}
