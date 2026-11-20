//! Operações do Mail Manager. Cada uma passa pela permissão da caixa antes de tocar o
//! transporte; efeitos (enviar, responder, lixeira) conferem de novo na mesma transação que
//! reserva o recibo, então revogar entre abrir e agir nega o efeito.
use super::{
    model::{
        Attachment, AttachmentRef, BackendError, BackendSummary, MailBackend, MailError, Message, MessageRef, Notice,
        Outcome, Outgoing, Participant, Permission, Result, SUBJECT_MAX, Summary, address, db_error, subject,
    },
    store::{Target, audience, default_sender, denied, mint, require, resolve, sender_belongs, viewable},
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

/// Emite as referências da lista numa só transação.
fn summaries(c: &Connection, mailbox: &str, rows: Vec<BackendSummary>) -> Result<Vec<Summary>> {
    let tx = c.unchecked_transaction().map_err(db_error)?;
    let out = rows
        .into_iter()
        .map(|row| {
            Ok(Summary {
                mailbox: mailbox.to_owned(),
                message: MessageRef(mint(&tx, mailbox, &row.id, None)?),
                from: row.from,
                subject: row.subject,
                preview: row.preview,
                has_attachments: row.has_attachments,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    tx.commit().map_err(db_error)?;
    Ok(out)
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
    let Some(Target { mailbox, message, attachment: None }) = resolve(c, &message.0)? else {
        return denied("referência inválida ou de anexo");
    };
    require(c, &mailbox, who, need)?;
    Ok((mailbox, message))
}

pub(crate) fn open(c: &Connection, mail: &dyn MailBackend, who: &Participant, message: &MessageRef) -> Result<Message> {
    let (mailbox, id) = message_in(c, who, message, &[Permission::View])?;
    let found = mail.open(&mailbox, &id).map_err(backend)?.ok_or(MailError::NotFound)?;
    let attachments = found
        .attachments
        .into_iter()
        .map(|a| {
            Ok(Attachment {
                reference: AttachmentRef(mint(c, &mailbox, &id, Some(&a.id))?),
                name: a.name,
                size: a.size,
            })
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
    let Some(Target { mailbox, message, attachment: Some(id) }) = resolve(c, &attachment.0)? else {
        return denied("referência inválida ou de mensagem");
    };
    require(c, &mailbox, who, &[Permission::View])?;
    mail.attachment(&mailbox, &message, &id).map_err(backend)?.ok_or(MailError::NotFound)
}

/// Aviso de mensagem nova: referência opaca para quem pode ver a caixa, sem conteúdo.
pub(crate) fn notice(c: &Connection, mailbox: &str, provider_message: &str) -> Result<Notice> {
    let to = audience(c, mailbox)?;
    if to.is_empty() {
        return denied("ninguém pode ver a caixa");
    }
    Ok(Notice { mailbox: mailbox.to_owned(), message: MessageRef(mint(c, mailbox, provider_message, None)?), to })
}

fn sender(c: &Connection, mailbox: &str, from: Option<&str>) -> Result<String> {
    let Some(from) = from else { return default_sender(c, mailbox) };
    let from = address(from).map_err(|_| MailError::SenderNotAllowed)?;
    if sender_belongs(c, mailbox, &from)? {
        return Ok(from);
    }
    tracing::warn!("mail_manager: remetente não pertence à caixa");
    Err(MailError::SenderNotAllowed)
}

pub(crate) fn send_new(
    c: &Connection,
    mail: &mut dyn MailBackend,
    who: &Participant,
    mailbox: &str,
    new: &NewMail<'_>,
    key: &str,
) -> Result<Outcome> {
    let need = [Permission::Send];
    require(c, mailbox, who, &need)?;
    let outgoing = Outgoing {
        from: sender(c, mailbox, new.from)?,
        to: recipients(new.to)?,
        subject: subject(new.subject)?,
        body: new.body.to_owned(),
        in_reply_to: None,
    };
    let effect = Effect { who, key, mailbox, need: &need, from: Some(&outgoing.from) };
    effect.run(c, &("send", mailbox, &outgoing), || mail.send(mailbox, &outgoing).map(Some).map_err(backend))
}

/// Resposta no fio: exige Ver (para ter o contexto) e Enviar na caixa da mensagem. Vai
/// para o remetente original, a partir de um endereço da mesma caixa. O recibo é da
/// intenção (mensagem, remetente e texto): repetir não relê o original no provedor.
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
    let from = sender(c, &mailbox, from)?;
    let effect = Effect { who, key, mailbox: &mailbox, need: &need, from: Some(&from) };
    effect.run(c, &("reply", &mailbox, &id, &from, body), || {
        // Ler o original não tem efeito: qualquer falha aqui é certa e libera a chave.
        let original = mail.open(&mailbox, &id).map_err(|_| MailError::Rejected)?.ok_or(MailError::NotFound)?;
        let outgoing = Outgoing {
            from: from.clone(),
            to: vec![address(&original.from)?],
            subject: subject(&reply_subject(&original.subject))?,
            body: body.to_owned(),
            in_reply_to: Some(id.clone()),
        };
        mail.send(&mailbox, &outgoing).map(Some).map_err(backend)
    })
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
    let need = [Permission::Delete];
    let (mailbox, id) = message_in(c, who, message, &need)?;
    let effect = Effect { who, key, mailbox: &mailbox, need: &need, from: None };
    effect.run(c, &("trash", &mailbox, &id), || mail.trash(&mailbox, &id).map(|()| None).map_err(backend))
}

/// "Re: " antes do assunto original, cortado numa fronteira de caractere para caber no
/// limite. Assunto que já começa com "Re:" fica como está.
fn reply_subject(original: &str) -> String {
    let title = original.trim();
    if title.to_lowercase().starts_with("re:") {
        return title.to_owned();
    }
    let mut end = title.len().min(SUBJECT_MAX - "Re: ".len());
    while !title.is_char_boundary(end) {
        end -= 1;
    }
    format!("Re: {}", &title[..end])
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
    /// o transporte (`Some(id)` = enviada, `None` = na lixeira). A reserva é o ponto de
    /// autorização: revogar antes dela nega; revogar depois não cancela o efeito já iniciado,
    /// e vale para a operação seguinte. Repetir a chave com o mesmo pedido devolve o
    /// resultado guardado, sem novo efeito. Falha certa sem efeito libera a chave; qualquer
    /// incerteza (inclusive não conseguir gravar o resultado) a deixa não confirmada.
    fn run(
        &self,
        c: &Connection,
        request: &impl serde::Serialize,
        act: impl FnOnce() -> Result<Option<String>>,
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
        match act() {
            Ok(id) => self.settle(c, id).map_err(|_| MailError::Unconfirmed),
            Err(MailError::Unconfirmed) => Err(MailError::Unconfirmed),
            Err(error) => {
                let released = c.execute(
                    "DELETE FROM mail_manager_receipts
                     WHERE participant_kind=?1 AND participant_id=?2 AND key=?3 AND state='pending'",
                    params![who.kind(), who.id(), key],
                );
                if released.is_ok() { Err(error) } else { Err(MailError::Unconfirmed) }
            }
        }
    }

    fn settle(&self, c: &Connection, id: Option<String>) -> Result<Outcome> {
        let outcome = match id {
            Some(id) => Outcome::Sent { message: MessageRef(mint(c, self.mailbox, &id, None)?) },
            None => Outcome::Trashed,
        };
        let stored = serde_json::to_string(&outcome).map_err(|_| MailError::Storage)?;
        c.execute(
            "UPDATE mail_manager_receipts SET state='done',outcome=?4
             WHERE participant_kind=?1 AND participant_id=?2 AND key=?3 AND state='pending'",
            params![self.who.kind(), self.who.id(), self.key, stored],
        )
        .map_err(db_error)?;
        Ok(outcome)
    }
}
