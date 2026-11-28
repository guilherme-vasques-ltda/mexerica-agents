// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Marketplace methods: connectors, credentials and skills.

use super::devices::Caller;
use super::str_arg;
use crate::agent::bot::Cmd;
use crate::hub::Hub;
use crate::market;
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::ops::ControlFlow;
use std::sync::Arc;

pub(super) async fn call(
    hub: &Arc<Hub>,
    caller: &Caller,
    method: &str,
    b: Value,
    execution: Option<&crate::fork::sessoes::ExecutionRef>,
) -> Result<ControlFlow<Value, Value>> {
    market::reject_registry_request(method, &b)?;
    if hub.desktop_socket.get().is_some()
        && matches!(method, "importConnectors" | "installConnector" | "marketPersonalizado")
    {
        return Err(super::devices::Forbidden.into());
    }
    if crate::fork::skill_catalog::methods(method) {
        return crate::fork::skill_catalog::call(hub, caller, method, b).await.map(ControlFlow::Break);
    }
    if let Some(value) = market::contas_api::call(hub, caller, method, &b).await? {
        return Ok(ControlFlow::Break(value));
    }
    Ok(ControlFlow::Break(match method {
        "credentialUpdateConnector" => {
            let id = str_arg(&b, "id")?;
            let fields = b["fields"].as_object().ok_or_else(|| anyhow!("Credential fields are required"))?;
            market::update_connectors(&hub.store, |all| {
                let c = all.iter_mut().find(|c| c.id == id).ok_or_else(|| anyhow!("Unknown connector"))?;
                if hub.desktop_socket.get().is_some() && !market::contas::desktop_catalog(c) {
                    return Err(super::devices::Forbidden.into());
                }
                let values = if c.command.is_some() { &mut c.env } else { &mut c.headers };
                for (key, value) in fields {
                    if !values.contains_key(key) {
                        bail!("Unknown credential field");
                    }
                    let value = value
                        .as_str()
                        .filter(|s| !s.is_empty() && s.len() <= 64 * 1024)
                        .ok_or_else(|| anyhow!("Invalid credential value"))?;
                    values.insert(key.clone(), value.to_owned());
                }
                Ok(())
            })?;
            // A chave já está gravada: falha ao avisar um bot é aviso, nunca erro de gravação.
            let mut aviso = None;
            let bots = hub.store.bots().unwrap_or_else(|error| {
                tracing::warn!(%error, "chave salva; lista de bots indisponível para atualizar as ferramentas");
                aviso = Some("bot_nao_atualizado");
                Vec::new()
            });
            for row in bots {
                if !row.deleted
                    && !row.config.is_group()
                    && row.config.connectors.iter().any(|c| c == id)
                    && let Err(error) = hub.send_cmd(&row.config.id, Cmd::RefreshTools)
                {
                    tracing::warn!(%error, bot = %row.config.id, "chave salva; atualização das ferramentas do bot pendente");
                    aviso = Some("bot_nao_atualizado");
                }
            }
            if crate::environment::Environment::is_open() {
                return Ok(ControlFlow::Break(
                    json!({"saved": true, "verified": false, "status": "saved", "aviso": aviso}),
                ));
            }
            let mut ready = market::verify::verify(&hub.store, id, hub.bridge_destination()).await?;
            ready["saved"] = json!(true);
            ready["verified"] = json!(true);
            ready["aviso"] = json!(aviso);
            ready
        }
        "credentialStatus" => market::passwords::status(&hub.store)?,
        "credentialLogins" => market::logins::list(&hub.store)?,
        "credentialSaveLogin" => {
            // `createOnly: true` (MX-277, "login novo"): never replaces; the same site and username refuse (409).
            // Ausente = não é criação exclusiva; `null` e qualquer outro tipo recusam (sem salvar nada).
            let create_only = match b.get("createOnly") {
                None => false,
                Some(Value::Bool(v)) => *v,
                Some(_) => bail!("`createOnly` must be true or false"),
            };
            let save = if create_only { market::logins::create } else { market::logins::save };
            let login = save(
                &hub.store,
                str_arg(&b, "site")?,
                b["username"].as_str().unwrap_or_default(),
                str_arg(&b, "password")?,
                b["label"].as_str().unwrap_or_default(),
            )?;
            json!({"login": login.public()})
        }
        "credentialLabelLogin" => {
            let login =
                market::logins::relabel(&hub.store, str_arg(&b, "id")?, b["label"].as_str().unwrap_or_default())?;
            json!({"login": login.public()})
        }
        "credentialRemoveLogin" => {
            let id = str_arg(&b, "id")?;
            market::logins::remove(&hub.store, id)?;
            hub.login_codes.drop_login(id);
            json!({})
        }
        "credentialSetOnePassword" => {
            market::passwords::set_token(&hub.store, b["token"].as_str().unwrap_or_default()).await?
        }
        "connectorCall" => {
            let execution = execution.ok_or_else(|| anyhow!("chamada vencida: execução ausente"))?;
            crate::fork::sessoes::bindings::recheck(hub, execution)?;
            let result = market::requests::call_in_execution(
                hub,
                str_arg(&b, "botId")?,
                str_arg(&b, "name")?,
                &b["arguments"],
                execution,
            )?;
            hub.computers.mark_external(str_arg(&b, "botId")?);
            result
        }
        "connectorInfo" | "marketConnectors" => bail!(market::REGISTRY_UNAVAILABLE),
        "connectorRequestFinish" => {
            // D368: autoridade original antes da verificação externa e da fila.
            let owner = crate::fork::autoridade::Autoridade::capturar(hub, caller)?;
            market::requests::finish_as(hub, &b, &owner).await?
        }
        // MX-209: leitura do cartão para a janela segura; nunca devolve valor.
        "connectorRequestGet" => market::requests::inspect(hub, &b)?,
        "connectorVerify" => market::verify::verify(&hub.store, str_arg(&b, "id")?, hub.bridge_destination()).await?,
        "connectors" => market::list_connectors(&hub.store)?,
        "importConnectors" => {
            let space = market::contas::space_do_pedido(&hub.store, &b)?;
            market::import_connectors(&hub.store, str_arg(&b, "config")?, &space).await?
        }
        "installConnector" => {
            let c = market::add_custom_connector(&hub.store, &b).await?;
            json!({"connector": c})
        }
        "connectorSignInFinish" => {
            let c = market::oauth::finish(
                hub,
                Some(caller),
                str_arg(&b, "state")?,
                b["code"].as_str(),
                b["error"].as_str(),
                crate::fork::conectores::oauth_iss::do_pedido(&b)?,
            )
            .await?;
            json!({"connector": c.public()})
        }
        "connectorTarget" => {
            // O getter antigo não entrega credenciais sem contexto mediado.
            let mut request = b.clone();
            request["connector"] = json!(str_arg(&b, "id")?);
            request["transport"] = json!("http");
            crate::guard_hub::mcp::call(hub, caller, "donnaGuardRuntime", request).await?
        }
        "removeConnector" => {
            let id = str_arg(&b, "id")?;
            market::remove_connector(&hub.store, id)?;
            json!({})
        }
        _ => return Ok(ControlFlow::Continue(b)),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::BotConfig;

    #[tokio::test]
    async fn desktop_hub_refuses_custom_connector_creation_and_verification() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = crate::store::Store::open(std::path::Path::new(":memory:"))?;
        let saved = serde_json::from_value(json!({
            "id":"saved", "name":"Saved", "url":"https://fixture.example.invalid/mcp"
        }))?;
        market::save_connectors(&store, &[saved])?;
        let hub = Hub::new(
            store,
            "fixture".into(),
            crate::remote::identity::Identity::load_or_create(dir.path())?,
            "fixture".into(),
            19222,
        );
        hub.desktop_socket.set(dir.path().join("s")).expect("private transport");
        for (method, body) in [
            ("installConnector", json!({"name":"Fixture", "command":"unused"})),
            ("importConnectors", json!({"config":r#"{"mcpServers":{"Fixture":{"command":"unused"}}}"#})),
        ] {
            let error = call(&hub, &Caller::Local, method, body, None).await.unwrap_err();
            assert!(error.downcast_ref::<super::super::devices::Forbidden>().is_some());
        }
        let error = call(&hub, &Caller::Local, "connectorVerify", json!({"id":"saved"}), None).await.unwrap_err();
        assert!(error.to_string().contains("só conecta contas do catálogo"));
        assert_eq!(market::connectors(&hub.store)?.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn new_mcp_connectors_and_saved_logins_stay_off_without_revoking_existing_access() {
        let dir = std::env::temp_dir().join(format!("codync-connectors-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let store = crate::store::Store::open(std::path::Path::new(":memory:")).expect("memory store");
        for (id, on) in [("a", json!([])), ("b", json!(["old"]))] {
            let cfg: BotConfig = serde_json::from_value(
                json!({"id": id, "name": id, "backend": "fixture", "command": "true", "cwd": dir, "connectors": on, "notify": false}),
            )
            .expect("bot config");
            store.save_bot(&cfg).expect("save bot");
        }
        let connector = |id: &str| -> market::Connector {
            serde_json::from_value(json!({"id": id, "name": id, "url": "https://example.com/mcp"})).expect("connector")
        };
        market::save_connectors(&store, &[connector("old")]).expect("save");
        let hub = Hub::new(
            store,
            "test".into(),
            crate::remote::identity::Identity::load_or_create(&dir).expect("identity"),
            "test".into(),
            19222,
        );
        hub.start().expect("start");
        for name in ["Linear", "Mail"] {
            let added = call(&hub, &Caller::Local, "installConnector", json!({"name":name, "command":"unused"}), None)
                .await
                .unwrap();
            assert!(matches!(added, ControlFlow::Break(v) if v["connector"]["id"].is_string()));
        }
        let _ = call(
            &hub,
            &Caller::Local,
            "importConnectors",
            json!({"config":r#"{"mcpServers":{"Imported":{"command":"unused"}}}"#}),
            None,
        )
        .await
        .unwrap();
        let _ = call(
            &hub,
            &Caller::Local,
            "credentialSaveLogin",
            json!({"site":"example.test", "username":"fixture", "password":"fixture-only"}),
            None,
        )
        .await
        .unwrap();
        assert_eq!(market::connectors(&hub.store).unwrap().len(), 4);
        let on = |id: &str| hub.store.bot(id).expect("read").expect("bot").config;
        assert!(on("a").connectors.is_empty(), "new connectors stay off");
        assert_eq!(on("b").connectors, ["old"], "existing access is preserved");
        assert!(on("a").logins.is_empty() && on("b").logins.is_empty());
        let _ = call(&hub, &Caller::Local, "removeConnector", json!({"id":"old"}), None).await.unwrap();
        let replacement =
            call(&hub, &Caller::Local, "installConnector", json!({"name":"old", "command":"unused"}), None)
                .await
                .unwrap();
        assert!(matches!(replacement, ControlFlow::Break(v) if v["connector"]["id"] == "old-2"));
        assert_eq!(on("b").connectors, ["old"], "reinstalling a name cannot revive access to a new account");
        let _ = call(&hub, &Caller::Local, "removeConnector", json!({"id":"old-2"}), None).await.unwrap();
        let imported = call(
            &hub,
            &Caller::Local,
            "importConnectors",
            json!({"config":r#"{"mcpServers":{"old":{"command":"unused"}}}"#}),
            None,
        )
        .await
        .unwrap();
        assert!(matches!(imported, ControlFlow::Break(v) if v["items"][0]["id"] == "old-2"));
        assert_eq!(on("b").connectors, ["old"], "reimporting cannot revive access either");
        assert_eq!(hub.store.bot("a").unwrap().unwrap().config.skills, Vec::<String>::new());
        assert_eq!(hub.store.bot("b").unwrap().unwrap().config.skills, Vec::<String>::new());
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    /// MX-277: "login novo" (`createOnly`) nunca substitui; o login existente, ligado a outro bot, fica intacto.
    #[tokio::test]
    async fn credential_save_login_create_only_refuses_a_collision_and_leaves_the_existing_login_intact() {
        let dir = std::env::temp_dir().join(format!("codync-login-novo-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let store = crate::store::Store::open(std::path::Path::new(":memory:")).expect("memory store");
        let bot = |logins: &[String]| -> BotConfig {
            serde_json::from_value(
                json!({"id": "b", "name": "b", "backend": "fixture", "command": "true", "cwd": dir, "logins": logins, "notify": false}),
            )
            .expect("bot config")
        };
        store.save_bot(&bot(&[])).expect("save bot");
        let hub = Hub::new(
            store,
            "test".into(),
            crate::remote::identity::Identity::load_or_create(&dir).expect("identity"),
            "test".into(),
            19222,
        );
        let save = |body: Value| {
            let hub = hub.clone();
            async move { crate::api::dispatch(&hub, &Caller::Local, "credentialSaveLogin", body).await }
        };
        let first = save(json!({"site": "https://www.painel.exemplo.com/entrar", "username": "ana", "password": "pw-antiga", "label": "Painel da Ana"}))
            .await
            .unwrap();
        let id = first["login"]["id"].as_str().unwrap().to_owned();
        // The login is on for another bot.
        hub.store.save_bot(&bot(std::slice::from_ref(&id))).expect("bind");

        // Same site (normalized) + username, creation only: 409 `login_exists`, nothing modified.
        let err = save(json!({"site": "painel.exemplo.com", "username": " ana ", "password": "pw-nova", "label": "Outro", "createOnly": true}))
            .await
            .unwrap_err();
        let (status, message) = crate::api::error_status(&err);
        assert_eq!((status, message.as_str()), (axum::http::StatusCode::CONFLICT, market::logins::LOGIN_EXISTS));
        assert!(!err.to_string().contains("pw-"), "the refusal never echoes a password");
        let kept = market::logins::get(&hub.store, &id).unwrap();
        assert_eq!((kept.password.as_str(), kept.label.as_str()), ("pw-antiga", "Painel da Ana"));
        assert_eq!(market::logins::all(&hub.store).unwrap().len(), 1);
        assert_eq!(
            hub.store.bot("b").unwrap().unwrap().config.logins,
            std::slice::from_ref(&id),
            "the bot keeps exactly what it had"
        );

        // `createOnly` must be a boolean.
        for malformed in [json!("true"), json!(null), json!(1), json!([true]), json!({})] {
            let bad =
                save(json!({"site": "x.exemplo.com", "password": "pw", "createOnly": malformed})).await.unwrap_err();
            assert!(bad.to_string().contains("createOnly"), "{malformed}");
        }
        assert_eq!(market::logins::all(&hub.store).unwrap().len(), 1, "a malformed flag saves nothing");

        // A different username is created (new id, public projection only, no bot gets it).
        let novo =
            save(json!({"site": "painel.exemplo.com", "username": "bia", "password": "pw-bia", "createOnly": true}))
                .await
                .unwrap();
        assert_ne!(novo["login"]["id"], json!(id));
        assert!(!novo.to_string().contains("pw-bia"));
        assert_eq!(hub.store.bot("b").unwrap().unwrap().config.logins, std::slice::from_ref(&id));

        // Without the flag (or `false`) the normal path is unchanged: it replaces the password, same id, same bots.
        for body in [
            json!({"site": "painel.exemplo.com", "username": "ana", "password": "pw-trocada"}),
            json!({"site": "painel.exemplo.com", "username": "ana", "password": "pw-trocada-2", "createOnly": false}),
        ] {
            let again = save(body).await.unwrap();
            assert_eq!(again["login"]["id"], json!(id));
        }
        assert_eq!(market::logins::get(&hub.store, &id).unwrap().password, "pw-trocada-2");
        assert_eq!(hub.store.bot("b").unwrap().unwrap().config.logins, [id]);
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }
}
