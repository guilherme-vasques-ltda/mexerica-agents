//! DTOs descritivos v1. Não transportar nomes, corpos, credenciais ou erros brutos.
//!
//! `parse_json` sanitiza falhas. Deserialize também existe para composição
//! interna, mas sua mensagem serde pode ecoar entrada: consumidores não devem
//! expô-la em logs, recibos, UI ou ao modelo. Nenhum JSON constrói autoridade.
use crate::fork::escopo::{Id, Revision, Version};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PluginError {
    InvalidDescriptor,
    OwnerMismatch,
    SnapshotConflict,
    InvalidTransition,
    CounterExhausted,
    NotFound,
    PolicyDenied,
    Constraint,
    Storage,
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidDescriptor => "plugin_invalid_descriptor",
            Self::OwnerMismatch => "plugin_owner_mismatch",
            Self::SnapshotConflict => "plugin_snapshot_conflict",
            Self::InvalidTransition => "plugin_invalid_transition",
            Self::CounterExhausted => "plugin_counter_exhausted",
            Self::NotFound => "plugin_not_found",
            Self::PolicyDenied => "plugin_policy_denied",
            Self::Constraint => "plugin_constraint",
            Self::Storage => "plugin_storage_unavailable",
        })
    }
}

impl std::error::Error for PluginError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InstallationState {
    Disabled,
    Enabled,
}

/// Snapshot descritivo; `enabled` não comprova conta, grant ou autorização.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginInstallationRef {
    pub version: Version,
    pub hub_id: Id,
    pub installation_id: Id,
    pub generation: Revision,
    pub revision: Revision,
    pub state: InstallationState,
}

impl PluginInstallationRef {
    pub fn parse_json(bytes: &[u8]) -> Result<Self, PluginError> {
        parse_descriptor(bytes)
    }
}

/// Descreve uma conta externa vinculada à geração exata da instalação.
/// Não atesta conexão, posse ou cofre; secret-ref pertence ao recorte de custódia.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginAccountRef {
    pub version: Version,
    pub installation_ref: PluginInstallationRef,
    pub account_id: Id,
    pub generation: Revision,
    pub revision: Revision,
}

impl PluginAccountRef {
    pub fn parse_json(bytes: &[u8]) -> Result<Self, PluginError> {
        parse_descriptor(bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub(crate) enum PluginResourceOwner {
    /// Nativo pertence ao Hub/Space; não usa conta externa fictícia.
    Native {
        hub_id: Id,
        space_id: Id,
    },
    External {
        account_ref: PluginAccountRef,
    },
}

/// Vocabulário mínimo de identidade, sem capacidades ou efeitos concedidos.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PluginResourceKind {
    Calendar,
    /// O número de `WhatsApp` conectado de um bot (R3); sempre externo, dono = conta.
    WhatsappNumber,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResourceWire {
    version: Version,
    installation_ref: PluginInstallationRef,
    owner: PluginResourceOwner,
    resource_id: Id,
    kind: PluginResourceKind,
    /// Geração da encarnação do recurso, não versão de conteúdo.
    generation: Revision,
    revision: Revision,
}

/// Somente descrição. Igualdade inclui proprietário, identidades e gerações;
/// não há resolução de recurso, consulta de grant ou admissão nesta comparação.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ResourceWire", into = "ResourceWire")]
pub(crate) struct PluginResourceRef(ResourceWire);

impl TryFrom<ResourceWire> for PluginResourceRef {
    type Error = PluginError;

    fn try_from(value: ResourceWire) -> Result<Self, Self::Error> {
        let same_installation = match &value.owner {
            PluginResourceOwner::Native { hub_id, .. } => *hub_id == value.installation_ref.hub_id,
            PluginResourceOwner::External { account_ref } => account_ref.installation_ref == value.installation_ref,
        };
        if !same_installation {
            return Err(PluginError::OwnerMismatch);
        }
        if matches!(&value.owner, PluginResourceOwner::Native { .. }) && value.kind != PluginResourceKind::Calendar {
            return Err(PluginError::InvalidDescriptor);
        }
        Ok(Self(value))
    }
}

impl From<PluginResourceRef> for ResourceWire {
    fn from(value: PluginResourceRef) -> Self {
        value.0
    }
}

impl PluginResourceRef {
    pub fn parse_json(bytes: &[u8]) -> Result<Self, PluginError> {
        parse_descriptor(bytes)
    }

    pub(super) fn new(
        installation_ref: PluginInstallationRef,
        owner: PluginResourceOwner,
        resource_id: Id,
        kind: PluginResourceKind,
        generation: Revision,
        revision: Revision,
    ) -> Result<Self, PluginError> {
        ResourceWire {
            version: installation_ref.version,
            installation_ref,
            owner,
            resource_id,
            kind,
            generation,
            revision,
        }
        .try_into()
    }

    pub(super) fn owner(&self) -> &PluginResourceOwner {
        &self.0.owner
    }

    pub(super) fn resource_id(&self) -> &Id {
        &self.0.resource_id
    }

    pub(super) fn kind(&self) -> PluginResourceKind {
        self.0.kind
    }

    pub(super) fn generation(&self) -> Revision {
        self.0.generation
    }

    pub(super) fn revision(&self) -> Revision {
        self.0.revision
    }

    pub fn installation_ref(&self) -> &PluginInstallationRef {
        &self.0.installation_ref
    }
}

fn parse_descriptor<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, PluginError> {
    // Limite do pequeno envelope descritivo; não é canal de conteúdo.
    if bytes.len() > 8192 {
        return Err(PluginError::InvalidDescriptor);
    }
    serde_json::from_slice(bytes).map_err(|_| PluginError::InvalidDescriptor)
}
