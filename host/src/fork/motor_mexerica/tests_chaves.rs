#![cfg(unix)]
//! Cadastro seguro da chave do Motor Mexerica: salvar só depois de o fornecedor aceitar a chave, listar
//! sem nunca devolver a chave, remover, e a chave do dono só para os bots do Space pessoal. O fornecedor aqui é o
//! falso do motor (`faux`), sem rede; os fornecedores reais são provados no motor com servidores de mentira.
use super::chaves;
use super::*;
use crate::LockExt;
use std::path::Path;

const CHAVE: &str = "chave-ficticia-do-cadastro-123";

/// O Hub da prova, num processo à parte com o motor ligado (`test_support::case` reexecuta o teste pelo nome).
fn hub_de_prova(teste: &str) -> Option<Arc<crate::hub::Hub>> {
    crate::fork::motor_exec::test_support::case(&format!("fork::motor_mexerica::tests_chaves::{teste}"))?;
    if !tests::require_motor() {
        return None;
    }
    let dir = crate::store::private_fixture_dir("motor-chaves");
    let store = Store::open(&dir.join("test.db")).unwrap();
    *store.secret_key.locked() = Some(zeroize::Zeroizing::new(vec![7; 32]));
    Some(crate::hub::Hub::new(
        store,
        "test".into(),
        crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
        "test".into(),
        1,
    ))
}

#[tokio::test]
async fn chave_aceita_vai_ao_cofre_e_a_lista_de_modelos_nunca_leva_a_chave() {
    let Some(hub) = hub_de_prova("chave_aceita_vai_ao_cofre_e_a_lista_de_modelos_nunca_leva_a_chave") else { return };
    let acesso = chaves::salvar(&hub, "faux", zeroize::Zeroizing::new(CHAVE.into()), &[(FAUX_ENV, "1")]).await.unwrap();
    assert_eq!(acesso["ok"], true, "{acesso}");
    assert!(acesso["modelos"].as_array().unwrap().iter().any(|m| m["id"] == "faux/faux-1"), "{acesso}");
    assert_eq!(crate::market::vault::read(&hub.store, &credential_slot("faux")).unwrap().as_deref(), Some(CHAVE));
    let cifrado = hub.store.kv_read(&credential_slot("faux")).unwrap().unwrap();
    assert!(!cifrado.contains(CHAVE), "a chave tem de ficar cifrada no banco");
    assert!(!acesso.to_string().contains(CHAVE));
    assert!(!chaves::listar(&hub.store).unwrap().to_string().contains(CHAVE));

    chaves::remover(&hub.store, "faux").unwrap();
    assert_eq!(
        crate::market::vault::read(&hub.store, &credential_slot("faux")).unwrap(),
        None,
        "a chave removida não pode ser lida"
    );
}

#[tokio::test]
async fn chave_recusada_pelo_fornecedor_nao_e_salva() {
    let Some(hub) = hub_de_prova("chave_recusada_pelo_fornecedor_nao_e_salva") else { return };
    let recusada = "recusada-pelo-fornecedor-456";
    let acesso =
        chaves::salvar(&hub, "faux", zeroize::Zeroizing::new(recusada.into()), &[(FAUX_ENV, "1")]).await.unwrap();
    assert_eq!(acesso["ok"], false);
    assert_eq!(acesso["erro"], "chave_invalida");
    assert_eq!(acesso["status"], 401);
    assert!(!acesso.to_string().contains(recusada));
    assert_eq!(hub.store.kv_read(&credential_slot("faux")).unwrap(), None, "chave recusada não entra no cofre");
}

#[tokio::test]
async fn testar_sem_chave_no_cofre_diz_sem_chave_sem_abrir_conversa() {
    let Some(hub) = hub_de_prova("testar_sem_chave_no_cofre_diz_sem_chave_sem_abrir_conversa") else { return };
    let acesso = chaves::testar(hub_vault(hub.clone()), "faux", &[(FAUX_ENV, "1")]).await.unwrap();
    assert_eq!(acesso, serde_json::json!({"ok": false, "erro": "sem_chave"}));
}

#[test]
fn catalogo_e_lista_mostram_so_fornecedor_com_chave() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let vazia = chaves::listar(&store).unwrap();
    let ids: Vec<_> = vazia["fornecedores"].as_array().unwrap().iter().map(|f| f["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["anthropic", "openai", "google", "xai", "openrouter", "meta"]);
    assert!(vazia["fornecedores"].as_array().unwrap().iter().all(|f| f["configurada"] == false));
    assert_eq!(chaves::catalogo(&store, crate::fork::spaces::PERSONAL).unwrap()["models"], serde_json::json!([]));

    crate::market::vault::write(&store, &credential_slot("openai"), CHAVE).unwrap();
    store
        .kv_set(
            "motor-mexerica.modelos/openai",
            r#"{"modelos":[{"id":"openai/gpt-teste","name":"GPT Teste"}],"conferidoEm":1}"#,
        )
        .unwrap();
    // Lista de modelos sem chave (a chave foi removida por fora) não aparece na escolha.
    store
        .kv_set("motor-mexerica.modelos/xai", r#"{"modelos":[{"id":"xai/grok-teste","name":"Grok"}],"conferidoEm":1}"#)
        .unwrap();
    let catalogo = chaves::catalogo(&store, crate::fork::spaces::PERSONAL).unwrap();
    let modelos: Vec<_> = catalogo["models"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(modelos, ["openai/gpt-teste"]);
    assert_eq!(
        chaves::catalogo(&store, "space-compartilhado").unwrap()["models"],
        serde_json::json!([]),
        "outro Space não vê"
    );
    let lista = chaves::listar(&store).unwrap();
    assert!(!lista.to_string().contains(CHAVE));
    let openai = lista["fornecedores"].as_array().unwrap().iter().find(|f| f["id"] == "openai").unwrap();
    assert_eq!(openai["configurada"], true);
    assert_eq!(openai["modelos"][0]["id"], "openai/gpt-teste");
}

#[test]
fn chave_so_sai_para_bot_do_space_pessoal() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let cfg = |space: &str| -> crate::store::BotConfig {
        serde_json::from_value(serde_json::json!({"id": "bot-de-prova", "name": "Teste", "backend": "mexerica",
            "model": "openai/gpt-teste", "spaceId": space, "notify": false}))
        .unwrap()
    };
    store.save_bot(&cfg(crate::fork::spaces::PERSONAL)).unwrap();
    assert!(chaves::bot_pessoal(&store, "bot-de-prova").unwrap());
    assert!(!chaves::bot_pessoal(&store, "bot-que-nao-existe").unwrap());

    let mut row = store.bot("bot-de-prova").unwrap().unwrap();
    assert!(chaves::pessoal(&row));
    row.config = cfg("space-compartilhado");
    assert!(!chaves::pessoal(&row), "a chave do dono não serve a bot de outro Space");
    row.config = cfg(crate::fork::spaces::PERSONAL);
    row.deleted = true;
    assert!(!chaves::pessoal(&row), "bot apagado não lê chave");
}

#[test]
fn chaves_do_motor_sao_so_deste_computador() {
    use crate::api::devices::{Caller, permit};
    use crate::store::Scope;
    let aparelho = Caller::Device { key: "k".into(), scopes: vec![Scope::Control, Scope::Screen], autoridade: None };
    for metodo in CHAVE_METHODS {
        assert!(permit(&Caller::Local, metodo).is_ok());
        assert!(permit(&aparelho, metodo).is_err(), "{metodo} não pode vir de um aparelho pareado");
    }
}

#[test]
fn chave_colada_fora_do_formato_e_recusada_sem_repetir_a_chave() {
    assert_eq!(chaves::chave(Some("  chave-ficticia-boa-789 ")).unwrap().as_str(), "chave-ficticia-boa-789");
    for ruim in [
        "curta",
        "chave com espaço no meio",
        "chave-com\ncontrole",
        "chave-com-\"aspas",
        "chave-com-\\barra",
        "chave-acentuação",
        &"x".repeat(5000),
    ] {
        let erro = chaves::chave(Some(ruim)).unwrap_err().to_string();
        assert!(erro.starts_with("chave_formato"), "{erro}");
        assert!(!erro.contains(ruim), "o erro repetiu a chave");
    }
    assert!(chaves::chave(None).is_err());
}

#[test]
fn pedido_vindo_de_um_space_aberto_e_aceito_e_campo_estranho_nao() {
    let pedido: chaves::Pedido =
        serde_json::from_value(serde_json::json!({"fornecedor": "openai", "spaceId": "space-aberto"})).unwrap();
    assert_eq!(pedido.fornecedor.as_deref(), Some("openai"));
    assert!(serde_json::from_value::<chaves::Pedido>(serde_json::json!({"fornecedor": "openai", "outro": 1})).is_err());
}
