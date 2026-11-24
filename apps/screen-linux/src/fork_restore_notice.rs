use serde_json::{Value, json};

use crate::Outbox;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RestoreNotice {
    StateDirectoryNotPrivate,
}

impl RestoreNotice {
    pub(super) fn from_error(error: &anyhow::Error) -> Option<Self> {
        error.downcast_ref::<Self>().copied()
    }
}

impl std::fmt::Display for RestoreNotice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StateDirectoryNotPrivate => {
                f.write_str("token directory is not privately controlled")
            }
        }
    }
}

impl std::error::Error for RestoreNotice {}

#[derive(Default)]
pub(super) struct Status {
    pub(super) epoch: u64,
    pub(super) notice: Option<RestoreNotice>,
    last: Option<Value>,
}

impl Status {
    pub(super) fn publish(&mut self, mut status: Value, out: &Outbox) {
        if let Some(RestoreNotice::StateDirectoryNotPrivate) = self.notice {
            status["portalRestoreNotice"] = json!("stateDirectoryNotPrivate");
        }
        if self.last.as_ref() != Some(&status) {
            self.last = Some(status.clone());
            let _ = out.send(json!({"jsonrpc": "2.0", "method": "status", "params": status}));
        }
    }

    pub(super) fn clear(&mut self, out: &Outbox) {
        if self.notice.take().is_none() {
            return;
        }
        if let Some(Value::Object(mut status)) = self.last.clone()
            && status.remove("portalRestoreNotice").is_some()
        {
            self.publish(Value::Object(status), out);
        }
    }
}
