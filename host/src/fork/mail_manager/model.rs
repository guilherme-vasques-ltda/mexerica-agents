//! Tipos do Mail Manager: participante, permissão, referências opacas, erros e o
//! contrato com o transporte (`MailBackend`). Nada aqui concede acesso: só `access`
//! decide, a cada operação.
use serde::{Deserialize, Serialize};

/// Quem age sobre uma caixa. O dono que administra não é participante: administrar
/// (conectar, conceder, revogar) não dá leitura nem envio.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Participant {
    Bot(String),
    /// Previsto no contrato; o Hub ainda não tem identidade de pessoa, então nenhuma
    /// concessão a pessoa é aceita e toda operação de pessoa é negada.
    Person(String),
}

impl Participant {
    pub(super) fn kind(&self) -> &'static str {
        match self {
            Self::Bot(_) => "bot",
            Self::Person(_) => "person",
        }
    }

    pub(super) fn id(&self) -> &str {
        match self {
            Self::Bot(id) | Self::Person(id) => id,
        }
    }
}

/// As três permissões, independentes entre si.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Permission {
    /// Lista, busca, prévia, corpo, anexos e aviso de mensagem nova.
    View,
    /// Escrever novo; responder no fio exige também `View`.
    Send,
    /// Mover para a lixeira com uma referência já obtida. Não existe apagar de vez.
    Delete,
}

impl Permission {
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::Send => "send",
            Self::Delete => "delete",
        }
    }

    pub(super) fn parse(key: &str) -> Option<Self> {
        match key {
            "view" => Some(Self::View),
            "send" => Some(Self::Send),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }
}

/// Referência opaca a uma mensagem, emitida pelo Hub e presa à caixa. Não carrega
/// autoridade: quem a usa passa de novo pela permissão da caixa dela.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct MessageRef(pub(crate) String);

/// Referência opaca a um anexo de uma mensagem, nas mesmas condições de [`MessageRef`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct AttachmentRef(pub(crate) String);

/// Linha de lista ou de busca, só para quem pode ver a caixa.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Summary {
    pub(crate) mailbox: String,
    pub(crate) message: MessageRef,
    pub(crate) from: String,
    pub(crate) subject: String,
    pub(crate) preview: String,
    pub(crate) has_attachments: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Attachment {
    pub(crate) reference: AttachmentRef,
    pub(crate) name: String,
    pub(crate) size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Message {
    pub(crate) mailbox: String,
    pub(crate) reference: MessageRef,
    pub(crate) from: String,
    pub(crate) subject: String,
    pub(crate) body: String,
    pub(crate) attachments: Vec<Attachment>,
}

/// Aviso de mensagem nova: a referência e quem pode recebê-lo, sem assunto, remetente
/// nem corpo. Quem não pode ver a caixa não entra em `to`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Notice {
    pub(crate) mailbox: String,
    pub(crate) message: MessageRef,
    pub(crate) to: Vec<Participant>,
}

/// Visão do administrador: estado e concessões, sem nenhum dado de mensagem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MailboxAdmin {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) addresses: Vec<String>,
    pub(crate) grants: Vec<(Participant, Permission)>,
}

/// Resultado de um efeito, guardado no recibo e devolvido igual a cada repetição.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Outcome {
    Sent { message: MessageRef },
    Trashed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MailError {
    /// Sem permissão, plugin desligado, participante inválido ou referência que não é
    /// desta caixa: a mesma resposta, para não revelar o que existe.
    Denied,
    /// O remetente pedido não é endereço da caixa autorizada.
    SenderNotAllowed,
    Invalid,
    /// Autorizado, mas a mensagem ou o anexo não existe mais no provedor.
    NotFound,
    /// A mesma chave de operação foi usada para um pedido diferente.
    KeyReused,
    /// Uma tentativa anterior com esta chave não teve resultado confirmado; não repetir.
    Unconfirmed,
    /// O provedor recusou sem efeito; a mesma chave pode tentar de novo.
    Rejected,
    Storage,
}

impl std::fmt::Display for MailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Denied => "mail_denied",
            Self::SenderNotAllowed => "mail_sender_not_allowed",
            Self::Invalid => "mail_invalid",
            Self::NotFound => "mail_not_found",
            Self::KeyReused => "mail_key_reused",
            Self::Unconfirmed => "mail_unconfirmed",
            Self::Rejected => "mail_rejected",
            Self::Storage => "mail_storage_unavailable",
        })
    }
}

impl std::error::Error for MailError {}

pub(crate) type Result<T> = std::result::Result<T, MailError>;

/// Só o código estático do erro sai no log; a mensagem do SQLite é descartada.
pub(super) fn db_error(error: rusqlite::Error) -> MailError {
    let code = match error {
        rusqlite::Error::SqliteFailure(code, message) => {
            drop(message);
            Some(code.code)
        }
        _ => None,
    };
    tracing::warn!(?code, "mail_manager: falha no banco");
    MailError::Storage
}

/// Mensagem a enviar, já validada e com remetente conferido.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Outgoing {
    pub(crate) from: String,
    pub(crate) to: Vec<String>,
    pub(crate) subject: String,
    pub(crate) body: String,
    /// Mensagem respondida (ID do provedor), quando é resposta no fio.
    pub(crate) in_reply_to: Option<String>,
}

/// O que o transporte devolve numa lista ou busca. IDs são do provedor e nunca saem do Hub.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BackendSummary {
    pub(crate) id: String,
    pub(crate) from: String,
    pub(crate) subject: String,
    pub(crate) preview: String,
    pub(crate) has_attachments: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BackendAttachment {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BackendMessage {
    /// Endereço puro do remetente, sem nome de exibição: separar o nome é do transporte.
    pub(crate) from: String,
    pub(crate) subject: String,
    pub(crate) body: String,
    pub(crate) attachments: Vec<BackendAttachment>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BackendError {
    /// Recusado sem nenhum efeito.
    Rejected,
    /// Não se sabe se o efeito aconteceu.
    Unknown,
}

/// Transporte de uma caixa. Só o Hub o chama, depois de autorizar; não há operação de
/// apagar de vez, só mover para a lixeira.
pub(crate) trait MailBackend {
    fn list(&self, mailbox: &str) -> std::result::Result<Vec<BackendSummary>, BackendError>;
    fn search(&self, mailbox: &str, query: &str) -> std::result::Result<Vec<BackendSummary>, BackendError>;
    fn open(&self, mailbox: &str, message: &str) -> std::result::Result<Option<BackendMessage>, BackendError>;
    fn attachment(
        &self,
        mailbox: &str,
        message: &str,
        attachment: &str,
    ) -> std::result::Result<Option<Vec<u8>>, BackendError>;
    /// Devolve o ID do provedor da mensagem enviada.
    fn send(&mut self, mailbox: &str, mail: &Outgoing) -> std::result::Result<String, BackendError>;
    fn trash(&mut self, mailbox: &str, message: &str) -> std::result::Result<(), BackendError>;
}

/// Tamanho máximo do assunto, em bytes.
pub(super) const SUBJECT_MAX: usize = 998;

/// Endereço simples `local@dominio`, sem espaço nem caractere de controle (impede injeção
/// de cabeçalho). Só o domínio vai para minúsculas: a parte local mantém a caixa, como
/// pede o SMTP. Qualquer outra forma é inválida.
pub(super) fn address(raw: &str) -> Result<String> {
    let raw = raw.trim();
    let mut parts = raw.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return invalid("endereço sem a forma local@dominio");
    };
    let ok = !local.is_empty()
        && domain.contains('.')
        && domain.split('.').all(|label| !label.is_empty())
        && raw.chars().all(|c| !c.is_whitespace() && !c.is_control() && !matches!(c, '<' | '>' | ',' | ';' | '"'));
    let address = format!("{local}@{}", domain.to_lowercase());
    if !ok || address.len() > 254 {
        return invalid("endereço com caractere, domínio ou tamanho inválido");
    }
    Ok(address)
}

/// Assunto em uma linha, sem caractere de controle.
pub(super) fn subject(raw: &str) -> Result<String> {
    if raw.chars().any(char::is_control) || raw.len() > SUBJECT_MAX {
        return invalid("assunto com quebra de linha ou longo demais");
    }
    Ok(raw.to_owned())
}

/// Recusa por formato, com o motivo estático no log e nunca o valor recebido.
pub(super) fn invalid<T>(reason: &'static str) -> Result<T> {
    tracing::warn!(reason, "mail_manager: entrada recusada");
    Err(MailError::Invalid)
}
