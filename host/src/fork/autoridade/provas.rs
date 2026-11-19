//! Provas focais do produtor; entradas vêm da autenticação usada pelos transportes.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::{
    remote::{cloud, identity::Identity},
    store::Store,
};
use serde_json::json;
use std::path::PathBuf;

struct Ambiente {
    hub: Arc<Hub>,
    pasta: PathBuf,
}
impl Ambiente {
    fn novo() -> Self {
        let pasta = std::env::temp_dir().join(format!("donna-autoridade-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&pasta).unwrap();
        let store = Store::open(&pasta.join("estado.db")).unwrap();
        let identity = Identity::load_or_create(&pasta).unwrap();
        let hub = Hub::new(store, "hub-teste".into(), identity, "token-teste".into(), 0);
        Self { hub, pasta }
    }
    fn aparelho(&self, origem: DeviceSource) -> Device {
        let aparelho = Device {
            key: "aparelho-teste".into(),
            name: "Teste".into(),
            platform: "teste".into(),
            source: origem,
            grant_id: (origem == DeviceSource::Account).then(|| "concessao-teste".into()),
            scopes: vec![Scope::Control],
            lease_until: (origem == DeviceSource::Account).then(|| now_ms() + 60_000),
            created_at: now_ms(),
            last_seen_at: None,
        };
        if origem == DeviceSource::Account {
            cloud::update_status(&self.hub, |s| {
                s.owner = Some(cloud::Owner { user_id: "dono-teste".into(), email: None });
            });
            self.hub.cloud_synced.store(true, Ordering::Release);
        }
        self.hub.store.put_device(&aparelho).unwrap();
        aparelho
    }
    fn geracao(&self, chave: &str) -> i64 {
        concessao::geracao(&self.hub.store.db.locked(), chave).unwrap()
    }
}
impl Drop for Ambiente {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.pasta);
    }
}

#[test]
fn local_e_device_control_gravam_sem_capacidade_de_contas() {
    let a = Ambiente::novo();
    let local = Autoridade::capturar(&a.hub, &Caller::Local).unwrap();
    local
        .with_db(&a.hub, None, |db| {
            db.execute("INSERT INTO kv(k,v) VALUES('c1-local','ok')", [])?;
            Ok(())
        })
        .unwrap();
    let aparelho = a.aparelho(DeviceSource::Local);
    let (_, caller) = autenticar(&a.hub, &aparelho.key).unwrap();
    let original = Autoridade::capturar(&a.hub, &caller).unwrap();
    a.hub.cloud_synced.store(true, Ordering::Release);
    a.hub.cloud_synced.store(false, Ordering::Release);
    local.with_db(&a.hub, None, |_| Ok(())).unwrap();
    original
        .with_db(&a.hub, None, |db| {
            db.execute("INSERT INTO kv(k,v) VALUES('c1-device','ok')", [])?;
            Ok(())
        })
        .unwrap();
    a.hub.cloud_synced.store(true, Ordering::Release);
    original.with_db(&a.hub, None, |_| Ok(())).unwrap();
    assert!(crate::fork::engine_setup::access::resolve(&a.hub, &caller).is_err());
    assert!(original.with_db(&a.hub, Some(&Authority::Local), |_| Ok(())).is_err());
    assert!(crate::api::devices::permit(&caller, "setCloud").is_err());
    assert!(matches!(caller, Caller::Device { .. }));
    assert_eq!(a.hub.store.kv_read("c1-device").unwrap().as_deref(), Some("ok"));
    let sem_transporte = Caller::Device { key: aparelho.key, scopes: vec![Scope::Control], autoridade: None };
    assert!(Autoridade::capturar(&a.hub, &sem_transporte).is_err());
}

#[tokio::test]
async fn auth_e_dispatch_recusam_aba_antes_da_captura_do_consumidor() {
    let a = Ambiente::novo();
    let aparelho = a.aparelho(DeviceSource::Local);
    let (_, caller) = autenticar(&a.hub, &aparelho.key).unwrap();
    let notificacao = a.hub.auth.subscribe();
    // O Caller autenticado fica na fila ANTES de o consumidor chamar capturar.
    let (liberar, fila) = tokio::sync::oneshot::channel();
    let hub = a.hub.clone();
    let pedido = tokio::spawn(async move {
        fila.await.unwrap();
        crate::api::dispatch(&hub, &caller, "createBot", json!({"name":"Não criar","backend":"codex"})).await
    });
    assert!(a.hub.revoke_device(&aparelho.key).unwrap());
    a.hub.store.put_device(&aparelho).unwrap();
    a.hub.auth_changed();
    assert!(notificacao.has_changed().unwrap());
    liberar.send(()).unwrap();
    assert!(pedido.await.unwrap().is_err());
    assert!(a.hub.store.bots().unwrap().is_empty());
    let (_, atual) = autenticar(&a.hub, &aparelho.key).unwrap();
    assert!(Autoridade::capturar(&a.hub, &atual).is_ok());
}

#[test]
fn revogar_e_reconceder_nao_executa_callback_antigo() {
    let a = Ambiente::novo();
    let aparelho = a.aparelho(DeviceSource::Local);
    let (_, caller) = autenticar(&a.hub, &aparelho.key).unwrap();
    let original = Autoridade::capturar(&a.hub, &caller).unwrap();
    a.hub.store.remove_device(&aparelho.key).unwrap();
    a.hub.store.put_device(&aparelho).unwrap();
    let mut executou = false;
    assert!(
        original
            .with_db(&a.hub, None, |_| {
                executou = true;
                Ok(())
            })
            .is_err()
    );
    assert!(!executou);
    assert!(reconferir(&a.hub, &caller).is_err(), "canal não adota outra concessão");
}

#[test]
fn mudancas_durante_fila_recusam_antes_da_escrita() {
    for mudanca in ["origem", "concessao", "escopos", "dono", "prazo"] {
        let a = Ambiente::novo();
        let aparelho = a.aparelho(DeviceSource::Account);
        let (_, caller) = autenticar(&a.hub, &aparelho.key).unwrap();
        let original = Autoridade::capturar(&a.hub, &caller).unwrap();
        match mudanca {
            "origem" => {
                a.hub.store.db.locked().execute("UPDATE devices SET source='local'", []).unwrap();
            }
            "concessao" => {
                a.hub.store.db.locked().execute("UPDATE devices SET grant_id='outra'", []).unwrap();
            }
            "escopos" => {
                a.hub.store.db.locked().execute("UPDATE devices SET scopes='[]'", []).unwrap();
            }
            "dono" => {
                let epoca = a.hub.cloud.with_transport_owner(|_, epoca| Ok(epoca)).unwrap();
                cloud::update_status(&a.hub, |_| {});
                assert_eq!(a.hub.cloud.with_transport_owner(|_, epoca| Ok(epoca)).unwrap(), epoca);
                cloud::update_status(&a.hub, |s| s.owner = None);
                assert_eq!(a.hub.cloud.with_transport_owner(|_, epoca| Ok(epoca)).unwrap(), epoca + 1);
                cloud::update_status(&a.hub, |s| {
                    s.owner = Some(cloud::Owner { user_id: "dono-teste".into(), email: None });
                });
                assert_eq!(a.hub.cloud.with_transport_owner(|_, epoca| Ok(epoca)).unwrap(), epoca + 2);
            }
            _ => {
                a.hub.store.set_lease(&aparelho.key, now_ms() - 1).unwrap();
            }
        }
        let mut executou = false;
        assert!(
            original
                .with_db(&a.hub, None, |_| {
                    executou = true;
                    Ok(())
                })
                .is_err(),
            "{mudanca}"
        );
        assert!(!executou, "{mudanca}");
    }
}

#[test]
fn revalidacao_antes_do_commit_reverte_efeito_e_contador() {
    let a = Ambiente::novo();
    let aparelho = a.aparelho(DeviceSource::Local);
    let (_, caller) = autenticar(&a.hub, &aparelho.key).unwrap();
    let original = Autoridade::capturar(&a.hub, &caller).unwrap();
    let geracao = a.geracao(&aparelho.key);
    assert!(
        original
            .with_db(&a.hub, None, |db| {
                db.execute("INSERT INTO kv(k,v) VALUES('c1-nao-confirmar','efeito')", [])?;
                db.execute("UPDATE devices SET scopes='[]'", [])?;
                Ok(())
            })
            .is_err()
    );
    assert!(a.hub.store.kv_read("c1-nao-confirmar").unwrap().is_none());
    assert_eq!(a.geracao(&aparelho.key), geracao);
    original.with_db(&a.hub, None, |_| Ok(())).unwrap();
    assert!(
        original
            .with_db(&a.hub, None, |db| -> Result<()> {
                db.execute("DELETE FROM devices", [])?;
                anyhow::bail!("falha simulada")
            })
            .is_err()
    );
    assert_eq!(a.geracao(&aparelho.key), geracao);
}

#[test]
fn contador_persiste_na_remocao_reabertura_e_recusa_valor_invalido() {
    let a = Ambiente::novo();
    let aparelho = a.aparelho(DeviceSource::Local);
    let primeira = a.geracao(&aparelho.key);
    a.hub.store.put_device(&aparelho).unwrap();
    assert!(a.geracao(&aparelho.key) > primeira, "put idêntico é nova concessão");
    a.hub.store.remove_device(&aparelho.key).unwrap();
    let removida = a.geracao(&aparelho.key);
    let reaberta = Store::open(&a.pasta.join("estado.db")).unwrap();
    assert_eq!(concessao::geracao(&reaberta.db.locked(), &aparelho.key).unwrap(), removida);
    a.hub.store.put_device(&aparelho).unwrap();
    let db = a.hub.store.db.locked();
    for sql in [
        "UPDATE donna_device_concessao SET geracao=-1",
        "UPDATE donna_device_concessao SET geracao='invalida'",
        "DELETE FROM donna_device_concessao",
    ] {
        assert!(db.execute(sql, []).is_err());
    }
    db.execute("UPDATE donna_device_concessao SET geracao=9223372036854775807", []).unwrap();
    drop(db);
    assert!(a.hub.store.put_device(&aparelho).is_err(), "overflow não repara contador");
    let armazenada: i64 =
        a.hub.store.db.locked().query_row("SELECT geracao FROM donna_device_concessao", [], |r| r.get(0)).unwrap();
    assert_eq!(armazenada, i64::MAX);
    assert!(autenticar(&a.hub, &aparelho.key).is_err(), "contador esgotado não autoriza");
    assert!(a.hub.store.device(&aparelho.key).is_some(), "escrita falha foi revertida");
}

#[tokio::test]
async fn renovacao_da_mesma_concessao_nao_amplia_prazo_do_pedido() {
    let a = Ambiente::novo();
    let mut aparelho = a.aparelho(DeviceSource::Account);
    aparelho.lease_until = Some(now_ms() + 1500);
    a.hub.store.put_device(&aparelho).unwrap();
    let (_, caller) = autenticar(&a.hub, &aparelho.key).unwrap();
    let original = Autoridade::capturar(&a.hub, &caller).unwrap();
    let geracao = a.geracao(&aparelho.key);
    a.hub.store.set_lease(&aparelho.key, now_ms() + 60_000).unwrap();
    assert_eq!(a.geracao(&aparelho.key), geracao);
    original.with_db(&a.hub, None, |_| Ok(())).unwrap();
    // Sem espera sob guardas; o relógio real determina o prazo original.
    let prazo = aparelho.lease_until.unwrap();
    while now_ms() < prazo {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(original.with_db(&a.hub, None, |_| Ok(())).is_err());
    let (_, nova_requisicao) = reconferir(&a.hub, &caller).unwrap();
    assert!(Autoridade::capturar(&a.hub, &nova_requisicao).is_ok());
    assert!(original.with_db(&a.hub, None, |_| Ok(())).is_err());
}

#[tokio::test]
async fn saida_reserva_vaga_antes_e_nao_envia_perfil_apos_perder_sincronizacao() {
    let a = Ambiente::novo();
    let aparelho = a.aparelho(DeviceSource::Account);
    let (_, caller) = autenticar(&a.hub, &aparelho.key).unwrap();
    let original = Autoridade::capturar(&a.hub, &caller).unwrap();
    let geracao = a.geracao(&aparelho.key);
    let (liberar, fila) = tokio::sync::oneshot::channel();
    let original_em_fila = original.clone();
    let hub = a.hub.clone();
    let pedido = tokio::spawn(async move {
        fila.await.unwrap();
        let mut executou = false;
        let resultado = original_em_fila.with_db(&hub, None, |db| {
            executou = true;
            db.execute("INSERT INTO kv(k,v) VALUES('c1-sync-fila','indevido')", [])?;
            Ok(())
        });
        (resultado.is_err(), executou)
    });
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let resposta = json!({"perfil":"privado"});
    let vaga = tx.reserve().await.unwrap();
    a.hub.cloud_synced.store(false, Ordering::Release);
    assert!(original.com_saida(&a.hub, None, || vaga.send(resposta)).is_err());
    assert!(rx.try_recv().is_err());
    a.hub.cloud_synced.store(true, Ordering::Release);
    // Mesmo dono/grant/lease/geração e bool restaurado não recuperam o pedido.
    assert_eq!(a.geracao(&aparelho.key), geracao);
    liberar.send(()).unwrap();
    let (recusado, executou) = pedido.await.unwrap();
    assert!(recusado);
    assert!(!executou);
    assert!(a.hub.store.kv_read("c1-sync-fila").unwrap().is_none());
    let resposta = json!({"perfil":"antigo"});
    let vaga = tx.reserve().await.unwrap();
    assert!(original.com_saida(&a.hub, None, || vaga.send(resposta)).is_err());
    assert!(rx.try_recv().is_err());
    let (_, nova_solicitacao) = reconferir(&a.hub, &caller).unwrap();
    let nova = Autoridade::capturar(&a.hub, &nova_solicitacao).unwrap();
    nova.with_db(&a.hub, None, |db| {
        db.execute("INSERT INTO kv(k,v) VALUES('c1-sync-nova','ok')", [])?;
        Ok(())
    })
    .unwrap();
    let resposta = json!({"perfil":"atual"});
    let vaga = tx.reserve().await.unwrap();
    nova.com_saida(&a.hub, None, || vaga.send(resposta)).unwrap();
    assert_eq!(rx.recv().await.unwrap()["perfil"], "atual");
    assert!(Autoridade::capturar(&a.hub, &caller).is_err(), "Arc antigo permanece encerrado");
    let (_, autenticada) = autenticar(&a.hub, &aparelho.key).unwrap();
    assert!(Autoridade::capturar(&a.hub, &autenticada).is_ok());
    a.hub.revoke_device(&aparelho.key).unwrap();
    let resposta = json!({"perfil":"revogado"});
    let vaga = tx.reserve().await.unwrap();
    assert!(nova.com_saida(&a.hub, None, || vaga.send(resposta)).is_err());
    assert!(rx.try_recv().is_err());
}

/// HOST-R1-02: origem pessoal (Local pareado DEPOIS da geração pessoal) é recusada nas três validações reais do canal
/// legado/subscriptions; o controle Local sem origem pessoal segue admitido.
#[test]
fn origem_pessoal_recusa_autenticar_reconferir_e_conferir_do_canal_generico() {
    let controle = Ambiente::novo();
    let aparelho = controle.aparelho(DeviceSource::Local);
    let (_, caller) = autenticar(&controle.hub, &aparelho.key).unwrap();
    assert!(reconferir(&controle.hub, &caller).is_ok(), "controle legítimo sem origem pessoal");
    let original = Autoridade::capturar(&controle.hub, &caller).unwrap();
    original.conferir(&controle.hub.store.db.locked(), None, 0, 0, false).unwrap();

    let pessoal = Ambiente::novo();
    let (cfg, _) = crate::fork::personal_bootstrap::provisioning::fixture().unwrap();
    pessoal.hub.store.personal_install_generation(&cfg).unwrap();
    let depois = pessoal.aparelho(DeviceSource::Local);
    assert_eq!(depois.key, aparelho.key);
    assert!(autenticar(&pessoal.hub, &depois.key).is_err(), "autenticar");
    assert!(reconferir(&pessoal.hub, &caller).is_err(), "reconferir");
    assert!(original.conferir(&pessoal.hub.store.db.locked(), None, 0, 0, false).is_err(), "conferir");
}
