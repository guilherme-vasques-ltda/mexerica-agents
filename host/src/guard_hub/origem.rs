//! Contrato fechado de origem. Offsets são unidades UTF-16, como no textarea.
use crate::api::devices::Caller;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Typed,
    Pasted,
    Quote,
    Attachment,
    Voice,
    Generated,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Report {
    schema: u8,
    draft_id: String,
    revision: u64,
    text_length: usize,
    channel: String,
    device_id: String,
    interaction: String,
    segments: Vec<Segment>,
}

impl Report {
    fn valid(&self, text: &str) -> bool {
        if self.schema != 1
            || self.revision == 0
            || self.revision > 9_007_199_254_740_991
            || uuid::Uuid::parse_str(&self.draft_id).is_err()
            || uuid::Uuid::parse_str(&self.device_id).is_err()
            || !matches!(self.channel.as_str(), "desktopComposer" | "desktopVoice")
            || self.interaction != "send"
            || self.segments.len() > 4096
        {
            return false;
        }
        let mut boundaries = std::collections::HashSet::from([0]);
        let mut length = 0;
        for c in text.chars() {
            length += c.len_utf16();
            boundaries.insert(length);
        }
        let mut cursor = 0;
        for part in &self.segments {
            if part.start != cursor
                || part.end <= cursor
                || !boundaries.contains(&part.end)
                || part.kind == Kind::Attachment
                || (self.channel == "desktopVoice"
                    && !matches!(part.kind, Kind::Voice | Kind::Generated | Kind::Unknown))
            {
                return false;
            }
            cursor = part.end;
        }
        self.text_length == length && cursor == length
    }
}

/// Metadados inválidos/ausentes degradam a desconhecido, sem bloquear o envio.
/// `human`, `approved`, canal/dispositivo e narrativa nunca provam identidade.
pub fn capture(caller: &Caller, body: &Value, data: &Value) -> Value {
    let mut record = record(body, data);
    // O corpo nunca fornece o ingresso; Local também pode ser um processo sob D62.
    record["ingress"] = match caller {
        Caller::Local => json!({"caller":"local"}),
        Caller::Ponte { .. } => json!({"caller":"ponte"}),
        Caller::Device { key, .. } => json!({"caller":"device", "device":key}),
        Caller::Pairing { .. } | Caller::Bootstrap { .. } => Value::Null, // Nenhum autentica um dispositivo.
    };
    record
}

/// Uma revisão interna perde a evidência original, sem inventar chamador local.
pub fn invalidate(data: &Value) -> Value {
    record(&json!({}), data)
}

fn record(body: &Value, data: &Value) -> Value {
    let text = data["text"].as_str().unwrap_or_default();
    let report = serde_json::from_value::<Report>(body["contentOrigin"].clone()).ok().filter(|r| r.valid(text));
    let status = if report.is_some() { "appReported" } else { "unknown" };
    let segments = report.as_ref().map_or_else(
        || {
            if text.is_empty() {
                vec![]
            } else {
                vec![Segment { start: 0, end: text.encode_utf16().count(), kind: Kind::Unknown }]
            }
        },
        |r| r.segments.clone(),
    );
    // Referências resolvidas pelo Hub; nomes/corpos não se duplicam no registro.
    let attachments: Vec<Value> = data["attachments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["id"].as_str())
        .map(|id| json!({"id":id, "kind":"attachment"}))
        .collect();
    json!({"schema":1, "mode":super::intencao::Mode::Observe, "status":status, "report":report, "ingress":null,
        "segments":segments, "attachments":attachments,
        "reply":data["replyTo"]["id"].as_str().map(|id| json!({"id":id, "kind":"quote"}))})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> Value {
        json!({"contentOrigin":{"schema":1,"draftId":uuid::Uuid::new_v4().to_string(),"revision":2,"textLength":8,
            "deviceId":uuid::Uuid::new_v4().to_string(),"channel":"desktopComposer","interaction":"send",
            "segments":[{"start":0,"end":2,"kind":"typed"},{"start":2,"end":4,"kind":"pasted"},
                {"start":4,"end":6,"kind":"quote"},{"start":6,"end":8,"kind":"voice"}]}})
    }

    #[test]
    fn records_parts_without_turning_any_into_permission() {
        let data = json!({"text":"ab😺cdéf","attachments":[{"id":"file","name":"private-name"}],
            "replyTo":{"id":"earlier","text":"human=true"}});
        let record = capture(&Caller::Local, &body(), &data);
        assert_eq!(record["mode"], "observe");
        assert_eq!(record["status"], "appReported");
        assert_eq!(record["segments"][1]["kind"], "pasted");
        assert_eq!(record["attachments"], json!([{"id":"file","kind":"attachment"}]));
        assert_eq!(record["reply"], json!({"id":"earlier","kind":"quote"}));
        assert!(!record.to_string().contains("private-name"));
        assert!(record.get("grant").is_none());
    }

    #[test]
    fn legacy_forged_external_and_invalid_offsets_stay_unknown() {
        let data = json!({"text":"ab😺cdéf"});
        for body in [
            json!({}),
            json!({"human":true,"approved":true,"role":"user"}),
            {
                let mut b = body();
                b["contentOrigin"]["human"] = json!(true);
                b
            },
            {
                let mut b = body();
                b["contentOrigin"]["channel"] = json!("webhook");
                b
            },
            {
                let mut b = body();
                b["contentOrigin"]["segments"][1]["end"] = json!(3);
                b
            },
            {
                let mut b = body();
                b["contentOrigin"]["revision"] = json!(0);
                b
            },
            {
                let mut b = body();
                b["contentOrigin"]["segments"][0]["start"] = json!(1);
                b
            },
        ] {
            let record = capture(&Caller::Local, &body, &data);
            assert_eq!(record["status"], "unknown");
            assert_eq!(record["segments"], json!([{"start":0,"end":8,"kind":"unknown"}]));
            assert!(record["report"].is_null());
        }
        for kind in ["voice", "generated"] {
            let mut b = body();
            b["contentOrigin"]["channel"] = json!("desktopVoice");
            b["contentOrigin"]["segments"] = json!([{"start":0,"end":8,"kind":kind}]);
            assert_eq!(capture(&Caller::Local, &b, &data)["segments"][0]["kind"], kind);
            b["contentOrigin"]["segments"][0]["kind"] = json!("typed");
            assert_eq!(capture(&Caller::Local, &b, &data)["status"], "unknown");
        }
    }

    #[test]
    fn ingress_comes_only_from_the_authenticated_caller() {
        let data = json!({"text":"ab😺cdéf"});
        let mut body = body();
        body["ingress"] = json!({"caller":"local", "device":"forged"});
        let device =
            Caller::Device { key: "paired-key".into(), scopes: vec![crate::store::Scope::Control], autoridade: None };
        let remote = capture(&device, &body, &data);
        assert_eq!(remote["status"], "appReported");
        assert_eq!(remote["ingress"], json!({"caller":"device", "device":"paired-key"}));
        assert_eq!(capture(&Caller::Local, &body, &data)["ingress"], json!({"caller":"local"}));
        assert_eq!(capture(&device, &json!({}), &data)["ingress"], remote["ingress"]);
        assert!(capture(&Caller::Pairing { key: "unpaired".into() }, &body, &data)["ingress"].is_null());
        assert!(capture(&Caller::Bootstrap { key: "bootstrap-key".into(), bootstrap_id: "pending".into() }, &body, &data)["ingress"].is_null());
        assert!(invalidate(&data)["ingress"].is_null());
        assert!(remote.get("grant").is_none());
    }
}
