// D70 autoral da DONNA Bot. Incluído apenas no módulo de testes bootstrap existente.
// Mesma Connection e leitores ACS4; sem provedor, autoridade nova ou SQL na cloud.
#[derive(Clone, Copy)]
enum OAuthUpgradeFault {
    Statement,
    Commit,
}

thread_local! {
    static OAUTH_UPGRADE_FAULT: std::cell::Cell<Option<OAuthUpgradeFault>> = const { std::cell::Cell::new(None) };
}

pub(super) fn oauth_upgrade_fault(db: &Connection) -> Result<()> {
    match OAUTH_UPGRADE_FAULT.with(std::cell::Cell::take) {
        Some(OAuthUpgradeFault::Statement) => db.execute_batch("SELECT no_such_oauth_upgrade_function();")?,
        Some(OAuthUpgradeFault::Commit) => db.execute_batch("CREATE TABLE oauth_upgrade_parent(id INTEGER PRIMARY KEY);
            CREATE TABLE oauth_upgrade_failure(id INTEGER REFERENCES oauth_upgrade_parent(id) DEFERRABLE INITIALLY DEFERRED);
            INSERT INTO oauth_upgrade_failure VALUES(1);")?,
        None => {},
    }
    Ok(())
}

fn oauth_scope_device() -> Device {
    Device {
        key: "oauth-shape".into(),
        name: "fixture".into(),
        platform: "fixture".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control, Scope::Screen, Scope::Control],
        lease_until: None,
        created_at: 1,
        last_seen_at: None,
    }
}

fn oauth_epoch(db: &Connection) -> String {
    db.query_row("SELECT donna_oauth_epoch FROM devices WHERE key='oauth-shape'", [], |row| row.get(0)).unwrap()
}

fn oauth_acl_version(db: &Connection) -> i64 {
    db.query_row("SELECT acl_version FROM bootstrap_generation WHERE singleton=1", [], |row| row.get(0)).unwrap()
}

fn oauth_bootstrap_state(db: &Connection) -> (String, String) {
    let generation = db
        .query_row(
            "SELECT json_array(config,ever_claimed,revision,a1_input,a1_signature)
        FROM bootstrap_generation WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let slot = db
        .query_row(
            "SELECT json_array(hex(grant_wire),grant_hash,hex(challenge_wire),status,pending,
        hex(receipt_wire),receipt_hash,hex(proof_wire),hex(revocation_wire),hex(activation_wire))
        FROM bootstrap_slot WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    (generation, slot)
}

fn oauth_install_old_bootstrap_trigger(db: &Connection) {
    // Definição exata anterior: conserva duplicatas, mas não protegia JSON inválido.
    db.execute_batch(
        "DROP TRIGGER bootstrap_acl_update;
        CREATE TRIGGER bootstrap_acl_update AFTER UPDATE ON devices
          WHEN OLD.key IS NOT NEW.key OR OLD.source IS NOT NEW.source OR OLD.grant_id IS NOT NEW.grant_id
            OR (SELECT json_group_array(value) FROM (SELECT value FROM json_each(OLD.scopes) ORDER BY value))
              IS NOT (SELECT json_group_array(value) FROM (SELECT value FROM json_each(NEW.scopes) ORDER BY value))
            OR OLD.lease_until IS NOT NEW.lease_until BEGIN
          UPDATE bootstrap_generation SET acl_version=acl_version+1 WHERE singleton=1; END;",
    )
    .unwrap();
}

#[test]
fn upstream02_oauth_bootstrap_keeps_acl_duplicates_order_and_epoch_only_updates() {
    let (store, issuer, _, _, cfg, grant) = fixture();
    store.bootstrap_install_generation(&cfg).unwrap();
    let wire = codec::encode(&grant, |bytes| crypto::sign(&issuer, bytes)).unwrap();
    store.bootstrap_install_grant(&wire, 2_000).unwrap();
    store.put_device(&oauth_scope_device()).unwrap();
    let db = store.db.locked();
    let state = oauth_bootstrap_state(&db);
    let before = acl(&db).unwrap();
    let epoch = oauth_epoch(&db);
    db.execute("UPDATE devices SET scopes='[\"screen\",\"control\",\"control\"]' WHERE key='oauth-shape'", []).unwrap();
    assert_eq!(acl(&db).unwrap(), before, "ordem mantém o multiconjunto e o hash existentes");
    assert_eq!(oauth_epoch(&db), epoch);
    db.execute("UPDATE devices SET scopes='[\"screen\",\"control\"]' WHERE key='oauth-shape'", []).unwrap();
    let without_duplicate = acl(&db).unwrap();
    assert_eq!(without_duplicate.0, before.0 + 1, "bootstrap preserva duplicatas na versão");
    assert_ne!(without_duplicate.1, before.1, "acl() também preserva duplicatas no hash");
    assert_eq!(oauth_epoch(&db), epoch, "época OAuth normaliza conjunto; não copiar isso para ACL");
    db.execute("UPDATE devices SET lease_until=123 WHERE key='oauth-shape'", []).unwrap();
    assert_eq!(oauth_acl_version(&db), without_duplicate.0 + 1, "lease mantém sua semântica ACL");
    assert_eq!(oauth_epoch(&db), epoch, "renovação não estende a intenção OAuth original");
    let version = oauth_acl_version(&db);
    db.execute("UPDATE devices SET donna_oauth_epoch=lower(hex(randomblob(16))) WHERE key='oauth-shape'", []).unwrap();
    db.execute("UPDATE devices SET name='renamed',last_seen_at=99 WHERE key='oauth-shape'", []).unwrap();
    assert_eq!(oauth_acl_version(&db), version, "época interna/metadados não alteram ACL");
    assert_eq!(oauth_bootstrap_state(&db), state, "nenhum reset de geração/estado/revisão");
}

#[test]
fn upstream02_oauth_bootstrap_corruption_recovery_and_rollback_are_joint_and_fail_closed() {
    let (store, issuer, _, _, cfg, grant) = fixture();
    store.bootstrap_install_generation(&cfg).unwrap();
    let wire = codec::encode(&grant, |bytes| crypto::sign(&issuer, bytes)).unwrap();
    store.bootstrap_install_grant(&wire, 2_000).unwrap();
    let device = oauth_scope_device();
    store.put_device(&device).unwrap();
    let db = store.db.locked();
    let state = oauth_bootstrap_state(&db);
    for invalid in ["{", "\"control\""] {
        let before = acl(&db).unwrap();
        let epoch = oauth_epoch(&db);
        {
            let tx = db.unchecked_transaction().unwrap();
            tx.execute("UPDATE devices SET scopes=?1 WHERE key='oauth-shape'", [invalid]).unwrap();
            assert_eq!(oauth_acl_version(&tx), before.0 + 1, "inclui UPDATE interno de época sem parser inválido");
            assert_ne!(oauth_epoch(&tx), epoch);
            assert!(acl(&tx).is_err(), "leitor existente recusa JSON malformado e não-array");
            tx.rollback().unwrap();
        }
        assert!(db.is_autocommit());
        assert_eq!(acl(&db).unwrap(), before);
        assert_eq!(oauth_epoch(&db), epoch, "rollback reverte scopes/época/ACL juntos");
        db.execute("UPDATE devices SET scopes=?1 WHERE key='oauth-shape'", [invalid]).unwrap();
        let invalid_epoch = oauth_epoch(&db);
        let version = oauth_acl_version(&db);
        db.execute("UPDATE devices SET donna_oauth_epoch=lower(hex(randomblob(16))) WHERE key='oauth-shape'", [])
            .unwrap();
        assert_eq!(oauth_acl_version(&db), version, "UPDATE só época em registro inválido é seguro");
        assert!(acl(&db).is_err());
        db.execute("UPDATE devices SET scopes='[\"control\",\"screen\",\"control\"]' WHERE key='oauth-shape'", [])
            .unwrap();
        assert_eq!(oauth_acl_version(&db), version + 1, "restaurar não ressuscita versão antiga");
        assert_ne!(oauth_epoch(&db), invalid_epoch);
        assert_ne!(oauth_epoch(&db), epoch);
        assert_eq!(acl(&db).unwrap().1, before.1, "hash restaurado não substitui versão monotônica");
        assert_eq!(oauth_bootstrap_state(&db), state);
    }
}

#[test]
fn upstream02_oauth_bootstrap_upgrade_error_restores_trigger_state_and_autocommit() {
    let (store, issuer, _, _, cfg, grant) = fixture();
    store.bootstrap_install_generation(&cfg).unwrap();
    let wire = codec::encode(&grant, |bytes| crypto::sign(&issuer, bytes)).unwrap();
    store.bootstrap_install_grant(&wire, 2_000).unwrap();
    store.put_device(&oauth_scope_device()).unwrap();
    let db = store.db.locked();
    db.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    oauth_install_old_bootstrap_trigger(&db);
    let old_sql: String =
        db.query_row("SELECT sql FROM sqlite_master WHERE name='bootstrap_acl_update'", [], |row| row.get(0)).unwrap();
    let before = (oauth_bootstrap_state(&db), acl(&db).unwrap(), oauth_epoch(&db));
    for fault in [OAuthUpgradeFault::Statement, OAuthUpgradeFault::Commit] {
        OAUTH_UPGRADE_FAULT.with(|slot| slot.set(Some(fault)));
        assert!(upgrade_acl_update(&db).is_err());
        assert!(db.is_autocommit(), "erro de instrução/commit não deixa transação pendurada");
        assert_eq!(
            db.query_row("SELECT sql FROM sqlite_master WHERE name='bootstrap_acl_update'", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            old_sql
        );
        assert_eq!((oauth_bootstrap_state(&db), acl(&db).unwrap(), oauth_epoch(&db)), before);
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name LIKE 'oauth_upgrade_%'", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    upgrade_acl_update(&db).unwrap();
    assert!(db.is_autocommit());
    assert_eq!((oauth_bootstrap_state(&db), acl(&db).unwrap(), oauth_epoch(&db)), before);
}

#[test]
fn upstream02_oauth_bootstrap_reopen_upgrades_old_trigger_before_epoch_backfill() {
    let (_seed, issuer, _, _, cfg, grant) = fixture();
    let dir = crate::store::private_fixture_dir("oauth-bootstrap-upgrade");
    let path = dir.join("bootstrap.sqlite");
    let before = {
        let store = Store::open(&path).unwrap();
        store.bootstrap_install_generation(&cfg).unwrap();
        let wire = codec::encode(&grant, |bytes| crypto::sign(&issuer, bytes)).unwrap();
        store.bootstrap_install_grant(&wire, 2_000).unwrap();
        store.put_device(&oauth_scope_device()).unwrap();
        let db = store.db.locked();
        db.execute("UPDATE devices SET scopes='{' WHERE key='oauth-shape'", []).unwrap();
        db.execute("INSERT INTO devices(key,name,platform,source,scopes,created_at) VALUES('empty-epoch','Legacy','fixture','local','{',1)", []).unwrap();
        db.execute("UPDATE devices SET donna_oauth_epoch='' WHERE key='empty-epoch'", []).unwrap();
        oauth_install_old_bootstrap_trigger(&db);
        db.execute_batch("CREATE TRIGGER oauth_unrelated_keep BEFORE UPDATE OF name ON devices BEGIN SELECT 1; END;")
            .unwrap();
        (oauth_bootstrap_state(&db), oauth_acl_version(&db), oauth_epoch(&db))
    };
    let after = {
        let store = Store::open(&path).unwrap();
        let db = store.db.locked();
        assert_eq!(
            (oauth_bootstrap_state(&db), oauth_acl_version(&db), oauth_epoch(&db)),
            before,
            "DDL não redefine geração/ACL/época não vazia"
        );
        let backfilled: String = db
            .query_row("SELECT donna_oauth_epoch FROM devices WHERE key='empty-epoch'", [], |row| row.get(0))
            .unwrap();
        assert_ne!(backfilled, "", "época vazia recebe valor sem usar o parser do trigger antigo");
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name='oauth_unrelated_keep'", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        db.execute("UPDATE devices SET donna_oauth_epoch=lower(hex(randomblob(16))) WHERE key='oauth-shape'", [])
            .unwrap();
        assert_eq!(oauth_acl_version(&db), before.1);
        assert!(acl(&db).is_err());
        db.execute("UPDATE devices SET scopes='[\"control\",\"screen\",\"control\"]' WHERE key IN ('oauth-shape','empty-epoch')", []).unwrap();
        assert!(acl(&db).is_ok());
        assert_eq!(oauth_acl_version(&db), before.1 + 2);
        (oauth_bootstrap_state(&db), acl(&db).unwrap(), oauth_epoch(&db))
    };
    {
        let store = Store::open(&path).unwrap();
        let db = store.db.locked();
        assert_eq!((oauth_bootstrap_state(&db), acl(&db).unwrap(), oauth_epoch(&db)), after);
    }
    let _ = std::fs::remove_dir_all(dir);
}
