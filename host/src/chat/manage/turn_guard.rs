//! D125: uma mensagem humana não transforma conteúdo externo em autorização de gestão.
use crate::hub::Hub;
use anyhow::{Result, bail};

pub(super) fn check(hub: &Hub, bot: &str) -> Result<()> {
    if hub.store.guard_management_exposed(bot, hub.computers.external(bot))? {
        bail!(
            "A gestão leu ou recebeu conteúdo externo e precisa de confirmação do dono. \
             Nada foi alterado. Peça ao dono para conferir e aplicar a mudança no editor de bots; \
             o cartão com o antes e depois ainda não está disponível."
        );
    }
    Ok(())
}

/// Procedência ausente/desconhecida nunca basta para dispensar o cartão, mesmo em Auto.
pub(super) fn clearly_typed(hub: &Hub, message: &str) -> bool {
    hub.store.entry(message).is_some_and(|entry| {
        let origin = &entry.data["contentOrigin"];
        origin["status"] == "appReported"
            && origin["segments"]
                .as_array()
                .is_some_and(|parts| !parts.is_empty() && parts.iter().all(|part| part["kind"] == "typed"))
    })
}
