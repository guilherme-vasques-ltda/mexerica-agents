use anyhow::{Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

pub const METHODS: &[&str] = &[
    "donnaWorkplaceAccess",
    "donnaWorkplaceGuide",
    "donnaWorkplaceList",
    "donnaWorkplaceCreate",
    "donnaWorkplaceRename",
    "donnaWorkplacePrepareConnection",
    "donnaWorkplacePinRunner",
    "donnaWorkplaceRefix",
    "donnaWorkplaceInspectStart",
    "donnaWorkplaceInspectStatus",
    "donnaWorkplaceInspectCancel",
    "donnaWorkplaceRemove",
];
pub const RUNNER_PORT: u16 = 47631;
pub const PURPOSE: &str = "donna/runner/inspect/v1";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountState {
    AssinaturaOficial,
    ApiNaoAdmitida,
    Desconectado,
    NaoVerificado,
    Indisponivel,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    NaoVerificado,
    Conferido,
    Indisponivel,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Observation {
    pub connection: Axis,
    pub identity: Axis,
    pub preparation: Axis,
    pub account: AccountState,
    pub helpers: Unavailable,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unavailable {
    Indisponivel,
}
impl Default for Observation {
    fn default() -> Self {
        Self {
            connection: Axis::NaoVerificado,
            identity: Axis::NaoVerificado,
            preparation: Axis::NaoVerificado,
            account: AccountState::NaoVerificado,
            helpers: Unavailable::Indisponivel,
        }
    }
}
/// Fixado pela cerimônia Local a partir do artefato privado, nunca aprendido do peer.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Manifest {
    pub schema: u32,
    pub commit: String,
    pub sha256: String,
    pub size: u64,
    pub os: String,
    pub arch: String,
    pub libc: String,
    pub protocol: u32,
    pub journal: u32,
    pub driver: Driver,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
pub enum Driver {
    Indisponivel,
    CodexAccountV1 { version: String, sha256: String },
}
impl Manifest {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema == 1 && self.protocol == 1 && self.journal == 1, "artifact_incompatible");
        ensure!(self.os == "linux" && matches!(self.arch.as_str(), "x86_64" | "aarch64"), "artifact_incompatible");
        ensure!(self.size > 0 && hex(&self.sha256, 64) && hex(&self.commit, 40), "artifact_invalid");
        ensure!(
            !self.libc.is_empty() && self.libc.len() <= 80 && !self.libc.chars().any(char::is_control),
            "artifact_invalid"
        );
        if let Driver::CodexAccountV1 { version, sha256 } = &self.driver {
            ensure!(
                !version.is_empty() && version.len() <= 80 && !version.chars().any(char::is_control) && hex(sha256, 64),
                "driver_invalid"
            );
        }
        Ok(())
    }
}
fn hex(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionPackage {
    pub sandbox_id: String,
    pub host: String,
    pub port: u16,
    pub known_hosts_line: String,
}
impl ConnectionPackage {
    pub fn validate(&self, resource: &str) -> Result<()> {
        ensure!(self.sandbox_id == resource && resource_id(resource), "resource_mismatch");
        ensure!(self.port > 0 && valid_host(&self.host), "endpoint_invalid");
        let parts: Vec<_> = self.known_hosts_line.split(' ').collect();
        ensure!(parts.len() == 3 && parts[0] == resource && parts[1] == "ssh-ed25519", "host_key_invalid");
        let bytes = STANDARD.decode(parts[2]).map_err(|_| anyhow::anyhow!("host_key_invalid"))?;
        ensure!(STANDARD.encode(&bytes) == parts[2], "host_key_invalid");
        let mut expected = Vec::from(&b"\0\0\0\x0bssh-ed25519\0\0\0\x20"[..]);
        ensure!(bytes.len() == expected.len() + 32 && bytes.starts_with(&expected), "host_key_invalid");
        let key: [u8; 32] = bytes[expected.len()..].try_into()?;
        ed25519_dalek::VerifyingKey::from_bytes(&key).map_err(|_| anyhow::anyhow!("host_key_invalid"))?;
        expected.clear();
        Ok(())
    }
}
pub fn resource_id(s: &str) -> bool {
    s.starts_with("bx_") && (4..=100).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
pub fn valid_host(s: &str) -> bool {
    if s.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    !s.is_empty()
        && s.len() <= 253
        && s.split('.').all(|p| {
            !p.is_empty()
                && p.len() <= 63
                && p.as_bytes()[0].is_ascii_alphanumeric()
                && p.as_bytes()[p.len() - 1].is_ascii_alphanumeric()
                && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}
pub fn label(s: &str) -> Result<()> {
    ensure!(!s.trim().is_empty() && s.len() <= 120 && !s.chars().any(char::is_control), "name_invalid");
    Ok(())
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Place {
    pub id: String,
    pub hub_id: String,
    pub resource_id: String,
    pub name: String,
    pub revision: u64,
    pub admission_revision: u64,
    pub connection_revision: u64,
    pub capacity_revision: u64,
    pub removed: bool,
    pub ssh_public: Option<String>,
    pub control_public: Option<String>,
    pub runner_pin: Option<String>,
    pub manifest: Option<Manifest>,
    pub connection: Option<ConnectionPackage>,
}
impl Place {
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"id":self.id,"hubId":self.hub_id,"resourceId":self.resource_id,
            "name":self.name,"kind":"cloud","provider":"boat","revision":self.revision,
            "admissionRevision":self.admission_revision,"connectionRevision":self.connection_revision,
            "capacityRevision":self.capacity_revision,"prepared":self.control_public.is_some(),
            "pinned":self.runner_pin.is_some(),"connected":self.connection.is_some(),"helpers":"indisponivel"})
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Verificando,
    Concluida,
    Falhou,
    CanceladaIncerta,
    InterrompidaIncerta,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Operation {
    pub id: String,
    pub place_id: String,
    pub hub_id: String,
    pub admission_revision: u64,
    pub connection_revision: u64,
    pub authority: String,
    pub intent: String,
    pub phase: Phase,
    pub observed_at: Option<i64>,
    pub deadline: i64,
    pub observation: Observation,
    pub reason: Option<String>,
}
