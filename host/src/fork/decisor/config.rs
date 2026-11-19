//! Escolha do usuário fora da Cloud. Nada secreto aqui: a chave vive no cofre (`chave`).
//! Esta config NÃO escolhe a chave da plataforma: isso vem da política server-side (`plataforma`).
use super::{Alvo, ProvedorDecisor};
use crate::store::Store;
use anyhow::{Result, anyhow, ensure};
use serde::{Deserialize, Serialize};

pub(crate) const SLOT: &str = "fork.decisor.v1";

/// Sem `provedor` o decisor está desligado e o Hub segue no modo "Fixo" (padrão, sem opt-in).
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConfigDecisor {
    pub provedor: Option<ProvedorDecisor>,
    /// `OpenRouter`: um id da lista filtrada por modelos de decisão. Google: `gemini-decision-1` se vazio.
    pub modelo: Option<String>,
}

impl ConfigDecisor {
    /// `None` = desligado ou incompleto (`OpenRouter` sem modelo): cai no Fixo.
    pub(crate) fn alvo(&self) -> Option<Alvo> {
        let provedor = self.provedor?;
        let modelo = match (provedor, &self.modelo) {
            (_, Some(m)) => m.clone(),
            (ProvedorDecisor::Google, None) => "gemini-decision-1".to_owned(),
            (ProvedorDecisor::OpenRouter, None) => return None,
        };
        modelo_valido(&modelo).then_some(Alvo { provedor, modelo })
    }

    pub(crate) fn validar(&self) -> Result<()> {
        if let Some(modelo) = &self.modelo {
            ensure!(modelo_valido(modelo), "Modelo do decisor inválido.");
        }
        ensure!(
            self.provedor != Some(ProvedorDecisor::OpenRouter) || self.modelo.is_some(),
            "O OpenRouter exige escolher um modelo de decisão."
        );
        Ok(())
    }
}

/// Ids do `OpenRouter` têm `/`, `~` e `.` (ex.: `fornecedor/modelo-1.2`, `~fornecedor/modelo-latest`).
/// Variantes `:free` nunca: o pedido leva dado de cliente e o nível grátis pode usá-lo para treino.
pub(crate) fn modelo_valido(modelo: &str) -> bool {
    !modelo.is_empty()
        && !modelo.ends_with(":free")
        && modelo.len() <= 100
        && !modelo.contains("..")
        && modelo.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./~".contains(&b))
}

/// Config ausente ou ilegível = desligado (fail-closed para o Fixo).
pub(crate) fn carregar(store: &Store) -> ConfigDecisor {
    store.kv_get(SLOT).and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default()
}

pub(crate) fn salvar(store: &Store, config: &ConfigDecisor) -> Result<()> {
    config.validar()?;
    let raw = serde_json::to_string(config).map_err(|_| anyhow!("Configuração do decisor não serializável."))?;
    store.kv_set(SLOT, &raw)
}
