//! Ajustes de conexão dos motores legados da Mexerica.
use crate::agent::auth::{EnvVar, Method, MethodKind};
use anyhow::Result;

pub(crate) fn add_methods(backend: &str, methods: &mut Vec<Method>) {
    if backend == "opencode" {
        methods.push(Method {
            id: "mexerica-opencode-go".into(),
            name: "Chave OpenCode Go".into(),
            description: Some("Guardada no cofre do Hub e entregue ao processo do OpenCode.".into()),
            kind: MethodKind::EnvVar {
                vars: vec![EnvVar {
                    name: "OPENCODE_API_KEY".into(),
                    label: "Chave OpenCode Go".into(),
                    secret: true,
                    optional: false,
                }],
                link: None,
            },
        });
    }
}

/// Corpo fechado do método exclusivo engineGoKeySave; slot global do Hub.
pub(crate) fn go_request(body: &serde_json::Value) -> bool {
    body["backend"] == "opencode"
        && body.as_object().is_some_and(|o| o.keys().all(|k| matches!(k.as_str(), "backend" | "vars" | "hubId")))
        && body["vars"].as_object().is_some_and(|vars| {
            vars.len() == 1 && vars.get("OPENCODE_API_KEY").is_some_and(|v| v.as_str().is_some_and(|s| s.len() <= 8192))
        })
}

pub(crate) async fn save_go(
    hub: &std::sync::Arc<crate::hub::Hub>,
    caller: &crate::api::devices::Caller,
    body: &serde_json::Value,
) -> Result<serde_json::Value> {
    anyhow::ensure!(go_request(body), "Chave Go inválida.");
    let authority = crate::fork::engine_setup::access::resolve(hub, caller)?;
    anyhow::ensure!(
        body.get("hubId").is_none_or(|id| id.as_str() == Some(hub.host_id.as_str())),
        "Hub da chave Go inválido."
    );
    crate::market::vault::unlock(hub.clone()).await?;
    // Serializa com outras gravações Go; não lê/migra um slot sem autoridade vigente.
    let _guard = crate::fork::engine_accounts::MUTATION.lock().await;
    authority.require_current(hub)?;
    let slot = "agent-env:opencode";
    let mut saved: std::collections::BTreeMap<String, String> = match hub.store.kv_read(slot)? {
        Some(raw) => serde_json::from_str(&crate::market::vault::open(&hub.store, slot, &raw)?)?,
        None => std::collections::BTreeMap::new(),
    };
    let key = body["vars"]["OPENCODE_API_KEY"].as_str().unwrap_or_default().trim();
    if key.is_empty() {
        saved.remove("OPENCODE_API_KEY");
    } else {
        saved.insert("OPENCODE_API_KEY".into(), key.to_owned());
    }
    let sealed = crate::market::vault::seal(&hub.store, slot, &serde_json::to_string(&saved)?)?;
    authority.with_db(hub, |db| crate::market::vault::write_sealed(db, slot, &sealed))?;
    if matches!(caller, crate::api::devices::Caller::Local) {
        let status = crate::agent::auth::check(&hub.store, "opencode").await?;
        if status["signedIn"] == true {
            crate::market::requests::agent_signed_in(hub, "opencode")?;
        }
        return Ok(status);
    }
    let mut methods = Vec::new();
    add_methods("opencode", &mut methods);
    Ok(
        serde_json::json!({"signedIn":null,"detail":null,"methods":methods,"savedEnv":saved.keys().collect::<Vec<_>>(),"login":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chave_go_usa_slot_do_motor() {
        let mut methods = Vec::new();
        add_methods("opencode", &mut methods);
        assert_eq!(methods.len(), 1);
        match &methods[0].kind {
            MethodKind::EnvVar { vars, .. } => {
                assert_eq!(vars[0].name, "OPENCODE_API_KEY");
                assert!(vars[0].secret);
            }
            _ => panic!("método deve guardar chave"),
        }
        let mut others = Vec::new();
        add_methods("grok", &mut others);
        assert!(others.is_empty());
    }
}
