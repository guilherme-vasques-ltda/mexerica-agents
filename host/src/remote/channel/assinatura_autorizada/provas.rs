//! O cleanup atrasado só pode finalizar o job da própria tarefa.
use super::*;

#[tokio::test]
async fn terminal_cancelado_envia_so_end_e_conserva_transporte() -> Result<()> {
    use crate::{
        remote::identity::Identity,
        store::{Device, DeviceSource, Scope, Store, now_ms},
    };
    let store = Store::open(std::path::Path::new(":memory:"))?;
    let root = store.engine_setup_root.clone();
    std::fs::create_dir_all(&root)?;
    let identity = Identity::load_or_create(&root)?;
    let hub = Hub::new(store, "fixture".into(), identity, "fixture".into(), 0);
    let key = crate::remote::crypto::b64(&[17; 32]);
    hub.store.put_device(&Device {
        key: key.clone(),
        name: "Fixture".into(),
        platform: "web".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: now_ms(),
        last_seen_at: None,
    })?;
    let (_, caller) = autoridade::autenticar(&hub, &key).map_err(|_| anyhow::anyhow!("fixture sem autoridade"))?;
    let login = crate::fork::terminal_login::Login::fixture(&hub, &Caller::Local, "cursor", "/fixture".into())?;
    login.stop();
    let jobs = Arc::new(Mutex::new(HashMap::new()));
    let (tx, mut rx) = mpsc::channel(2);
    let (start, ready) = tokio::sync::oneshot::channel();
    let running = jobs.clone();
    let host = hub.clone();
    let task = tokio::spawn(async move {
        ready.await.unwrap();
        executar(
            7,
            futures::stream::iter([json!({"type":"output","data":"nao-entregar"})]).boxed(),
            host,
            Arc::new(caller),
            running,
            tx,
            Some(login),
        )
        .await;
    });
    jobs.locked().insert(7, Job::Sub(task.abort_handle()));
    start.send(()).unwrap();
    assert!(matches!(rx.recv().await, Some(ToDevice::Inner(value)) if value == json!({"id":7,"end":true})));
    task.await?;
    assert!(rx.recv().await.is_none(), "sem saída antiga nem Close do canal");
    assert!(jobs.locked().is_empty());
    let _ = std::fs::remove_dir_all(root);
    Ok(())
}

#[tokio::test]
async fn cleanup_antigo_nao_remove_nem_fecha_assinatura_do_id_reutilizado() -> Result<()> {
    let antiga = tokio::spawn(std::future::pending::<()>());
    let nova = tokio::spawn(std::future::pending::<()>());
    let jobs = Mutex::new(HashMap::from([(7, Job::Sub(antiga.abort_handle()))]));
    let (tx, mut rx) = mpsc::channel(1);
    let vaga_antiga = tx.reserve().await?;
    let Some(Job::Sub(cancelada)) = jobs.locked().remove(&7) else { panic!("job original ausente") };
    cancelada.abort();
    jobs.locked().insert(7, Job::Sub(nova.abort_handle()));

    finalizar(&jobs, 7, antiga.id(), Some(vaga_antiga));
    assert!(propria(&jobs.locked(), 7, nova.id()));
    assert!(matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
    finalizar(&jobs, 7, nova.id(), Some(tx.reserve().await?));
    assert!(jobs.locked().is_empty());
    assert!(matches!(rx.recv().await, Some(ToDevice::Close(close::REKEY, "subscription authorization ended"))));
    nova.abort();
    assert!(matches!(antiga.await, Err(erro) if erro.is_cancelled()));
    assert!(matches!(nova.await, Err(erro) if erro.is_cancelled()));
    Ok(())
}
