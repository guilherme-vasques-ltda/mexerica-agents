#![cfg(unix)]
//! Cadastro seguro da chave do Motor Mexerica (GUI-205): salvar só depois de o fornecedor aceitar a chave, listar
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
    assert_eq!(acesso, serde_json::json!({"ok": false, "erro": "sem_chave", "estado": "nao_verificado"}));
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

// Endereço próprio (GUI-208): um serviço de mentira em 127.0.0.1, que só vale com a opção «local».

/// Servidor HTTP mínimo: anota cada pedido (linha inicial, cabeçalhos e corpo) e responde com `resposta(pedido)`.
pub(super) async fn servico_local(
    resposta: impl Fn(&str) -> (u16, String, &'static str) + Send + Sync + 'static,
) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let escuta = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", escuta.local_addr().unwrap());
    let pedidos = Arc::new(std::sync::Mutex::new(Vec::new()));
    let anotados = pedidos.clone();
    let resposta = Arc::new(resposta);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = escuta.accept().await {
            let anotados = anotados.clone();
            let resposta = resposta.clone();
            tokio::spawn(async move {
                let mut lido = Vec::new();
                let mut buf = [0u8; 8192];
                loop {
                    let Ok(n) = socket.read(&mut buf).await else { return };
                    if n == 0 {
                        return;
                    }
                    lido.extend_from_slice(&buf[..n]);
                    let texto = String::from_utf8_lossy(&lido).to_string();
                    let Some(fim) = texto.find("\r\n\r\n") else { continue };
                    let tamanho = texto[..fim]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if lido.len() < fim + 4 + tamanho {
                        continue;
                    }
                    let (status, corpo, tipo) = resposta(&texto);
                    anotados.lock().unwrap().push(texto);
                    let saida = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: {tipo}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{corpo}",
                        corpo.len()
                    );
                    let _ = socket.write_all(saida.as_bytes()).await;
                    return;
                }
            });
        }
    });
    (base, pedidos)
}

fn pedido_endereco(base: &str, local: bool, modelo: Option<&str>) -> chaves::Pedido {
    serde_json::from_value(serde_json::json!({
        "nome": "Serviço de prova", "url": format!("{base}/v1"), "formato": "chat-completions",
        "local": local, "chave": CHAVE, "modelo": modelo,
    }))
    .unwrap()
}

#[tokio::test]
async fn endereco_proprio_e_salvo_depois_do_teste_e_o_app_nunca_ve_a_chave() {
    let Some(hub) = hub_de_prova("endereco_proprio_e_salvo_depois_do_teste_e_o_app_nunca_ve_a_chave") else { return };
    let (base, pedidos) =
        servico_local(|_| (200, r#"{"data":[{"id":"modelo-local"}]}"#.into(), "application/json")).await;
    let acesso = chaves::salvar_endereco(&hub, pedido_endereco(&base, true, None), &[]).await.unwrap();
    assert_eq!(acesso["ok"], true, "{acesso}");
    assert_eq!(acesso["estado"], "modelos_no_catalogo");
    let id = acesso["id"].as_str().unwrap().to_owned();
    assert!(proprio::eh_proprio(&id));
    assert_eq!(acesso["modelos"][0]["id"], format!("{id}/modelo-local"));
    let pedido = pedidos.lock().unwrap()[0].clone();
    assert!(pedido.starts_with("GET /v1/models "), "{pedido}");
    assert!(
        pedido.to_ascii_lowercase().contains(&format!("authorization: bearer {CHAVE}")),
        "a chave não foi ao serviço"
    );

    let lista = chaves::listar(&hub.store).unwrap();
    assert!(!lista.to_string().contains(CHAVE), "o app viu a chave");
    assert_eq!(lista["enderecos"][0]["id"], id.as_str());
    assert_eq!(lista["enderecos"][0]["url"], format!("{base}/v1"));
    let cifrado = hub.store.kv_read(&credential_slot(&id)).unwrap().unwrap();
    assert!(!cifrado.contains(CHAVE) && !cifrado.contains("127.0.0.1"), "o registro tem de ficar cifrado");
    let catalogo = chaves::catalogo(&hub.store, crate::fork::spaces::PERSONAL).unwrap();
    assert!(catalogo["models"].as_array().unwrap().iter().any(|m| m["id"] == format!("{id}/modelo-local")));

    chaves::remover(&hub.store, &id).unwrap();
    assert_eq!(chaves::listar(&hub.store).unwrap()["enderecos"], serde_json::json!([]));
    assert_eq!(hub.store.kv_read(&credential_slot(&id)).unwrap(), None);
}

#[tokio::test]
async fn servico_sem_lista_so_e_salvo_com_o_modelo_digitado() {
    let Some(hub) = hub_de_prova("servico_sem_lista_so_e_salvo_com_o_modelo_digitado") else { return };
    let (base, _) = servico_local(|_| (404, r#"{"error":"sem lista"}"#.into(), "application/json")).await;
    let sem_modelo = chaves::salvar_endereco(&hub, pedido_endereco(&base, true, None), &[]).await.unwrap();
    assert_eq!(sem_modelo["erro"], "sem_lista");
    assert_eq!(sem_modelo["estado"], "acesso_nao_confirmado");
    assert_eq!(chaves::listar(&hub.store).unwrap()["enderecos"], serde_json::json!([]), "nada salvo sem modelo");

    let com_modelo =
        chaves::salvar_endereco(&hub, pedido_endereco(&base, true, Some("dono/modelo:tag")), &[]).await.unwrap();
    assert_eq!(com_modelo["ok"], true, "{com_modelo}");
    assert_eq!(com_modelo["estado"], "acesso_nao_confirmado");
    let id = com_modelo["id"].as_str().unwrap();
    assert_eq!(com_modelo["modelos"][0]["id"], format!("{id}/dono/modelo:tag"));
}

#[tokio::test]
async fn endereco_local_sem_a_opcao_local_e_recusado_pelo_motor_e_nao_e_salvo() {
    let Some(hub) = hub_de_prova("endereco_local_sem_a_opcao_local_e_recusado_pelo_motor_e_nao_e_salvo") else {
        return;
    };
    let (base, pedidos) = servico_local(|_| (200, r#"{"data":[]}"#.into(), "application/json")).await;
    // https para a própria máquina: a forma passa no Hub, o portão do motor recusa antes de conectar.
    let url = base.replace("http://", "https://");
    let acesso = chaves::salvar_endereco(&hub, pedido_endereco(&url, false, Some("x")), &[]).await.unwrap();
    assert_eq!(acesso["erro"], "destino_recusado", "{acesso}");
    assert_eq!(acesso["estado"], "nao_verificado");
    assert!(pedidos.lock().unwrap().is_empty());
    assert_eq!(chaves::listar(&hub.store).unwrap()["enderecos"], serde_json::json!([]));
}

#[test]
fn forma_do_endereco_proprio() {
    use proprio::{conferir_url, eh_proprio};
    assert!(eh_proprio("proprio-abcdef12"));
    for ruim in ["proprio-", "proprio-ABCDEF12", "proprio-abc", "openai", "proprio-abcdef12/x"] {
        assert!(!eh_proprio(ruim), "{ruim}");
    }
    assert_eq!(conferir_url("https://api.example.invalid/v1/", false).unwrap(), "https://api.example.invalid/v1");
    assert_eq!(conferir_url("http://127.0.0.1:11434/v1", true).unwrap(), "http://127.0.0.1:11434/v1");
    for (ruim, local) in [
        ("http://api.example.invalid/v1", false),
        ("https://usuario:senha@api.example.invalid/v1", false),
        ("https://api.example.invalid/v1?key=1", false),
        ("https://api.example.invalid/v1#x", false),
        ("ftp://api.example.invalid", true),
        ("não é endereço", true),
        ("", true),
    ] {
        let erro = conferir_url(ruim, local).unwrap_err().to_string();
        assert!(erro.starts_with("endereco_invalido"), "{ruim}: {erro}");
    }
    let registro = r#"{"key":"chave-ficticia-123","baseUrl":"https://api.example.invalid/v1","formato":"responses","local":false,"nome":"x"}"#;
    assert_eq!(proprio::segredo("proprio-abcdef12", registro).unwrap(), "chave-ficticia-123");
    assert_eq!(proprio::segredo("openai", "chave-direta-1234").unwrap(), "chave-direta-1234");
    let motor = proprio::credencial_do_motor(registro).unwrap();
    assert_eq!(motor["key"], "chave-ficticia-123");
    assert_eq!(
        motor["endpoint"],
        serde_json::json!({"baseUrl": "https://api.example.invalid/v1", "formato": "responses", "local": false})
    );
    assert!(proprio::segredo("proprio-abcdef12", "não é json").is_err(), "registro ilegível falha fechado");
}
