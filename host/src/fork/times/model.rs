use crate::fork_avatars::AvatarConfig;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ReportsTo {
    Owner {},
    Bot {
        #[serde(rename = "botId")]
        bot_id: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Role {
    Manager,
    Executor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum OwnerContact {
    Reports,
    UrgentOnly,
    OnRequest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Node {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub manager_bot_id: Option<String>,
    pub position: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Member {
    pub bot_id: String,
    pub team_id: Option<String>,
    pub parent_bot_id: Option<String>,
    pub role: Role,
    pub reports_to: ReportsTo,
    pub owner_contact: OwnerContact,
}

impl Member {
    pub fn initial(bot_id: String, primary: bool) -> Self {
        Self {
            bot_id,
            team_id: None,
            parent_bot_id: None,
            role: if primary { Role::Manager } else { Role::Executor },
            reports_to: ReportsTo::Owner {},
            owner_contact: OwnerContact::Reports,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RouteState {
    Proposed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Route {
    pub from_bot_id: String,
    pub to_bot_id: String,
    pub state: RouteState,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Graph {
    pub nodes: Vec<Node>,
    pub members: Vec<Member>,
    pub routes: Vec<Route>,
}

impl Graph {
    pub fn sort(&mut self) {
        self.nodes.sort_by(|a, b| a.id.cmp(&b.id));
        self.members.sort_by(|a, b| a.bot_id.cmp(&b.bot_id));
        self.routes.sort_by(|a, b| (&a.from_bot_id, &a.to_bot_id).cmp(&(&b.from_bot_id, &b.to_bot_id)));
    }
}

/// DTO fechado: nunca serializar `BotConfig` na resposta ou no digest do catálogo.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Bot {
    pub id: String,
    pub name: String,
    pub space_id: String,
    pub title: String,
    pub avatar_color: String,
    pub avatar_shape: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_config: Option<AvatarConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mexeriquinha: Option<crate::fork_avatars::MexeriquinhaConfig>,
    pub hidden: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Snapshot {
    pub version: u32,
    pub space_id: String,
    pub revision: i64,
    pub catalog_version: String,
    pub primary_bot_id: Option<String>,
    pub bots: Vec<Bot>,
    #[serde(flatten)]
    pub graph: Graph,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Read {
    pub space_id: Option<String>,
}

/// Os dois métodos usam o mesmo conteúdo fechado; api recusa nonce até nulo na prévia.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Change {
    pub space_id: Option<String>,
    pub expected_revision: i64,
    pub catalog_version: String,
    pub nodes: Vec<Node>,
    pub members: Vec<Member>,
    pub routes: Vec<Route>,
    pub client_request_id: Option<String>,
}

impl Change {
    pub fn graph(&self) -> Graph {
        Graph { nodes: self.nodes.clone(), members: self.members.clone(), routes: self.routes.clone() }
    }
}
