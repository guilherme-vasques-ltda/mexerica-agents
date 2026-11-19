//! Estado público para impedir início de par gerido fora da janela permitida.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// `Revertendo`: reversão iniciada dentro da janela e ainda não concluída (queda no meio
/// dela). Ninguém inicia, mas, ao contrário de `Bloqueado`, a volta é retomável até o prazo.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "fase", rename_all = "snake_case", deny_unknown_fields)]
pub enum Status {
    Bloqueado,
    Pendente { slot: String, sequencia: u64, inicio: i64, prazo: i64 },
    Revertendo { slot: String, sequencia: u64, inicio: i64, prazo: i64 },
    Liberado { slot: String, sequencia: u64 },
}

pub fn verificar(status: &Status, slot_atual: &str, agora: i64) -> Result<()> {
    match status {
        Status::Bloqueado => anyhow::bail!("serviço bloqueado; recuperação administrativa necessária"),
        Status::Revertendo { .. } => {
            anyhow::bail!("reversão da atualização em andamento; o serviço só inicia depois dela")
        }
        Status::Pendente { slot, sequencia, inicio, prazo } => {
            ensure!(
                slot == slot_atual && *sequencia > 0 && *inicio <= agora && agora < *prazo && inicio < prazo,
                "janela da atualização inválida ou expirada"
            );
        }
        Status::Liberado { slot, sequencia } => {
            ensure!(slot == slot_atual && *sequencia > 0, "par ativo não corresponde ao estado");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expiracao_e_par_errado_impedem_inicio() {
        let pendente = Status::Pendente { slot: "slot-a".into(), sequencia: 8, inicio: 100, prazo: 200 };
        assert!(verificar(&pendente, "slot-a", 100).is_ok());
        assert!(verificar(&pendente, "slot-a", 200).is_err());
        assert!(verificar(&pendente, "slot-b", 150).is_err());
        assert!(verificar(&Status::Bloqueado, "slot-a", 150).is_err());
        let revertendo = Status::Revertendo { slot: "slot-a".into(), sequencia: 8, inicio: 100, prazo: 200 };
        assert!(verificar(&revertendo, "slot-a", 150).is_err(), "reversão em andamento libera o início");
        let json = serde_json::to_string(&revertendo).unwrap_or_default();
        assert!(json.contains("\"fase\":\"revertendo\"") && json.contains("\"prazo\":200"), "{json}");
    }
}
