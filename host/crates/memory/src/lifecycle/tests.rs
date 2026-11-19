use super::*;
use crate::{
    nature,
    origin::{self, Kind, Source},
    schema,
    store::{New, Writer},
};

fn db() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    schema::migrate(&db).unwrap();
    db
}
fn fact(db: &Connection, text: &str) -> Entry {
    let e = store::add(db, Writer::Bot("a"), New { bot: Some("a"), text, room: None, evidence: &["m1".into()] }, 1, 1)
        .unwrap();
    nature::set(db, e.id, KindNature::Inferred, Some("a"), None).unwrap();
    store::get(db, e.id).unwrap().unwrap()
}
fn origin(message: &str) -> Origin {
    Origin {
        schema: 1,
        kind: Kind::Conversation,
        external: false,
        sources: vec![Source { message_id: message.into(), kind: Kind::User, detail: None }],
    }
}

#[test]
fn weak_fact_archives_at_boundary_keeps_history_and_owner_restoration_survives() {
    let db = db();
    let e = fact(&db, "Mora em Recife");
    assert_eq!(age(&db, Some("a"), WEAK_AGE_MS, 2).unwrap(), 0);
    assert_eq!(age(&db, Some("a"), WEAK_AGE_MS + 1, 2).unwrap(), 1);
    let archived = store::get(&db, e.id).unwrap().unwrap();
    assert_eq!(archived.state, State::Archived);
    assert_eq!(archived.text, e.text);
    assert_eq!(archived.previous_version.unwrap().state, "active");
    let reason: String =
        db.query_row("SELECT reason FROM donna_memory_aging WHERE entry=?1", [e.id], |r| r.get(0)).unwrap();
    assert_eq!(reason, "weak_unreinforced_90d");
    assert_eq!(age(&db, Some("a"), WEAK_AGE_MS + 2, 3).unwrap(), 0);
    assert_eq!(store::search(&db, "a", "Recife", 5).unwrap().len(), 1);
    store::bring_back(&db, e.id, WEAK_AGE_MS + 3, 3).unwrap();
    assert_eq!(age(&db, Some("a"), 3 * WEAK_AGE_MS, 4).unwrap(), 0);
}

#[test]
fn repetition_adds_strength_once_per_new_source_without_claiming_certainty() {
    let db = db();
    let e = fact(&db, "Mora em Recife");
    assert!(!reinforce(&db, Some("a"), &e.text, &["m1".into()], &origin("m1"), 2, 2).unwrap());
    assert!(reinforce(&db, Some("a"), &e.text, &["m2".into(), "m2".into()], &origin("m2"), 3, 3).unwrap());
    assert!(!reinforce(&db, Some("a"), &e.text, &["m2".into()], &origin("m2"), 4, 4).unwrap());
    let next = store::get(&db, e.id).unwrap().unwrap();
    assert_eq!(next.nature, KindNature::Inferred);
    assert_eq!(next.updated_at, 3);
    assert_eq!(next.evidence, ["m1", "m2"]);
    assert_eq!(age(&db, Some("a"), 3 * WEAK_AGE_MS, 5).unwrap(), 0);
    assert!(!reinforce(&db, Some("b"), &e.text, &["m3".into()], &origin("m3"), 5, 5).unwrap());
    assert!(!reinforce(&db, Some("a"), &e.text, &["m3".into()], &Origin::default(), 5, 5).unwrap());
    store::revise(&db, Writer::Bot("a"), e.id, "Mora em Natal", 6, 6).unwrap();
    let count: i64 = db.query_row("SELECT COUNT(*) FROM donna_memory_reinforcements", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 0, "reescrita não herda reforços do texto anterior");
}

#[test]
fn aging_protects_explicit_pinned_conflicting_legacy_and_human_facts() {
    let db = db();
    for (i, kind) in
        [KindNature::Said, KindNature::Confirmed, KindNature::Conflict, KindNature::Unknown].into_iter().enumerate()
    {
        let e = fact(&db, &format!("Mora na cidade {i}"));
        nature::set(&db, e.id, kind, None, None).unwrap();
    }
    let pinned = fact(&db, "Gosta de chá");
    store::pin(&db, pinned.id, true, 2, 2).unwrap();
    let conflict = fact(&db, "Gosta de café");
    nature::set(&db, conflict.id, KindNature::Inferred, Some("a"), Some("Gosta de mate")).unwrap();
    let human = store::add(
        &db,
        Writer::User,
        New { bot: Some("a"), text: "Mora em Belém", room: None, evidence: &["m1".into()] },
        1,
        1,
    )
    .unwrap();
    nature::set(&db, human.id, KindNature::Inferred, Some("a"), None).unwrap();
    let human_before = store::get(&db, human.id).unwrap().unwrap();
    assert!(!reinforce(&db, Some("a"), &human.text, &["m2".into()], &origin("m2"), 2, 2).unwrap());
    assert_eq!(store::get(&db, human.id).unwrap().unwrap(), human_before, "reforço não edita um fato humano");
    assert_eq!(age(&db, Some("a"), 3 * WEAK_AGE_MS, 3).unwrap(), 0);
    assert!(!reinforce(&db, Some("a"), &conflict.text, &["m2".into()], &origin("m2"), 4, 4).unwrap());
    origin::set(&db, pinned.id, &Origin::default()).unwrap();
}

#[test]
fn failure_rolls_back_archive_reason_and_versions_together() {
    let mut db = db();
    let e = fact(&db, "Mora em Recife");
    db.execute_batch(
        "CREATE TRIGGER fail_age BEFORE INSERT ON donna_memory_aging BEGIN SELECT RAISE(ABORT,'fixture'); END;",
    )
    .unwrap();
    {
        let tx = db.transaction().unwrap();
        assert!(age(&tx, Some("a"), WEAK_AGE_MS + 1, 2).is_err());
    }
    assert_eq!(store::get(&db, e.id).unwrap().unwrap(), e);
}
