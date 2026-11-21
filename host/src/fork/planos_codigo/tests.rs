//! Provas dos planos de código com servidores de mentira: nenhuma rede, nenhuma chave real.
use super::acesso::{Falha, listar_modelos_de_prova};
use super::*;
use crate::{LockExt, remote::identity::Identity};
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const CHAVE: &str = "chave-ficticia-do-plano-123";

/// Um servidor de mentira que responde sempre a mesma resposta HTTP e guarda os pedidos que recebeu.
async fn servidor(resposta: Vec<u8>) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
    let escuta = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/inference/go/v1/models", escuta.local_addr().unwrap());
    let pedidos = Arc::new(std::sync::Mutex::new(Vec::new()));
    let guardados = pedidos.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut conexao, _)) = escuta.accept().await else { return };
            let (resposta, guardados) = (resposta.clone(), guardados.clone());
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 8192];
                let n = conexao.read(&mut buffer).await.unwrap_or(0);
                guardados.locked().push(String::from_utf8_lossy(&buffer[..n]).into_owned());
                let _ = conexao.write_all(&resposta).await;
                let _ = conexao.shutdown().await;
            });
        }
    });
    (url, pedidos)
}

fn http(status: &str, extra: &str, corpo: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n{extra}connection: close\r\n\r\n{corpo}", corpo.len())
        .into_bytes()
}

async fn testar(resposta: Vec<u8>) -> Result<Vec<acesso::Modelo>, Falha> {
    let (url, _) = servidor(resposta).await;
    listar_modelos_de_prova("opencode-go", &url, CHAVE, Duration::from_secs(5)).await
}

#[tokio::test]
async fn lista_so_modelos_de_conversa_com_o_prefixo_do_plano_e_manda_a_chave_como_bearer() {
    let corpo = r#"{"object":"list","data":[
        {"id":"glm-5.3","name":"GLM 5.3","type":"chat"},
        {"id":"kimi-k3"},
        {"id":"glm-5.3","name":"repetido"},
        {"id":"imagem-1","type":"image"},
        {"id":"com/barra"},
        {"id":"com espaço"},
        {"id":""},
        {"nome":"sem id"}]}"#;
    let (url, pedidos) = servidor(http("200 OK", "", corpo)).await;
    let modelos = listar_modelos_de_prova("opencode-go", &url, CHAVE, Duration::from_secs(5)).await.unwrap();
    let ids: Vec<_> = modelos.iter().map(|m| (m.id.as_str(), m.name.as_str())).collect();
    assert_eq!(ids, [("opencode-go/glm-5.3", "GLM 5.3"), ("opencode-go/kimi-k3", "opencode-go/kimi-k3")]);
    let pedido = pedidos.locked()[0].to_ascii_lowercase();
    assert!(pedido.starts_with("get /inference/go/v1/models"), "{pedido}");
    assert!(pedido.contains(&format!("authorization: bearer {}", CHAVE.to_ascii_lowercase())), "{pedido}");
    assert!(pedido.contains("user-agent: mexerica/"), "{pedido}");
}

#[tokio::test]
async fn recusa_do_fornecedor_e_falha_sao_classificadas_so_pelo_status() {
    for (status, esperado) in [
        ("401 Unauthorized", Falha::ChaveInvalida),
        ("403 Forbidden", Falha::SemPermissao),
        ("402 Payment Required", Falha::Limite),
        ("429 Too Many Requests", Falha::Limite),
        ("500 Internal Server Error", Falha::FornecedorFalhou),
        ("404 Not Found", Falha::FornecedorFalhou),
    ] {
        // O corpo do erro repete a chave: ele nunca é lido.
        let corpo = format!(r#"{{"error":"{CHAVE}"}}"#);
        assert_eq!(testar(http(status, "", &corpo)).await.unwrap_err(), esperado, "{status}");
    }
    assert_eq!(Falha::ChaveInvalida.estado(), "recusada");
    assert_eq!(Falha::SemPermissao.estado(), "recusada");
    for falha in [Falha::Limite, Falha::FornecedorFalhou, Falha::SemResposta, Falha::RespostaInvalida] {
        assert_eq!(falha.estado(), "nao_verificado", "{falha:?} não diz nada sobre a chave");
    }
}

#[tokio::test]
async fn resposta_quebrada_ou_enorme_e_invalida() {
    assert_eq!(testar(http("200 OK", "", "isto não é json")).await.unwrap_err(), Falha::RespostaInvalida);
    assert_eq!(testar(http("200 OK", "", r#"{"data":"não é lista"}"#)).await.unwrap_err(), Falha::RespostaInvalida);
    let enorme = format!(r#"{{"data":[{{"id":"{}"}}]}}"#, "x".repeat(2 << 20));
    assert_eq!(testar(http("200 OK", "", &enorme)).await.unwrap_err(), Falha::RespostaInvalida);
}

#[tokio::test]
async fn resposta_lenta_vira_sem_resposta() {
    let escuta = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/models", escuta.local_addr().unwrap());
    tokio::spawn(async move {
        let (_conexao, _) = escuta.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    let falha = listar_modelos_de_prova("opencode-go", &url, CHAVE, Duration::from_millis(300)).await.unwrap_err();
    assert_eq!(falha, Falha::SemResposta);
}

#[tokio::test]
async fn redirecionamento_nao_leva_a_chave_a_outro_endereco() {
    let (alvo, pedidos_do_alvo) = servidor(http("200 OK", "", r#"{"data":[{"id":"x"}]}"#)).await;
    let redireciona = http("302 Found", &format!("location: {alvo}\r\n"), "");
    assert_eq!(testar(redireciona).await.unwrap_err(), Falha::FornecedorFalhou);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(pedidos_do_alvo.locked().is_empty(), "o destino do redirecionamento não pode receber a chave");
}

#[test]
fn o_endereco_de_cada_plano_e_https_do_fornecedor_e_nunca_vem_do_app() {
    for plano in PLANOS {
        let url = reqwest::Url::parse(plano.lista).unwrap();
        assert_eq!(url.scheme(), "https", "{}", plano.id);
        assert_eq!(url.host_str(), Some("opencode.ai"), "{}", plano.id);
        assert!(url.username().is_empty() && url.query().is_none());
    }
    let pedido = serde_json::from_value::<Pedido>(json!({"plano": "opencode-go", "url": "http://127.0.0.1"}));
    assert!(pedido.is_err(), "o pedido não aceita endereço");
}

#[test]
fn o_plano_de_um_modelo_vem_do_prefixo() {
    assert_eq!(plano_do_modelo("opencode-go/glm-5.3").map(|p| p.id), Some("opencode-go"));
    assert!(plano_do_modelo("opencode-go/").is_none());
    assert!(plano_do_modelo("opencode-go").is_none());
    assert!(plano_do_modelo("openai/gpt-6").is_none());
    assert!(plano_do_modelo("").is_none());
}

#[test]
fn chave_colada_fora_do_formato_e_recusada_sem_repetir_a_chave() {
    assert_eq!(chave(Some("  chave-ficticia-boa-789 ")).unwrap().as_str(), "chave-ficticia-boa-789");
    for ruim in [
        "curta",
        "chave com espaço no meio",
        "chave-com\ncontrole",
        "chave-com-\"aspas",
        "chave-com-\\barra",
        "chave-acentuação",
        &"x".repeat(5000),
    ] {
        let erro = chave(Some(ruim)).unwrap_err().to_string();
        assert!(erro.starts_with("chave_formato"), "{erro}");
        assert!(!erro.contains(ruim), "o erro repetiu a chave");
    }
    assert!(chave(None).is_err());
}

#[test]
fn metodos_dos_planos_sao_so_deste_computador() {
    use crate::api::devices::{Caller, permit};
    use crate::store::Scope;
    let aparelho = Caller::Device { key: "k".into(), scopes: vec![Scope::Control, Scope::Screen], autoridade: None };
    for metodo in METHODS {
        assert!(permit(&Caller::Local, metodo).is_ok());
        assert!(permit(&aparelho, metodo).is_err(), "{metodo} não pode vir de um aparelho pareado");
    }
}

pub(super) fn hub_de_prova() -> Arc<Hub> {
    let dir = crate::store::private_fixture_dir("planos-codigo");
    let store = Store::open(&dir.join("test.db")).unwrap();
    *store.secret_key.locked() = Some(Zeroizing::new(vec![7; 32]));
    Hub::new(store, "test".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 1)
}

pub(super) fn bot(id: &str, backend: &str, modelo: &str, space: &str) -> crate::store::BotConfig {
    serde_json::from_value(json!({"id": id, "name": "Teste", "backend": backend, "model": modelo,
        "spaceId": space, "notify": false}))
    .unwrap()
}

const CORPO_OK: &str = r#"{"data":[{"id":"glm-5.3","name":"GLM 5.3"},{"id":"kimi-k3"}]}"#;

#[tokio::test]
async fn chave_aceita_vai_cifrada_ao_cofre_e_a_lista_nunca_leva_a_chave() {
    let hub = hub_de_prova();
    let (url, _) = servidor(http("200 OK", "", CORPO_OK)).await;
    let acesso = salvar(&hub, &PLANOS[0], &url, Zeroizing::new(CHAVE.into())).await.unwrap();
    assert_eq!(acesso["ok"], true, "{acesso}");
    assert_eq!(acesso["estado"], "modelos_no_catalogo");
    assert_eq!(crate::market::vault::read(&hub.store, "plano-codigo/opencode-go").unwrap().as_deref(), Some(CHAVE));
    let cifrado = hub.store.kv_read("plano-codigo/opencode-go").unwrap().unwrap();
    assert!(!cifrado.contains(CHAVE), "a chave tem de ficar cifrada no banco");
    assert!(!acesso.to_string().contains(CHAVE));

    let lista = listar(&hub.store).unwrap();
    assert!(!lista.to_string().contains(CHAVE));
    let go = &lista["planos"][0];
    assert_eq!(
        (go["id"].clone(), go["grupo"].clone(), go["configurada"].clone()),
        (json!("opencode-go"), json!("planos"), json!(true))
    );
    assert_eq!(go["modelos"][0]["id"], "opencode-go/glm-5.3");
    assert_eq!(go["avisos"], json!(["saldo_extra"]));

    let mut catalogo = json!({"models": [{"id": "opencode-go/glm-5.3", "name": "já listado"}]});
    acrescentar_modelos(&hub.store, &mut catalogo).unwrap();
    let ids: Vec<_> = catalogo["models"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["opencode-go/glm-5.3", "opencode-go/kimi-k3"], "sem repetir o que o OpenCode já lista");
}

#[tokio::test]
async fn chave_recusada_nao_e_salva_e_falha_de_rede_tambem_nao() {
    let hub = hub_de_prova();
    for (resposta, erro, estado) in [
        (http("401 Unauthorized", "", &format!(r#"{{"e":"{CHAVE}"}}"#)), "chave_invalida", "recusada"),
        (http("500 Internal Server Error", "", ""), "fornecedor_falhou", "nao_verificado"),
    ] {
        let (url, _) = servidor(resposta).await;
        let acesso = salvar(&hub, &PLANOS[0], &url, Zeroizing::new(CHAVE.into())).await.unwrap();
        assert_eq!(acesso, json!({"ok": false, "erro": erro, "estado": estado}));
        assert!(!acesso.to_string().contains(CHAVE));
        assert_eq!(hub.store.kv_read("plano-codigo/opencode-go").unwrap(), None, "só a chave aceita entra no cofre");
    }
    assert_eq!(listar(&hub.store).unwrap()["planos"][0]["configurada"], false);
}

#[tokio::test]
async fn testar_sem_chave_no_cofre_diz_sem_chave() {
    let hub = hub_de_prova();
    let acesso = call(&hub, &Caller::Local, "planoCodigoTestar", json!({"plano": "opencode-go"})).await.unwrap();
    assert_eq!(acesso["erro"], "sem_chave");
    assert_eq!(acesso["ok"], false);
}

#[tokio::test]
async fn pedidos_invalidos_ou_de_aparelho_pareado_sao_recusados_sem_repetir_a_chave() {
    use crate::store::Scope;
    let hub = hub_de_prova();
    let aparelho = Caller::Device { key: "k".into(), scopes: vec![Scope::Control], autoridade: None };
    let erro = call(&hub, &aparelho, "planoCodigoSalvar", json!({"plano": "opencode-go", "chave": CHAVE}))
        .await
        .unwrap_err()
        .to_string();
    assert!(!erro.contains(CHAVE));
    for corpo in [
        json!({"plano": "outro", "chave": CHAVE}),
        json!({"chave": CHAVE}),
        json!({"plano": "opencode-go", "chave": CHAVE, "x": 1}),
    ] {
        let erro = call(&hub, &Caller::Local, "planoCodigoSalvar", corpo).await.unwrap_err().to_string();
        assert!(!erro.contains(CHAVE), "{erro}");
    }
    assert_eq!(hub.store.kv_read("plano-codigo/opencode-go").unwrap(), None);
}

pub(super) async fn com_chave(hub: &Arc<Hub>) {
    let (url, _) = servidor(http("200 OK", "", CORPO_OK)).await;
    assert_eq!(salvar(hub, &PLANOS[0], &url, Zeroizing::new(CHAVE.into())).await.unwrap()["ok"], true);
}

#[tokio::test]
async fn ambiente_so_para_bot_do_opencode_com_modelo_de_plano() {
    let hub = hub_de_prova();
    com_chave(&hub).await;
    for cfg in [
        bot("b1", "codex", "opencode-go/glm-5.3", crate::fork::spaces::PERSONAL),
        bot("b2", "opencode", "openai/gpt-6", crate::fork::spaces::PERSONAL),
    ] {
        hub.store.save_bot(&cfg).unwrap();
        assert!(ambiente_do_bot(&hub, &cfg).await.unwrap().is_none(), "{}", cfg.id);
    }
    let mut sem_modelo = bot("b3", "opencode", "x", crate::fork::spaces::PERSONAL);
    sem_modelo.model = None;
    assert!(ambiente_do_bot(&hub, &sem_modelo).await.unwrap().is_none());
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut arquivos = Vec::new();
    for entrada in std::fs::read_dir(dir).unwrap().flatten() {
        let caminho = entrada.path();
        if caminho.is_dir() { arquivos.extend(walk(&caminho)) } else { arquivos.push(caminho) }
    }
    arquivos
}

#[tokio::test]
async fn o_ambiente_do_plano_tem_perfil_proprio_e_nunca_a_chave_so_o_codigo_da_sessao() {
    let hub = hub_de_prova();
    com_chave(&hub).await;
    let cfg = bot("b1", "opencode", "opencode-go/glm-5.3", crate::fork::spaces::PERSONAL);
    hub.store.save_bot(&cfg).unwrap();
    let ambiente = ambiente_do_bot(&hub, &cfg).await.unwrap().expect("modelo de plano");
    let env: std::collections::BTreeMap<_, _> = ambiente.env.iter().cloned().collect();

    assert!(env["OPENCODE_API_KEY"].starts_with("mxs_") && env["OPENCODE_API_KEY"] != CHAVE, "só o código de sessão");
    assert!(
        ambiente.env.iter().all(|(nome, valor)| !valor.contains(CHAVE) && !nome.contains(CHAVE)),
        "a chave não está no ambiente"
    );
    assert_eq!(env["OPENCODE_API_KEY"], ambiente.concessao.codigo());
    let configuracao: Value = serde_json::from_str(&env["OPENCODE_CONFIG_CONTENT"]).unwrap();
    assert_eq!(configuracao["provider"]["opencode-go"]["options"]["baseURL"], json!(ambiente.concessao.base()));
    assert!(ambiente.concessao.base().starts_with("http://127.0.0.1:"));
    assert_eq!(env["OPENCODE_DISABLE_PROJECT_CONFIG"], "1", "o opencode.json do projeto não vale no plano");
    assert!(!ambiente.carimbo.contains(CHAVE) && !ambiente.carimbo.contains(ambiente.concessao.codigo()));

    let casa = std::path::PathBuf::from(&env["HOME"]);
    assert!(casa.starts_with(hub.store.engine_setup_root.join("planos").join("opencode-go")), "{casa:?}");
    assert_ne!(Some(casa.clone().into_os_string()), std::env::var_os("HOME"), "a casa do plano não é a do dono");
    for nome in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"] {
        assert!(Path::new(&env[nome]).starts_with(&casa), "{nome}");
        assert!(Path::new(&env[nome]).is_dir(), "{nome}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&casa).unwrap().permissions().mode() & 0o777, 0o700);
    }
    for arquivo in walk(&casa) {
        let conteudo = std::fs::read(&arquivo).unwrap();
        assert!(!conteudo.windows(CHAVE.len()).any(|janela| janela == CHAVE.as_bytes()), "{arquivo:?}");
    }
}

#[tokio::test]
async fn ambiente_fecha_sem_chave_fora_do_space_pessoal_com_comando_proprio_e_para_bot_apagado() {
    let hub = hub_de_prova();
    let pessoal = bot("b1", "opencode", "opencode-go/glm-5.3", crate::fork::spaces::PERSONAL);
    hub.store.save_bot(&pessoal).unwrap();
    let erro = ambiente_do_bot(&hub, &pessoal).await.err().expect("sem chave").to_string();
    assert!(erro.starts_with("plano_sem_chave"), "{erro}");

    com_chave(&hub).await;
    let outro_space = bot("b2", "opencode", "opencode-go/glm-5.3", "space-compartilhado");
    let erro = ambiente_do_bot(&hub, &outro_space).await.err().expect("outro Space").to_string();
    assert!(erro.starts_with("plano_so_pessoal"), "{erro}");
    let nao_gravado = bot("b4", "opencode", "opencode-go/glm-5.3", crate::fork::spaces::PERSONAL);
    assert!(ambiente_do_bot(&hub, &nao_gravado).await.is_err(), "bot que não existe no banco não recebe chave");

    let mut substituto = pessoal.clone();
    substituto.command = Some("meu-wrapper --acp".into());
    let erro = ambiente_do_bot(&hub, &substituto).await.err().expect("comando próprio").to_string();
    assert!(erro.starts_with("plano_comando_proprio"), "{erro}");

    assert!(ambiente_do_bot(&hub, &pessoal).await.unwrap().is_some());
    hub.store.delete_bot("b1").unwrap();
    assert!(ambiente_do_bot(&hub, &pessoal).await.is_err(), "bot apagado não recebe chave");
}

#[tokio::test]
async fn slot_em_texto_claro_nao_e_aceito_nem_migrado() {
    let hub = hub_de_prova();
    let cfg = bot("b1", "opencode", "opencode-go/glm-5.3", crate::fork::spaces::PERSONAL);
    hub.store.save_bot(&cfg).unwrap();
    hub.store.kv_set("plano-codigo/opencode-go", CHAVE).unwrap();
    assert!(ambiente_do_bot(&hub, &cfg).await.is_err(), "texto claro no slot novo não vale");
    assert_eq!(hub.store.kv_read("plano-codigo/opencode-go").unwrap().as_deref(), Some(CHAVE), "e não é regravado");
}

#[tokio::test]
async fn trocar_ou_remover_a_chave_muda_o_carimbo_revoga_as_sessoes_e_apaga_chave_lista_e_perfil() {
    let hub = hub_de_prova();
    let cfg = bot("b1", "opencode", "opencode-go/glm-5.3", crate::fork::spaces::PERSONAL);
    hub.store.save_bot(&cfg).unwrap();
    com_chave(&hub).await;
    let primeiro = ambiente_do_bot(&hub, &cfg).await.unwrap().unwrap();
    let repasse = repasse_do_hub(&hub).await.unwrap();
    assert_eq!(repasse.sessoes_vivas(), 1);
    let perfil = hub.store.engine_setup_root.join("planos").join("opencode-go");
    assert!(perfil.is_dir());

    // Com a sessão viva, troca e remoção são recusadas: a recusa é a confirmação de que há
    // conversa aberta. Chave, lista de modelos e perfil ficam intactos.
    let (url, _) = servidor(http("200 OK", "", CORPO_OK)).await;
    let erro = salvar(&hub, &PLANOS[0], &url, Zeroizing::new(CHAVE.into())).await.unwrap_err().to_string();
    assert!(erro.starts_with("plano_em_uso"), "{erro}");
    let erro = call(&hub, &Caller::Local, "planoCodigoRemover", json!({"plano": "opencode-go"}))
        .await
        .unwrap_err()
        .to_string();
    assert!(erro.starts_with("plano_em_uso"), "{erro}");
    assert_eq!(crate::market::vault::read(&hub.store, "plano-codigo/opencode-go").unwrap().as_deref(), Some(CHAVE));
    assert!(hub.store.kv_read("plano-codigo.modelos/opencode-go").unwrap().is_some());
    assert!(perfil.is_dir());

    // Largada a concessão da sessão velha, as duas operações passam.
    let carimbo_velho = primeiro.carimbo.clone();
    drop(primeiro);
    let segundo = salvar(&hub, &PLANOS[0], &url, Zeroizing::new("outra-chave-ficticia-456".into())).await.unwrap();
    assert_eq!(segundo["ok"], true, "{segundo}");
    assert_eq!(
        crate::market::vault::read(&hub.store, "plano-codigo/opencode-go").unwrap().as_deref(),
        Some("outra-chave-ficticia-456")
    );
    let novo = ambiente_do_bot(&hub, &cfg).await.unwrap().unwrap();
    assert_ne!(carimbo_velho, novo.carimbo, "chave trocada reinicia o processo do OpenCode");
    drop(novo);

    let resposta = call(&hub, &Caller::Local, "planoCodigoRemover", json!({"plano": "opencode-go"})).await.unwrap();
    assert_eq!(resposta, json!({"ok": true}));
    assert_eq!(repasse.sessoes_vivas(), 0, "sem concessão viva a remoção passa e não sobra sessão");
    assert_eq!(hub.store.kv_read("plano-codigo/opencode-go").unwrap(), None);
    assert_eq!(hub.store.kv_read("plano-codigo.modelos/opencode-go").unwrap(), None);
    assert!(!perfil.exists(), "o perfil do plano some com a chave");
    assert!(ambiente_do_bot(&hub, &cfg).await.is_err());
    let lista = listar(&hub.store).unwrap();
    assert_eq!(lista["planos"][0]["configurada"], false);
    assert_eq!(lista["planos"][0]["modelos"], Value::Null);
}

#[tokio::test]
async fn a_troca_e_a_remocao_sao_recusadas_com_abertura_em_curso_sem_processo() {
    let hub = hub_de_prova();
    let cfg = bot("b1", "opencode", "opencode-go/glm-5.3", crate::fork::spaces::PERSONAL);
    hub.store.save_bot(&cfg).unwrap();
    com_chave(&hub).await;
    // Abertura em curso: a concessão saiu do cofre, o processo do OpenCode ainda não existe.
    let ambiente = ambiente_do_bot(&hub, &cfg).await.unwrap().unwrap();
    let repasse = repasse_do_hub(&hub).await.unwrap();
    assert_eq!(repasse.usos_do_plano(&PLANOS[0]), 1);
    let (url, _) = servidor(http("200 OK", "", CORPO_OK)).await;
    let erro = salvar(&hub, &PLANOS[0], &url, Zeroizing::new("outra-chave-ficticia-456".into()))
        .await
        .unwrap_err()
        .to_string();
    assert!(erro.starts_with("plano_em_uso"), "abertura em curso também trava a troca: {erro}");
    let erro = call(&hub, &Caller::Local, "planoCodigoRemover", json!({"plano": "opencode-go"}))
        .await
        .unwrap_err()
        .to_string();
    assert!(erro.starts_with("plano_em_uso"), "{erro}");
    // A concessão que se largou tira o uso: a troca passa na chamada seguinte.
    drop(ambiente);
    assert_eq!(repasse.usos_do_plano(&PLANOS[0]), 0);
    let acesso = salvar(&hub, &PLANOS[0], &url, Zeroizing::new("outra-chave-ficticia-456".into())).await.unwrap();
    assert_eq!(acesso["ok"], true, "{acesso}");
}

#[tokio::test]
async fn uma_concessao_pedida_antes_da_troca_nunca_fica_viva() {
    let hub = hub_de_prova();
    com_chave(&hub).await;
    let repasse = repasse_do_hub(&hub).await.unwrap();
    let geracao = repasse.geracao(&PLANOS[0]);
    com_chave(&hub).await;
    let erro = repasse.conceder(&PLANOS[0], Zeroizing::new(CHAVE.into()), geracao).err().expect("geração velha");
    assert!(erro.to_string().starts_with("plano_em_troca"), "{erro}");
    assert_eq!(repasse.sessoes_vivas(), 0);
}

#[tokio::test]
async fn resposta_de_sucesso_que_repete_a_chave_nao_e_devolvida_nem_guardada() {
    let hub = hub_de_prova();
    for corpo in [
        format!(r#"{{"data":[{{"id":"{CHAVE}"}}]}}"#),
        format!(r#"{{"data":[{{"id":"ok","name":"nome com {CHAVE}"}}]}}"#),
        format!(r#"{{"data":[{{"id":"ok"}}],"eco":"{CHAVE}"}}"#),
        format!(
            r#"{{"data":[{{"id":"{}"}}]}}"#,
            CHAVE.bytes().fold(String::new(), |mut s, b| {
                use std::fmt::Write as _;
                let _ = write!(s, "\\u{b:04x}");
                s
            })
        ),
        format!(
            r#"{{"data":[{{"id":"ok","name":"{}"}}]}}"#,
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, format!("x{CHAVE}"))
        ),
    ] {
        let (url, _) = servidor(http("200 OK", "", &corpo)).await;
        let acesso = salvar(&hub, &PLANOS[0], &url, Zeroizing::new(CHAVE.into())).await.unwrap();
        assert_eq!(acesso, json!({"ok": false, "erro": "resposta_invalida", "estado": "nao_verificado"}));
        assert!(!acesso.to_string().contains(CHAVE));
    }
    assert_eq!(hub.store.kv_read("plano-codigo/opencode-go").unwrap(), None);
    assert_eq!(hub.store.kv_read("plano-codigo.modelos/opencode-go").unwrap(), None);
}

#[test]
fn o_codigo_de_sessao_e_mascarado_em_texto_que_o_hub_registra() {
    let codigo = format!("mxs_{}", "ab12".repeat(16));
    assert_eq!(mascarar_codigo("nada aqui"), "nada aqui");
    assert_eq!(mascarar_codigo(&format!("erro com {codigo} no meio")), "erro com mxs_[oculto] no meio");
    assert_eq!(mascarar_codigo(&format!("{codigo}{codigo}")), "mxs_[oculto]mxs_[oculto]");
    assert_eq!(mascarar_codigo("mxs_curto e mxs_"), "mxs_curto e mxs_", "o que não é um código inteiro não muda");
    let mut json = json!({"error": {"message": format!("falhou {codigo}"), "data": [codigo.clone(), 1]}});
    mascarar_json(&mut json);
    assert!(!json.to_string().contains(&codigo));
}

#[test]
fn texto_de_conversa_segura_so_o_fim_que_pode_ser_o_comeco_de_um_codigo() {
    let codigo = format!("mxs_{}", "ab12".repeat(16));
    for corte in [1, 3, 4, 5, 20, 67] {
        let texto = format!("abc {}", &codigo[..corte]);
        assert_eq!(mascarar_saida(&texto, false), "abc ", "corte {corte}");
        assert_eq!(mascarar_saida(&texto, true), texto, "no fim o que sobra não é código");
    }
    assert_eq!(mascarar_saida(&format!("abc {codigo}"), false), "abc mxs_[oculto]");
    assert_eq!(mascarar_saida("abc m", false), "abc ");
    assert_eq!(mascarar_saida("abc mxs_[oculto] xyz", false), "abc mxs_[oculto] xyz");
    assert_eq!(mascarar_saida("ação mxs", false), "ação ");
}

#[tokio::test]
async fn codigo_em_escapes_de_json_e_em_mensagem_de_erro_do_motor_nao_chega_ao_erro_nem_ao_texto() {
    let codigo = format!("mxs_{}", "ab12".repeat(16));
    let escapado = format!("\\u006dxs_{}", &codigo[4..]);
    let pasta = crate::store::private_fixture_dir("planos-motor-de-mentira");
    let roteiro = pasta.join("motor.sh");
    std::fs::write(
        &roteiro,
        format!(
            "printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{{\"texto\":\"{escapado}\"}}}}'\n\
             read linha\n\
             printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{{\"code\":-1,\"message\":\"falhou {escapado}\"}}}}'\n\
             sleep 5\n"
        ),
    )
    .unwrap();
    let (acp, mut entrada) =
        crate::agent::acp::Acp::spawn(&format!("sh {}", roteiro.display()), &pasta.to_string_lossy(), &[]).unwrap();
    let Some(crate::agent::acp::Incoming::Notification { params, .. }) =
        tokio::time::timeout(Duration::from_secs(5), entrada.recv()).await.unwrap()
    else {
        panic!("notificação esperada");
    };
    assert!(!params.to_string().contains(&codigo), "o código escrito em escapes não passa do parse: {params}");
    assert!(params.to_string().contains("mxs_[oculto]"));
    let erro = acp.request("x", json!({})).await.unwrap_err().to_string();
    assert!(!erro.contains(&codigo), "{erro}");
    assert!(erro.contains("mxs_[oculto]"), "{erro}");
    acp.kill_and_wait().await;
}

#[test]
fn o_codigo_como_nome_de_campo_tambem_e_mascarado() {
    let codigo = format!("mxs_{}", "ab12".repeat(16));
    let mut json: Value = serde_json::from_str(&format!(
        r#"{{"{codigo}":1,"objeto":{{"{codigo}":"texto"}},"lista":[{{"outra":2}},{{"{codigo}":3}}]}}"#
    ))
    .unwrap();
    mascarar_json(&mut json);
    assert_eq!(json["mxs_[oculto]"], 1, "o nome do campo de cima sai mascarado");
    assert_eq!(json["objeto"]["mxs_[oculto]"], "texto", "o do objeto aninhado também");
    assert_eq!(json["lista"][0]["outra"], 2, "o que não é código não muda");
    assert_eq!(json["lista"][1]["mxs_[oculto]"], 3, "o de dentro do array de objetos também");
    assert!(!json.to_string().contains(&codigo));
}

#[test]
fn mascara_codigo_colisao_de_nomes_de_campo_mantem_os_dois_campos() {
    let um = "mxs_[oculto]";
    let dois = format!("mxs_{}", "ab12".repeat(16));
    let tres = format!("mxs_{}", "cd56".repeat(16));
    // Três nomes distintos que a máscara iguala: nenhum campo pode sumir por causa da colisão.
    let mut json: Value = serde_json::from_str(&format!(r#"{{"{um}":1,"{dois}":2,"{tres}":3,"outro":4}}"#)).unwrap();
    mascarar_json(&mut json);
    assert_eq!(json["mxs_[oculto]"], 1, "o primeiro nome mascarado fica com a base");
    assert_eq!(json["mxs_[oculto]#2"], 2, "o segundo ganha o sufixo de índice");
    assert_eq!(json["mxs_[oculto]#3"], 3, "e o terceiro o seguinte");
    assert_eq!(json["outro"], 4, "o nome sem colisão não muda");
    assert!(!json.to_string().contains(&dois) && !json.to_string().contains(&tres));
    assert_eq!(json.as_object().unwrap().len(), 4, "nenhum campo foi perdido");
}

#[tokio::test]
async fn codigo_em_escapes_no_nome_do_campo_nao_passa_do_leitor_do_acp() {
    let codigo = format!("mxs_{}", "ab12".repeat(16));
    let escapado = format!("\\u006dxs_{}", &codigo[4..]);
    let pasta = crate::store::private_fixture_dir("planos-motor-nome-escapado");
    let roteiro = pasta.join("motor.sh");
    std::fs::write(
        &roteiro,
        format!(
            "printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{{\"{escapado}\":\"ok\"}}}}'\n\
             read linha\n\
             sleep 5\n"
        ),
    )
    .unwrap();
    let (acp, mut entrada) =
        crate::agent::acp::Acp::spawn(&format!("sh {}", roteiro.display()), &pasta.to_string_lossy(), &[]).unwrap();
    let Some(crate::agent::acp::Incoming::Notification { params, .. }) =
        tokio::time::timeout(Duration::from_secs(5), entrada.recv()).await.unwrap()
    else {
        panic!("notificação esperada");
    };
    assert!(!params.to_string().contains(&codigo), "o código em escapes vira nome de campo no parse: {params}");
    assert!(params.to_string().contains("mxs_[oculto]"), "{params}");
    acp.kill_and_wait().await;
}
