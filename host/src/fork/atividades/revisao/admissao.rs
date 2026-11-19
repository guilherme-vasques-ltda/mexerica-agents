//! Segurança05b: barreiras negativas comuns, sem produtor positivo ou I/O.
//!
//! Nenhum relatório recebido é evidência confiável. O sucesso inabitável impede
//! usar matching de descritores como autorização de callback de cofre/efeito.
use super::{DecideReviewInput, PrepareReviewInput, ReviewDescriptor};
use crate::fork::{atividades::erro_operacao::OperationError, escopo::Revision};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;

/// Requisitos a provar pelos produtores reais, não atestados deste módulo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IsolationSurface {
    AppInstanceAndPackage,
    PrivateChannel,
    HubInstance,
    ExecutorInstance,
    PriorShellAndProcesses,
    VaultAndKeychain,
    HubMemory,
    KeyboardAndAccessibility,
    CaptureAndMirror,
    DomAndDevtools,
}

pub const REQUIRED_ISOLATION_SURFACES: [IsolationSurface; 10] = [
    IsolationSurface::AppInstanceAndPackage,
    IsolationSurface::PrivateChannel,
    IsolationSurface::HubInstance,
    IsolationSurface::ExecutorInstance,
    IsolationSurface::PriorShellAndProcesses,
    IsolationSurface::VaultAndKeychain,
    IsolationSurface::HubMemory,
    IsolationSurface::KeyboardAndAccessibility,
    IsolationSurface::CaptureAndMirror,
    IsolationSurface::DomAndDevtools,
];

/// Somente impedimentos relatados. Não existe variante positiva importável.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IsolationObstacle {
    Missing,
    Unknown,
    Revoked,
    Changed,
    Debuggable,
    Observable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IsolationObservation {
    pub surface: IsolationSurface,
    pub obstacle: IsolationObstacle,
}

/// Dados não confiáveis, sem IDs, valores, texto livre ou autoridade capturada.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "ReportWire")]
pub struct IsolationReport(Vec<IsolationObservation>);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReportWire {
    observations: Vec<IsolationObservation>,
}

impl TryFrom<ReportWire> for IsolationReport {
    type Error = &'static str;

    fn try_from(value: ReportWire) -> Result<Self, Self::Error> {
        if value.observations.len() > REQUIRED_ISOLATION_SURFACES.len()
            || value.observations.iter().enumerate().any(|(index, observation)| {
                value.observations[..index].iter().any(|previous| previous.surface == observation.surface)
            })
        {
            return Err("invalid_isolation_report");
        }
        Ok(Self(value.observations))
    }
}

impl IsolationReport {
    /// Parser limitado, sem eco de bytes ou diagnóstico do desserializador.
    pub fn parse_json(bytes: &[u8]) -> Result<Self, DenialReason> {
        if bytes.len() > 4096 {
            return Err(DenialReason::InvalidIsolationReport);
        }
        serde_json::from_slice(bytes).map_err(|_| DenialReason::InvalidIsolationReport)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionStage {
    Prepare,
    Decide,
    Consume,
}

/// Projeção pública contém apenas códigos fechados, nunca o payload de revisão.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DenialReason {
    InvalidIsolationReport,
    Contract { error: OperationError },
    Isolation { surface: IsolationSurface, obstacle: IsolationObstacle },
    TrustedDependenciesUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdmissionDenied {
    pub stage: AdmissionStage,
    pub reason: DenialReason,
}

impl std::fmt::Display for AdmissionDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Revisão humana protegida indisponível.")
    }
}
impl std::error::Error for AdmissionDenied {}

/// Não há token positivo, callback, construtor de Proof nem opção de ativação.
pub type UnavailableAdmission = Result<Infallible, AdmissionDenied>;

fn deny(stage: AdmissionStage, report: Option<&IsolationReport>) -> UnavailableAdmission {
    let reason = match report.and_then(|report| report.0.first()) {
        Some(observation) => DenialReason::Isolation { surface: observation.surface, obstacle: observation.obstacle },
        None => DenialReason::TrustedDependenciesUnavailable,
    };
    Err(AdmissionDenied { stage, reason })
}

/// Preparação descritiva não é gesto. Nada é persistido ou emitido aqui.
pub fn prepare(_input: &PrepareReviewInput, report: Option<&IsolationReport>) -> UnavailableAdmission {
    deny(AdmissionStage::Prepare, report)
}

fn recheck(
    stage: AdmissionStage,
    descriptor: &ReviewDescriptor,
    input: &DecideReviewInput,
    now: Revision,
    report: Option<&IsolationReport>,
) -> UnavailableAdmission {
    // Mesmo um match completo permanece descritivo. O relógio recebido também
    // não é promovido a relógio confiável para emissão/consumo.
    descriptor
        .check_decision(input, now)
        .map_err(|error| AdmissionDenied { stage, reason: DenialReason::Contract { error } })?;
    deny(stage, report)
}

/// Revalidação descritiva na decisão; não aprova, não consome, não renova.
pub fn decide(
    descriptor: &ReviewDescriptor,
    input: &DecideReviewInput,
    now: Revision,
    report: Option<&IsolationReport>,
) -> UnavailableAdmission {
    recheck(AdmissionStage::Decide, descriptor, input, now, report)
}

/// Última barreira antes do efeito; sem produtor/consumo atômico, sempre fecha.
/// Não recebe Connection, Authority substituta, closure, valor ou acesso ao cofre.
pub fn consume(
    descriptor: &ReviewDescriptor,
    input: &DecideReviewInput,
    now: Revision,
    report: Option<&IsolationReport>,
) -> UnavailableAdmission {
    recheck(AdmissionStage::Consume, descriptor, input, now, report)
}
