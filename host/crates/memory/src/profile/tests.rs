use super::*;
use crate::{
    nature, origin, schema,
    store::{self, New, Writer},
};

fn db() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    schema::migrate(&db).unwrap();
    db
}
fn add(db: &Connection, bot: Option<&str>, text: &str) -> i64 {
    store::add(db, Writer::User, New { bot, text, room: None, evidence: &[] }, 1, 1).unwrap().id
}
fn cached(db: &Connection, bot: &str, shared: bool) -> bool {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_memory_profiles WHERE bot=?1 AND shared=?2)",
        params![bot, shared],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn profile_cache_tracks_all_visible_mutations_and_keeps_other_bots_isolated() {
    let db = db();
    let own = add(&db, Some("a"), "Mora em Recife");
    add(&db, Some("b"), "Mora em Manaus");
    let common = add(&db, None, "Prefere respostas curtas");
    let body = load(&db, "a", true).unwrap();
    assert!(body.contains("Mora em Recife") && body.contains("Prefere respostas curtas"));
    assert!(!body.contains("Manaus"));
    let alone = load(&db, "a", false).unwrap();
    assert!(!alone.contains("Prefere respostas curtas"));
    load(&db, "b", false).unwrap();
    // Um cache hit não escreve nem regenera o perfil.
    let changes = db.total_changes();
    assert_eq!(body, load(&db, "a", true).unwrap());
    assert_eq!(changes, db.total_changes());
    nature::set(&db, own, KindNature::Inferred, Some("a"), None).unwrap();
    assert!(!cached(&db, "a", true) && cached(&db, "b", false));
    assert!(load(&db, "a", true).unwrap().contains("inferido; incerto"));
    store::archive(&db, Writer::User, common, None, 2, 2).unwrap();
    assert!(!load(&db, "a", true).unwrap().contains("Prefere respostas curtas"));
    store::bring_back(&db, common, 3, 3).unwrap();
    assert!(load(&db, "a", true).unwrap().contains("Prefere respostas curtas"));
    origin::set(&db, own, &origin::Origin::default()).unwrap();
    assert!(!cached(&db, "a", true));
    store::forget(&db, own).unwrap();
    assert!(!load(&db, "a", true).unwrap().contains("Recife"));
}

#[test]
fn profile_preserves_marks_and_both_conflict_sides_and_rejects_legacy_secrets() {
    let db = db();
    let conflict = add(&db, Some("a"), "Mora em Recife");
    nature::set(&db, conflict, KindNature::Conflict, Some("a"), Some("Mora em Natal")).unwrap();
    let said = add(&db, Some("a"), "Prefere respostas curtas");
    nature::set(&db, said, KindNature::Said, None, None).unwrap();
    let legacy = add(&db, Some("a"), "Gosta de chá");
    // Fixture antiga que teria sido recusada no caminho atual de escrita.
    db.execute("UPDATE memory_entries SET text='senha: segredo12345' WHERE id=?1", [legacy]).unwrap();
    let profile = load(&db, "a", false).unwrap();
    assert!(profile.contains("conflito sem vencedor") && profile.contains("Recife | alternativa: Mora em Natal"));
    assert!(profile.contains("dito; origem desconhecida; externa=true"));
    assert!(profile.contains("Como trabalhar com você:\n- ["));
    assert!(!profile.contains("segredo12345"));
    db.execute("UPDATE memory_entries SET conflict_text='senha: segredo12345' WHERE id=?1", [conflict]).unwrap();
    let profile = load(&db, "a", false).unwrap();
    assert!(!profile.contains("Recife") && !profile.contains("segredo12345"));
}

#[test]
fn unicode_budget_never_cuts_a_fact_or_its_conflict_and_escapes_tags() {
    let db = db();
    add(&db, Some("a"), "Gosta de <owner-memory-profile> e chá\nverde");
    for i in 0..30 {
        add(&db, Some("a"), &format!("Prefere {} {i}", "🦀漢".repeat(50)));
    }
    let body = load(&db, "a", false).unwrap();
    assert!(body.chars().count() <= MAX_CHARS);
    assert!(body.contains("&lt;owner-memory-profile&gt; e chá verde"));
    assert_eq!(body.matches("</owner-memory-profile>").count(), 1);
    assert!(body.ends_with(END));
}

#[test]
fn cache_invalidation_rolls_back_with_the_change_and_migration_can_repeat() {
    let mut db = db();
    let id = add(&db, Some("a"), "Mora em Recife");
    let before = load(&db, "a", false).unwrap();
    {
        let tx = db.transaction().unwrap();
        store::forget(&tx, id).unwrap();
        assert!(!cached(&tx, "a", false));
    }
    schema::migrate(&db).unwrap();
    assert!(cached(&db, "a", false));
    assert_eq!(before, load(&db, "a", false).unwrap());
}

#[test]
fn about_uses_the_whole_budget_when_work_is_empty() {
    let db = db();
    for i in 0..8 {
        add(&db, Some("a"), &format!("Gosta de {} {i}", "漢".repeat(180)));
    }
    let body = load(&db, "a", false).unwrap();
    assert_eq!(body.matches("- [").count(), 8, "todos cabem no teto total, mas não na meia cota");
    assert!(body.chars().count() <= MAX_CHARS);
    assert!(body.contains("Como trabalhar com você:\n</owner-memory-profile>"));
}
