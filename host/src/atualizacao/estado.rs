use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Modelo de transição apenas: a persistência durável e a parada do serviço
/// são pré-requisitos independentes antes de habilitar qualquer publicação.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Estado {
    pub marca: u64,
    pub piso: u64,
    pub fase: Fase,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "fase", rename_all = "snake_case", deny_unknown_fields)]
pub enum Fase {
    Livre,
    Pendente { anterior: Option<u64>, prazo: i64 },
    Aceita,
    Revertida,
    Expirada,
}

impl Estado {
    pub fn expirar(&mut self, agora: i64) -> bool {
        if matches!(self.fase, Fase::Pendente { prazo, .. } if agora >= prazo) {
            self.fase = Fase::Expirada;
            return true;
        }
        false
    }

    pub fn iniciar(&mut self, sequencia: u64, piso: u64, anterior: Option<u64>, prazo: i64, agora: i64) -> Result<()> {
        ensure!(!matches!(self.fase, Fase::Pendente { .. } | Fase::Expirada), "transação bloqueada");
        ensure!(
            sequencia > self.marca && piso >= self.piso && piso <= sequencia && prazo > agora,
            "transição inválida"
        );
        ensure!(anterior.is_none_or(|s| s > 0 && s <= self.marca), "sequência anterior inconsistente");
        self.marca = sequencia;
        self.piso = piso;
        self.fase = Fase::Pendente { anterior, prazo };
        Ok(())
    }

    pub fn aceitar(&mut self, agora: i64) -> Result<()> {
        self.expirar(agora);
        ensure!(matches!(self.fase, Fase::Pendente { .. }), "sem transação aceitável");
        self.fase = Fase::Aceita;
        Ok(())
    }

    pub fn reverter(&mut self, agora: i64) -> Result<()> {
        self.expirar(agora);
        match self.fase {
            Fase::Revertida => return Ok(()),
            Fase::Pendente { anterior: Some(anterior), .. } => ensure!(anterior >= self.piso, "piso impede reversão"),
            Fase::Pendente { anterior: None, .. } => (),
            _ => anyhow::bail!("sem transação reversível"),
        }
        self.fase = Fase::Revertida;
        Ok(())
    }
}
