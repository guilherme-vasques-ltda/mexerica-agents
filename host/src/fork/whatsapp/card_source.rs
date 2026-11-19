//! Fonte do rascunho nativo; não emite permissão ou prova humana.
//! Módulo filho de number, para usar o estado real sem expor setters.
use super::{Inner, Link};
use crate::{
    LockExt,
    fork::{
        plugins::whatsapp_grants::{self, Read, ReadSnapshot},
        spaces::PERSONAL,
    },
    hub::Hub,
    store::BotConfig,
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Weak};

/// Não desserializável. O custódio do rascunho conserva este objeto original.
/// Sem Debug: telefone não deve aparecer em diagnósticos.
pub(crate) struct WhatsappCardSource {
    owner: Weak<Inner>,
    bot: String,
    account: String,
    phone: String,
    run: u64,
    epoch: u64,
    chat: String,
    message: String,
    group: bool,
    message_digest: Vec<u8>,
    read: ReadSnapshot,
    sender: String,
    excerpt: String,
}

impl WhatsappCardSource {
    /// Bot, conta, conversa e mensagem de origem fixadas na captura. Sem telefone.
    pub(crate) fn coordinates(&self) -> (&str, &str, &str, &str) {
        (&self.bot, &self.account, &self.chat, &self.message)
    }

    /// Só no privado cifrado e na leitura Local; nunca Entry ou recibo.
    pub(crate) fn presentation(&self, text: &str) -> serde_json::Value {
        serde_json::json!({"text":text,"account":self.phone,
            "replyTo":{"messageId":self.message,"sender":self.sender,
                "excerpt":self.excerpt,"origin":"external"}})
    }
}

/// Exige transação já aberta e DB travado pelo chamador. Não resolve contexto
/// humano, não autoriza efeito e não escolhe conta a partir de argumentos.
pub(crate) fn capture_card_source_on(
    hub: &Hub,
    c: &Connection,
    bot: &str,
    chat: &str,
    message: &str,
) -> Result<WhatsappCardSource> {
    ensure!(!c.is_autocommit(), "Fonte exige transação do chamador");
    let config: String = c.query_row("SELECT config FROM bots WHERE id=?1 AND deleted=0", [bot], |r| r.get(0))?;
    let config: BotConfig = serde_json::from_str(&config)?;
    ensure!(config.id == bot && !config.is_group() && config.space_id == PERSONAL, "Audiência indisponível");
    let (account, phone, group, body, media, received, expires, sender): (
        String,
        String,
        bool,
        String,
        Option<String>,
        i64,
        i64,
        String,
    ) = c.query_row(
        "SELECT n.account_id,n.phone,m.is_group,m.body,m.media,m.received_at,m.expires_at,m.sender_name
         FROM donna_whatsapp_numbers n JOIN donna_whatsapp_messages m
           ON m.bot_id=n.bot_id AND m.account_id=n.account_id
         WHERE n.bot_id=?1 AND n.state='linked' AND n.phone IS NOT NULL
           AND m.chat_id=?2 AND m.message_id=?3 AND m.call_media IS NULL",
        params![bot, chat, message],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
    )?;
    ensure!(expires > crate::store::now_ms(), "Mensagem de origem expirada");
    let read = whatsapp_grants::capture_read_on(
        c,
        &hub.host_id,
        &account,
        bot,
        if group { Read::Groups } else { Read::Chats },
    )?;
    // A conexão real exige ordem DB → bots. Nenhum await ou Store no escopo.
    let bots = hub.whatsapp.0.bots.locked();
    let follow = bots.get(bot).and_then(|live| live.sync.as_ref());
    let follow = follow.ok_or_else(|| anyhow::anyhow!("Número sem conexão viva"))?;
    ensure!(
        follow.link == Link::Connected && follow.phone.as_deref() == Some(phone.as_str()),
        "Número não confirmado nesta conexão"
    );
    ensure!(!hub.whatsapp.closing(), "WhatsApp encerrando");
    let excerpt = body.chars().take(160).collect();
    let message_digest =
        Sha256::digest(serde_json::to_vec(&(group, body, media, received, expires, &sender))?).to_vec();
    Ok(WhatsappCardSource {
        owner: Arc::downgrade(&hub.whatsapp.0),
        bot: bot.into(),
        account,
        phone,
        run: follow.run,
        epoch: follow.epoch,
        chat: chat.into(),
        message: message.into(),
        group,
        message_digest,
        read,
        sender,
        excerpt,
    })
}

/// Apenas checagem pontual. Não usar como autorização seguida de escrita fora
/// da guarda; para leitura privada/escrita/CAS usar `with_card_source_on`.
pub(crate) fn check_card_source_on(hub: &Hub, c: &Connection, original: &WhatsappCardSource) -> Result<()> {
    let owner = original.owner.upgrade().ok_or_else(|| anyhow::anyhow!("Custódia encerrada"))?;
    ensure!(Arc::ptr_eq(&owner, &hub.whatsapp.0), "Outra instância de custódia");
    let current = capture_card_source_on(hub, c, &original.bot, &original.chat, &original.message)?;
    ensure!(
        current.account == original.account
            && current.phone == original.phone
            && current.run == original.run
            && current.epoch == original.epoch
            && current.group == original.group
            && current.message_digest == original.message_digest,
        "Conta, conexão ou mensagem original alteradas"
    );
    whatsapp_grants::check_read_on(c, &original.read)?;
    Ok(())
}

/// A transação e a trava do estado vivo permanecem no escopo da ação síncrona.
/// O callback só usa c; não chama Store/Hub/Cloud/número nem abre transação.
/// Este envoltório confirma a transação ainda sob a trava viva.
/// Não confere execução humana: na captura isso cabe a `prepare_whatsapp_human`;
/// na leitura posterior só pode ser usado pela custódia Local do rascunho.
pub(crate) fn with_card_source_on<T>(
    hub: &Hub,
    c: &mut Connection,
    original: &WhatsappCardSource,
    action: impl FnOnce(&Connection) -> Result<T>,
) -> Result<T> {
    let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    // Até esta checagem há somente SELECTs, sem ação/escrita. Se ela falhar,
    // a Tx ainda sem escritas faz rollback antes de adquirir a guarda viva.
    check_card_source_on(hub, &tx, original)?;
    let bots = hub.whatsapp.0.bots.locked();
    // Daqui em diante, em erro/pânico, rollback termina antes de bots ser
    // solto. No sucesso, commit também ocorre sob a mesma guarda.
    let guarded_transaction = tx;
    let follow = bots
        .get(&original.bot)
        .and_then(|live| live.sync.as_ref())
        .ok_or_else(|| anyhow::anyhow!("Número sem conexão viva"))?;
    ensure!(
        follow.link == Link::Connected
            && follow.run == original.run
            && follow.epoch == original.epoch
            && follow.phone.as_deref() == Some(original.phone.as_str())
            && !hub.whatsapp.closing(),
        "Conexão original revogada"
    );
    let value = action(&guarded_transaction)?;
    guarded_transaction.commit()?;
    Ok(value)
}

/// Fechamento restritivo pela `StoreTx` única. Não exige nem recupera uma fonte:
/// só apaga o privado e fecha recibo/projeção do rascunho já existente.
pub(crate) fn close_draft(hub: &Hub, id: &str) -> Result<Vec<crate::store::Entry>> {
    // Declarada no CHAMADOR, antes de entrar em with_store_tx. Preenchida só
    // depois de DB/TxImmediate; sobrevive ao retorno da closure e ao commit.
    let mut bots = None;
    let outcome = crate::fork::atividades::store::with_store_tx(&hub.store, |tx| {
        bots = Some(hub.whatsapp.0.bots.locked());
        #[cfg(test)]
        {
            let result = tx.whatsapp_draft_close(id);
            assert!(matches!(hub.whatsapp.0.bots.try_lock(), Err(std::sync::TryLockError::WouldBlock)));
            result
        }
        #[cfg(not(test))]
        tx.whatsapp_draft_close(id)
    });
    // Em sucesso, erro SQL/semântico ou falha de commit, with_store_tx já
    // encerrou a Tx. Em panic, sua pilha também cai antes deste guard externo.
    // DB já foi solto dentro de with_store_tx, antes de bots. A aquisição
    // continua DB → bots; quem obtiver DB nesse intervalo só espera por bots.
    drop(bots);
    Ok(outcome?)
}

/// Limpeza só restritiva, inclusive quando a fonte já caiu ou foi revogada.
/// Conserva DB → bots também no commit/rollback de remoção do privado.
pub(crate) fn with_card_cleanup_on<T>(
    hub: &Hub,
    c: &mut Connection,
    action: impl FnOnce(&Connection) -> Result<T>,
) -> Result<T> {
    let transaction = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let _bots = hub.whatsapp.0.bots.locked();
    let guarded_transaction = transaction;
    let value = action(&guarded_transaction)?;
    guarded_transaction.commit()?;
    Ok(value)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::fork::whatsapp::{draft_tests::ready, number_tests::DONNA};

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn storetx_fecha_rascunho_sem_efeito_em_busy_erro_sql_ou_commit() {
        use crate::fork::{
            atividades::{
                modelo::CoreError,
                store::{commit_probe, with_store_tx},
            },
            conectores::whatsapp_human::{WhatsappHumanRequest, prepare_whatsapp_human},
        };
        let (f, execution) = ready().await;
        let commit_checks = std::rc::Rc::new(std::cell::Cell::new(0));
        let install_probe = || {
            let hub = f.hub.clone();
            let checks = commit_checks.clone();
            commit_probe::install(move || {
                assert!(matches!(hub.whatsapp.0.bots.try_lock(), Err(std::sync::TryLockError::WouldBlock)));
                checks.set(checks.get() + 1);
            })
        };
        let receipt = prepare_whatsapp_human(
            &f.hub,
            &execution,
            WhatsappHumanRequest {
                chat_id: "5511988887777@s.whatsapp.net".into(),
                message_id: "M1".into(),
                text: "Resposta de fixture".into(),
            },
        )
        .unwrap();
        let receipt = serde_json::to_value(receipt).unwrap();
        let id = receipt["draftId"].as_str().unwrap();
        let private = format!("donna.whatsapp.draft.private.{id}");
        let receipt_key = format!("donna.whatsapp.draft.receipt.{id}");
        let before = f.hub.store.kv_read(&private).unwrap();
        assert!(before.is_some());
        {
            let _db = f.hub.store.db.locked();
            // Não há commit no busy; a sonda não executada sai por RAII.
            let _probe = install_probe();
            let error = close_draft(&f.hub, id).unwrap_err();
            assert_eq!(error.downcast_ref::<CoreError>(), Some(&CoreError::StoreBusy));
            assert!(f.hub.whatsapp.0.bots.try_lock().is_ok());
        }
        assert_eq!(f.hub.store.kv_read(&private).unwrap(), before);
        // A 11ª operação ultrapassa 64 (11 × 6). Mesmo que o erro seja
        // engolido, os fechamentos anteriores desta Tx também são revertidos.
        assert_eq!(
            with_store_tx(&f.hub.store, |tx| {
                for _ in 0..10 {
                    tx.whatsapp_draft_close(id)?;
                }
                assert_eq!(tx.whatsapp_draft_close(id).unwrap_err(), CoreError::TransactionLimit);
                Ok(())
            }),
            Err(CoreError::RollbackOnly)
        );
        assert_eq!(f.hub.store.kv_read(&private).unwrap(), before);
        // Erro depois do DELETE: mesmo se o consumidor engolir o erro de domínio,
        // StoreTx fica rollback-only e conserva privado + recibo originais.
        f.hub
            .store
            .db
            .locked()
            .execute_batch(
                "CREATE TEMP TRIGGER draft_close_fault BEFORE UPDATE ON kv
                 WHEN OLD.k LIKE 'donna.whatsapp.draft.receipt.%'
                 BEGIN SELECT RAISE(ABORT,'fixture'); END;",
            )
            .unwrap();
        let result = with_store_tx(&f.hub.store, |tx| {
            assert_eq!(tx.whatsapp_draft_close(id).unwrap_err(), CoreError::Storage);
            Ok(())
        });
        assert_eq!(result, Err(CoreError::RollbackOnly));
        assert!(close_draft(&f.hub, id).is_err());
        assert!(f.hub.whatsapp.0.bots.try_lock().is_ok());
        assert_eq!(f.hub.store.kv_read(&private).unwrap(), before);
        {
            let db = f.hub.store.db.locked();
            db.execute_batch(
                "PRAGMA foreign_keys=ON;
                 DROP TRIGGER draft_close_fault;
                 CREATE TEMP TABLE draft_close_parent(id INTEGER PRIMARY KEY);
                 CREATE TEMP TABLE draft_close_child(id INTEGER REFERENCES draft_close_parent(id)
                     DEFERRABLE INITIALLY DEFERRED);
                 CREATE TEMP TRIGGER draft_close_commit_fault AFTER UPDATE ON kv
                 WHEN OLD.k LIKE 'donna.whatsapp.draft.receipt.%'
                 BEGIN INSERT INTO draft_close_child(id) VALUES(1); END;",
            )
            .unwrap();
        }
        // Todas as escritas passam; só o COMMIT SQLite real falha (FK diferida).
        assert_eq!(commit_checks.get(), 0);
        {
            let _probe = install_probe();
            assert!(close_draft(&f.hub, id).is_err());
        }
        assert_eq!(commit_checks.get(), 1);
        assert!(f.hub.whatsapp.0.bots.try_lock().is_ok());
        assert_eq!(f.hub.store.kv_read(&private).unwrap(), before);
        let current: serde_json::Value =
            serde_json::from_str(&f.hub.store.kv_read(&receipt_key).unwrap().unwrap()).unwrap();
        assert_eq!(current, receipt);
        f.hub.store.db.locked().execute_batch("DROP TRIGGER draft_close_commit_fault").unwrap();
        let entries = {
            let _probe = install_probe();
            close_draft(&f.hub, id).unwrap()
        };
        assert_eq!(commit_checks.get(), 2);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].data["conectorDraft"]["state"], "closed");
        assert_eq!(f.hub.store.kv_read(&private).unwrap(), None);
        assert!(f.hub.whatsapp.0.bots.try_lock().is_ok());
        assert!(close_draft(&f.hub, id).unwrap().is_empty());
        f.done().await;
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn guarda_viva_cobre_acao_sql_e_commit_rollback() {
        let (f, _) = ready().await;
        {
            let mut db = f.hub.store.db.locked();
            let tx = db.transaction().unwrap();
            let source = capture_card_source_on(&f.hub, &tx, DONNA, "5511988887777@s.whatsapp.net", "M1").unwrap();
            tx.commit().unwrap();
            with_card_source_on(&f.hub, &mut db, &source, |c| {
                assert!(f.hub.whatsapp.0.bots.try_lock().is_err());
                c.execute("INSERT INTO kv(k,v) VALUES('draft-guard-test','commit')", [])?;
                Ok(())
            })
            .unwrap();
            assert!(db.is_autocommit());
            let failure: Result<()> = with_card_source_on(&f.hub, &mut db, &source, |c| {
                assert!(f.hub.whatsapp.0.bots.try_lock().is_err());
                c.execute("UPDATE kv SET v='rollback' WHERE k='draft-guard-test'", [])?;
                anyhow::bail!("fixture rollback")
            });
            assert!(failure.is_err() && db.is_autocommit());
            let value: String = db.query_row("SELECT v FROM kv WHERE k='draft-guard-test'", [], |r| r.get(0)).unwrap();
            assert_eq!(value, "commit");
        }
        f.done().await;
    }
}

#[cfg(all(test, unix))]
#[path = "card_source_retry_tests.rs"]
mod retry_tests;
