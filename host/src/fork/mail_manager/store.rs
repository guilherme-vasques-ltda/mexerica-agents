//! Estado persistido do Mail Manager e a única checagem de permissão ([`allowed`]).
//! Funções de administração devolvem só estado operacional e concessões.
use super::model::{MailError, MailboxAdmin, Participant, Permission, Result, address, db_error};
use crate::fork::spaces::PERSONAL;
use rusqlite::{Connection, OptionalExtension, params};

/// Bot vivo, agente e do Pessoal: único participante que pode ter concessão hoje.
const LIVE_BOT: &str = "SELECT 1 FROM bots b WHERE b.id=?1 AND b.deleted=0 AND json_valid(b.config)
    AND json_extract(b.config,'$.id')=b.id AND json_extract(b.config,'$.spaceId')=?2
    AND COALESCE(json_extract(b.config,'$.kind'),'agent')='agent'";

pub(super) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(include_str!("schema.sql")).map_err(db_error)
}

pub(crate) fn enabled(c: &Connection) -> Result<bool> {
    c.query_row("SELECT EXISTS(SELECT 1 FROM mail_manager_plugin WHERE singleton=1 AND state='enabled')", [], |r| {
        r.get(0)
    })
    .map_err(db_error)
}

/// Liga ou desliga o plugin. Desligar revoga todas as concessões; religar não as devolve.
pub(crate) fn set_enabled(c: &Connection, on: bool) -> Result<()> {
    let tx = c.unchecked_transaction().map_err(db_error)?;
    tx.execute(
        "INSERT INTO mail_manager_plugin(singleton,state) VALUES(1,?1)
         ON CONFLICT(singleton) DO UPDATE SET state=excluded.state",
        [if on { "enabled" } else { "disabled" }],
    )
    .map_err(db_error)?;
    if !on {
        tx.execute("UPDATE mail_manager_grants SET state='revoked' WHERE state='active'", []).map_err(db_error)?;
    }
    tx.commit().map_err(db_error)
}

/// Conecta uma caixa com seus endereços remetentes. Cada endereço pertence a uma só caixa.
/// Conectar não concede nada a ninguém.
pub(crate) fn connect_mailbox(c: &Connection, label: &str, addresses: &[&str]) -> Result<String> {
    if !enabled(c)? {
        return Err(MailError::Denied);
    }
    let label = label.trim();
    if label.is_empty() || label.chars().any(char::is_control) || addresses.is_empty() {
        return Err(MailError::Invalid);
    }
    let addresses = addresses.iter().map(|a| address(a)).collect::<Result<Vec<_>>>()?;
    let id = uuid::Uuid::new_v4().to_string();
    let tx = c.unchecked_transaction().map_err(db_error)?;
    tx.execute("INSERT INTO mail_manager_mailboxes(id,label) VALUES(?1,?2)", params![id, label]).map_err(db_error)?;
    for a in &addresses {
        let taken: bool = tx
            .query_row("SELECT EXISTS(SELECT 1 FROM mail_manager_addresses WHERE address=?1)", [a], |r| r.get(0))
            .map_err(db_error)?;
        if taken {
            return Err(MailError::Invalid);
        }
        tx.execute("INSERT INTO mail_manager_addresses(address,mailbox_id) VALUES(?1,?2)", params![a, id])
            .map_err(db_error)?;
    }
    tx.commit().map_err(db_error)?;
    Ok(id)
}

/// Desconecta a caixa: concessões, referências e endereços somem com ela, e as
/// referências emitidas antes deixam de valer.
pub(crate) fn disconnect_mailbox(c: &Connection, mailbox: &str) -> Result<()> {
    let tx = c.unchecked_transaction().map_err(db_error)?;
    for table in ["mail_manager_refs", "mail_manager_grants", "mail_manager_addresses"] {
        tx.execute(&format!("DELETE FROM {table} WHERE mailbox_id=?1"), [mailbox]).map_err(db_error)?;
    }
    tx.execute("DELETE FROM mail_manager_mailboxes WHERE id=?1", [mailbox]).map_err(db_error)?;
    tx.commit().map_err(db_error)
}

/// Concede ou revoga uma permissão. Conceder exige plugin ligado, caixa conectada e um
/// bot vivo do Pessoal; pessoa é recusada. Revogar nunca falha por isso e vale para a
/// próxima operação. Uma concessão revogada não volta: conceder de novo cria outra.
pub(crate) fn set_grant(
    c: &Connection,
    mailbox: &str,
    who: &Participant,
    permission: Permission,
    want: bool,
) -> Result<()> {
    let tx = c.unchecked_transaction().map_err(db_error)?;
    if want {
        let Participant::Bot(bot) = who else { return Err(MailError::Denied) };
        let ok: bool = tx
            .query_row(
                &format!(
                    "SELECT EXISTS(SELECT 1 FROM mail_manager_plugin WHERE singleton=1 AND state='enabled')
                     AND EXISTS(SELECT 1 FROM mail_manager_mailboxes WHERE id=?3) AND EXISTS({LIVE_BOT})"
                ),
                params![bot, PERSONAL, mailbox],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if !ok {
            return Err(MailError::Denied);
        }
        tx.execute(
            "INSERT INTO mail_manager_grants(id,mailbox_id,participant_kind,participant_id,permission,state)
             SELECT ?1,?2,'bot',?3,?4,'active' WHERE NOT EXISTS(SELECT 1 FROM mail_manager_grants
               WHERE mailbox_id=?2 AND participant_kind='bot' AND participant_id=?3 AND permission=?4 AND state='active')",
            params![uuid::Uuid::new_v4().to_string(), mailbox, bot, permission.key()],
        )
        .map_err(db_error)?;
    } else {
        tx.execute(
            "UPDATE mail_manager_grants SET state='revoked' WHERE mailbox_id=?1 AND participant_kind=?2
             AND participant_id=?3 AND permission=?4 AND state='active'",
            params![mailbox, who.kind(), who.id(), permission.key()],
        )
        .map_err(db_error)?;
    }
    tx.commit().map_err(db_error)
}

/// Na transação de exclusão do bot: todas as concessões dele ficam revogadas.
pub(crate) fn on_bot_deleted(c: &Connection, bot: &str) -> Result<()> {
    c.execute(
        "UPDATE mail_manager_grants SET state='revoked' WHERE participant_kind='bot' AND participant_id=?1 AND state='active'",
        [bot],
    )
    .map(drop)
    .map_err(db_error)
}

/// Caixas, endereços e concessões ativas, para o painel do administrador. Nenhum dado de
/// mensagem passa por aqui.
pub(crate) fn mailboxes(c: &Connection) -> Result<Vec<MailboxAdmin>> {
    let mut boxes: Vec<MailboxAdmin> = c
        .prepare("SELECT id,label FROM mail_manager_mailboxes ORDER BY label,id")
        .and_then(|mut s| {
            s.query_map([], |r| {
                Ok(MailboxAdmin { id: r.get(0)?, label: r.get(1)?, addresses: Vec::new(), grants: Vec::new() })
            })?
            .collect()
        })
        .map_err(db_error)?;
    for mailbox in &mut boxes {
        mailbox.addresses = c
            .prepare("SELECT address FROM mail_manager_addresses WHERE mailbox_id=?1 ORDER BY address")
            .and_then(|mut s| s.query_map([&mailbox.id], |r| r.get(0))?.collect())
            .map_err(db_error)?;
        let rows: Vec<(String, String, String)> = c
            .prepare(
                "SELECT participant_kind,participant_id,permission FROM mail_manager_grants
                 WHERE mailbox_id=?1 AND state='active' ORDER BY participant_kind,participant_id,permission",
            )
            .and_then(|mut s| s.query_map([&mailbox.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect())
            .map_err(db_error)?;
        mailbox.grants = rows
            .into_iter()
            .filter_map(|(kind, id, permission)| {
                let who = if kind == "bot" { Participant::Bot(id) } else { Participant::Person(id) };
                Some((who, Permission::parse(&permission)?))
            })
            .collect();
    }
    Ok(boxes)
}

/// A única checagem de permissão: plugin ligado, caixa conectada, bot vivo do Pessoal e
/// concessão ativa. Pessoa sempre é negada neste recorte.
pub(super) fn allowed(c: &Connection, mailbox: &str, who: &Participant, permission: Permission) -> Result<bool> {
    let Participant::Bot(bot) = who else { return Ok(false) };
    c.query_row(
        &format!(
            "SELECT EXISTS(SELECT 1 FROM mail_manager_plugin p
               JOIN mail_manager_mailboxes m ON m.id=?3
               JOIN mail_manager_grants g ON g.mailbox_id=m.id
             WHERE p.singleton=1 AND p.state='enabled' AND g.participant_kind='bot' AND g.participant_id=?1
               AND g.permission=?4 AND g.state='active' AND EXISTS({LIVE_BOT}))"
        ),
        params![bot, PERSONAL, mailbox, permission.key()],
        |r| r.get(0),
    )
    .map_err(db_error)
}

pub(super) fn require(c: &Connection, mailbox: &str, who: &Participant, permissions: &[Permission]) -> Result<()> {
    for &permission in permissions {
        if !allowed(c, mailbox, who, permission)? {
            return Err(MailError::Denied);
        }
    }
    Ok(())
}

/// Caixas que o participante pode ver agora.
pub(super) fn viewable(c: &Connection, who: &Participant) -> Result<Vec<String>> {
    let ids: Vec<String> = c
        .prepare("SELECT id FROM mail_manager_mailboxes ORDER BY id")
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .map_err(db_error)?;
    let mut out = Vec::new();
    for id in ids {
        if allowed(c, &id, who, Permission::View)? {
            out.push(id);
        }
    }
    Ok(out)
}

/// Quem pode receber aviso de mensagem nova na caixa: só quem pode vê-la agora.
pub(super) fn audience(c: &Connection, mailbox: &str) -> Result<Vec<Participant>> {
    let bots: Vec<String> = c
        .prepare(
            "SELECT DISTINCT participant_id FROM mail_manager_grants WHERE mailbox_id=?1 AND participant_kind='bot'
             AND permission='view' AND state='active' ORDER BY participant_id",
        )
        .and_then(|mut s| s.query_map([mailbox], |r| r.get(0))?.collect())
        .map_err(db_error)?;
    let mut out = Vec::new();
    for bot in bots {
        let who = Participant::Bot(bot);
        if allowed(c, mailbox, &who, Permission::View)? {
            out.push(who);
        }
    }
    Ok(out)
}

pub(super) fn sender_belongs(c: &Connection, mailbox: &str, sender: &str) -> Result<bool> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM mail_manager_addresses WHERE address=?1 AND mailbox_id=?2)",
        params![sender, mailbox],
        |r| r.get(0),
    )
    .map_err(db_error)
}

pub(super) fn default_sender(c: &Connection, mailbox: &str) -> Result<String> {
    c.query_row(
        "SELECT address FROM mail_manager_addresses WHERE mailbox_id=?1 ORDER BY address LIMIT 1",
        [mailbox],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_error)?
    .ok_or(MailError::SenderNotAllowed)
}

/// Emite (ou reaproveita) a referência opaca de uma mensagem ou anexo da caixa.
pub(super) fn mint(c: &Connection, mailbox: &str, message: &str, attachment: &str) -> Result<String> {
    c.execute(
        "INSERT INTO mail_manager_refs(handle,mailbox_id,message_id,attachment_id) VALUES(?1,?2,?3,?4)
         ON CONFLICT(mailbox_id,message_id,attachment_id) DO NOTHING",
        params![uuid::Uuid::new_v4().to_string(), mailbox, message, attachment],
    )
    .map_err(db_error)?;
    c.query_row(
        "SELECT handle FROM mail_manager_refs WHERE mailbox_id=?1 AND message_id=?2 AND attachment_id=?3",
        params![mailbox, message, attachment],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// A caixa, a mensagem e o anexo (vazio para mensagem) de uma referência, ou nada.
pub(super) fn resolve(c: &Connection, handle: &str) -> Result<Option<(String, String, String)>> {
    c.query_row(
        "SELECT r.mailbox_id,r.message_id,r.attachment_id FROM mail_manager_refs r
         JOIN mail_manager_mailboxes m ON m.id=r.mailbox_id WHERE r.handle=?1",
        [handle],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()
    .map_err(db_error)
}
