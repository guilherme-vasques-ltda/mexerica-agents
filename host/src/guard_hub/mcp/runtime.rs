//! Credenciais transitórias, compartilhadas por conector e referenciadas por ponte.
use crate::LockExt;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const MAX_SECRETS: usize = 512 * 1024;
type ConnectorKey = (String, String); // namespace do banco, conector
type Lease = (String, String); // contexto, ponte

struct Secret {
    value: Zeroizing<String>,
    leases: HashSet<Lease>,
}

#[derive(Default)]
struct ConnectorSecrets {
    values: Vec<Secret>,
    leases: HashSet<Lease>, // Inclui pontes cujo ambiente não tem segredo.
    touched: u64,
    remote: HashMap<Lease, String>,
    local_discovery: HashMap<Lease, (String, i64, bool, Instant)>,
}

#[derive(Default)]
struct Cache {
    connectors: HashMap<ConnectorKey, ConnectorSecrets>,
    clock: u64,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Mutex::default);

impl Cache {
    fn bytes(&self) -> usize {
        self.connectors.values().flat_map(|c| &c.values).map(|s| s.value.len()).sum()
    }

    fn remember(&mut self, key: ConnectorKey, lease: &Lease, values: &[Zeroizing<String>]) {
        self.clock = self.clock.saturating_add(1);
        let entry = self.connectors.entry(key).or_default();
        entry.touched = self.clock;
        entry.leases.insert(lease.clone());
        for value in values {
            if let Some(secret) = entry.values.iter_mut().find(|s| s.value.as_str() == value.as_str()) {
                secret.leases.insert(lease.clone());
            } else {
                entry.values.push(Secret { value: value.clone(), leases: HashSet::from([lease.clone()]) });
            }
        }
        // Descartar invalida as leases junto dos valores. Observe então retém o resultado;
        // nunca recria um cache incompleto de uma ponte que ainda usa credenciais antigas.
        while self.bytes() > MAX_SECRETS {
            let oldest = self.connectors.iter().min_by_key(|(_, c)| c.touched).map(|(k, _)| k.clone());
            let Some(oldest) = oldest else { break };
            self.connectors.remove(&oldest);
        }
    }

    fn snapshot(&self, key: &ConnectorKey, context: &str, bridge: Option<&str>) -> Option<Zeroizing<Vec<String>>> {
        let entry = self.connectors.get(key)?;
        if !entry.leases.iter().any(|(c, b)| c == context && bridge.is_none_or(|expected| b == expected)) {
            return None;
        }
        // Inclui versões antigas ainda usadas por qualquer ponte deste conector.
        let mut values: Vec<String> = entry.values.iter().map(|s| s.value.to_string()).collect();
        values.sort_by_key(|value| std::cmp::Reverse(value.len()));
        Some(Zeroizing::new(values))
    }

    fn release(&mut self, key: &ConnectorKey, context: &str, bridge: Option<&str>) {
        if let Some(entry) = self.connectors.get_mut(key) {
            let keep = |(c, b): &Lease| c != context || bridge.is_some_and(|expected| b != expected);
            entry.leases.retain(keep);
            entry.remote.retain(|lease, _| keep(lease));
            entry.local_discovery.retain(|lease, _| keep(lease));
            for secret in &mut entry.values {
                secret.leases.retain(keep);
            }
            entry.values.retain(|s| !s.leases.is_empty());
            if entry.leases.is_empty() {
                self.connectors.remove(key);
            }
        }
    }
}

pub(crate) fn remember(namespace: &str, connector: &str, context: &str, bridge: &str, values: &[Zeroizing<String>]) {
    CACHE.locked().remember((namespace.into(), connector.into()), &(context.into(), bridge.into()), values);
}

pub(crate) fn snapshot(
    namespace: &str,
    connector: &str,
    context: &str,
    bridge: Option<&str>,
) -> Option<Zeroizing<Vec<String>>> {
    CACHE.locked().snapshot(&(namespace.into(), connector.into()), context, bridge)
}

pub(crate) fn release(namespace: &str, connector: &str, context: &str, bridge: Option<&str>) {
    CACHE.locked().release(&(namespace.into(), connector.into()), context, bridge);
}

pub(crate) fn forget_contexts(namespace: &str, contexts: &[String]) {
    let mut cache = CACHE.locked();
    let keys: Vec<_> = cache.connectors.keys().filter(|(ns, _)| ns == namespace).cloned().collect();
    for key in keys {
        for context in contexts {
            cache.release(&key, context, None);
        }
    }
}

/// Credenciais remotas conhecidas não dependem da heurística de env opaco.
pub(crate) fn remote_secret(value: &str, values: &mut Vec<Zeroizing<String>>) {
    if !value.is_empty() && !values.iter().any(|s| s.as_str() == value) {
        values.push(Zeroizing::new(value.into()));
    }
    if let Some(token) = value.get(..7).filter(|s| s.eq_ignore_ascii_case("bearer ")).and_then(|_| value.get(7..))
        && !token.is_empty()
        && !values.iter().any(|s| s.as_str() == token)
    {
        values.push(Zeroizing::new(token.into()));
    }
}

pub(super) fn remote_fingerprint(connector: &crate::market::Connector) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let config = Zeroizing::new(
        serde_json::json!({
            "url":connector.url,"headers":connector.headers,"command":connector.command,"binding":connector.oauth_binding,
            "servico":connector.servico,"tipo":connector.tipo,"liberacoes":connector.liberacoes,"space":connector.space
        }).to_string(),
    );
    Sha256::digest(config.as_bytes()).iter().fold(String::with_capacity(64), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

pub(super) fn bind_remote(namespace: &str, connector: &str, context: &str, bridge: &str, fingerprint: &str) {
    if let Some(entry) = CACHE.locked().connectors.get_mut(&(namespace.into(), connector.into())) {
        entry.remote.insert((context.into(), bridge.into()), fingerprint.into());
    }
}

pub(super) fn remote_matches(namespace: &str, connector: &str, context: &str, bridge: &str, fingerprint: &str) -> bool {
    CACHE
        .locked()
        .connectors
        .get(&(namespace.into(), connector.into()))
        .and_then(|entry| entry.remote.get(&(context.into(), bridge.into())))
        .is_some_and(|saved| saved == fingerprint)
}

pub(super) enum LocalDiscovery {
    Start { generation: i64, paginated: bool },
    Finish,
    Cancel,
}

pub(super) fn local_discovery(
    namespace: &str,
    connector: &str,
    context: &str,
    bridge: &str,
    request: &str,
    action: LocalDiscovery,
) -> anyhow::Result<Option<(i64, bool)>> {
    use anyhow::{anyhow, ensure};
    let mut cache = CACHE.locked();
    let lease = (context.into(), bridge.into());
    let entry = cache
        .connectors
        .get_mut(&(namespace.into(), connector.into()))
        .filter(|entry| entry.leases.contains(&lease))
        .ok_or_else(|| anyhow!("Lease MCP local indisponível"))?;
    let pending = entry.local_discovery.get(&lease).filter(|(_, _, _, at)| at.elapsed() < Duration::from_secs(90));
    if let LocalDiscovery::Start { generation, paginated } = action {
        if pending.is_some() {
            return Ok(None);
        }
        entry.local_discovery.insert(lease, (request.into(), generation, paginated, Instant::now()));
        return Ok(Some((generation, paginated)));
    }
    if matches!(action, LocalDiscovery::Cancel) {
        if pending.is_some_and(|(id, _, _, _)| id == request) {
            entry.local_discovery.remove(&lease);
        }
        return Ok(None);
    }
    let (id, generation, paginated, _) = pending.ok_or_else(|| anyhow!("Descoberta MCP local não iniciada"))?;
    ensure!(id == request, "Resposta de descoberta MCP local não correlacionada");
    let result = (*generation, *paginated);
    entry.local_discovery.remove(&lease);
    Ok(Some(result))
}

fn opaque(value: &str) -> bool {
    value.chars().count() >= 8 && value.bytes().any(|b| b.is_ascii_digit())
}

fn add(values: &mut Vec<Zeroizing<String>>, value: &str) {
    if opaque(value) && !values.iter().any(|s| s.as_str() == value) {
        values.push(Zeroizing::new(value.into()));
    }
}

static BEARER: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)\bbearer[ \t]+([a-z0-9._~+/-]+=*)").expect("padrão Bearer válido"));

fn components(value: &str, values: &mut Vec<Zeroizing<String>>) {
    add(values, value);
    for capture in BEARER.captures_iter(value) {
        add(values, &capture[1]);
    }
    // Fatias do valor protegido: um parser de URL criaria outra cópia não zerável.
    if let Some((_, rest)) = value.split_once("://")
        && let Some(authority) = rest.split(['/', '?', '#']).next()
        && let Some((user_info, _)) = authority.rsplit_once('@')
        && let Some((_, password)) = user_info.split_once(':')
    {
        add(values, password);
        let mut decoded = Zeroizing::new(Vec::with_capacity(password.len()));
        let mut bytes = password.bytes();
        while let Some(byte) = bytes.next() {
            if byte == b'%' {
                let hex = |b: u8| char::from(b).to_digit(16).and_then(|n| u8::try_from(n).ok());
                if let (Some(a), Some(b)) = (bytes.next().and_then(hex), bytes.next().and_then(hex)) {
                    decoded.push(a * 16 + b);
                }
            } else {
                decoded.push(byte);
            }
        }
        let decoded = Zeroizing::new(String::from_utf8_lossy(&decoded).into_owned());
        add(values, &decoded);
    }
}

/// Todo env é candidato, independente do nome; mesma forma Opaque do redator.
/// Os valores temporários e os componentes extraídos são zerados ao sair.
pub(super) fn collect(value: &str, values: &mut Vec<Zeroizing<String>>) {
    fn strings(value: &mut Value, values: &mut Vec<Zeroizing<String>>) {
        match value {
            Value::String(s) => components(&Zeroizing::new(std::mem::take(s)), values),
            Value::Array(items) => {
                for item in items {
                    strings(item, values);
                }
            }
            Value::Object(object) => {
                for item in object.values_mut() {
                    strings(item, values);
                }
            }
            _ => {}
        }
    }
    components(value, values);
    if let Ok(mut json) = serde_json::from_str::<Value>(value) {
        strings(&mut json, values);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn values(value: &str) -> Vec<Zeroizing<String>> {
        vec![Zeroizing::new(value.into())]
    }

    #[test]
    fn connector_cache_deduplicates_contexts_and_releases_only_the_closed_bridge() {
        let mut cache = Cache::default();
        let key = ("store".into(), "connector".into());
        cache.remember(key.clone(), &("first".into(), "a".into()), &values("synthetic-old123"));
        cache.remember(key.clone(), &("second".into(), "b".into()), &values("synthetic-old123"));
        assert_eq!(cache.bytes(), "synthetic-old123".len());
        cache.remember(key.clone(), &("second".into(), "b".into()), &values("synthetic-new456"));
        let snapshot = cache.snapshot(&key, "second", Some("b")).unwrap();
        assert_eq!(snapshot.len(), 2);
        cache.release(&key, "first", Some("wrong"));
        assert!(cache.snapshot(&key, "first", Some("a")).is_some());
        cache.release(&key, "first", Some("a"));
        assert_eq!(cache.snapshot(&key, "second", Some("b")).unwrap().len(), 2);
        cache.release(&key, "second", Some("b"));
        assert!(cache.connectors.is_empty());
    }

    #[test]
    fn eviction_is_bounded_and_missing_or_foreign_leases_cannot_observe() {
        let mut cache = Cache::default();
        let old = ("store-one".into(), "connector".into());
        let new = ("store-two".into(), "connector".into());
        cache.remember(old.clone(), &("context".into(), "bridge".into()), &values(&"1".repeat(MAX_SECRETS)));
        cache.remember(new.clone(), &("context".into(), "bridge".into()), &values("synthetic-new456"));
        assert!(cache.bytes() <= MAX_SECRETS);
        assert!(cache.snapshot(&old, "context", Some("bridge")).is_none());
        assert!(cache.snapshot(&new, "context", Some("foreign")).is_none());
        assert_eq!(cache.snapshot(&new, "context", Some("bridge")).unwrap().len(), 1);
        cache.remember(old.clone(), &("large".into(), "bridge".into()), &values(&"1".repeat(MAX_SECRETS + 1)));
        assert!(cache.snapshot(&old, "large", Some("bridge")).is_none());
        assert!(cache.bytes() <= MAX_SECRETS);
    }

    #[test]
    fn env_values_extract_opaque_bearer_json_headers_and_url_password() {
        let mut secrets = Vec::new();
        for value in [
            "true",
            "4096",
            "ordinaryword",
            "Tr0ub4dorAndHorse",
            r#"{"Authorization":"Bearer ntn_synthetic123","X-Custom":"nested456secret"}"#,
            "https://user:synthetic%31password@example.invalid/path",
        ] {
            collect(value, &mut secrets);
        }
        for expected in ["Tr0ub4dorAndHorse", "ntn_synthetic123", "nested456secret", "synthetic1password"] {
            assert!(secrets.iter().any(|s| s.as_str() == expected));
        }
        assert!(!secrets.iter().any(|s| ["true", "4096", "ordinaryword"].contains(&s.as_str())));
    }

    #[tokio::test]
    async fn missing_cache_retains_content_and_release_is_idempotent_after_context_revocation() {
        use crate::{
            api::devices::Caller,
            guard_hub::mcp::{call, contexto::Context},
            hub::Hub,
            remote::identity::Identity,
            store::Store,
        };
        use serde_json::json;
        let dir = std::env::temp_dir().join(format!("guard-runtime-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let bot = serde_json::from_value(json!({"id":"bot","name":"Fixture","connectors":["connector"]})).unwrap();
        store.save_bot(&bot).unwrap();
        let connector =
            serde_json::from_value(json!({"id":"connector","name":"Fixture","command":"not-executed"})).unwrap();
        crate::market::save_connectors(&store, &[connector]).unwrap();
        let context = Context::issue(&store, "bot", "connector", "bot", None).unwrap();
        let hub =
            Hub::new(store, "fixture".into(), Identity::load_or_create(&dir).unwrap(), "synthetic-token".into(), 0);
        let binding =
            crate::fork::sessoes::bindings::resolve(&hub.store.db.locked(), context.binding.as_deref().unwrap(), "bot")
                .unwrap();
        hub.start_bot_turn("bot", |r| {
            r.lane = Some(crate::store::Lane::main("bot"));
            r.session = binding.acp_session.clone();
        })
        .unwrap();
        let bridge = uuid::Uuid::new_v4().to_string();
        let answer = call(
            &hub,
            &Caller::Local,
            "donnaGuardObserve",
            json!({
                "context":context.id,"connector":"connector","bridgeId":bridge,"method":"tools/call",
                "frame":{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"synthetic-raw-secret123"}]}}
            }),
        )
        .await
        .unwrap();
        assert!(!answer.to_string().contains("synthetic-raw-secret123"));
        assert!(answer["message"]["error"].is_object());
        let (events, _) = hub.store.guard_test_snapshot().unwrap();
        let event: Value = serde_json::from_str(&events[0]).unwrap();
        assert_eq!(event["coverage"], "unsupported");
        assert_eq!(event["decision"], "retained");
        assert!(!events[0].contains("synthetic-raw-secret123"));
        hub.store.guard_context_remove(&context.id).unwrap();
        for _ in 0..2 {
            assert_eq!(
                call(
                    &hub,
                    &Caller::Local,
                    "donnaGuardRelease",
                    json!({
                        "context":context.id,"connector":"connector","bridgeId":bridge
                    })
                )
                .await
                .unwrap(),
                json!({"released":true})
            );
        }
        drop(hub);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
