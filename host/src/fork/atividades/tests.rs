#![allow(clippy::unwrap_used)]
use super::{modelo::*, store::with_store_tx};
use crate::{fork::escopo::ContextRef, store::Store};
use serde_json::{Value, json};

fn id(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}
fn context() -> Value {
    json!({"version":1,"hubId":id(1),"spaceId":id(2),"conversationId":id(3),"botId":id(3),
        "audience":{"kind":"principal"},"teamId":null,"privacyDomainId":id(4),
        "domainRevision":1,"conversationRevision":2})
}
fn store() -> Store {
    Store::open(std::path::Path::new(":memory:")).unwrap()
}

#[test]
fn context_roundtrip_rejects_versions_aliases_and_extra_authority() {
    let raw = context();
    let parsed: ContextRef = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), raw);
    for (key, value) in [
        ("version", json!(2)),
        ("hubId", json!("computer-label")),
        ("hubId", json!("00000000-0000-0000-0000-000000000000")),
        ("domainRevision", json!(0)),
        ("domainRevision", json!(9_007_199_254_740_992_u64)),
        ("conversationId", json!(id(9))),
        ("approved", json!(true)),
        ("audience", json!({"kind":"principal","owner":true})),
    ] {
        let mut invalid = raw.clone();
        invalid[key] = value;
        assert!(serde_json::from_value::<ContextRef>(invalid).is_err(), "{key}");
    }
    let mut other = raw.clone();
    other["hubId"] = json!(id(10));
    assert_ne!(parsed, serde_json::from_value::<ContextRef>(other).unwrap());
    let mut other = raw;
    other["spaceId"] = json!(id(11));
    assert_ne!(parsed, serde_json::from_value::<ContextRef>(other).unwrap());
}

#[test]
fn audiences_do_not_fallback_to_primary_or_infer_group_from_label() {
    let mut raw = context();
    raw["audience"] = json!({"kind":"additional"});
    assert!(serde_json::from_value::<ContextRef>(raw.clone()).is_err());
    raw["conversationId"] = json!(id(8));
    assert!(serde_json::from_value::<ContextRef>(raw.clone()).is_ok());
    raw["audience"] = json!({"kind":"group","groupId":id(9)});
    assert!(serde_json::from_value::<ContextRef>(raw.clone()).is_err());
    raw["audience"]["groupId"] = json!(id(8));
    assert!(serde_json::from_value::<ContextRef>(raw).is_ok());
}

#[test]
fn refs_roundtrip_without_creating_agent_or_human_authority() {
    let step = json!({"version":1,"rootActivityId":id(10),"stepExecutionId":id(11),
        "logicalStepKey":"check.input","planVersionRef":{"domain":"workflow","definitionId":id(13),
        "versionId":id(14),"digest":"a".repeat(64)}});
    let owner = json!({"kind":"deterministic_step","rootActivityId":id(10),"stepRef":step});
    let parsed: RequestOwnerRef = serde_json::from_value(owner.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), owner);
    for (key, value) in [
        ("logicalStepKey", json!("../outside")),
        (
            "planVersionRef",
            json!({
        "domain":"workflow","definitionId":id(13),"versionId":id(14),"digest":"private-body"}),
        ),
    ] {
        let mut invalid = owner.clone();
        invalid["stepRef"][key] = value;
        assert!(serde_json::from_value::<RequestOwnerRef>(invalid).is_err());
    }
    let mut invalid = owner.clone();
    invalid["rootActivityId"] = json!(id(99));
    assert!(serde_json::from_value::<RequestOwnerRef>(invalid).is_err());
    let mut invalid = owner;
    invalid["executionRef"] = json!({"botId":id(3)});
    assert!(serde_json::from_value::<RequestOwnerRef>(invalid).is_err());
    let route = json!({"hubId":id(1),"version":1,"originContext":null,"originEntryId":null,"requesterExecutionId":null,
        "destination":{"kind":"retained"},"routeRevision":1,"policyRevision":1});
    let parsed: RouteRef = serde_json::from_value(route.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), route);
    let mut invalid = route.clone();
    invalid["destination"] = json!({"kind":"group","contextRef":context()});
    assert!(serde_json::from_value::<RouteRef>(invalid).is_err());
    let mut invalid = route;
    invalid["destination"]["contextRef"] = context();
    assert!(serde_json::from_value::<RouteRef>(invalid).is_err());
    let origin = json!({"version":1,"kind":"typed_owner","sourceRef":null,
        "authenticatedCarrierRef":null,"appReported":{"kind":"unknown"}});
    assert!(serde_json::from_value::<OriginRef>(origin.clone()).is_ok());
    let mut invalid = origin.clone();
    invalid["appReported"]["human"] = json!(true);
    assert!(serde_json::from_value::<OriginRef>(invalid).is_err());
    let mut invalid = origin;
    invalid["human"] = json!(true);
    assert!(serde_json::from_value::<OriginRef>(invalid).is_err());
    let execution = json!({"version":1,"executionId":id(20),"activityId":null,"attemptId":id(21),
        "turnId":id(22),"generation":1,"hubBootId":id(23),"fence":2,"contextRef":context()});
    let parsed: ExecutionRef = serde_json::from_value(execution.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), execution);
    let mut invalid = execution;
    invalid["contextRef"]["conversationId"] = json!(id(90));
    invalid["contextRef"]["audience"] = json!({"kind":"activity","activityId":id(91)});
    assert!(serde_json::from_value::<ExecutionRef>(invalid).is_err());
    assert!(serde_json::from_value::<ActivityState>(json!("unknown")).is_err());
}

#[test]
fn tx_uses_same_connection_and_publishes_only_committed_value() {
    let store = store();
    store.kv_set("d171.fixture", "before").unwrap();
    let committed = with_store_tx(&store, |tx| {
        let before: String = tx.scalar("SELECT v FROM kv WHERE k=?1", ["d171.fixture"])?;
        tx.execute("UPDATE kv SET v=?1 WHERE k=?2", ["after", "d171.fixture"])?;
        Ok(before)
    })
    .unwrap();
    assert_eq!(committed, "before");
    assert_eq!(store.kv_read("d171.fixture").unwrap().as_deref(), Some("after"));
}

#[test]
fn sql_fault_and_swallowed_error_cannot_commit_partial_state() {
    let store = store();
    // Falha SQLite real após uma escrita; nenhuma tabela de domínio simulada no produto.
    store
        .db
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TEMP TRIGGER d171_fault BEFORE INSERT ON kv
        WHEN NEW.k='d171.fail' BEGIN SELECT RAISE(ABORT,'fixture'); END;",
        )
        .unwrap();
    let result = with_store_tx(&store, |tx| {
        tx.execute("INSERT INTO kv(k,v) VALUES(?1,?2)", ["d171.partial", "value"])?;
        let error = tx.execute("INSERT INTO kv(k,v) VALUES(?1,?2)", ["d171.fail", "value"]);
        assert_eq!(error, Err(CoreError::Storage));
        Ok("false_ack")
    });
    assert_eq!(result, Err(CoreError::RollbackOnly));
    assert_eq!(store.kv_read("d171.partial").unwrap(), None);
    assert_eq!(store.kv_read("d171.fail").unwrap(), None);
    assert_eq!(with_store_tx(&store, |_| Err::<(), _>(CoreError::ScopeUnavailable)), Err(CoreError::ScopeUnavailable));
}

#[test]
fn nested_tx_and_sql_control_taint_outer_even_when_error_is_swallowed() {
    let store = store();
    assert_eq!(
        with_store_tx(&store, |tx| {
            tx.execute("INSERT INTO kv(k,v) VALUES('d171.nested','value')", [])?;
            assert_eq!(with_store_tx(&store, |_| Ok(())), Err(CoreError::NestedTransaction));
            Ok(())
        }),
        Err(CoreError::RollbackOnly)
    );
    assert_eq!(store.kv_read("d171.nested").unwrap(), None);
    for sql in [
        "COMMIT",
        "ROLLBACK",
        "SAVEPOINT nested",
        "ATTACH DATABASE ':memory:' AS other",
        "INSERT INTO kv(k,v) VALUES('d171.control','x'); COMMIT",
    ] {
        assert_eq!(
            with_store_tx(&store, |tx| {
                let _ = tx.execute(sql, []);
                Ok(())
            }),
            Err(CoreError::RollbackOnly)
        );
    }
    assert_eq!(store.kv_read("d171.control").unwrap(), None);
}

#[test]
fn tx_limits_and_commit_fault_have_no_success_path() {
    let store = store();
    assert_eq!(
        with_store_tx(&store, |tx| {
            tx.execute("INSERT INTO kv(k,v) VALUES('d171.budget','value')", [])?;
            for _ in 0..64 {
                let _ = tx.scalar::<i64>("SELECT 1", []);
            }
            Ok(())
        }),
        Err(CoreError::RollbackOnly)
    );
    assert_eq!(store.kv_read("d171.budget").unwrap(), None);
    // FK diferida falha somente no COMMIT, não no INSERT/closure.
    store
        .db
        .lock()
        .unwrap()
        .execute_batch(
            "PRAGMA foreign_keys=ON;
        CREATE TEMP TABLE d171_parent(id INTEGER PRIMARY KEY);
        CREATE TEMP TABLE d171_child(id INTEGER REFERENCES d171_parent(id) DEFERRABLE INITIALLY DEFERRED);",
        )
        .unwrap();
    assert_eq!(
        with_store_tx(&store, |tx| {
            tx.execute("INSERT INTO d171_child(id) VALUES(1)", [])?;
            Ok(())
        }),
        Err(CoreError::Storage)
    );
    assert_eq!(with_store_tx(&store, |tx| tx.scalar::<i64>("SELECT COUNT(*) FROM d171_child", [])).unwrap(), 0);
}
