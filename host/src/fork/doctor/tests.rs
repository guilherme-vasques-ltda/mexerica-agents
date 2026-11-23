#![allow(clippy::unwrap_used)]
use super::{METHOD, exit_code_for_report, format_human, offline_report_json};
use crate::{
    LockExt,
    api::{devices::Caller, dispatch},
    fork::spaces::PERSONAL,
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, Device, DeviceSource, EntryKind, Lane, Scope, Store, now_ms},
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("donna-doctor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let hub = Hub::new(
            Store::open(std::path::Path::new(":memory:")).unwrap(),
            "hub-doctor".into(),
            Identity::load_or_create(&dir).unwrap(),
            "token-secreto-nao-vaza".into(),
            0,
        );
        Self { hub, dir }
    }

    fn bot(&self, id: &str, space: &str) {
        let cfg: BotConfig = serde_json::from_value(json!({
            "id": id,
            "name": id,
            "spaceId": space,
            "kind": "agent",
            "backend": "claude",
            "description": "segredo-interno-bot",
            "cwd": "/caminho/privado/nao-deve-vazar",
        }))
        .unwrap();
        self.hub.store.save_bot(&cfg).unwrap();
    }

    async fn space(&self, name: &str) -> String {
        let initial = self.hub.store.spaces().unwrap();
        let created = dispatch(
            &self.hub,
            &Caller::Local,
            "donnaSpaceCreate",
            json!({
                "hubId": self.hub.host_id,
                "expectedRevision": initial.revision,
                "name": name,
                "tile": {"color": "oceano", "style": "soft", "symbol": {"kind": "letters", "value": "TR"}}
            }),
        )
        .await
        .unwrap();
        created["spaces"].as_array().unwrap().last().unwrap()["id"].as_str().unwrap().into()
    }

    fn seed_connected_ai(&self, paused: bool) {
        let reg = json!({
            "salt": "00000000-0000-4000-8000-000000000001",
            "accounts": [{
                "id": format!("ac_{}", "a".repeat(64)),
                "backend": "claude",
                "billing": "paid_api",
                "state": "connected",
                "launch": {"kind": "cli_profile", "executable": "/usr/local/bin/claude", "directory": "/tmp/privado-conta"},
                "scope": "owner",
                "quotaIdentity": format!("ac_{}", "b".repeat(64)),
                "verifiedAt": now_ms(),
                "subscriptionProof": null,
                "models": [],
                "managed": {
                    "label": "Conta Secreta Principal",
                    "revision": 1,
                    "generation": 1,
                    "enabled": true,
                    "paused": paused,
                    "requestId": "req-1",
                    "principal": null,
                    "profile": null,
                    "setupRequest": null
                }
            }]
        });
        self.hub.store.kv_set(crate::fork::engine_accounts::SLOT, &reg.to_string()).unwrap();
    }

    fn device(&self, key: &str, source: DeviceSource, scopes: Vec<Scope>, lease: Option<i64>) -> Caller {
        self.hub
            .store
            .put_device(&Device {
                key: key.into(),
                name: "aparelho".into(),
                platform: "ios".into(),
                source,
                grant_id: None,
                scopes: scopes.clone(),
                lease_until: lease,
                created_at: now_ms(),
                last_seen_at: None,
            })
            .unwrap();
        Caller::Device { key: key.into(), scopes, autoridade: None }
    }

    async fn read(&self, caller: &Caller, space: &str) -> anyhow::Result<Value> {
        dispatch(&self.hub, caller, METHOD, json!({"spaceId": space})).await
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn check_of<'a>(report: &'a Value, area: &str) -> &'a Value {
    report["checks"].as_array().unwrap().iter().find(|c| c["area"] == area).unwrap()
}

#[tokio::test]
async fn autoridade_e_validacao_fail_closed() {
    let f = Fixture::new();
    f.seed_connected_ai(false);

    // Local permitido.
    let ok_local = f.read(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(ok_local["version"], 1);
    assert_eq!(ok_local["hubId"], f.hub.host_id);
    assert_eq!(ok_local["spaceId"], PERSONAL);

    // Device com Control atual permitido.
    let control = f.device("dev-ctrl", DeviceSource::Local, vec![Scope::Control], None);
    assert!(f.read(&control, PERSONAL).await.is_ok());

    // Device sem Control, com lease vencido, Ponte, Pairing e Bootstrap recusados.
    let no_control = f.device("dev-screen", DeviceSource::Local, vec![Scope::Screen], None);
    assert!(f.read(&no_control, PERSONAL).await.is_err());
    let expired = f.device("dev-exp", DeviceSource::Account, vec![Scope::Control], Some(now_ms() - 10_000));
    assert!(f.read(&expired, PERSONAL).await.is_err());
    f.bot("bot_ponte", PERSONAL);
    let binding =
        crate::fork::sessoes::bindings::issue(&f.hub.store, "bot_ponte", "bot_ponte", "bot_ponte", Some("fixture-acp"))
            .unwrap();
    crate::fork::sessoes::bindings::activate(&f.hub.store, &binding, "fixture-acp").unwrap();
    let mut servers = vec![json!({"env": [], "args": ["mcp", "remote", "--context", "original"]})];
    crate::fork::ponte_credencial::provisionar(&f.hub, "bot_ponte", None, &binding, &mut servers).unwrap();
    let ponte_token = servers[0]["env"][0]["value"].as_str().unwrap().to_owned();
    let ponte = f.hub.pontes.autenticar(&f.hub, &ponte_token).unwrap();
    assert!(f.read(&ponte, PERSONAL).await.is_err());
    assert!(f.read(&Caller::Pairing { key: "k".into() }, PERSONAL).await.is_err());
    assert!(f.read(&Caller::Bootstrap { key: "k".into(), bootstrap_id: "b".into() }, PERSONAL).await.is_err());

    // Campos desconhecidos, spaceId inexistente e hubId errado são recusados.
    assert!(dispatch(&f.hub, &Caller::Local, METHOD, json!({})).await.is_err());
    assert!(dispatch(&f.hub, &Caller::Local, METHOD, json!({"spaceId": PERSONAL, "extra": 1})).await.is_err());
    assert!(dispatch(&f.hub, &Caller::Local, METHOD, json!({"hubId": "errado", "spaceId": PERSONAL})).await.is_err());
    assert!(f.read(&Caller::Local, "space-inexistente").await.is_err());
}

#[tokio::test]
async fn distingue_estados_ia_passivamente_sem_escrever_ou_vazar() {
    let f = Fixture::new();
    f.bot("bot1", PERSONAL);

    // Sem contas -> ai_no_active_account, overall error.
    let no_ai = f.read(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(no_ai["overall"], "error");
    assert_eq!(check_of(&no_ai, "hub")["code"], "hub_ok");
    assert_eq!(check_of(&no_ai, "ai_engine")["code"], "ai_no_active_account");
    assert_eq!(check_of(&no_ai, "ai_engine")["status"], "error");
    assert_eq!(check_of(&no_ai, "ai_engine")["actionKind"], "connect_ai");
    assert_eq!(exit_code_for_report(&no_ai), 1);

    // Todas as contas pausadas -> ai_all_paused, overall error.
    f.seed_connected_ai(true);
    let paused = f.read(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(paused["overall"], "error");
    assert_eq!(check_of(&paused, "ai_engine")["code"], "ai_all_paused");
    assert_eq!(check_of(&paused, "ai_engine")["actionKind"], "connect_ai");

    // Conta em descanso por limite -> ai_rate_limited, overall degraded.
    f.seed_connected_ai(false);
    let account_id = format!("ac_{}", "a".repeat(64));
    f.hub
        .store
        .kv_set(
            "fork.fallback-contas.v1",
            &json!({
                "descanso": {
                    account_id: {"ate": now_ms() + 600_000, "motivo": "esgotada"}
                }
            })
            .to_string(),
        )
        .unwrap();
    let limited = f.read(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(limited["overall"], "degraded");
    assert_eq!(check_of(&limited, "ai_engine")["code"], "ai_rate_limited");
    assert_eq!(check_of(&limited, "ai_engine")["status"], "warning");
    assert_eq!(exit_code_for_report(&limited), 1);

    // Conta saudável -> overall healthy.
    f.hub.store.kv_set("fork.fallback-contas.v1", "{}").unwrap();
    let kv_before = f.hub.store.kv_read(crate::fork::engine_accounts::SLOT).unwrap();
    let healthy = f.read(&Caller::Local, PERSONAL).await.unwrap();
    let kv_after = f.hub.store.kv_read(crate::fork::engine_accounts::SLOT).unwrap();
    assert_eq!(kv_before, kv_after, "doctor nunca escreve em banco");
    assert_eq!(healthy["overall"], "healthy");
    assert_eq!(check_of(&healthy, "ai_engine")["code"], "ai_ok");
    assert_eq!(exit_code_for_report(&healthy), 0);

    // Zero segredo ou caminho privado serializado.
    let wire = healthy.to_string();
    for forbidden in [
        "token-secreto-nao-vaza",
        "Conta Secreta Principal",
        "/tmp/privado-conta",
        "/caminho/privado/nao-deve-vazar",
        "segredo-interno-bot",
    ] {
        assert!(!wire.contains(forbidden), "não vazar {forbidden}");
    }

    // Registro corrompido -> fail-closed unavailable, nunca healthy.
    f.hub.store.kv_set(crate::fork::engine_accounts::SLOT, "{corrompido").unwrap();
    let corrupt = f.read(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(corrupt["overall"], "degraded");
    assert_eq!(check_of(&corrupt, "ai_engine")["status"], "unavailable");
    assert_eq!(check_of(&corrupt, "ai_engine")["count"], Value::Null);
}

#[tokio::test]
async fn pendencias_geram_waiting_user_com_dedupe_e_isolamento_de_space() {
    let f = Fixture::new();
    f.seed_connected_ai(false);
    f.bot("bot_pessoal", PERSONAL);
    let work = f.space("Trabalho").await;
    f.bot("bot_trabalho", &work);

    // Vincula a conta ao Space Trabalho para que IA do Trabalho também fique ok.
    let account_id = format!("ac_{}", "a".repeat(64));
    let mut cfg = f.hub.store.bot("bot_trabalho").unwrap().unwrap().config;
    cfg.fork_editor.engine_account_id = Some(account_id);
    f.hub.store.save_bot(&cfg).unwrap();

    // 1) Permissão pendente no Space Pessoal.
    f.hub
        .store
        .insert_entry(
            &Lane::main("bot_pessoal"),
            EntryKind::Permission,
            1,
            &json!({"status": "pending", "author": "bot_pessoal", "tool": "bash"}),
        )
        .unwrap();

    // 2) Tarefa em conferência (review) em projeto ativo do Space Pessoal (e uma em arquivado ignorada).
    let active_proj = uuid::Uuid::new_v4().to_string();
    let archived_proj = uuid::Uuid::new_v4().to_string();
    let review_task = uuid::Uuid::new_v4().to_string();
    let ignored_task = uuid::Uuid::new_v4().to_string();
    {
        let db = f.hub.store.db.locked();
        db.execute(
            "INSERT INTO donna_projects(hub_id,space_id,id,project,tasks) VALUES(?1,?2,?3,?4,?5)",
            rusqlite::params![
                f.hub.host_id,
                PERSONAL,
                active_proj,
                json!({
                    "id": active_proj, "name": "P1", "objective": "Obj",
                    "coordinatorBotId": null, "state": "active", "isDefault": true,
                    "revision": 1, "createdAt": 1, "updatedAt": 1
                })
                .to_string(),
                json!([{
                    "id": review_task, "parentTaskId": null, "assigneeBotId": "bot_pessoal",
                    "title": "Revisar entrega", "description": "", "acceptance": "", "evidence": "",
                    "state": "review"
                }])
                .to_string()
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO donna_projects(hub_id,space_id,id,project,tasks) VALUES(?1,?2,?3,?4,?5)",
            rusqlite::params![
                f.hub.host_id,
                PERSONAL,
                archived_proj,
                json!({
                    "id": archived_proj, "name": "P2", "objective": "Obj",
                    "coordinatorBotId": null, "state": "archived", "isDefault": false,
                    "revision": 1, "createdAt": 1, "updatedAt": 1
                })
                .to_string(),
                json!([{
                    "id": ignored_task, "parentTaskId": null, "assigneeBotId": "bot_pessoal",
                    "title": "Arquivada", "description": "", "acceptance": "", "evidence": "",
                    "state": "review"
                }])
                .to_string()
            ],
        )
        .unwrap();
    }

    let report_personal = f.read(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(report_personal["overall"], "waiting_user");
    assert_eq!(check_of(&report_personal, "approvals")["code"], "approvals_waiting");
    assert_eq!(check_of(&report_personal, "approvals")["count"], 2);
    assert_eq!(check_of(&report_personal, "approvals")["actionKind"], "review_waiting");
    assert_eq!(exit_code_for_report(&report_personal), 0);

    // Space Trabalho não vê as pendências do Space Pessoal.
    let report_work = f.read(&Caller::Local, &work).await.unwrap();
    assert_eq!(report_work["overall"], "healthy");
    assert_eq!(check_of(&report_work, "approvals")["code"], "approvals_ok");
    assert_eq!(check_of(&report_work, "approvals")["count"], 0);

    f.hub.store.kv_set("connectors", r#"[{"id":"legacy","name":"Fixture"}]"#).unwrap();
    let changes = f.hub.store.db.locked().total_changes();
    let partial = f.read(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(check_of(&partial, "approvals")["code"], "approvals_waiting");
    assert_eq!(check_of(&partial, "mcps")["status"], "unavailable");
    assert_eq!(partial["overall"], "degraded");
    assert_eq!(exit_code_for_report(&partial), 1);
    assert_eq!(f.hub.store.db.locked().total_changes(), changes);
}

#[tokio::test]
async fn conta_sem_slot_e_mcp_sem_header_nao_parecem_prontos() {
    let f = Fixture::new();
    f.seed_connected_ai(false);
    let store = &f.hub.store;
    let key = crate::fork::engine_accounts::SLOT;
    let mut registry: Value = serde_json::from_str(&store.kv_read(key).unwrap().unwrap()).unwrap();
    registry["accounts"][0]["launch"] = json!({"kind":"vault_profile","slot":"fixture-slot"});
    store.kv_set(key, &registry.to_string()).unwrap();
    let connector = |header: &str| {
        json!([{"id":"fixture","name":"Fixture","servico":"fixture","space":PERSONAL,
            "url":"https://mcp.example.invalid","headers":{"Authorization":header}}])
    };
    store.kv_set("connectors", &connector("").to_string()).unwrap();
    let before = store.db.locked().total_changes();
    let report = f.read(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(check_of(&report, "ai_engine")["count"], 0);
    assert_eq!(check_of(&report, "mcps")["code"], "mcps_attention_needed");
    assert_eq!(store.db.locked().total_changes(), before);

    store.kv_set("fixture-slot", "{}").unwrap();
    store.kv_set("connectors", &connector("fixture").to_string()).unwrap();
    let report = f.read(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(check_of(&report, "ai_engine")["code"], "ai_ok");
    assert_eq!(check_of(&report, "mcps")["code"], "mcps_ok");
}

#[tokio::test]
async fn space_vazio_nao_conta_credenciais_de_outros_spaces() {
    let f = Fixture::new();
    f.seed_connected_ai(false);
    let empty = f.space("Vazio").await;
    let report = f.read(&Caller::Local, &empty).await.unwrap();
    assert_eq!(check_of(&report, "ai_engine")["code"], "ai_no_active_account");
    assert_eq!(check_of(&report, "ai_engine")["count"], 0);
}

#[tokio::test]
async fn canais_rotinas_e_pedidos_entre_bots_sinalizam_degradacao() {
    use crate::fork::{cadeias::modelo::Recibo, retornos, sessoes::bindings};

    let f = Fixture::new();
    f.seed_connected_ai(false);
    f.bot("bot_a", PERSONAL);
    f.bot("bot_b", PERSONAL);
    f.hub.store.set_team_settings(&json!({"primaryBotId":"bot_a"})).unwrap();

    // WhatsApp habilitado com número desconectado + rotina com falha.
    {
        let db = f.hub.store.db.locked();
        db.execute(
            "INSERT INTO donna_plugin_installations(id,version,hub_id,builtin,state,generation,revision) VALUES('ins_1',1,?1,'whatsapp','enabled',1,1)",
            [&f.hub.host_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO donna_whatsapp_numbers(bot_id,account_id,state,linked_at,phone) VALUES('bot_a','acc_1','logged_out',1,NULL)",
            [],
        )
        .unwrap();
    }
    let execucao = bindings::fixture_execution(&f.hub, "bot_a");
    let notice = |bot| f.hub.store.insert_entry(&Lane::main(bot), EntryKind::Notice, 1, &json!({})).unwrap().id;
    let (origem, destino) = (notice("bot_a"), notice("bot_b"));
    let recibo = Recibo {
        chain_id: "chain_1".into(),
        admission_id: "adm_1".into(),
        run_id: "req_1".into(),
        bot_id: "bot_b".into(),
        level: 2,
        ordinal: 2,
    };
    let pedido = retornos::NovoPedido {
        id: "req_1",
        modo: retornos::Modo::Assincrono,
        de: "bot_a",
        para: "bot_b",
        execucao: &execucao,
        recibo: &recibo,
        aviso_origem: &origem,
        aviso_destino: &destino,
    };
    {
        let mut db = f.hub.store.db.locked();
        let tx = db.transaction().unwrap();
        retornos::aceitar_na(&tx, &pedido).unwrap();
        tx.commit().unwrap();
    }
    assert!(retornos::encerrar(&f.hub.store, pedido.id, retornos::Decisao::Prazo).unwrap());
    f.hub
        .store
        .kv_set(
            "routines.v1",
            &json!({
                "paused": false,
                "routines": [{
                    "id": "r1",
                    "botId": "bot_a",
                    "enabled": true,
                    "deleted": false,
                    "lastError": "falha de exemplo"
                }],
                "runs": []
            })
            .to_string(),
        )
        .unwrap();

    let report = f.read(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(report["overall"], "degraded");
    assert_eq!(exit_code_for_report(&report), 1);
    assert_eq!(check_of(&report, "channels")["code"], "channels_whatsapp_logged_out");
    assert_eq!(check_of(&report, "channels")["actionKind"], "check_channels");
    assert_eq!(check_of(&report, "routines")["code"], "routines_failed");
    assert_eq!(check_of(&report, "routines")["actionKind"], "check_routines");
    assert_eq!(check_of(&report, "delegation")["code"], "inter_bot_stalled");
    assert_eq!(check_of(&report, "delegation")["actionKind"], "open_torre");

    {
        let db = f.hub.store.db.locked();
        db.execute("UPDATE donna_whatsapp_numbers SET state='linked' WHERE bot_id='bot_a'", []).unwrap();
        db.execute(
            "INSERT INTO donna_whatsapp_numbers(bot_id,account_id,state,linked_at,phone) VALUES('bot_b','acc_2','logged_out',1,NULL)", [],
        ).unwrap();
        db.execute(
            "INSERT INTO donna_whatsapp_outbox(id,bot_id,account_id,chat_id,is_group,reply_to,body,origin,state,
             not_before,created_at,updated_at,expires_at)
             VALUES('fixture','bot_b','acc_2','fixture',0,'fixture','fixture','owner_card','uncertain',0,1,1,2)",
            [],
        )
        .unwrap();
    }
    let before = f.hub.store.db.locked().total_changes();
    let donna_only = f.read(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(check_of(&donna_only, "channels")["code"], "channels_ok");
    assert_eq!(check_of(&donna_only, "channels")["count"], 1);
    assert_eq!(f.hub.store.db.locked().total_changes(), before);
    f.hub.store.db.locked().execute("INSERT INTO donna_whatsapp_hub(id,mode) VALUES(1,'per_bot')", []).unwrap();
    let per_bot = f.read(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(check_of(&per_bot, "channels")["code"], "channels_whatsapp_logged_out");
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_whatsapp_numbers SET state='linked' WHERE bot_id='bot_b'", [])
        .unwrap();
    let per_bot = f.read(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(check_of(&per_bot, "channels")["code"], "channels_whatsapp_outbox_uncertain");
}

#[tokio::test]
async fn rotina_interrompida_exige_atencao_mas_cancelamento_nao() {
    let f = Fixture::new();
    f.seed_connected_ai(false);
    f.bot("bot_a", PERSONAL);
    for (latest, expected) in [("interrupted", "routines_failed"), ("cancelled", "routines_ok")] {
        let state = json!({"routines":[{"id":"r1","botId":"bot_a","enabled":true}],"runs":[
            {"routineId":"r1","status":"interrupted","createdAt":1},
            {"routineId":"r1","status":latest,"createdAt":2}
        ]});
        f.hub.store.kv_set("routines.v1", &state.to_string()).unwrap();
        let report = f.read(&Caller::Local, PERSONAL).await.unwrap();
        assert_eq!(check_of(&report, "routines")["code"], expected);
    }
}

#[test]
fn cli_offline_e_formatacao_distinguem_hub_offline() {
    let offline = offline_report_json("personal", 123_456);
    assert_eq!(offline["overall"], "hub_offline");
    assert_eq!(offline["hubId"], Value::Null);
    assert_eq!(offline["checks"][0]["code"], "hub_offline");
    assert_eq!(offline["checks"][0]["actionKind"], "start_hub");
    let text = format_human(&offline);
    assert!(text.contains("hub_offline"));
    assert!(text.contains("donnabot-host serve"));
}
