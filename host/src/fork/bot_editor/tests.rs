#![allow(clippy::unwrap_used)]
use super::{api, authorization, errors};
use crate::api::{devices::Caller, dispatch};
use crate::hub::{BotStatus, Hub};
use crate::remote::identity::Identity;
use crate::store::{BotConfig, EntryKind, Lane, Permission, Scope, Store};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("donnabot-editor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.kv_set("team.settings.v1", r#"{"primaryBotId":"donna","permission":"auto"}"#).unwrap();
        for id in ["donna", "dev"] {
            let cfg: BotConfig = serde_json::from_value(json!({
                "id":id,"name":id,"backend":"claude","cwd":dir,
                "command":"never-executed-private-launch-argument",
                "model":"saved-model","permission":"auto","permissionChosenByUser":true,
                "manageTeam":id == "donna", "description":"x".repeat(4001)
            }))
            .unwrap();
            store.save_bot(&cfg).unwrap();
        }
        Self {
            hub: Hub::new(store, "editor-test".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0),
            dir,
        }
    }

    fn cfg(&self, id: &str) -> BotConfig {
        self.hub.store.bot(id).unwrap().unwrap().config
    }

    fn start(&self) {
        for id in ["donna", "dev"] {
            self.hub.start_created_bot(&self.cfg(id));
        }
    }

    async fn read(&self, id: &str) -> Value {
        dispatch(&self.hub, &Caller::Local, "donnaBotEditor", json!({"botId":id})).await.unwrap()
    }

    async fn save(&self, id: &str, version: &Value, patch: Value) -> anyhow::Result<Value> {
        dispatch(
            &self.hub,
            &Caller::Local,
            "donnaBotEditorSave",
            json!({"botId":id,"expectedVersion":version,"patch":patch}),
        )
        .await
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn transport_and_tool_boundaries_deny_self_edit_without_writes() {
    let f = Fixture::new();
    let before = serde_json::to_value(f.cfg("dev")).unwrap();
    // D368: o salvamento exige a autoridade original do transporte, como a de um aparelho autenticado.
    let device = crate::store::Device {
        key: "owner".into(),
        name: "Telefone".into(),
        platform: "teste".into(),
        source: crate::store::DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: crate::store::now_ms(),
        last_seen_at: None,
    };
    f.hub.store.put_device(&device).unwrap();
    let (_, control) = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap();
    let legacy = Caller::Device { key: "owner".into(), scopes: vec![Scope::Control], autoridade: None };
    let read = dispatch(&f.hub, &legacy, "donnaBotEditor", json!({"botId":"dev"})).await.unwrap();
    let refused = json!({"botId":"dev","expectedVersion":read["settingsVersion"],"patch":{"name":"Sem transporte"}});
    assert!(dispatch(&f.hub, &legacy, "donnaBotEditorSave", refused).await.is_err());
    let read = dispatch(&f.hub, &control, "donnaBotEditor", json!({"botId":"dev"})).await.unwrap();
    assert!(!read.to_string().contains("never-executed-private-launch-argument"));
    assert_eq!(read["permissions"]["canEdit"], true);
    let request = json!({"botId":"dev","expectedVersion":read["settingsVersion"],"patch":{"name":"Humano"}});
    assert!(dispatch(&f.hub, &control, "donnaBotEditorSave", request.clone()).await.is_ok());
    for caller in [
        Caller::Device { key: "executor".into(), scopes: vec![Scope::Executor], autoridade: None },
        Caller::Device { key: "screen".into(), scopes: vec![Scope::Screen], autoridade: None },
        Caller::Pairing { key: "pairing".into() },
    ] {
        for method in ["donnaBotEditor", "donnaBotEditorSave"] {
            assert!(dispatch(&f.hub, &caller, method, request.clone()).await.is_err());
            assert!(api::call(&f.hub, &caller, method, request.clone()).await.is_err());
        }
    }
    let changed = serde_json::to_value(f.cfg("dev")).unwrap();
    assert_ne!(changed, before);
    for id in ["dev", "donna"] {
        for (tool, args) in [
            ("set_name", json!({"name":"Bot editou"})),
            ("set_instructions", json!({"instructions":"Bot editou","actor":"user"})),
        ] {
            assert!(
                crate::chat::profile::call(&f.hub, id, tool, &args)
                    .await
                    .unwrap_err()
                    .to_string()
                    .starts_with("not_allowed")
            );
            assert!(
                dispatch(&f.hub, &Caller::Local, "chatCall", json!({"botId":id,"name":tool,"arguments":args}))
                    .await
                    .is_err()
            );
        }
    }
    assert_eq!(serde_json::to_value(f.cfg("dev")).unwrap(), changed);
    assert_eq!(f.cfg("donna").name, "donna");
    let mut tools = Vec::from(crate::chat::profile::tools());
    tools.extend([json!({"name":"send_message"}), json!({"name":"donnaBotEditorSave"})]);
    tools.retain(authorization::retain_tool);
    assert_eq!(tools, [json!({"name":"send_message"})]);
}

#[tokio::test]
async fn partial_save_conflicts_and_keeps_human_autonomy_and_legacy_instructions() {
    let f = Fixture::new();
    f.start();
    let read = f.read("dev").await;
    f.hub.store.touch_bot("dev").unwrap();
    f.hub.set_runtime("dev", |r| r.activity = "Progresso".into());
    assert_eq!(f.read("dev").await["settingsVersion"], read["settingsVersion"]);
    let mut avatar = serde_json::to_value(f.cfg("dev").mexeriquinha).unwrap();
    avatar["aparencia"]["cor"] = json!("roxo");
    let saved =
        f.save("dev", &read["settingsVersion"], json!({"name":"  Revisora  ","mexeriquinha":avatar})).await.unwrap();
    assert_eq!(saved["application"], "next_turn");
    assert_eq!(saved["profile"]["name"], "Revisora");
    assert_eq!(saved["profile"]["mexeriquinha"], avatar);
    assert_ne!(saved["settingsVersion"], read["settingsVersion"]);
    let cfg = f.cfg("dev");
    assert_eq!(cfg.description.chars().count(), 4001, "abrir/renomear não trunca instruções antigas");
    assert_eq!(cfg.permission, Permission::Auto);
    assert!(cfg.permission_chosen_by_user);
    assert_eq!(cfg.model.as_deref(), Some("saved-model"));
    let error = f.save("dev", &read["settingsVersion"], json!({"description":"rascunho antigo"})).await.unwrap_err();
    assert!(error.to_string().starts_with("settings_conflict"));
    f.hub.update_bot_by_user(&json!({"id":"dev","description":"Mudança legada"})).unwrap();
    assert!(f.save("dev", &saved["settingsVersion"], json!({"name":"Atrasado"})).await.is_err());
    assert_eq!(f.cfg("dev").name, "Revisora");
    assert_eq!(f.cfg("dev").description, "Mudança legada");
    // Ordem de chaves do avatar não deve fabricar uma versão nova.
    let mut cfg = f.cfg("dev");
    let version = f.read("dev").await["settingsVersion"].clone();
    let mut raw = serde_json::to_value(&cfg).unwrap();
    let mut pairs: Vec<_> = raw["avatarConfig"].as_object().unwrap().clone().into_iter().collect();
    pairs.reverse();
    raw["avatarConfig"] = Value::Object(pairs.into_iter().collect());
    cfg = serde_json::from_value(raw).unwrap();
    f.hub.store.save_bot(&cfg).unwrap();
    assert_eq!(f.read("dev").await["settingsVersion"], version);
    f.hub.shutdown().await;
}

#[tokio::test]
async fn invalid_patch_never_writes_partially_or_forges_human_choice() {
    let f = Fixture::new();
    let mut legacy = f.cfg("dev");
    legacy.name = "n".repeat(41);
    f.hub.store.save_bot(&legacy).unwrap();
    let read = f.read("dev").await;
    let before = serde_json::to_value(f.cfg("dev")).unwrap();
    let rev = f.hub.store.bot("dev").unwrap().unwrap().rev;
    for suffix in ["", "  "] {
        let saved = f
            .save(
                "dev",
                &read["settingsVersion"],
                json!({"name":format!("{}{suffix}", legacy.name),"description":format!("{}{suffix}", legacy.description)}),
            )
            .await
            .unwrap();
        assert_eq!(saved["settingsVersion"], read["settingsVersion"]);
    }
    for field in [
        "id",
        "actor",
        "createdBy",
        "kind",
        "members",
        "backend",
        "command",
        "fastMode",
        "modelOptions",
        "permission",
        "permissionChosenByUser",
        "manageTeam",
        "primaryBotId",
        "deleted",
        "runtime",
        "cwd",
        "connectors",
        "logins",
        "skills",
        "reserveEngine",
    ] {
        let mut patch = json!({"name":"Válido"});
        patch[field] = json!(null);
        assert!(
            f.save("dev", &read["settingsVersion"], patch).await.unwrap_err().to_string().starts_with("invalid_field")
        );
    }
    for patch in [
        json!({"name":"Válido","description":"y".repeat(4001)}),
        json!({"name":"Válido","description":null}),
        json!({"name":"Válido","avatarConfig":{}}),
        json!({"avatarConfig":null}),
        json!({"name":null}),
        json!({"name":42}),
        json!({"name":" "}),
        json!({"name":"x".repeat(41)}),
        json!({"name":" USUÁRIO "}),
        json!([]),
    ] {
        assert!(f.save("dev", &read["settingsVersion"], patch).await.is_err());
    }
    for request in [
        json!({"botId":"dev","patch":{"name":"Sem versão"}}),
        json!({"botId":"dev","expectedVersion":read["settingsVersion"],"patch":{},"actor":"user"}),
    ] {
        assert!(dispatch(&f.hub, &Caller::Local, "donnaBotEditorSave", request).await.is_err());
    }
    assert_eq!(serde_json::to_value(f.cfg("dev")).unwrap(), before);
    assert_eq!(f.hub.store.bot("dev").unwrap().unwrap().rev, rev);
    f.save("dev", &read["settingsVersion"], json!({"name":"Reduzido","description":"Reduzidas"})).await.unwrap();
    assert_eq!(f.cfg("dev").name, "Reduzido");
    assert_eq!(f.cfg("dev").description, "Reduzidas");
    f.hub.store.delete_bot("dev").unwrap();
    assert!(
        f.save("dev", &read["settingsVersion"], json!({"name":"Ressuscitado"}))
            .await
            .unwrap_err()
            .to_string()
            .starts_with("bot_deleted")
    );
}

#[tokio::test]
async fn save_maps_validation_failures_without_leaking_paths_or_writing() {
    for (backend, command, code) in [
        ("unavailable-private-backend", None, "model_unavailable"),
        ("custom", Some("sh /private/adapter-secret.sh"), "command_not_allowed"),
    ] {
        let f = Fixture::new();
        let mut cfg = f.cfg("dev");
        cfg.backend = backend.into();
        cfg.command = command.map(str::to_owned);
        f.hub.store.save_bot(&cfg).unwrap();
        let read = f.read("dev").await;
        let rev = f.hub.store.bot("dev").unwrap().unwrap().rev;
        let error = f.save("dev", &read["settingsVersion"], json!({"name":"Novo"})).await.unwrap_err().to_string();
        assert!(error.starts_with(&format!("{code}:")), "{error}");
        assert!(!error.contains(&cfg.cwd));
        assert!(!error.contains("private"));
        assert_eq!(serde_json::to_value(f.cfg("dev")).unwrap(), serde_json::to_value(cfg).unwrap());
        assert_eq!(f.hub.store.bot("dev").unwrap().unwrap().rev, rev);
    }
}

#[tokio::test]
async fn save_preserves_unavailable_legacy_folder_and_returns_its_warning() {
    let f = Fixture::new();
    let mut cfg = f.cfg("dev");
    cfg.cwd = f.dir.join("private-missing-folder").to_string_lossy().into_owned();
    f.hub.store.save_bot(&cfg).unwrap();
    let read = f.read("dev").await;
    assert!(read["workspaceIssue"].as_str().unwrap().starts_with("Revise a pasta."));
    let mut avatar = serde_json::to_value(&cfg.mexeriquinha).unwrap();
    avatar["aparencia"]["cor"] = json!("roxo");
    let saved = f
        .save(
            "dev",
            &read["settingsVersion"],
            json!({
                "name":"Novo", "description":"Novas instruções", "mexeriquinha":avatar
            }),
        )
        .await
        .unwrap();
    assert_eq!(saved["profile"]["name"], "Novo");
    assert_eq!(saved["profile"]["description"], "Novas instruções");
    assert_eq!(saved["profile"]["mexeriquinha"], avatar);
    assert_eq!(saved["workspaceIssue"], read["workspaceIssue"]);
    assert_eq!(f.cfg("dev").cwd, cfg.cwd);
    assert!(!std::path::Path::new(&cfg.cwd).exists());
}

#[tokio::test]
async fn a_funcao_do_bot_e_editavel_e_passa_pela_mesma_regra() {
    let f = Fixture::new();
    let read = f.read("dev").await;
    assert!(read["capabilities"]["editableFields"].as_array().unwrap().contains(&json!("title")));
    let saved = f.save("dev", &read["settingsVersion"], json!({"title":"  Chefe   de gabinete "})).await.unwrap();
    assert_eq!(saved["profile"]["title"], "Chefe de gabinete");
    assert_eq!(f.cfg("dev").title, "Chefe de gabinete");
    let error = f.save("dev", &saved["settingsVersion"], json!({"title":"Ven\u{202E}das"})).await.unwrap_err();
    assert!(error.to_string().contains("invisível"), "{error}");
}

#[test]
fn save_error_mapping_accepts_both_folder_validators_and_hides_unknown_errors() {
    for message in [
        "workspace folder does not exist: /private/path",
        "A pasta não existe ou não está acessível no Hub.",
        "A pasta raiz não pode ser usada como pasta de trabalho.",
        "A pasta pessoal inteira ou seus ancestrais não podem ser usados. Escolha uma subpasta de trabalho.",
        "O caminho da pasta precisa ser texto UTF-8 válido.",
        "O caminho da pasta deve ter no máximo 1024 bytes e não pode conter caracteres de controle.",
        "O caminho escolhido não é uma pasta.",
        "Escolha um caminho absoluto para a pasta no Hub.",
        "Informe a pasta de trabalho.",
        "Esta pasta contém dados internos do Hub. Escolha uma pasta de trabalho.",
        "Pastas ocultas da pasta pessoal não podem ser usadas como pasta de trabalho.",
        "Diretórios do sistema não podem ser usados como pasta de trabalho.",
        "Pastas pessoais de outros usuários não podem ser usadas como pasta de trabalho.",
        "Não foi possível resolver a pasta do Hub.",
        "Não foi possível resolver a pasta protegida do Hub.",
        "Não foi possível resolver o nome da pasta protegida.",
        "Não foi possível verificar as pastas protegidas do Hub.",
    ] {
        let error = anyhow::anyhow!("/private/path").context(message);
        assert!(errors::save(&error).to_string().starts_with("folder_unavailable:"));
    }
    assert_eq!(
        errors::save(&anyhow::anyhow!("disk error /private/path secret")).to_string(),
        "save_failed: Não foi possível salvar."
    );
}

#[tokio::test]
async fn automatic_naming_rejects_reserved_names_before_writing() {
    let f = Fixture::new();
    let mut cfg = f.cfg("dev");
    cfg.auto_name = true;
    f.hub.store.save_bot(&cfg).unwrap();
    let rev = f.hub.store.bot("dev").unwrap().unwrap().rev;
    for name in [" User ", "USUÁRIO", "Você", "todos", "all", "everyone"] {
        assert!(
            crate::chat::naming::apply_name(&f.hub, "dev", name)
                .await
                .unwrap_err()
                .to_string()
                .starts_with("invalid_field: nome reservado")
        );
    }
    assert_eq!(f.cfg("dev").name, cfg.name);
    assert!(f.cfg("dev").auto_name);
    assert_eq!(f.hub.store.bot("dev").unwrap().unwrap().rev, rev);
}

#[tokio::test]
async fn persisted_save_reports_pending_and_read_retry_reconciles_without_rewriting() {
    let f = Fixture::new(); // Sem ator: update_bot_by_user persiste antes de falhar no Reconfigure.
    let read = f.read("dev").await;
    let saved = f.save("dev", &read["settingsVersion"], json!({"description":"Salvo"})).await.unwrap();
    assert_eq!(saved["application"], "apply_pending");
    assert_eq!(f.cfg("dev").description, "Salvo");
    let rev = f.hub.store.bot("dev").unwrap().unwrap().rev;
    let retry = f.save("dev", &saved["settingsVersion"], json!({"description":"Salvo"})).await.unwrap();
    assert_eq!(retry, saved);
    assert_eq!(f.hub.store.bot("dev").unwrap().unwrap().rev, rev);
    f.start();
    let rev = f.hub.store.bot("dev").unwrap().unwrap().rev;
    let recovered = f.read("dev").await;
    assert_eq!(recovered["application"], "next_turn");
    assert_eq!(recovered["settingsVersion"], saved["settingsVersion"]);
    assert_eq!(f.hub.store.bot("dev").unwrap().unwrap().rev, rev);
    f.hub.shutdown().await;
}

#[tokio::test]
async fn manager_keeps_other_bot_flow_but_never_self_model_autonomy_donna_or_deletion() {
    let f = Fixture::new();
    f.start();
    // D125: o fluxo Auto sem cartão precisa de procedência claramente digitada.
    let mut data = json!({"text":"Ajuste o time"});
    let body = json!({"contentOrigin":{"schema":1,"draftId":uuid::Uuid::new_v4().to_string(),
        "revision":1,"textLength":13,"deviceId":uuid::Uuid::new_v4().to_string(),
        "channel":"desktopComposer","interaction":"send","segments":[{"start":0,"end":13,"kind":"typed"}]}});
    data["contentOrigin"] = crate::guard_hub::origem::capture(&Caller::Local, &body, &data);
    let entry = f.hub.store.insert_entry(&Lane::main("donna"), EntryKind::User, 1, &data).unwrap();
    f.hub.set_runtime("donna", |runtime| {
        runtime.status = BotStatus::Working;
        runtime.message = Some(entry.id);
        runtime.lane = Some(Lane::main("donna"));
    });
    let call = async |tool: &str, args: Value| crate::chat::manage::call(&f.hub, "donna", tool, &args).await;
    // O teste direto da política cobre um futuro gestor que não seja o Berga (D120).
    assert!(authorization::check_management_target("creator", "creator").is_err());
    assert!(authorization::check_management_target("creator", "dev").is_ok());
    assert!(
        call("update_bot", json!({"botId":"donna","name":"Alterada"}))
            .await
            .unwrap_err()
            .to_string()
            .starts_with("not_allowed:")
    );
    for (tool, args) in [("create_bot", json!({"name":"User"})), ("update_bot", json!({"botId":"dev","name":"User"}))] {
        assert!(call(tool, args).await.unwrap_err().to_string().starts_with("invalid_field: nome reservado"));
    }
    for field in ["model", "permission", "manageTeam", "deleted"] {
        let mut args = json!({"botId":"dev","name":"Não pode salvar"});
        args[field] = json!(true);
        assert!(call("update_bot", args).await.is_err());
    }
    assert!(call("delete_bot", json!({"botId":"dev"})).await.is_err());
    crate::chat::manage::call(
        &f.hub,
        "donna",
        "update_bot",
        &json!({"botId":"dev","name":"Revisora","instructions":"Revise o código."}),
    )
    .await
    .unwrap();
    assert_eq!(f.cfg("dev").name, "Revisora");
    assert_eq!(f.cfg("dev").description, "Revise o código.");
    assert!(
        f.hub.store.history("donna", i64::MAX, 10).unwrap().iter().any(|entry| entry.data["teamChange"].is_object())
    );
    let read = f.read("donna").await;
    // D366: nem a intenção humana muda as instruções da Donna.
    let refused = f.save("donna", &read["settingsVersion"], json!({"description":"Intenção humana"})).await;
    assert_eq!(crate::api::error_status(&refused.unwrap_err()).1, "donna_fixed: O Berga é fixo; só a aparência muda.");
    assert_eq!(f.cfg("donna").description, "x".repeat(4001));
    f.hub.shutdown().await;
}

#[tokio::test]
async fn troca_motor_conhecido_exige_retirada_explicita_conta_e_preserva_versao() {
    let f = Fixture::new();
    let mut cfg = f.cfg("dev");
    cfg.command = None;
    cfg.fork_editor.engine_account_id = Some(format!("ac_{}", "a".repeat(64)));
    f.hub.store.save_bot(&cfg).unwrap();
    let read = f.read("dev").await;
    let before = serde_json::to_value(&cfg).unwrap();
    for patch in [
        json!({"backend":"cursor","model":null,"modelOptions":{}}),
        json!({"backend":"motor-arbitrario","forkEditor":{"engineAccountId":null}}),
        json!({"backend":"custom","forkEditor":{"engineAccountId":null}}),
    ] {
        assert!(f.save("dev", &read["settingsVersion"], patch).await.is_err());
        assert_eq!(serde_json::to_value(f.cfg("dev")).unwrap(), before);
    }
    let patch = json!({"backend":"cursor","model":null,"modelOptions":{},"forkEditor":{"engineAccountId":null}});
    let device = Caller::Device { key: "owner".into(), scopes: vec![Scope::Control], autoridade: None };
    assert!(
        dispatch(
            &f.hub,
            &device,
            "donnaBotEditorSave",
            json!({"botId":"dev","expectedVersion":read["settingsVersion"],"patch":patch})
        )
        .await
        .unwrap_err()
        .to_string()
        .starts_with("remote_setup_unavailable")
    );
    let saved = f.save("dev", &read["settingsVersion"], patch).await.unwrap();
    assert_eq!(saved["profile"]["backend"], "cursor");
    assert!(f.cfg("dev").fork_editor.engine_account_id.is_none());
    assert!(f.cfg("dev").model.is_none());
    assert_ne!(saved["settingsVersion"], read["settingsVersion"]);
    assert!(
        f.save("dev", &read["settingsVersion"], json!({"backend":"codex"}))
            .await
            .unwrap_err()
            .to_string()
            .starts_with("settings_conflict")
    );
    // Os motores legados reconhecidos continuam selecionáveis, sem comando ou conta implícitos.
    for backend in ["gemini", "codex", "claude"] {
        let read = f.read("dev").await;
        f.save("dev", &read["settingsVersion"], json!({"backend":backend})).await.unwrap();
        assert_eq!(f.cfg("dev").backend, backend);
    }
}

#[tokio::test]
async fn escolha_legada_igual_a_projecao_remove_mexeriquinha_em_vez_de_ser_retry() {
    let f = Fixture::new();
    let before = f.read("dev").await;
    assert!(before["profile"]["mexeriquinha"].is_object());
    let legacy = before["profile"]["avatarConfig"].clone();
    let saved = f.save("dev", &before["settingsVersion"], json!({"avatarConfig":legacy})).await.unwrap();
    assert_eq!(saved["profile"]["avatarConfig"], legacy);
    assert!(saved["profile"]["mexeriquinha"].is_null());
    assert!(f.cfg("dev").mexeriquinha.is_none());
    assert_ne!(saved["settingsVersion"], before["settingsVersion"]);
}

#[tokio::test]
async fn preferencia_de_navegador_somente_dono_com_validacao_e_sem_ui() {
    let f = Fixture::new();
    for id in ["dev", "donna"] {
        let _commands = f.hub.test_bot_commands(id);
        let read = f.read(id).await;
        assert_eq!(read["profile"]["browserPreference"], "integrated");
        assert!(read["capabilities"]["editableFields"].as_array().unwrap().contains(&json!("browserPreference")));
        for value in [Value::Null, json!("chrome"), json!(true)] {
            let before = f.cfg(id).browser_preference;
            assert!(f.save(id, &read["settingsVersion"], json!({"browserPreference":value})).await.is_err());
            assert_eq!(f.cfg(id).browser_preference, before);
        }
        let saved = f.save(id, &read["settingsVersion"], json!({"browserPreference":"computer"})).await.unwrap();
        assert_eq!(saved["profile"]["browserPreference"], "computer");
        assert!(!f.cfg(id).computer);
        let unchanged = f.save(id, &saved["settingsVersion"], json!({})).await.unwrap();
        assert_eq!(unchanged["profile"]["browserPreference"], "computer");
    }
}

#[tokio::test]
async fn desligado_pelo_editor_e_so_navegador_computador_preservado() {
    let f = Fixture::new();
    for id in ["dev", "donna"] {
        let _commands = f.hub.test_bot_commands(id);
        dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":id,"computer":true})).await.unwrap();
        assert!(f.cfg(id).computer);
        let read = f.read(id).await;
        let saved = f.save(id, &read["settingsVersion"], json!({"browserPreference":"off"})).await.unwrap();
        assert_eq!(saved["profile"]["browserPreference"], "off");
        let cfg = f.cfg(id);
        assert_eq!(cfg.browser_preference, crate::fork::browser::BrowserPreference::Off);
        // Desligado = só navegador (dono, 10/11): "Usar o computador" não muda.
        assert!(cfg.computer, "off não desliga Usar o computador");
        // Patch sem o campo preserva a preferência canônica (fastMode é editável no Berga;
        // description/title são campos travados do bot fixo e não servem de patch neutro aqui).
        let read = f.read(id).await;
        let unchanged = f.save(id, &read["settingsVersion"], json!({"fastMode":true})).await.unwrap();
        assert_eq!(unchanged["profile"]["browserPreference"], "off");
        // Sair de Desligado não mexe no computador.
        let read = f.read(id).await;
        let saved = f.save(id, &read["settingsVersion"], json!({"browserPreference":"computer"})).await.unwrap();
        assert_eq!(saved["profile"]["browserPreference"], "computer");
        assert!(f.cfg(id).computer);
    }
}
