use super::*;
use registry::{Asset, packages};

// A serialização é só da suíte: mantém os locks reais e a concorrência explícita
// dentro dos testes, sem disputa acidental entre casos do mesmo processo.
pub(super) static TEST_ISOLATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn install(store: &Store, body: &Value) -> Result<Value> {
    super::install(store, body, || Ok(()))
}
fn custom(store: &Store, body: &Value) -> Result<Value> {
    super::custom(store, body, || Ok(()))
}
fn remove(store: &Store, body: &Value) -> Result<()> {
    super::remove(store, body, || Ok(()))
}

fn current_removal(store: &Store, id: &str) -> Value {
    let listing = list(store).unwrap();
    r6_captured(listing["items"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap())
}

fn list(store: &Store) -> Result<Value> {
    super::list(store, || Ok(()))
}
fn browse(store: &Store) -> Result<Value> {
    super::browse(store, || Ok(()))
}
fn details(store: &Store, body: &Value) -> Result<Value> {
    super::details(store, body, || Ok(()))
}

struct Fixture(Store);
impl Fixture {
    fn new() -> Self {
        Self(Store::open(Path::new(":memory:")).unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0.skills_root);
    }
}
fn intent(p: &Package) -> Value {
    json!({"catalogId":p.metadata.id,"expectedDigest":p.metadata.digest})
}

// A mesma prova roda na fonte pré-R6: reproduz exatamente o pedido que aquele DTO permite.
fn r6_captured(skill: &Value) -> Value {
    if let Some(version) = skill["installationId"].as_str() {
        let receipt = if skill["receipt"].is_null() {
            json!({"kind":"none"})
        } else {
            json!({"kind":"catalog","catalogId":skill["receipt"]["catalogId"],"digest":skill["receipt"]["digest"]})
        };
        json!({"id":skill["id"],"expectedInstallationId":version,"expectedReceipt":receipt})
    } else if skill["receipt"].is_null() {
        json!({"id":skill["id"]})
    } else {
        json!({"id":skill["id"],"expectedDigest":skill["receipt"]["digest"]})
    }
}

#[tokio::test]
async fn installation_r6_unseen_manual_to_catalog_replacement_is_preserved() {
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let old = f
        .call("installSkill", json!({"name":"frontend-design","description":"synthetic","instructions":"OLD_LITERAL"}))
        .await
        .unwrap();
    let captured = r6_captured(&old["skill"]);
    f.call("removeSkill", captured.clone()).await.unwrap();
    let p = registry::find("frontend-design").unwrap();
    let new = f.call("installSkill", intent(&p)).await.unwrap();
    assert_eq!(old["skill"]["id"], new["skill"]["id"]);
    assert!(f.call("removeSkill", json!({"id":p.install_id,"expectedDigest":"wrong"})).await.is_err());
    let before = f.snapshot();
    let result = f.call("removeSkill", captured.clone()).await;
    println!(
        "INSTALLATION_R6 {}",
        json!({"case":"unseen_manual_to_catalog","captured":captured,"old":old,"new":new,"rejected":result.is_err(),"preserved":f.snapshot()==before})
    );
    assert!(result.is_err(), "intenção da manual apagou o pacote novo");
    assert_eq!(f.snapshot(), before);
    f.call("removeSkill", r6_captured(&new["skill"])).await.unwrap();
    assert!(!f.hub.store.skills_root.join(&p.install_id).exists());
}

#[tokio::test]
async fn installation_r6_identical_manual_aba_is_preserved() {
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let manual = json!({"name":"frontend-design","description":"same metadata","instructions":"SAME_LITERAL"});
    {
        let body = manual;
        let first = f.call("installSkill", body.clone()).await.unwrap();
        let captured = r6_captured(&first["skill"]);
        f.call("removeSkill", captured.clone()).await.unwrap();
        let second = f.call("installSkill", body).await.unwrap();
        assert_eq!(first["skill"]["id"], second["skill"]["id"]);
        let before = f.snapshot();
        let result = f.call("removeSkill", captured.clone()).await;
        println!(
            "INSTALLATION_R6 {}",
            json!({"case":"identical_reinstall","captured":captured,"first":first,"second":second,"rejected":result.is_err(),"preserved":f.snapshot()==before})
        );
        assert!(result.is_err(), "intenção velha apagou reinstalação idêntica");
        assert_eq!(f.snapshot(), before);
        f.call("removeSkill", r6_captured(&second["skill"])).await.unwrap();
    }
}
fn refresh(p: &mut Package) {
    p.metadata.files = p.assets.iter().map(Asset::info).collect();
    p.metadata.digest = p.envelope_digest().unwrap();
}

#[test]
fn original_language_metadata_and_bytes_match_all_pinned_sources() {
    let sources: Vec<Value> = serde_json::from_str(include_str!("source-manifest.json")).unwrap();
    let all: Vec<_> = packages()
        .unwrap()
        .into_iter()
        .filter(|p| p.metadata.id != criador_de_skills::ID && p.metadata.category != "Conectores")
        .collect();
    assert_eq!(sources.len(), 28);
    assert_eq!(all.iter().filter(|p| p.installable()).count(), 14);
    for p in all {
        let s = sources.iter().find(|s| s["id"] == p.metadata.id).unwrap();
        assert_eq!(p.metadata.name, s["sourceName"]);
        assert_eq!(p.metadata.description, s["sourceDescription"]);
        assert_eq!(p.install_id, s["sourceName"]);
        assert_eq!(serde_json::to_value(&p.metadata.origin).unwrap(), s["origin"]);
        if let Some(previous) = s["previousDigest"].as_str() {
            assert_ne!(p.metadata.digest, previous);
            let archived = registry::captured(&p.metadata.id, previous).unwrap().unwrap();
            assert!(archived.assets.is_empty());
            assert_eq!(archived.metadata.digest, previous);
            assert_eq!(archived.metadata.origin, p.metadata.origin);
            assert!(registry::captured("outro-catalogo", &archived.metadata.digest).unwrap().is_none());
        } else {
            assert!(matches!(p.metadata.id.as_str(), "stop-slop" | "copy-review-panel"));
        }
        for a in &p.assets {
            let source = s["files"].as_array().unwrap().iter().find(|f| f["destination"] == a.path).unwrap();
            assert_eq!(a.sha256, source["sha256"]);
            assert_eq!(a.bytes, usize::try_from(source["bytes"].as_u64().unwrap()).unwrap());
            let adapted_skill =
                a.path == "SKILL.md" && matches!(p.metadata.id.as_str(), "copywriting" | "copy-review-panel");
            if a.path == "SKILL.md" {
                if adapted_skill {
                    assert_eq!(source["sourceSha256"], s["sourceSkill"]["sha256"]);
                    assert_eq!(source["modification"], "adaptação textual declarada em ADAPTATION.md");
                } else {
                    assert_eq!(a.sha256, s["sourceSkill"]["sha256"]);
                }
            }
            if a.path != "ADAPTATION.md" {
                if p.metadata.id == "copywriting" && a.path == "references/donna-transitions.md" {
                    assert!(source["source"].is_null());
                    assert_eq!(source["modification"], "autoria própria Donna Bot D353");
                } else if !adapted_skill {
                    assert_eq!(source["modification"], "nenhuma; bytes originais");
                }
                source["licenseEvidence"].as_array().unwrap().first().expect("Aviso de licença da fonte");
            }
        }
        if !p.installable() {
            assert!(p.assets.is_empty());
            assert_ne!(p.metadata.reason, "");
        }
    }
    let security = registry::find("security-audit").unwrap();
    assert_eq!(
        security.assets.iter().find(|a| a.path == "SKILL.md").unwrap().sha256,
        "5e3e96a1e438d8f35fef0a1e38f02d4f00e2bdf910401b7dfe00e6779c6dac85"
    );
    assert_eq!(
        registry::find("find-skills").unwrap().assets[0].sha256,
        "c00eeea0e13e74fe4a9d84ba0a8542205a1b736d65f13134fe1a6647eb14976f"
    );
}

fn captured_fixture(s: &Store, id: &str) -> (Package, Receipt) {
    let fixtures: Vec<Value> = serde_json::from_str(include_str!("legacy-fixtures.json")).unwrap();
    let f = fixtures.iter().find(|f| f["package"]["metadata"]["id"] == id).unwrap();
    let p: Package = serde_json::from_value(f["package"].clone()).unwrap();
    let r: Receipt = serde_json::from_value(f["receipt"].clone()).unwrap();
    assert_eq!(p.envelope_digest().unwrap(), r.digest);
    assert_eq!(receipt(&p), r);
    let skill = Skill {
        id: p.install_id.clone(),
        name: p.metadata.name.clone(),
        description: p.metadata.description.clone(),
        source: format!("catalog:{}", p.metadata.id),
    };
    let _guard = MUTATION.locked();
    let _lock = root_lock(s).unwrap();
    publish(s, &skill.id, &p.assets, Some(&r)).unwrap();
    let state = State {
        seed_done: false,
        receipts: BTreeMap::from([(r.digest.clone(), r.clone())]),
        installed: BTreeMap::from([(skill.id.clone(), r.digest.clone())]),
        ..State::default()
    };
    r7_legacy_write(s, &[skill], &serde_json::to_value(&state).unwrap());
    (p, r)
}

#[test]
fn installed_legacy_receipt_keeps_exact_identity_metadata_bytes_and_removal_after_catalog_repair() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    for id in ["find-skills", "frontend-design"] {
        let f = Fixture::new();
        let s = &f.0;
        let (old, r) = captured_fixture(s, id);
        let dest = s.skills_root.join(&old.install_id);
        let before_receipt = fs::read(dest.join(RECEIPT)).unwrap();
        let before_skills = s.kv_get("skills").unwrap();
        let current = registry::find(id).unwrap();
        let listing = browse(s).unwrap();
        let item = listing["items"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap();
        assert_eq!(item["installed"], false, "o digest atual não é o recibo legado");
        let d = details(s, &json!({"id":old.install_id})).unwrap();
        assert_eq!(d["catalog"]["digest"], r.digest);
        assert_eq!(d["catalog"]["name"], old.metadata.name);
        assert_eq!(d["catalog"]["installedId"], old.install_id);
        assert_eq!(d["catalog"]["installable"], false);
        assert_eq!(d["receipt"], serde_json::to_value(&r).unwrap());
        assert!(install(s, &intent(&old)).is_err(), "digest legado não aceita novo install");
        assert!(remove(s, &json!({"id":old.install_id,"expectedDigest":current.metadata.digest})).is_err());
        if id == "find-skills" {
            seed(s).unwrap();
            assert_eq!(load(s).unwrap().0.len(), 1, "nenhum novo seed por mudança de destino");
            assert!(!s.skills_root.join(&current.install_id).exists());
            install(s, &intent(&current)).unwrap();
            assert_eq!(load(s).unwrap().0.len(), 2, "novo destino só por intenção explícita");
            assert_eq!(details(s, &json!({"id":old.install_id})).unwrap()["receipt"], d["receipt"]);
        } else {
            assert_eq!(item["installable"], false, "colisão no mesmo destino bloqueia");
            assert!(install(s, &intent(&current)).is_err());
            assert_eq!(s.kv_get("skills").unwrap(), before_skills);
        }
        assert_eq!(fs::read(dest.join(RECEIPT)).unwrap(), before_receipt);
        for a in &old.assets {
            assert_eq!(fs::read(dest.join(&a.path)).unwrap(), a.content.as_bytes());
        }
        remove(s, &current_removal(s, &old.install_id)).unwrap();
        assert!(!dest.exists());
        assert_eq!(load(s).unwrap().1.receipts.get(&r.digest), Some(&r));
        let installed = install(s, &intent(&current)).unwrap();
        assert_eq!(installed["skill"]["id"], current.install_id);
        assert_eq!(installed["skill"]["receipt"]["digest"], current.metadata.digest);
    }
}

#[test]
fn original_destination_collision_never_matches_display_name_or_translates_manual_portuguese() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let f = Fixture::new();
    let s = &f.0;
    let manual = json!({"name":"find-skills","description":"Minha habilidade manual em português",
        "instructions":"# Roteiro meu\nMantenha este texto em português."});
    let one = custom(s, &manual).unwrap();
    let two = custom(s, &manual).unwrap();
    let second = two["skill"]["id"].as_str().unwrap();
    let before = fs::read(s.skills_root.join(second).join("SKILL.md")).unwrap();
    let p = registry::find("find-skills").unwrap();
    assert!(install(s, &intent(&p)).is_err());
    remove(s, &r6_captured(&one["skill"])).unwrap();
    install(s, &intent(&p)).unwrap();
    assert_eq!(load(s).unwrap().0.len(), 2);
    assert_eq!(fs::read(s.skills_root.join(second).join("SKILL.md")).unwrap(), before);
    assert_eq!(details(s, &json!({"id":second})).unwrap()["markdown"], manual["instructions"]);
    assert!(details(s, &json!({"id":second})).unwrap()["receipt"].is_null());
}

#[test]
fn curated_bundle_has_all_entries_aliases_notices_and_strict_video_readiness() {
    let all = packages().unwrap();
    assert_eq!(all.len(), 33);
    for id in [
        "security-audit",
        "hallmark",
        "impeccable",
        "skill-creator",
        "criador-de-skills",
        "frontend-design",
        "pdf",
        "xlsx",
        "find-skills",
        "grilling",
        "handoff",
        "research",
        "brainstorming",
        "copywriting",
        "stop-slop",
        "copy-review-panel",
        "content-strategy",
        "marketing-psychology",
        "orca-reference",
        "browser-use",
        "hyperframes",
        "firecrawl",
        "firecrawl-search",
        "firecrawl-scrape",
        "firecrawl-build-scrape",
        "firecrawl-build-search",
        "firecrawl-build-interact",
        "firecrawl-deep-research",
        "boat",
    ] {
        assert!(all.iter().any(|p| p.metadata.id == id), "{id}");
    }
    assert_eq!(registry::find("grill-me").unwrap().metadata.id, "grilling");
    assert_eq!(registry::find("orca").unwrap().metadata.id, "orca-reference");
    for p in all {
        p.validate().unwrap();
        if p.installable() {
            assert!(p.assets.iter().any(|a| a.path.starts_with("LICENSE")));
            if p.metadata.availability == "adapted" {
                assert!(p.assets.iter().any(|a| a.path == "ADAPTATION.md"));
            }
        } else {
            assert!(p.assets.is_empty());
        }
    }
    let p = registry::find("hyperframes").unwrap();
    let readiness = p.metadata.readiness.unwrap();
    assert!(!readiness.enabled);
    assert_eq!(readiness.runtime, "unavailable");
    for action in ["render", "preview", "hosted", "registryImport", "probe", "download"] {
        assert!(readiness.unavailable_actions.iter().any(|a| a == action));
    }
}

#[test]
fn manifests_reject_origin_digest_hash_mode_license_path_limits_and_missing_files() {
    let original = registry::find("find-skills").unwrap();
    for field in ["name", "description"] {
        let mut p = original.clone();
        if field == "name" {
            p.metadata.name = "Encontrar habilidades".into();
        } else {
            p.metadata.description = "Descrição traduzida sem alterar o original".into();
        }
        refresh(&mut p);
        assert!(p.validate().is_err(), "metadado traduzido não equivale ao original");
    }
    let mut p = original.clone();
    p.metadata.digest = "0".repeat(64);
    assert!(p.validate().is_err());
    let mut p = original.clone();
    p.metadata.origin.revision = "main".into();
    refresh(&mut p);
    assert!(p.validate().is_err());
    let mut p = original.clone();
    p.metadata.license = "Proprietary".into();
    refresh(&mut p);
    assert!(p.validate().is_err());
    let mut p = original.clone();
    p.assets[0].content.push('!');
    assert!(p.validate().is_err());
    let mut p = original.clone();
    p.assets[0].mode = "100755".into();
    refresh(&mut p);
    assert!(p.validate().is_err());
    for path in [
        "../SKILL.md",
        "/SKILL.md",
        "a\\SKILL.md",
        "a//b.md",
        "a/./b.md",
        "a/b/c/d/e/f.md",
        "hook.sh",
        "setup.py",
        "index.html",
        ".donnabot-receipt.json",
    ] {
        let mut p = original.clone();
        p.assets[0].path = path.into();
        refresh(&mut p);
        assert!(p.validate().is_err(), "{path}");
    }
    let mut p = original.clone();
    p.assets.push(p.assets[0].clone());
    refresh(&mut p);
    assert!(p.validate().is_err());
    let mut p = original.clone();
    p.assets.remove(0);
    refresh(&mut p);
    assert!(p.validate().is_err());
    let mut p = original.clone();
    p.assets[0].bytes = registry::MAX_SKILL + 1;
    refresh(&mut p);
    assert!(p.validate().is_err());
    let mut p = original.clone();
    p.assets[1].bytes = 1024 * 1024 + 1;
    refresh(&mut p);
    assert!(p.validate().is_err());
    let mut p = original.clone();
    p.assets = vec![p.assets[0].clone(); 101];
    refresh(&mut p);
    assert!(p.validate().is_err());
    let mut p = original.clone();
    p.metadata.losses.clear();
    refresh(&mut p);
    assert!(p.validate().is_err());
    let mut p = registry::find("hyperframes").unwrap();
    p.metadata.readiness.as_mut().unwrap().enabled = true;
    refresh(&mut p);
    assert!(p.validate().is_err());
}

#[test]
fn intent_refuses_legacy_source_free_urls_stale_digest_and_blocked_licenses() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let f = Fixture::new();
    let s = &f.0;
    let p = registry::find("find-skills").unwrap();
    for body in [
        json!({"source":"pdf"}),
        json!({"source":"frontend-design","catalogId":p.metadata.id,"expectedDigest":p.metadata.digest}),
        json!({"catalogId":"https://example.test/pkg","expectedDigest":p.metadata.digest}),
        json!({"catalogId":p.metadata.id,"expectedDigest":"old"}),
        json!({"catalogId":p.metadata.id,"expectedDigest":p.metadata.digest,"botId":"injected"}),
        json!({"catalogId":p.metadata.id,"expectedDigest":p.metadata.digest,"manifest":{}}),
    ] {
        assert!(install(s, &body).is_err());
    }
    for id in ["pdf", "xlsx", "orca"] {
        let p = registry::find(id).unwrap();
        assert!(install(s, &intent(&p)).is_err());
    }
    assert_eq!(load(s).unwrap().0, Vec::<Skill>::new());
}

#[test]
fn import_is_atomic_idempotent_and_recovers_only_complete_matching_orphans() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let f = Fixture::new();
    let s = &f.0;
    let p = registry::find("find-skills").unwrap();
    let first = install(s, &intent(&p)).unwrap();
    assert_eq!(first["alreadyInstalled"], false);
    let again = install(s, &intent(&p)).unwrap();
    assert_eq!(again["alreadyInstalled"], true);
    assert_eq!(first["skill"]["receipt"], again["skill"]["receipt"]);
    assert_eq!(load(s).unwrap().0.len(), 1);
    let r: Receipt = serde_json::from_value(first["skill"]["receipt"].clone()).unwrap();
    assert_eq!(r.digest, p.metadata.digest);
    assert_eq!(r.coverage, "pinnedPassiveTextOnly");
    // Simula perda após rename e antes da transação: pacote completo não é sobrescrito.
    let (_, state) = load(s).unwrap();
    r7_legacy_write(s, &[], &json!({"seedDone":state.seed_done,"receipts":{},"installed":{}}));
    assert_eq!(install(s, &intent(&p)).unwrap()["alreadyInstalled"], false);
    assert_eq!(load(s).unwrap().0.len(), 1);
    fs::write(s.skills_root.join(&p.install_id).join("SKILL.md"), "tampered").unwrap();
    assert!(install(s, &intent(&p)).is_err());
    assert_eq!(load(s).unwrap().0.len(), 1);
    assert!(
        fs::read_dir(&s.skills_root).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".stage-"))
    );
}

#[test]
fn staging_failure_never_registers_or_overwrites_existing_versions() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let f = Fixture::new();
    let s = &f.0;
    let p = registry::find("find-skills").unwrap();
    private_dir(&s.skills_root).unwrap();
    let dest = s.skills_root.join(&p.install_id);
    fs::create_dir(&dest).unwrap();
    fs::write(dest.join("SKILL.md"), "legacy preserved").unwrap();
    assert!(install(s, &intent(&p)).is_err());
    assert_eq!(fs::read_to_string(dest.join("SKILL.md")).unwrap(), "legacy preserved");
    assert_eq!(load(s).unwrap().0, Vec::<Skill>::new());
    fs::remove_dir_all(&dest).unwrap();
    let mut bad = p.clone();
    bad.assets[1].path = "SKILL.md/child.txt".into();
    refresh(&mut bad);
    // Validação estrutural/colisão também é feita antes do registro.
    let _guard = MUTATION.locked();
    let _lock = root_lock(s).unwrap();
    assert!(install_locked(s, &bad, false, None).is_err());
    assert!(!dest.exists());
    assert_eq!(load(s).unwrap().0, Vec::<Skill>::new());
}

#[test]
fn sqlite_failure_does_not_publish_registration_or_leave_new_package() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let f = Fixture::new();
    let s = &f.0;
    let p = registry::find("find-skills").unwrap();
    s.db.locked().execute_batch("CREATE TRIGGER fail_skill_state BEFORE INSERT ON kv WHEN NEW.k='donna.passiveSkills.v1' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(install(s, &intent(&p)).is_err());
    assert_eq!(load(s).unwrap().0, Vec::<Skill>::new());
    assert!(!s.skills_root.join(&p.install_id).exists());
}

#[test]
fn seed_once_removal_and_reinstall_do_not_activate_or_resurrect_bindings() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let f = Fixture::new();
    let s = &f.0;
    let mut cfg: crate::store::BotConfig = serde_json::from_value(
        json!({"id":"bot","name":"bot","backend":"fixture","command":"true","cwd":"/tmp","skills":["legacy"]}),
    )
    .unwrap();
    s.save_bot(&cfg).unwrap();
    seed(s).unwrap();
    seed(s).unwrap();
    assert_eq!(load(s).unwrap().0.len(), 1);
    assert_eq!(s.bot("bot").unwrap().unwrap().config.skills, ["legacy"]);
    let p = registry::find("find-skills").unwrap();
    cfg.skills.push(p.install_id.clone());
    crate::fork::skill_selection::set_for_test(s, "bot", &["legacy", p.install_id.as_str()]);
    s.save_bot(&cfg).unwrap();
    assert!(remove(s, &json!({"id":p.install_id,"expectedDigest":"wrong"})).is_err());
    remove(s, &current_removal(s, &p.install_id)).unwrap();
    seed(s).unwrap();
    assert_eq!(load(s).unwrap().0, Vec::<Skill>::new());
    assert_eq!(s.bot("bot").unwrap().unwrap().config.skills, ["legacy"]);
    install(s, &intent(&p)).unwrap();
    assert_eq!(s.bot("bot").unwrap().unwrap().config.skills, ["legacy"]);
    let d = details(s, &json!({"id":p.install_id})).unwrap();
    assert_eq!(d["receipt"]["digest"], p.metadata.digest);
    assert_eq!(d["catalog"]["installedId"], p.install_id);
    assert!(s.kv_get("skillsOnForEveryBot").is_none());
}

#[test]
fn manual_text_is_bounded_passive_and_preserves_legacy_and_bot_choices() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let f = Fixture::new();
    let s = &f.0;
    let marker = s.skills_root.with_extension("must-not-exist");
    let body = format!("npm install evil; touch {}", marker.display());
    let response = custom(s, &json!({"name":"Texto meu","description":"Manual","instructions":body})).unwrap();
    assert!(!marker.exists());
    assert!(response["skill"]["receipt"].is_null());
    assert_eq!(details(s, &json!({"id":response["skill"]["id"]})).unwrap()["markdown"], body);
    for body in [
        json!({"name":"bad","description":""}),
        json!({"name":"bad","description":"manual","instructions":"x".repeat(32768)}),
        json!({"name":"bad","description":"manual","source":"url"}),
    ] {
        assert!(custom(s, &body).is_err());
    }
    assert_eq!(load(s).unwrap().0.len(), 1);
}

#[cfg(unix)]
#[test]
fn symlinks_special_modes_and_external_lock_cannot_bypass_publication() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let f = Fixture::new();
    let s = &f.0;
    let p = registry::find("find-skills").unwrap();
    private_dir(&s.skills_root).unwrap();
    let outside = s.skills_root.with_extension("outside");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, s.skills_root.join(&p.install_id)).unwrap();
    assert!(install(s, &intent(&p)).is_err());
    assert!(fs::read_dir(&outside).unwrap().next().is_none());
    fs::remove_file(s.skills_root.join(&p.install_id)).unwrap();
    let held = root_lock(s).unwrap();
    // Modela uma cópia herdada por fork/exec sem depender de processos concorrentes.
    let inherited = held.try_clone().unwrap();
    assert!(install(s, &intent(&p)).is_err());
    drop(held);
    assert!(install(s, &intent(&p)).is_err(), "a cópia ainda retém a trava externa");
    fs2::FileExt::unlock(&inherited).unwrap();
    install(s, &intent(&p)).unwrap();
    drop(inherited);
    let file = s.skills_root.join(&p.install_id).join("SKILL.md");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(install(s, &intent(&p)).is_err());
    fs::set_permissions(file, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&outside, s.skills_root.join(&p.install_id).join("extra.md")).unwrap();
    assert!(install(s, &intent(&p)).is_err());
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn concurrent_duplicate_imports_publish_one_version_and_one_receipt() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let f = Fixture::new();
    let p = registry::find("frontend-design").unwrap();
    std::thread::scope(|scope| {
        let jobs: Vec<_> = (0..4).map(|_| scope.spawn(|| install(&f.0, &intent(&p)))).collect();
        let results: Vec<_> = jobs.into_iter().map(|t| t.join().unwrap().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r["alreadyInstalled"] == false).count(), 1);
    });
    let (all, state) = load(&f.0).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(state.receipts.len(), 1);
}

#[test]
fn legacy_text_without_proof_keeps_files_and_explicit_removal_but_is_not_adopted() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let f = Fixture::new();
    let s = &f.0;
    private_dir(&s.skills_root).unwrap();
    let legacy = s.skills_root.join("legacy");
    fs::create_dir(&legacy).unwrap();
    fs::write(legacy.join("SKILL.md"), "---\nname: InvalidLegacy\n---\nTexto legado").unwrap();
    s.kv_set("skills", r#"[{"id":"legacy","name":"Antiga","source":"anthropics/skills"}]"#).unwrap();
    seed(s).unwrap();
    assert_eq!(details(s, &json!({"id":"legacy"})).unwrap_err().to_string(), installation::UNAVAILABLE);
    let listed = list(s).unwrap();
    let d = listed["items"].as_array().unwrap().iter().find(|s| s["id"] == "legacy").unwrap();
    assert_eq!(d["contentStatus"], "unavailable");
    assert!(d["receipt"].is_null());
    assert_eq!(fs::read_to_string(legacy.join("SKILL.md")).unwrap(), "---\nname: InvalidLegacy\n---\nTexto legado");
    let captured = current_removal(s, "legacy");
    remove(s, &captured).unwrap();
    assert!(!legacy.exists());
    assert_eq!(load(s).unwrap().0.len(), 1, "somente o seed permanece");
    remove(s, &captured).unwrap();
}

#[tokio::test]
async fn rpc_revalidates_device_scope_and_installation_never_selects_bots() {
    use crate::api::devices::Caller;
    use crate::store::{Device, DeviceSource, Scope};
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = Fixture::new();
    let root = f.0.skills_root.clone();
    let identity_dir = root.with_extension("identity");
    fs::create_dir_all(&identity_dir).unwrap();
    // O Store é movido ao Hub fixture; os caminhos são limpos explicitamente ao final.
    let fixture_store = Store::open(Path::new(":memory:")).unwrap();
    let owned_root = fixture_store.skills_root.clone();
    let cfg: crate::store::BotConfig = serde_json::from_value(
        json!({"id":"fixture-bot","name":"Fixture","backend":"fixture","command":"true","cwd":"/tmp","skills":[]}),
    )
    .unwrap();
    fixture_store.save_bot(&cfg).unwrap();
    let hub = crate::hub::Hub::new(
        fixture_store,
        "fixture".into(),
        crate::remote::identity::Identity::load_or_create(&identity_dir).unwrap(),
        "fixture".into(),
        0,
    );
    let p = registry::find("find-skills").unwrap();
    let stale = Caller::Device { key: "revoked".into(), scopes: vec![Scope::Control], autoridade: None };
    assert!(crate::api::dispatch(&hub, &stale, "installSkill", intent(&p)).await.is_err());
    let mut device = Device {
        key: "revoked".into(),
        name: "Fixture".into(),
        platform: "fixture".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Screen],
        lease_until: None,
        created_at: 0,
        last_seen_at: None,
    };
    hub.store.put_device(&device).unwrap();
    assert!(
        crate::api::dispatch(&hub, &stale, "installSkill", intent(&p)).await.is_err(),
        "escopo atual prevalece sobre o snapshot antigo"
    );
    device.scopes = vec![Scope::Control];
    hub.store.put_device(&device).unwrap();
    // D368: sem o retrato do transporte o aparelho não grava (fail-closed); autenticado, grava.
    assert!(crate::api::dispatch(&hub, &stale, "installSkill", intent(&p)).await.is_err());
    let (_, atual) = crate::fork::autoridade::autenticar(&hub, &device.key).unwrap();
    let installed = crate::api::dispatch(&hub, &atual, "installSkill", intent(&p)).await.unwrap();
    assert_eq!(installed["skill"]["receipt"]["digest"], p.metadata.digest);
    assert_eq!(hub.store.bot("fixture-bot").unwrap().unwrap().config.skills, Vec::<String>::new());
    assert!(crate::api::dispatch(&hub, &Caller::Local, "installSkill", json!({"source":"pdf"})).await.is_err());
    let catalog = crate::api::dispatch(&hub, &Caller::Local, "marketSkills", json!({})).await.unwrap();
    assert_eq!(catalog["items"].as_array().unwrap().len(), 33);
    assert!(load(&hub.store).unwrap().1.receipts.contains_key(&p.metadata.digest), "recibo indexado pelo digest");
    fs::remove_dir_all(owned_root).unwrap();
    fs::remove_dir_all(identity_dir).unwrap();
}

#[test]
fn mutation_rechecks_authority_after_its_locks_before_publishing() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let f = Fixture::new();
    let p = registry::find("find-skills").unwrap();
    assert!(super::install(&f.0, &intent(&p), || bail!("revoked")).is_err());
    assert_eq!(load(&f.0).unwrap().0, Vec::<Skill>::new());
    assert!(!f.0.skills_root.join(&p.install_id).exists());
}

#[test]
fn seed_identity_collision_preserves_legacy_and_reports_unavailable_instead_of_claiming_installation() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let f = Fixture::new();
    let s = &f.0;
    let p = registry::find("find-skills").unwrap();
    s.kv_set("skills", &json!([{"id":p.install_id,"name":"Meu legado","source":"custom"}]).to_string()).unwrap();
    seed(s).unwrap();
    let items = browse(s).unwrap();
    let item = items["items"].as_array().unwrap().iter().find(|i| i["id"] == "find-skills").unwrap();
    assert_eq!(item["installed"], false);
    assert_eq!(item["installable"], false);
    assert!(item["reason"].as_str().unwrap().contains("legado"));
    assert_eq!(load(s).unwrap().0[0].name, "Meu legado");
    assert!(load(s).unwrap().1.receipts.is_empty());
}

#[test]
fn cumulative_limits_and_atomic_no_replace_are_enforced() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let mut p = registry::find("find-skills").unwrap();
    for n in 0..6 {
        let content = "x".repeat(1024 * 1024 - 1);
        p.assets.push(Asset {
            path: format!("references/{n}.txt"),
            bytes: content.len(),
            sha256: hash(content.as_bytes()),
            mode: "100644".into(),
            content,
        });
    }
    refresh(&mut p);
    assert!(p.validate().is_err());
    let f = Fixture::new();
    private_dir(&f.0.skills_root).unwrap();
    let source = f.0.skills_root.join("stage");
    let dest = f.0.skills_root.join("existing");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&dest).unwrap();
    fs::write(source.join("data.txt"), "source").unwrap();
    assert!(rename_new(&source, &dest).is_err());
    assert!(source.join("data.txt").exists());
    assert!(fs::read_dir(dest).unwrap().next().is_none());
}

// SPACE-SKILLS-1: dispatch/SQLite reais, dados e identidade descartáveis; não inicia Hub/motor.
pub(super) struct ScopeFixture {
    pub(super) hub: std::sync::Arc<crate::hub::Hub>,
    root: PathBuf,
    additional: Value,
}
impl ScopeFixture {
    pub(super) async fn new() -> Self {
        use crate::{
            api::{devices::Caller, dispatch},
            hub::Hub,
            remote::identity::Identity,
        };
        let root = std::env::temp_dir().join(format!("d166-scope-r4-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("identity")).unwrap();
        let hub = Hub::new(
            Store::open(&root.join("fixture.db")).unwrap(),
            "scope-fixture".into(),
            Identity::load_or_create(&root.join("identity")).unwrap(),
            "synthetic-only".into(),
            0,
        );
        let state = hub.store.spaces().unwrap();
        let spaces = dispatch(
            &hub,
            &Caller::Local,
            "donnaSpaceCreate",
            json!({"hubId":hub.host_id,
            "expectedRevision":state.revision,"name":"Synthetic additional",
            "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"SA"}}}),
        )
        .await
        .unwrap();
        Self { hub, root, additional: spaces["spaces"][1]["id"].clone() }
    }
    pub(super) async fn call(&self, method: &str, body: Value) -> Result<Value> {
        crate::api::dispatch(&self.hub, &crate::api::devices::Caller::Local, method, body).await
    }
    fn snapshot(&self) -> Value {
        fn walk(root: &Path, path: &Path, files: &mut BTreeMap<String, String>) {
            if !path.exists() {
                return;
            }
            for e in fs::read_dir(path).unwrap() {
                let e = e.unwrap();
                if e.file_type().unwrap().is_dir() {
                    walk(root, &e.path(), files);
                } else {
                    files.insert(
                        e.path().strip_prefix(root).unwrap().to_string_lossy().into_owned(),
                        hash(&fs::read(e.path()).unwrap()),
                    );
                }
            }
        }
        let mut files = BTreeMap::new();
        walk(&self.hub.store.skills_root, &self.hub.store.skills_root, &mut files);
        json!({"registry":self.hub.store.kv_get("skills"),"state":self.hub.store.kv_get(STATE),"stateV2":self.hub.store.kv_get(storage::META),"files":files,"bots":self.hub.store.bots().unwrap().iter().map(|r| (&r.config.id,&r.config.skills)).collect::<Vec<_>>()})
    }
}
impl Drop for ScopeFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

// SQL expandido somente desta conexão sintética: observa os dois SELECTs pessoais do load real.
// O callback não imprime SQL ou dados e é removido antes de liberar o contador.
struct PersonalSqlTrace<'a> {
    store: &'a Store,
    reads: Box<std::sync::atomic::AtomicUsize>,
}
unsafe extern "C" fn scope_sql_trace(
    _: u32,
    context: *mut std::ffi::c_void,
    stmt: *mut std::ffi::c_void,
    _: *mut std::ffi::c_void,
) -> i32 {
    use rusqlite::ffi;
    // SAFETY: SQLite fornece o statement vivo no evento STMT; context é o Box estável do guard.
    unsafe {
        let sql = ffi::sqlite3_expanded_sql(stmt.cast());
        if !sql.is_null() {
            let sql_text = std::ffi::CStr::from_ptr(sql).to_string_lossy();
            if sql_text.starts_with("SELECT v FROM kv WHERE k=")
                && (sql_text.contains("'skills'") || sql_text.contains("'donna.passiveSkills.v1'"))
            {
                (*context.cast::<std::sync::atomic::AtomicUsize>()).fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            ffi::sqlite3_free(sql.cast());
        }
    }
    0
}
impl<'a> PersonalSqlTrace<'a> {
    fn new(store: &'a Store) -> Self {
        let mut trace = Self { store, reads: Box::new(std::sync::atomic::AtomicUsize::new(0)) };
        // SAFETY: conexão sob mutex; Box vive até o Drop desregistrar o callback.
        let rc = unsafe {
            rusqlite::ffi::sqlite3_trace_v2(
                store.db.locked().handle(),
                rusqlite::ffi::SQLITE_TRACE_STMT,
                Some(scope_sql_trace),
                std::ptr::from_mut(trace.reads.as_mut()).cast(),
            )
        };
        assert_eq!(rc, rusqlite::ffi::SQLITE_OK);
        trace
    }
    fn take(&self) -> usize {
        self.reads.swap(0, std::sync::atomic::Ordering::Relaxed)
    }
}
impl Drop for PersonalSqlTrace<'_> {
    fn drop(&mut self) {
        // SAFETY: conexão viva sob mutex; callback removido antes do Box ser destruído.
        unsafe {
            rusqlite::ffi::sqlite3_trace_v2(self.store.db.locked().handle(), 0, None, std::ptr::null_mut());
        }
    }
}

#[tokio::test]
async fn space_skills_r4_dispatch_additional_refuses_before_personal_query_or_effect() {
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let manual = f
        .call(
            "installSkill",
            json!({"name":"private-sentinel","description":"PERSONAL_SENTINEL_R4","instructions":"PRIVATE_BODY_R4"}),
        )
        .await
        .unwrap();
    let p = registry::find("find-skills").unwrap();
    f.call("installSkill", intent(&p)).await.unwrap();
    let id = &manual["skill"]["id"];
    let before = f.snapshot();
    let trace = PersonalSqlTrace::new(&f.hub.store);
    let control = f.call("skills", json!({})).await.unwrap();
    assert!(control.to_string().contains("PERSONAL_SENTINEL_R4"));
    assert_eq!(trace.take(), 2, "controle positivo observa load real no mesmo Store");
    let mut failures = Vec::new();
    for (method, mut body) in [
        ("skills", json!({})),
        ("marketSkills", json!({})),
        ("skillDetails", json!({"id":id})),
        ("skillDetails", json!({"source":"find-skills"})),
        ("installSkill", intent(&p)),
        ("installSkill", json!({"name":"forbidden","description":"synthetic","instructions":"literal"})),
        ("removeSkill", json!({"id":p.install_id,"expectedDigest":p.metadata.digest})),
    ] {
        body["spaceId"] = f.additional.clone();
        let result = f.call(method, body).await;
        let reads = trace.take();
        let (denied, exposed) = match result {
            Ok(v) => (false, v.to_string().contains("PERSONAL_SENTINEL_R4") || v.to_string().contains("installedId")),
            Err(e) => (e.to_string() == "skills_personal_only", false),
        };
        println!(
            "SPACE_SKILLS_R4 {}",
            json!({"method":method,"additionalDenied":denied,"personalQueries":reads,"personalMetadataExposed":exposed})
        );
        if !denied || reads != 0 || exposed {
            failures.push((method, denied, reads, exposed));
        }
    }
    assert_eq!(f.snapshot(), before, "registro, recibos e todos os bytes permanecem idênticos");
    assert!(failures.is_empty(), "fronteira adicional: {failures:?}");
}

#[tokio::test]
async fn space_skills_r4_dispatch_personal_explicit_and_legacy_keep_original_contracts() {
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let mut failures = Vec::new();
    for explicit in [false, true] {
        let scope = |mut b: Value| {
            if explicit {
                b["spaceId"] = json!(crate::fork::spaces::PERSONAL);
            }
            b
        };
        let manual = f
            .call(
                "installSkill",
                scope(json!({"name":"manual-r4","description":"Original description","instructions":"Literal body"})),
            )
            .await;
        let manual = match manual {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!("manual explicit={explicit}: {e}"));
                f.call(
                    "installSkill",
                    json!({"name":"manual-r4","description":"Original description","instructions":"Literal body"}),
                )
                .await
                .unwrap()
            }
        };
        let id = &manual["skill"]["id"];
        let listed = f.call("skills", scope(json!({}))).await.unwrap();
        assert!(listed["items"].as_array().unwrap().iter().any(|s| s["id"] == *id));
        match f.call("skillDetails", scope(json!({"id":id}))).await {
            Ok(d) => assert!(d["markdown"].as_str().unwrap().contains("Literal body")),
            Err(e) => failures.push(format!("details id explicit={explicit}: {e}")),
        }
        let catalog = f.call("marketSkills", scope(json!({}))).await.unwrap();
        assert_eq!(catalog["items"].as_array().unwrap().len(), 33);
        let p = registry::find("find-skills").unwrap();
        match f.call("skillDetails", scope(json!({"source":"find-skills"}))).await {
            Ok(d) => assert_eq!(d["catalog"]["digest"], p.metadata.digest),
            Err(e) => failures.push(format!("details source explicit={explicit}: {e}")),
        }
        let installed = match f.call("installSkill", scope(intent(&p))).await {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!("catalog install explicit={explicit}: {e}"));
                f.call("installSkill", intent(&p)).await.unwrap()
            }
        };
        assert_eq!(installed["skill"]["receipt"]["digest"], p.metadata.digest);
        match f.call("installSkill", scope(intent(&p))).await {
            Ok(v) => assert!(v["alreadyInstalled"].as_bool().unwrap()),
            Err(e) => failures.push(format!("idempotence explicit={explicit}: {e}")),
        }
        assert!(f.call("removeSkill", scope(json!({"id":p.install_id,"expectedDigest":"wrong"}))).await.is_err());
        if let Err(e) = f.call("removeSkill", scope(r6_captured(&installed["skill"]))).await {
            failures.push(format!("remove catalog explicit={explicit}: {e}"));
            f.call("removeSkill", r6_captured(&installed["skill"])).await.unwrap();
        }
        if let Err(e) = f.call("removeSkill", scope(r6_captured(&manual["skill"]))).await {
            failures.push(format!("remove manual explicit={explicit}: {e}"));
            f.call("removeSkill", r6_captured(&manual["skill"])).await.unwrap();
        }
        assert_eq!(f.call("skills", scope(json!({}))).await.unwrap()["items"], json!([]));
        println!(
            "SPACE_SKILLS_R4 {}",
            json!({"personalExplicit":explicit,"allSevenShapes":failures.is_empty(),"idempotenceAndDigest":true})
        );
    }
    assert!(failures.is_empty(), "compatibilidade pessoal: {failures:?}");
}

#[tokio::test]
async fn space_skills_r4_dispatch_invalid_context_unknown_fields_and_no_grant_stay_closed() {
    use crate::{
        api::{devices::Caller, dispatch},
        store::{BotConfig, Scope},
    };
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    for (id, space) in [
        ("personal-bot", json!(crate::fork::spaces::PERSONAL)),
        ("personal-peer", json!(crate::fork::spaces::PERSONAL)),
        ("other-bot", f.additional.clone()),
        ("deleted-bot", json!(crate::fork::spaces::PERSONAL)),
    ] {
        let cfg: BotConfig =
            serde_json::from_value(json!({"id":id,"name":id,"backend":"codex","cwd":"/tmp","spaceId":space})).unwrap();
        f.hub.store.save_bot(&cfg).unwrap();
    }
    f.hub.store.db.locked().execute("UPDATE bots SET deleted=1 WHERE id='deleted-bot'", []).unwrap();
    let before = f.snapshot();
    let trace = PersonalSqlTrace::new(&f.hub.store);
    let mut failures = Vec::new();
    let p = registry::find("find-skills").unwrap();
    for (method, body) in [
        ("skills", json!({"spaceId":false})),
        ("skills", json!({"spaceId":null})),
        ("skills", json!({"spaceId":"missing"})),
        ("skills", json!({"hubId":"other-hub"})),
        ("skills", json!({"spaceId":crate::fork::spaces::PERSONAL,"botId":"other-bot"})),
        (
            "skills",
            json!({"spaceId":crate::fork::spaces::PERSONAL,"conversationId":"other-bot","botId":"personal-bot"}),
        ),
        ("skills", json!({"spaceId":crate::fork::spaces::PERSONAL,"conversationId":"personal-bot"})),
        (
            "skills",
            json!({"spaceId":crate::fork::spaces::PERSONAL,"conversationId":"personal-bot","botId":"personal-peer"}),
        ),
        // R3: `skills {botId}` de bot vivo do Pessoal passou a ser leitura válida (teste positivo próprio abaixo).
        ("skills", json!({"spaceId":crate::fork::spaces::PERSONAL,"botId":"deleted-bot"})),
        ("skills", json!({"spaceId":crate::fork::spaces::PERSONAL,"botId":"missing-bot"})),
        ("skills", json!({"spaceId":crate::fork::spaces::PERSONAL,"botId":"personal-bot","grant":"owner"})),
        ("skills", json!({"spaceId":crate::fork::spaces::PERSONAL,"validatedPersonal":true})),
        ("marketSkills", json!({"spaceId":crate::fork::spaces::PERSONAL,"grant":"owner"})),
        ("marketSkills", json!({"spaceId":crate::fork::spaces::PERSONAL,"botId":"personal-bot"})),
        ("skillDetails", json!({"spaceId":crate::fork::spaces::PERSONAL,"id":"missing","source":"find-skills"})),
        ("skillDetails", json!({"spaceId":crate::fork::spaces::PERSONAL,"id":42})),
        (
            "installSkill",
            json!({"spaceId":crate::fork::spaces::PERSONAL,"catalogId":"find-skills","expectedDigest":p.metadata.digest,"approved":true}),
        ),
        (
            "installSkill",
            json!({"spaceId":crate::fork::spaces::PERSONAL,"name":"forged","description":"literal","instructions":"body","caller":"local"}),
        ),
        ("removeSkill", json!({"spaceId":crate::fork::spaces::PERSONAL,"id":"missing","grant":"owner"})),
    ] {
        let result = f.call(method, body).await;
        let reads = trace.take();
        if result.is_ok() || reads != 0 {
            failures.push((method, reads));
        }
    }
    for caller in [
        Caller::Pairing { key: "synthetic".into() },
        Caller::Device { key: "synthetic".into(), scopes: vec![Scope::Screen], autoridade: None },
        Caller::Device { key: "absent".into(), scopes: vec![Scope::Control], autoridade: None },
    ] {
        for method in ["skills", "marketSkills", "skillDetails", "installSkill", "removeSkill"] {
            let result =
                dispatch(&f.hub, &caller, method, json!({"spaceId":crate::fork::spaces::PERSONAL,"id":"missing"}))
                    .await;
            let reads = trace.take();
            if result.is_ok() || reads != 0 {
                failures.push((method, reads));
            }
        }
    }
    assert_eq!(f.snapshot(), before);
    println!("SPACE_SKILLS_R4 {}", json!({"invalidAndUnknownCases":19,"noGrantCases":15,"failures":failures}));
    assert!(failures.is_empty(), "negativas antes de dados: {failures:?}");
}

/// R3: `skills {botId}` de bot vivo do Pessoal lê a lista e a medida do índice pelo dispatch real.
#[tokio::test]
async fn r3_skills_com_bot_do_pessoal_le_lista_e_medida_pelo_dispatch() {
    use crate::store::BotConfig;
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let manual = f
        .call("installSkill", json!({"name":"orçamento-r3","description":"Revisão — ação","instructions":"R3"}))
        .await
        .unwrap();
    let id = manual["skill"]["id"].clone();
    for (bot, space) in [("personal-bot", json!(crate::fork::spaces::PERSONAL)), ("other-bot", f.additional.clone())] {
        let cfg: BotConfig = serde_json::from_value(
            json!({"id":bot,"name":bot,"backend":"codex","cwd":"/tmp","spaceId":space,"skills":[id, "nao-instalada"]}),
        )
        .unwrap();
        f.hub.store.save_bot(&cfg).unwrap();
    }
    let trace = PersonalSqlTrace::new(&f.hub.store);
    let lida = f.call("skills", json!({"spaceId":crate::fork::spaces::PERSONAL,"botId":"personal-bot"})).await.unwrap();
    assert!(trace.take() > 0, "leitura permitida");
    let row = f.hub.store.bot("personal-bot").unwrap().unwrap();
    let bloco = brief(&f.hub.store, &row.config.skills).unwrap();
    assert_eq!(
        lida["index"],
        json!({"enabled":2,"listed":1,"bytes":bloco.len(),"selectionVersion":row.skills_version.unwrap()})
    );
    let sem_bot = f.call("skills", json!({})).await.unwrap();
    assert_eq!(lida["items"], sem_bot["items"]);
    assert!(sem_bot.get("index").is_none(), "pedido vazio mantém o contrato anterior");
    // A leitura efetiva confere a linha de novo: outro Space ou apagado depois da guarda não medem.
    assert!(list_for_bot(&f.hub.store, "other-bot", || Ok(())).is_err());
    f.hub.store.db.locked().execute("UPDATE bots SET deleted=1 WHERE id='personal-bot'", []).unwrap();
    assert!(list_for_bot(&f.hub.store, "personal-bot", || Ok(())).is_err());
}

#[tokio::test]
async fn space_skills_r4_dispatch_revocation_during_team_lock_prevents_effect() {
    use crate::{
        api::{devices::Caller, dispatch},
        store::{Device, DeviceSource, Scope},
    };
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let mut device = Device {
        key: "scope-revoked".into(),
        name: "Synthetic".into(),
        platform: "fixture".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: 0,
        last_seen_at: None,
    };
    f.hub.store.put_device(&device).unwrap();
    let before = f.snapshot();
    let trace = PersonalSqlTrace::new(&f.hub.store);
    let held = f.hub.store.team_lock.lock().await;
    let hub = f.hub.clone();
    let mut pending = tokio::spawn(async move {
        dispatch(&hub, &Caller::Device {key:"scope-revoked".into(),scopes:vec![Scope::Control], autoridade: None }, "installSkill",
            json!({"spaceId":crate::fork::spaces::PERSONAL,"name":"denied-after-wait","description":"synthetic","instructions":"literal"})).await
    });
    assert!(tokio::time::timeout(std::time::Duration::from_millis(50), &mut pending).await.is_err());
    device.scopes = vec![Scope::Screen];
    f.hub.store.put_device(&device).unwrap();
    drop(held);
    let err = pending.await.unwrap().unwrap_err();
    assert!(err.is::<crate::api::devices::Forbidden>(), "{err}");
    assert_eq!(trace.take(), 0);
    assert_eq!(f.snapshot(), before);
}

#[tokio::test]
async fn space_skills_r4_http_headers_and_context_precede_data() {
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    f.call(
        "installSkill",
        json!({"name":"http-sentinel","description":"HTTP_PERSONAL_SENTINEL","instructions":"literal"}),
    )
    .await
    .unwrap();
    let trace = PersonalSqlTrace::new(&f.hub.store);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = crate::api::router(f.hub.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
    });
    let client = reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(5)).build().unwrap();
    for token in [None, Some("invalid-synthetic")] {
        let mut request = client.post(format!("{base}/api/skills")).body("{invalid-json");
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        assert_eq!(request.send().await.unwrap().status(), 401);
        assert_eq!(trace.take(), 0, "header negado antes de parser/consulta");
    }
    let personal = client
        .post(format!("{base}/api/skills"))
        .bearer_auth("synthetic-only")
        .json(&json!({"spaceId":crate::fork::spaces::PERSONAL,"hubId":f.hub.host_id}))
        .send()
        .await
        .unwrap();
    assert_eq!(personal.status(), 200);
    assert!(personal.text().await.unwrap().contains("HTTP_PERSONAL_SENTINEL"));
    assert_eq!(trace.take(), 2);
    let response = client
        .post(format!("{base}/api/skills"))
        .bearer_auth("synthetic-only")
        .json(&json!({"spaceId":f.additional}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    let reads = trace.take();
    server.abort();
    let _ = server.await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body, r#"{"error":"skills_personal_only"}"#);
    assert_eq!(reads, 0);
}

#[tokio::test]
async fn installation_r6_same_digest_catalog_aba_is_preserved() {
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let p = registry::find("frontend-design").unwrap();
    let first = f.call("installSkill", intent(&p)).await.unwrap();
    let captured = r6_captured(&first["skill"]);
    f.call("removeSkill", captured.clone()).await.unwrap();
    let second = f.call("installSkill", intent(&p)).await.unwrap();
    assert_eq!(first["skill"]["receipt"], second["skill"]["receipt"]);
    let before = f.snapshot();
    let result = f.call("removeSkill", captured.clone()).await;
    println!(
        "INSTALLATION_R6 {}",
        json!({"case":"same_digest_catalog","captured":captured,"first":first,"second":second,"rejected":result.is_err(),"preserved":f.snapshot()==before})
    );
    assert!(result.is_err(), "digest igual deixou apagar a nova encarnação");
    assert_eq!(f.snapshot(), before);
    f.call("removeSkill", r6_captured(&second["skill"])).await.unwrap();
}

fn r6_select(store: &Store, id: &str) {
    let cfg: crate::store::BotConfig = serde_json::from_value(
        json!({"id":"r6-bot","name":"Synthetic","backend":"fixture","command":"true","cwd":"/tmp","skills":[id]}),
    )
    .unwrap();
    store.save_bot(&cfg).unwrap();
}

#[tokio::test]
async fn installation_r6_manual_same_metadata_new_body_and_list_detail_coherence() {
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let a = f
        .call("installSkill", json!({"name":"manual-r6","description":"identical","instructions":"BODY_A"}))
        .await
        .unwrap();
    let captured = r6_captured(&a["skill"]);
    let id = a["skill"]["id"].as_str().unwrap();
    for _ in 0..2 {
        let listed = f.call("skills", json!({})).await.unwrap();
        let d = f.call("skillDetails", json!({"id":id})).await.unwrap();
        assert_eq!(listed["items"][0], a["skill"]);
        assert_eq!(d["installation"], a["skill"]);
        assert_eq!(d["markdown"], "BODY_A");
    }
    f.call("removeSkill", captured.clone()).await.unwrap();
    let b = f
        .call("installSkill", json!({"name":"manual-r6","description":"identical","instructions":"BODY_B"}))
        .await
        .unwrap();
    assert_eq!(a["skill"]["id"], b["skill"]["id"]);
    assert_ne!(a["skill"]["installationId"], b["skill"]["installationId"]);
    r6_select(&f.hub.store, id);
    let before = f.snapshot();
    assert_eq!(f.call("removeSkill", captured).await.unwrap_err().to_string(), installation::STALE);
    assert_eq!(f.snapshot(), before);
    let d = f.call("skillDetails", json!({"id":id})).await.unwrap();
    assert_eq!(d["installation"], b["skill"]);
    assert_eq!(d["markdown"], "BODY_B");
    assert_ne!(
        d["installation"]["installationId"], a["skill"]["installationId"],
        "lista antiga não corresponde ao detalhe novo"
    );
    f.call("removeSkill", r6_captured(&b["skill"])).await.unwrap();
    assert_eq!(f.hub.store.bot("r6-bot").unwrap().unwrap().config.skills, Vec::<String>::new());
}

#[tokio::test]
async fn installation_r6_strict_shape_receipts_absence_and_context_guards() {
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let p = registry::find("frontend-design").unwrap();
    let installed = f.call("installSkill", intent(&p)).await.unwrap();
    r6_select(&f.hub.store, &p.install_id);
    let captured = r6_captured(&installed["skill"]);
    let before = f.snapshot();
    let trace = PersonalSqlTrace::new(&f.hub.store);
    for body in [
        json!({"id":p.install_id}),
        json!({"id":p.install_id,"expectedDigest":p.metadata.digest}),
        json!({"id":p.install_id,"expectedInstallationId":captured["expectedInstallationId"]}),
        json!({"id":p.install_id,"expectedInstallationId":null,"expectedReceipt":{"kind":"none"}}),
        json!({"id":p.install_id,"expectedInstallationId":"invented","expectedReceipt":{"kind":"none"}}),
        json!({"id":p.install_id,"expectedInstallationId":captured["expectedInstallationId"],"expectedReceipt":null}),
        json!({"id":p.install_id,"expectedInstallationId":captured["expectedInstallationId"],"expectedReceipt":{"kind":"none","digest":p.metadata.digest}}),
        json!({"id":p.install_id,"expectedInstallationId":captured["expectedInstallationId"],"expectedReceipt":{"kind":"catalog","digest":p.metadata.digest}}),
    ] {
        assert_eq!(f.call("removeSkill", body).await.unwrap_err().to_string(), installation::UNSUPPORTED);
        assert_eq!(trace.take(), 0, "condição insuficiente não consulta catálogo pessoal");
    }
    for expected_receipt in [
        json!({"kind":"none"}),
        json!({"kind":"catalog","catalogId":"wrong","digest":p.metadata.digest}),
        json!({"kind":"catalog","catalogId":p.metadata.id,"digest":"0".repeat(64)}),
    ] {
        let mut wrong = captured.clone();
        wrong["expectedReceipt"] = expected_receipt;
        assert_eq!(f.call("removeSkill", wrong).await.unwrap_err().to_string(), installation::STALE);
    }
    trace.take();
    for mut body in [captured.clone(), json!({})] {
        body["spaceId"] = f.additional.clone();
        assert_eq!(f.call("removeSkill", body).await.unwrap_err().to_string(), "skills_personal_only");
        assert_eq!(trace.take(), 0);
    }
    let mut other_hub = captured.clone();
    other_hub["hubId"] = json!("wrong");
    assert!(f.call("removeSkill", other_hub).await.is_err());
    assert_eq!(trace.take(), 0);
    let mut forged = captured.clone();
    forged["grant"] = json!("owner");
    assert!(f.call("removeSkill", forged).await.is_err());
    assert_eq!(trace.take(), 0);
    assert_eq!(f.snapshot(), before);
    f.call("removeSkill", captured.clone()).await.unwrap();
    f.call("removeSkill", captured.clone()).await.unwrap();
    let reopened = Store::open(&f.root.join("fixture.db")).unwrap();
    assert_eq!(list(&reopened).unwrap()["items"], json!([]));
    let new = f.call("installSkill", intent(&p)).await.unwrap();
    assert_ne!(new["skill"]["installationId"], installed["skill"]["installationId"]);
    assert_eq!(list(&reopened).unwrap()["items"][0], new["skill"]);
    let before = f.snapshot();
    assert_eq!(f.call("removeSkill", captured).await.unwrap_err().to_string(), installation::STALE);
    assert_eq!(f.snapshot(), before);
    assert_eq!(
        f.call("installSkill", intent(&p)).await.unwrap()["skill"],
        new["skill"],
        "retry presente conserva encarnação"
    );
}

#[tokio::test]
async fn installation_r6_waiting_dispatch_revalidates_replacement_and_revoked_caller() {
    use crate::api::{devices::Caller, dispatch};
    use crate::store::{Device, DeviceSource, Scope};
    use std::{
        future::{Future, poll_fn},
        task::Poll,
    };
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let body = json!({"name":"waiting-r6","description":"same","instructions":"SAME"});
    let first = f.call("installSkill", body.clone()).await.unwrap();
    let captured = r6_captured(&first["skill"]);
    let held = f.hub.store.team_lock.lock().await;
    let mut pending = Box::pin(dispatch(&f.hub, &Caller::Local, "removeSkill", captured.clone()));
    poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    // Escritor que já detém team_lock conclui a troca antes de liberar o pedido pendente.
    super::remove(&f.hub.store, &captured, || Ok(())).unwrap();
    let second = super::custom(&f.hub.store, &body, || Ok(())).unwrap();
    r6_select(&f.hub.store, second["skill"]["id"].as_str().unwrap());
    let before = f.snapshot();
    drop(held);
    assert_eq!(pending.await.unwrap_err().to_string(), installation::STALE);
    assert_eq!(f.snapshot(), before);
    let mut device = Device {
        key: "r6-revoked".into(),
        name: "Synthetic".into(),
        platform: "fixture".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: 0,
        last_seen_at: None,
    };
    f.hub.store.put_device(&device).unwrap();
    // D368: aparelho autenticado pelo transporte, como em produção (traz a autoridade original).
    let (_, caller) = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap();
    let held = f.hub.store.team_lock.lock().await;
    let mut pending = Box::pin(dispatch(&f.hub, &caller, "removeSkill", r6_captured(&second["skill"])));
    poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    device.scopes = vec![Scope::Screen];
    f.hub.store.put_device(&device).unwrap();
    let trace = PersonalSqlTrace::new(&f.hub.store);
    drop(held);
    assert!(pending.await.unwrap_err().is::<crate::api::devices::Forbidden>());
    assert_eq!(trace.take(), 0);
    assert_eq!(f.snapshot(), before);
}

#[test]
fn installation_r6_legacy_migration_is_state_only_atomic_stable_and_preserves_receipts() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let f = Fixture::new();
    let s = &f.0;
    let (p, r) = captured_fixture(s, "find-skills");
    r6_select(s, &p.install_id);
    let before_skills = s.kv_get("skills");
    let before_state: Value = serde_json::from_str(&s.kv_get(STATE).unwrap()).unwrap();
    let before_files: Vec<_> = p
        .assets
        .iter()
        .map(|a| (a.path.clone(), fs::read(s.skills_root.join(&p.install_id).join(&a.path)).unwrap()))
        .collect();
    let before_receipt = fs::read(s.skills_root.join(&p.install_id).join(RECEIPT)).unwrap();
    s.db.locked().execute_batch("CREATE TRIGGER r6_fail BEFORE INSERT ON kv WHEN NEW.k='donna.passiveSkills.v2' BEGIN SELECT RAISE(ABORT,'r6 fault'); END;").unwrap();
    assert!(list(s).is_err());
    assert_eq!(serde_json::from_str::<Value>(&s.kv_get(STATE).unwrap()).unwrap(), before_state);
    assert_eq!(s.kv_get("skills"), before_skills);
    s.db.locked().execute_batch("DROP TRIGGER r6_fail").unwrap();
    let listed = list(s).unwrap();
    for _ in 0..2 {
        assert_eq!(list(s).unwrap(), listed);
        let detail = details(s, &json!({"id":p.install_id})).unwrap();
        assert_eq!(detail["installation"], listed["items"][0]);
        assert_eq!(detail["receipt"], serde_json::to_value(&r).unwrap());
    }
    assert_eq!(s.kv_get("skills"), before_skills, "não reserializa registro legado");
    let after: Value = serde_json::from_str(&s.kv_get(STATE).unwrap()).unwrap();
    for key in ["seedDone", "receipts", "installed"] {
        assert_eq!(after[key], before_state[key]);
    }
    for (path, bytes) in before_files {
        assert_eq!(fs::read(s.skills_root.join(&p.install_id).join(path)).unwrap(), bytes);
    }
    assert_eq!(fs::read(s.skills_root.join(&p.install_id).join(RECEIPT)).unwrap(), before_receipt);
    assert_eq!(s.bot("r6-bot").unwrap().unwrap().config.skills, [p.install_id]);
}

#[tokio::test]
async fn installation_r6_sqlfault_preserves_registration_identity_bytes_and_selection() {
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let body = json!({"name":"sql-r6","description":"fixture","instructions":"LITERAL"});
    let installed = f.call("installSkill", body).await.unwrap();
    r6_select(&f.hub.store, installed["skill"]["id"].as_str().unwrap());
    let before = f.snapshot();
    f.hub.store.db.locked().execute_batch("CREATE TRIGGER r6_fail BEFORE INSERT ON kv WHEN NEW.k='donna.passiveSkills.v1' BEGIN SELECT RAISE(ABORT,'r6 fault'); END;").unwrap();
    assert!(f.call("removeSkill", r6_captured(&installed["skill"])).await.is_err());
    assert_eq!(f.snapshot(), before);
    assert!(
        f.call("installSkill", json!({"name":"sql-new-r6","description":"fixture","instructions":"NEW"}))
            .await
            .is_err()
    );
    assert_eq!(f.snapshot(), before, "registro e sequência falham juntos; publicação própria limpa");
    f.hub.store.db.locked().execute_batch("DROP TRIGGER r6_fail").unwrap();
    assert_eq!(f.call("skills", json!({})).await.unwrap()["items"][0], installed["skill"]);
}

#[cfg(unix)]
#[tokio::test]
async fn installation_r6_partial_fs_cleanup_does_not_replay_or_remove_new_incarnation() {
    use std::os::unix::fs::PermissionsExt;
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let body = json!({"name":"fs-r6","description":"fixture","instructions":"LITERAL"});
    let first = f.call("installSkill", body.clone()).await.unwrap();
    let id = first["skill"]["id"].as_str().unwrap();
    r6_select(&f.hub.store, id);
    let captured = r6_captured(&first["skill"]);
    let dest = f.hub.store.skills_root.join(id);
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o500)).unwrap();
    let result = f.call("removeSkill", captured.clone()).await;
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result.is_err(), "limpeza de diretório sem escrita deve falhar no usuário donnabot");
    assert!(dest.join("SKILL.md").exists());
    assert_eq!(f.call("skills", json!({})).await.unwrap()["items"], json!([]));
    assert_eq!(f.hub.store.bot("r6-bot").unwrap().unwrap().config.skills, Vec::<String>::new());
    let before = f.snapshot();
    f.call("removeSkill", captured.clone()).await.unwrap();
    assert_eq!(f.snapshot(), before, "retry ausente não limpa órfão");
    assert!(f.call("installSkill", body.clone()).await.is_err(), "manual órfã não é adotada");
    fs::remove_dir_all(&dest).unwrap(); // Somente a fixture própria, após comprovar a recusa.
    let second = f.call("installSkill", body).await.unwrap();
    assert_ne!(first["skill"]["installationId"], second["skill"]["installationId"]);
    let before = f.snapshot();
    assert_eq!(f.call("removeSkill", captured).await.unwrap_err().to_string(), installation::STALE);
    assert_eq!(f.snapshot(), before);
}

#[tokio::test]
async fn installation_r6_external_body_record_and_irregular_legacy_fail_closed() {
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let installed = f
        .call("installSkill", json!({"name":"body-r6","description":"fixture","instructions":"ORIGINAL"}))
        .await
        .unwrap();
    let id = installed["skill"]["id"].as_str().unwrap();
    let path = f.hub.store.skills_root.join(id).join("SKILL.md");
    let original = fs::read(&path).unwrap();
    fs::write(&path, "CHANGED_EXTERNAL").unwrap();
    let listed = f.call("skills", json!({})).await.unwrap();
    assert_eq!(listed["items"][0]["installationId"], installed["skill"]["installationId"]);
    assert_eq!(listed["items"][0]["contentStatus"], "changed");
    let before = f.snapshot();
    assert_eq!(f.call("skillDetails", json!({"id":id})).await.unwrap_err().to_string(), installation::CHANGED);
    // D306: remover a alterada é recuperação permitida (entrega_tests); aqui só a leitura fica fechada.
    assert_eq!(f.snapshot(), before);
    fs::write(&path, original).unwrap();
    let skills = f.hub.store.kv_get("skills").unwrap();
    let mut altered: Value = serde_json::from_str(&skills).unwrap();
    altered[0]["description"] = json!("external metadata");
    f.hub.store.kv_set("skills", &altered.to_string()).unwrap();
    // Registro trocado por escritor antigo não herda a prova da manual anterior.
    assert_eq!(f.call("skillDetails", json!({"id":id})).await.unwrap_err().to_string(), installation::UNAVAILABLE);
    let reattested = f.call("skills", json!({})).await.unwrap()["items"][0].clone();
    assert_eq!(reattested["description"], "external metadata");
    assert_ne!(reattested["installationId"], installed["skill"]["installationId"]);
    assert_eq!(reattested["contentStatus"], "unavailable");
    assert_eq!(
        f.call("removeSkill", r6_captured(&installed["skill"])).await.unwrap_err().to_string(),
        installation::STALE
    );
    f.hub.store.kv_set("skills", &skills).unwrap();
    let current = f.call("skills", json!({})).await.unwrap()["items"][0].clone();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert_eq!(f.call("skillDetails", json!({"id":id})).await.unwrap_err().to_string(), installation::UNAVAILABLE);
    // D306: ilegível também se remove pela encarnação vista; só a pasta da própria instalação sai.
    f.call("removeSkill", r6_captured(&current)).await.unwrap();
    assert_eq!(f.call("skills", json!({})).await.unwrap()["items"], json!([]));
    assert!(fs::symlink_metadata(f.hub.store.skills_root.join(id)).is_err());
    let legacy = Fixture::new();
    legacy.0.kv_set("skills", r#"[{"id":"missing","name":"Legacy","source":"custom"}]"#).unwrap();
    let first = list(&legacy.0).unwrap();
    assert_eq!(first["items"][0]["contentStatus"], "unavailable");
    fs::create_dir(legacy.0.skills_root.join("missing")).unwrap();
    fs::write(legacy.0.skills_root.join("missing/SKILL.md"), "LATER").unwrap();
    assert_eq!(list(&legacy.0).unwrap(), first, "não recaptura baseline por leitura");
    assert_eq!(details(&legacy.0, &json!({"id":"missing"})).unwrap_err().to_string(), installation::UNAVAILABLE);
}

// Sinaliza a segunda validação original de Space no dispatch, antes do lock do domínio.
// O contador pessoal comprova que nada foi carregado/projetado após a revogação.
struct R6ReadTrace {
    spaces: std::sync::atomic::AtomicUsize,
    personal: std::sync::atomic::AtomicUsize,
    ready: std::sync::mpsc::SyncSender<()>,
}
unsafe extern "C" fn r6_read_trace(
    _: u32,
    context: *mut std::ffi::c_void,
    stmt: *mut std::ffi::c_void,
    _: *mut std::ffi::c_void,
) -> i32 {
    // SAFETY: callback só existe enquanto o Box está vivo e registrado na conexão fixture.
    unsafe {
        let sql = rusqlite::ffi::sqlite3_expanded_sql(stmt.cast());
        if !sql.is_null() {
            let text = std::ffi::CStr::from_ptr(sql).to_string_lossy();
            let gate = &*context.cast::<R6ReadTrace>();
            if text.starts_with("SELECT v FROM kv WHERE k=")
                && text.contains("'donna.spaces.v1'")
                && gate.spaces.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1
            {
                let _ = gate.ready.try_send(());
            }
            if text.contains("'skills'") || text.contains("donna.passiveSkills") || text.contains("d166_skill_v2_") {
                gate.personal.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            rusqlite::ffi::sqlite3_free(sql.cast());
        }
    }
    0
}

#[tokio::test]
async fn installation_r6_read_wrappers_revalidate_caller_and_original_context_after_lock() {
    use crate::{
        api::devices::Caller,
        store::{Device, DeviceSource, Scope},
    };
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let installed = f
        .call(
            "installSkill",
            json!({"name":"read-sentinel-r6","description":"PRIVATE_SENTINEL","instructions":"PRIVATE_LITERAL"}),
        )
        .await
        .unwrap();
    let mut device = Device {
        key: "r6-reader".into(),
        name: "Synthetic".into(),
        platform: "fixture".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: 0,
        last_seen_at: None,
    };
    let original_spaces = f.hub.store.kv_get("donna.spaces.v1").unwrap();
    for action in ["revoke", "context-fault"] {
        for (method, mut body) in [
            ("skills", json!({})),
            ("marketSkills", json!({})),
            ("skillDetails", json!({"id":installed["skill"]["id"]})),
            ("skillDetails", json!({"source":"find-skills"})),
        ] {
            device.scopes = vec![Scope::Control];
            f.hub.store.put_device(&device).unwrap();
            let caller = Caller::Device { key: device.key.clone(), scopes: vec![Scope::Control], autoridade: None };
            require_current(&f.hub, &caller, method).unwrap();
            body["spaceId"] = json!(crate::fork::spaces::PERSONAL);
            body["skillProtocol"] = json!(2);
            let before = f.snapshot();
            let held = MUTATION.locked();
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            let mut trace = Box::new(R6ReadTrace {
                spaces: std::sync::atomic::AtomicUsize::new(0),
                personal: std::sync::atomic::AtomicUsize::new(0),
                ready: tx,
            });
            // SAFETY: Box estável até desregistrar o callback depois de join.
            unsafe {
                assert_eq!(
                    rusqlite::ffi::sqlite3_trace_v2(
                        f.hub.store.db.locked().handle(),
                        rusqlite::ffi::SQLITE_TRACE_STMT,
                        Some(r6_read_trace),
                        std::ptr::from_mut(trace.as_mut()).cast()
                    ),
                    rusqlite::ffi::SQLITE_OK
                );
            }
            let result = std::thread::scope(|scope| {
                let pending = scope.spawn(|| {
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap()
                        .block_on(crate::api::dispatch(&f.hub, &caller, method, body))
                });
                rx.recv_timeout(std::time::Duration::from_secs(60)).unwrap();
                // MUTATION segue retido: nenhuma leitura do domínio pode passar ainda.
                if action == "revoke" {
                    device.scopes = vec![Scope::Screen];
                    f.hub.store.put_device(&device).unwrap();
                } else {
                    // Fault sintético de contexto, não é uma transição suportada de Pessoal.
                    let mut invalid: Value = serde_json::from_str(&original_spaces).unwrap();
                    invalid["spaces"] = json!([]);
                    f.hub.store.kv_set("donna.spaces.v1", &invalid.to_string()).unwrap();
                }
                drop(held);
                pending.join().unwrap()
            });
            // SAFETY: conexão fixture viva; callback removido antes do Box.
            unsafe {
                rusqlite::ffi::sqlite3_trace_v2(f.hub.store.db.locked().handle(), 0, None, std::ptr::null_mut());
            }
            if action == "revoke" {
                assert!(result.unwrap_err().is::<crate::api::devices::Forbidden>());
            } else {
                assert_eq!(result.unwrap_err().to_string(), "space_not_found");
            }
            assert_eq!(trace.personal.load(std::sync::atomic::Ordering::SeqCst), 0);
            f.hub.store.kv_set("donna.spaces.v1", &original_spaces).unwrap();
            assert_eq!(f.snapshot(), before);
            println!(
                "INSTALLATION_R6_READ {}",
                json!({"method":method,"action":action,"personalQueries":0,"projected":false})
            );
        }
    }
}

#[test]
fn installation_r6_legacy_receipt_prevents_rebaselining_an_already_changed_body() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let fixture = Fixture::new();
    let store = &fixture.0;
    let (package, receipt) = captured_fixture(store, "frontend-design");
    let path = store.skills_root.join(&package.install_id).join("SKILL.md");
    let original = fs::read(&path).unwrap();
    fs::write(&path, "SYNTHETIC_CHANGED_BEFORE_MIGRATION").unwrap();
    let registry = store.kv_get("skills");
    let disk_receipt = fs::read(store.skills_root.join(&package.install_id).join(RECEIPT)).unwrap();
    let listing = list(store).unwrap();
    let skill = &listing["items"][0];
    assert_eq!(
        skill["contentStatus"], "changed",
        "baseline deve usar hash capturado no recibo, nunca legitimar corpo adulterado"
    );
    assert_eq!(details(store, &json!({"id":package.install_id})).unwrap_err().to_string(), installation::CHANGED);
    let state = store.kv_get(STATE);
    // D306: remover a alterada passou a ser permitido (entrega_tests); a leitura segue fechada.
    assert_eq!(store.kv_get(STATE), state);
    assert_eq!(store.kv_get("skills"), registry);
    assert_eq!(load(store).unwrap().1.receipts.get(&receipt.digest), Some(&receipt));
    assert_eq!(fs::read(store.skills_root.join(&package.install_id).join(RECEIPT)).unwrap(), disk_receipt);
    assert_eq!(fs::read_to_string(&path).unwrap(), "SYNTHETIC_CHANGED_BEFORE_MIGRATION");
    fs::write(&path, original).unwrap();
    let current = list(store).unwrap();
    assert_eq!(current["items"][0]["installationId"], skill["installationId"]);
    assert_eq!(current["items"][0]["contentStatus"], "verified");
    assert_eq!(
        details(store, &json!({"id":package.install_id})).unwrap()["receipt"],
        serde_json::to_value(receipt).unwrap()
    );
}

// Escritor legado sintético: o MESMO registro/Store e transação v1, sem protocolo forte.
fn r7_legacy_write(store: &Store, skills: &[Skill], state: &Value) {
    let mut db = store.db.locked();
    let tx = db.transaction().unwrap();
    put(&tx, "skills", &skills).unwrap();
    put(&tx, STATE, &json!({"seedDone":state["seedDone"],"receipts":state["receipts"],"installed":state["installed"]}))
        .unwrap();
    tx.commit().unwrap();
}
fn r7_metadata(store: &Store) -> Value {
    serde_json::from_str(&store.kv_get(storage::META).unwrap()).unwrap()
}
fn r7_guards(store: &Store) -> Vec<(String, String)> {
    let db = store.db.locked();
    let mut stmt = db
        .prepare(
            "SELECT name,sql FROM sqlite_master WHERE type='trigger' AND name LIKE 'd166_skill_v2_%' ORDER BY name",
        )
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap()
}
fn r7_old_loader(store: &Store) -> State {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct OldState {
        seed_done: bool,
        receipts: BTreeMap<String, Receipt>,
        installed: BTreeMap<String, String>,
    }
    let old: OldState = serde_json::from_str(&store.kv_get(STATE).unwrap()).unwrap();
    State { seed_done: old.seed_done, receipts: old.receipts, installed: old.installed, ..State::default() }
}

#[tokio::test]
async fn recovery_r7_old_writer_identical_aba_invalidates_before_projection_and_preserves_current_data() {
    let _isolamento = TEST_ISOLATION.lock().await;
    for catalog in [false, true] {
        let f = ScopeFixture::new().await;
        let first = f
            .call(
                "installSkill",
                if catalog {
                    intent(&registry::find("frontend-design").unwrap())
                } else {
                    json!({"name":"frontend-design","description":"same","instructions":"IDENTICAL"})
                },
            )
            .await
            .unwrap();
        let store = &f.hub.store;
        let id = first["skill"]["id"].as_str().unwrap();
        r6_select(store, id);
        let all = load(store).unwrap().0;
        let oldstate: Value = serde_json::from_str(&store.kv_get(STATE).unwrap()).unwrap();
        // Mesmo bytes/corpo/recibo após remove/reinstall: o hash final não denuncia ABA.
        r7_legacy_write(store, &[], &json!({"seedDone":true,"receipts":oldstate["receipts"],"installed":{}}));
        let dir = store.skills_root.join(id);
        let private = store.skills_root.join(".r7-aba-private");
        fs::rename(&dir, &private).unwrap();
        fs::rename(&private, &dir).unwrap();
        r7_legacy_write(store, &all, &oldstate);
        assert_eq!(r7_metadata(store)["legacyWriteObserved"], true);
        r7_old_loader(store); // O loader estrito antigo aceita o v1 atual.
        let before = f.snapshot();
        let err = f.call("removeSkill", r6_captured(&first["skill"])).await.unwrap_err();
        assert_eq!(err.to_string(), installation::STALE);
        let after = f.snapshot();
        for key in ["registry", "state", "files", "bots"] {
            assert_eq!(after[key], before[key]);
        }
        let current = f.call("skills", json!({"skillProtocol":2})).await.unwrap();
        assert_eq!(current["skillProtocol"], 2);
        assert_ne!(current["items"][0]["installationId"], first["skill"]["installationId"]);
        assert_eq!(current["items"][0]["contentStatus"], "verified");
        assert_eq!(r7_metadata(store)["legacyWriteObserved"], false);
        assert_eq!(f.call("skills", json!({})).await.unwrap(), current);
        assert_eq!(f.call("removeSkill", r6_captured(&current["items"][0])).await.unwrap()["skillProtocol"], 2);
    }
}

#[tokio::test]
async fn recovery_r7_056_conversion_is_atomic_preserves_ids_and_bootstrap_restart_is_stable() {
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let s = &f.hub.store;
    let p = registry::find("find-skills").unwrap();
    let created = f.call("installSkill", intent(&p)).await.unwrap();
    r6_select(s, &p.install_id);
    let (_, state) = load(s).unwrap();
    let raw056 = serde_json::to_string(&state).unwrap();
    {
        let mut db = s.db.locked();
        let tx = db.transaction().unwrap();
        tx.execute_batch(
            "DROP TRIGGER d166_skill_v2_insert; DROP TRIGGER d166_skill_v2_update; DROP TRIGGER d166_skill_v2_delete;",
        )
        .unwrap();
        tx.execute("DELETE FROM kv WHERE k=?1", [storage::META]).unwrap();
        put(&tx, STATE, &state).unwrap();
        tx.commit().unwrap();
    }
    let before = f.snapshot();
    s.db.locked().execute_batch("CREATE TRIGGER r7_fault BEFORE INSERT ON kv WHEN NEW.k='donna.passiveSkills.v2' BEGIN SELECT RAISE(ABORT,'r7'); END;").unwrap();
    assert!(seed(s).is_err());
    assert_eq!(s.kv_get(STATE).unwrap(), raw056);
    assert_eq!(f.snapshot(), before);
    assert!(r7_guards(s).is_empty(), "DDL e migração não podem ficar parciais");
    s.db.locked().execute_batch("DROP TRIGGER r7_fault").unwrap();
    seed(s).unwrap();
    assert!(r7_old_loader(s).seed_done);
    assert_eq!(r7_guards(s).len(), 3);
    let listed = f.call("skills", json!({"skillProtocol":2})).await.unwrap();
    assert_eq!(listed["items"][0], created["skill"]);
    assert_eq!(s.kv_get("skills"), before["registry"].as_str().map(str::to_owned));
    assert_eq!(f.snapshot()["files"], before["files"]);
    assert_eq!(f.snapshot()["bots"], before["bots"]);
    let stable = f.snapshot();
    let reopened = Store::open(&f.root.join("fixture.db")).unwrap();
    seed(&reopened).unwrap();
    assert_eq!(list(&reopened).unwrap(), listed);
    assert_eq!(f.snapshot(), stable);
}

#[test]
fn recovery_r7_056_writer_dirty_dual_namespace_and_counter_are_retired_before_seed_returns() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let f = Fixture::new();
    let s = &f.0;
    let first = custom(s, &json!({"name":"same","description":"d","instructions":"SAME"})).unwrap();
    let (all, mut writer056) = load(s).unwrap();
    writer056.seed_done = true;
    writer056.installation_namespace = Some(uuid::Uuid::new_v4().to_string());
    writer056.installation_sequence = 100;
    writer056.installations.clear();
    installation::issue(&all[0], &mut writer056).unwrap();
    let other = serde_json::to_value(&writer056).unwrap()["installations"][&all[0].id]["id"].clone();
    s.kv_set(STATE, &serde_json::to_string(&writer056).unwrap()).unwrap();
    let before = s.kv_get("skills");
    seed(s).unwrap();
    assert_eq!(s.kv_get("skills"), before);
    let listed = list(s).unwrap();
    let new = &listed["items"][0];
    assert_ne!(new["installationId"], first["skill"]["installationId"]);
    assert_ne!(new["installationId"], other);
    assert_eq!(r7_metadata(s)["installationSequence"], 102);
    assert_eq!(r7_metadata(s)["legacyWriteObserved"], false);
    assert!(r7_old_loader(s).seed_done);
    let clean = r7_metadata(s);
    seed(s).unwrap();
    assert_eq!(r7_metadata(s), clean);
    // Dual sem sinal de escritor não ganha interpretação conveniente.
    s.kv_set(STATE, &serde_json::to_string(&writer056).unwrap()).unwrap();
    let mut meta = r7_metadata(s);
    meta["legacyWriteObserved"] = json!(false);
    s.kv_set(storage::META, &meta.to_string()).unwrap();
    let state = s.kv_get(STATE);
    assert!(seed(s).is_err());
    assert_eq!(s.kv_get(STATE), state);
    assert_eq!(r7_metadata(s), meta);
}

#[tokio::test]
async fn recovery_r7_late_marker_second_connection_cas_and_final_put_fault_preserve_everything() {
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let s = &f.hub.store;
    let made = f.call("installSkill", json!({"name":"cas","description":"d","instructions":"SAME"})).await.unwrap();
    r6_select(s, made["skill"]["id"].as_str().unwrap());
    {
        let _domain = MUTATION.locked();
        let _files = root_lock(s).unwrap();
        let (all, state) = installation::load_initialized(s).unwrap();
        let second = rusqlite::Connection::open(f.root.join("fixture.db")).unwrap();
        second.execute("UPDATE kv SET v=v WHERE k='skills'", []).unwrap();
        let before = f.snapshot();
        assert_eq!(persist(s, &all, &state, None).unwrap_err().to_string(), installation::STALE);
        assert_eq!(f.snapshot(), before);
        assert_eq!(r7_metadata(s)["legacyWriteObserved"], true);
    }
    let current = f.call("skills", json!({})).await.unwrap()["items"][0].clone();
    for key in ["skills", STATE, storage::META] {
        let before = f.snapshot();
        s.db.locked()
            .execute_batch(&format!(
                "CREATE TRIGGER r7_fault BEFORE INSERT ON kv WHEN NEW.k='{key}' BEGIN SELECT RAISE(ABORT,'r7'); END;"
            ))
            .unwrap();
        assert!(f.call("removeSkill", r6_captured(&current)).await.is_err());
        assert_eq!(f.snapshot(), before, "registro/IDs/dirty/seleção/FS atômicos: {key}");
        s.db.locked().execute_batch("DROP TRIGGER r7_fault").unwrap();
    }
    s.db.locked().execute_batch("PRAGMA recursive_triggers=ON;").unwrap();
    f.call("removeSkill", r6_captured(&current)).await.unwrap();
    assert_eq!(r7_metadata(s)["legacyWriteObserved"], false);
}

#[test]
fn recovery_r7_guard_gap_reattests_once_altered_or_partial_layout_fails_closed() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let f = Fixture::new();
    let s = &f.0;
    let first = custom(s, &json!({"name":"gap","description":"d","instructions":"SAME"})).unwrap();
    let (old_all, old_state) = {
        let _guard = MUTATION.locked();
        let _file = root_lock(s).unwrap();
        installation::load_initialized(s).unwrap()
    };
    let old_meta = r7_metadata(s);
    s.db.locked().execute_batch("DROP TRIGGER d166_skill_v2_update").unwrap();
    assert_eq!(persist(s, &old_all, &old_state, None).unwrap_err().to_string(), installation::STALE);
    assert_eq!(r7_metadata(s), old_meta, "CAS não limpa/repara guard de snapshot vencido");
    let new = list(s).unwrap();
    assert_ne!(new["items"][0]["installationId"], first["skill"]["installationId"]);
    assert_eq!(list(s).unwrap(), new);
    assert_eq!(r7_guards(s).len(), 3);
    let (all, state) = {
        let _domain = MUTATION.locked();
        let _files = root_lock(s).unwrap();
        installation::load_initialized(s).unwrap()
    };
    s.db.locked().execute_batch("DROP TRIGGER d166_skill_v2_update; CREATE TRIGGER d166_skill_v2_update AFTER UPDATE ON kv BEGIN SELECT 1; END;").unwrap();
    let defs = r7_guards(s);
    let meta = r7_metadata(s);
    assert!(list(s).is_err());
    assert!(persist(s, &all, &state, None).is_err());
    assert_eq!(r7_guards(s), defs);
    assert_eq!(r7_metadata(s), meta);
    s.db.locked().execute_batch("DROP TRIGGER d166_skill_v2_update").unwrap();
    s.kv_delete(storage::META).unwrap();
    assert!(list(s).is_err(), "guards sem metadado são estado parcial");
    assert!(s.kv_get(storage::META).is_none());
    let overflow = Fixture::new();
    let s = &overflow.0;
    custom(s, &json!({"name":"overflow","description":"d","instructions":"BODY"})).unwrap();
    let mut meta = r7_metadata(s);
    meta["installationSequence"] = json!(u64::MAX);
    meta["legacyWriteObserved"] = json!(true);
    s.kv_set(storage::META, &meta.to_string()).unwrap();
    let defs = r7_guards(s);
    let registry = s.kv_get("skills");
    let legacy = s.kv_get(STATE);
    assert!(list(s).is_err());
    assert_eq!(r7_metadata(s), meta);
    assert_eq!(r7_guards(s), defs);
    assert_eq!(s.kv_get("skills"), registry);
    assert_eq!(s.kv_get(STATE), legacy);
}

// Inclui as três keys e sqlite_master/DDL: erro de contexto/Caller/shape não consulta nem migra.
struct R7DomainTrace<'a> {
    store: &'a Store,
    count: Box<std::sync::atomic::AtomicUsize>,
}
unsafe extern "C" fn r7_sql_trace(
    _: u32,
    ctx: *mut std::ffi::c_void,
    stmt: *mut std::ffi::c_void,
    _: *mut std::ffi::c_void,
) -> i32 {
    // SAFETY: SQLite fornece statement vivo; contador Box permanece registrado até Drop.
    unsafe {
        let sql = rusqlite::ffi::sqlite3_expanded_sql(stmt.cast());
        if !sql.is_null() {
            let text = std::ffi::CStr::from_ptr(sql).to_string_lossy();
            if text.contains("'skills'") || text.contains("donna.passiveSkills") || text.contains("d166_skill_v2_") {
                (*ctx.cast::<std::sync::atomic::AtomicUsize>()).fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            rusqlite::ffi::sqlite3_free(sql.cast());
        }
    }
    0
}
impl<'a> R7DomainTrace<'a> {
    fn new(store: &'a Store) -> Self {
        let mut out = Self { store, count: Box::default() };
        // SAFETY: Box estável e conexão viva sob mutex até remover o callback em Drop.
        unsafe {
            rusqlite::ffi::sqlite3_trace_v2(
                store.db.locked().handle(),
                rusqlite::ffi::SQLITE_TRACE_STMT,
                Some(r7_sql_trace),
                std::ptr::from_mut(out.count.as_mut()).cast(),
            );
        }
        out
    }
    fn take(self) -> usize {
        self.count.load(std::sync::atomic::Ordering::Relaxed)
    }
}
impl Drop for R7DomainTrace<'_> {
    fn drop(&mut self) {
        // SAFETY: desregistra o callback sob mutex antes de destruir o contador Box.
        unsafe {
            rusqlite::ffi::sqlite3_trace_v2(self.store.db.locked().handle(), 0, None, std::ptr::null_mut());
        }
    }
}

#[tokio::test]
async fn recovery_r7_protocol_and_guard_reject_before_domain_data_or_ddl_including_empty() {
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    for (method, body) in [
        ("skills", json!({"skillProtocol":null})),
        ("marketSkills", json!({"skillProtocol":1})),
        ("skillDetails", json!({"skillProtocol":"2","id":"x"})),
        ("installSkill", json!({"skillProtocol":2.0,"name":"x","description":"d","instructions":"SAME"})),
        ("removeSkill", json!({"skillProtocol":2,"id":"x"})),
        ("skills", json!({"skillProtocol":2,"invented":true})),
        ("skills", json!({"skillProtocol":2,"spaceId":f.additional})),
    ] {
        let trace = R7DomainTrace::new(&f.hub.store);
        assert!(f.call(method, body).await.is_err());
        assert_eq!(trace.take(), 0, "{method}");
    }
    let empty = f.call("skills", json!({"skillProtocol":2})).await.unwrap();
    assert_eq!(empty, json!({"skillProtocol":2,"items":[]}));
    let p = registry::find("find-skills").unwrap();
    let mut body = intent(&p);
    body["skillProtocol"] = json!(2);
    let made = f.call("installSkill", body).await.unwrap();
    assert_eq!(made["skillProtocol"], 2);
    assert_eq!(
        f.call("skillDetails", json!({"skillProtocol":2,"id":p.install_id})).await.unwrap()["installation"],
        made["skill"]
    );
    let trace = R7DomainTrace::new(&f.hub.store);
    assert!(
        crate::api::dispatch(
            &f.hub,
            &crate::api::devices::Caller::Pairing { key: "r7-denied".into() },
            "skills",
            json!({"skillProtocol":2})
        )
        .await
        .is_err()
    );
    assert_eq!(trace.take(), 0);
}

// Corrida real medida na Instabox: filho criado por outra thread herda o lock até o exec.
#[cfg(unix)]
#[test]
fn catalog_lock_waits_for_copy_inherited_by_concurrent_spawn() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let f = Fixture::new();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let spawner = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut spawned = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                std::process::Command::new("true").status().unwrap();
                spawned += 1;
            }
            spawned
        })
    };
    let busy = (0..300).filter_map(|_| list(&f.0).err()).map(|e| format!("{e:#}")).collect::<Vec<_>>();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(spawner.join().unwrap() > 0);
    assert!(busy.is_empty(), "{} leituras recusadas: {:?}", busy.len(), busy.first());
}

#[test]
fn catalog_lock_waits_short_copy_and_refuses_real_holder() {
    let _isolamento = TEST_ISOLATION.blocking_lock();
    let f = Fixture::new();
    let p = registry::find("find-skills").unwrap();
    let held = root_lock(&f.0).unwrap().try_clone().unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        drop(held);
    });
    install(&f.0, &intent(&p)).unwrap();
    release.join().unwrap();
    let removal = current_removal(&f.0, &p.install_id);
    let held = root_lock(&f.0).unwrap();
    let started = std::time::Instant::now();
    let error = remove(&f.0, &removal);
    assert!(error.is_err());
    drop(held);
    assert_eq!(error.unwrap_err().to_string(), "Outra alteração de habilidades está em andamento");
    assert!(started.elapsed() >= LOCK_WAIT);
}

#[path = "review_tests.rs"]
mod review_tests;

/// D368 (revisão Sol, rodada 2): a remoção de skill que tiraria a skill do Berga grava só com a
/// autoridade ORIGINAL de quem pediu, reconferida na transação. Revogar e dar uma nova concessão à
/// mesma chave durante a espera, ou revogar entre a última checagem e o commit, não grava.
#[tokio::test]
async fn d368_remocao_da_skill_da_donna_exige_a_autoridade_original() {
    use crate::api::dispatch;
    use crate::store::{Device, DeviceSource, Scope};
    use std::{
        future::{Future, poll_fn},
        task::Poll,
    };
    let _isolamento = TEST_ISOLATION.lock().await;
    let f = ScopeFixture::new().await;
    let body = json!({"name":"d368-skill","description":"same","instructions":"SAME"});
    let installed = f.call("installSkill", body).await.unwrap();
    let id = installed["skill"]["id"].as_str().unwrap().to_owned();
    let donna: crate::store::BotConfig = serde_json::from_value(
        json!({"id":"donna","name":"Donna","backend":"fixture","command":"true","cwd":"/tmp","skills":[id]}),
    )
    .unwrap();
    f.hub.store.save_bot(&donna).unwrap();
    f.hub.store.set_team_settings(&json!({"primaryBotId":"donna"})).unwrap();
    let skills = || f.hub.store.bot("donna").unwrap().unwrap().config.skills;
    let device = Device {
        key: "d368-mac".into(),
        name: "Mac do dono".into(),
        platform: "fixture".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: 0,
        last_seen_at: None,
    };
    let captured = r6_captured(&installed["skill"]);
    // 1. Revogado na fila e com nova concessão para a mesma chave: o pedido antigo não grava.
    f.hub.store.put_device(&device).unwrap();
    let (_, old) = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap();
    let held = f.hub.store.team_lock.lock().await;
    let mut pending = Box::pin(dispatch(&f.hub, &old, "removeSkill", captured.clone()));
    poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(f.hub.revoke_device(&device.key).unwrap());
    f.hub.store.put_device(&device).unwrap();
    f.hub.auth_changed();
    let before = f.snapshot();
    drop(held);
    assert!(pending.await.is_err());
    assert_eq!(f.snapshot(), before);
    assert_eq!(skills(), std::slice::from_ref(&id));
    // 2. Revogado entre a última checagem e o commit: a transação da autoridade recusa.
    let (_, caller) = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap();
    let owner = crate::fork::autoridade::Autoridade::capturar(&f.hub, &caller).unwrap();
    let hub = f.hub.clone();
    let key = device.key.clone();
    let late = remove_with(
        &f.hub.store,
        Some((&f.hub, &owner)),
        &captured,
        || Ok(()),
        || {
            hub.revoke_device(&key)?;
            Ok(())
        },
    );
    assert!(late.is_err());
    assert_eq!(skills(), std::slice::from_ref(&id));
    // 3. Com a autoridade vigente, a remoção pelo dono tira a skill do Berga; sem ela, o Berga recusa.
    f.hub.store.put_device(&device).unwrap();
    let (_, caller) = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap();
    assert!(super::remove(&f.hub.store, &captured, || Ok(())).is_err(), "sem autoridade não mexe na Donna");
    assert_eq!(skills(), std::slice::from_ref(&id));
    dispatch(&f.hub, &caller, "removeSkill", captured).await.unwrap();
    assert_eq!(skills(), [] as [String; 0]);
}
