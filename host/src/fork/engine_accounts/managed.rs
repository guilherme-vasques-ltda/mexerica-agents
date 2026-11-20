//! D164: metadados aditivos e perfis próprios, sem importar credenciais do legado.
use super::{Account, Backend, Billing, LaunchReference, Registry, State, SubscriptionProof, probe};
use crate::{
    LockExt,
    hub::{BotStatus, Hub},
    store::{BotConfig, Store},
};
use anyhow::{Result, anyhow, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Metadata {
    pub label: String,
    pub revision: u64,
    pub generation: u64,
    pub enabled: bool,
    pub request_id: String,
    pub principal: Option<String>,
    #[serde(default)]
    pub profile: Option<PathBuf>,
    #[serde(default)]
    pub setup_request: Option<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<CatalogBinding>,
}

/// Prova privada do catálogo; uma leitura de auth status sem catálogo não a substitui.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CatalogBinding {
    account_id: String,
    principal: String,
    launch: LaunchReference,
    revision: u64,
    subscription: Option<SubscriptionProof>,
}
impl Account {
    pub(crate) fn catalog_valid(&self) -> bool {
        self.managed.as_ref().is_some_and(|meta| {
            meta.enabled
                && meta.catalog.as_ref().is_some_and(|catalog| {
                    self.state == State::Connected
                        && !self.models.is_empty()
                        && catalog.account_id == self.id
                        && Some(&catalog.principal) == meta.principal.as_ref()
                        && catalog.launch == self.launch
                        && catalog.revision == meta.revision
                        && catalog.subscription == self.subscription_proof
                })
        })
    }
    pub(crate) fn bind_catalog(&mut self) {
        let Some(meta) = &mut self.managed else { return };
        meta.catalog = if self.backend == Backend::Claude
            && self.state == State::Connected
            && self.subscription_proof.is_some()
            && !self.models.is_empty()
        {
            meta.principal.as_ref().map(|principal| CatalogBinding {
                account_id: self.id.clone(),
                principal: principal.clone(),
                launch: self.launch.clone(),
                revision: meta.revision,
                subscription: self.subscription_proof.clone(),
            })
        } else {
            None
        };
    }
    pub(crate) fn rename(&mut self, label: String) -> Result<()> {
        let valid = self.catalog_valid();
        let meta = self.managed.as_mut().ok_or_else(|| anyhow!("legacy_preserved"))?;
        meta.label = label;
        meta.revision += 1;
        if valid && let Some(catalog) = &mut meta.catalog {
            catalog.revision = meta.revision;
        }
        Ok(())
    }
}

pub(crate) fn request_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty() && id.len() <= 100 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_:".contains(&b)),
        "invalid_field: requestId inválido."
    );
    Ok(())
}

pub(crate) fn label(label: &str) -> Result<String> {
    let label = label.trim();
    ensure!(
        !label.is_empty() && label.chars().count() <= 60 && !label.chars().any(char::is_control),
        "invalid_field: rótulo deve ter entre 1 e 60 caracteres."
    );
    Ok(label.to_owned())
}

/// MX-6: id de modelo opaco e curto. Não exige presença no catálogo — um id que some do
/// catálogo continua guardado, sem erro.
pub(crate) fn model_id(id: &str) -> Result<String> {
    let id = id.trim();
    ensure!(
        !id.is_empty() && id.chars().count() <= 128 && !id.chars().any(char::is_control),
        "invalid_field: identificador de modelo inválido."
    );
    Ok(id.to_owned())
}

const MAX_HIDDEN_MODELS: usize = 128;

impl Account {
    /// Ocultar/mostrar é só a lista `hidden_model_ids`: não toca `models`, revisão,
    /// catálogo nem execução — modelo oculto em uso continua valendo. Idempotente.
    pub(crate) fn set_model_hidden(&mut self, id: &str, hidden: bool) -> Result<()> {
        if hidden {
            if !self.hidden_model_ids.iter().any(|m| m == id) {
                ensure!(
                    self.hidden_model_ids.len() < MAX_HIDDEN_MODELS,
                    "account_limit: limite de modelos ocultos atingido."
                );
                self.hidden_model_ids.push(id.to_owned());
            }
        } else {
            self.hidden_model_ids.retain(|m| m != id);
        }
        Ok(())
    }
}

pub(crate) fn private_dir(path: &Path) -> Result<()> {
    // Cada ancestral dentro da raiz criada pelo Hub é conferido antes de usá-lo.
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        ensure!(meta.is_dir() && !meta.file_type().is_symlink(), "profile_invalid: perfil indisponível.");
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::create_dir(path)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub(crate) fn new_profile(store: &Store, id: &str, backend: Backend) -> Result<PathBuf> {
    ensure!(super::valid_id(id), "account_invalid: conta inválida.");
    if crate::fork::motor_exec::configured() {
        let result = crate::fork::motor_roots::profile("criar", &[id.as_ref(), backend.id().as_ref()])?;
        return Ok(PathBuf::from(String::from_utf8(result)?.trim()));
    }
    private_dir(&store.engine_setup_root)?;
    let accounts = store.engine_setup_root.join("accounts");
    private_dir(&accounts)?;
    let account = accounts.join(id);
    private_dir(&account)?;
    let home = account.join(uuid::Uuid::new_v4().to_string());
    private_dir(&home)?;
    let marker = home.join(".donnabot-profile");
    std::fs::write(&marker, [])?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o600))?;
    }
    let directory = home.join(backend.profile_directory());
    private_dir(&directory)?;
    Ok(directory)
}

pub(crate) fn validate_profile(store: &Store, id: &str, directory: &Path) -> Result<()> {
    if crate::fork::motor_exec::configured() {
        ensure!(
            super::valid_id(id) && directory.starts_with(crate::fork::motor_roots::accounts().join(id)),
            "profile_invalid: referência inválida."
        );
        crate::fork::motor_roots::profile("validar", &[directory.as_os_str()])?;
        return Ok(());
    }
    let root = store.engine_setup_root.join("accounts").join(id);
    ensure!(super::valid_id(id) && directory.starts_with(&root), "profile_invalid: referência inválida.");
    let relative = directory.strip_prefix(&store.engine_setup_root)?;
    let mut path = store.engine_setup_root.clone();
    ensure!(!std::fs::symlink_metadata(&path)?.file_type().is_symlink(), "profile_invalid: link recusado.");
    for part in relative.components() {
        ensure!(matches!(part, std::path::Component::Normal(_)), "profile_invalid: referência inválida.");
        path.push(part);
        let meta = std::fs::symlink_metadata(&path)?;
        ensure!(meta.is_dir() && !meta.file_type().is_symlink(), "profile_invalid: link recusado.");
    }
    Ok(())
}

pub(crate) fn secure_files(directory: &Path) -> Result<()> {
    if crate::fork::motor_exec::configured() {
        // O chamador histórico protege a home; o ajudante recebe o perfil canônico.
        let profile = if directory.file_name().is_some_and(|v| v == ".claude" || v == ".codex" || v == ".grok") {
            directory.to_path_buf()
        } else {
            anyhow::bail!("profile_invalid: operação exige perfil, não home")
        };
        crate::fork::motor_roots::profile("proteger", &[profile.as_os_str()])?;
        return Ok(());
    }
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let meta = entry.file_type()?;
        ensure!(!meta.is_symlink(), "profile_invalid: link no perfil.");
        if meta.is_dir() {
            private_dir(&entry.path())?;
            secure_files(&entry.path())?;
        } else {
            ensure!(meta.is_file(), "profile_invalid: arquivo especial.");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(entry.path(), std::fs::Permissions::from_mode(0o600))?;
            }
        }
    }
    Ok(())
}

impl Registry {
    pub(crate) fn persist(&self, store: &Store) -> Result<()> {
        store.kv_set(super::SLOT, &serde_json::to_string(self)?)?;
        // MEX-1355: mudança no cadastro tira a função de fundo da parada.
        crate::fork::background_selection::backoff::changed(store);
        Ok(())
    }
    pub(crate) fn account(&self, id: &str) -> Result<&Account> {
        self.accounts.iter().find(|a| a.id == id).ok_or_else(|| anyhow!("account_not_found: conta não encontrada."))
    }
    /// Perfil antigo que pode sair da lista: já desconectado, perfil CLI do próprio computador (sem cópia de credencial
    /// do Hub nem cofre) e na revisão que o DTO mostra. Falha fechada: o resto continua `legacy_preserved`.
    /// Remover tira só o registro; nenhum arquivo, login ou segredo é apagado.
    pub(crate) fn legacy_removable(&self, id: &str, revision: u64) -> Result<()> {
        let account = self.account(id)?;
        ensure!(
            account.managed.is_none()
                && account.state == State::Disconnected
                && matches!(account.launch, LaunchReference::CliProfile { .. })
                && !migrated_legacy(account),
            "legacy_preserved: configuração legada deve ser mantida."
        );
        ensure!(account.dto().revision == revision, "account_conflict: releia a conta antes de continuar.");
        Ok(())
    }
    pub(crate) fn account_mut(&mut self, id: &str, revision: u64) -> Result<&mut Account> {
        let account = self
            .accounts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| anyhow!("account_not_found: conta não encontrada."))?;
        let managed = account
            .managed
            .as_ref()
            .ok_or_else(|| anyhow!("legacy_preserved: configuração legada deve ser mantida."))?;
        ensure!(managed.revision == revision, "account_conflict: releia a conta antes de continuar.");
        Ok(account)
    }
    pub(crate) fn response_with_bots(&self, store: &Store) -> Result<Value> {
        let mut value = self.response();
        attach_bots(store, &mut value)?;
        Ok(value)
    }
    pub(crate) fn dto_for(&self, store: &Store, id: &str) -> Result<Value> {
        self.response_with_bots(store)?["accounts"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|a| a["id"] == id)
            .cloned()
            .ok_or_else(|| anyhow!("account_not_found"))
    }
    pub(crate) fn apply_managed(&mut self, index: usize, observation: probe::Observation) {
        let identity = observation.identity.as_deref().map(principal);
        let principal = identity.map(|id| self.fingerprint(observation.backend, "principal", id));
        let quota = observation.identity.as_deref().map(|id| self.fingerprint(observation.backend, "quota", id));
        let account = &mut self.accounts[index];
        let preserve_catalog = observation.backend == Backend::Claude
            && !observation.catalog_observed
            && observation.models.is_empty()
            && account.catalog_valid()
            && observation.state == State::Connected
            && observation.billing == account.billing
            && observation.launch == account.launch
            && observation.subscription_proof == account.subscription_proof
            && account.managed.as_ref().is_some_and(|m| m.principal == principal);
        let Some(meta) = account.managed.as_mut() else { return };
        if !meta.enabled {
            return;
        }
        if observation.state == State::Unavailable || observation.launch != account.launch {
            account.state = State::Unavailable;
            account.models.clear();
            meta.catalog = None;
            return;
        }
        if observation.state == State::Disconnected {
            account.state = State::Disconnected;
            account.models.clear();
            account.verified_at = None;
            meta.catalog = None;
            return;
        }
        // Descoberta não pode trocar a identidade da conta silenciosamente.
        if meta.principal.is_some() && meta.principal != principal {
            account.state = State::Unverified;
            account.models.clear();
            meta.catalog = None;
            return;
        }
        account.state = observation.state;
        account.billing = observation.billing;
        account.subscription_proof = observation.subscription_proof;
        if !preserve_catalog {
            account.models = observation.models;
        }
        account.verified_at = (account.state == State::Connected && principal.is_some()).then(crate::store::now_ms);
        account.quota_identity = quota;
        if account.verified_at.is_some() {
            meta.principal = principal;
        }
        if !preserve_catalog {
            if observation.catalog_observed {
                account.bind_catalog();
            } else {
                account.managed.as_mut().and_then(|meta| meta.catalog.take());
            }
        }
    }
}

pub(crate) fn principal(identity: &str) -> &str {
    // Alteração de plano não troca a pessoa/workspace na reconexão; cota é outra chave.
    identity.split("\0plan:").next().unwrap_or(identity)
}

pub(crate) fn vault_states(store: &Store, registry: &mut Registry) -> Result<()> {
    for account in &mut registry.accounts {
        if let LaunchReference::VaultProfile { slot } = &account.launch
            && account.managed.as_ref().is_none_or(|m| m.enabled)
        {
            if store.kv_read(slot)?.is_none() {
                account.state = State::Disconnected;
            } else if store.secret_key.locked().is_none() {
                account.state = State::Unavailable;
            } else if account.managed.is_some() {
                account.state =
                    if probe::vault_has_content(store, slot) { State::Connected } else { State::Disconnected };
            }
        }
    }
    Ok(())
}

pub(crate) fn bot_ids(store: &Store, id: &str) -> Result<Vec<String>> {
    bot_ids_in(&store.db.locked(), id)
}

/// Os bots vivos que usam a conta: padrão do bot, conversa de Space e (D372) seleção efetiva ou próxima das conversas
/// do Pessoal. Só a `Connection` recebida (leitura final dentro da transação da Authority na remoção).
pub(crate) fn bot_ids_in(c: &rusqlite::Connection, id: &str) -> Result<Vec<String>> {
    bot_ids_scoped(c, id, None, true)
}

/// MEX-1294/F6-A: a conta está vinculada ao Space? Mesma fonte de `bot_ids_in`, restrita ao Space: padrão de um bot
/// do Space, conversa do Space ligada à conta e seleção D372 de um bot do Space (as do Pessoal incluídas). O Pessoal
/// também é um Space: sem vínculo comprovado a leitura de uso não acontece.
pub(crate) fn linked_to_space(c: &rusqlite::Connection, id: &str, space: &str) -> Result<bool> {
    // Vínculo de verdade (um bot do Space usa a conta). Constar de uma ordem de reserva e rodar numa reserva por causa
    // do fallback NÃO contam: a reserva precisa de um vínculo independente da própria lista (MEX-1391, regra (k)(2)).
    Ok(!bot_ids_scoped(c, id, Some(space), false)?.is_empty())
}

fn bot_ids_scoped(c: &rusqlite::Connection, id: &str, space: Option<&str>, with_fallback: bool) -> Result<Vec<String>> {
    let in_space = |row: &crate::store::BotRow| space.is_none_or(|s| row.config.space_id == s);
    let alive: Vec<String> = c
        .prepare("SELECT id FROM bots WHERE deleted=0")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut bots = Vec::new();
    for bot in alive {
        let row = Store::bot_on(c, &bot)?.filter(|b| !b.deleted);
        if row.is_some_and(|b| in_space(&b) && b.config.fork_editor.engine_account_id.as_deref() == Some(id)) {
            bots.push(bot);
        }
    }
    let bound=c.prepare("SELECT DISTINCT c.bot_id FROM donna_space_conversations c JOIN bots b ON b.id=c.bot_id AND b.deleted=0 WHERE c.engine_account_id=?1 AND (?2 IS NULL OR c.space_id=?2)")?.query_map(rusqlite::params![id, space],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    bots.extend(bound);
    for bot in crate::fork::sessoes::engine_choice::bots_using_account(c, id, with_fallback)? {
        if Store::bot_on(c, &bot)?.filter(|b| !b.deleted).is_some_and(|b| in_space(&b)) {
            bots.push(bot);
        }
    }
    bots.sort();
    bots.dedup();
    Ok(bots)
}

/// Preflight; a garantia final é a cerca de cada mutador (`Hub::with_idle_engine_account`).
pub(crate) fn idle(hub: &Hub, id: &str) -> Result<()> {
    hub.with_idle_engine_account(id, || Ok(()))
}

/// Predicado único de ocupação da conta. `status` lê o mapa de runtime que o chamador já bloqueou (ausente = Idle).
pub(crate) fn idle_with(store: &Store, id: &str, status: impl Fn(&str) -> BotStatus) -> Result<()> {
    ensure!(
        bot_ids(store, id)?
            .iter()
            .all(|id| !matches!(status(id), BotStatus::Working | BotStatus::NeedsInput | BotStatus::Waiting)),
        "account_busy: aguarde o turno terminar."
    );
    Ok(())
}

#[derive(Clone)]
pub(crate) struct AccountLaunch {
    pub env: Vec<(String, String)>,
    pub revision: u64,
}

pub(crate) fn migrated_legacy(account: &Account) -> bool {
    let LaunchReference::CliProfile { directory, .. } = &account.launch else { return false };
    account.managed.is_none()
        && *directory
            == crate::fork::motor_roots::accounts()
                .join(&account.id)
                .join("legacy")
                .join(account.backend.profile_directory())
}

pub(crate) fn resolve_for_bot(store: &Store, bot: &BotConfig) -> Result<Option<AccountLaunch>> {
    let Some(id) = bot.fork_editor.engine_account_id.as_deref() else { return Ok(None) };
    ensure!(!crate::fork::engine_setup::in_setup(store, id), "setup_in_progress: aguarde ou cancele o setup.");
    validate_selection(store, bot)?;
    let registry = Registry::load(store)?;
    let account = registry.account(id)?;
    ensure!(account.backend.id() == bot.backend && bot.command.is_none(), "account_incompatible: motor incompatível.");
    ensure!(account.state == State::Connected, "account_unavailable: reconecte a conta selecionada.");
    if let Some(meta) = &account.managed {
        ensure!(meta.enabled, "account_disconnected: conta desconectada.");
    }
    match &account.launch {
        LaunchReference::CliProfile { executable, directory } => {
            ensure!(
                crate::fork::motor_install::executable(executable),
                "cli_unavailable: prepare o conector da conta."
            );
            if account.managed.is_some() || migrated_legacy(account) {
                validate_profile(store, id, directory)?;
            }
            let mut env = Vec::new();
            if account.managed.is_some() || migrated_legacy(account) {
                env.push((
                    "HOME".into(),
                    directory.parent().ok_or_else(|| anyhow!("profile_invalid"))?.to_string_lossy().into_owned(),
                ));
            }
            env.push((account.backend.profile_variable().into(), directory.to_string_lossy().into_owned()));
            env.push(("DISABLE_AUTOUPDATER".into(), "1".into()));
            if account.backend == Backend::Grok {
                env.push(("GROK_DISABLE_AUTOUPDATER".into(), "1".into()));
            } else {
                env.push(("CODEX_PATH".into(), executable.to_string_lossy().into_owned()));
                env.push(("CLAUDE_CODE_EXECUTABLE".into(), executable.to_string_lossy().into_owned()));
            }
            Ok(Some(AccountLaunch { env, revision: account.managed.as_ref().map_or(1, |m| m.revision) }))
        }
        LaunchReference::VaultProfile { slot } => {
            let raw = zeroize::Zeroizing::new(
                crate::market::vault::read(store, slot)?
                    .ok_or_else(|| anyhow!("account_unavailable: credencial indisponível."))?,
            );
            let vars: BTreeMap<String, String> = serde_json::from_str(&raw)?;
            let mut env: Vec<(String, String)> = vars.into_iter().collect();
            let executable = crate::agent::backends::which(account.backend.id());
            if let Some(profile) = account.managed.as_ref().and_then(|m| m.profile.as_ref()) {
                validate_profile(store, id, profile)?;
                env.extend(crate::fork::engine_setup::profile_environment(profile, account.backend));
                if let Some(executable) = executable {
                    env.push(("CODEX_PATH".into(), executable.to_string_lossy().into_owned()));
                    env.push(("CLAUDE_CODE_EXECUTABLE".into(), executable.to_string_lossy().into_owned()));
                }
            }
            Ok(Some(AccountLaunch { env, revision: account.managed.as_ref().map_or(1, |m| m.revision) }))
        }
    }
}

pub(crate) fn validate_selection(store: &Store, bot: &BotConfig) -> Result<()> {
    if bot.fork_editor.engine_account_id.is_none() {
        return Ok(());
    }
    let registry = Registry::load(store)?;
    validate_selection_in(&registry, bot)
}

/// Mesma conferência na revisão lida dentro da transação, sem reentrar em Store.
pub(crate) fn validate_selection_in(registry: &Registry, bot: &BotConfig) -> Result<()> {
    let account = registry.account(bot.fork_editor.engine_account_id.as_deref().unwrap_or_default())?;
    ensure!(
        account.backend.id() == bot.backend && !bot.is_group() && bot.command.is_none(),
        "account_incompatible: conta e motor incompatíveis."
    );
    ensure!(
        account.state == State::Connected && account.managed.as_ref().is_none_or(|m| m.enabled),
        "account_unavailable: reconecte a conta."
    );
    // Sem modelo explícito, opções não teriam a quem se aplicar: recusa em vez de ignorá-las na execução.
    ensure!(bot.model.is_some() || bot.model_options.is_empty(), "model_option_unavailable: opção não comprovada.");
    if let Some(model) = &bot.model {
        ensure!(
            account.backend != Backend::Claude || account.managed.is_none() || account.catalog_valid(),
            "model_unavailable: catálogo sem prova desta revisão."
        );
        let model = account
            .models
            .iter()
            .find(|m| m.id == *model)
            .ok_or_else(|| anyhow!("model_unavailable: modelo não comprovado nesta conta."))?;
        for (key, value) in &bot.model_options {
            ensure!(
                model.options.iter().any(|o| o.id == *key && o.values.contains(value)),
                "model_option_unavailable: opção não comprovada."
            );
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn create(store: &Store, backend: Backend, name: &str, kind: &str, request: &str) -> Result<Value> {
    create_with(store, backend, name, kind, request, |registry| registry.persist(store))
}
pub(crate) fn create_with(
    store: &Store,
    backend: Backend,
    name: &str,
    kind: &str,
    request: &str,
    persist: impl FnOnce(&Registry) -> Result<()>,
) -> Result<Value> {
    request_id(request)?;
    let label = label(name)?;
    ensure!(matches!(kind, "cli" | "api"), "invalid_field: tipo inválido.");
    let mut registry = Registry::load(store)?;
    if let Some(account) =
        registry.accounts.iter().find(|a| a.managed.as_ref().is_some_and(|m| m.request_id == request))
    {
        ensure!(
            account.backend == backend
                && account.managed.as_ref().is_some_and(|m| m.label == label)
                && matches!(
                    (&account.launch, kind),
                    (LaunchReference::CliProfile { .. }, "cli") | (LaunchReference::VaultProfile { .. }, "api")
                ),
            "request_conflict: pedido já utilizado."
        );
        return registry.dto_for(store, &account.id);
    }
    ensure!(registry.accounts.len() < 64, "account_limit: limite de contas atingido.");
    let id = registry.fingerprint(backend, "managed", &uuid::Uuid::new_v4().to_string());
    let profile = new_profile(store, &id, backend)?;
    let launch = if kind == "cli" {
        LaunchReference::CliProfile { executable: PathBuf::new(), directory: profile.clone() }
    } else {
        LaunchReference::VaultProfile { slot: format!("agent-account:{id}") }
    };
    registry.accounts.push(Account {
        id: id.clone(),
        backend,
        billing: Billing::Unverified,
        state: State::Unverified,
        launch,
        scope: super::AuthorizedScope::Owner,
        quota_identity: None,
        verified_at: None,
        subscription_proof: None,
        models: vec![],
        hidden_model_ids: Vec::new(),
        managed: Some(Metadata {
            label,
            revision: 1,
            generation: 0,
            enabled: false,
            request_id: request.into(),
            principal: None,
            profile: Some(profile),
            setup_request: None,
            catalog: None,
        }),
    });
    persist(&registry)?;
    registry.dto_for(store, &id)
}

pub(crate) fn editor_accounts(store: &Store) -> Result<Value> {
    Ok(Registry::load(store)?.response_with_bots(store)?["accounts"].clone())
}
pub(crate) fn editor_runtime(store: &Store, bot: &BotConfig) -> Result<Value> {
    let Some(id) = bot.fork_editor.engine_account_id.as_deref() else { return Ok(Value::Null) };
    let registry = Registry::load(store)?;
    Ok(registry.account(id).map_or_else(|_|json!({"engineAccountId":id,"state":"unavailable"}), |account| json!({"engineAccountId":id,"state":account.dto().state,"revision":account.managed.as_ref().map_or(1,|m|m.revision)})))
}

pub(crate) async fn adapter_commands(backend: &str) -> Result<Vec<String>> {
    let names: &[&str] = if backend == "codex" { &["codex-acp"] } else { &["claude-agent-acp", "claude-code-acp"] };
    for name in names {
        if let Some(path) = crate::agent::backends::which(name) {
            return Ok(vec![crate::shell::quote(&path.to_string_lossy())]);
        }
    }
    Ok(crate::agent::backends::launch_candidates(backend, |_, _| {})
        .await?
        .iter()
        .map(crate::agent::registry::Cmd::acp)
        .collect())
}

pub(crate) fn attach_bots(store: &Store, value: &mut Value) -> Result<()> {
    for item in value["accounts"].as_array_mut().into_iter().flatten() {
        item["botIds"] = json!(bot_ids(store, item["id"].as_str().unwrap_or_default())?);
    }
    Ok(())
}

pub(crate) fn remove_profile_home(home: &Path) -> Result<()> {
    if crate::fork::motor_exec::configured() {
        crate::fork::motor_roots::profile("remover", &[home.as_os_str()])?;
    } else {
        std::fs::remove_dir_all(home)?;
    }
    Ok(())
}
