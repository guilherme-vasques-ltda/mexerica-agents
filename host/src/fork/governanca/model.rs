use crate::hub::BotStatus;
use anyhow::{Result, anyhow, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GovernancePolicy {
    pub(crate) title_instruction: String,
    pub(crate) auto_archive_idle_days: u16,
    pub(crate) preserve_pinned_and_open_work: bool,
    pub(crate) max_concurrent_subagents: u16,
    pub(crate) max_subagent_depth: u8,
}

impl Default for GovernancePolicy {
    fn default() -> Self {
        Self {
            title_instruction: String::new(),
            auto_archive_idle_days: 3,
            preserve_pinned_and_open_work: true,
            max_concurrent_subagents: 12,
            max_subagent_depth: 3,
        }
    }
}

impl GovernancePolicy {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            self.title_instruction.chars().count() <= 500,
            "invalid_field: titleInstruction excede 500 caracteres."
        );
        ensure!(self.auto_archive_idle_days <= 365, "invalid_field: autoArchiveIdleDays fora da faixa.");
        ensure!(self.preserve_pinned_and_open_work, "invalid_field: preservePinnedAndOpenWork deve ser true.");
        ensure!(
            (1..=32).contains(&self.max_concurrent_subagents),
            "invalid_field: maxConcurrentSubagents fora da faixa."
        );
        ensure!((1..=5).contains(&self.max_subagent_depth), "invalid_field: maxSubagentDepth fora da faixa.");
        Ok(())
    }

    #[allow(dead_code, reason = "regra pura; não agenda arquivamento nesta etapa")]
    pub(crate) fn can_auto_archive(&self, state: Option<ArchiveState>) -> bool {
        let Some(state) = state else { return false };
        self.validate().is_ok()
            && self.auto_archive_idle_days != 0
            && !state.pinned
            && !matches!(state.status, BotStatus::Working | BotStatus::NeedsInput)
            && !state.has_open_work
            && !state.in_review
            && state.idle_days >= self.auto_archive_idle_days
    }
}

#[allow(dead_code, reason = "insumo da regra pura; sem agendamento nesta etapa")]
pub(crate) struct ArchiveState {
    pub(crate) idle_days: u16,
    pub(crate) status: BotStatus,
    pub(crate) pinned: bool,
    pub(crate) has_open_work: bool,
    pub(crate) in_review: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Snapshot<'a> {
    pub(super) version: u8,
    pub(super) hub_id: &'a str,
    pub(super) space_id: &'a str,
    pub(super) revision: i64,
    pub(super) policy: GovernancePolicy,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Coords {
    hub_id: String,
    space_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Save {
    hub_id: String,
    space_id: String,
    pub(super) expected_revision: i64,
    pub(super) client_request_id: String,
    pub(super) policy: GovernancePolicy,
}

pub(super) enum Request {
    Read(Coords),
    Save(Save),
}

impl Request {
    pub(super) fn parse(body: Value, save: bool) -> Result<Self> {
        let invalid = |_| anyhow!("invalid_field: solicitação de governança inválida ou campo desconhecido.");
        if save {
            Ok(Self::Save(serde_json::from_value(body).map_err(invalid)?))
        } else {
            Ok(Self::Read(serde_json::from_value(body).map_err(invalid)?))
        }
    }

    pub(super) fn coords(&self) -> (&str, &str) {
        match self {
            Self::Read(r) => (&r.hub_id, &r.space_id),
            Self::Save(r) => (&r.hub_id, &r.space_id),
        }
    }
}
