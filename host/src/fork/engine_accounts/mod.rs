//! AC: um registro do Hub, sem credencial por bot e sem escolher/executar motores.
pub(crate) mod managed;
pub(crate) mod probe;
pub(crate) static MUTATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

use crate::{LockExt, api::devices::Caller, hub::Hub, store::Store};
use anyhow::{Result, anyhow, bail};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    future::Future,
    path::PathBuf,
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

pub(crate) const SLOT: &str = "fork.engine-accounts.v1";
const CACHE_TTL: Duration = Duration::from_secs(5 * 60);
// Quem esperou uma sondagem reaproveita seu resultado, inclusive em refresh forçado.
static DISCOVERY: tokio::sync::Mutex<Option<DiscoveryCache>> = tokio::sync::Mutex::const_new(None);

static EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) fn invalidate() {
    EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

struct DiscoveryCache {
    epoch: u64,
    pub(crate) hub: Weak<Hub>,
    pub(crate) completed_at: Instant,
    pub(crate) response: Value,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Backend {
    Claude,
    Codex,
    Grok,
}

impl Backend {
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Grok => "grok",
        }
    }

    fn brand(self) -> &'static str {
        match self {
            Self::Claude => "Anthropic",
            Self::Codex => "OpenAI",
            Self::Grok => "xAI",
        }
    }

    /// Variável que aponta o programa oficial para a pasta de perfil da conta.
    pub(crate) fn profile_variable(self) -> &'static str {
        match self {
            Self::Claude => "CLAUDE_CONFIG_DIR",
            Self::Codex => "CODEX_HOME",
            Self::Grok => "GROK_HOME",
        }
    }

    /// Nome da pasta de perfil dentro da home própria da conta.
    pub(crate) fn profile_directory(self) -> &'static str {
        match self {
            Self::Claude => ".claude",
            Self::Codex => ".codex",
            Self::Grok => ".grok",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Billing {
    Subscription,
    PaidApi,
    Unverified,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Connected,
    Unverified,
    Unavailable,
    Disconnected,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ModelOption {
    pub(crate) id: String,
    pub(crate) values: Vec<String>,
    pub(crate) default: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Model {
    pub(crate) id: String,
    pub(crate) options: Vec<ModelOption>,
}

/// Referência privada. O launcher isolado será consumidor na 17.3/20.2, sem mudar login global.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum LaunchReference {
    CliProfile { executable: PathBuf, directory: PathBuf },
    VaultProfile { slot: String },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthorizedScope {
    Owner,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SubscriptionProof {
    ClaudeAuthStatus {
        plan: String,
    },
    CodexAccountRead {
        plan: String,
    },
    /// `grok models` respondeu com o login de sessão do perfil isolado (sem chave de API no ambiente).
    GrokModels {
        login: String,
    },
}

/// Persistência privada; não implementar Serialize do DTO reaproveitando este tipo.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Account {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) managed: Option<managed::Metadata>,
    pub(crate) id: String,
    pub(crate) backend: Backend,
    pub(crate) billing: Billing,
    pub(crate) state: State,
    pub(crate) launch: LaunchReference,
    pub(crate) scope: AuthorizedScope,
    pub(crate) quota_identity: Option<String>,
    pub(crate) verified_at: Option<i64>,
    pub(crate) subscription_proof: Option<SubscriptionProof>,
    pub(crate) models: Vec<Model>,
    // MX-6: ids ocultados pelo dono. Ausente em registros antigos; nunca filtrado de `models`,
    // revisão ou execução — ocultar só muda a listagem/escolha nova no app.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) hidden_model_ids: Vec<String>,
}

impl Account {
    pub(crate) fn subscription_verified(&self) -> bool {
        self.billing == Billing::Subscription
            && self.state == State::Connected
            && self.verified_at.is_some()
            && self.subscription_proof.is_some()
            && self.quota_identity.is_some()
            && matches!(self.launch, LaunchReference::CliProfile { .. })
    }

    pub(crate) fn dto(&self) -> AccountDto {
        AccountDto {
            id: self.id.clone(),
            revision: self.managed.as_ref().map_or(1, |m| m.revision),
            origin: if self.managed.is_some() { "managed" } else { "legacy" },
            bot_ids: Vec::new(),
            label: self.managed.as_ref().map_or_else(
                || {
                    match (&self.launch, self.backend) {
                        (LaunchReference::CliProfile { .. }, Backend::Claude) => "Claude — login deste computador",
                        (LaunchReference::CliProfile { .. }, Backend::Codex) => "Codex — login deste computador",
                        (LaunchReference::CliProfile { .. }, Backend::Grok) => "Grok Build — login deste computador",
                        (LaunchReference::VaultProfile { .. }, Backend::Claude) => {
                            "Claude Code — configuração no cofre"
                        }
                        (LaunchReference::VaultProfile { .. }, Backend::Codex) => "Codex — configuração no cofre",
                        (LaunchReference::VaultProfile { .. }, Backend::Grok) => "Grok Build — configuração no cofre",
                    }
                    .to_owned()
                },
                |m| m.label.clone(),
            ),
            brand: self.backend.brand(),
            backend: self.backend,
            billing: self.billing,
            // Nenhuma assinatura ambígua aparece conectada/elegível aos consumidores.
            state: if self.billing == Billing::Subscription
                && !self.subscription_verified()
                && self.state == State::Connected
            {
                State::Unverified
            } else {
                self.state
            },
            models: if self.backend == Backend::Claude && self.managed.is_some() && !self.catalog_valid() {
                Vec::new()
            } else {
                self.models.clone()
            },
            hidden_model_ids: self.hidden_model_ids.clone(),
        }
    }
}

/// Allowlist do protocolo: sem identidade pessoal, referência de cofre, caminho ou stderr.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountDto {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) revision: u64,
    pub(crate) origin: &'static str,
    pub(crate) bot_ids: Vec<String>,
    pub(crate) brand: &'static str,
    pub(crate) backend: Backend,
    pub(crate) billing: Billing,
    pub(crate) state: State,
    pub(crate) models: Vec<Model>,
    // Sempre presente na resposta do Hub novo; a ausência marca um Hub antigo sem o recurso.
    pub(crate) hidden_model_ids: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Registry {
    // Sal estável e privado: nem e-mail nem hash de e-mail enumerável são enviados ao app.
    pub(crate) salt: String,
    pub(crate) accounts: Vec<Account>,
}

impl Registry {
    pub(crate) fn load(store: &Store) -> Result<Self> {
        Self::load_db(&store.db.locked())
    }

    /// Leitura síncrona para guardas Cloud → DB, sem Store recursivo ou sondagem.
    pub(crate) fn load_db(db: &rusqlite::Connection) -> Result<Self> {
        let raw: Option<String> = db.query_row("SELECT v FROM kv WHERE k=?1", [SLOT], |r| r.get(0)).optional()?;
        match raw {
            None => Ok(Self { salt: uuid::Uuid::new_v4().to_string(), accounts: Vec::new() }),
            Some(raw) => {
                let registry: Self = serde_json::from_str(&raw).map_err(|_| {
                    anyhow!("accounts_unavailable: registro de contas inválido; configuração preservada.")
                })?;
                if uuid::Uuid::parse_str(&registry.salt).is_err()
                    || registry.accounts.iter().any(|account| {
                        !valid_id(&account.id) || account.quota_identity.as_deref().is_some_and(|id| !valid_id(id))
                    })
                {
                    bail!("accounts_unavailable: registro de contas inválido; configuração preservada.");
                }
                Ok(registry)
            }
        }
    }

    pub(crate) fn fingerprint(&self, backend: Backend, namespace: &str, identity: &str) -> String {
        let mut hash = Sha256::new();
        for part in [&self.salt, backend.id(), namespace, identity] {
            hash.update(part.as_bytes());
            hash.update([0]);
        }
        let mut id = String::from("ac_");
        for byte in hash.finalize() {
            let _ = write!(id, "{byte:02x}");
        }
        id
    }

    fn refresh(&mut self, observations: Vec<probe::Observation>) {
        let previous = self.accounts.clone();
        for account in &mut self.accounts {
            if account.managed.is_some() {
                continue;
            }
            account.state = State::Disconnected;
            account.models.clear();
            account.verified_at = None;
        }
        for observation in observations {
            let catalog_observed = observation.catalog_observed;
            if let Some(index) =
                self.accounts.iter().position(|a| a.managed.is_some() && a.launch == observation.launch)
            {
                self.apply_managed(index, observation);
                continue;
            }
            if observation.identity.is_none() && matches!(observation.launch, LaunchReference::CliProfile { .. }) {
                match observation.state {
                    State::Unavailable => {
                        for account in &mut self.accounts {
                            if observation.expected_account.as_ref().is_none_or(|id| id == &account.id)
                                && account.backend == observation.backend
                                && account.managed.is_none()
                                && matches!(account.launch, LaunchReference::CliProfile { .. })
                                && previous.iter().any(|old| old.id == account.id && old.state != State::Disconnected)
                            {
                                account.state = State::Unavailable;
                            }
                        }
                        continue;
                    }
                    State::Disconnected => continue,
                    _ => {}
                }
            }
            let quota =
                observation.identity.as_ref().map(|identity| self.fingerprint(observation.backend, "quota", identity));
            let identity = quota.as_deref().unwrap_or(&observation.source);
            let id = self.fingerprint(observation.backend, "account", identity);
            if observation.expected_account.as_ref().is_some_and(|expected| expected != &id) {
                continue;
            }
            let verified = observation.billing != Billing::Unverified && quota.is_some();
            let mut account = Account {
                managed: None,
                id: id.clone(),
                backend: observation.backend,
                billing: observation.billing,
                state: if observation.state == State::Connected && !verified {
                    State::Unverified
                } else {
                    observation.state
                },
                launch: observation.launch,
                scope: AuthorizedScope::Owner,
                quota_identity: quota,
                verified_at: verified.then(crate::store::now_ms),
                subscription_proof: observation.subscription_proof,
                models: observation.models,
                hidden_model_ids: Vec::new(),
            };
            if verified && let Some(existing) = previous.iter().find(|existing| existing.id == id) {
                if !catalog_observed
                    && account.models.is_empty()
                    && account.launch == existing.launch
                    && account.subscription_proof == existing.subscription_proof
                {
                    account.models.clone_from(&existing.models);
                }
                // Uma prova idêntica não provoca escrita só por causa do relógio.
                account.verified_at = existing.verified_at;
                if existing.verified_at.is_none() || account != *existing {
                    account.verified_at = Some(crate::store::now_ms());
                }
            }
            match self.accounts.iter_mut().find(|existing| existing.id == id) {
                Some(existing) => *existing = account,
                None => self.accounts.push(account),
            }
        }
    }

    fn response(&self) -> Value {
        serde_json::json!({"accounts":self.accounts.iter().map(Account::dto).collect::<Vec<_>>()})
    }
}

fn valid_id(id: &str) -> bool {
    id.len() == 67 && id.starts_with("ac_") && id[3..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub async fn call(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    let authority = super::engine_setup::access::resolve(hub, caller)?;
    let query: Query = serde_json::from_value(body)
        .map_err(|_| anyhow!("invalid_field: a consulta de contas aceita somente refresh booleano opcional."))?;
    if crate::environment::Environment::is_open() {
        super::engine_setup::require_local(caller)?;
        let mut registry = Registry::load(&hub.store)?;
        registry.accounts.retain(super::engine_setup::is_open_account);
        return registry.response_with_bots(&hub.store);
    }
    let value = read_authorized(&DISCOVERY, hub, query.refresh, probe::discover(&hub.store), &authority)
        .await
        .map_err(|_| anyhow!("accounts_unavailable: não foi possível consultar contas; configuração preservada."))?;
    authority.require_current(hub)?;
    Ok(value)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    #[serde(default)]
    pub(crate) refresh: bool,
}

#[cfg(test)]
async fn read(
    cache: &tokio::sync::Mutex<Option<DiscoveryCache>>,
    hub: &Arc<Hub>,
    refresh: bool,
    discovery: impl Future<Output = Vec<probe::Observation>>,
) -> Result<Value> {
    read_authorized(cache, hub, refresh, discovery, &super::engine_setup::access::Authority::Local).await
}
async fn read_authorized(
    cache: &tokio::sync::Mutex<Option<DiscoveryCache>>,
    hub: &Arc<Hub>,
    refresh: bool,
    discovery: impl Future<Output = Vec<probe::Observation>>,
    authority: &super::engine_setup::access::Authority,
) -> Result<Value> {
    let requested_at = Instant::now();
    let mut cache = cache.lock().await;
    authority.require_current(hub)?;
    if let Some(cached) = cache.as_ref()
        && cached.epoch == EPOCH.load(std::sync::atomic::Ordering::Relaxed)
        && cached.hub.ptr_eq(&Arc::downgrade(hub))
        && ((!refresh && cached.completed_at.elapsed() < CACHE_TTL) || cached.completed_at >= requested_at)
    {
        let mut response = cached.response.clone();
        managed::attach_bots(&hub.store, &mut response)?;
        return Ok(response);
    }
    let _mutation = MUTATION.lock().await;
    authority.require_current(hub)?;
    let mut registry = Registry::load(&hub.store)?;
    let mut observations = discovery.await;
    for account in &registry.accounts {
        if account.managed.as_ref().is_some_and(|m| m.enabled)
            && !crate::fork::engine_setup::in_setup(&hub.store, &account.id)
            && let LaunchReference::CliProfile { executable, directory } = &account.launch
        {
            if managed::validate_profile(&hub.store, &account.id, directory).is_err() {
                let mut observation = probe::Observation::cli(account.backend, executable);
                observation.launch = account.launch.clone();
                observation.state = State::Unavailable;
                observations.push(observation);
                continue;
            }
            let mut observation = probe::inspect(account.backend, executable, directory).await;
            // Registros anteriores à prova explícita ou com revisão/perfil alterados precisam de catálogo novo.
            if account.backend == Backend::Claude
                && !account.catalog_valid()
                && observation.state == State::Connected
                && observation.identity.as_deref().is_some_and(|identity| {
                    account.managed.as_ref().and_then(|m| m.principal.as_ref())
                        == Some(&registry.fingerprint(account.backend, "principal", managed::principal(identity)))
                })
                && let Ok(models) = super::engine_setup::claude_catalog(executable, directory).await
            {
                observation.models = models;
                observation.catalog_observed = true;
            }
            observations.push(observation);
        }
    }
    registry.refresh(observations);
    managed::vault_states(&hub.store, &mut registry)?;
    let serialized = serde_json::to_string(&registry)?;
    if hub.store.kv_read(SLOT)?.as_deref() != Some(&serialized) {
        authority.persist(hub, &registry)?;
    }
    let response = registry.response_with_bots(&hub.store)?;
    authority.require_current(hub)?;
    *cache = Some(DiscoveryCache {
        epoch: EPOCH.load(std::sync::atomic::Ordering::Relaxed),
        hub: Arc::downgrade(hub),
        completed_at: Instant::now(),
        response: response.clone(),
    });
    Ok(response)
}

#[cfg(test)]
pub(crate) mod tests;

/// Snapshot interno estrito, sem sondagem, rede, desbloqueio do cofre ou referência de lançamento.
/// Titular/escopo são o único dono (D118); identidade de cota é opaca, nunca enviada à UI.
pub(crate) fn read_for_functions(store: &Store) -> Result<Value> {
    let registry = Registry::load(store)?;
    Ok(serde_json::json!({"accounts": registry.accounts.iter().map(|account| {
        let mut dto = serde_json::to_value(account.dto())?;
        dto["owner"] = "owner".into();
        dto["quotaIdentity"] = serde_json::to_value(&account.quota_identity)?;
        dto["subscriptionVerified"] = account.subscription_verified().into();
        Ok(dto)
    }).collect::<Result<Vec<Value>>>()?}))
}

/// Descobre apenas o login Claude local para o trabalhador; não inicia Codex
/// nem toca nas contas gerenciadas ou nos slots dos outros provedores.
pub(crate) async fn discover_background(hub: &Arc<Hub>) -> Result<()> {
    discover_background_with(
        hub,
        super::background_policy::enforce(false),
        || crate::agent::backends::which("claude"),
        false,
    )
    .await
}

/// Conta que a sondagem de reserva de uma função parada confere.
#[derive(Debug, PartialEq, Eq)]
enum ProbeTarget {
    /// Login Claude deste computador (ou nenhuma conta escolhida ainda).
    Local,
    /// Conta gerenciada escolhida para a função.
    Managed(String),
}

/// A conta escolhida para a função: personalizada, vínculo automático ou conta pedida.
fn probe_target(
    store: &Store,
    registry: &Registry,
    function: crate::fork::modelos_funcao::catalogo::FunctionId,
) -> ProbeTarget {
    use crate::fork::modelos_funcao::config::{self, Selection};
    let document = config::load(store).ok();
    let chosen = document.as_ref().and_then(|document| match document.functions.get(&function)? {
        Selection::Custom { binding } | Selection::Automatic { binding: Some(binding), .. } => {
            Some(binding.account_id.as_str())
        }
        Selection::Automatic { account_id, .. } => account_id.as_deref(),
        Selection::ManagedInVoice => None,
    });
    match chosen.and_then(|id| registry.accounts.iter().find(|account| account.id == id)) {
        Some(account) if account.managed.is_some() => ProbeTarget::Managed(account.id.clone()),
        _ => ProbeTarget::Local,
    }
}

/// MEX-1355: reserva da função parada. Sonda só a conta escolhida para ela, com
/// o catálogo do mesmo caminho que a cadastrou: conta gerenciada pelo adaptador
/// das contas gerenciadas (e o cofre, sem processo), login local pelo adaptador
/// de fundo, mesmo quando outra conta já anuncia Haiku. A gravação do cadastro
/// dispara o evento que tira a função da parada.
pub(crate) async fn probe_background_choice(
    hub: &Arc<Hub>,
    function: crate::fork::modelos_funcao::catalogo::FunctionId,
) -> Result<()> {
    super::background_policy::enforce(false).await.map_err(|_| super::background_notice::Problem::Policy)?;
    let target = probe_target(&hub.store, &Registry::load(&hub.store)?, function);
    match target {
        ProbeTarget::Local => {
            discover_background_with(hub, std::future::ready(Ok(())), || crate::agent::backends::which("claude"), true)
                .await
        }
        ProbeTarget::Managed(id) => probe_managed(hub, &id).await,
    }
}

async fn probe_managed(hub: &Arc<Hub>, id: &str) -> Result<()> {
    let _guard = MUTATION.lock().await;
    let mut registry = Registry::load(&hub.store)?;
    let Some(index) = registry.accounts.iter().position(|account| account.id == id) else { return Ok(()) };
    let account = &registry.accounts[index];
    if account.backend == Backend::Claude
        && account.managed.as_ref().is_some_and(|meta| meta.enabled)
        && !crate::fork::engine_setup::in_setup(&hub.store, id)
        && let LaunchReference::CliProfile { executable, directory } = account.launch.clone()
    {
        let expected = account.managed.as_ref().and_then(|meta| meta.principal.clone());
        let mut observation = if managed::validate_profile(&hub.store, id, &directory).is_err() {
            let mut observation = probe::Observation::cli(Backend::Claude, &executable);
            observation.launch =
                LaunchReference::CliProfile { executable: executable.clone(), directory: directory.clone() };
            observation.state = State::Unavailable;
            observation
        } else {
            probe::inspect(Backend::Claude, &executable, &directory).await
        };
        if observation.state == State::Connected
            && observation.identity.as_deref().is_some_and(|identity| {
                expected.as_ref()
                    == Some(&registry.fingerprint(Backend::Claude, "principal", managed::principal(identity)))
            })
        {
            match super::engine_setup::claude_catalog(&executable, &directory).await {
                Ok(models) => {
                    observation.models = models;
                    observation.catalog_observed = true;
                }
                Err(error)
                    if error
                        .downcast_ref::<super::background_notice::Problem>()
                        .is_some_and(|problem| matches!(problem, super::background_notice::Problem::Policy)) =>
                {
                    return Err(error);
                }
                Err(_) => {}
            }
        }
        registry.apply_managed(index, observation);
    }
    managed::vault_states(&hub.store, &mut registry)?;
    if hub.store.kv_read(SLOT)?.as_deref() != Some(&serde_json::to_string(&registry)?) {
        crate::fork::engine_setup::access::Authority::Local.persist(hub, &registry)?;
        invalidate();
    }
    Ok(())
}

/// `force`: a função está parada e a reserva precisa do catálogo atual mesmo
/// com um login local que já anuncia Haiku (escolha de outro modelo ou opção).
async fn discover_background_with(
    hub: &Arc<Hub>,
    policy: impl std::future::Future<Output = Result<()>>,
    executable: impl FnOnce() -> Option<PathBuf>,
    force: bool,
) -> Result<()> {
    policy.await.map_err(|_| super::background_notice::Problem::Policy)?;
    let Some(executable) = executable() else { return Ok(()) };
    let _guard = MUTATION.lock().await;
    let mut registry = Registry::load(&hub.store)?;
    if !force
        && registry.accounts.iter().any(|account| {
            account.managed.is_none()
                && account.backend == Backend::Claude
                && account.subscription_verified()
                && account.models.iter().any(|model| model.id == "haiku")
        })
    {
        return Ok(());
    }
    let template = probe::Observation::cli(Backend::Claude, &executable);
    let LaunchReference::CliProfile { directory, .. } = &template.launch else { return Ok(()) };
    let mut observation = probe::inspect(Backend::Claude, &executable, directory).await;
    if observation.state == State::Connected && observation.billing == Billing::Subscription {
        match super::engine_setup::background_claude_catalog(&executable, directory).await {
            Ok(models) => {
                observation.models = models;
                observation.catalog_observed = true;
            }
            Err(error)
                if error.downcast_ref::<super::background_notice::Problem>().is_some_and(|problem| {
                    matches!(
                        problem,
                        super::background_notice::Problem::Version | super::background_notice::Problem::Policy
                    )
                }) =>
            {
                return Err(error);
            }
            Err(_) => {}
        }
    }
    let saved: Vec<_> = registry
        .accounts
        .iter()
        .filter(|account| {
            account.managed.is_none()
                && !(account.backend == Backend::Claude && matches!(account.launch, LaunchReference::CliProfile { .. }))
        })
        .cloned()
        .collect();
    registry.refresh(vec![observation]);
    for account in saved {
        if let Some(current) = registry.accounts.iter_mut().find(|current| current.id == account.id) {
            *current = account;
        }
    }
    crate::fork::engine_setup::access::Authority::Local.persist(hub, &registry)?;
    invalidate();
    Ok(())
}
