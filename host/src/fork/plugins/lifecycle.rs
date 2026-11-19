//! Cálculo puro: não consulta Authority, não persiste e não publica nada.
use super::model::{InstallationState, PluginError, PluginInstallationRef};
use crate::fork::escopo::Revision;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LifecycleChange {
    /// Inclui reativação; não restaura concessões, cartões ou filas anteriores.
    Enable,
    Disable,
}

/// Compara o snapshot inteiro, inclusive Hub/instalação/geração/revisão/estado.
/// O consumidor futuro precisa obter o snapshot confiável e comparar/gravar
/// atomicamente sob suas guardas de autoridade; esta função não faz esse commit.
/// Repetir o estado desejado é erro, sem recibo de sucesso ou contador reciclado.
pub(crate) fn transition_installation(
    snapshot: &PluginInstallationRef,
    expected: &PluginInstallationRef,
    change: LifecycleChange,
) -> Result<PluginInstallationRef, PluginError> {
    if snapshot != expected {
        return Err(PluginError::SnapshotConflict);
    }
    let next_state = match (snapshot.state, change) {
        (InstallationState::Disabled, LifecycleChange::Enable) => InstallationState::Enabled,
        (InstallationState::Enabled, LifecycleChange::Disable) => InstallationState::Disabled,
        _ => return Err(PluginError::InvalidTransition),
    };
    // Calcula ambos antes de retornar; overflow não produz mudança parcial.
    let revision = increment(snapshot.revision)?;
    let generation = increment(snapshot.generation)?;
    Ok(PluginInstallationRef { revision, generation, state: next_state, ..snapshot.clone() })
}

fn increment(value: Revision) -> Result<Revision, PluginError> {
    u64::from(value).checked_add(1).and_then(|next| Revision::try_from(next).ok()).ok_or(PluginError::CounterExhausted)
}
