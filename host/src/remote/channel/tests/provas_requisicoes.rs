//! Request cancelada não pode tomar o slot de uma assinatura ou request sucessora.
use super::*;

async fn mensagem(canal: &mut Channel, valor: Value) {
    canal.handle(&serde_json::to_vec(&valor).unwrap()).await.unwrap();
}

fn tarefa(canal: &Channel, id: u64) -> AbortHandle {
    match canal.jobs.locked().get(&id) {
        Some(Job::Request(handle) | Job::Sub(handle)) => handle.clone(),
        None => panic!("o slot real deve conter a tarefa"),
    }
}

async fn concluida(handle: &AbortHandle) {
    timeout(Duration::from_secs(60), async {
        while !handle.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("a tarefa deve concluir sem abortar o trabalho");
}

async fn recebida(rx: &mut mpsc::Receiver<ToDevice>) -> Value {
    let pacote = timeout(Duration::from_secs(60), rx.recv()).await.unwrap().unwrap();
    let ToDevice::Inner(valor) = pacote else { panic!("não deve fechar o canal nem enviar pacote externo") };
    valor
}

#[tokio::test]
async fn request_cancelada_conclui_sem_tomar_sub_ou_request_do_mesmo_id() {
    let hub = temp_hub();
    let key = paired_phone(&hub).await;
    let chave = crypto::b64(key.verifying_key().as_bytes());
    let (_, caller) = autoridade::autenticar(&hub, &chave).unwrap();
    let (tx, mut rx) = mpsc::channel(1);
    // O mesmo handle/dispatch e a fila de saída usados após abrir o frame do canal.
    // A identidade vem da autenticação real, não de um Caller fabricado pelo teste.
    let mut canal = Channel {
        hub: hub.clone(),
        tx,
        jobs: Arc::default(),
        key: chave,
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
    let bloqueio = hub.store.team_lock.lock().await;
    mensagem(&mut canal, json!({"id":7,"m":"setRoutinesPaused","b":{"paused":true}})).await;
    let request_a = tarefa(&canal, 7);
    assert!(!request_a.is_finished());
    mensagem(&mut canal, json!({"id":7,"cancel":true})).await;
    assert!(!request_a.is_finished(), "cancelar não aborta a request bloqueada");
    mensagem(&mut canal, json!({"id":7,"sub":"events","b":{"since":hub.store.current_rev(),"client":"ios"}})).await;
    let sub_b = tarefa(&canal, 7);
    assert_ne!(request_a.id(), sub_b.id());
    let hello = recebida(&mut rx).await;
    assert_eq!((hello["id"].as_u64(), hello["ev"]["type"].as_str()), (Some(7), Some("hello")));
    assert!(hub.phone_connected());
    drop(bloqueio);
    concluida(&request_a).await;
    assert_eq!(hub.routines.pause_state()["paused"], true, "o trabalho cancelado deve concluir");
    assert!(matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)), "sem resposta tardia de A");
    assert_eq!(tarefa(&canal, 7).id(), sub_b.id());
    assert!(!sub_b.is_finished(), "B não pode desaparecer sem end/fechamento");
    hub.events.send(json!({"type":"resync","marcador":"evento-legitimo-de-B"})).unwrap();
    let evento = recebida(&mut rx).await;
    assert_eq!((evento["id"].as_u64(), evento["ev"]["marcador"].as_str()), (Some(7), Some("evento-legitimo-de-B")));
    mensagem(&mut canal, json!({"id":7,"cancel":true})).await;
    assert_eq!(recebida(&mut rx).await, json!({"id":7,"end":true}));
    concluida(&sub_b).await;
    assert!(!hub.phone_connected());

    // Mesma variante também exige a identidade: a fila cheia conserva a vaga em espera.
    assert!(canal.tx.try_send(ToDevice::Inner(json!({"sentinela":true}))).is_ok());
    mensagem(&mut canal, json!({"id":7,"m":"routinesPause","b":{}})).await;
    let request_c = tarefa(&canal, 7);
    tokio::task::yield_now().await;
    assert!(!request_c.is_finished(), "a resposta aguarda vaga real na fila");
    mensagem(&mut canal, json!({"id":7,"cancel":true})).await;
    mensagem(&mut canal, json!({"id":7,"m":"setRoutinesPaused","b":{"paused":false}})).await;
    let request_d = tarefa(&canal, 7);
    assert_ne!(request_c.id(), request_d.id());
    assert_eq!(recebida(&mut rx).await, json!({"sentinela":true}));
    assert_eq!(recebida(&mut rx).await, json!({"id":7,"ok":{"paused":false}}));
    concluida(&request_c).await;
    concluida(&request_d).await;
    assert!(!canal.jobs.locked().contains_key(&7));
    assert!(matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)), "C não responde por D");
}
