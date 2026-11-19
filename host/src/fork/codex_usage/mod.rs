//! Consumo global atual do Codex, sem turno, sessão ou fallback para rollouts.
//! Adaptado de Codync ded969dc / e9980b3b (Apache-2.0; `THIRD_PARTY.md`).
use crate::{
    LockExt,
    hub::Hub,
    store::Store,
    usage::{Provider, Window},
};
use anyhow::Result;
use serde_json::{Value, json};
use std::{ffi::OsString, path::Path, time::Duration};

// O RPC `usage` e o poller podem consultar juntos. Serializar consulta + publicação
// impede uma resposta antiga de sobrescrever a leitura seguinte; nenhum cache novo.
static READING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) async fn refresh(hub: &Hub) {
    let executable = crate::agent::backends::which("codex");
    let home = dirs::home_dir().unwrap_or_default();
    refresh_at(hub, executable.as_deref(), &home, environment(&hub.store), Duration::from_secs(20)).await;
}

fn environment(store: &Store) -> Vec<(OsString, OsString)> {
    crate::agent::environment::for_cli(store, "codex")
}

async fn refresh_at(
    hub: &Hub,
    executable: Option<&Path>,
    home: &Path,
    env: Vec<(OsString, OsString)>,
    timeout: Duration,
) {
    let _reading = READING.lock().await;
    let windows = if let Some(executable) = executable {
        read(executable, home, env, timeout).await.unwrap_or_default()
    } else {
        Vec::new()
    };
    replace(hub, windows);
}

async fn read(
    executable: &Path,
    home: &Path,
    env: Vec<(OsString, OsString)>,
    timeout: Duration,
) -> Result<Vec<Window>> {
    let response = tokio::time::timeout(timeout, async {
        let mut rpc = crate::fork::engine_setup::Rpc::start_usage(executable, home, env)?;
        rpc.initialize().await?;
        rpc.call(2, "account/rateLimits/read", json!({})).await
        // Drop do transporte existente encerra o filho e seu grupo em todos os caminhos.
    })
    .await??;
    Ok(parse(&response, crate::store::now_ms()))
}

pub(crate) fn parse(response: &Value, now: i64) -> Vec<Window> {
    // Um mapa presente é a leitura completa: não recuperar a compat de outro bucket
    // se Codex estiver ausente, vazio ou inválido.
    let rates =
        if let Some(map) = response.get("rateLimitsByLimitId") { &map["codex"] } else { &response["rateLimits"] };
    crate::usage::parse_codex(rates, now).into_iter().filter(|w| w.resets_at.is_none_or(|at| at > now)).collect()
}

fn replace(hub: &Hub, windows: Vec<Window>) {
    crate::fork::uso_historico::record(hub, "codex", &windows);
    // Uma única seção crítica mantém eventos parciais Claude que chegam durante o RPC.
    let mut usage = hub.usage.locked();
    usage.providers.retain(|p| p.id != "codex");
    if !windows.is_empty() {
        usage.providers.push(Provider {
            id: "codex",
            name: "Codex",
            windows,
            source: "cli",
            updated_at: crate::store::now_ms(),
        });
        usage.providers.sort_by_key(|p| p.id);
    }
    let _ = hub.events.send(json!({"type":"usage", "usage": &*usage}));
}

#[cfg(all(test, unix))]
mod tests;
