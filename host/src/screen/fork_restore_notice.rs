use serde::Serialize;
use serde_json::Value;

use super::protocol::HelperStatus;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RestoreNotice {
    StateDirectoryNotPrivate,
}

#[derive(Default)]
pub(super) struct LinkNotice {
    observed: Option<RestoreNotice>,
    last_public: Option<HelperStatus>,
    gate: Gate,
}

#[derive(Clone, Copy, Default)]
enum Gate {
    #[default]
    Current,
    Disabled,
    Closing(i64),
}

impl LinkNotice {
    pub(super) fn observe(&mut self, params: &Value) {
        self.observed = match params.get("portalRestoreNotice").and_then(Value::as_str) {
            Some("stateDirectoryNotPrivate") if params["platform"] == "linux" => {
                Some(RestoreNotice::StateDirectoryNotPrivate)
            }
            _ => None,
        };
    }

    pub(super) fn new_public_status(&mut self, status: &HelperStatus) -> bool {
        if self.last_public.as_ref() == Some(status) {
            return false;
        }
        self.last_public = Some(status.clone());
        true
    }

    pub(super) fn current(&self) -> Option<RestoreNotice> {
        match self.gate {
            Gate::Current => self.observed,
            _ => None,
        }
    }

    pub(super) fn suspend(&mut self, id: Option<i64>) {
        self.gate = id.map_or(Gate::Disabled, Gate::Closing);
    }

    pub(super) fn close_reply(&mut self, id: i64, reply: &Value) {
        if !matches!(self.gate, Gate::Closing(expected) if expected == id) {
            return;
        }
        self.observed = None;
        self.gate = if reply.get("error").is_none()
            && reply.get("result").and_then(Value::as_object).is_some_and(|result| result.is_empty())
        {
            Gate::Current
        } else {
            Gate::Disabled
        };
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
