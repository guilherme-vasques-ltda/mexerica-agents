//! Qualified epoch supplied by trusted provisioning, advanced by a monotonic clock.
use crate::LockExt;
use anyhow::{Result, ensure};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

struct Reference {
    epoch_ms: i64,
    at: Instant,
    valid_for: Duration,
}
static REFERENCE: OnceLock<Mutex<Option<Reference>>> = OnceLock::new();
fn state() -> &'static Mutex<Option<Reference>> {
    REFERENCE.get_or_init(|| Mutex::new(None))
}
fn elapsed_now(r: &Reference) -> Result<i64> {
    r.epoch_ms.checked_add(i64::try_from(r.at.elapsed().as_millis())?).ok_or_else(|| anyhow::anyhow!("clock overflow"))
}
/// Internal seam only for a qualified provisioning source. Never accept a request/body/health clock.
pub(crate) fn install_qualified(epoch_ms: i64, valid_for: Duration) -> Result<()> {
    ensure!(epoch_ms > 0 && !valid_for.is_zero() && valid_for <= Duration::from_secs(300), "invalid qualified time");
    let mut current = state().locked();
    if let Some(previous) = current.as_ref() {
        ensure!(epoch_ms >= elapsed_now(previous)?, "clock moved backwards");
    }
    *current = Some(Reference { epoch_ms, at: Instant::now(), valid_for });
    Ok(())
}
pub(crate) fn now() -> Result<i64> {
    let current = state().locked();
    let r = current.as_ref().ok_or_else(|| anyhow::anyhow!("qualified clock unavailable"))?;
    ensure!(r.at.elapsed() < r.valid_for, "qualified clock expired");
    elapsed_now(r)
}
pub(crate) fn remaining_until(deadline_ms: i64) -> Result<Duration> {
    let current = state().locked();
    let r = current.as_ref().ok_or_else(|| anyhow::anyhow!("qualified clock unavailable"))?;
    let elapsed = r.at.elapsed();
    ensure!(elapsed < r.valid_for, "qualified clock expired");
    let now = elapsed_now(r)?;
    ensure!(deadline_ms > now, "deadline expired");
    Ok(Duration::from_millis(u64::try_from(deadline_ms - now)?).min(r.valid_for.saturating_sub(elapsed)))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn qualified_reference_fences_deadline_and_rollback() {
        install_qualified(10_000, Duration::from_secs(30)).unwrap();
        let n = now().unwrap();
        assert!(remaining_until(n).is_err());
        assert!(remaining_until(n + 1_000).unwrap() <= Duration::from_secs(1));
        assert!(install_qualified(1, Duration::from_secs(1)).is_err());
    }
}
