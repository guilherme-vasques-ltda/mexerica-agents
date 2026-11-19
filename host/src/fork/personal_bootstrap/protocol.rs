//! Bytes canônicos PL1. Não seleciona raiz, concede posse ou interpreta ACS4.
use anyhow::{Result, ensure};
use serde_json::Value;

pub(super) const PROFILE: &str = "personal-linux-v1";
pub(super) const LIMIT: usize = 16 * 1024;
const MAX_SAFE: u64 = 9_007_199_254_740_991;

fn safe_numbers(value: &Value) -> bool {
    match value {
        Value::Number(n) => n.as_u64().is_some_and(|n| n <= MAX_SAFE),
        Value::Array(items) => items.iter().all(safe_numbers),
        Value::Object(items) => items.values().all(safe_numbers),
        _ => true,
    }
}

fn canonical_limit(raw: &[u8], limit: usize) -> Result<Value> {
    ensure!(!raw.is_empty() && raw.len() <= limit, "invalid personal wire size");
    let value: Value = serde_json::from_slice(raw)?;
    ensure!(safe_numbers(&value) && serde_json::to_vec(&value)? == raw, "noncanonical personal wire");
    Ok(value)
}

pub(super) fn canonical(raw: &[u8]) -> Result<Value> {
    canonical_limit(raw, LIMIT)
}

pub(super) fn packet(raw: &[u8]) -> Result<Value> {
    canonical_limit(raw, 192 * 1024)
}

pub(super) fn hash(raw: &[u8]) -> String {
    crate::fork::bootstrap::codec::hash_bytes(raw)
}

/// Contexto derivado exclusivamente de um manifesto já validado.
pub(super) struct Context(pub(super) Value);
