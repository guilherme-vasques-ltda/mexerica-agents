//! D70 causal do cartão: API real de resposta e predicado real, sem driver nativo.
#![allow(clippy::unwrap_used)]
use super::super::fork_observation::Attempt;
use super::*;
use crate::{
    api::{devices::Caller, dispatch},
    remote::{cloud, identity::Identity},
    store::{BotConfig, Device, DeviceSource, Scope},
};
struct Fixture {
    hub: Arc<Hub>,
    dir: std::path::PathBuf,
    execution: ExecutionRef,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("d511-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("state.db")).unwrap();
        for id in ["bot", "other"] {
            let cfg: BotConfig =
                serde_json::from_value(json!({"id":id,"name":id,"computer":true,"backend":"fixture"})).unwrap();
            store.save_bot(&cfg).unwrap();
        }
        let hub = Hub::new(store, "d511".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0);
        *hub.screen.enabled.locked() = Some(true);
        let execution = bindings::fixture_execution(&hub, "bot");
        Self { hub, dir, execution }
    }
    fn original(&self) -> Attempt {
        Attempt::capture_original(&self.hub, &Caller::Local, "bot", &self.execution, true).unwrap()
    }
    fn ask(&self) -> (Entry, Waiting) {
        request(&self.hub, &self.original(), Some(&format!("MOTIVO_EXTERNO\n\r\t\u{202e}{}", "x".repeat(5000))))
            .unwrap()
    }
    async fn answer(&self, card: &Entry, option: &str, caller: &Caller) -> Result<Value> {
        dispatch(
            &self.hub,
            caller,
            "respondPermission",
            json!({"botId":card.bot_id,"entryId":card.id,"entryRev":card.rev,"optionId":option}),
        )
        .await
    }
    fn audit(&self) -> i64 {
        self.hub
            .store
            .db
            .locked()
            .query_row("SELECT COUNT(*) FROM donna_screen_access_audit", [], |r| r.get(0))
            .unwrap()
    }
    fn permanent(&self) -> bool {
        access::allowed_on(&self.hub.store.db.locked(), "bot", &access::host_target(&self.hub), None).is_ok()
    }
    fn device(&self, key: &str, source: DeviceSource) -> Caller {
        if source == DeviceSource::Account {
            cloud::update_status(&self.hub, |s| s.owner = Some(cloud::Owner { user_id: "owner".into(), email: None }));
            self.hub.cloud_synced.store(true, std::sync::atomic::Ordering::Release);
        }
        self.hub
            .store
            .put_device(&Device {
                key: key.into(),
                name: "Teste".into(),
                platform: "fixture".into(),
                source,
                grant_id: (source == DeviceSource::Account).then(|| "grant".into()),
                scopes: vec![Scope::Control],
                lease_until: (source == DeviceSource::Account).then(|| now_ms() + 60_000),
                created_at: now_ms(),
                last_seen_at: None,
            })
            .unwrap();
        crate::fork::autoridade::autenticar(&self.hub, key).unwrap().1
    }
    fn admitted(&self) -> Arc<Attempt> {
        Attempt::capture(&self.hub, &Caller::Local, "bot", &self.execution, true).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn d502_agora_original_valido_hold_antes_claim_sem_renovar_grant() {
    use crate::fork::atividades::revisao::estado::fixture;
    let f = Fixture::new();
    let (card, _waiting) = f.ask();
    let device = f.device("grant-owner", DeviceSource::Account);
    f.answer(&card, "now", &device).await.unwrap();
    let attempt = f.admitted();
    let original_grant = attempt.grant.clone();
    assert!(original_grant.is_some());
    assert!(attempt.verify().is_ok());
    assert!(attempt.check_waiting().is_ok());
    fixture::hold(&f.hub, "original");
    assert!(attempt.claim().is_err());
    assert!(f.hub.screen.control.locked().active_agent().is_none());
    assert_eq!(attempt.grant, original_grant);
    assert!(fixture::abort(&f.hub, "original"));
    assert!(attempt.verify().is_err(), "a mesma tentativa não retoma");
    assert_eq!(attempt.grant, original_grant);
}

#[tokio::test(start_paused = true)]
async fn pedido_de_tela_com_falha_na_expiracao_e_recusado_sem_espera() {
    let f = Fixture::new();
    let original = f.original();
    let card = f
        .hub
        .store
        .insert_entry(
            &crate::store::Lane::main("bot"),
            crate::store::EntryKind::Permission,
            1,
            &json!({"status":"pending","author":"bot","toolKind":"screen_access",
                "screenAccess":{"target":access::host_target(&f.hub),"generation":original.generation,
                    "expiresAt":now_ms()+1_200_000}}),
        )
        .unwrap();
    f.hub
        .store
        .db
        .locked()
        .execute_batch(
            "CREATE TRIGGER reject_screen_expiration BEFORE UPDATE OF data ON entries
             WHEN json_extract(OLD.data,'$.toolKind') = 'screen_access'
              AND json_extract(NEW.data,'$.status') = 'expired'
             BEGIN SELECT RAISE(ABORT, 'falha simulada'); END",
        )
        .unwrap();
    f.hub.store.expire_pending().unwrap();
    assert!(f.hub.store.expiration_failed(&card.id));
    assert_eq!(f.hub.store.entry(&card.id).unwrap().data["status"], "pending");

    let error = request(&f.hub, &original, None).err().expect("pedido não pode reaproveitar o cartão bloqueado");
    assert_eq!(error.to_string(), "O pedido de tela anterior não pôde ser encerrado.");
    let error = tokio::time::timeout(Duration::from_secs(1), authorize(original, None))
        .await
        .expect("autorização não pode aguardar resposta ao cartão bloqueado")
        .err()
        .expect("autorização não pode liberar acesso");
    assert_eq!(error.to_string(), "O pedido de tela anterior não pôde ser encerrado.");
    assert!(f.hub.screen.control.locked().permissions.pending.is_empty());
    assert!(!f.permanent());
    assert_eq!(f.audit(), 0);
}

#[tokio::test(start_paused = true)]
async fn d511_cartao_fronteiras_e_tempos() {
    // O pedido reaproveita o mesmo cartão, sem claim/captura, com textos fixos e três kinds ACP.
    {
        let f = Fixture::new();
        let mut events = f.hub.events.subscribe();
        let (card, _wait) = f.ask();
        assert!(card.data["screenAccess"].get("reason").is_none());
        assert!(f.hub.store.entry(&card.id).unwrap().data["screenAccess"].get("reason").is_none());
        while let Ok(event) = events.try_recv() {
            assert!(!event.to_string().contains("MOTIVO_EXTERNO"));
            if event["entry"]["kind"] == "permission" {
                assert!(event["entry"]["data"]["screenAccess"].get("reason").is_none());
            }
        }
        for tool in super::super::computer::computer_tools(&f.hub).await["tools"].as_array().unwrap() {
            assert!(tool["inputSchema"]["properties"].get("screenReason").is_none());
            let description = tool["description"].as_str().unwrap();
            assert!(!description.contains("terminal pode responder como dono"));
            assert!(!description.contains("Caller::Local"));
        }
        let (same, _wait2) = f.ask();
        assert_eq!(card.id, same.id);
        assert_eq!(card.rev, same.rev);
        assert!(!f.permanent());
        assert_eq!(f.audit(), 0);
        assert!(f.hub.screen.control.locked().active_agent().is_none());
        assert_eq!(card.data["voiceApproval"], "app_only");
        assert_eq!(card.data["title"], "Pode ver a tela deste computador?");
        assert!(
            (599_950..=600_000).contains(&(card.data["screenAccess"]["expiresAt"].as_i64().unwrap() - card.created_at))
        );
        for option in ["always", "now", "deny"] {
            let body =
                json!({"botId":card.bot_id,"entryId":card.id,"entryRev":card.rev,"optionId":option,"source":"voice"});
            assert!(dispatch(&f.hub, &Caller::Local, "respondPermission", body).await.is_err());
            let after = f.hub.store.entry(&card.id).unwrap();
            assert_eq!(after.rev, card.rev);
            assert_eq!(after.data, card.data);
        }
        assert_eq!(f.audit(), 0);
        // Uma revisão realmente trocada entre captura e Tx não concede Sempre (R1-01).
        let h = f.hub.clone();
        let id = card.id.clone();
        *f.hub.store.before_owner_write.locked() = Some(Box::new(move || {
            let e = h.store.entry(&id).unwrap();
            h.store.update_entry(&id, &e.data).unwrap();
        }));
        let local = f.device("local", DeviceSource::Account);
        assert!(f.answer(&card, "always", &local).await.is_err());
        assert!(!f.permanent());
        assert_eq!(f.audit(), 0);
    }
    // Sempre: cartão, par e auditoria confirmam juntos; retransmitir não duplica.
    {
        let f = Fixture::new();
        let (card, _wait) = f.ask();
        f.answer(&card, "always", &Caller::Local).await.unwrap();
        assert!(f.permanent());
        let e = f.hub.store.entry(&card.id).unwrap();
        assert_eq!(e.data["status"], "answered");
        assert_eq!(e.data["selected"], "always");
        assert_eq!(f.audit(), 2);
        f.answer(&card, "always", &Caller::Local).await.unwrap();
        assert_eq!(f.audit(), 2);
    }
    // A origem original revogada antes da Tx deixa cartão, par e auditoria intactos.
    {
        let f = Fixture::new();
        let (card, _wait) = f.ask();
        let caller = f.device("phone", DeviceSource::Account);
        let h = f.hub.clone();
        *f.hub.store.before_owner_write.locked() = Some(Box::new(move || {
            h.store.remove_device("phone").unwrap();
        }));
        assert!(f.answer(&card, "always", &caller).await.is_err());
        assert!(!f.permanent());
        assert_eq!(f.audit(), 0);
        assert_eq!(f.hub.store.entry(&card.id).unwrap().rev, card.rev);
    }
    // Com próprio/outro bot ativo, Local e QR não concedem. Negar sempre permanece disponível.
    for active in ["bot", "other"] {
        let f = Fixture::new();
        let (card, _wait) = f.ask();
        let local = f.device("desktop", DeviceSource::Local);
        f.hub.screen.agent_for_test(Some(active));
        for caller in [&Caller::Local, &local] {
            for option in ["always", "now"] {
                let error = f.answer(&card, option, caller).await.unwrap_err();
                let expected = if matches!(caller, Caller::Local) && option == "always" {
                    "Pare o uso da tela"
                } else {
                    ACCOUNT_APP
                };
                assert!(error.to_string().contains(expected));
                let after = f.hub.store.entry(&card.id).unwrap();
                assert_eq!(after.rev, card.rev);
                assert_eq!(after.data, card.data);
                assert!(!f.permanent());
                assert_eq!(f.audit(), 0);
            }
        }
        let phone = f.device("phone", DeviceSource::Account);
        f.answer(&card, "now", &phone).await.unwrap();
        assert!(!f.permanent());
        assert_eq!(f.audit(), 1);
    }
    // QR nunca concede, nem sem tela ativa; Local preserva o caminho do Mac via SSH (opção B do dono até D330-F5).
    for active in [None, Some("bot"), Some("other")] {
        let f = Fixture::new();
        let (card, _wait) = f.ask();
        let qr = f.device("qr", DeviceSource::Local);
        f.hub.screen.agent_for_test(active);
        for option in ["always", "now"] {
            assert!(
                f.answer(&card, option, &qr)
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("abra o app conectado à sua conta")
            );
            assert_eq!(f.hub.store.entry(&card.id).unwrap().rev, card.rev);
            assert_eq!(f.hub.store.entry(&card.id).unwrap().data, card.data);
            assert_eq!(f.audit(), 0);
            assert!(!f.permanent());
        }
        f.answer(&card, "deny", &qr).await.unwrap();
        assert_eq!(f.audit(), 1);
        assert_eq!(f.hub.store.entry(&card.id).unwrap().data["selected"], "deny");
    }
    for active in [None, Some("bot"), Some("other")] {
        for account in [false, true] {
            let f = Fixture::new();
            let (card, _wait) = f.ask();
            let caller = if account { f.device("account", DeviceSource::Account) } else { Caller::Local };
            f.hub.screen.agent_for_test(active);
            f.answer(&card, "deny", &caller).await.unwrap();
            assert_eq!(f.audit(), 1);
            assert!(!f.permanent());
            assert_eq!(f.hub.store.entry(&card.id).unwrap().data["selected"], "deny");
        }
    }
    // Agora não persiste; cada gatilho retira a mesma tentativa e não permite renovar o claim antigo.
    for change in [
        "idle20",
        "max15",
        "idle20wall",
        "max15wall",
        "turn",
        "lane",
        "binding",
        "bot",
        "device",
        "other",
        "off",
        "restart",
    ] {
        let f = Fixture::new();
        let (card, _wait) = f.ask();
        let device = f.device("grant-owner", DeviceSource::Account);
        f.answer(&card, "now", &device).await.unwrap();
        assert!(!f.permanent());
        assert_eq!(access::projection(&f.hub, "bot").unwrap()[0]["canEndTemporary"], true);
        let attempt = f.admitted();
        assert!(attempt.claim().is_ok());
        match change {
            "idle20" => {
                f.hub.screen.control.locked().permissions.grants.get_mut("bot").unwrap().last_use =
                    Instant::now().checked_sub(Duration::from_secs(21)).unwrap();
            }
            "max15" => {
                f.hub.screen.control.locked().permissions.grants.get_mut("bot").unwrap().started =
                    Instant::now().checked_sub(Duration::from_secs(901)).unwrap();
            }
            "idle20wall" => {
                f.hub.screen.control.locked().permissions.grants.get_mut("bot").unwrap().last_use_at =
                    now_ms() - 20_001;
            }
            "max15wall" => {
                f.hub.screen.control.locked().permissions.grants.get_mut("bot").unwrap().started_at =
                    now_ms() - 900_001;
            }
            "turn" => f.hub.set_runtime("bot", |r| r.status = BotStatus::Idle),
            "lane" => f.hub.set_runtime("bot", |r| r.lane = Some(crate::store::Lane::main("other"))),
            "binding" => bindings::revoke(&f.hub.store, "bot", None).unwrap(),
            "bot" => {
                f.hub.store.db.locked().execute("UPDATE bots SET deleted=1 WHERE id='bot'", []).unwrap();
            }
            "device" => {
                f.hub.store.remove_device("grant-owner").unwrap();
            }
            "other" => {
                let mut c = f.hub.screen.control.locked();
                c.permissions.claimed("other");
                c.agent = Some(("other".into(), Instant::now()));
            }
            "off" => {
                access::call(
                    &f.hub,
                    &Caller::Local,
                    &json!({"botId":"bot","target":access::host_target(&f.hub),"enabled":false}),
                )
                .unwrap();
            }
            _ => f.hub.screen.control.locked().permissions = State::default(),
        }
        assert!(
            access::projection(&f.hub, "bot").unwrap()[0].get("canEndTemporary").is_none(),
            "sem botão obsoleto: {change}"
        );
        assert!(attempt.verify().is_err(), "{change}");
        assert!(attempt.claim().is_err(), "não revive: {change}");
        assert!(!f.permanent());
    }
    // Recusa impede novo pedido por uma hora E no mesmo turno, mesmo após essa hora.
    {
        let f = Fixture::new();
        let (card, _wait) = f.ask();
        f.answer(&card, "deny", &Caller::Local).await.unwrap();
        assert!(request(&f.hub, &f.original(), None).is_err());
        f.hub.store.db.locked().execute("UPDATE donna_screen_access_audit SET at=?1", [now_ms() - 3_600_001]).unwrap();
        assert!(request(&f.hub, &f.original(), None).is_err());
        access::call(
            &f.hub,
            &Caller::Local,
            &json!({"botId":"bot","target":access::host_target(&f.hub),"enabled":true}),
        )
        .unwrap();
        assert!(f.permanent());
    }
    // 270 s devolve aguardando sem claim; Agora tardio audita sem efeito e não cria par.
    {
        let f = Fixture::new();
        let mut events = f.hub.events.subscribe();
        let original = f.original();
        let task = tokio::spawn(async move { authorize(original, None).await });
        let card = loop {
            let event = events.recv().await.unwrap();
            if event["entry"]["kind"] == "permission" {
                break f.hub.store.entry(event["entry"]["id"].as_str().unwrap()).unwrap();
            }
        };
        assert!(f.hub.screen.control.locked().active_agent().is_none());
        tokio::time::advance(WAIT + Duration::from_millis(1)).await;
        assert!(task.await.unwrap().err().unwrap().to_string().contains("aguardando o dono"));
        f.answer(&card, "now", &Caller::Local).await.unwrap();
        assert!(!f.permanent());
        assert!(f.hub.screen.control.locked().permissions.grants.is_empty());
        let after = f.hub.store.entry(&card.id).unwrap();
        assert_eq!(after.data["screenAccess"]["effect"], false);
        assert_eq!(f.audit(), 1);
    }
    // Fim do turno encerra a espera, mas Sempre tardio pode somente ligar a chave futura.
    {
        let f = Fixture::new();
        let (card, wait) = f.ask();
        drop(wait);
        f.hub.set_runtime("bot", |r| r.status = BotStatus::Idle);
        f.answer(&card, "always", &Caller::Local).await.unwrap();
        assert!(f.permanent());
        assert!(f.hub.screen.control.locked().active_agent().is_none());
    }
    // Prazo fixo de 10 min: expiração real do cartão não cria chave nem resposta de concessão.
    {
        let f = Fixture::new();
        let (mut card, _wait) = f.ask();
        card.data["screenAccess"]["expiresAt"] = json!(now_ms() - 1);
        f.hub.store.update_entry(&card.id, &card.data).unwrap();
        expire(&f.hub, &card.id).unwrap();
        assert_eq!(f.hub.store.entry(&card.id).unwrap().data["status"], "expired");
        assert!(!f.permanent());
        assert_eq!(f.audit(), 0);
    }
}

#[tokio::test(start_paused = true)]
async fn d511_retomada_sucessor_e_recusa_no_novo_turno() {
    // O commit de cada escolha libera só a chamada que conservou a origem, sem claim antecipado.
    for option in ["always", "now"] {
        let f = Fixture::new();
        let mut events = f.hub.events.subscribe();
        let original = f.original();
        let task = tokio::spawn(async move { authorize(original, None).await });
        let card = loop {
            let e = events.recv().await.unwrap();
            if e["entry"]["kind"] == "permission" {
                break f.hub.store.entry(e["entry"]["id"].as_str().unwrap()).unwrap();
            }
        };
        f.answer(&card, option, &Caller::Local).await.unwrap();
        let attempt = task.await.unwrap().unwrap();
        assert!(attempt.verify().is_ok());
        assert!(f.hub.screen.control.locked().active_agent().is_none());
        assert!(attempt.claim().is_ok());
        assert_eq!(f.permanent(), option == "always");
    }
    // Outra concessão de Agora na mesma execução não revive o writer com recibo antigo.
    {
        let f = Fixture::new();
        let (card, _wait) = f.ask();
        f.answer(&card, "now", &Caller::Local).await.unwrap();
        let old = f.admitted();
        f.hub.screen.control.locked().permissions.grants.get_mut("bot").unwrap().last_use =
            Instant::now().checked_sub(Duration::from_secs(21)).unwrap();
        assert!(old.verify().is_err());
        let (next, _wait) = f.ask();
        assert_ne!(next.id, card.id);
        f.answer(&next, "now", &Caller::Local).await.unwrap();
        assert!(f.admitted().verify().is_ok());
        assert!(old.verify().is_err());
    }
    // Mesmo depois de mudar de execução, a hora de recusa vale; após a hora, nova execução pode pedir.
    {
        let f = Fixture::new();
        let (card, _wait) = f.ask();
        f.answer(&card, "deny", &Caller::Local).await.unwrap();
        f.hub.set_runtime("bot", |r| r.memory_turn_token = Some("next-turn".into()));
        let next = bindings::capture(&f.hub, "bot", f.execution.acp_binding().unwrap()).unwrap();
        let original = Attempt::capture_original(&f.hub, &Caller::Local, "bot", &next, true).unwrap();
        assert!(request(&f.hub, &original, None).is_err());
        f.hub.store.db.locked().execute("UPDATE donna_screen_access_audit SET at=?1", [now_ms() - 3_600_001]).unwrap();
        let (next, _wait) = request(&f.hub, &original, None).unwrap();
        assert_ne!(next.id, card.id);
    }
    // O prazo monotônico não se amplia se o relógio civil voltar.
    {
        let f = Fixture::new();
        let (card, _wait) = f.ask();
        f.hub.screen.control.locked().permissions.pending.get_mut(&card.id).unwrap().deadline =
            Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
        assert!(f.answer(&card, "always", &Caller::Local).await.is_err());
        assert_eq!(f.audit(), 0);
        assert_eq!(f.hub.store.entry(&card.id).unwrap().rev, card.rev);
        expire(&f.hub, &card.id).unwrap();
        assert_eq!(f.hub.store.entry(&card.id).unwrap().data["status"], "expired");
        assert!(!f.permanent());
    }
    // Reinício conserva somente o prazo do cartão: Agora tardio não restaura RAM; Sempre só liga a chave.
    for option in ["now", "always"] {
        let f = Fixture::new();
        let (card, _wait) = f.ask();
        let fresh = Hub::new(
            Store::open(&f.dir.join("state.db")).unwrap(),
            "d511".into(),
            Identity::load_or_create(&f.dir).unwrap(),
            "test".into(),
            0,
        );
        fresh.store.expire_pending().unwrap();
        restore_pending(&fresh).unwrap();
        assert_eq!(fresh.store.entry(&card.id).unwrap().data["status"], "pending");
        assert!(fresh.screen.control.locked().permissions.pending.is_empty());
        let execution = bindings::fixture_execution(&fresh, "bot");
        let original = Attempt::capture_original(&fresh, &Caller::Local, "bot", &execution, true).unwrap();
        let (same, _new_wait) = request(&fresh, &original, None).unwrap();
        assert_eq!(same.id, card.id);
        assert!(
            fresh.screen.control.locked().permissions.pending.is_empty(),
            "cartão restaurado não fabrica contexto de Agora"
        );
        dispatch(
            &fresh,
            &Caller::Local,
            "respondPermission",
            json!({"botId":card.bot_id,"entryId":card.id,"entryRev":card.rev,"optionId":option}),
        )
        .await
        .unwrap();
        assert!(fresh.screen.control.locked().permissions.grants.is_empty());
        let allowed = access::allowed_on(&fresh.store.db.locked(), "bot", &access::host_target(&fresh), None).is_ok();
        assert_eq!(allowed, option == "always");
        assert_eq!(fresh.store.entry(&card.id).unwrap().data["screenAccess"]["effect"], option == "always");
    }
    // Prazo da ferramenta é conferido no commit, mesmo se o future ainda não observou o timer.
    {
        let f = Fixture::new();
        let (card, _wait) = f.ask();
        for until in f.hub.screen.control.locked().permissions.pending.get_mut(&card.id).unwrap().waiters.values_mut() {
            *until = tokio::time::Instant::now() - Duration::from_secs(1);
        }
        f.answer(&card, "now", &Caller::Local).await.unwrap();
        assert_eq!(f.hub.store.entry(&card.id).unwrap().data["screenAccess"]["effect"], false);
        assert!(f.hub.screen.control.locked().permissions.grants.is_empty());
    }
    // Sempre respondido ao cartão de outro turno vale para a próxima chamada, sem retomar esta espera.
    {
        let f = Fixture::new();
        let (card, wait) = f.ask();
        drop(wait);
        f.hub.set_runtime("bot", |r| r.memory_turn_token = Some("successor".into()));
        let next = bindings::capture(&f.hub, "bot", f.execution.acp_binding().unwrap()).unwrap();
        let original = Attempt::capture_original(&f.hub, &Caller::Local, "bot", &next, true).unwrap();
        let future = authorize(original, None);
        tokio::pin!(future);
        assert!(futures::poll!(&mut future).is_pending());
        f.answer(&card, "always", &Caller::Local).await.unwrap();
        assert!(future.await.err().unwrap().to_string().contains("próximas chamadas"));
        assert!(f.permanent());
        assert!(f.hub.screen.control.locked().active_agent().is_none());
        assert!(Attempt::capture(&f.hub, &Caller::Local, "bot", &next, true).is_ok());
    }

    // Remoção do membro depois da leitura da resposta e antes da Tx não concede Sempre.
    {
        let f = Fixture::new();
        let room: BotConfig =
            serde_json::from_value(json!({"id":"room","name":"Sala","kind":"group","members":["bot"]})).unwrap();
        f.hub.store.save_bot(&room).unwrap();
        crate::fork::sessoes::ensure_principal(&f.hub.store.db.locked(), "room").unwrap();
        let binding = bindings::issue(&f.hub.store, "bot", "room", "room", Some("room-acp")).unwrap();
        bindings::activate(&f.hub.store, &binding, "room-acp").unwrap();
        f.hub.set_runtime("bot", |r| {
            r.lane = Some(crate::store::Lane::main("room"));
            r.session = Some("room-acp".into());
            r.memory_turn_token = Some("room-turn".into());
        });
        let execution = bindings::capture(&f.hub, "bot", binding.id.as_str()).unwrap();
        let original = Attempt::capture_original(&f.hub, &Caller::Local, "bot", &execution, true).unwrap();
        let (card, _wait) = request(&f.hub, &original, None).unwrap();
        assert_eq!(card.bot_id, "room");
        let owner = f.device("room-owner", DeviceSource::Account);
        let h = f.hub.clone();
        *f.hub.store.before_owner_write.locked() = Some(Box::new(move || {
            h.store
                .db
                .locked()
                .execute("UPDATE bots SET config=json_set(config,'$.members',json('[]')) WHERE id='room'", [])
                .unwrap();
        }));
        assert!(f.answer(&card, "always", &owner).await.is_err());
        assert!(!f.permanent());
        assert_eq!(f.audit(), 0);
        assert_eq!(f.hub.store.entry(&card.id).unwrap().rev, card.rev);
    }
}

#[tokio::test(start_paused = true)]
async fn d511_sec01_sempre_com_tela_ativa_nao_grava() {
    // D330-F5: Caller::Local só nega é a troca FUTURA. A opção B fixa o Mac via SSH hoje.
    for option in ["always", "now", "deny"] {
        let f = Fixture::new();
        let (card, _wait) = f.ask();
        f.answer(&card, option, &Caller::Local).await.unwrap();
        assert_eq!(f.hub.store.entry(&card.id).unwrap().data["selected"], option);
        assert_eq!(f.permanent(), option == "always");
        assert_eq!(f.hub.store.entry(&card.id).unwrap().data["screenAccess"]["effect"], option != "deny");
    }
    for active in ["bot", "other"] {
        let f = Fixture::new();
        let (card, _wait) = f.ask();
        let phone = f.device("phone", DeviceSource::Account);
        let hub = f.hub.clone();
        *f.hub.store.before_owner_write.locked() = Some(Box::new(move || {
            hub.screen.agent_for_test(Some(active));
        }));
        let before = f.hub.store.entry(&card.id).unwrap();
        let error = f.answer(&card, "always", &phone).await.unwrap_err();
        assert!(error.to_string().contains("Pare o uso da tela"));
        let after = f.hub.store.entry(&card.id).unwrap();
        assert_eq!(after.rev, before.rev);
        assert_eq!(after.data, before.data);
        assert_eq!(f.audit(), 0);
        assert!(!f.permanent());
        f.answer(&card, "now", &phone).await.unwrap();
        assert!(!f.permanent());
        assert_eq!(f.audit(), 1);
    }
}

// Contrato futuro solicitado: ativar ao separar o motor na D330-F5; não é sinal de F5 em produção.
#[tokio::test(start_paused = true)]
#[ignore = "D330-F5: Caller::Local só nega; ativar quando o motor for separado"]
async fn d511_d330_f5_local_so_nega_futuro() {
    for option in ["always", "now"] {
        let f = Fixture::new();
        let (card, _wait) = f.ask();
        assert!(f.answer(&card, option, &Caller::Local).await.is_err());
        let after = f.hub.store.entry(&card.id).unwrap();
        assert_eq!(after.rev, card.rev);
        assert_eq!(after.data, card.data);
        assert_eq!(f.audit(), 0);
        assert!(!f.permanent());
        f.answer(&card, "deny", &Caller::Local).await.unwrap();
        assert_eq!(f.hub.store.entry(&card.id).unwrap().data["selected"], "deny");
    }
}
