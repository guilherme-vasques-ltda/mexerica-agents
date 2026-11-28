// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Encrypted records backed by the operating system credential store. The database
//! contains ciphertext only; its random master key lives in Keychain/Secret Service.
//! Modificado pela DONNA Bot em 2026-11-03: selagem/abertura fora de DB e política
//! de leitura legada separadas da gravação; adaptação própria do fork, sem novo porte externo.
use crate::{LockExt, store::Store};
use anyhow::{Result, anyhow, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use std::sync::Arc;

const PREFIX: &str = "vault:v1:";
/// Serviço próprio do Donnabot (`app.donnabot.credentials`), separado do cofre do Codync.
const SERVICE: &str = crate::environment::Environment::current().vault_service();

#[path = "vault_unlock.rs"]
mod vault_unlock;

pub async fn unlock(hub: Arc<crate::hub::Hub>) -> Result<()> {
    if crate::environment::Environment::is_open() {
        return Err(crate::api::devices::Forbidden.into());
    }
    unlock_aberto(hub).await
}

/// Cofre da chamada de API. Na Open só o dono local o abre (conectar serviços); turno de bot, ponte e aparelho seguem recusados.
pub(crate) async fn unlock_para(hub: Arc<crate::hub::Hub>, caller: &crate::api::devices::Caller) -> Result<()> {
    if crate::environment::Environment::is_open() && !matches!(caller, crate::api::devices::Caller::Local) {
        return Err(crate::api::devices::Forbidden.into());
    }
    unlock_aberto(hub).await
}

async fn unlock_aberto(hub: Arc<crate::hub::Hub>) -> Result<()> {
    #[cfg(test)]
    hub.store.vault_unlock_attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    vault_unlock::unlock(hub).await
}

fn unavailable() -> anyhow::Error {
    anyhow!(
        "Secure credential storage is locked or unavailable. Unlock Keychain on macOS, or start and unlock a Secret Service keyring in this Linux user's D-Bus session (Windows keeps it in Credential Manager). Credentials are never saved as plain text."
    )
}

/// O erro é o de cofre trancado ou indisponível (sem chave aberta): quem fala com a tela troca por um código estável.
pub(crate) fn indisponivel(error: &anyhow::Error) -> bool {
    error.to_string() == unavailable().to_string()
}

pub(crate) fn read_unmigrated(store: &Store, slot: &str) -> Result<(Option<String>, bool)> {
    let Some(raw) = store.kv_read(slot)? else {
        return Ok((None, false));
    };
    let (plain, legacy) = decode_unmigrated(store, slot, &raw)?;
    Ok((Some(plain), legacy))
}

/// Política do registro legado; não lê DB, migra, grava ou cria chave.
/// Não usar em ledgers/proofs/credenciais novas. Ciphertext inválido nunca vira plaintext.
pub(crate) fn decode_unmigrated(store: &Store, slot: &str, raw: &str) -> Result<(String, bool)> {
    if !raw.starts_with(PREFIX) {
        return Ok((raw.to_owned(), true));
    }
    Ok((open(store, slot, raw)?, false))
}

/// Copie o ciphertext e libere SQLite antes desta função.
/// Somente ciphertext válido; não migra plaintext nem acessa DB.
/// Nunca chamar sob a guarda de DB: unlock mantém a ordem chave -> DB.
pub(crate) fn open(store: &Store, slot: &str, raw: &str) -> Result<String> {
    let encoded = raw.strip_prefix(PREFIX).ok_or_else(|| anyhow!("Invalid encrypted credential record"))?;
    let key = store.secret_key.locked();
    let key = key.as_ref().ok_or_else(unavailable)?;
    let bytes = STANDARD.decode(encoded).map_err(|_| anyhow!("Invalid encrypted credential record"))?;
    if bytes.len() < 24 {
        bail!("Invalid encrypted credential record");
    }
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| unavailable())?;
    let plain = cipher
        .decrypt(
            &XNonce::try_from(&bytes[..24]).map_err(|_| anyhow!("Invalid credential nonce"))?,
            Payload { msg: &bytes[24..], aad: slot.as_bytes() },
        )
        .map_err(|_| anyhow!("Could not decrypt credentials; the stored record was preserved"))?;
    Ok(String::from_utf8(plain)?)
}

/// Alias estrito para consumidores da main; mesmo primitivo, chave/AAD e zero DB.
pub(crate) fn open_ciphertext(store: &Store, slot: &str, raw: &str) -> Result<String> {
    open(store, slot, raw)
}

pub fn read(store: &Store, slot: &str) -> Result<Option<String>> {
    let (value, migrate) = read_unmigrated(store, slot)?;
    if migrate && let Some(value) = &value {
        write(store, slot, value)?;
    }
    Ok(value)
}

/// Sela fora da transação; o chamador pode compor KV e suas linhas atomicamente.
/// Cifra sem DB; nunca chamar sob a guarda de DB.
pub(crate) fn seal(store: &Store, slot: &str, value: &str) -> Result<String> {
    let key = store.secret_key.locked();
    let key = key.as_ref().ok_or_else(unavailable)?;
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| unavailable())?;
    let mut nonce = [0; 24];
    getrandom::fill(&mut nonce).map_err(|_| anyhow!("Could not generate credential nonce"))?;
    let sealed = cipher
        .encrypt(&nonce.into(), Payload { msg: value.as_bytes(), aad: slot.as_bytes() })
        .map_err(|_| anyhow!("Could not encrypt credentials"))?;
    let bytes: Vec<u8> = nonce.into_iter().chain(sealed).collect();
    Ok(format!("{PREFIX}{}", STANDARD.encode(bytes)))
}

/// Apenas UPSERT KV. O chamador controla a transação; não toca chave/cofre/Store.
pub(crate) fn write_sealed(db: &rusqlite::Connection, slot: &str, sealed: &str) -> Result<()> {
    if !sealed.starts_with(PREFIX) {
        bail!("Registro privado não cifrado");
    }
    db.execute("INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=?2", rusqlite::params![slot, sealed])?;
    Ok(())
}

pub fn write(store: &Store, slot: &str, value: &str) -> Result<()> {
    let sealed = seal(store, slot, value)?;
    // D368: num pedido do app pareado, grava sob a autoridade original reconferida na transação.
    crate::fork::donna_fixa::pedido::gravar(store, |db| write_sealed(db, slot, &sealed))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires an unlocked OS keychain or Secret Service session"]
    fn platform_keyring_roundtrip() {
        let id = format!("test-{}", uuid::Uuid::new_v4());
        let entry = keyring::Entry::new(SERVICE, &id).unwrap();
        let key = [42u8; 32];
        entry.set_secret(&key).unwrap();
        let loaded = entry.get_secret().unwrap();
        entry.delete_credential().unwrap();
        assert_eq!(loaded, key);
        assert!(matches!(entry.get_secret(), Err(keyring::Error::NoEntry)));
    }

    #[test]
    fn existing_plaintext_is_migrated_before_use() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.kv_set("record", "private password").unwrap();
        assert_eq!(read(&store, "record").unwrap().as_deref(), Some("private password"));
        assert!(store.kv_read("record").unwrap().unwrap().starts_with(PREFIX));
    }

    #[test]
    fn ciphertext_is_random_and_bound_to_its_record() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        write(&store, "one", "private password").unwrap();
        let first = store.kv_read("one").unwrap().unwrap();
        assert!(!first.contains("private password"));
        assert_eq!(read(&store, "one").unwrap().as_deref(), Some("private password"));
        write(&store, "one", "private password").unwrap();
        assert_ne!(first, store.kv_read("one").unwrap().unwrap());
        store.kv_set("two", &first).unwrap();
        assert!(read(&store, "two").is_err());
    }
    #[test]
    fn locked_storage_preserves_existing_data() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.kv_set("record", "existing secret").unwrap();
        *store.secret_key.locked() = None;
        assert!(read(&store, "record").is_err());
        assert!(write(&store, "record", "replacement").is_err());
        assert_eq!(store.kv_read("record").unwrap().as_deref(), Some("existing secret"));
    }
    #[test]
    fn open_is_strict_and_does_not_migrate_or_write() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        assert!(open(&store, "one", "plaintext").is_err());
        assert!(store.kv_read("one").unwrap().is_none());
        let sealed = seal(&store, "one", "fixture secret").unwrap();
        assert_eq!(open(&store, "one", &sealed).unwrap(), "fixture secret");
        assert!(open(&store, "other", &sealed).is_err());
        assert!(store.kv_read("one").unwrap().is_none());
    }
    #[test]
    fn decode_legacy_policy_is_zero_db_and_never_downgrades_bad_cipher() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let raw = r#"[{"id":"legacy"}]"#;
        let (plain, legacy) = decode_unmigrated(&store, "connectors", raw).unwrap();
        assert!(legacy);
        assert!(plain.eq(raw));
        assert!(store.kv_read("connectors").unwrap().is_none());
        let sealed = seal(&store, "connectors", raw).unwrap();
        let (plain, legacy) = decode_unmigrated(&store, "connectors", &sealed).unwrap();
        assert!(!legacy);
        assert!(plain.eq(raw));
        assert!(decode_unmigrated(&store, "other", &sealed).is_err());
        assert!(decode_unmigrated(&store, "connectors", "vault:v1:AA==").is_err());
        assert!(store.kv_read("connectors").unwrap().is_none());
        assert!(open(&store, "connectors", raw).is_err());
    }
}
