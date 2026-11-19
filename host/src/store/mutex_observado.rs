//! Identifica contenção da conexão compartilhada sem registrar dados do banco.
use rusqlite::Connection;
use std::ops::{Deref, DerefMut};
use std::panic::Location;
use std::sync::{LockResult, Mutex, MutexGuard, PoisonError, TryLockError, TryLockResult};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
struct Holder {
    location: &'static Location<'static>,
    since: Instant,
}

pub(crate) struct ObservedMutex {
    inner: Mutex<Connection>,
    holder: Mutex<Option<Holder>>,
}

#[derive(Debug)]
pub(crate) struct Guard<'a> {
    inner: MutexGuard<'a, Connection>,
    holder: &'a Mutex<Option<Holder>>,
}

impl ObservedMutex {
    pub(super) fn new(connection: Connection) -> Self {
        Self { inner: Mutex::new(connection), holder: Mutex::new(None) }
    }

    #[track_caller]
    fn guard<'a>(&'a self, inner: MutexGuard<'a, Connection>) -> Guard<'a> {
        *self.holder.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(Holder { location: Location::caller(), since: Instant::now() });
        Guard { inner, holder: &self.holder }
    }

    #[track_caller]
    pub(crate) fn lock(&self) -> LockResult<Guard<'_>> {
        match self.inner.lock() {
            Ok(inner) => Ok(self.guard(inner)),
            Err(error) => Err(PoisonError::new(self.guard(error.into_inner()))),
        }
    }

    #[track_caller]
    pub(crate) fn try_lock(&self) -> TryLockResult<Guard<'_>> {
        // Nunca espera os metadados numa tentativa que deve falhar imediatamente.
        // Quando disponíveis, mantê-los até o try_lock evita atribuir a outro detentor.
        let metadata = self.holder.try_lock().ok();
        match self.inner.try_lock() {
            Ok(inner) => {
                drop(metadata);
                Ok(self.guard(inner))
            }
            Err(TryLockError::Poisoned(error)) => {
                drop(metadata);
                Err(TryLockError::Poisoned(PoisonError::new(self.guard(error.into_inner()))))
            }
            Err(TryLockError::WouldBlock) => {
                warn_busy(Location::caller(), metadata.and_then(|metadata| *metadata));
                Err(TryLockError::WouldBlock)
            }
        }
    }
}

impl ObservedMutex {
    /// Espera curta e limitada por uma disputa breve, sem registrar cada volta; só a desistência registra.
    /// Nenhuma sondagem acontece depois do prazo: passado o corte, devolve `WouldBlock`.
    #[track_caller]
    pub(crate) fn try_lock_for(&self, limit: Duration) -> TryLockResult<Guard<'_>> {
        let caller = Location::caller();
        let deadline = Instant::now() + limit;
        loop {
            match self.inner.try_lock() {
                Ok(inner) => return Ok(self.guard(inner)),
                Err(TryLockError::Poisoned(error)) => {
                    return Err(TryLockError::Poisoned(PoisonError::new(self.guard(error.into_inner()))));
                }
                Err(TryLockError::WouldBlock) => {}
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            std::thread::sleep(remaining.min(RETRY_STEP));
            if Instant::now() >= deadline {
                break;
            }
        }
        let holder = self.holder.try_lock().ok().and_then(|metadata| *metadata);
        warn_busy(caller, holder);
        Err(TryLockError::WouldBlock)
    }
}

const RETRY_STEP: Duration = Duration::from_millis(2);

fn warn_busy(caller: &Location<'_>, holder: Option<Holder>) {
    if let Some(holder) = holder {
        tracing::warn!(
            request_file = caller.file(),
            request_line = caller.line(),
            observed_holder_file = holder.location.file(),
            observed_holder_line = holder.location.line(),
            observed_held_ms = u64::try_from(holder.since.elapsed().as_millis()).unwrap_or(u64::MAX),
            "mutex do Store ocupado"
        );
    } else {
        tracing::warn!(
            request_file = caller.file(),
            request_line = caller.line(),
            "mutex do Store ocupado; detentor indisponível"
        );
    }
}

impl crate::LockExt<Connection> for ObservedMutex {
    type Guard<'a> = Guard<'a>;

    #[track_caller]
    fn locked(&self) -> Self::Guard<'_> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Deref for Guard<'_> {
    type Target = Connection;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl DerefMut for Guard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
/// Acima disto, soltar a conexão registra quem a segurou: é o que falta para achar a causa do `StoreBusy`.
const SEGURADO_DEMAIS_MS: u64 = 250;

fn segurado_demais(held_ms: u64) -> bool {
    held_ms >= SEGURADO_DEMAIS_MS
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        let holder = self.holder.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(holder) = holder {
            let held_ms = u64::try_from(holder.since.elapsed().as_millis()).unwrap_or(u64::MAX);
            if segurado_demais(held_ms) {
                // Só arquivo e linha do código que pegou a conexão, e o tempo: nenhum dado do banco ou do usuário.
                tracing::warn!(
                    holder_file = holder.location.file(),
                    holder_line = holder.location.line(),
                    held_ms,
                    "mutex do Store segurado por muito tempo"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LockExt;

    #[test]
    fn reports_the_holder_and_clears_it_after_release() {
        let db = ObservedMutex::new(Connection::open_in_memory().unwrap());
        let line = line!() + 1;
        let guard = db.locked();
        assert!(matches!(db.try_lock(), Err(TryLockError::WouldBlock)));
        let holder = db.holder.lock().unwrap().unwrap();
        assert_eq!(holder.location.line(), line);
        assert_eq!(holder.location.file(), file!());
        // Os metadados podem estar sendo atualizados: a falha do try_lock não espera por eles.
        let metadata = db.holder.lock().unwrap();
        assert!(matches!(db.try_lock(), Err(TryLockError::WouldBlock)));
        drop(metadata);
        drop(guard);
        assert!(db.holder.lock().unwrap().is_none());
        assert!(db.try_lock().is_ok());
    }

    #[test]
    fn try_lock_for_espera_a_disputa_curta_e_desiste_no_limite() {
        let db = std::sync::Arc::new(ObservedMutex::new(Connection::open_in_memory().unwrap()));
        let (tx, rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn({
            let db = db.clone();
            move || {
                let guard = db.locked();
                tx.send(()).unwrap();
                std::thread::sleep(Duration::from_millis(60));
                drop(guard);
            }
        });
        rx.recv().unwrap();
        assert!(db.try_lock_for(Duration::from_secs(2)).is_ok(), "disputa curta passa quando o detentor solta");
        holder.join().unwrap();
        let guard = db.locked();
        let started = Instant::now();
        assert!(matches!(db.try_lock_for(Duration::from_millis(30)), Err(TryLockError::WouldBlock)));
        let esperou = started.elapsed();
        assert!(esperou >= Duration::from_millis(30) && esperou < Duration::from_millis(500), "{esperou:?}");
        assert!(
            matches!(db.try_lock_for(Duration::ZERO), Err(TryLockError::WouldBlock)),
            "prazo zero só sonda uma vez"
        );
        drop(guard);
    }

    #[test]
    fn so_registra_quem_segurou_acima_do_limite() {
        assert!(!segurado_demais(0));
        assert!(!segurado_demais(SEGURADO_DEMAIS_MS - 1));
        assert!(segurado_demais(SEGURADO_DEMAIS_MS));
        // Soltar depois do limite limpa o detentor como antes.
        let db = ObservedMutex::new(Connection::open_in_memory().unwrap());
        let guard = db.locked();
        std::thread::sleep(Duration::from_millis(SEGURADO_DEMAIS_MS + 10));
        drop(guard);
        assert!(db.holder.lock().unwrap().is_none());
        assert!(db.try_lock().is_ok());
    }
}
