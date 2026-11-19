//! Roteiro público fixo, da entrada documental já consumida pelo Host.
use anyhow::{Result, ensure};
use serde_json::{Value, json};

const DOCUMENT: &str = include_str!("../../../../docs/features/manual-do-harness.md");
const START: &str = "<!-- donna-workplace-guide:v1:start -->";
const END: &str = "<!-- donna-workplace-guide:v1:end -->";
const LIMIT: usize = 64 * 1024;

pub(super) fn extract(document: &str) -> Result<&str> {
    ensure!(document.matches(START).count() == 1 && document.matches(END).count() == 1, "guide_markers_invalid");
    let start = document.find(START).ok_or_else(|| anyhow::anyhow!("guide_markers_invalid"))? + START.len();
    let end = document.find(END).ok_or_else(|| anyhow::anyhow!("guide_markers_invalid"))?;
    let text = document.get(start..end).ok_or_else(|| anyhow::anyhow!("guide_markers_invalid"))?.trim();
    ensure!(!text.is_empty() && text.len() <= LIMIT, "guide_size_invalid");
    Ok(text)
}

pub(super) fn response() -> Result<Value> {
    Ok(json!({"guide":{"schema":1,"hubVersion":env!("CARGO_PKG_VERSION"),"text":extract(DOCUMENT)?}}))
}
