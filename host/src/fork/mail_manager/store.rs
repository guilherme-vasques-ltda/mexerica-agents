//! Estado persistido do Mail Manager e a única checagem de permissão ([`allowed`]).
//! Funções de administração devolvem só estado operacional e concessões.
use super::model::{MailError, MailboxAdmin, Participant, Permission, Result, address, db_error, invalid};
use crate::{
    fork::spaces::PERSONAL,
    store::{BotConfig, BotKind},
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

/// Bot vivo, agente e do Pessoal, lido pelo cadastro canônico do Hub: único participante
/// que pode ter concessão hoje. Cadastro que o Hub não consegue ler é negado.
fn live_bot(c: &Connection, bot: &str) -> Result<bool> {
    let config: Option<String> = c
        .query_row("SELECT config FROM bots WHERE id=?1 AND deleted=0", [bot], |r| r.get(0))
        .optional()
        .map_err(db_error)?;
    let Some(Ok(config)) = config.map(|raw| serde_json::from_str::<BotConfig>(&raw)) else { return Ok(false) };
    Ok(config.id == bot && config.space_id == PERSONAL && matches!(config.kind, BotKind::Agent))
}

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
    let label = label.trim();
    if label.is_empty() || label.chars().any(char::is_control) || addresses.is_empty() {
        return invalid("caixa sem nome ou sem endereço");
    }
    let addresses = addresses.iter().map(|a| address(a)).collect::<Result<Vec<_>>>()?;
    let id = uuid::Uuid::new_v4().to_string();
    let tx = Transaction::new_unchecked(c, TransactionBehavior::Immediate).map_err(db_error)?;
    if !enabled(&tx)? {
        return denied("plugin desligado");
    }
    tx.execute("INSERT INTO mail_manager_mailboxes(id,label) VALUES(?1,?2)", params![id, label]).map_err(db_error)?;
    for a in &addresses {
        let taken: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM mail_manager_addresses WHERE canonical=?1)",
                [a.to_lowercase()],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if taken {
            return invalid("endereço já pertence a outra caixa");
        }
        tx.execute(
            "INSERT INTO mail_manager_addresses(canonical,address,mailbox_id) VALUES(?1,?2,?3)",
            params![a.to_lowercase(), a, id],
        )
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
        let Participant::Bot(bot) = who else { return denied("concessão a pessoa ainda não existe") };
        let ok: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM mail_manager_plugin WHERE singleton=1 AND state='enabled')
                 AND EXISTS(SELECT 1 FROM mail_manager_mailboxes WHERE id=?1)",
                [mailbox],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if !ok || !live_bot(&tx, bot)? {
            return denied("plugin desligado, caixa inexistente ou bot inválido");
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
    let granted: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM mail_manager_plugin p
               JOIN mail_manager_mailboxes m ON m.id=?2
               JOIN mail_manager_grants g ON g.mailbox_id=m.id
             WHERE p.singleton=1 AND p.state='enabled' AND g.participant_kind='bot' AND g.participant_id=?1
               AND g.permission=?3 AND g.state='active')",
            params![bot, mailbox, permission.key()],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    Ok(granted && live_bot(c, bot)?)
}

pub(super) fn require(c: &Connection, mailbox: &str, who: &Participant, permissions: &[Permission]) -> Result<()> {
    for &permission in permissions {
        if !allowed(c, mailbox, who, permission)? {
            tracing::warn!(permission = permission.key(), kind = who.kind(), "mail_manager: operação negada");
            return Err(MailError::Denied);
        }
    }
    Ok(())
}

/// Negação com o motivo estático no log.
pub(super) fn denied<T>(reason: &'static str) -> Result<T> {
    tracing::warn!(reason, "mail_manager: operação negada");
    Err(MailError::Denied)
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

/// Emite (ou reaproveita) a referência opaca de uma mensagem (`attachment` vazio) ou de
/// um anexo dela. ID vazio do provedor é recusado.
pub(super) fn mint(c: &Connection, mailbox: &str, message: &str, attachment: Option<&str>) -> Result<String> {
    if message.is_empty() || attachment == Some("") {
        return Err(MailError::Invalid);
    }
    let kind = if attachment.is_some() { "attachment" } else { "message" };
    let attachment = attachment.unwrap_or_default();
    c.execute(
        "INSERT INTO mail_manager_refs(handle,mailbox_id,kind,message_id,attachment_id) VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(mailbox_id,kind,message_id,attachment_id) DO NOTHING",
        params![uuid::Uuid::new_v4().to_string(), mailbox, kind, message, attachment],
    )
    .map_err(db_error)?;
    c.query_row(
        "SELECT handle FROM mail_manager_refs WHERE mailbox_id=?1 AND kind=?2 AND message_id=?3 AND attachment_id=?4",
        params![mailbox, kind, message, attachment],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// O que uma referência aponta: caixa, mensagem e, se for de anexo, o anexo.
pub(super) struct Target {
    pub(super) mailbox: String,
    pub(super) message: String,
    pub(super) attachment: Option<String>,
}

pub(super) fn resolve(c: &Connection, handle: &str) -> Result<Option<Target>> {
    c.query_row(
        "SELECT r.mailbox_id,r.message_id,r.kind,r.attachment_id FROM mail_manager_refs r
         JOIN mail_manager_mailboxes m ON m.id=r.mailbox_id WHERE r.handle=?1",
        [handle],
        |r| {
            let kind: String = r.get(2)?;
            Ok(Target {
                mailbox: r.get(0)?,
                message: r.get(1)?,
                attachment: if kind == "attachment" { Some(r.get(3)?) } else { None },
            })
        },
    )
    .optional()
    .map_err(db_error)
}
