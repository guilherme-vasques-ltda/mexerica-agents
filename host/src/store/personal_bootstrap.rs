//! Origem durável local, na mesma Connection/tx de A1 e ACS4. Não concede acesso.
use super::Store;
use crate::LockExt;
use crate::fork::personal_bootstrap::provisioning::AdmittedConfig;
use anyhow::{Result, ensure};
#[cfg(any(test, not(target_os = "linux")))]
use rusqlite::OpenFlags;
use rusqlite::{Connection, OptionalExtension, params};
#[cfg(any(test, not(target_os = "linux")))]
use std::path::Path;

/// Só a criação exclusiva permite registrar nascimento local; ausência lida antes não basta.
#[cfg(any(test, not(target_os = "linux")))]
pub(super) fn create_database(path: &Path) -> Result<bool> {
    if path == Path::new(":memory:") {
        return Ok(true);
    }
    #[cfg(unix)]
    let opened = (|| -> Result<std::fs::File> {
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let dir = crate::fork::private_fs::Directory::open(parent, false)?;
        dir.file(
            path.file_name().ok_or_else(|| anyhow::anyhow!("database name"))?,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )
    })();
    #[cfg(not(unix))]
    let opened: Result<std::fs::File> =
        std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(Into::into);
    match opened {
        Ok(_) => Ok(true),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::AlreadyExists) =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}
pub(super) fn migrate(c: &Connection, new_database: bool) -> Result<()> {
    let tx = c.unchecked_transaction()?;
    tx.execute_batch(include_str!("personal_bootstrap.sql"))?;
    super::personal_link::migrate(&tx)?;
    super::personal_session::migrate(&tx)?;
    super::personal_prepare::migrate(&tx)?;
    crate::fork::personal_bootstrap::live::migrate(&tx)?;
    if new_database {
        tx.execute("INSERT INTO personal_local_birth(singleton,schema) SELECT 1,1 WHERE NOT EXISTS(SELECT 1 FROM personal_local_birth)",[])?;
    }
    tx.execute("INSERT INTO first_owner(singleton,origin,ever_claimed)
        SELECT 1,'acs4',CASE WHEN ever_claimed=1 OR EXISTS(SELECT 1 FROM bootstrap_slot) OR EXISTS(SELECT 1 FROM devices) THEN 1 ELSE 0 END
        FROM bootstrap_generation WHERE NOT EXISTS(SELECT 1 FROM first_owner)", [])?;
    tx.execute(
        "INSERT INTO first_owner(singleton,origin,ever_claimed) SELECT 1,'legacy',1
        WHERE EXISTS(SELECT 1 FROM devices) AND NOT EXISTS(SELECT 1 FROM first_owner)",
        [],
    )?;
    // Sem evidência não há backfill: ausência de linha não certifica instalação virgem.
    tx.commit()?;
    if c.query_row(
        "SELECT EXISTS(SELECT 1 FROM personal_generation) OR EXISTS(SELECT 1 FROM personal_link_origins) OR EXISTS(SELECT 1 FROM personal_link_installation)",
        [],
        |r| r.get::<_, bool>(0),
    )? {
        c.pragma_update(None, "synchronous", "FULL")?;
    }
    Ok(())
}
#[derive(Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Cinco fatos independentes da leitura podem coexistir e devem preservar combinações inconsistentes para recusa"
)]
pub(crate) struct Evidence {
    pub personal: bool,
    pub acs4: bool,
    pub prior_devices: bool,
    pub config: Option<Vec<u8>>,
    pub inconsistent: bool,
    pub tracked_birth: bool,
    pub binding: Option<(String, String, String, String, String)>,
    ledger: Vec<String>,
}
fn table(c: &Connection, name: &str) -> Result<bool> {
    Ok(c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)", [name], |r| r.get(0))?)
}
/// Somente SELECT/read-only, antes de `Store::open`/migração e antes de gerar H.
#[cfg(any(test, not(target_os = "linux")))]
pub(crate) fn preflight(path: &Path) -> Result<Evidence> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Evidence::default()),
        Err(e) => return Err(e.into()),
        Ok(m) => ensure!(m.is_file() && !m.file_type().is_symlink(), "personal database must be regular"),
    }
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    evidence(&c)
}
/// A mesma leitura também é usada pelo handoff da Connection existente, sem migração.
pub(super) fn evidence(c: &Connection) -> Result<Evidence> {
    let tx = c.unchecked_transaction()?;
    let c = &tx;
    let mut out = Evidence::default();
    for (t, dest) in [
        ("personal_generation", &mut out.personal),
        ("bootstrap_generation", &mut out.acs4),
        ("devices", &mut out.prior_devices),
    ] {
        if table(c, t)? {
            *dest = c.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {t})"), [], |r| r.get(0))?;
        }
    }
    if table(c, "personal_generation")? {
        out.config =
            c.query_row("SELECT config FROM personal_generation WHERE singleton=1", [], |r| r.get(0)).optional()?;
        out.binding = c
            .query_row(
                "SELECT generation,context,computer,sign_key,box_key FROM personal_generation WHERE singleton=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;
    }
    if table(c, "first_owner")? {
        out.prior_devices |=
            c.query_row("SELECT EXISTS(SELECT 1 FROM first_owner WHERE origin<>'personal')", [], |r| {
                r.get::<_, bool>(0)
            })?;
        out.personal |= c.query_row("SELECT EXISTS(SELECT 1 FROM first_owner WHERE origin='personal')", [], |r| {
            r.get::<_, bool>(0)
        })?;
    }
    if table(c, "personal_device_origins")? {
        out.personal |=
            c.query_row("SELECT EXISTS(SELECT 1 FROM personal_device_origins)", [], |r| r.get::<_, bool>(0))?;
    }
    if table(c, "bootstrap_slot")? {
        out.acs4 |= c.query_row("SELECT EXISTS(SELECT 1 FROM bootstrap_slot)", [], |r| r.get::<_, bool>(0))?;
    }
    if table(c, "personal_local_birth")? {
        out.tracked_birth =
            c.query_row("SELECT EXISTS(SELECT 1 FROM personal_local_birth WHERE singleton=1 AND schema=1)", [], |r| {
                r.get(0)
            })?;
    }
    // Não converter D430 em instalação virgem ou geração PL1 no preflight antigo.
    let d430 = (table(c, "personal_link_origins")?
        && c.query_row("SELECT EXISTS(SELECT 1 FROM personal_link_origins)", [], |r| r.get::<_, bool>(0))?)
        || (table(c, "personal_link_installation")?
            && c.query_row("SELECT EXISTS(SELECT 1 FROM personal_link_installation)", [], |r| r.get::<_, bool>(0))?);
    out.personal |= d430;
    let personal_owner = table(c, "first_owner")?
        && c.query_row("SELECT EXISTS(SELECT 1 FROM first_owner WHERE origin='personal')", [], |r| {
            r.get::<_, bool>(0)
        })?;
    out.inconsistent = d430 || (out.personal && (out.config.is_none() || !personal_owner));
    for (name, columns, order) in [
        (
            "personal_generation",
            "singleton,hex(config),generation,context,computer,sign_key,box_key,ever_claimed,tombstone,revision",
            "singleton",
        ),
        ("first_owner", "singleton,origin,ever_claimed,a1_input,a1_signature", "singleton"),
        ("personal_device_origins", "key,generation,grant_id", "key"),
        ("personal_link_origins", "key,hex(binding),tombstone", "key"),
        ("personal_link_installation", "install_id,hub_key,hex(birth)", "singleton"),
        ("personal_link_journal", "seq,kind,hex(bytes)", "seq"),
    ] {
        if table(c, name)? {
            let mut stmt = c.prepare(&format!("SELECT json_array({columns}) FROM {name} ORDER BY {order}"))?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            for row in rows {
                out.ledger.push(format!("{name}:{}", row?));
            }
        }
    }
    tx.commit()?;
    Ok(out)
}
pub(super) fn legacy_a1(
    c: &Connection,
    input: &str,
    sign: impl FnOnce(&Connection) -> Result<String>,
) -> Result<String> {
    ensure!(!c.query_row("SELECT EXISTS(SELECT 1 FROM personal_link_origins) OR EXISTS(SELECT 1 FROM personal_generation) OR EXISTS(SELECT 1 FROM personal_device_origins) OR EXISTS(SELECT 1 FROM bootstrap_generation) OR EXISTS(SELECT 1 FROM bootstrap_slot)",[],|r|r.get::<_,bool>(0))?,"configured installation excludes legacy A1");
    let old: Option<(String, Option<String>, Option<String>)> = c
        .query_row("SELECT origin,a1_input,a1_signature FROM first_owner WHERE singleton=1", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    if let Some((origin, previous, signature)) = old {
        ensure!(origin == "a1" || origin == "legacy", "first ownership profile conflict");
        if previous.is_some() {
            let signature = signature.ok_or_else(|| anyhow::anyhow!("A1 reservation incomplete"))?;
            if previous.as_deref() == Some(input) {
                return Ok(signature);
            }
            // Sem ACS4, novo claim/nonce Local pode substituir uma intenção expirada.
            // Conserva a primeira evidência e a origem; assina ainda sob a mesma tx.
            return sign(c);
        }
        ensure!(origin == "legacy", "A1 reservation incomplete");
        ensure!(
            c.execute("UPDATE first_owner SET a1_input=? WHERE singleton=1 AND a1_input IS NULL", [input])? == 1,
            "A1 claim race"
        );
        let signature = sign(c)?;
        ensure!(
            c.execute(
                "UPDATE first_owner SET a1_signature=? WHERE singleton=1 AND a1_input=? AND a1_signature IS NULL",
                params![signature, input]
            )? == 1,
            "A1 signature race"
        );
        return Ok(signature);
    }
    c.execute("INSERT INTO first_owner(singleton,origin,ever_claimed,a1_input) VALUES(1,'a1',1,?)", [input])?;
    let signature = sign(c)?;
    ensure!(
        c.execute(
            "UPDATE first_owner SET a1_signature=? WHERE singleton=1 AND a1_input=? AND a1_signature IS NULL",
            params![signature, input]
        )? == 1,
        "A1 signature race"
    );
    Ok(signature)
}
/// Mesmo predicado de `Store::personal_origin`, só na Connection recebida (chamador já sob a trava DB).
pub(crate) fn personal_origin_on(c: &Connection, key: &str) -> Result<Option<String>> {
    if let Some(generation) = super::personal_link::origin_on(c, key)? {
        return Ok(Some(generation));
    }
    Ok(c.query_row("SELECT generation FROM personal_generation UNION SELECT generation FROM personal_device_origins WHERE key=? LIMIT 1",[key],|r|r.get(0)).optional()?)
}
impl Store {
    pub(crate) fn personal_install_generation(&self, cfg: &AdmittedConfig) -> Result<()> {
        let mut db = self.db.locked();
        db.pragma_update(None, "synchronous", "FULL")?;
        let tx = db.transaction()?;
        let old: Option<Vec<u8>> =
            tx.query_row("SELECT config FROM personal_generation WHERE singleton=1", [], |r| r.get(0)).optional()?;
        if let Some(old) = old {
            ensure!(old == cfg.original_bytes(), "personal generation differs");
            let i = cfg.identity();
            ensure!(tx.query_row("SELECT EXISTS(SELECT 1 FROM personal_generation p JOIN first_owner f ON f.singleton=p.singleton WHERE f.origin='personal' AND p.generation=? AND p.context=? AND p.computer=? AND p.sign_key=? AND p.box_key=?)",params![i.generation,serde_json::to_string(cfg.context())?,i.computer_id,i.sign_key,i.box_key],|r|r.get::<_,bool>(0))?,"personal binding differs");
            tx.commit()?;
            return Ok(());
        }
        ensure!(!tx.query_row("SELECT EXISTS(SELECT 1 FROM first_owner) OR EXISTS(SELECT 1 FROM devices) OR EXISTS(SELECT 1 FROM bootstrap_generation) OR EXISTS(SELECT 1 FROM bootstrap_slot)",[],|r|r.get::<_,bool>(0))?,"existing ownership requires separate migration");
        ensure!(
            tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM personal_local_birth WHERE singleton=1 AND schema=1)",
                [],
                |r| r.get::<_, bool>(0)
            )?,
            "untracked installation requires separate migration"
        );
        let i = cfg.identity();
        tx.execute("INSERT INTO personal_generation(singleton,config,generation,context,computer,sign_key,box_key) VALUES(1,?,?,?,?,?,?)",
            params![cfg.original_bytes(),i.generation,serde_json::to_string(cfg.context())?,i.computer_id,i.sign_key,i.box_key])?;
        tx.commit()?;
        Ok(())
    }
    /// A instalação pessoal cerca inclusive Devices Local pareados depois e chaves apagadas.
    pub(crate) fn personal_origin(&self, key: &str) -> Result<Option<String>> {
        personal_origin_on(&self.db.locked(), key)
    }
    pub(crate) fn personal_configured(&self) -> Result<bool> {
        Ok(self.db.locked().query_row("SELECT EXISTS(SELECT 1 FROM personal_link_origins) OR EXISTS(SELECT 1 FROM personal_generation) OR EXISTS(SELECT 1 FROM first_owner WHERE origin='personal') OR EXISTS(SELECT 1 FROM personal_device_origins)",[],|r|r.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::crypto;
    use crate::store::{Device, DeviceSource, Scope};
    use std::sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    };
    fn acs4() -> crate::store::bootstrap::TrustedGeneration {
        crate::store::bootstrap::TrustedGeneration {
            generation: crypto::b64(&[51; 32]),
            issuer_key_id: "operator_fixture".into(),
            issuer_key: crypto::b64(&[52; 32]),
            issuer: "https://issuer.invalid".into(),
            deployment_id: "fixture".into(),
            env: "dev".into(),
            manifest_hash: "a".repeat(64),
            resource_digest: "b".repeat(64),
            computer_id: crypto::computer_id(&[53; 32]),
            sign_key: crypto::b64(&[53; 32]),
            box_key: crypto::b64(&[54; 32]),
        }
    }
    /// HOST-R1-03: prova a escolha FULL e a ordem (antes da reserva, na MESMA Connection do callback), não simula
    /// queda de energia. A falha de assinatura desfaz a reserva.
    #[test]
    fn legacy_a1_reserves_first_owner_under_full_on_the_same_connection() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("donna-a1-full-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir)?;
        let store = Store::open(&dir.join("codync.db"))?;
        let mode = |c: &Connection| c.pragma_query_value(None, "synchronous", |r| r.get::<_, i64>(0));
        assert_eq!(mode(&store.db.locked())?, 1, "NORMAL antes da primeira A1");
        let failed = store.bootstrap_claim_a1(None, "input-a", |c| {
            assert_eq!(mode(c)?, 2);
            anyhow::bail!("sign failed")
        });
        assert!(failed.is_err());
        let owners = || -> Result<i64> {
            Ok(store.db.locked().query_row("SELECT count(*) FROM first_owner", [], |r| r.get(0))?)
        };
        assert_eq!(owners()?, 0, "rollback sem reserva");
        let signature = store.bootstrap_claim_a1(None, "input-a", |c| {
            assert_eq!(mode(c)?, 2, "FULL na mesma Connection antes do ACK");
            assert_eq!(c.query_row("SELECT count(*) FROM first_owner", [], |r| r.get::<_, i64>(0))?, 1);
            Ok("signature-a".into())
        })?;
        assert_eq!(signature.as_deref(), Some("signature-a"));
        assert_eq!(owners()?, 1);
        drop(store);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
    #[test]
    fn recursive_triggers_production_store_preserves_claimed_acs4_on_replace() -> Result<()> {
        let store = Store::open(Path::new(":memory:"))?;
        assert_eq!(store.db.locked().pragma_query_value(None, "recursive_triggers", |r| r.get::<_, i64>(0))?, 1);
        let (_, h) = crate::fork::personal_bootstrap::provisioning::fixture()?;
        let mut generation = acs4();
        generation.computer_id = h.computer_id();
        generation.sign_key = h.sign_pub_b64();
        generation.box_key = h.box_pub_b64();
        store.bootstrap_install_generation(&generation)?;
        let input =
            crypto::claim_input("claim_fixture", "nonce_fixture", "owner_fixture", &h.computer_id(), &h.box_pub_b64());
        let signature = store
            .bootstrap_claim_a1(Some(&generation.generation), &input, |_| Ok(crypto::b64(&h.sign(input.as_bytes()))))?
            .ok_or_else(|| anyhow::anyhow!("fixture signature missing"))?;
        crypto::verify(&h.sign_pub(), input.as_bytes(), &crypto::unb64_n::<64>(&signature)?)?;
        let before = {
            let c = store.db.locked();
            let before:String=c.query_row("SELECT json_array(config,ever_claimed,revision,acl_version,a1_input,a1_signature) FROM bootstrap_generation",[],|r|r.get(0))?;
            assert!(
                c.execute(
                    "INSERT OR REPLACE INTO bootstrap_generation(singleton,config,ever_claimed) VALUES(1,?,0)",
                    [serde_json::to_string(&generation)?]
                )
                .is_err()
            );
            assert_eq!(c.query_row("SELECT json_array(config,ever_claimed,revision,acl_version,a1_input,a1_signature) FROM bootstrap_generation",[],|r|r.get::<_,String>(0))?,before);
            assert_eq!(
                c.query_row("SELECT origin,ever_claimed FROM first_owner", [], |r| Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u32>(1)?
                )))?,
                ("acs4".into(), 1)
            );
            before
        };
        assert!(
            store
                .bootstrap_claim_a1(Some(&generation.generation), "another-claim", |_| panic!(
                    "claimed ACS4 must not sign again"
                ))
                .is_err()
        );
        assert_eq!(
            store.bootstrap_claim_a1(Some(&generation.generation), &input, |_| panic!(
                "same claim must use retained signature"
            ))?,
            Some(signature)
        );
        assert_eq!(store.db.locked().query_row("SELECT json_array(config,ever_claimed,revision,acl_version,a1_input,a1_signature) FROM bootstrap_generation",[],|r|r.get::<_,String>(0))?,before);
        Ok(())
    }
    #[test]
    fn personal_ledger_survives_device_delete_restart_and_rejects_both_legacy_ingresses() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("personal-ledger-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir)?;
        let path = dir.join("db");
        let s = Store::open(&path)?;
        let (cfg, _) = crate::fork::personal_bootstrap::provisioning::fixture()?;
        s.personal_install_generation(&cfg)?;
        s.personal_install_generation(&cfg)?;
        let signed = AtomicUsize::new(0);
        assert!(
            s.bootstrap_claim_a1(None, "claim", |_| {
                signed.fetch_add(1, Ordering::SeqCst);
                Ok("signature".into())
            })
            .is_err()
        );
        assert_eq!(signed.load(Ordering::SeqCst), 0);
        assert!(s.bootstrap_install_generation(&acs4()).is_err());
        {
            let c = s.db.locked();
            assert!(c.execute("INSERT INTO bootstrap_generation(singleton,config) VALUES(1,'{}')", []).is_err());
            assert!(
                c.execute(
                    "INSERT INTO bootstrap_slot(singleton,grant_wire,grant_hash,status) VALUES(1,x'01','x','active')",
                    []
                )
                .is_err()
            );
            c.execute("UPDATE personal_generation SET ever_claimed=1,tombstone=1,revision=2", [])?;
            assert!(c.execute("UPDATE personal_generation SET ever_claimed=0,tombstone=0", []).is_err());
            assert!(c.execute("DELETE FROM personal_generation", []).is_err());
            assert!(c.execute("DELETE FROM first_owner", []).is_err());
            assert!(
                c.execute(
                    "INSERT OR REPLACE INTO first_owner(singleton,origin,ever_claimed) VALUES(1,'personal',0)",
                    []
                )
                .is_err()
            );
        }
        let key = crypto::b64(&[91; 32]);
        s.put_device(&Device {
            key: key.clone(),
            name: "fixture".into(),
            platform: "fixture".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: 1,
            last_seen_at: None,
        })?;
        assert_eq!(s.personal_origin(&key)?.as_deref(), Some(cfg.identity().generation.as_str()));
        assert!(
            s.db.locked()
                .execute(
                    "INSERT OR REPLACE INTO personal_device_origins(key,generation,grant_id) VALUES(?,?,NULL)",
                    params![key, cfg.identity().generation]
                )
                .is_err()
        );
        s.remove_device(&key)?;
        drop(s);
        let s = Store::open(&path)?;
        assert!(s.device(&key).is_none());
        assert!(s.personal_origin(&key)?.is_some());
        assert!(s.bootstrap_claim_a1(None, "claim", |_| panic!("personal loser must not sign")).is_err());
        assert_eq!(s.db.locked().query_row("SELECT ever_claimed FROM first_owner", [], |r| r.get::<_, u32>(0))?, 1);
        assert_eq!(s.db.locked().pragma_query_value(None, "synchronous", |r| r.get::<_, i64>(0))?, 2);
        drop(s);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
    #[test]
    fn a1_and_acs4_reservations_block_personal_and_a1_callback_uses_the_reserved_connection() -> Result<()> {
        let (cfg, _) = crate::fork::personal_bootstrap::provisioning::fixture()?;
        let a1 = Store::open(Path::new(":memory:"))?;
        assert_eq!(
            a1.bootstrap_claim_a1(None, "original", |c| {
                assert_eq!(
                    c.query_row("SELECT a1_input FROM first_owner WHERE origin='a1'", [], |r| r.get::<_, String>(0))?,
                    "original"
                );
                Ok("signature".into())
            })?
            .as_deref(),
            Some("signature")
        );
        assert_eq!(
            a1.bootstrap_claim_a1(None, "original", |_| panic!("idempotence must not sign"))?.as_deref(),
            Some("signature")
        );
        assert_eq!(
            a1.bootstrap_claim_a1(None, "other", |c| {
                assert_eq!(
                    c.query_row("SELECT a1_input FROM first_owner WHERE origin='a1'", [], |r| r.get::<_, String>(0))?,
                    "original"
                );
                Ok("new-signature".into())
            })?
            .as_deref(),
            Some("new-signature")
        );
        assert!(a1.personal_install_generation(&cfg).is_err());
        assert!(a1.bootstrap_install_generation(&acs4()).is_err());
        let acs = Store::open(Path::new(":memory:"))?;
        acs.bootstrap_install_generation(&acs4())?;
        assert!(acs.personal_install_generation(&cfg).is_err());
        let i = cfg.identity();
        assert!(acs.db.locked().execute("INSERT INTO personal_generation(singleton,config,generation,context,computer,sign_key,box_key) VALUES(1,?,?,?,?,?,?)",params![cfg.original_bytes(),i.generation,serde_json::to_string(cfg.context())?,i.computer_id,i.sign_key,i.box_key]).is_err());
        let generation = acs4().generation;
        acs.bootstrap_claim_a1(Some(&generation), "acs4-a1", |c| {
            assert_eq!(c.query_row("SELECT ever_claimed FROM bootstrap_generation", [], |r| r.get::<_, u32>(0))?, 1);
            assert_eq!(
                c.query_row("SELECT ever_claimed FROM first_owner WHERE origin='acs4'", [], |r| r.get::<_, u32>(0))?,
                1
            );
            Ok("signature".into())
        })?;
        Ok(())
    }
    #[test]
    fn legacy_without_config_accepts_new_local_claim_after_cancel_without_reopening_origin() -> Result<()> {
        let s = Store::open(Path::new(":memory:"))?;
        s.db.locked().execute("INSERT INTO devices(key,name,platform,source,scopes,created_at) VALUES('old','old','fixture','local','[]',1)",[])?;
        let calls = AtomicUsize::new(0);
        for input in ["expired-claim-and-nonce", "new-claim-and-nonce"] {
            assert_eq!(
                s.bootstrap_claim_a1(None, input, |c| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(
                        c.query_row("SELECT origin,ever_claimed FROM first_owner", [], |r| Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, u32>(1)?
                        )))?,
                        ("legacy".into(), 1)
                    );
                    Ok(format!("signed:{input}"))
                })?,
                Some(format!("signed:{input}"))
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let (cfg, _) = crate::fork::personal_bootstrap::provisioning::fixture()?;
        assert!(s.personal_install_generation(&cfg).is_err());
        assert!(s.bootstrap_install_generation(&acs4()).is_err());
        let c = s.db.locked();
        assert_eq!(
            c.query_row("SELECT a1_input FROM first_owner", [], |r| r.get::<_, String>(0))?,
            "expired-claim-and-nonce"
        );
        assert!(c.execute("UPDATE first_owner SET origin='a1',ever_claimed=0", []).is_err());
        Ok(())
    }
    #[test]
    fn backfill_keeps_proven_history_but_does_not_certify_an_existing_empty_database() -> Result<()> {
        let s = Store::open(Path::new(":memory:"))?;
        {
            let c = s.db.locked();
            migrate(&c, false)?;
            assert_eq!(c.query_row("SELECT count(*) FROM first_owner", [], |r| r.get::<_, u32>(0))?, 0);
            c.execute("INSERT INTO devices(key,name,platform,source,scopes,created_at) VALUES('old','old','fixture','local','[]',1)",[])?;
            c.execute_batch("DROP TRIGGER first_owner_keep; DELETE FROM first_owner;")?;
            migrate(&c, false)?;
            assert_eq!(c.query_row("SELECT origin FROM first_owner", [], |r| r.get::<_, String>(0))?, "legacy");
            c.execute("DELETE FROM devices", [])?;
            assert!(c.execute("DELETE FROM first_owner", []).is_err());
        }
        let (cfg, _) = crate::fork::personal_bootstrap::provisioning::fixture()?;
        assert!(s.personal_install_generation(&cfg).is_err());
        let untracked = Store::open(Path::new(":memory:"))?;
        {
            let c = untracked.db.locked();
            c.execute_batch("DROP TRIGGER personal_birth_keep; DELETE FROM personal_local_birth;")?;
            migrate(&c, false)?;
        }
        assert!(untracked.personal_install_generation(&cfg).is_err());
        Ok(())
    }
    #[test]
    fn shared_store_race_has_one_origin_and_no_losing_a1_signature() -> Result<()> {
        let store = Arc::new(Store::open(Path::new(":memory:"))?);
        let barrier = Arc::new(Barrier::new(2));
        let count = Arc::new(AtomicUsize::new(0));
        let a = store.clone();
        let b = barrier.clone();
        let signatures = count.clone();
        let worker = std::thread::spawn(move || {
            b.wait();
            a.bootstrap_claim_a1(None, "a1", |_| {
                signatures.fetch_add(1, Ordering::SeqCst);
                Ok("signature".into())
            })
            .is_ok()
        });
        let (cfg, _) = crate::fork::personal_bootstrap::provisioning::fixture()?;
        barrier.wait();
        let personal = store.personal_install_generation(&cfg).is_ok();
        let a1 = worker.join().map_err(|_| anyhow::anyhow!("fixture thread failed"))?;
        assert_ne!(personal, a1);
        assert_eq!(count.load(Ordering::SeqCst), usize::from(a1));
        assert_eq!(store.db.locked().query_row("SELECT count(*) FROM first_owner", [], |r| r.get::<_, u32>(0))?, 1);
        Ok(())
    }
}
