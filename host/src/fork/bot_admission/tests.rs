use super::*;
use crate::store::{EntryKind, Lane};

fn store() -> Store {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    store.save_bot(&config()).unwrap();
    store
}
fn queued(store: &Store) -> String {
    store.insert_entry(&Lane::main("bot"), EntryKind::User, 1, &json!({"text":"pedido","status":"queued"})).unwrap().id
}
fn config() -> BotConfig {
    serde_json::from_value(json!({"id":"bot","name":"Bot"})).unwrap()
}
fn ticket(store: &Store, origin: &Origin) -> Ticket {
    reserve(store, &config(), "bot", "token-do-hub", origin).unwrap()
}

#[test]
fn turno_nao_humano_reconfere_reserva_antes_da_permissao() {
    use crate::fork::fallback_contas::{estado, prova};

    let conta = format!("ac_{}", "b".repeat(64));
    let cfg: BotConfig = serde_json::from_value(json!({
        "id":"bot", "name":"Bot", "backend":"claude", "forkEditor":{"engineAccountId":conta}
    }))
    .unwrap();
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    store.save_bot(&cfg).unwrap();
    let launch = json!({"kind":"cli_profile","executable":"/fixture/claude","directory":"/fixture/b"});
    let proof = json!({"method":"claude_auth_status","plan":"max"});
    store
        .kv_set(
            "fork.engine-accounts.v1",
            &json!({"salt":uuid::Uuid::new_v4().to_string(),"accounts":[{
                "id":conta,"backend":"claude","billing":"subscription","state":"connected","launch":launch,
                "scope":"owner","quotaIdentity":conta,"verifiedAt":1,"subscriptionProof":proof,"models":[],
                "managed":{"label":"Conta","revision":1,"generation":0,"enabled":true,"requestId":"f","principal":"f",
                    "profile":null,"setupRequest":null,"catalog":{"accountId":conta,"principal":"f","launch":launch,
                        "revision":1,"subscription":proof}}
            }]})
            .to_string(),
        )
        .unwrap();
    estado::alterar(&store, |e| {
        e.ordens.insert(estado::chave_ordem(&cfg.space_id, "claude"), vec![conta.clone()]);
        Ok(())
    })
    .unwrap();
    {
        let db = store.db.locked();
        estado::gravar_sobreposicao(
            &db,
            "bot",
            Some(&estado::Sobreposicao {
                space: cfg.space_id.clone(),
                bot: cfg.id.clone(),
                origem: "origem".into(),
                atual: conta.clone(),
                desde: 1,
                motivo: estado::Motivo::Esgotada,
            }),
        )
        .unwrap();
        assert_eq!(prova::conferir_vinculos_on(&db, &cfg, &conta), Ok(()));
    }
    let ticket = reserve(&store, &cfg, "bot", "token-do-hub", &Origin::Compaction).unwrap();
    estado::alterar(&store, |e| {
        e.ordens.clear();
        Ok(())
    })
    .unwrap();
    let error = permit(&store, &cfg, &ticket, None, &[], None).unwrap_err();
    assert!(error.to_string().contains("fallback_no_proof"));
    assert_eq!(current(&store, "bot").unwrap().unwrap().phase, Phase::Reserved);
    estado::alterar(&store, |e| {
        e.ordens.insert(estado::chave_ordem(&cfg.space_id, "claude"), vec![conta.clone()]);
        Ok(())
    })
    .unwrap();
    assert!(permit(&store, &cfg, &ticket, None, &[], None).unwrap().is_some());
}

#[test]
fn pausa_vence_permissao_sem_consumir_humano_e_retomar_nao_reusa_permissao() {
    let store = store();
    let id = queued(&store);
    let ticket = ticket(&store, &Origin::Human);
    set_paused_for_test(&store, "bot", true).unwrap();
    assert!(permit(&store, &config(), &ticket, None, std::slice::from_ref(&id), None).unwrap().is_none());
    assert_eq!(store.entry(&id).unwrap().data["status"], "queued");
    assert_eq!(current(&store, "bot").unwrap().unwrap().phase, Phase::Reserved);
    set_paused_for_test(&store, "bot", false).unwrap();
    assert!(permit(&store, &config(), &ticket, None, std::slice::from_ref(&id), None).unwrap().is_some());
    assert_eq!(store.entry(&id).unwrap().data["status"], "sent");
    assert!(permit(&store, &config(), &ticket, None, &[id], None).is_err());
}

#[test]
fn permissao_vence_pausa_so_para_tentativa_capturada_e_conclusao_velha_nao_libera_sucessor() {
    let store = store();
    let first = ticket(&store, &Origin::Group);
    assert!(permit(&store, &config(), &first, None, &[], None).unwrap().is_some());
    set_paused_for_test(&store, "bot", true).unwrap();
    assert_eq!(current(&store, "bot").unwrap().unwrap().phase, Phase::Permitted);
    assert!(release_reserved(&store, "bot", &first).is_err(), "retry de setup não apaga permissão");
    assert_eq!(current(&store, "bot").unwrap().unwrap().phase, Phase::Permitted);
    finish(&store, "bot", &first).unwrap();
    let next = ticket(&store, &Origin::Ask);
    assert_ne!(first.attempt, next.attempt);
    assert!(finish(&store, "bot", &first).is_err());
    assert!(permit(&store, &config(), &first, None, &[], None).is_err());
    assert!(permit(&store, &config(), &next, None, &[], None).unwrap().is_none());
    assert_eq!(current(&store, "bot").unwrap().unwrap().attempt, next.attempt);
    assert!(release_reserved(&store, "bot", &first).is_err(), "retry antigo não libera sucessor");
    assert_eq!(current(&store, "bot").unwrap().unwrap().attempt, next.attempt);
    release_reserved(&store, "bot", &next).unwrap();
    assert!(current(&store, "bot").unwrap().is_none());
}

#[test]
fn falha_da_permissao_reverte_sent_checkpoint_e_cerca() {
    let store = store();
    let id = queued(&store);
    let ticket = ticket(&store, &Origin::Human);
    // Espera gravada por um binário com o gravador ligado traz a classificação de roteamento (D305).
    let checkpoint =
        json!({"executionToken":ticket.execution_token,"routing":{"kind":"notRouted","reason":"preRoteamento"},
        "input":{"chat":"bot","entries":[id]}})
        .to_string();
    store.db.locked().execute_batch("CREATE TRIGGER reject_permit BEFORE UPDATE ON kv WHEN NEW.k='turn.inflight.bot' AND json_extract(NEW.v,'$.phase')='permitted' BEGIN SELECT RAISE(ABORT,'indisponível'); END;").unwrap();
    let error = permit(&store, &config(), &ticket, Some(&checkpoint), std::slice::from_ref(&id), None).unwrap_err();
    assert!(error.to_string().contains("indisponível"), "a prova alcança a gravação da permissão: {error:#}");
    assert_eq!(store.entry(&id).unwrap().data["status"], "queued");
    assert!(store.kv_read("turn.waiting.bot").unwrap().is_none());
    assert_eq!(current(&store, "bot").unwrap().unwrap().phase, Phase::Reserved);
    store.db.locked().execute_batch("DROP TRIGGER reject_permit").unwrap();
    assert!(permit(&store, &config(), &ticket, None, &[id], None).unwrap().is_some());
}

#[test]
fn extensoes_de_sessoes_sobrevivem_permissao_e_limpeza_legada() {
    let store = store();
    let ticket = ticket(&store, &Origin::Human);
    let mut value: Value = serde_json::from_str(&store.kv_read(&key("bot")).unwrap().unwrap()).unwrap();
    value["extensionFromSessions"] = json!({"opaque":"preservar"});
    store.kv_set(&key("bot"), &value.to_string()).unwrap();
    store.kv_set("turn.waiting.bot", "tombstone").unwrap();
    store.clear_waiting_inflight("bot").unwrap();
    assert!(permit(&store, &config(), &ticket, None, &[], None).unwrap().is_some());
    let value: Value = serde_json::from_str(&store.kv_read(&key("bot")).unwrap().unwrap()).unwrap();
    assert_eq!(value["extensionFromSessions"]["opaque"], "preservar");
    assert_eq!(value["executionToken"], "token-do-hub");
    assert_eq!(value["phase"], "permitted");
}

#[test]
fn reinicio_preserva_reserva_e_classifica_permissao_sem_inventar_replay_transitorio() {
    let dir = std::env::temp_dir().join(format!("donna-admission-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.db");
    for origin in [
        Origin::Human,
        Origin::Ask,
        Origin::Message,
        Origin::Group,
        Origin::Compaction,
        Origin::Recovery,
        Origin::Routine { id: "run".into() },
    ] {
        let store = Store::open(&path).unwrap();
        store.save_bot(&config()).unwrap();
        let ticket = ticket(&store, &origin);
        drop(store);
        let store = Store::open(&path).unwrap();
        let record = current(&store, "bot").unwrap().unwrap();
        assert_eq!(record.phase, Phase::Reserved);
        assert_eq!(record.origin, origin);
        assert!(crate::fork::spaces::inflight(&store.kv_read(&key("bot")).unwrap().unwrap(), "bot").is_none());
        assert!(permit(&store, &config(), &ticket, None, &[], None).unwrap().is_some());
        drop(store);
        let store = Store::open(&path).unwrap();
        assert_eq!(current(&store, "bot").unwrap().unwrap().phase, Phase::Permitted);
        assert!(permit(&store, &config(), &ticket, None, &[], None).is_err());
        finish(&store, "bot", &ticket).unwrap();
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn legado_e_formato_novo_nao_se_confundem_e_erro_de_leitura_fecha_admissao() {
    let store = store();
    for raw in ["1", "", r#"{"started":1,"conversationId":"bot","extra":true}"#] {
        assert!(decode(raw).unwrap().is_none());
    }
    store.kv_set("donna.bot.admission.bot", "inválido").unwrap();
    assert!(paused(&store, "bot").is_err());
    assert!(decode(r#"{"admissionVersion":2}"#).is_err());
    assert!(decode(r#"{"admissionVersion":1,"phase":"permitted"}"#).is_err());
}

#[test]
fn pausa_e_por_identidade_e_boot_preserva_fila_reservada_sem_promover_assinatura() {
    let store = store();
    let id = queued(&store);
    let ticket = ticket(&store, &Origin::Human);
    store.expire_pending().unwrap();
    let entry = store.entry(&id).unwrap();
    assert_eq!(entry.data["status"], "queued");
    assert!(entry.data.get("subscriptionQueued").is_none());
    set_paused_for_test(&store, "bot", true).unwrap();
    assert!(!paused(&store, "outro").unwrap());
    finish(&store, "bot", &ticket).unwrap();
    store.expire_pending().unwrap();
    assert_eq!(store.entry(&id).unwrap().data["status"], "queued");
}

#[test]
fn retomar_nao_ressuscita_humano_cancelado_ou_expirado() {
    let store = store();
    for status in ["cancelled", "expired", "failed"] {
        let id = queued(&store);
        let mut entry = store.entry(&id).unwrap();
        entry.data["status"] = json!(status);
        store.update_entry(&id, &entry.data).unwrap();
        let ticket = ticket(&store, &Origin::Human);
        set_paused_for_test(&store, "bot", true).unwrap();
        set_paused_for_test(&store, "bot", false).unwrap();
        assert!(permit(&store, &config(), &ticket, None, std::slice::from_ref(&id), None).is_err());
        assert_eq!(store.entry(&id).unwrap().data["status"], status);
        finish(&store, "bot", &ticket).unwrap();
    }
}

#[test]
fn disputa_real_de_pausa_e_permissao_serializa_na_mesma_guarda() {
    let store = std::sync::Arc::new(store());
    let id = queued(&store);
    let ticket = ticket(&store, &Origin::Human);
    let barrier = std::sync::Barrier::new(2);
    let granted = std::thread::scope(|scope| {
        let paused = scope.spawn(|| {
            barrier.wait();
            set_paused_for_test(&store, "bot", true).unwrap();
        });
        barrier.wait();
        let granted = permit(&store, &config(), &ticket, None, std::slice::from_ref(&id), None).unwrap().is_some();
        paused.join().unwrap();
        granted
    });
    assert!(paused(&store, "bot").unwrap());
    assert_eq!(
        current(&store, "bot").unwrap().unwrap().phase,
        if granted { Phase::Permitted } else { Phase::Reserved }
    );
    assert_eq!(store.entry(&id).unwrap().data["status"], if granted { "sent" } else { "queued" });
}

#[test]
fn claim_confere_sessao_esperada_e_reverte_lote_com_sessoes_misturadas() {
    let store = store();
    // Sessões PR5: o claim recusa sessão inexistente ou encerrada antes de tocar entradas; as duas extras abertas
    // vêm do produtor canônico para que as negativas abaixo provem o conflito de sessão no próprio claim.
    let create = || {
        crate::fork::sessoes::store::create(
            &store.db.locked(),
            &crate::fork::sessoes::store::NewSession {
                chat: "bot".into(),
                origin: crate::fork::sessoes::origin::SessionOrigin::Owner,
                source_session: None,
                parent_session: None,
                title: String::new(),
                runs_on: None,
            },
        )
        .unwrap()
        .id
    };
    let (extra_id, outra_id) = (create(), create());
    let extra_lane = Lane { chat: "bot".into(), session: Some(extra_id.clone()) };
    let extra =
        store.insert_entry(&extra_lane, EntryKind::User, 1, &json!({"text":"extra","status":"queued"})).unwrap();
    let principal = queued(&store);
    let ticket = ticket(&store, &Origin::Human);
    let conflict = |result: Result<Option<Vec<Entry>>>| {
        let error = result.unwrap_err();
        assert!(error.to_string().contains("admission_session_conflict"), "{error:#}");
    };
    conflict(permit(&store, &config(), &ticket, None, std::slice::from_ref(&extra.id), None));
    conflict(permit(&store, &config(), &ticket, None, std::slice::from_ref(&extra.id), Some(&outra_id)));
    // O primeiro item seria sent; falhar o segundo precisa reverter o lote todo.
    conflict(permit(&store, &config(), &ticket, None, &[extra.id.clone(), principal.clone()], Some(&extra_id)));
    assert_eq!(store.entry(&extra.id).unwrap().data["status"], "queued");
    assert_eq!(store.entry(&principal).unwrap().data["status"], "queued");
    assert_eq!(current(&store, "bot").unwrap().unwrap().phase, Phase::Reserved);
    assert!(
        permit(&store, &config(), &ticket, None, std::slice::from_ref(&extra.id), Some(&extra_id)).unwrap().is_some()
    );
    let saved = store.entry(&extra.id).unwrap();
    assert_eq!(saved.data["status"], "sent");
    assert_eq!(saved.session_id, extra_id);
    assert_eq!(store.entry(&principal).unwrap().data["status"], "queued");
}

#[test]
fn permissao_revalida_fechamento_e_membro_do_grupo_pelo_mesmo_produtor_de_contexto() {
    let store = store();
    let cfg = config();
    store.save_bot(&cfg).unwrap();
    let mut other = cfg.clone();
    other.id = "other".into();
    store.save_bot(&other).unwrap();
    let mut group: BotConfig = serde_json::from_value(json!({
        "id":"group","name":"Grupo","kind":"group","members":["bot","other"]
    }))
    .unwrap();
    store.save_bot(&group).unwrap();
    let ticket = reserve(&store, &config(), "group", "hub-grupo", &Origin::Group).unwrap();
    store.db.locked().execute("UPDATE donna_space_conversations SET closed=1 WHERE id='group'", []).unwrap();
    assert!(permit(&store, &cfg, &ticket, None, &[], None).unwrap_err().to_string().contains("conversation_closed"));
    assert_eq!(current(&store, "bot").unwrap().unwrap().phase, Phase::Reserved);
    store.db.locked().execute("UPDATE donna_space_conversations SET closed=0 WHERE id='group'", []).unwrap();
    group.members = vec!["other".into()];
    store.save_bot(&group).unwrap();
    assert!(permit(&store, &cfg, &ticket, None, &[], None).unwrap_err().to_string().contains("cross_space_member"));
    assert_eq!(current(&store, "bot").unwrap().unwrap().phase, Phase::Reserved);
    group.members.push("bot".into());
    store.save_bot(&group).unwrap();
    assert!(permit(&store, &cfg, &ticket, None, &[], None).unwrap().is_some());
    assert_eq!(current(&store, "bot").unwrap().unwrap().phase, Phase::Permitted);
}

#[test]
fn reserva_sem_politica_e_checkpoint_sent_sem_token_nao_recebem_identidade_fabricada() {
    let store = store();
    let id = queued(&store);
    let captured = ticket(&store, &Origin::Human);
    let mut value: Value = serde_json::from_str(&store.kv_read(&key("bot")).unwrap().unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("memoryPolicy");
    store.kv_set(&key("bot"), &value.to_string()).unwrap();
    assert!(permit(&store, &config(), &captured, None, std::slice::from_ref(&id), None).is_err());
    assert_eq!(store.entry(&id).unwrap().data["status"], "queued");
    release_reserved(&store, "bot", &captured).unwrap();
    let original = ticket(&store, &Origin::Human);
    permit(&store, &config(), &original, None, std::slice::from_ref(&id), None).unwrap().unwrap();
    finish(&store, "bot", &original).unwrap();
    let next = reserve(&store, &config(), "bot", &original.execution_token, &Origin::Human).unwrap();
    let legacy = json!({"phase":"retrying","input":{"chat":"bot","entries":[id]}}).to_string();
    assert!(permit(&store, &config(), &next, Some(&legacy), std::slice::from_ref(&id), None).is_err());
    assert_eq!(current(&store, "bot").unwrap().unwrap().phase, Phase::Reserved);
    assert!(store.kv_read("turn.waiting.bot").unwrap().is_none());
    assert_eq!(store.entry(&id).unwrap().data["status"], "sent");
}

#[test]
fn retomada_transacional_reusa_token_e_reverte_checkpoint_sent_e_permissao_juntos() {
    let store = store();
    let id = queued(&store);
    let first = ticket(&store, &Origin::Human);
    permit(&store, &config(), &first, None, std::slice::from_ref(&id), None).unwrap().unwrap();
    finish(&store, "bot", &first).unwrap();
    let checkpoint = json!({"phase":"waiting","executionToken":first.execution_token,
            "routing":{"kind":"notRouted","reason":"preRoteamento"},"input":{"chat":"bot","entries":[id]}});
    store.kv_set("turn.waiting.bot", &checkpoint.to_string()).unwrap();
    let mut retry = checkpoint.clone();
    retry["phase"] = json!("retrying");
    let next = reserve(&store, &config(), "bot", &first.execution_token, &Origin::Human).unwrap();
    assert_ne!(first.attempt, next.attempt);
    let mut wrong = retry.clone();
    wrong["executionToken"] = json!("outra-execucao");
    assert!(permit(&store, &config(), &next, Some(&wrong.to_string()), std::slice::from_ref(&id), None).is_err());
    assert_eq!(store.kv_read("turn.waiting.bot").unwrap().unwrap(), checkpoint.to_string());
    store.db.locked().execute_batch("CREATE TRIGGER reject_token_permit BEFORE UPDATE ON kv WHEN NEW.k='turn.inflight.bot' AND json_extract(NEW.v,'$.phase')='permitted' BEGIN SELECT RAISE(ABORT,'sem permissão'); END;").unwrap();
    assert!(permit(&store, &config(), &next, Some(&retry.to_string()), std::slice::from_ref(&id), None).is_err());
    assert_eq!(store.kv_read("turn.waiting.bot").unwrap().unwrap(), checkpoint.to_string());
    assert_eq!(current(&store, "bot").unwrap().unwrap().phase, Phase::Reserved);
    store.db.locked().execute_batch("DROP TRIGGER reject_token_permit").unwrap();
    permit(&store, &config(), &next, Some(&retry.to_string()), std::slice::from_ref(&id), None).unwrap().unwrap();
    let saved: Value = serde_json::from_str(&store.kv_read("turn.waiting.bot").unwrap().unwrap()).unwrap();
    assert_eq!(saved["executionToken"], current(&store, "bot").unwrap().unwrap().execution_token);
}

#[test]
fn orcamento_humano_duravel_reserva_antes_do_preparo_e_nao_renova_no_restart() {
    let dir = std::env::temp_dir().join(format!("donna-setup-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.db");
    let store = Store::open(&path).unwrap();
    store.save_bot(&config()).unwrap();
    let id = queued(&store);
    let ids = vec![id.clone()];
    let ticket = reserve_for_entries(&store, &config(), "bot", "token-original", &Origin::Human, &ids).unwrap();
    store.db.locked().execute_batch("CREATE TRIGGER reject_setup_budget BEFORE UPDATE ON kv WHEN NEW.k='turn.inflight.bot' BEGIN SELECT RAISE(ABORT,'orçamento indisponível'); END;").unwrap();
    assert!(begin_human_setup(&store, "bot", &ticket).unwrap_err().to_string().contains("orçamento indisponível"));
    assert_eq!(current(&store, "bot").unwrap().unwrap().setup_attempts, 0);
    store.db.locked().execute_batch("DROP TRIGGER reject_setup_budget").unwrap();
    begin_human_setup(&store, "bot", &ticket).unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    reuse_reserved(&store, &config(), &ticket, "bot", "token-original", &Origin::Human, &ids).unwrap();
    assert!(reuse_reserved(&store, &config(), &ticket, "bot", "outro-token", &Origin::Human, &ids).is_err());
    assert!(reuse_reserved(&store, &config(), &ticket, "bot", "token-original", &Origin::Human, &[]).is_err());
    begin_human_setup(&store, "bot", &ticket).unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(current(&store, "bot").unwrap().unwrap().setup_attempts, 2);
    begin_human_setup(&store, "bot", &ticket).unwrap();
    assert!(begin_human_setup(&store, "bot", &ticket).is_err(), "não existe quarta tentativa");
    assert_eq!(current(&store, "bot").unwrap().unwrap().ticket(), ticket);
    assert_eq!(store.entry(&id).unwrap().data["status"], "queued");
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn preparo_pronto_em_pausa_nao_gasta_falha_e_permitted_nao_reabre_preparo() {
    let store = store();
    let id = queued(&store);
    let ticket =
        reserve_for_entries(&store, &config(), "bot", "original", &Origin::Human, std::slice::from_ref(&id)).unwrap();
    begin_human_setup(&store, "bot", &ticket).unwrap();
    begin_human_setup(&store, "bot", &ticket).unwrap();
    begin_human_setup(&store, "bot", &ticket).unwrap();
    assert!(
        permit(&store, &config(), &ticket, None, std::slice::from_ref(&id), None).is_err(),
        "preparo falho/incompleto não permite envio"
    );
    mark_human_setup_ready(&store, "bot", &ticket).unwrap();
    set_paused_for_test(&store, "bot", true).unwrap();
    assert!(permit(&store, &config(), &ticket, None, std::slice::from_ref(&id), None).unwrap().is_none());
    assert_eq!(current(&store, "bot").unwrap().unwrap().setup_attempts, 3);
    // Reconstruir preparo comprovadamente pronto após espera/restart não é outra falha.
    begin_human_setup(&store, "bot", &ticket).unwrap();
    assert_eq!(current(&store, "bot").unwrap().unwrap().setup_attempts, 3);
    mark_human_setup_ready(&store, "bot", &ticket).unwrap();
    set_paused_for_test(&store, "bot", false).unwrap();
    permit(&store, &config(), &ticket, None, std::slice::from_ref(&id), None).unwrap().unwrap();
    let raw = store.kv_read(&key("bot")).unwrap();
    assert!(begin_human_setup(&store, "bot", &ticket).is_err());
    assert!(reuse_reserved(&store, &config(), &ticket, "bot", "original", &Origin::Human, &[id]).is_err());
    assert_eq!(store.kv_read(&key("bot")).unwrap(), raw);
}

fn hub_with_turn() -> (std::path::PathBuf, std::sync::Arc<crate::hub::Hub>, BotConfig, String, String) {
    let dir = std::env::temp_dir().join(format!("admissao-encaixe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = crate::hub::Hub::new(
        store(),
        "test".into(),
        crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
        "test".into(),
        0,
    );
    let id = queued(&hub.store);
    let cfg = hub.prepare_bot_turn("bot", None, |r| r.lane = Some(Lane::main("bot"))).unwrap();
    let token = hub.runtime("bot").preparing_turn_token.unwrap();
    (dir, hub, cfg, id, token)
}

/// Encaixe PR4 (Dispatch): pré-imagem na mesma Tx IMMEDIATE, antes do claim; falha desfaz
/// claim/permitted/inflight e nada é ativado. O `BindingRef` é o original do setup, mesmo
/// depois de um await e de outro vínculo emitido para a mesma sessão.
#[tokio::test]
async fn encaixe_dispatch_falho_desfaz_claim_e_nao_ativa_com_binding_original() {
    let (dir, hub, cfg, id, token) = hub_with_turn();
    let ticket = reserve(&hub.store, &cfg, "bot", &token, &Origin::Human).unwrap();
    // Vínculo do setup já com a ACP da sessão; emitir outro (pendente) não o substitui.
    let original =
        crate::fork::sessoes::bindings::issue(&hub.store, "bot", "bot", "bot", Some("acp-original")).unwrap();
    tokio::task::yield_now().await;
    let newer = crate::fork::sessoes::bindings::issue(&hub.store, "bot", "bot", "bot", None).unwrap();
    assert_ne!(newer.id, original.id);
    let before = hub.store.kv_read("turn.inflight.bot").unwrap();
    let mut seen = Vec::new();
    let mut dispatch = |c: &Connection, binding: &crate::fork::sessoes::bindings::BindingRef| -> Result<RoutingClass> {
        seen.push(binding.id.clone());
        let entry = Store::entry_on(c, &id)?.context("entrada ausente")?;
        ensure!(entry.data["status"] == "queued", "pré-imagem antes do claim");
        c.execute("INSERT INTO kv(k,v) VALUES('pr4.preimagem','x')", [])?;
        anyhow::bail!("routing_record_failed")
    };
    let mut resume = |_: &Connection, _: &str| -> Result<RoutingClass> { panic!("Dispatch não chama Resume") };
    let hooks = ClaimHooks {
        binding: Some(&original),
        dispatch: Some(&mut dispatch),
        resume: Some(&mut resume),
        ..Default::default()
    };
    let error = hub.permit_bot_dispatch(&cfg, &ticket, None, std::slice::from_ref(&id), None, hooks).unwrap_err();
    assert!(error.to_string().contains("routing_record_failed"), "{error:#}");
    assert_eq!(seen, std::slice::from_ref(&original.id), "mesmo vínculo do setup, não o mais novo");
    assert_eq!(hub.store.entry(&id).unwrap().data["status"], "queued", "claim desfeito");
    assert!(hub.store.kv_read("pr4.preimagem").unwrap().is_none(), "pré-imagem desfeita");
    assert_eq!(hub.store.kv_read("turn.inflight.bot").unwrap(), before, "inflight intacto");
    assert_eq!(current(&hub.store, "bot").unwrap().unwrap().phase, Phase::Reserved);
    let runtime = hub.runtime("bot");
    assert!(runtime.memory_turn_token.is_none(), "nada ativado");
    assert_eq!(runtime.preparing_turn_token.as_deref(), Some(token.as_str()));
    // Vínculo original revogado: falha fechada, sem adotar o outro.
    crate::fork::sessoes::bindings::revoke(&hub.store, "bot", None).unwrap();
    let mut revoked_hook = |_: &Connection, _: &crate::fork::sessoes::bindings::BindingRef| -> Result<RoutingClass> {
        panic!("vínculo revogado não chega ao encaixe")
    };
    let hooks =
        ClaimHooks { binding: Some(&original), dispatch: Some(&mut revoked_hook), resume: None, ..Default::default() };
    assert!(hub.permit_bot_dispatch(&cfg, &ticket, None, std::slice::from_ref(&id), None, hooks).is_err());
    assert_eq!(hub.store.entry(&id).unwrap().data["status"], "queued");
    // D305: turno humano sem o consumidor de Sessões não segue mais por omissão; falha fechado, sem claim.
    let hooks = ClaimHooks { binding: Some(&original), ..Default::default() };
    let error = hub.permit_bot_dispatch(&cfg, &ticket, None, std::slice::from_ref(&id), None, hooks).unwrap_err();
    assert!(error.to_string().contains("admission_routing_consumer_missing"), "{error:#}");
    assert_eq!(hub.store.entry(&id).unwrap().data["status"], "queued");
    assert_eq!(current(&hub.store, "bot").unwrap().unwrap().phase, Phase::Reserved);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Resume (checkpoint original) não chama o encaixe inicial de Dispatch.
#[test]
fn encaixe_resume_nao_chama_gravador_de_dispatch() {
    let (dir, hub, cfg, id, token) = hub_with_turn();
    let ticket = reserve(&hub.store, &cfg, "bot", &token, &Origin::Human).unwrap();
    let checkpoint = json!({"executionToken":token,"input":{"chat":"bot","entries":[id]}}).to_string();
    let mut dispatch = |_: &Connection, _: &crate::fork::sessoes::bindings::BindingRef| -> Result<RoutingClass> {
        panic!("Resume não grava recibo inicial")
    };
    let resumed = std::cell::RefCell::new(Vec::new());
    let mut resume = |_: &Connection, raw: &str| -> Result<RoutingClass> {
        resumed.borrow_mut().push(raw.to_owned());
        Ok(RoutingClass::NotRouted { reason: NotRoutedReason::PreRoteamento })
    };
    // R2: a retomada humana exige o vínculo do setup desta tentativa, vivo e do destino original da espera.
    let missing =
        ClaimHooks { binding: None, dispatch: Some(&mut dispatch), resume: Some(&mut resume), ..Default::default() };
    let error = hub
        .permit_bot_dispatch(&cfg, &ticket, Some(&checkpoint), std::slice::from_ref(&id), None, missing)
        .unwrap_err();
    assert!(error.to_string().contains("admission_binding_missing"), "{error:#}");
    // Vínculo do setup revogado durante os awaits: recusa antes do claim, sem chamar o consumidor nem recapturar
    // o vínculo emitido depois.
    let revoked = crate::fork::sessoes::bindings::issue(&hub.store, "bot", "bot", "bot", Some("acp-setup")).unwrap();
    crate::fork::sessoes::bindings::revoke(&hub.store, "bot", None).unwrap();
    let newer = crate::fork::sessoes::bindings::issue(&hub.store, "bot", "bot", "bot", Some("acp-novo")).unwrap();
    let stale = ClaimHooks {
        binding: Some(&revoked),
        dispatch: Some(&mut dispatch),
        resume: Some(&mut resume),
        ..Default::default()
    };
    assert!(hub.permit_bot_dispatch(&cfg, &ticket, Some(&checkpoint), std::slice::from_ref(&id), None, stale).is_err());
    assert!(resumed.borrow().is_empty(), "nada chegou ao consumidor");
    assert_eq!(current(&hub.store, "bot").unwrap().unwrap().phase, Phase::Reserved);
    assert!(hub.runtime("bot").memory_turn_token.is_none(), "nada ativado");
    // Vínculo vivo de outro destino (sessão extra) também recusa.
    let lane = Lane {
        chat: "bot".into(),
        session: Some(
            crate::fork::sessoes::store::create(
                &hub.store.db.locked(),
                &crate::fork::sessoes::store::NewSession {
                    chat: "bot".into(),
                    origin: crate::fork::sessoes::origin::SessionOrigin::Owner,
                    source_session: None,
                    parent_session: None,
                    title: String::new(),
                    runs_on: None,
                },
            )
            .unwrap()
            .id,
        ),
    };
    let elsewhere =
        crate::fork::sessoes::bindings::issue(&hub.store, "bot", "bot", lane.session_id(), Some("acp-extra")).unwrap();
    let wrong = ClaimHooks {
        binding: Some(&elsewhere),
        dispatch: Some(&mut dispatch),
        resume: Some(&mut resume),
        ..Default::default()
    };
    let error =
        hub.permit_bot_dispatch(&cfg, &ticket, Some(&checkpoint), std::slice::from_ref(&id), None, wrong).unwrap_err();
    assert!(error.to_string().contains("admission_binding_changed"), "{error:#}");
    assert!(resumed.borrow().is_empty());
    // Controle positivo: o vínculo vivo do setup desta tentativa, no destino original.
    let hooks = ClaimHooks {
        binding: Some(&newer),
        dispatch: Some(&mut dispatch),
        resume: Some(&mut resume),
        ..Default::default()
    };
    assert!(hub.permit_bot_dispatch(&cfg, &ticket, Some(&checkpoint), std::slice::from_ref(&id), None, hooks).unwrap());
    assert_eq!(*resumed.borrow(), [checkpoint]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// D305: migração de boot atômica (marcas + flag na mesma Tx), idempotente, sem tocar valor que não é objeto JSON e sem
/// remarcar registro novo depois da flag.
#[test]
fn migracao_de_boot_marca_pre_roteamento_com_flag_na_mesma_transacao_e_nao_remarca() {
    let store = store();
    let reserved = json!({"admissionVersion":1,"executionToken":"t","attempt":"a","phase":"permitted",
        "conversationId":"bot","origin":{"kind":"human"},"entryIds":["e"]});
    let waiting = json!({"phase":"waiting","executionToken":"t","input":{"chat":"bot","entries":["e"]}});
    store.kv_set("turn.inflight.bot", &reserved.to_string()).unwrap();
    store.kv_set("turn.waiting.bot", &waiting.to_string()).unwrap();
    store.kv_set("turn.inflight.velho", "1700000000000").unwrap();
    store.kv_set("turn.inflight.vazio", "").unwrap();
    store.kv_set("turn.waiting.quebrado", "{quebrado").unwrap();
    // Falha ao gravar a flag desfaz também as marcas: sem flag parcial.
    store.db.locked().execute_batch("CREATE TRIGGER reject_flag BEFORE INSERT ON kv WHEN NEW.k='donna.routing.legacy_marked' BEGIN SELECT RAISE(ABORT,'sem flag'); END;").unwrap();
    assert!(mark_pre_routing(&store).is_err());
    assert_eq!(store.kv_read("turn.inflight.bot").unwrap().unwrap(), reserved.to_string());
    assert!(store.kv_read(LEGACY_MARKED).unwrap().is_none());
    store.db.locked().execute_batch("DROP TRIGGER reject_flag").unwrap();
    assert_eq!(mark_pre_routing(&store).unwrap(), 3);
    let mark = json!({"kind":"notRouted","reason":"preRoteamento"});
    for key in ["turn.inflight.bot", "turn.waiting.bot"] {
        let value: Value = serde_json::from_str(&store.kv_read(key).unwrap().unwrap()).unwrap();
        assert_eq!(value["routing"], mark, "{key}");
        assert_eq!(value["executionToken"], "t", "campos originais conservados");
    }
    assert_eq!(
        current(&store, "bot").unwrap().unwrap().routing,
        Some(RoutingClass::NotRouted { reason: NotRoutedReason::PreRoteamento })
    );
    // Numérico histórico: normalizado para o objeto legado equivalente, com o mesmo `started` e o mesmo chat.
    let velho: Value = serde_json::from_str(&store.kv_read("turn.inflight.velho").unwrap().unwrap()).unwrap();
    assert_eq!(velho, json!({"started":1_700_000_000_000_i64,"conversationId":"velho","routing":mark}));
    assert!(decode(&store.kv_read("turn.inflight.velho").unwrap().unwrap()).unwrap().is_none(), "segue legado");
    assert_eq!(store.kv_read("turn.inflight.vazio").unwrap().as_deref(), Some(""), "vazio (sem turno) intocado");
    assert_eq!(
        store.kv_read("turn.waiting.quebrado").unwrap().as_deref(),
        Some("{quebrado"),
        "corrompido não vira legado"
    );
    assert!(store.kv_read(LEGACY_MARKED).unwrap().is_some());
    // Segundo boot: nada é remarcado, nem um registro novo sem classificação.
    store.kv_set("turn.waiting.outro", &waiting.to_string()).unwrap();
    assert_eq!(mark_pre_routing(&store).unwrap(), 0);
    assert!(!store.kv_read("turn.waiting.outro").unwrap().unwrap().contains("routing"));
    assert!(checkpoint_routing(&store.kv_read("turn.waiting.outro").unwrap().unwrap()).is_err());
}

/// D305: o permit grava a classificação no mesmo escritor e na mesma Tx; rollback não deixa classificação; a retomada
/// sem encaixe conserva a da espera e, sem ela, falha fechado.
#[test]
fn permit_grava_classificacao_na_mesma_transacao_e_retomada_sem_classificacao_falha() {
    let store = store();
    let id = queued(&store);
    let first = ticket(&store, &Origin::Human);
    store.db.locked().execute_batch("CREATE TRIGGER reject_permit BEFORE UPDATE ON kv WHEN NEW.k='turn.inflight.bot' AND json_extract(NEW.v,'$.phase')='permitted' BEGIN SELECT RAISE(ABORT,'indisponível'); END;").unwrap();
    let mut routed = |_: &Connection, _: &crate::fork::sessoes::bindings::BindingRef| -> Result<RoutingClass> {
        Ok(RoutingClass::Routed {})
    };
    let binding = crate::fork::sessoes::bindings::fixture_binding(&store, "bot");
    let hooks = ClaimHooks { binding: Some(&binding), dispatch: Some(&mut routed), resume: None, ..Default::default() };
    assert!(permit_with(&store, &config(), &first, None, std::slice::from_ref(&id), None, hooks).is_err());
    assert_eq!(current(&store, "bot").unwrap().unwrap().routing, None, "rollback sem classificação");
    assert_eq!(store.entry(&id).unwrap().data["status"], "queued");
    store.db.locked().execute_batch("DROP TRIGGER reject_permit").unwrap();
    let mut routed = |_: &Connection, _: &crate::fork::sessoes::bindings::BindingRef| -> Result<RoutingClass> {
        Ok(RoutingClass::Routed {})
    };
    let hooks = ClaimHooks { binding: Some(&binding), dispatch: Some(&mut routed), resume: None, ..Default::default() };
    permit_with(&store, &config(), &first, None, std::slice::from_ref(&id), None, hooks).unwrap().unwrap();
    assert_eq!(current(&store, "bot").unwrap().unwrap().routing, Some(RoutingClass::Routed {}));
    finish(&store, "bot", &first).unwrap();
    // A classificação vem da origem capturada: humano sem consumidor falha fechado; rotina é `notHumanTurn`,
    // mesmo com um encaixe passado.
    let human = ticket(&store, &Origin::Human);
    let error = permit_with(&store, &config(), &human, None, &[], None, ClaimHooks::default()).unwrap_err();
    assert!(error.to_string().contains("admission_routing_consumer_missing"), "{error:#}");
    assert_eq!(current(&store, "bot").unwrap().unwrap().routing, None);
    finish(&store, "bot", &human).unwrap();
    let routine = reserve(&store, &config(), "bot", "token-rotina", &Origin::Routine { id: "r".into() }).unwrap();
    let mut never = |_: &Connection, _: &crate::fork::sessoes::bindings::BindingRef| -> Result<RoutingClass> {
        panic!("rotina não chama o consumidor humano")
    };
    let hooks = ClaimHooks { binding: Some(&binding), dispatch: Some(&mut never), resume: None, ..Default::default() };
    permit_with(&store, &config(), &routine, None, &[], None, hooks).unwrap().unwrap();
    assert_eq!(
        current(&store, "bot").unwrap().unwrap().routing,
        Some(RoutingClass::NotRouted { reason: NotRoutedReason::NotHumanTurn })
    );
    finish(&store, "bot", &routine).unwrap();
    // Retomada sem classificação na espera: falha fechado, sem claim nem permitted.
    let next = ticket(&store, &Origin::Human);
    let waiting =
        json!({"phase":"retrying","executionToken":next.execution_token,"input":{"chat":"bot","entries":[id]}});
    store
        .kv_set(
            "turn.waiting.bot",
            &json!({"phase":"waiting","executionToken":next.execution_token,"input":{"chat":"bot","entries":[id]}})
                .to_string(),
        )
        .unwrap();
    assert!(permit(&store, &config(), &next, Some(&waiting.to_string()), std::slice::from_ref(&id), None).is_err());
    assert_eq!(current(&store, "bot").unwrap().unwrap().phase, Phase::Reserved);
}
