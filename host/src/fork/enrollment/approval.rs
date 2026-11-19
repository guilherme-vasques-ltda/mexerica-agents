//! Local approval binds the human gesture to one in-memory incarnation.
use crate::remote::crypto::{self, b64};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Expected {
    pub device_key: String,
    pub code: String,
    pub transcript: String,
}

impl Expected {
    pub fn valid(&self) -> bool {
        self.code.len() == 6
            && self.code.bytes().all(|b| b.is_ascii_digit())
            && crypto::unb64_n::<32>(&self.device_key).is_ok_and(|v| b64(&v) == self.device_key)
            && crypto::unb64_n::<32>(&self.transcript).is_ok_and(|v| b64(&v) == self.transcript)
    }
}

pub fn transcript(
    cid: &[u8; 16],
    id: &str,
    key: &[u8; 32],
    commit: &[u8; 32],
    host: &[u8; 32],
    device: &[u8; 32],
    incarnation: &[u8; 16],
) -> String {
    b64(&crypto::sha256(&[b"donna/access/v1", cid, id.as_bytes(), key, commit, host, device, incarnation]))
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "result", rename_all = "camelCase")]
pub enum DecideResult {
    #[serde(rename_all = "camelCase")]
    Approved {
        device_key: String,
        grant_id: String,
        /// Shown on this computer only after the device was saved; the phone asks for it (MX-149).
        #[serde(skip_serializing_if = "Option::is_none")]
        confirm_code: Option<String>,
    },
    Denied,
    Refused {
        reason: &'static str,
    },
    Unknown {
        reason: &'static str,
    },
}

struct Tombstone {
    id: String,
    at: i64,
    expected: Option<Expected>,
    result: DecideResult,
}

#[derive(Default)]
pub struct Tombstones(VecDeque<Tombstone>);
impl Tombstones {
    fn prune(&mut self, now: i64) {
        while self.0.front().is_some_and(|entry| now.saturating_sub(entry.at) >= 24 * 60 * 60 * 1000) {
            self.0.pop_front();
        }
    }
    pub fn contains(&mut self, id: &str, now: i64) -> bool {
        self.prune(now);
        self.0.iter().any(|entry| entry.id == id)
    }
    pub fn result(&mut self, id: &str, expected: Option<&Expected>, now: i64) -> Option<DecideResult> {
        self.prune(now);
        self.0.iter().find(|entry| entry.id == id).map(|entry| {
            if entry.expected.as_ref() == expected {
                entry.result.clone()
            } else {
                DecideResult::Refused { reason: "notFound" }
            }
        })
    }
    pub fn insert(&mut self, id: String, expected: Option<Expected>, result: DecideResult, now: i64) {
        self.prune(now);
        if let Some(entry) = self.0.iter_mut().find(|entry| entry.id == id) {
            if expected.is_some() {
                entry.expected = expected;
                entry.result = result;
            }
            return;
        }
        if self.0.len() >= 2000 {
            self.0.pop_front();
        }
        self.0.push_back(Tombstone { id, at: now, expected, result });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incarnation_changes_transcript_and_tombstones_expire_without_replay() {
        let first = transcript(&[1; 16], "req_fixture", &[2; 32], &[3; 32], &[4; 32], &[5; 32], &[6; 16]);
        let second = transcript(&[1; 16], "req_fixture", &[2; 32], &[3; 32], &[4; 32], &[5; 32], &[7; 16]);
        assert_ne!(first, second);
        let expected = Expected { device_key: b64(&[2; 32]), code: "123456".into(), transcript: first };
        let mut tombstones = Tombstones::default();
        tombstones.insert("req_fixture".into(), Some(expected.clone()), DecideResult::Denied, 10);
        assert_eq!(tombstones.result("req_fixture", Some(&expected), 11), Some(DecideResult::Denied));
        let other = Expected { transcript: second, ..expected };
        assert_eq!(
            tombstones.result("req_fixture", Some(&other), 11),
            Some(DecideResult::Refused { reason: "notFound" })
        );
        assert!(!tombstones.contains("req_fixture", 10 + 24 * 60 * 60 * 1000));
    }
}
