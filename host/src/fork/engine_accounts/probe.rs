//! Consultas nativas sem prompt, login, leitura de arquivo de auth ou saída crua no protocolo/log.
use super::{Backend, Billing, LaunchReference, Model, ModelOption, State, SubscriptionProof};
use crate::fork::motor_exec::Child;
use crate::{LockExt, agent::backends, store::Store};
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout, Command},
};

pub(crate) struct Observation {
    pub expected_account: Option<String>,
    pub backend: Backend,
    pub source: String,
    pub identity: Option<String>,
    pub billing: Billing,
    pub state: State,
    pub launch: LaunchReference,
    pub models: Vec<Model>,
    pub catalog_observed: bool,
    pub subscription_proof: Option<SubscriptionProof>,
}

impl Observation {
    pub(crate) fn cli(backend: Backend, executable: &Path) -> Self {
        let (variable, default) = (backend.profile_variable(), backend.profile_directory());
        let directory = if crate::fork::motor_exec::configured() {
            Path::new(crate::fork::motor_roots::MOTOR).join(default)
        } else {
            std::env::var_os(variable)
                .map_or_else(|| dirs::home_dir().unwrap_or_default().join(default), std::path::PathBuf::from)
        };
        Self {
            expected_account: None,
            backend,
            source: format!("{}:cli", backend.id()),
            identity: None,
            billing: Billing::Unverified,
            state: State::Unverified,
            launch: LaunchReference::CliProfile { executable: executable.into(), directory },
            models: Vec::new(),
            catalog_observed: false,
            subscription_proof: None,
        }
    }
}

/// Não desbloqueia o cofre; conteúdo já acessível não comprova credencial/cobrança.
pub(crate) async fn discover(store: &Store) -> Vec<Observation> {
    discover_with(store, |backend| backends::which(backend.id())).await
}
pub(super) async fn discover_with(
    store: &Store,
    which: impl Fn(Backend) -> Option<std::path::PathBuf>,
) -> Vec<Observation> {
    let mut observations = Vec::new();
    let migrated = if crate::fork::motor_exec::configured() {
        super::Registry::load(store).ok().map(|r| r.accounts).unwrap_or_default()
    } else {
        Vec::new()
    };
    for backend in [Backend::Claude, Backend::Codex] {
        let legacy: Vec<_> =
            migrated.iter().filter(|a| a.backend == backend && super::managed::migrated_legacy(a)).collect();
        for account in &legacy {
            if let LaunchReference::CliProfile { executable, directory } = &account.launch {
                let mut observation = if super::managed::validate_profile(store, &account.id, directory).is_ok() {
                    inspect(backend, executable, directory).await
                } else {
                    let mut failed = Observation::cli(backend, executable);
                    failed.launch = account.launch.clone();
                    failed.state = State::Unavailable;
                    failed
                };
                observation.expected_account = Some(account.id.clone());
                observations.push(observation);
            }
        }
        if let Some(executable) = which(backend).filter(|_| legacy.is_empty()) {
            let mut observation = Observation::cli(backend, &executable);
            let result = tokio::time::timeout(Duration::from_secs(25), async {
                match backend {
                    Backend::Claude => claude(&executable, &mut observation).await,
                    Backend::Codex => codex(&executable, &mut observation).await,
                    Backend::Grok => grok(&mut observation).await,
                }
            })
            .await;
            if !matches!(result, Ok(Ok(()))) {
                observation.state = State::Unavailable;
                observation.models.clear();
            }
            if observation.state != State::Disconnected {
                observations.push(observation);
            }
        }
        let slot = format!("agent-env:{}", backend.id());
        if vault_has_content(store, &slot) {
            observations.push(Observation {
                expected_account: None,
                backend,
                source: slot.clone(),
                identity: None,
                billing: Billing::Unverified,
                state: State::Unverified,
                launch: LaunchReference::VaultProfile { slot },
                models: Vec::new(),
                catalog_observed: false,
                subscription_proof: None,
            });
        }
    }
    observations
}

pub(crate) fn vault_has_content(store: &Store, slot: &str) -> bool {
    // Não migrar plaintext nem iniciar acesso ao chaveiro durante uma leitura de contas.
    if store.secret_key.locked().is_none()
        || !store.kv_read(slot).ok().flatten().is_some_and(|raw| raw.starts_with("vault:v1:"))
    {
        return false;
    }
    let Ok(Some(raw)) = crate::market::vault::read(store, slot) else {
        return false;
    };
    let raw = zeroize::Zeroizing::new(raw);
    serde_json::from_str::<std::collections::BTreeMap<String, String>>(&raw)
        .is_ok_and(|values| values.values().any(|value| !value.trim().is_empty()))
}

pub(crate) fn command(observation: &Observation) -> Command {
    command_at(observation, &crate::service::data_dir())
}

pub(crate) fn command_at(observation: &Observation, cwd: &Path) -> Command {
    let LaunchReference::CliProfile { executable, directory } = &observation.launch else { unreachable!() };
    if crate::fork::motor_exec::configured() {
        let cwd = if cwd == crate::service::data_dir() { directory.as_path() } else { cwd };
        let mut command = crate::fork::motor_exec::command(executable, cwd);
        command
            .env_clear()
            .envs(crate::fork::engine_setup::profile_environment(directory, observation.backend))
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        return command;
    }
    let mut command = crate::fork::motor_exec::command(executable, cwd);
    command
        .env_clear()
        .envs(crate::agent::environment::inherited(std::env::vars_os()))
        .current_dir(crate::fork::motor_exec::hub_cwd(cwd))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .kill_on_drop(true);
    command.env(observation.backend.profile_variable(), directory);
    // Somente perfis criados pelo Hub recebem HOME próprio; o legado conserva seu HOME.
    if directory.file_name().is_some_and(|name| name == observation.backend.profile_directory())
        && directory.parent().is_some_and(|p| p.join(".donnabot-profile").is_file())
    {
        command.env("HOME", directory.parent().unwrap_or(directory)).env("DISABLE_AUTOUPDATER", "1");
    }
    if directory.parent().is_some_and(|p| p.join(".donnabot-profile").is_file()) {
        let mut managed = crate::fork::engine_setup::probe_command(executable, directory, observation.backend);
        if crate::fork::motor_exec::configured() && cwd != crate::service::data_dir() {
            // Fundo tem cwd próprio; conserva o ambiente fechado do perfil gerenciado.
            managed = crate::fork::motor_exec::command(executable, cwd);
            managed
                .env_clear()
                .envs(crate::fork::engine_setup::profile_environment(directory, observation.backend))
                .current_dir(crate::fork::motor_exec::hub_cwd(cwd))
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .stdout(Stdio::piped())
                .kill_on_drop(true);
        }
        return managed;
    }
    command
}

pub(crate) async fn inspect(backend: Backend, executable: &Path, directory: &Path) -> Observation {
    if crate::fork::motor_exec::configured() && !crate::fork::motor_install::executable(executable) {
        return Observation {
            launch: LaunchReference::CliProfile { executable: executable.into(), directory: directory.into() },
            state: State::Unavailable,
            ..Observation::cli(backend, executable)
        };
    }
    inspect_timeout(backend, executable, directory, Duration::from_secs(25)).await
}

async fn inspect_timeout(backend: Backend, executable: &Path, directory: &Path, timeout: Duration) -> Observation {
    let mut observation = Observation::cli(backend, executable);
    observation.launch = LaunchReference::CliProfile { executable: executable.into(), directory: directory.into() };
    let result = tokio::time::timeout(timeout, async {
        match backend {
            Backend::Claude => claude(executable, &mut observation).await,
            Backend::Codex => codex(executable, &mut observation).await,
            Backend::Grok => grok(&mut observation).await,
        }
    })
    .await;
    if !matches!(result, Ok(Ok(()))) {
        observation.state = State::Unavailable;
        observation.models.clear();
    }
    observation
}

async fn claude(_executable: &Path, observation: &mut Observation) -> Result<()> {
    let mut child = crate::fork::motor_exec::spawn(command(observation).args(["auth", "status", "--json"]))?;
    let _group = crate::fork::engine_setup::ProcessGroup(child.id());
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("status indisponível"))?
        .take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > 256 * 1024 {
        bail!("status indisponível");
    }
    let success = child.wait().await?.success();
    let status: Value = serde_json::from_slice(&bytes)?;
    claude_status(observation, &status, success);
    Ok(())
}

pub(crate) fn claude_status(observation: &mut Observation, status: &Value, success: bool) {
    if status["loggedIn"] == false {
        observation.state = State::Disconnected;
        return;
    }
    if !success || status["loggedIn"] != true {
        observation.state = State::Unavailable;
        return;
    }
    observation.identity = identity(status, "email").map(|email| {
        // Organização também faz parte da conta de cobrança do Claude.
        format!("{email}\0{}", status["orgId"].as_str().unwrap_or_default())
    });
    observation.billing = match (status["authMethod"].as_str(), status["apiProvider"].as_str()) {
        (Some("claude.ai"), Some("firstParty"))
            if matches!(status["subscriptionType"].as_str(), Some("pro" | "max" | "team" | "enterprise")) =>
        {
            Billing::Subscription
        }
        (Some("api_key"), Some("firstParty")) => Billing::PaidApi,
        _ => Billing::Unverified,
    };
    observation.state = if observation.billing == Billing::Unverified { State::Unverified } else { State::Connected };
    if observation.billing == Billing::Subscription {
        observation.subscription_proof = Some(SubscriptionProof::ClaudeAuthStatus {
            plan: status["subscriptionType"].as_str().unwrap_or_default().into(),
        });
    }
    // auth status não anuncia modelos/esforço. Não inventar catálogo a partir da marca/plano.
}

/// `grok models` não gera mensagem nem gasto. O Hub só lê o texto de estado do programa; o token da conta fica
/// no perfil isolado e nunca é aberto por ele.
async fn grok(observation: &mut Observation) -> Result<()> {
    let mut child = crate::fork::motor_exec::spawn(command(observation).arg("models"))?;
    let _group = crate::fork::engine_setup::ProcessGroup(child.id());
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("status indisponível"))?
        .take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > 256 * 1024 {
        bail!("status indisponível");
    }
    let success = child.wait().await?.success();
    grok_status(observation, success, &String::from_utf8_lossy(&bytes));
    Ok(())
}

pub(crate) fn grok_status(observation: &mut Observation, success: bool, output: &str) {
    let signed_in =
        backends::harness("grok").and_then(|h| h.signed_in).is_some_and(|check| (check.ok)(success, output));
    if !signed_in {
        let not_authenticated = output.contains("You are not authenticated.");
        tracing::warn!(
            motivo = if not_authenticated { "sem login" } else { "comando falhou ou saída inesperada" },
            "grok: login não confirmado"
        );
        observation.state = if not_authenticated { State::Disconnected } else { State::Unavailable };
        return;
    }
    // O programa não informa a pessoa dona do login sem abrir a credencial, que o Hub nunca lê: a identidade
    // é a da própria conta do Mexerica (pasta de perfil), estável entre reconexões.
    let LaunchReference::CliProfile { directory, .. } = &observation.launch else { return };
    let Some(owner) = directory.ancestors().find_map(|path| {
        let name = path.file_name()?.to_str()?;
        (path.parent()?.file_name()? == "accounts" && super::valid_id(name)).then(|| name.to_owned())
    }) else {
        observation.state = State::Unverified;
        return;
    };
    observation.identity = Some(format!("grok-profile:{owner}"));
    // Lista fechada: só o texto exato do login de sessão vira assinatura; qualquer outro login (chave de API, outro
    // método ou texto novo) fica sem comprovação.
    let session = output.lines().any(|line| line.trim() == "You are logged in with grok.com.");
    observation.billing = if session { Billing::Subscription } else { Billing::Unverified };
    observation.state = if session { State::Connected } else { State::Unverified };
    if session {
        observation.subscription_proof = Some(SubscriptionProof::GrokModels { login: "session".into() });
    }
}

async fn codex(_executable: &Path, observation: &mut Observation) -> Result<()> {
    let mut process = command(observation);
    process.args(["app-server"]).stdin(Stdio::piped());
    let child = crate::fork::motor_exec::spawn(&mut process)?;
    let mut rpc = Rpc::new(child)?;
    let result = async {
        rpc.call(1, "initialize", json!({"clientInfo":{"name":"donnabot-accounts","version":"1"}})).await?;
        rpc.write(json!({"method":"initialized"})).await?;
        // O CLI ainda pode renovar token/cache como no uso normal. O Hub não lê/escreve auth.json.
        let account = rpc.call(2, "account/read", json!({"refreshToken":false})).await?;
        codex_account(observation, &account);
        if observation.billing == Billing::Subscription && observation.identity.is_some() {
            // Nenhum thread/start, turn/start ou prompt. Só o catálogo do processo autenticado.
            if let Ok(catalog) = rpc.call(3, "model/list", json!({"includeHidden":false})).await {
                observation.models = codex_models(&catalog);
                observation.catalog_observed = true;
            }
        }
        Ok(())
    }
    .await;
    rpc.child.kill().await?;
    result
}

pub(crate) fn codex_account(observation: &mut Observation, response: &Value) {
    let Some(account) = response.get("account") else {
        observation.state = State::Unavailable;
        return;
    };
    if account.is_null() {
        observation.state = State::Disconnected;
        return;
    }
    let workspace = identifier(&response["workspaceRouting"], "chatgptAccountId")
        .or_else(|| identifier(account, "chatgptAccountId"));
    let plan = identifier(account, "planType");
    observation.identity = identity(account, "email").or_else(|| identity(account, "id")).and_then(|email| {
        if account["type"] != "chatgpt" {
            return Some(email);
        }
        // Plano também distingue IDs quando o mesmo workspace muda de assinatura.
        match (workspace, plan) {
            (Some(workspace), plan) => {
                Some(format!("{email}\0workspace:{workspace}\0plan:{}", plan.unwrap_or_default()))
            }
            (None, Some(plan)) => Some(format!("{email}\0plan:{plan}")),
            (None, None) => None,
        }
    });
    observation.billing = match account["type"].as_str() {
        Some("chatgpt")
            if matches!(
                account["planType"].as_str(),
                Some("plus" | "pro" | "team" | "business" | "enterprise" | "edu")
            ) =>
        {
            Billing::Subscription
        }
        Some("apiKey") => Billing::PaidApi,
        _ => Billing::Unverified,
    };
    observation.state = if observation.billing == Billing::Unverified { State::Unverified } else { State::Connected };
    if observation.billing == Billing::Subscription {
        observation.subscription_proof =
            Some(SubscriptionProof::CodexAccountRead { plan: account["planType"].as_str().unwrap_or_default().into() });
    }
}

fn identity(value: &Value, field: &str) -> Option<String> {
    identifier(value, field).map(str::to_lowercase)
}

fn identifier<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value[field].as_str().map(str::trim).filter(|s| !s.is_empty() && s.len() <= 320)
}

pub(crate) fn codex_models(catalog: &Value) -> Vec<Model> {
    let mut models = Vec::new();
    for model in catalog["data"].as_array().into_iter().flatten() {
        let Some(id) = model["id"].as_str().filter(|id| safe_model_id(id)) else {
            continue;
        };
        if model["hidden"] == true || models.iter().any(|known: &Model| known.id == id) {
            continue;
        }
        let values: Vec<String> = model["supportedReasoningEfforts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|option| option["reasoningEffort"].as_str())
            .filter(|value| effort(value))
            .map(str::to_owned)
            .collect();
        let default = model["defaultReasoningEffort"]
            .as_str()
            .filter(|value| values.iter().any(|known| known == value))
            .map(str::to_owned);
        let options = if values.is_empty() {
            Vec::new()
        } else {
            vec![ModelOption { id: "reasoning_effort".into(), values, default }]
        };
        // O padrão do Codex vem primeiro: é o modelo já marcado quando a Open pede a escolha.
        let entry = Model { id: id.into(), options };
        if model["isDefault"] == true {
            models.insert(0, entry);
        } else {
            models.push(entry);
        }
    }
    models
}

fn safe_model_id(id: &str) -> bool {
    id.len() <= 100
        && (id.starts_with("gpt-")
            || id.starts_with("codex-")
            || id.starts_with("o1")
            || id.starts_with("o3")
            || id.starts_with("o4"))
        && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-._".contains(&byte))
}

fn effort(value: &str) -> bool {
    matches!(value, "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra")
}

struct Rpc {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Rpc {
    fn new(mut child: Child) -> Result<Self> {
        let input = child.stdin.take().ok_or_else(|| anyhow!("stdin indisponível"))?;
        let output = BufReader::new(child.stdout.take().ok_or_else(|| anyhow!("stdout indisponível"))?);
        Ok(Self { child, input, output })
    }

    async fn write(&mut self, value: Value) -> Result<()> {
        self.input.write_all(format!("{value}\n").as_bytes()).await?;
        self.input.flush().await?;
        Ok(())
    }

    async fn call(&mut self, id: u64, method: &str, params: Value) -> Result<Value> {
        self.write(json!({"id":id,"method":method,"params":params})).await?;
        loop {
            let mut line = Vec::new();
            let count = (&mut self.output).take(1024 * 1024 + 1).read_until(b'\n', &mut line).await?;
            if count == 0 || count > 1024 * 1024 {
                bail!("consulta indisponível");
            }
            let value: Value = serde_json::from_slice(&line)?;
            if value["id"] == id {
                // Erro nativo pode conter caminhos ou dados de autenticação; nunca propagá-lo.
                if value.get("error").is_some() {
                    bail!("consulta indisponível");
                }
                return Ok(value["result"].clone());
            }
            if value.get("method").is_some() && value.get("id").is_some() {
                self.write(json!({"id":value["id"],"error":{"code":-32601,"message":"unsupported during discovery"}}))
                    .await?;
            }
        }
    }
}

#[cfg(all(test, unix))]
mod cancellation_tests {
    use super::*;
    #[tokio::test]
    async fn motor_codex_persistente_e_timeout_encerram_grupo() {
        let Some(case) = crate::fork::motor_exec::test_support::case(
            "fork::engine_accounts::probe::cancellation_tests::motor_codex_persistente_e_timeout_encerram_grupo",
        ) else {
            return;
        };
        for hang in [false, true] {
            let fixture = case.fixture(if hang { "codex-hang" } else { "codex" }, hang);
            let timeout = if hang { Duration::from_millis(500) } else { Duration::from_secs(5) };
            let observation = tokio::time::timeout(
                Duration::from_secs(8),
                inspect_timeout(Backend::Codex, &fixture, &case.dir, timeout),
            )
            .await
            .unwrap();
            assert_eq!(observation.state, if hang { State::Unavailable } else { State::Connected });
            case.stopped(&fixture).await;
        }
    }
}
