use crate::store::BotConfig;
use anyhow::Result;

pub(crate) const UNAVAILABLE: &str = "anthropic_automation_platform: Rotinas e delegação com chave de API da Anthropic ainda não estão disponíveis neste sistema. Nenhuma outra conta foi usada.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    Platform,
    PersonalOnly,
    VaultKey,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Platform => UNAVAILABLE,
            Self::PersonalOnly => "anthropic_automation_personal: A chave Anthropic para automação só pode ser usada no Pessoal, sem comando personalizado nem outra conta de motor.",
            Self::VaultKey => "anthropic_automation_vault: Cadastre uma chave de API da Anthropic válida no cofre do Hub.",
        })
    }
}

impl std::error::Error for Refusal {}

impl Refusal {
    pub(crate) fn from_saved_error(message: &str) -> Option<Self> {
        [Self::Platform, Self::PersonalOnly, Self::VaultKey].into_iter().find(|reason| reason.to_string() == message)
    }
}

pub(crate) fn check(store: &crate::store::Store, cfg: &BotConfig) -> Result<()> {
    check_platform(cfg, cfg!(target_os = "linux"))?;
    if !uses_anthropic(cfg) {
        return Ok(());
    }
    anyhow::ensure!(
        cfg.space_id == crate::fork::spaces::PERSONAL
            && cfg.context.is_none()
            && !cfg.is_group()
            && cfg.command.is_none()
            && cfg.fork_editor.engine_account_id.is_none(),
        Refusal::PersonalOnly
    );
    crate::fork::motor_mexerica::ensure_runnable()?;
    anyhow::ensure!(vault_key(store)?.is_some(), Refusal::VaultKey);
    Ok(())
}

pub(crate) fn vault_key(store: &crate::store::Store) -> Result<Option<zeroize::Zeroizing<String>>> {
    let slot = crate::fork::motor_mexerica::credential_slot("anthropic");
    let Some(raw) = store.kv_read(&slot)? else { return Ok(None) };
    let key = zeroize::Zeroizing::new(crate::market::vault::open_ciphertext(store, &slot, &raw).map_err(|_| {
        tracing::warn!(reason = "vault_open", "chave Anthropic recusada");
        Refusal::VaultKey
    })?);
    crate::fork::background_anthropic::api_key(&key).map_err(|_| {
        tracing::warn!(reason = "key_format", "chave Anthropic recusada");
        Refusal::VaultKey
    })?;
    Ok(Some(key))
}

fn check_platform(cfg: &BotConfig, linux: bool) -> Result<()> {
    anyhow::ensure!(!uses_anthropic(cfg) || linux, Refusal::Platform);
    Ok(())
}

pub(crate) fn uses_anthropic(cfg: &BotConfig) -> bool {
    cfg.backend == crate::fork::motor_mexerica::ENGINE_ID
        && cfg.model.as_deref().is_some_and(|model| model.starts_with("anthropic/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recusa_apenas_o_modelo_anthropic_do_motor_nas_plataformas_sem_suporte() {
        let mut cfg: BotConfig = serde_json::from_value(json!({"id":"bot","name":"Bot","backend":"mexerica",
            "model":"anthropic/claude-fixture"}))
        .unwrap();
        assert_eq!(check_platform(&cfg, false).unwrap_err().downcast_ref(), Some(&Refusal::Platform));
        assert!(check_platform(&cfg, true).is_ok());
        cfg.model = Some("openai/modelo-ficticio".into());
        assert!(check_platform(&cfg, false).is_ok());
    }

    #[test]
    fn cofre_aceita_chave_api_cifrada_e_recusa_login_ou_texto_legado() {
        use crate::LockExt;
        let store = crate::store::Store::open(std::path::Path::new(":memory:")).unwrap();
        *store.secret_key.locked() = Some(zeroize::Zeroizing::new(vec![5; 32]));
        let slot = crate::fork::motor_mexerica::credential_slot("anthropic");
        let fixture = concat!("sk-ant-api", "-automation-fixture-not-live");
        assert!(vault_key(&store).unwrap().is_none());
        store.kv_set(&slot, fixture).unwrap();
        assert!(vault_key(&store).is_err());
        crate::market::vault::write(&store, &slot, "sk-ant-oat-fixture-not-live").unwrap();
        assert!(vault_key(&store).is_err());
        crate::market::vault::write(&store, &slot, fixture).unwrap();
        assert!(vault_key(&store).unwrap().is_some_and(|key| key.as_str() == fixture));
    }
}
