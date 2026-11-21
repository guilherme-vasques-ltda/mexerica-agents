//! RPCs administrativas do Mail Manager: estado do plugin, caixas conectadas e
//! concessões por participante, sem expor conteúdo de mensagens ou credenciais.
use super::{
    model::{MailError, Participant, Permission, db_error},
    store,
};
use crate::{
    LockExt,
    api::devices::{Caller, Forbidden},
    fork::spaces::PERSONAL,
    hub::Hub,
    store::{BotConfig, BotKind},
};
use anyhow::{Result, anyhow, bail};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) const METHODS: &[&str] = &[
    "donnaMailManager",
    "donnaMailManagerSave",
    "donnaMailManagerConnect",
    "donnaMailManagerDisconnect",
    "donnaMailManagerGrant",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmptyRequest {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveRequest {
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConnectRequest {
    label: String,
    addresses: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DisconnectRequest {
    mailbox_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ParticipantKindInput {
    Bot,
    Person,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum PermissionInput {
    View,
    Send,
    Delete,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GrantRequest {
    mailbox_id: String,
    participant_kind: ParticipantKindInput,
    participant_id: String,
    permission: PermissionInput,
    allowed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginView {
    enabled: bool,
    mode: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BotView {
    bot_id: String,
    name: String,
    is_primary: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GrantView {
    participant_kind: String,
    participant_id: String,
    view: bool,
    send: bool,
    delete: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MailboxView {
    id: String,
    label: String,
    addresses: Vec<String>,
    grants: Vec<GrantView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminSnapshot {
    plugin: PluginView,
    transport_available: bool,
    person_grants_supported: bool,
    bots: Vec<BotView>,
    mailboxes: Vec<MailboxView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    connected_mailbox_id: Option<String>,
}

pub(crate) fn call(hub: &Hub, caller: &Caller, method: &str, mut body: Value) -> Result<Value> {
    if !matches!(caller, Caller::Local) {
        return Err(Forbidden.into());
    }
    validate_and_strip_space(&mut body)?;
    let db = hub.store.db.locked();
    match method {
        "donnaMailManager" => {
            let _: EmptyRequest = parse(body)?;
            snapshot(&db, None)
        }
        "donnaMailManagerSave" => {
            let req: SaveRequest = parse(body)?;
            store::set_enabled(&db, req.enabled).map_err(map_mail_error)?;
            snapshot(&db, None)
        }
        "donnaMailManagerConnect" => {
            let req: ConnectRequest = parse(body)?;
            let refs: Vec<&str> = req.addresses.iter().map(String::as_str).collect();
            let id = store::connect_mailbox(&db, &req.label, &refs).map_err(map_mail_error)?;
            snapshot(&db, Some(id))
        }
        "donnaMailManagerDisconnect" => {
            let req: DisconnectRequest = parse(body)?;
            if req.mailbox_id.trim().is_empty() {
                bail!("mail_invalid: identificador da caixa inválido.");
            }
            store::disconnect_mailbox(&db, &req.mailbox_id).map_err(map_mail_error)?;
            snapshot(&db, None)
        }
        "donnaMailManagerGrant" => {
            let req: GrantRequest = parse(body)?;
            if req.mailbox_id.trim().is_empty() || req.participant_id.trim().is_empty() {
                bail!("mail_invalid: identificador inválido.");
            }
            let who = match req.participant_kind {
                ParticipantKindInput::Bot => Participant::Bot(req.participant_id),
                ParticipantKindInput::Person => Participant::Person(req.participant_id),
            };
            let perm = match req.permission {
                PermissionInput::View => Permission::View,
                PermissionInput::Send => Permission::Send,
                PermissionInput::Delete => Permission::Delete,
            };
            store::set_grant(&db, &req.mailbox_id, &who, perm, req.allowed).map_err(map_mail_error)?;
            snapshot(&db, None)
        }
        _ => bail!("mail_invalid: método desconhecido do Mail Manager."),
    }
}

fn validate_and_strip_space(body: &mut Value) -> Result<()> {
    let Some(obj) = body.as_object_mut() else {
        bail!("mail_invalid: pedido do Mail Manager inválido.");
    };
    match obj.get("spaceId") {
        None | Some(Value::Null) => {}
        Some(Value::String(space)) if space == PERSONAL => {}
        Some(Value::String(_)) => bail!("mail_denied: Mail Manager disponível apenas no Space pessoal."),
        Some(_) => bail!("mail_invalid: spaceId inválido."),
    }
    obj.remove("spaceId");
    Ok(())
}

fn parse<T: serde::de::DeserializeOwned>(body: Value) -> Result<T> {
    serde_json::from_value(body).map_err(|_| anyhow!("mail_invalid: pedido do Mail Manager inválido."))
}

fn map_mail_error(error: MailError) -> anyhow::Error {
    match error {
        MailError::Denied => anyhow!("mail_denied: operação não permitida pelo Mail Manager."),
        MailError::SenderNotAllowed => anyhow!("mail_sender_not_allowed: remetente não pertence à caixa."),
        MailError::Invalid => anyhow!("mail_invalid: dados inválidos para o Mail Manager."),
        MailError::NotFound => anyhow!("mail_not_found: recurso não encontrado."),
        MailError::KeyReused => anyhow!("mail_key_reused: chave de operação já utilizada."),
        MailError::Unconfirmed => anyhow!("mail_unconfirmed: operação sem confirmação."),
        MailError::Rejected => anyhow!("mail_rejected: operação recusada pelo provedor."),
        MailError::Storage => anyhow!("mail_storage_unavailable: armazenamento do Mail Manager indisponível."),
    }
}

fn snapshot(c: &Connection, connected_mailbox_id: Option<String>) -> Result<Value> {
    let enabled = store::enabled(c).map_err(map_mail_error)?;
    let bots = eligible_bots(c)?;
    let mailboxes = store::mailboxes(c)
        .map_err(map_mail_error)?
        .into_iter()
        .map(|mailbox| {
            let mut grouped: BTreeMap<(String, String), (bool, bool, bool)> = BTreeMap::new();
            for (who, permission) in mailbox.grants {
                let entry =
                    grouped.entry((who.kind().to_owned(), who.id().to_owned())).or_insert((false, false, false));
                match permission {
                    Permission::View => entry.0 = true,
                    Permission::Send => entry.1 = true,
                    Permission::Delete => entry.2 = true,
                }
            }
            let grants = grouped
                .into_iter()
                .map(|((participant_kind, participant_id), (view, send, delete))| GrantView {
                    participant_kind,
                    participant_id,
                    view,
                    send,
                    delete,
                })
                .collect();
            MailboxView { id: mailbox.id, label: mailbox.label, addresses: mailbox.addresses, grants }
        })
        .collect();
    let view = AdminSnapshot {
        plugin: PluginView { enabled, mode: "preview" },
        transport_available: false,
        person_grants_supported: false,
        bots,
        mailboxes,
        connected_mailbox_id,
    };
    serde_json::to_value(view).map_err(|_| {
        tracing::warn!(operation = "serialize_snapshot", "mail_manager: falha ao serializar estado");
        map_mail_error(MailError::Storage)
    })
}

fn eligible_bots(c: &Connection) -> Result<Vec<BotView>> {
    let primary_bot_id = crate::store::read_team_settings(c)
        .map_err(|_| {
            tracing::warn!(
                operation = "read_team_settings",
                error_kind = "storage",
                "mail_manager: falha ao ler configurações do time"
            );
            map_mail_error(MailError::Storage)
        })?
        .primary_bot_id;
    let mut stmt = c.prepare("SELECT id,config FROM bots WHERE deleted=0").map_err(db_error).map_err(map_mail_error)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(db_error)
        .map_err(map_mail_error)?;
    let mut bots = Vec::new();
    for row in rows {
        let (id, raw) = row.map_err(db_error).map_err(map_mail_error)?;
        let Ok(cfg) = serde_json::from_str::<BotConfig>(&raw) else {
            continue;
        };
        if cfg.id == id && cfg.space_id == PERSONAL && matches!(cfg.kind, BotKind::Agent) {
            let is_primary = primary_bot_id.as_deref() == Some(cfg.id.as_str());
            bots.push(BotView { bot_id: cfg.id, name: cfg.name, is_primary });
        }
    }
    bots.sort_by(|a, b| {
        (!a.is_primary)
            .cmp(&!b.is_primary)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.bot_id.cmp(&b.bot_id))
    });
    Ok(bots)
}
