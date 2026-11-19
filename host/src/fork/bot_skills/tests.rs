use super::*;
use crate::{
    fork::{skill_catalog, skill_selection},
    store::BotConfig,
};
use serde_json::{Value, json};

struct Fixture {
    store: Store,
    cfg: BotConfig,
    ticket: Ticket,
    skill: String,
    dir: std::path::PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("bot-skills-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("test.db")).unwrap();
        let cfg: BotConfig =
            serde_json::from_value(json!({"id":"bot", "name":"Teste", "backend":"fixture", "cwd":dir})).unwrap();
        store.save_bot(&cfg).unwrap();
        let skill = skill_catalog::custom(
            &store,
            &json!({"name":"observada", "description":"perfil", "instructions":"BYTES_ORIGINAIS"}),
            || Ok(()),
        )
        .unwrap();
        let skill = skill["skill"]["id"].as_str().unwrap().into();
        let ticket = bot_admission::reserve(&store, &cfg, "bot", "token-original", &Origin::Human).unwrap();
        Self { store, cfg, ticket, skill, dir }
    }
    fn capture(&self, fresh: bool) -> Prepared {
        Prepared::capture(&self.store, "bot", &self.ticket, &Lane::main("bot"), "bot".into(), fresh).unwrap()
    }
    fn select(&self) {
        skill_selection::set_for_test(&self.store, "bot", &[&self.skill]);
    }
}
#[test]
fn selecao_db_atrasada_no_ator_aba_e_recuperacao_nao_promovem_observacao() {
    let f = Fixture::new();
    f.select();
    assert!(f.cfg.skills.is_empty(), "ator atrasado é proposital");
    let prepared = f.capture(true);
    assert!(prepared.project(&f.store).unwrap().unwrap().contains("read "));
    skill_selection::set_for_test(&f.store, "bot", &[]);
    f.select();
    let stale = prepared.project(&f.store).unwrap().unwrap();
    assert!(stale.contains(preparo::WARNING));
    assert!(!stale.contains("read "));
    assert!(!f.capture(false).project(&f.store).unwrap().unwrap().contains("read "), "restart não recaptura positivos");
    bot_admission::finish(&f.store, "bot", &f.ticket).unwrap();
    let next = bot_admission::reserve(&f.store, &f.cfg, "bot", "sucessor", &Origin::Human).unwrap();
    let recovered = Prepared::capture(&f.store, "bot", &next, &Lane::main("bot"), "bot".into(), true).unwrap();
    assert!(recovered.project(&f.store).unwrap().unwrap().contains("read "));
    assert!(prepared.project(&f.store).is_err());
    assert_eq!(bot_admission::current(&f.store, "bot").unwrap().unwrap().ticket(), next);
}
#[test]
fn d306_compara_bytes_retidos_e_preserva_outra_skill_integra() {
    let f = Fixture::new();
    let other = skill_catalog::custom(
        &f.store,
        &json!({"name":"integra", "description":"outra", "instructions":"SEGUNDA"}),
        || Ok(()),
    )
    .unwrap();
    let other = other["skill"]["id"].as_str().unwrap();
    skill_selection::set_for_test(&f.store, "bot", &[&f.skill, other]);
    let prepared = f.capture(true);
    let before = prepared.project(&f.store).unwrap().unwrap();
    let path = f.store.skills_root.join(&f.skill).join("SKILL.md");
    let original = std::fs::read(&path).unwrap();
    std::fs::write(&path, "BYTES_ADULTERADOS").unwrap();
    let changed = prepared.project(&f.store).unwrap().unwrap();
    assert!(!changed.contains(&path.display().to_string()));
    assert!(changed.contains("integra") && changed.contains(preparo::WARNING));
    assert!(!changed.contains("BYTES_ORIGINAIS") && !changed.contains("BYTES_ADULTERADOS"));
    std::fs::write(&path, original).unwrap();
    assert_eq!(prepared.project(&f.store).unwrap().unwrap(), before, "a referência interna não adotou bytes novos");
    f.store.kv_set("donna.passiveSkills.v2", "{").unwrap();
    assert_eq!(prepared.project(&f.store).unwrap().unwrap(), preparo::WARNING);
}
#[test]
fn reinstalacao_com_mesmo_corpo_invalida_observacao_e_identidade() {
    let f = Fixture::new();
    let catalog = skill_catalog::browse(&f.store, || Ok(())).unwrap();
    let item = catalog["items"].as_array().unwrap().iter().find(|v| v["id"] == "find-skills").unwrap();
    let body = json!({"catalogId":item["id"], "expectedDigest":item["digest"]});
    let installed = skill_catalog::install(&f.store, &body, || Ok(())).unwrap();
    let id = installed["skill"]["id"].as_str().unwrap();
    skill_selection::set_for_test(&f.store, "bot", &[id]);
    let prepared = f.capture(true);
    let old = &installed["skill"];
    let path = f.store.skills_root.join(id).join("SKILL.md");
    let bytes = std::fs::read(&path).unwrap();
    skill_catalog::remove(&f.store, &json!({"id":id, "expectedInstallationId":old["installationId"],
        "expectedReceipt":{"kind":"catalog", "catalogId":old["receipt"]["catalogId"], "digest":old["receipt"]["digest"]}}), || Ok(())).unwrap();
    let new = skill_catalog::install(&f.store, &body, || Ok(())).unwrap();
    assert_ne!(old["installationId"], new["skill"]["installationId"]);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    // Comparação do catálogo é independente da comparação c1 da seleção.
    assert!(prepared.catalog.as_ref().unwrap().check(&f.store.db.locked()).is_err());
    assert_eq!(prepared.project(&f.store).unwrap().unwrap(), preparo::WARNING);
}

#[test]
fn origem_attempt_destino_e_binding_originais_sao_condicoes_distintas() {
    let f = Fixture::new();
    f.select();
    let mut prepared = f.capture(true);
    let binding = bindings::issue(&f.store, "bot", "bot", "bot", None).unwrap();
    prepared.bind(&f.store, &binding).unwrap();
    assert_eq!(prepared.pending_binding(&f.store).unwrap().unwrap().id, binding.id);
    bindings::activate(&f.store, &binding, "acp-original").unwrap();
    prepared.complete(&f.store, &binding, "acp-original").unwrap();
    let replacement = bindings::issue(&f.store, "bot", "bot", "bot", None).unwrap();
    assert!(prepared.bind(&f.store, &replacement).is_err());
    let mut wrong_destination = f.capture(true);
    wrong_destination.lane.chat = "outra".into();
    assert!(wrong_destination.project(&f.store).is_err());
    let mut record: Value = serde_json::from_str(&f.store.kv_get(&bot_admission::key("bot")).unwrap()).unwrap();
    record["attempt"] = json!("outra-tentativa");
    f.store.kv_set(&bot_admission::key("bot"), &record.to_string()).unwrap();
    assert!(prepared.project(&f.store).is_err());
    assert_eq!(bot_admission::current(&f.store, "bot").unwrap().unwrap().attempt, "outra-tentativa");
}
#[test]
fn vazio_valido_nao_exige_catalogo_e_c1_ausente_nao_vira_vazio() {
    let f = Fixture::new();
    f.store.kv_set("donna.passiveSkills.v2", "{").unwrap();
    assert!(f.capture(true).project(&f.store).unwrap().is_none());
    f.store.db.locked().execute("UPDATE bots SET donna_skills_version=NULL WHERE id='bot'", []).unwrap();
    assert_eq!(f.capture(true).project(&f.store).unwrap().unwrap(), preparo::WARNING);
}
