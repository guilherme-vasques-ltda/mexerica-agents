//! Rascunho humano nativo: dado cifrado por 90 s, sem decisão ou efeito externo.
use super::contexto::ContextoExecucao;
use crate::{
    LockExt,
    fork::{
        bot_admission::{self, Origin, Phase},
        sessoes::{ExecutionRef, bindings},
        whatsapp::{self, WhatsappCardSource},
    },
    hub::{Hub, Runtime},
    market::vault,
    store::{Entry, EntryKind, Store, entry_on, insert_visible_on, next_rev, now_ms},
};
use anyhow::{Result, anyhow, bail, ensure};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::Instant;
use zeroize::Zeroizing;

const RECEIPT: &str = "donna.whatsapp.draft.receipt.";
const PRIVATE: &str = "donna.whatsapp.draft.private.";
const LIFETIME: Duration = Duration::from_secs(90);

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WhatsappHumanRequest {
    pub chat_id: String,
    pub message_id: String,
    pub text: String,
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum State {
    Draft,
    Closed,
}

/// Somente metadados: não é autorização, conta ou identidade de execução.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WhatsappPreparedReceipt {
    draft_id: String,
    revision: i64,
    deadline: i64,
    state: State,
}

struct Draft {
    source: WhatsappCardSource,
    until: Instant,
}

#[derive(Default)]
pub(super) struct Custody(Mutex<HashMap<String, Arc<Draft>>>);

fn human_on(
    hub: &Hub,
    c: &Connection,
    bot: &str,
    execution: &ExecutionRef,
    runtime: &Runtime,
    id: &str,
) -> Result<ContextoExecucao> {
    let session = bindings::assert_execution_live(c, execution)?;
    ensure!(
        session.chat_id == bot && runtime.memory_turn_token.as_deref() == Some(execution.turn_token()),
        "Execução original indisponível"
    );
    let record = bot_admission::current_on(c, bot)?.ok_or_else(|| anyhow!("Pedido humano ausente"))?;
    ensure!(
        record.origin == Origin::Human
            && record.phase == Phase::Permitted
            && !record.stop_requested
            && record.execution_token == execution.turn_token()
            && !record.entry_ids.is_empty(),
        "Exige pedido humano original"
    );
    for message in &record.entry_ids {
        let entry = entry_on(c, message)?.ok_or_else(|| anyhow!("Mensagem humana ausente"))?;
        ensure!(entry.is_user_authored() && entry.bot_id == bot && entry.session_id == session.id, "Origem não humana");
    }
    let context = ContextoExecucao::capture(hub, id, bot, runtime, c)?;
    ensure!(
        context.session_binding.as_deref() == execution.acp_binding() && record.entry_ids.contains(&context.message),
        "Execução ou mensagem divergente"
    );
    Ok(context)
}

pub(crate) fn prepare_whatsapp_human(
    hub: &Arc<Hub>,
    execution: &ExecutionRef,
    request: WhatsappHumanRequest,
) -> Result<WhatsappPreparedReceipt> {
    ensure!(
        !request.text.trim().is_empty()
            && request.text.chars().count() <= 4096
            && !request.text.contains('\0')
            && !request.chat_id.is_empty()
            && request.chat_id.len() <= 128
            && !request.message_id.is_empty()
            && request.message_id.len() <= 256,
        "Pedido de rascunho inválido"
    );
    let text = Zeroizing::new(request.text);
    let bot = {
        let db = hub.store.db.locked();
        bindings::assert_execution_live(&db, execution)?.chat_id
    };
    let id = uuid::Uuid::new_v4().to_string();
    let clock = Instant::now();
    let deadline = now_ms() + 90_000;
    // Capture real primeiro; cifragem fora de DB/runtime e antes da escrita.
    let (source, context) = hub.conector_fence(&bot, |runtime| {
        let mut db = hub.store.db.locked();
        let tx = db.transaction()?;
        let context = human_on(hub, &tx, &bot, execution, runtime, &id)?;
        let source = whatsapp::capture_card_source_on(hub, &tx, &bot, &request.chat_id, &request.message_id)?;
        tx.commit()?;
        Ok((source, context))
    })?;
    let plain = Zeroizing::new(source.presentation(&text).to_string());
    let sealed = vault::seal(&hub.store, &format!("{PRIVATE}{id}"), &plain)?;
    let receipt = WhatsappPreparedReceipt { draft_id: id.clone(), revision: 1, deadline, state: State::Draft };
    let draft = Arc::new(Draft { source, until: clock + LIFETIME });
    let result = hub.conector_publish(|| {
        let entries = hub.conector_fence(&bot, |runtime| {
            let mut db = hub.store.db.locked();
            whatsapp::with_card_source_on(hub, &mut db, &draft.source, |c| {
                ensure!(Instant::now() < draft.until && now_ms() < deadline, "Rascunho expirado");
                let current = human_on(hub, c, &bot, execution, runtime, &id)?;
                ensure!(current == context, "Contexto original alterado");
                let active: i64 =
                    c.query_row("SELECT COUNT(*) FROM kv WHERE k LIKE 'donna.whatsapp.draft.private.%'", [], |r| {
                        r.get(0)
                    })?;
                ensure!(active < 16, "Limite global de 16 rascunhos abertos no Hub");
                vault::write_sealed(c, &format!("{PRIVATE}{id}"), &sealed)?;
                c.execute(
                    "INSERT INTO kv(k,v) VALUES(?1,?2)",
                    params![format!("{RECEIPT}{id}"), serde_json::to_string(&receipt)?],
                )?;
                let data = json!({"author":bot,"conectorDraft":receipt});
                let (marker, entry) = insert_visible_on(c, &id, &context.lane, EntryKind::Notice, context.turn, &data)?;
                Ok(marker.into_iter().chain([entry]).collect::<Vec<_>>())
            })
        })?;
        // A custódia existe antes da publicação, somente após o commit real.
        hub.conectores.drafts.0.locked().insert(id.clone(), draft);
        Ok((receipt.clone(), entries))
    })?;
    let weak = Arc::downgrade(hub);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            let Some(hub) = weak.upgrade() else { break };
            match inspect(&hub, &id).await {
                Ok(Some(_)) => {
                    #[cfg(test)]
                    retry_probe::timer_inspected(&id);
                }
                Ok(None) => break,
                Err(error) => {
                    // Busy final conserva a custódia para próxima leitura/recover;
                    // o timer para mesmo assim, sem retry a cada segundo.
                    if !crate::fork::atividades::store::is_store_busy(&error) {
                        hub.conectores.drafts.0.locked().remove(&id);
                    }
                    tracing::warn!("Rascunho WhatsApp indisponível; limpeza cifrada pendente");
                    break;
                }
            }
        }
        #[cfg(test)]
        retry_probe::timer_stopped(&id);
    });
    Ok(result)
}

fn receipt_on(c: &Connection, id: &str) -> Result<WhatsappPreparedReceipt> {
    let raw: String = c.query_row("SELECT v FROM kv WHERE k=?1", [format!("{RECEIPT}{id}")], |r| r.get(0))?;
    let receipt: WhatsappPreparedReceipt = serde_json::from_str(&raw)?;
    ensure!(receipt.draft_id == id && receipt.revision > 0 && receipt.revision < i64::MAX, "Recibo inválido");
    Ok(receipt)
}

/// SQL fechado de domínio, na transação do chamador; sem Store, locks ou I/O.
/// Máximo de seis comandos SQL, ligado à reserva conservadora de seis em
/// `StoreTx::whatsapp_draft_close`; revisar essa reserva se este caminho crescer.
/// Chamadores atuais: esse método da `StoreTx` e `recover` legado, que já roda
/// na transação de `with_card_cleanup_on`. Nenhum chama com Connection solta.
pub(crate) fn close_on(c: &Connection, id: &str) -> Result<Vec<Entry>> {
    let mut receipt = receipt_on(c, id)?;
    if receipt.state == State::Closed {
        c.execute("DELETE FROM kv WHERE k=?1", [format!("{PRIVATE}{id}")])?;
        return Ok(vec![]);
    }
    receipt.state = State::Closed;
    receipt.revision += 1;
    store_closed_on(c, id, &receipt)
}

/// Tombstone do próprio domínio: preserva a cerca de `respondPermission` mesmo
/// quando o recibo anterior não podia ser interpretado. Erros SQL propagam.
fn store_closed_on(c: &Connection, id: &str, receipt: &WhatsappPreparedReceipt) -> Result<Vec<Entry>> {
    c.execute("DELETE FROM kv WHERE k=?1", [format!("{PRIVATE}{id}")])?;
    c.execute("UPDATE kv SET v=?2 WHERE k=?1", params![format!("{RECEIPT}{id}"), serde_json::to_string(receipt)?])?;
    let Some(mut entry) = entry_on(c, id)? else {
        return Ok(vec![]);
    };
    if !entry.data.is_object() || entry.data.get("conectorDraft").is_none() {
        return Ok(vec![]);
    }
    entry.data["conectorDraft"] = serde_json::to_value(receipt)?;
    entry.rev = next_rev(c)?;
    entry.updated_at = now_ms();
    c.execute(
        "UPDATE entries SET data=?2,rev=?3,updated_at=?4 WHERE id=?1",
        params![id, entry.data.to_string(), entry.rev, entry.updated_at],
    )?;
    Ok(vec![entry])
}

fn close_once(hub: &Hub, id: &str) -> Result<()> {
    hub.conector_publish(|| {
        let entries = whatsapp::close_draft(hub, id)?;
        hub.conectores.drafts.0.locked().remove(id);
        Ok(((), entries))
    })
}

/// Até três tentativas inteiras; só `StoreBusy`, sem renovar prazo ou custódia.
/// A seção síncrona retorna antes da espera, já sem emit/DB/bots/drafts.
pub(crate) async fn close(hub: &Hub, id: &str) -> Result<()> {
    for _ in 0..2 {
        match close_once(hub, id) {
            Err(error) if crate::fork::atividades::store::is_store_busy(&error) => {}
            result => return result,
        }
        #[cfg(test)]
        retry_probe::run();
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    close_once(hub, id)
}

/// Copia só ciphertext sob a guarda original; sem exigir turno vivo na leitura.
fn inspect_current(hub: &Hub, id: &str) -> Result<Option<String>> {
    let draft = hub.conectores.drafts.0.locked().get(id).cloned();
    let Some(draft) = draft else {
        return Ok(None);
    };
    let mut db = hub.store.db.locked();
    current_cipher(hub, &mut db, id, &draft).map(Some)
}

/// Segunda conferência síncrona: banco ocupado não mantém plain esperando DB.
fn recheck_current(hub: &Hub, id: &str) -> Result<Option<String>> {
    let draft = hub.conectores.drafts.0.locked().get(id).cloned();
    let Some(draft) = draft else {
        return Ok(None);
    };
    let mut db = hub.store.db.try_lock().map_err(|error| match error {
        std::sync::TryLockError::WouldBlock => crate::fork::atividades::modelo::CoreError::StoreBusy,
        std::sync::TryLockError::Poisoned(_) => crate::fork::atividades::modelo::CoreError::StorePoisoned,
    })?;
    current_cipher(hub, &mut db, id, &draft).map(Some)
}

fn current_cipher(hub: &Hub, db: &mut Connection, id: &str, draft: &Draft) -> Result<String> {
    whatsapp::with_card_source_on(hub, db, &draft.source, |c| {
        let receipt = receipt_on(c, id)?;
        ensure!(
            receipt.state == State::Draft && now_ms() < receipt.deadline && Instant::now() < draft.until,
            "Custódia encerrada"
        );
        ensure!(entry_on(c, id)?.is_some(), "Projeção removida");
        c.query_row("SELECT v FROM kv WHERE k=?1", [format!("{PRIVATE}{id}")], |r| r.get(0)).map_err(Into::into)
    })
}

async fn inspect(hub: &Hub, id: &str) -> Result<Option<String>> {
    if let Ok(Some(cipher)) = inspect_current(hub, id) {
        return Ok(Some(cipher));
    }
    #[cfg(test)]
    retry_probe::before_close();
    close(hub, id).await?;
    Ok(None)
}

/// O plaintext só existe neste frame síncrono, sem fechamento ou espera.
fn open_current(hub: &Hub, id: &str, cipher: &str) -> Result<Option<Value>> {
    // A sonda nasce antes de plain: seu Drop só roda depois de Zeroizing.
    #[cfg(test)]
    let _plain_scope = retry_probe::plain_scope();
    let plain = Zeroizing::new(vault::open(&hub.store, &format!("{PRIVATE}{id}"), cipher)?);
    #[cfg(test)]
    retry_probe::after_open();
    match recheck_current(hub, id) {
        Ok(Some(_)) => Ok(Some(serde_json::from_str::<Value>(&plain)?)),
        // Contenção não encerra custódia. O Err sai deste frame após o Drop de plain.
        Err(error) if crate::fork::atividades::store::is_store_busy(&error) => Err(error),
        // Demais resultados conservam o fechamento anterior, após o Drop de plain.
        _ => Ok(None),
    }
}

fn coordinates(hub: &Hub, id: &str) -> Option<(String, String, String, String)> {
    let draft = hub.conectores.drafts.0.locked().get(id).cloned();
    let draft = draft?;
    let (bot, account, chat, message) = draft.source.coordinates();
    Some((bot.into(), account.into(), chat.into(), message.into()))
}

/// Gesto do dono no cartão: abre o rascunho válido e enfileira a resposta.
/// A fila reconfere tudo na vez; o rascunho fecha para não ser reutilizado.
pub(crate) async fn send(hub: &Arc<Hub>, id: &str) -> Result<Value> {
    ensure!(uuid::Uuid::parse_str(id).is_ok(), "Rascunho inválido");
    let Some((bot, _account, chat, message)) = coordinates(hub, id) else {
        bail!("Rascunho indisponível");
    };
    let mut queued = None;
    if let Some(cipher) = inspect(hub, id).await? {
        if let Some(presentation) = open_current(hub, id, &cipher)? {
            let text = presentation["text"].as_str().ok_or_else(|| anyhow!("Rascunho inválido"))?;
            let request = whatsapp::outbox::Enqueue {
                bot: &bot,
                chat: &chat,
                reply_to: &message,
                text,
                origin: whatsapp::outbox::Origin::OwnerCard,
            };
            queued = Some(whatsapp::outbox::enqueue(hub, &request, now_ms())?);
        }
        close(hub, id).await?;
    }
    let Some(queued) = queued else { bail!("Rascunho indisponível") };
    Ok(json!({"draftId": id, "queued": format!("{queued:?}")}))
}

pub(crate) async fn read(hub: &Hub, id: &str) -> Result<Value> {
    ensure!(uuid::Uuid::parse_str(id).is_ok(), "Rascunho inválido");
    if let Some(cipher) = inspect(hub, id).await? {
        if let Some(presentation) = open_current(hub, id, &cipher)? {
            // Nenhum await após obter a apresentação. Recibo/prazo ainda conferidos.
            return read_result(hub, id, Some(presentation));
        }
        // open_current já encerrou seu frame e descartou plain antes deste await.
        #[cfg(test)]
        retry_probe::before_close();
        close(hub, id).await?;
    }
    read_result(hub, id, None)
}

fn read_result(hub: &Hub, id: &str, mut presentation: Option<Value>) -> Result<Value> {
    let receipt = receipt_on(&hub.store.db.locked(), id)?;
    if receipt.state != State::Draft || now_ms() >= receipt.deadline {
        presentation = None;
    }
    Ok(json!({"receipt":receipt,"presentation":presentation,
        "capability":{"prepare":true,"approve":false,"send":false},"reason":"approval_unavailable"}))
}

/// Ingresso Local não fabrica rascunho; IDs conhecidos também recusam o bypass legado.
pub(crate) fn is_draft(store: &Store, id: &str) -> Result<bool> {
    Ok(store
        .db
        .locked()
        .query_row("SELECT EXISTS(SELECT 1 FROM kv WHERE k=?1)", [format!("{RECEIPT}{id}")], |r| r.get(0))?)
}

/// Startup: não reconstitui snapshots vivos a partir de metadados persistidos.
pub(crate) fn recover(hub: &Hub) -> Result<()> {
    let mut db = hub.store.db.locked();
    whatsapp::with_card_cleanup_on(hub, &mut db, |c| {
        let records: Vec<(String, Option<String>)> = {
            let mut statement = c.prepare("SELECT substr(k,length('donna.whatsapp.draft.receipt.')+1),v FROM kv WHERE k LIKE 'donna.whatsapp.draft.receipt.%'")?;
            statement
                .query_map([], |r| {
                    let raw = match r.get_ref(1)? {
                        rusqlite::types::ValueRef::Text(bytes) => std::str::from_utf8(bytes).ok().map(str::to_owned),
                        _ => None,
                    };
                    Ok((r.get(0)?, raw))
                })?
                .collect::<rusqlite::Result<_>>()?
        };
        for (id, raw) in records {
            let valid = raw.as_deref().is_some_and(|raw| {
                serde_json::from_str::<WhatsappPreparedReceipt>(raw)
                    .is_ok_and(|receipt| receipt.draft_id == id && receipt.revision > 0 && receipt.revision < i64::MAX)
            });
            if valid {
                close_on(c, &id)?;
            } else {
                // Apenas corrupção do recibo é contida. Não captura erro
                // de SQLite/commit nem reconstrói conta, fonte ou privado.
                let closed =
                    WhatsappPreparedReceipt { draft_id: id.clone(), revision: 1, deadline: 0, state: State::Closed };
                store_closed_on(c, &id, &closed)?;
            }
        }
        // Startup também remove eventual privado órfão de recibo perdido.
        c.execute("DELETE FROM kv WHERE k LIKE 'donna.whatsapp.draft.private.%'", [])?;
        Ok(())
    })?;
    hub.conectores.drafts.0.locked().clear();
    Ok(())
}

/// Sonda síncrona entre tentativas, exclusiva de testes e da thread corrente.
#[cfg(test)]
pub(crate) mod retry_probe {
    use crate::LockExt;
    use std::{cell::RefCell, marker::PhantomData, rc::Rc};
    use tokio::sync::oneshot;

    thread_local! {
        static PROBE: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
        static AFTER_OPEN: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
        static BEFORE_CLOSE: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
        static PLAIN_LIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        static TIMER: RefCell<Option<TimerWatch>> = const { RefCell::new(None) };
    }

    struct TimerWatch {
        id: String,
        inspected: Option<oneshot::Sender<()>>,
        stopped: Option<oneshot::Sender<()>>,
    }

    /// Observa só este rascunho; nenhuma confirmação carrega conteúdo privado.
    #[must_use]
    pub(crate) struct TimerWatchGuard(PhantomData<Rc<()>>);

    pub(crate) fn watch_timer(id: &str) -> (TimerWatchGuard, oneshot::Receiver<()>, oneshot::Receiver<()>) {
        let (inspected_tx, inspected_rx) = oneshot::channel();
        let (stopped_tx, stopped_rx) = oneshot::channel();
        TIMER.with(|slot| {
            let mut slot = slot.borrow_mut();
            assert!(slot.is_none(), "observação de timer já instalada nesta thread");
            *slot = Some(TimerWatch { id: id.into(), inspected: Some(inspected_tx), stopped: Some(stopped_tx) });
        });
        (TimerWatchGuard(PhantomData), inspected_rx, stopped_rx)
    }

    impl Drop for TimerWatchGuard {
        fn drop(&mut self) {
            TIMER.with(|slot| *slot.borrow_mut() = None);
        }
    }

    pub(super) fn timer_inspected(id: &str) {
        let tx = TIMER.with(|slot| {
            slot.borrow_mut().as_mut().filter(|watch| watch.id == id).and_then(|watch| watch.inspected.take())
        });
        if let Some(tx) = tx {
            let _ = tx.send(());
        }
    }

    pub(super) fn timer_stopped(id: &str) {
        let tx = TIMER.with(|slot| {
            let mut slot = slot.borrow_mut();
            let watch = slot.as_mut().filter(|watch| watch.id == id)?;
            // Se o timer saiu antes de Some, a espera do primeiro tick falha.
            drop(watch.inspected.take());
            watch.stopped.take()
        });
        if let Some(tx) = tx {
            let _ = tx.send(());
        }
    }

    #[must_use]
    pub(crate) struct Guard(PhantomData<Rc<()>>);

    pub(crate) fn install(probe: impl FnMut() + 'static) -> Guard {
        PROBE.with(|slot| {
            let mut slot = slot.borrow_mut();
            assert!(slot.is_none(), "sonda de retry já instalada nesta thread");
            *slot = Some(Box::new(probe));
        });
        Guard(PhantomData)
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            PROBE.with(|slot| *slot.borrow_mut() = None);
        }
    }

    #[must_use]
    pub(crate) struct OpenGuard(PhantomData<Rc<()>>);

    pub(crate) fn install_after_open(probe: impl FnOnce() + 'static) -> OpenGuard {
        AFTER_OPEN.with(|slot| {
            let mut slot = slot.borrow_mut();
            assert!(slot.is_none(), "sonda de abertura já instalada nesta thread");
            *slot = Some(Box::new(probe));
        });
        OpenGuard(PhantomData)
    }

    impl Drop for OpenGuard {
        fn drop(&mut self) {
            AFTER_OPEN.with(|slot| *slot.borrow_mut() = None);
        }
    }

    pub(super) fn after_open() {
        let probe = AFTER_OPEN.with(|slot| slot.borrow_mut().take());
        if let Some(probe) = probe {
            probe();
        }
    }

    #[must_use]
    pub(crate) struct BeforeCloseGuard(PhantomData<Rc<()>>);

    pub(crate) fn install_before_close(probe: impl FnOnce() + 'static) -> BeforeCloseGuard {
        BEFORE_CLOSE.with(|slot| {
            let mut slot = slot.borrow_mut();
            assert!(slot.is_none(), "sonda de fechamento já instalada nesta thread");
            *slot = Some(Box::new(probe));
        });
        BeforeCloseGuard(PhantomData)
    }

    impl Drop for BeforeCloseGuard {
        fn drop(&mut self) {
            BEFORE_CLOSE.with(|slot| *slot.borrow_mut() = None);
        }
    }

    pub(super) fn before_close() {
        let probe = BEFORE_CLOSE.with(|slot| slot.borrow_mut().take());
        if let Some(probe) = probe {
            probe();
        }
    }

    pub(crate) fn has_draft(hub: &crate::hub::Hub, id: &str) -> bool {
        hub.conectores.drafts.0.locked().contains_key(id)
    }

    pub(crate) fn assert_auxiliary_unlocked(hub: &crate::hub::Hub) {
        assert!(hub.emit_lock.try_lock().is_ok());
        assert!(hub.conectores.drafts.0.try_lock().is_ok());
        assert!(hub.store.secret_key.try_lock().is_ok());
    }

    pub(crate) fn plain_live() -> bool {
        PLAIN_LIVE.with(std::cell::Cell::get)
    }

    pub(super) struct PlainScope;

    pub(super) fn plain_scope() -> PlainScope {
        PLAIN_LIVE.with(|live| {
            assert!(!live.replace(true));
        });
        PlainScope
    }

    impl Drop for PlainScope {
        fn drop(&mut self) {
            PLAIN_LIVE.with(|live| live.set(false));
        }
    }

    pub(super) fn run() {
        PROBE.with(|slot| {
            if let Some(probe) = slot.borrow_mut().as_mut() {
                probe();
            }
        });
    }
}
