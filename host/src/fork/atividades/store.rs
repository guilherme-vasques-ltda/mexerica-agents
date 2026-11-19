//! Seam interno do MESMO Store. SQL fixo auditado; não é API para cliente/modelo.
use super::modelo::CoreError;
use crate::store::Store;
use rusqlite::{Params, Transaction, TransactionBehavior, types::FromSql};
use std::{cell::RefCell, rc::Rc, sync::TryLockError};

thread_local! {
    static ACTIVE: RefCell<Option<Rc<std::cell::Cell<bool>>>> = const { RefCell::new(None) };
}
struct ActiveGuard;
impl Drop for ActiveGuard {
    fn drop(&mut self) {
        ACTIVE.with(|active| *active.borrow_mut() = None);
    }
}

/// Sem Deref/Connection/commit. O lifetime e o retorno 'static impedem fuga do handle.
/// Não chamar outro método Store ou fazer I/O externo enquanto esta closure vive.
pub(crate) struct StoreTx<'db> {
    tx: Transaction<'db>,
    failed: Rc<std::cell::Cell<bool>>,
    remaining: u8,
}
impl StoreTx<'_> {
    /// D502: leitura fechada na mesma transação; não emite prova de quiescência.
    pub(crate) fn review_work_blocked(&mut self) -> Result<bool, CoreError> {
        self.scalar(super::revisao::estado::BLOCKED, [])
    }

    /// Só teste: aborto negativo da MESMA instância, antes de qualquer entrega.
    /// O único UPDATE invalida gesto/entrega e retira a causa atomicamente.
    #[cfg(test)]
    pub(crate) fn review_abort_undelivered(&mut self, instance: &str) -> Result<bool, CoreError> {
        self.execute(
            "UPDATE donna_reviews SET gesture_eligible=0,delivery_eligible=0,phase='aborted' WHERE instance_ref=?1 AND phase='holding' AND gesture_eligible=1 AND delivery_eligible=1",
            [instance],
        ).map(|changed| changed == 1)
    }

    fn record<T>(&self, value: Result<T, CoreError>) -> Result<T, CoreError> {
        if value.is_err() {
            self.failed.set(true);
        }
        value
    }
    fn statement(&mut self, sql: &'static str, allowed: &[&str]) -> Result<(), CoreError> {
        if self.failed.get() {
            return Err(CoreError::RollbackOnly);
        }
        if self.remaining == 0 {
            return self.record(Err(CoreError::TransactionLimit));
        }
        self.remaining -= 1;
        // Sem lote, comentários iniciais, PRAGMA, DDL, controle de Tx ou conexão extra.
        let keyword = sql.split_whitespace().next().unwrap_or("");
        self.record(if sql.contains(';') || !allowed.contains(&keyword) {
            Err(CoreError::StatementForbidden)
        } else {
            Ok(())
        })
    }
    pub(super) fn execute(&mut self, sql: &'static str, params: impl Params) -> Result<usize, CoreError> {
        self.statement(sql, &["INSERT", "UPDATE", "DELETE"])?;
        let result = self.tx.execute(sql, params).map_err(|_| CoreError::Storage);
        self.record(result)
    }
    pub(super) fn scalar<T: FromSql>(&mut self, sql: &'static str, params: impl Params) -> Result<T, CoreError> {
        self.statement(sql, &["SELECT"])?;
        let result = self.tx.query_row(sql, params, |row| row.get(0)).map_err(|_| CoreError::Storage);
        self.record(result)
    }
    /// A linha principal do catálogo de sessões para a conversa criada nesta transação, pelo produtor único
    /// (`fork::sessoes`). Uma instrução do orçamento; falha deixa a transação rollback-only.
    pub(super) fn ensure_session_principal(&mut self, chat: &str) -> Result<(), CoreError> {
        self.statement("INSERT", &["INSERT"])?;
        let result = crate::fork::sessoes::ensure_principal(&self.tx, chat).map_err(|_| CoreError::Storage);
        self.record(result)
    }
    /// SQL de domínio D220, sem expor Connection ou permitir closures arbitrárias.
    /// Raiz antiga custa cinco; predecessor acrescenta uma classificação. Descendência
    /// reserva 32 para até cinco ancestrais. O teto da transação continua 64.
    pub(crate) fn chain_admit(
        &mut self,
        pedido: &crate::fork::cadeias::modelo::Pedido,
    ) -> Result<Result<crate::fork::cadeias::modelo::Recibo, crate::fork::cadeias::modelo::Recusa>, CoreError> {
        let cost = if matches!(pedido.escopo, crate::fork::cadeias::modelo::Escopo::RotinaDescendente { .. }) {
            32
        } else if let Some(previous) = &pedido.anterior {
            // Escopo divergente também pode alcançar o validador ancestral antes
            // da recusa. A classificação pelo recibo cobra essa leitura adicional.
            if self.descendant_receipt(previous)? { 31 } else { 5 }
        } else {
            5
        };
        for _ in 0..cost {
            self.statement("SELECT", &["SELECT"])?;
        }
        let result = crate::fork::cadeias::store::admit(&self.tx, pedido).map_err(|_| CoreError::Storage);
        self.record(result)
    }
    pub(crate) fn chain_dispatch(
        &mut self,
        recibo: &crate::fork::cadeias::modelo::Recibo,
    ) -> Result<Result<(), crate::fork::cadeias::modelo::Recusa>, CoreError> {
        // Uma leitura de classificação de custo, nunca autoridade. A identidade e a
        // procedência completas continuam sendo conferidas pelo consumidor D220.
        let descendant = self.descendant_receipt(recibo)?;
        for _ in 0..if descendant { 31 } else { 2 } {
            self.statement("SELECT", &["SELECT"])?;
        }
        let result = crate::fork::cadeias::store::dispatch(&self.tx, recibo).map_err(|_| CoreError::Storage);
        self.record(result)
    }
    fn descendant_receipt(&mut self, recibo: &crate::fork::cadeias::modelo::Recibo) -> Result<bool, CoreError> {
        self.scalar(
            "SELECT EXISTS(SELECT 1 FROM donna_chain_admissions WHERE id=?1 AND chain_id=?2 AND run_id=?3 AND bot_id=?4 AND level=?5 AND ordinal=?6 AND json_extract(scope,'$.kind')='rotina_descendente')",
            rusqlite::params![recibo.admission_id, recibo.chain_id, recibo.run_id, recibo.bot_id, recibo.level, recibo.ordinal],
        )
    }
    pub(crate) fn chain_finish(&mut self, recibo: &crate::fork::cadeias::modelo::Recibo) -> Result<(), CoreError> {
        for _ in 0..2 {
            self.statement("SELECT", &["SELECT"])?;
        }
        let result = crate::fork::cadeias::store::finish(&self.tx, recibo).map_err(|_| CoreError::Storage);
        self.record(result)
    }
    /// Guarda de Sessões na mesma Tx, só leitura: `assert_execution_live` com a Connection recebida,
    /// sem Store, runtime ou await. Cobra oito leituras (cerca, vínculo e ativo). Falha deixa rollback-only.
    pub(super) fn execution_session(
        &mut self,
        execution: &crate::fork::sessoes::ExecutionRef,
    ) -> Result<crate::fork::sessoes::store::SessionRef, CoreError> {
        for _ in 0..8 {
            self.statement("SELECT", &["SELECT"])?;
        }
        let result = crate::fork::sessoes::bindings::assert_execution_live(&self.tx, execution)
            .map_err(|_| CoreError::ScopeUnavailable);
        self.record(result)
    }
    /// Fechamento restritivo de UM rascunho, sem autorização, fonte nova ou callback.
    /// Máximo de seis instruções: recibo, DELETE privado, UPDATE recibo, entrada,
    /// `next_rev` e UPDATE projeção. Reserva o máximo antes de qualquer escrita.
    pub(crate) fn whatsapp_draft_close(&mut self, id: &str) -> Result<Vec<crate::store::Entry>, CoreError> {
        for _ in 0..6 {
            self.statement("SELECT", &["SELECT"])?;
        }
        let result = crate::fork::conectores::whatsapp_human::close_on(&self.tx, id).map_err(|_| CoreError::Storage);
        self.record(result)
    }
    /// Uma instrução fixa do helper nativo; participa do orçamento e rollback-only.
    pub(super) fn next_revision(&mut self) -> Result<i64, CoreError> {
        self.statement("INSERT", &["INSERT"])?;
        let result = crate::store::next_rev(&self.tx).map_err(|_| CoreError::Storage);
        self.record(result)
    }
}

/// Prazo do `retry_store_busy` (o mesmo do `mark_started`), medido no relógio: nenhuma tentativa
/// começa depois dele.
const STORE_BUSY_LIMIT: std::time::Duration = std::time::Duration::from_secs(2);
const STORE_BUSY_WAIT: std::time::Duration = std::time::Duration::from_millis(25);

pub(crate) fn is_store_busy(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.downcast_ref::<CoreError>() == Some(&CoreError::StoreBusy))
}

/// `StoreBusy` é o `try_lock` de `with_store_tx` recusando um banco segurado por outra tarefa naquele
/// instante. Repete a operação INTEIRA (que revalida tudo e é idempotente por ocorrência), com espera
/// async curta, fora do executor bloqueado e de qualquer lock do Store ou das Rotinas. Outro erro
/// volta na hora; um banco que continua ocupado após ~2 s devolve o mesmo `StoreBusy`.
pub(crate) async fn retry_store_busy<T>(mut op: impl FnMut() -> anyhow::Result<T>) -> anyhow::Result<T> {
    let deadline = std::time::Instant::now() + STORE_BUSY_LIMIT;
    loop {
        let error = match op() {
            Err(error) if is_store_busy(&error) => error,
            other => return other,
        };
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(error);
        }
        tokio::time::sleep(remaining.min(STORE_BUSY_WAIT)).await;
        if std::time::Instant::now() >= deadline {
            return Err(error);
        }
    }
}

/// Só retorna valores próprios após commit. Não emite evento/wake nem resolve scope.
pub(crate) fn with_store_tx<T: 'static>(
    store: &Store,
    body: impl for<'db> FnOnce(&mut StoreTx<'db>) -> Result<T, CoreError>,
) -> Result<T, CoreError> {
    let nested = ACTIVE.with(|active| {
        if let Some(failed) = active.borrow().as_ref() {
            failed.set(true);
            true
        } else {
            false
        }
    });
    if nested {
        return Err(CoreError::NestedTransaction);
    }
    let mut db = store.db.try_lock().map_err(|error| match error {
        TryLockError::WouldBlock => CoreError::StoreBusy,
        TryLockError::Poisoned(_) => CoreError::StorePoisoned,
    })?;
    // Recusa também uma Tx iniciada pelo legado na mesma conexão.
    if !db.is_autocommit() {
        return Err(CoreError::NestedTransaction);
    }
    let failed = Rc::new(std::cell::Cell::new(false));
    ACTIVE.with(|active| *active.borrow_mut() = Some(failed.clone()));
    let _active = ActiveGuard;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|_| CoreError::Storage)?;
    let mut scope = StoreTx { tx, failed, remaining: 64 };
    let result = body(&mut scope)?;
    if scope.failed.get() {
        return Err(CoreError::RollbackOnly);
    }
    #[cfg(test)]
    commit_probe::run();
    scope.tx.commit().map_err(|_| CoreError::Storage)?;
    Ok(result)
}

/// Sonda de commit só em testes; não recebe `Connection` nem muda a transação.
#[cfg(test)]
pub(crate) mod commit_probe {
    use std::{cell::RefCell, marker::PhantomData, rc::Rc};

    thread_local! {
        static PROBE: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    }

    /// Não atravessa threads; `Drop` remove a sonda mesmo se não houve commit.
    #[must_use]
    pub(crate) struct Guard(PhantomData<Rc<()>>);

    pub(crate) fn install(probe: impl FnOnce() + 'static) -> Guard {
        PROBE.with(|slot| {
            let mut slot = slot.borrow_mut();
            assert!(slot.is_none(), "sonda de commit já instalada nesta thread");
            *slot = Some(Box::new(probe));
        });
        Guard(PhantomData)
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            PROBE.with(|slot| *slot.borrow_mut() = None);
        }
    }

    pub(super) fn run() {
        let probe = PROBE.with(|slot| slot.borrow_mut().take());
        if let Some(probe) = probe {
            probe();
        }
    }
}
