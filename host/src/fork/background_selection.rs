//! Seleção global do trabalhador, sem herdar a conta da conversa.
use crate::fork::background_notice::Problem;
use crate::{
    fork::{
        engine_accounts::{Backend, Billing, LaunchReference, Registry, State, probe},
        modelos_funcao::{
            catalogo::FunctionId,
            config::{self, Selection},
        },
    },
    store::{BotConfig, Store},
};
use anyhow::{Result, bail, ensure};

pub(crate) mod backoff;

pub(crate) struct Resolved {
    pub config: BotConfig,
    /// Revisão do documento de funções; `None` na conta do próprio bot (MB-3).
    revision: Option<u64>,
    account_revision: u64,
    identity: Option<String>,
    launch: LaunchReference,
    proof: Option<crate::fork::engine_accounts::SubscriptionProof>,
    pub(super) api: Option<(String, String)>,
    pub(super) catalog: Vec<String>,
}

impl Resolved {
    pub(crate) fn paid_api(&self) -> bool {
        self.api.is_some()
    }
}

pub(crate) fn resolve(store: &Store, original: &BotConfig, function: FunctionId) -> Result<Resolved> {
    ensure!(
        matches!(function, FunctionId::MemoryConsolidation | FunctionId::BotNaming),
        "função de fundo indisponível"
    );
    config::initialize(store)?;
    let document = config::load(store)?;
    let selected = document.functions.get(&function).ok_or_else(|| anyhow::anyhow!("função sem seleção"))?;
    let (Selection::Custom { binding } | Selection::Automatic { binding: Some(binding), .. }) = selected else {
        // MEX-1355: conta escolhida desconectada é motivo próprio, não "sem modelo".
        if let Selection::Automatic { account_id: Some(id), .. } = selected
            && let Some(problem) = disconnected(store, id)
        {
            return Err(problem.into());
        }
        let registry = Registry::load(store)?;
        if registry.accounts.iter().any(|account| account.backend == Backend::Claude && account.subscription_verified())
        {
            return Err(Problem::Model.into());
        }
        return Err(if original.backend == "codex" { Problem::Codex } else { Problem::NoClaude }.into());
    };
    let registry = Registry::load(store)?;
    let account = registry.account(&binding.account_id)?;
    if account.state != State::Connected {
        return Err(disconnected(store, &binding.account_id).unwrap_or(Problem::NoClaude).into());
    }
    let api = if account.billing == Billing::PaidApi {
        let slot = super::background_anthropic::slot(account).ok_or(Problem::Api)?;
        ensure!(matches!(selected, Selection::Custom { .. }), Problem::Api);
        let (sealed, _) = super::background_anthropic::credential(store, slot)
            .map_err(|_| Problem::AccountDisconnected { label: account.dto().label })?;
        Some((slot.to_owned(), sealed))
    } else {
        None
    };
    if account.backend == Backend::Codex {
        return Err(Problem::Codex.into());
    }
    if api.is_none() && !account.subscription_verified() {
        return Err(Problem::NoClaude.into());
    }
    let mut cfg = original.clone();
    cfg.backend = account.backend.id().into();
    cfg.command = None;
    cfg.fork_editor.engine_account_id = Some(binding.account_id.clone());
    cfg.model = Some(binding.model_id.clone());
    cfg.model_options = binding.model_options.clone();
    // MEX-1355: modelo ou opção que deixou de ser anunciado (inclusive numa
    // escolha personalizada) é falha de seleção classificada, sem trocar a escolha.
    crate::fork::engine_accounts::managed::validate_selection_in(&registry, &cfg).map_err(
        |error| -> anyhow::Error {
            let code = error.to_string();
            if code.starts_with("model_unavailable") || code.starts_with("model_option_unavailable") {
                Problem::Model.into()
            } else if code.starts_with("account_unavailable") {
                disconnected(store, &binding.account_id).unwrap_or(Problem::NoClaude).into()
            } else {
                error
            }
        },
    )?;
    Ok(Resolved {
        config: cfg,
        revision: Some(document.revision),
        account_revision: account.managed.as_ref().map_or(1, |m| m.revision),
        identity: account.quota_identity.clone(),
        launch: account.launch.clone(),
        proof: account.subscription_proof.clone(),
        catalog: if api.is_some() { account.models.iter().map(|model| model.id.clone()).collect() } else { Vec::new() },
        api,
    })
}

/// A conta do cadastro com o rótulo visto pelo dono, quando está desconectada ou indisponível.
fn disconnected(store: &Store, id: &str) -> Option<Problem> {
    let accounts = crate::fork::modelos_funcao::resolver::accounts(store).ok()?;
    let account = accounts.into_iter().find(|account| account.id == id)?;
    (config::usable(&account) == Err(crate::fork::modelos_funcao::resolver::Reason::AccountDisconnected))
        .then_some(Problem::AccountDisconnected { label: account.label })
}

/// A conta vinculada ao próprio bot é revalidada sem herdar a seleção global.
pub(crate) fn resolve_own(store: &Store, original: &BotConfig) -> Result<Resolved> {
    if original.backend == "codex" {
        return Err(Problem::Codex.into());
    }
    ensure!(original.backend == "claude" && original.command.is_none(), "Motor sem resumo isolado.");
    let id = original.fork_editor.engine_account_id.as_deref().ok_or(Problem::NoClaude)?;
    let registry = Registry::load(store)?;
    let account = registry.account(id)?;
    if account.billing == Billing::PaidApi {
        return Err(Problem::Api.into());
    }
    if account.backend != Backend::Claude
        || account.state != State::Connected
        || account.billing != Billing::Subscription
        || !account.subscription_verified()
    {
        return Err(Problem::NoClaude.into());
    }
    let mut cfg = original.clone();
    if cfg.model.is_none() {
        // Sem modelo escolhido: a mesma receita econômica da Memória (Haiku anunciado, esforço
        // baixo quando anunciado). Sem Haiku na conta, fica fora do resumo.
        let haiku = account.models.iter().find(|m| m.id == "haiku").ok_or(Problem::Model)?;
        cfg.model = Some(haiku.id.clone());
        cfg.model_options = haiku
            .options
            .iter()
            .find(|o| matches!(o.id.as_str(), "reasoning_effort" | "effort") && o.values.iter().any(|v| v == "low"))
            .map(|o| (o.id.clone(), "low".into()))
            .into_iter()
            .collect();
    }
    crate::fork::engine_accounts::managed::validate_selection(store, &cfg)?;
    Ok(Resolved {
        config: cfg,
        revision: None,
        account_revision: account.managed.as_ref().map_or(1, |m| m.revision),
        identity: account.quota_identity.clone(),
        launch: account.launch.clone(),
        proof: account.subscription_proof.clone(),
        api: None,
        catalog: Vec::new(),
    })
}

pub(crate) fn revalidate(store: &Store, resolved: &Resolved) -> Result<()> {
    match resolved.revision {
        Some(revision) if config::load(store)?.revision != revision => return Err(Problem::AccountChanged.into()),
        Some(_) => {}
        // Conta própria: o bot continua vinculado à mesma conta e ao mesmo motor.
        None => {
            let current = store.bot(&resolved.config.id)?.filter(|row| !row.deleted);
            if current.is_none_or(|row| {
                row.config.backend != resolved.config.backend
                    || row.config.fork_editor.engine_account_id != resolved.config.fork_editor.engine_account_id
            }) {
                return Err(Problem::AccountChanged.into());
            }
        }
    }
    let registry = Registry::load(store)?;
    let id =
        resolved.config.fork_editor.engine_account_id.as_deref().ok_or_else(|| anyhow::anyhow!("Conta ausente."))?;
    let account = registry.account(id)?;
    if account.managed.as_ref().map_or(1, |m| m.revision) != resolved.account_revision
        || account.quota_identity != resolved.identity
        || account.launch != resolved.launch
        || account.subscription_proof != resolved.proof
        || match &resolved.api {
            Some((slot, sealed)) => {
                super::background_anthropic::slot(account) != Some(slot)
                    || store.kv_read(slot)?.as_ref() != Some(sealed)
            }
            None => !account.subscription_verified(),
        }
    {
        return Err(Problem::AccountChanged.into());
    }
    crate::fork::engine_accounts::managed::validate_selection_in(&registry, &resolved.config)
}

pub(crate) async fn verify_identity(store: &Store, resolved: &Resolved) -> Result<()> {
    if let Some((slot, expected)) = &resolved.api {
        ensure!(super::background_anthropic::credential(store, slot)?.0 == *expected, Problem::AccountChanged);
        return revalidate(store, resolved);
    }
    let registry = Registry::load(store)?;
    let id =
        resolved.config.fork_editor.engine_account_id.as_deref().ok_or_else(|| anyhow::anyhow!("Conta ausente."))?;
    let account = registry.account(id)?;
    let LaunchReference::CliProfile { executable, directory } = &account.launch else {
        bail!("Conta sem perfil de assinatura verificável.");
    };
    crate::fork::background_policy::enforce(false).await.map_err(|_| Problem::Policy)?;
    let observation = probe::inspect(Backend::Claude, executable, directory).await;
    let identity = observation.identity.as_deref().map(|id| registry.fingerprint(Backend::Claude, "quota", id));
    if observation.state != State::Connected
        || observation.billing != Billing::Subscription
        || identity.is_none()
        || identity != account.quota_identity
        || observation.subscription_proof != account.subscription_proof
    {
        return Err(Problem::AccountChanged.into());
    }
    revalidate(store, resolved)
}

#[cfg(test)]
fn check_windows(windows: &[crate::usage::Window], model: &str) -> Result<()> {
    if !windows.iter().any(|w| w.id == "seven_day") {
        return Err(Problem::Quota.into());
    }
    let model = model.to_ascii_lowercase();
    for window in windows.iter().filter(|window| {
        window.id == "five_hour"
            || window.id == "seven_day"
            || window.id.strip_prefix("seven_day_").is_some_and(|name| model.contains(name))
    }) {
        if !window.percent.is_finite() {
            return Err(Problem::Quota.into());
        }
        if !(0.0..=75.0).contains(&window.percent) {
            return Err(crate::fork::background_executor::SubscriptionLimit.into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fork::modelos_funcao::config::{Binding, SelectionInput};
    use serde_json::json;
    use std::{collections::BTreeMap, path::Path};

    #[test]
    fn cota_semanal_permite_janela_de_sessao_ausente() {
        let weekly = crate::usage::parse_claude_usage("Current week (all models): 20% used");
        assert!(check_windows(&weekly, "haiku").is_ok());
        let session = crate::usage::parse_claude_usage("Current session: 10% used");
        assert!(check_windows(&session, "haiku").is_err());
        let exhausted = crate::usage::parse_claude_usage("Current week (all models): 90% used");
        assert!(check_windows(&exhausted, "haiku").is_err());
        let both = crate::usage::parse_claude_usage("Current session: 90% used\nCurrent week (all models): 20% used");
        assert!(check_windows(&both, "haiku").is_err());
        let another_model =
            crate::usage::parse_claude_usage("Current week (all models): 20% used\nCurrent week (Opus): 90% used");
        assert!(check_windows(&another_model, "haiku").is_ok());
        assert!(check_windows(&another_model, "opus").is_err());
    }

    #[test]
    fn usa_conta_escolhida_e_recusa_revisao_alterada() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let id = format!("ac_{}", "a".repeat(64));
        let registry = json!({"salt":uuid::Uuid::new_v4().to_string(), "accounts":[{
            "id":id, "backend":"claude", "billing":"subscription", "state":"connected",
            "launch":{"kind":"cli_profile", "executable":"/bin/true", "directory":"/tmp"},
            "scope":"owner", "quotaIdentity":id, "verifiedAt":1,
            "subscriptionProof":{"method":"claude_auth_status", "plan":"max"},
            "models":[{"id":"haiku", "options":[]}]
        }]});
        store.kv_set("fork.engine-accounts.v1", &registry.to_string()).unwrap();
        config::initialize(&store).unwrap();
        let initialized = config::load(&store).unwrap();
        assert!(matches!(&initialized.functions[&FunctionId::MemoryConsolidation],
            Selection::Automatic { binding: Some(binding), .. } if binding.account_id == id));
        assert!(matches!(&initialized.functions[&FunctionId::BotNaming],
            Selection::Automatic { binding: Some(binding), .. } if binding.account_id == id));
        let revision = config::load(&store).unwrap().revision;
        config::save(
            &store,
            revision,
            Some(FunctionId::MemoryConsolidation),
            Some(SelectionInput::Custom {
                binding: Binding { account_id: id.clone(), model_id: "haiku".into(), model_options: BTreeMap::new() },
            }),
        )
        .unwrap();
        let bot: BotConfig = serde_json::from_value(json!({"id":"bot", "name":"Bot", "backend":"codex"})).unwrap();
        let selected = resolve(&store, &bot, FunctionId::MemoryConsolidation).unwrap();
        assert_eq!(selected.config.backend, "claude");
        assert_eq!(selected.config.fork_editor.engine_account_id.as_deref(), Some(id.as_str()));
        assert!(revalidate(&store, &selected).is_ok());
        let mut changed_identity = registry.clone();
        changed_identity["accounts"][0]["quotaIdentity"] = format!("ac_{}", "f".repeat(64)).into();
        store.kv_set("fork.engine-accounts.v1", &changed_identity.to_string()).unwrap();
        assert!(revalidate(&store, &selected).is_err(), "revisão fixa não pode esconder outra identidade");
        store.kv_set("fork.engine-accounts.v1", &registry.to_string()).unwrap();
        let revision = config::load(&store).unwrap().revision;
        config::save(&store, revision, Some(FunctionId::BotNaming), None).unwrap();
        assert!(revalidate(&store, &selected).is_err());
        let mut broken = registry;
        broken["accounts"][0]["state"] = "disconnected".into();
        store.kv_set("fork.engine-accounts.v1", &broken.to_string()).unwrap();
        assert!(resolve(&store, &bot, FunctionId::MemoryConsolidation).is_err());
    }

    #[test]
    fn assinatura_sem_identidade_verificacao_ou_catalogo_continua_fechada_sem_reset() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let id = format!("ac_{}", "a".repeat(64));
        let registry = json!({"salt":uuid::Uuid::new_v4().to_string(), "accounts":[{
            "id":id, "backend":"claude", "billing":"subscription", "state":"unverified",
            "launch":{"kind":"cli_profile", "executable":"/fixture/claude", "directory":"/fixture/profile"},
            "scope":"owner", "subscriptionProof":{"method":"claude_auth_status", "plan":"max"},
            "models":[]
        }]});
        store.kv_set("fork.engine-accounts.v1", &registry.to_string()).unwrap();
        config::initialize(&store).unwrap();
        let before = config::load(&store).unwrap();
        let bot: BotConfig = serde_json::from_value(json!({"id":"bot", "name":"Bot", "backend":"claude"})).unwrap();
        for function in [FunctionId::MemoryConsolidation, FunctionId::BotNaming] {
            assert!(matches!(
                &before.functions[&function],
                Selection::Automatic { account_id: None, binding: None, issue: Some(_), .. }
            ));
            let Err(error) = resolve(&store, &bot, function) else {
                panic!("registro sem comprovação não pode executar");
            };
            assert!(matches!(error.downcast_ref::<Problem>(), Some(Problem::NoClaude)));
        }
        assert_eq!(config::load(&store).unwrap(), before, "não reinicia nem troca as escolhas");
        assert_eq!(store.kv_read("fork.engine-accounts.v1").unwrap().unwrap(), registry.to_string());
    }

    #[test]
    fn assinatura_codex_nao_e_trocada_por_outra_conta() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let id = format!("ac_{}", "b".repeat(64));
        let registry = json!({"salt":uuid::Uuid::new_v4().to_string(), "accounts":[{
            "id":id, "backend":"codex", "billing":"subscription", "state":"connected",
            "launch":{"kind":"cli_profile", "executable":"/bin/true", "directory":"/tmp"},
            "scope":"owner", "quotaIdentity":id, "verifiedAt":1,
            "subscriptionProof":{"method":"codex_account_read", "plan":"pro"},
            "models":[{"id":"codex-modelo", "options":[]}]
        }, {
            "id":format!("ac_{}", "c".repeat(64)), "backend":"claude", "billing":"subscription", "state":"connected",
            "launch":{"kind":"cli_profile", "executable":"/bin/true", "directory":"/tmp/claude"},
            "scope":"owner", "quotaIdentity":format!("ac_{}", "c".repeat(64)), "verifiedAt":1,
            "subscriptionProof":{"method":"claude_auth_status", "plan":"max"},
            "models":[{"id":"haiku", "options":[]}]
        }]});
        store.kv_set("fork.engine-accounts.v1", &registry.to_string()).unwrap();
        let revision = config::load(&store).unwrap().revision;
        config::save(
            &store,
            revision,
            Some(FunctionId::BotNaming),
            Some(SelectionInput::Custom {
                binding: Binding { account_id: id, model_id: "codex-modelo".into(), model_options: BTreeMap::new() },
            }),
        )
        .unwrap();
        let bot: BotConfig = serde_json::from_value(json!({"id":"bot", "name":"Bot", "backend":"claude"})).unwrap();
        assert!(resolve(&store, &bot, FunctionId::BotNaming).is_err());
    }
}
