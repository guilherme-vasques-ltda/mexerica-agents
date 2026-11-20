//! Operações do Mail Manager. Cada uma passa pela permissão da caixa antes de tocar o
//! transporte; efeitos (enviar, responder, lixeira) conferem de novo na mesma transação que
//! reserva o recibo, então revogar entre abrir e agir nega o efeito.
use super::{
    model::{
        Attachment, AttachmentRef, BackendError, BackendSummary, MailBackend, MailError, Message, MessageRef, Notice,
        Outcome, Outgoing, Participant, Permission, Result, Summary, address, db_error, subject,
    },
    store::{audience, default_sender, mint, require, resolve, sender_belongs, viewable},
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::fmt::Write;

/// Mensagem nova: só exige Enviar e um remetente da própria caixa.
pub(crate) struct NewMail<'a> {
    /// Sem remetente, usa o primeiro endereço da caixa.
    pub(crate) from: Option<&'a str>,
    pub(crate) to: &'a [&'a str],
    pub(crate) subject: &'a str,
    pub(crate) body: &'a str,
}

fn backend(error: BackendError) -> MailError {
    match error {
        BackendError::Rejected => MailError::Rejected,
        BackendError::Unknown => MailError::Unconfirmed,
    }
}

fn summaries(c: &Connection, mailbox: &str, rows: Vec<BackendSummary>) -> Result<Vec<Summary>> {
    rows.into_iter()
        .map(|row| {
            Ok(Summary {
                mailbox: mailbox.to_owned(),
                message: MessageRef(mint(c, mailbox, &row.id, "")?),
                from: row.from,
                subject: row.subject,
                preview: row.preview,
                has_attachments: row.has_attachments,
            })
        })
        .collect()
}

pub(crate) fn list(c: &Connection, mail: &dyn MailBackend, who: &Participant, mailbox: &str) -> Result<Vec<Summary>> {
    require(c, mailbox, who, &[Permission::View])?;
    summaries(c, mailbox, mail.list(mailbox).map_err(backend)?)
}

/// Busca numa caixa (exige Ver nela) ou, sem caixa, só nas caixas que o participante vê.
pub(crate) fn search(
    c: &Connection,
    mail: &dyn MailBackend,
    who: &Participant,
    mailbox: Option<&str>,
    query: &str,
) -> Result<Vec<Summary>> {
    let boxes = match mailbox {
        Some(mailbox) => {
            require(c, mailbox, who, &[Permission::View])?;
            vec![mailbox.to_owned()]
        }
        None => viewable(c, who)?,
    };
    let mut out = Vec::new();
    for mailbox in boxes {
        out.extend(summaries(c, &mailbox, mail.search(&mailbox, query).map_err(backend)?)?);
    }
    Ok(out)
}

/// Caixa e ID do provedor de uma referência de mensagem, se o participante tem todas as
/// permissões pedidas nela. Referência desconhecida, de anexo ou de outra caixa: negado.
fn message_in(
    c: &Connection,
    who: &Participant,
    message: &MessageRef,
    need: &[Permission],
) -> Result<(String, String)> {
    let Some((mailbox, id, attachment)) = resolve(c, &message.0)? else { return Err(MailError::Denied) };
    if !attachment.is_empty() {
        return Err(MailError::Denied);
    }
    require(c, &mailbox, who, need)?;
    Ok((mailbox, id))
}

pub(crate) fn open(c: &Connection, mail: &dyn MailBackend, who: &Participant, message: &MessageRef) -> Result<Message> {
    let (mailbox, id) = message_in(c, who, message, &[Permission::View])?;
    let found = mail.open(&mailbox, &id).map_err(backend)?.ok_or(MailError::NotFound)?;
    let attachments = found
        .attachments
        .into_iter()
        .map(|a| {
            Ok(Attachment { reference: AttachmentRef(mint(c, &mailbox, &id, &a.id)?), name: a.name, size: a.size })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Message {
        mailbox,
        reference: message.clone(),
        from: found.from,
        subject: found.subject,
        body: found.body,
        attachments,
    })
}

pub(crate) fn attachment(
    c: &Connection,
    mail: &dyn MailBackend,
    who: &Participant,
    attachment: &AttachmentRef,
) -> Result<Vec<u8>> {
    let Some((mailbox, message, id)) = resolve(c, &attachment.0)? else { return Err(MailError::Denied) };
    if id.is_empty() {
        return Err(MailError::Denied);
    }
    require(c, &mailbox, who, &[Permission::View])?;
    mail.attachment(&mailbox, &message, &id).map_err(backend)?.ok_or(MailError::NotFound)
}

/// Aviso de mensagem nova: referência opaca para quem pode ver a caixa, sem conteúdo.
pub(crate) fn notice(c: &Connection, mailbox: &str, provider_message: &str) -> Result<Notice> {
    let to = audience(c, mailbox)?;
    if to.is_empty() {
        return Err(MailError::Denied);
    }
    Ok(Notice { mailbox: mailbox.to_owned(), message: MessageRef(mint(c, mailbox, provider_message, "")?), to })
}

fn sender(c: &Connection, mailbox: &str, from: Option<&str>) -> Result<String> {
    let Some(from) = from else { return default_sender(c, mailbox) };
    let from = address(from).map_err(|_| MailError::SenderNotAllowed)?;
    if sender_belongs(c, mailbox, &from)? { Ok(from) } else { Err(MailError::SenderNotAllowed) }
}

pub(crate) fn send_new(
    c: &Connection,
    mail: &mut dyn MailBackend,
    who: &Participant,
    mailbox: &str,
    new: &NewMail<'_>,
    key: &str,
) -> Result<Outcome> {
    let outgoing = Outgoing {
        from: sender(c, mailbox, new.from)?,
        to: recipients(new.to)?,
        subject: subject(new.subject)?,
        body: new.body.to_owned(),
        in_reply_to: None,
    };
    let effect = Effect { who, key, mailbox, need: &[Permission::Send], from: Some(&outgoing.from) };
    effect.run(c, &("send", mailbox, &outgoing), || mail.send(mailbox, &outgoing).map(Some))
}

/// Resposta no fio: exige Ver (para ter o contexto) e Enviar na caixa da mensagem. Vai
/// para o remetente original, a partir de um endereço da mesma caixa.
pub(crate) fn reply(
    c: &Connection,
    mail: &mut dyn MailBackend,
    who: &Participant,
    message: &MessageRef,
    from: Option<&str>,
    body: &str,
    key: &str,
) -> Result<Outcome> {
    let need = [Permission::View, Permission::Send];
    let (mailbox, id) = message_in(c, who, message, &need)?;
    let original = mail.open(&mailbox, &id).map_err(backend)?.ok_or(MailError::NotFound)?;
    let title = original.subject.trim();
    let outgoing = Outgoing {
        from: sender(c, &mailbox, from)?,
        to: vec![address(&original.from)?],
        subject: subject(&if title.to_lowercase().starts_with("re:") {
            title.to_owned()
        } else {
            format!("Re: {title}")
        })?,
        body: body.to_owned(),
        in_reply_to: Some(id),
    };
    let effect = Effect { who, key, mailbox: &mailbox, need: &need, from: Some(&outgoing.from) };
    effect.run(c, &("reply", &mailbox, &outgoing), || mail.send(&mailbox, &outgoing).map(Some))
}

/// Move para a lixeira. Exige só Excluir e uma referência daquela caixa obtida por via
/// autorizada; não lista, não abre e não apaga de vez.
pub(crate) fn trash(
    c: &Connection,
    mail: &mut dyn MailBackend,
    who: &Participant,
    message: &MessageRef,
    key: &str,
) -> Result<Outcome> {
    let (mailbox, id) = message_in(c, who, message, &[Permission::Delete])?;
    let effect = Effect { who, key, mailbox: &mailbox, need: &[Permission::Delete], from: None };
    effect.run(c, &("trash", &mailbox, &id), || mail.trash(&mailbox, &id).map(|()| None))
}

fn recipients(to: &[&str]) -> Result<Vec<String>> {
    if to.is_empty() || to.len() > 100 {
        return Err(MailError::Invalid);
    }
    to.iter().map(|a| address(a)).collect()
}

/// Um efeito sobre a caixa: quem age, a chave de operação, as permissões e o remetente.
struct Effect<'a> {
    who: &'a Participant,
    key: &'a str,
    mailbox: &'a str,
    need: &'a [Permission],
    from: Option<&'a str>,
}

impl Effect<'_> {
    /// Reserva o recibo e confere permissão e remetente na mesma transação; só depois chama
    /// o transporte (`Some(id)` = enviada, `None` = na lixeira). Repetir a chave com o mesmo
    /// pedido devolve o resultado guardado, sem novo efeito.
    fn run(
        &self,
        c: &Connection,
        request: &impl serde::Serialize,
        act: impl FnOnce() -> std::result::Result<Option<String>, BackendError>,
    ) -> Result<Outcome> {
        let (who, key) = (self.who, self.key);
        if key.trim().is_empty() || key.len() > 200 || key.chars().any(char::is_control) {
            return Err(MailError::Invalid);
        }
        let request = serde_json::to_vec(request).map_err(|_| MailError::Invalid)?;
        let digest = Sha256::digest(&request).iter().fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        });
        let tx = Transaction::new_unchecked(c, TransactionBehavior::Immediate).map_err(db_error)?;
        require(&tx, self.mailbox, who, self.need)?;
        if let Some(from) = self.from
            && !sender_belongs(&tx, self.mailbox, from)?
        {
            return Err(MailError::SenderNotAllowed);
        }
        let previous: Option<(String, String, Option<String>)> = tx
            .query_row(
                "SELECT request,state,outcome FROM mail_manager_receipts
                 WHERE participant_kind=?1 AND participant_id=?2 AND key=?3",
                params![who.kind(), who.id(), key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(db_error)?;
        match previous {
            Some((stored, ..)) if stored != digest => return Err(MailError::KeyReused),
            Some((_, state, Some(outcome))) if state == "done" => {
                return serde_json::from_str(&outcome).map_err(|_| MailError::Storage);
            }
            Some(_) => return Err(MailError::Unconfirmed),
            None => {}
        }
        tx.execute(
            "INSERT INTO mail_manager_receipts(participant_kind,participant_id,key,request,state)
             VALUES(?1,?2,?3,?4,'pending')",
            params![who.kind(), who.id(), key, digest],
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        let outcome = match act() {
            Ok(Some(id)) => Outcome::Sent { message: MessageRef(mint(c, self.mailbox, &id, "")?) },
            Ok(None) => Outcome::Trashed,
            Err(BackendError::Rejected) => {
                c.execute(
                    "DELETE FROM mail_manager_receipts
                     WHERE participant_kind=?1 AND participant_id=?2 AND key=?3 AND state='pending'",
                    params![who.kind(), who.id(), key],
                )
                .map_err(db_error)?;
                return Err(MailError::Rejected);
            }
            Err(BackendError::Unknown) => return Err(MailError::Unconfirmed),
        };
        c.execute(
            "UPDATE mail_manager_receipts SET state='done',outcome=?4
             WHERE participant_kind=?1 AND participant_id=?2 AND key=?3 AND state='pending'",
            params![who.kind(), who.id(), key, serde_json::to_string(&outcome).map_err(|_| MailError::Storage)?],
        )
        .map_err(db_error)?;
        Ok(outcome)
    }
}
