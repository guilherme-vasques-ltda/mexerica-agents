use crate::store::BotConfig;
use anyhow::Result;

pub(crate) const UNAVAILABLE: &str = "anthropic_automation_platform: Rotinas e delegação com chave de API da Anthropic ainda não estão disponíveis neste sistema. Nenhuma outra conta foi usada.";

#[derive(Debug)]
pub(crate) struct UnsupportedPlatform;

impl std::fmt::Display for UnsupportedPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(UNAVAILABLE)
    }
}

impl std::error::Error for UnsupportedPlatform {}

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
        "A chave Anthropic do Motor Mexerica só pode ser usada no Pessoal."
    );
    crate::fork::motor_mexerica::ensure_runnable()?;
    anyhow::ensure!(vault_key(store)?.is_some(), "Cadastre a chave Anthropic no cofre do Hub.");
    Ok(())
}

pub(crate) fn vault_key(store: &crate::store::Store) -> Result<Option<zeroize::Zeroizing<String>>> {
    let slot = crate::fork::motor_mexerica::credential_slot("anthropic");
    let Some(raw) = store.kv_read(&slot)? else { return Ok(None) };
    let key = zeroize::Zeroizing::new(crate::market::vault::open_ciphertext(store, &slot, &raw)?);
    crate::fork::background_anthropic::api_key(&key)?;
    Ok(Some(key))
}

fn check_platform(cfg: &BotConfig, linux: bool) -> Result<()> {
    anyhow::ensure!(!uses_anthropic(cfg) || linux, UnsupportedPlatform);
    Ok(())
}

fn uses_anthropic(cfg: &BotConfig) -> bool {
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
        assert!(check_platform(&cfg, false).unwrap_err().is::<UnsupportedPlatform>());
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
