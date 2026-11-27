//! MEX-1447: fronteiras do "app Google da pessoa": a chave secreta nunca sai do cofre (resposta, recibo, conta pública,
//! `Debug`), o app de um Space não serve a outro, trocar o app encerra o login das contas Google do Space e conectar sem
//! app cadastrado recusa com o código que abre o guia. Dados sintéticos; nenhuma rede.
use super::contas::{self, Pedido};
use super::{Connector, connectors, google_app, oauth, update_connectors};
use crate::LockExt;
use crate::api::devices::Caller;
use crate::fork::spaces::PERSONAL;
use crate::hub::Hub;
use serde_json::json;
use std::sync::Arc;

const ID: &str = "1048293716-q8f3m2xk.apps.googleusercontent.com";
const CHAVE: &str = "GOCSPX-chave-sintetica-de-teste-0001";

fn hub() -> Arc<Hub> {
    crate::fork::conectores::oauth_binding::tests::hub()
}

async fn novo_space(hub: &Arc<Hub>, nome: &str) -> String {
    let atual = hub.store.spaces().unwrap();
    let r = crate::api::dispatch(
        hub,
        &Caller::Local,
        "donnaSpaceCreate",
        json!({"hubId": hub.host_id, "expectedRevision": atual.revision, "name": nome,
            "tile": {"color": "oceano", "style": "soft", "symbol": {"kind": "letters", "value": "TR"}}}),
    )
    .await
    .unwrap();
    r["spaces"].as_array().unwrap().last().unwrap()["id"].as_str().unwrap().to_owned()
}

fn pedido<'a>(servico: &'a str, tipo: &'a str, space: &'a str) -> Pedido<'a> {
    Pedido {
        servico,
        tipo,
        space,
        nome: None,
        nome_primeira: None,
        liberacoes: None,
        aceita_credencial_com_escrita: false,
    }
}

fn gmail(space: &str) -> Pedido<'_> {
    pedido("gmail", "gmail.oficial_app", space)
}

fn erro<T: std::fmt::Debug>(r: anyhow::Result<T>) -> String {
    r.unwrap_err().to_string()
}

/// Tudo o que o Hub grava ou devolve em claro sobre o app: respostas, recibos e a tabela `kv` crua.
fn tudo_em_claro(hub: &Hub) -> String {
    let db = hub.store.db.locked();
    let mut saida = String::new();
    let mut st = db.prepare("SELECT acao||conta||servico||space||resultado FROM mexerica_conta_recibos").unwrap();
    for linha in st.query_map([], |r| r.get::<_, String>(0)).unwrap() {
        saida.push_str(&linha.unwrap());
    }
    let mut st = db.prepare("SELECT k||v FROM kv").unwrap();
    for linha in st.query_map([], |r| r.get::<_, String>(0)).unwrap() {
        saida.push_str(&linha.unwrap());
    }
    saida
}

#[tokio::test]
async fn app_proprio_da_ficha_isola_space_e_encerra_apenas_os_logins_daquele_tipo() {
    let hub = hub();
    let store = &hub.store;
    let ficha = crate::fork::conectores::catalogo_servicos::fixture_app_proprio();
    let tipo = &ficha.tipos[0];
    let app = google_app::AppDaFicha::novo(ficha, tipo).unwrap();
    let outro = novo_space(&hub, "Outro").await;
    let hoje = chrono::NaiveDate::parse_from_str(ficha.conferida_em, "%Y-%m-%d").unwrap();
    let literal = pedido(ficha.id, tipo.id, PERSONAL);
    assert!(erro(contas::conectar_em(store, ficha, &literal, hoje)).starts_with("app_da_ficha_ausente:"));
    let salvo = app.escrever(store, PERSONAL, Some((ID, CHAVE))).unwrap();
    assert_eq!(salvo, json!({"configurado": true}));
    assert_eq!(app.publico(store, &outro).unwrap(), json!({"configurado": false}));
    assert!(google_app::AppDaFicha::resolver("gmail", "gmail.oficial_app").is_err());
    let claro = tudo_em_claro(&hub);
    assert!(!claro.contains(CHAVE) && !claro.contains(ID));
    for space in [PERSONAL, &outro] {
        app.escrever(store, space, Some((ID, CHAVE))).unwrap();
        contas::conectar_em(store, ficha, &pedido(ficha.id, tipo.id, space), hoje).unwrap();
    }
    update_connectors(store, |all| {
        all.iter_mut().find(|c| c.space == PERSONAL).unwrap().oauth = None;
        Ok(())
    })
    .unwrap();
    let snapshot = connectors(store).unwrap().into_iter().find(|c| c.space == PERSONAL).unwrap();
    app.escrever(store, PERSONAL, Some(("outro-id-ficticio", CHAVE))).unwrap();
    assert!(crate::fork::conectores::oauth_binding::change(store, &snapshot, None, || Ok(()), |_| Ok(())).is_err());
    update_connectors(store, |all| {
        for c in all {
            c.oauth = Some(oauth::OAuth { access_token: "fixture-access".into(), ..Default::default() });
        }
        Ok(())
    })
    .unwrap();
    app.escrever(store, PERSONAL, None).unwrap();
    for c in connectors(store).unwrap() {
        assert_eq!(c.oauth.as_ref().unwrap().signed_in(), c.space == outro);
    }
    assert!(app.ler(store, PERSONAL).unwrap().is_none());
    assert!(app.ler(store, &outro).unwrap().is_some());
}

#[tokio::test]
async fn a_chave_secreta_nunca_sai_do_cofre() {
    let hub = hub();
    let store = &hub.store;
    let resposta = google_app::salvar(store, PERSONAL, ID, CHAVE).unwrap();
    assert_eq!(resposta, json!({"configurado": true, "clientId": ID}));
    assert_eq!(google_app::publico(store, PERSONAL).unwrap(), resposta, "a leitura devolve só o ID");
    assert!(!resposta.to_string().contains(CHAVE));
    // O registro está cifrado; recibos e o resto do banco não têm a chave nem o ID.
    let cru = store.kv_read(&format!("google_app.{PERSONAL}")).unwrap().unwrap();
    assert!(cru.starts_with("vault:v1:") && !cru.contains(CHAVE) && !cru.contains(ID));
    let claro = tudo_em_claro(&hub);
    assert!(!claro.contains(CHAVE) && !claro.contains("GOCSPX") && !claro.contains(ID), "{claro}");
    assert!(claro.contains("google_app_salvar"), "o recibo existe");
    // Conta pública e `Debug` não carregam a chave.
    let app = google_app::ler(store, PERSONAL).unwrap().unwrap();
    assert!(!format!("{app:?}").contains(CHAVE));
    contas::conectar(store, &gmail(PERSONAL)).unwrap();
    for c in connectors(store).unwrap() {
        assert!(!c.public().to_string().contains(CHAVE));
    }
    assert!(!contas::listar(store, PERSONAL).unwrap().to_string().contains(CHAVE));
}

#[test]
fn id_e_chave_invalidos_sao_recusados_sem_eco_do_valor() {
    let hub = hub();
    let store = &hub.store;
    for (id, chave) in [
        ("abc", CHAVE),
        ("1048293716-q8f3m2xk.apps.googleusercontent.com.evil.example", CHAVE),
        ("1048293716 q8f3m2xk.apps.googleusercontent.com", CHAVE),
        (ID, "curta"),
        (ID, "GOCSPX com espaco no meio da chave"),
        (ID, "GOCSPX-quebra\nde-linha-na-chave"),
        ("", CHAVE),
        (ID, ""),
    ] {
        let e = erro(google_app::salvar(store, PERSONAL, id, chave));
        assert!(e.starts_with("app_google_invalido"), "{e}");
        assert!(!e.contains("GOCSPX") && !e.contains("curta"), "a recusa não repete o valor: {e}");
    }
    assert!(google_app::ler(store, PERSONAL).unwrap().is_none(), "recusa não grava nada");
    // Espaço sobrando na colagem é aparado.
    google_app::salvar(store, PERSONAL, &format!("  {ID}\n"), &format!(" {CHAVE} ")).unwrap();
    assert_eq!(google_app::ler(store, PERSONAL).unwrap().unwrap().client_secret, CHAVE);
}

#[tokio::test]
async fn o_app_de_um_space_nao_serve_a_outro() {
    let hub = hub();
    let store = &hub.store;
    let outro = novo_space(&hub, "Trabalho").await;
    google_app::salvar(store, PERSONAL, ID, CHAVE).unwrap();
    assert!(google_app::ler(store, &outro).unwrap().is_none());
    assert_eq!(google_app::publico(store, &outro).unwrap(), json!({"configurado": false, "clientId": null}));
    // Copiar o registro selado para o slot do outro Space não o abre: o slot é parte do selo.
    let selado = store.kv_read(&format!("google_app.{PERSONAL}")).unwrap().unwrap();
    store.kv_set(&format!("google_app.{outro}"), &selado).unwrap();
    assert!(google_app::ler(store, &outro).is_err(), "registro copiado de outro Space não abre");
    store.kv_delete(&format!("google_app.{outro}")).unwrap();
    // Conectar no Space sem app abre o guia; no Space com app, conecta.
    let e = erro(contas::conectar(store, &gmail(&outro)));
    assert!(e.starts_with("app_proprio_ausente"), "{e}");
    assert!(connectors(store).unwrap().is_empty(), "recusa não cria conta");
    let r = contas::conectar(store, &gmail(PERSONAL)).unwrap();
    assert_eq!(r["proximoPasso"], "entrar");
    assert_eq!(erro(google_app::para_a_conta(store, &outro)).split(':').next(), Some("app_proprio_ausente"));
}

#[test]
fn conectar_sem_app_cadastrado_recusa_com_o_codigo_que_abre_o_guia_e_nao_grava_nada() {
    let hub = hub();
    for (servico, tipo) in [
        ("gmail", "gmail.oficial_app"),
        ("google-agenda", "google-agenda.oficial_app"),
        ("google-drive", "google-drive.oficial_app"),
        ("google-docs", "google-docs.oficial_app"),
        ("google-planilhas", "google-planilhas.oficial_app"),
    ] {
        let e = erro(contas::conectar(&hub.store, &pedido(servico, tipo, PERSONAL)));
        assert!(e.starts_with("app_proprio_ausente"), "{servico}: {e}");
    }
    assert_eq!(connectors(&hub.store).unwrap().len(), 0);
}

#[test]
fn a_conta_google_nasce_so_leitura_com_o_endereco_e_os_escopos_da_ficha() {
    let hub = hub();
    let store = &hub.store;
    google_app::salvar(store, PERSONAL, ID, CHAVE).unwrap();
    let r = contas::conectar(store, &pedido("gmail", "gmail.oficial_app_self", PERSONAL)).unwrap();
    assert_eq!(r["connector"]["liberacoes"], json!(["ler"]));
    assert_eq!(r["connector"]["url"], "https://gmailmcp.googleapis.com/mcp/v1self");
    assert_eq!(r["connector"]["auth"], "signedOut");
    let conta = connectors(store).unwrap().remove(0);
    assert_eq!(conta.oauth, Some(oauth::OAuth::default()), "o ID e a chave só entram no login");
    assert_eq!(
        contas::escopos_da_conta(&conta).unwrap().unwrap(),
        ["https://www.googleapis.com/auth/gmail.readonly"],
        "só os escopos dos grupos ligados"
    );
    assert!(matches!(
        contas::auth_proprio(&conta).unwrap(),
        Some(crate::fork::conectores::catalogo_servicos::Auth::AppProprio {
            emissor: "https://accounts.google.com",
            configuracao: None,
            ..
        })
    ));
    // A escrita que pode apagar não liga; grupo inventado também não.
    for ruim in ["criar_editar", "admin"] {
        let mut p = pedido("google-agenda", "google-agenda.oficial_app", PERSONAL);
        p.liberacoes = Some(vec![ruim.to_owned()]);
        assert!(contas::conectar(store, &p).is_err(), "{ruim}");
    }
    // Escrever (rascunhos no Gmail) fica indisponível até o filtro de ações perigosas: não liga, nem pedindo.
    let mut p = pedido("gmail", "gmail.oficial_app", PERSONAL);
    p.nome = Some("Trabalho");
    p.nome_primeira = Some("Pessoal");
    p.liberacoes = Some(vec!["ler".into(), "rascunhos".into()]);
    assert!(contas::conectar(store, &p).is_err(), "rascunhos não liga");
    // A 2ª conta do mesmo serviço, só lendo, pede nome para as duas e pede só o escopo de ler.
    p.liberacoes = Some(vec!["ler".into()]);
    contas::conectar(store, &p).unwrap();
    let segunda = connectors(store).unwrap().into_iter().find(|c| c.name == "Trabalho").unwrap();
    assert_eq!(segunda.url.as_deref(), Some("https://gmailmcp.googleapis.com/mcp/v1"));
    assert_ne!(segunda.id, conta.id);
    assert_eq!(
        contas::escopos_da_conta(&segunda).unwrap().unwrap(),
        ["https://www.googleapis.com/auth/gmail.readonly"]
    );
}

#[test]
fn trocar_ou_remover_o_app_encerra_so_o_login_das_contas_google_do_space() {
    let hub = hub();
    let store = &hub.store;
    google_app::salvar(store, PERSONAL, ID, CHAVE).unwrap();
    contas::conectar(store, &gmail(PERSONAL)).unwrap();
    contas::conectar(store, &pedido("linear", "linear.oficial_oauth", PERSONAL)).unwrap();
    // Só os tokens mudam (cadastro igual): mexer no cadastro de fora já encerraria o login pela regra do vínculo.
    let entrou = |c: &mut Connector| {
        let o = c.oauth.get_or_insert_with(oauth::OAuth::default);
        o.access_token = "token-a".into();
        o.refresh_token = Some("token-r".into());
    };
    update_connectors(store, |all| {
        all.iter_mut().for_each(entrou);
        Ok(())
    })
    .unwrap();
    google_app::salvar(store, PERSONAL, "2222222222-outro.apps.googleusercontent.com", CHAVE).unwrap();
    let todas = connectors(store).unwrap();
    let google = todas.iter().find(|c| c.servico.as_deref() == Some("gmail")).unwrap();
    assert_eq!(google.oauth, Some(oauth::OAuth::default()), "login do Google encerrado, sem segredo para trás");
    let linear = todas.iter().find(|c| c.servico.as_deref() == Some("linear")).unwrap();
    assert!(linear.oauth.as_ref().is_some_and(oauth::OAuth::signed_in), "outros serviços não são tocados");
    update_connectors(store, |all| {
        all.iter_mut().for_each(entrou);
        Ok(())
    })
    .unwrap();
    google_app::remover(store, PERSONAL).unwrap();
    assert!(google_app::ler(store, PERSONAL).unwrap().is_none());
    assert!(
        !connectors(store)
            .unwrap()
            .iter()
            .find(|c| c.servico.as_deref() == Some("gmail"))
            .unwrap()
            .oauth
            .as_ref()
            .unwrap()
            .signed_in()
    );
    let claro = tudo_em_claro(&hub);
    assert!(claro.contains("google_app_remover") && !claro.contains(CHAVE));
}

#[tokio::test]
async fn trocar_o_app_de_um_space_nao_mexe_nas_contas_de_outro() {
    let hub = hub();
    let store = &hub.store;
    let outro = novo_space(&hub, "Trabalho").await;
    google_app::salvar(store, PERSONAL, ID, CHAVE).unwrap();
    google_app::salvar(store, &outro, "3333333333-b.apps.googleusercontent.com", CHAVE).unwrap();
    contas::conectar(store, &gmail(&outro)).unwrap();
    update_connectors(store, |all| {
        all[0].oauth.as_mut().unwrap().access_token = "token-a".into();
        Ok(())
    })
    .unwrap();
    google_app::salvar(store, PERSONAL, "4444444444-c.apps.googleusercontent.com", CHAVE).unwrap();
    assert!(connectors(store).unwrap()[0].oauth.as_ref().unwrap().signed_in(), "o login do outro Space segue");
}

#[test]
fn cofre_trancado_vira_codigo_estavel_e_nao_grava_nada() {
    let hub = hub();
    let store = &hub.store;
    *store.secret_key.locked() = None;
    for e in [
        erro(google_app::salvar(store, PERSONAL, ID, CHAVE)),
        erro(google_app::remover(store, PERSONAL)),
        erro(google_app::ler(store, PERSONAL)),
        erro(contas::conectar(store, &gmail(PERSONAL))),
    ] {
        assert!(e.starts_with("cofre_trancado") || e.contains("Secure credential storage"), "{e}");
    }
    assert!(erro(google_app::salvar(store, PERSONAL, ID, CHAVE)).starts_with("cofre_trancado"));
    assert!(!tudo_em_claro(&hub).contains(CHAVE));
}

// MARK: achados da revisão de Segurança (MEX-1447, #1018)

/// P2-1: o registro do app Google só abre selado, para o próprio Space. Texto claro, sem selo e "migração" não existem.
#[tokio::test]
async fn registro_em_texto_claro_ou_de_outro_space_nunca_abre() {
    let hub = hub();
    let store = &hub.store;
    let outro = novo_space(&hub, "Trabalho").await;
    let em_claro = json!({"clientId": ID, "clientSecret": CHAVE}).to_string();
    // Texto claro direto no slot: recusado, não migrado, e o registro fica como estava.
    store.kv_set(&format!("google_app.{PERSONAL}"), &em_claro).unwrap();
    let e = erro(google_app::ler(store, PERSONAL));
    assert!(e.starts_with("app_google_ilegivel"), "{e}");
    assert!(!e.contains(CHAVE) && !e.contains(ID), "{e}");
    assert_eq!(
        store.kv_read(&format!("google_app.{PERSONAL}")).unwrap().as_deref(),
        Some(em_claro.as_str()),
        "nada é migrado"
    );
    assert!(google_app::publico(store, PERSONAL).is_none(), "a tela não vê um registro inválido como app");
    assert!(erro(contas::conectar(store, &gmail(PERSONAL))).starts_with("app_google_ilegivel"));
    // Selado para outro Space e copiado para este: recusado.
    store.kv_delete(&format!("google_app.{PERSONAL}")).unwrap();
    google_app::salvar(store, &outro, ID, CHAVE).unwrap();
    let selado = store.kv_read(&format!("google_app.{outro}")).unwrap().unwrap();
    store.kv_set(&format!("google_app.{PERSONAL}"), &selado).unwrap();
    assert!(erro(google_app::ler(store, PERSONAL)).starts_with("app_google_ilegivel"));
    // O selado no próprio slot continua abrindo.
    assert_eq!(google_app::ler(store, &outro).unwrap().unwrap().client_id, ID);
}

/// P3-1: nem `{:?}` do OAuth da conta imprime a chave do app ou os tokens.
#[test]
fn debug_do_oauth_nao_imprime_chave_nem_tokens() {
    let o = oauth::OAuth {
        client_id: "cliente-publico".into(),
        client_secret_basic: false,
        client_secret: Some("chave-secreta-sintetica".into()),
        redirect_uris: vec!["http://127.0.0.1:1/oauth/callback".into()],
        token_endpoint: "https://oauth.exemplo.invalid/token".into(),
        access_token: "token-de-acesso-sintetico".into(),
        refresh_token: Some("token-de-renovacao-sintetico".into()),
        expires_at: Some(1),
    };
    for texto in [format!("{o:?}"), format!("{o:#?}")] {
        for segredo in ["chave-secreta-sintetica", "token-de-acesso-sintetico", "token-de-renovacao-sintetico"] {
            assert!(!texto.contains(segredo), "{texto}");
        }
        assert!(!texto.contains("cliente-publico") && texto.contains("<oculto>"));
    }
    assert!(format!("{:?}", oauth::OAuth::default()).contains("<vazio>"));
}

/// P3-2: o Google só volta pelo loopback; o retorno pela página da nuvem é recusado antes de qualquer rede.
#[tokio::test]
async fn login_do_google_recusa_o_retorno_pela_nuvem() {
    use crate::fork::conectores::oauth_binding::StartIntent;
    let hub = hub();
    // Com a nuvem ligada o retorno pela página dela seria possível para qualquer outra conta; para o Google, não.
    hub.store.kv_set("test_cloud_url", "https://fixture.invalid").unwrap();
    google_app::salvar(&hub.store, PERSONAL, ID, CHAVE).unwrap();
    contas::conectar(&hub.store, &gmail(PERSONAL)).unwrap();
    let id = connectors(&hub.store).unwrap().remove(0).id;
    let intent = StartIntent::capture(&hub, &Caller::Local, &id).unwrap();
    let e = oauth::start(&hub, intent, oauth::Callback::App).await.unwrap_err().to_string();
    assert_eq!(e, oauth::APP_PROPRIO_SO_LOOPBACK);
    // A recusa não deixa login pendente nem mexe na conta.
    assert_eq!(connectors(&hub.store).unwrap()[0].oauth, Some(oauth::OAuth::default()));
}

/// P3-5: `marketGoogleApp` é do dono, como salvar e remover: aparelho sem a autoridade ou revogado não o lê.
#[tokio::test]
async fn market_google_app_e_do_dono_e_aparelho_revogado_nao_le() {
    use crate::store::{Device, DeviceSource, Scope};
    let hub = hub();
    google_app::salvar(&hub.store, PERSONAL, ID, CHAVE).unwrap();
    let device = Device {
        key: "mac-google".into(),
        name: "Mac".into(),
        platform: "macos".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: crate::store::now_ms(),
        last_seen_at: None,
    };
    hub.store.put_device(&device).unwrap();
    let (_, mac) = crate::fork::autoridade::autenticar(&hub, &device.key).unwrap();
    let r = crate::api::dispatch(&hub, &mac, "marketGoogleApp", json!({})).await.unwrap();
    assert_eq!(r, json!({"configurado": true, "clientId": ID}));
    hub.revoke_device(&device.key).unwrap();
    assert!(crate::api::dispatch(&hub, &mac, "marketGoogleApp", json!({})).await.is_err());
    assert!(!r.to_string().contains(CHAVE));
}
