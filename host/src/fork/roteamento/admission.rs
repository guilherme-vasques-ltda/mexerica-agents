//! Dados privados de persistência da rota-03a, sem autoridade ou produtor de execução.
//! Contrato semântico de Sessões: handoff 93ca43c6. Nada aqui cria token/binding.
use super::{config, selection};
use crate::fork::{
    sessoes::{origin::SessionOrigin, store::SessionRef},
    spaces::EngineSelection,
};
use anyhow::{Result, anyhow, ensure};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, HashSet};

pub(crate) const DOCUMENT_MAX: usize = 65_536;
pub(crate) const STAMP_MAX: usize = 4_096;
pub(crate) const ENTRIES_MAX: usize = 128;
pub(crate) const REVISION_MAX: u64 = 9_007_199_254_740_991;

/// Só os tipos privados passam pelo codec. Value é temporário, nunca o modelo de armazenamento.
/// A volta exata recusa campos Option ausentes que serde interpretaria como NULL.
pub(super) fn decode<T: DeserializeOwned + Serialize>(raw: &str, max: usize) -> Result<T> {
    ensure!(raw.len() <= max, "routing_document_invalid");
    // Desserializar o tipo diretamente também recusa campos de struct duplicados.
    let parsed: T = serde_json::from_str(raw).map_err(|_| anyhow!("routing_document_invalid"))?;
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|_| anyhow!("routing_document_invalid"))?;
    ensure!(serde_json::to_value(&parsed)? == value, "routing_document_invalid");
    Ok(parsed)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Destination {
    pub bot_id: String,
    pub space_id: String,
    #[serde(deserialize_with = "session_ref")]
    pub session: SessionRef,
}

fn session_ref<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<SessionRef, D::Error> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct StoredSession {
        id: String,
        chat_id: String,
    }
    let value = StoredSession::deserialize(d)?;
    Ok(SessionRef { id: value.id, chat_id: value.chat_id })
}

/// A audiência é o próprio destino canônico, sem campo livre paralelo.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CheckpointKey {
    pub destination: Destination,
    /// Mesmo token recebido do produtor. Não é gerado, normalizado ou atestado aqui.
    pub execution_token: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Intent {
    pub entry_ids: Vec<String>,
    pub hub_turn: i64,
    /// Pré-imagem capturada: conferir antes do CAS de Sessões na Tx externa IMMEDIATE.
    pub expected_session_revision: i64,
}

/// Referência recebida, não concessão nem declaração de binding vivo.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CapturedBinding {
    pub binding_ref: String,
}

/// Dado do snapshot; não é `EngineChoice` nem escritor da coluna de Sessões.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SelectionSnapshot {
    pub account_id: String,
    pub selection: EngineSelection,
}

/// Ausência real da fonte é distinta de texto "null" e de fato não atestado.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum CapturedText {
    Absent,
    Present { value: String },
}
impl CapturedText {
    /// Codec de valor já capturado. Não lê fonte, não gera identidade ou prova.
    pub(crate) fn from_captured(value: Option<String>) -> Self {
        match value {
            None => Self::Absent,
            Some(value) => Self::Present { value },
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BotStamp {
    account: Option<String>,
    revision: Option<u64>,
    backend: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SpaceStamp {
    account: Option<String>,
    revision: Option<u64>,
    backend: String,
    model: Option<String>,
    model_options: BTreeMap<String, String>,
}

/// Fonte/schema explícitos, mantendo inclusive os bytes do JSON original.
/// Modelo/opções do snapshot nunca completam o carimbo Bot.
/// `SessionAccountStampV1`: carimbo próprio da sessão com seleção efetiva (extra ou, pela decisão A
/// da D372, a principal do Pessoal), mesmo JSON do carimbo Bot; produtor é Sessões. Não cai para o
/// carimbo Bot nem completa campos faltantes.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum StampCapture {
    BotAccountStampV1 { capture: CapturedText },
    SpaceEngineStampV1 { capture: CapturedText },
    SessionAccountStampV1 { capture: CapturedText },
}
impl StampCapture {
    pub(super) fn source(&self, d: &Destination) -> (String, &CapturedText) {
        match self {
            Self::BotAccountStampV1 { capture } => (format!("account.stamp.{}", d.bot_id), capture),
            Self::SpaceEngineStampV1 { capture } => (format!("donna.space.engine.{}", d.session.chat_id), capture),
            Self::SessionAccountStampV1 { capture } => {
                (format!("account.stamp.{}/session:{}", d.bot_id, d.session.id), capture)
            }
        }
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::BotAccountStampV1 { capture: CapturedText::Present { value } }
            | Self::SessionAccountStampV1 { capture: CapturedText::Present { value } } => {
                if let Some(stamp) = decode::<Option<BotStamp>>(value, STAMP_MAX)? {
                    stamp_fields(stamp.account.as_deref(), stamp.revision, &stamp.backend)?;
                }
            }
            Self::SpaceEngineStampV1 { capture: CapturedText::Present { value } } => {
                if let Some(stamp) = decode::<Option<SpaceStamp>>(value, STAMP_MAX)? {
                    stamp_fields(stamp.account.as_deref(), stamp.revision, &stamp.backend)?;
                    model_fields(stamp.model.as_deref(), &stamp.model_options)?;
                }
            }
            Self::BotAccountStampV1 { capture: CapturedText::Absent }
            | Self::SpaceEngineStampV1 { capture: CapturedText::Absent }
            | Self::SessionAccountStampV1 { capture: CapturedText::Absent } => {}
        }
        Ok(())
    }
}

/// Evidência opaca diferente do carimbo; não é decodificada nem recalculada.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum ExecutionSeal {
    NotCaptured,
    SpaceExecutionStampV1 { capture: CapturedText },
}

/// Estado obrigatório e fechado. Não habilita admissão que dependa deste fato.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RequiredFact {
    RequiredFactNotAttested,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UnattestedFacts {
    pub coverage_before: RequiredFact,
    pub coverage_after: RequiredFact,
    pub account_identity_generation_catalog: RequiredFact,
}

/// Fatos estreitos da entrada original; nem `SessionOrigin::Owner` nem esses marcadores
/// atestam origem humana/Authority. Não se guarda o corpo da mensagem.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub(crate) enum EntryEvidence {
    RequiredFactNotAttested,
    HubEntrySourceBotV1 { source_bot_id: String },
    HubEntryRunV1 { run_id: String },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct EntryProvenance {
    pub entry_id: String,
    pub evidence: EntryEvidence,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TurnProvenance {
    pub entries: Vec<EntryProvenance>,
    /// Não existe um produtor uniforme de prova ampla da origem do turno nesta base.
    pub authority_origin: RequiredFact,
}

/// Rótulos semânticos persistidos, sem implementação de Auto/reserva/seleção.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DecidedBy {
    Fixo,
    Auto,
    Reserva,
    Dono,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) enum Reason {
    #[serde(rename = "fixedSelection")]
    Fixed,
    #[serde(rename = "explicitSelection")]
    Explicit,
    #[serde(rename = "frozenSessionSelection")]
    FrozenSession,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RoutingSnapshot {
    pub requested: SelectionSnapshot,
    pub admitted: SelectionSnapshot,
    pub decided_by: DecidedBy,
    pub reason: Reason,
    /// As três revisões são as do INSTANTE DA ADMISSÃO (Fixo, sem decisão Auto), lidas por
    /// `receipt::revisions` na mesma Tx do permit; não são snapshot de proposta.
    pub policy_revision: u64,
    pub general_guide_revision: u64,
    pub bot_guide_revision: u64,
    pub session_origin: SessionOrigin,
    pub turn_provenance: TurnProvenance,
    pub required_facts: UnattestedFacts,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AdmissionCapture {
    pub schema_version: u8,
    pub key: CheckpointKey,
    pub intent: Intent,
    pub binding: CapturedBinding,
    /// Fora de `EngineSelection/engine_choice` e distinta da revisão do carimbo antigo.
    pub account_revision: u64,
    /// Pré-imagem bruta da fonte: não substituir pelo carimbo pós-escrita da admissão.
    pub private_stamp: StampCapture,
    pub execution_seal: ExecutionSeal,
    pub routing: RoutingSnapshot,
}

pub(super) fn opaque(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 256
            && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b)),
        "routing_reference_invalid"
    );
    Ok(())
}
fn account_id(value: &str) -> Result<()> {
    ensure!(
        value.len() == 67 && value.starts_with("ac_") && value[3..].bytes().all(|b| b.is_ascii_hexdigit()),
        "routing_account_invalid"
    );
    Ok(())
}
fn atom(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
        && !value.contains("://")
        && !value.starts_with('/')
}
fn model_fields(model: Option<&str>, options: &BTreeMap<String, String>) -> Result<()> {
    ensure!(
        model.is_none_or(atom) && options.len() <= 8 && options.iter().all(|(k, v)| atom(k) && atom(v)),
        "routing_selection_invalid"
    );
    Ok(())
}
fn stamp_fields(account: Option<&str>, revision: Option<u64>, backend: &str) -> Result<()> {
    if let Some(account) = account {
        account_id(account)?;
    }
    ensure!(revision.is_none_or(|r| r > 0 && r <= REVISION_MAX), "routing_stamp_invalid");
    ensure!(matches!(backend, "codex" | "claude"), "routing_stamp_invalid");
    Ok(())
}
fn session_origin_shape(origin: &SessionOrigin) -> Result<()> {
    match origin {
        SessionOrigin::Bot { bot_id, from_session, from_execution } => {
            config::id(bot_id)?;
            if let Some(id) = from_session {
                config::id(id)?;
            }
            if let Some(id) = from_execution {
                opaque(id)?;
            }
        }
        SessionOrigin::Routine { routine_id, run_id } => {
            config::id(routine_id)?;
            if let Some(id) = run_id {
                opaque(id)?;
            }
        }
        SessionOrigin::Suggestion { entry_ids } => {
            ensure!(!entry_ids.is_empty() && entry_ids.len() <= ENTRIES_MAX, "routing_origin_invalid");
            for id in entry_ids {
                config::id(id)?;
            }
        }
        SessionOrigin::Routed { rule } => config::id(rule)?,
        SessionOrigin::Task { task_id } => config::id(task_id)?,
        SessionOrigin::Owner
        | SessionOrigin::Continuation
        | SessionOrigin::Notice
        | SessionOrigin::Group
        | SessionOrigin::Subsession => {}
    }
    Ok(())
}
impl CheckpointKey {
    pub(super) fn validate(&self) -> Result<()> {
        let d = &self.destination;
        for id in [&d.bot_id, &d.space_id, &d.session.id, &d.session.chat_id] {
            config::id(id)?;
        }
        opaque(&self.execution_token)
    }
}
impl AdmissionCapture {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(self.schema_version == 1, "routing_document_invalid");
        self.key.validate()?;
        opaque(&self.binding.binding_ref)?;
        ensure!(
            self.intent.hub_turn > 0
                && u64::try_from(self.intent.hub_turn)? <= REVISION_MAX
                && self.intent.expected_session_revision > 0
                && u64::try_from(self.intent.expected_session_revision)? <= REVISION_MAX
                && self.account_revision > 0
                && self.account_revision <= REVISION_MAX,
            "routing_revision_invalid"
        );
        let ids = &self.intent.entry_ids;
        ensure!(!ids.is_empty() && ids.len() <= ENTRIES_MAX, "routing_entries_invalid");
        let mut unique = HashSet::new();
        for id in ids {
            config::id(id)?;
            ensure!(unique.insert(id), "routing_entries_invalid");
        }
        for selection in [&self.routing.requested, &self.routing.admitted] {
            // Só a extra admite o padrão implícito no recibo; a principal conserva o caminho legado.
            ensure!(
                self.key.destination.session.id != self.key.destination.session.chat_id
                    || selection.selection.model.is_some(),
                "routing_selection_invalid"
            );
            selection::receipt_shape(&config::Reserve {
                account_id: selection.account_id.clone(),
                account_revision: self.account_revision,
                selection: selection.selection.clone(),
            })?;
        }
        for revision in
            [self.routing.policy_revision, self.routing.general_guide_revision, self.routing.bot_guide_revision]
        {
            ensure!(revision <= REVISION_MAX, "routing_revision_invalid");
        }
        let origins = &self.routing.turn_provenance.entries;
        ensure!(origins.len() == ids.len(), "routing_provenance_invalid");
        for (origin, id) in origins.iter().zip(ids) {
            ensure!(origin.entry_id == *id, "routing_provenance_invalid");
            match &origin.evidence {
                EntryEvidence::HubEntrySourceBotV1 { source_bot_id } => config::id(source_bot_id)?,
                EntryEvidence::HubEntryRunV1 { run_id } => opaque(run_id)?,
                EntryEvidence::RequiredFactNotAttested => {}
            }
        }
        // Parser real de Sessões, inclusive validações semânticas; sem ampliar seu significado.
        crate::fork::sessoes::origin::parse(&self.routing.session_origin.column()?)?;
        session_origin_shape(&self.routing.session_origin)?;
        self.private_stamp.validate()?;
        if let ExecutionSeal::SpaceExecutionStampV1 { capture: CapturedText::Present { value } } = &self.execution_seal
        {
            ensure!(
                !value.is_empty()
                    && value.len() <= 256
                    && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
                "routing_seal_invalid"
            );
        }
        Ok(())
    }
}
