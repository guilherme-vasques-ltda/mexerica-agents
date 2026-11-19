//! D70/F2a: apenas fixtures sintéticas e fronteiras do codec. Não prova custódia.
#![allow(clippy::unwrap_used, reason = "Fixtures sintéticas, sem dados ou chaves reais.")]
use super::*;

fn id(value: u8) -> Id {
    format!("00000000-0000-4000-8000-{value:012x}").try_into().unwrap()
}

fn revision(value: u64) -> Revision {
    value.try_into().unwrap()
}

fn synthetic_key() -> Zeroizing<[u8; 32]> {
    // Constante somente nesta fixture cfg(test), nunca custódia/mestra/credencial.
    Zeroizing::new([0xa7; 32])
}

fn external_binding() -> PayloadBinding {
    PayloadBinding {
        installation: InstallationMetadata {
            hub_id: id(1),
            installation_id: id(2),
            generation: revision(1),
            revision: revision(1),
        },
        domain: PayloadDomain::ExternalAccount { account_id: id(3), generation: revision(1), revision: revision(1) },
        object_id: id(4),
        object_version: revision(1),
        class: PayloadClass::HumanRequest,
    }
}

fn native_binding() -> PayloadBinding {
    let mut binding = external_binding();
    binding.domain = PayloadDomain::Native { domain_id: id(3), revision: revision(1) };
    binding
}

fn rejects_binding(key: &Zeroizing<[u8; 32]>, binding: &PayloadBinding, envelope: &Envelope) {
    assert_eq!(open(key, binding, envelope).err(), Some(CodecError::AuthenticationFailed));
}

/// Recebe só o resultado `bool`: falhas não formatam plaintext, chave ou ciphertext.
/// Não substituir por asserções que imprimam os buffers, mesmo nestas fixtures.
#[track_caller]
fn assert_bytes_comparison(matches: bool) {
    assert!(matches, "Comparação de bytes da fixture falhou");
}

#[test]
fn roundtrip_has_only_version_nonce_and_ciphertext_on_wire() {
    let key = synthetic_key();
    let binding = external_binding();
    let plaintext = Zeroizing::new(b"fixture sintetica F2a".to_vec());
    let envelope = seal(&key, &binding, &plaintext).unwrap();
    let bytes = envelope.to_bytes().unwrap();
    assert_eq!(&bytes[..4], b"DNPL");
    assert_eq!(bytes[4], 1);
    assert_eq!(&bytes[5..29], &envelope.nonce);
    assert_eq!(&bytes[29..33], &u32::try_from(plaintext.len() + TAG_LEN).unwrap().to_be_bytes());
    assert_eq!(bytes.len(), HEADER_LEN + TAG_LEN + plaintext.len());
    let parsed = Envelope::parse(&bytes).unwrap();
    assert_bytes_comparison(open(&key, &binding, &parsed).unwrap().as_slice() == plaintext.as_slice());
    assert_bytes_comparison(parsed.to_bytes().unwrap() == bytes);
}

#[test]
fn every_external_identity_generation_revision_object_and_class_is_bound() {
    let key = synthetic_key();
    let binding = external_binding();
    let envelope = seal(&key, &binding, b"fixture sintetica").unwrap();
    let mutations: [fn(&mut PayloadBinding); 9] = [
        |b| b.installation.hub_id = id(9),
        |b| b.installation.installation_id = id(9),
        |b| b.installation.generation = revision(2),
        |b| b.installation.revision = revision(2),
        |b| {
            b.domain =
                PayloadDomain::ExternalAccount { account_id: id(9), generation: revision(1), revision: revision(1) };
        },
        |b| {
            b.domain =
                PayloadDomain::ExternalAccount { account_id: id(3), generation: revision(2), revision: revision(1) };
        },
        |b| {
            b.domain =
                PayloadDomain::ExternalAccount { account_id: id(3), generation: revision(1), revision: revision(2) };
        },
        |b| b.object_id = id(9),
        |b| b.object_version = revision(2),
    ];
    for mutate in mutations {
        let mut changed = binding.clone();
        mutate(&mut changed);
        rejects_binding(&key, &changed, &envelope);
    }
    for class in [
        PayloadClass::ToolResult,
        PayloadClass::ModelResponse,
        PayloadClass::Summary,
        PayloadClass::ReviewCard,
        PayloadClass::Error,
        PayloadClass::Artifact,
    ] {
        let mut changed = binding.clone();
        changed.class = class;
        rejects_binding(&key, &changed, &envelope);
        let own = seal(&key, &changed, b"fixture sintetica").unwrap();
        assert_bytes_comparison(open(&key, &changed, &own).unwrap().as_slice() == b"fixture sintetica");
    }
}

#[test]
fn native_domain_id_revision_and_domain_tag_are_bound() {
    let key = synthetic_key();
    let binding = native_binding();
    let envelope = seal(&key, &binding, b"dominio sintetico").unwrap();
    assert_bytes_comparison(open(&key, &binding, &envelope).unwrap().as_slice() == b"dominio sintetico");
    for domain in [
        PayloadDomain::Native { domain_id: id(9), revision: revision(1) },
        PayloadDomain::Native { domain_id: id(3), revision: revision(2) },
        PayloadDomain::ExternalAccount { account_id: id(3), generation: revision(1), revision: revision(1) },
    ] {
        let mut changed = binding.clone();
        changed.domain = domain;
        rejects_binding(&key, &changed, &envelope);
    }
    let external = external_binding();
    let other = seal(&key, &external, b"conta sintetica").unwrap();
    rejects_binding(&key, &binding, &other);
}

#[test]
fn canonical_aad_prefixes_every_field_in_fixed_order() {
    let binding = external_binding();
    let bytes = aad(&binding).unwrap();
    let mut remaining = bytes.as_slice();
    let mut decoded = Vec::new();
    while !remaining.is_empty() {
        assert!(remaining.len() >= 2);
        let length = usize::from(u16::from_be_bytes(remaining[..2].try_into().unwrap()));
        remaining = &remaining[2..];
        assert!(remaining.len() >= length);
        decoded.push(remaining[..length].to_vec());
        remaining = &remaining[length..];
    }
    assert_eq!(
        decoded,
        vec![
            b"donna/plugins/payload".to_vec(),
            vec![1],
            id(1).as_str().as_bytes().to_vec(),
            id(2).as_str().as_bytes().to_vec(),
            1_u64.to_be_bytes().to_vec(),
            1_u64.to_be_bytes().to_vec(),
            b"external-account".to_vec(),
            id(3).as_str().as_bytes().to_vec(),
            1_u64.to_be_bytes().to_vec(),
            1_u64.to_be_bytes().to_vec(),
            id(4).as_str().as_bytes().to_vec(),
            1_u64.to_be_bytes().to_vec(),
            b"human-request".to_vec(),
        ]
    );
    let mut first = Vec::new();
    field(&mut first, b"ab").unwrap();
    field(&mut first, b"c").unwrap();
    let mut second = Vec::new();
    field(&mut second, b"a").unwrap();
    field(&mut second, b"bc").unwrap();
    assert_ne!(first, second);
    let original = first.clone();
    assert_eq!(field(&mut first, &vec![0; 65_536]), Err(CodecError::InvalidMetadata));
    assert_eq!(first, original);
}

#[test]
fn protocol_and_envelope_version_are_authenticated_aad_fields() {
    let key = synthetic_key();
    let binding = external_binding();
    let envelope = seal(&key, &binding, b"fixture de protocolo").unwrap();
    let expected_aad = aad(&binding).unwrap();
    let cipher = XChaCha20Poly1305::new_from_slice(&key[..]).unwrap();
    for index in [2, 2 + PROTOCOL.len() + 2] {
        let mut changed_aad = expected_aad.clone();
        changed_aad[index] ^= 1;
        assert!(
            cipher
                .decrypt(&XNonce::from(envelope.nonce), Payload { msg: &envelope.ciphertext, aad: &changed_aad })
                .is_err()
        );
    }
}

#[test]
fn nonce_ciphertext_tag_and_wrong_key_are_rejected() {
    let key = synthetic_key();
    let binding = external_binding();
    let envelope = seal(&key, &binding, b"fixture de adulteracao").unwrap();
    let bytes = envelope.to_bytes().unwrap();
    for index in (5..29).chain(HEADER_LEN..bytes.len()) {
        let mut tampered = bytes.clone();
        tampered[index] ^= 1;
        let parsed = Envelope::parse(&tampered).unwrap();
        rejects_binding(&key, &binding, &parsed);
    }
    // Inclusive chave sintética zerada: bytes não são prova de custódia.
    for wrong in [[0; 32], [0xb8; 32]] {
        rejects_binding(&Zeroizing::new(wrong), &binding, &envelope);
    }
}

#[test]
fn production_entry_uses_fresh_random_nonces() {
    let key = synthetic_key();
    let binding = external_binding();
    let first = seal(&key, &binding, b"fixture repetida").unwrap();
    let second = seal(&key, &binding, b"fixture repetida").unwrap();
    // Teste de regressão, não prova matemática de unicidade nem gestão de chaves.
    assert_ne!(first.nonce, second.nonce);
    assert_bytes_comparison(first.ciphertext != second.ciphertext);
}

#[test]
fn entropy_failure_is_static_and_returns_no_envelope() {
    struct ResetFailure;
    impl Drop for ResetFailure {
        fn drop(&mut self) {
            FAIL_ENTROPY.with(|flag| flag.set(false));
        }
    }
    let key = synthetic_key();
    let binding = external_binding();
    FAIL_ENTROPY.with(|flag| flag.set(true));
    let guard = ResetFailure;
    assert!(matches!(seal(&key, &binding, b"fixture"), Err(CodecError::EntropyUnavailable)));
    drop(guard);
    assert!(seal(&key, &binding, b"fixture").is_ok());
}

#[test]
fn plaintext_and_ciphertext_limits_cover_zero_short_maximum_and_long() {
    let key = synthetic_key();
    let binding = external_binding();
    assert!(matches!(seal(&key, &binding, b""), Err(CodecError::InvalidSize)));
    assert!(matches!(seal(&key, &binding, &vec![0; MAX_PLAINTEXT_LEN + 1]), Err(CodecError::InvalidSize)));
    for length in [1, MAX_PLAINTEXT_LEN] {
        let plaintext = Zeroizing::new(vec![0; length]);
        let envelope = seal(&key, &binding, &plaintext).unwrap();
        let bytes = envelope.to_bytes().unwrap();
        assert_eq!(bytes.len(), HEADER_LEN + TAG_LEN + length);
        let parsed = Envelope::parse(&bytes).unwrap();
        assert_bytes_comparison(open(&key, &binding, &parsed).unwrap().as_slice() == plaintext.as_slice());
    }
    for length in [0, TAG_LEN - 1, TAG_LEN, MAX_CIPHERTEXT_LEN + 1] {
        let envelope = Envelope { nonce: [0; NONCE_LEN], ciphertext: vec![0; length] };
        assert_eq!(open(&key, &binding, &envelope).err(), Some(CodecError::InvalidSize));
        assert_eq!(envelope.to_bytes().err(), Some(CodecError::InvalidSize));
    }
}

#[test]
fn parser_rejects_wrong_version_magic_lengths_truncation_and_suffixes() {
    let envelope = seal(&synthetic_key(), &external_binding(), b"fixture parser").unwrap();
    let valid = envelope.to_bytes().unwrap();
    for length in [0, 1, 4, 5, 28, HEADER_LEN, HEADER_LEN + TAG_LEN] {
        assert_eq!(Envelope::parse(&valid[..length]).err(), Some(CodecError::InvalidSize));
    }
    for version in [0, 2, 255] {
        let mut bytes = valid.clone();
        bytes[4] = version;
        assert_eq!(Envelope::parse(&bytes).err(), Some(CodecError::UnsupportedVersion));
    }
    let mut wrong_magic = valid.clone();
    wrong_magic[0] ^= 1;
    assert_eq!(Envelope::parse(&wrong_magic).err(), Some(CodecError::InvalidEnvelope));
    for size in [0_u32, 16, u32::MAX] {
        let mut bytes = valid.clone();
        bytes[29..33].copy_from_slice(&size.to_be_bytes());
        assert_eq!(Envelope::parse(&bytes).err(), Some(CodecError::InvalidEnvelope));
    }
    let mut trailing = valid.clone();
    trailing.push(0);
    assert_eq!(Envelope::parse(&trailing).err(), Some(CodecError::InvalidEnvelope));
    let truncated = &valid[..valid.len() - 1];
    assert_eq!(Envelope::parse(truncated).err(), Some(CodecError::InvalidEnvelope));
    assert_eq!(Envelope::parse(&vec![0; MAX_ENVELOPE_LEN + 1]).err(), Some(CodecError::InvalidSize));
}

#[test]
fn errors_have_only_static_codes_without_raw_sources() {
    use std::error::Error;
    for error in [
        CodecError::InvalidEnvelope,
        CodecError::UnsupportedVersion,
        CodecError::InvalidSize,
        CodecError::InvalidMetadata,
        CodecError::EntropyUnavailable,
        CodecError::EncryptionFailed,
        CodecError::AuthenticationFailed,
    ] {
        assert!(error.to_string().starts_with("plugin_payload_"));
        assert!(error.source().is_none());
    }
}
