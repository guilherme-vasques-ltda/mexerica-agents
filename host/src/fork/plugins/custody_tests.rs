//! Fixtures sintéticas de metadados; nenhuma implementa um provedor de custódia.
use super::*;

/// Compara metadados sem formatar observações nem IDs em caso de falha.
#[track_caller]
fn assert_metadata_comparison(matches: bool) {
    assert!(matches, "Comparação de metadados da fixture falhou");
}

fn unknown() -> NegativeCustody {
    NegativeCustody::new(BackendId(11), BootId(21))
}

fn current(machine: &NegativeCustody) -> ObservationRef {
    machine.observation_ref().expect("referência negativa não esgotada")
}

#[test]
fn unknown_cache_and_point_access_never_authorize() {
    for signal in [Restriction::CachedMaterialOnly, Restriction::PointAccessOnly] {
        let mut machine = unknown();
        let original = current(&machine);
        assert_eq!(machine.state(), CustodyState::Unknown);
        assert_eq!(machine.revalidate(&original), CustodyError::Unknown);

        assert_eq!(machine.restrict(&original, signal), Ok(()));
        assert_eq!(machine.state(), CustodyState::Unavailable);
        assert_eq!(machine.revalidate(&original), CustodyError::StaleReference);
        assert_eq!(machine.revalidate(&current(&machine)), CustodyError::NoContinuousProof);
    }
}

#[test]
fn cached_material_then_lock_cannot_be_reopened_by_point_access() {
    let mut machine = unknown();
    machine.restrict(&current(&machine), Restriction::CachedMaterialOnly).expect("sinal");
    let before_lock = current(&machine);
    machine.restrict(&before_lock, Restriction::Locked).expect("lock");
    assert_eq!(machine.revalidate(&before_lock), CustodyError::StaleReference);
    assert_eq!(machine.revalidate(&current(&machine)), CustodyError::Locked);

    for signal in [Restriction::PointAccessOnly, Restriction::CachedMaterialOnly] {
        machine.restrict(&current(&machine), signal).expect("sinal sem segredo");
        assert_eq!(machine.state(), CustodyState::Locked);
        assert_eq!(machine.revalidate(&current(&machine)), CustodyError::Locked);
    }
}

#[test]
fn old_callback_cannot_mutate_or_refresh_the_original_reference() {
    let mut machine = unknown();
    let original = current(&machine);
    machine.restrict(&original, Restriction::Locked).expect("lock");
    let after_lock = current(&machine);

    assert_eq!(machine.restrict(&original, Restriction::PointAccessOnly), Err(CustodyError::StaleReference));
    assert_eq!(machine.replace_backend(&original, BackendId(12)), Err(CustodyError::StaleReference));
    assert_eq!(machine.restart(&original, BootId(22)), Err(CustodyError::StaleReference));
    assert_metadata_comparison(current(&machine) == after_lock);
    assert_eq!(machine.revalidate(&original), CustodyError::StaleReference);
    assert_eq!(machine.revalidate(&after_lock), CustodyError::Locked);
}

#[test]
fn backend_identity_and_replacement_reject_original_callbacks() {
    let mut machine = unknown();
    let original = current(&machine);
    // Mesma geração/boot, outro backend: igualdade numérica da geração não basta.
    let other_backend = NegativeCustody::new(BackendId(12), BootId(21));
    assert_eq!(other_backend.revalidate(&original), CustodyError::StaleReference);

    machine.replace_backend(&original, BackendId(12)).expect("troca negativa");
    let replaced = current(&machine);
    assert_metadata_comparison(replaced.backend == BackendId(12));
    assert_eq!(machine.revalidate(&original), CustodyError::StaleReference);
    assert_eq!(machine.restrict(&original, Restriction::Locked), Err(CustodyError::StaleReference));
    assert_metadata_comparison(current(&machine) == replaced);
    assert_eq!(machine.revalidate(&replaced), CustodyError::BackendChanged);
}

#[test]
fn new_boot_and_restart_reject_original_callbacks() {
    let mut machine = unknown();
    let original = current(&machine);
    // Outro boot recusa a referência mesmo com backend e geração iguais.
    let next_boot = NegativeCustody::new(BackendId(11), BootId(22));
    assert_eq!(next_boot.revalidate(&original), CustodyError::StaleReference);

    machine.restart(&original, BootId(22)).expect("restart negativo");
    let restarted = current(&machine);
    assert_metadata_comparison(restarted.boot == BootId(22));
    assert_eq!(machine.revalidate(&original), CustodyError::StaleReference);
    assert_eq!(machine.restrict(&original, Restriction::PointAccessOnly), Err(CustodyError::StaleReference));
    assert_metadata_comparison(current(&machine) == restarted);
    assert_eq!(machine.revalidate(&restarted), CustodyError::Restarted);
}

#[test]
fn loss_and_credential_change_invalidate_even_after_point_access() {
    for (restriction, denial) in [
        (Restriction::WatcherLost, CustodyError::WatcherLost),
        (Restriction::BackendLost, CustodyError::BackendLost),
        (Restriction::CredentialChanged, CustodyError::CredentialChanged),
    ] {
        let mut machine = unknown();
        machine.restrict(&current(&machine), Restriction::PointAccessOnly).expect("sinal pontual");
        let before_loss = current(&machine);
        machine.restrict(&before_loss, restriction).expect("invalidação");
        assert_eq!(machine.state(), CustodyState::Unavailable);
        assert_eq!(machine.revalidate(&before_loss), CustodyError::StaleReference);
        assert_eq!(machine.restrict(&before_loss, Restriction::PointAccessOnly), Err(CustodyError::StaleReference));
        machine.restrict(&current(&machine), Restriction::PointAccessOnly).expect("sinal insuficiente");
        assert_eq!(machine.revalidate(&current(&machine)), denial);
    }
}

#[test]
fn exhausted_generation_never_wraps_or_reopens() {
    let mut machine = unknown();
    // Limite sintético privado, sem setter produtivo.
    machine.generation = Generation::Current(u64::MAX - 1);
    let before_last = current(&machine);
    machine.restrict(&before_last, Restriction::Locked).expect("última geração");
    let last = current(&machine);
    assert_eq!(last.generation, u64::MAX);
    assert_eq!(machine.revalidate(&before_last), CustodyError::StaleReference);
    assert_eq!(machine.restrict(&before_last, Restriction::WatcherLost), Err(CustodyError::StaleReference));

    assert_eq!(machine.restrict(&last, Restriction::Locked), Err(CustodyError::GenerationExhausted));
    assert!(matches!(machine.generation, Generation::Exhausted));
    assert!(matches!(machine.observation_ref(), Err(CustodyError::GenerationExhausted)));
    assert_eq!(machine.state(), CustodyState::Unavailable);
    assert_eq!(machine.revalidate(&last), CustodyError::GenerationExhausted);
    assert_eq!(machine.replace_backend(&last, BackendId(12)), Err(CustodyError::GenerationExhausted));
    assert_eq!(machine.restart(&last, BootId(22)), Err(CustodyError::GenerationExhausted));
    assert!(matches!(machine.generation, Generation::Exhausted));
}

#[test]
fn errors_have_only_static_codes_without_backend_input() {
    // Metadados bem diferentes produzem as mesmas recusas sem ecoar os IDs.
    // A API nem aceita texto/bytes de erro ou segredo.
    for (backend, boot) in [(11, 21), (u128::MAX, u128::MAX)] {
        let mut machine = NegativeCustody::new(BackendId(backend), BootId(boot));
        let original = current(&machine);
        machine.restrict(&original, Restriction::PointAccessOnly).expect("sinal");
        let stale = machine.revalidate(&original);
        let static_code: &'static str = stale.code();
        assert_eq!(static_code, "custody_stale_reference");
        assert_eq!(format!("{stale:?}"), "StaleReference");
        let denied = machine.revalidate(&current(&machine));
        assert_eq!(denied.code(), "custody_no_continuous_proof");
        assert_eq!(format!("{denied:?}"), "NoContinuousProof");
    }
}
