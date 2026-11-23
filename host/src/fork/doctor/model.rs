//! DTOs do diagnóstico somente leitura do Hub (`donnaHubDoctor`).
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Read {
    pub space_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DoctorOverallStatus {
    Healthy,
    WaitingUser,
    Degraded,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DoctorCheckStatus {
    Ok,
    Warning,
    Error,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DoctorArea {
    Hub,
    AiEngine,
    Approvals,
    Channels,
    Mcps,
    Routines,
    Delegation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DoctorActionKind {
    None,
    StartHub,
    ConnectAi,
    ReviewWaiting,
    CheckChannels,
    CheckMcps,
    CheckRoutines,
    OpenTorre,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DoctorCheck {
    pub id: String,
    pub area: DoctorArea,
    pub status: DoctorCheckStatus,
    pub code: String,
    pub count: Option<u64>,
    pub action_kind: DoctorActionKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HubDoctorReport {
    pub version: u32,
    pub hub_id: String,
    pub space_id: String,
    pub collected_at: i64,
    pub overall: DoctorOverallStatus,
    pub checks: Vec<DoctorCheck>,
}
