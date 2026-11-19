use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FunctionId {
    Vision,
    ContextCompression,
    MessageTriage,
    SkillsHub,
    SkillSelection,
    ApprovalJudge,
    McpRouting,
    MemoryConsolidation,
    BotNaming,
    BotDraft,
    VoiceLive,
    SpeechToText,
    TextToSpeech,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Function {
    pub id: FunctionId,
    pub name: &'static str,
    pub managed_in_voice: bool,
    pub deadline_ms: u64,
    pub recipe: &'static str,
    pub alternative_policy: Option<&'static str>,
}

impl FunctionId {
    pub fn definition(self) -> Function {
        use FunctionId::{
            ApprovalJudge, BotDraft, BotNaming, ContextCompression, McpRouting, MemoryConsolidation, MessageTriage,
            SkillSelection, SkillsHub, SpeechToText, TextToSpeech, Vision, VoiceLive,
        };
        let (name, deadline_ms) = match self {
            Vision => ("Visão", 30_000),
            ContextCompression => ("Compactar contexto", 120_000),
            MessageTriage => ("Mensagens durante o trabalho", 3_000),
            SkillsHub => ("Hub de skills", 30_000),
            SkillSelection => ("Selecionar skills", 3_000),
            ApprovalJudge => ("Aprovações", 3_000),
            McpRouting => ("Roteamento de ferramentas MCP", 3_000),
            MemoryConsolidation => ("Organizar memória", 90_000),
            BotNaming => ("Títulos", 30_000),
            BotDraft => ("Rascunho de novo bot", 25_000),
            VoiceLive => ("Conversa por voz", 0),
            SpeechToText => ("Ditado", 0),
            TextToSpeech => ("Ler respostas", 0),
        };
        let managed_in_voice = matches!(self, VoiceLive | SpeechToText | TextToSpeech);
        Function {
            id: self,
            name,
            deadline_ms,
            managed_in_voice,
            recipe: if managed_in_voice {
                "Gerenciado em Voz"
            } else if self.codex_default() {
                "Codex padrão anunciado pela conta; esforço baixo quando suportado"
            } else {
                "Haiku anunciado pela conta; versão administrada pelo provedor; esforço baixo quando suportado"
            },
            alternative_policy: (self == BotDraft).then_some(
                "D109: Codex → Haiku da conta vinculada, mesmo titular; só por lentidão após 15 s, \
                 encerrar antes da alternativa; 25 s totais; uma tentativa por conta; nunca por cota, recusa ou negação",
            ),
        }
    }

    pub fn codex_default(self) -> bool {
        matches!(self, Self::Vision | Self::ContextCompression | Self::BotDraft)
    }
}

pub const FUNCTIONS: [FunctionId; 13] = [
    FunctionId::Vision,
    FunctionId::ContextCompression,
    FunctionId::MessageTriage,
    FunctionId::SkillsHub,
    FunctionId::SkillSelection,
    FunctionId::ApprovalJudge,
    FunctionId::McpRouting,
    FunctionId::MemoryConsolidation,
    FunctionId::BotNaming,
    FunctionId::BotDraft,
    FunctionId::VoiceLive,
    FunctionId::SpeechToText,
    FunctionId::TextToSpeech,
];
