//! Cadastro Local e consulta limitada. Não contém execução de agentes.
pub(crate) mod api;
pub(crate) mod autoridade;
pub(crate) mod guia;
pub(crate) mod inspect;
// Preparação interna 05a/05b sem RPC, runner ou consumidor de execução.
#[allow(dead_code, reason = "prova de snapshot preparada; execução remota ainda indisponível")]
pub(crate) mod pacote;
pub(crate) mod protocolo;
pub(crate) mod runner;
pub(crate) mod store;
pub(crate) mod tipos;
pub(crate) mod transporte;

pub(crate) const INSTRUCTIONS: &str = "Lugares de trabalho: o usuário cadastra e consulta máquinas Boat em Ajustes → Lugares de trabalho, com conexão direta ou SSH ao Hub. Só cadastro e consulta estão disponíveis; ajudantes remotos ainda estão indisponíveis. Nenhum bot, rotina ou ferramenta administra esses lugares. Não chame RPCs administrativos por shell, não prepare máquinas nem copie login, token ou perfil do motor. Login e preparo são pessoais na máquina do executor; uma consulta concluída não habilita execução. O Berga não cria, liga, desliga ou apaga máquinas Boat. Em resultado incerto ou interrompido, não repita consulta por inferência.";

#[cfg(test)]
mod tests;
