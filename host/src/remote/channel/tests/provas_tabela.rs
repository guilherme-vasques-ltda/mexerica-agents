//! Prova focal: os ramos próprios do canal (`browserStream*` e `sub`) consultam a tabela de acesso antes das suas
//! guardas, e quem não tem a classe recebe 403.
use super::*;

async fn aparelho_so_tela(hub: &Arc<Hub>) -> Phone {
    let key = SigningKey::from_bytes(&crypto::random());
    hub.store
        .put_device(&Device {
            key: crypto::b64(key.verifying_key().as_bytes()),
            name: "Aparelho de prova".into(),
            platform: "ios".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Screen],
            lease_until: None,
            created_at: now_ms(),
            last_seen_at: None,
        })
        .unwrap();
    let mut p = Phone::direct(hub, key);
    assert!(matches!(p.hello(hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
    p
}

#[tokio::test]
async fn navegador_e_assinatura_recusam_quem_nao_tem_a_classe() {
    let hub = temp_hub();
    let mut p = aparelho_so_tela(&hub).await;
    for (id, metodo) in (1..).zip(browser_canal::METODOS) {
        let resposta = p.call(id, metodo, json!({})).await;
        assert_eq!(resposta["err"]["status"], 403, "{metodo}");
        assert_eq!(resposta["err"]["message"], devices::Forbidden.to_string(), "{metodo}");
    }
    p.send(json!({"id": 10, "sub": "events", "b": {"since": hub.store.current_rev(), "client": "ios"}})).await;
    let resposta = p.recv().await.unwrap();
    assert_eq!((resposta["id"].as_u64(), resposta["err"]["status"].as_u64()), (Some(10), Some(403)));
}

#[tokio::test]
async fn navegador_segue_para_as_guardas_proprias_com_a_classe() {
    let hub = temp_hub();
    let key = paired_phone(&hub).await;
    let mut p = Phone::direct(&hub, key);
    assert!(matches!(p.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
    assert_eq!(p.call(1, "browserStreamAck", json!({})).await["ok"]["acknowledged"], true);
    assert_eq!(p.call(2, "browserStreamInput", json!({})).await["err"]["status"], 400, "guarda própria do navegador");
}
