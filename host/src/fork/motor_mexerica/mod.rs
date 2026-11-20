//! Motor Mexerica (MEX-1293/MEX-1359): o loop do Pi vendorizado em `motor/`,
//! num processo `node` à parte ligado ao Hub pelo modo `rpc` do Pi (JSONL em
//! `stdin`/`stdout`). Skills, memória e prompt vêm do harness do Hub. As
//! ferramentas também: o Hub declara a lista (`set_tools`), o motor só pede
//! (`tool_request`) e o Hub executa pelo caminho que já existe para os outros
//! motores e devolve `tool_result` ([`tools`]). O motor NUNCA conecta direto
//! a servidor MCP. A chave de API (Google ou `OpenAI`) sai do cofre do Hub e
//! chega ao motor SOMENTE pelo canal do cofre (`fd 3`, JSON-RPC
//! `credential.read`): não é colocada em `argv`, ambiente ou URL. O `AuthContext`
//! do `pi-ai` no motor é vazio, então credencial ambiente não resolve nada.
//!
//! Feature flag `MEXERICA_MOTOR=1`, desligada por padrão: desligada, o motor
//! não é registrado (`backends::harness`/`is_known` ignoram o id `mexerica`)
//! e nenhum processo é aberto (`launch_candidates` e `Engine::spawn` recusam).

use crate::store::Store;
use anyhow::{Context, Result, anyhow, bail, ensure};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[cfg(unix)]
pub(crate) mod chaves;
#[cfg(unix)]
pub(crate) mod historico;
#[cfg(unix)]
pub(crate) mod link;
pub(crate) mod mcp_host;
pub(crate) mod tools;
pub(crate) use tools::{ToolRunner, ToolSpec};

pub(crate) const ENGINE_ID: &str = "mexerica";
/// Feature flag: `MEXERICA_MOTOR=1` registra e permite abrir o motor.
pub(crate) const FLAG: &str = "MEXERICA_MOTOR";
/// Caminho do entrypoint Node; em desenvolvimento cai no `motor/` do checkout.
pub(crate) const ENTRY_ENV: &str = "MEXERICA_MOTOR_ENTRY";
/// Liga o provedor `faux` local (eco), somente para testes do Hub e do motor.
pub(crate) const FAUX_ENV: &str = "MEXERICA_MOTOR_FAUX";
/// Marca CI/prova obrigatória: com `MEXERICA_MOTOR_TESTS_REQUIRED=1`, os testes
/// que abrem o motor falham em vez de pular quando falta pré-requisito.
#[cfg(test)]
pub(crate) const REQUIRED_ENV: &str = "MEXERICA_MOTOR_TESTS_REQUIRED";

/// Leitor do cofre injetado no processo do motor: recebe o slot
/// `motor-mexerica/<provider>` e devolve a chave, sem nunca registrar o valor.
/// No bot o leitor fecha sobre o `Arc<Hub>`; nos testes, sobre um `Arc<Store>`.
pub(crate) type VaultSource = Arc<dyn Fn(&str) -> Result<Option<String>> + Send + Sync>;

/// Fonte do cofre sobre um `Store` próprio (provas e testes).
pub(crate) fn store_vault(store: Arc<Store>) -> VaultSource {
    Arc::new(move |slot: &str| crate::market::vault::read(&store, slot))
}

/// Fonte do cofre sobre o `Hub` do processo (o caminho do bot).
pub(crate) fn hub_vault(hub: Arc<crate::hub::Hub>) -> VaultSource {
    Arc::new(move |slot: &str| crate::market::vault::read(&hub.store, slot))
}

/// Métodos do app para as chaves do motor (veja `chaves`).
pub(crate) const CHAVE_METHODS: [&str; 4] =
    ["motorChaves", "motorChaveTestar", "motorChaveSalvar", "motorChaveRemover"];

pub(crate) async fn chaves_call(
    hub: &Arc<crate::hub::Hub>,
    caller: &crate::api::devices::Caller,
    method: &str,
    body: Value,
) -> Result<Value> {
    #[cfg(not(unix))]
    {
        let _ = (hub, caller, method, body);
        bail!("motor_desligado: o Motor Mexerica não roda nesta plataforma.")
    }
    #[cfg(unix)]
    {
        chaves::call(hub, caller, method, body).await
    }
}

/// Modelos da escolha do bot no backend do motor (veja `chaves::catalogo`).
pub(crate) fn catalogo(store: &Store) -> Result<Value> {
    #[cfg(not(unix))]
    {
        let _ = store;
        Ok(json!({"models": [], "currentModelId": null, "fastMode": false}))
    }
    #[cfg(unix)]
    {
        chaves::catalogo(store)
    }
}

/// Slot do cofre por provedor: `motor-mexerica/<provider>`.
pub(crate) fn credential_slot(provider_id: &str) -> String {
    format!("motor-mexerica/{provider_id}")
}

/// Provedores do motor: cada um com o seu slot no cofre (as chaves de todos saem do texto da conversa guardada).
#[cfg(unix)]
const PROVIDERS: [&str; 8] = ["anthropic", "openai", "google", "xai", "openrouter", "meta", "faux", "faux2"];

#[cfg(unix)]
const OWNER_SEPARATOR: char = '\u{1f}';

/// O dono de uma conversa guardada: o bot e a identidade com que a sessão abriu (Space, conta, revisão e perfil de
/// login). Outra identidade nunca reabre a conversa.
#[cfg(unix)]
pub(crate) fn conversation_owner(bot: &str, identity: &str) -> String {
    format!("{bot}{OWNER_SEPARATOR}{identity}")
}

/// Apaga a conversa guardada de UMA sessão do bot (a sessão foi invalidada). Sem o motor (fora do Unix) não há
/// conversa guardada, e o ator chama isto sem `cfg` espalhado.
#[cfg(unix)]
pub(crate) fn forget_conversation(bot: &str, session: &str) {
    historico::forget_session(bot, session);
}

#[cfg(not(unix))]
pub(crate) fn forget_conversation(_bot: &str, _session: &str) {}

fn enabled_value(flag: Option<&std::ffi::OsStr>) -> bool {
    flag.is_some_and(|v| v == "1")
}

/// Ambiente extra do motor aberto pelo ator do bot: vazio em produção; só as provas
/// ligam o provedor falso (ator real sobre o motor real).
#[cfg(not(test))]
pub(crate) fn actor_extra_env() -> &'static [(&'static str, &'static str)] {
    &[]
}

/// Só para provas: o ator do bot abre o motor com o provedor falso e o adaptador não abre os servidores MCP do Hub.
#[cfg(test)]
pub(crate) static ACTOR_FAUX: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
pub(crate) fn actor_extra_env() -> &'static [(&'static str, &'static str)] {
    if ACTOR_FAUX.load(std::sync::atomic::Ordering::Relaxed) { &[(FAUX_ENV, "1")] } else { &[] }
}

/// Desligada por padrão: só `MEXERICA_MOTOR=1` registra o motor.
pub(crate) fn enabled() -> bool {
    !crate::environment::Environment::is_open() && enabled_value(std::env::var_os(FLAG).as_deref())
}

/// Entrypoint `motor/mexerica/rpc.ts`; `MEXERICA_MOTOR_ENTRY` sobrepõe
/// (desenvolvimento, testes e futura pasta instalada).
pub(crate) fn entry_path() -> PathBuf {
    if let Some(path) = std::env::var_os(ENTRY_ENV) {
        return PathBuf::from(path);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../motor/mexerica/rpc.ts")
}

pub(crate) fn node_path() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(path) = std::env::var_os("DONNABOT_MEX_NODE_TEST") {
        return Some(path.into());
    }
    crate::agent::backends::which("node")
}

/// `node <entry>`: a chave NUNCA vai em argv. Flag desligada recusa aqui, na
/// fronteira efetiva de lançamento — não só no registro por id.
pub(crate) fn launch_candidates() -> Result<Vec<crate::agent::registry::Cmd>> {
    ensure!(enabled(), "Motor Mexerica desligado (defina {FLAG}=1)");
    bail!("Motor Mexerica ainda não está ligado ao lançamento genérico")
}

/// O processo do motor não existe mais (saída fechada) ou não aceita escrita.
#[derive(Debug)]
pub(crate) struct EngineGone;

impl std::fmt::Display for EngineGone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("o processo do motor encerrou a saída")
    }
}

impl std::error::Error for EngineGone {}

/// O erro é "o motor morreu" (EOF no stdout ou pipe de stdin quebrado)?
pub(crate) fn engine_gone(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.is::<EngineGone>()
            || cause.downcast_ref::<std::io::Error>().is_some_and(|io| io.kind() == std::io::ErrorKind::BrokenPipe)
    })
}

/// Fronteira de abertura do motor: flag ligada, ajudante de UID separado e
/// fora do worker de instalação. Vale para o `Engine` e para tudo que o
/// antecede no caminho do bot (servidores MCP do `session/new`): desligada,
/// nenhum processo — nem de motor, nem de servidor MCP.
pub(crate) fn ensure_runnable() -> Result<()> {
    ensure!(enabled(), "Motor Mexerica desligado (defina {FLAG}=1)");
    ensure!(
        cfg!(target_os = "linux") && crate::fork::motor_exec::configured(),
        "Motor Mexerica exige identidade separada pelo ajudante Linux"
    );
    ensure!(!crate::fork::motor_install::worker(), "motor indisponível no worker de instalação");
    Ok(())
}

/// Um processo do motor preso a um canal de cofre (fd 3) e a uma conversa.
/// Descartar o `Engine` sem `finish()`/`kill()` também encerra o grupo do
/// processo, colhe o filho e aborta as tarefas (veja o `Drop`).
#[cfg(unix)]
pub(crate) struct Engine {
    child: Option<crate::fork::motor_exec::Child>,
    stdin: Option<tokio::process::ChildStdin>,
    reader: BufReader<tokio::process::ChildStdout>,
    line_buf: Vec<u8>,
    stderr: Option<tokio::task::JoinHandle<Vec<u8>>>,
    vault: Option<tokio::task::JoinHandle<Result<()>>>,
    provider: String,
    /// Geração da credencial com que o cofre deste processo foi preso (ver `link::pinned`); vazia = não preso.
    pub(super) credential: String,
    /// Um turno largado no meio deixa eventos velhos no stdout: prompt novo com a marca presa é recusado.
    turn_open: bool,
    /// Nomes declarados por `set_tools`: um `tool_request` fora da lista nunca chega ao executor.
    tools: std::collections::HashSet<String>,
    _home: PrivateHome,
    next_id: u64,
}

/// A pasta é criada e removida pelo ajudante já na identidade do motor.
#[cfg(unix)]
struct PrivateHome(PathBuf);

#[cfg(unix)]
impl Drop for PrivateHome {
    fn drop(&mut self) {
        // `remove` é um exec síncrono: fora da thread do runtime quando há um.
        let dir = std::mem::take(&mut self.0);
        let remove = move || {
            if let Err(error) = crate::fork::motor_background::remove(&dir) {
                tracing::warn!(%error, "não foi possível remover a pasta privada do motor");
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => drop(runtime.spawn_blocking(remove)),
            Err(_) => remove(),
        }
    }
}

/// Resposta final de um turno: o texto da última mensagem do assistente.
#[cfg(unix)]
pub(crate) struct Turn {
    pub text: String,
    pub events: Vec<Value>,
}

#[cfg(unix)]
impl Engine {
    /// Abre `node <entry>` preso a UM provedor: o cofre responde só o slot
    /// `motor-mexerica/<provider>` deste processo e a escrita fica desligada.
    /// Ambiente mínimo explícito (nada herdado além de `PATH`/localização) e
    /// `HOME`/`XDG_*` privados em pasta 0700 — sem as credenciais nem a pasta
    /// pessoal do Hub no ambiente.
    /// `extra_env` é só para provas (`MEXERICA_MOTOR_FAUX=1`); credencial
    /// jamais entra aí.
    pub(crate) fn spawn(store: Arc<Store>, provider: &str, extra_env: &[(&str, &str)]) -> Result<Self> {
        Self::spawn_with(store_vault(store), provider, extra_env)
    }

    /// Igual a [`Engine::spawn`], com o cofre injetado (o bot lê pelo `Hub`).
    pub(crate) fn spawn_with(vault_source: VaultSource, provider: &str, extra_env: &[(&str, &str)]) -> Result<Self> {
        // A flag vale na fronteira efetiva também: desligada, nenhum processo.
        ensure_runnable()?;
        ensure!(
            extra_env.iter().all(|(k, v)| cfg!(test) && *k == FAUX_ENV && *v == "1"),
            "ambiente adicional não permitido"
        );
        let node = node_path().ok_or_else(|| anyhow!("o Motor Mexerica precisa de Node.js instalado"))?;
        let entry = entry_path();
        ensure!(entry.is_file(), "entrypoint do Motor Mexerica ausente: {}", entry.display());

        // Criado como motor, 0700; o Hub não precisa abrir essa pasta.
        let home = PrivateHome(crate::fork::motor_background::create()?);
        let env: Vec<(std::ffi::OsString, std::ffi::OsString)> = vec![
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("HOME".into(), home.0.clone().into_os_string()),
            ("XDG_CONFIG_HOME".into(), home.0.join("config").into_os_string()),
            ("XDG_DATA_HOME".into(), home.0.join("data").into_os_string()),
            ("XDG_CACHE_HOME".into(), home.0.join("cache").into_os_string()),
            ("XDG_STATE_HOME".into(), home.0.join("state").into_os_string()),
            ("TMPDIR".into(), home.0.clone().into_os_string()),
            ("LANG".into(), "C.UTF-8".into()),
        ];

        // Canal do cofre: socket par bidirecional; o filho o vê como fd 3.
        // NONBLOCK dos dois lados: o lado Hub entra no executor do Tokio.
        let (child_end, hub_end) =
            std::os::unix::net::UnixStream::pair().context("socketpair do canal do cofre falhou")?;
        child_end.set_nonblocking(true).context("nonblocking do canal do cofre falhou")?;
        hub_end.set_nonblocking(true).context("nonblocking do canal do cofre falhou")?;
        let hub_socket = tokio::net::UnixStream::from_std(hub_end).context("canal do cofre não entrou no Tokio")?;

        // A pasta de trabalho é a pasta privada do motor (0700, dele), nunca o /tmp compartilhado.
        let mut command = crate::fork::motor_exec::command(&node, &home.0.join("work"));
        command
            .arg(&entry)
            .env_clear()
            .envs(env)
            .envs(extra_env.iter().map(|(k, v)| (std::ffi::OsString::from(k), std::ffi::OsString::from(v))))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        {
            use std::os::fd::AsRawFd;
            let child_fd = child_end.as_raw_fd();
            // SAFETY: pre_exec roda no filho antes do exec; só mexe em descritores.
            unsafe {
                command.pre_exec(move || {
                    // Marca todos os demais descritores para fechar no exec, inclusive os sem CLOEXEC.
                    #[cfg(target_os = "linux")]
                    if libc::syscall(libc::SYS_close_range, 4_u32, u32::MAX, 4_u32) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::dup2(child_fd, 3) == -1 || libc::fcntl(3, libc::F_SETFD, 0) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        let mut child = crate::fork::motor_exec::spawn(&mut command).context("abrindo o processo do Motor Mexerica")?;
        // Fecha a cópia do Hub: o filho ficou com o canal no fd 3.
        drop(child_end);
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("stdin do motor ausente"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("stdout do motor ausente"))?;
        let mut stderr = child.stderr.take().ok_or_else(|| anyhow!("stderr do motor ausente"))?;
        let stderr_task = tokio::spawn(async move {
            let mut captured = Vec::new();
            tokio::io::AsyncReadExt::read_to_end(&mut stderr, &mut captured).await.ok();
            captured
        });
        let vault = tokio::spawn(serve_vault(vault_source, hub_socket, provider.to_owned()));
        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            reader: BufReader::new(stdout),
            line_buf: Vec::new(),
            stderr: Some(stderr_task),
            vault: Some(vault),
            provider: provider.to_owned(),
            credential: String::new(),
            turn_open: false,
            tools: std::collections::HashSet::new(),
            _home: home,
            next_id: 0,
        })
    }

    async fn send(&mut self, command: Value) -> Result<()> {
        let mut line = serde_json::to_string(&command)?;
        line.push('\n');
        let stdin = self.stdin.as_mut().ok_or_else(|| anyhow!("o motor já foi encerrado"))?;
        stdin.write_all(line.as_bytes()).await?;
        stdin.flush().await?;
        Ok(())
    }

    async fn next_line(&mut self) -> Result<Value> {
        let n = mcp_host::read_line(&mut self.reader, &mut self.line_buf).await?;
        if n == 0 {
            return Err(EngineGone.into());
        }
        serde_json::from_slice(&self.line_buf).context("linha RPC inválida do motor")
    }

    /// Espera a resposta ao comando `id` e ignora eventos soltos antes dela.
    async fn response(&mut self, id: &str) -> Result<Value> {
        loop {
            let line = self.next_line().await?;
            if line["type"] == "response" && line["id"] == id {
                return Ok(line);
            }
        }
    }

    /// `set_model` do provedor fixado no nascimento. Fora de um turno.
    pub(crate) async fn set_model(&mut self, provider: &str, model_id: &str) -> Result<()> {
        ensure!(provider == self.provider, "este motor está preso ao provedor {}", self.provider);
        self.next_id += 1;
        let set_id = format!("set-{}", self.next_id);
        self.send(json!({"id": set_id, "type": "set_model", "provider": provider, "modelId": model_id})).await?;
        let res = self.response(&set_id).await?;
        ensure!(res["success"] == true, "set_model recusado: {}", res["error"].as_str().unwrap_or_default());
        Ok(())
    }

    /// «Testar acesso»: a lista de modelos que a chave do cofre enxerga, como o motor a devolveu
    /// (`{ok, modelos}` ou `{ok: false, erro, status}`). Nenhuma conversa é aberta.
    pub(crate) async fn list_models(&mut self) -> Result<Value> {
        self.next_id += 1;
        let id = format!("list-{}", self.next_id);
        self.send(json!({"id": id, "type": "list_models", "provider": self.provider})).await?;
        let res = self.response(&id).await?;
        ensure!(
            res["success"] == true,
            "o motor recusou o teste de acesso: {}",
            res["error"].as_str().unwrap_or_default()
        );
        Ok(res["data"].clone())
    }

    /// Provedor fixado no nascimento do processo.
    pub(crate) fn provider(&self) -> &str {
        &self.provider
    }

    /// `set_model` + `prompt` + eventos até `agent_end`/`error`, sem ferramentas.
    /// Devolve o texto final da assistente e todos os eventos recebidos (para
    /// provas). O provedor precisa ser o fixado no nascimento do processo.
    pub(crate) async fn prompt(&mut self, provider: &str, model_id: &str, message: &str) -> Result<Turn> {
        self.prompt_with(provider, model_id, message, &tools::NoTools, &mut |_| {}).await
    }

    /// Substitui a lista inteira de ferramentas do motor (vai antes do prompt).
    /// O motor só declara as ferramentas ao modelo; quem executa é o Hub.
    pub(crate) async fn set_tools(&mut self, specs: &[ToolSpec]) -> Result<()> {
        self.next_id += 1;
        let id = format!("tools-{}", self.next_id);
        self.send(
            json!({"id": id, "type": "set_tools", "tools": specs.iter().map(ToolSpec::wire).collect::<Vec<_>>()}),
        )
        .await?;
        let res = self.response(&id).await?;
        ensure!(res["success"] == true, "set_tools recusado: {}", res["error"].as_str().unwrap_or_default());
        self.tools = specs.iter().map(|spec| spec.name.clone()).collect();
        Ok(())
    }

    /// Prompt de sistema vindo do harness do Hub (nunca de pastas nativas).
    pub(crate) async fn set_system_prompt(&mut self, prompt: &str) -> Result<()> {
        self.next_id += 1;
        let id = format!("sys-{}", self.next_id);
        self.send(json!({"id": id, "type": "set_system_prompt", "prompt": prompt})).await?;
        let res = self.response(&id).await?;
        ensure!(res["success"] == true, "set_system_prompt recusado: {}", res["error"].as_str().unwrap_or_default());
        Ok(())
    }

    /// Devolve ao processo novo a conversa que o Hub guardou (cancelamento,
    /// troca de provedor ou queda mataram o processo antigo). Vai depois do
    /// prompt de sistema e das ferramentas, antes do prompt. Fora de um turno.
    pub(crate) async fn restore_history(&mut self, messages: &[Value]) -> Result<()> {
        self.next_id += 1;
        let id = format!("hist-{}", self.next_id);
        // Cada mensagem cruza a fronteira como o texto JSON que o Hub mediu: o motor mede esses mesmos bytes.
        let wire: Vec<String> = messages.iter().map(Value::to_string).collect();
        self.send(json!({"id": id, "type": "restore_history", "messages": wire})).await?;
        let res = self.response(&id).await?;
        ensure!(res["success"] == true, "restore_history recusado: {}", res["error"].as_str().unwrap_or_default());
        Ok(())
    }

    /// Como [`Engine::prompt`], mas atende cada `tool_request` com `runner` e
    /// entrega cada evento a `on_event` (streaming). Cancelar = largar este
    /// futuro e chamar [`Engine::kill`]: o motor não executa nada por conta própria.
    pub(crate) async fn prompt_with(
        &mut self,
        provider: &str,
        model_id: &str,
        message: &str,
        runner: &dyn ToolRunner,
        on_event: &mut (dyn FnMut(&Value) + Send),
    ) -> Result<Turn> {
        ensure!(!self.turn_open, "há um turno aberto neste motor; encerre ou mate o processo antes de outro prompt");
        self.turn_open = true;
        let result = self.drive(provider, model_id, message, runner, on_event).await;
        self.turn_open = false;
        result
    }

    /// O laço do turno em si: `prompt` + eventos até `agent_end`/`error`.
    /// Eventos só são acumulados em `cfg(test)` (provas): em produção cada um
    /// vai a `on_event` e morre — nada cresce ao quadrado da resposta.
    async fn drive(
        &mut self,
        provider: &str,
        model_id: &str,
        message: &str,
        runner: &dyn ToolRunner,
        on_event: &mut (dyn FnMut(&Value) + Send),
    ) -> Result<Turn> {
        self.set_model(provider, model_id).await?;
        self.next_id += 1;
        self.send(json!({"id": format!("p-{}", self.next_id), "type": "prompt", "message": message})).await?;

        let mut events = Vec::new();
        let mut text = String::new();
        loop {
            let event = self.next_line().await?;
            on_event(&event);
            match event["type"].as_str() {
                Some("message_end") => {
                    let msg = &event["message"];
                    ensure!(
                        msg["stopReason"] != "error",
                        "o motor falhou no turno: {}",
                        msg["errorMessage"].as_str().unwrap_or("erro sem detalhe")
                    );
                    if msg["role"] == "assistant" {
                        text = msg["content"]
                            .as_array()
                            .map(|c| {
                                c.iter()
                                    .filter(|b| b["type"] == "text")
                                    .filter_map(|b| b["text"].as_str())
                                    .collect::<String>()
                            })
                            .unwrap_or_default();
                    }
                }
                Some("tool_request") => self.answer_tool(&event, runner).await?,
                Some("error") => bail!("o motor falhou: {}", event["error"].as_str().unwrap_or("erro sem detalhe")),
                Some("agent_end") => {
                    if cfg!(test) {
                        events.push(event);
                    }
                    break;
                }
                _ => {}
            }
            if cfg!(test) {
                events.push(event);
            }
        }
        Ok(Turn { text, events })
    }

    /// `tool_request` → executor do Hub → `tool_result`. Nome fora da lista
    /// declarada, `callId` ausente e execução recusada viram `ok:false`: nada
    /// fora da lista chega ao executor e a negação de permissão volta ao modelo.
    async fn answer_tool(&mut self, request: &Value, runner: &dyn ToolRunner) -> Result<()> {
        let call_id = request["callId"].as_str().ok_or_else(|| anyhow!("tool_request sem callId"))?.to_owned();
        let name = request["name"].as_str().unwrap_or_default();
        let reply = if self.tools.contains(name) {
            runner.call(&call_id, name, request["input"].clone()).await
        } else {
            Err("ferramenta não declarada para este motor".to_owned())
        };
        let message = match reply {
            Ok(output) => json!({"type": "tool_result", "callId": call_id, "ok": true, "output": tools::cap(output)}),
            Err(error) => json!({"type": "tool_result", "callId": call_id, "ok": false, "error": tools::cap(error)}),
        };
        self.send(message).await
    }

    /// O processo do motor ainda está vivo? `try_wait` não bloqueia; um filho
    /// já colhido, morto (OOM, SIGKILL) ou sem estado conhecido não está vivo.
    pub(crate) fn alive(&mut self) -> bool {
        self.child.as_mut().is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }

    /// Pid do processo do motor (prova de argv, sem ler o ambiente).
    pub(crate) fn pid(&self) -> Option<u32> {
        self.child.as_ref().and_then(|child| child.id())
    }

    /// Encerra o motor já (cancelamento do turno): mata o grupo pelo ajudante,
    /// espera o filho e encerra as tarefas. Idempotente.
    pub(crate) async fn kill(&mut self) -> Result<()> {
        self.stdin = None;
        let result = match self.child.take() {
            Some(mut child) => child.kill().await.context("encerrando o motor"),
            None => Ok(()),
        };
        self.stop_tasks().await;
        result
    }

    /// Aborta a tarefa do cofre e espera as duas tarefas terminarem; devolve o stderr.
    async fn stop_tasks(&mut self) -> Vec<u8> {
        if let Some(vault) = self.vault.take() {
            vault.abort();
            let _ = vault.await;
        }
        match self.stderr.take() {
            Some(task) => task.await.unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// Fim ordenado: fecha o stdin, espera o filho (sem zumbi), encerra a
    /// tarefa do cofre e devolve o stderr capturado.
    pub(crate) async fn finish(mut self) -> Result<(Vec<u8>, Value)> {
        self.stdin = None;
        let mut status = None;
        if let Some(mut child) = self.child.take() {
            if let Ok(waited) = tokio::time::timeout(std::time::Duration::from_secs(10), child.wait()).await {
                status = Some(waited?);
            } else {
                child.kill().await.context("o motor não saiu sozinho e não aceitou o kill")?;
            }
        }
        let stderr = self.stop_tasks().await;
        Ok((stderr, json!({"status": status.and_then(|s| s.code())})))
    }
}

#[cfg(unix)]
impl Drop for Engine {
    /// Descartado sem `finish()`/`kill()`: o grupo do motor morre agora, o
    /// filho é colhido em segundo plano e a tarefa do cofre (que segura o
    /// `Arc<Store>`) é abortada.
    fn drop(&mut self) {
        if let Some(vault) = self.vault.take() {
            vault.abort();
        }
        if let Some(stderr) = self.stderr.take() {
            stderr.abort();
        }
        self.stdin = None;
        if let Some(child) = self.child.take() {
            child.terminate_and_reap();
        }
    }
}

/// JSON-RPC do cofre no fd 3, preso ao provedor fixado no nascimento do
/// processo: `credential.read` só responde o slot `motor-mexerica/<provider>`
/// desta instância; outro provedor é recusado. Escrita (`credential.modify`)
/// está desligada nesta fatia — nada cria, substitui ou apaga credencial.
/// Nenhum valor vai a log.
#[cfg(unix)]
async fn serve_vault(vault_source: VaultSource, socket: tokio::net::UnixStream, provider: String) -> Result<()> {
    let slot = credential_slot(&provider);
    let (read, mut write) = socket.into_split();
    let mut lines = BufReader::new(read).lines();
    while let Some(line) = lines.next_line().await? {
        let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
        let id = req["id"].clone();
        let reply = match req["method"].as_str() {
            Some("credential.read") if req["params"]["providerId"].as_str() == Some(provider.as_str()) => {
                match vault_source(&slot) {
                    Ok(Some(key)) => json!({"id": id, "result": {"type": "api_key", "key": key}}),
                    Ok(None) => json!({"id": id, "result": null}),
                    Err(e) => json!({"id": id, "error": {"message": format!("{e:#}")}}),
                }
            }
            Some("credential.read") => {
                json!({"id": id, "error": {"message": "provedor não autorizado neste processo"}})
            }
            Some("credential.modify") => {
                json!({"id": id, "error": {"message": "escrita no cofre desligada nesta fatia"}})
            }
            _ => json!({"id": id, "error": {"message": "método desconhecido"}}),
        };
        let mut out = serde_json::to_string(&reply)?;
        out.push('\n');
        write.write_all(out.as_bytes()).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_tools;

#[cfg(test)]
mod tests_link;

#[cfg(test)]
mod tests_paridade;

#[cfg(test)]
mod tests_chaves;
