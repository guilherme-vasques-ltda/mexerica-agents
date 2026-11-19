//! Fronteira real do executor. Sem callback de autorização não se prepara nem aplica material.
use super::queue::{Context, Reservation};
use crate::{LockExt, hub::Hub};
use anyhow::{Result, ensure};
use codync_mem::origin::Origin;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// Contexto/prova mudaram de modo permanente; erros SQL continuam distinguíveis como transitórios.
#[derive(Debug)]
pub(crate) enum InvalidSnapshot {
    Job,
    Context,
    Evidence,
    InputTooLarge,
}
impl std::fmt::Display for InvalidSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Job => "Trabalho de memória desatualizado.",
            Self::Context => "Contexto de memória desatualizado.",
            Self::Evidence => "Evidência de memória desatualizada.",
            Self::InputTooLarge => "Material de memória acima do limite técnico.",
        })
    }
}
impl std::error::Error for InvalidSnapshot {}

pub(crate) fn is_transient(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.downcast_ref::<rusqlite::Error>().is_some())
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Message {
    pub id: String,
    pub revision: i64,
    pub from_user: bool,
    pub text: String,
    pub origin: Origin,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExtractionInput {
    pub job_id: String,
    pub bot_id: String,
    pub chat_id: String,
    pub space_id: String,
    pub memory_id: String,
    pub scope_revision: i64,
    pub messages: Vec<Message>,
    pub existing: Vec<codync_mem::store::Entry>,
    pub notes: Vec<Note>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Note {
    pub file: codync_mem::files::File,
    pub content: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Operation {
    /// Decisão fechada exclusiva do extrator autenticado; não carrega candidato nem razão livre.
    NotDurable { evidence: Vec<String> },
    Remember {
        text: String,
        #[serde(default)]
        shared: bool,
        evidence: Vec<String>,
        #[serde(default)]
        nature: Option<codync_mem::nature::Hint>,
    },
    Revise {
        entry: i64,
        #[serde(rename = "expectedRev")]
        expected_rev: i64,
        text: String,
        evidence: Vec<String>,
        #[serde(default)]
        nature: Option<codync_mem::nature::Hint>,
    },
    Archive {
        entry: i64,
        #[serde(rename = "expectedRev")]
        expected_rev: i64,
        evidence: Vec<String>,
        #[serde(default)]
        nature: Option<codync_mem::nature::Hint>,
    },
    Note {
        file: String,
        content: Option<String>,
        #[serde(default)]
        shared: bool,
        #[serde(rename = "expectedHash")]
        expected_hash: Option<String>,
        evidence: Vec<String>,
        #[serde(default)]
        nature: Option<codync_mem::nature::Hint>,
    },
    /// Sono 2b1c, atalho literal: a inferência sem conflito está, letra por letra, numa mensagem limpa do dono e vira
    /// "Você disse". Só o Hub a emite (o sono, ou o dono ao aprovar a sugestão); `commit_in` recusa no plano do
    /// extrator e nas propostas do MCP. A prova é recalculada pelo Hub em `apply_one`, nunca vem do pedido.
    Confirm {
        entry: i64,
        #[serde(rename = "expectedRev")]
        expected_rev: i64,
        evidence: Vec<String>,
    },
    /// Sono 2b1d-2, (a): a inferência sem conflito que o modelo achou sustentada por uma mensagem do dono vira
    /// "Confirmada pela sua mensagem" (`confirmed`, nunca `said`), só quando o dono aprova a sugestão. Só o Hub a
    /// emite; `commit_in` a recusa no plano do extrator e nas propostas do MCP, como o `Confirm`. `sources`: as
    /// mensagens da linhagem com as revisões de quando a sugestão nasceu, reconferidas na aprovação.
    Reclassify {
        entry: i64,
        #[serde(rename = "expectedRev")]
        expected_rev: i64,
        evidence: Vec<String>,
        sources: Vec<(String, i64)>,
    },
}
impl Operation {
    pub(crate) fn evidence(&self) -> &[String] {
        match self {
            Self::Remember { evidence, .. }
            | Self::NotDurable { evidence }
            | Self::Revise { evidence, .. }
            | Self::Archive { evidence, .. }
            | Self::Note { evidence, .. }
            | Self::Confirm { evidence, .. }
            | Self::Reclassify { evidence, .. } => evidence,
        }
    }
    pub(crate) fn nature(&self) -> Option<codync_mem::nature::Hint> {
        match self {
            Self::NotDurable { .. } | Self::Confirm { .. } | Self::Reclassify { .. } => None,
            Self::Remember { nature, .. }
            | Self::Revise { nature, .. }
            | Self::Archive { nature, .. }
            | Self::Note { nature, .. } => *nature,
        }
    }
    pub(crate) fn text(&self) -> &str {
        match self {
            Self::Remember { text, .. } | Self::Revise { text, .. } => text,
            Self::Note { content, .. } => content.as_deref().unwrap_or_default(),
            Self::Archive { .. } | Self::NotDurable { .. } | Self::Confirm { .. } | Self::Reclassify { .. } => "",
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExtractionPlan {
    pub operations: Vec<Operation>,
}

#[derive(Clone, Copy)]
pub(crate) enum Unavailable {
    Transient,
    NoSelection,
    Rejected,
    /// D389 `off` decidido na fronteira de preparo/envio: o pedido volta a esperar, sem ocorrência nova.
    LearningOff,
}

pub(crate) fn release(hub: &Hub, reservation: &Reservation, why: Unavailable) -> Result<()> {
    let (status, reason, audit_reason) = match why {
        Unavailable::Transient => ("waiting", "transient", super::audit::Reason::TechnicalDeferred),
        Unavailable::NoSelection => ("waiting", "noSelection", super::audit::Reason::NoSelection),
        Unavailable::Rejected => ("ineligible", "ineligible", super::audit::Reason::Ineligible),
        Unavailable::LearningOff => return release_state(hub, reservation, "waiting", "noSelection", None),
    };
    release_state(hub, reservation, status, reason, Some(audit_reason))
}

/// Checkpoint ainda não tentou selecionar extrator: conserva a fila, sem inventar uma ocorrência `NoSelection`.
pub(super) fn await_extractor(hub: &Hub, reservation: &Reservation) -> Result<()> {
    release_state(hub, reservation, "waiting", "noSelection", None)
}

fn release_state(
    hub: &Hub,
    reservation: &Reservation,
    status: &str,
    reason: &str,
    audit_reason: Option<super::audit::Reason>,
) -> Result<()> {
    let mut db = hub.store.db.locked();
    let tx = db.transaction()?;
    let changed = tx.execute(
        "UPDATE donna_memory_auto_jobs SET status=?3,reason=?4,reservation=NULL
        WHERE id=?1 AND reservation=?2 AND status='reserved'",
        params![reservation.id, reservation.token, status, reason],
    )? > 0;
    let recorded = match (changed, audit_reason) {
        (true, Some(reason)) => super::audit::record_job(&tx, &reservation.id, reason, None, crate::store::now_ms())?,
        _ => false,
    };
    tx.commit()?;
    drop(db);
    if recorded {
        super::audit::notify(hub, &reservation.context);
    }
    Ok(())
}

/// Guarda Connection-only: não chama Store, Cloud ou resolvedor dentro da transação.
pub(crate) fn validate(db: &Connection, r: &Reservation) -> Result<()> {
    let valid: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_memory_auto_jobs WHERE id=?1
        AND reservation=?2 AND status='reserved' AND outcome='success' AND scope='allowed' AND scope_revision=?3
        AND admitted_at>?4)",
        params![r.id, r.token, r.scope_revision, crate::store::now_ms() - super::queue::LIFE_MS],
        |row| row.get(0),
    )?;
    ensure!(valid, InvalidSnapshot::Job);
    ensure!(r.context.closing_turn.is_some_and(|turn| turn > 0), InvalidSnapshot::Evidence);
    validate_context(db, &r.context)?;
    for evidence in &r.context.messages {
        // A mesma classificação da admissão, inclusive a cerca genUI da main.
        // Ler pela Connection mantém a guarda transacional sem reentrar no Store.
        let entry = db
            .query_row(
                "SELECT seq,id,bot_id,rev,kind,turn,data,created_at,updated_at,COALESCE(session,bot_id)
                FROM entries WHERE id=?1 AND bot_id=?2 AND rev=?3",
                params![evidence.id, r.context.chat_id, evidence.revision],
                |row| {
                    let raw: String = row.get(6)?;
                    let data = serde_json::from_str(&raw).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(error))
                    })?;
                    Ok(crate::store::Entry {
                        seq: row.get(0)?,
                        id: row.get(1)?,
                        bot_id: row.get(2)?,
                        rev: row.get(3)?,
                        kind: row.get(4)?,
                        turn: row.get(5)?,
                        data,
                        created_at: row.get(7)?,
                        updated_at: row.get(8)?,
                        session_id: row.get(9)?,
                    })
                },
            )
            .optional()?
            .ok_or(InvalidSnapshot::Evidence)?;
        ensure!(entry.is_user_authored(), InvalidSnapshot::Evidence);
        ensure!(
            super::queue::eligible(db, &r.context.bot_id, &r.context.chat_id, &evidence.id, evidence.revision)?,
            InvalidSnapshot::Evidence
        );
    }
    for response in &r.context.responses {
        let raw: String = db
            .query_row(
                "SELECT data FROM entries WHERE id=?1 AND bot_id=?2 AND kind='agent' AND turn=?3",
                params![response.id, r.context.chat_id, r.context.closing_turn],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(InvalidSnapshot::Evidence)?;
        let data: serde_json::Value = serde_json::from_str(&raw)?;
        ensure!(
            data["author"].as_str() == Some(r.context.bot_id.as_str())
                && super::queue::hash(data["text"].as_str().unwrap_or_default()) == response.hash,
            InvalidSnapshot::Evidence
        );
    }
    Ok(())
}

pub(crate) fn validate_context(db: &Connection, c: &Context) -> Result<()> {
    let raw: Option<String> = db
        .query_row(
            "SELECT b.config FROM bots b JOIN donna_memory_auto_bot_bindings m ON m.bot=b.id
             WHERE b.id=?1 AND b.deleted=0 AND m.revision=?2",
            params![c.bot_id, c.bot_revision],
            |r| r.get(0),
        )
        .optional()?;
    let cfg: crate::store::BotConfig =
        serde_json::from_str(&raw.ok_or(InvalidSnapshot::Context)?).map_err(|_| InvalidSnapshot::Context)?;
    ensure!(!cfg.is_group(), InvalidSnapshot::Context);
    if let Some(revision) = c.conversation_revision {
        let valid: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM donna_space_conversations WHERE id=?1
            AND bot_id=?2 AND space_id=?3 AND revision=?4 AND closed=0)",
            params![c.chat_id, c.bot_id, c.space_id, revision],
            |r| r.get(0),
        )?;
        ensure!(valid, InvalidSnapshot::Context);
        if cfg.space_id == c.space_id {
            ensure!(cfg.memory_id() == c.memory_id, InvalidSnapshot::Context);
        } else {
            let key = format!("donna.space.memory.{}.{}", c.space_id, c.bot_id);
            let mapped: Option<String> = db.query_row("SELECT v FROM kv WHERE k=?1", [key], |r| r.get(0)).optional()?;
            ensure!(mapped.as_deref() == Some(c.memory_id.as_str()), InvalidSnapshot::Context);
        }
    } else {
        ensure!(
            c.chat_id == c.bot_id && cfg.space_id == c.space_id && cfg.memory_id() == c.memory_id,
            InvalidSnapshot::Context
        );
    }
    Ok(())
}

/// D384: posse tipada do `memory_boundary` de UM Hub. A composição Owner a adquire ANTES da guarda Cloud
/// (MB → C); as variantes `*_guarded` a recebem e nunca readquirem MB. RAII: o `MutexGuard` solta ao sair do escopo
/// e não é `Send`, então a seção não atravessa await nem thread. Não é Authority, token de turno nem concessão.
pub(crate) struct MemorySection<'a> {
    hub: &'a Hub,
    _guard: std::sync::MutexGuard<'a, ()>,
}
impl<'a> MemorySection<'a> {
    /// Único construtor: adquire o MB deste Hub.
    pub(crate) fn enter(hub: &'a Hub) -> Self {
        Self { hub, _guard: hub.store.memory_boundary.locked() }
    }
    /// O Hub cujo MB está retido.
    pub(crate) fn hub(&self) -> &'a Hub {
        self.hub
    }
}

/// Fachada local (testes): cria a seção e prepara.
#[cfg(test)]
pub(crate) fn prepare(
    hub: &Hub,
    r: &Reservation,
    authorize: impl Fn(&Connection, &Context) -> Result<()>,
) -> Result<ExtractionInput> {
    let section = MemorySection::enter(hub);
    prepare_guarded(&section, r, authorize)
}

/// Preparo sob o MB já retido pela composição; não readquire MB. Elegibilidade antes do DB longo: cada leitura do
/// Store pega e solta o DB, sem reentrada.
pub(crate) fn prepare_guarded(
    section: &MemorySection<'_>,
    r: &Reservation,
    authorize: impl Fn(&Connection, &Context) -> Result<()>,
) -> Result<ExtractionInput> {
    let hub = section.hub();
    let mut evidence_entries = Vec::new();
    for reference in &r.context.messages {
        let entry = hub.store.entry(&reference.id).ok_or(InvalidSnapshot::Evidence)?;
        ensure!(
            hub.store.memory_turn_eligible(&r.context.bot_id, &r.context.chat_id, entry.turn)?,
            InvalidSnapshot::Evidence
        );
        evidence_entries.push(entry);
    }
    // D6: a pessoa do job vem das próprias mensagens de evidência, pela origem que o Hub carimbou. Sem pessoa
    // comprovada, o extrator não lê nota "Sobre você" nenhuma; com ela, só a dessa pessoa.
    let person = crate::chat::memory::profile::person_of(&evidence_entries);
    let owner = person.as_deref() == Some(crate::chat::memory::profile::OWNER);
    let db = hub.store.db.locked();
    validate(&db, r)?;
    authorize(&db, &r.context)?;
    let mut messages = Vec::new();
    let mut bytes = 0;
    for evidence in &r.context.messages {
        let raw: String = db.query_row("SELECT data FROM entries WHERE id=?1", [&evidence.id], |row| row.get(0))?;
        let data: serde_json::Value = serde_json::from_str(&raw)?;
        let text = codync_mem::redact::redact(data["text"].as_str().unwrap_or_default());
        bytes += text.len();
        ensure!(bytes <= 96_000, InvalidSnapshot::InputTooLarge);
        let origin = super::origin::derive(&db, &r.context, std::slice::from_ref(&evidence.id), "")?;
        messages.push(Message { id: evidence.id.clone(), revision: evidence.revision, from_user: true, text, origin });
    }
    for response in &r.context.responses {
        let (raw, revision): (String, i64) =
            db.query_row("SELECT data,rev FROM entries WHERE id=?1", [&response.id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
        let data: serde_json::Value = serde_json::from_str(&raw)?;
        let text = codync_mem::redact::redact(data["text"].as_str().unwrap_or_default());
        bytes += text.len();
        ensure!(bytes <= 96_000, InvalidSnapshot::InputTooLarge);
        let origin = super::origin::derive(&db, &r.context, std::slice::from_ref(&response.id), "")?;
        messages.push(Message { id: response.id.clone(), revision, from_user: false, text, origin });
    }
    let mut existing = codync_mem::store::layer(&db, Some(&r.context.memory_id))?;
    // Compartilhada somente Pessoal, com escolha atual do bot (verificada no efeito novamente).
    let raw: String = db.query_row("SELECT config FROM bots WHERE id=?1", [&r.context.bot_id], |row| row.get(0))?;
    let cfg: crate::store::BotConfig = serde_json::from_str(&raw)?;
    if r.context.space_id == crate::fork::spaces::PERSONAL && crate::chat::memory::shared(&cfg) {
        existing.extend(codync_mem::store::layer(&db, None)?);
    }
    for entry in &mut existing {
        entry.text = codync_mem::redact::redact(&entry.text);
        redact_origin(&mut entry.origin);
        entry.conflict_text = entry.conflict_text.as_ref().map(|s| codync_mem::redact::redact(s));
        if let Some(previous) = &mut entry.previous_version {
            previous.text = codync_mem::redact::redact(&previous.text);
            previous.conflict_text = previous.conflict_text.as_ref().map(|s| codync_mem::redact::redact(s));
            redact_origin(&mut previous.origin);
        }
    }
    let mut notes = Vec::new();
    let mut layers = vec![Some(r.context.memory_id.as_str())];
    if r.context.space_id == crate::fork::spaces::PERSONAL && crate::chat::memory::shared(&cfg) {
        layers.push(None);
    }
    for layer in layers {
        for mut file in codync_mem::files::layer(&db, layer)? {
            // A nota da camada compartilhada é a do dono no Pessoal: só num job dele.
            if file.deleted || (layer.is_none() && file.name == crate::chat::memory::profile::NAME && !owner) {
                continue;
            }
            file.description = file.description.map(|s| codync_mem::redact::redact(&s));
            redact_origin(&mut file.origin);
            file.conflict_text = file.conflict_text.as_ref().map(|s| codync_mem::redact::redact(s));
            let content = codync_mem::files::current(&db, layer, &file.name)?.map(|s| {
                let text = if layer.is_none() && file.name == crate::chat::memory::profile::NAME {
                    crate::chat::memory::profile::visible(&s)
                } else {
                    &s
                };
                codync_mem::redact::redact(text)
            });
            notes.push(Note { file, content });
        }
    }
    // D6: fora do Pessoal, o extrator lê (sem poder gravar) só a nota "Sobre você" da pessoa do job no Space da
    // conversa, e só quando o bot é desse Space e a recebe; nunca a do Pessoal nem a de outra pessoa.
    if r.context.space_id == cfg.space_id
        && crate::chat::memory::sees_profile(&cfg)
        && let Some(person) = person.as_deref()
        && let Some((mut file, text)) = crate::chat::memory::profile::extractor_note(&db, person, &r.context.space_id)?
    {
        file.description = file.description.map(|s| codync_mem::redact::redact(&s));
        redact_origin(&mut file.origin);
        file.conflict_text = file.conflict_text.as_ref().map(|s| codync_mem::redact::redact(s));
        notes.push(Note { file, content: Some(codync_mem::redact::redact(&text)) });
    }
    let input = ExtractionInput {
        job_id: r.id.clone(),
        bot_id: r.context.bot_id.clone(),
        chat_id: r.context.chat_id.clone(),
        space_id: r.context.space_id.clone(),
        memory_id: r.context.memory_id.clone(),
        scope_revision: r.scope_revision,
        messages,
        existing,
        notes,
    };
    ensure!(serde_json::to_vec(&input)?.len() <= 128_000, InvalidSnapshot::InputTooLarge);
    validate(&db, r)?;
    authorize(&db, &r.context)?;
    Ok(input)
}

fn redact_origin(origin: &mut Origin) {
    for source in &mut origin.sources {
        source.detail = source.detail.as_ref().map(|s| codync_mem::redact::redact(s));
    }
}
