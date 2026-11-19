#![allow(clippy::unwrap_used)]
use super::{erro_operacao::OperationError as Error, operacao::OperationSelector, revisao::*};
use crate::fork::escopo::Revision;
use serde_json::{Value, json};

fn id(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}
fn context() -> Value {
    json!({"version":1,"hubId":id(1),"spaceId":id(2),"conversationId":id(3),"botId":id(3),
        "audience":{"kind":"principal"},"teamId":null,"privacyDomainId":id(4),"domainRevision":1,"conversationRevision":2})
}
fn object(n: u32, kind: &str) -> Value {
    json!({"contextRef":context(),"kind":kind,"objectId":id(n),"revision":1,"digest":"a".repeat(64)})
}
fn binding() -> Value {
    json!({"version":1,"contextRef":context(),"callerDevice":{"principalId":id(5),"deviceId":id(6),"sessionId":id(7),"principalEpoch":1},
        "recoveryEpoch":id(8),"policyRevision":1,"action":"accept_with_edit","target":object(9,"candidate"),"before":object(9,"candidate"),
        "effect":{"body":"conteúdo-sentinela α","account":object(10,"account"),"destination":object(11,"destination"),"attachments":[object(12,"attachment")]},
        "sources":[object(13,"source"),object(14,"source")],"bindings":[object(15,"binding")]})
}
fn digest(value: Value) -> Value {
    serde_json::to_value(serde_json::from_value::<ReviewBinding>(value).unwrap().digest().unwrap()).unwrap()
}
fn selector(n: u32, kind: &str) -> Value {
    json!({"version":1,"operationId":id(n),"kind":kind})
}
// Projeções NÃO confiáveis. Estas fixtures não constroem Proof/Intent/OperationRef.
fn descriptor() -> ReviewDescriptor {
    serde_json::from_value(
        json!({"version":1,"reviewId":id(20),"prepareOperation":selector(21,"prepare_review"),"prepareNonce":id(24),
        "decisionOperation":selector(22,"decide_review"),"decisionNonce":id(23),"binding":binding(),
        "semanticDigest":digest(binding()),"window":{"issuedAt":1000,"expiresAt":121_000}}),
    )
    .unwrap()
}
fn decision() -> Value {
    json!({"version":1,"reviewId":id(20),"decisionOperation":selector(22,"decide_review"),"decisionNonce":id(23),"binding":binding()})
}
fn time(value: u64) -> Revision {
    Revision::try_from(value).unwrap()
}

#[test]
fn closed_input_never_promotes_authority_or_reuses_prepare_nonce() {
    let raw = json!({"version":1,"prepareNonce":id(24),"decisionNonce":id(23),"binding":binding()});
    let input = PrepareReviewInput::parse_json(&serde_json::to_vec(&raw).unwrap()).unwrap();
    assert_eq!(descriptor().check_prepare(&input), Ok(()));
    let mut changed = raw.clone();
    changed["binding"]["effect"]["body"] = json!("outro corpo");
    assert_eq!(
        descriptor().check_prepare(&PrepareReviewInput::parse_json(&serde_json::to_vec(&changed).unwrap()).unwrap()),
        Err(Error::BindingMismatch)
    );
    let failure = PrepareReviewInput::parse_json(b"{private-sentinel}").unwrap_err();
    assert_eq!(failure, Error::InvalidContract);
    assert!(!failure.to_string().contains("private-sentinel"));
    assert_eq!(DecideReviewInput::parse_json(&vec![b' '; 262_145]), Err(Error::InvalidContract));
    for (key, value) in [
        ("version", json!(2)),
        ("prepareNonce", json!(id(23))),
        ("approved", json!(true)),
        ("human", json!(true)),
        ("operationRef", selector(21, "prepare_review")),
    ] {
        let mut bad = raw.clone();
        bad[key] = value;
        assert!(serde_json::from_value::<PrepareReviewInput>(bad).is_err());
    }
    let mut bad = decision();
    bad["binding"]["callerDevice"]["local"] = json!(true);
    assert!(serde_json::from_value::<DecideReviewInput>(bad).is_err());
    let mut bad = selector(21, "prepare_review");
    bad["kind"] = json!("approved");
    assert!(serde_json::from_value::<OperationSelector>(bad).is_err());
}

#[test]
fn binding_limits_and_scoped_refs_are_validated_before_digest() {
    for pointer in
        ["/effect/body", "/effect/attachments", "/bindings", "/target/contextRef/spaceId", "/effect/account/kind"]
    {
        let mut bad = binding();
        *bad.pointer_mut(pointer).unwrap() = match pointer {
            "/effect/body" => json!("x".repeat(32769)),
            "/effect/attachments" => json!(vec![object(12, "attachment"); 65]),
            "/bindings" => json!([object(15, "memory")]),
            "/effect/account/kind" => json!("memory"),
            _ => json!(id(99)),
        };
        assert!(serde_json::from_value::<ReviewBinding>(bad).is_err(), "{pointer}");
    }
    let mut bad = binding();
    bad["sources"][0]["contextRef"]["hubId"] = json!(id(99));
    assert!(serde_json::from_value::<ReviewBinding>(bad).is_err());
}

#[test]
fn semantic_digest_binds_bytes_and_every_captured_axis_without_logging_body() {
    let raw = binding();
    let original = digest(raw.clone());
    let fields = [
        "/effect/body",
        "/action",
        "/callerDevice/principalId",
        "/callerDevice/deviceId",
        "/callerDevice/sessionId",
        "/callerDevice/principalEpoch",
        "/recoveryEpoch",
        "/policyRevision",
        "/target/objectId",
        "/target/revision",
        "/target/digest",
        "/before/revision",
        "/before/digest",
        "/effect/account/objectId",
        "/effect/account/revision",
        "/effect/destination/objectId",
        "/effect/attachments/0/digest",
        "/sources/0/digest",
        "/bindings/0/digest",
    ];
    for pointer in fields {
        let mut changed = raw.clone();
        let field = changed.pointer_mut(pointer).unwrap();
        *field = if field.is_number() {
            json!(2)
        } else if pointer.ends_with("digest") {
            json!("b".repeat(64))
        } else if pointer == "/action" {
            json!("reject")
        } else if pointer == "/effect/body" {
            json!("conteúdo-sentinela α ")
        } else {
            json!(id(99))
        };
        assert_ne!(digest(changed), original, "{pointer}");
    }
    let mut changed = raw.clone();
    changed["sources"].as_array_mut().unwrap().reverse();
    assert_ne!(digest(changed), original);
    let mut changed = raw.clone();
    changed["contextRef"]["spaceId"] = json!(id(99));
    changed["target"]["contextRef"] = changed["contextRef"].clone();
    assert_ne!(digest(changed), original);
    let parsed: ReviewBinding = serde_json::from_value(raw.clone()).unwrap();
    assert!(!format!("{parsed:?}").contains("conteúdo-sentinela"));
    let reversed: serde_json::Map<String, Value> =
        raw.as_object().unwrap().iter().rev().map(|(k, v)| (k.clone(), v.clone())).collect();
    assert_eq!(digest(Value::Object(reversed)), original);
}

#[test]
fn review_window_is_exact_and_cannot_renew_by_refresh_or_client_clock() {
    let window = ReviewWindow::at(time(1000)).unwrap();
    assert_eq!(window.check_at(time(999)), Err(Error::NotYetIssued));
    assert_eq!(window.check_at(time(1000)), Ok(()));
    assert_eq!(window.check_at(time(120_999)), Ok(()));
    assert_eq!(window.check_at(time(121_000)), Err(Error::Expired));
    assert_eq!(window.check_at(time(999_999)), Err(Error::Expired));
    assert!(ReviewWindow::at(time(9_007_199_254_740_991)).is_err());
    for raw in [
        json!({"issuedAt":1000,"expiresAt":120_999}),
        json!({"issuedAt":0,"expiresAt":120_000}),
        json!({"issuedAt":121_000,"expiresAt":1000}),
    ] {
        assert!(serde_json::from_value::<ReviewWindow>(raw).is_err());
    }
}

#[test]
fn descriptor_match_is_not_a_decision_and_rejects_cross_binding_operation_and_digest() {
    let description = descriptor();
    let input: DecideReviewInput = serde_json::from_value(decision()).unwrap();
    assert_eq!(description.check_decision(&input, time(1000)), Ok(()));
    for (pointer, value, error) in [
        ("/reviewId", json!(id(99)), Error::ReviewMismatch),
        ("/decisionNonce", json!(id(99)), Error::ReviewMismatch),
        ("/decisionOperation/kind", json!("prepare_review"), Error::OperationMismatch),
        ("/decisionOperation/operationId", json!(id(99)), Error::OperationMismatch),
        ("/binding/effect/body", json!("outro conteúdo"), Error::BindingMismatch),
        ("/binding/recoveryEpoch", json!(id(99)), Error::BindingMismatch),
    ] {
        let mut changed = decision();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert_eq!(description.check_decision(&serde_json::from_value(changed).unwrap(), time(1000)), Err(error));
    }
    let mut invalid = serde_json::to_value(&description).unwrap();
    invalid["semanticDigest"] = json!("0".repeat(64));
    assert_eq!(
        serde_json::from_value::<ReviewDescriptor>(invalid).unwrap().check_decision(&input, time(1000)),
        Err(Error::DigestMismatch)
    );
    let mut invalid = serde_json::to_value(&description).unwrap();
    invalid["prepareOperation"]["operationId"] = json!(id(22));
    assert_eq!(
        serde_json::from_value::<ReviewDescriptor>(invalid).unwrap().check_decision(&input, time(1000)),
        Err(Error::OperationMismatch)
    );
}

mod seguranca_05b;
