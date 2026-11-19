//! D543 fatia 2: contrato público, somente fixtures sintéticas; não inicia motores.
use super::{Caller, Hub, Identity, Store, dispatch};
use crate::{
    LockExt,
    store::{BotConfig, Permission, Scope},
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

const ACCOUNT: &str = "ac_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = std::fs::canonicalize(std::fs::canonicalize(std::env::temp_dir()).unwrap())
            .unwrap()
            .join(format!("d543-birth-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Hub::new(
            Store::open(&dir.join("fixture.db")).unwrap(),
            "fixture".into(),
            Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        let f = Self { hub, dir };
        f.account("connected");
        let personal: BotConfig = serde_json::from_value(json!({
            "id":"personal-principal", "name":"Berga", "backend":"codex", "cwd":f.dir,
            "description":"PROMPT_PESSOAL_SINTETICO", "ownerPreferences":"PREFERENCIA_PESSOAL_SINTETICA",
            "manageTeam":true, "sharedMemory":true, "harnessMemory":true
        }))
        .unwrap();
        f.hub.store.save_bot(&personal).unwrap();
        f.hub
            .store
            .kv_set(
                "team.settings.v1",
                &json!({
                    "primaryBotId":personal.id, "permission":"auto"
                })
                .to_string(),
            )
            .unwrap();
        f
    }

    fn account(&self, state: &str) {
        // Referências inertes dentro da fixture: nenhum probe, CLI, cofre ou credencial.
        self.hub
            .store
            .kv_set(
                crate::fork::engine_accounts::SLOT,
                &json!({
                    "salt":"00000000-0000-4000-8000-000000000543", "accounts":[{
                        "id":ACCOUNT, "backend":"codex", "billing":"paid_api", "state":state,
                        "launch":{"kind":"cli_profile", "executable":self.dir.join("cli-inerte"),
                            "directory":self.dir.join("perfil-inerte")},
                        "scope":"owner", "quotaIdentity":null, "verifiedAt":null,
                        "subscriptionProof":null, "models":[]
                    }]
                })
                .to_string(),
            )
            .unwrap();
    }

    fn snapshot(&self) -> Value {
        let mut value = serde_json::to_value(self.hub.store.spaces().unwrap()).unwrap();
        value["hubId"] = json!(self.hub.host_id);
        value
    }

    fn request(&self, nonce: &str) -> Value {
        json!({"hubId":self.hub.host_id, "expectedRevision":self.hub.store.spaces().unwrap().revision,
            "clientNonce":nonce, "name":"Trabalho",
            "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}},
            "principal":{"name":"Berga","backend":"codex","engineAccountId":ACCOUNT}})
    }

    async fn birth(&self, body: Value) -> anyhow::Result<Value> {
        dispatch(&self.hub, &Caller::Local, "donnaSpaceBirth", body).await
    }

    fn counts(&self) -> Vec<i64> {
        let db = self.hub.store.db.locked();
        ["bots", "donna_space_conversations", "entries", "kv", "donna_space_birth_receipts", "donna_bot_sessions"]
            .iter()
            .map(|table| db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap())
            .collect()
    }

    fn principal(&self, snapshot: &Value) -> BotConfig {
        let id = snapshot["spaces"][1]["principalBotId"].as_str().expect("referência persistida do principal");
        self.hub.store.bot(id).unwrap().unwrap().config
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Somente diretórios vazios de UUIDs emitidos neste teste; nunca apagar conteúdo.
        for row in self.hub.store.bots().unwrap() {
            if row.config.id != "personal-principal" && crate::agent::workspace::is_managed(&row.config) {
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
async fn d543_birth_atomico_persiste_principal_e_conversa_sem_herdar_pessoal() {
    let f = Fixture::new();
    for (bot, text) in [(None, "MEMORIA_COMPARTILHADA_D543"), (Some("personal-principal"), "MEMORIA_PESSOAL_D543")] {
        f.hub
            .store
            .memory_write(|db, now, rev| {
                codync_mem::store::add(
                    db,
                    codync_mem::store::Writer::User,
                    codync_mem::store::New { bot, text, room: None, evidence: &[] },
                    now,
                    rev,
                )
            })
            .unwrap();
    }
    let settings = f.hub.store.kv_get("team.settings.v1");
    let mut events = f.hub.events.subscribe();
    let snapshot = f.birth(f.request("nascimento")).await.expect("birth deve estar disponível");
    assert_eq!(snapshot, f.snapshot());
    assert_eq!(snapshot["spaces"].as_array().unwrap().len(), 2);
    let cfg = f.principal(&snapshot);
    assert_eq!(cfg.space_id, snapshot["spaces"][1]["id"].as_str().unwrap());
    assert_eq!(cfg.name, "Berga");
    assert_eq!(cfg.backend, "codex");
    assert_eq!(cfg.fork_editor.engine_account_id.as_deref(), Some(ACCOUNT));
    assert_eq!(cfg.permission, Permission::Ask);
    assert!(!cfg.permission_chosen_by_user && !cfg.manage_team && !cfg.harness_memory && !cfg.computer);
    assert_eq!(cfg.shared_memory, Some(false));
    assert!(cfg.connectors.is_empty() && cfg.logins.is_empty() && cfg.skills.is_empty() && cfg.members.is_empty());
    assert!(cfg.command.is_none() && cfg.context.is_none() && cfg.created_by.is_none());
    assert_eq!(cfg.owner_preferences, "");
    assert!(!crate::chat::manage::enabled(&f.hub.store, &cfg));
    assert!(crate::fork::donna_fixa::is_donna_here(&f.hub.store, &cfg));
    assert!(crate::fork::donna_fixa::description(&f.hub.store, &cfg).contains("neste Space"));
    assert!(crate::fork::donna_fixa::preferences_line(&f.hub.store, &cfg).is_none());
    assert!(!serde_json::to_string(&cfg).unwrap().contains("PESSOAL_SINTETIC"));
    assert_eq!(cfg.memory_id(), cfg.id);
    assert!(crate::fork::pasta::workspace_proprio(&cfg));
    let conversations = f.hub.store.space_conversations(&cfg.space_id).unwrap();
    assert_eq!(conversations.len(), 1);
    let chat = &conversations[0];
    assert!(chat.primary && !chat.closed);
    assert_eq!(chat.id, cfg.id);
    assert_eq!(chat.bot_id, cfg.id);
    assert_eq!(chat.engine_account_id.as_deref(), Some(ACCOUNT));
    assert_eq!(f.counts()[2], 0, "nenhum turno, aviso ou prompt pessoal no nascimento");
    assert!(!f.hub.busy());
    assert_eq!(f.hub.store.kv_get("team.settings.v1"), settings);
    let mut published = vec![];
    while let Ok(event) = events.try_recv() {
        assert!(!matches!(event["type"].as_str(), Some("entry" | "status")), "{event}");
        published.push(event);
    }
    let space_event = published.iter().position(|e| e["type"] == "spaces").unwrap();
    let bot_event = published.iter().position(|e| e["type"] == "bot" && e["bot"]["id"] == cfg.id).unwrap();
    assert!(space_event < bot_event);
    assert!(published[space_event]["rev"].as_i64().unwrap() < published[bot_event]["rev"].as_i64().unwrap());
    let reopened = Store::open(&f.dir.join("fixture.db")).unwrap();
    assert_eq!(serde_json::to_value(reopened.spaces().unwrap()).unwrap(), {
        let mut stored = snapshot.clone();
        stored.as_object_mut().unwrap().remove("hubId");
        stored
    });
    assert_eq!(reopened.space_conversations(&cfg.space_id).unwrap()[0].bot_id, cfg.id);
    let screen =
        dispatch(&f.hub, &Caller::Local, "memory", json!({"spaceId":cfg.space_id,"botId":cfg.id})).await.unwrap();
    assert!(!screen.to_string().contains("MEMORIA_COMPARTILHADA_D543"));
    assert!(!screen.to_string().contains("MEMORIA_PESSOAL_D543"));
    f.hub
        .store
        .memory_write(|db, now, rev| {
            codync_mem::store::add(
                db,
                codync_mem::store::Writer::User,
                codync_mem::store::New { bot: Some(&cfg.id), text: "MEMORIA_TRABALHO_D543", room: None, evidence: &[] },
                now,
                rev,
            )
        })
        .unwrap();
    let own = dispatch(&f.hub, &Caller::Local, "memory", json!({"spaceId":cfg.space_id,"botId":cfg.id})).await.unwrap();
    assert!(own.to_string().contains("MEMORIA_TRABALHO_D543"));
    let personal = dispatch(&f.hub, &Caller::Local, "memory", json!({"botId":"personal-principal"})).await.unwrap();
    assert!(!personal.to_string().contains("MEMORIA_TRABALHO_D543"));
    let other = f.birth(f.request("outro-space-memoria")).await.unwrap();
    let other = other["spaces"].as_array().unwrap().last().unwrap();
    let foreign =
        dispatch(&f.hub, &Caller::Local, "memory", json!({"spaceId":other["id"],"botId":other["principalBotId"]}))
            .await
            .unwrap();
    assert!(!foreign.to_string().contains("MEMORIA_TRABALHO_D543"));
    assert!(dispatch(&f.hub, &Caller::Local, "memory", json!({"spaceId":other["id"],"botId":cfg.id})).await.is_err());
    f.hub.shutdown().await;
}

#[tokio::test]
async fn d543_birth_retry_nonce_digest_e_cas_sem_duplicar() {
    let f = Fixture::new();
    let request = f.request("mesmo-nonce");
    let first = f.birth(request.clone()).await.unwrap();
    let second = f.birth(f.request("outro-nonce")).await.unwrap();
    let before = (f.counts(), f.hub.store.current_rev());
    assert_eq!(f.birth(request.clone()).await.unwrap(), second, "retry vence CAS antigo e retorna snapshot atual");
    for (field, value) in [
        ("name", json!("Outro")),
        ("expectedRevision", second["revision"].clone()),
        ("principal", json!({"name":"Outro","backend":"codex","engineAccountId":ACCOUNT})),
    ] {
        let mut changed = request.clone();
        changed[field] = value;
        assert!(f.birth(changed).await.is_err(), "nonce com digest diferente: {field}");
    }
    let mut stale = request;
    stale["clientNonce"] = json!("nonce-novo-cas-velho");
    assert!(f.birth(stale).await.is_err());
    assert_eq!((f.counts(), f.hub.store.current_rev()), before);
    assert_eq!(f.snapshot(), second);
    assert_eq!(f.counts()[4], 2);
    assert_eq!(first["spaces"][1]["principalBotId"], second["spaces"][1]["principalBotId"]);
    assert_ne!(first["spaces"][1]["principalBotId"], second["spaces"][2]["principalBotId"]);
    assert_eq!(f.hub.store.bots().unwrap().iter().filter(|b| b.config.name == "Berga").count(), 3);
    f.hub.shutdown().await;
}

#[tokio::test]
async fn d543_birth_recusa_sem_dono_conta_ausente_outro_hub_motor_e_estado() {
    let f = Fixture::new();
    let initial = f.snapshot();
    let before = (f.counts(), f.hub.store.current_rev());
    for caller in [
        Caller::Device { key: "sem-vinculo".into(), scopes: vec![Scope::Control], autoridade: None },
        Caller::Device { key: "executor".into(), scopes: vec![Scope::Executor], autoridade: None },
        Caller::Device { key: "devices".into(), scopes: vec![Scope::OwnerDevices], autoridade: None },
        Caller::Pairing { key: "pair".into() },
    ] {
        assert!(dispatch(&f.hub, &caller, "donnaSpaceBirth", f.request("sem-dono")).await.is_err());
    }
    let other = Fixture::new();
    other
        .hub
        .store
        .kv_set(
            crate::fork::engine_accounts::SLOT,
            &json!({
                "salt":"00000000-0000-4000-8000-000000000544", "accounts":[]
            })
            .to_string(),
        )
        .unwrap();
    assert!(other.birth(other.request("conta-so-no-outro-registry")).await.is_err());
    for patch in [
        json!({"hubId":"outro-hub"}),
        json!({"principal":{"name":"Berga","backend":"codex"}}),
        json!({"principal":{"name":"Berga","backend":"codex","engineAccountId":"ausente"}}),
        json!({"principal":{"name":"Berga","backend":"claude","engineAccountId":ACCOUNT}}),
        json!({"principal":{"name":"","backend":"codex","engineAccountId":ACCOUNT}}),
        json!({"principal":{"name":"Berga","backend":"codex","engineAccountId":ACCOUNT,"manageTeam":true}}),
        json!({"principal":{"name":"Berga","backend":"codex","engineAccountId":ACCOUNT,"cwd":"/fixture/injetado"}}),
        json!({"connectors":["fixture-injetado"]}),
        json!({"clientNonce":""}),
        json!({"name":""}),
    ] {
        let mut request = f.request("invalido");
        for (key, value) in patch.as_object().unwrap() {
            request[key] = value.clone();
        }
        assert!(f.birth(request).await.is_err());
    }
    for state in ["disconnected", "unavailable", "unverified"] {
        f.account(state);
        assert!(f.birth(f.request(state)).await.is_err());
    }
    assert_eq!((f.counts(), f.hub.store.current_rev()), before);
    assert_eq!(f.snapshot(), initial);
    f.hub.shutdown().await;
    other.hub.shutdown().await;
}

#[tokio::test]
async fn d543_birth_falha_conversa_reverte_space_bot_nonce_revisao_e_eventos() {
    let f = Fixture::new();
    let previous = crate::agent::workspace::path(&uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&previous).unwrap();
    std::fs::write(previous.join("marcador-sintetico.txt"), "CONTEUDO_PREEXISTENTE_D543").unwrap();
    let initial = f.snapshot();
    let before = (f.counts(), f.hub.store.current_rev());
    let mut events = f.hub.events.subscribe();
    f.hub
        .store
        .db
        .locked()
        .execute_batch(
            "CREATE TRIGGER d543_fail_conversation BEFORE INSERT ON donna_space_conversations
         BEGIN SELECT RAISE(ABORT,'fixture-d543:' || NEW.bot_id); END;",
        )
        .unwrap();
    let request = f.request("rollback");
    let error = f.birth(request.clone()).await.unwrap_err();
    let failure = format!("{error:#}");
    let id: String = failure
        .split("fixture-d543:")
        .nth(1)
        .expect("falha deve atingir a escrita da conversa")
        .chars()
        .take(36)
        .collect();
    uuid::Uuid::parse_str(&id).expect("o trigger captura o UUID emitido pelo Host antes do rollback");
    let workspace = crate::agent::workspace::path(&id);
    assert!(!workspace.exists(), "rollback remove o workspace novo vazio");
    assert!(!workspace.parent().unwrap().exists(), "rollback remove também a pasta nova do bot");
    assert!(workspace.parent().unwrap().parent().unwrap().is_dir(), "a raiz compartilhada é preservada");
    assert_eq!((f.counts(), f.hub.store.current_rev()), before);
    assert_eq!(f.snapshot(), initial);
    assert!(matches!(events.try_recv(), Err(tokio::sync::broadcast::error::TryRecvError::Empty)));
    assert!(!f.hub.busy());
    assert!(previous.is_dir() && previous.parent().unwrap().is_dir());
    assert_eq!(std::fs::read_to_string(previous.join("marcador-sintetico.txt")).unwrap(), "CONTEUDO_PREEXISTENTE_D543");
    std::fs::remove_file(previous.join("marcador-sintetico.txt")).unwrap();
    std::fs::remove_dir(&previous).unwrap();
    std::fs::remove_dir(previous.parent().unwrap()).unwrap();
    f.hub.store.db.locked().execute_batch("DROP TRIGGER d543_fail_conversation").unwrap();
    f.birth(request).await.expect("rollback não consome nonce nem CAS");
    f.hub.shutdown().await;
}

#[tokio::test]
async fn d543_birth_principal_protegido_contra_apagar_mover_e_renomeavel_pelo_dono() {
    let f = Fixture::new();
    let snapshot = f.birth(f.request("protegido")).await.unwrap();
    let mut cfg = f.principal(&snapshot);
    assert!(f.hub.store.delete_bot(&cfg.id).is_err());
    assert!(
        dispatch(
            &f.hub,
            &Caller::Local,
            "deleteBot",
            json!({
                "spaceId":cfg.space_id, "botId":cfg.id
            })
        )
        .await
        .is_err()
    );
    let space = cfg.space_id.clone();
    assert!(f.hub.store.db.locked().execute("DELETE FROM bots WHERE id=?1", [&cfg.id]).is_err());
    assert!(f.hub.store.db.locked().execute("UPDATE bots SET deleted=1 WHERE id=?1", [&cfg.id]).is_err());
    assert!(
        dispatch(
            &f.hub,
            &Caller::Local,
            "donnaSpaceUpdate",
            json!({
                "spaceId":space,"expectedRevision":snapshot["revision"],"patch":{"principalBotId":"personal-principal"}
            })
        )
        .await
        .is_err()
    );
    let mut tampered = f.snapshot();
    tampered.as_object_mut().unwrap().remove("hubId");
    tampered["spaces"][1]["principalBotId"] = json!("personal-principal");
    assert!(f.hub.store.kv_set("donna.spaces.v1", &tampered.to_string()).is_err());
    cfg.space_id = crate::fork::spaces::PERSONAL.into();
    assert!(f.hub.store.save_bot(&cfg).is_err());
    cfg.space_id = space;
    let editor = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaBotEditor",
        json!({
            "spaceId":cfg.space_id, "botId":cfg.id
        }),
    )
    .await
    .unwrap();
    // D543 E1: o nome do principal é fixo "Berga" — o editor recusa a troca.
    let renamed = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaBotEditorSave",
        json!({
            "spaceId":cfg.space_id, "botId":cfg.id, "expectedVersion":editor["settingsVersion"],
            "patch":{"name":"Nome escolhido pelo dono"}
        }),
    )
    .await;
    assert!(renamed.is_err(), "nome do principal é fixo: {renamed:?}");
    assert_eq!(f.hub.store.bot(&cfg.id).unwrap().unwrap().config.name, "Berga");
    let list = dispatch(&f.hub, &Caller::Local, "donnaConversations", json!({"spaceId":cfg.space_id})).await.unwrap();
    assert_eq!(list["principal"]["id"], cfg.id);
    assert_eq!(list["principal"]["name"], "Berga");
    let mut global = f.hub.store.bot("personal-principal").unwrap().unwrap().config;
    global.name = "Troca global recusada".into();
    assert!(f.hub.store.save_bot(&global).is_err());
    assert_eq!(f.hub.store.team_settings().unwrap().primary_bot_id.as_deref(), Some("personal-principal"));
    f.hub.shutdown().await;
}

#[tokio::test]
async fn d543_birth_legado_sem_principal_nao_apresenta_pessoal_e_serde_aceita_ausencia() {
    let f = Fixture::new();
    let mut request = f.request("legado");
    request.as_object_mut().unwrap().remove("principal");
    request.as_object_mut().unwrap().remove("clientNonce");
    let legacy = dispatch(&f.hub, &Caller::Local, "donnaSpaceCreate", request).await.unwrap();
    let mut raw = legacy["spaces"][1].clone();
    raw.as_object_mut().unwrap().remove("principalBotId");
    let _: crate::fork::spaces::Space = serde_json::from_value(raw).unwrap();
    assert!(legacy["spaces"][1]["principalBotId"].is_null());
    let list = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaConversations",
        json!({
            "spaceId":legacy["spaces"][1]["id"]
        }),
    )
    .await
    .unwrap();
    assert!(list["principal"].is_null(), "Space legado não deve apresentar o principal pessoal");
    let personal = dispatch(&f.hub, &Caller::Local, "donnaConversations", json!({})).await.unwrap();
    assert_eq!(personal["principal"]["id"], "personal-principal");
    f.hub.shutdown().await;
}

#[tokio::test]
async fn d543_birth_hello_anuncia_space_birth_contract_1() {
    use futures::StreamExt;
    let f = Fixture::new();
    let events = crate::api::events_stream(&f.hub, 0, None, &Caller::Local).unwrap();
    tokio::pin!(events);
    let hello = events.next().await.unwrap();
    assert_eq!(hello["type"], "hello");
    assert_eq!(hello["spaceBirthContract"], 1);
    assert_eq!(hello["spaceLifecycleContract"], 1);
    let rpc_hello = dispatch(&f.hub, &Caller::Local, "hello", json!({})).await.unwrap();
    assert_eq!(rpc_hello["spaceBirthContract"], 1);
    assert_eq!(rpc_hello["spaceLifecycleContract"], 1);
    f.hub.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn d543_birth_concorrente_mesmo_nonce_uma_identidade_e_cas_disputa_uma_vaga() {
    let f = Fixture::new();
    let request = f.request("concorrente");
    let (a, b) = tokio::join!(f.birth(request.clone()), f.birth(request));
    assert_eq!(a.unwrap(), b.unwrap());
    assert_eq!(f.hub.store.spaces().unwrap().spaces.len(), 2);
    assert_eq!(f.counts()[0], 2);
    assert_eq!(f.counts()[1], 2);
    let (a, b) = tokio::join!(f.birth(f.request("cas-a")), f.birth(f.request("cas-b")));
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(f.hub.store.spaces().unwrap().spaces.len(), 3);
    assert_eq!(f.counts()[0], 3);
    assert_eq!(f.counts()[1], 3);
    f.hub.shutdown().await;
}

#[path = "birth_review_tests.rs"]
mod review;
