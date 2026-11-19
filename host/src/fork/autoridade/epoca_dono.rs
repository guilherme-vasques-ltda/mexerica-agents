//! Incremento da época RAM do dono, compatível com o MSRV 1.88.
use std::sync::atomic::{AtomicU64, Ordering};

/// Chamado no ramo de troca de identidade sob o lock do status Cloud.
/// CAS forte evita falha espúria; disputa recalcula pelo valor retornado.
pub(crate) fn avancar(epoca: &AtomicU64) {
    let mut observado = epoca.load(Ordering::Relaxed);
    loop {
        let seguinte = observado.saturating_add(1);
        match epoca.compare_exchange(observado, seguinte, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(atual) => observado = atual,
        }
    }
}

#[cfg(test)]
mod provas {
    use super::*;

    #[test]
    fn cas_conserva_incrementos_concorrentes_e_satura() {
        let epoca = AtomicU64::new(0);
        avancar(&epoca);
        assert_eq!(epoca.load(Ordering::Relaxed), 1);
        let barreira = std::sync::Barrier::new(4);
        std::thread::scope(|escopo| {
            for _ in 0..4 {
                escopo.spawn(|| {
                    barreira.wait();
                    for _ in 0..64 {
                        avancar(&epoca);
                    }
                });
            }
        });
        assert_eq!(epoca.load(Ordering::Relaxed), 257);
        let limite = AtomicU64::new(u64::MAX - 1);
        avancar(&limite);
        assert_eq!(limite.load(Ordering::Relaxed), u64::MAX);
        avancar(&limite);
        assert_eq!(limite.load(Ordering::Relaxed), u64::MAX);
    }
}
