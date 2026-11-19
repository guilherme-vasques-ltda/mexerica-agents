//! F2a/D247: codec privado sem consumidor produtivo, custódia ou admissão.
//!
//! Metadados são descrições autenticadas, nunca handles de autoridade. A chave
//! de DADOS deve ser fornecida por domínio/conta próprio, distinto de vault e
//! webhook. Este módulo não busca, deriva, gera ou atesta a origem dessa chave.
//! Só fixtures sintéticas o consomem; a futura custódia continua não implementada.
//! Não registrar chave, plaintext, envelope ou metadados em logs/erros.
use crate::fork::escopo::{Id, Revision};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use zeroize::Zeroizing;

const PROTOCOL: &[u8] = b"donna/plugins/payload";
const MAGIC: &[u8; 4] = b"DNPL";
const ENVELOPE_VERSION: u8 = 1;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const MAX_PLAINTEXT_LEN: usize = 64 * 1024;
const MAX_CIPHERTEXT_LEN: usize = MAX_PLAINTEXT_LEN + TAG_LEN;
// magic(4), versão(1), nonce(24), tamanho ciphertext u32 big-endian(4).
const HEADER_LEN: usize = 4 + 1 + NONCE_LEN + 4;
const MAX_ENVELOPE_LEN: usize = HEADER_LEN + MAX_CIPHERTEXT_LEN;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CodecError {
    InvalidEnvelope,
    UnsupportedVersion,
    InvalidSize,
    InvalidMetadata,
    EntropyUnavailable,
    EncryptionFailed,
    AuthenticationFailed,
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidEnvelope => "plugin_payload_invalid_envelope",
            Self::UnsupportedVersion => "plugin_payload_unsupported_version",
            Self::InvalidSize => "plugin_payload_invalid_size",
            Self::InvalidMetadata => "plugin_payload_invalid_metadata",
            Self::EntropyUnavailable => "plugin_payload_entropy_unavailable",
            Self::EncryptionFailed => "plugin_payload_encryption_failed",
            Self::AuthenticationFailed => "plugin_payload_authentication_failed",
        })
    }
}

impl std::error::Error for CodecError {}

/// Vocabulário do codec, sem classificador, política de audiência ou projeção.
#[derive(Clone, Copy)]
pub(crate) enum PayloadClass {
    HumanRequest,
    ToolResult,
    ModelResponse,
    Summary,
    ReviewCard,
    Error,
    Artifact,
}

impl PayloadClass {
    fn tag(self) -> &'static [u8] {
        match self {
            Self::HumanRequest => b"human-request",
            Self::ToolResult => b"tool-result",
            Self::ModelResponse => b"model-response",
            Self::Summary => b"summary",
            Self::ReviewCard => b"review-card",
            Self::Error => b"error",
            Self::Artifact => b"artifact",
        }
    }
}

/// Identidades/revisões apenas; não herda estado enabled nem qualquer grant.
#[derive(Clone)]
pub(super) struct InstallationMetadata {
    pub(super) hub_id: Id,
    pub(super) installation_id: Id,
    pub(super) generation: Revision,
    pub(super) revision: Revision,
}

/// Domínio nativo não inventa conta externa. Tags distintas também entram na AAD.
#[derive(Clone)]
pub(super) enum PayloadDomain {
    Native { domain_id: Id, revision: Revision },
    ExternalAccount { account_id: Id, generation: Revision, revision: Revision },
}

/// O chamador fornece o binding esperado, separado dos bytes não confiáveis.
/// Validade de `Id`/`Revision` é só forma; não resolve fonte, posse ou permissão.
#[derive(Clone)]
pub(super) struct PayloadBinding {
    pub(super) installation: InstallationMetadata,
    pub(super) domain: PayloadDomain,
    pub(super) object_id: Id,
    pub(super) object_version: Revision,
    pub(super) class: PayloadClass,
}

/// Envelope imutável, sem plaintext, chave, nomes, paths ou descritor de poder.
/// A versão está no fio; os demais metadados ficam exclusivamente na AAD.
pub(super) struct Envelope {
    nonce: [u8; NONCE_LEN],
    ciphertext: Vec<u8>,
}

impl Envelope {
    /// Rejeita antes de alocar: sem fallback legado, campos livres ou sufixos.
    pub(super) fn parse(bytes: &[u8]) -> Result<Self, CodecError> {
        if !(HEADER_LEN + TAG_LEN + 1..=MAX_ENVELOPE_LEN).contains(&bytes.len()) {
            return Err(CodecError::InvalidSize);
        }
        if &bytes[..4] != MAGIC {
            return Err(CodecError::InvalidEnvelope);
        }
        if bytes[4] != ENVELOPE_VERSION {
            return Err(CodecError::UnsupportedVersion);
        }
        let nonce = bytes[5..5 + NONCE_LEN].try_into().map_err(|_| CodecError::InvalidEnvelope)?;
        let encoded_len = bytes[5 + NONCE_LEN..HEADER_LEN].try_into().map_err(|_| CodecError::InvalidEnvelope)?;
        let ciphertext_len = usize::try_from(u32::from_be_bytes(encoded_len)).map_err(|_| CodecError::InvalidSize)?;
        if ciphertext_len != bytes.len() - HEADER_LEN {
            return Err(CodecError::InvalidEnvelope);
        }
        Ok(Self { nonce, ciphertext: bytes[HEADER_LEN..].to_vec() })
    }

    /// Serialização binária em memória; nenhuma gravação é feita aqui.
    pub(super) fn to_bytes(&self) -> Result<Vec<u8>, CodecError> {
        self.check_size()?;
        let size = u32::try_from(self.ciphertext.len()).map_err(|_| CodecError::InvalidSize)?;
        let mut bytes = Vec::with_capacity(HEADER_LEN + self.ciphertext.len());
        bytes.extend_from_slice(MAGIC);
        bytes.push(ENVELOPE_VERSION);
        bytes.extend_from_slice(&self.nonce);
        bytes.extend_from_slice(&size.to_be_bytes());
        bytes.extend_from_slice(&self.ciphertext);
        Ok(bytes)
    }

    fn check_size(&self) -> Result<(), CodecError> {
        if !(TAG_LEN + 1..=MAX_CIPHERTEXT_LEN).contains(&self.ciphertext.len()) {
            return Err(CodecError::InvalidSize);
        }
        Ok(())
    }
}

// Prefixo de comprimento para TODOS os campos, inclusive protocolo, tags e
// inteiros big-endian. Ordem e tags são fixadas pelo protocolo 1; não há serde,
// concatenação de strings, campos opcionais ambíguos ou descritor emprestado.
fn field(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), CodecError> {
    let len = u16::try_from(bytes.len()).map_err(|_| CodecError::InvalidMetadata)?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

fn aad(binding: &PayloadBinding) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::new();
    field(&mut out, PROTOCOL)?;
    field(&mut out, &[ENVELOPE_VERSION])?;
    field(&mut out, binding.installation.hub_id.as_str().as_bytes())?;
    field(&mut out, binding.installation.installation_id.as_str().as_bytes())?;
    field(&mut out, &u64::from(binding.installation.generation).to_be_bytes())?;
    field(&mut out, &u64::from(binding.installation.revision).to_be_bytes())?;
    match &binding.domain {
        PayloadDomain::Native { domain_id, revision } => {
            field(&mut out, b"native-domain")?;
            field(&mut out, domain_id.as_str().as_bytes())?;
            field(&mut out, &u64::from(*revision).to_be_bytes())?;
        }
        PayloadDomain::ExternalAccount { account_id, generation, revision } => {
            field(&mut out, b"external-account")?;
            field(&mut out, account_id.as_str().as_bytes())?;
            field(&mut out, &u64::from(*generation).to_be_bytes())?;
            field(&mut out, &u64::from(*revision).to_be_bytes())?;
        }
    }
    field(&mut out, binding.object_id.as_str().as_bytes())?;
    field(&mut out, &u64::from(binding.object_version).to_be_bytes())?;
    field(&mut out, binding.class.tag())?;
    Ok(out)
}

// Injeção APENAS de falha; não permite escolher nonce nem existe em produção.
#[cfg(test)]
std::thread_local! {
    static FAIL_ENTROPY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Chave de dados explícita de 32 bytes, sem tipo de capacidade produtiva.
/// Não chamar com mestra, cache legado, chave de webhook ou dados reais.
pub(super) fn seal(
    data_key: &Zeroizing<[u8; 32]>,
    binding: &PayloadBinding,
    plaintext: &[u8],
) -> Result<Envelope, CodecError> {
    if !(1..=MAX_PLAINTEXT_LEN).contains(&plaintext.len()) {
        return Err(CodecError::InvalidSize);
    }
    let aad = aad(binding)?;
    let mut nonce = [0; NONCE_LEN];
    #[cfg(test)]
    if FAIL_ENTROPY.with(std::cell::Cell::get) {
        return Err(CodecError::EntropyUnavailable);
    }
    getrandom::fill(&mut nonce).map_err(|_| CodecError::EntropyUnavailable)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&data_key[..]).map_err(|_| CodecError::EncryptionFailed)?;
    let ciphertext = cipher
        .encrypt(&XNonce::from(nonce), Payload { msg: plaintext, aad: &aad })
        .map_err(|_| CodecError::EncryptionFailed)?;
    Ok(Envelope { nonce, ciphertext })
}

/// Só transforma bytes sob chave/binding explícitos; não é leitor ou resolvedor.
/// `Zeroizing` cobre o `Vec` retornado, sem prometer apagar cópias do processo,
/// temporários da biblioteca criptográfica, allocator, dumps ou harness/provedor.
pub(super) fn open(
    data_key: &Zeroizing<[u8; 32]>,
    binding: &PayloadBinding,
    envelope: &Envelope,
) -> Result<Zeroizing<Vec<u8>>, CodecError> {
    envelope.check_size()?;
    let aad = aad(binding)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&data_key[..]).map_err(|_| CodecError::AuthenticationFailed)?;
    cipher
        .decrypt(&XNonce::from(envelope.nonce), Payload { msg: &envelope.ciphertext, aad: &aad })
        .map(Zeroizing::new)
        .map_err(|_| CodecError::AuthenticationFailed)
}

#[cfg(test)]
#[path = "payload_tests.rs"]
mod tests;
