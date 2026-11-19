use crate::fork::escopo::{Audience, ContextRef, Id, Revision, Version};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreError {
    Storage,
    StoreBusy,
    StorePoisoned,
    NestedTransaction,
    RollbackOnly,
    StatementForbidden,
    TransactionLimit,
    ScopeUnavailable,
}
impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Sem SQL, corpo ou detalhe privado no erro.
        write!(f, "core_{self:?}")
    }
}
impl std::error::Error for CoreError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityState {
    Queued,
    Starting,
    Running,
    WaitingInput,
    WaitingApproval,
    Cancelling,
    Recovering,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExecutionWire {
    pub version: Version,
    pub execution_id: Id,
    pub activity_id: Option<Id>,
    pub attempt_id: Id,
    pub turn_id: Id,
    pub generation: Revision,
    pub hub_boot_id: Id,
    pub fence: Revision,
    pub context_ref: ContextRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ExecutionWire", into = "ExecutionWire")]
pub struct ExecutionRef(ExecutionWire);
impl TryFrom<ExecutionWire> for ExecutionRef {
    type Error = &'static str;
    fn try_from(value: ExecutionWire) -> Result<Self, Self::Error> {
        if matches!(value.context_ref.audience(), Audience::Activity { activity_id } if value.activity_id.as_ref() != Some(activity_id))
        {
            return Err("execution_activity_mismatch");
        }
        Ok(Self(value))
    }
}
impl From<ExecutionRef> for ExecutionWire {
    fn from(value: ExecutionRef) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginKind {
    TypedOwner,
    Delegated,
    Routine,
    Webhook,
    Group,
    OtherBot,
    Voice,
    Unknown,
}

/// Relato do app é dado; nunca prova autoria humana, grant ou cobertura do Hub.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum AppReportedOrigin {
    Unknown {},
    Segmented { has_pasted: bool, segments_revision: Revision },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OriginRef {
    pub version: Version,
    pub kind: OriginKind,
    pub source_ref: Option<Id>,
    pub authenticated_carrier_ref: Option<Id>,
    pub app_reported: AppReportedOrigin,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum RouteDestination {
    Retained {},
    Conversation { context_ref: ContextRef },
    Group { context_ref: ContextRef },
    OwnerInbox { owner_id: Id },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RouteWire {
    pub hub_id: Id,
    pub version: Version,
    pub origin_context: Option<ContextRef>,
    pub origin_entry_id: Option<Id>,
    pub requester_execution_id: Option<Id>,
    pub destination: RouteDestination,
    pub route_revision: Revision,
    pub policy_revision: Revision,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RouteWire", into = "RouteWire")]
pub struct RouteRef(RouteWire);
impl TryFrom<RouteWire> for RouteRef {
    type Error = &'static str;
    fn try_from(value: RouteWire) -> Result<Self, Self::Error> {
        let destination = match &value.destination {
            RouteDestination::Conversation { context_ref } | RouteDestination::Group { context_ref } => {
                Some(context_ref)
            }
            _ => None,
        };
        if value.origin_context.as_ref().is_some_and(|c| c.hub_id() != &value.hub_id)
            || destination.is_some_and(|c| c.hub_id() != &value.hub_id)
            || (value.origin_entry_id.is_some() && value.origin_context.is_none())
            || matches!(&value.destination, RouteDestination::Group { context_ref } if !matches!(context_ref.audience(), Audience::Group { .. }))
        {
            return Err("route_context_mismatch");
        }
        Ok(Self(value))
    }
}
impl From<RouteRef> for RouteWire {
    fn from(value: RouteRef) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PlanDigest(String);
impl TryFrom<String> for PlanDigest {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err("invalid_plan_digest");
        }
        Ok(Self(value))
    }
}
impl From<PlanDigest> for String {
    fn from(value: PlanDigest) -> Self {
        value.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct StepKey(String);
impl TryFrom<String> for StepKey {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 128
            || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err("invalid_step_key");
        }
        Ok(Self(value))
    }
}
impl From<StepKey> for String {
    fn from(value: StepKey) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanDomain {
    Workflow,
    Group,
    Routine,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanVersionRef {
    pub domain: PlanDomain,
    pub definition_id: Id,
    pub version_id: Id,
    pub digest: PlanDigest,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepRef {
    pub version: Version,
    pub root_activity_id: Id,
    pub step_execution_id: Id,
    pub logical_step_key: StepKey,
    pub plan_version_ref: PlanVersionRef,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
enum RequestOwnerWire {
    AgentActivity { activity_id: Id, step_ref: Option<StepRef> },
    DeterministicStep { root_activity_id: Id, step_ref: StepRef },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RequestOwnerWire", into = "RequestOwnerWire")]
pub struct RequestOwnerRef(RequestOwnerWire);
impl TryFrom<RequestOwnerWire> for RequestOwnerRef {
    type Error = &'static str;
    fn try_from(value: RequestOwnerWire) -> Result<Self, Self::Error> {
        if matches!(&value, RequestOwnerWire::DeterministicStep { root_activity_id, step_ref } if *root_activity_id != step_ref.root_activity_id)
        {
            return Err("request_step_root_mismatch");
        }
        Ok(Self(value))
    }
}
impl From<RequestOwnerRef> for RequestOwnerWire {
    fn from(value: RequestOwnerRef) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum SummaryItemRef {
    Activity { activity_id: Id },
    HumanRequest { request_id: Id },
}
