//! Prova focal do fechamento observável pelo cliente após vencer o pedido original.
use super::*;

#[tokio::test]
async fn assinatura_account_vencida_fecha_canal_sem_evento_e_nova_conexao_funciona() {
    let hub = temp_hub();
    let key = SigningKey::from_bytes(&crypto::random());
    let dk = crypto::b64(key.verifying_key().as_bytes());
    let prazo = now_ms() + 5_000;
    hub.store
        .put_device(&Device {
            key: dk.clone(),
            name: "Aparelho de prova".into(),
            platform: "ios".into(),
            source: DeviceSource::Account,
            grant_id: Some("concessao-canal".into()),
            scopes: vec![Scope::Control],
            lease_until: Some(prazo),
            created_at: now_ms(),
            last_seen_at: None,
        })
        .unwrap();
    crate::remote::cloud::update_status(&hub, |s| {
        s.owner = Some(crate::remote::cloud::Owner { user_id: "dono-canal".into(), email: None });
    });
    hub.cloud_synced.store(true, std::sync::atomic::Ordering::Release);
    let geracao = geracao_persistida(&hub.store.db.locked(), &dk);
    let mut p = Phone::direct(&hub, key.clone());
    assert!(matches!(p.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
    p.send(json!({"id": 1, "sub": "events", "b": {"since": hub.store.current_rev(), "client": "ios"}})).await;
    assert_eq!(p.recv().await.unwrap()["ev"]["type"], "hello");
    assert!(hub.phone_connected());

    // A concessão é a mesma; só pedidos novos recebem o prazo renovado.
    hub.store.set_lease(&dk, now_ms() + 120_000).unwrap();
    assert_eq!(geracao_persistida(&hub.store.db.locked(), &dk), geracao);
    assert!(p.call(2, "hello", json!({})).await["ok"].is_object());
    while now_ms() <= prazo {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(p.call(3, "hello", json!({})).await["ok"].is_object(), "o canal tem captura atual válida");
    hub.events.send(json!({"type": "resync", "marcador": "nao-entregar-apos-prazo"})).unwrap();
    assert_eq!(p.recv().await.unwrap_err().0, close::REKEY, "nenhum evento pode preceder o fechamento");
    timeout(Duration::from_secs(60), &mut p.run).await.unwrap().unwrap();
    assert!(!hub.phone_connected(), "a assinatura deve liberar o contador do cliente");
    assert!(hub.connected.locked().is_empty(), "o canal deve liberar a presença");
    assert!(p.rx.recv().await.is_none(), "não há pacote após fechar o canal");

    let mut nova = Phone::direct(&hub, key);
    assert!(matches!(nova.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
    nova.send(json!({"id": 1, "sub": "events", "b": {"since": hub.store.current_rev(), "client": "ios"}})).await;
    assert_eq!(nova.recv().await.unwrap()["ev"]["type"], "hello");
    nova.send(json!({"id": 1, "cancel": true})).await;
    assert_eq!(nova.recv().await.unwrap(), json!({"id": 1, "end": true}));
    drop(nova.tx);
    timeout(Duration::from_secs(60), nova.run).await.unwrap().unwrap();
    assert!(!hub.phone_connected());
}
