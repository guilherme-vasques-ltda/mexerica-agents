// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Encaminha tarefas de fundo à implementação do fork.

pub(crate) use crate::fork::background_executor::{SubscriptionLimit, one_shot, one_shot_observed};
