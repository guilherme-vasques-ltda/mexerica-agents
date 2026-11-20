//! Mail Manager: permissões de e-mail por caixa e por participante, validadas no Hub.
//!
//! Três permissões independentes por caixa e participante: Ver (lista, busca, prévia,
//! corpo, anexos e aviso de mensagem nova), Enviar (escrever novo, com remetente da
//! própria caixa) e Excluir (mover para a lixeira com uma referência já obtida).
//! Responder no fio exige Ver e Enviar. Tudo é negado até alguém conceder; administrar
//! (conectar caixa, conceder, revogar) não dá leitura. Revogar vale para a próxima
//! operação, e repetir um efeito com a mesma chave não o repete.
//!
//! Neste recorte não há transporte real: as operações falam com um [`model::MailBackend`] e
//! os testes usam caixas e identidades fictícias. Concessão a pessoa é recusada até o
//! Hub ter identidade de pessoa. Autorizar a operação não substitui a aprovação de cada
//! efeito externo, que continua a cargo de quem liga o transporte.
pub(crate) mod model;
pub(crate) mod ops;
pub(crate) mod store;

pub(crate) fn migrate(c: &rusqlite::Connection) -> anyhow::Result<()> {
    Ok(store::migrate(c)?)
}

pub(crate) fn on_bot_deleted(c: &rusqlite::Connection, bot: &str) -> anyhow::Result<()> {
    Ok(store::on_bot_deleted(c, bot)?)
}

#[cfg(test)]
mod tests;
