#![allow(clippy::unwrap_used)]
//! D395 (D70): o idioma vem só do app de quem escreve, cada um grava só o seu, e o Berga responde no
//! idioma da última mensagem humana; canal, `WhatsApp` e rotina seguem o dono; sem nada, pt-BR.
use super::{APARELHO, IDIOMAS, METHOD, do_chamador, do_dono, do_turno, turno};
use crate::api::{devices::Caller, dispatch};
use crate::hub::Hub;
use crate::remote::identity::Identity;
use crate::store::{BotConfig, Device, DeviceSource, EntryKind, Lane, Scope, Store};
use serde_json::{Value, json};
use std::path::Path;

fn bot(id: &str, name: &str) -> BotConfig {
    serde_json::from_value(json!({"id":id,"name":name,"backend":"codex"})).unwrap()
}

fn hub() -> (std::sync::Arc<Hub>, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("donna-idioma-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(Path::new(":memory:")).unwrap();
    store.save_bot(&bot("donna", "Donna")).unwrap();
    store.save_bot(&bot("dev", "Dev")).unwrap();
    store.set_team_settings(&json!({"primaryBotId":"donna"})).unwrap();
    (Hub::new(store, "donna-idioma".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0), dir)
}

fn aparelho(hub: &Hub, key: &str, scopes: &[Scope]) -> Caller {
    // Reconcessão de aparelho Local = o passo do pareamento por código (`readmitir_pareado`), que libera o bloqueio e grava o aparelho numa Tx.
    crate::fork::workspace::readmitir_pareado(
        &hub.store,
        &Device {
            key: key.into(),
            name: key.into(),
            platform: "ios".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: scopes.to_vec(),
            lease_until: None,
            created_at: crate::store::now_ms(),
            last_seen_at: None,
        },
    )
    .unwrap();
    // Autenticação real: a autoridade original do transporte, como no canal do aparelho.
    crate::fork::autoridade::autenticar(hub, key).unwrap().1
}

fn donna(hub: &Hub) -> BotConfig {
    hub.store.bot("donna").unwrap().unwrap().config
}

async fn salvar(hub: &std::sync::Arc<Hub>, caller: &Caller, body: Value) -> anyhow::Result<Value> {
    dispatch(hub, caller, METHOD, body).await
}

/// Envia como `caller` e devolve o id da entrada gravada.
async fn enviar(hub: &std::sync::Arc<Hub>, caller: &Caller, body: Value) -> String {
    let mut body = body;
    body["botId"] = json!("donna");
    let sent = dispatch(hub, caller, "send", body).await.unwrap();
    sent["entry"]["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn seis_idiomas_e_o_padrao_portugues() {
    let (hub, dir) = hub();
    // Sem idioma conhecido: português do Brasil.
    assert_eq!(do_dono(&hub.store), "pt-BR");
    assert!(turno(&hub.store, &donna(&hub), &[], false).contains("português do Brasil (pt-BR)"));
    for (codigo, nome) in IDIOMAS {
        assert_eq!(salvar(&hub, &Caller::Local, json!({"language":codigo})).await.unwrap()["language"], codigo);
        assert_eq!(do_dono(&hub.store), codigo);
        let linha = turno(&hub.store, &donna(&hub), &[], false);
        assert!(linha.contains(&format!("{nome} ({codigo})")), "{codigo}: {linha}");
    }
    // Só os seis, escritos exatamente; nada além do idioma no corpo.
    for body in [
        json!({"language":"de"}),
        json!({"language":"PT-BR"}),
        json!({"language":"pt"}),
        json!({}),
        json!({"language":"en","bot":"x"}),
    ] {
        assert!(salvar(&hub, &Caller::Local, body.clone()).await.is_err(), "{body}");
    }
    assert_eq!(do_dono(&hub.store), "ar-AE");
    // A base fixa troca só a frase do idioma.
    let base = crate::fork::donna_fixa::BASE;
    assert!(base.contains("Responda no idioma do app da pessoa (pt-BR, en, fr, es, zh-CN ou ar-AE)"));
    assert!(!base.contains("Responda em português do Brasil"));
    // Os outros bots seguem como antes: sem linha de idioma.
    assert_eq!(turno(&hub.store, &hub.store.bot("dev").unwrap().unwrap().config, &[], false), "");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn cada_app_grava_so_o_proprio_e_revogar_apaga() {
    let (hub, dir) = hub();
    // Aparelho sem Control grava o próprio idioma (preferência dele, não poder sobre o Berga).
    let fone = aparelho(&hub, "fone", &[]);
    salvar(&hub, &fone, json!({"language":"fr"})).await.unwrap();
    assert_eq!(do_chamador(&hub.store, &fone), Some("fr"));
    assert_eq!(do_dono(&hub.store), "pt-BR", "o aparelho não muda o idioma do dono");
    // Revogar apaga a chave do aparelho no mesmo passo.
    assert!(hub.revoke_device("fone").unwrap());
    assert_eq!(hub.store.kv_read(&format!("{APARELHO}fone")).unwrap(), None);
    assert_eq!(do_chamador(&hub.store, &fone), None);
    // SEG-486-01 (b): concessão nova na mesma chave não herda o idioma antigo, nem se o valor antigo
    // for recriado (pedido velho em corrida com a revogação): a geração gravada não é a atual.
    let antigo = aparelho(&hub, "tablet", &[]);
    salvar(&hub, &antigo, json!({"language":"es"})).await.unwrap();
    let valor = hub.store.kv_read(&format!("{APARELHO}tablet")).unwrap().unwrap();
    assert!(hub.revoke_device("tablet").unwrap());
    let novo = aparelho(&hub, "tablet", &[Scope::Control]);
    hub.store.kv_set(&format!("{APARELHO}tablet"), &valor).unwrap();
    assert_eq!(do_chamador(&hub.store, &novo), None, "a concessão nova não herda o idioma antigo");
    salvar(&hub, &novo, json!({"language":"zh-CN"})).await.unwrap();
    assert_eq!(do_chamador(&hub.store, &novo), Some("zh-CN"));
    // O pedido antigo, já sem concessão, não grava por cima.
    assert!(salvar(&hub, &antigo, json!({"language":"es"})).await.is_err());
    assert_eq!(do_chamador(&hub.store, &novo), Some("zh-CN"));
    std::fs::remove_dir_all(dir).unwrap();
}

/// SEG-486-01 (a): revogado depois da captura e antes da escrita, nada é gravado; sem autoridade
/// original do transporte, também não.
#[tokio::test]
async fn revogar_entre_a_captura_e_a_escrita_nao_grava() {
    let (hub, dir) = hub();
    let fone = aparelho(&hub, "fone", &[]);
    let fired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let revoker = hub.clone();
        let marca = fired.clone();
        *hub.store.before_owner_write.lock().unwrap() = Some(Box::new(move || {
            marca.store(true, std::sync::atomic::Ordering::SeqCst);
            revoker.revoke_device("fone").unwrap();
        }));
    }
    assert!(salvar(&hub, &fone, json!({"language":"fr"})).await.is_err());
    assert!(fired.load(std::sync::atomic::Ordering::SeqCst), "a fronteira da escrita foi atingida");
    assert_eq!(hub.store.kv_read(&format!("{APARELHO}fone")).unwrap(), None, "nada gravado");
    let sem = Caller::Device { key: "outro".into(), scopes: vec![], autoridade: None };
    assert!(super::call(&hub, &sem, &json!({"language":"fr"})).is_err());
    assert_eq!(hub.store.kv_read(&format!("{APARELHO}outro")).unwrap(), None);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn bot_rotina_e_pareamento_nao_mudam_o_idioma() {
    let (hub, dir) = hub();
    salvar(&hub, &Caller::Local, json!({"language":"es"})).await.unwrap();
    // A ponte de ferramentas de bot (MCP, inclusive em rotinas) não é admitida neste método.
    assert!(!crate::fork::ponte_credencial::PERMITIDOS.contains(&METHOD));
    let pareando = Caller::Pairing { key: "p".into() };
    assert!(salvar(&hub, &pareando, json!({"language":"en"})).await.is_err());
    assert!(super::call(&hub, &pareando, &json!({"language":"en"})).is_err());
    assert_eq!(do_dono(&hub.store), "es");
    // Turno de rotina (sem mensagem humana): o idioma do dono.
    let rotina =
        hub.add_entry(&Lane::main("donna"), EntryKind::Notice, 1, &json!({"text":"Rotina","runId":"r1"})).unwrap();
    assert_eq!(do_turno(&hub.store, &[rotina.id], false), "es");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn carimbo_vem_do_idioma_guardado_e_ignora_o_corpo() {
    let (hub, dir) = hub();
    let _cmds = hub.test_bot_commands("donna");
    salvar(&hub, &Caller::Local, json!({"language":"es"})).await.unwrap();
    let fone = aparelho(&hub, "fone", &[Scope::Control]);
    salvar(&hub, &fone, json!({"language":"zh-CN"})).await.unwrap();
    let forjado = enviar(&hub, &fone, json!({"text":"oi","appLanguage":"ar-AE"})).await;
    assert_eq!(hub.store.entry(&forjado).unwrap().data["appLanguage"], "zh-CN");
    let local = enviar(&hub, &Caller::Local, json!({"text":"oi","appLanguage":"fr"})).await;
    assert_eq!(hub.store.entry(&local).unwrap().data["appLanguage"], "es");
    // Sem idioma guardado, o corpo não cria carimbo.
    let novo = aparelho(&hub, "novo", &[Scope::Control]);
    let sem = enviar(&hub, &novo, json!({"text":"oi","appLanguage":"ar-AE"})).await;
    assert!(hub.store.entry(&sem).unwrap().data["appLanguage"].is_null());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn varias_pessoas_vale_a_ultima_mensagem_humana() {
    let (hub, dir) = hub();
    let _cmds = hub.test_bot_commands("donna");
    salvar(&hub, &Caller::Local, json!({"language":"es"})).await.unwrap();
    let fone = aparelho(&hub, "fone", &[Scope::Control]);
    salvar(&hub, &fone, json!({"language":"zh-CN"})).await.unwrap();
    let do_dono_msg = enviar(&hub, &Caller::Local, json!({"text":"hola"})).await;
    let do_fone = enviar(&hub, &fone, json!({"text":"你好"})).await;
    assert_eq!(do_turno(&hub.store, &[do_dono_msg.clone(), do_fone.clone()], false), "zh-CN");
    assert_eq!(do_turno(&hub.store, &[do_fone.clone(), do_dono_msg.clone()], false), "es");
    // Última humana de idioma desconhecido: o do dono.
    let novo = aparelho(&hub, "novo", &[Scope::Control]);
    let desconhecido = enviar(&hub, &novo, json!({"text":"oi"})).await;
    assert_eq!(do_turno(&hub.store, &[do_fone.clone(), desconhecido], false), "es");
    // Mensagem que não é humana (de outro bot) depois não conta.
    let de_bot =
        hub.add_entry(&Lane::main("donna"), EntryKind::User, 1, &json!({"text":"x","sourceBotId":"dev"})).unwrap();
    assert_eq!(do_turno(&hub.store, &[do_fone, de_bot.id], false), "zh-CN");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn canal_e_whatsapp_seguem_o_idioma_do_dono() {
    let (hub, dir) = hub();
    let _cmds = hub.test_bot_commands("donna");
    salvar(&hub, &Caller::Local, json!({"language":"fr"})).await.unwrap();
    let fone = aparelho(&hub, "fone", &[Scope::Control]);
    salvar(&hub, &fone, json!({"language":"en"})).await.unwrap();
    let do_fone = enviar(&hub, &fone, json!({"text":"hi"})).await;
    // Turno de canal: o do dono, mesmo com mensagem humana carimbada.
    assert_eq!(do_turno(&hub.store, std::slice::from_ref(&do_fone), true), "fr");
    assert!(turno(&hub.store, &donna(&hub), &[do_fone], true).contains("(fr)"));
    // Mensagem que chega de fora (WhatsApp, `from`): o do dono.
    let whatsapp =
        hub.add_entry(&Lane::main("donna"), EntryKind::User, 1, &json!({"text":"olá","from":"whatsapp:5511"})).unwrap();
    assert_eq!(do_turno(&hub.store, &[whatsapp.id], false), "fr");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn migracao_d368_trata_a_base_anterior_como_base() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let mut cfg = bot("donna", "Donna");
    cfg.description = crate::fork::donna_fixa::BASE_D366.into();
    store.save_bot(&cfg).unwrap();
    store.set_team_settings(&json!({"primaryBotId":"donna"})).unwrap();
    assert_eq!(store.bot("donna").unwrap().unwrap().config.owner_preferences, "");
}

/// A resposta ao cartão de escolha conta como mensagem humana para o idioma (carimbo server-owned).
#[tokio::test]
async fn resposta_de_cartao_conta_como_mensagem_humana() {
    let (hub, dir) = hub();
    salvar(&hub, &Caller::Local, json!({"language":"pt-BR"})).await.unwrap();
    let resposta = hub
        .add_entry(
            &Lane::main("donna"),
            EntryKind::User,
            1,
            &json!({"text":"Resposta à pergunta q: A","choice_response":{"version":1,"questionId":"q","revision":2},"appLanguage":"fr"}),
        )
        .unwrap();
    assert_eq!(do_turno(&hub.store, &[resposta.id], false), "fr");
    std::fs::remove_dir_all(dir).unwrap();
}
