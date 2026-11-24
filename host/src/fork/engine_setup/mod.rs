//! D164/SAAS-04: setup pessoal com capacidade explícita do dono, sem ativação comercial.
pub(crate) mod access;
mod grok;
pub(crate) mod install;
pub(crate) mod process;
#[cfg(test)]
pub(crate) use process::background_claude_catalog;
pub(crate) use process::{ProcessGroup, Rpc};
#[cfg(test)]
mod tests;
mod transport;
mod usage_cache;

use crate::{
    LockExt,
    api::devices::Caller,
    fork::engine_accounts::{self as accounts, Backend, Billing, LaunchReference, Registry, State, managed},
    hub::Hub,
    store::Store,
};
use anyhow::{Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BotEngineSelection {
    #[serde(default)]
    pub engine_account_id: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Preparing,
    AwaitingAuthorization,
    Verifying,
    Connected,
    Cancelled,
    Expired,
    Failed,
}
impl Phase {
    fn active(self) -> bool {
        matches!(self, Self::Preparing | Self::AwaitingAuthorization | Self::Verifying)
    }
}

struct Operation {
    hub: Weak<Hub>,
    account: String,
    authority: access::Authority,
    generation: u64,
    request: String,
    step: Step,
    revision: u64,
    start_revision: u64,
    expires: i64,
    deadline: tokio::time::Instant,
    phase: Phase,
    error: Option<&'static str>,
    challenge: Option<transport::Challenge>,
    terminal: bool,
    cursor: u64,
    output: String,
    cancel: watch::Sender<bool>,
    input: mpsc::Sender<process::Control>,
    worker_lifetime: watch::Receiver<()>,
}
struct ProfileStaging(Option<std::path::PathBuf>);
impl Drop for ProfileStaging {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = managed::remove_profile_home(path);
        }
    }
}

static OPERATIONS: Mutex<Option<HashMap<String, Operation>>> = Mutex::new(None);
/// Hubs em encerramento: lido e gravado só com OPERATIONS retido, para nenhuma operação entrar depois da espera.
static CLOSING: Mutex<Vec<Weak<Hub>>> = Mutex::new(Vec::new());

/// Barreiras determinísticas de teste (D372/R1), só para a chave indicada (`clientRequestId` do setup ou id da conta
/// em desconectar/remover). `Antes`: depois dos preflights, fora da cerca. `Cerca`: dentro da cerca de runtime (no
/// início, também com OPERATIONS), antes da persistência; o gancho não pode reentrar no Hub, só disparar outro thread.
/// `Depois`: cerca já solta. `Conclusao`/`ConclusaoCerca`: o mesmo, na conclusão do setup.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Barreira {
    Antes,
    Cerca,
    Depois,
    Conclusao,
    ConclusaoCerca,
}
#[cfg(test)]
type Gancho = (String, Barreira, Box<dyn FnOnce() + Send>);
#[cfg(test)]
pub(super) static BARREIRA: Mutex<Vec<Gancho>> = Mutex::new(Vec::new());
#[cfg(test)]
fn barreira(chave: &str, momento: Barreira) {
    let hook = {
        let mut guard = BARREIRA.locked();
        let found = guard.iter().position(|(id, quando, _)| id == chave && *quando == momento);
        found.map(|i| guard.remove(i).2)
    };
    if let Some(hook) = hook {
        hook();
    }
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Step {
    Install,
    Login,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Start {
    engine_account_id: String,
    expected_revision: u64,
    action: Step,
    client_request_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Create {
    backend: Backend,
    label: String,
    kind: String,
    client_request_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Mutation {
    engine_account_id: String,
    expected_revision: u64,
}
/// Pausar/reativar uma conexão sem tocar credencial. `false` pausa; `true` volta
/// a valer só depois de a sondagem ou o cofre confirmarem a conta, nunca por fé do pedido.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetEnabled {
    engine_account_id: String,
    expected_revision: u64,
    enabled: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Rename {
    engine_account_id: String,
    expected_revision: u64,
    label: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ModelVisibility {
    engine_account_id: String,
    expected_revision: u64,
    model_id: String,
    hidden: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Usage {
    engine_account_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperationAction {
    operation_id: String,
    generation: u64,
    #[serde(flatten)]
    action: ActionKind,
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum ActionKind {
    Read {
        #[serde(default, rename = "afterCursor")]
        after_cursor: Option<u64>,
    },
    Cancel,
    OpenTerminal,
    SubmitCode {
        code: String,
    },
    TerminalInput {
        data: String,
    },
    TerminalResize {
        cols: u16,
        rows: u16,
    },
}
fn parse<T: serde::de::DeserializeOwned>(body: Value) -> Result<T> {
    serde_json::from_value(body).map_err(|_| anyhow!("invalid_field: solicitação de setup inválida."))
}
pub(crate) fn require_local(caller: &Caller) -> Result<()> {
    ensure!(
        matches!(caller, Caller::Local),
        "remote_setup_unavailable: gestão remota de provedores ainda indisponível."
    );
    Ok(())
}

pub(crate) fn methods(method: &str) -> bool {
    matches!(
        method,
        "donnaEngineAccountCreate"
            | "donnaEngineAccountRename"
            | "donnaEngineAccountModelVisibility"
            | "donnaEngineAccountSetEnabled"
            | "donnaEngineAccountDisconnect"
            | "donnaEngineAccountRemove"
            | "donnaEngineAccountUsage"
            | "donnaEngineFallback"
            | "donnaEngineFallbackSave"
            | "donnaEngineFallbackAction"
            | "donnaEngineSetupStart"
            | "donnaEngineSetupAction"
    )
}

pub(crate) fn open_login_method(method: &str) -> bool {
    matches!(
        method,
        "donnaEngineAccountCreate"
            | "donnaEngineAccountRename"
            | "donnaEngineAccountDisconnect"
            | "donnaEngineAccountRemove"
            | "donnaEngineSetupStart"
            | "donnaEngineSetupAction"
    )
}

pub(crate) fn is_open_account(account: &accounts::Account) -> bool {
    account.backend == Backend::Codex
        && account.managed.is_some()
        && matches!(account.launch, LaunchReference::CliProfile { .. })
}

pub async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    let open = crate::environment::Environment::is_open();
    if open {
        require_local(caller)?;
        ensure!(open_login_method(method), "open_codex_unavailable");
    }
    let authority = access::resolve(hub, caller)?;
    if matches!(method, "donnaEngineAccountDisconnect" | "donnaEngineAccountRemove") {
        grok::before_mutation(hub, method, &body).await?;
    }
    let value = match method {
        "donnaEngineSetupStart" => {
            start_authorized(hub, parse(body)?, authority.clone(), Duration::from_secs(600)).await
        }
        "donnaEngineSetupAction" => {
            let request: OperationAction = parse(body)?;
            ensure!(
                !open || matches!(request.action, ActionKind::Read { .. } | ActionKind::Cancel),
                "open_codex_unavailable"
            );
            action_authorized(hub, request, &authority)
        }
        "donnaEngineFallback" | "donnaEngineFallbackSave" | "donnaEngineFallbackAction" => {
            crate::fork::fallback_contas::api::call(hub, &authority, method, body)
        }
        "donnaEngineAccountUsage" => {
            let space_id = body.get("spaceId").cloned();
            usage(hub, parse::<Usage>(body)?.engine_account_id, space_id.as_ref()).await
        }
        _ => {
            let _guard = accounts::MUTATION.lock().await;
            authority.require_current(hub)?;
            let value = match method {
                "donnaEngineAccountCreate" => {
                    let request: Create = parse(body)?;
                    ensure!(
                        request.backend != Backend::Grok
                            || (request.kind == "cli" && !crate::fork::motor_exec::configured()),
                        "invalid_field: o Grok entra só pelo login do programa oficial."
                    );
                    ensure!(
                        !open || (request.backend == Backend::Codex && request.kind == "cli"),
                        "open_codex_unavailable"
                    );
                    if !matches!(authority, access::Authority::Local) {
                        ensure!(request.kind == "cli", "remote_api_unavailable: use o login oficial do provedor.");
                    }
                    managed::create_with(
                        &hub.store,
                        request.backend,
                        &request.label,
                        &request.kind,
                        &request.client_request_id,
                        |registry| authority.persist(hub, registry),
                    )?
                }
                "donnaEngineAccountRename" => {
                    let request: Rename = parse(body)?;
                    let name = managed::label(&request.label)?;
                    let mut registry = Registry::load(&hub.store)?;
                    let account = registry.account_mut(&request.engine_account_id, request.expected_revision)?;
                    ensure!(!open || is_open_account(account), "open_codex_unavailable");
                    account.rename(name)?;
                    authority.persist(hub, &registry)?;
                    registry.dto_for(&hub.store, &request.engine_account_id)?
                }
                // MX-6: ocultar/mostrar não incrementa a revisão — o carimbo das sessões a inclui
                // e uma sessão em uso não pode cair por causa de uma preferência de listagem.
                "donnaEngineAccountModelVisibility" => {
                    let request: ModelVisibility = parse(body)?;
                    let model = managed::model_id(&request.model_id)?;
                    let mut registry = Registry::load(&hub.store)?;
                    let account = registry.account_mut(&request.engine_account_id, request.expected_revision)?;
                    account.set_model_hidden(&model, request.hidden)?;
                    authority.persist(hub, &registry)?;
                    registry.dto_for(&hub.store, &request.engine_account_id)?
                }
                // Pausar/reativar sem apagar credencial. Pausar deixa a conexão fora de
                // qualquer uso novo (state Disconnected) e mantém a chave do cofre ou o login do perfil
                // exatamente como estão — sem wipe, sem logout. Reativar não confia no pedido: volta
                // `Unverified` e só o cofre (conteúdo do slot) ou a sondagem com a MESMA identidade
                // devolvem `Connected`. Bots ligados à conta pausada falham alto, sem troca silenciosa.
                "donnaEngineAccountSetEnabled" => {
                    let request: SetEnabled = parse(body)?;
                    ensure!(
                        !in_setup(&hub.store, &request.engine_account_id),
                        "setup_in_progress: cancele o setup primeiro."
                    );
                    managed::idle(hub, &request.engine_account_id)?;
                    let id = request.engine_account_id.as_str();
                    // Leitura dentro da cerca, como no desconectar: um setup aceito no meio não perde
                    // o pedido/geração gravado no registro.
                    hub.with_idle_engine_account(id, || {
                        let mut registry = Registry::load(&hub.store)?;
                        ensure!(!open || is_open_account(registry.account(id)?), "open_codex_unavailable");
                        {
                            let account = registry.account_mut(id, request.expected_revision)?;
                            account.state = if request.enabled { State::Unverified } else { State::Disconnected };
                            account.models.clear();
                            let meta = account.managed.as_mut().ok_or_else(|| anyhow!("legacy_preserved"))?;
                            meta.catalog = None;
                            meta.enabled = request.enabled;
                            // A marca de pausa só vale na pausa que preserva a credencial: reativar
                            // apaga, e a saída destrutiva (desconectar) também a apaga.
                            meta.paused = !request.enabled;
                            meta.revision += 1;
                        }
                        authority.persist(hub, &registry)?;
                        accounts::invalidate();
                        registry.dto_for(&hub.store, id)
                    })?
                }
                "donnaEngineAccountDisconnect" | "donnaEngineAccountRemove" => {
                    let request: Mutation = parse(body)?;
                    ensure!(
                        !in_setup(&hub.store, &request.engine_account_id),
                        "setup_in_progress: cancele o setup primeiro."
                    );
                    managed::idle(hub, &request.engine_account_id)?;
                    let id = request.engine_account_id.as_str();
                    let remove = method == "donnaEngineAccountRemove";
                    #[cfg(test)]
                    barreira(id, Barreira::Antes);
                    // D372 (R1): leitura final de ocupação e escrita sob o runtime retido; um preparo na conta espera
                    // o commit (e a ativação/permit/lançamento recusam a conta desconectada) ou já ocupou o runtime.
                    // Tira a conta do cadastro: referência/slot próprios, sem logout global nem promessa de revogação remota.
                    let remover_do_cadastro = |registry: &mut Registry, revisao: u64| -> Result<()> {
                        // Perfil antigo desconectado pode sair da lista (só o registro); o resto segue `account_mut`.
                        if registry.account(id)?.managed.is_none() {
                            registry.legacy_removable(id, revisao)?;
                        } else {
                            registry.account_mut(id, revisao)?;
                        }
                        ensure!(
                            managed::bot_ids(&hub.store, id)?.is_empty(),
                            "account_in_use: remova os vínculos dos bots primeiro."
                        );
                        let slot = if let LaunchReference::VaultProfile { slot } = &registry.account(id)?.launch {
                            Some(slot.clone())
                        } else {
                            None
                        };
                        registry.accounts.retain(|a| a.id != id);
                        authority.persist_removing(hub, registry, slot.as_deref(), Some(id))?;
                        if let Err(error) = crate::fork::fallback_contas::esquecer_conta(&hub.store, id) {
                            tracing::warn!(%error, "ordem de reserva não limpa após remover a conta");
                        }
                        Ok(())
                    };
                    let (registry, credential, perfil) = hub.with_idle_engine_account(id, || {
                        #[cfg(test)]
                        barreira(id, Barreira::Cerca);
                        let mut registry = Registry::load(&hub.store)?;
                        let credential = grok::profile_home(registry.account(id)?);
                        ensure!(!open || is_open_account(registry.account(id)?), "open_codex_unavailable");
                        // Perfil isolado de conta Claude com login feito pelo programa oficial, lido antes de a
                        // conta sair do cadastro. Sem executável gravado, nunca houve login nesta conta.
                        let perfil = registry.account(id).ok().and_then(|account| match &account.launch {
                            LaunchReference::CliProfile { executable, directory }
                                if account.backend == Backend::Claude
                                    && account.managed.is_some()
                                    && !executable.as_os_str().is_empty() =>
                            {
                                Some((executable.clone(), directory.clone()))
                            }
                            _ => None,
                        });
                        if remove && perfil.is_none() {
                            remover_do_cadastro(&mut registry, request.expected_revision)?;
                        } else {
                            // Conta Claude com login no perfil: remover começa como desconectar (a conta deixa de ser
                            // usada e fica na lista); ela só sai do cadastro depois de o programa confirmar o logout.
                            if remove {
                                ensure!(
                                    managed::bot_ids(&hub.store, id)?.is_empty(),
                                    "account_in_use: remova os vínculos dos bots primeiro."
                                );
                            }
                            let account = registry.account_mut(id, request.expected_revision)?;
                            account.state = State::Disconnected;
                            account.models.clear();
                            let meta = account.managed.as_mut().ok_or_else(|| anyhow!("legacy_preserved"))?;
                            meta.enabled = false;
                            meta.paused = false;
                            meta.catalog = None;
                            meta.revision += 1;
                            authority.persist(hub, &registry)?;
                        }
                        Ok((registry, credential, perfil))
                    })?;
                    accounts::invalidate();
                    // O login local já saiu antes da mutação (remover) ou sai agora, depois do cadastro (desconectar).
                    if let Some(home) = &credential {
                        if remove {
                            if let Err(error) = grok::erase(&hub.store, id, home) {
                                tracing::warn!(%error, "pasta vazia do Grok não removida");
                            }
                            grok::sweep_orphans(&hub.store, id, None);
                        } else {
                            grok::wipe(&hub.store, id, home)?;
                            grok::sweep_orphans(&hub.store, id, Some(home));
                        }
                    }
                    #[cfg(test)]
                    barreira(id, Barreira::Depois);
                    // O cadastro já mudou: a lista relida mostra a conta desconectada mesmo se a saída falhar abaixo.
                    accounts::invalidate();
                    // Com a conta já desconectada ou removida (nenhum turno novo a usa), o próprio programa
                    // oficial apaga a credencial do perfil isolado, que pode estar no chaveiro do sistema.
                    if let Some((executable, directory)) = perfil {
                        managed::validate_profile(&hub.store, id, &directory)?;
                        ensure!(
                            crate::fork::motor_install::executable(&executable),
                            "cli_unavailable: a conta foi desconectada, mas o Claude Code dela não está disponível para apagar o login."
                        );
                        // Sem confirmação do logout, o erro chega ao dono e a conta continua na lista, desconectada.
                        process::claude_logout(&executable, &directory).await?;
                        if remove {
                            let revisao = registry.account(id)?.dto().revision;
                            hub.with_idle_engine_account(id, || {
                                let mut registry = Registry::load(&hub.store)?;
                                remover_do_cadastro(&mut registry, revisao)
                            })?;
                            accounts::invalidate();
                        }
                    }
                    if remove { json!({}) } else { registry.dto_for(&hub.store, id)? }
                }
                _ => bail!("invalid_field: método de setup desconhecido."),
            };
            accounts::invalidate();
            Ok(value)
        }
    }?;
    authority.require_current(hub)?;
    Ok(value)
}

pub(crate) fn in_setup(store: &Store, id: &str) -> bool {
    OPERATIONS.locked().as_mut().is_some_and(|ops| active_in(ops, store, id))
}
/// Operação ativa da conta, sobre OPERATIONS já bloqueado.
fn active_in(ops: &mut HashMap<String, Operation>, store: &Store, id: &str) -> bool {
    ops.values_mut().any(|op| {
        if op.account != id || op.hub.upgrade().is_none_or(|hub| hub.store.engine_setup_root != store.engine_setup_root)
        {
            return false;
        }
        expire(op);
        op.phase.active()
    })
}

async fn until_deadline<T>(
    deadline: tokio::time::Instant,
    mut cancel: watch::Receiver<bool>,
    work: impl Future<Output = Result<T>>,
) -> Result<T> {
    ensure!(!*cancel.borrow(), "setup_cancelled");
    tokio::select! {
        biased;
        () = tokio::time::sleep_until(deadline) => bail!("setup_expired"),
        _ = cancel.changed() => bail!("setup_cancelled"),
        result = work => result,
    }
}

fn expire(op: &mut Operation) {
    if op.phase.active() && (op.expires <= crate::store::now_ms() || tokio::time::Instant::now() >= op.deadline) {
        clear(op, Phase::Expired, Some("setup_expired"));
    } else if op.phase.active() && op.hub.upgrade().is_none_or(|hub| !op.authority.current(&hub)) {
        clear(op, Phase::Cancelled, Some("setup_authority_lost"));
    }
}
fn dto(id: &str, op: &mut Operation, after: Option<u64>) -> Value {
    expire(op);
    json!({"operationId":id,"engineAccountId":op.account,"generation":op.generation,"phase":op.phase,"revision":op.revision,"expiresAt":op.expires,"errorCode":op.error,"challenge":op.challenge,"terminal":if op.terminal {json!({"available":op.phase == Phase::AwaitingAuthorization && op.challenge.as_ref().is_some_and(|c|c.kind == "paste_code"),"cursor":op.cursor,"output":if after == Some(op.cursor) {""} else {&op.output}})} else {Value::Null}})
}
fn clear(op: &mut Operation, phase: Phase, error: Option<&'static str>) {
    op.phase = phase;
    op.error = error;
    op.challenge = None;
    op.output.clear();
    op.cursor += 1;
    op.terminal = false;
    let _ = op.cancel.send(true);
}

pub(crate) async fn shutdown_open(hub: &Arc<Hub>) {
    let waiting: Vec<_> = {
        let mut operations = OPERATIONS.locked();
        CLOSING.locked().push(Arc::downgrade(hub));
        operations
            .as_mut()
            .into_iter()
            .flat_map(|ops| ops.values_mut())
            .filter(|op| op.hub.ptr_eq(&Arc::downgrade(hub)))
            .map(|op| {
                if op.phase.active() {
                    clear(op, Phase::Cancelled, None);
                }
                op.worker_lifetime.clone()
            })
            .collect()
    };
    let _ = tokio::time::timeout(Duration::from_secs(3), async {
        for mut worker in waiting {
            let _ = worker.changed().await;
        }
    })
    .await;
}
#[cfg(test)]
fn action(hub: &Arc<Hub>, request: OperationAction) -> Result<Value> {
    action_authorized(hub, request, &access::Authority::Local)
}
fn action_authorized(hub: &Arc<Hub>, request: OperationAction, authority: &access::Authority) -> Result<Value> {
    let mut guard = OPERATIONS.locked();
    let op = guard
        .as_mut()
        .and_then(|ops| ops.get_mut(&request.operation_id))
        .filter(|op| op.hub.ptr_eq(&Arc::downgrade(hub)))
        .ok_or_else(|| anyhow!("operation_expired: releia a conta e inicie outra operação."))?;
    ensure!(op.authority == *authority, "operation_forbidden: operação pertence a outro vínculo.");
    ensure!(op.generation == request.generation, "operation_stale: operação substituída.");
    expire(op);
    let mut after = None;
    authority.with_db(hub, |_| {
        match request.action {
            ActionKind::Read { after_cursor } => {
                ensure!(after_cursor.is_none_or(|n| n <= op.cursor), "terminal_cursor_invalid");
                after = after_cursor;
            }
            ActionKind::Cancel => {
                if op.phase.active() {
                    clear(op, Phase::Cancelled, None);
                }
            }
            ActionKind::OpenTerminal => {
                ensure!(
                    op.phase == Phase::AwaitingAuthorization
                        && op.challenge.as_ref().is_some_and(|c| c.kind == "paste_code"),
                    "terminal_unavailable: esta operação não aceita entrada interativa."
                );
                op.terminal = true;
                op.output = "Aguardando o código de autorização do provedor.\r\n".into();
                op.cursor += 1;
            }
            ActionKind::TerminalResize { cols, rows } => {
                ensure!(
                    op.terminal && (20..=240).contains(&cols) && (5..=100).contains(&rows),
                    "terminal_size_invalid"
                );
                op.input.try_send(process::Control::Resize(cols, rows)).map_err(|_| anyhow!("terminal_unavailable"))?;
            }
            ActionKind::SubmitCode { code } => submit(op, code)?,
            ActionKind::TerminalInput { data } => {
                use base64::Engine as _;
                ensure!(op.terminal && data.len() <= 4096, "terminal_input_invalid");
                let raw =
                    base64::prelude::BASE64_STANDARD.decode(data).map_err(|_| anyhow!("terminal_input_invalid"))?;
                let code = String::from_utf8(raw).map_err(|_| anyhow!("terminal_input_invalid"))?;
                submit(op, code.trim_end_matches(['\r', '\n']).to_owned())?;
            }
        }
        Ok(())
    })?;
    Ok(dto(&request.operation_id, op, after))
}
fn submit(op: &mut Operation, code: String) -> Result<()> {
    ensure!(
        op.phase == Phase::AwaitingAuthorization && op.challenge.as_ref().is_some_and(|c| c.kind == "paste_code"),
        "auth_input_unavailable"
    );
    transport::validate_code(&code)?;
    op.input.try_send(process::Control::Code(code)).map_err(|_| anyhow!("auth_input_unavailable"))?;
    op.phase = Phase::Verifying;
    op.output.clear();
    op.cursor += 1;
    Ok(())
}

#[cfg(test)]
async fn start_with_ttl(hub: &Arc<Hub>, request: Start, ttl: Duration) -> Result<Value> {
    start_authorized(hub, request, access::Authority::Local, ttl).await
}
async fn start_authorized(
    hub: &Arc<Hub>,
    request: Start,
    authority: access::Authority,
    ttl: Duration,
) -> Result<Value> {
    let open = crate::environment::Environment::is_open();
    ensure!(!open || request.action == Step::Login, "open_codex_unavailable");
    managed::request_id(&request.client_request_id)?;
    let _mutation = accounts::MUTATION.lock().await;
    ensure!(authority.current(hub), "setup_authority_lost");
    // Retry idempotente também depois da conclusão; corpo diferente é conflito. Solta OPERATIONS antes do runtime.
    {
        let mut guard = OPERATIONS.locked();
        if let Some((id, op)) = guard.as_mut().and_then(|ops| {
            ops.iter_mut()
                .find(|(_, op)| op.hub.ptr_eq(&Arc::downgrade(hub)) && op.request == request.client_request_id)
        }) {
            ensure!(op.authority == authority, "operation_forbidden");
            ensure!(
                op.account == request.engine_account_id
                    && op.step == request.action
                    && op.start_revision == request.expected_revision,
                "request_conflict"
            );
            return Ok(dto(id, op, None));
        }
    }
    // Preflights; a garantia final é a cerca abaixo.
    managed::idle(hub, &request.engine_account_id)?;
    ensure!(
        !in_setup(&hub.store, &request.engine_account_id),
        "setup_in_progress: já existe uma operação nesta conta."
    );
    #[cfg(test)]
    let barrier_request = request.client_request_id.clone();
    #[cfg(test)]
    barreira(&barrier_request, Barreira::Antes);
    let id = uuid::Uuid::new_v4().to_string();
    let (cancel, rx_cancel) = watch::channel(false);
    let (tx_input, rx_input) = mpsc::channel(4);
    let (tx_event, mut rx_event) = mpsc::channel(8);
    let (worker_done, worker_lifetime) = watch::channel(());
    let deadline = tokio::time::Instant::now() + ttl;
    // D372 (R1): aceitar a tentativa é um só trecho sob o runtime retido (ordem runtime → OPERATIONS → Authority):
    // ociosidade final, metadados e perfil novo (com limpeza RAII imediata), capacidade, primeira escrita pela
    // Authority original e só então a operação visível. Recusa antes do commit não grava nada nem consome o pedido;
    // um preparo na conta espera o runtime e encontra a operação ativa.
    let (answer, generation, backend, profile, cleanup) =
        hub.with_idle_engine_account(&request.engine_account_id, || {
            let mut registry = Registry::load(&hub.store)?;
            let account = registry.account_mut(&request.engine_account_id, request.expected_revision)?;
            ensure!(!open || is_open_account(account), "open_codex_unavailable");
            let LaunchReference::CliProfile { directory, .. } = &account.launch else {
                bail!("api_form_required: use o formulário seguro de API.")
            };
            managed::validate_profile(&hub.store, &account.id, directory)?;
            let backend = account.backend;
            let old_directory = directory.clone();
            let meta = account.managed.as_mut().ok_or_else(|| anyhow!("legacy_preserved"))?;
            ensure!(
                meta.setup_request.as_ref().is_none_or(|(id, _)| *id != request.client_request_id),
                "operation_expired: pedido anterior encerrado; inicie uma nova operação."
            );
            meta.setup_request = Some((
                request.client_request_id.clone(),
                if request.action == Step::Login { "login" } else { "install" }.into(),
            ));
            meta.generation += 1;
            let generation = meta.generation;
            let (profile, cleanup) = if request.action == Step::Login {
                let profile = managed::new_profile(&hub.store, &request.engine_account_id, backend)?;
                let cleanup = ProfileStaging(profile.parent().map(std::path::Path::to_path_buf));
                (profile, cleanup)
            } else {
                (old_directory, ProfileStaging(None))
            };
            let mut guard = OPERATIONS.locked();
            ensure!(!CLOSING.locked().iter().any(|closing| closing.ptr_eq(&Arc::downgrade(hub))), "setup_cancelled");
            let ops = guard.get_or_insert_with(HashMap::new);
            let now = crate::store::now_ms();
            ops.retain(|_, op| op.hub.strong_count() > 0 && (op.phase.active() || op.expires + 600_000 > now));
            ensure!(ops.len() < 128 && !ops.contains_key(&id), "operation_limit");
            ensure!(
                !active_in(ops, &hub.store, &request.engine_account_id),
                "setup_in_progress: já existe uma operação nesta conta."
            );
            #[cfg(test)]
            barreira(&barrier_request, Barreira::Cerca);
            authority.persist(hub, &registry)?;
            let op = ops.entry(id.clone()).or_insert(Operation {
                hub: Arc::downgrade(hub),
                account: request.engine_account_id.clone(),
                authority,
                generation,
                request: request.client_request_id.clone(),
                step: request.action,
                revision: request.expected_revision,
                start_revision: request.expected_revision,
                expires: now.saturating_add(i64::try_from(ttl.as_millis()).unwrap_or(i64::MAX)),
                deadline,
                phase: Phase::Preparing,
                error: None,
                challenge: None,
                terminal: false,
                cursor: 0,
                output: String::new(),
                cancel,
                input: tx_input,
                worker_lifetime,
            });
            Ok((dto(&id, op, None), generation, backend, profile, cleanup))
        })?;
    #[cfg(test)]
    barreira(&barrier_request, Barreira::Depois);
    let monitor_id = id.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(200));
        loop {
            tick.tick().await;
            let mut guard = OPERATIONS.locked();
            let Some(op) = guard.as_mut().and_then(|ops| ops.get_mut(&monitor_id)) else { break };
            expire(op);
            if !op.phase.active() {
                break;
            }
        }
    });
    let weak = Arc::downgrade(hub);
    let worker_id = id.clone();
    tokio::spawn(async move {
        while let Some(event) = rx_event.recv().await {
            let mut guard = OPERATIONS.locked();
            if let Some(op) = guard.as_mut().and_then(|ops| ops.get_mut(&worker_id)) {
                expire(op);
                if !op.phase.active() {
                    continue;
                }
                match event {
                    process::Event::Challenge(challenge) => {
                        op.challenge = Some(challenge);
                        op.phase = Phase::AwaitingAuthorization;
                    }
                    process::Event::Verifying => op.phase = Phase::Verifying,
                }
            }
        }
    });
    let store_root = hub.store.engine_setup_root.clone();
    let binaries = if crate::fork::motor_exec::configured() {
        crate::fork::motor_install::binaries()
    } else {
        store_root.join("bin")
    };
    tokio::spawn(async move {
        let _worker_done = worker_done;
        // Declarado depois: o perfil provisório some antes de o encerramento ver o worker concluído.
        let mut cleanup = cleanup;
        let outcome = until_deadline(deadline, rx_cancel.clone(), async {
            let (executable, mut proof) = if open {
                #[cfg(unix)]
                {
                    let (executable, proof) = process::login_open_codex(&profile, tx_event).await?;
                    (executable, Some(proof))
                }
                #[cfg(not(unix))]
                anyhow::bail!("open_codex_platform")
            } else {
                let executable = match request.action {
                    Step::Install => {
                        let hub = weak.upgrade().ok_or_else(|| anyhow!("setup_cancelled"))?;
                        ensure!(
                            !hub.store.bots()?.iter().any(|b| !b.deleted && b.config.backend == backend.id() && {
                                let runtime = hub.runtime(&b.config.id);
                                runtime.preparing_turn_token.is_some()
                                    || matches!(
                                        runtime.status,
                                        crate::hub::BotStatus::Working | crate::hub::BotStatus::NeedsInput
                                    )
                            }),
                            "account_busy"
                        );
                        drop(hub);
                        install::install(&binaries, backend, &profile).await?
                    }
                    Step::Login => install::existing(&binaries, backend, &profile).await.ok_or_else(|| {
                        if backend == Backend::Grok {
                            anyhow!("grok_not_installed")
                        } else {
                            anyhow!("cli_incompatible: prepare o conector antes de entrar.")
                        }
                    })?,
                };
                let proof = if request.action == Step::Login {
                    Some(process::login(backend, &executable, &profile, tx_event, rx_input, rx_cancel).await?)
                } else {
                    None
                };
                (executable, proof)
            };
            if let Some(proof) = &mut proof
                && backend == Backend::Claude
                && proof.state == State::Connected
                && let Ok(models) = process::claude_catalog(&executable, &profile).await
            {
                proof.models = models;
                proof.catalog_observed = true;
            }
            let hub = weak.upgrade().ok_or_else(|| anyhow!("setup_cancelled"))?;
            let _mutation = accounts::MUTATION.lock().await;
            // Ajuste de arquivos do perfil novo fora da cerca; dentro, rechecagem final de OP/prazo/Authority.
            if proof.is_some() {
                managed::secure_files(if crate::fork::motor_exec::configured() {
                    &profile
                } else {
                    profile.parent().unwrap_or(&profile)
                })?;
            }
            #[cfg(test)]
            barreira(&barrier_request, Barreira::Conclusao);
            // D372 (R1): ordem M → runtime → OPERATIONS → Authority. Ociosidade final, validação, escrita, desarme do
            // perfil novo e Connected sob o runtime retido; erros e eventos depois de soltá-lo.
            hub.with_idle_engine_account(&request.engine_account_id, || {
                #[cfg(test)]
                barreira(&barrier_request, Barreira::ConclusaoCerca);
                let mut registry = Registry::load(&hub.store)?;
                let quota = proof
                    .as_ref()
                    .and_then(|p| p.identity.as_deref())
                    .map(|identity| registry.fingerprint(backend, "quota", identity));
                let principal = proof
                    .as_ref()
                    .and_then(|p| p.identity.as_deref())
                    .map(|identity| registry.fingerprint(backend, "principal", managed::principal(identity)));
                let account = registry.account_mut(&request.engine_account_id, request.expected_revision)?;
                let superseded = grok::previous_home(account, &profile);
                let meta = account.managed.as_mut().ok_or_else(|| anyhow!("legacy_preserved"))?;
                ensure!(meta.generation == generation, "operation_stale");
                let mut guard = OPERATIONS.locked();
                let op = guard.as_mut().and_then(|ops| ops.get_mut(&id)).ok_or_else(|| anyhow!("operation_expired"))?;
                expire(op);
                ensure!(op.phase.active(), "operation_stale");
                if let Some(proof) = proof {
                    ensure!(
                        proof.state == State::Connected
                            && proof.billing == Billing::Subscription
                            && proof.subscription_proof.is_some()
                            && principal.is_some(),
                        "account_unverified"
                    );
                    ensure!(
                        meta.principal.is_none() || meta.principal == principal,
                        "identity_changed: crie outra conta para este titular."
                    );
                    expire(op);
                    ensure!(op.phase.active(), "operation_stale");
                    meta.profile = Some(profile.clone());
                    meta.principal = principal;
                    meta.enabled = true;
                    account.launch = LaunchReference::CliProfile { executable, directory: profile.clone() };
                    account.billing = proof.billing;
                    account.state = State::Connected;
                    account.quota_identity = quota;
                    account.subscription_proof = proof.subscription_proof;
                    account.verified_at = Some(crate::store::now_ms());
                    account.models = proof.models;
                } else if let LaunchReference::CliProfile { executable: current, .. } = &mut account.launch {
                    *current = executable;
                    account.models.clear();
                    meta.catalog = None;
                }
                meta.revision += 1;
                let revision = meta.revision;
                if request.action == Step::Login {
                    account.bind_catalog();
                }
                // A gravação do ponteiro novo vem primeiro: se ela falhar, a conta continua apontando para o
                // login antigo, intacto. Só então o login anterior sai — se o apagamento falhar, sobra pasta
                // antiga sem dono, nunca credencial perdida. O perfil novo já é o da conta, então a limpeza
                // de preparo desarma antes do apagamento.
                op.authority.persist(&hub, &registry)?;
                op.revision = revision;
                cleanup.0 = None;
                if request.action == Step::Login
                    && let Some(home) = &superseded
                {
                    grok::erase(&hub.store, &request.engine_account_id, home)?;
                }
                if request.action == Step::Login && backend == Backend::Grok {
                    grok::sweep_orphans(&hub.store, &request.engine_account_id, profile.parent());
                }
                accounts::invalidate();
                clear(op, Phase::Connected, None);
                Ok(())
            })
        })
        .await;
        if let Err(error) = outcome {
            let text = error.to_string();
            let code = if text.starts_with("remote_setup_unavailable") {
                "setup_authority_lost"
            } else {
                [
                    "setup_cancelled",
                    "setup_authority_lost",
                    "setup_expired",
                    "auth_url_invalid",
                    "auth_correlation_invalid",
                    "auth_capability_unavailable",
                    "auth_protocol_invalid",
                    "auth_output_limit",
                    "auth_failed",
                    "auth_shutdown_failed",
                    "cli_incompatible",
                    "grok_not_installed",
                    "credential_not_erased",
                    "open_codex_platform",
                    "open_codex_unavailable",
                    "account_busy",
                    "account_unverified",
                    "identity_changed",
                    "account_conflict",
                    "install_integrity_invalid",
                    "operation_stale",
                ]
                .into_iter()
                .find(|code| text.starts_with(code))
                .unwrap_or("setup_failed")
            };
            let mut guard = OPERATIONS.locked();
            if let Some(op) = guard.as_mut().and_then(|ops| ops.get_mut(&id))
                && op.phase.active()
            {
                clear(
                    op,
                    if code == "setup_expired" {
                        Phase::Expired
                    } else if code == "setup_authority_lost" {
                        Phase::Cancelled
                    } else {
                        Phase::Failed
                    },
                    Some(code),
                );
            }
        }
    });
    Ok(answer)
}

/// MEX-1294/F6-A: a leitura é por conta E por Space. Ausente = Pessoal; `null` ou tipo errado =
/// `invalid_space`; Space (o Pessoal incluído) sem vínculo comprovado com a conta = `account_wrong_space`
/// (a conta nunca é consultada por outro Space, nem de raspão).
fn usage_space(store: &Store, id: &str, space_id: Option<&Value>) -> Result<String> {
    let space = match space_id {
        None => crate::fork::spaces::PERSONAL.to_owned(),
        Some(Value::String(s)) if !s.is_empty() && s.len() <= 100 => s.clone(),
        Some(_) => bail!("invalid_space"),
    };
    let c = store.db.locked();
    crate::fork::spaces::require_space(&c, &space)?;
    ensure!(managed::linked_to_space(&c, id, &space)?, "account_wrong_space");
    Ok(space)
}

pub(crate) async fn usage(hub: &Arc<Hub>, id: String, space_id: Option<&Value>) -> Result<Value> {
    let space = usage_space(&hub.store, &id, space_id)?;
    let registry = Registry::load(&hub.store)?;
    let account = registry.account(&id)?;
    let mut value =
        json!({"engineAccountId":id,"spaceId":space,"state":"unavailable","source":null,"updatedAt":null,"windows":[]});
    // Cota global nunca é reutilizada; o Codex confirma a identidade no mesmo app-server.
    if account.backend == Backend::Codex
        && account.state == State::Connected
        && !in_setup(&hub.store, &id)
        && let LaunchReference::CliProfile { executable, directory } = &account.launch
    {
        if account.managed.is_some() && managed::validate_profile(&hub.store, &id, directory).is_err() {
            return Ok(value);
        }
        let backend = account.backend;
        // Uma leitura por vez por conta, com validade curta: as linhas da tela e as trocas de conversa dividem o
        // mesmo processo do CLI. A chave leva a identidade da cota: reconectar noutra conta não herda a leitura.
        let key = format!("{id}:{}", account.quota_identity.as_deref().unwrap_or_default());
        let reading = usage_cache::read_once(&key, || async {
            let read = tokio::time::timeout(usage_cache::READ_TIMEOUT, async {
                ensure!(
                    !crate::fork::motor_exec::configured() || crate::fork::motor_install::executable(executable),
                    "cli_unavailable: reinstale o conector como motor"
                );
                let windows = match backend {
                    Backend::Codex => {
                        let mut rpc = Rpc::start(executable, directory)?;
                        rpc.initialize().await?;
                        let proof = rpc.proof(executable, directory).await?;
                        let quota =
                            proof.identity.as_deref().map(|identity| registry.fingerprint(backend, "quota", identity));
                        ensure!(
                            quota.is_some() && quota == account.quota_identity && proof.state == State::Connected,
                            "usage_unavailable"
                        );
                        let rates = rpc.call(5, "account/rateLimits/read", json!({})).await?;
                        // A mesma leitura do painel global do Codex: bucket `codex` e janela vencida descartada.
                        crate::fork::codex_usage::parse(&rates, crate::store::now_ms())
                    }
                    Backend::Grok | Backend::Claude => bail!("usage_unavailable"),
                };
                Ok::<_, anyhow::Error>(windows)
            })
            .await;
            read.ok().and_then(Result::ok)
        })
        .await;
        if let Some((windows, read_at)) = reading {
            let now = crate::store::now_ms();
            let windows: Vec<_> = crate::usage::session_week(backend.id(), &windows)
                .into_iter()
                // Uma janela que já renovou (também dentro da validade do cache) não marca uso antigo.
                .filter(|w| w.resets_at.is_none_or(|at| at > now))
                .collect();
            if !windows.is_empty() {
                value["state"] = "available".into();
                value["source"] = "cli".into();
                value["updatedAt"] = read_at.into();
                value["windows"] = json!(
                    windows
                        .iter()
                        .map(|w| json!({
                            "id": w.id,
                            "percent": w.percent,
                            "resetsAt": w.resets_at,
                            "resetsText": w.resets_text.clone(),
                            "nearLimit": w.percent >= crate::usage::NEAR_LIMIT_PERCENT,
                        }))
                        .collect::<Vec<_>>()
                );
            }
        }
    }
    Ok(value)
}

/// MEX-1391/F6-B, regra (k): o próprio CLI confirma a identidade da conta, no perfil isolado dela, antes de qualquer
/// contexto ir para ela. Mesma prova da leitura de uso, sem ler a cota: identidade divergente ou sem resposta, falha.
pub(crate) async fn confirm_identity(registry: &Registry, id: &str) -> Result<()> {
    let account = registry.account(id)?;
    ensure!(account.state == State::Connected, "account_unavailable");
    let LaunchReference::CliProfile { executable, directory } = &account.launch else {
        bail!("identity_unconfirmed");
    };
    let backend = account.backend;
    let proof = tokio::time::timeout(usage_cache::READ_TIMEOUT, async {
        match backend {
            Backend::Codex => {
                let mut rpc = Rpc::start(executable, directory)?;
                rpc.initialize().await?;
                let proof = rpc.proof(executable, directory).await?;
                Ok::<_, anyhow::Error>((proof.identity, proof.state))
            }
            Backend::Claude | Backend::Grok => {
                let proof = accounts::probe::inspect(backend, executable, directory).await;
                Ok((proof.identity, proof.state))
            }
        }
    })
    .await
    .map_err(|_| anyhow!("identity_unconfirmed"))??;
    let quota = proof.0.as_deref().map(|identity| registry.fingerprint(backend, "quota", identity));
    ensure!(quota.is_some() && quota == account.quota_identity && proof.1 == State::Connected, "identity_unconfirmed");
    Ok(())
}

pub(crate) async fn set_api_env(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Api {
        engine_account_id: String,
        expected_revision: u64,
        backend: Backend,
        vars: std::collections::BTreeMap<String, String>,
    }
    require_local(caller)?;
    let request: Api = parse(body)?;
    ensure!(request.backend != Backend::Grok, "invalid_field: o Grok entra só pelo login do programa oficial.");
    let key = if request.backend == Backend::Codex { "OPENAI_API_KEY" } else { "ANTHROPIC_API_KEY" };
    ensure!(
        request.vars.len() == 1
            && request
                .vars
                .get(key)
                .is_some_and(|v| !v.trim().is_empty() && v.len() <= 8192 && !v.chars().any(char::is_control)),
        "invalid_field: use a chave permitida pelo provedor."
    );
    let models = if request.backend == Backend::Claude {
        let registry = Registry::load(&hub.store)?;
        let account = registry.account(&request.engine_account_id)?;
        ensure!(
            account.backend == Backend::Claude
                && account.managed.as_ref().is_some_and(|meta| meta.revision == request.expected_revision)
                && matches!(&account.launch, LaunchReference::VaultProfile { slot }
                    if *slot == format!("agent-account:{}", account.id)),
            "account_conflict: a conta de API mudou; recarregue antes de salvar."
        );
        let credential = request.vars.get(key).ok_or_else(|| anyhow!("invalid_field: chave ausente."))?;
        let header = crate::fork::background_anthropic::api_key(credential)?;
        Some(crate::fork::background_anthropic::models(header).await?)
    } else {
        None
    };
    let _guard = accounts::MUTATION.lock().await;
    let mut registry = Registry::load(&hub.store)?;
    let account = registry.account_mut(&request.engine_account_id, request.expected_revision)?;
    ensure!(account.backend == request.backend, "account_incompatible");
    let LaunchReference::VaultProfile { slot } = &account.launch else { bail!("api_form_required") };
    let serialized = zeroize::Zeroizing::new(serde_json::to_string(&request.vars)?);
    let slot = slot.clone();
    let sealed = crate::market::vault::seal(&hub.store, &slot, &serialized)?;
    account.billing = Billing::PaidApi;
    account.state = State::Connected;
    let meta = account.managed.as_mut().ok_or_else(|| anyhow!("legacy_preserved"))?;
    meta.enabled = true;
    meta.paused = false;
    meta.revision += 1;
    if let Some(models) = models {
        meta.principal = Some(request.engine_account_id.clone());
        account.models = models;
        account.bind_catalog();
    }
    let registry_json = serde_json::to_string(&registry)?;
    {
        let mut db = hub.store.db.locked();
        let tx = db.transaction()?;
        crate::market::vault::write_sealed(&tx, &slot, &sealed)?;
        tx.execute(
            "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            (accounts::SLOT, &registry_json),
        )?;
        tx.commit()?;
    }
    crate::fork::background_selection::backoff::changed(&hub.store);
    accounts::invalidate();
    registry.dto_for(&hub.store, &request.engine_account_id)
}
pub(crate) fn profile_environment(directory: &std::path::Path, backend: Backend) -> Vec<(String, String)> {
    process::environment(directory, backend)
}

pub(crate) fn probe_command(
    executable: &std::path::Path,
    directory: &std::path::Path,
    backend: Backend,
) -> tokio::process::Command {
    process::command(executable, directory, backend)
}

/// Fecha as cerimônias legadas deste recorte sem alterar execução ou contas legadas.
pub(crate) fn require_legacy_setup(caller: &Caller, backend: &str) -> Result<()> {
    if matches!(backend, "claude" | "codex" | "claude-acp" | "claude-agent-acp" | "claude-code-acp" | "codex-acp") {
        require_local(caller)?;
    }
    Ok(())
}

pub(crate) async fn claude_catalog(
    executable: &std::path::Path,
    directory: &std::path::Path,
) -> Result<Vec<accounts::Model>> {
    process::claude_catalog(executable, directory).await
}

pub(crate) fn kill_setup_group(pid: Option<u32>) {
    process::kill_group(pid);
}

pub(crate) async fn discovery_output(command: tokio::process::Command) -> std::io::Result<std::process::Output> {
    process::output_with_group(command).await
}
