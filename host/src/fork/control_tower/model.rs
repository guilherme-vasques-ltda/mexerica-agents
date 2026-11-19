//! DTO efêmero da Torre; não faz parte de bot/hello/entrada nem do mirror.
use crate::{fork::sessoes::store::SessionRef, fork_avatars::AvatarConfig, hub::BotStatus};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Read {
    pub space_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TowerSnapshot {
    pub version: u32,
    pub hub_id: String,
    /// Identidade desta coleta, sem contador global que revele mudanças privadas.
    pub snapshot_id: String,
    pub collected_at: i64,
    pub space_id: String,
    pub bots: Vec<TowerBot>,
    pub delegated_permissions: Observation<super::delegated_permissions::DelegatedPermissions>,
    pub inter_bot_requests: Observation<crate::fork::retornos::pendentes::InterBotRequests>,
    /// MB-3: "O que aconteceu", fora dos contadores; só no Pessoal.
    pub what_happened: Observation<crate::fork::resumo_diario::WhatHappened>,
    pub routine_creations: Observation<crate::fork::routine_creation::Reviews>,
    /// MEX-1391 (o).2: um cartão por troca de conta do fallback, últimos 7 dias.
    pub account_switches: Observation<crate::fork::fallback_contas::torre::Trocas>,
}

#[derive(Serialize)]
#[serde(tag = "availability", rename_all = "camelCase")]
pub(crate) enum Observation<T> {
    Available {
        source: &'static str,
        coverage: &'static str,
        value: T,
    },
    /// A mesma resposta para ausência e contexto sem audiência; não revela existência.
    Unavailable,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TowerBot {
    pub bot_id: String,
    pub name: String,
    pub title: Option<String>,
    pub avatar_color: String,
    pub avatar_shape: String,
    pub avatar_config: Option<AvatarConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mexeriquinha: Option<crate::fork_avatars::MexeriquinhaConfig>,
    pub runtime: Observation<Work>,
    pub open: Option<Open>,
    pub pending: Observation<Vec<Pending>>,
    /// MB-1b: perguntas `ask_owner` pendentes do runtime, já filtradas pela audiência deste Space.
    pub questions: Observation<Vec<Question>>,
    pub queue: Observation<u64>,
    pub usage: Usage,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Work {
    pub status: BotStatus,
    pub activity: Option<String>,
    /// MX-147: código estável projetado junto do texto legado; mesmo gate de `activity`.
    pub activity_code: Option<serde_json::Value>,
    pub started_at: Option<i64>,
    pub resume_at: Option<i64>,
    pub reason: Option<&'static str>,
    /// Referência de chat real; nunca `SessionRef`, ACP ou `ExecutionRef`.
    pub conversation: Option<ConversationRef>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConversationRef {
    pub conversation_id: String,
    pub revision: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Open {
    pub label: &'static str,
    pub bot_id: String,
    pub conversation: ConversationRef,
    pub session_ref: Option<SessionRef>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Pending {
    pub entry_id: String,
    /// Referência original do produtor canônico de sessões; nunca uma sessão ACP.
    pub session_ref: SessionRef,
    pub kind: &'static str,
    pub status: &'static str,
    pub conversation: ConversationRef,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Question {
    pub entry_id: String,
    pub conversation: ConversationRef,
    /// Sessão adicional do catálogo; ausente na principal da conversa.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub text: String,
    pub asked_at: i64,
}

/// Fontes distintas; nenhum limite de assinatura/contexto é consumo diário do bot.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Usage {
    pub daily_bot_tokens: Observation<u64>,
    pub subscription_limit: Observation<u64>,
    pub context_tokens: Observation<u64>,
}
