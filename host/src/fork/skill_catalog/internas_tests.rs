use super::*;
use crate::api::devices::Caller;
use crate::hub::Hub;

async fn rpc(hub: &std::sync::Arc<Hub>, method: &str, body: Value) -> Result<Value> {
    call(hub, &Caller::Local, method, body).await
}

fn ids(list: &Value) -> Vec<String> {
    list["internas"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn internas_da_edicao_tem_identidade_que_nenhuma_skill_pode_usar() {
    for open in [false, true] {
        let lista = internas::listar(open).unwrap();
        assert_eq!(lista.len(), if open { 1 } else { 2 });
        for item in &lista {
            let id = item["id"].as_str().unwrap();
            assert!(id.starts_with(internas::PREFIXO), "{id}");
            assert!(crate::fork::agent_skills::validate_name(id, id).is_err(), "{id} não pode virar skill instalada");
            assert!(registry::find(id).is_err(), "{id} fora do catálogo instalável");
            assert!(!item["name"].as_str().unwrap().is_empty() && !item["description"].as_str().unwrap().is_empty());
            assert!(matches!(item["uso"].as_str(), Some("todaConversa" | "criarSkill")), "{item}");
        }
    }
    let open: Vec<_> = internas::listar(true).unwrap().into_iter().map(|s| s["id"].clone()).collect();
    assert_eq!(open, [json!("interna:regras-da-open")], "a Open só mostra o que a conversa dela recebe");
}

#[tokio::test]
async fn hub_lista_as_internas_e_a_api_nao_altera_nem_apaga() {
    let _serial = tests::TEST_ISOLATION.lock().await;
    let dir = std::env::temp_dir().join(format!("mexerica-internas-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let store = Store::open(Path::new(":memory:")).unwrap();
    let root = store.skills_root.clone();
    let hub = Hub::new(
        store,
        "test".into(),
        crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
        "test".into(),
        19222,
    );
    let antes = rpc(&hub, "skills", json!({"skillProtocol": 2})).await.unwrap();
    let esperadas = if crate::environment::Environment::is_open() {
        vec!["interna:regras-da-open"]
    } else {
        vec!["interna:manual-do-harness", "interna:criador-de-skills"]
    };
    assert_eq!(ids(&antes), esperadas);
    let instaladas = antes["items"].as_array().unwrap();
    assert!(instaladas.iter().all(|s| !s["id"].as_str().unwrap().starts_with(internas::PREFIXO)));

    for id in ids(&antes) {
        let tentativas = [
            (
                "removeSkill",
                json!({"skillProtocol": 2, "id": id, "expectedInstallationId": "00000000-0000-4000-8000-000000000000:1", "expectedReceipt": {"kind": "none"}}),
            ),
            ("removeSkill", json!({"id": id})),
            ("installSkill", json!({"skillProtocol": 2, "catalogId": id, "expectedDigest": "0".repeat(64)})),
            ("skillDetails", json!({"skillProtocol": 2, "id": id})),
        ];
        for (method, body) in tentativas {
            assert!(rpc(&hub, method, body.clone()).await.is_err(), "{method} {body}");
        }
    }
    let depois = rpc(&hub, "skills", json!({"skillProtocol": 2})).await.unwrap();
    assert_eq!(depois["internas"], antes["internas"], "as internas seguem iguais");
    assert_eq!(depois["items"], antes["items"], "nada foi instalado no lugar delas");

    // Uma skill manual com o mesmo nome vira outra skill, com identidade própria, e a interna fica igual.
    let manual = rpc(
        &hub,
        "installSkill",
        json!({"skillProtocol": 2, "name": esperadas[0], "description": "d", "instructions": "texto novo"}),
    )
    .await
    .unwrap();
    assert!(!manual["skill"]["id"].as_str().unwrap().starts_with(internas::PREFIXO), "{manual}");
    let depois = rpc(&hub, "skills", json!({"skillProtocol": 2})).await.unwrap();
    assert_eq!(depois["internas"], antes["internas"], "a skill manual não substitui a interna");
    hub.shutdown().await;
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(dir);
}
