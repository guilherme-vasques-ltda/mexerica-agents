//! Exercita a barreira real; as fixtures do pai são somente projeções forjadas.
use super::{binding, decision, descriptor, digest, id, time};
use crate::fork::atividades::{
    erro_operacao::OperationError,
    revisao::{
        DecideReviewInput, PrepareReviewInput,
        admissao::{self, AdmissionDenied, AdmissionStage, DenialReason, IsolationReport},
    },
};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn prepare_json() -> Value {
    json!({"version":1,"prepareNonce":id(24),"decisionNonce":id(23),"binding":binding()})
}

fn report(value: &Value) -> Result<IsolationReport, String> {
    IsolationReport::parse_json(&serde_json::to_vec(value).map_err(|_| "fixture inválida")?)
        .map_err(|_| "relatório inválido".into())
}

#[test]
fn matching_completo_e_replay_nao_abrem_preparacao_decisao_ou_consumo() -> TestResult {
    let description = descriptor();
    let input: DecideReviewInput = serde_json::from_value(decision())?;
    let prepare = PrepareReviewInput::parse_json(&serde_json::to_vec(&prepare_json())?)?;
    assert_eq!(description.check_decision(&input, time(1000)), Ok(()));
    assert_eq!(description.check_prepare(&prepare), Ok(()));
    let empty = report(&json!({"observations":[]}))?;
    let mut vault_or_effect_calls = 0;
    for supplied in [None, Some(&empty)] {
        for _ in 0..2 {
            for (stage, result) in [
                (AdmissionStage::Prepare, admissao::prepare(&prepare, supplied)),
                (AdmissionStage::Decide, admissao::decide(&description, &input, time(1000), supplied)),
                (AdmissionStage::Consume, admissao::consume(&description, &input, time(1000), supplied)),
            ] {
                let safe_projection = result.map(|_| vault_or_effect_calls += 1);
                assert_eq!(
                    safe_projection,
                    Err(AdmissionDenied { stage, reason: DenialReason::TrustedDependenciesUnavailable })
                );
            }
        }
    }
    assert_eq!(vault_or_effect_calls, 0);
    Ok(())
}

#[test]
fn binding_escopo_revisoes_nonce_e_prazo_sao_rechecados_antes_do_efeito() -> TestResult {
    let description = descriptor();
    let mut vault_or_effect_calls = 0;
    for (pointer, value, error) in [
        ("/binding/contextRef/domainRevision", json!(2), OperationError::BindingMismatch),
        ("/binding/contextRef/spaceId", json!(id(99)), OperationError::BindingMismatch),
        ("/binding/target/revision", json!(2), OperationError::BindingMismatch),
        ("/binding/callerDevice/principalEpoch", json!(2), OperationError::BindingMismatch),
        ("/binding/policyRevision", json!(2), OperationError::BindingMismatch),
        ("/binding/recoveryEpoch", json!(id(99)), OperationError::BindingMismatch),
        ("/binding/callerDevice/sessionId", json!(id(99)), OperationError::BindingMismatch),
        ("/binding/effect/destination/objectId", json!(id(99)), OperationError::BindingMismatch),
        ("/decisionNonce", json!(id(99)), OperationError::ReviewMismatch),
    ] {
        let mut changed = decision();
        let Some(field) = changed.pointer_mut(pointer) else { return Err("fixture sem campo".into()) };
        *field = value;
        // Contexto/target devem concordar na forma; ambos continuam dados forjados.
        if pointer.starts_with("/binding/contextRef/") {
            changed["binding"]["target"]["contextRef"] = changed["binding"]["contextRef"].clone();
        }
        let input: DecideReviewInput = serde_json::from_value(changed)?;
        for result in [
            admissao::decide(&description, &input, time(1000), None),
            admissao::consume(&description, &input, time(1000), None),
        ] {
            let failure = result.map(|_| vault_or_effect_calls += 1).err().ok_or("barreira abriu")?;
            assert_eq!(failure.reason, DenialReason::Contract { error });
        }
    }
    let input: DecideReviewInput = serde_json::from_value(decision())?;
    for (now, error) in [(999, OperationError::NotYetIssued), (121_000, OperationError::Expired)] {
        let failure = admissao::consume(&description, &input, time(now), None)
            .map(|_| vault_or_effect_calls += 1)
            .err()
            .ok_or("prazo abriu")?;
        assert_eq!(failure.reason, DenialReason::Contract { error });
    }
    assert_eq!(vault_or_effect_calls, 0);
    Ok(())
}

#[test]
fn cada_superficie_e_impedimento_fecha_sem_aceitar_atestado_autodeclarado() -> TestResult {
    let description = descriptor();
    let input: DecideReviewInput = serde_json::from_value(decision())?;
    let mut vault_or_effect_calls = 0;
    // Todas as superfícies são exigidas; o relatório pode somente piorar o diagnóstico.
    let obstacles = ["missing", "unknown", "revoked", "changed", "debuggable", "observable"];
    for (index, surface) in admissao::REQUIRED_ISOLATION_SURFACES.into_iter().enumerate() {
        let obstacle = obstacles[index % obstacles.len()];
        let supplied = report(&json!({"observations":[{"surface":surface,"obstacle":obstacle}]}))?;
        for result in [
            admissao::decide(&description, &input, time(1000), Some(&supplied)),
            admissao::consume(&description, &input, time(1000), Some(&supplied)),
        ] {
            let failure = result.map(|_| vault_or_effect_calls += 1).err().ok_or("isolamento abriu")?;
            assert!(matches!(failure.reason, DenialReason::Isolation { surface: found, .. } if found == surface));
        }
    }
    // Projeção completa não é evidência de instâncias/revisões/executor reais.
    let complete = report(&json!({"observations":admissao::REQUIRED_ISOLATION_SURFACES.map(|surface|
        json!({"surface":surface,"obstacle":"unknown"}))}))?;
    assert!(admissao::consume(&description, &input, time(1000), Some(&complete)).is_err());
    assert_eq!(vault_or_effect_calls, 0);
    Ok(())
}

#[test]
fn json_origem_e_flags_nao_fabricam_proof_e_recusa_nao_projeta_payload() -> TestResult {
    for origin in ["Local", "bearer", "Control", "app", "GenUI", "clique", "voz", "sessao"] {
        let mut claimed = prepare_json();
        claimed["source"] = json!(origin);
        assert!(PrepareReviewInput::parse_json(&serde_json::to_vec(&claimed)?).is_err());
    }
    for claimed in [
        json!({"observations":[],"human":true}),
        json!({"observations":[],"proof":descriptor()}),
        json!({"observations":[],"isolated":true}),
        json!({"observations":[],"executorInstance":"ficticio"}),
        json!({"observations":[{"surface":"hub_memory","obstacle":"verified"}]}),
        json!({"observations":[{"surface":"hub_memory","obstacle":"unknown","source":"app"}]}),
        json!({"observations":vec![json!({"surface":"hub_memory","obstacle":"unknown"}); 2]}),
    ] {
        assert!(IsolationReport::parse_json(&serde_json::to_vec(&claimed)?).is_err());
    }
    assert!(IsolationReport::parse_json(&[b' '; 4097]).is_err());
    let malformed = IsolationReport::parse_json(b"sentinela-ficticia-invalida").err().ok_or("JSON aceito")?;
    assert_eq!(malformed, DenialReason::InvalidIsolationReport);
    let mut raw = decision();
    let sentinel = format!("valor-ficticio-{}", id(88));
    raw["binding"]["effect"]["body"] = json!(sentinel);
    let mut forged = serde_json::to_value(descriptor())?;
    forged["binding"] = raw["binding"].clone();
    forged["semanticDigest"] = digest(raw["binding"].clone());
    let matching = serde_json::from_value(forged)?;
    let input: DecideReviewInput = serde_json::from_value(raw)?;
    let failure = admissao::consume(&matching, &input, time(1000), None).err().ok_or("efeito abriu")?;
    assert_eq!(failure.reason, DenialReason::TrustedDependenciesUnavailable);
    let outputs = [format!("{input:?}"), format!("{failure:?}"), failure.to_string(), serde_json::to_string(&failure)?];
    for output in outputs {
        assert!(!output.contains(&sentinel));
        assert!(!output.contains(&sentinel.len().to_string()));
    }
    Ok(())
}
