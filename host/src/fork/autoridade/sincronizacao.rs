//! Transições de sincronização e a última admissão usam uma única cerca.
use crate::LockExt;
use std::sync::{
    Mutex, MutexGuard,
    atomic::{AtomicBool, Ordering},
};

pub struct EstadoSincronizado {
    valor: AtomicBool,
    cerca: Mutex<u64>,
}

/// A época só pode ser lida enquanto a mesma cerca impede uma transição.
pub(crate) struct ReservaSincronizacao<'a> {
    epoca: MutexGuard<'a, u64>,
}
impl ReservaSincronizacao<'_> {
    pub(crate) fn epoca(&self) -> u64 {
        *self.epoca
    }
}

impl EstadoSincronizado {
    pub fn new(valor: bool) -> Self {
        Self { valor: AtomicBool::new(valor), cerca: Mutex::new(0) }
    }
    pub fn load(&self, ordem: Ordering) -> bool {
        self.valor.load(ordem)
    }
    pub fn store(&self, valor: bool, ordem: Ordering) {
        let mut epoca = self.cerca.locked();
        if !valor && self.valor.load(Ordering::Relaxed) {
            // Nunca volta a uma época anterior. Esgotamento recusa Account no produtor.
            *epoca = epoca.saturating_add(1);
        }
        self.valor.store(valor, ordem);
    }
    /// Ordem Cloud → DB → cerca. Sem await nem chamadas Store/Cloud sob a reserva.
    pub(crate) fn congelar(&self) -> ReservaSincronizacao<'_> {
        ReservaSincronizacao { epoca: self.cerca.locked() }
    }
}
