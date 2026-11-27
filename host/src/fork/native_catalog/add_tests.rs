#![allow(clippy::unwrap_used)]
use super::{ADD_METHOD, METHOD};
use crate::{
    LockExt,
    api::{devices::Caller, dispatch},
    fork::spaces::PERSONAL,
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, EntryKind, Lane, Permission, Scope, Store},
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("donna-native-add-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("hub.db")).unwrap();
        Self {
            hub: Hub::new(store, "native-test".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0),
            dir,
        }
    }
    fn request(&self, key: &str) -> Value {
        json!({"hubId":self.hub.host_id,"spaceId":PERSONAL,"catalogKey":"pesquisador",
            "profileVersion":2,"clientRequestKey":key,"intent":"add",
            "expectedGeneration":0,"expectedRevision":0,"backend":"codex"})
    }
    async fn add(&self, body: Value) -> anyhow::Result<Value> {
        dispatch(&self.hub, &Caller::Local, ADD_METHOD, body).await
    }
    async fn link(&self, space: &str) -> Value {
        let preview = dispatch(&self.hub, &Caller::Local, METHOD, json!({"hubId":self.hub.host_id,"spaceId":space}))
            .await
            .unwrap();
        preview["links"][0].clone()
    }
    fn cfg(&self, result: &Value) -> BotConfig {
        self.hub.store.bot(result["botId"].as_str().unwrap()).unwrap().unwrap().config
    }
    /// Conta API conectada do Codex, ligável em qualquer Space do dono.
    fn connected_account(&self) -> String {
        let id = format!("ac_{}", "4".repeat(64));
        let registry = json!({
            "salt":uuid::Uuid::new_v4().to_string(),"accounts":[{
                "id":id,"backend":"codex","billing":"paid_api","state":"connected","scope":"owner",
                "launch":{"kind":"cli_profile","executable":self.dir.join("cli-inexistente"),
                    "directory":self.dir.join("perfil-sintetico")},
                "quotaIdentity":null,"verifiedAt":null,"subscriptionProof":null,"models":[]
            }]
        });
        self.hub.store.kv_set(crate::fork::engine_accounts::SLOT, &registry.to_string()).unwrap();
        id
    }
    fn count(&self, table: &str) -> i64 {
        self.hub.store.db.locked().query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for row in self.hub.store.bots().unwrap() {
            if crate::agent::workspace::is_managed(&row.config) {
                let path = PathBuf::from(&row.config.cwd);
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
async fn auto_gera_pedir_sem_poderes_e_repeticao_preserva_edicao_e_homonimo() {
    let f = Fixture::new();
    f.hub.store.set_team_settings(&json!({"permission":"auto"})).unwrap();
    let manual: BotConfig = serde_json::from_value(json!({
        "id":"homonimo","name":"Pesquisador","description":"Alma manual","backend":"codex"
    }))
    .unwrap();
    f.hub.store.save_bot(&manual).unwrap();
    let request = f.request("A");
    let added = f.add(request.clone()).await.unwrap();
    assert_eq!(added["state"], "active");
    let mut cfg = f.cfg(&added);
    assert_ne!(cfg.id, "homonimo");
    assert_eq!(cfg.permission, Permission::Ask);
    assert!(!cfg.permission_chosen_by_user);
    assert!(cfg.created_by.is_none());
    assert!(!cfg.is_group());
    assert!(!cfg.computer && !cfg.manage_team && !cfg.harness_memory);
    assert_eq!(cfg.shared_memory, Some(false));
    assert!(cfg.skills.is_empty() && cfg.connectors.is_empty() && cfg.logins.is_empty());
    assert!(cfg.members.is_empty() && cfg.command.is_none() && cfg.context.is_none());
    assert!(cfg.fork_editor.engine_account_id.is_none() && cfg.model.is_none());
    assert!(crate::fork::pasta::workspace_proprio(&cfg));
    assert!(cfg.description.starts_with("Você é o Pesquisador da Mexerica."));
    assert_eq!(cfg.avatar_config, Some(crate::fork_avatars::preset("research", "", false)));
    assert_eq!(f.count("entries"), 0);
    assert!(!f.hub.busy());
    assert_eq!(f.hub.routines.list(&cfg.id)["routines"].as_array().unwrap().as_slice(), &[] as &[Value]);
    // O editor normal continua dono da cópia. Hidden não é remoção.
    let editor = dispatch(&f.hub, &Caller::Local, "donnaBotEditor", json!({"botId":cfg.id})).await.unwrap();
    dispatch(
        &f.hub,
        &Caller::Local,
        "donnaBotEditorSave",
        json!({
            "botId":cfg.id,"expectedVersion":editor["settingsVersion"],
            "patch":{"name":"Nome do dono","description":"Alma privada do dono"}
        }),
    )
    .await
    .unwrap();
    cfg = f.cfg(&added);
    cfg.hidden = true;
    f.hub.store.save_bot(&cfg).unwrap();
    f.hub.store.set_team_settings(&json!({"permission":"auto"})).unwrap();
    assert_eq!(f.cfg(&added).permission, Permission::Ask);
    assert_eq!(f.link(PERSONAL).await["state"], "active");
    let rev = f.hub.store.current_rev();
    assert_eq!(f.add(request).await.unwrap(), added);
    let repeated = f.add(f.request("outra-aba")).await.unwrap();
    assert_eq!(repeated["botId"], added["botId"]);
    assert_eq!(f.cfg(&added).description, "Alma privada do dono");
    assert_eq!(f.cfg(&added).name, "Nome do dono");
    assert_eq!(f.hub.store.current_rev(), rev, "retry não salva/reconfigura o bot");
    assert_eq!(f.hub.store.bot("homonimo").unwrap().unwrap().config.description, "Alma manual");
    assert_eq!(f.count("donna_native_slots"), 1);
    assert_eq!(f.count("donna_native_receipts"), 2);
    let mut changed = f.request("A");
    changed["backend"] = json!("claude");
    assert!(f.add(changed).await.is_err());
    f.hub.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duas_intencoes_concorrentes_da_mesma_previa_resolvem_um_bot() {
    let f = Fixture::new();
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let launch = |request: Value| {
        let hub = f.hub.clone();
        let barrier = barrier.clone();
        tokio::spawn(async move {
            barrier.wait().await;
            dispatch(&hub, &Caller::Local, ADD_METHOD, request).await.unwrap()
        })
    };
    let one = launch(f.request("A"));
    let two = launch(f.request("B"));
    barrier.wait().await;
    let (one, two) = tokio::join!(one, two);
    let (one, two) = (one.unwrap(), two.unwrap());
    assert_eq!(one["botId"], two["botId"]);
    assert_eq!(one["generation"], 1);
    assert_eq!(f.count("bots"), 1);
    assert_eq!(f.count("donna_native_slots"), 1);
    assert_eq!(f.count("donna_native_receipts"), 2);
    let (one, two) = tokio::join!(f.add(f.request("A")), f.add(f.request("A")));
    assert_eq!(one.unwrap(), two.unwrap());
    assert_eq!(f.count("donna_native_receipts"), 2);
    f.hub.shutdown().await;
}

#[tokio::test]
async fn remocao_readicao_e_replays_conservam_destino_historico_sem_recriar_no_boot() {
    let f = Fixture::new();
    let a = f.request("A");
    let first = f.add(a.clone()).await.unwrap();
    let bot1 = first["botId"].as_str().unwrap();
    f.hub
        .store
        .insert_entry(
            &Lane::main(bot1),
            EntryKind::User,
            1,
            &json!({"text":"Histórico privado da primeira identidade"}),
        )
        .unwrap();
    f.hub
        .store
        .memory_write(|db, now, rev| {
            codync_mem::store::add(
                db,
                codync_mem::store::Writer::User,
                codync_mem::store::New {
                    bot: Some(bot1),
                    text: "Prefere revisar comparações",
                    room: None,
                    evidence: &[],
                },
                now,
                rev,
            )
        })
        .unwrap();
    assert_eq!(f.count("memory_entries"), 1);
    dispatch(&f.hub, &Caller::Local, "deleteBot", json!({"botId":bot1})).await.unwrap();
    let removed = f.link(PERSONAL).await;
    assert_eq!(removed["state"], "removed");
    assert_eq!(removed["generation"], 1);
    assert_eq!(removed["botId"], bot1);
    assert_eq!(f.add(a.clone()).await.unwrap()["state"], "removed");
    assert!(f.add(f.request("desconhecida-atrasada")).await.is_err());
    // Migração/reabertura/refresh não cadastram outro bot nem apagam tombstones/recibos.
    let reopened = Store::open(&f.dir.join("hub.db")).unwrap();
    reopened.initialize_default_donna().unwrap();
    assert!(reopened.bots().unwrap().iter().all(|b| b.deleted));
    let before = f.count("bots");
    assert_eq!(f.link(PERSONAL).await, removed);
    assert_eq!(f.count("bots"), before);
    drop(reopened);
    let mut b = f.request("B");
    b["intent"] = json!("readd");
    b["expectedGeneration"] = removed["generation"].clone();
    b["expectedRevision"] = removed["revision"].clone();
    let second = f.add(b.clone()).await.unwrap();
    let bot2 = second["botId"].as_str().unwrap();
    assert_ne!(bot1, bot2);
    assert_eq!(second["generation"], 2);
    let replay = f.add(a.clone()).await.unwrap();
    assert_eq!(replay["botId"], bot1);
    assert_eq!(replay["generation"], 1);
    assert_eq!(replay["state"], "removed");
    assert_eq!(f.add(b.clone()).await.unwrap(), second);
    assert!(
        f.hub
            .store
            .db
            .locked()
            .execute("UPDATE donna_native_receipts SET bot_id=?1 WHERE client_request_key='A'", [bot2])
            .is_err(),
        "recibo é imutável também no armazenamento"
    );
    assert!(f.add(f.request("ainda-desconhecida")).await.is_err());
    let mut wrong_readd = b;
    wrong_readd["clientRequestKey"] = json!("A");
    assert!(f.add(wrong_readd).await.is_err());
    assert!(dispatch(&f.hub, &Caller::Local, "deleteBot", json!({"botId":bot1})).await.is_err());
    // Também cobre repetição interna da exclusão antiga: CAS por ID não alcança bot2.
    f.hub.store.delete_bot(bot1).unwrap();
    assert!(!f.hub.store.bot(bot2).unwrap().unwrap().deleted);
    assert_eq!(f.link(PERSONAL).await["botId"], bot2);
    assert_eq!(f.link(PERSONAL).await["state"], "active");
    assert_eq!(f.count("entries"), 0);
    assert_eq!(f.count("memory_entries"), 0, "readição não restaura memória da identidade removida");
    assert_eq!(f.cfg(&second).shared_memory, Some(false));
    assert_ne!(f.cfg(&first).cwd, f.cfg(&second).cwd);
    f.hub.shutdown().await;
}

#[tokio::test]
async fn falha_de_recibo_reverte_bot_slot_sessao_e_revisao_na_mesma_transacao() {
    let f = Fixture::new();
    f.hub
        .store
        .db
        .locked()
        .execute_batch(
            "CREATE TRIGGER falha_bn02 BEFORE INSERT ON donna_native_receipts
         BEGIN SELECT RAISE(ABORT,'falha deliberada de recibo'); END;",
        )
        .unwrap();
    let revision = f.hub.store.current_rev();
    assert!(f.add(f.request("rollback")).await.is_err());
    for table in
        ["bots", "donna_native_slots", "donna_native_receipts", "donna_space_conversations", "donna_bot_sessions"]
    {
        assert_eq!(f.count(table), 0, "rollback de {table}");
    }
    assert_eq!(f.hub.store.current_rev(), revision);
    assert!(!f.hub.busy());
    // Mesmo produtor de preparação e guarda de rollback, com ID capturado antes da falha.
    let mut cleanup = super::workspace::PreparedWorkspace::default();
    let cfg: BotConfig = serde_json::from_value(json!({
        "id":"","name":"Rollback","backend":"codex","permission":"ask"
    }))
    .unwrap();
    let cfg = f.hub.prepare_new_bot_with_workspace(cfg, |cfg| cleanup.prepare(cfg)).unwrap();
    let directory = crate::agent::workspace::path(&cfg.id).parent().unwrap().to_owned();
    assert!(directory.join("workspace").is_dir());
    let mut body = f.request("rollback-capturado");
    body.as_object_mut().unwrap().remove("hubId");
    let request: super::model::Add = serde_json::from_value(body).unwrap();
    assert!(
        crate::fork::engine_setup::access::Authority::Local
            .with_db(&f.hub, |db| super::store::apply(db, &f.hub.host_id, &request, "digest-rollback", Some(&cfg)))
            .is_err()
    );
    drop(cleanup);
    assert!(!directory.exists(), "rollback não deixa bots/<uuid> nem workspace vazios");
    assert!(f.hub.store.bot(&cfg.id).unwrap().is_none());
    f.hub.store.db.locked().execute_batch("DROP TRIGGER falha_bn02").unwrap();
    let result = f.add(f.request("rollback")).await.unwrap();
    assert_eq!(result["generation"], 1);
    assert_eq!(f.count("bots"), 1);
    assert!(PathBuf::from(f.cfg(&result).cwd).is_dir(), "commit conserva diretórios mesmo vazios");
    f.hub
        .store
        .db
        .locked()
        .execute_batch(
            "CREATE TRIGGER falha_remocao_bn02 BEFORE UPDATE ON donna_native_slots
         WHEN NEW.state='removed' BEGIN SELECT RAISE(ABORT,'falha deliberada de remoção'); END;",
        )
        .unwrap();
    assert!(f.hub.store.delete_bot(result["botId"].as_str().unwrap()).is_err());
    assert!(!f.hub.store.bot(result["botId"].as_str().unwrap()).unwrap().unwrap().deleted);
    assert_eq!(f.link(PERSONAL).await["state"], "active", "exclusão e slot compartilham rollback");
    f.hub.store.db.locked().execute_batch("DROP TRIGGER falha_remocao_bn02").unwrap();
    f.hub.shutdown().await;
}

#[tokio::test]
async fn contexto_fechado_autoridade_e_space_nao_divulgam_nem_copiam_estado_privado() {
    let f = Fixture::new();
    for caller in [
        Caller::Device { key: "control-sem-posse".into(), scopes: vec![Scope::Control], autoridade: None },
        Caller::Device { key: "devices".into(), scopes: vec![Scope::OwnerDevices], autoridade: None },
        Caller::Device { key: "executor".into(), scopes: vec![Scope::Executor], autoridade: None },
        Caller::Device { key: "screen".into(), scopes: vec![Scope::Screen], autoridade: None },
        Caller::Pairing { key: "pair".into() },
        Caller::Bootstrap { key: "boot".into(), bootstrap_id: "boot".into() },
    ] {
        assert!(dispatch(&f.hub, &caller, ADD_METHOD, f.request("recusada")).await.is_err());
    }
    for (field, value) in [
        ("hubId", json!("outro-hub")),
        ("spaceId", json!("inexistente")),
        ("profileVersion", json!(999)),
        ("catalogKey", json!("desconhecido")),
        ("expectedGeneration", json!(7)),
        ("expectedRevision", json!(-1)),
        ("clientRequestKey", json!("")),
        ("intent", json!("restore")),
        ("backend", json!("custom")),
        ("permission", json!("auto")),
        ("permissionChosenByUser", json!(true)),
        ("createdBy", json!("principal")),
        ("engineAccountId", json!("conta")),
        ("forkEditor", json!({"engineAccountId":"conta"})),
        ("human", json!(true)),
        ("config", json!({})),
        ("computer", json!(true)),
    ] {
        let mut body = f.request("recusada");
        body[field] = value;
        assert!(f.add(body).await.is_err(), "recusa {field}");
    }
    let mut no_hub = f.request("sem-hub");
    no_hub.as_object_mut().unwrap().remove("hubId");
    assert!(f.add(no_hub).await.is_err());
    let mut no_space = f.request("sem-space");
    no_space.as_object_mut().unwrap().remove("spaceId");
    assert!(f.add(no_space).await.is_err());
    assert_eq!(f.count("bots"), 0);
    let state = f.hub.store.spaces().unwrap();
    let spaces = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceCreate",
        json!({
            "hubId":f.hub.host_id,"expectedRevision":state.revision,"name":"Reservado",
            "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"RE"}}
        }),
    )
    .await
    .unwrap();
    let other = spaces["spaces"][1]["id"].as_str().unwrap();
    let personal = f.add(f.request("A")).await.unwrap();
    let mut cfg = f.cfg(&personal);
    cfg.description = "Alma privada que nunca vai para a biblioteca".into();
    f.hub.store.save_bot(&cfg).unwrap();
    let other_link = f.link(other).await;
    assert_eq!(other_link["state"], "available");
    assert!(other_link["botId"].is_null());
    assert!(!other_link.to_string().contains(cfg.id.as_str()));
    let mut body = f.request("A");
    body["spaceId"] = json!(other);
    body.as_object_mut().unwrap().remove("backend");
    assert!(f.add(body.clone()).await.is_err());
    body["backend"] = json!("codex");
    assert!(f.add(body.clone()).await.is_err(), "fora do Pessoal exige a conta de IA");
    body["engineAccountId"] = json!(format!("ac_{}", "5".repeat(64)));
    assert!(f.add(body.clone()).await.is_err(), "conta desconhecida");
    let account = f.connected_account();
    body["engineAccountId"] = json!(account);
    body["backend"] = json!("claude");
    assert!(f.add(body.clone()).await.is_err(), "conta de outro motor");
    body["backend"] = json!("codex");
    assert_eq!(f.count("bots"), 1);
    let separate = f.add(body).await.unwrap();
    let space_cfg = f.cfg(&separate);
    assert_eq!(space_cfg.fork_editor.engine_account_id.as_deref(), Some(account.as_str()));
    assert_eq!(space_cfg.permission, Permission::Ask);
    assert!(space_cfg.skills.is_empty() && space_cfg.connectors.is_empty() && space_cfg.logins.is_empty());
    assert_ne!(separate["botId"], personal["botId"]);
    assert_ne!(f.cfg(&separate).cwd, cfg.cwd);
    assert_ne!(f.cfg(&separate).description, cfg.description);
    assert!(!f.link(other).await.to_string().contains(&cfg.description));
    let mut guessed = f.request("tentativa-cross-space");
    guessed["spaceId"] = json!(other);
    guessed["engineAccountId"] = json!(account);
    guessed["expectedGeneration"] = json!(1);
    guessed["expectedRevision"] = f.link(PERSONAL).await["revision"].clone();
    assert!(f.add(guessed).await.is_err());
    f.hub.shutdown().await;
}

/// A decisão A preserva a Alma integral e usa função editorial compatível com MAIN.
/// Não truncar silenciosamente nem embutir estilo no serviço.
#[tokio::test]
async fn planejador_deve_passar_pelo_limite_canonico_sem_alterar_a_alma() {
    let f = Fixture::new();
    let mut request = f.request("planejador");
    request["catalogKey"] = json!("planejador");
    let result = f.add(request).await.unwrap();
    let cfg = f.cfg(&result);
    let preview =
        dispatch(&f.hub, &Caller::Local, METHOD, json!({"hubId":f.hub.host_id,"spaceId":PERSONAL})).await.unwrap();
    assert_eq!(cfg.description, preview["profiles"][4]["description"].as_str().unwrap());
    assert_eq!(cfg.title, preview["profiles"][4]["title"].as_str().unwrap());
    assert_eq!(cfg.permission, Permission::Ask);
    f.hub.shutdown().await;
}

#[tokio::test]
async fn replay_ignora_backend_retirado_mas_novo_gesto_e_digest_divergente_recusam() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let f = Fixture::new();
    // Simula só o predicado mutável do registro, sem alterar estado global de outros testes.
    let known = AtomicBool::new(true);
    let checks = AtomicUsize::new(0);
    let check = |_: &str| {
        checks.fetch_add(1, Ordering::SeqCst);
        known.load(Ordering::SeqCst)
    };
    let mut a = f.request("backend-retirado");
    a.as_object_mut().unwrap().remove("hubId");
    let first = super::add::with_backend_check(&f.hub, &Caller::Local, a.clone(), check).await.unwrap();
    known.store(false, Ordering::SeqCst);
    assert_eq!(super::add::with_backend_check(&f.hub, &Caller::Local, a.clone(), check).await.unwrap(), first);
    assert_eq!(checks.load(Ordering::SeqCst), 1, "retry não consulta registro mutável");
    let mut changed = a.clone();
    changed["backend"] = json!("claude");
    let conflict = super::add::with_backend_check(&f.hub, &Caller::Local, changed, check).await.unwrap_err();
    assert!(conflict.is::<crate::remote::cloud::Conflict>());
    assert_eq!(checks.load(Ordering::SeqCst), 1, "digest diverge antes da validade dinâmica");
    let mut new = a.clone();
    new["clientRequestKey"] = json!("novo-gesto");
    assert!(super::add::with_backend_check(&f.hub, &Caller::Local, new, check).await.is_err());
    assert_eq!(checks.load(Ordering::SeqCst), 2);
    dispatch(&f.hub, &Caller::Local, "deleteBot", json!({"botId":first["botId"]})).await.unwrap();
    let historic = super::add::with_backend_check(&f.hub, &Caller::Local, a.clone(), check).await.unwrap();
    assert_eq!(historic["botId"], first["botId"]);
    assert_eq!(historic["state"], "removed");
    assert_eq!(checks.load(Ordering::SeqCst), 2, "replay removido também dispensa registro atual");
    let denied = Caller::Device { key: "sem-posse".into(), scopes: vec![Scope::Control], autoridade: None };
    assert!(super::add::with_backend_check(&f.hub, &denied, a, check).await.is_err());
    assert_eq!(f.count("donna_native_receipts"), 1);
    f.hub.shutdown().await;
}

#[tokio::test]
async fn falha_de_preparacao_limpa_so_diretorios_novos_vazios_e_preserva_conteudo() {
    let f = Fixture::new();
    let config = || {
        serde_json::from_value::<BotConfig>(json!({
            "id":"","name":"Preparação","backend":"codex","permission":"ask"
        }))
        .unwrap()
    };
    let mut empty = super::workspace::PreparedWorkspace::default();
    let mut allocated = PathBuf::new();
    let failure = f.hub.prepare_new_bot_with_workspace(config(), |cfg| {
        empty.prepare(cfg)?;
        allocated = crate::agent::workspace::path(&cfg.id).parent().unwrap().to_owned();
        anyhow::bail!("falha deliberada depois de preparar")
    });
    assert!(failure.is_err());
    drop(empty);
    assert!(!allocated.exists(), "falha de preparação limpa a identidade especulativa inteira");

    let mut content = super::workspace::PreparedWorkspace::default();
    let mut kept = PathBuf::new();
    assert!(
        f.hub
            .prepare_new_bot_with_workspace(config(), |cfg| {
                content.prepare(cfg)?;
                kept = crate::agent::workspace::path(&cfg.id);
                std::fs::write(kept.join("do-dono.txt"), "Conteúdo preservado")?;
                anyhow::bail!("falha com conteúdo posterior")
            })
            .is_err()
    );
    drop(content);
    assert_eq!(std::fs::read_to_string(kept.join("do-dono.txt")).unwrap(), "Conteúdo preservado");
    assert!(kept.parent().unwrap().is_dir());

    let mut existing = super::workspace::PreparedWorkspace::default();
    let mut owner = PathBuf::new();
    assert!(
        f.hub
            .prepare_new_bot_with_workspace(config(), |cfg| {
                owner = crate::agent::workspace::path(&cfg.id).parent().unwrap().to_owned();
                std::fs::create_dir_all(&owner)?;
                existing.prepare(cfg)
            })
            .is_err()
    );
    drop(existing);
    assert!(owner.is_dir(), "diretório pré-existente vazio não pertence à operação");
    assert_eq!(f.count("bots"), 0);
    // Artefatos exclusivos desta fixture, sem remoção recursiva.
    std::fs::remove_file(kept.join("do-dono.txt")).unwrap();
    std::fs::remove_dir(&kept).unwrap();
    std::fs::remove_dir(kept.parent().unwrap()).unwrap();
    std::fs::remove_dir(owner).unwrap();
    f.hub.shutdown().await;
}

#[test]
fn open_so_aceita_conta_codex_gerenciada() {
    use crate::fork::engine_accounts::Registry;
    let f = Fixture::new();
    let api = f.connected_account();
    let registry = Registry::load(&f.hub.store).unwrap();
    super::add::conta_permitida(&registry, &api, false).unwrap();
    let erro = super::add::conta_permitida(&registry, &api, true).unwrap_err();
    assert!(erro.to_string().contains("open_codex_unavailable"));
    let gerida = format!("ac_{}", "6".repeat(64));
    let launch = json!({"kind":"cli_profile","executable":f.dir.join("cli-inexistente"),"directory":f.dir.join("perfil-gerido")});
    let conta = json!({"salt":uuid::Uuid::new_v4().to_string(),"accounts":[{
        "id":gerida,"backend":"codex","billing":"subscription","state":"connected","launch":launch,
        "scope":"owner","quotaIdentity":null,"verifiedAt":null,"subscriptionProof":null,"models":[],
        "managed":{"label":"Conta","revision":1,"generation":0,"enabled":true,"requestId":"f","principal":"f",
            "profile":null,"setupRequest":null,"catalog":null}
    }]});
    f.hub.store.kv_set(crate::fork::engine_accounts::SLOT, &conta.to_string()).unwrap();
    let registry = Registry::load(&f.hub.store).unwrap();
    super::add::conta_permitida(&registry, &gerida, true).unwrap();
    assert!(super::add::conta_permitida(&registry, &api, true).is_err(), "conta desconhecida");
}
