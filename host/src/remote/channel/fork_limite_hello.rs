use std::{collections::VecDeque, time::Duration};
use tokio::time::Instant;

pub(super) const JANELA: Duration = Duration::from_secs(10);
pub(super) const POR_CHAVE: usize = 20;
const TOTAL: usize = 64;

pub(super) struct Excedido;

#[derive(Default)]
pub(crate) struct Limite {
    global: VecDeque<Instant>,
    verificadas: VecDeque<(Instant, [u8; 32])>,
}

impl Limite {
    pub(super) fn permitir(&mut self) -> Result<(), Excedido> {
        self.permitir_em(Instant::now())
    }

    fn permitir_em(&mut self, agora: Instant) -> Result<(), Excedido> {
        while self.global.front().is_some_and(|instante| agora.duration_since(*instante) >= JANELA) {
            self.global.pop_front();
        }
        if self.global.len() >= TOTAL {
            return Err(Excedido);
        }
        self.global.push_back(agora);
        Ok(())
    }

    pub(super) fn permitir_verificada(&mut self, dk: [u8; 32]) -> Result<(), Excedido> {
        self.permitir_verificada_em(dk, Instant::now())
    }

    fn permitir_verificada_em(&mut self, dk: [u8; 32], agora: Instant) -> Result<(), Excedido> {
        while self.verificadas.front().is_some_and(|(instante, _)| agora.duration_since(*instante) >= JANELA) {
            self.verificadas.pop_front();
        }
        if self.verificadas.len() >= TOTAL
            || self.verificadas.iter().filter(|(_, chave)| chave == &dk).count() >= POR_CHAVE
        {
            return Err(Excedido);
        }
        self.verificadas.push_back((agora, dk));
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
            assert!(limite.permitir_verificada_em([0; 32], agora).is_ok());
        }
        assert!(limite.permitir_verificada_em([0; 32], agora).is_err());
        for _ in 0..TOTAL {
            assert!(limite.permitir_em(agora).is_ok());
        }
        assert!(limite.permitir_em(agora).is_err());
        assert!(limite.permitir_verificada_em([255; 32], agora).is_ok());
        assert!(limite.permitir_em(agora + JANELA).is_ok());
        assert!(limite.permitir_verificada_em([0; 32], agora + JANELA).is_ok());
        assert_eq!(limite.global.len(), 1);
        assert_eq!(limite.verificadas.len(), 1);
    }
}
