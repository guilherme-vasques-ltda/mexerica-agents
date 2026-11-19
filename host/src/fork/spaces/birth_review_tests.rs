//! R1 do #625: recursos especulativos e transporte Control, sem motores ou dados reais.
use super::{ACCOUNT, Fixture};
use crate::LockExt;
use crate::{
    api::{devices::Caller, dispatch},
    store::{BotConfig, Scope},
};
use serde_json::json;

// Mesma cadeia ACS4 pública da fixture de engine_setup; assinaturas inertes e sem rede.
fn owner(f: &Fixture) -> Caller {
    use crate::{
        fork::bootstrap::codec::{self, Challenge, Grant, Proof, Receipt},
        remote::{cloud, crypto},
        store::bootstrap::{CloudGrant, Publication, TrustedGeneration},
    };
    use ed25519_dalek::SigningKey;
    let issuer = SigningKey::from_bytes(&[21; 32]);
    let device = SigningKey::from_bytes(&[22; 32]);
    let host = &f.hub.identity.sign;
    let now = crate::store::now_ms();
    let cfg = TrustedGeneration {
        generation: crypto::b64(&[23; 32]),
        issuer_key_id: "issuer".into(),
        issuer_key: crypto::b64(issuer.verifying_key().as_bytes()),
        issuer: "donna".into(),
        deployment_id: "fixture".into(),
        env: "test".into(),
        manifest_hash: "a".repeat(64),
        resource_digest: "b".repeat(64),
        computer_id: "computer".into(),
        sign_key: crypto::b64(host.verifying_key().as_bytes()),
        box_key: crypto::b64(&[24; 32]),
    };
    let g = Grant(
        "donna/bootstrap-grant/v1".into(),
        "boot".into(),
        "intent".into(),
        cfg.resource_digest.clone(),
        cfg.generation.clone(),
        cfg.issuer_key_id.clone(),
        cfg.issuer.clone(),
        cfg.deployment_id.clone(),
        cfg.env.clone(),
        cfg.manifest_hash.clone(),
        "owner".into(),
        "device".into(),
        crypto::b64(device.verifying_key().as_bytes()),
        crypto::b64(&[25; 32]),
        cfg.computer_id.clone(),
        cfg.sign_key.clone(),
        cfg.box_key.clone(),
        crypto::b64(&[26; 32]),
        now - 1000,
        now + 200_000,
        vec!["control".into(), "owner.devices".into()],
    );
    let store = &f.hub.store;
    store.bootstrap_install_generation(&cfg).unwrap();
    store.bootstrap_install_grant(&codec::encode(&g, |b| crypto::sign(&issuer, b)).unwrap(), now).unwrap();
    let wire = store.bootstrap_challenge(host, "boot", &g.12, now).unwrap();
    let ch: Challenge = codec::decode(&wire, &host.verifying_key().to_bytes()).unwrap();
    let proof = Proof(
        "donna/bootstrap-proof/v1".into(),
        g.1.clone(),
        g.4.clone(),
        codec::hash(&g).unwrap(),
        codec::hash(&ch).unwrap(),
    );
    let wire =
        store.bootstrap_commit_proof(host, &codec::encode(&proof, |b| crypto::sign(&device, b)).unwrap(), now).unwrap();
    let r: Receipt = codec::decode(&wire, &host.verifying_key().to_bytes()).unwrap();
    let publication = Publication {
        bootstrap_id: r.1.clone(),
        generation: r.2.clone(),
        grant_id: r.11.clone(),
        device_key: r.8.clone(),
        owner_user_id: r.6.clone(),
        receipt_hash: codec::hash(&r).unwrap(),
    };
    let grant = CloudGrant { grant_id: r.11, device_key: r.8.clone(), owner_user_id: r.6 };
    assert!(store.bootstrap_promote(&publication, &grant, now).unwrap());
    assert!(store.bootstrap_activation_envelope(host, now).unwrap().is_some());
    cloud::update_status(&f.hub, |s| s.owner = Some(cloud::Owner { user_id: "owner".into(), email: None }));
    f.hub.cloud_synced.store(true, std::sync::atomic::Ordering::Release);
    crate::fork::autoridade::autenticar(&f.hub, &r.8).unwrap().1
}

#[tokio::test]
async fn d543_birth_r1_workspace_preexistente_nao_e_adotado_ou_removido() {
    let f = Fixture::new();
    for with_content in [false, true] {
        let before = (f.snapshot(), f.counts(), f.hub.store.current_rev());
        let mut events = f.hub.events.subscribe();
        let config: BotConfig = serde_json::from_value(json!({
            "id":"", "name":"Principal sintético", "backend":"codex",
            "forkEditor":{"engineAccountId":ACCOUNT}
        }))
        .unwrap();
        let mut prepared = crate::fork::native_catalog::workspace::PreparedWorkspace::default();
        let mut existing = None;
        // A fábrica emite o UUID. A fixture ocupa a pasta antes do mesmo guard usado pelo Birth.
        let result = f.hub.prepare_new_bot_with_workspace(config, |cfg| {
            let path = crate::agent::workspace::path(&cfg.id);
            std::fs::create_dir_all(&path)?;
            if with_content {
                std::fs::write(path.join("marcador-sintetico.txt"), "PRESERVAR_D543")?;
            }
            existing = Some(path);
            prepared.prepare(cfg)
        });
        assert!(result.is_err());
        drop(prepared);
        let path = existing.expect("a intenção deve chegar à preparação da pasta");
        assert!(path.is_dir() && path.parent().unwrap().is_dir(), "pasta prévia não pertence ao rollback");
        if with_content {
            assert_eq!(std::fs::read_to_string(path.join("marcador-sintetico.txt")).unwrap(), "PRESERVAR_D543");
            std::fs::remove_file(path.join("marcador-sintetico.txt")).unwrap();
        }
        assert_eq!((f.snapshot(), f.counts(), f.hub.store.current_rev()), before);
        assert!(matches!(events.try_recv(), Err(tokio::sync::broadcast::error::TryRecvError::Empty)));
        std::fs::remove_dir(&path).unwrap();
        std::fs::remove_dir(path.parent().unwrap()).unwrap();
    }
    f.hub.shutdown().await;
}

#[tokio::test]
async fn d543_birth_r1_dono_control_cria_e_replay_sem_promocao_a_local() {
    let f = Fixture::new();
    let caller = owner(&f);
    assert!(matches!(&caller, Caller::Device { scopes, .. } if scopes.contains(&Scope::Control)));
    crate::fork::engine_setup::access::require(&f.hub, &caller).unwrap();
    let body = f.request("control-positivo");
    let snapshot = dispatch(&f.hub, &caller, "donnaSpaceBirth", body.clone()).await.unwrap();
    let principal = f.principal(&snapshot);
    assert_eq!(principal.fork_editor.engine_account_id.as_deref(), Some(ACCOUNT));
    assert_eq!(f.counts()[4], 1);
    assert!(crate::agent::workspace::path(&principal.id).is_dir());
    let after = (f.counts(), f.hub.store.current_rev());
    assert_eq!(dispatch(&f.hub, &caller, "donnaSpaceBirth", body).await.unwrap(), snapshot);
    assert_eq!((f.counts(), f.hub.store.current_rev()), after);
    // Birth remoto não libera os mutantes locais legados, mesmo para esse dono válido.
    let mut events = f.hub.events.subscribe();
    let before = f.snapshot();
    for method in ["donnaSpaceCreate", "donnaSpaceUpdate", "donnaSpaceReorder", "donnaOwnerPresentation"] {
        let error = dispatch(&f.hub, &caller, method, f.request("legado-local")).await.unwrap_err();
        assert!(error.to_string().contains("remote_setup_unavailable"), "método legado deve continuar local");
    }
    assert_eq!(f.snapshot(), before);
    assert_eq!((f.counts(), f.hub.store.current_rev()), after);
    assert!(matches!(events.try_recv(), Err(tokio::sync::broadcast::error::TryRecvError::Empty)));
    f.hub.shutdown().await;
}

#[tokio::test]
async fn d543_birth_r1_revogacao_e_conta_desconectada_apos_captura_nao_deixam_efeitos() {
    for revoke in [true, false] {
        let f = Fixture::new();
        let caller = owner(&f);
        crate::fork::engine_setup::access::require(&f.hub, &caller).unwrap();
        let body = f.request(if revoke { "revogado-na-fila" } else { "conta-desconectada-na-fila" });
        let held = f.hub.store.team_lock.clone().lock_owned().await;
        let pending = dispatch(&f.hub, &caller, "donnaSpaceBirth", body.clone());
        tokio::pin!(pending);
        // Poll causal: Authority já foi resolvida, e Birth está parado em team_lock.
        tokio::select! {
            biased;
            result = &mut pending => panic!("pedido deveria esperar a trava: {result:?}"),
            () = std::future::ready(()) => {}
        }
        if revoke {
            f.hub.store.remove_device(caller.device_key()).unwrap();
            assert!(crate::fork::engine_setup::access::require(&f.hub, &caller).is_err());
        } else {
            f.account("disconnected");
            assert!(
                crate::fork::engine_setup::access::require(&f.hub, &caller).is_ok(),
                "a negativa deve ser a conta, não a autoridade"
            );
        }
        // A única mudança permitida é a externa que a fixture acabou de efetuar.
        let before = (f.snapshot(), f.counts(), f.hub.store.current_rev());
        let mut events = f.hub.events.subscribe();
        drop(held);
        let error = pending.await.unwrap_err();
        let code = format!("{error:#}");
        if revoke {
            assert!(code.contains("remote_setup_unavailable"));
        } else {
            assert!(code.contains("account_unavailable"));
        }
        assert_eq!(
            (f.snapshot(), f.counts(), f.hub.store.current_rev()),
            before,
            "não criar Space, bot, conversa, sessão, entrada, recibo ou revisão na recusa"
        );
        assert!(matches!(events.try_recv(), Err(tokio::sync::broadcast::error::TryRecvError::Empty)));
        assert!(!f.hub.busy(), "nenhum motor inicia na recusa");
        // Falha não consome nonce: intenção original ainda pode nascer por dono válido após restaurar conta.
        f.account("connected");
        let valid = if revoke { Caller::Local } else { caller.clone() };
        let created = dispatch(&f.hub, &valid, "donnaSpaceBirth", body).await.unwrap();
        assert_eq!(created["spaces"].as_array().unwrap().len(), 2);
        assert_eq!(f.counts()[4], 1);
        f.hub.shutdown().await;
    }
}

#[tokio::test]
async fn d543_lifecycle_revogacao_na_fila_preserva_estado_e_eventos() {
    for method in ["donnaSpacePrincipal", "donnaSpaceDelete"] {
        let f = Fixture::new();
        let caller = owner(&f);
        // O contrato de plataforma usa o dispositivo já autenticado; teste isolado
        // da revogação, com plataforma desktop no registro sintético.
        f.hub
            .store
            .db
            .locked()
            .execute("UPDATE devices SET platform='macos' WHERE key=?1", [caller.device_key()])
            .unwrap();
        let created = dispatch(
            &f.hub,
            &Caller::Local,
            "donnaSpaceCreate",
            json!({
                "hubId":f.hub.host_id,"expectedRevision":f.snapshot()["revision"],"name":"Legado",
                "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"LG"}}
            }),
        )
        .await
        .unwrap();
        let space = created["spaces"][1]["id"].as_str().unwrap();
        let body = if method == "donnaSpacePrincipal" {
            json!({"hubId":f.hub.host_id,"spaceId":space,"expectedRevision":created["revision"],
                "clientNonce":"capturado","principal":{"backend":"codex","engineAccountId":ACCOUNT}})
        } else {
            json!({"hubId":f.hub.host_id,"spaceId":space,"expectedRevision":created["revision"],"confirmName":"Legado"})
        };
        let held = f.hub.store.team_lock.clone().lock_owned().await;
        let pending = dispatch(&f.hub, &caller, method, body);
        tokio::pin!(pending);
        tokio::select! {
            biased;
            result = &mut pending => panic!("deveria esperar: {result:?}"),
            () = std::future::ready(()) => {}
        }
        f.hub.store.remove_device(caller.device_key()).unwrap();
        let before = (f.snapshot(), f.counts(), f.hub.store.current_rev());
        let mut events = f.hub.events.subscribe();
        drop(held);
        let error = pending.await.unwrap_err();
        assert!(format!("{error:#}").contains("remote_setup_unavailable"), "{error:#}");
        assert_eq!((f.snapshot(), f.counts(), f.hub.store.current_rev()), before);
        assert!(events.try_recv().is_err());
        f.hub.shutdown().await;
    }
}
