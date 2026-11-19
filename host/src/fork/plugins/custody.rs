//! Mexerica: máquina pura de custódia negativa, sem emissor de uso.
//! Referências são metadados de observação, nunca leases ou provas do SO.

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct BackendId(u128);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct BootId(u128);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CustodyState {
    Unknown,
    Locked,
    Unavailable,
}

/// Sem payload: nenhum texto de entrada, identidade ou erro de backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CustodyError {
    Unknown,
    Locked,
    NoContinuousProof,
    WatcherLost,
    BackendLost,
    CredentialChanged,
    BackendChanged,
    Restarted,
    StaleReference,
    GenerationExhausted,
}

impl CustodyError {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::Unknown => "custody_unknown",
            Self::Locked => "custody_locked",
            Self::NoContinuousProof => "custody_no_continuous_proof",
            Self::WatcherLost => "custody_watcher_lost",
            Self::BackendLost => "custody_backend_lost",
            Self::CredentialChanged => "custody_credential_changed",
            Self::BackendChanged => "custody_backend_changed",
            Self::Restarted => "custody_restarted",
            Self::StaleReference => "custody_stale_reference",
            Self::GenerationExhausted => "custody_generation_exhausted",
        }
    }
}

/// Sinais internos abstratos; não recebem material secreto nem provam um backend.
#[derive(Clone, Copy)]
pub(super) enum Restriction {
    Locked,
    WatcherLost,
    BackendLost,
    CredentialChanged,
    CachedMaterialOnly,
    PointAccessOnly,
}

/// Vincula o callback original. Sem serde, `Debug`, validade temporal ou poder de uso.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct ObservationRef {
    backend: BackendId,
    boot: BootId,
    generation: u64,
}

enum Generation {
    Current(u64),
    Exhausted,
}

/// Não contém chave, cache ou lease e não acessa o SO.
pub(super) struct NegativeCustody {
    backend: BackendId,
    boot: BootId,
    generation: Generation,
    denial: CustodyError,
}

impl NegativeCustody {
    /// Inicialização negativa. Os IDs não são autenticados por esta máquina.
    /// Não há produtor de identidade de backend/boot nesta máquina isolada.
    pub(super) fn new(backend: BackendId, boot: BootId) -> Self {
        Self { backend, boot, generation: Generation::Current(0), denial: CustodyError::Unknown }
    }

    pub(super) fn state(&self) -> CustodyState {
        match self.denial {
            CustodyError::Unknown => CustodyState::Unknown,
            CustodyError::Locked => CustodyState::Locked,
            _ => CustodyState::Unavailable,
        }
    }

    /// Só identifica um futuro callback negativo; não autoriza acesso.
    pub(super) fn observation_ref(&self) -> Result<ObservationRef, CustodyError> {
        match self.generation {
            Generation::Current(generation) => {
                Ok(ObservationRef { backend: self.backend, boot: self.boot, generation })
            }
            Generation::Exhausted => Err(CustodyError::GenerationExhausted),
        }
    }

    /// A saída é sempre uma recusa, sem reemitir ou substituir a referência.
    pub(super) fn revalidate(&self, original: &ObservationRef) -> CustodyError {
        if let Err(error) = self.check_original(original) {
            return error;
        }
        self.denial
    }

    /// `Ok` significa somente que a restrição foi registrada, nunca admissão.
    pub(super) fn restrict(&mut self, original: &ObservationRef, restriction: Restriction) -> Result<(), CustodyError> {
        self.check_original(original)?;
        self.advance()?;
        self.denial =
            match restriction {
                Restriction::Locked => CustodyError::Locked,
                Restriction::WatcherLost => CustodyError::WatcherLost,
                Restriction::BackendLost => CustodyError::BackendLost,
                Restriction::CredentialChanged => CustodyError::CredentialChanged,
                Restriction::CachedMaterialOnly | Restriction::PointAccessOnly => {
                    if self.denial == CustodyError::Unknown { CustodyError::NoContinuousProof } else { self.denial }
                }
            };
        Ok(())
    }

    pub(super) fn replace_backend(
        &mut self,
        original: &ObservationRef,
        backend: BackendId,
    ) -> Result<(), CustodyError> {
        self.check_original(original)?;
        self.advance()?;
        self.backend = backend;
        self.denial = CustodyError::BackendChanged;
        Ok(())
    }

    pub(super) fn restart(&mut self, original: &ObservationRef, boot: BootId) -> Result<(), CustodyError> {
        self.check_original(original)?;
        self.advance()?;
        self.boot = boot;
        self.denial = CustodyError::Restarted;
        Ok(())
    }

    /// Igualdade local de metadados, não autoridade nem CAS transacional.
    fn check_original(&self, original: &ObservationRef) -> Result<(), CustodyError> {
        let current = self.observation_ref()?;
        if *original != current {
            return Err(CustodyError::StaleReference);
        }
        Ok(())
    }

    fn advance(&mut self) -> Result<(), CustodyError> {
        if let Generation::Current(current) = self.generation
            && let Some(next) = current.checked_add(1)
        {
            self.generation = Generation::Current(next);
            return Ok(());
        }
        self.generation = Generation::Exhausted;
        self.denial = CustodyError::GenerationExhausted;
        Err(CustodyError::GenerationExhausted)
    }
}

#[cfg(test)]
#[path = "custody_tests.rs"]
mod tests;
