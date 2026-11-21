//! D543 fatia 3: principal fixo e exclusão de Space, fixtures sintéticas.
use super::{Caller, Hub, Identity, Store, dispatch};
use crate::{
    LockExt,
    store::{BotConfig, Device, DeviceSource},
};
use serde_json::json;

const ACCOUNT: &str = "ac_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Lifecycle {
    hub: std::sync::Arc<Hub>,
    dir: std::path::PathBuf,
}

impl Lifecycle {
    fn new() -> Self {
        let dir = std::fs::canonicalize(std::fs::canonicalize(std::env::temp_dir()).unwrap())
            .unwrap()
            .join(format!("d543-lifecycle-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Hub::new(
            Store::open(&dir.join("fixture.db")).unwrap(),
            "fixture".into(),
            Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        // Principal do Pessoal, como na fixture do Birth: prova que a exclusão do
        // Space não toca nele.
        let personal: BotConfig = serde_json::from_value(json!({
            "id":"personal-principal", "name":"Berga", "backend":"codex", "cwd":dir,
            "description":"PROMPT_PESSOAL_SINTETICO"
        }))
        .unwrap();
        hub.store.save_bot(&personal).unwrap();
        hub.store
            .kv_set("team.settings.v1", &json!({"primaryBotId":personal.id, "permission":"auto"}).to_string())
            .unwrap();
        hub.store
            .kv_set(
                crate::fork::engine_accounts::SLOT,
                &json!({
                    "salt":"00000000-0000-4000-8000-000000000543", "accounts":[{
                        "id":ACCOUNT, "backend":"codex", "billing":"paid_api", "state":"connected",
                        "launch":{"kind":"cli_profile", "executable":dir.join("cli-inerte"),
                            "directory":dir.join("perfil-inerte")},
                        "scope":"owner", "quotaIdentity":null, "verifiedAt":null,
                        "subscriptionProof":null, "models":[]
                    }]
                })
                .to_string(),
            )
            .unwrap();
        Self { hub, dir }
    }

    async fn spaces(&self) -> serde_json::Value {
        dispatch(&self.hub, &Caller::Local, "donnaSpaces", json!({})).await.unwrap()
    }

    async fn create_space(&self, name: &str) -> serde_json::Value {
        let snap = self.spaces().await;
        dispatch(
            &self.hub,
            &Caller::Local,
            "donnaSpaceCreate",
            json!({"hubId":self.hub.host_id,"expectedRevision":snap["revision"],"name":name,
                "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}}),
        )
        .await
        .unwrap()
    }

    fn space_id(snap: &serde_json::Value, index: usize) -> String {
        snap["spaces"][index]["id"].as_str().unwrap().to_owned()
    }

    fn principal_request(&self, space: &str, snap: &serde_json::Value, nonce: &str) -> serde_json::Value {
        json!({"hubId":self.hub.host_id,"spaceId":space,"expectedRevision":snap["revision"],
            "clientNonce":nonce,
            "principal":{"name":"Berga","backend":"codex","engineAccountId":ACCOUNT,
                "appearance":{"v":2,"estilo":"simples","aparencia":{"cor":"roxo","olhos":"brilhantes",
                    "roupa":"polo","acessorio":"fone","objeto":"nenhum"}}}})
    }

    async fn activate(&self, space: &str, snap: &serde_json::Value, nonce: &str) -> anyhow::Result<serde_json::Value> {
        dispatch(&self.hub, &Caller::Local, "donnaSpacePrincipal", self.principal_request(space, snap, nonce)).await
    }

    fn bot_of_space(&self, space: &str) -> Option<BotConfig> {
        self.hub.store.bots().unwrap().into_iter().find(|r| !r.deleted && r.config.space_id == space).map(|r| r.config)
    }
}

impl Drop for Lifecycle {
    fn drop(&mut self) {
        // Mesma limpeza do Birth: pastas gerenciadas saem antes do diretório da fixture.
        for row in self.hub.store.bots().unwrap_or_default() {
            if row.config.id != "personal-principal" && crate::agent::workspace::is_managed(&row.config) {
                let path = std::path::PathBuf::from(&row.config.cwd);
                let _ = std::fs::remove_dir(&path);
                if let Some(parent) = path.parent() {
                    let _ = std::fs::remove_dir(parent);
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn d543_principal_ativa_berga_fixo_com_aparencia_e_idempotencia() {
    let f = Lifecycle::new();
    let created = f.create_space("Cliente").await;
    let space = Lifecycle::space_id(&created, 1);
    let snap = f.spaces().await;
    let out = f.activate(&space, &snap, "nonce-1").await.expect("ativa o principal");
    let principal_id = out["spaces"][1]["principalBotId"].as_str().unwrap().to_owned();
    let cfg = f.hub.store.bot(&principal_id).unwrap().unwrap().config;
    assert_eq!(cfg.name, "Berga");
    assert_eq!(cfg.space_id, space);
    assert!(cfg.description.contains("neste Space"), "persona do Space: {cfg:?}");
    assert!(!cfg.description.contains("coordenar os outros bots"), "sem promessa de gestão");
    assert_eq!(crate::fork::donna_fixa::description(&f.hub.store, &cfg), cfg.description);
    assert_eq!(cfg.shared_memory, Some(false));
    assert!(!cfg.harness_memory && !cfg.computer && !cfg.manage_team);
    let mex = serde_json::to_value(cfg.mexeriquinha.unwrap()).unwrap();
    assert_eq!(mex["aparencia"]["cor"], "roxo");
    assert_eq!(mex["aparencia"]["acessorio"], "fone");
    // Conversa canônica do Space existe e aponta a conta escolhida.
    let conv = f.hub.store.db.locked().query_row(
        "SELECT engine_account_id FROM donna_space_conversations WHERE id=?1",
        [&principal_id],
        |r| r.get::<_, Option<String>>(0),
    );
    assert!(matches!(conv, Ok(Some(ref a)) if a == ACCOUNT));
    // Replay do MESMO corpo (o digest inclui a revisão) devolve o snapshot atual.
    let body = f.principal_request(&space, &snap, "nonce-1");
    let replay = dispatch(&f.hub, &Caller::Local, "donnaSpacePrincipal", body.clone()).await.unwrap();
    assert_eq!(replay["spaces"][1]["principalBotId"], principal_id.as_str());
    assert!(f.bot_of_space(&space).is_some(), "replay não duplica o principal");
    // Mesmo nonce com conteúdo diferente é conflito, nunca replay.
    let mut divergent = body;
    divergent["principal"]["appearance"]["aparencia"]["cor"] = json!("lima");
    let conflict = dispatch(&f.hub, &Caller::Local, "donnaSpacePrincipal", divergent).await.unwrap_err();
    assert!(conflict.to_string().contains("space_principal_nonce_conflict"), "{conflict}");
    // Outro nonce no Space que já tem principal é recusado.
    let snap3 = f.spaces().await;
    let again = f.activate(&space, &snap3, "nonce-2").await.unwrap_err();
    assert!(again.to_string().contains("space_principal_exists"), "{again}");
}

#[tokio::test]
async fn d543_principal_recusa_nome_e_estilo_fora_do_contrato() {
    let f = Lifecycle::new();
    let created = f.create_space("Cliente").await;
    let space = Lifecycle::space_id(&created, 1);
    let snap = f.spaces().await;
    let mut body = f.principal_request(&space, &snap, "n-nome");
    body["principal"]["name"] = json!("Outro Nome");
    let err = dispatch(&f.hub, &Caller::Local, "donnaSpacePrincipal", body).await.unwrap_err();
    assert!(err.to_string().contains("space_principal_name_fixed"), "{err}");
    let mut body = f.principal_request(&space, &snap, "n-3d");
    body["principal"]["appearance"] = json!({"v":2,"estilo":"3d","conjuntoId":"berga"});
    let err = dispatch(&f.hub, &Caller::Local, "donnaSpacePrincipal", body).await.unwrap_err();
    assert!(err.to_string().contains("space_principal_appearance"), "{err}");
    // Sem aparência: nasce o provisório (Simples, laranja, ponto).
    let mut body = f.principal_request(&space, &snap, "n-default");
    body["principal"].as_object_mut().unwrap().remove("appearance");
    let out = dispatch(&f.hub, &Caller::Local, "donnaSpacePrincipal", body).await.unwrap();
    let id = out["spaces"][1]["principalBotId"].as_str().unwrap().to_owned();
    let cfg = f.hub.store.bot(&id).unwrap().unwrap().config;
    let mex = serde_json::to_value(cfg.mexeriquinha.unwrap()).unwrap();
    assert_eq!(mex["aparencia"]["cor"], "laranja");
    assert_eq!(mex["aparencia"]["olhos"], "ponto");
}

#[tokio::test]
async fn d543_principal_nao_pode_ser_renomeado_por_update_bot() {
    let f = Lifecycle::new();
    let created = f.create_space("Cliente").await;
    let space = Lifecycle::space_id(&created, 1);
    let snap = f.spaces().await;
    f.activate(&space, &snap, "nonce-r").await.unwrap();
    let cfg = f.bot_of_space(&space).unwrap();
    let mut renamed = serde_json::to_value(&cfg).unwrap();
    renamed["name"] = json!("Zé");
    let err =
        crate::store::bots::save_bot(&f.hub.store.db.locked(), &serde_json::from_value::<BotConfig>(renamed).unwrap())
            .unwrap_err();
    assert!(err.to_string().contains("space_principal_name_fixed"), "{err}");
}

#[tokio::test]
async fn d543_delete_exige_nome_digitado_e_remove_tudo_do_space() {
    let f = Lifecycle::new();
    let created = f.create_space("Cliente").await;
    let space = Lifecycle::space_id(&created, 1);
    let snap = f.spaces().await;
    let out = f.activate(&space, &snap, "nonce-d").await.unwrap();
    let principal = out["spaces"][1]["principalBotId"].as_str().unwrap().to_owned();
    // A concessão privada do bot acompanha a exclusão transacional do Space.
    let grant_key = "donna.routine-external-grant.v1.synthetic-space-delete";
    f.hub.store.kv_set(grant_key, &json!({"version":1,"grantId":"synthetic","origin":"localTransport","spaceId":space,"botId":principal,"routineId":"synthetic","entryId":"synthetic","reviewedEntryRevision":1,"authorityRevision":"synthetic","contract":{"repository":"org/repo","event":"pull_request.opened","action":{"type":"notify","text":"test"}}}).to_string()).unwrap();
    for bot in [None, Some(principal.as_str())] {
        dispatch(
            &f.hub,
            &Caller::Local,
            "nuncaFazerAdicionar",
            json!({
                "spaceId":space, "botId":bot, "texto":"Não enviar", "expectedRevision":0
            }),
        )
        .await
        .unwrap();
    }
    dispatch(
        &f.hub,
        &Caller::Local,
        "instrucaoGeralSalvar",
        json!({"escopo":"space", "spaceId":space, "texto":"Respostas curtas.", "expectedRevision":0}),
    )
    .await
    .unwrap();
    let snap = f.spaces().await;
    let delete = |confirm: &str, rev: i64| {
        let hub = f.hub.clone();
        let space = space.clone();
        let confirm = confirm.to_owned();
        async move {
            dispatch(
                &hub,
                &Caller::Local,
                "donnaSpaceDelete",
                json!({"hubId":hub.host_id,"spaceId":space,"expectedRevision":rev,"confirmName":confirm}),
            )
            .await
        }
    };
    let err = delete("Errado", snap["revision"].as_i64().unwrap()).await.unwrap_err();
    assert!(err.to_string().contains("space_delete_confirmation_mismatch"), "{err}");
    let out = delete("Cliente", snap["revision"].as_i64().unwrap()).await.expect("exclui o Space");
    assert!(f.hub.store.kv_read(grant_key).unwrap().is_none());
    assert_eq!(out["spaces"].as_array().unwrap().len(), 1, "só o Pessoal resta");
    assert!(f.hub.store.bot(&principal).unwrap().unwrap().deleted);
    assert!(f.bot_of_space(&space).is_none());
    assert!(!f.hub.store.bot("personal-principal").unwrap().unwrap().deleted, "Pessoal intacto");
    let convs: i64 = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM donna_space_conversations WHERE space_id=?1", [&space], |r| r.get(0))
        .unwrap();
    assert_eq!(convs, 0);
    let receipts: i64 = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM donna_space_principal_receipts WHERE space_id=?1", [&space], |r| r.get(0))
        .unwrap();
    assert_eq!(receipts, 0, "recibos do Space também saem");
    for table in ["donna_nunca_fazer", "donna_nunca_fazer_revisoes", "donna_instrucao_geral"] {
        let count: i64 = f
            .hub
            .store
            .db
            .locked()
            .query_row(&format!("SELECT COUNT(*) FROM {table} WHERE space_id=?1"), [&space], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "{table}");
    }
}

#[tokio::test]
async fn d543_delete_recusa_pessoal_e_espaco_inexistente() {
    let f = Lifecycle::new();
    let snap = f.spaces().await;
    let personal = snap["spaces"][0]["id"].as_str().unwrap().to_owned();
    let err = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceDelete",
        json!({"hubId":f.hub.host_id,"spaceId":personal,"expectedRevision":snap["revision"],"confirmName":"Pessoal"}),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("personal_space_immutable"), "{err}");
    let err = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceDelete",
        json!({"hubId":f.hub.host_id,"spaceId":"00000000-0000-4000-8000-0000000000ff",
            "expectedRevision":snap["revision"],"confirmName":"X"}),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("space_not_found") || err.to_string().contains("space"), "{err}");
}

#[tokio::test]
async fn d543_delete_recusa_com_nomes_quando_bot_esta_ocupado() {
    let f = Lifecycle::new();
    let created = f.create_space("Cliente").await;
    let space = Lifecycle::space_id(&created, 1);
    let snap = f.spaces().await;
    let out = f.activate(&space, &snap, "nonce-b").await.unwrap();
    let principal = out["spaces"][1]["principalBotId"].as_str().unwrap().to_owned();
    // Admissão em andamento ilegível = trabalho vivo; a recusa traz só o nome.
    f.hub.store.kv_set(&format!("turn.inflight.{principal}"), "registro-incompleto").unwrap();
    let snap = f.spaces().await;
    let out = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceDelete",
        json!({"hubId":f.hub.host_id,"spaceId":space,"expectedRevision":snap["revision"],"confirmName":"Cliente"}),
    )
    .await
    .unwrap();
    assert_eq!(out["busy"].as_array().unwrap().len(), 1);
    assert_eq!(out["busy"][0]["name"], "Berga");
    assert!(out["busy"][0].get("activity").is_none(), "só nomes, sem atividades");
    // Nada foi apagado: Space, bot e conversa continuam.
    let snap = f.spaces().await;
    assert_eq!(snap["spaces"].as_array().unwrap().len(), 2);
    assert!(!f.hub.store.bot(&principal).unwrap().unwrap().deleted);
}

#[tokio::test]
async fn d543_delete_so_no_computador_pela_plataforma_do_aparelho() {
    let f = Lifecycle::new();
    // Aparelho iOS registrado: a mesma Tx recusa antes de qualquer escrita.
    f.hub
        .store
        .put_device(&Device {
            key: "iphone-key".into(),
            name: "iPhone".into(),
            platform: "ios".into(),
            source: DeviceSource::Account,
            grant_id: None,
            scopes: vec![],
            lease_until: None,
            created_at: 0,
            last_seen_at: None,
        })
        .unwrap();
    let created = f.create_space("Cliente").await;
    let space = created["spaces"][1]["id"].as_str().unwrap().to_owned();
    let rev = created["revision"].as_i64().unwrap();
    let mut c = f.hub.store.db.locked();
    let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).unwrap();
    let err = Store::delete_space_on(
        &tx,
        &space,
        rev,
        "Cliente",
        Some("iphone-key"),
        &f.hub.host_id,
        &[],
        &[],
        "{}",
        &[],
        &[],
    )
    .unwrap_err();
    assert!(err.to_string().contains("space_delete_desktop_only"), "{err}");
    drop(tx);
    drop(c);
}

// ---- Matriz 4.6: recusas e isolamento do principal e da exclusão ----

fn save_cfg(hub: &Hub, cfg: &BotConfig) -> anyhow::Result<()> {
    crate::fork::donna_fixa::como_dono(&hub.store.db.locked(), |db| crate::store::bots::save_bot(db, cfg)).map(|_| ())
}

#[tokio::test]
async fn d543_principal_conta_desconectada_e_chamador_sem_dono_recusam() {
    let f = Lifecycle::new();
    // Conta vira desconectada: a ativação recusa dentro da mesma Tx.
    let mut slot: serde_json::Value =
        serde_json::from_str(&f.hub.store.kv_get(crate::fork::engine_accounts::SLOT).unwrap()).unwrap();
    slot["accounts"][0]["state"] = json!("disconnected");
    f.hub.store.kv_set(crate::fork::engine_accounts::SLOT, &slot.to_string()).unwrap();
    let created = f.create_space("Cliente").await;
    let space = Lifecycle::space_id(&created, 1);
    let snap = f.spaces().await;
    let err = f.activate(&space, &snap, "n-off").await.unwrap_err();
    assert!(err.to_string().contains("account_unavailable"), "{err}");
    // Chamadores sem autoridade de dono recusam os dois métodos.
    slot["accounts"][0]["state"] = json!("connected");
    f.hub.store.kv_set(crate::fork::engine_accounts::SLOT, &slot.to_string()).unwrap();
    for caller in [
        Caller::Device { key: "sem-vinculo".into(), scopes: vec![crate::store::Scope::Control], autoridade: None },
        Caller::Device { key: "executor".into(), scopes: vec![crate::store::Scope::Executor], autoridade: None },
        Caller::Pairing { key: "pair".into() },
    ] {
        let body = f.principal_request(&space, &snap, "n-caller");
        assert!(dispatch(&f.hub, &caller, "donnaSpacePrincipal", body).await.is_err(), "principal recusado");
        let del =
            json!({"hubId":f.hub.host_id,"spaceId":space,"expectedRevision":snap["revision"],"confirmName":"Cliente"});
        assert!(dispatch(&f.hub, &caller, "donnaSpaceDelete", del).await.is_err(), "delete recusado");
    }
}

#[tokio::test]
async fn d543_birth_nome_fixo_berga() {
    let f = Lifecycle::new();
    let birth = |principal: serde_json::Value, nonce: &str| {
        let hub = f.hub.clone();
        let nonce = nonce.to_owned();
        async move {
            let snap = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({})).await.unwrap();
            dispatch(
                &hub,
                &Caller::Local,
                "donnaSpaceBirth",
                json!({"hubId":hub.host_id,"expectedRevision":snap["revision"],"clientNonce":nonce,
                    "name":"Espaço","tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"ES"}},
                    "principal":principal}),
            )
            .await
        }
    };
    // Sem nome e com "Berga" aceitam; outro nome e string vazia recusam.
    birth(json!({"backend":"codex","engineAccountId":ACCOUNT}), "b-sem").await.unwrap();
    birth(json!({"name":"Berga","backend":"codex","engineAccountId":ACCOUNT}), "b-berga").await.unwrap();
    let err = birth(json!({"name":"Zé","backend":"codex","engineAccountId":ACCOUNT}), "b-zeze").await.unwrap_err();
    assert!(err.to_string().contains("space_principal_name_fixed"), "{err}");
    let err = birth(json!({"name":"","backend":"codex","engineAccountId":ACCOUNT}), "b-vazio").await.unwrap_err();
    assert!(err.to_string().contains("space_principal_name_fixed"), "{err}");
}

#[tokio::test]
async fn d543_bot_comum_do_space_nao_pode_se_chamar_berga() {
    let f = Lifecycle::new();
    let created = f.create_space("Cliente").await;
    let space = Lifecycle::space_id(&created, 1);
    let cfg: BotConfig = serde_json::from_value(json!({
        "id":uuid::Uuid::new_v4().to_string(), "spaceId":space, "name":"Berga",
        "backend":"codex", "cwd":f.dir
    }))
    .unwrap();
    let err = save_cfg(&f.hub, &cfg).unwrap_err();
    assert!(err.to_string().contains("space_principal_name_fixed") || err.to_string().contains("reserv"), "{err}");
}

#[tokio::test]
async fn d543_aparencia_por_space_e_campos_fixos() {
    let f = Lifecycle::new();
    let a = f.create_space("Alpha").await;
    let space_a = Lifecycle::space_id(&a, 1);
    let snap = f.spaces().await;
    f.activate(&space_a, &snap, "n-a").await.unwrap();
    let b = f.create_space("Beta").await;
    let space_b = Lifecycle::space_id(&b, 2);
    let snap = f.spaces().await;
    let mut body = f.principal_request(&space_b, &snap, "n-b");
    body["principal"]["appearance"]["aparencia"]["cor"] = json!("lima");
    dispatch(&f.hub, &Caller::Local, "donnaSpacePrincipal", body).await.unwrap();
    let cfg_a = f.bot_of_space(&space_a).unwrap();
    let cfg_b = f.bot_of_space(&space_b).unwrap();
    // Auditoria is_donna_here: o Berga do Space recebe a persona BASE_SPACE — sem a
    // promessa de coordenar bots (isso só entra na Entrega 3b) — e nasce sem nenhum
    // poder: sem gestão, computador, memória do harness nem memória compartilhada.
    assert_eq!(crate::fork::donna_fixa::description(&f.hub.store, &cfg_a), crate::fork::donna_fixa::BASE_SPACE);
    assert!(crate::fork::donna_fixa::is_donna_here(&f.hub.store, &cfg_a));
    assert!(!cfg_a.manage_team && !cfg_a.computer && !cfg_a.harness_memory && cfg_a.shared_memory != Some(true));
    assert!(cfg_a.skills.is_empty() && cfg_a.connectors.is_empty() && cfg_a.logins.is_empty());
    let cor_a = serde_json::to_value(cfg_a.mexeriquinha.clone().unwrap()).unwrap()["aparencia"]["cor"].clone();
    let cor_b = serde_json::to_value(cfg_b.mexeriquinha.clone().unwrap()).unwrap()["aparencia"]["cor"].clone();
    assert_eq!(cor_a, "roxo");
    assert_eq!(cor_b, "lima");
    // Mudar cor/figura do A não toca B nem o Pessoal.
    let mut edit = serde_json::to_value(&cfg_a).unwrap();
    edit["mexeriquinha"]["aparencia"]["cor"] = json!("toranja");
    save_cfg(&f.hub, &serde_json::from_value::<BotConfig>(edit).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(f.bot_of_space(&space_b).unwrap().mexeriquinha.unwrap()).unwrap()["aparencia"]["cor"],
        "lima"
    );
    let personal = f.hub.store.bot("personal-principal").unwrap().unwrap().config;
    assert_eq!(personal.description, "PROMPT_PESSOAL_SINTETICO");
    // Descrição, instruções e preferências são fixas.
    for (field, value) in [
        ("description", json!("instrução nova")),
        ("ownerPreferences", json!("pref nova")),
        ("title", json!("Título novo")),
    ] {
        let mut bad = serde_json::to_value(&cfg_a).unwrap();
        bad[field] = value;
        let err = save_cfg(&f.hub, &serde_json::from_value::<BotConfig>(bad).unwrap()).unwrap_err();
        assert!(err.to_string().contains("space_principal_fixed"), "{field}: {err}");
    }
    let mut unauthorized = cfg_a.clone();
    unauthorized.name = "Berga".into();
    unauthorized.backend = "claude".into();
    assert!(
        crate::store::bots::save_bot(&f.hub.store.db.locked(), &unauthorized).is_err(),
        "motor só pelo caminho confirmado do dono"
    );
    // Estilo 3D pela edição também é recusado.
    let mut bad = serde_json::to_value(&cfg_a).unwrap();
    bad["mexeriquinha"] = json!({"v":2,"estilo":"3d","conjuntoId":"base"});
    let err = save_cfg(&f.hub, &serde_json::from_value::<BotConfig>(bad).unwrap()).unwrap_err();
    assert!(err.to_string().contains("space_principal_appearance"), "{err}");
}

#[tokio::test]
async fn d543_principal_legado_renomeia_so_para_berga() {
    let f = Lifecycle::new();
    let created = f.create_space("Cliente").await;
    let space = Lifecycle::space_id(&created, 1);
    // Principal legado com nome próprio, como os Spaces nascidos antes do fixo.
    let legacy: BotConfig = serde_json::from_value(json!({
        "id":uuid::Uuid::new_v4().to_string(), "spaceId":space, "name":"Zé",
        "backend":"codex", "cwd":f.dir, "description":"persona antiga"
    }))
    .unwrap();
    save_cfg(&f.hub, &legacy).unwrap();
    // Vínculo gravado como o Birth antigo faria (None → principal é permitido).
    let mut state = f.hub.store.spaces().unwrap();
    state.spaces.iter_mut().find(|s| s.id == space).unwrap().principal_bot_id = Some(legacy.id.clone());
    f.hub.store.kv_set(crate::fork::spaces::store::KEY, &serde_json::to_string(&state).unwrap()).unwrap();
    let stored = f.hub.store.bot(&legacy.id).unwrap().unwrap().config;
    // Manter "Zé" e mudar aparência passa.
    let mut keep = serde_json::to_value(&stored).unwrap();
    keep["mexeriquinha"] = json!({"v":2,"estilo":"simples","aparencia":{"cor":"celeste","olhos":"ponto","roupa":"nenhuma","acessorio":"nenhum","objeto":"nenhum"}});
    save_cfg(&f.hub, &serde_json::from_value::<BotConfig>(keep).unwrap()).unwrap();
    // Renomear para "Berga" passa; para outro nome recusa.
    let mut to_berga = serde_json::to_value(&stored).unwrap();
    to_berga["name"] = json!("Berga");
    save_cfg(&f.hub, &serde_json::from_value::<BotConfig>(to_berga).unwrap()).unwrap();
    let mut to_other = serde_json::to_value(&f.hub.store.bot(&legacy.id).unwrap().unwrap().config).unwrap();
    to_other["name"] = json!("Outro");
    let err = save_cfg(&f.hub, &serde_json::from_value::<BotConfig>(to_other).unwrap()).unwrap_err();
    assert!(err.to_string().contains("space_principal_name_fixed"), "{err}");
}

#[tokio::test]
async fn d543_memoria_do_space_nao_atravessa_para_o_pessoal() {
    let f = Lifecycle::new();
    let created = f.create_space("Cliente").await;
    let space = Lifecycle::space_id(&created, 1);
    let snap = f.spaces().await;
    f.activate(&space, &snap, "n-mem").await.unwrap();
    let cfg = f.bot_of_space(&space).unwrap();
    // Memória do Space visível só dentro do Space; nada vaza para o Pessoal.
    f.hub
        .store
        .memory_write(|db, now, rev| {
            codync_mem::store::add(
                db,
                codync_mem::store::Writer::User,
                codync_mem::store::New { bot: Some(&cfg.id), text: "MEMORIA_DO_SPACE_D543", room: None, evidence: &[] },
                now,
                rev,
            )
        })
        .unwrap();
    let own = dispatch(&f.hub, &Caller::Local, "memory", json!({"spaceId":space,"botId":cfg.id})).await.unwrap();
    assert!(own.to_string().contains("MEMORIA_DO_SPACE_D543"));
    let personal = dispatch(&f.hub, &Caller::Local, "memory", json!({"botId":"personal-principal"})).await.unwrap();
    assert!(!personal.to_string().contains("MEMORIA_DO_SPACE_D543"));
    let cross =
        dispatch(&f.hub, &Caller::Local, "memory", json!({"spaceId":crate::fork::spaces::PERSONAL,"botId":cfg.id}))
            .await;
    assert!(cross.is_err() || !cross.unwrap().to_string().contains("MEMORIA_DO_SPACE_D543"));
    // Preferências, skills, conectores e logins do Pessoal não são herdados: o
    // principal do Space nasce com a allowlist vazia (o Pessoal fixo nem aceita
    // ganhar esses campos — `donna_fixa` protege o perfil dele).
    let space_cfg = f.hub.store.bot(&cfg.id).unwrap().unwrap().config;
    assert_eq!(space_cfg.owner_preferences, "");
    assert!(space_cfg.skills.is_empty() && space_cfg.connectors.is_empty() && space_cfg.logins.is_empty());
    // E o prompt do Space não carrega a linha de preferências do Pessoal.
    assert!(crate::fork::donna_fixa::preferences_line(&f.hub.store, &space_cfg).is_none());
}

#[tokio::test]
async fn d543_delete_revisao_antiga_e_estado_pos_exclusao() {
    let f = Lifecycle::new();
    let a = f.create_space("Alpha").await;
    let space_a = Lifecycle::space_id(&a, 1);
    let b = f.create_space("Beta").await;
    let space_b = Lifecycle::space_id(&b, 2);
    let snap = f.spaces().await;
    f.activate(&space_a, &snap, "n-a2").await.unwrap();
    let snap = f.spaces().await;
    f.activate(&space_b, &snap, "n-b2").await.unwrap();
    let snap = f.spaces().await;
    let principal_b = snap["spaces"].as_array().unwrap().iter().find(|s| s["id"] == space_b).unwrap()["principalBotId"]
        .as_str()
        .unwrap()
        .to_owned();
    let stale = snap["revision"].as_i64().unwrap() - 1;
    let err = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceDelete",
        json!({"hubId":f.hub.host_id,"spaceId":space_a,"expectedRevision":stale,"confirmName":"Alpha"}),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("spaces_revision_conflict") || err.to_string().contains("revision"), "{err}");
    // Exclui A; B e o Pessoal ficam iguais e a conta de IA intacta.
    dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceDelete",
        json!({"hubId":f.hub.host_id,"spaceId":space_a,"expectedRevision":snap["revision"],"confirmName":"Alpha"}),
    )
    .await
    .unwrap();
    let snap2 = f.spaces().await;
    assert_eq!(snap2["spaces"].as_array().unwrap().len(), 2);
    assert_eq!(snap2["spaces"][1]["principalBotId"].as_str().unwrap(), principal_b);
    assert!(!f.hub.store.bot(&principal_b).unwrap().unwrap().deleted);
    let slot = f.hub.store.kv_get(crate::fork::engine_accounts::SLOT).unwrap();
    assert!(slot.contains(ACCOUNT), "conta de IA intacta");
    // Os gatilhos seguem protegendo o principal de OUTRO Space.
    {
        let c = f.hub.store.db.locked();
        let direct = c.execute("DELETE FROM bots WHERE id=?1", [&principal_b]);
        assert!(direct.is_err(), "gatilho ainda protege o principal de B");
    }
    // Erro antes do commit não encerra ator nem publica: A já era inválida antes.
    let err = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceDelete",
        json!({"hubId":f.hub.host_id,"spaceId":space_a,"expectedRevision":snap["revision"],"confirmName":"Alpha"}),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("space_not_found"), "{err}");
    assert!(!f.hub.store.bot(&principal_b).unwrap().unwrap().deleted);
}

#[tokio::test]
async fn d543_delete_space_com_resposta_open_em_curso_devolve_space_busy_sem_apagar() {
    let f = Lifecycle::new();
    let created = f.create_space("Cliente").await;
    let space = Lifecycle::space_id(&created, 1);
    let snap = f.spaces().await;
    f.activate(&space, &snap, "n-open").await.unwrap();
    let bot = f.bot_of_space(&space).unwrap();
    let snap = f.spaces().await;
    let ticket = crate::fork::open_chat::admit_for_test(&f.hub, &bot.id, ACCOUNT).unwrap();
    let body =
        json!({"hubId":f.hub.host_id,"spaceId":space,"expectedRevision":snap["revision"],"confirmName":"Cliente"});
    let busy = dispatch(&f.hub, &Caller::Local, "donnaSpaceDelete", body).await.unwrap();
    assert_eq!(busy["code"], "space_busy");
    assert_eq!(busy["busy"], json!([{"botId": bot.id, "name": bot.name}]));
    assert_eq!(f.spaces().await, snap, "a recusa não apaga nada");
    assert!(!f.hub.store.bot(&bot.id).unwrap().unwrap().deleted);
    drop(ticket);
    dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceDelete",
        json!({"hubId":f.hub.host_id,"spaceId":space,"expectedRevision":snap["revision"],"confirmName":"Cliente"}),
    )
    .await
    .unwrap();
    assert!(f.hub.store.bot(&bot.id).unwrap().unwrap().deleted, "livre da reserva, o Space sai");
}

#[tokio::test]
async fn d543_delete_so_desktop_lista_de_permissao() {
    let f = Lifecycle::new();
    let created = f.create_space("Cliente").await;
    let space = created["spaces"][1]["id"].as_str().unwrap().to_owned();
    let rev = created["revision"].as_i64().unwrap();
    // Plataforma NULL/desconhecida recusa (antes passava pela lista de bloqueio).
    f.hub
        .store
        .put_device(&Device {
            key: "desktop-null".into(),
            name: "App".into(),
            platform: String::new(),
            source: DeviceSource::Account,
            grant_id: None,
            scopes: vec![],
            lease_until: None,
            created_at: 0,
            last_seen_at: None,
        })
        .unwrap();
    {
        let mut c = f.hub.store.db.locked();
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).unwrap();
        let err = Store::delete_space_on(
            &tx,
            &space,
            rev,
            "Cliente",
            Some("desktop-null"),
            &f.hub.host_id,
            &[],
            &[],
            "{}",
            &[],
            &[],
        )
        .unwrap_err();
        assert!(err.to_string().contains("space_delete_desktop_only"), "{err}");
    }
    // macos passa pela guarda de plataforma (falha depois, no nome — prova de que
    // a plataforma não é o motivo da recusa).
    f.hub
        .store
        .put_device(&Device {
            key: "mac-key".into(),
            name: "Mac".into(),
            platform: "macos".into(),
            source: DeviceSource::Account,
            grant_id: None,
            scopes: vec![],
            lease_until: None,
            created_at: 0,
            last_seen_at: None,
        })
        .unwrap();
    {
        let mut c = f.hub.store.db.locked();
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).unwrap();
        let err = Store::delete_space_on(
            &tx,
            &space,
            rev,
            "NomeErrado",
            Some("mac-key"),
            &f.hub.host_id,
            &[],
            &[],
            "{}",
            &[],
            &[],
        )
        .unwrap_err();
        assert!(err.to_string().contains("space_delete_confirmation_mismatch"), "{err}");
    }
}

#[tokio::test]
async fn d543_delete_foreign_trabalho_duravel_e_ator_recusam_sem_efeitos() {
    let f = Lifecycle::new();
    let created = f.create_space("Legado").await;
    let space = Lifecycle::space_id(&created, 1);
    let conv = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaConversationCreate",
        json!({
            "hubId":f.hub.host_id,"spaceId":space,"botId":"personal-principal"
        }),
    )
    .await
    .unwrap();
    let chat = conv["conversation"]["id"].as_str().unwrap().to_owned();
    let before = f.hub.store.bot("personal-principal").unwrap().unwrap().config;
    // Nenhum bot nativo: a única conversa pertence ao Berga do Pessoal.
    f.hub.store.kv_set(&format!("turn.inflight.{chat}"), "incompleto").unwrap();
    let snap = f.spaces().await;
    let body =
        json!({"hubId":f.hub.host_id,"spaceId":space,"expectedRevision":snap["revision"],"confirmName":"Legado"});
    let busy = dispatch(&f.hub, &Caller::Local, "donnaSpaceDelete", body.clone()).await.unwrap();
    assert_eq!(busy["code"], "space_busy");
    assert_eq!(busy["busy"], json!([{"botId":"personal-principal","name":"Berga"}]));
    assert_eq!(f.spaces().await, snap);
    f.hub.store.db.locked().execute("DELETE FROM kv WHERE k=?1", [format!("turn.inflight.{chat}")]).unwrap();
    // Agora só trabalho RAM, sem checkpoint SQL: o handler consulta o executor
    // foreign e usa sua decisão. A recusa não envia Shutdown nem altera a conversa.
    let mut commands = f.hub.test_bot_commands("personal-principal");
    let pending = dispatch(&f.hub, &Caller::Local, "donnaSpaceDelete", body);
    let decision = async {
        match commands.recv().await.unwrap() {
            crate::agent::bot::Cmd::SpaceDeleteGuard { chats, all, reply, .. } => {
                assert!(!all);
                assert_eq!(chats, vec![chat.clone()]);
                reply.send(true).unwrap();
            }
            _ => panic!("comando de reserva esperado"),
        }
    };
    let (busy, ()) = tokio::join!(pending, decision);
    assert_eq!(busy.unwrap()["code"], "space_busy");
    assert_eq!(f.spaces().await, snap);
    assert_eq!(
        serde_json::to_value(f.hub.store.bot("personal-principal").unwrap().unwrap().config).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    assert!(commands.try_recv().is_err(), "sem Shutdown antes de commit");
}

#[tokio::test]
async fn d543_delete_falha_sql_nao_publica_nem_encerra_e_reabrir_nao_recria_conversa() {
    let f = Lifecycle::new();
    let created = f.create_space("Alvo").await;
    let space = Lifecycle::space_id(&created, 1);
    let snap = f.spaces().await;
    f.activate(&space, &snap, "rollback").await.unwrap();
    let bot = f.bot_of_space(&space).unwrap();
    let snap = f.spaces().await;
    f.hub.store.db.locked().execute_batch("CREATE TEMP TRIGGER fail_space_delete BEFORE DELETE ON donna_space_conversations BEGIN SELECT RAISE(ABORT,'fixture-delete'); END;").unwrap();
    let mut events = f.hub.events.subscribe();
    let body = json!({"hubId":f.hub.host_id,"spaceId":space,"expectedRevision":snap["revision"],"confirmName":"Alvo"});
    assert!(dispatch(&f.hub, &Caller::Local, "donnaSpaceDelete", body.clone()).await.is_err());
    assert_eq!(f.spaces().await, snap);
    assert!(!f.hub.store.bot(&bot.id).unwrap().unwrap().deleted);
    while let Ok(event) = events.try_recv() {
        assert_ne!(event["type"], "spaces");
        assert_ne!(event["bot"]["deleted"], true);
    }
    f.hub.store.db.locked().execute_batch("DROP TRIGGER fail_space_delete").unwrap();
    dispatch(&f.hub, &Caller::Local, "donnaSpaceDelete", body).await.unwrap();
    f.hub.shutdown().await;
    let reopened = Store::open(&f.dir.join("fixture.db")).unwrap();
    let n: i64 = reopened
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM donna_space_conversations WHERE space_id=?1", [&space], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
    assert!(reopened.bot(&bot.id).unwrap().unwrap().deleted);
}

#[tokio::test]
async fn d543_delete_apaga_camadas_jobs_kv_e_arquivos_sem_tocar_outro_space() {
    let f = Lifecycle::new();
    let created = f.create_space("Alvo").await;
    let space = Lifecycle::space_id(&created, 1);
    let snap = f.spaces().await;
    f.activate(&space, &snap, "limpeza").await.unwrap();
    let bot = f.bot_of_space(&space).unwrap();
    let layer = uuid::Uuid::new_v4().to_string();
    let kept = uuid::Uuid::new_v4().to_string();
    f.hub.store.kv_set(&format!("donna.space.memory.{space}.personal-principal"), &layer).unwrap();
    f.hub.store.kv_set(&format!("donna.routing.v1:{space}:guide"), "APAGAR").unwrap();
    f.hub.store.kv_set("donna.routing.v1:outro:guide", "MANTER").unwrap();
    f.hub
        .store
        .memory_write(|db, now, rev| {
            for (id, text) in [(&layer, "memória foreign alvo"), (&kept, "memória preservada")] {
                codync_mem::store::add(
                    db,
                    codync_mem::store::Writer::User,
                    codync_mem::store::New { bot: Some(id), text, room: None, evidence: &[] },
                    now,
                    rev,
                )?;
            }
            Ok(())
        })
        .unwrap();
    {
        let db = f.hub.store.db.locked();
        db.execute(
            "INSERT INTO donna_memory_auto_jobs(id,bot,chat,space,context) VALUES('job-alvo',?1,?1,?2,'{}')",
            rusqlite::params![bot.id, space],
        )
        .unwrap();
        db.execute("INSERT INTO donna_memory_suggestions(id,job,bot,memory,operation,text,origin,nature,fact,created_at) VALUES('sugestao-alvo','job-alvo',?1,?1,'add','apagar','{}','unknown','f',0)", [&bot.id]).unwrap();
        db.execute("INSERT INTO donna_memory_sleep_jobs(id,bot,chat,space,day,context,source,created_at) VALUES('sono-alvo',?1,?1,?2,'dia','{}','conteudo',0)", rusqlite::params![bot.id,space]).unwrap();
        db.execute("INSERT INTO donna_memory_sleep_kept(entry,rev,run,sources) VALUES(1,1,'sono-alvo','conteudo')", [])
            .unwrap();
        db.execute("INSERT INTO donna_memory_sleep_facts(entry,rev,run,verdict,reason) VALUES(1,1,'sono-alvo','covered','conteudo')", []).unwrap();
    }
    let files = f.dir.join("spaces").join(&space).join("conversations/chat/workspace/uploads");
    std::fs::create_dir_all(&files).unwrap();
    std::fs::write(files.join("alvo.txt"), "apagar").unwrap();
    let untouched = f.dir.join("spaces/outro/workspace");
    std::fs::create_dir_all(&untouched).unwrap();
    std::fs::write(untouched.join("manter.txt"), "manter").unwrap();
    let personal = serde_json::to_value(f.hub.store.bot("personal-principal").unwrap().unwrap().config).unwrap();
    let accounts = f.hub.store.kv_get(crate::fork::engine_accounts::SLOT).unwrap();
    let snap = f.spaces().await;
    dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceDelete",
        json!({"hubId":f.hub.host_id,"spaceId":space,"expectedRevision":snap["revision"],"confirmName":"Alvo"}),
    )
    .await
    .unwrap();
    {
        let db = f.hub.store.db.locked();
        assert_eq!(codync_mem::store::layer(&db, Some(&layer)).unwrap().len(), 0);
        assert_eq!(codync_mem::store::layer(&db, Some(&kept)).unwrap().len(), 1);
        for table in [
            "donna_memory_auto_jobs",
            "donna_memory_suggestions",
            "donna_memory_sleep_jobs",
            "donna_memory_sleep_kept",
            "donna_memory_sleep_facts",
            "donna_space_delete_spaces",
            "donna_space_delete_bots",
        ] {
            let count: i64 = db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap();
            assert_eq!(count, 0, "{table}");
        }
    }
    assert!(f.hub.store.kv_get(&format!("donna.routing.v1:{space}:guide")).is_none());
    assert_eq!(f.hub.store.kv_get("donna.routing.v1:outro:guide").unwrap(), "MANTER");
    assert_eq!(serde_json::to_value(f.hub.store.bot("personal-principal").unwrap().unwrap().config).unwrap(), personal);
    assert_eq!(f.hub.store.kv_get(crate::fork::engine_accounts::SLOT).unwrap(), accounts);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while files.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("limpeza pós-commit conclui");
    assert_eq!(std::fs::read_to_string(untouched.join("manter.txt")).unwrap(), "manter");
}

#[tokio::test]
async fn d543_lifecycle_chamador_bot_recusa_os_dois_metodos() {
    let f = Lifecycle::new();
    let created = f.create_space("Alvo").await;
    let space = Lifecycle::space_id(&created, 1);
    let binding = crate::fork::sessoes::bindings::issue_in(
        &f.hub.store,
        "personal-principal",
        "personal-principal",
        "personal-principal",
        None,
        None,
    )
    .unwrap();
    let mut servers = [json!({"env":[]})];
    crate::fork::ponte_credencial::provisionar(&f.hub, "personal-principal", None, &binding, &mut servers).unwrap();
    let token =
        servers[0]["env"].as_array().unwrap().iter().find(|e| e["name"] == crate::fork::ponte_credencial::ENV).unwrap()
            ["value"]
            .as_str()
            .unwrap();
    let caller = f.hub.pontes.autenticar(&f.hub, token).unwrap();
    assert!(matches!(caller, Caller::Ponte { .. }));
    let before = f.spaces().await;
    for (method, body) in [
        ("donnaSpacePrincipal", f.principal_request(&space, &before, "bot-refused")),
        (
            "donnaSpaceDelete",
            json!({"hubId":f.hub.host_id,"spaceId":space,"expectedRevision":before["revision"],"confirmName":"Alvo"}),
        ),
    ] {
        assert!(dispatch(&f.hub, &caller, method, body).await.is_err());
    }
    assert_eq!(f.spaces().await, before);
}

#[tokio::test]
async fn d543_delete_limpa_selecao_background_e_custodia_de_sessao_so_do_alvo() {
    let f = Lifecycle::new();
    let created = f.create_space("Alvo").await;
    let space = Lifecycle::space_id(&created, 1);
    let snap = f.activate(&space, &created, "cleanup-review").await.unwrap();
    let bot = snap["spaces"][1]["principalBotId"].as_str().unwrap();
    {
        let db = f.hub.store.db.locked();
        for id in [&space, "outro"] {
            db.execute("INSERT INTO donna_background_space_selections(space,revision,updated_at) VALUES(?1,1,0)", [id])
                .unwrap();
        }
        for id in [bot, "personal-principal"] {
            db.execute(
                "INSERT INTO donna_session_acp(bot,acp_session,session) VALUES(?1,?2,?1)",
                rusqlite::params![id, format!("acp-{id}")],
            )
            .unwrap();
            db.execute("INSERT INTO donna_session_exposure(bot,session,at) VALUES(?1,?1,0)", [id]).unwrap();
            db.execute("INSERT INTO donna_memory_sessions(bot,session) VALUES(?1,?1)", [id]).unwrap();
            db.execute("INSERT INTO donna_memory_gateway_pending(bot,at) VALUES(?1,0)", [id]).unwrap();
            db.execute("INSERT INTO donna_memory_turns(bot,chat,turn,session) VALUES(?1,?1,1,?1)", [id]).unwrap();
            db.execute("INSERT INTO donna_memory_turn_origin(bot,chat,turn,origin) VALUES(?1,?1,1,'principal')", [id])
                .unwrap();
        }
    }
    dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceDelete",
        json!({"hubId":f.hub.host_id,"spaceId":space,"expectedRevision":snap["revision"],"confirmName":"Alvo"}),
    )
    .await
    .unwrap();
    let db = f.hub.store.db.locked();
    let remaining: String =
        db.query_row("SELECT space FROM donna_background_space_selections", [], |r| r.get(0)).unwrap();
    assert_eq!(remaining, "outro");
    for table in [
        "donna_session_acp",
        "donna_session_exposure",
        "donna_memory_sessions",
        "donna_memory_gateway_pending",
        "donna_memory_turns",
        "donna_memory_turn_origin",
    ] {
        let remaining: String = db.query_row(&format!("SELECT bot FROM {table}"), [], |r| r.get(0)).unwrap();
        assert_eq!(remaining, "personal-principal", "{table}");
    }
}

#[tokio::test]
async fn d543_delete_recusa_conversa_foreign_nao_reservada_antes_da_tx() {
    let f = Lifecycle::new();
    let created = f.create_space("Alvo").await;
    let space = Lifecycle::space_id(&created, 1);
    dispatch(
        &f.hub,
        &Caller::Local,
        "donnaConversationCreate",
        json!({"hubId":f.hub.host_id,"spaceId":space,"botId":"personal-principal"}),
    )
    .await
    .unwrap();
    let snap = f.spaces().await;
    let mut db = f.hub.store.db.locked();
    let tx = db.transaction().unwrap();
    let err = Store::delete_space_on(
        &tx,
        &space,
        snap["revision"].as_i64().unwrap(),
        "Alvo",
        None,
        &f.hub.host_id,
        &[],
        &[],
        "{}",
        &[],
        &[],
    )
    .unwrap_err();
    assert!(err.to_string().contains("space_delete_membership_changed"), "{err}");
    assert_eq!(crate::fork::spaces::store::read(&tx).unwrap().spaces.len(), 2);
}

#[tokio::test]
async fn d543_delete_inventaria_tambem_conversa_de_bot_ja_tombstonado() {
    let f = Lifecycle::new();
    let created = f.create_space("Alvo").await;
    let space = Lifecycle::space_id(&created, 1);
    {
        let db = f.hub.store.db.locked();
        db.execute("INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary) VALUES('orfa',?1,'ja-excluido','Histórico',1,0)", [&space]).unwrap();
    }
    let snap = f.spaces().await;
    dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceDelete",
        json!({"hubId":f.hub.host_id,"spaceId":space,"expectedRevision":snap["revision"],"confirmName":"Alvo"}),
    )
    .await
    .unwrap();
    assert_eq!(f.spaces().await["spaces"].as_array().unwrap().len(), 1);
    assert_eq!(f.hub.store.space_delete_conversations(&space).unwrap().len(), 0);
}
