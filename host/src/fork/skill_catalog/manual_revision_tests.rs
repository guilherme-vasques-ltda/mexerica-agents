//! d1: fronteiras do salvamento manual e do mesmo verificador usado pelo motor.
use super::*;

#[test]
fn d1_adulteracao_entre_publicar_e_registrar_nao_vira_base_aprovada() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let result = custom_with(
        &store,
        &json!({"name":"d1-race","description":"manual","instructions":"APROVADO"}),
        || Ok(()),
        |path| Ok(fs::write(path, "ADULTERADO")?),
    );
    assert_eq!(list(&store, || Ok(())).unwrap()["items"], json!([]));
    assert!(!store.skills_root.join("d1-race").exists());
    let _ = fs::remove_dir_all(&store.skills_root);
    assert!(result.is_err(), "corpo adulterado aceito: {result:?}");
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

fn save(store: &Store) -> Value {
    custom(store, &json!({"name":"d1-manual","description":"manual","instructions":"APROVADO"}), || Ok(())).unwrap()["skill"].clone()
}

fn metadata(store: &Store) -> Value {
    serde_json::from_str(&store.kv_get(storage::META).unwrap()).unwrap()
}

fn removal(skill: &Value) -> Value {
    json!({"id":skill["id"],"expectedInstallationId":skill["installationId"],"expectedReceipt":{"kind":"none"}})
}

#[test]
fn d1_revisao_inicial_usa_digest_canonico_e_leituras_nao_mudam_identidade_ou_perfil() {
    let f = Fixture::new();
    let store = &f.0;
    let skill = save(store);
    let id = skill["id"].as_str().unwrap();
    let path = store.skills_root.join(id).join("SKILL.md");
    let md = fs::read_to_string(&path).unwrap();
    let meta = metadata(store);
    assert_eq!(meta["installations"][id]["manualRevision"], 1);
    assert_eq!(meta["installations"][id]["bodyDigest"], hash(md.as_bytes()));
    let enabled = vec![id.to_owned()];
    let cfg: crate::store::BotConfig = serde_json::from_value(json!({
        "id":"d1-bot","name":"D1","backend":"fixture","cwd":"/tmp","skills":enabled
    }))
    .unwrap();
    store.save_bot(&cfg).unwrap();
    let saved_bot = store.bot(&cfg.id).unwrap().unwrap();
    let brief = crate::market::skills_brief(store, &enabled).unwrap();
    assert!(brief.contains(&path.display().to_string()) && !brief.contains("APROVADO"));
    let command = crate::chat::prompt::of_message(store, &enabled, Path::new("/tmp"), &format!("/{id} pedido"), &[]);
    assert!(command.contains(&path.display().to_string()) && command.ends_with("pedido"));
    assert!(!command.contains("APROVADO"));
    assert_eq!(list(store, || Ok(())).unwrap()["items"], json!([skill]));
    assert_eq!(details(store, &json!({"id":id}), || Ok(())).unwrap()["installation"], skill);
    assert_eq!(metadata(store), meta, "leitura não emite aprovação ou identidade");
    assert_eq!(fs::read_to_string(path).unwrap(), md);
    let current_bot = store.bot(&cfg.id).unwrap().unwrap();
    assert_eq!(current_bot.config.skills, enabled);
    assert_eq!(current_bot.skills_version, saved_bot.skills_version);
    assert!(
        custom(
            store,
            &json!({"name":"d1-forjada","description":"manual","instructions":"x","manualRevision":1}),
            || Ok(())
        )
        .is_err()
    );
}

#[test]
fn d1_r7_preserva_expectativa_anterior_sem_recapturar_corpo_nem_inventar_revisao_legada() {
    for gap in [false, true] {
        for legacy in [false, true] {
            let f = Fixture::new();
            let store = &f.0;
            let first = save(store);
            let id = first["id"].as_str().unwrap();
            let path = store.skills_root.join(id).join("SKILL.md");
            let mut meta = metadata(store);
            if legacy {
                meta["installations"][id].as_object_mut().unwrap().remove("manualRevision");
                store.kv_set(storage::META, &meta.to_string()).unwrap();
            }
            let expected = meta["installations"][id].clone();
            // V2 íntegro anterior a d1: continua disponível, sem revisão fabricada.
            assert_eq!(list(store, || Ok(())).unwrap()["items"], json!([first]));
            assert_eq!(metadata(store), meta);
            let profile = crate::market::skills_brief(store, &[id.to_owned()]);
            assert!(profile.is_some());
            fs::write(&path, "ADULTERADO ANTES DA REEMISSAO").unwrap();
            if gap {
                store.db.locked().execute_batch("DROP TRIGGER d166_skill_v2_update").unwrap();
            } else {
                store.kv_set("skills", &store.kv_get("skills").unwrap()).unwrap();
            }
            let current = list(store, || Ok(())).unwrap()["items"][0].clone();
            let version = &metadata(store)["installations"][id];
            assert_ne!(current["installationId"], first["installationId"], "anti-ABA R7");
            assert_eq!(version["bodyDigest"], expected["bodyDigest"]);
            assert_eq!(version["manualRevision"], expected["manualRevision"]);
            assert_eq!(current["contentStatus"], "changed");
            assert_eq!(details(store, &json!({"id":id}), || Ok(())).unwrap_err().to_string(), installation::CHANGED);
            assert!(crate::market::skills_brief(store, &[id.to_owned()]).is_none());
            assert_eq!(fs::read_to_string(&path).unwrap(), "ADULTERADO ANTES DA REEMISSAO");
            assert_eq!(remove(store, &removal(&first), || Ok(())).unwrap_err().to_string(), installation::STALE);
            remove(store, &removal(&current), || Ok(())).unwrap();
            let saved = save(store);
            assert_eq!(saved["contentStatus"], "verified");
            assert_ne!(saved["installationId"], current["installationId"]);
            assert_eq!(metadata(store)["installations"][id]["manualRevision"], 1);
        }
    }
}

#[test]
fn d1_manual_sem_prova_nao_adota_disco_e_continua_removivel_para_novo_gesto() {
    let f = Fixture::new();
    let store = &f.0;
    private_dir(&store.skills_root).unwrap();
    let path = store.skills_root.join("d1-manual");
    private_dir(&path).unwrap();
    fs::write(path.join("SKILL.md"), "SEM EXPECTATIVA PRESERVADA").unwrap();
    store
        .kv_set("skills", r#"[{"id":"d1-manual","name":"d1-manual","description":"manual","source":"custom"}]"#)
        .unwrap();
    let skill = list(store, || Ok(())).unwrap()["items"][0].clone();
    assert_eq!(skill["contentStatus"], "unavailable");
    assert!(metadata(store)["installations"]["d1-manual"]["bodyDigest"].is_null());
    assert!(metadata(store)["installations"]["d1-manual"]["manualRevision"].is_null());
    assert!(crate::market::skills_brief(store, &["d1-manual".to_owned()]).is_none());
    remove(store, &removal(&skill), || Ok(())).unwrap();
    let saved = save(store);
    assert_eq!(saved["contentStatus"], "verified");
    assert_ne!(saved["installationId"], skill["installationId"]);
    assert_eq!(metadata(store)["installations"]["d1-manual"]["manualRevision"], 1);
}
