use super::*;
use std::path::Path;

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

fn install(store: &Store, id: &str) -> Value {
    let p = registry::find(id).unwrap();
    super::install(store, &json!({"catalogId": id, "expectedDigest": p.metadata.digest}), || Ok(())).unwrap()
}

#[test]
fn open_so_entra_skill_de_texto_da_lista_fechada() {
    for id in ["grilling", "copywriting", "stop-slop", SEED] {
        assert!(open::permitida(id), "{id}");
        open::conferir(id).unwrap();
    }
    // Catálogo embutido que pede conector, credencial, runtime, rede ou descoberta externa fica fora.
    for id in [
        "boat",
        "hyperframes",
        "research",
        "security-audit",
        "criador-de-skills",
        "exa-pesquisar-na-web",
        "linear-triar-issues-novas",
        "supabase-revisar-seguranca",
        "e-commerce-pedidos-e-anuncios-do-dia",
        "desconhecida",
    ] {
        assert!(!open::permitida(id), "{id}");
        assert!(open::conferir(id).is_err(), "{id}");
    }
}

#[test]
fn open_marca_fora_da_lista_como_indisponivel_sem_tocar_nas_permitidas() {
    let mut fora = json!({"installable": true, "reason": "motivo do catálogo"});
    open::marcar(&mut fora, "boat");
    assert_eq!(fora["installable"], false);
    assert_eq!(fora["reason"], open::MOTIVO);
    let mut dentro = json!({"installable": true, "reason": "motivo do catálogo"});
    open::marcar(&mut dentro, "grilling");
    assert_eq!(dentro["installable"], true);
    assert_eq!(dentro["reason"], "motivo do catálogo");
    let mut ja_fora = json!({"installable": false, "reason": "licença"});
    open::marcar(&mut ja_fora, "boat");
    assert_eq!(ja_fora["reason"], "licença", "indisponibilidade anterior não é sobrescrita");
}

#[test]
fn open_entrega_so_skill_permitida_instalada_do_catalogo_e_conferida() {
    let f = Fixture::new();
    install(&f.0, "grilling");
    install(&f.0, "boat");
    custom(&f.0, &json!({"name": "manual-open", "description": "d", "instructions": "TEXTO MANUAL NAO ENTRA"}), || {
        Ok(())
    })
    .unwrap();
    let texto = open::texto_para_bot(&f.0, &["boat".into(), "manual-open".into(), "grilling".into()])
        .expect("a skill permitida entra no texto do bot");
    assert!(texto.contains("Skill: grilling"), "{texto}");
    assert!(!texto.contains("TEXTO MANUAL NAO ENTRA"));
    assert!(!texto.contains("Skill: boat"));
    assert!(open::texto_para_bot(&f.0, &["boat".into()]).is_none());
    assert!(open::texto_para_bot(&f.0, &[]).is_none());
    // Corpo alterado fora do app sai da entrega em vez de ser entregue pela metade.
    fs::write(f.0.skills_root.join("grilling/SKILL.md"), "ALTERADO").unwrap();
    assert!(open::texto_para_bot(&f.0, &["grilling".into()]).is_none());
}

#[test]
fn find_skills_vem_instalada_e_usa_o_catalogo_do_hub_quando_ligada() {
    let f = Fixture::new();
    seed(&f.0).unwrap();
    seed(&f.0).unwrap();
    let installed = open::installed(&f.0, &[]).unwrap();
    let items = installed["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], SEED);
    assert_eq!(items[0]["enabled"], false);
    assert!(open::texto_para_bot(&f.0, &[]).is_none());
    let selection = [SEED.into()];
    let text = open::texto_para_bot(&f.0, &selection).unwrap();
    for tool in ["list_catalog_skills", "install_skill", "enable_skill"] {
        assert!(text.contains(tool));
    }
    assert!(!text.contains("npx"));
    fs::write(f.0.skills_root.join(SEED).join("SKILL.md"), "ALTERADO").unwrap();
    assert!(open::texto_para_bot(&f.0, &selection).is_none());
}
