use std::{collections::VecDeque, time::Duration};
use tokio::time::Instant;

const JANELA: Duration = Duration::from_secs(10);
const POR_CHAVE: usize = 8;
const TOTAL: usize = 64;

pub(super) struct Excedido;

#[derive(Default)]
pub(crate) struct Limite(VecDeque<(Instant, [u8; 32])>);

impl Limite {
    pub(super) fn permitir(&mut self, dk: [u8; 32]) -> Result<(), Excedido> {
        self.permitir_em(dk, Instant::now())
    }

    fn permitir_em(&mut self, dk: [u8; 32], agora: Instant) -> Result<(), Excedido> {
        while self.0.front().is_some_and(|(instante, _)| agora.duration_since(*instante) >= JANELA) {
            self.0.pop_front();
        }
        if self.0.len() >= TOTAL || self.0.iter().filter(|(_, chave)| chave == &dk).count() >= POR_CHAVE {
            return Err(Excedido);
        }
        self.0.push_back((agora, dk));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teto_por_chave_e_global_expiram_sem_memoria_ilimitada() {
        let mut limite = Limite::default();
        let agora = Instant::now();
        for _ in 0..POR_CHAVE {
            assert!(limite.permitir_em([0; 32], agora).is_ok());
        }
        assert!(limite.permitir_em([0; 32], agora).is_err());
        for i in 1..=TOTAL - POR_CHAVE {
            assert!(limite.permitir_em([u8::try_from(i).unwrap(); 32], agora).is_ok());
        }
        assert!(limite.permitir_em([255; 32], agora).is_err());
        assert!(limite.permitir_em([0; 32], agora + JANELA).is_ok());
        assert_eq!(limite.0.len(), 1);
    }
}
