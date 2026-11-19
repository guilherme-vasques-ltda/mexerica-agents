//! Acesso pela nuvem do Donnabot (contrato `donna-acesso/v1`). Esta fatia só lê a nuvem que o
//! operador configurou no serviço do Hub; a admissão de aparelhos continua no código herdado
//! (`remote::cloud`, `api::devices`), sem autoridade nova.

pub(crate) mod approval;
mod config;
mod fences;

pub(crate) use fences::{Fence, FenceAck, acknowledged, confirmable};

pub(crate) use config::cloud_url;
