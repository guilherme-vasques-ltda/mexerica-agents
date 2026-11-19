#![allow(clippy::unwrap_used)]
//! Testes de fronteira na Store real, só fixtures sintéticas; sem scope positivo.
use super::{
    lifecycle::LifecycleChange,
    manifest::{BUILTINS, Builtin, Capability, Effect},
    model::{PluginError, PluginInstallationRef, PluginResourceKind, PluginResourceOwner, PluginResourceRef},
    schema,
    store::{Registry, db_error},
};
use crate::{
    LockExt,
    fork::{
        escopo::{ContextRef, Id, Revision},
        spaces::PERSONAL,
    },
    store::{BotConfig, Store},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::path::Path;

fn id(n: u32) -> Id {
    format!("00000000-0000-4000-8000-{n:012x}").try_into().unwrap()
}

fn context_json() -> Value {
    json!({"version":1,"hubId":id(10),"spaceId":PERSONAL,"botId":id(40),
        "conversationId":id(40),"audience":{"kind":"principal"},"teamId":null,
        "privacyDomainId":id(41),"domainRevision":1,"conversationRevision":1})
}

fn context() -> ContextRef {
    serde_json::from_value(context_json()).unwrap()
}

fn fixture() -> Store {
    let store = Store::open(Path::new(":memory:")).unwrap();
    // O mesmo nome não concede o papel padrão nem permissão a outro bot.
    for bot in [id(40), id(42)] {
        let cfg: BotConfig =
            serde_json::from_value(json!({"id":bot,"name":"Scatty","backend":"fixture","cwd":"/synthetic"})).unwrap();
        store.save_bot(&cfg).unwrap();
    }
    store
}

fn enabled(registry: &Registry<'_>, builtin: Builtin, n: u32) -> PluginInstallationRef {
    let initial = registry.register(&id(10), &id(n), builtin).unwrap();
    registry.change(&initial, LifecycleChange::Enable).unwrap()
}

fn native(installation: &PluginInstallationRef, n: u32) -> PluginResourceRef {
    PluginResourceRef::new(
        installation.clone(),
        PluginResourceOwner::Native { hub_id: id(10), space_id: PERSONAL.to_owned().try_into().unwrap() },
        id(n),
        PluginResourceKind::Calendar,
        Revision::try_from(1).unwrap(),
        Revision::try_from(1).unwrap(),
    )
    .unwrap()
}

fn external(
    registry: &Registry<'_>,
    installation: &PluginInstallationRef,
    account_id: u32,
    resource_id: u32,
    kind: PluginResourceKind,
) -> PluginResourceRef {
    let account_ref = registry.add_account(installation, &id(account_id)).unwrap();
    let resource = PluginResourceRef::new(
        installation.clone(),
        PluginResourceOwner::External { account_ref },
        id(resource_id),
        kind,
        Revision::try_from(1).unwrap(),
        Revision::try_from(1).unwrap(),
    )
    .unwrap();
    registry.add_resource(&resource).unwrap();
    resource
}

#[test]
fn migration_is_empty_idempotent_and_never_reseeds_removed_configuration() {
    let store = fixture();
    {
        let c = store.db.locked();
        // O DDL não muda a configuração global de FKs da Store.
        let fk: bool = c.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        schema::migrate(&c).unwrap();
        schema::migrate(&c).unwrap();
        assert_eq!(c.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, bool>(0)).unwrap(), fk);
        let count: i64 = c.query_row(
            "SELECT (SELECT count(*) FROM donna_plugin_installations)+(SELECT count(*) FROM donna_plugin_accounts)+(SELECT count(*) FROM donna_plugin_resources)+(SELECT count(*) FROM donna_plugin_grants)", [], |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 0);
        assert!(include_str!("schema.sql").contains(PERSONAL));
    }
    let registry = Registry::new(&store);
    registry.register(&id(10), &id(20), Builtin::Scatty).unwrap();
    store.db.locked().execute("DELETE FROM donna_plugin_installations", []).unwrap();
    schema::migrate(&store.db.locked()).unwrap();
    assert_eq!(registry.installation(&id(20)), Err(PluginError::NotFound));
    assert_eq!(BUILTINS.len(), 2);
    assert_eq!(BUILTINS[0].id, Builtin::Scatty);
    assert_eq!(BUILTINS[0].name, "Scatty");
    assert_eq!(BUILTINS[0].publisher, "Mexerica");
    assert_eq!(BUILTINS[0].origin, "bundle_proprio_donna");
    assert_eq!(BUILTINS[0].provenance, "wrapper_proprio_sem_importacao_de_auxiliar");
    assert_eq!(BUILTINS[0].distribution, "licencas_de_auxiliares_nao_avaliadas");
    assert_eq!(BUILTINS[0].version, 1);
    assert!(BUILTINS[0].capabilities.contains(&Capability::CalendarAvailability));
    assert_eq!(Builtin::parse("email"), Err(PluginError::InvalidDescriptor));
    assert_eq!(Builtin::parse("remote_script"), Err(PluginError::InvalidDescriptor));
}

#[test]
fn relation_constraints_reject_crossed_accounts_and_resources_with_fks_on_or_off() {
    let store = fixture();
    let registry = Registry::new(&store);
    let scatty = enabled(&registry, Builtin::Scatty, 20);
    let whatsapp = enabled(&registry, Builtin::Whatsapp, 21);
    registry.add_account(&whatsapp, &id(50)).unwrap();
    let calendar = native(&scatty, 30);
    registry.add_resource(&calendar).unwrap();
    for fk in [false, true] {
        let c = store.db.locked();
        c.pragma_update(None, "foreign_keys", fk).unwrap();
        let err = c.execute(
            "INSERT INTO donna_plugin_resources(id,installation_id,owner,account_id,kind,generation,revision) VALUES(?1,?2,'external',?3,'calendar',1,1)",
            params![id(31).as_str(), scatty.installation_id.as_str(), id(50).as_str()],
        ).unwrap_err();
        assert_eq!(db_error(err), PluginError::Constraint);
        let err = c.execute(
            "INSERT INTO donna_plugin_grants(id,installation_id,resource_id,bot_id,context,capability,effect,installation_generation,resource_generation,resource_revision,generation,revision,state) VALUES(?1,?2,?3,?4,?5,'calendar_read','read',2,1,1,1,1,'active')",
            params![id(60).as_str(), whatsapp.installation_id.as_str(), calendar.resource_id().as_str(), id(40).as_str(), context_json().to_string()],
        ).unwrap_err();
        assert_eq!(db_error(err), PluginError::Constraint);
        let err = c
            .execute(
                "INSERT INTO donna_plugin_accounts(id,installation_id,generation,revision) VALUES(?1,?2,1,1)",
                params![id(51).as_str(), id(99).as_str()],
            )
            .unwrap_err();
        assert_eq!(db_error(err), PluginError::Constraint);
        assert!(
            c.query_row("PRAGMA foreign_key_check(donna_plugin_resources)", [], |_| Ok(()))
                .optional()
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn explicit_capabilities_are_independent_and_private_contexts_fail_closed() {
    let store = fixture();
    let registry = Registry::new(&store);
    let installation = enabled(&registry, Builtin::Scatty, 20);
    let calendar = native(&installation, 30);
    registry.add_resource(&calendar).unwrap();
    registry.grant(&id(60), &calendar, &context(), Capability::CalendarAvailability).unwrap();
    assert_eq!(
        registry.configured_effect(&calendar, &context(), Capability::CalendarAvailability),
        Ok(Some(Effect::Read))
    );
    for capability in [Capability::CalendarRead, Capability::CalendarCreate, Capability::CalendarUpdate] {
        assert_eq!(registry.configured_effect(&calendar, &context(), capability), Ok(None));
    }
    registry.grant(&id(61), &calendar, &context(), Capability::CalendarCreate).unwrap();
    assert_eq!(
        registry.configured_effect(&calendar, &context(), Capability::CalendarCreate),
        Ok(Some(Effect::NativeWriteWithCard))
    );
    assert_eq!(registry.configured_effect(&calendar, &context(), Capability::CalendarUpdate), Ok(None));
    registry.grant(&id(62), &calendar, &context(), Capability::CalendarUpdate).unwrap();
    assert_eq!(
        registry.configured_effect(&calendar, &context(), Capability::CalendarUpdate),
        Ok(Some(Effect::NativeWriteWithCard))
    );
    assert_eq!(
        registry.grant(&id(63), &calendar, &context(), Capability::WhatsappReadChats),
        Err(PluginError::PolicyDenied)
    );
    for patch in [
        json!({"spaceId":id(90)}),
        json!({"hubId":id(90)}),
        json!({"teamId":id(90)}),
        json!({"audience":{"kind":"group","groupId":id(90)},"conversationId":id(90)}),
        json!({"audience":{"kind":"activity","activityId":id(90)},"conversationId":id(90)}),
    ] {
        let mut raw = context_json();
        for (k, v) in patch.as_object().unwrap() {
            raw[k] = v.clone();
        }
        let invalid: ContextRef = serde_json::from_value(raw).unwrap();
        assert_eq!(
            registry.grant(&id(64), &calendar, &invalid, Capability::CalendarRead),
            Err(PluginError::PolicyDenied)
        );
    }
    let mut other_bot = context_json();
    other_bot["botId"] = json!(id(42));
    other_bot["conversationId"] = json!(id(42));
    let other_bot = serde_json::from_value(other_bot).unwrap();
    assert_eq!(registry.configured_effect(&calendar, &other_bot, Capability::CalendarAvailability), Ok(None));
    // Um recurso novo do mesmo plugin/Space não herda o grant.
    let new_calendar = native(&installation, 31);
    registry.add_resource(&new_calendar).unwrap();
    assert_eq!(registry.configured_effect(&new_calendar, &context(), Capability::CalendarAvailability), Ok(None));
}

#[test]
fn external_calendar_policy_requires_cards_and_rejects_other_contexts() {
    let store = fixture();
    let registry = Registry::new(&store);
    let scatty = enabled(&registry, Builtin::Scatty, 20);
    let remote = external(&registry, &scatty, 51, 31, PluginResourceKind::Calendar);
    registry.grant(&id(63), &remote, &context(), Capability::CalendarRead).unwrap();
    assert_eq!(
        registry.configured_effect(&remote, &context(), Capability::CalendarRead),
        Ok(Some(Effect::ExternalReadWithCard))
    );
    assert_eq!(
        registry.grant(&id(64), &remote, &context(), Capability::CalendarCreate),
        Err(PluginError::PolicyDenied)
    );
    let mut wrong_space = context_json();
    wrong_space["spaceId"] = json!(id(90));
    assert_eq!(
        registry.grant(&id(64), &remote, &serde_json::from_value(wrong_space).unwrap(), Capability::CalendarRead),
        Err(PluginError::PolicyDenied)
    );
    let replacement = external(&registry, &scatty, 52, 32, PluginResourceKind::Calendar);
    assert_eq!(registry.configured_effect(&replacement, &context(), Capability::CalendarRead), Ok(None));
}

#[test]
fn stale_cas_and_descriptive_account_forgery_cannot_change_persisted_configuration() {
    let store = fixture();
    let registry = Registry::new(&store);
    let installation = enabled(&registry, Builtin::Scatty, 20);
    let mut stale = installation.clone();
    stale.revision = Revision::try_from(1).unwrap();
    assert_eq!(registry.change(&stale, LifecycleChange::Disable), Err(PluginError::SnapshotConflict));
    assert_eq!(registry.installation(&id(20)).unwrap(), installation);
    let mut account_ref = registry.add_account(&installation, &id(50)).unwrap();
    account_ref.generation = Revision::try_from(2).unwrap();
    let forged = PluginResourceRef::new(
        installation.clone(),
        PluginResourceOwner::External { account_ref },
        id(30),
        PluginResourceKind::Calendar,
        Revision::try_from(1).unwrap(),
        Revision::try_from(1).unwrap(),
    )
    .unwrap();
    assert_eq!(registry.add_resource(&forged), Err(PluginError::SnapshotConflict));
    let calendar = native(&installation, 31);
    registry.add_resource(&calendar).unwrap();
    let grant = registry.grant(&id(60), &calendar, &context(), Capability::CalendarRead).unwrap();
    let revoked = registry.revoke(&grant).unwrap();
    assert_eq!(u64::from(revoked.generation), 2);
    assert_eq!(u64::from(revoked.revision), 2);
    assert_eq!(registry.revoke(&grant), Err(PluginError::SnapshotConflict));
    assert_eq!(registry.configured_effect(&calendar, &context(), Capability::CalendarRead), Ok(None));
}

#[test]
fn disable_reenable_advances_generation_without_resurrecting_grants() {
    let store = fixture();
    let registry = Registry::new(&store);
    let installation = enabled(&registry, Builtin::Scatty, 20);
    let calendar = native(&installation, 30);
    registry.add_resource(&calendar).unwrap();
    registry.grant(&id(60), &calendar, &context(), Capability::CalendarRead).unwrap();
    let disabled = registry.change(&installation, LifecycleChange::Disable).unwrap();
    assert_eq!(u64::from(disabled.generation), 3);
    assert_eq!(
        registry.configured_effect(&calendar, &context(), Capability::CalendarRead),
        Err(PluginError::SnapshotConflict)
    );
    let reenabled = registry.change(&disabled, LifecycleChange::Enable).unwrap();
    assert_eq!(u64::from(reenabled.generation), 4);
    let refreshed = native(&reenabled, 30);
    assert_eq!(registry.configured_effect(&refreshed, &context(), Capability::CalendarRead), Ok(None));
    let c = store.db.locked();
    let row: (String, i64, i64) = c
        .query_row("SELECT state,generation,revision FROM donna_plugin_grants WHERE id=?1", [id(60).as_str()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!(row, ("revoked".into(), 2, 2));
}

#[test]
fn sqlite_failure_rolls_back_disable_and_bot_deletion_in_the_original_transaction() {
    let store = fixture();
    let registry = Registry::new(&store);
    let installation = enabled(&registry, Builtin::Scatty, 20);
    let calendar = native(&installation, 30);
    registry.add_resource(&calendar).unwrap();
    registry.grant(&id(60), &calendar, &context(), Capability::CalendarRead).unwrap();
    store.db.locked().execute_batch(
        "CREATE TEMP TRIGGER fail_plugin_revoke BEFORE UPDATE ON donna_plugin_grants BEGIN SELECT RAISE(ABORT,'synthetic_private_error'); END;",
    ).unwrap();
    let error = registry.change(&installation, LifecycleChange::Disable).unwrap_err();
    assert_eq!(error, PluginError::Constraint);
    assert_eq!(error.to_string(), "plugin_constraint");
    assert_eq!(registry.installation(&id(20)).unwrap(), installation);
    assert_eq!(registry.configured_effect(&calendar, &context(), Capability::CalendarRead), Ok(Some(Effect::Read)));
    assert!(store.delete_bot(id(40).as_str()).is_err());
    assert!(!store.bot(id(40).as_str()).unwrap().unwrap().deleted);
    assert_eq!(registry.configured_effect(&calendar, &context(), Capability::CalendarRead), Ok(Some(Effect::Read)));
    store.db.locked().execute_batch("DROP TRIGGER fail_plugin_revoke").unwrap();
    store.delete_bot(id(40).as_str()).unwrap();
    assert_eq!(
        registry.configured_effect(&calendar, &context(), Capability::CalendarRead),
        Err(PluginError::PolicyDenied)
    );
    let row: (String, i64) = store
        .db
        .locked()
        .query_row("SELECT state,generation FROM donna_plugin_grants WHERE id=?1", [id(60).as_str()], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(row, ("revoked".into(), 2));
    assert_eq!(
        registry.grant(&id(61), &calendar, &context(), Capability::CalendarRead),
        Err(PluginError::PolicyDenied)
    );
}

#[test]
fn persisted_bot_space_or_kind_change_invalidates_configuration_without_issuing_scope() {
    let store = fixture();
    let registry = Registry::new(&store);
    let installation = enabled(&registry, Builtin::Scatty, 20);
    let calendar = native(&installation, 30);
    registry.add_resource(&calendar).unwrap();
    registry.grant(&id(60), &calendar, &context(), Capability::CalendarRead).unwrap();
    for patch in [json!({"spaceId":id(90)}), json!({"spaceId":PERSONAL,"kind":"group"})] {
        // Simula alteração concorrente futura/estado inconsistente, fora da API.
        store
            .db
            .locked()
            .execute(
                "UPDATE bots SET config=json_patch(config,?1) WHERE id=?2",
                params![patch.to_string(), id(40).as_str()],
            )
            .unwrap();
        assert_eq!(
            registry.configured_effect(&calendar, &context(), Capability::CalendarRead),
            Err(PluginError::PolicyDenied)
        );
        assert_eq!(
            registry.grant(&id(61), &calendar, &context(), Capability::CalendarUpdate),
            Err(PluginError::PolicyDenied)
        );
    }
}

#[test]
fn old_schema_with_agenda_boots_keeps_rows_and_trigger() {
    let dir = std::env::temp_dir().join(format!("donna-plugins-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hub.db");
    // Antes do R1 (sem 'whatsapp') e do R3 (sem número nem capacidades de WhatsApp).
    let old = include_str!("schema.sql")
        .replace("'scatty','email','whatsapp'", "'scatty','email'")
        .replace("'calendar','mailbox','whatsapp_number'", "'calendar','mailbox'")
        .replace(",'whatsapp_read_chats','whatsapp_read_groups','whatsapp_reply_chats','whatsapp_reply_groups')", ")")
        .replace(",'external_write_with_card')", ")")
        .replace("\n       OR (capability IN ('whatsapp_read_chats','whatsapp_read_groups') AND effect='read')", "")
        .replace("\n       OR (capability IN ('whatsapp_reply_chats','whatsapp_reply_groups') AND effect='external_write_with_card')", "");
    assert!(old.contains("CHECK(kind IN ('calendar','mailbox'))") && !old.contains("AND effect='read')),"));
    let events_parent = |store: &Store| -> bool {
        let sql =
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='trigger' AND name='donna_scatty_events_parent')";
        store.db.locked().query_row(sql, [], |r| r.get(0)).unwrap()
    };
    let event = |store: &Store, event: u32, generation: i64| {
        store.db.locked().execute(
            "INSERT INTO donna_scatty_events(id,installation_id,resource_id,resource_generation,revision,timing,starts_at,ends_at,time_zone,busy)
             VALUES(?1,?2,?3,?4,1,'timed',1,2,'UTC',1)",
            params![id(event).as_str(), id(20).as_str(), id(30).as_str(), generation],
        )
    };
    {
        // Banco com F1b + F2d (Agenda) e o CHECK antigo, sem 'whatsapp'.
        let store = Store::open(&path).unwrap();
        let c = store.db.locked();
        c.execute_batch(
            "DROP TABLE donna_plugin_grants; DROP TABLE donna_plugin_resources; DROP TABLE donna_plugin_installations;",
        )
        .unwrap();
        c.execute_batch(&old).unwrap();
        c.execute(
            "INSERT INTO donna_plugin_installations(id,version,hub_id,builtin,state,generation,revision) VALUES(?1,1,?2,'scatty','enabled',3,4)",
            params![id(20).as_str(), id(10).as_str()],
        )
        .unwrap();
        c.execute(
            "INSERT INTO donna_plugin_resources(id,installation_id,owner,hub_id,space_id,kind,generation,revision) VALUES(?1,?2,'native',?3,?4,'calendar',1,1)",
            params![id(30).as_str(), id(20).as_str(), id(10).as_str(), PERSONAL],
        )
        .unwrap();
        let whatsapp = c.execute(
            "INSERT INTO donna_plugin_installations(id,version,hub_id,builtin,state,generation,revision) VALUES(?1,1,?2,'whatsapp','disabled',1,1)",
            params![id(21).as_str(), id(10).as_str()],
        );
        assert!(whatsapp.is_err());
        drop(c);
        assert!(events_parent(&store));
    }
    // (a) O Hub sobe de novo: a migração completa reconstrói a tabela e preserva as linhas.
    let store = Store::open(&path).unwrap();
    let registry = Registry::new(&store);
    assert_eq!(registry.installation(&id(20)).unwrap().generation, Revision::try_from(3_u64).unwrap());
    let whatsapp = registry.register(&id(10), &id(21), Builtin::Whatsapp).unwrap();
    assert_eq!(Builtin::parse("whatsapp"), Ok(Builtin::Whatsapp));
    // (b) O gatilho da Agenda volta e continua barrando o que barrava.
    assert!(events_parent(&store));
    assert_eq!(db_error(event(&store, 60, 2).unwrap_err()), PluginError::Constraint);
    event(&store, 61, 1).unwrap();
    // Os gatilhos do registro também voltaram: conta só de instalação existente.
    let err = store.db.locked().execute(
        "INSERT INTO donna_plugin_accounts(id,installation_id,generation,revision) VALUES(?1,?2,1,1)",
        params![id(50).as_str(), id(99).as_str()],
    );
    assert_eq!(db_error(err.unwrap_err()), PluginError::Constraint);
    let on = registry.change(&whatsapp, LifecycleChange::Enable).unwrap();
    assert_eq!(u64::from(on.generation), u64::from(whatsapp.generation) + 1);
    // (c) R3: recursos e grants reconstruídos aceitam o número do WhatsApp; a linha da Agenda fica.
    {
        let c = store.db.locked();
        assert!(super::whatsapp_grants::link(&c, &id(10), id(70).as_str()).unwrap());
        let count = |sql: &str| -> i64 { c.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(count("SELECT COUNT(*) FROM donna_plugin_resources WHERE kind='whatsapp_number'"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM donna_plugin_resources WHERE kind='calendar'"), 1);
        let sql =
            "SELECT COUNT(*) FROM sqlite_master WHERE name='donna_plugin_grants' AND sql LIKE '%whatsapp_reply_chats%'";
        assert_eq!(count(sql), 1);
        assert_eq!(count("SELECT COUNT(*) FROM sqlite_master WHERE name='donna_plugin_grants_active'"), 1);
    }
    let fk: bool = store.db.locked().query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
    let default: bool =
        rusqlite::Connection::open_in_memory().unwrap().query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
    assert_eq!(fk, default, "a migração restaura o PRAGMA");
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn email_removido_nao_migra_nem_apaga_dados_legados_no_boot() {
    let dir = std::env::temp_dir().join(format!("d537-email-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hub.db");
    {
        let store = Store::open(&path).unwrap();
        let cfg: BotConfig = serde_json::from_value(json!({
            "id":id(40),"name":"fixture","backend":"fixture","cwd":"/synthetic"
        }))
        .unwrap();
        store.save_bot(&cfg).unwrap();
        let c = store.db.locked();
        let count: i64 = c
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name IN ('donna_email_schema','donna_email_draft_versions')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        // Simula instalação anterior; versão desconhecida também não deve ser validada pelo boot.
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS donna_email_schema(
           id INTEGER PRIMARY KEY CHECK(id=1), version INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS donna_email_draft_versions(
           installation BLOB NOT NULL CHECK(length(installation)=16),
           account BLOB NOT NULL CHECK(length(account)=16),
           incarnation BLOB NOT NULL CHECK(length(incarnation)=16),
           principal BLOB NOT NULL CHECK(length(principal)=16),
           audience BLOB NOT NULL CHECK(length(audience)=16),
           object BLOB NOT NULL CHECK(length(object)=16),
           revision INTEGER NOT NULL CHECK(revision>0),
           intent BLOB NOT NULL CHECK(length(intent)=16),
           expected INTEGER NOT NULL CHECK(expected>=0 AND expected=revision-1),
           sealed BLOB NOT NULL CHECK(length(sealed)>=41 AND length(sealed)<=26214441),
           PRIMARY KEY(installation,account,incarnation,object,revision),
           UNIQUE(installation,account,incarnation,principal,audience,intent)
         ) WITHOUT ROWID;
             INSERT INTO donna_email_schema(id,version) VALUES(1,99);
             INSERT INTO donna_email_draft_versions VALUES(
               zeroblob(16),zeroblob(16),zeroblob(16),zeroblob(16),zeroblob(16),
               zeroblob(16),1,zeroblob(16),0,zeroblob(41));",
        )
        .unwrap();
        c.execute(
            "INSERT INTO donna_plugin_installations(id,version,hub_id,builtin,state,generation,revision)
             VALUES(?1,1,?2,'email','enabled',2,2)",
            params![id(21).as_str(), id(10).as_str()],
        )
        .unwrap();
        c.execute(
            "INSERT INTO donna_plugin_accounts(id,installation_id,generation,revision) VALUES(?1,?2,1,1)",
            params![id(50).as_str(), id(21).as_str()],
        )
        .unwrap();
        c.execute(
            "INSERT INTO donna_plugin_resources(id,installation_id,owner,account_id,kind,generation,revision)
             VALUES(?1,?2,'external',?3,'mailbox',1,1)",
            params![id(30).as_str(), id(21).as_str(), id(50).as_str()],
        )
        .unwrap();
        for (grant, capability, effect) in [(60, "mailbox_read", "read"), (61, "mailbox_draft_local", "local_draft")] {
            c.execute(
                "INSERT INTO donna_plugin_grants(id,installation_id,resource_id,bot_id,context,capability,effect,
                 installation_generation,resource_generation,resource_revision,account_generation,account_revision,
                 generation,revision,state) VALUES(?1,?2,?3,?4,?5,?6,?7,2,1,1,1,1,1,1,'active')",
                params![
                    id(grant).as_str(),
                    id(21).as_str(),
                    id(30).as_str(),
                    id(40).as_str(),
                    context_json().to_string(),
                    capability,
                    effect
                ],
            )
            .unwrap();
        }
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(Registry::new(&store).installation(&id(21)), Err(PluginError::InvalidDescriptor));
    {
        let c = store.db.locked();
        assert_eq!(
            c.query_row("SELECT version FROM donna_email_schema WHERE id=1", [], |r| r.get::<_, i64>(0)).unwrap(),
            99
        );
        assert_eq!(
            c.query_row("SELECT sealed FROM donna_email_draft_versions", [], |r| r.get::<_, Vec<u8>>(0)).unwrap(),
            vec![0; 41]
        );
        let grants: Vec<(String, String, String)> = c
            .prepare("SELECT capability,effect,state FROM donna_plugin_grants ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            grants,
            vec![
                ("mailbox_read".into(), "read".into(), "active".into()),
                ("mailbox_draft_local".into(), "local_draft".into(), "active".into())
            ]
        );
        for table in ["donna_plugin_installations", "donna_plugin_accounts", "donna_plugin_resources"] {
            assert_eq!(c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        }
    }
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

/// D544 mantém o banco antigo intacto e fecha as três RPCs, inclusive para clientes antigos.
#[tokio::test]
async fn d544_preserva_legado_sem_api_de_plugin() {
    use crate::{
        api::{devices::Caller, dispatch},
        hub::Hub,
        remote::identity::Identity,
    };
    fn snapshot(store: &Store) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
        let db = store.db.locked();
        ["donna_plugin_installations", "donna_plugin_resources", "donna_plugin_grants", "donna_scatty_events"]
            .iter()
            .map(|table| {
                let mut statement = db.prepare(&format!("SELECT * FROM {table} ORDER BY id")).unwrap();
                let columns = statement.column_count();
                statement
                    .query_map([], |row| (0..columns).map(|i| row.get(i)).collect::<rusqlite::Result<Vec<_>>>())
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap()
            })
            .collect()
    }
    let store = fixture();
    {
        let registry = Registry::new(&store);
        let installed = enabled(&registry, Builtin::Scatty, 20);
        let calendar = native(&installed, 30);
        registry.add_resource(&calendar).unwrap();
        registry.grant(&id(60), &calendar, &context(), Capability::CalendarRead).unwrap();
        store.db.locked().execute("INSERT INTO donna_scatty_events(id,installation_id,resource_id,resource_generation,revision,timing,starts_at,ends_at,time_zone,busy) VALUES(?1,?2,?3,1,1,'timed',1,2,'UTC',1)", params![id(70).as_str(),id(20).as_str(),id(30).as_str()]).unwrap();
    }
    let before = snapshot(&store);
    super::migrate(&store.db.locked()).unwrap();
    super::migrate(&store.db.locked()).unwrap();
    assert_eq!(snapshot(&store), before);
    let dir = std::env::temp_dir().join(format!("d544-legacy-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(store, id(10).as_str().into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
    for method in ["donnaAgenda", "donnaAgendaSave", "donnaAgendaRead"] {
        let error =
            dispatch(&hub, &Caller::Local, method, json!({"enabled":true,"donnaCanRead":true})).await.unwrap_err();
        assert_eq!(error.to_string(), "unknown method", "{method}");
        assert_eq!(snapshot(&hub.store), before);
    }
    std::fs::remove_dir_all(dir).unwrap();
}
