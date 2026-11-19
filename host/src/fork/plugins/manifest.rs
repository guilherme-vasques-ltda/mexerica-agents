//! Bundle próprio e fechado. Sugerir e `AllAgenda` pertencem a recortes futuros;
//! nenhuma capacidade declarada equivale a grant. Licenças de auxiliares são
//! evidência separada, sem importação nem alegação de distribuição neste bundle.
//! Descrições não instalam nem concedem acesso.
use super::model::PluginError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Builtin {
    Scatty,
    /// Instalação (ligado/desligado) e, desde o R3, o número conectado como conta e
    /// recurso externos; "ver conversas/grupos" são concessões por bot (`whatsapp_grants`).
    Whatsapp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Capability {
    CalendarAvailability,
    CalendarRead,
    CalendarCreate,
    CalendarUpdate,
    /// R3: ver mensagens de conversas individuais do número. Responder fica para o R4.
    WhatsappReadChats,
    /// R3: ver mensagens de grupos do número.
    WhatsappReadGroups,
    /// R4c: responder conversas individuais (escrita externa sempre com cartão).
    WhatsappReplyChats,
    /// R4c: responder grupos (escrita externa sempre com cartão).
    WhatsappReplyGroups,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Effect {
    Read,
    NativeWriteWithCard,
    ExternalReadWithCard,
    /// R4c: escrita fora do Hub (mensagem do `WhatsApp`), só por cartão do dono.
    ExternalWriteWithCard,
}

pub(super) struct Definition {
    pub id: Builtin,
    pub name: &'static str,
    pub publisher: &'static str,
    pub origin: &'static str,
    pub provenance: &'static str,
    pub distribution: &'static str,
    pub version: u8,
    pub capabilities: &'static [Capability],
}

pub(super) const BUILTINS: [Definition; 2] = [
    Definition {
        id: Builtin::Scatty,
        name: "Scatty",
        publisher: "Mexerica",
        origin: "bundle_proprio_donna",
        provenance: "wrapper_proprio_sem_importacao_de_auxiliar",
        distribution: "licencas_de_auxiliares_nao_avaliadas",
        version: 1,
        capabilities: &[
            Capability::CalendarAvailability,
            Capability::CalendarRead,
            Capability::CalendarCreate,
            Capability::CalendarUpdate,
        ],
    },
    Definition {
        id: Builtin::Whatsapp,
        name: "WhatsApp (não oficial)",
        publisher: "Mexerica",
        origin: "bundle_proprio_donna",
        provenance: "wrapper_proprio_sem_importacao_de_auxiliar",
        distribution: "licencas_de_auxiliares_nao_avaliadas",
        version: 1,
        capabilities: &[
            Capability::WhatsappReadChats,
            Capability::WhatsappReadGroups,
            Capability::WhatsappReplyChats,
            Capability::WhatsappReplyGroups,
        ],
    },
];

impl Builtin {
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Scatty => "scatty",
            Self::Whatsapp => "whatsapp",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, PluginError> {
        match value {
            "scatty" => Ok(Self::Scatty),
            "whatsapp" => Ok(Self::Whatsapp),
            _ => Err(PluginError::InvalidDescriptor),
        }
    }
}

impl Capability {
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::CalendarAvailability => "calendar_availability",
            Self::CalendarRead => "calendar_read",
            Self::CalendarCreate => "calendar_create",
            Self::CalendarUpdate => "calendar_update",
            Self::WhatsappReadChats => "whatsapp_read_chats",
            Self::WhatsappReadGroups => "whatsapp_read_groups",
            Self::WhatsappReplyChats => "whatsapp_reply_chats",
            Self::WhatsappReplyGroups => "whatsapp_reply_groups",
        }
    }
}

impl Effect {
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::NativeWriteWithCard => "native_write_with_card",
            Self::ExternalReadWithCard => "external_read_with_card",
            Self::ExternalWriteWithCard => "external_write_with_card",
        }
    }
}
