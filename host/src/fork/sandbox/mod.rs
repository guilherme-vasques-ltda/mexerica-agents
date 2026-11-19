//! MX-175: contrato único de provedor de sandbox, com o provedor local de referência.
//! MX-177: executor bwrap (Linux). Sem consumidor no Hub: nenhum bot, rotina ou
//! ferramenta chama este módulo ainda.
#[cfg(target_os = "linux")]
pub(crate) mod bwrap;
pub(crate) mod contrato;
pub(crate) mod copia;
pub(crate) mod local;
pub(crate) mod montagem;
pub(crate) mod processo;

#[cfg(all(test, target_os = "linux"))]
mod bwrap_tests;
#[cfg(test)]
mod tests;
