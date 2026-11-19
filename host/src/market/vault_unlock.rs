//! Desbloqueio serializado, adaptado do Codync 25202822 (ver `THIRD_PARTY.md`).
//! A tarefa bloqueante conserva a guarda mesmo se quem a espera desistir.
use super::{SERVICE, unavailable};
use crate::{LockExt, store::Store};
use anyhow::{Result, anyhow, bail};
use std::{sync::Arc, time::Duration};
use zeroize::Zeroizing;

const PROMPT_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) async fn unlock(hub: Arc<crate::hub::Hub>) -> Result<()> {
    if hub.store.secret_key.locked().is_some() {
        return Ok(());
    }
    // Margem para chamadas D-Bus em torno do prompt de um minuto.
    let wait = PROMPT_TIMEOUT + Duration::from_secs(5);
    let Ok(guard) = Arc::clone(&hub.store.vault_unlock).try_lock_owned() else {
        let _ = tokio::time::timeout(wait, hub.store.vault_unlock.lock()).await;
        return if hub.store.secret_key.locked().is_some() { Ok(()) } else { Err(unavailable()) };
    };
    let read = tokio::task::spawn_blocking(move || {
        // NÃO mover para fora da closure: timeout não cancela uma leitura OS.
        let _guard = guard;
        if hub.store.secret_key.locked().is_some() {
            return Ok(());
        }
        let key = read_key(&hub.store)?;
        *hub.store.secret_key.locked() = Some(key);
        // Com a chave à mão, as contas antigas ganham Space (uma vez, com marca).
        if let Err(error) = crate::market::contas_migracao::run(&hub) {
            tracing::error!(%error, "migração do Space das contas não concluída");
        }
        Ok(())
    });
    tokio::time::timeout(wait, read).await.map_err(|_| unavailable())??
}

fn read_key(store: &Store) -> Result<Zeroizing<Vec<u8>>> {
    let id = if let Some(id) = store.kv_read("vault.id")? {
        id
    } else {
        let id = uuid::Uuid::new_v4().to_string();
        store.kv_set("vault.id", &id)?;
        id
    };
    // Só as fixtures inscritas podem evitar D-Bus/Entry::new no binário de testes.
    #[cfg(test)]
    let fixture_entry = tests::entry(store);
    #[cfg(test)]
    let entry = if let Some(entry) = fixture_entry { entry } else { platform_entry(&id)? };
    #[cfg(not(test))]
    let entry = platform_entry(&id)?;
    let key = match entry.get_secret() {
        Ok(key) => key,
        Err(keyring::Error::NoEntry) => {
            if store.kv_read("vault.initialized")?.is_some() {
                bail!(
                    "The credential key is missing. Restore this computer's keychain; encrypted credentials were preserved."
                );
            }
            let mut key = vec![0; 32];
            getrandom::fill(&mut key).map_err(|_| anyhow!("Could not generate credential key"))?;
            entry.set_secret(&key).map_err(|_| unavailable())?;
            key
        }
        Err(_) => return Err(unavailable()),
    };
    if key.len() != 32 {
        bail!("Invalid credential key; restore this computer's keychain.");
    }
    store.kv_set("vault.initialized", "true")?;
    Ok(Zeroizing::new(key))
}

fn platform_entry(id: &str) -> Result<keyring::Entry> {
    #[cfg(target_os = "linux")]
    unlock_secret_service().map_err(|_| unavailable())?;
    keyring::Entry::new(SERVICE, id).map_err(|_| unavailable())
}

/// Pré-desbloqueia a coleção padrão com prazo antes da leitura keyring.
#[cfg(target_os = "linux")]
fn unlock_secret_service() -> Result<(), dbus_secret_service::Error> {
    use dbus_secret_service::{EncryptionType, Error, SecretService};
    let service = SecretService::connect_with_max_prompt_timeout(EncryptionType::Plain, PROMPT_TIMEOUT.as_secs())?;
    match service.get_default_collection() {
        Ok(collection) => collection.ensure_unlocked(),
        // 4.1.0: read_alias("default") retornou "/" (ausência legítima).
        Err(Error::NoResult) => Ok(()),
        // Transporte, prompt dispensado e demais falhas nunca autorizam geração.
        Err(error) => Err(error),
    }
}

#[cfg(test)]
#[path = "vault_unlock_tests.rs"]
mod tests;
