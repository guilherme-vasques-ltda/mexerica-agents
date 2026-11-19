//! c1: provas do produtor pelo `updateBot`/`createBot` reais, catálogo e autoridade atuais.
#![allow(clippy::unwrap_used)]
use super::{CONFLICT, UNSUPPORTED, migrate};
use crate::{
    LockExt,
    api::devices::Caller,
    hub::Hub,
    remote::identity::Identity,
    store::{Device, DeviceSource, Scope, Store, now_ms},
};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

struct Ambiente {
    hub: Arc<Hub>,
    pasta: PathBuf,
}
impl Ambiente {
    fn novo() -> Self {
        let pasta = std::env::temp_dir().join(format!("donna-c1-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&pasta).unwrap();
        let store = Store::open(&pasta.join("estado.db")).unwrap();
        let cfg = serde_json::from_value(json!({"id":"alvo","name":"Alvo","backend":"codex","cwd":pasta})).unwrap();
        store.save_bot(&cfg).unwrap();
        let hub = Hub::new(store, "hub-c1".into(), Identity::load_or_create(&pasta).unwrap(), "token".into(), 0);
        // Atores ativos: o caminho antigo reconfigura o ator depois de salvar.
        hub.start().unwrap();
        Self { hub, pasta }
    }
    /// Instalação manual real do catálogo; devolve id e identidade da encarnação.
    fn instalar(&self, nome: &str) -> (String, String) {
        let r = crate::fork::skill_catalog::custom(
            &self.hub.store,
            &json!({"name":nome,"description":"Prova c1","instructions":"Texto passivo."}),
            || Ok(()),
        )
        .unwrap();
        (r["skill"]["id"].as_str().unwrap().into(), r["skill"]["installationId"].as_str().unwrap().into())
    }
    fn versao(&self) -> String {
        self.hub.store.bot("alvo").unwrap().unwrap().skills_version.unwrap()
    }
    fn skills(&self) -> Vec<String> {
        self.hub.store.bot("alvo").unwrap().unwrap().config.skills
    }
    async fn update(&self, caller: &Caller, body: Value) -> anyhow::Result<Value> {
        crate::api::dispatch(&self.hub, caller, "updateBot", body).await
    }
}
impl Drop for Ambiente {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.pasta);
    }
}

fn celula(versao: &str, skill: &str, ligar: bool, instalacao: &str) -> Value {
    json!({"id":"alvo","skillSelection":{"protocol":1,"expectedVersion":versao,"skillId":skill,"enabled":ligar,"expectedInstallationId":instalacao}})
}
fn codigo(r: anyhow::Result<Value>) -> String {
    format!("{:#}", r.unwrap_err())
}

/// Antes do c1, dois formulários com a mesma leitura perdiam a primeira escolha
/// (reprodução focal registrada no PR). Agora: a segunda recusa, a primeira fica.
#[tokio::test]
async fn dois_formularios_nao_perdem_escolha_e_caminho_antigo_recusa() {
    let a = Ambiente::novo();
    let (skill_a, inst_a) = a.instalar("Skill A");
    let (skill_b, inst_b) = a.instalar("Skill B");
    let lida = a.versao();
    let resposta = a.update(&Caller::Local, celula(&lida, &skill_a, true, &inst_a)).await.unwrap();
    assert_eq!(resposta["bot"]["skills"], json!([skill_a]));
    let nova = resposta["bot"]["skillSelection"]["version"].as_str().unwrap().to_owned();
    assert_ne!(nova, lida);
    assert_eq!(nova, a.versao());
    // Segundo formulário, mesma leitura, outra célula: recusa conservadora.
    assert!(codigo(a.update(&Caller::Local, celula(&lida, &skill_b, true, &inst_b)).await).contains(CONFLICT));
    // Resposta perdida: reenviar o mesmo pedido não reaplica; a leitura reconcilia.
    assert!(codigo(a.update(&Caller::Local, celula(&lida, &skill_a, true, &inst_a)).await).contains(CONFLICT));
    assert_eq!(a.skills(), std::slice::from_ref(&skill_a));
    // Perfil completo antigo: mudar a lista recusa; lista igual e campos incidentais passam.
    let antigo = a.update(&Caller::Local, json!({"id":"alvo","skills":[skill_b]})).await;
    assert!(codigo(antigo).contains(UNSUPPORTED));
    a.update(&Caller::Local, json!({"id":"alvo","name":"Renomeado","skills":[skill_a]})).await.unwrap();
    a.update(&Caller::Local, json!({"id":"alvo","name":"Outro"})).await.unwrap();
    let misturado = celula(&a.versao(), &skill_b, true, &inst_b);
    let mut misturado = misturado.as_object().unwrap().clone();
    misturado.insert("name".into(), "Junto".into());
    assert!(codigo(a.update(&Caller::Local, Value::Object(misturado)).await).contains(UNSUPPORTED));
    // Escritor de perfil completo com snapshot velho não restaura a seleção.
    let mut velho = a.hub.store.bot("alvo").unwrap().unwrap().config;
    velho.skills.clear();
    a.hub.store.save_bot(&velho).unwrap();
    a.hub.reorder_bots(&["alvo".into()]).unwrap();
    assert_eq!(a.skills(), [skill_a]);
}

#[tokio::test]
async fn aba_remocao_e_reinstalacao_recusam_condicao_antiga() {
    let a = Ambiente::novo();
    let (skill, inst) = a.instalar("Skill A");
    let v0 = a.versao();
    a.update(&Caller::Local, celula(&v0, &skill, true, &inst)).await.unwrap();
    let v1 = a.versao();
    a.update(&Caller::Local, celula(&v1, &skill, false, &inst)).await.unwrap();
    // A lista voltou a ser a de v0, mas a condição não volta (ABA).
    assert_eq!(a.skills(), Vec::<String>::new());
    assert!(codigo(a.update(&Caller::Local, celula(&v0, &skill, true, &inst)).await).contains(CONFLICT));
    a.update(&Caller::Local, celula(&a.versao(), &skill, true, &inst)).await.unwrap();
    let antes = a.versao();
    // Remoção revoga o vínculo na transação do catálogo e invalida a condição.
    let lista = crate::fork::skill_catalog::list(&a.hub.store, || Ok(())).unwrap();
    let item = lista["items"].as_array().unwrap().iter().find(|s| s["id"] == skill.as_str()).unwrap();
    let remocao = json!({"id":skill,"expectedInstallationId":item["installationId"],"expectedReceipt":{"kind":"none"}});
    crate::fork::skill_catalog::remove(&a.hub.store, &remocao, || Ok(())).unwrap();
    assert_eq!(a.skills(), Vec::<String>::new());
    assert_ne!(a.versao(), antes);
    // Reinstalação com o mesmo id: a identidade vista no gesto antigo não serve.
    let (mesmo, nova) = a.instalar("Skill A");
    assert_eq!(mesmo, skill);
    assert_ne!(nova, inst);
    let velha = a.update(&Caller::Local, celula(&a.versao(), &skill, true, &inst)).await;
    assert!(codigo(velha).contains("skill_installation_stale"));
    assert_eq!(a.skills(), Vec::<String>::new());
    a.update(&Caller::Local, celula(&a.versao(), &skill, true, &nova)).await.unwrap();
    assert_eq!(a.skills(), [skill]);
}

#[tokio::test]
async fn autoridade_revogada_na_fila_e_criacao_sem_instalacao_nao_gravam() {
    let a = Ambiente::novo();
    let (skill, inst) = a.instalar("Skill A");
    let aparelho = Device {
        key: "aparelho-c1".into(),
        name: "Teste".into(),
        platform: "teste".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: now_ms(),
        last_seen_at: None,
    };
    a.hub.store.put_device(&aparelho).unwrap();
    let (_, caller) = crate::fork::autoridade::autenticar(&a.hub, &aparelho.key).unwrap();
    // Captura na entrada; revogar e reconceder enquanto o pedido espera não o reabilita.
    let pedido =
        super::Selection::capture(&a.hub, &caller, &celula(&a.versao(), &skill, true, &inst)).unwrap().unwrap();
    // A criação com skills também conserva a mesma autoridade original até o commit.
    let mut creation = json!({"id":"criacao-revogada-c1","name":"Novo","backend":"codex","cwd":a.pasta,
        "browserPreference":"computer","skills":[skill],
        "skillSelection":{"protocol":1,"installations":{(skill.clone()):inst.clone()}}});
    let pending_creation = super::CreationSelection::capture(&a.hub, &caller, &mut creation).unwrap().unwrap();
    let creation_cfg: crate::store::BotConfig = serde_json::from_value(creation).unwrap();
    a.hub.store.remove_device(&aparelho.key).unwrap();
    a.hub.store.put_device(&aparelho).unwrap();
    assert!(pedido.commit(&a.hub).is_err());
    assert!(pending_creation.save(&a.hub, &creation_cfg, None).is_err());
    assert!(a.hub.store.bot("criacao-revogada-c1").unwrap().is_none());
    assert_eq!(a.skills(), Vec::<String>::new());
    // Mesmo aparelho, nova autenticação: grava como Control sem capacidade de contas.
    let (_, atual) = crate::fork::autoridade::autenticar(&a.hub, &aparelho.key).unwrap();
    a.update(&atual, celula(&a.versao(), &skill, true, &inst)).await.unwrap();
    assert_eq!(a.skills(), std::slice::from_ref(&skill));
    // Criação: positiva só com a instalação vista no formulário.
    assert!(codigo(criar(&a, &skill, None).await).contains(UNSUPPORTED));
    assert!(
        codigo(criar(&a, &skill, Some("00000000-0000-4000-8000-000000000000:1")).await)
            .contains("skill_installation_stale")
    );
    assert_eq!(a.hub.store.bots().unwrap().len(), 1);
    let criado = criar(&a, &skill, Some(inst.as_str())).await.unwrap();
    assert_eq!(criado["bot"]["skills"], json!([skill]));
    assert!(criado["bot"]["skillSelection"]["version"].is_string());
    let id = criado["bot"]["id"].as_str().unwrap();
    let cfg = a.hub.store.bot(id).unwrap().unwrap().config;
    assert_eq!(cfg.browser_preference, crate::fork::browser::BrowserPreference::Computer);
    assert_eq!(criado["bot"]["browserPreference"], "computer");
    assert!(!cfg.computer);
    assert!(
        crate::fork::tela_por_computador::allowed_on(
            &a.hub.store.db.locked(),
            id,
            &crate::fork::tela_por_computador::host_target(&a.hub),
            None
        )
        .is_err(),
        "criação humana com skills não concede tela D536"
    );
}

async fn criar(a: &Ambiente, skill: &str, instalacao: Option<&str>) -> anyhow::Result<Value> {
    let mut body =
        json!({"name":"Novo","backend":"codex","cwd":a.pasta,"skills":[skill],"browserPreference":"computer"});
    if let Some(instalacao) = instalacao {
        let mut vistas = serde_json::Map::new();
        vistas.insert(skill.to_owned(), instalacao.into());
        body["skillSelection"] = json!({"protocol":1,"installations":vistas});
    }
    crate::api::dispatch(&a.hub, &Caller::Local, "createBot", body).await
}

#[tokio::test]
async fn migracao_idempotente_lacuna_de_trigger_e_metadado_invalido() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE TABLE bots(id TEXT PRIMARY KEY, config TEXT NOT NULL, rev INTEGER NOT NULL, deleted INTEGER NOT NULL DEFAULT 0);
         INSERT INTO bots VALUES('bot','{\"skills\":[\"a\"]}',7,0);",
    )
    .unwrap();
    migrate(&db).unwrap();
    let ler = || db.query_row("SELECT donna_skills_version FROM bots", [], |r| r.get::<_, String>(0)).unwrap();
    let primeira = ler();
    migrate(&db).unwrap();
    assert_eq!(ler(), primeira, "migração repetida não invalida");
    // ABA feita sem o trigger (Host antigo/ferramenta): a migração reemite todas as condições.
    db.execute_batch(
        "DROP TRIGGER donna_skills_version_update;
         UPDATE bots SET config='{\"skills\":[]}'; UPDATE bots SET config='{\"skills\":[\"a\"]}';",
    )
    .unwrap();
    assert_eq!(ler(), primeira);
    migrate(&db).unwrap();
    assert_ne!(ler(), primeira);
    // Trigger divergente não é substituído: recusa.
    db.execute_batch(
        "DROP TRIGGER donna_skills_version_insert;
         CREATE TRIGGER donna_skills_version_insert AFTER INSERT ON bots BEGIN SELECT 1; END;",
    )
    .unwrap();
    assert!(migrate(&db).is_err());
    // Metadado inválido vira ausência de condição, sem sumir com o bot.
    let a = Ambiente::novo();
    a.hub.store.db.locked().execute("UPDATE bots SET donna_skills_version=x'00' WHERE id='alvo'", []).unwrap();
    let row = a.hub.store.bot("alvo").unwrap().unwrap();
    assert!(row.skills_version.is_none());
    assert!(a.hub.bot_json(&row)["skillSelection"].is_null());
    assert_eq!(a.hub.store.bots().unwrap().len(), 1);
}
