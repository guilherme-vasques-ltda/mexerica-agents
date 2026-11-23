//! Prova do `/channel`: evento em voo não sai depois do bump, o canal fecha com `reject`
//! estável, e o aparelho revogado não reabre canal.
use super::*;

const MARCA: &str = "evento-em-voo-canal";

#[tokio::test]
async fn bump_com_canal_aberto_fecha_sem_entregar_evento_em_voo() {
    let hub = temp_hub();
    let fica = paired_phone(&hub).await;
    let sai = paired_phone(&hub).await;
    let sai_dk = crypto::b64(sai.verifying_key().as_bytes());
    let mut p = Phone::direct(&hub, fica);
    assert!(matches!(p.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));

    p.send(json!({"id": 1, "sub": "events", "b": {"since": 0, "workspaceEpoch": 9}})).await;
    let recusa = p.recv().await.unwrap();
    assert_eq!((recusa["id"].as_u64(), &recusa["err"]["status"]), (Some(1), &json!(409)));
    assert_eq!(recusa["err"]["code"], crate::fork::workspace::EPOCH_MISMATCH);
    let recusa = p.call(2, crate::fork::workspace::METHOD, json!({"workspaceEpoch": 9})).await;
    assert_eq!((&recusa["err"]["status"], &recusa["err"]["code"]), (&json!(409), &json!("workspace_epoch_mismatch")));
    let contexto = p.call(3, crate::fork::workspace::METHOD, json!({"workspaceEpoch": 0})).await;
    assert_eq!(contexto["ok"]["role"], "owner");

    p.send(json!({"id": 4, "sub": "events", "b": {"since": hub.store.current_rev(), "workspaceEpoch": 0}})).await;
    let hello = p.recv().await.unwrap();
    assert_eq!((hello["id"].as_u64(), hello["ev"]["type"].as_str()), (Some(4), Some("hello")));
    assert_eq!(hello["ev"]["workspace"]["workspaceEpoch"], 0);

    // Evento produzido e ainda não entregue, revogação de outro aparelho, evento depois.
    let _ = hub.events.send(json!({"type": "usage", "usage": {"marca": MARCA}}));
    assert!(hub.revoke_device(&sai_dk).unwrap());
    let _ = hub.events.send(json!({"type": "usage", "usage": {"marca": MARCA}}));
    let (code, reject) = loop {
        match p.recv().await {
            Ok(frame) => assert!(!frame.to_string().contains(MARCA), "evento depois do bump: {frame}"),
            Err(fim) => break fim,
        }
    };
    assert_eq!(code, 4003);
    assert_eq!((reject["code"].as_str(), reject["stableCode"].as_str()), (Some("revoked"), Some("workspace_revoked")));

    // O aparelho revogado não reabre, nem com a linha devolvida por uma releitura atrasada.
    let mut revogado = Phone::direct(&hub, sai.clone());
    let Out::Msg(reject) = revogado.hello(&hub, false).await else { panic!("expected reject") };
    assert_eq!(
        (reject["code"].as_str(), reject["stableCode"].as_str()),
        (Some("unauthorized"), Some("device_unpaired"))
    );
    hub.store
        .put_device(&Device {
            key: sai_dk,
            name: "Releitura".into(),
            platform: "ios".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control, Scope::Screen],
            lease_until: None,
            created_at: now_ms(),
            last_seen_at: None,
        })
        .unwrap();
    let mut revogado = Phone::direct(&hub, sai);
    let Out::Msg(reject) = revogado.hello(&hub, false).await else { panic!("expected reject") };
    assert_eq!((reject["code"].as_str(), reject["stableCode"].as_str()), (Some("revoked"), Some("device_revoked")));
    assert_eq!(closed(&revogado.next().await), 4003);
}
