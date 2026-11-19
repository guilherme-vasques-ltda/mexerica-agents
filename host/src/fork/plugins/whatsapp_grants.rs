//! `WhatsApp` R3 (D247): o número conectado é CONTA + RECURSO externos do registro (mesmo
//! ID, `account_id` do módulo, novo a cada pareamento) e "este bot pode ver conversas/grupos"
//! é CONCESSÃO do registro. Só `fork::whatsapp` chama, pelo caminho do dono ou do leitor,
//! sempre na `Connection`/transação do chamador. A checagem "pode ver" é só [`can_read`].
//! R4c: "pode responder" também é concessão do registro (escrita externa com cartão), e a
//! checagem é só [`can_reply`]. Ninguém concede ainda (D360: a ferramenta do R4c, depois da
//! 05b); a opção "Responder" do dono é só preferência e só revoga.
use super::{
    manifest::{Builtin, Capability},
    model::{InstallationState, PluginError, PluginResourceKind, PluginResourceOwner, PluginResourceRef},
    store::{
        account, add_account_in, add_resource_in, builtin_installation, current_installation, db_error, first,
        grant_in, identifier, resource,
    },
};
use crate::fork::escopo::{ContextRef, Id};
use rusqlite::{Connection, OptionalExtension, params};

type Result<T> = std::result::Result<T, PluginError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Read {
    Chats,
    Groups,
}

/// Ver (R3) ou responder (R4c) em conversas ou grupos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Access {
    See,
    Reply,
}

impl Read {
    fn capability(self, access: Access) -> Capability {
        match (access, self) {
            (Access::See, Self::Chats) => Capability::WhatsappReadChats,
            (Access::See, Self::Groups) => Capability::WhatsappReadGroups,
            (Access::Reply, Self::Chats) => Capability::WhatsappReplyChats,
            (Access::Reply, Self::Groups) => Capability::WhatsappReplyGroups,
        }
    }
}

impl Access {
    fn effect(self) -> &'static str {
        match self {
            Self::See => "read",
            Self::Reply => "external_write_with_card",
        }
    }
}

/// Garante a conta e o recurso do número. Plugin desligado ou nunca ligado: nada
/// (`false`). Repetir não cria nada novo.
pub(crate) fn link(c: &Connection, hub: &Id, account_id: &str) -> Result<bool> {
    let Some(installation) = builtin_installation(c, hub, Builtin::Whatsapp)? else { return Ok(false) };
    if installation.state != InstallationState::Enabled {
        return Ok(false);
    }
    let id = identifier(account_id.to_owned())?;
    if account(c, &installation, &id).is_ok() {
        return Ok(true);
    }
    let account_ref = add_account_in(c, &installation, &id)?;
    let resource = PluginResourceRef::new(
        installation,
        PluginResourceOwner::External { account_ref },
        id,
        PluginResourceKind::WhatsappNumber,
        first()?,
        first()?,
    )?;
    add_resource_in(c, &resource)?;
    Ok(true)
}

/// Desconectar/apagar o número ou o bot: concessões, recurso e conta somem juntos.
pub(crate) fn unlink(c: &Connection, account_id: &str) -> Result<()> {
    const MINE: &str = "installation_id IN (SELECT id FROM donna_plugin_installations WHERE builtin='whatsapp')";
    c.execute(&format!("DELETE FROM donna_plugin_grants WHERE resource_id=?1 AND {MINE}"), [account_id])
        .map_err(db_error)?;
    c.execute(
        &format!("DELETE FROM donna_plugin_resources WHERE id=?1 AND kind='whatsapp_number' AND {MINE}"),
        [account_id],
    )
    .map_err(db_error)?;
    c.execute(&format!("DELETE FROM donna_plugin_accounts WHERE id=?1 AND {MINE}"), [account_id]).map_err(db_error)?;
    Ok(())
}

/// Concede ou revoga "ver" ou "responder" para o bot no número. Conceder exige plugin ligado, conta
/// atual e o bot no contexto pessoal (gatilhos e `grant_in`); sem contexto, nega.
/// Revogar nunca falha por isso.
pub(crate) fn set(
    c: &Connection,
    hub: &Id,
    account_id: &str,
    bot: &str,
    context: Option<&ContextRef>,
    (read, access): (Read, Access),
    want: bool,
) -> Result<()> {
    let active: Option<String> = c
        .query_row(
            "SELECT g.id FROM donna_plugin_grants g JOIN donna_plugin_installations i ON i.id=g.installation_id
             WHERE i.hub_id=?1 AND i.builtin='whatsapp' AND g.resource_id=?2 AND g.bot_id=?3 AND g.capability=?4
             AND g.state='active' AND g.installation_generation=i.generation LIMIT 1",
            params![hub.as_str(), account_id, bot, read.capability(access).key()],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?;
    match (want, active) {
        (true, None) => {
            let context = context.filter(|ctx| ctx.bot_id().as_str() == bot).ok_or(PluginError::PolicyDenied)?;
            let installation = builtin_installation(c, hub, Builtin::Whatsapp)?.ok_or(PluginError::NotFound)?;
            current_installation(c, &installation)?;
            let expected = resource(c, &installation, &identifier(account_id.to_owned())?)?;
            let id = identifier(uuid::Uuid::new_v4().to_string())?;
            grant_in(c, &id, &expected, context, read.capability(access))?;
        }
        (false, Some(id)) => {
            c.execute(
                "UPDATE donna_plugin_grants SET state='revoked',generation=generation+1,revision=revision+1
                 WHERE id=?1 AND state='active'",
                [id],
            )
            .map_err(db_error)?;
        }
        _ => {}
    }
    Ok(())
}

/// A única checagem de "este bot pode ver": instalação ligada, conta e recurso atuais do
/// número e concessão ativa da geração atual para o bot vivo.
pub(crate) fn can_read(c: &Connection, hub: &str, account_id: &str, bot: &str, read: Read) -> Result<bool> {
    can(c, hub, account_id, bot, read, Access::See)
}

/// R4c: a única checagem de "este bot pode responder", com as mesmas condições de [`can_read`].
pub(crate) fn can_reply(c: &Connection, hub: &str, account_id: &str, bot: &str, read: Read) -> Result<bool> {
    can(c, hub, account_id, bot, read, Access::Reply)
}

fn can(c: &Connection, hub: &str, account_id: &str, bot: &str, read: Read, access: Access) -> Result<bool> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_plugin_installations i
         JOIN donna_plugin_resources r ON r.installation_id=i.id
         JOIN donna_plugin_accounts a ON a.id=r.account_id AND a.installation_id=i.id
         JOIN donna_plugin_grants g ON g.installation_id=i.id AND g.resource_id=r.id
         JOIN bots b ON b.id=g.bot_id AND b.deleted=0
         WHERE i.hub_id=?1 AND i.builtin='whatsapp' AND i.state='enabled'
         AND r.id=?2 AND r.owner='external' AND r.kind='whatsapp_number' AND a.id=?2
         AND g.bot_id=?3 AND g.capability=?4 AND g.effect=?5 AND g.state='active'
         AND g.installation_generation=i.generation AND g.resource_generation=r.generation
         AND g.resource_revision=r.revision AND g.account_generation=a.generation AND g.account_revision=a.revision)",
        params![hub, account_id, bot, read.capability(access).key(), access.effect()],
        |r| r.get(0),
    )
    .map_err(db_error)
}

#[derive(Clone, PartialEq, Eq)]
struct ReadStamp {
    installation: String,
    account: String,
    resource: String,
    grant: String,
    context: String,
    revisions: [i64; 8],
}

/// Só captura interna produz este snapshot; descritor de RPC não o reconstrói.
pub(crate) struct ReadSnapshot {
    hub: String,
    account: String,
    bot: String,
    read: Read,
    stamps: Vec<ReadStamp>,
}

pub(crate) fn capture_read_on(c: &Connection, hub: &str, account: &str, bot: &str, read: Read) -> Result<ReadSnapshot> {
    if !can_read(c, hub, account, bot, read)? {
        return Err(PluginError::PolicyDenied);
    }
    let mut statement = c
        .prepare(
            "SELECT i.id,a.id,r.id,g.id,g.context,i.generation,i.revision,
                a.generation,a.revision,r.generation,r.revision,g.generation,g.revision
         FROM donna_plugin_installations i
         JOIN donna_plugin_resources r ON r.installation_id=i.id
         JOIN donna_plugin_accounts a ON a.id=r.account_id AND a.installation_id=i.id
         JOIN donna_plugin_grants g ON g.installation_id=i.id AND g.resource_id=r.id
         WHERE i.hub_id=?1 AND i.builtin='whatsapp' AND i.state='enabled'
           AND r.id=?2 AND r.owner='external' AND r.kind='whatsapp_number' AND a.id=?2
           AND g.bot_id=?3 AND g.capability=?4 AND g.effect='read' AND g.state='active'
           AND g.installation_generation=i.generation AND g.resource_generation=r.generation
           AND g.resource_revision=r.revision AND g.account_generation=a.generation
           AND g.account_revision=a.revision ORDER BY g.id",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map(params![hub, account, bot, read.capability(Access::See).key()], |r| {
            Ok(ReadStamp {
                installation: r.get(0)?,
                account: r.get(1)?,
                resource: r.get(2)?,
                grant: r.get(3)?,
                context: r.get(4)?,
                revisions: [r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?],
            })
        })
        .map_err(db_error)?;
    let stamps = rows.collect::<std::result::Result<Vec<_>, _>>().map_err(db_error)?;
    if stamps.is_empty() {
        return Err(PluginError::PolicyDenied);
    }
    Ok(ReadSnapshot { hub: hub.into(), account: account.into(), bot: bot.into(), read, stamps })
}

pub(crate) fn check_read_on(c: &Connection, original: &ReadSnapshot) -> Result<()> {
    let current = capture_read_on(c, &original.hub, &original.account, &original.bot, original.read)?;
    if current.stamps != original.stamps {
        return Err(PluginError::SnapshotConflict);
    }
    Ok(())
}
