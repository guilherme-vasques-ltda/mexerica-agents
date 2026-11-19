use super::*;

#[tokio::test]
async fn e2_catalogo_e_projecao_leem_os_mesmos_bytes_sem_instalar_ou_selecionar() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let root = std::env::temp_dir().join(format!("donna-e2-{}", uuid::Uuid::new_v4()));
    let catalog = browse(&store, || Ok(())).unwrap();
    let item = catalog["items"].as_array().unwrap().iter().find(|p| p["id"] == "criador-de-skills").unwrap();
    assert_eq!(item["installed"], false);
    assert_eq!(item["installable"], true);
    let package = registry::find("criador-de-skills").unwrap();
    let details = details(&store, &json!({"source":"criador-de-skills"}), || Ok(())).unwrap();
    assert_eq!(details["catalog"]["digest"], package.metadata.digest);
    assert!(details["markdown"].as_str().unwrap().contains("references/mcp.md"));
    assert!(details["markdown"].as_str().unwrap().contains("references/plugins.md"));
    assert!(!registry::find("skill-creator").unwrap().installable(), "Anthropic continua bloqueada");
    let before = (store.kv_get("skills"), store.kv_get(STATE), store.kv_get(storage::META));
    install_guide(&root).await.unwrap();
    let projected = root.join("extensibilidade/criador-de-skills");
    for asset in &package.assets {
        let bytes = fs::read(projected.join(&asset.path)).unwrap();
        assert_eq!(bytes, asset.content.as_bytes());
        assert_eq!(hash(&bytes), asset.sha256);
        assert_eq!(bytes.len(), asset.bytes);
    }
    // Startup repetido repara a projeção pública ausente; não altera o catálogo pessoal.
    fs::remove_file(projected.join("references/mcp.md")).unwrap();
    install_guide(&root).await.unwrap();
    assert_eq!(
        fs::read(projected.join("references/mcp.md")).unwrap(),
        package.assets.iter().find(|a| a.path == "references/mcp.md").unwrap().content.as_bytes()
    );
    assert_eq!((store.kv_get("skills"), store.kv_get(STATE), store.kv_get(storage::META)), before);
    assert_eq!(crate::market::skills(&store), Vec::<Skill>::new());
    assert!(crate::market::skills_brief(&store, &[]).is_none());
    assert!(crate::market::expand_command(&store, &[], "/criador-de-skills revisar").is_none());
    let md = &package.assets.iter().find(|a| a.path == "SKILL.md").unwrap().content;
    assert!(!md.split("---").nth(1).unwrap().contains("allowed-tools"));
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(&store.skills_root).unwrap();
}
