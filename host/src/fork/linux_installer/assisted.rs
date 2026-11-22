use anyhow::{Result, ensure};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const SIGNING_CONTEXT: &[u8] = b"Mexerica Linux assisted-install test manifest v1";
const TARGET_PLATFORM: &str = "linux";
const TARGET_ARCH: &str = "x86_64";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InstallPhase {
    Checking,
    Downloading,
    Verifying,
    AwaitingPrivilegedSetup,
    Configuring,
    Starting,
    Ready,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstallError {
    pub code: InstallErrorCode,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InstallErrorCode {
    UnsupportedPlatform,
    MissingPrivilegedHelper,
    InvalidManifest,
    InvalidSignature,
    InvalidArtifact,
    Downgrade,
    MissingIdentity,
    InvalidOAuthState,
    OAuthCancelled,
    Cancelled,
    SetupFailed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstallEvent {
    pub job_id: String,
    pub phase: InstallPhase,
    pub progress_percent: Option<u8>,
    pub requires_privileged_setup: bool,
    pub result: Option<InstallResult>,
    pub error: Option<InstallError>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstallResult {
    pub hub_version: String,
    pub motor_version: String,
    pub node_version: String,
    pub identity_separated: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReleaseManifest {
    pub version: u64,
    pub artifacts: BTreeMap<ArtifactKind, Artifact>,
    pub signature: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ArtifactKind {
    Hub,
    Helper,
    Node,
    Motor,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Artifact {
    pub version: String,
    pub sha256: String,
}

impl InstallEvent {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(!self.job_id.is_empty() && self.job_id.len() <= 64, "install_job_invalid");
        ensure!(self.progress_percent.is_none_or(|value| value <= 100), "install_progress_invalid");
        ensure!(
            self.phase != InstallPhase::Ready
                || (self.result.as_ref().is_some_and(|result| result.identity_separated)
                    && self.error.is_none()
                    && !self.requires_privileged_setup),
            "install_result_missing"
        );
        ensure!(
            matches!(self.phase, InstallPhase::Failed | InstallPhase::Cancelled) || self.error.is_none(),
            "install_error_unexpected"
        );
        ensure!(
            !matches!(self.phase, InstallPhase::Failed | InstallPhase::Cancelled)
                || (self.error.is_some() && self.result.is_none()),
            "install_error_missing"
        );
        ensure!(self.phase == InstallPhase::Ready || self.result.is_none(), "install_result_unexpected");
        ensure!(
            !matches!(self.phase, InstallPhase::AwaitingPrivilegedSetup)
                || (self.requires_privileged_setup && self.result.is_none()),
            "install_privilege_state_invalid"
        );
        Ok(())
    }
}

impl ReleaseManifest {
    fn verify(&self, installed_version: u64, trusted_key: &[u8; 32]) -> Result<()> {
        ensure!(self.version > 0, "manifest_invalid");
        ensure!(self.version >= installed_version, "release_downgrade");
        ensure!(
            [ArtifactKind::Hub, ArtifactKind::Helper, ArtifactKind::Node, ArtifactKind::Motor]
                .iter()
                .all(|kind| self.artifacts.contains_key(kind)),
            "manifest_artifacts_missing"
        );
        ensure!(
            self.artifacts.values().all(|artifact| {
                artifact.version.len() <= 80
                    && !artifact.version.is_empty()
                    && artifact.sha256.len() == 64
                    && artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            }),
            "manifest_artifact_invalid"
        );
        let signed =
            serde_json::to_vec(&(SIGNING_CONTEXT, TARGET_PLATFORM, TARGET_ARCH, self.version, &self.artifacts))?;
        let signature = hex_decode(&self.signature)?;
        let key = VerifyingKey::from_bytes(trusted_key)?;
        key.verify_strict(&signed, &Signature::from_slice(&signature)?)
            .map_err(|_| anyhow::anyhow!("manifest_signature_invalid"))?;
        Ok(())
    }

    fn verify_artifact(&self, kind: ArtifactKind, bytes: &[u8]) -> Result<()> {
        let artifact = self.artifacts.get(&kind).ok_or_else(|| anyhow::anyhow!("manifest_artifact_missing"))?;
        let digest = Sha256::digest(bytes);
        ensure!(hex_encode(&digest) == artifact.sha256.to_ascii_lowercase(), "artifact_hash_invalid");
        Ok(())
    }
}

fn hex_decode(input: &str) -> Result<Vec<u8>> {
    ensure!(input.len() == 128 && input.bytes().all(|byte| byte.is_ascii_hexdigit()), "manifest_signature_invalid");
    (0..input.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&input[index..index + 2], 16).map_err(Into::into))
        .collect()
}

fn hex_encode(input: &[u8]) -> String {
    use std::fmt::Write;
    let mut output = String::with_capacity(input.len() * 2);
    for byte in input {
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signed_for(version: u64, platform: &str, arch: &str) -> ReleaseManifest {
        let artifacts = [ArtifactKind::Hub, ArtifactKind::Helper, ArtifactKind::Node, ArtifactKind::Motor]
            .into_iter()
            .map(|kind| (kind, Artifact { version: "24.22.3".into(), sha256: hex_encode(&Sha256::digest(b"fixture")) }))
            .collect::<BTreeMap<_, _>>();
        let signed = serde_json::to_vec(&(SIGNING_CONTEXT, platform, arch, version, &artifacts)).unwrap();
        let signing = SigningKey::from_bytes(&[17; 32]);
        let signature = signing.sign(&signed);
        ReleaseManifest { version, artifacts, signature: hex_encode(&signature.to_bytes()) }
    }

    fn signed(version: u64) -> ReleaseManifest {
        signed_for(version, TARGET_PLATFORM, TARGET_ARCH)
    }

    #[test]
    fn manifest_signature_hash_and_anti_rollback_are_verified_from_fixtures() {
        let manifest = signed(4);
        let trusted = SigningKey::from_bytes(&[17; 32]).verifying_key().to_bytes();
        assert!(manifest.verify(4, &trusted).is_ok());
        assert!(manifest.verify_artifact(ArtifactKind::Node, b"fixture").is_ok());
        assert!(manifest.verify_artifact(ArtifactKind::Motor, b"changed").is_err());
        assert!(manifest.verify(5, &trusted).is_err());
        assert!(manifest.verify(1, &[18; 32]).is_err());
        let other_target = signed_for(4, TARGET_PLATFORM, "aarch64");
        assert!(other_target.verify(4, &trusted).is_err());
        let old_format_signature =
            SigningKey::from_bytes(&[17; 32]).sign(&serde_json::to_vec(&(4_u64, &manifest.artifacts)).unwrap());
        let mut old_format = manifest.clone();
        old_format.signature = hex_encode(&old_format_signature.to_bytes());
        assert!(old_format.verify(4, &trusted).is_err());

        let mut tampered = manifest;
        tampered.artifacts.get_mut(&ArtifactKind::Hub).unwrap().version = "other".into();
        assert!(tampered.verify(1, &trusted).is_err());
    }

    #[test]
    fn install_contract_rejects_false_success_and_privilege_without_wait() {
        let invalid = InstallEvent {
            job_id: "fixture-job".into(),
            phase: InstallPhase::Ready,
            progress_percent: Some(100),
            requires_privileged_setup: false,
            result: None,
            error: None,
        };
        assert!(invalid.validate().is_err());

        let not_isolated = InstallEvent {
            job_id: "fixture-job".into(),
            phase: InstallPhase::Ready,
            progress_percent: Some(100),
            requires_privileged_setup: false,
            result: Some(InstallResult {
                hub_version: "1".into(),
                motor_version: "1".into(),
                node_version: "24.22.3".into(),
                identity_separated: false,
            }),
            error: None,
        };
        assert!(not_isolated.validate().is_err());

        let waiting = InstallEvent {
            job_id: "fixture-job".into(),
            phase: InstallPhase::AwaitingPrivilegedSetup,
            progress_percent: Some(80),
            requires_privileged_setup: true,
            result: None,
            error: None,
        };
        assert!(waiting.validate().is_ok());
        assert!(serde_json::to_string(&waiting).unwrap().contains("awaiting_privileged_setup"));

        let ready = InstallEvent {
            job_id: "fixture-job".into(),
            phase: InstallPhase::Ready,
            progress_percent: Some(100),
            requires_privileged_setup: false,
            result: Some(InstallResult {
                hub_version: "1".into(),
                motor_version: "1".into(),
                node_version: "24.22.3".into(),
                identity_separated: true,
            }),
            error: None,
        };
        assert!(ready.validate().is_ok());

        for (phase, result, error) in [
            (InstallPhase::Checking, None, Some(InstallError { code: InstallErrorCode::SetupFailed })),
            (
                InstallPhase::Failed,
                Some(ready.result.clone().unwrap()),
                Some(InstallError { code: InstallErrorCode::SetupFailed }),
            ),
            (InstallPhase::Cancelled, None, None),
            (
                InstallPhase::Ready,
                Some(ready.result.clone().unwrap()),
                Some(InstallError { code: InstallErrorCode::SetupFailed }),
            ),
        ] {
            let invalid = InstallEvent {
                job_id: "fixture-job".into(),
                phase,
                progress_percent: None,
                requires_privileged_setup: false,
                result,
                error,
            };
            assert!(invalid.validate().is_err());
        }
    }
}
