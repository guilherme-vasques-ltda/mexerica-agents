//! D430: origem durável e, no PR-A, procedimento inicial SSH ancorado da instalação isolada.
//! Não emite sessão, convite, direito de bot nem Authority genérica.
pub(crate) mod canon;
pub(crate) mod channel;
mod channel_crypto;
pub(crate) mod channel_root;
mod channel_socket;
pub(crate) mod installation;
pub(crate) mod procedure;
pub(crate) mod time;
pub(crate) mod wire;
pub(crate) mod writer;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Os bytes exatos (incluindo LF final) vinculam esquema e tabela fechada.
const POLICY: &[u8] = include_bytes!("policy-v1.json");

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyBinding {
    policy_id: String,
    policy_version: u32,
    policy_digest: String,
    rights: Vec<String>,
    delegation: bool,
    rights_revision: u64,
}

impl PolicyBinding {
    fn check(&self) -> Result<()> {
        ensure!(
            self.policy_id == "donna.personal-link/core"
                && self.policy_version == 1
                && self.policy_digest == crate::fork::bootstrap::codec::hash_bytes(POLICY),
            "not_allowed: política D430 desconhecida"
        );
        let rights: Vec<&str> = self.rights.iter().map(String::as_str).collect();
        ensure!(
            matches!(
                rights.as_slice(),
                ["link.connect"]
                    | ["devices.manage", "link.connect"]
                    | ["devices.delegate", "devices.manage", "link.connect"]
            ) && self.delegation == rights.contains(&"devices.delegate")
                && (1..=9_007_199_254_740_991).contains(&self.rights_revision),
            "not_allowed: direitos D430 divergentes"
        );
        Ok(())
    }
}

/// Envelope LOCAL de origem, não wire de admissão. Referências não provam assinatura.
/// Não aceitar de renderer/header/cloud; só a futura admissão autenticada poderá gravar.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Origin {
    schema: String,
    hub: String,
    context: String,
    generation: String,
    key: String,
    admission_digest: String,
    approver: String,
    consent_digest: String,
    policy: PolicyBinding,
}

pub(crate) fn policy_digest() -> String {
    crate::fork::bootstrap::codec::hash_bytes(POLICY)
}

/// Origem connect-only imutável gravada pelo escritor da raiz (P0, revisão 1).
pub(crate) fn origem_raiz(
    hub: &str,
    contexto: &str,
    geracao: &str,
    chave: &str,
    admissao: &str,
    root_id: &str,
    consentimento: &str,
) -> Result<Vec<u8>> {
    let origem = Origin {
        schema: "donna.personal-link/origin/v1".into(),
        hub: hub.into(),
        context: contexto.into(),
        generation: geracao.into(),
        key: chave.into(),
        admission_digest: admissao.into(),
        approver: format!("administrative:{root_id}"),
        consent_digest: consentimento.into(),
        policy: PolicyBinding {
            policy_id: "donna.personal-link/core".into(),
            policy_version: 1,
            policy_digest: policy_digest(),
            rights: vec!["link.connect".into()],
            delegation: false,
            rights_revision: 1,
        },
    };
    let bytes = serde_json::to_vec(&origem)?;
    ensure!(generation(&bytes, chave)? == geracao, "not_allowed: origem D430 divergente");
    Ok(bytes)
}

/// Leitura usada pelo predicado central, na mesma Connection da autoridade existente.
/// Mesmo um registro corrompido é recusa: erro nunca significa Device legado.
pub(crate) fn generation(raw: &[u8], key: &str) -> Result<String> {
    ensure!(raw.len() <= 16 * 1024, "not_allowed: origem D430 inválida");
    let origin: Origin = serde_json::from_slice(raw)?;
    ensure!(serde_json::to_vec(&origin)? == raw, "not_allowed: origem D430 não canônica");
    ensure!(
        origin.schema == "donna.personal-link/origin/v1" && origin.key == key,
        "not_allowed: origem D430 divergente"
    );
    for value in [&origin.hub, &origin.context, &origin.generation, &origin.key, &origin.approver] {
        ensure!(!value.is_empty() && value.len() <= 1024, "not_allowed: contexto D430 ausente");
    }
    for digest in [&origin.admission_digest, &origin.consent_digest] {
        ensure!(
            digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "not_allowed: consentimento D430 inválido"
        );
    }
    origin.policy.check()?;
    Ok(origin.generation)
}

#[cfg(all(test, target_os = "linux"))]
mod initial_tests;
#[cfg(test)]
mod tests;

#[cfg(all(test, target_os = "linux"))]
mod test_sync;
