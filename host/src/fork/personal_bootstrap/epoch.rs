//! Intervalo de época autenticada: inferior emite, superior somente vence.
use super::clock::Reference;
use anyhow::{Context, Result, ensure};
const SAFE: u64 = 9_007_199_254_740_991;
#[derive(Clone)]
pub(super) struct Origin {
    pub(super) boot: String,
    epoch: u64,
    sent: u64,
    received: u64,
}
#[derive(Clone, Copy)]
pub(super) struct Window {
    pub(super) lower: u64,
    pub(super) upper: u64,
}
impl Origin {
    pub(super) fn authenticated(boot: String, epoch: u64, sent: u64, received: u64) -> Result<Self> {
        ensure!(
            epoch > 0 && epoch <= SAFE && received <= SAFE && received.checked_sub(sent).is_some_and(|n| n <= 5000),
            "invalid authenticated epoch flight"
        );
        Ok(Self { boot, epoch, sent, received })
    }
    pub(super) fn bounds(&self, boot: &str, at: u64) -> Result<Window> {
        ensure!(boot == self.boot && at >= self.received, "epoch flight boot/regression");
        let lower = self
            .epoch
            .checked_add(at.saturating_sub(self.received.checked_add(1).context("epoch overflow")?))
            .context("epoch overflow")?;
        let upper = self
            .epoch
            .checked_add(1)
            .and_then(|n| n.checked_add(at.checked_add(1)?.checked_sub(self.sent)?))
            .filter(|n| *n <= SAFE)
            .context("epoch overflow")?;
        Ok(Window { lower, upper })
    }
}
/// Só `RootJournal` verificado constrói o seed de produção; não é JSON/config de cliente.
pub(super) struct VerifiedSeed {
    pub(super) reference: Reference,
    pub(super) origin: Origin,
}
impl VerifiedSeed {
    pub(super) fn checked(
        reference: Reference,
        signed_time: &super::wire::Signed,
        boot: &str,
        sent: u64,
        received: u64,
    ) -> Result<Self> {
        let payload = signed_time.payload();
        let epoch = payload[4].as_u64().context("root time epoch")?;
        let rtt = received.checked_sub(sent).context("root time regression")?;
        ensure!(
            boot == reference.boot_id
                && reference.observed_boottime_ms == received
                && epoch.checked_add(rtt) == Some(reference.observed_epoch_ms)
                && payload[5].as_u64().and_then(|n| n.checked_sub(rtt)) == Some(reference.valid_for_ms),
            "root epoch evidence mismatch"
        );
        Ok(Self { origin: Origin::authenticated(boot.into(), epoch, sent, received)?, reference })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn personal_host_b2_epoch_independent_rtt_uses_lower_for_emission_upper_for_expiry() -> Result<()> {
        let origin = Origin::authenticated("boot".into(), 1_800_000_000_000, 100, 180)?;
        let received = origin.bounds("boot", 180)?;
        assert_eq!(received.lower, 1_800_000_000_000);
        assert_eq!(received.upper, 1_800_000_000_082); // RTT uma vez, dois extremos de ms.
        let worker_predecessor = 1_800_000_000_050;
        assert!(received.lower < worker_predecessor && received.upper > worker_predecessor);
        let later = origin.bounds("boot", 231)?;
        assert_eq!(later.lower, worker_predecessor);
        assert_eq!(later.upper, 1_800_000_000_133);
        assert!(origin.bounds("other-boot", 231).is_err());
        assert!(origin.bounds("boot", 179).is_err());
        Ok(())
    }
    #[test]
    fn personal_host_b2_epoch_rejects_rtt_overflow_and_never_adds_second_rtt() -> Result<()> {
        assert!(Origin::authenticated("boot".into(), 100, 100, 5101).is_err());
        assert!(Origin::authenticated("boot".into(), 100, 100, 99).is_err());
        let origin = Origin::authenticated("boot".into(), SAFE - 2, 100, 101)?;
        assert!(origin.bounds("boot", 102).is_err());
        let origin = Origin::authenticated("boot".into(), 1000, 100, 110)?;
        assert_eq!(origin.bounds("boot", 120)?.upper, 1022);
        Ok(())
    }
}
