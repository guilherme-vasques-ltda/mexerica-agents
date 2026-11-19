use super::*;
use crate::fork::spaces::PERSONAL;

#[test]
fn aceita_emoji_compostos_e_recusa_caracteres_invisiveis() {
    let england = "🏴\u{e0067}\u{e0062}\u{e0065}\u{e006e}\u{e0067}\u{e007f} Time";
    for name in ["❤️ Família", "👨‍💻 Dev", "🏳️‍🌈", england, "👍🏽 Equipe", "❤️‍🔥", "1️⃣", "🇧🇷"]
    {
        assert_eq!(valid_name(name).unwrap(), name, "{name:?}");
    }
    for name in [
        "\u{200d}",
        "a\u{200d}b",
        "a\u{200e}b",
        "\u{2800}",
        "a\u{2800}b",
        "\u{3164}",
        "\u{0301}",
        "🏴\u{e0067}",
        "a\u{fe0f}",
        "🏽",
    ] {
        let error = valid_name(name).unwrap_err();
        assert_eq!(crate::api::error_code(&error), Some("name_invalid"), "{name:?}");
    }
}

#[test]
fn grupo_migrado_com_emoji_pode_ser_renomeado() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let db = store.db.locked();
    db.execute_batch("DELETE FROM kv WHERE k='bot_groups.migrated'; CREATE TABLE donna_bot_sections(id TEXT PRIMARY KEY,name TEXT NOT NULL,position INTEGER NOT NULL); INSERT INTO donna_bot_sections VALUES('emoji','👨‍💻 Dev',0);").unwrap();
    legado::migrate(&db).unwrap();
    drop(db);
    let (snapshot, _, _) =
        store.edit_bot_groups(PERSONAL, Edit::Rename { id: "emoji".into(), name: "👨‍💻 Equipe".into() }).unwrap();
    assert_eq!(snapshot.groups[0].name, "👨‍💻 Equipe");
}
