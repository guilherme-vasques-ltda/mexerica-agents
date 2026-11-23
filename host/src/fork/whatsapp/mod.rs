//! `WhatsApp` por bot (não oficial). R1: ajustes guardados no Hub. R2: o número
//! de cada bot pelo wacli (pareamento, um `sync --follow` por bot, desconectar).
//! R3: mensagens recebidas vão para a caixa do `WhatsApp` do bot (`inbox` lê, `messages`
//! guarda), fora do chat do bot. R4a oferece leitura delimitada ao motor;
//! o preparo humano R4c cria só rascunho cifrado não decisível, sem envio.
//!
//! "Plugin ligado" é a instalação `whatsapp` do registro `fork::plugins` (D247). Desde
//! o R3 o número conectado é CONTA + RECURSO do registro (ID = `account_id` do módulo,
//! novo a cada pareamento) e "ver conversas/grupos" é CONCESSÃO do registro. As opções
//! do bot (`donna_whatsapp_bots`) guardam a escolha do dono; o Hub concede/revoga a partir
//! delas, e a checagem "pode ver" é só a do registro.
//!
//! Gancho para cartões (R4): cada troca de ligado/desligado sobe a geração da
//! instalação. Cartão de resposta precisa guardar essa geração e cair quando ela
//! mudar; desligar também zera ver/responder de todos os bots (religar não restaura).
pub(crate) mod api;
mod binario;
mod inbox;
pub(crate) mod leitura;
mod messages;
mod number;
pub(crate) mod outbox;
mod pace;
pub(crate) mod send;
mod store;
pub(crate) use store::eligible_in_mode;
mod voz;
mod wacli;

pub(crate) use number::{State, bot_deleted, start};
pub(crate) use store::{migrate, on_bot_deleted};

#[cfg(all(test, unix))]
mod draft_tests;
#[cfg(all(test, unix))]
mod inbox_tests;
#[cfg(all(test, unix))]
mod number_tests;
#[cfg(all(test, unix))]
mod outbox_tests;
#[cfg(test)]
mod pace_tests;
#[cfg(test)]
mod tests;

/// C6: só metadados, nunca abre os bancos do wacli.
pub(crate) fn storage_bytes(bot: &str) -> anyhow::Result<u64> {
    let dir = wacli::bot_dir(&crate::service::data_dir().join("whatsapp"), bot)?;
    let mut bytes = 0;
    for name in ["wacli.db", "wacli.db-wal"] {
        bytes += super::armazenamento::file_bytes(&dir.join(name), true)?;
    }
    Ok(bytes)
}

pub(crate) use number::{
    WhatsappCardSource, capture_card_source_on, close_draft, with_card_cleanup_on, with_card_source_on,
};
