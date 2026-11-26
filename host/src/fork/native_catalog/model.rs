use crate::fork_avatars::AvatarConfig;
use serde::{Deserialize, Serialize};

/// O despacho valida e retira hubId antes de desserializar este corpo fechado.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Read {
    pub space_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Preview {
    pub hub_id: String,
    pub space_id: String,
    pub catalog_version: u32,
    pub profiles: Vec<NativeProfile>,
    pub links: Vec<Link>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Origin {
    DonnaOriginal,
}

/// description é a Alma; title é função de apresentação, sem estilo ou grants.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativeProfile {
    pub catalog_key: &'static str,
    pub profile_version: u32,
    pub origin: Origin,
    pub name: &'static str,
    pub title: &'static str,
    pub avatar_config: AvatarConfig,
    pub description: &'static str,
    pub examples: [&'static str; 2],
    /// Sugestão editorial, sem afirmar inventário, acesso, conta ou prontidão.
    pub suggested_resources: &'static str,
}

/// Uma intenção explícita da prévia, nunca uma configuração de bot.
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Add {
    pub space_id: String,
    pub catalog_key: String,
    pub profile_version: u32,
    pub client_request_key: String,
    pub intent: Intent,
    pub expected_generation: i64,
    pub expected_revision: i64,
    pub backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_account_id: Option<String>,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) enum Intent {
    Add,
    Readd,
}
impl Intent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Readd => "readd",
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Link {
    pub catalog_key: String,
    pub generation: i64,
    pub revision: i64,
    pub bot_id: Option<String>,
    pub profile_version: Option<u32>,
    pub state: LinkState,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum LinkState {
    Available,
    Active,
    Removed,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Added {
    pub hub_id: String,
    pub space_id: String,
    pub catalog_key: String,
    pub client_request_key: String,
    pub generation: i64,
    pub bot_id: String,
    pub state: LinkState,
}
