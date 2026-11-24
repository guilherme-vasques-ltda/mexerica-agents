//! Recusa da Request ainda própria fecha o canal sem renovar a captura antiga.
use super::*;

#[tokio::test]
async fn request_account_propria_expirada_fecha_sem_dados_apos_renovacao() {
    let hub = temp_hub();
    let key = SigningKey::from_bytes(&crypto::random());
    let chave = crypto::b64(key.verifying_key().as_bytes());
    let prazo = now_ms() + 5_000;
    hub.store
        .put_device(&Device {
            key: chave.clone(),
            name: "Aparelho da request".into(),
            platform: "ios".into(),
            source: DeviceSource::Account,
            grant_id: Some("concessao-request".into()),
            scopes: vec![Scope::Control],
            lease_until: Some(prazo),
            created_at: now_ms(),
            last_seen_at: None,
        })
        .unwrap();
    crate::remote::cloud::update_status(&hub, |s| {
        s.owner = Some(crate::remote::cloud::Owner { user_id: "dono-request".into(), email: None });
    });
    hub.cloud_synced.store(true, std::sync::atomic::Ordering::Release);
    let geracao = geracao_persistida(&hub.store.db.locked(), &chave);
    let (_, caller) = autoridade::autenticar(&hub, &chave).unwrap();
    let (tx, mut rx) = mpsc::channel(1);
    let mut canal = Channel {
        hub: hub.clone(),
        tx,
        jobs: Arc::default(),
        key: chave.clone(),
        caller: Arc::new(caller),
        lease: None,
        online: None,
        quadros: false,
        browser: None,
        sessao: None,
        prazo: tokio::sync::watch::channel(None).0,
        th: [0; 32],
        epoch: hub.workspace.atual(),
        nonces: crate::fork::sessao_clerk::posse::Nonces::default(),
    };
    let original = canal.caller.clone();
    assert!(canal.tx.try_send(ToDevice::Inner(json!({"sentinela":true}))).is_ok());
    let bloqueio = hub.store.team_lock.lock().await;
    canal
        .handle(&serde_json::to_vec(&json!({"id":7,"m":"setRoutinesPaused","b":{"paused":true}})).unwrap())
        .await
        .unwrap();
    let request = {
        let lista = canal.jobs.locked();
        let Some(Job::Request(request)) = lista.get(&7) else { panic!("request própria ausente") };
        request.clone()
    };
    assert!(!request.is_finished());

    hub.store.set_lease(&chave, now_ms() + 120_000).unwrap();
    assert_eq!(geracao_persistida(&hub.store.db.locked(), &chave), geracao);
    canal.recheck().await.unwrap();
    assert!(!Arc::ptr_eq(&original, &canal.caller), "o canal recebe uma captura nova para pedidos novos");
    drop(bloqueio);
    timeout(Duration::from_secs(60), async {
        while hub.routines.pause_state()["paused"] != true {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("o efeito deve concluir antes da admissão de saída");
    assert!(!request.is_finished(), "a resposta aguarda a vaga real da fila");
    while now_ms() <= prazo {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    canal.recheck().await.unwrap();
    assert!(autoridade::Autoridade::capturar(&hub, &canal.caller).is_ok());
    assert!(autoridade::Autoridade::capturar(&hub, &original).is_err(), "o prazo antigo não foi renovado");
    assert_eq!(geracao_persistida(&hub.store.db.locked(), &chave), geracao);
    assert!(!request.is_finished());

    // Protocolo interno pós-decriptação: a primeira saída após liberar a vaga é só Close.
    assert!(matches!(rx.recv().await, Some(ToDevice::Inner(v)) if v == json!({"sentinela":true})));
    let saida = timeout(Duration::from_secs(60), rx.recv()).await.unwrap().unwrap();
    assert!(matches!(saida, ToDevice::Close(close::REKEY, "request authorization ended")));
    timeout(Duration::from_secs(60), async {
        while !request.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("a request deve liberar seu slot sem abortar o efeito");
    assert!(!canal.jobs.locked().contains_key(&7));
    assert!(matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)), "nenhuma resposta de dados");
    assert_eq!(hub.routines.pause_state()["paused"], true);
    drop(canal);

    // A recuperação positiva faz o handshake criptográfico real e cria outra captura.
    let mut nova = Phone::direct(&hub, key);
    assert!(matches!(nova.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
    assert_eq!(nova.call(1, "routinesPause", json!({})).await["ok"]["paused"], true);
    drop(nova.tx);
    timeout(Duration::from_secs(60), nova.run).await.unwrap().unwrap();
}
