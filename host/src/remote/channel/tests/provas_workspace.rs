//! Prova do `/channel`: evento em voo não sai depois do bump, o canal fecha sem dados
//! após o corte, o aparelho revogado não reabre canal, e só o pareamento por código no fio readmite.
use super::*;

const MARCA: &str = "evento-em-voo-canal";

#[tokio::test]
async fn anuncio_selado_vale_apenas_no_canal_que_o_enviou() {
    let hub = temp_hub();
    let key = paired_phone(&hub).await;
    let mut anuncia = Phone::direct(&hub, key.clone());
    let mut legado = Phone::direct(&hub, key.clone());
    for phone in [&mut anuncia, &mut legado] {
        assert!(matches!(phone.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
    }
    assert!(anuncia.call(1, "hello", json!({"corte":1})).await["ok"].is_object());
    assert!(legado.call(1, "hello", json!({"corte":"1"})).await["ok"].is_object());
    crate::fork::workspace::bump(&hub, crate::fork::workspace::Motivo::RoleChanged).unwrap();
    assert_eq!(anuncia.recv().await.unwrap_err(), (close::TIMEOUT, Value::Null));
    assert_eq!(legado.recv().await.unwrap_err(), (close::REKEY, Value::Null));
    let mut nova = Phone::direct(&hub, key);
    assert!(matches!(nova.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
    crate::fork::workspace::bump(&hub, crate::fork::workspace::Motivo::RoleChanged).unwrap();
    assert_eq!(nova.recv().await.unwrap_err(), (close::REKEY, Value::Null));
}

#[tokio::test]
async fn geracao_alterada_ou_banco_ilegivel_fecha_sem_prova() {
    for leitura_falha in [false, true] {
        let hub = temp_hub();
        let key = paired_phone(&hub).await;
        let mut phone = Phone::direct(&hub, key);
        assert!(matches!(phone.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
        if leitura_falha {
            hub.store.db.locked().execute_batch("CREATE TEMP TABLE devices(unreadable TEXT)").unwrap();
        } else {
            let mut device = hub.store.device(&phone.dk()).unwrap();
            device.scopes = vec![Scope::Control];
            hub.store.put_device(&device).unwrap();
        }
        crate::fork::workspace::bump(&hub, crate::fork::workspace::Motivo::RoleChanged).unwrap();
        assert_eq!(phone.recv().await.unwrap_err(), (close::TIMEOUT, Value::Null));
    }
}

#[tokio::test]
async fn um_bump_permite_reconexao_de_tres_aparelhos_distintos() {
    use crate::fork::workspace::{self, Motivo};
    let hub = temp_hub();
    let mut keys = Vec::new();
    let mut canais = Vec::new();
    for _ in 0..3 {
        let key = paired_phone(&hub).await;
        let mut phone = Phone::direct(&hub, key.clone());
        assert!(matches!(phone.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
        keys.push(key);
        canais.push(phone);
    }
    let before = hub.workspace.atual();
    assert_eq!(workspace::bump(&hub, Motivo::RoleChanged).unwrap(), before + 1);
    for (key, mut anterior) in keys.into_iter().zip(canais) {
        assert_eq!(anterior.recv().await.unwrap_err(), (close::REKEY, Value::Null));
        let mut nova = Phone::direct(&hub, key);
        assert!(matches!(nova.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
    }
    assert_eq!(hub.workspace.atual(), before + 1);
}

#[tokio::test]
async fn teto_de_bumps_limita_reconexao_sem_atrasar_corte() {
    use crate::fork::workspace::{self, LIMITE_BUMPS_POR_MINUTO, Motivo};
    let hub = temp_hub();
    let key = paired_phone(&hub).await;
    for _ in 1..LIMITE_BUMPS_POR_MINUTO {
        workspace::bump(&hub, Motivo::RoleChanged).unwrap();
    }
    let mut ativa = Phone::direct(&hub, key.clone());
    assert!(matches!(ativa.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
    let before = hub.workspace.atual();
    assert_eq!(workspace::bump(&hub, Motivo::RoleChanged).unwrap(), before + 1);
    assert_eq!(ativa.recv().await.unwrap_err(), (close::REKEY, Value::Null));
    assert_eq!(workspace::bump(&hub, Motivo::RoleChanged).unwrap(), before + 2);
    let mut tenta = Phone::direct(&hub, key);
    assert!(matches!(tenta.hello(&hub, false).await, Out::Msg(v) if v["code"] == "rateLimited"));
    assert_eq!(closed(&tenta.next().await), 4008);
}

#[tokio::test]
async fn bump_com_canal_aberto_fecha_sem_entregar_evento_em_voo() {
    let hub = temp_hub();
    let fica = paired_phone(&hub).await;
    let fica_dk = crypto::b64(fica.verifying_key().as_bytes());
    let generation = autoridade::Autoridade::geracao_atual(&hub.store.db.locked(), &fica_dk).unwrap();
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
    assert_eq!(contexto["ok"]["grantGeneration"], generation);

    p.send(json!({"id": 4, "sub": "events", "b": {"since": hub.store.current_rev(), "workspaceEpoch": 0}})).await;
    let hello = p.recv().await.unwrap();
    assert_eq!((hello["id"].as_u64(), hello["ev"]["type"].as_str()), (Some(4), Some("hello")));
    assert_eq!(hello["ev"]["workspace"]["workspaceEpoch"], 0);
    assert_eq!(hello["ev"]["workspace"]["grantGeneration"], contexto["ok"]["grantGeneration"]);

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
    assert_eq!((code, reject), (close::REKEY, Value::Null));

    // O aparelho revogado não reabre, nem com a linha devolvida por uma releitura atrasada.
    // Sem linha, o revogado só é nomeado depois de a assinatura provar a posse da chave.
    let mut revogado = Phone::direct(&hub, sai.clone());
    let Out::Msg(reject) = revogado.hello(&hub, false).await else { panic!("expected reject") };
    assert_eq!((reject["code"].as_str(), reject["stableCode"].as_str()), (Some("revoked"), Some("device_revoked")));
    assert_eq!(closed(&revogado.next().await), 4003);
    // Quem só conhece a chave pública não descobre que ela foi revogada.
    let mut impostor = Phone::direct(&hub, sai.clone());
    let ek_pub = crypto::x25519_pub(&StaticSecret::from(crypto::random::<32>()));
    let n = crypto::random::<32>();
    let falsa = SigningKey::from_bytes(&crypto::random());
    let sig =
        crypto::sign(&falsa, &crypto::hs1_input(&hub.identity.cid_raw(), &sai.verifying_key().to_bytes(), &ek_pub, &n));
    impostor
        .tx
        .send(json!({"t": "hello", "v": 1, "dk": sai_dk, "ek": crypto::b64(&ek_pub), "n": crypto::b64(&n),
            "sig": crypto::b64(&sig), "pair": false}))
        .await
        .unwrap();
    let Out::Msg(reject) = impostor.next().await else { panic!("expected reject") };
    let mut desconhecido = Phone::direct(&hub, falsa);
    let Out::Msg(recusa_desconhecida) = desconhecido.hello(&hub, false).await else { panic!("expected reject") };
    assert_eq!(reject, recusa_desconhecida);
    assert!(reject.get("prova").is_none());
    assert_eq!(closed(&impostor.next().await), closed(&desconhecido.next().await));
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

#[tokio::test]
async fn escritor_com_fila_ocupada_substitui_o_fechamento_antigo() {
    let hub = temp_hub();
    let fica = crypto::b64(paired_phone(&hub).await.verifying_key().as_bytes());
    let sai = crypto::b64(paired_phone(&hub).await.verifying_key().as_bytes());
    let (tx, rx) = mpsc::channel(256);
    let (out, mut transporte) = mpsc::channel(1);
    let (_, sem_prazo) = tokio::sync::watch::channel(None);
    let (_, caller) = autoridade::autenticar(&hub, &fica).unwrap();
    let barreira = Barreira::nova(hub.clone(), caller, hub.workspace.assinar(), hub.workspace.atual(), sem_prazo, None);
    let escritor = tokio::spawn(write_frames(Sealer::new([7; 32]), rx, out, Some(barreira)));
    // Eventos que já passaram pelo filtro de autoridade, e o transporte parado.
    for n in 0..64 {
        tx.send(ToDevice::Inner(json!({"id": 1, "ev": {"marca": MARCA, "n": n}}))).await.unwrap();
    }
    tx.send(ToDevice::Close(close::REKEY, "subscription authorization ended")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(hub.revoke_device(&sai).unwrap());
    let mut saida = vec![];
    while let Some(frame) = timeout(Duration::from_secs(5), transporte.recv()).await.unwrap() {
        saida.push(frame);
    }
    escritor.await.unwrap();
    let quadros = saida.iter().filter(|o| matches!(o, Out::Msg(v) if v["t"] == "f")).count();
    assert!(quadros <= 1, "só o quadro já entregue ao transporte antes do bump sai: {quadros}");
    assert!(matches!(saida.last(), Some(Out::Close(close::REKEY, "rekey"))));
    assert!(saida.iter().all(|o| !matches!(o, Out::Msg(v) if v["t"] != "f")));
}

#[tokio::test]
async fn porta_espera_o_commit_em_curso_e_so_corta_quando_o_numero_avanca() {
    let hub = temp_hub();
    let fica = crypto::b64(paired_phone(&hub).await.verifying_key().as_bytes());
    let sai = crypto::b64(paired_phone(&hub).await.verifying_key().as_bytes());
    let (entrada, rx) = mpsc::channel(8);
    let mut saida = Box::pin(Porta::armada(&hub, &fica).filtrar(rx));
    let marca = hub.workspace.marcar_para_prova();
    entrada.send(Out::Msg(json!({"t": "f", "marca": MARCA}))).await.unwrap();
    assert!(timeout(Duration::from_millis(100), saida.next()).await.is_err(), "com um commit em curso nada sai");
    drop(marca);
    let Some(Out::Msg(quadro)) = timeout(Duration::from_secs(5), saida.next()).await.unwrap() else {
        panic!("o quadro sai depois do rollback")
    };
    assert_eq!(quadro["marca"], MARCA, "rollback não emite terminal: o canal segue aberto");

    assert!(hub.revoke_device(&sai).unwrap());
    entrada.send(Out::Msg(json!({"t": "f", "marca": MARCA}))).await.unwrap();
    assert!(matches!(
        timeout(Duration::from_secs(5), saida.next()).await.unwrap(),
        Some(Out::Close(close::REKEY, "rekey"))
    ));
    assert!(saida.next().await.is_none());
}

#[tokio::test]
async fn bump_durante_a_autenticacao_fecha_o_canal_logo_depois_do_welcome() {
    let hub = temp_hub();
    let fica = paired_phone(&hub).await;
    let sai = crypto::b64(paired_phone(&hub).await.verifying_key().as_bytes());
    let mut p = Phone::direct(&hub, fica);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(hub.revoke_device(&sai).unwrap());
    assert!(matches!(p.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
    let (code, reject) = p.recv().await.unwrap_err();
    assert_eq!((code, reject), (close::REKEY, Value::Null));
}

/// Lê quadros crus até o fechamento; nenhum quadro cifrado pode sair depois do bump.
async fn fecha_sem_quadro(p: &mut Phone) {
    assert!(matches!(p.next().await, Out::Close(close::REKEY, "rekey")));
}

#[tokio::test]
async fn terminal_no_canal_confere_o_epoch_e_para_no_bump() {
    let hub = temp_hub();
    let fica = paired_phone(&hub).await;
    let sai = crypto::b64(paired_phone(&hub).await.verifying_key().as_bytes());
    let saida = hub.terms.fixture("term-canal", Some(&crypto::b64(fica.verifying_key().as_bytes())));
    let mut p = Phone::direct(&hub, fica);
    assert!(matches!(p.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));

    p.send(json!({"id": 1, "sub": "term", "b": {"term": "term-canal", "workspaceEpoch": 9}})).await;
    let recusa = p.recv().await.unwrap();
    assert_eq!((&recusa["err"]["status"], &recusa["err"]["code"]), (&json!(409), &json!("workspace_epoch_mismatch")));

    p.send(json!({"id": 2, "sub": "term", "b": {"term": "term-canal", "workspaceEpoch": 0}})).await;
    // A resposta seguinte prova que a assinatura anterior já foi registrada.
    assert_eq!(p.call(3, crate::fork::workspace::METHOD, json!({})).await["ok"]["workspaceEpoch"], 0);
    let _ = saida.send(json!({"type": "output", "data": MARCA}));
    assert!(hub.revoke_device(&sai).unwrap());
    let _ = saida.send(json!({"type": "output", "data": MARCA}));
    fecha_sem_quadro(&mut p).await;
}

#[tokio::test]
async fn navegador_no_canal_confere_o_epoch_e_para_no_bump() {
    use crate::fork::browser::stream::{Frame, Size, canal};
    let hub = temp_hub();
    let bot: crate::store::BotConfig =
        serde_json::from_value(json!({"id": "bot-nav", "name": "Nav", "spaceId": crate::fork::spaces::PERSONAL}))
            .unwrap();
    hub.store.save_bot(&bot).unwrap();
    let fica = paired_phone(&hub).await;
    let sai = crypto::b64(paired_phone(&hub).await.verifying_key().as_bytes());
    let mut p = Phone::direct(&hub, fica);
    assert!(matches!(p.hello_caps(&hub).await, Out::Msg(v) if v["t"] == "welcome"));
    let sessao = format!("sess-{}", uuid::Uuid::new_v4());
    let inscricao = json!({"hubId": hub.host_id, "spaceId": crate::fork::spaces::PERSONAL, "botId": "bot-nav",
        "session": sessao, "target": "aba-1"});
    let mut velha = inscricao.clone();
    velha["workspaceEpoch"] = json!(9);
    let recusa = p.call(1, "browserStreamSubscribe", velha).await;
    assert_eq!((&recusa["err"]["status"], &recusa["err"]["code"]), (&json!(409), &json!("workspace_epoch_mismatch")));
    let mut atual = inscricao;
    atual["workspaceEpoch"] = json!(0);
    let aceita = p.call(2, "browserStreamSubscribe", atual).await;
    assert_eq!(aceita["ok"]["subscribed"], true, "{aceita}");

    let quadro = |sequence| Frame {
        session: sessao.clone(),
        target: "aba-1".into(),
        sequence,
        target_generation: 0,
        image: Size { width: 960, height: 540 },
        viewport: Size { width: 1280, height: 720 },
        jpeg: vec![0xff, 0xd8, 1, 0xff, 0xd9],
    };
    assert!(canal::oferecer(quadro(1)));
    loop {
        if let Out::Msg(f) = p.next().await
            && f["t"] == "f"
            && p.opener.as_mut().unwrap().open(&f).unwrap().is_some()
        {
            break;
        }
    }
    let _ = p.call(3, "browserStreamAck", json!({})).await;
    assert!(canal::oferecer(quadro(2)));
    assert!(hub.revoke_device(&sai).unwrap());
    fecha_sem_quadro(&mut p).await;
}

#[tokio::test]
async fn aparelho_revogado_volta_so_pelo_pareamento_por_codigo_no_fio() {
    let hub = temp_hub();
    let chave = paired_phone(&hub).await;
    let dk = crypto::b64(chave.verifying_key().as_bytes());
    assert!(hub.revoke_device(&dk).unwrap());
    let mut p = Phone::direct(&hub, chave.clone());
    assert!(matches!(p.hello(&hub, false).await, Out::Msg(v) if v["code"] == "revoked"));

    let code = hub.pairing.locked().issue().code;
    let mut p = Phone::direct(&hub, chave.clone());
    assert!(matches!(p.hello(&hub, true).await, Out::Msg(v) if v["t"] == "welcome"));
    p.send(json!({"id": 1, "m": "pair", "b": {"code": code, "name": "Telefone", "platform": "ios"}})).await;
    assert!(p.recv().await.unwrap()["ok"].is_object());
    assert_eq!(p.recv().await.unwrap_err().0, close::PAIRED);

    let mut p = Phone::direct(&hub, chave);
    assert!(matches!(p.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"), "o código readmite");
    assert_eq!(p.call(2, crate::fork::workspace::METHOD, json!({})).await["ok"]["workspaceEpoch"], 1);
}

/// Fila real do transporte (64) cheia de quadros já liberados pelo escritor, com um 4011 no fim.
async fn fila_cheia() -> (mpsc::Sender<Out>, mpsc::Receiver<Out>) {
    let (saida, fila) = mpsc::channel::<Out>(64);
    for n in 0..63 {
        saida.send(Out::Msg(json!({"t": "f", "marca": MARCA, "n": n}))).await.unwrap();
    }
    saida.send(Out::Close(close::REKEY, "subscription authorization ended")).await.unwrap();
    assert_eq!(saida.capacity(), 0, "a fila do transporte está cheia");
    (saida, fila)
}

#[tokio::test]
async fn socket_direto_com_fila_cheia_so_entrega_o_terminal_depois_do_bump() {
    let hub = temp_hub();
    let fica = crypto::b64(paired_phone(&hub).await.verifying_key().as_bytes());
    let sai = crypto::b64(paired_phone(&hub).await.verifying_key().as_bytes());
    let (_saida, cheia) = fila_cheia().await;
    let porta = Porta::armada(&hub, &fica);
    assert!(hub.revoke_device(&sai).unwrap());
    let (socket, enviados) = futures::channel::mpsc::unbounded::<Message>();
    timeout(Duration::from_secs(5), write_socket(socket, porta.filtrar(cheia))).await.unwrap();
    let enviados: Vec<Message> = enviados.collect().await;
    let [Message::Close(Some(fim))] = enviados.as_slice() else { panic!("só o terminal pode sair: {enviados:?}") };
    assert_eq!((fim.code, fim.reason.as_str()), (close::REKEY, "rekey"));
}

#[tokio::test]
async fn porta_entrega_o_terminal_da_revogacao_confirmada_com_o_dono_retido() {
    let hub = temp_hub();
    let fica = crypto::b64(paired_phone(&hub).await.verifying_key().as_bytes());
    let sai = crypto::b64(paired_phone(&hub).await.verifying_key().as_bytes());
    crate::remote::cloud::update_status(&hub, |s| {
        s.owner = Some(crate::remote::cloud::Owner { user_id: "user_a.example.invalid".into(), email: None });
    });
    let (entrada, rx) = mpsc::channel(8);
    let mut saida = Box::pin(Porta::armada(&hub, &fica).filtrar(rx));
    hub.store
        .db
        .locked()
        .execute_batch(
            "CREATE TEMP TRIGGER dono_nao_grava BEFORE UPDATE ON main.kv WHEN NEW.k='fork.workspace.owner_seen'
             BEGIN SELECT RAISE(ABORT, 'falha de gravação de prova'); END;",
        )
        .unwrap();
    crate::remote::cloud::update_status(&hub, |s| {
        s.owner = Some(crate::remote::cloud::Owner { user_id: "user_b.example.invalid".into(), email: None });
    });
    entrada.send(Out::Msg(json!({"t": "f", "marca": MARCA}))).await.unwrap();
    assert!(timeout(Duration::from_millis(100), saida.next()).await.is_err(), "dono retido: nada sai");

    assert!(hub.revoke_device(&sai).unwrap());
    assert!(hub.workspace.retrato().pendente(), "a retenção do dono continua");
    assert!(matches!(
        timeout(Duration::from_secs(5), saida.next()).await.unwrap(),
        Some(Out::Close(close::TIMEOUT, "tryAgain"))
    ));
    assert!(saida.next().await.is_none());
    hub.store.db.locked().execute_batch("DROP TRIGGER dono_nao_grava").unwrap();
}

#[tokio::test]
async fn assinatura_sem_a_classe_recebe_403_antes_do_epoch() {
    let hub = temp_hub();
    let chave = paired_phone(&hub).await;
    let dk = crypto::b64(chave.verifying_key().as_bytes());
    let mut aparelho = hub.store.device(&dk).unwrap();
    aparelho.scopes = vec![Scope::OwnerDevices];
    hub.store.put_device(&aparelho).unwrap();
    let mut p = Phone::direct(&hub, chave);
    assert!(matches!(p.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
    for (id, epoch) in [(1, 9), (2, 0)] {
        p.send(json!({"id": id, "sub": "events", "b": {"since": 0, "workspaceEpoch": epoch}})).await;
        let recusa = p.recv().await.unwrap();
        assert_eq!((recusa["id"].as_u64(), &recusa["err"]["status"]), (Some(id), &json!(403)), "{recusa}");
        assert!(recusa["err"]["code"].is_null(), "sem a classe, o epoch não é conferido: {recusa}");
    }
}
