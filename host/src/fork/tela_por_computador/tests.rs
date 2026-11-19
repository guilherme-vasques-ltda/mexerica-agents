//! D70 da Parte 1: autoridade, identidade, limpeza e ausência de herança. Sem prova de isolamento nativo.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::{
    api::dispatch,
    remote::identity::Identity,
    store::{BotConfig, Device, DeviceSource, Scope},
};
use std::sync::Arc;

struct Fixture {
    hub: Arc<Hub>,
    dir: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("d510-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("state.db")).unwrap();
        for id in ["bot", "other"] {
            let cfg: BotConfig = serde_json::from_value(json!({
                "id":id,"name":id,"backend":"fixture","command":"true","cwd":dir,
                "computer":true,"manageTeam":true
            }))
            .unwrap();
            store.save_bot(&cfg).unwrap();
        }
        let hub = Hub::new(store, "d510".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0);
        Self { hub, dir }
    }
    fn body(&self, enabled: bool) -> Value {
        json!({"botId":"bot","target":host_target(&self.hub),"enabled":enabled})
    }
    fn allowed(&self, bot: &str, target: &str) -> bool {
        allowed_on(&self.hub.store.db.locked(), bot, target, None).is_ok()
    }
    fn audit(&self) -> i64 {
        self.hub
            .store
            .db
            .locked()
            .query_row("SELECT COUNT(*) FROM donna_screen_access_audit", [], |r| r.get(0))
            .unwrap()
    }
    fn device(&self, key: &str, scopes: Vec<Scope>) -> Caller {
        self.hub
            .store
            .put_device(&Device {
                key: key.into(),
                name: "Teste".into(),
                platform: "ios".into(),
                source: DeviceSource::Local,
                grant_id: None,
                scopes,
                lease_until: None,
                created_at: 1,
                last_seen_at: None,
            })
            .unwrap();
        crate::fork::autoridade::autenticar(&self.hub, key).unwrap().1
    }
    fn enroll(&self, key: &str) -> String {
        let code = crate::computers::pairing::issue(&self.hub)["code"].clone();
        crate::computers::pairing::redeem(
            &self.hub,
            key,
            &json!({"name":"Mac","platform":"macos","code":code}),
            "fixture",
        )
        .unwrap()["computerId"]
            .as_str()
            .unwrap()
            .into()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn d510_sem_migracao_par_exato_auditoria_e_bot_excluido() {
    let f = Fixture::new();
    let target = host_target(&f.hub);
    assert!(!f.allowed("bot", &target));
    assert_eq!(f.audit(), 0);
    let snapshot = dispatch(&f.hub, &Caller::Local, METHOD, f.body(true)).await.unwrap();
    assert_eq!(snapshot["screenAccess"][0]["enabled"], true);
    assert!(f.allowed("bot", &target));
    assert!(!f.allowed("other", &target));
    assert!(!f.allowed("bot", "host:outro"));
    assert_eq!(f.audit(), 1);
    let mut invalid = f.body(true);
    for bad in ["nome", "host:outro", "inexistente"] {
        invalid["target"] = json!(bad);
        assert!(dispatch(&f.hub, &Caller::Local, METHOD, invalid.clone()).await.is_err());
    }
    f.hub.store.delete_bot("bot").unwrap();
    assert!(!f.allowed("bot", &target));
    assert_eq!(f.audit(), 2);
    let count: i64 =
        f.hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_screen_access", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn d510_control_original_revogado_entre_captura_commit_nao_grava() {
    let f = Fixture::new();
    let caller = f.device("phone", vec![Scope::Control]);
    // O mesmo método admite um aparelho Control legítimo, sem engine.accounts.
    dispatch(&f.hub, &caller, METHOD, f.body(true)).await.unwrap();
    dispatch(&f.hub, &caller, METHOD, f.body(false)).await.unwrap();
    let before = f.audit();
    let h = f.hub.clone();
    *f.hub.store.before_owner_write.locked() = Some(Box::new(move || {
        h.store.remove_device("phone").unwrap();
    }));
    assert!(dispatch(&f.hub, &caller, METHOD, f.body(true)).await.is_err());
    assert!(!f.allowed("bot", &host_target(&f.hub)));
    assert_eq!(f.audit(), before);
}

#[tokio::test]
async fn d510_reparear_mesmo_id_remove_device_e_computer_remove_limpam() {
    let f = Fixture::new();
    let key = crate::remote::crypto::b64(&[17; 32]);
    let id = f.enroll(&key);
    let mut body = f.body(true);
    body["target"] = json!(id);
    // Simula linha legada: a API atual não concede tela D83.
    f.hub.store.db.locked().execute("INSERT INTO donna_screen_access VALUES('bot',?1)", [&id]).unwrap();
    assert!(f.allowed("bot", &id));
    let rows = projection(&f.hub, "bot").unwrap();
    assert_eq!(rows[1]["available"], false);
    assert_eq!(rows[1]["notice"], "tela ainda indisponível neste computador");
    f.hub.store.remove_device(&key).unwrap();
    assert!(!f.allowed("bot", &id));
    assert!(dispatch(&f.hub, &Caller::Local, METHOD, body.clone()).await.is_err());
    assert_eq!(f.enroll(&key), id);
    assert!(!f.allowed("bot", &id));
    // Mesmo uma linha antiga residual é apagada na Tx de novo enrollment do MESMO ID.
    f.hub.store.remove_device(&key).unwrap();
    f.hub.store.db.locked().execute("INSERT INTO donna_screen_access VALUES('bot',?1)", [&id]).unwrap();
    assert_eq!(f.enroll(&key), id);
    assert!(!f.allowed("bot", &id));
    // Simula linha legada: a API atual não concede tela D83.
    f.hub.store.db.locked().execute("INSERT INTO donna_screen_access VALUES('bot',?1)", [&id]).unwrap();
    dispatch(&f.hub, &Caller::Local, "computerRemove", json!({"computerId":id})).await.unwrap();
    assert!(!f.allowed("bot", &id));
    assert!(dispatch(&f.hub, &Caller::Local, METHOD, body).await.is_err());
}

#[tokio::test]
async fn d510_bot_ponte_rotina_manage_team_e_aparelho_sem_control_nao_ligam() {
    let f = Fixture::new();
    assert!(!crate::fork::ponte_credencial::PERMITIDOS.contains(&METHOD));
    for caller in [
        f.device("screen", vec![Scope::Screen]),
        Caller::Device { key: "missing".into(), scopes: vec![Scope::Control], autoridade: None },
        Caller::Pairing { key: "pairing".into() },
    ] {
        assert!(dispatch(&f.hub, &caller, METHOD, f.body(true)).await.is_err());
    }
    // Nem perfil completo de bot nem manage_team escrevem a tabela da permissão.
    let _commands = f.hub.test_bot_commands("bot");
    let target = host_target(&f.hub);
    f.hub.update_bot(&json!({"id":"bot","screenAccess":[{"target":target,"enabled":true}]})).unwrap();
    assert!(
        crate::chat::manage::call(
            &f.hub,
            "bot",
            "update_bot",
            &json!({"botId":"other","screenAccess":[{"target":target,"enabled":true}]}),
        )
        .await
        .is_err()
    );
    assert!(!f.allowed("bot", &target));
    assert!(!f.allowed("other", &target));
    assert_eq!(f.audit(), 0);
}

#[tokio::test]
async fn d510_d83_nao_grava_liberacao_invisivel_e_desligar_limpa_legado() {
    let f = Fixture::new();
    let key = crate::remote::crypto::b64(&[19; 32]);
    let id = f.enroll(&key);
    let mut body = f.body(true);
    body["target"] = json!(id);
    let error = dispatch(&f.hub, &Caller::Local, METHOD, body.clone()).await.unwrap_err();
    assert_eq!(error.to_string(), "tela ainda indisponível neste computador");
    assert!(!f.allowed("bot", &id));
    assert_eq!(f.audit(), 0, "recusa não grava auditoria de ligar");
    let count: i64 =
        f.hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_screen_access", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 0);
    body["enabled"] = json!(false);
    dispatch(&f.hub, &Caller::Local, METHOD, body.clone()).await.unwrap();
    assert_eq!(f.audit(), 0, "desligar sem linha é no-op");
    f.hub.store.db.locked().execute("INSERT INTO donna_screen_access VALUES('bot',?1)", [&id]).unwrap();
    dispatch(&f.hub, &Caller::Local, METHOD, body).await.unwrap();
    assert!(!f.allowed("bot", &id));
    assert_eq!(f.audit(), 2, "linha legada foi removida com auditoria de desligar");
}

#[tokio::test]
async fn d510_sec01_tela_ativa_recusa_ligar_pelo_local_e_device_control() {
    for active in ["bot", "other"] {
        for local in [true, false] {
            let f = Fixture::new();
            let caller = if local { Caller::Local } else { f.device("phone", vec![Scope::Control]) };
            let generation = crate::fork::spaces::config_generation_on(&f.hub.store.db.locked(), "bot").unwrap();
            f.hub.screen.agent_for_test(Some(active));
            let error = dispatch(&f.hub, &caller, METHOD, f.body(true)).await.unwrap_err();
            assert_eq!(error.to_string(), "Pare o uso da tela pelos bots antes de ligar esta opção.");
            assert!(!f.allowed("bot", &host_target(&f.hub)));
            assert_eq!(f.audit(), 0);
            assert_eq!(crate::fork::spaces::config_generation_on(&f.hub.store.db.locked(), "bot").unwrap(), generation);
        }
    }
}

#[tokio::test]
async fn d510_sec01_tela_ativa_entre_captura_e_commit_nao_grava() {
    let f = Fixture::new();
    let h = f.hub.clone();
    *f.hub.store.before_owner_write.locked() = Some(Box::new(move || h.screen.agent_for_test(Some("other"))));
    assert!(dispatch(&f.hub, &Caller::Local, METHOD, f.body(true)).await.is_err());
    assert!(!f.allowed("bot", &host_target(&f.hub)));
    assert_eq!(f.audit(), 0);
}

#[tokio::test]
async fn d510_sec01_desligar_com_tela_ativa_e_ligar_depois() {
    let f = Fixture::new();
    let caller = f.device("phone", vec![Scope::Control]);
    dispatch(&f.hub, &Caller::Local, METHOD, f.body(true)).await.unwrap();
    f.hub.screen.agent_for_test(Some("other"));
    dispatch(&f.hub, &Caller::Local, METHOD, f.body(false)).await.unwrap();
    dispatch(&f.hub, &caller, METHOD, f.body(false)).await.unwrap();
    assert!(!f.allowed("bot", &host_target(&f.hub)));
    assert_eq!(f.audit(), 2);
    f.hub.screen.agent_for_test(None);
    dispatch(&f.hub, &caller, METHOD, f.body(true)).await.unwrap();
    assert!(f.allowed("bot", &host_target(&f.hub)));
    assert_eq!(f.audit(), 3);
}
