//! Fonte única de admissão PL1. Nem arquivo root nem request fornece raiz.
use super::protocol::{self, Context, PROFILE};
use anyhow::{Result, ensure};
use serde_json::{Value, json};

const ADMITTED: Option<&[u8]> = None;
const FIELDS: [&str; 17] = [
    "schema",
    "deploymentId",
    "env",
    "issuer",
    "origin",
    "accountId",
    "workerScript",
    "d1DatabaseId",
    "relayNamespaceId",
    "relayClass",
    "relayScript",
    "relayEnvironment",
    "relayWitnessName",
    "relayWitnessId",
    "schemaVersion",
    "bootstrap",
    "personalBootstrap",
];

fn exact(value: &Value, fields: &[&str]) -> bool {
    value.as_object().is_some_and(|m| m.len() == fields.len() && fields.iter().all(|key| m.contains_key(*key)))
}

fn name(value: &Value, max: usize) -> bool {
    value.as_str().is_some_and(|s| {
        !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    })
}

fn hex(value: &Value, length: usize) -> bool {
    value
        .as_str()
        .is_some_and(|s| s.len() == length && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

fn origin(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        reqwest::Url::parse(s).is_ok_and(|url| url.scheme() == "https" && url.origin().ascii_serialization() == s)
    })
}

fn root(value: &Value, personal: bool) -> Result<()> {
    let fields: &[&str] = if personal {
        &["profile", "wire", "issuerKeyId", "issuerPublicKey"]
    } else {
        &["issuerKeyId", "issuerPublicKey"]
    };
    ensure!(exact(value, fields) && name(&value["issuerKeyId"], 64), "invalid personal manifest root");
    let key = value["issuerPublicKey"].as_str().ok_or_else(|| anyhow::anyhow!("invalid public key"))?;
    let decoded = crate::remote::crypto::unb64_n::<32>(key)?;
    ensure!(crate::remote::crypto::b64(&decoded) == key, "noncanonical public key");
    if personal {
        ensure!(value["profile"] == PROFILE && value["wire"] == 1, "unsupported personal profile");
    }
    Ok(())
}

fn parse(raw: &[u8]) -> Result<Context> {
    let m = protocol::canonical(raw)?;
    ensure!(exact(&m, &FIELDS) && m["schema"] == 3 && m["schemaVersion"] == 7, "unsupported personal manifest");
    ensure!(m["env"] == "dev" || m["env"] == "main", "invalid personal environment");
    ensure!(
        name(&m["deploymentId"], 128) && origin(&m["issuer"]) && origin(&m["origin"]),
        "invalid personal deployment"
    );
    ensure!(
        hex(&m["accountId"], 32) && hex(&m["relayNamespaceId"], 32) && hex(&m["relayWitnessId"], 64),
        "invalid deployment identifiers"
    );
    let db = m["d1DatabaseId"].as_str().ok_or_else(|| anyhow::anyhow!("invalid database identifier"))?;
    let groups: Vec<&str> = db.split('-').collect();
    ensure!(
        groups.len() == 5 && groups.iter().zip([8, 4, 4, 4, 12]).all(|(s, n)| hex(&json!(s), n)),
        "invalid database identifier"
    );
    let deployment_id = m["deploymentId"].as_str().ok_or_else(|| anyhow::anyhow!("invalid deployment identifier"))?;
    ensure!(
        m["relayClass"] == "ComputerRelay"
            && ["workerScript", "relayScript", "relayEnvironment"].iter().all(|key| name(&m[*key], 128))
            && m["relayWitnessName"] == format!("donna/deployment/v4/{deployment_id}"),
        "invalid relay identity"
    );
    root(&m["bootstrap"], false)?;
    root(&m["personalBootstrap"], true)?;
    ensure!(
        m["bootstrap"]["issuerPublicKey"] != m["personalBootstrap"]["issuerPublicKey"],
        "personal root equals operator root"
    );
    let canonical: serde_json::Map<String, Value> = FIELDS.iter().map(|key| ((*key).into(), m[*key].clone())).collect();
    let operator = &m["bootstrap"];
    let personal = &m["personalBootstrap"];
    let mut canonical = Value::Object(canonical);
    canonical["bootstrap"] = json!({
        "issuerKeyId":operator["issuerKeyId"],"issuerPublicKey":operator["issuerPublicKey"]
    });
    canonical["personalBootstrap"] = json!({
        "profile":PROFILE,"wire":1,"issuerKeyId":personal["issuerKeyId"],"issuerPublicKey":personal["issuerPublicKey"]
    });
    ensure!(serde_json::to_vec(&canonical)? == raw, "noncanonical personal manifest order");
    Ok(Context(json!([
        PROFILE,
        1,
        m["env"],
        m["origin"],
        m["issuer"],
        m["deploymentId"],
        protocol::hash(raw),
        personal["issuerKeyId"]
    ])))
}

pub(super) fn admitted() -> Result<Option<Context>> {
    ADMITTED.map(parse).transpose()
}

/// A chave vem dos MESMOS bytes admitidos no binário, jamais do arquivo root.
pub(super) fn admitted_source() -> Result<Option<(Context, String)>> {
    ADMITTED
        .map(|raw| {
            let context = parse(raw)?;
            let m = protocol::canonical(raw)?;
            let key = m["personalBootstrap"]["issuerPublicKey"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("admitted root missing"))?;
            Ok((context, key.to_owned()))
        })
        .transpose()
}

#[cfg(test)]
pub(super) fn inspect_fixture(raw: &[u8]) -> Result<Context> {
    parse(raw)
}
