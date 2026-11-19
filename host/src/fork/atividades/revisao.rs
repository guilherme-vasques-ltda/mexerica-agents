//! Contratos e estado durável preparatório. Sem consumo, RPC ou autoridade humana ativa.
pub mod admissao;
pub(crate) mod estado;

use super::{
    erro_operacao::OperationError as Error,
    modelo::PlanDigest,
    operacao::{ClientNonce, OperationKind, OperationRef, OperationSelector},
};
use crate::fork::escopo::{ContextRef, Id, ResolvedScope, Revision, Version};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SemanticDigest(PlanDigest);
impl std::fmt::Debug for SemanticDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SemanticDigest([redigido])")
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ReviewBody(String);
impl TryFrom<String> for ReviewBody {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Error> {
        if value.len() > 32768 {
            return Err(Error::InvalidContract);
        }
        Ok(Self(value))
    }
}
impl From<ReviewBody> for String {
    fn from(value: ReviewBody) -> Self {
        value.0
    }
}
impl std::fmt::Debug for ReviewBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReviewBody([redigido])")
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAction {
    Save,
    Accept,
    AcceptWithEdit,
    Reject,
    Delete,
    Copy,
    Bind,
    Release,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewObjectKind {
    Candidate,
    Memory,
    Dataset,
    Skill,
    Update,
    Effect,
    Account,
    Destination,
    Attachment,
    Source,
    Binding,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewObjectRef {
    pub context_ref: ContextRef,
    pub kind: ReviewObjectKind,
    pub object_id: Id,
    pub revision: Revision,
    pub digest: SemanticDigest,
}
/// Descrição capturada; não autentica pessoa, dispositivo ou sessão.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CallerDeviceDescriptor {
    pub principal_id: Id,
    pub device_id: Id,
    pub session_id: Id,
    pub principal_epoch: Revision,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExactEffect {
    /// Bytes do after mostrado no gesto, sem transformação silenciosa.
    pub body: ReviewBody,
    pub account: Option<ReviewObjectRef>,
    pub destination: Option<ReviewObjectRef>,
    pub attachments: Vec<ReviewObjectRef>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BindingWire {
    version: Version,
    context_ref: ContextRef,
    caller_device: CallerDeviceDescriptor,
    recovery_epoch: Id,
    policy_revision: Revision,
    action: ReviewAction,
    target: ReviewObjectRef,
    before: Option<ReviewObjectRef>,
    effect: ExactEffect,
    sources: Vec<ReviewObjectRef>,
    bindings: Vec<ReviewObjectRef>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BindingWire", into = "BindingWire")]
pub struct ReviewBinding(BindingWire);
impl TryFrom<BindingWire> for ReviewBinding {
    type Error = Error;
    fn try_from(value: BindingWire) -> Result<Self, Error> {
        let mut refs = std::iter::once(&value.target)
            .chain(value.before.iter())
            .chain(value.effect.account.iter())
            .chain(value.effect.destination.iter())
            .chain(value.effect.attachments.iter())
            .chain(value.sources.iter())
            .chain(value.bindings.iter());
        if value.effect.attachments.len() + value.sources.len() + value.bindings.len() > 64
            || value.target.context_ref != value.context_ref
            || value.effect.account.as_ref().is_some_and(|r| r.kind != ReviewObjectKind::Account)
            || value.effect.destination.as_ref().is_some_and(|r| r.kind != ReviewObjectKind::Destination)
            || value.effect.attachments.iter().any(|r| r.kind != ReviewObjectKind::Attachment)
            || value.bindings.iter().any(|r| r.kind != ReviewObjectKind::Binding)
            || refs.any(|r| r.context_ref.hub_id() != value.context_ref.hub_id())
        {
            return Err(Error::InvalidContract);
        }
        Ok(Self(value))
    }
}
impl From<ReviewBinding> for BindingWire {
    fn from(value: ReviewBinding) -> Self {
        value.0
    }
}
impl ReviewBinding {
    pub fn digest(&self) -> Result<SemanticDigest, Error> {
        let bytes = serde_json::to_vec(&self.0).map_err(|_| Error::InvalidContract)?;
        let mut hash = Sha256::new();
        hash.update(b"donnabot.review-binding.v1\0");
        hash.update(bytes);
        let mut hex = String::with_capacity(64);
        for byte in hash.finalize() {
            // Escrever em String não falha.
            let _ = write!(hex, "{byte:02x}");
        }
        PlanDigest::try_from(hex).map(SemanticDigest).map_err(|_| Error::InvalidContract)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PrepareWire {
    version: Version,
    prepare_nonce: ClientNonce,
    decision_nonce: ClientNonce,
    binding: ReviewBinding,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PrepareWire", into = "PrepareWire")]
pub struct PrepareReviewInput(PrepareWire);
impl TryFrom<PrepareWire> for PrepareReviewInput {
    type Error = Error;
    fn try_from(value: PrepareWire) -> Result<Self, Error> {
        if value.prepare_nonce == value.decision_nonce {
            return Err(Error::NoncesMustDiffer);
        }
        Ok(Self(value))
    }
}
impl PrepareReviewInput {
    pub fn parse_json(bytes: &[u8]) -> Result<Self, Error> {
        parse_input(bytes)
    }
}
fn parse_input<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    if bytes.len() > 262_144 {
        return Err(Error::InvalidContract);
    }
    serde_json::from_slice(bytes).map_err(|_| Error::InvalidContract)
}
impl From<PrepareReviewInput> for PrepareWire {
    fn from(value: PrepareReviewInput) -> Self {
        value.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecideReviewInput {
    pub version: Version,
    pub decision_operation: OperationSelector,
    pub review_id: Id,
    pub decision_nonce: ClientNonce,
    pub binding: ReviewBinding,
}
impl DecideReviewInput {
    pub fn parse_json(bytes: &[u8]) -> Result<Self, Error> {
        parse_input(bytes)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WindowWire {
    issued_at: Revision,
    expires_at: Revision,
}
/// Janela de contrato, não TTL persistido ou relógio de autorização do cliente.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "WindowWire", into = "WindowWire")]
pub struct ReviewWindow(WindowWire);
impl TryFrom<WindowWire> for ReviewWindow {
    type Error = Error;
    fn try_from(value: WindowWire) -> Result<Self, Error> {
        if u64::from(value.expires_at).checked_sub(u64::from(value.issued_at)) != Some(120_000) {
            return Err(Error::InvalidWindow);
        }
        Ok(Self(value))
    }
}
impl From<ReviewWindow> for WindowWire {
    fn from(value: ReviewWindow) -> Self {
        value.0
    }
}
impl ReviewWindow {
    pub fn at(issued_at: Revision) -> Result<Self, Error> {
        let expires_at = u64::from(issued_at)
            .checked_add(120_000)
            .and_then(|v| Revision::try_from(v).ok())
            .ok_or(Error::InvalidWindow)?;
        Ok(Self(WindowWire { issued_at, expires_at }))
    }
    pub fn check_at(&self, now: Revision) -> Result<(), Error> {
        if u64::from(now) < u64::from(self.0.issued_at) {
            return Err(Error::NotYetIssued);
        }
        if u64::from(now) >= u64::from(self.0.expires_at) {
            return Err(Error::Expired);
        }
        Ok(())
    }
}
/// Projeção NÃO confiável. Match bem-sucedido não emite grant ou autoriza efeito.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewDescriptor {
    pub version: Version,
    pub review_id: Id,
    pub prepare_operation: OperationSelector,
    pub prepare_nonce: ClientNonce,
    pub decision_operation: OperationSelector,
    pub decision_nonce: ClientNonce,
    pub binding: ReviewBinding,
    pub semantic_digest: SemanticDigest,
    pub window: ReviewWindow,
}
impl ReviewDescriptor {
    fn check_contract(&self) -> Result<(), Error> {
        if self.prepare_nonce == self.decision_nonce
            || self.prepare_operation.kind != OperationKind::PrepareReview
            || self.decision_operation.kind != OperationKind::DecideReview
            || self.prepare_operation.operation_id == self.decision_operation.operation_id
        {
            return Err(Error::OperationMismatch);
        }
        if self.semantic_digest != self.binding.digest()? {
            return Err(Error::DigestMismatch);
        }
        Ok(())
    }
    pub fn check_prepare(&self, input: &PrepareReviewInput) -> Result<(), Error> {
        self.check_contract()?;
        if self.prepare_nonce != input.0.prepare_nonce || self.decision_nonce != input.0.decision_nonce {
            return Err(Error::ReviewMismatch);
        }
        if self.binding != input.0.binding {
            return Err(Error::BindingMismatch);
        }
        Ok(())
    }
    pub fn check_decision(&self, input: &DecideReviewInput, now: Revision) -> Result<(), Error> {
        self.check_contract()?;
        if input.decision_operation != self.decision_operation {
            return Err(Error::OperationMismatch);
        }
        if input.review_id != self.review_id || input.decision_nonce != self.decision_nonce {
            return Err(Error::ReviewMismatch);
        }
        if input.binding != self.binding {
            return Err(Error::BindingMismatch);
        }
        self.window.check_at(now)
    }
}
/// Dependência única13/22, SEM emissor/construtor/Deserialize nesta fonte.
/// Não vem de `Caller::Local`, bearer ou flag. Mantém a captura confiável do gesto.
pub(super) struct HumanReviewProof<'a> {
    scope: &'a ResolvedScope,
    binding: ReviewBinding,
    issued_at: Revision,
}
#[derive(Serialize)]
#[serde(transparent)]
pub struct ReviewIntent(ReviewDescriptor);
impl ReviewIntent {
    /// Draft condicionado à prova ausente. M1 ainda deve persistir antes do aceite.
    pub(super) fn issue_draft(proof: &HumanReviewProof<'_>, input: &PrepareReviewInput) -> Result<Self, Error> {
        if proof.binding != input.0.binding {
            return Err(Error::BindingMismatch);
        }
        let prepare = OperationRef::issue(proof.scope, OperationKind::PrepareReview)?;
        let decision = OperationRef::issue(proof.scope, OperationKind::DecideReview)?;
        let review_id = Id::try_from(uuid::Uuid::new_v4().to_string()).map_err(|_| Error::InvalidContract)?;
        Ok(Self(ReviewDescriptor {
            version: Version,
            review_id,
            prepare_operation: prepare.descriptor(),
            prepare_nonce: input.0.prepare_nonce.clone(),
            decision_operation: decision.descriptor(),
            decision_nonce: input.0.decision_nonce.clone(),
            binding: proof.binding.clone(),
            semantic_digest: proof.binding.digest()?,
            window: ReviewWindow::at(proof.issued_at)?,
        }))
    }
    pub(super) fn check_decision(&self, input: &DecideReviewInput, now: Revision) -> Result<(), Error> {
        self.0.check_decision(input, now)
    }
}
