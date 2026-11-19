use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Recibo {
    pub chain_id: String,
    pub admission_id: String,
    pub run_id: String,
    pub bot_id: String,
    pub level: i64,
    pub ordinal: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tipo {
    Raiz,
    Continuacao,
    /// Repasse entre bots (`ask_bot`/`message_bot`); consumidor em `equipe`.
    Delegacao,
}
impl Tipo {
    pub(super) fn sql(self) -> &'static str {
        match self {
            Self::Raiz => "root",
            Self::Continuacao => "continuation",
            Self::Delegacao => "delegation",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origem {
    Agenda,
    Manual,
    Legada,
    Interna,
}
impl Origem {
    pub(super) fn sql(self) -> &'static str {
        match self {
            Self::Agenda => "schedule",
            Self::Manual => "manual",
            Self::Legada => "legacy-pending",
            Self::Interna => "internal",
        }
    }
    pub(super) fn from_sql(value: &str) -> Option<Self> {
        [Self::Agenda, Self::Manual, Self::Legada, Self::Interna].into_iter().find(|o| o.sql() == value)
    }
}

/// Contexto persistido. `ConversaDoBot` é o de Rotinas; as outras duas são a origem
/// tipada MCP entre bots (R0) e nunca satisfazem a regra de continuação de Rotinas.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Escopo {
    ConversaDoBot {
        bot_id: String,
        space_id: Option<String>,
    },
    /// Raiz de um turno ACP: `SessionRef` viva conferida na Tx da admissão.
    TurnoMcp {
        bot_id: String,
        space_id: Option<String>,
        chat_id: String,
        session_id: String,
    },
    /// Rotina criada durante execução ancestral; só o consumidor especializado admite.
    RotinaDescendente {
        bot_id: String,
        space_id: String,
        rotina_id: String,
        criacao: super::descendencia_rotina::RoutineOrigin,
    },
    /// Repasse `de` → `bot_id` admitido junto dos avisos do pedido.
    PedidoEntreBots {
        bot_id: String,
        space_id: Option<String>,
        de: String,
    },
}

pub(crate) struct ContextoPersistido {
    pub escopo: Escopo,
    pub rotina: Option<String>,
}

/// IDs candidatos são gerados fora da closure SQL. O recibo existente sempre prevalece.
pub(crate) struct Pedido {
    pub tipo: Tipo,
    pub evento: String,
    pub intencao: String,
    pub anterior: Option<Recibo>,
    pub candidato: Recibo,
    pub rotina: Option<String>,
    pub origem: Origem,
    pub escopo: Escopo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Recusa {
    Conflito,
    Nivel,
    Execucoes,
    ReciboInvalido,
    EnvioPossivel,
}
impl std::fmt::Display for Recusa {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Conflito => "Esta ocorrência já está vinculada a outra execução.",
            Self::Nivel => "O pedido atingiu o limite de etapas e foi interrompido. O histórico foi preservado.",
            Self::Execucoes => {
                "O pedido atingiu o limite de tentativas e foi interrompido. O histórico foi preservado."
            }
            Self::ReciboInvalido => "Não foi possível conferir a tentativa original desta rotina.",
            Self::EnvioPossivel => {
                "Esta tentativa pode ter sido enviada; a rotina foi interrompida para evitar repetição."
            }
        })
    }
}
impl std::error::Error for Recusa {}

/// A preparação terminou, mas seu estado não foi confirmado. Não é recusa
/// de autoridade/recibo nem prova de `StoreBusy`; a causa original é preservada.
#[derive(Debug)]
pub(crate) struct PreparacaoNaoPersistida;

impl std::fmt::Display for PreparacaoNaoPersistida {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Não foi possível salvar a preparação da rotina.")
    }
}
impl std::error::Error for PreparacaoNaoPersistida {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fase {
    Reservada,
    EnvioPossivel,
    Concluida,
}
