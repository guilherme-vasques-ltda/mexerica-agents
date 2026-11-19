//! D110: autoridade humana e edição parcial do perfil, sem ferramentas de autoedição.
pub mod api;
pub mod authorization;
mod errors;
mod validation;

pub(crate) use validation::{checked_model, checked_model_options, checked_name};

#[cfg(test)]
mod tests;
