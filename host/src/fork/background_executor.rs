//! Background jobs on the bot's own harness (dreaming, automatic naming): one prompt to a throwaway agent.

use crate::agent::acp::{self, Acp, Incoming};
use crate::agent::bot::{Adapter, Instructions};
use crate::hub::Hub;
use crate::store::BotConfig;
use anyhow::{Result, anyhow, bail};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(180);
pub(crate) const MAX_OUTPUT_BYTES: usize = 128_000;
static BACKGROUND_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
static BACKGROUND_QUEUE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);

#[derive(Debug)]
pub(crate) struct Busy;
impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("fila de fundo cheia")
    }
}
impl std::error::Error for Busy {}

async fn wait_for_slot<'a>(
    queue: &tokio::sync::Semaphore,
    slots: &'a tokio::sync::Semaphore,
) -> Result<tokio::sync::SemaphorePermit<'a>> {
    let queued = queue.try_acquire().map_err(|_| Busy)?;
    let permit = slots.acquire().await.map_err(|_| anyhow!("A fila de fundo foi encerrada."))?;
    drop(queued);
    Ok(permit)
}

#[derive(Debug)]
pub(crate) struct SubscriptionLimit;
impl std::fmt::Display for SubscriptionLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("limite da assinatura")
    }
}
impl std::error::Error for SubscriptionLimit {}

fn classify_error(error: anyhow::Error, subscription: bool) -> anyhow::Error {
    if subscription
        && error.downcast_ref::<acp::RpcError>().is_some_and(|rpc| {
            crate::agent::subscription::detect(
                "claude",
                rpc.kind.as_deref(),
                &rpc.message,
                &rpc.data,
                crate::store::now_ms(),
            )
            .is_some()
        })
    {
        SubscriptionLimit.into()
    } else {
        error
    }
}

pub(crate) struct JobResponse {
    pub(crate) text: String,
    pub(crate) selection: crate::fork::background_selection::Resolved,
    history: Option<(Arc<Hub>, String)>,
}

impl JobResponse {
    /// A resposta como o executor a entrega, sem histórico (testes do sono).
    #[cfg(test)]
    pub(crate) fn for_test(text: String, selection: crate::fork::background_selection::Resolved) -> Self {
        Self { text, selection, history: None }
    }

    pub(crate) fn finish(&mut self, status: crate::fork::background_history::Status) {
        if let Some((hub, id)) = self.history.take() {
            if let Err(error) = crate::fork::background_history::finish(&hub.store, &id, status) {
                tracing::warn!(%error, "não foi possível atualizar histórico de fundo");
            }
            let _ = hub.events.send(json!({"type":"backgroundHistoryChanged"}));
        }
    }

    pub(crate) fn fail_revalidation(&mut self) {
        if let Some((hub, id)) = self.history.take() {
            if let Err(error) = crate::fork::background_history::finish_with_stage(
                &hub.store,
                &id,
                crate::fork::background_history::Status::Failed,
                Some(crate::fork::background_history::FailureStage::Revalidation),
            ) {
                tracing::warn!(%error, "não foi possível atualizar histórico de fundo");
            }
            let _ = hub.events.send(json!({"type":"backgroundHistoryChanged"}));
        }
    }
}

impl Drop for JobResponse {
    fn drop(&mut self) {
        self.finish(crate::fork::background_history::Status::Failed);
    }
}

/// Callbacks internos do Hub: prepara o conteúdo só após resolver a conta e
/// revalida a mesma prova antes de cada fronteira de envio.
#[derive(Clone, Copy)]
pub(crate) struct MemoryBoundary<'a> {
    pub prepare: &'a (dyn Fn(&crate::fork::background_selection::Resolved) -> Result<String> + Send + Sync),
    pub verify: &'a (dyn Fn(&crate::fork::background_selection::Resolved) -> Result<()> + Send + Sync),
}

fn check_send_boundary(
    hub: &Hub,
    selected: &crate::fork::background_selection::Resolved,
    boundary: Option<MemoryBoundary<'_>>,
) -> Result<()> {
    if let Some(boundary) = boundary {
        crate::fork::background_selection::revalidate(&hub.store, selected)?;
        (boundary.verify)(selected)?;
    }
    Ok(())
}

pub(crate) async fn one_shot(hub: &Arc<Hub>, cfg: &BotConfig, system: &str, user: &str) -> Result<JobResponse> {
    run_observed(hub, cfg, (system, user), &mut false, crate::fork::background_history::Function::Titles, None).await
}

/// MB-3: uma chamada isolada na assinatura Claude do próprio bot (`resolve_own`).
pub(crate) async fn one_shot_own(hub: &Arc<Hub>, cfg: &BotConfig, system: &str, user: &str) -> Result<JobResponse> {
    run_observed_with(
        hub,
        cfg,
        (system, user),
        &mut false,
        crate::fork::background_history::Function::DailySummary,
        || async { Ok(()) },
        None,
    )
    .await
}

pub(crate) async fn one_shot_observed(
    hub: &Arc<Hub>,
    cfg: &BotConfig,
    system: &str,
    exposed: &mut bool,
    boundary: MemoryBoundary<'_>,
) -> Result<JobResponse> {
    run_observed(hub, cfg, (system, ""), exposed, crate::fork::background_history::Function::Memory, Some(boundary))
        .await
}

async fn run_observed(
    hub: &Arc<Hub>,
    cfg: &BotConfig,
    input: (&str, &str),
    exposed: &mut bool,
    function: crate::fork::background_history::Function,
    boundary: Option<MemoryBoundary<'_>>,
) -> Result<JobResponse> {
    run_observed_with(
        hub,
        cfg,
        input,
        exposed,
        function,
        || crate::fork::engine_accounts::discover_background(hub),
        boundary,
    )
    .await
}

async fn run_observed_with<F, Fut>(
    hub: &Arc<Hub>,
    cfg: &BotConfig,
    input: (&str, &str),
    exposed: &mut bool,
    function: crate::fork::background_history::Function,
    discover: F,
    boundary: Option<MemoryBoundary<'_>>,
) -> Result<JobResponse>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<()>>,
{
    use crate::fork::background_history::{self, FailureStage, Status};
    let (system, user) = input;
    let mut observed = Observed::default();
    let mut failure_stage = FailureStage::Selection;
    let result = async {
        use crate::fork::modelos_funcao::catalogo::FunctionId;
        // A conta global pertence ao Pessoal; nenhum contexto de outro Space
        // pode ser encaminhado até existir uma seleção autorizada por Space.
        if cfg.space_id != crate::fork::spaces::PERSONAL || cfg.context.is_some() {
            return Err(crate::fork::background_notice::Problem::Space.into());
        }
        let job = job_name(function);
        // MX-145: a mesma decisão fail-closed antes de consumir vaga, conta ou
        // processo — e de novo na fronteira real do `session/prompt` em `execute`.
        crate::fork::teto::background_gate(hub, &cfg.id, job)?;
        // Aguarda a vaga em ordem de chegada; não classifica concorrência como
        // falha da conta e não consome o prazo de execução enquanto espera.
        let _permit = wait_for_slot(&BACKGROUND_QUEUE, &BACKGROUND_SLOTS).await?;
        // MB-3: o resumo do dia usa a conta do próprio bot, sem seleção global nem descoberta.
        let id = match function {
            background_history::Function::Memory => Some(FunctionId::MemoryConsolidation),
            background_history::Function::Titles => Some(FunctionId::BotNaming),
            background_history::Function::DailySummary => None,
        };
        let limit = match function {
            background_history::Function::Memory | background_history::Function::DailySummary => {
                Duration::from_secs(90)
            }
            background_history::Function::Titles => Duration::from_secs(30),
        };
        let needs_discovery = id.is_some_and(|id| {
            crate::fork::modelos_funcao::config::load(&hub.store)
                .ok()
                .and_then(|doc| doc.functions.get(&id).cloned())
                .is_none_or(|selection| {
                    matches!(selection, crate::fork::modelos_funcao::config::Selection::Automatic { binding: None, .. })
                })
        });
        if needs_discovery {
            discover().await?;
        }
        async {
            let resolved = match id {
                None => crate::fork::background_selection::resolve_own(&hub.store, cfg)?,
                Some(id) => {
                    let before = crate::fork::modelos_funcao::config::load(&hub.store).ok().map(|doc| doc.revision);
                    let resolved = crate::fork::background_selection::resolve(&hub.store, cfg, id)?;
                    if id == FunctionId::MemoryConsolidation {
                        crate::fork::background_selection::backoff::clear(&hub.store);
                    }
                    let after = crate::fork::modelos_funcao::config::load(&hub.store).ok().map(|doc| doc.revision);
                    if before != after
                        && let Some(revision) = after
                    {
                        let _ = hub.events.send(json!({"type":"functionModelsChanged", "revision":revision}));
                    }
                    resolved
                }
            };
            if crate::fork::assinatura_pessoal::claude(&resolved.config.backend) {
                return Err(crate::fork::background_notice::Problem::PersonalSubscription.into());
            }
            // Não ler/materializar o pedido antes de resolver a conta real. A
            // guarda também captura sua revisão original para os awaits seguintes.
            failure_stage = FailureStage::Revalidation;
            crate::fork::background_selection::revalidate(&hub.store, &resolved)?;
            let prepared_user = boundary.map(|b| (b.prepare)(&resolved)).transpose()?;
            let user = prepared_user.as_deref().unwrap_or(user);
            check_send_boundary(hub, &resolved, boundary)?;
            failure_stage = FailureStage::Account;
            crate::fork::background_selection::verify_identity(&hub.store, &resolved).await.map_err(|error| {
                if error.downcast_ref::<crate::fork::background_notice::Problem>().is_some() {
                    error
                } else {
                    crate::fork::background_notice::Problem::AccountChanged.into()
                }
            })?;
            failure_stage = FailureStage::Revalidation;
            check_send_boundary(hub, &resolved, boundary)?;
            failure_stage = FailureStage::Execution;
            let response = tokio::time::timeout(
                limit,
                execute(hub, &resolved, system, user, exposed, &mut observed, boundary, job),
            )
            .await
            .map_err(|_| crate::fork::background_notice::Problem::Timeout)?
            .map_err(|error| classify_error(error, true))?;
            anyhow::ensure!(!*exposed, "O trabalhador solicitou uma ferramenta.");
            failure_stage = FailureStage::Revalidation;
            crate::fork::background_selection::verify_identity(&hub.store, &resolved).await?;
            Ok(JobResponse { text: response, selection: resolved, history: None })
        }
        .await
    }
    .await;
    if result.as_ref().err().is_some_and(|error| error.downcast_ref::<Busy>().is_some()) {
        return result;
    }
    // MX-145: a linha do envio já está no ledger (gravada antes do `session/prompt`);
    // aqui só entra o uso devolvido. Sem retorno => tokens NULL (`Unverified`).
    if let Some(row) = &observed.sent
        && let Some(tokens) = observed.usage.as_ref().map(|u| crate::fork::custo::UsoFundo {
            input: u.input,
            output: u.output,
            read: u.cached_input,
            write: u.cached_write,
            invalid: u.cache_invalid,
            backend: observed.backend.clone().unwrap_or_default(),
            adapter_versao: observed.adapter_versao.clone(),
        })
        && let Err(error) = crate::fork::custo::complete_background(
            &hub.store,
            row,
            observed.models.first().map(String::as_str),
            Some(tokens),
        )
    {
        tracing::error!(%error, "uso de fundo não gravado no ledger; envio segue registrado sem tokens");
    }
    let status = if result.is_ok() && !*exposed { Status::ResponseReceived } else { Status::Failed };
    let stage = if result.is_err() || *exposed { Some(failure_stage) } else { None };
    // MEX-1355: a mesma falha de seleção da memória atualiza a linha anterior da
    // Atividade (etapa "selection") e entra na espera crescente, em vez de uma
    // linha por tentativa. Contagem e linha passam pela mesma seção serializada.
    let selection_failure = match (&result, function, stage) {
        (Err(error), background_history::Function::Memory, Some(FailureStage::Selection)) => error
            .downcast_ref::<crate::fork::background_notice::Problem>()
            .and_then(crate::fork::background_selection::backoff::selection_reason),
        _ => None,
    };
    if let Some(reason) = selection_failure {
        crate::fork::background_selection::backoff::record_failure(
            &hub.store,
            reason,
            |id| background_history::touch(&hub.store, id).unwrap_or(false),
            || {
                background_history::record(&hub.store, function, &cfg.id, status, Vec::new(), None, stage)
                    .inspect_err(|error| tracing::warn!(%error, "não foi possível registrar histórico de fundo"))
                    .ok()
            },
        );
        let _ = hub.events.send(json!({"type":"backgroundHistoryChanged"}));
        return result;
    }
    let id =
        match background_history::record(&hub.store, function, &cfg.id, status, observed.models, observed.usage, stage)
        {
            Ok(id) => {
                let _ = hub.events.send(json!({"type":"backgroundHistoryChanged"}));
                Some(id)
            }
            Err(error) => {
                tracing::warn!(%error, "não foi possível registrar histórico de fundo");
                None
            }
        };
    result.map(|mut response| {
        if let Some(id) = id
            && !*exposed
        {
            response.history = Some((hub.clone(), id));
        }
        response
    })
}

fn job_name(function: crate::fork::background_history::Function) -> &'static str {
    use crate::fork::background_history::Function;
    match function {
        Function::Memory => "memory",
        Function::Titles => "titles",
        Function::DailySummary => "dailySummary",
    }
}

#[derive(Default)]
struct Observed {
    models: Vec<String>,
    usage: Option<crate::fork::background_history::Usage>,
    /// MX-145: linha do ledger do `session/prompt` que saiu (ou pode ter saído).
    sent: Option<String>,
    /// Motor selecionado e `agentInfo.version`: a política de cache ausente é
    /// por adaptador na versão medida.
    backend: Option<String>,
    adapter_versao: Option<String>,
}

#[allow(clippy::too_many_arguments)]
async fn execute(
    hub: &Arc<Hub>,
    selected: &crate::fork::background_selection::Resolved,
    system: &str,
    user: &str,
    exposed: &mut bool,
    observed: &mut Observed,
    boundary: Option<MemoryBoundary<'_>>,
    job: &'static str,
) -> Result<String> {
    let cfg = &selected.config;
    check_send_boundary(hub, selected, boundary)?;
    anyhow::ensure!(system.len().saturating_add(user.len()) <= 128_000, "A tarefa de fundo é grande demais.");
    let prepared = crate::fork::background_isolation::prepare_job(hub, cfg, system).await?;
    let cwd = prepared.cwd();
    let commands = &prepared.commands;
    let env = &prepared.env;
    let mut conn = None;
    let mut last_err = None;
    for command in commands {
        check_send_boundary(hub, selected, boundary)?;
        match crate::agent::bot::start_background_agent(command, &cwd, env, Duration::from_secs(60)).await {
            Ok(c) => {
                conn = Some(c);
                break;
            }
            Err(e) => last_err = Some(e),
        }
    }
    let mut conn = conn.ok_or_else(|| last_err.unwrap_or_else(|| anyhow!("no agent to run a background job")))?;
    if !matches!((cfg.backend.as_str(), conn.adapter), ("claude", Adapter::Claude) | ("codex", Adapter::Codex)) {
        conn.acp.kill_and_wait().await;
        bail!("O motor não confirmou isolamento para tarefa de fundo.");
    }
    observed.backend = Some(cfg.backend.clone());
    observed.adapter_versao.clone_from(&conn.adapter_version);
    if conn.adapter == Adapter::Codex && !conn.config_warnings.is_empty() {
        conn.acp.kill_and_wait().await;
        bail!("Configuração persistente do Codex impede verificar o isolamento da tarefa de fundo.");
    }
    let acp = conn.acp.clone();
    let mut out = String::new();
    let mut limit_seen = false;
    let mut output_limited = false;
    let result = async {
        let mut params = json!({"cwd": cwd, "mcpServers": []});
        let text = match conn.instructions() {
            Instructions::ClaudeMeta => {
                params["_meta"] = json!({
                    "systemPrompt": system,
                    "claudeCode": {"options": {"tools": [], "disallowedTools": ["*"], "strictMcpConfig": true, "persistSession": false, "settingSources": [], "maxTurns": 1, "model": cfg.model, "permissionMode": "default", "allowDangerouslySkipPermissions": false, "settings": {"autoMemoryEnabled": false, "autoDreamEnabled": false, "disableAllHooks": true}}},
                });
                user.to_owned()
            }
            Instructions::SystemPrompt => {
                params["systemPrompt"] = crate::agent::bot::system_prompt(system);
                user.to_owned()
            }
            Instructions::CodexConfig => user.to_owned(),
            Instructions::FirstMessage => format!("{system}\n\n---\n\n{user}"),
        };
        // session/new também pode enviar instruções; não é uma fronteira neutra.
        check_send_boundary(hub, selected, boundary)?;
        let res = acp.request("session/new", params).await?;
        if conn.adapter == Adapter::Codex {
            crate::agent::bot::verify_codex_background_mode(&res)?;
        }
        let sid = res["sessionId"].as_str().ok_or_else(|| anyhow!("agent returned no sessionId"))?.to_owned();
        // A seleção global pode usar outro motor e outra conta, sem herdar o
        // modelo da conversa. O conector precisa confirmá-la antes do pedido.
        let model_response = crate::agent::bot::select_model(&acp, &res, &sid, cfg).await?;
        verify_selection(&model_response, cfg)?;
        check_send_boundary(hub, selected, boundary)?;
        // MX-145: a fronteira real do envio de fundo — a mesma decisão fail-closed
        // do turno; qualquer indecisão recusa o prompt (trabalho autônomo).
        crate::fork::teto::background_gate(hub, &cfg.id, job)?;
        // Daqui em diante o prompt pode consumir: a linha do ledger nasce ANTES do
        // envio (tokens NULL), então cancelar este future (timeout, heartbeat,
        // queda) nunca apaga o envio; `run_job` só completa o uso. Sem linha, sem envio.
        observed.sent = Some(crate::fork::custo::begin_background(
            &hub.store,
            &cfg.id,
            job,
            cfg.model.as_deref(),
            cfg.fork_editor.engine_account_id.as_deref(),
        )?);
        let prompt = acp.request("session/prompt", json!({"sessionId": sid, "prompt": [{"type": "text", "text": text}]}));
        tokio::pin!(prompt);
        let deadline = tokio::time::sleep(TIMEOUT);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                r = &mut prompt => {
                    let response = r?;
                    observed.models = observed_models(&response);
                    observed.usage = observed_usage(&response);
                    if crate::agent::subscription::detect("claude", None, "", &response,
                        crate::store::now_ms()).is_some() { return Err(SubscriptionLimit.into()); }
                    crate::fork::background_completion::validate(&response)?;
                    break
                }
                inc = conn.rx.recv() => {
                    if !collect(&acp, inc, &mut out, exposed, &mut limit_seen, &mut output_limited).await {
                        bail!("the background job's agent exited");
                    }
                }
                () = &mut deadline => bail!("the background job timed out"),
            }
        }
        Ok(())
    }
    .await;
    // Drene também quando o agente falhou: uma ferramenta já pode ter sido observada.
    while let Ok(inc) = conn.rx.try_recv() {
        collect(&acp, Some(inc), &mut out, exposed, &mut limit_seen, &mut output_limited).await;
    }
    acp.kill_and_wait().await;
    while let Ok(inc) = conn.rx.try_recv() {
        collect(&acp, Some(inc), &mut out, exposed, &mut limit_seen, &mut output_limited).await;
    }
    if output_limited {
        Err(crate::fork::background_notice::Problem::OutputLimit.into())
    } else if limit_seen {
        Err(SubscriptionLimit.into())
    } else {
        result.map(|()| out)
    }
}

fn observed_usage(response: &serde_json::Value) -> Option<crate::fork::background_history::Usage> {
    let count = response.pointer("/_meta/quota/token_count")?;
    // MX-159/P2: ausência chega como ausência ao estimador (não como zero);
    // campo presente mas não representável marca `cache_invalid` (sem preço).
    let campo = |k: &str| (count.get(k).is_some(), count.get(k).and_then(serde_json::Value::as_u64));
    let (r, cached_input) = campo("cachedInputTokens");
    let (w, cached_write) = campo("cachedWriteTokens");
    Some(crate::fork::background_history::Usage {
        input: count.get("inputTokens")?.as_u64()?,
        output: count.get("outputTokens")?.as_u64()?,
        cached_input,
        cached_write,
        cache_invalid: (r && cached_input.is_none()) || (w && cached_write.is_none()),
    })
}

fn verify_selection(response: &serde_json::Value, cfg: &BotConfig) -> Result<()> {
    let model = cfg.model.as_deref().ok_or_else(|| anyhow!("Modelo de fundo ausente."))?;
    let actual = crate::agent::auth::model_catalog(response, &serde_json::Value::Null);
    anyhow::ensure!(actual["currentModelId"] == model, "O adaptador não confirmou o modelo de fundo.");
    for (id, value) in &cfg.model_options {
        let confirmed = response["configOptions"].as_array().is_some_and(|options| {
            options.iter().any(|option| option["id"] == *id && option["currentValue"] == *value)
        });
        anyhow::ensure!(confirmed, "O adaptador não confirmou uma opção do modelo de fundo.");
    }
    Ok(())
}

/// O adaptador informa o uso por modelo no resultado do turno. Uma lista ausente
/// significa desconhecido; o modelo solicitado não comprova qual foi executado.
fn observed_models(response: &serde_json::Value) -> Vec<String> {
    let mut models = Vec::new();
    for row in response["_meta"]["quota"]["model_usage"].as_array().into_iter().flatten().take(32) {
        if let Some(model) = row["model"].as_str()
            && !model.is_empty()
            && model.len() <= 128
            && model.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.[]".contains(&b))
            && !models.iter().any(|entry| entry == model)
        {
            models.push(model.to_owned());
        }
    }
    models
}

// Mede por empréstimo antes de renderizar/copiar o bloco; acp::content_text
// possui a mesma projeção. Assim até um único chunk excessivo é recusado antes
// de alocar a sua cópia ou acrescentá-lo à resposta acumulada.
fn output_len(value: &serde_json::Value) -> Result<usize> {
    use serde_json::Value;
    let size = match value {
        Value::Array(items) => {
            let mut total: usize = 0;
            let mut nonempty = false;
            for item in items {
                let size = output_len(item)?;
                if size > 0 {
                    total = total.saturating_add(size).saturating_add(usize::from(nonempty));
                    anyhow::ensure!(total <= MAX_OUTPUT_BYTES, crate::fork::background_notice::Problem::OutputLimit);
                    nonempty = true;
                }
            }
            total
        }
        Value::Object(object) => match object.get("type").and_then(Value::as_str) {
            Some("text") => object.get("text").and_then(Value::as_str).unwrap_or_default().len(),
            Some("content") => object.get("content").map_or(Ok(0), output_len)?,
            Some("resource_link") => object.get("uri").and_then(Value::as_str).unwrap_or_default().len(),
            Some("resource") => {
                object.get("resource").and_then(|r| r.get("text")).and_then(Value::as_str).unwrap_or_default().len()
            }
            _ => 0,
        },
        _ => 0,
    };
    anyhow::ensure!(size <= MAX_OUTPUT_BYTES, crate::fork::background_notice::Problem::OutputLimit);
    Ok(size)
}

fn append_output(out: &mut String, content: &serde_json::Value) -> Result<()> {
    let size = output_len(content)?;
    anyhow::ensure!(
        out.len().saturating_add(size) <= MAX_OUTPUT_BYTES,
        crate::fork::background_notice::Problem::OutputLimit
    );
    out.push_str(&acp::content_text(content));
    Ok(())
}

/// Keeps reply text; refuses anything the agent asks of us. False once the agent is gone.
async fn collect(
    acp: &Acp,
    inc: Option<Incoming>,
    out: &mut String,
    exposed: &mut bool,
    limit_seen: &mut bool,
    output_limited: &mut bool,
) -> bool {
    match inc {
        None | Some(Incoming::Closed { .. }) => false,
        Some(Incoming::Notification { method, params }) => {
            crate::fork::memory::dream::observe_update(&method, &params, exposed);
            let u = &params["update"];
            if method == "session/update"
                && crate::agent::subscription::detect("claude", None, "", u, crate::store::now_ms()).is_some()
            {
                *limit_seen = true;
            }
            if method == "session/update"
                && u["sessionUpdate"] == "agent_message_chunk"
                && !*output_limited
                && append_output(out, &u["content"]).is_err()
            {
                *output_limited = true;
            }
            !*output_limited
        }
        Some(Incoming::Request { id, method, .. }) => {
            // Qualquer pedido de capacidade do agente é vedado ao trabalhador,
            // mesmo quando não houve uma notificação de ferramenta anterior.
            *exposed = true;
            let _ = if method == "session/request_permission" {
                acp.respond(id, json!({"outcome": {"outcome": "cancelled"}})).await
            } else {
                acp.respond_error(id, -32601, "not supported").await
            };
            true
        }
    }
}

#[cfg(test)]
mod history_tests {
    use super::*;

    #[tokio::test]
    async fn terceiro_trabalho_aguarda_vaga_sem_falha() {
        let queue = tokio::sync::Semaphore::new(8);
        let slots = tokio::sync::Semaphore::new(2);
        let first = wait_for_slot(&queue, &slots).await.unwrap();
        let second = wait_for_slot(&queue, &slots).await.unwrap();
        let pending = wait_for_slot(&queue, &slots);
        tokio::pin!(pending);
        assert!(tokio::time::timeout(Duration::from_millis(25), &mut pending).await.is_err());
        drop(first);
        let third = tokio::time::timeout(Duration::from_secs(1), pending).await.unwrap().unwrap();
        drop((second, third));
        let full_queue = tokio::sync::Semaphore::new(0);
        assert!(wait_for_slot(&full_queue, &slots).await.err().unwrap().is::<Busy>());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn space_privado_nao_dispara_descoberta_nem_encaminha_conteudo() {
        use crate::{remote::identity::Identity, store::Store};
        async fn discovery_marker(path: &std::path::Path) -> Result<()> {
            let status = tokio::process::Command::new("/bin/sh")
                .args(["-c", "printf iniciado > \"$1\"", "marcador"])
                .arg(path)
                .status()
                .await?;
            anyhow::ensure!(status.success(), "processo de prova falhou");
            anyhow::bail!("interromper após o marcador")
        }
        let dir = std::env::temp_dir().join(format!("fundo-space-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let hub = Hub::new(store, "teste".into(), Identity::load_or_create(&dir).unwrap(), "teste".into(), 0);
        let mut cfg: BotConfig = serde_json::from_value(json!({"id":"b", "name":"Bot", "backend":"claude",
            "spaceId":uuid::Uuid::new_v4().to_string()}))
        .unwrap();
        let marker = dir.join("processo-iniciado");
        let error = run_observed_with(
            &hub,
            &cfg,
            ("instrução", "conteúdo privado"),
            &mut false,
            crate::fork::background_history::Function::Titles,
            || discovery_marker(&marker),
            None,
        )
        .await
        .err()
        .unwrap();
        assert!(matches!(
            error.downcast_ref::<crate::fork::background_notice::Problem>(),
            Some(crate::fork::background_notice::Problem::Space)
        ));
        assert_eq!(hub.store.kv_read(crate::fork::engine_accounts::SLOT).unwrap(), None);
        assert!(!marker.exists());
        cfg.space_id = crate::fork::spaces::PERSONAL.into();
        cfg.context = Some(crate::fork::spaces::ExecutionContext {
            conversation_id: "outro".into(),
            memory_id: "outra".into(),
            foreign_identity: true,
        });
        let error = run_observed_with(
            &hub,
            &cfg,
            ("instrução", "conteúdo privado"),
            &mut false,
            crate::fork::background_history::Function::Titles,
            || discovery_marker(&marker),
            None,
        )
        .await
        .err()
        .unwrap();
        assert!(matches!(
            error.downcast_ref::<crate::fork::background_notice::Problem>(),
            Some(crate::fork::background_notice::Problem::Space)
        ));
        assert_eq!(hub.store.kv_read(crate::fork::engine_accounts::SLOT).unwrap(), None);
        assert!(!marker.exists());
        cfg.context = None;
        let _ = run_observed_with(
            &hub,
            &cfg,
            ("instrução", "conteúdo de controle"),
            &mut false,
            crate::fork::background_history::Function::Titles,
            || discovery_marker(&marker),
            None,
        )
        .await;
        assert!(marker.exists(), "o controle Pessoal precisa iniciar o processo de prova");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// MEX-1355: conta escolhida desconectada → motivo próprio, uma linha só na
    /// Atividade e espera crescente; a seleção voltando zera tudo.
    #[tokio::test]
    async fn falha_de_selecao_da_memoria_vira_uma_linha_com_espera_crescente() {
        use crate::fork::{background_history, background_notice::Problem, background_selection::backoff};
        use crate::{remote::identity::Identity, store::Store};
        let dir = std::env::temp_dir().join(format!("fundo-selecao-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let hub = Hub::new(
            Store::open(std::path::Path::new(":memory:")).unwrap(),
            "teste".into(),
            Identity::load_or_create(&dir).unwrap(),
            "teste".into(),
            0,
        );
        let id = format!("ac_{}", "a".repeat(64));
        let mut registry = json!({"salt":uuid::Uuid::new_v4().to_string(), "accounts":[{
            "id":id, "backend":"claude", "billing":"subscription", "state":"connected",
            "launch":{"kind":"cli_profile", "executable":"/bin/true", "directory":"/tmp"},
            "scope":"owner", "quotaIdentity":id, "verifiedAt":1,
            "subscriptionProof":{"method":"claude_auth_status", "plan":"max"},
            "models":[{"id":"haiku", "options":[]}]
        }]});
        hub.store.kv_set(crate::fork::engine_accounts::SLOT, &registry.to_string()).unwrap();
        crate::fork::modelos_funcao::config::initialize(&hub.store).unwrap();
        registry["accounts"][0]["state"] = "disconnected".into();
        hub.store.kv_set(crate::fork::engine_accounts::SLOT, &registry.to_string()).unwrap();
        let cfg: BotConfig = serde_json::from_value(json!({"id":"b", "name":"Bot", "backend":"claude"})).unwrap();
        for attempt in 1..=3 {
            let error = run_observed_with(
                &hub,
                &cfg,
                ("instrução", "conteúdo"),
                &mut false,
                background_history::Function::Memory,
                || async { Ok(()) },
                None,
            )
            .await
            .err()
            .unwrap();
            let Some(Problem::AccountDisconnected { label }) = error.downcast_ref::<Problem>() else {
                panic!("motivo esperado: conta desconectada, veio {error:#}");
            };
            assert_eq!(label, "Claude — login deste computador");
            assert_eq!(
                crate::fork::background_notice::memory_selection_text(error.downcast_ref::<Problem>().unwrap())
                    .unwrap(),
                "Organizar memória: a conta Claude — login deste computador está desconectada; reconecte em Contas de IA ou escolha outra em Bot de fundo > Modelos por função"
            );
            assert_eq!(backoff::failures(&hub.store), attempt);
            assert!(backoff::wait_ms(&hub.store, crate::store::now_ms()).is_some(), "espera antes da próxima");
            let history = serde_json::to_value(background_history::read(&hub.store).unwrap()).unwrap();
            assert_eq!(history.as_array().unwrap().len(), 1, "uma linha enquanto o motivo é o mesmo");
            assert_eq!(history[0]["failureStage"], "selection");
            backoff::expire(&hub.store);
        }
        // Reconectar muda a configuração: a espera some sem esperar o prazo.
        registry["accounts"][0]["state"] = "connected".into();
        hub.store.kv_set(crate::fork::engine_accounts::SLOT, &registry.to_string()).unwrap();
        assert_eq!(backoff::wait_ms(&hub.store, crate::store::now_ms()), None);
        assert_eq!(backoff::failures(&hub.store), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saida_excessiva_e_recusada_antes_de_copiar_bloco_ou_acumular_chunks() {
        let mut out = String::new();
        append_output(&mut out, &json!({"type":"text","text":"á".repeat(MAX_OUTPUT_BYTES / 2)})).unwrap();
        assert_eq!(out.len(), MAX_OUTPUT_BYTES);
        assert!(matches!(
            append_output(&mut out, &json!({"type":"text","text":"x"}))
                .unwrap_err()
                .downcast_ref::<crate::fork::background_notice::Problem>(),
            Some(crate::fork::background_notice::Problem::OutputLimit)
        ));
        assert_eq!(out.len(), MAX_OUTPUT_BYTES, "chunk rejeitado não é parcialmente anexado");
        let mut empty = String::new();
        assert!(append_output(&mut empty, &json!({"type":"text","text":"x".repeat(MAX_OUTPUT_BYTES + 1)})).is_err());
        assert_eq!(empty.len(), 0);
        let blocks = json!([{"type":"text","text":"a".repeat(MAX_OUTPUT_BYTES)}, {"type":"text","text":"b"}]);
        assert!(append_output(&mut empty, &blocks).is_err());
        assert_eq!(empty.len(), 0, "também conta os separadores sem renderização antecipada");
        let content = json!([{"type":"text","text":"primeiro"}, {"type":"content","content":[
            {"type":"text","text":""},{"type":"resource","resource":{"text":"segundo"}}]}]);
        append_output(&mut empty, &content).unwrap();
        assert_eq!(empty, acp::content_text(&content));
    }

    #[test]
    fn modelos_somente_do_uso_reportado_pelo_adaptador() {
        assert_eq!(observed_models(&json!({"models":{"currentModelId":"solicitado"}})), Vec::<String>::new());
        let response = json!({"_meta":{"quota":{"model_usage":[
            {"model":"claude-haiku-4-5-20251001"}, {"model":"claude-haiku-4-5-20251001"},
            {"model":"outro-modelo"}, {"model":"inválido\n"}
        ]}}});
        assert_eq!(observed_models(&response), ["claude-haiku-4-5-20251001", "outro-modelo"]);
    }

    #[test]
    fn uso_so_quando_informado_pelo_adaptador() {
        assert!(observed_usage(&json!({})).is_none());
        let value = json!({"_meta":{"quota":{"token_count":{
            "inputTokens":123,"outputTokens":45,"cachedInputTokens":7,"cachedWriteTokens":8
        }}}});
        let usage = observed_usage(&value).unwrap();
        assert_eq!(
            (usage.input, usage.output, usage.cached_input, usage.cached_write, usage.cache_invalid),
            (123, 45, Some(7), Some(8), false)
        );
        // Ausência de cache fica ausente (não zero); presente e inválido marca.
        let usage =
            observed_usage(&json!({"_meta":{"quota":{"token_count":{"inputTokens":1,"outputTokens":2}}}})).unwrap();
        assert_eq!((usage.cached_input, usage.cached_write, usage.cache_invalid), (None, None, false));
        let usage = observed_usage(
            &json!({"_meta":{"quota":{"token_count":{"inputTokens":1,"outputTokens":2,"cachedWriteTokens":-3}}}}),
        )
        .unwrap();
        assert!(usage.cache_invalid);
        assert!(observed_usage(&json!({"_meta":{"quota":{"token_count":{"inputTokens":123}}}})).is_none());
    }

    #[test]
    fn reconhece_as_formas_estruturadas_e_mensagem_de_limite() {
        for (kind, message, data) in [
            (Some("rate_limit"), "erro", json!({})),
            (None, "You've hit your weekly limit", json!({})),
            (None, "erro", json!({"stopReason":"usage_limit_reached"})),
            (None, "erro", json!({"_meta":{"_claude/rateLimit":{"status":"rejected"}}})),
        ] {
            let rpc = acp::RpcError { code: -32603, message: message.into(), kind: kind.map(str::to_owned), data };
            assert!(classify_error(rpc.into(), true).downcast_ref::<SubscriptionLimit>().is_some());
        }
    }

    #[test]
    fn recusa_modelo_ou_esforco_nao_confirmados() {
        let mut cfg: BotConfig = serde_json::from_value(json!({"id":"b", "name":"Bot", "backend":"claude",
            "model":"haiku", "modelOptions":{"effort":"low"}}))
        .unwrap();
        let response = json!({"configOptions":[
            {"id":"model", "category":"model", "type":"select", "currentValue":"haiku", "options":[]},
            {"id":"effort", "type":"select", "currentValue":"low", "options":[]}
        ]});
        assert!(verify_selection(&response, &cfg).is_ok());
        cfg.model_options.insert("effort".into(), "high".into());
        assert!(verify_selection(&response, &cfg).is_err());
        cfg.model = Some("outro".into());
        assert!(verify_selection(&response, &cfg).is_err());
    }
}
