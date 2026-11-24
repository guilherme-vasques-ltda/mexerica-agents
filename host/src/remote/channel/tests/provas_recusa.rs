use super::*;

#[tokio::test]
async fn fato_mudou_entre_a_admissao_e_a_assinatura() {
    let hub = temp_hub();
    let dk = paired_phone(&hub).await.verifying_key().to_bytes();
    let chave = crypto::b64(&dk);
    let aparelho = hub.store.device(&chave).unwrap();
    assert!(hub.store.remove_device(&chave).unwrap());
    let recusa = fork_assinatura_terminal::Recusa {
        cid: hub.identity.cid_raw(),
        dk,
        ek_d: [5; 32],
        n: [6; 32],
        estavel: fork_recusa_hello::fato(&hub.store.db.locked(), &chave).unwrap().unwrap(),
    };
    let prova = || fork_recusa_hello::prova(&hub.store.db.locked(), &recusa, &hub.identity);
    assert!(prova().is_some());
    hub.store.put_device(&aparelho).unwrap();
    assert!(prova().is_none());
}

async fn recusa(hub: &Arc<Hub>, key: &SigningKey, valido: bool, pair: bool) -> (Value, u16, [u8; 32], [u8; 32]) {
    let mut p = Phone::direct(hub, key.clone());
    let cid = hub.identity.cid_raw();
    let dk = key.verifying_key().to_bytes();
    let ek = crypto::x25519_pub(&StaticSecret::from(crypto::random::<32>()));
    let n = crypto::random::<32>();
    let mut sig = crypto::sign(key, &crypto::hs1_input(&cid, &dk, &ek, &n));
    if !valido {
        sig[0] ^= 1;
    }
    p.tx.send(json!({"t":"hello", "v":1, "dk":crypto::b64(&dk), "ek":crypto::b64(&ek),
        "n":crypto::b64(&n), "sig":crypto::b64(&sig), "pair":pair}))
        .await
        .unwrap();
    let Out::Msg(reject) = p.next().await else { panic!("expected reject") };
    assert_eq!(reject["t"], "reject");
    let close = closed(&p.next().await);
    p.run.await.unwrap();
    (reject, close, ek, n)
}

#[tokio::test]
async fn fatos_lidos_e_posse_geram_recusa_assinada() {
    let hub = temp_hub();
    let revogada = paired_phone(&hub).await;
    let despareada = paired_phone(&hub).await;
    let revogada_dk = crypto::b64(revogada.verifying_key().as_bytes());
    let aparelho = hub.store.device(&revogada_dk).unwrap();
    assert!(hub.revoke_device(&revogada_dk).unwrap());
    assert!(hub.store.remove_device(&crypto::b64(despareada.verifying_key().as_bytes())).unwrap());

    for (key, code, stable, byte, close) in [
        (&revogada, "revoked", "device_revoked", 1, 4003),
        (&despareada, "unauthorized", "device_unpaired", 4, 4001),
        (&revogada, "revoked", "device_revoked", 1, 4003),
    ] {
        let (rejeicao, fechamento, ek, n) = recusa(&hub, key, true, false).await;
        assert_eq!((rejeicao["code"].as_str(), rejeicao["stableCode"].as_str()), (Some(code), Some(stable)));
        assert_eq!(fechamento, close);
        assert_eq!(rejeicao["prova"]["v"], 1);
        assert_eq!(rejeicao["prova"]["tipo"], "recusa");
        let sig = crypto::unb64_n(rejeicao["prova"]["sig"].as_str().unwrap()).unwrap();
        let mut bytes = b"codync/recusa/v1".to_vec();
        bytes.extend(hub.identity.cid_raw());
        bytes.extend(key.verifying_key().to_bytes());
        bytes.extend(ek);
        bytes.extend(n);
        bytes.push(byte);
        crypto::verify(&hub.identity.sign_pub(), &bytes, &sig).unwrap();
        if close == 4003 {
            hub.store.put_device(&aparelho).unwrap();
        }
    }
}

#[tokio::test]
async fn recusa_sem_prova_para_chave_desconhecida_hs1_invalido_ou_leitura_incerta() {
    let hub = temp_hub();
    let revogada = paired_phone(&hub).await;
    let despareada = paired_phone(&hub).await;
    assert!(hub.revoke_device(&crypto::b64(revogada.verifying_key().as_bytes())).unwrap());
    assert!(hub.store.remove_device(&crypto::b64(despareada.verifying_key().as_bytes())).unwrap());
    let desconhecida = SigningKey::from_bytes(&crypto::random());
    for (key, pair, code) in [
        (&revogada, false, "badSignature"),
        (&despareada, false, "badSignature"),
        (&desconhecida, false, "unauthorized"),
        (&desconhecida, true, "pairingClosed"),
    ] {
        let (rejeicao, _, _, _) = recusa(&hub, key, false, pair).await;
        assert_eq!(rejeicao["code"], code);
        assert!(rejeicao.get("prova").is_none());
    }
    hub.store.db.locked().execute("DROP TABLE fork_workspace_revoked_devices", []).unwrap();
    for key in [&revogada, &despareada] {
        let (rejeicao, fechamento, _, _) = recusa(&hub, key, true, false).await;
        assert_eq!(rejeicao["code"], "unauthorized");
        assert_eq!(fechamento, 4001);
        assert!(rejeicao.get("prova").is_none());
    }
}

#[tokio::test(start_paused = true)]
async fn teto_antes_da_autenticacao_nao_assina_nem_readmite() {
    let hub = temp_hub();
    let chave = paired_phone(&hub).await;
    assert!(hub.revoke_device(&crypto::b64(chave.verifying_key().as_bytes())).unwrap());
    for _ in 1..fork_limite_hello::POR_CHAVE {
        assert!(recusa(&hub, &chave, true, false).await.0.get("prova").is_some());
    }
    let (rejeicao, fechamento, _, _) = recusa(&hub, &chave, true, false).await;
    assert_eq!((rejeicao["code"].as_str(), fechamento), (Some("rateLimited"), 4008));
    assert!(rejeicao.get("prova").is_none());
    tokio::time::advance(fork_limite_hello::JANELA).await;
    let (rejeicao, fechamento, _, _) = recusa(&hub, &chave, true, false).await;
    assert_eq!((rejeicao["code"].as_str(), fechamento), (Some("revoked"), 4003));
    assert!(rejeicao.get("prova").is_some());
}
