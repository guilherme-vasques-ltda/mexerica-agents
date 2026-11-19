//! D353: somente o recorte textual e o fluxo real de instalação/seleção existente.
use super::*;

fn text<'a>(p: &'a Package, path: &str) -> &'a str {
    &p.assets.iter().find(|a| a.path == path).unwrap().content
}

#[test]
fn d353_procedencia_exclusao_e_contrato_editorial() {
    let copy = registry::find("copywriting").unwrap();
    assert_eq!(copy.metadata.origin.revision, "e85f3cbc2e4a79bf33b2a12e9bdd9087d414735b");
    assert_eq!(copy.metadata.origin.source_digest, "b3784366f7e35e9358a2274ffe44905b0fb9bf3148b483f3745f030da7d139b7");
    assert!(!copy.assets.iter().any(|a| a.path.ends_with("natural-transitions.md")));
    let md = text(&copy, "SKILL.md");
    assert!(!md.contains("natural-transitions"));
    // Reconstitui a fonte somente desfazendo as duas alterações declaradas; não usa o auxiliar excluído.
    let source = md.split("\n## Donna Bot passive-use boundary (local addition)\n").next().unwrap();
    assert!(md.contains("[references/donna-transitions.md](references/donna-transitions.md)"));
    let guide = text(&copy, "references/donna-transitions.md");
    assert!(guide.contains("Original local adaptation authored for Donna Bot"));
    let restored = source.replace(
        "**For connecting sentences**: See [references/donna-transitions.md](references/donna-transitions.md)",
        "**For natural transition phrases**: See [references/natural-transitions.md](references/natural-transitions.md)",
    );
    assert_eq!(hash(restored.as_bytes()), "13c580c2432a4499e9dfdcf5d553df7aa6589e90cf5bd869fb4fbde5caf9f567");
    assert!(text(&copy, "LICENSE").contains("Copyright (c) 2025 Corey Haines"));
    assert!(text(&copy, "LICENSE-Profound.txt").contains("Copyright (c) 2026 Profound"));
    let stop = registry::find("stop-slop").unwrap();
    assert_eq!(stop.metadata.origin.revision, "8da1f030185bdfe8471220585162991eaeb970e9");
    assert!(text(&stop, "LICENSE").contains("Copyright (c) 2025 Hardik Pandya"));
    let panel = registry::find("copy-review-panel").unwrap();
    assert_eq!(panel.metadata.origin.revision, "bb67dff5f04b390e861ee11433166e4519e7f4c0");
    assert!(text(&panel, "LICENSE").contains("Copyright (c) 2026 Marketing OS contributors"));
    let md = text(&panel, "SKILL.md");
    for term in [
        "The skeptic",
        "The stranger",
        "The competitor",
        "The buyer",
        "The editor",
        "0-20",
        "0-100",
        "Below 70",
        "70-84",
        "85 or higher",
        "at most three",
        "never authorizes publication",
        "heuristic",
    ] {
        assert!(md.contains(term), "Contrato textual ausente: {term}");
    }
    for p in [&copy, &stop, &panel] {
        p.validate().unwrap();
        assert_eq!(p.metadata.availability, "adapted");
        assert_ne!(p.metadata.losses, [] as [String; 0]);
        assert!(text(p, "ADAPTATION.md").contains("D353"));
        assert!(text(p, "LICENSE").contains("Permission is hereby granted, free of charge"));
    }
}

#[tokio::test]
async fn d353_tres_pacotes_listados_instalados_por_gesto_sem_selecao_ou_sobrescrita() {
    let _isolamento = tests::TEST_ISOLATION.lock().await;
    let f = tests::ScopeFixture::new().await;
    let cfg: crate::store::BotConfig = serde_json::from_value(json!({
        "id":"d353-bot","name":"Sintético","backend":"fixture","command":"true","cwd":"/tmp","skills":[]
    }))
    .unwrap();
    f.hub.store.save_bot(&cfg).unwrap();
    let catalog = f.call("marketSkills", json!({"skillProtocol":2})).await.unwrap();
    assert_eq!(catalog["items"].as_array().unwrap().len(), 33);
    for id in ["copywriting", "stop-slop", "copy-review-panel"] {
        let p = registry::find(id).unwrap();
        let item = catalog["items"].as_array().unwrap().iter().find(|i| i["id"] == id).unwrap();
        assert_eq!(item["installed"], false);
        assert_eq!(item["installable"], true);
        assert!(!f.hub.store.skills_root.join(id).exists());
        let body = json!({"skillProtocol":2,"catalogId":id,"expectedDigest":p.metadata.digest});
        let installed = f.call("installSkill", body.clone()).await.unwrap();
        assert_eq!(installed["alreadyInstalled"], false);
        assert_eq!(installed["skill"]["receipt"]["digest"], p.metadata.digest);
        for a in &p.assets {
            assert_eq!(fs::read(f.hub.store.skills_root.join(id).join(&a.path)).unwrap(), a.content.as_bytes());
        }
        assert_eq!(f.hub.store.bot("d353-bot").unwrap().unwrap().config.skills, Vec::<String>::new());
        assert!(crate::market::skills_brief(&f.hub.store, &[]).is_none());
        let again = f.call("installSkill", body.clone()).await.unwrap();
        assert_eq!(again["alreadyInstalled"], true);
        assert_eq!(again["skill"]["installationId"], installed["skill"]["installationId"]);
        let path = f.hub.store.skills_root.join(id).join("SKILL.md");
        fs::write(&path, "ALTERADO SINTETICAMENTE").unwrap();
        assert!(f.call("installSkill", body).await.is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "ALTERADO SINTETICAMENTE");
    }
}
