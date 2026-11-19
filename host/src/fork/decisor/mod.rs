//! MX-387 fatia A: decisor de IA do roteamento (modo "Auto"). Núcleo do Hub, ainda sem consumidor.
//!
//! O decisor só SUGERE uma opção entre as que o chamador já permitiu; nunca concede autoridade nem
//! é a única trava de segurança (ver `(arquivo de design privado do projeto de origem, fora deste repositório)`). Sem
//! provedor, sem chave, abstenção, teto atingido, prazo estourado ou erro, o resultado é o modo
//! "Fixo", ou seja, o comportamento atual do Hub. Nenhum turno ou sessão chama este módulo.
//!
//! - [`config`]: escolha do usuário (fora da Cloud), no KV, sem segredo.
//! - [`chave`]: chave no cofre do produto e a costura da chave da plataforma.
//! - [`plataforma`]: política da Cloud e teto mensal, persistidos no servidor, com CAS.
//! - [`clientes`]: `OpenRouter` (genérico, só modelos de decisão) e Google atrás de [`DecisionProvider`].
//! - [`sugestao`]: `decidir`, que junta tudo e devolve sugestão validada ou Fixo.
pub(crate) mod chave;
pub(crate) mod clientes;
pub(crate) mod config;
pub(crate) mod plataforma;
pub(crate) mod sugestao;
#[cfg(test)]
mod tests;
mod tipos;

pub(crate) use tipos::{Alvo, Chave, DecisionProvider, ErroDecisao, Opcao, Pedido, ProvedorDecisor, Saida};
