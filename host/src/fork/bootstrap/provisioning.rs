//! Read the public configuration installed by the qualified provider operator.
//! This is not an installer or an attestation producer. No RPC/body/environment
//! may choose the root or this path; the E/M/P ceremony remains a separate gate.
use crate::hub::Hub;
use anyhow::Result;

pub(crate) fn load(hub: &Hub) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        linux::load(hub)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = hub;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{Hub, Result};
    use crate::fork::bootstrap::clock;
    use crate::store::bootstrap::TrustedGeneration;
    use anyhow::{Context, ensure};
    use serde::Deserialize;
    use std::fs::{Metadata, OpenOptions};
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
    use std::time::Duration;

    const FILE: &str = "/etc/donnabot/bootstrap-v1.json";
    const LIMIT: u64 = 16 * 1024;
    const SAFE_INTEGER: i64 = 9_007_199_254_740_991;

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Installed {
        schema: u8,
        config: TrustedGeneration,
        clock: Reference,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Reference {
        boot_id: String,
        observed_epoch_ms: i64,
        observed_boottime_ms: u64,
        valid_for_ms: u64,
    }

    impl Reference {
        fn current(&self, boot_id: &str, boottime_ms: u64) -> Result<(i64, Duration)> {
            ensure!(uuid::Uuid::parse_str(&self.boot_id).is_ok() && self.boot_id == boot_id, "bootstrap boot mismatch");
            ensure!((1..=SAFE_INTEGER).contains(&self.observed_epoch_ms), "invalid qualified epoch");
            ensure!(
                self.observed_boottime_ms <= SAFE_INTEGER as u64 && boottime_ms <= SAFE_INTEGER as u64,
                "invalid boot clock"
            );
            ensure!((1..=300_000).contains(&self.valid_for_ms), "invalid reference lifetime");
            let elapsed = boottime_ms.checked_sub(self.observed_boottime_ms).context("boot clock moved backwards")?;
            ensure!(elapsed < self.valid_for_ms, "qualified reference expired");
            let epoch = self.observed_epoch_ms.checked_add(i64::try_from(elapsed)?).context("clock overflow")?;
            ensure!(epoch <= SAFE_INTEGER, "clock overflow");
            Ok((epoch, Duration::from_millis(self.valid_for_ms - elapsed)))
        }
    }

    fn protected_file(metadata: &Metadata) -> Result<()> {
        ensure!(
            metadata.is_file() && metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
            "bootstrap configuration is not protected by root"
        );
        ensure!(metadata.len() > 0 && metadata.len() <= LIMIT, "invalid bootstrap configuration size");
        Ok(())
    }

    fn boottime_ms() -> Result<u64> {
        let mut value = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: a valid mutable timespec is supplied for this synchronous call.
        ensure!(unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &raw mut value) } == 0, "boot clock unavailable");
        u64::try_from(value.tv_sec)?
            .checked_mul(1000)
            .and_then(|ms| u64::try_from(value.tv_nsec).ok().and_then(|ns| ms.checked_add(ns / 1_000_000)))
            .context("boot clock overflow")
    }

    pub(super) fn load(hub: &Hub) -> Result<()> {
        let metadata = match std::fs::symlink_metadata(FILE) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        protected_file(&metadata)?;
        for path in ["/", "/etc", "/etc/donnabot"] {
            let parent = std::fs::symlink_metadata(path)?;
            ensure!(
                parent.is_dir() && parent.uid() == 0 && parent.mode() & 0o022 == 0,
                "bootstrap configuration parent is not protected"
            );
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(FILE)?;
        protected_file(&file.metadata()?)?;
        let mut bytes = Vec::new();
        file.take(LIMIT + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= usize::try_from(LIMIT)?, "bootstrap configuration too large");
        let installed: Installed =
            serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("invalid protected bootstrap configuration"))?;
        ensure!(installed.schema == 1, "unsupported bootstrap configuration");
        ensure!(
            installed.config.computer_id == hub.identity.computer_id()
                && installed.config.sign_key == hub.identity.sign_pub_b64()
                && installed.config.box_key == hub.identity.box_pub_b64(),
            "bootstrap configuration identity mismatch"
        );
        // Identical installation is idempotent and must never clear everClaimed.
        hub.store.bootstrap_install_generation(&installed.config)?;
        let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
        let (epoch, remaining) = installed.clock.current(boot_id.trim(), boottime_ms()?)?;
        clock::install_qualified(epoch, remaining)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn reference_does_not_renew_across_restart_or_boot() {
            let boot = "c5386ca1-bad0-4cfb-b19b-88badf199264";
            let r = Reference {
                boot_id: boot.into(),
                observed_epoch_ms: 10_000,
                observed_boottime_ms: 100,
                valid_for_ms: 200,
            };
            assert_eq!(r.current(boot, 150).ok(), Some((10_050, Duration::from_millis(150))));
            assert!(r.current(boot, 300).is_err());
            assert!(r.current(boot, 99).is_err());
            assert!(r.current("c5386ca1-bad0-4cfb-b19b-88badf199265", 150).is_err());
        }

        #[test]
        fn user_file_is_not_a_provisioning_root() -> Result<()> {
            let dir = std::env::temp_dir().join(format!("bootstrap-source-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&dir)?;
            let p = dir.join("config.json");
            std::fs::write(&p, b"{}")?;
            let metadata = std::fs::metadata(&p)?;
            // Gates run as the unprivileged donnabot user, never as root.
            ensure!(metadata.uid() != 0, "unprivileged fixture required");
            assert!(protected_file(&metadata).is_err());
            std::fs::remove_file(&p)?;
            std::fs::remove_dir(&dir)?;
            Ok(())
        }
    }
}
