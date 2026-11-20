#![allow(clippy::unwrap_used)]
//! Duas caixas fictícias, bots fictícios e um transporte em memória: nenhuma conta,
//! provedor ou rede real.
use super::{
    model::{
        AttachmentRef, BackendAttachment, BackendError, BackendMessage, BackendSummary, MailBackend, MailError,
        MessageRef, Outcome, Outgoing, Participant, Permission,
    },
    ops::{NewMail, attachment, list, notice, open, reply, search, send_new, trash},
    store::{self, connect_mailbox, disconnect_mailbox, mailboxes, set_enabled, set_grant},
};
use crate::{LockExt, store::Store};
use rusqlite::Connection;
use serde_json::json;
use std::{cell::Cell, collections::HashMap};

const SUPORTE: &str = "suporte@loja.example.invalid";
const ATENDIMENTO: &str = "atendimento@loja.example.invalid";
const OPERACOES: &str = "operacoes@loja.example.invalid";
const CLIENTE: &str = "cliente@cliente.example.invalid";
/// Marcadores de conteúdo de cada caixa: não podem aparecer onde não há permissão.
const SEGREDO_A: &str = "SEGREDO-CAIXA-A";
const SEGREDO_B: &str = "SEGREDO-CAIXA-B";

struct FakeMessage {
    id: String,
    from: String,
    subject: String,
    body: String,
    attachments: Vec<(String, String, Vec<u8>)>,
}

/// Transporte em memória. Conta leituras para provar que negar acontece antes de tocá-lo.
#[derive(Default)]
struct Fake {
    boxes: HashMap<String, Vec<FakeMessage>>,
    sent: Vec<(String, Outgoing)>,
    trashed: Vec<(String, String)>,
    fail: Option<BackendError>,
    reads: Cell<usize>,
}

impl Fake {
    fn find(&self, mailbox: &str, id: &str) -> Option<&FakeMessage> {
        self.boxes.get(mailbox)?.iter().find(|m| m.id == id)
    }

    fn rows(&self, mailbox: &str, keep: impl Fn(&FakeMessage) -> bool) -> Vec<BackendSummary> {
        self.reads.set(self.reads.get() + 1);
        let trashed = |m: &FakeMessage| self.trashed.iter().any(|(b, id)| b == mailbox && *id == m.id);
        self.boxes
            .get(mailbox)
            .map(|all| {
                all.iter()
                    .filter(|m| !trashed(m) && keep(m))
                    .map(|m| BackendSummary {
                        id: m.id.clone(),
                        from: m.from.clone(),
                        subject: m.subject.clone(),
                        preview: m.body.chars().take(40).collect(),
                        has_attachments: !m.attachments.is_empty(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn effects(&self) -> usize {
        self.sent.len() + self.trashed.len()
    }
}

impl MailBackend for Fake {
    fn list(&self, mailbox: &str) -> Result<Vec<BackendSummary>, BackendError> {
        Ok(self.rows(mailbox, |_| true))
    }

    fn search(&self, mailbox: &str, query: &str) -> Result<Vec<BackendSummary>, BackendError> {
        Ok(self.rows(mailbox, |m| m.subject.contains(query) || m.body.contains(query)))
    }

    fn open(&self, mailbox: &str, message: &str) -> Result<Option<BackendMessage>, BackendError> {
        self.reads.set(self.reads.get() + 1);
        Ok(self.find(mailbox, message).map(|m| BackendMessage {
            from: m.from.clone(),
            subject: m.subject.clone(),
            body: m.body.clone(),
            attachments: m
                .attachments
                .iter()
                .map(|(id, name, bytes)| BackendAttachment {
                    id: id.clone(),
                    name: name.clone(),
                    size: bytes.len() as u64,
                })
                .collect(),
        }))
    }

    fn attachment(&self, mailbox: &str, message: &str, attachment: &str) -> Result<Option<Vec<u8>>, BackendError> {
        self.reads.set(self.reads.get() + 1);
        Ok(self
            .find(mailbox, message)
            .and_then(|m| m.attachments.iter().find(|(id, ..)| id == attachment))
            .map(|(.., bytes)| bytes.clone()))
    }

    fn send(&mut self, mailbox: &str, mail: &Outgoing) -> Result<String, BackendError> {
        if let Some(error) = self.fail.take() {
            if error == BackendError::Unknown {
                self.sent.push((mailbox.to_owned(), mail.clone()));
            }
            return Err(error);
        }
        self.sent.push((mailbox.to_owned(), mail.clone()));
        Ok(format!("enviada-{}", self.sent.len()))
    }

    fn trash(&mut self, mailbox: &str, message: &str) -> Result<(), BackendError> {
        if let Some(error) = self.fail.take() {
            return Err(error);
        }
        self.trashed.push((mailbox.to_owned(), message.to_owned()));
        Ok(())
    }
}

fn bot(n: u32) -> Participant {
    Participant::Bot(format!("00000000-0000-4000-8000-{n:012x}"))
}

fn id(who: &Participant) -> &str {
    let Participant::Bot(id) = who else { unreachable!() };
    id
}

struct Fx {
    store: Store,
    a: String,
    b: String,
    mail: Fake,
}

/// Hub novo com o plugin ligado, as caixas A (Suporte) e B (Operações), uma mensagem com
/// anexo em cada uma e os bots 1 a 9 no Pessoal. Ninguém tem permissão alguma.
fn fixture() -> Fx {
    let root = std::env::temp_dir().join(format!("mail-manager-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store = Store::open(&root.join("hub.db")).unwrap();
    for n in 1..=9 {
        let who = bot(n);
        let cfg = serde_json::from_value(json!({"id": id(&who), "name": format!("Bot {n}")})).unwrap();
        store.save_bot(&cfg).unwrap();
    }
    let (a, b) = {
        let c = store.db.locked();
        set_enabled(&c, true).unwrap();
        (
            connect_mailbox(&c, "Suporte", &[SUPORTE, ATENDIMENTO]).unwrap(),
            connect_mailbox(&c, "Operações", &[OPERACOES]).unwrap(),
        )
    };
    let mut mail = Fake::default();
    for (mailbox, secret) in [(&a, SEGREDO_A), (&b, SEGREDO_B)] {
        mail.boxes.insert(
            mailbox.clone(),
            vec![FakeMessage {
                id: format!("msg-{secret}"),
                from: CLIENTE.into(),
                subject: format!("Chamado {secret}"),
                body: format!("Corpo com {secret} e mais texto"),
                attachments: vec![(
                    format!("anexo-{secret}"),
                    format!("contrato-{secret}.pdf"),
                    secret.as_bytes().to_vec(),
                )],
            }],
        );
    }
    Fx { store, a, b, mail }
}

fn grant(c: &Connection, mailbox: &str, who: &Participant, permissions: &[Permission]) {
    for &p in permissions {
        set_grant(c, mailbox, who, p, true).unwrap();
    }
}

/// Referência da mensagem da caixa, obtida pela via autorizada: o aviso de mensagem nova
/// para um bot que só vê (bot 9), como faria um encaminhamento a quem pode agir.
fn message_ref(c: &Connection, mailbox: &str, secret: &str) -> MessageRef {
    let viewer = bot(9);
    let had = store::allowed(c, mailbox, &viewer, Permission::View).unwrap();
    set_grant(c, mailbox, &viewer, Permission::View, true).unwrap();
    let r = notice(c, mailbox, &format!("msg-{secret}")).unwrap().message;
    if !had {
        set_grant(c, mailbox, &viewer, Permission::View, false).unwrap();
    }
    r
}

fn new_mail<'a>(from: Option<&'a str>, subject: &'a str) -> NewMail<'a> {
    NewMail { from, to: &[CLIENTE], subject, body: "Olá" }
}

#[test]
fn matriz_duas_caixas_permissoes_mistas() {
    use Permission::{Delete, Send, View};
    // (Ver, Enviar, Excluir) na caixa A; a caixa B nunca é concedida a quem está na matriz.
    let cases: [&[Permission]; 8] =
        [&[], &[View], &[Send], &[Delete], &[View, Send], &[View, Delete], &[Send, Delete], &[View, Send, Delete]];
    for grants in cases {
        let mut fx = fixture();
        let c = fx.store.db.locked();
        let who = bot(1);
        grant(&c, &fx.a, &who, grants);
        let has = |p| grants.contains(&p);
        let a_ref = message_ref(&c, &fx.a, SEGREDO_A);
        let b_ref = message_ref(&c, &fx.b, SEGREDO_B);
        let label = format!("{grants:?}");

        assert_eq!(list(&c, &fx.mail, &who, &fx.a).is_ok(), has(View), "listar {label}");
        assert_eq!(search(&c, &fx.mail, &who, Some(&fx.a), "Chamado").is_ok(), has(View), "buscar {label}");
        let opened = open(&c, &fx.mail, &who, &a_ref);
        assert_eq!(opened.is_ok(), has(View), "abrir {label}");
        if let Ok(message) = opened {
            let file = &message.attachments[0].reference;
            assert_eq!(attachment(&c, &fx.mail, &who, file).unwrap(), SEGREDO_A.as_bytes());
        }
        let before = fx.mail.effects();
        let sent = send_new(&c, &mut fx.mail, &who, &fx.a, &new_mail(None, "Novo"), "k-novo");
        assert_eq!(sent.is_ok(), has(Send), "escrever novo {label}");
        let replied = reply(&c, &mut fx.mail, &who, &a_ref, None, "Resposta", "k-resposta");
        assert_eq!(replied.is_ok(), has(View) && has(Send), "responder {label}");
        let trashed = trash(&c, &mut fx.mail, &who, &a_ref, "k-lixeira");
        assert_eq!(trashed.is_ok(), has(Delete), "lixeira {label}");
        let expected = usize::from(has(Send)) + usize::from(has(View) && has(Send)) + usize::from(has(Delete));
        assert_eq!(fx.mail.effects() - before, expected, "efeitos {label}");
        for denied in [sent, replied, trashed].into_iter().filter_map(Result::err) {
            assert_eq!(denied, MailError::Denied, "{label}");
        }

        // Nada na caixa B, com qualquer combinação na A.
        let reads = fx.mail.reads.get();
        assert_eq!(list(&c, &fx.mail, &who, &fx.b), Err(MailError::Denied));
        assert_eq!(search(&c, &fx.mail, &who, Some(&fx.b), "Chamado"), Err(MailError::Denied));
        assert_eq!(open(&c, &fx.mail, &who, &b_ref), Err(MailError::Denied));
        assert_eq!(fx.mail.reads.get(), reads, "negar antes de tocar o transporte");
        let effects = fx.mail.effects();
        assert_eq!(send_new(&c, &mut fx.mail, &who, &fx.b, &new_mail(None, "x"), "k-b1"), Err(MailError::Denied));
        assert_eq!(reply(&c, &mut fx.mail, &who, &b_ref, None, "x", "k-b2"), Err(MailError::Denied));
        assert_eq!(trash(&c, &mut fx.mail, &who, &b_ref, "k-b3"), Err(MailError::Denied));
        assert_eq!(fx.mail.effects(), effects);
    }
}

#[test]
fn enviar_sem_ler_escreve_novo_mas_nao_ve_nem_responde() {
    let mut fx = fixture();
    let c = fx.store.db.locked();
    let x = bot(1);
    grant(&c, &fx.a, &x, &[Permission::Send]);
    let outcome = send_new(&c, &mut fx.mail, &x, &fx.a, &new_mail(Some(ATENDIMENTO), "Aviso"), "k1").unwrap();
    assert!(matches!(outcome, Outcome::Sent { .. }));
    assert_eq!(fx.mail.sent[0].1.from, ATENDIMENTO);
    assert_eq!(fx.mail.sent[0].1.in_reply_to, None);
    let a_ref = message_ref(&c, &fx.a, SEGREDO_A);
    assert_eq!(list(&c, &fx.mail, &x, &fx.a), Err(MailError::Denied));
    assert_eq!(open(&c, &fx.mail, &x, &a_ref), Err(MailError::Denied));
    // A referência da própria mensagem enviada também não abre sem Ver.
    let Outcome::Sent { message } = outcome else { unreachable!() };
    assert_eq!(open(&c, &fx.mail, &x, &message), Err(MailError::Denied));
    assert_eq!(reply(&c, &mut fx.mail, &x, &a_ref, None, "oi", "k2"), Err(MailError::Denied));
    assert_eq!(fx.mail.sent.len(), 1);
}

#[test]
fn ver_sem_enviar_le_tudo_e_nao_tem_efeito() {
    let mut fx = fixture();
    let c = fx.store.db.locked();
    let y = bot(2);
    grant(&c, &fx.a, &y, &[Permission::View]);
    let rows = list(&c, &fx.mail, &y, &fx.a).unwrap();
    assert!(rows[0].subject.contains(SEGREDO_A) && rows[0].has_attachments);
    let message = open(&c, &fx.mail, &y, &rows[0].message).unwrap();
    assert!(message.body.contains(SEGREDO_A));
    assert_eq!(send_new(&c, &mut fx.mail, &y, &fx.a, &new_mail(None, "x"), "k1"), Err(MailError::Denied));
    assert_eq!(reply(&c, &mut fx.mail, &y, &rows[0].message, None, "x", "k2"), Err(MailError::Denied));
    assert_eq!(trash(&c, &mut fx.mail, &y, &rows[0].message, "k3"), Err(MailError::Denied));
    assert_eq!(fx.mail.effects(), 0);
}

#[test]
fn excluir_sem_listar_so_move_para_a_lixeira_com_referencia_da_caixa() {
    let mut fx = fixture();
    let c = fx.store.db.locked();
    let d = bot(3);
    grant(&c, &fx.a, &d, &[Permission::Delete]);
    let reads = fx.mail.reads.get();
    assert_eq!(list(&c, &fx.mail, &d, &fx.a), Err(MailError::Denied));
    assert_eq!(search(&c, &fx.mail, &d, None, "Chamado").unwrap(), vec![]);
    let a_ref = message_ref(&c, &fx.a, SEGREDO_A);
    assert_eq!(open(&c, &fx.mail, &d, &a_ref), Err(MailError::Denied));
    assert_eq!(fx.mail.reads.get(), reads, "excluir não lê nada");

    // Referência de outra caixa, inventada ou de anexo: mesma recusa, nada revelado.
    let b_ref = message_ref(&c, &fx.b, SEGREDO_B);
    assert_eq!(trash(&c, &mut fx.mail, &d, &b_ref, "k-b"), Err(MailError::Denied));
    let guessed = MessageRef(uuid::Uuid::new_v4().to_string());
    assert_eq!(trash(&c, &mut fx.mail, &d, &guessed, "k-x"), Err(MailError::Denied));
    let viewer = bot(4);
    grant(&c, &fx.a, &viewer, &[Permission::View]);
    let file = open(&c, &fx.mail, &viewer, &a_ref).unwrap().attachments.remove(0).reference;
    assert_eq!(trash(&c, &mut fx.mail, &d, &MessageRef(file.0), "k-anexo"), Err(MailError::Denied));
    assert_eq!(fx.mail.trashed, vec![]);

    assert_eq!(trash(&c, &mut fx.mail, &d, &a_ref, "k-a").unwrap(), Outcome::Trashed);
    assert_eq!(fx.mail.trashed, vec![(fx.a.clone(), format!("msg-{SEGREDO_A}"))]);
}

#[test]
fn revogar_entre_abrir_e_agir_nega_a_operacao_seguinte() {
    let mut fx = fixture();
    let c = fx.store.db.locked();
    let z = bot(5);
    grant(&c, &fx.a, &z, &[Permission::View, Permission::Send, Permission::Delete]);
    let rows = list(&c, &fx.mail, &z, &fx.a).unwrap();
    let message = open(&c, &fx.mail, &z, &rows[0].message).unwrap();

    set_grant(&c, &fx.a, &z, Permission::Send, false).unwrap();
    assert_eq!(reply(&c, &mut fx.mail, &z, &message.reference, None, "ok", "k1"), Err(MailError::Denied));
    assert_eq!(send_new(&c, &mut fx.mail, &z, &fx.a, &new_mail(None, "x"), "k2"), Err(MailError::Denied));
    set_grant(&c, &fx.a, &z, Permission::Delete, false).unwrap();
    assert_eq!(trash(&c, &mut fx.mail, &z, &message.reference, "k3"), Err(MailError::Denied));
    set_grant(&c, &fx.a, &z, Permission::View, false).unwrap();
    assert_eq!(open(&c, &fx.mail, &z, &message.reference), Err(MailError::Denied));
    assert_eq!(attachment(&c, &fx.mail, &z, &message.attachments[0].reference), Err(MailError::Denied));
    assert_eq!(fx.mail.effects(), 0);

    // Concessão revogada não volta; conceder de novo cria outra, e só ela vale.
    let revived = c.execute("UPDATE mail_manager_grants SET state='active' WHERE state='revoked'", []);
    assert!(revived.is_err());
    set_grant(&c, &fx.a, &z, Permission::View, true).unwrap();
    assert!(open(&c, &fx.mail, &z, &message.reference).is_ok());
    assert_eq!(reply(&c, &mut fx.mail, &z, &message.reference, None, "ok", "k1"), Err(MailError::Denied));
}

#[test]
fn remetente_tem_que_ser_da_caixa_autorizada() {
    let mut fx = fixture();
    let c = fx.store.db.locked();
    let x = bot(1);
    grant(&c, &fx.a, &x, &[Permission::View, Permission::Send]);
    grant(&c, &fx.b, &x, &[Permission::View]);
    for from in [OPERACOES, "ceo@loja.example.invalid", "Suporte <suporte@loja.example.invalid>", "x"] {
        let got = send_new(&c, &mut fx.mail, &x, &fx.a, &new_mail(Some(from), "x"), &format!("k-{from}"));
        assert_eq!(got, Err(MailError::SenderNotAllowed), "{from}");
    }
    // Caixa B só tem Ver: o remetente certo não supre a falta de Enviar.
    let got = send_new(&c, &mut fx.mail, &x, &fx.b, &new_mail(Some(OPERACOES), "x"), "k-b");
    assert_eq!(got, Err(MailError::Denied));
    let a_ref = message_ref(&c, &fx.a, SEGREDO_A);
    assert_eq!(reply(&c, &mut fx.mail, &x, &a_ref, Some(OPERACOES), "x", "k-r"), Err(MailError::SenderNotAllowed));
    // Cabeçalho injetado no destinatário ou no assunto é recusado.
    let to = ["cliente@cliente.example.invalid\r\nBcc: outro@cliente.example.invalid"];
    let bad = NewMail { from: None, to: &to, subject: "x", body: "x" };
    assert_eq!(send_new(&c, &mut fx.mail, &x, &fx.a, &bad, "k-to"), Err(MailError::Invalid));
    let bad = NewMail { from: None, to: &[CLIENTE], subject: "x\nBcc: outro@cliente.example.invalid", body: "x" };
    assert_eq!(send_new(&c, &mut fx.mail, &x, &fx.a, &bad, "k-subject"), Err(MailError::Invalid));
    assert_eq!(fx.mail.sent.len(), 0);

    reply(&c, &mut fx.mail, &x, &a_ref, Some(" Atendimento@Loja.Example.Invalid "), "Resolvido", "k-ok").unwrap();
    let (mailbox, sent) = &fx.mail.sent[0];
    assert_eq!(
        (mailbox, sent.from.as_str(), sent.to.as_slice()),
        (&fx.a, ATENDIMENTO, [CLIENTE.to_owned()].as_slice())
    );
    assert_eq!(sent.subject, format!("Re: Chamado {SEGREDO_A}"));
    assert_eq!(sent.in_reply_to.as_deref(), Some(format!("msg-{SEGREDO_A}").as_str()));
}

#[test]
fn busca_previa_anexo_e_aviso_nao_vazam_para_quem_nao_ve() {
    let fx = fixture();
    let c = fx.store.db.locked();
    let (so_a, nada, envia_b) = (bot(1), bot(2), bot(3));
    grant(&c, &fx.a, &so_a, &[Permission::View]);
    grant(&c, &fx.b, &envia_b, &[Permission::Send, Permission::Delete]);

    // Busca sem caixa só percorre o que pode ver; a prévia é só da caixa A.
    let found = search(&c, &fx.mail, &so_a, None, "Chamado").unwrap();
    assert_eq!(found.len(), 1);
    assert!(found[0].mailbox == fx.a && found[0].preview.contains(SEGREDO_A));
    assert!(!format!("{found:?}").contains(SEGREDO_B));
    let reads = fx.mail.reads.get();
    assert_eq!(search(&c, &fx.mail, &nada, None, SEGREDO_B).unwrap(), vec![]);
    assert_eq!(search(&c, &fx.mail, &envia_b, None, SEGREDO_B).unwrap(), vec![]);
    assert_eq!(fx.mail.reads.get(), reads);

    // Anexo da caixa B por referência: negado para quem só vê a A.
    let watcher = bot(4);
    grant(&c, &fx.b, &watcher, &[Permission::View]);
    let b_ref = notice(&c, &fx.b, &format!("msg-{SEGREDO_B}")).unwrap().message;
    let file = open(&c, &fx.mail, &watcher, &b_ref).unwrap().attachments.remove(0).reference;
    assert_eq!(attachment(&c, &fx.mail, &so_a, &file), Err(MailError::Denied));
    assert_eq!(attachment(&c, &fx.mail, &envia_b, &file), Err(MailError::Denied));
    // Referência de mensagem não serve como anexo, e vice-versa.
    assert_eq!(attachment(&c, &fx.mail, &watcher, &AttachmentRef(b_ref.0.clone())), Err(MailError::Denied));
    assert_eq!(open(&c, &fx.mail, &watcher, &MessageRef(file.0.clone())), Err(MailError::Denied));

    // Aviso de mensagem nova: só quem vê, e sem assunto, remetente ou corpo.
    let n = notice(&c, &fx.b, &format!("msg-{SEGREDO_B}")).unwrap();
    assert_eq!(n.to, vec![watcher.clone()]);
    let shown = format!("{n:?}");
    assert!(!shown.contains(SEGREDO_B) && !shown.contains(CLIENTE) && !shown.contains("msg-"));
    set_grant(&c, &fx.b, &watcher, Permission::View, false).unwrap();
    assert_eq!(notice(&c, &fx.b, &format!("msg-{SEGREDO_B}")), Err(MailError::Denied));
}

#[test]
fn repetir_a_operacao_nao_duplica_o_efeito() {
    let mut fx = fixture();
    let c = fx.store.db.locked();
    let x = bot(1);
    grant(&c, &fx.a, &x, &[Permission::View, Permission::Send, Permission::Delete]);
    let first = send_new(&c, &mut fx.mail, &x, &fx.a, &new_mail(None, "Uma vez"), "k1").unwrap();
    let again = send_new(&c, &mut fx.mail, &x, &fx.a, &new_mail(None, "Uma vez"), "k1").unwrap();
    assert_eq!(first, again);
    assert_eq!(fx.mail.sent.len(), 1);
    assert_eq!(send_new(&c, &mut fx.mail, &x, &fx.a, &new_mail(None, "Outra"), "k1"), Err(MailError::KeyReused));
    // A mesma chave de outro bot é outra operação.
    let y = bot(2);
    grant(&c, &fx.a, &y, &[Permission::Send]);
    send_new(&c, &mut fx.mail, &y, &fx.a, &new_mail(None, "Uma vez"), "k1").unwrap();
    assert_eq!(fx.mail.sent.len(), 2);

    let a_ref = message_ref(&c, &fx.a, SEGREDO_A);
    assert_eq!(trash(&c, &mut fx.mail, &x, &a_ref, "t1").unwrap(), Outcome::Trashed);
    assert_eq!(trash(&c, &mut fx.mail, &x, &a_ref, "t1").unwrap(), Outcome::Trashed);
    assert_eq!(fx.mail.trashed.len(), 1);

    // Resultado incerto: fica não confirmado e não reenvia às cegas.
    fx.mail.fail = Some(BackendError::Unknown);
    assert_eq!(send_new(&c, &mut fx.mail, &x, &fx.a, &new_mail(None, "Incerta"), "k2"), Err(MailError::Unconfirmed));
    assert_eq!(send_new(&c, &mut fx.mail, &x, &fx.a, &new_mail(None, "Incerta"), "k2"), Err(MailError::Unconfirmed));
    assert_eq!(fx.mail.sent.len(), 3);
    // Recusa sem efeito: a mesma chave pode tentar de novo, e o efeito acontece uma vez.
    fx.mail.fail = Some(BackendError::Rejected);
    assert_eq!(send_new(&c, &mut fx.mail, &x, &fx.a, &new_mail(None, "Recusada"), "k3"), Err(MailError::Rejected));
    assert_eq!(fx.mail.sent.len(), 3);
    send_new(&c, &mut fx.mail, &x, &fx.a, &new_mail(None, "Recusada"), "k3").unwrap();
    send_new(&c, &mut fx.mail, &x, &fx.a, &new_mail(None, "Recusada"), "k3").unwrap();
    assert_eq!(fx.mail.sent.len(), 4);

    // Repetir depois de revogado também é negado, sem efeito novo.
    set_grant(&c, &fx.a, &x, Permission::Send, false).unwrap();
    assert_eq!(send_new(&c, &mut fx.mail, &x, &fx.a, &new_mail(None, "Uma vez"), "k1"), Err(MailError::Denied));
    assert_eq!(fx.mail.sent.len(), 4);
}

#[test]
fn administrar_nao_da_leitura_e_pessoa_nao_recebe_concessao() {
    let mut fx = fixture();
    let c = fx.store.db.locked();
    let x = bot(1);
    grant(&c, &fx.a, &x, &[Permission::View]);
    list(&c, &fx.mail, &x, &fx.a).unwrap();
    // A visão do administrador traz estado e concessões, nunca dado de mensagem.
    let admin = mailboxes(&c).unwrap();
    assert_eq!(admin.len(), 2);
    let suporte = admin.iter().find(|m| m.id == fx.a).unwrap();
    assert_eq!(suporte.addresses, vec![ATENDIMENTO.to_owned(), SUPORTE.to_owned()]);
    assert_eq!(suporte.grants, vec![(x.clone(), Permission::View)]);
    let shown = format!("{admin:?}");
    assert!(!shown.contains(SEGREDO_A) && !shown.contains(SEGREDO_B) && !shown.contains(CLIENTE));

    // Pessoa: concessão recusada e toda operação negada.
    let person = Participant::Person("dono-local".into());
    for p in [Permission::View, Permission::Send, Permission::Delete] {
        assert_eq!(set_grant(&c, &fx.a, &person, p, true), Err(MailError::Denied));
    }
    assert_eq!(list(&c, &fx.mail, &person, &fx.a), Err(MailError::Denied));
    assert_eq!(send_new(&c, &mut fx.mail, &person, &fx.a, &new_mail(None, "x"), "k"), Err(MailError::Denied));
    assert_eq!(mailboxes(&c).unwrap().iter().map(|m| m.grants.len()).sum::<usize>(), 1);
}

#[test]
fn bot_apagado_fora_do_pessoal_ou_plugin_desligado_nao_age() {
    let mut fx = fixture();
    let c = fx.store.db.locked();
    // Bot de outro Space e canal (grupo) gravados direto: nenhum dos dois recebe concessão.
    let (other_space, group) = (bot(20), bot(21));
    for (who, extra) in
        [(&other_space, json!({"spaceId": "00000000-0000-4000-8000-0000000000aa"})), (&group, json!({"kind": "group"}))]
    {
        let mut config = json!({"id": id(who), "name": "Fora", "spaceId": crate::fork::spaces::PERSONAL});
        config.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        c.execute("INSERT INTO bots(id,rev,deleted,config) VALUES(?1,1,0,?2)", [id(who), &config.to_string()]).unwrap();
        assert_eq!(set_grant(&c, &fx.a, who, Permission::View, true), Err(MailError::Denied));
    }
    assert_eq!(set_grant(&c, &fx.a, &bot(99), Permission::View, true), Err(MailError::Denied), "bot inexistente");
    assert_eq!(set_grant(&c, "caixa-inexistente", &bot(1), Permission::View, true), Err(MailError::Denied));

    let (x, y) = (bot(1), bot(2));
    grant(&c, &fx.a, &x, &[Permission::View]);
    grant(&c, &fx.a, &y, &[Permission::View, Permission::Send]);
    c.execute("UPDATE bots SET deleted=1 WHERE id=?1", [id(&x)]).unwrap();
    assert_eq!(list(&c, &fx.mail, &x, &fx.a), Err(MailError::Denied), "apagado nega mesmo antes do gancho");
    super::on_bot_deleted(&c, id(&x)).unwrap();
    assert!(mailboxes(&c).unwrap().iter().all(|m| m.grants.iter().all(|(who, _)| *who != x)));

    // Desligar o plugin nega tudo e revoga; religar não devolve as concessões.
    set_enabled(&c, false).unwrap();
    assert_eq!(list(&c, &fx.mail, &y, &fx.a), Err(MailError::Denied));
    assert_eq!(send_new(&c, &mut fx.mail, &y, &fx.a, &new_mail(None, "x"), "k"), Err(MailError::Denied));
    assert_eq!(connect_mailbox(&c, "Nova", &["nova@loja.example.invalid"]), Err(MailError::Denied));
    set_enabled(&c, true).unwrap();
    assert_eq!(list(&c, &fx.mail, &y, &fx.a), Err(MailError::Denied));
    assert_eq!(fx.mail.effects(), 0);
}

#[test]
fn desconectar_a_caixa_invalida_referencias_e_concessoes() {
    let mut fx = fixture();
    let c = fx.store.db.locked();
    let z = bot(5);
    grant(&c, &fx.a, &z, &[Permission::View, Permission::Delete]);
    let a_ref = message_ref(&c, &fx.a, SEGREDO_A);
    disconnect_mailbox(&c, &fx.a).unwrap();
    assert_eq!(open(&c, &fx.mail, &z, &a_ref), Err(MailError::Denied));
    assert_eq!(trash(&c, &mut fx.mail, &z, &a_ref, "k"), Err(MailError::Denied));
    assert_eq!(list(&c, &fx.mail, &z, &fx.a), Err(MailError::Denied));
    // O endereço volta a ser livre só para uma caixa nova, sem herdar nada.
    let again = connect_mailbox(&c, "Suporte", &[SUPORTE]).unwrap();
    assert_ne!(again, fx.a);
    assert_eq!(list(&c, &fx.mail, &z, &again), Err(MailError::Denied));
    // Endereço já usado por outra caixa e endereço inválido são recusados.
    assert_eq!(connect_mailbox(&c, "Dup", &[OPERACOES]), Err(MailError::Invalid));
    assert_eq!(connect_mailbox(&c, "Ruim", &["sem-arroba"]), Err(MailError::Invalid));
}

#[test]
fn migracao_repete_sem_efeito_e_nao_toca_o_registro_legado() {
    let fx = fixture();
    let c = fx.store.db.locked();
    super::migrate(&c).unwrap();
    super::migrate(&c).unwrap();
    assert_eq!(mailboxes(&c).unwrap().len(), 2);
    let legacy: i64 = c
        .query_row(
            "SELECT (SELECT COUNT(*) FROM donna_plugin_installations WHERE builtin='email')
                  + (SELECT COUNT(*) FROM donna_plugin_resources WHERE kind='mailbox')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(legacy, 0);
}
