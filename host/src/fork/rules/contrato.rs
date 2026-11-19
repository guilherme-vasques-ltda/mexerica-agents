//! Borda persistida preservada; interpretação nova exige vocabulário demonstrável.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalLevel {
    #[serde(rename = "ask")]
    Ask,
    #[serde(rename = "auto_approve")]
    AutoApprove,
    #[serde(rename = "full_access")]
    FullAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "deny")]
    Deny,
}

// O armazenamento aceita JSON legado sem reinterpretar um escopo desconhecido.
// A gravação de respostas de envio continua indisponível em store.rs.
pub type SendScope = serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub effect: Effect,
    pub field: String,
    pub op: String,
    pub value: String,
    pub bot_id: Option<String>,
    pub tool: Option<String>,
    pub scope: Option<SendScope>,
    pub enabled: bool,
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Campo {
    Acao,
    Alvo,
    Ferramenta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operador {
    Igual,
}

impl Rule {
    /// Não há tradução por aproximação de campos/operadores antigos.
    /// Só a igualdade literal nova possui contrato demonstrado nesta versão.
    pub fn interpretacao(&self) -> Option<(Campo, Operador)> {
        if self.scope.is_some() || self.op != "igual" {
            return None;
        }
        let campo = match self.field.as_str() {
            "acao" => Campo::Acao,
            "alvo" => Campo::Alvo,
            "ferramenta" => Campo::Ferramenta,
            _ => return None,
        };
        Some((campo, Operador::Igual))
    }

    pub fn sentence(&self) -> String {
        if self.interpretacao().is_none() {
            return "Regra guardada, mas ainda não reconhecida. Ela não libera ações automaticamente.".into();
        }
        let efeito = match self.effect {
            Effect::Allow => "Permitir",
            Effect::Deny => "Recusar",
        };
        let campo = match self.field.as_str() {
            "acao" => "a ação",
            "alvo" => "o destino",
            "ferramenta" => "a ferramenta",
            _ => "a informação",
        };
        format!("{efeito} quando {campo} for igual a {}. Regra ainda em preparação.", self.value)
    }
}

pub fn validate_with_floor(regra: &Rule, _contexto: &super::floor::FloorContext) -> Result<()> {
    if regra.interpretacao().is_none() {
        bail!("invalid_field: esta regra usa uma opção ainda não disponível. Nada foi alterado.");
    }
    if regra.id.trim().is_empty()
        || regra.value.trim().is_empty()
        || regra.value.len() > 4096
        || regra.value.chars().any(char::is_control)
        || regra.bot_id.as_ref().is_some_and(|id| id.trim().is_empty())
        || regra.tool.as_ref().is_some_and(|nome| nome.trim().is_empty())
    {
        bail!("invalid_field: confira a regra. Há campos vazios, textos longos demais ou caracteres inválidos.");
    }
    // Uma Permitir válida é apenas uma comparação; não contorna o chão.
    Ok(())
}
