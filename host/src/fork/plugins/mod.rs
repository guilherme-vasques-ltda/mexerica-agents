//! F1b/D247: registro próprio e configuração persistida, privados ao módulo.
//!
//! `Ref` continua snapshot descritivo; forma válida, CAS e grant configurado
//! não são admissão, prova humana, cartão consumido ou autorização de execução.
//! Boot migra só DDL vazio, sem habilitar/provisionar/reseed de qualquer linha.
//! O hook de exclusão só restringe grants na Connection da transação existente.
//!
//! D538/D544: Agenda deixou de ser plugin. O módulo `agenda` conserva apenas o DDL
//! legado; nenhuma RPC, grant ou instalação libera o agendamento dos bots.
//! Rotinas tem autoridade própria de bot/sessão/Space, sem acesso a eventos legados.
//! `WhatsApp` R3: número = conta+recurso e "ver conversas/grupos" = grant, só por
//! `whatsapp_grants` (caminho do dono e leitor de mensagens do `fork::whatsapp`).
pub(crate) mod agenda;
mod custody;
pub(crate) mod lifecycle;
mod manifest;
pub(crate) mod model;
mod payload;
mod policy;
pub(crate) mod private_content;
mod schema;
mod store;
pub(crate) mod whatsapp_grants;

pub(crate) use manifest::Builtin;
pub(crate) use store::{builtin_installation, set_builtin};

pub(super) fn migrate(c: &rusqlite::Connection) -> Result<(), model::PluginError> {
    schema::migrate(c)?;
    agenda::migrate(c)
}

pub(super) fn on_bot_deleted(c: &rusqlite::Connection, id: &str) -> Result<(), model::PluginError> {
    store::revoke_bot(c, id)
}

#[cfg(test)]
mod registry_tests;
#[cfg(test)]
mod tests;
