//! Closed local health producer. No caller-selected identity, artifact or clock.
use crate::hub::Hub;
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Installed {
    schema: u8,
    machine_id: String,
    #[serde(rename = "artifactSHA256")]
    artifact_sha256: String,
    generation: String,
}

#[cfg(target_os = "linux")]
fn installed() -> Result<Installed> {
    use std::fs::OpenOptions;
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
    for path in ["/", "/etc", "/etc/donnabot"] {
        let m = std::fs::symlink_metadata(path)?;
        ensure!(m.is_dir() && m.uid() == 0 && m.mode() & 0o022 == 0, "unprotected health parent");
    }
    let f = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open("/etc/donnabot/cloud-control-v1.json")?;
    let m = f.metadata()?;
    ensure!(m.is_file() && m.uid() == 0 && m.mode() & 0o022 == 0 && m.len() <= 4096, "unprotected health config");
    let mut raw = Vec::new();
    f.take(4097).read_to_end(&mut raw)?;
    Ok(serde_json::from_slice(&raw)?)
}

pub(crate) fn observe(hub: &Hub) -> Result<Value> {
    #[cfg(not(target_os = "linux"))]
    anyhow::bail!("managed cloud health requires Linux");
    #[cfg(target_os = "linux")]
    {
        let cfg = installed()?;
        let actual = super::codec::hash_bytes(&running_artifact()?);
        produce(
            &hub.store,
            &hub.identity.sign,
            hub.cloud_synced.load(std::sync::atomic::Ordering::Acquire),
            &cfg,
            &actual,
            crate::store::now_ms(),
        )
    }
}

#[cfg(target_os = "linux")]
fn running_artifact() -> Result<Vec<u8>> {
    // Follow the kernel's live inode reference. current_exe() may name a deleted
    // dentry after an atomic update, even though this ELF is still executing.
    Ok(std::fs::read("/proc/self/exe")?)
}

fn produce(
    store: &crate::store::Store,
    key: &ed25519_dalek::SigningKey,
    synced: bool,
    cfg: &Installed,
    actual: &str,
    now: i64,
) -> Result<Value> {
    ensure!(cfg.schema == 1 && !cfg.machine_id.is_empty() && cfg.machine_id.len() <= 128, "invalid health machine");
    ensure!(
        cfg.artifact_sha256.len() == 64 && cfg.artifact_sha256.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid health artifact"
    );
    super::codec::b64::<32>(&cfg.generation)?;
    let inspect = store.bootstrap_inspection()?;
    ensure!(inspect["generation"].as_str() == Some(cfg.generation.as_str()), "health generation mismatch");
    let activation = store.bootstrap_activation_envelope(key, now)?;
    let active_hash =
        activation.as_ref().map(|a| serde_json::to_vec(&a[0]).map(|raw| super::codec::hash_bytes(&raw))).transpose()?;
    let state = if actual == cfg.artifact_sha256 && activation.is_some() && synced { "healthy" } else { "failed" };
    let computer = inspect["computerId"].as_str().context("health computer missing")?;
    let payload = json!([
        "donna/control-hub-health/v1",
        cfg.machine_id,
        computer,
        cfg.generation,
        actual,
        active_hash,
        now,
        state
    ]);
    let sig = crate::remote::crypto::sign(key, &serde_json::to_vec(&payload)?);
    Ok(json!([payload, crate::remote::crypto::b64(&sig)]))
}

#[cfg(test)]
pub(crate) fn test_snapshot(
    store: &crate::store::Store,
    key: &ed25519_dalek::SigningKey,
    generation: &str,
    actual: &str,
    now: i64,
    synced: bool,
) -> Result<Value> {
    produce(
        store,
        key,
        synced,
        &Installed {
            schema: 1,
            machine_id: "m_fixture".into(),
            artifact_sha256: "a".repeat(64),
            generation: generation.into(),
        },
        actual,
        now,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_never_accepts_caller_clock_key_or_extra_field() {
        let raw = r#"{"schema":1,"machineId":"m_a","artifactSHA256":"a","generation":"g","observedAt":1}"#;
        assert!(serde_json::from_str::<Installed>(raw).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reads_running_inode_after_unlink() {
        let executable = std::env::current_exe().unwrap();
        if std::env::var_os("DONNABOT_HEALTH_INODE_CHILD").is_some() {
            assert_eq!(executable.file_name().unwrap(), "health-probe");
            assert!(
                executable
                    .parent()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with("donna-health-elf-inode-")
            );
            let expected = super::super::codec::hash_bytes(&std::fs::read(&executable).unwrap());
            std::fs::remove_file(&executable).unwrap();
            // Reproduces the previous implementation's failure after rename/unlink.
            assert!(std::fs::read(std::env::current_exe().unwrap()).is_err());
            assert_eq!(super::super::codec::hash_bytes(&running_artifact().unwrap()), expected);
            println!("inode-health-proof-ok");
            return;
        }
        let directory = std::env::temp_dir().join(format!("donna-health-elf-inode-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let probe = directory.join("health-probe");
        if let Err(error) = std::fs::hard_link(&executable, &probe) {
            assert_eq!(error.raw_os_error(), Some(libc::EXDEV));
            std::fs::copy(&executable, &probe).unwrap();
        }
        let result = std::process::Command::new(&probe)
            .args(["--exact", "fork::bootstrap::health::tests::reads_running_inode_after_unlink", "--nocapture"])
            .env("DONNABOT_HEALTH_INODE_CHILD", "1")
            .output();
        let _ = std::fs::remove_file(&probe);
        std::fs::remove_dir(directory).unwrap();
        let output = result.unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(String::from_utf8_lossy(&output.stdout).contains("inode-health-proof-ok"));
    }
}
