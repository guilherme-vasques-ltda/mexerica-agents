//! Confirma somente a retirada local da geração exata; ausência em ACL não é ACK.
use crate::remote::crypto::{b64, unb64_n};
use crate::store::Store;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Fence {
    #[serde(rename = "fenceId")]
    pub id: String,
    pub grant_id: String,
    pub device_key: String,
}

#[derive(Serialize)]
pub(crate) struct FenceAck {
    pub fences: Vec<String>,
}

pub(crate) fn confirmable(store: &Store, fences: &[Fence]) -> Result<FenceAck> {
    if fences.len() > 200 {
        bail!("lote de cercas excede o limite");
    }
    let mut identities = HashMap::new();
    for fence in fences {
        let suffix = fence.grant_id.strip_prefix("grt_").unwrap_or_default();
        if suffix.is_empty()
            || suffix.len() > 60
            || !suffix.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || fence.id != format!("fnc_{}", fence.grant_id)
            || !unb64_n::<32>(&fence.device_key).is_ok_and(|key| b64(&key) == fence.device_key)
        {
            bail!("identidade de cerca inválida");
        }
        let identity = (&fence.grant_id, &fence.device_key);
        if identities.insert(&fence.id, identity).is_some_and(|old| old != identity) {
            bail!("identidade de cerca divergente");
        }
    }
    let devices = store.devices()?;
    let mut seen = HashSet::new();
    let fences = fences
        .iter()
        .filter(|f| {
            !devices.iter().any(|d| d.key == f.device_key && d.grant_id.as_deref() == Some(f.grant_id.as_str()))
        })
        .filter(|f| seen.insert(&f.id))
        .map(|f| f.id.clone())
        .collect();
    Ok(FenceAck { fences })
}

/// Um subconjunto honesto continua pendente; IDs estranhos nunca são aceitos.
pub(crate) fn acknowledged(value: &Value, sent: &FenceAck) -> Result<bool> {
    let received: Vec<String> = serde_json::from_value(value.get("fences").cloned().unwrap_or(Value::Null))?;
    let mut seen = HashSet::new();
    if received.iter().any(|id| !sent.fences.contains(id) || !seen.insert(id)) {
        bail!("confirmação de cerca divergente");
    }
    Ok(received.len() == sent.fences.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LockExt;
    use crate::store::{Device, DeviceSource};
    use std::path::Path;

    fn fence(grant: &str, key: &str) -> Fence {
        Fence { id: format!("fnc_{grant}"), grant_id: grant.into(), device_key: key.into() }
    }

    #[test]
    fn exact_generation_and_storage_errors_control_ack() -> Result<()> {
        let store = Store::open(Path::new(":memory:"))?;
        let key = b64(&[1; 32]);
        store.put_device(&Device {
            key: key.clone(),
            name: String::new(),
            platform: "linux".into(),
            source: DeviceSource::Account,
            grant_id: Some("grt_current".into()),
            scopes: vec![],
            lease_until: None,
            created_at: 0,
            last_seen_at: None,
        })?;
        let ack = confirmable(&store, &[fence("grt_current", &key), fence("grt_old", &key)])?;
        assert_eq!(ack.fences, vec!["fnc_grt_old"]);
        assert!(store.device(&key).is_some());
        assert!(confirmable(&store, &[fence("grt_old", &key), fence("grt_old", &b64(&[2; 32]))]).is_err());
        let oversized: Vec<_> = (0..201).map(|_| fence("grt_old", &key)).collect();
        assert!(confirmable(&store, &oversized).is_err());
        assert!(confirmable(&store, &[fence("bad", &key)]).is_err());
        store.db.locked().execute_batch("DROP TABLE devices")?;
        assert!(confirmable(&store, &[fence("grt_old", &key)]).is_err());
        Ok(())
    }

    #[test]
    fn response_requires_contract_and_only_requested_unique_ids() -> Result<()> {
        let sent = FenceAck { fences: vec!["fnc_grt_a".into(), "fnc_grt_b".into()] };
        assert!(acknowledged(&serde_json::json!({"fences": sent.fences}), &sent)?);
        assert!(!acknowledged(&serde_json::json!({"fences": ["fnc_grt_a"]}), &sent)?);
        assert!(acknowledged(&serde_json::json!({"fences": ["fnc_grt_other"]}), &sent).is_err());
        assert!(acknowledged(&serde_json::json!({"fences": ["fnc_grt_a", "fnc_grt_a"]}), &sent).is_err());
        assert!(acknowledged(&serde_json::json!({"fenced": []}), &sent).is_err());
        assert!(acknowledged(&Value::Null, &sent).is_err());
        Ok(())
    }
}
