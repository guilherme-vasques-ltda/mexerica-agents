// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Row and wire types: bot configs, lanes, entries and their kinds.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// How a bot handles the agent's permission requests (wire values `ask` / `auto`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    /// Every request becomes an approval card on the phone.
    Ask,
    /// Requests are approved once, automatically. New bots start here.
    #[default]
    Auto,
}

/// Transcript entry kinds (the `kind` column / wire field).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    User,
    Agent,
    Thought,
    Tool,
    Plan,
    Permission,
    Notice,
}

impl EntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
            Self::Thought => "thought",
            Self::Tool => "tool",
            Self::Plan => "plan",
            Self::Permission => "permission",
            Self::Notice => "notice",
        }
    }
}

/// A roster row is an agent bot or a group chat of bots (wire values `agent` / `group`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BotKind {
    #[default]
    Agent,
    /// Several bots and the user in one conversation; it has no agent of its own (see `group`).
    Group,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[expect(clippy::struct_excessive_bools, reason = "independent user settings stored as plain JSON flags")]
pub struct BotConfig {
    pub id: String,
    #[serde(default = "crate::fork::spaces::personal_id")]
    pub space_id: String,
    #[serde(skip)]
    pub context: Option<crate::fork::spaces::ExecutionContext>,
    /// D6: a pessoa comprovada da conversa nesta execução, de quem é a nota "Sobre você" que ela pode receber.
    /// Só o ator a preenche, pela origem carimbada no turno; ausente, nenhuma nota entra.
    #[serde(skip)]
    pub person: Option<String>,
    #[serde(default)]
    pub kind: BotKind,
    /// A group's bots, in the order they were added.
    #[serde(default)]
    pub members: Vec<String>,
    /// O líder do canal, um dos `members`, ou nenhum (`fork::lider_canal`).
    #[serde(default)]
    pub leader: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// D368: "Suas preferências para a Donna", só do dono pelo app; entra depois da base no prompt.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner_preferences: String,
    #[serde(default = "default_color")]
    pub avatar_color: String,
    /// Grok-Bot-style character shape: blob | pebble | squircle | tablet | wedge | hex | cloud | teardrop.
    #[serde(default = "default_shape")]
    pub avatar_shape: String,
    /// D115: configuração própria, validada no Hub; ausente apenas em clientes antigos.
    #[serde(default)]
    pub avatar_config: Option<crate::fork_avatars::AvatarConfig>,
    /// Aparência v2 do fork; ignorada por versões antigas. Nunca vai em avatarConfig.
    #[serde(
        default,
        deserialize_with = "crate::fork_avatars::read_optional_mexeriquinha",
        skip_serializing_if = "Option::is_none"
    )]
    pub mexeriquinha: Option<crate::fork_avatars::MexeriquinhaConfig>,
    /// D133: a função do bot, mostrada como etiqueta ao lado do nome. Vazia em grupos e bots antigos.
    #[serde(default)]
    pub title: String,
    /// Empty for a group.
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub fork_editor: crate::fork::engine_setup::BotEngineSelection,
    #[serde(default)]
    pub command: Option<String>,
    /// Empty for a group.
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub permission: Permission,
    /// D78: escolha explícita do usuário; o recálculo geral não a substitui.
    /// Configs anteriores continuam herdando a regra da D67 até o usuário escolher.
    #[serde(default)]
    pub permission_chosen_by_user: bool,
    #[serde(default)]
    pub model: Option<String>,
    /// The model's own settings (effort, fast…): ACP config option id → value.
    #[serde(default)]
    pub model_options: BTreeMap<String, String>,
    /// Ask the agent for its Fast mode (Claude Code, Codex): faster replies for more usage.
    #[serde(default)]
    pub fast_mode: bool,
    #[serde(default)]
    pub pinned: bool,
    /// Place in the roster, lowest first (after pinned bots). All 0 until the user drags one:
    /// ties sort by recent activity (see `Hub::reorder_bots`).
    #[serde(default)]
    pub position: i64,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub notify: Option<bool>,
    /// Connector ids (MCP servers) handed to the agent; see `market`.
    #[serde(default)]
    pub connectors: Vec<String>,
    /// Marketplace: a conta padrão do bot em cada serviço (`id da ficha` → `id da conta`). Só vale para conta
    /// ligada neste bot (`connectors`) e do Space dele; o Hub confere na gravação e a cada pedido (`market::contas`).
    #[serde(default)]
    pub conta_padrao: BTreeMap<String, String>,
    /// Marketplace: serviços em que o bot sempre pergunta qual conta usar quando há mais de uma ligada.
    #[serde(default)]
    pub perguntar_conta: BTreeMap<String, bool>,
    /// Saved login ids (see `market::logins`) the bot may list and type (D82). Empty until the
    /// user turns one on, or saves or picks one on a `request_login` card this bot posted.
    #[serde(default)]
    pub logins: Vec<String>,
    /// Skill ids the bot is told about; see `market`.
    #[serde(default)]
    pub skills: Vec<String>,
    /// The bot gets the built-in `computer` MCP server (see `screen`).
    #[serde(default)]
    pub computer: bool,
    /// Preferência do dono; não concede computador, tela ou perfil.
    #[serde(default)]
    pub browser_preference: crate::fork::browser::BrowserPreference,
    /// MCP manage: apenas a principal configurada pelo usuário pode ter este poder.
    /// Bots criados por bot nascem sem ele (D67).
    #[serde(default)]
    pub manage_team: bool,
    /// The bot that created this one with `create_bot`; none when the user did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    /// Uses the memory shared by all bots ("yours"). Unset: on for a bot in its own workspace, off for one in a
    /// project folder (see `chat::memory::shared`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_memory: Option<bool>,
    /// Keeps the harness's own memory (Claude Code's auto memory…) on beside Codync's. Off unless the user turns it
    /// on, and only where Codync can turn it off (see `agent::own_memory::kept`). A client sending back the bot as
    /// it got it sends `null` where there is no switch.
    #[serde(default, deserialize_with = "null_as_false")]
    pub harness_memory: bool,
    /// Created without a name: the host names it from its first conversations (see `naming`)
    /// until the user renames it.
    #[serde(default)]
    pub auto_name: bool,
    #[serde(default)]
    pub created_at: i64,
}

impl BotConfig {
    pub fn session_owner(&self) -> &str {
        self.context.as_ref().map_or(&self.id, |c| c.conversation_id.as_str())
    }
    pub fn memory_id(&self) -> &str {
        self.context.as_ref().map_or(&self.id, |c| c.memory_id.as_str())
    }
    pub fn is_group(&self) -> bool {
        self.kind == BotKind::Group
    }
}

fn null_as_false<'de, D: serde::Deserializer<'de>>(value: D) -> Result<bool, D::Error> {
    Ok(Option::<bool>::deserialize(value)?.unwrap_or(false))
}

/// The avatar colors and shapes every client offers.
pub const AVATAR_COLORS: [&str; 11] =
    ["black", "brown", "red", "orange", "yellow", "green", "cyan", "blue", "violet", "magenta", "gray"];
pub const AVATAR_SHAPES: [&str; 8] = ["blob", "pebble", "squircle", "tablet", "wedge", "hex", "cloud", "teardrop"];

fn default_color() -> String {
    "blue".into()
}
fn default_shape() -> String {
    "blob".into()
}

/// kv key for the agent session of one of a bot's routine runs (`slot`: see `routines::session_slot`).
pub fn run_session_key(bot_id: &str, slot: &str) -> String {
    format!("lane.session.{bot_id}@{bot_id}/{slot}")
}

/// Where entries go and a turn talks: a bot's own chat or a group's (`chat`, a bot id), and in it a session
/// (`None` = the main one; see `fork::sessoes`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lane {
    pub chat: String,
    pub session: Option<String>,
}

impl Lane {
    pub fn main(chat: &str) -> Self {
        Self { chat: chat.to_owned(), session: None }
    }
    /// The session's id in the catalog: the main one has the chat's id.
    pub fn session_id(&self) -> &str {
        self.session.as_deref().unwrap_or(&self.chat)
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub seq: i64,
    pub bot_id: String,
    pub rev: i64,
    pub kind: String,
    pub turn: i64,
    pub data: Value,
    pub created_at: i64,
    pub updated_at: i64,
    /// The session it belongs to (the main one has the chat's id).
    pub session_id: String,
}

impl Entry {
    /// Mensagem escrita pelo usuário, incluindo clientes antigos sem metadados de origem.
    pub fn is_user_authored(&self) -> bool {
        // Antes do fallback legado: responder escolha não é pedido autoral confiável.
        !crate::fork::genui::is_choice_response(&self.data)
            && self.kind == EntryKind::User.as_str()
            && self.data["from"].is_null()
            && self.data["author"].is_null()
            && self.data["sourceBotId"].is_null()
            && self.data["botMessage"].is_null()
            && self.data["runId"].is_null()
    }
}

/// A message's text, or the names of its files when it has none.
pub fn message_text(data: &Value) -> String {
    match data["text"].as_str().unwrap_or_default() {
        "" => {
            let names: Vec<&str> =
                data["attachments"].as_array().into_iter().flatten().filter_map(|a| a["name"].as_str()).collect();
            names.join(", ")
        }
        text => text.to_owned(),
    }
}

pub struct LastMessage {
    pub text: String,
    pub at: i64,
    /// The bot that wrote it; `None` when the user did.
    pub author: Option<String>,
}

pub struct BotRow {
    pub config: BotConfig,
    /// c1: condição opaca da seleção; `None` se ausente ou inválida.
    pub skills_version: Option<String>,
    pub rev: i64,
    pub deleted: bool,
    pub session_id: Option<String>,
    pub read_rev: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_wire_values_round_trip() {
        let p: Permission = serde_json::from_str("\"auto\"").unwrap();
        assert_eq!(p, Permission::Auto);
        assert_eq!(serde_json::to_string(&Permission::Ask).unwrap(), "\"ask\"");
        assert!(serde_json::from_str::<Permission>("\"yolo\"").is_err(), "unknown modes are rejected at the boundary");
    }
}
