//! Clock pessoal: `CLOCK_BOOTTIME` inclui suspensão. Parede não participa da validade.
use super::{
    epoch::{Origin, VerifiedSeed, Window},
    provisioning::AdmittedConfig,
    wire,
};
use crate::remote::{crypto, identity::Identity};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::json;
static BOOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
const SAFE: u64 = 9_007_199_254_740_991;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Reference {
    pub boot_id: String,
    pub observed_epoch_ms: u64,
    pub observed_boottime_ms: u64,
    pub valid_for_ms: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sample {
    boot: String,
    ms: u64,
}
fn sample_os() -> Result<Sample> {
    #[cfg(target_os = "linux")]
    {
        let boot = BOOT.get().context("personal BOOTTIME sampler not prepared")?.clone();
        let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: t é timespec inicializado/alinhado, gravável e vivo na chamada; libc não retém o ponteiro.
        let status = unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &raw mut t) };
        ensure!(status == 0, "personal BOOTTIME unavailable");
        ensure!((0..1_000_000_000).contains(&t.tv_nsec), "invalid BOOTTIME nanos");
        let nanos = u64::try_from(t.tv_nsec)?;
        let ms = u64::try_from(t.tv_sec)?
            .checked_mul(1000)
            .and_then(|v| v.checked_add(nanos / 1_000_000))
            .context("BOOTTIME overflow")?;
        let out = Sample { boot, ms };
        out.valid()?;
        Ok(out)
    }
    #[cfg(not(target_os = "linux"))]
    {
        anyhow::bail!("personal BOOTTIME requires Linux")
    }
}
impl Sample {
    fn valid(&self) -> Result<()> {
        ensure!(
            self.ms <= SAFE && uuid::Uuid::parse_str(&self.boot).is_ok_and(|u| u.to_string() == self.boot),
            "invalid boot sample"
        );
        Ok(())
    }
    fn elapsed(&self, before: &Self) -> Result<u64> {
        self.valid()?;
        before.valid()?;
        ensure!(self.boot == before.boot, "personal boot changed");
        self.ms.checked_sub(before.ms).context("BOOTTIME regressed")
    }
}
#[derive(Clone, Copy, Default)]
enum Qualification {
    #[default]
    Unavailable,
    Qualified,
}
#[derive(Default)]
pub(crate) struct Clock {
    reference: Option<Reference>,
    qualification: Qualification,
    last: Option<Sample>,
    time_flight: Option<(Sample, String)>,
    origin: Option<Origin>,
    validity_floor: u64,
    issued_floor: u64,
    #[cfg(test)]
    fixture_sample: Option<Sample>,
}
impl Clock {
    #[cfg(test)]
    pub(super) fn freeze_fixture_time(&mut self) -> Result<()> {
        let sample = sample_os()?;
        self.now_at(&sample)?;
        self.fixture_sample = Some(sample);
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn advance_fixture_time(&mut self, elapsed: u64) -> Result<()> {
        let sample = self.fixture_sample.as_mut().context("fixture time not frozen")?;
        sample.ms = sample.ms.checked_add(elapsed).filter(|at| *at <= SAFE).context("fixture time overflow")?;
        Ok(())
    }
    /// Carga fora das guardas Cloud/runtime/DB; amostras seguintes não leem FS.
    pub(super) fn prepare_sampler() -> Result<()> {
        #[cfg(target_os = "linux")]
        if BOOT.get().is_none() {
            let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?.trim().to_owned();
            Sample { boot: boot.clone(), ms: 0 }.valid()?;
            let _ = BOOT.set(boot);
        }
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn fixture(epoch: u64) -> Result<Self> {
        Self::prepare_sampler()?;
        let sample = sample_os()?;
        Self::seeded_at(
            Reference {
                boot_id: sample.boot.clone(),
                observed_epoch_ms: epoch,
                observed_boottime_ms: sample.ms,
                valid_for_ms: 300_000,
            },
            &sample,
        )
    }
    pub(super) fn seeded(seed: VerifiedSeed) -> Self {
        let mut clock = Self {
            reference: Some(seed.reference),
            origin: Some(seed.origin),
            qualification: Qualification::Qualified,
            ..Self::default()
        };
        let _ = clock.window();
        clock
    }
    #[cfg(test)]
    fn seeded_at(reference: Reference, now: &Sample) -> Result<Self> {
        ensure!(
            (1..=SAFE).contains(&reference.observed_epoch_ms)
                && reference.observed_boottime_ms <= SAFE
                && (1..=300_000).contains(&reference.valid_for_ms),
            "invalid personal seed"
        );
        Sample { boot: reference.boot_id.clone(), ms: reference.observed_boottime_ms }.valid()?;
        let mut c = Self {
            origin: Some(Origin::authenticated(
                reference.boot_id.clone(),
                reference.observed_epoch_ms,
                reference.observed_boottime_ms,
                reference.observed_boottime_ms,
            )?),
            reference: Some(reference),
            qualification: Qualification::Qualified,
            ..Self::default()
        };
        // Expiry/reboot não ganha prazo: conserva a referência histórica, agora indisponível.
        let _ = c.now_at(now);
        Ok(c)
    }
    fn now_at(&mut self, sample: &Sample) -> Result<u64> {
        if let Some(last) = &self.last
            && let Err(error) = sample.elapsed(last)
        {
            self.qualification = Qualification::Unavailable;
            return Err(error);
        }
        self.last = Some(sample.clone());
        let result = (|| {
            ensure!(matches!(self.qualification, Qualification::Qualified), "personal time unavailable");
            let r = self.reference.as_ref().context("personal time unavailable")?;
            let elapsed = sample.elapsed(&Sample { boot: r.boot_id.clone(), ms: r.observed_boottime_ms })?;
            ensure!(elapsed < r.valid_for_ms, "personal time expired");
            r.observed_epoch_ms.checked_add(elapsed).filter(|e| *e <= SAFE).context("personal epoch overflow")
        })();
        if result.is_err() {
            self.qualification = Qualification::Unavailable;
        }
        result
    }
    pub(super) fn monotonic() -> Result<(String, u64)> {
        let s = sample_os()?;
        Ok((s.boot, s.ms))
    }
    pub(super) fn poll_marker(previous: &serde_json::Value) -> Result<Option<serde_json::Value>> {
        let sample = sample_os()?;
        if let Some(boot) = previous[0].as_str()
            && boot == sample.boot
        {
            let before = previous[1].as_u64().context("invalid personal poll marker")?;
            let elapsed = sample.ms.checked_sub(before).context("personal poll clock regressed")?;
            if elapsed < 5000 {
                return Ok(None);
            }
        }
        Ok(Some(json!([sample.boot, sample.ms])))
    }
    #[cfg(test)]
    pub(crate) fn now(&mut self) -> Result<u64> {
        self.issued()
    }
    pub(super) fn window(&mut self) -> Result<Window> {
        #[cfg(test)]
        let sample = self.fixture_sample.clone().map_or_else(sample_os, Ok)?;
        #[cfg(not(test))]
        let sample = sample_os()?;
        self.now_at(&sample)?;
        let mut window =
            self.origin.as_ref().context("authenticated epoch origin unavailable")?.bounds(&sample.boot, sample.ms)?;
        let reference = self.reference.as_ref().context("personal time unavailable")?;
        let until = reference.observed_epoch_ms.checked_add(reference.valid_for_ms).context("time expiry overflow")?;
        ensure!(window.upper < until, "personal time expired at upper bound");
        self.validity_floor = self.validity_floor.max(window.upper);
        window.upper = self.validity_floor;
        ensure!(window.upper < until, "personal epoch floor expired");
        Ok(window)
    }
    pub(super) fn validity(&mut self) -> Result<u64> {
        Ok(self.window()?.upper)
    }
    pub(super) fn issued_after(&mut self, predecessor: u64, until: u64) -> Result<u64> {
        let window = self.window()?;
        ensure!(
            window.upper < until && window.lower >= predecessor && window.lower >= self.issued_floor,
            "personal causal time unavailable"
        );
        self.issued_floor = window.lower;
        Ok(window.lower)
    }
    pub(super) fn issued(&mut self) -> Result<u64> {
        self.issued_after(0, SAFE)
    }
    pub(super) fn time_request(&mut self, cfg: &AdmittedConfig, identity: &Identity) -> Result<Vec<u8>> {
        self.time_request_at(cfg, identity, sample_os()?, crypto::b64(&crypto::random::<32>()))
    }
    fn time_request_at(
        &mut self,
        cfg: &AdmittedConfig,
        identity: &Identity,
        sample: Sample,
        nonce: String,
    ) -> Result<Vec<u8>> {
        cfg.matches(identity)?;
        sample.valid()?;
        if let Some(last) = &self.last {
            if last.boot != sample.boot {
                self.qualification = Qualification::Unavailable;
            } else if let Err(error) = sample.elapsed(last) {
                self.qualification = Qualification::Unavailable;
                return Err(error);
            }
        }
        ensure!(self.time_flight.is_none(), "personal time already in flight");
        let payload = json!([wire::domain("time-request"), cfg.context(), nonce, cfg.identity().computer_id]);
        let signature = crypto::b64(&identity.sign(&serde_json::to_vec(&payload)?));
        self.time_flight = Some((sample, nonce));
        Ok(serde_json::to_vec(&json!([payload, signature]))?)
    }
    pub(super) fn abandon_time(&mut self) {
        self.time_flight = None;
    }
    pub(super) fn accept_time(&mut self, cfg: &AdmittedConfig, raw: &[u8]) -> Result<()> {
        self.accept_time_at(cfg, raw, sample_os()?)
    }
    fn accept_time_at(&mut self, cfg: &AdmittedConfig, raw: &[u8], now: Sample) -> Result<()> {
        let (sent, nonce) = self.time_flight.take().context("no personal time request")?;
        if let Some(last) = &self.last
            && last.boot == now.boot
            && let Err(error) = now.elapsed(last)
        {
            self.qualification = Qualification::Unavailable;
            return Err(error);
        }
        let rtt = now.elapsed(&sent)?;
        ensure!(rtt.checked_add(1).is_some_and(|rtt| rtt <= 5000), "personal time RTT exceeded");
        let e = cfg.verify_time(raw, &nonce)?;
        let p = e.payload();
        ensure!(p[2] == nonce && p[3] == cfg.identity().computer_id, "personal time nonce/Hub mismatch");
        let epoch = p[4]
            .as_u64()
            .context("invalid epoch")?
            .checked_add(rtt)
            .filter(|n| *n <= SAFE)
            .context("epoch overflow")?;
        let validity = p[5]
            .as_u64()
            .context("invalid validity")?
            .checked_sub(rtt)
            .filter(|n| *n > 0)
            .context("time expired in transit")?;
        if let Some(old) = &self.origin {
            let floor = if old.boot == now.boot { old.bounds(&now.boot, now.ms)?.lower } else { self.issued_floor };
            ensure!(epoch >= floor && epoch >= self.issued_floor, "personal epoch regressed");
        }
        self.origin =
            Some(Origin::authenticated(now.boot.clone(), p[4].as_u64().context("invalid epoch")?, sent.ms, now.ms)?);
        self.reference = Some(Reference {
            boot_id: now.boot.clone(),
            observed_epoch_ms: epoch,
            observed_boottime_ms: now.ms,
            valid_for_ms: validity,
        });
        self.qualification = Qualification::Qualified;
        self.last = Some(now);
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SnapshotBinding {
    pub h_t: String,
    pub h_g: String,
    pub grant: String,
    pub h_r: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SnapshotFlight {
    sent: Sample,
    nonce: String,
    binding: SnapshotBinding,
    #[serde(default)]
    request: Vec<u8>,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
enum LeaseState {
    Current,
    Cut,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Lease {
    flight: SnapshotFlight,
    deadline: u64,
    received: Sample,
    last: Sample,
    state: LeaseState,
    original: Vec<u8>,
}
impl SnapshotFlight {
    pub(super) fn request(
        cfg: &AdmittedConfig,
        identity: &Identity,
        clock: &mut Clock,
        binding: SnapshotBinding,
    ) -> Result<(Self, Vec<u8>)> {
        let sent = sample_os()?;
        let epoch = clock.issued_after(0, SAFE - 30_000)?;
        cfg.matches(identity)?;
        let nonce = crypto::b64(&crypto::random::<32>());
        let p = json!([
            wire::domain("host-state-request"),
            cfg.context(),
            binding.h_t,
            binding.h_g,
            binding.grant,
            binding.h_r,
            nonce,
            epoch,
            epoch.checked_add(30_000).context("epoch overflow")?
        ]);
        let out = serde_json::to_vec(&json!([p, crypto::b64(&identity.sign(&serde_json::to_vec(&p)?))]))?;
        Ok((Self { sent, nonce, binding, request: out.clone() }, out))
    }
    pub(super) fn accept(self, cfg: &AdmittedConfig, raw: &[u8]) -> Result<Lease> {
        self.accept_at(cfg, raw, sample_os()?)
    }
    fn accept_at(self, cfg: &AdmittedConfig, raw: &[u8], now: Sample) -> Result<Lease> {
        ensure!(
            now.elapsed(&self.sent)?.checked_add(1).is_some_and(|rtt| rtt <= 5000),
            "personal snapshot RTT exceeded"
        );
        let e = cfg.verify_snapshot(raw, &self.nonce)?;
        let p = e.payload();
        let b = &self.binding;
        let request = super::protocol::canonical(&self.request)?;
        let checked = cfg.verifier(p[9].as_str().context("snapshot A")?)?.verify(
            super::shapes::Kind::HostStateRequest,
            &self.request,
            wire::Time::Historical(p[16].as_u64().context("snapshot observed")?),
            wire::Reader::Hub,
        )?;
        ensure!(
            checked.payload() == &request[0]
                && request[0][2] == b.h_t
                && request[0][3] == b.h_g
                && request[0][4] == b.grant
                && request[0][5] == b.h_r
                && request[0][6] == self.nonce
                && p[16].as_u64().zip(request[0][7].as_u64()).is_some_and(|(e, s)| e >= s)
                && p[16].as_u64().zip(request[0][8].as_u64()).is_some_and(|(e, end)| e < end),
            "snapshot original request/window mismatch"
        );
        ensure!(
            p[2] == b.h_t
                && p[3] == b.h_g
                && p[4] == b.grant
                && p[5] == b.h_r
                && p[15] == self.nonce
                && p[10] == cfg.identity().computer_id
                && p[11] == cfg.identity().generation,
            "personal snapshot binding mismatch"
        );
        let duration = p[17]
            .as_u64()
            .context("snapshot lease invalid")?
            .checked_sub(p[16].as_u64().context("snapshot epoch invalid")?)
            .filter(|n| *n > 0 && *n <= 900_000)
            .context("snapshot duration invalid")?;
        let deadline = self.sent.ms.checked_add(duration).filter(|n| *n <= SAFE).context("lease overflow")?;
        ensure!(deadline > now.ms, "snapshot expired in transit");
        Ok(Lease {
            flight: self,
            deadline,
            received: now.clone(),
            last: now,
            state: LeaseState::Current,
            original: raw.to_vec(),
        })
    }
}
impl Lease {
    pub(super) fn snapshot(&self) -> &[u8] {
        &self.original
    }
    pub(super) fn request_bytes(&self) -> &[u8] {
        &self.flight.request
    }
    pub(super) fn until(&self) -> Result<u64> {
        serde_json::from_slice::<serde_json::Value>(&self.original)?[0][17].as_u64().context("snapshot expiry")
    }
    pub(super) fn observed(&self) -> Result<u64> {
        serde_json::from_slice::<serde_json::Value>(&self.original)?[0][16].as_u64().context("snapshot observed")
    }
    pub(super) fn current(&mut self) -> Result<()> {
        self.current_at(&sample_os()?)
    }
    fn current_at(&mut self, now: &Sample) -> Result<()> {
        let result = (|| {
            ensure!(matches!(self.state, LeaseState::Current), "personal lease cut");
            now.elapsed(&self.last)?;
            now.elapsed(&self.flight.sent)?;
            self.last = now.clone();
            ensure!(now.ms.checked_add(1).is_some_and(|hi| hi < self.deadline), "personal lease expired");
            Ok(())
        })();
        if result.is_err() {
            self.state = LeaseState::Cut;
        }
        result
    }
    pub(super) fn restore(cfg: &AdmittedConfig, bytes: &[u8]) -> Result<Self> {
        Self::restore_at(cfg, bytes, &sample_os()?)
    }
    fn restore_at(cfg: &AdmittedConfig, bytes: &[u8], now: &Sample) -> Result<Self> {
        let lease: Self = serde_json::from_slice(bytes)?;
        ensure!(matches!(lease.state, LeaseState::Current), "personal lease cut");
        // Revalidar os mesmos originais e bSend, nunca criar um novo flight no restore.
        let e = cfg.verify_snapshot(&lease.original, &lease.flight.nonce)?;
        let duration = e.payload()[17]
            .as_u64()
            .context("lease missing")?
            .checked_sub(e.payload()[16].as_u64().context("epoch missing")?)
            .context("lease underflow")?;
        ensure!(
            lease.deadline == lease.flight.sent.ms.checked_add(duration).context("lease overflow")?,
            "lease deadline mismatch"
        );
        let mut original = lease.flight.accept_at(cfg, &lease.original, lease.received.clone())?;
        ensure!(original.deadline == lease.deadline, "lease reanchor refused");
        now.elapsed(&lease.last)?;
        original.current_at(now)?;
        Ok(original)
    }
    pub(super) fn bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    const BOOT: &str = "c5386ca1-bad0-4cfb-b19b-88badf199264";
    fn sample(ms: u64) -> Sample {
        Sample { boot: BOOT.into(), ms }
    }
    fn vectors() -> Result<Value> {
        Ok(serde_json::from_str(include_str!("../../../../docs/reference/fixtures/remote-relay-vectors.json"))?)
    }
    fn signed_time(v: &Value, p: Value) -> Result<Vec<u8>> {
        let seed = crypto::unb64_n::<32>(
            v["personalBootstrap"]["wireVectors"]["seeds"]["issuer"].as_str().context("fixture root")?,
        )?;
        let signature =
            crypto::b64(&crypto::sign(&ed25519_dalek::SigningKey::from_bytes(&seed), &serde_json::to_vec(&p)?));
        Ok(serde_json::to_vec(&Value::Array(vec![p, Value::String(signature)]))?)
    }
    #[test]
    fn time_before_any_app_qualifies_only_signed_e_and_fences_nonce_rtt_boot_and_regression() -> Result<()> {
        let (cfg, identity) = super::super::provisioning::fixture()?;
        let vectors = vectors()?;
        let wire_vectors = &vectors["personalBootstrap"]["wireVectors"];
        let nonce = wire_vectors["packet"]["time"][0][2].as_str().context("nonce")?.to_owned();
        let envelope = serde_json::to_vec(&wire_vectors["packet"]["time"])?;
        let mut clock = Clock::default();
        assert!(clock.now_at(&sample(100)).is_err());
        let request = clock.time_request_at(&cfg, &identity, sample(100), nonce.clone())?;
        wire::verify_time_request(&request, cfg.context(), &identity.sign_pub_b64())?;
        clock.accept_time_at(&cfg, &envelope, sample(150))?;
        let epoch = wire_vectors["packet"]["time"][0][4].as_u64().context("epoch")?;
        assert_eq!(clock.now_at(&sample(160))?, epoch + 60);
        assert!(clock.accept_time_at(&cfg, &envelope, sample(160)).is_err()); // nonce consumido, sem replay.
        assert!(clock.time_request_at(&cfg, &identity, sample(159), nonce.clone()).is_err());
        let next = crypto::b64(&[201; 32]);
        clock.time_request_at(&cfg, &identity, sample(170), next.clone())?;
        assert!(clock.accept_time_at(&cfg, &envelope, sample(171)).is_err()); // E válido, nonce antigo.
        clock.time_request_at(&cfg, &identity, sample(180), next.clone())?;
        let mut payload = wire_vectors["packet"]["time"][0].clone();
        payload[2] = json!(next);
        payload[4] = json!(1);
        assert!(clock.accept_time_at(&cfg, &signed_time(&vectors, payload)?, sample(181)).is_err());
        let mut slow = Clock::default();
        slow.time_request_at(&cfg, &identity, sample(100), nonce.clone())?;
        assert!(slow.accept_time_at(&cfg, &envelope, sample(5101)).is_err());
        let mut reboot = Clock::default();
        reboot.time_request_at(&cfg, &identity, sample(100), nonce)?;
        assert!(
            reboot
                .accept_time_at(
                    &cfg,
                    &envelope,
                    Sample { boot: "c5386ca1-bad0-4cfb-b19b-88badf199265".into(), ms: 150 }
                )
                .is_err()
        );
        Ok(())
    }
    #[test]
    fn time_verifier_rejects_other_context_hub_and_role_without_an_app_identity() -> Result<()> {
        let (cfg, h) = super::super::provisioning::fixture()?;
        let v = vectors()?;
        let envelope = &v["personalBootstrap"]["wireVectors"]["packet"]["time"];
        let nonce = envelope[0][2].as_str().context("nonce")?;
        let raw = serde_json::to_vec(envelope)?;
        cfg.verify_time(&raw, nonce)?;
        let mut context = cfg.context().clone();
        context[2] = json!("main");
        let issuer =
            v["personalBootstrap"]["manifest"]["personalBootstrap"]["issuerPublicKey"].as_str().context("issuer")?;
        assert!(wire::verify_time(&raw, &context, issuer, &h.sign_pub_b64(), nonce).is_err());
        let mut p = envelope[0].clone();
        p[3] = json!(crypto::computer_id(&[101; 32]));
        assert!(cfg.verify_time(&signed_time(&v, p)?, nonce).is_err());
        let p = &envelope[0];
        let signed_by_h = serde_json::to_vec(&json!([p, crypto::b64(&h.sign(&serde_json::to_vec(p)?))]))?;
        assert!(cfg.verify_time(&signed_by_h, nonce).is_err());
        Ok(())
    }
    #[test]
    fn seed_restart_and_sleep_advance_boottime_without_wall_clock_extension() -> Result<()> {
        let reference = Reference {
            boot_id: BOOT.into(),
            observed_epoch_ms: 1_800_000_000_000,
            observed_boottime_ms: 100,
            valid_for_ms: 300_000,
        };
        let mut initial = Clock::seeded_at(reference.clone(), &sample(150))?;
        assert_eq!(initial.now_at(&sample(160))?, reference.observed_epoch_ms + 60);
        // Reinício usa o mesmo observedBoottime; não ancora no instante de restart.
        let mut restart = Clock::seeded_at(reference.clone(), &sample(200))?;
        assert_eq!(restart.now_at(&sample(210))?, reference.observed_epoch_ms + 110);
        // Suspensão é avanço de BOOTTIME; retroceder parede não é entrada da API.
        assert!(restart.now_at(&sample(300_100)).is_err());
        assert!(restart.now_at(&sample(300_099)).is_err());
        let mut regressed = Clock::seeded_at(reference.clone(), &sample(99))?;
        assert!(regressed.now_at(&sample(101)).is_err()); // não volta por alcançar seed futura.
        let mut regressed = Clock::seeded_at(reference.clone(), &sample(150))?;
        assert!(regressed.now_at(&sample(149)).is_err());
        assert!(regressed.now_at(&sample(151)).is_err()); // só nova E requalifica.
        let mut expired = Clock::seeded_at(reference.clone(), &sample(300_100))?;
        assert!(expired.now_at(&sample(300_100)).is_err());
        let mut changed_boot =
            Clock::seeded_at(reference, &Sample { boot: "c5386ca1-bad0-4cfb-b19b-88badf199265".into(), ms: 1 })?;
        assert!(changed_boot.now_at(&sample(99)).is_err());
        // Referência expirada só volta com nova E autenticada, sem A/tupla.
        let (cfg, identity) = super::super::provisioning::fixture()?;
        let vectors = vectors()?;
        let wire_vectors = &vectors["personalBootstrap"]["wireVectors"];
        let nonce = crypto::b64(&[202; 32]);
        let mut payload = wire_vectors["packet"]["time"][0].clone();
        payload[2] = json!(nonce);
        payload[4] = json!(1_800_000_400_000u64);
        expired.time_request_at(&cfg, &identity, sample(400_100), nonce)?;
        expired.accept_time_at(&cfg, &signed_time(&vectors, payload)?, sample(400_200))?;
        assert_eq!(expired.now_at(&sample(400_210))?, 1_800_000_400_110u64);
        Ok(())
    }
    #[test]
    fn snapshot_nonce_rtt_boot_and_restart_keep_original_bsend_deadline() -> Result<()> {
        let (cfg, _) = super::super::provisioning::fixture()?;
        let v = vectors()?;
        let p = &v["personalBootstrap"]["wireVectors"]["packet"]["host-state"][0];
        let raw = serde_json::to_vec(&v["personalBootstrap"]["wireVectors"]["packet"]["host-state"])?;
        let flight = || -> Result<SnapshotFlight> {
            Ok(SnapshotFlight {
                sent: sample(100),
                request: serde_json::to_vec(&v["personalBootstrap"]["wireVectors"]["packet"]["host-state-request"])?,
                nonce: p[15].as_str().context("nonce")?.into(),
                binding: SnapshotBinding {
                    h_t: p[2].as_str().context("hT")?.into(),
                    h_g: p[3].as_str().context("hG")?.into(),
                    grant: p[4].as_str().context("grant")?.into(),
                    h_r: p[5].as_str().context("hR")?.into(),
                },
            })
        };
        let mut lease = flight()?.accept_at(&cfg, &raw, sample(140))?;
        let deadline = 100 + p[17].as_u64().context("until")? - p[16].as_u64().context("at")?;
        assert_eq!(lease.deadline, deadline);
        lease.current_at(&sample(deadline - 10))?;
        let saved = lease.bytes()?;
        let mut restored = Lease::restore_at(&cfg, &saved, &sample(deadline - 5))?;
        assert_eq!(restored.deadline, deadline);
        assert!(restored.current_at(&sample(deadline)).is_err());
        assert!(Lease::restore_at(&cfg, &restored.bytes()?, &sample(deadline - 1)).is_err());
        let mut regressed = flight()?.accept_at(&cfg, &raw, sample(140))?;
        assert!(regressed.current_at(&sample(139)).is_err());
        assert!(regressed.current_at(&sample(141)).is_err());
        let mut wrong = flight()?;
        wrong.nonce = crypto::b64(&[1; 32]);
        assert!(wrong.accept_at(&cfg, &raw, sample(140)).is_err());
        assert!(flight()?.accept_at(&cfg, &raw, sample(5101)).is_err());
        assert!(
            Lease::restore_at(&cfg, &saved, &Sample { boot: "c5386ca1-bad0-4cfb-b19b-88badf199265".into(), ms: 150 })
                .is_err()
        );
        assert!(Lease::restore_at(&cfg, &saved, &sample(139)).is_err());
        let mut tampered: Value = serde_json::from_slice(&saved)?;
        tampered["deadline"] = json!(deadline + 1000);
        assert!(Lease::restore_at(&cfg, &serde_json::to_vec(&tampered)?, &sample(150)).is_err());
        Ok(())
    }
}
