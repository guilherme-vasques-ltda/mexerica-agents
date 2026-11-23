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

pub(crate) fn validate_report(raw: &serde_json::Value, space: &str) -> Option<serde_json::Value> {
    let report: HubDoctorReport = serde_json::from_value(raw.clone()).ok()?;
    if report.version != 1
        || report.hub_id.trim().is_empty()
        || report.space_id.trim().is_empty()
        || report.space_id != space
        || !(0..=8_640_000_000_000_000).contains(&report.collected_at)
        || report.checks.is_empty()
    {
        return None;
    }
    let mut ids = std::collections::BTreeSet::new();
    for (check, wire) in report.checks.iter().zip(raw.get("checks")?.as_array()?) {
        if check.id.trim().is_empty()
            || check.code.trim().is_empty()
            || !ids.insert(&check.id)
            || wire.get("count").is_none()
            || check.count.is_some_and(|n| n > 9_007_199_254_740_991)
        {
            return None;
        }
    }
    serde_json::to_value(report).ok()
}
