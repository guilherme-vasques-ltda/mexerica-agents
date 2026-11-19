//! Contenção SPACE-SKILLS-1: o registro atual pertence somente a Pessoal.
use super::{PERSONAL, guard};
use crate::hub::Hub;
use anyhow::{Result, ensure};
use serde_json::Value;

/// Só este módulo constrói o payload de domínio após validar o contexto original.
/// Não é credencial; o chamador continua sujeito à autorização atual da API.
pub(crate) struct PersonalSkillRequest {
    context: Value,
    payload: Value,
}
impl PersonalSkillRequest {
    pub(crate) fn resolve(hub: &Hub, method: &str, context: Value) -> Result<Self> {
        Self::validate(hub, method, &context)?;
        let mut payload = context.clone();
        if let Some(fields) = payload.as_object_mut() {
            // Não remover bot/conversa/extras: os parsers de domínio continuam fechados.
            fields.remove("spaceId");
        }
        Ok(Self { context, payload })
    }

    fn validate(hub: &Hub, method: &str, context: &Value) -> Result<()> {
        guard(hub, method, context)?;
        ensure!(context.get("spaceId").is_none_or(|space| space.as_str() == Some(PERSONAL)), "skills_personal_only");
        Ok(())
    }

    pub(crate) fn payload(&self) -> &Value {
        &self.payload
    }

    pub(crate) fn revalidate(&self, hub: &Hub, method: &str) -> Result<()> {
        Self::validate(hub, method, &self.context)
    }
}
