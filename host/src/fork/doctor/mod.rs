//! Doctor do Hub: diagnóstico somente leitura da saúde do Hub e de um Space (`donnaHubDoctor`).
pub(crate) mod checks;
pub(crate) mod model;

use crate::{
    api::devices::{self, Caller, Forbidden},
    hub::Hub,
    store::Scope,
};
use anyhow::Result;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) const METHOD: &str = "donnaHubDoctor";
#[allow(dead_code, reason = "Evento canônico de invalidação do Doctor reservado para o stream do Hub.")]
pub(crate) const EVENT: &str = "donnaDoctorChanged";

/// Mesma guarda de autoridade de `control_tower::access::require` (Local ou Device com Control atual).
pub(crate) fn require(hub: &Hub, caller: &Caller) -> Result<()> {
    crate::fork::autoridade::conferir_caller(hub, caller)?;
    super::bot_editor::authorization::require_owner(caller)?;
    devices::permit(caller, METHOD)?;
    match caller {
        Caller::Local => Ok(()),
        Caller::Device { key, .. } => {
            let device = devices::authorize(hub, key).map_err(|_| Forbidden)?;
            if !device.scopes.contains(&Scope::Control) {
                return Err(Forbidden.into());
            }
            Ok(())
        }
        _ => Err(Forbidden.into()),
    }
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    let request: model::Read = serde_json::from_value(body)
        .map_err(|_| anyhow::anyhow!("invalid_field: solicitação do Doctor inválida ou campo desconhecido."))?;
    let _guard = hub.store.team_lock.lock().await;
    require(hub, caller)?;
    let report = checks::collect(hub, &request.space_id)?;
    require(hub, caller)?;
    hub.store.require_space(&request.space_id)?;
    Ok(serde_json::to_value(report)?)
}

/// Constrói a resposta `hub_offline` explícita para o CLI quando o Hub local não responde.
pub(crate) fn offline_report_json(space_id: &str, collected_at: i64) -> Value {
    json!({
        "version": 1,
        "hubId": Value::Null,
        "spaceId": space_id,
        "collectedAt": collected_at,
        "overall": "hub_offline",
        "checks": [
            {
                "id": "hub",
                "area": "hub",
                "status": "error",
                "code": "hub_offline",
                "count": Value::Null,
                "actionKind": "start_hub"
            }
        ]
    })
}

/// Código de saída do CLI `donnabot-host doctor` para uma resposta válida do Hub.
pub(crate) fn exit_code_for_report(report: &Value) -> i32 {
    match report["overall"].as_str() {
        Some("healthy" | "waiting_user") => 0,
        _ => 1,
    }
}

/// Formata saída legível para humanos do relatório do Doctor (online ou `hub_offline`).
pub(crate) fn format_human(report: &Value) -> String {
    let overall = report["overall"].as_str().unwrap_or("unavailable");
    let space = report["spaceId"].as_str().unwrap_or("personal");
    let mut lines = vec![format!("Hub Doctor ({space}): overall={overall}")];
    if overall == "hub_offline" {
        lines.push("  - hub [error] hub_offline (action: start_hub)".to_owned());
        lines.push(
            "Hub offline ou inacessível neste computador. Inicie com `donnabot-host serve` ou `donnabot-host install`."
                .to_owned(),
        );
        return lines.join("\n");
    }
    if let Some(checks) = report["checks"].as_array() {
        for c in checks {
            let area = c["area"].as_str().unwrap_or("unknown");
            let status = c["status"].as_str().unwrap_or("unavailable");
            let code = c["code"].as_str().unwrap_or("unknown");
            let action = c["actionKind"].as_str().unwrap_or("none");
            let count = c["count"].as_u64().map(|n| format!(" count={n}")).unwrap_or_default();
            lines.push(format!("  - {area} [{status}] {code}{count} (action: {action})"));
        }
    }
    if overall == "waiting_user" {
        lines.push("Sistema operacional; há itens aguardando sua aprovação ou resposta.".to_owned());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests;
