//! O agente ACP da ponte: atende o Hub pela entrada/saída padrão e conduz, por sessão, um processo do programa
//! oficial no modo sem tela (`claude -p --input-format stream-json --output-format stream-json`). Um processo
//! vive enquanto a sessão estiver aberta; se cair, ou se o modelo mudar, o turno seguinte abre outro com
//! `--resume` na mesma sessão. Cancelar envia SIGINT, que o programa documenta como fim do turno; fora do Unix não
//! há esse sinal e o cancelamento vale pelo prazo e pelo encerramento do processo.

use super::opcoes::{self, Arquivos, ESFORCOS, Opcoes, PromptSistema};
use super::permissao;
use super::traducao::Traducao;
use crate::LockExt;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{mpsc, oneshot, watch};

/// ACP: "authentication required".
const AUTH_REQUIRED: i64 = -32000;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL: i64 = -32603;
/// Quanto o turno espera o fim pedido por SIGINT antes de encerrar o processo.
const PRAZO_CANCELAR: Duration = Duration::from_secs(10);
/// Quanto um pedido de permissão espera o anúncio da chamada aparecer no fluxo.
const PRAZO_ANUNCIO: Duration = Duration::from_secs(3);

/// Apelidos de modelo documentados pelo Claude Code; o programa resolve cada um para a conta da pessoa.
const MODELOS: &[(&str, &str)] = &[
    ("default", "Default"),
    ("opus", "Opus"),
    ("sonnet", "Sonnet"),
    ("haiku", "Haiku"),
    ("fable", "Fable"),
    ("opus[1m]", "Opus (1M context)"),
    ("sonnet[1m]", "Sonnet (1M context)"),
];

/// Onde está o programa oficial e onde ele guarda as sessões.
#[derive(Clone)]
pub(super) struct Config {
    pub exe: Option<PathBuf>,
    pub pasta_config: Option<PathBuf>,
}

impl Config {
    pub(super) fn do_ambiente() -> Self {
        let exe = std::env::var_os("CLAUDE_CODE_EXECUTABLE").map(PathBuf::from).filter(|p| p.is_file()).or_else(|| {
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|d| d.join(if cfg!(windows) { "claude.exe" } else { "claude" }))
                .find(|p| p.is_file())
        });
        let pasta_config = std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude")));
        Self { exe, pasta_config }
    }
}

#[derive(Debug)]
struct Erro {
    code: i64,
    message: String,
    data: Option<Value>,
}

impl Erro {
    fn interno(message: impl Into<String>) -> Self {
        Self { code: INTERNAL, message: message.into(), data: None }
    }
    fn parametros(message: impl Into<String>) -> Self {
        Self { code: INVALID_PARAMS, message: message.into(), data: None }
    }
    fn login() -> Self {
        Self {
            code: AUTH_REQUIRED,
            message: "Authentication required: entre na conta do Claude Code.".into(),
            data: None,
        }
    }
}

impl From<anyhow::Error> for Erro {
    fn from(e: anyhow::Error) -> Self {
        Self::parametros(format!("{e:#}"))
    }
}

struct Processo {
    filho: Child,
    entrada: ChildStdin,
    /// Configuração MCP (segredo da ponte e variáveis dos servidores do Hub): apagada assim que o programa a leu.
    mcp: Option<PathBuf>,
    eventos: mpsc::UnboundedReceiver<Value>,
    _permissao: permissao::Servidor,
    _pasta: tempfile::TempDir,
    /// Uso por modelo no último resultado deste processo, para separar o turno do acumulado.
    modelos: Option<HashMap<String, [u64; 4]>>,
    retomado: bool,
}

struct Sessao {
    id: String,
    opcoes: Opcoes,
    modelo: Mutex<String>,
    esforco: Mutex<String>,
    /// A sessão já existe no disco do programa: o próximo processo usa `--resume`.
    iniciada: AtomicBool,
    /// Modelo ou esforço mudou: o processo atual precisa ser trocado no próximo turno.
    reabrir: AtomicBool,
    processo: tokio::sync::Mutex<Option<Processo>>,
    cancelar: watch::Sender<u64>,
    traducao: Mutex<Traducao>,
    /// Há um turno em andamento: pedido de permissão fora dele é negado.
    turno_ativo: AtomicBool,
    /// Chamadas que já tiveram um pedido de permissão: o mesmo id não é decidido duas vezes.
    decididas: Mutex<HashSet<String>>,
    /// Avança a cada fim de turno: pedido de permissão ainda aberto daquele turno é negado.
    fim_turno: watch::Sender<u64>,
}

struct Ponte {
    cfg: Config,
    saida: mpsc::UnboundedSender<String>,
    sessoes: Mutex<HashMap<String, Arc<Sessao>>>,
    pendentes: Mutex<HashMap<i64, oneshot::Sender<Value>>>,
    proximo: AtomicI64,
}

impl Ponte {
    fn enviar(&self, msg: &Value) {
        let _ = self.saida.send(msg.to_string());
    }

    fn atualizar(&self, sessao: &str, update: &Value) {
        self.enviar(
            &json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sessao, "update": update}}),
        );
    }

    async fn pedir(&self, metodo: &str, params: Value) -> Option<Value> {
        /// Pedido abandonado (turno cancelado) sai da fila de respostas esperadas.
        struct Pendente<'a>(&'a Ponte, i64);
        impl Drop for Pendente<'_> {
            fn drop(&mut self) {
                self.0.pendentes.locked().remove(&self.1);
            }
        }
        let id = self.proximo.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pendentes.locked().insert(id, tx);
        let _pendente = Pendente(self, id);
        self.enviar(&json!({"jsonrpc": "2.0", "id": id, "method": metodo, "params": params}));
        let resposta = rx.await.ok()?;
        resposta.get("error").is_none().then(|| resposta["result"].clone())
    }

    fn sessao(&self, params: &Value) -> Result<Arc<Sessao>, Erro> {
        let id = params["sessionId"].as_str().unwrap_or_default();
        self.sessoes.locked().get(id).cloned().ok_or_else(|| Erro::parametros("sessão desconhecida"))
    }
}

fn opcoes_config(modelo: &str, esforco: &str) -> Value {
    let mut modelos: Vec<Value> = MODELOS.iter().map(|(v, n)| json!({"value": v, "name": n})).collect();
    if !MODELOS.iter().any(|(v, _)| *v == modelo) {
        modelos.push(json!({"value": modelo, "name": modelo}));
    }
    let esforcos: Vec<Value> =
        ESFORCOS.iter().map(|e| json!({"value": e, "name": if *e == "default" { "Default" } else { e }})).collect();
    json!([
        {"id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": "default",
            "options": [{"value": "default", "name": "Default"}]},
        {"id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": modelo, "options": modelos},
        {"id": "effort", "name": "Effort", "category": "thought_level", "type": "select", "currentValue": esforco,
            "options": esforcos},
    ])
}

/// `claude auth status`: método de login sem token e sem gasto. Só o campo `loggedIn` é lido.
pub(super) async fn conectado(exe: &Path) -> Option<bool> {
    let mut cmd = tokio::process::Command::new(exe);
    cmd.args(["auth", "status"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    let saida = tokio::time::timeout(Duration::from_secs(20), cmd.output()).await.ok()?.ok()?;
    serde_json::from_slice::<Value>(&saida.stdout).ok().and_then(|v| v["loggedIn"].as_bool())
}

/// Depois de um erro do programa: só troca o erro pelo pedido de login quando `auth status` confirma a saída; se a
/// conferência falhar, vale o erro original.
async fn login_recusado(cfg: &Config) -> bool {
    match cfg.exe.as_deref() {
        Some(exe) => conectado(exe).await == Some(false),
        None => false,
    }
}

async fn exigir_login(cfg: &Config) -> Result<(), Erro> {
    let exe = cfg.exe.as_deref().ok_or_else(|| Erro::interno("Claude Code não está instalado"))?;
    match conectado(exe).await {
        Some(true) => Ok(()),
        Some(false) => Err(Erro::login()),
        None => Err(Erro::interno("não foi possível conferir o login do Claude Code")),
    }
}

fn uuid_valido(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|u| u.hyphenated().to_string() == id)
}

/// A transcrição documentada fica em `<pasta>/projects/<projeto>/<sessão>.jsonl`.
fn transcricao_existe(pasta: Option<&Path>, id: &str) -> bool {
    let Some(projetos) = pasta.map(|p| p.join("projects")) else { return false };
    std::fs::read_dir(projetos).into_iter().flatten().flatten().any(|d| d.path().join(format!("{id}.jsonl")).is_file())
}

fn gravar(caminho: &Path, conteudo: &[u8]) -> std::io::Result<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    std::io::Write::write_all(&mut o.open(caminho)?, conteudo)
}

/// Fora do Unix não há SIGINT: o cancelamento acontece pelo prazo e pelo encerramento do processo, e o turno
/// seguinte retoma a sessão com `--resume`.
fn interromper(pid: u32) {
    #[cfg(unix)]
    if let Ok(pid) = i32::try_from(pid) {
        // SAFETY: sinal ao filho direto desta ponte, cujo pid ela mesma guardou ao abrir o processo.
        unsafe {
            libc::kill(pid, libc::SIGINT);
        }
    }
    #[cfg(not(unix))]
    let _ = pid;
}

/// Blocos ACP do pedido → blocos de mensagem do Claude.
fn blocos(prompt: &Value) -> Result<Vec<Value>, Erro> {
    let mut out = Vec::new();
    let mut contexto = Vec::new();
    for b in prompt.as_array().into_iter().flatten() {
        match b["type"].as_str().unwrap_or_default() {
            "text" => out.push(json!({"type": "text", "text": b["text"].as_str().unwrap_or_default()})),
            "image" if b["data"].as_str().is_some_and(|d| !d.is_empty()) => out.push(json!({"type": "image",
                "source": {"type": "base64", "data": b["data"], "media_type": b["mimeType"]}})),
            "resource_link" => out.push(json!({"type": "text", "text": b["uri"].as_str().unwrap_or_default()})),
            "resource" => {
                let uri = b["resource"]["uri"].as_str().unwrap_or_default();
                out.push(json!({"type": "text", "text": uri}));
                if let Some(t) = b["resource"]["text"].as_str() {
                    contexto
                        .push(json!({"type": "text", "text": format!("\n<context ref=\"{uri}\">\n{t}\n</context>")}));
                } else if let Some(dados) = b["resource"]["blob"].as_str() {
                    let tipo = b["resource"]["mimeType"].as_str().unwrap_or_default();
                    if !["image/jpeg", "image/png", "image/gif", "image/webp"].contains(&tipo) {
                        return Err(Erro::parametros(
                            "recurso binário que não é imagem JPEG, PNG, GIF ou WebP não é suportado",
                        ));
                    }
                    contexto.push(
                        json!({"type": "image", "source": {"type": "base64", "data": dados, "media_type": tipo}}),
                    );
                }
            }
            _ => {}
        }
    }
    out.extend(contexto);
    if out.is_empty() {
        return Err(Erro::parametros("pedido sem conteúdo"));
    }
    Ok(out)
}

fn ler_uso(uso: &Value) -> [u64; 4] {
    ["input_tokens", "output_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"]
        .map(|k| uso[k].as_u64().unwrap_or(0))
}

fn contagem(u: [u64; 4]) -> Value {
    json!({"totalTokens": u.iter().sum::<u64>(), "inputTokens": u[0], "cachedInputTokens": u[2],
        "cachedWriteTokens": u[3], "outputTokens": u[1], "reasoningOutputTokens": 0})
}

/// Uso por modelo do turno: a diferença do acumulado do processo; no primeiro resultado de um processo que
/// retomou a sessão, só o modelo principal, com o uso do próprio resultado (o acumulado pode trazer o passado).
fn uso_por_modelo(p: &mut Processo, resultado: &Value, principal: Option<&str>) -> Vec<Value> {
    let mut atual = HashMap::new();
    for (modelo, u) in resultado["modelUsage"].as_object().into_iter().flatten() {
        let n = ["inputTokens", "outputTokens", "cacheReadInputTokens", "cacheCreationInputTokens"]
            .map(|k| u[k].as_u64().unwrap_or(0));
        atual.insert(modelo.clone(), n);
    }
    let linhas = match (&p.modelos, p.retomado) {
        (None, true) => {
            let modelo = principal
                .map(str::to_owned)
                .or_else(|| (atual.len() == 1).then(|| atual.keys().next().cloned()).flatten());
            modelo.map(|m| vec![(m, ler_uso(&resultado["usage"]))]).unwrap_or_default()
        }
        (anterior, _) => atual
            .iter()
            .map(|(m, n)| {
                let antes = anterior.as_ref().and_then(|a| a.get(m)).copied().unwrap_or_default();
                (m.clone(), [0, 1, 2, 3].map(|i| n[i].saturating_sub(antes[i])))
            })
            .filter(|(_, n)| n.iter().any(|v| *v > 0))
            .collect(),
    };
    // Resultado sem `modelUsage` (um erro, por exemplo) não zera a base: o próximo continua contando dela.
    if resultado["modelUsage"].is_object() {
        p.modelos = Some(atual);
    }
    linhas.into_iter().map(|(m, n)| json!({"model": m, "token_count": contagem(n)})).collect()
}

enum Passo {
    Evento(Option<Value>),
    Cancelar,
    Prazo,
}

impl Sessao {
    fn nova(id: String, opcoes: Opcoes, iniciada: bool) -> Self {
        let modelo = opcoes.modelo.clone().unwrap_or_else(|| "default".into());
        let esforco = opcoes.esforco.clone().unwrap_or_else(|| "default".into());
        let traducao = Traducao::new(&opcoes.cwd.display().to_string());
        Self {
            id,
            opcoes,
            modelo: Mutex::new(modelo),
            esforco: Mutex::new(esforco),
            iniciada: AtomicBool::new(iniciada),
            reabrir: AtomicBool::new(false),
            processo: tokio::sync::Mutex::new(None),
            cancelar: watch::channel(0).0,
            traducao: Mutex::new(traducao),
            turno_ativo: AtomicBool::new(false),
            decididas: Mutex::new(HashSet::new()),
            fim_turno: watch::channel(0).0,
        }
    }

    fn config(&self) -> Value {
        opcoes_config(&self.modelo.locked(), &self.esforco.locked())
    }
}

async fn decidir(ponte: Arc<Ponte>, sessao: Weak<Sessao>, args: Value) -> Value {
    // O motivo vai para o registro de diagnóstico da ponte; a entrada da ferramenta não.
    let negar = |m: &str| {
        tracing::warn!(motivo = m, "permissão negada pela ponte do Claude");
        json!({"behavior": "deny", "message": m})
    };
    let Some(s) = sessao.upgrade() else { return negar("Sessão encerrada.") };
    let mut cancelado = s.cancelar.subscribe();
    let mut acabou = s.fim_turno.subscribe();
    if !s.turno_ativo.load(Ordering::SeqCst) {
        return negar("Nenhum turno em andamento.");
    }
    let (Some(nome), Some(entrada)) = (args["tool_name"].as_str(), args.get("input").filter(|v| v.is_object())) else {
        return negar("Pedido de permissão inválido.");
    };
    if nome == opcoes::nome_ferramenta_permissao() {
        return negar("Ferramenta interna.");
    }
    let pedido_id = args["tool_use_id"].as_str().filter(|id| !id.is_empty());
    // A chamada precisa ter sido anunciada pelo programa, com o mesmo nome e a mesma entrada, e ainda aberta (uma
    // só). Sem isso dentro do prazo, nega: o cartão só mostra o que vai rodar de fato.
    let limite = tokio::time::Instant::now() + PRAZO_ANUNCIO;
    let id = loop {
        let achado = {
            let t = s.traducao.locked();
            let mut candidatos = t.ferramentas.iter().filter(|(id, f)| {
                pedido_id.is_none_or(|p| p == id.as_str())
                    && f.nome == nome
                    && f.entrada == *entrada
                    && !t.concluida(id)
            });
            match (candidatos.next(), candidatos.next()) {
                (Some((id, _)), None) => Some(id.clone()),
                _ => None,
            }
        };
        if let Some(id) = achado {
            break id;
        }
        if tokio::time::Instant::now() >= limite || !s.turno_ativo.load(Ordering::SeqCst) {
            return negar("Não foi possível identificar a chamada.");
        }
        tokio::select! {
            () = tokio::time::sleep(Duration::from_millis(25)) => {}
            _ = cancelado.changed() => return negar("Turno cancelado."),
            _ = acabou.changed() => return negar("Turno encerrado."),
        }
    };
    if !s.decididas.locked().insert(id.clone()) {
        return negar("Esta chamada já teve um pedido de permissão.");
    }
    let chamada = s.traducao.locked().chamada(&id, nome, entrada);
    let pedido = json!({"sessionId": s.id, "toolCall": chamada, "options": [
        {"optionId": "allow-once", "name": "Yes", "kind": "allow_once"},
        {"optionId": "reject", "name": "No", "kind": "reject_once"}]});
    let aprovado = tokio::select! {
        biased;
        _ = cancelado.changed() => return negar("Turno cancelado."),
        _ = acabou.changed() => return negar("Turno encerrado."),
        r = ponte.pedir("session/request_permission", pedido) =>
            r.is_some_and(|v| v["outcome"]["outcome"] == "selected" && v["outcome"]["optionId"] == "allow-once"),
    };
    // A resposta pode chegar junto com o fim ou o cancelamento do turno: só aprova o turno que ainda pediu.
    let ainda_ativo = s.turno_ativo.load(Ordering::SeqCst)
        && !cancelado.has_changed().unwrap_or(true)
        && !acabou.has_changed().unwrap_or(true);
    if !aprovado {
        return negar("O dono não aprovou esta ação.");
    }
    if !ainda_ativo {
        return negar("Turno encerrado.");
    }
    json!({"behavior": "allow", "updatedInput": entrada})
}

/// Encerra o programa com calma: fecha a entrada (o programa sai e encerra os próprios filhos) e, se não sair a
/// tempo, mata o processo.
async fn encerrar(mut p: Processo) {
    drop(p.entrada);
    if tokio::time::timeout(Duration::from_secs(5), p.filho.wait()).await.is_err() {
        let _ = p.filho.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), p.filho.wait()).await;
    }
}

async fn abrir(ponte: &Arc<Ponte>, s: &Arc<Sessao>) -> Result<Processo, Erro> {
    let exe = ponte.cfg.exe.clone().ok_or_else(|| Erro::interno("Claude Code não está instalado"))?;
    let pasta =
        tempfile::Builder::new().prefix("mexerica-claude-").tempdir().map_err(|e| Erro::interno(e.to_string()))?;
    let decisor: permissao::Decisor = {
        let ponte = ponte.clone();
        let sessao = Arc::downgrade(s);
        Arc::new(move |args| Box::pin(decidir(ponte.clone(), sessao.clone(), args)))
    };
    let servidor = permissao::iniciar(decisor).await.map_err(|e| Erro::interno(format!("{e:#}")))?;
    let o = &s.opcoes;
    let io = |e: std::io::Error| Erro::interno(e.to_string());
    let mcp = pasta.path().join("mcp.json");
    gravar(&mcp, opcoes::config_mcp(o, &servidor.url, &servidor.segredo).to_string().as_bytes()).map_err(io)?;
    let mcp_arquivo = mcp.clone();
    let settings = match &o.settings {
        Some(v) => {
            let p = pasta.path().join("settings.json");
            gravar(&p, v.to_string().as_bytes()).map_err(io)?;
            Some(p)
        }
        None => None,
    };
    let prompt = match &o.prompt {
        Some(PromptSistema::Acrescentar(t) | PromptSistema::Substituir(t)) => {
            let p = pasta.path().join("prompt.md");
            gravar(&p, t.as_bytes()).map_err(io)?;
            Some(p)
        }
        None => None,
    };
    let retomar = s.iniciada.load(Ordering::SeqCst);
    let args = opcoes::argumentos(
        o,
        &s.id,
        retomar,
        &s.modelo.locked().clone(),
        &s.esforco.locked().clone(),
        &Arquivos { mcp, settings, prompt },
    );
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(&args)
        .current_dir(&o.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut filho = cmd.spawn().map_err(|e| Erro::interno(format!("não foi possível abrir o Claude Code: {e}")))?;
    let entrada = filho.stdin.take().ok_or_else(|| Erro::interno("sem entrada"))?;
    let saida = filho.stdout.take().ok_or_else(|| Erro::interno("sem saída"))?;
    let erros = filho.stderr.take().ok_or_else(|| Erro::interno("sem saída de erros"))?;
    // Diagnóstico do programa segue para o Hub sem o segredo da ponte.
    let segredo = servidor.segredo.clone();
    tokio::spawn(async move {
        let mut leitor = BufReader::new(erros);
        let mut linha = Vec::new();
        while matches!(leitor.read_until(b'\n', &mut linha).await, Ok(n) if n > 0) {
            let texto = String::from_utf8_lossy(&linha).replace(&segredo, "***");
            let _ = tokio::io::stderr().write_all(texto.as_bytes()).await;
            linha.clear();
        }
    });
    let (tx, eventos) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut leitor = BufReader::new(saida);
        let mut linha = Vec::new();
        while matches!(leitor.read_until(b'\n', &mut linha).await, Ok(n) if n > 0) {
            if let Ok(v) = serde_json::from_slice::<Value>(&linha)
                && v.is_object()
                && tx.send(v).is_err()
            {
                break;
            }
            linha.clear();
        }
    });
    Ok(Processo {
        filho,
        entrada,
        mcp: Some(mcp_arquivo),
        eventos,
        _permissao: servidor,
        _pasta: pasta,
        modelos: None,
        retomado: retomar,
    })
}

/// Envia a mensagem ao programa sem deixar de ouvir o cancelamento. `None`: cancelado no meio do envio; a linha pode
/// ter ficado pela metade no cano, então esse processo não serve para o próximo turno.
async fn enviar(p: &mut Processo, linha: &str, cancelar: &mut watch::Receiver<u64>) -> Option<bool> {
    let escrita = async {
        p.entrada.write_all(linha.as_bytes()).await?;
        p.entrada.flush().await
    };
    tokio::select! {
        biased;
        Ok(()) = cancelar.changed() => None,
        r = escrita => Some(r.is_ok()),
    }
}

/// Marca o turno em andamento enquanto viver; ao sair, nenhum pedido de permissão é mais aceito.
struct TurnoAtivo<'a>(&'a Sessao);

impl Drop for TurnoAtivo<'_> {
    fn drop(&mut self) {
        self.0.turno_ativo.store(false, Ordering::SeqCst);
        self.0.decididas.locked().clear();
        self.0.fim_turno.send_modify(|v| *v += 1);
    }
}

/// `cancelar` foi assinado antes de o pedido sair da fila: um `session/cancel` logo depois não se perde.
async fn turno(
    ponte: &Arc<Ponte>,
    s: &Arc<Sessao>,
    prompt: &Value,
    mut cancelar: watch::Receiver<u64>,
) -> Result<Value, Erro> {
    let conteudo = blocos(prompt)?;
    let mut guarda = s.processo.lock().await;
    s.turno_ativo.store(true, Ordering::SeqCst);
    let _ativo = TurnoAtivo(s);
    // Cancelado enquanto esperava a vez: nada chega ao programa.
    if cancelar.has_changed().unwrap_or(false) {
        return Ok(json!({"stopReason": "cancelled"}));
    }
    if s.reabrir.swap(false, Ordering::SeqCst)
        && let Some(p) = guarda.take()
    {
        encerrar(p).await;
    }
    let linha = json!({"type": "user", "message": {"role": "user", "content": conteudo}, "parent_tool_use_id": null,
        "session_id": s.id})
    .to_string()
        + "\n";
    let mut tentativas = 0;
    loop {
        if guarda.is_none() {
            *guarda = Some(abrir(ponte, s).await?);
        }
        let Some(p) = guarda.as_mut() else { return Err(Erro::interno("processo indisponível")) };
        match enviar(p, &linha, &mut cancelar).await {
            Some(true) => break,
            Some(false) => {}
            None => {
                if let Some(p) = guarda.take() {
                    encerrar(p).await;
                }
                fechar_turno(ponte, s);
                return Ok(json!({"stopReason": "cancelled"}));
            }
        }
        if let Some(p) = guarda.take() {
            encerrar(p).await;
        }
        tentativas += 1;
        if tentativas > 1 {
            return Err(Erro::interno("o Claude Code não aceitou a mensagem"));
        }
    }
    let mut cancelado = false;
    let mut prazo: Option<tokio::time::Instant> = None;
    let mut recebeu = false;
    // Sinais que o programa dá antes de um resultado de erro: categoria e limite recusado.
    let mut categoria: Option<String> = None;
    let mut limite: Option<Value> = None;
    let resultado = loop {
        let Some(p) = guarda.as_mut() else { return Err(Erro::interno("processo indisponível")) };
        // Cancelamento pedido vence um resultado que já esteja pronto.
        let passo = tokio::select! {
            biased;
            Ok(()) = cancelar.changed(), if !cancelado => Passo::Cancelar,
            () = tokio::time::sleep_until(prazo.unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(86_400))), if prazo.is_some() => Passo::Prazo,
            ev = p.eventos.recv() => Passo::Evento(ev),
        };
        match passo {
            Passo::Cancelar => {
                cancelado = true;
                prazo = Some(tokio::time::Instant::now() + PRAZO_CANCELAR);
                if let Some(pid) = guarda.as_ref().and_then(|p| p.filho.id()) {
                    interromper(pid);
                }
            }
            Passo::Prazo => {
                if let Some(p) = guarda.take() {
                    encerrar(p).await;
                }
                fechar_turno(ponte, s);
                return Ok(json!({"stopReason": "cancelled"}));
            }
            Passo::Evento(None) => {
                if let Some(p) = guarda.take() {
                    encerrar(p).await;
                }
                if !recebeu && !cancelado && tentativas == 0 {
                    // O processo tinha saído entre turnos: abre outro na mesma sessão e reenvia a mensagem.
                    tentativas = 1;
                    let mut p = abrir(ponte, s).await?;
                    match enviar(&mut p, &linha, &mut cancelar).await {
                        Some(true) => {
                            *guarda = Some(p);
                            continue;
                        }
                        Some(false) => return Err(Erro::interno("o Claude Code não aceitou a mensagem")),
                        None => {
                            encerrar(p).await;
                            fechar_turno(ponte, s);
                            return Ok(json!({"stopReason": "cancelled"}));
                        }
                    }
                }
                fechar_turno(ponte, s);
                if cancelado {
                    return Ok(json!({"stopReason": "cancelled"}));
                }
                if login_recusado(&ponte.cfg).await {
                    return Err(Erro::login());
                }
                return Err(Erro::interno("o Claude Code encerrou sem concluir o turno"));
            }
            Passo::Evento(Some(ev)) if ev["type"] == "result" => break ev,
            Passo::Evento(Some(ev)) => {
                recebeu = true;
                if let Some(c) = ev["error"]
                    .as_str()
                    .or_else(|| (ev["subtype"] == "api_retry").then(|| ev["error"].as_str()).flatten())
                {
                    categoria = Some(c.to_owned());
                }
                if ev["type"] == "rate_limit_event" {
                    limite = Some(ev["rate_limit_info"].clone()).filter(|i| i["status"] == "rejected");
                }
                if ev["type"] == "system" && ev["subtype"] == "init" {
                    if s.opcoes.persistir {
                        s.iniciada.store(true, Ordering::SeqCst);
                    }
                    // O programa já leu a configuração MCP: o arquivo com os segredos sai do disco.
                    if let Some(arquivo) = guarda.as_mut().and_then(|p| p.mcp.take()) {
                        let _ = std::fs::remove_file(arquivo);
                    }
                }
                // O anúncio entra na fila de saída antes de `decidir` poder ver a chamada: a trava cobre os dois.
                let mut t = s.traducao.locked();
                for u in t.evento(&ev) {
                    ponte.atualizar(&s.id, &u);
                }
            }
        }
    };
    let Some(p) = guarda.as_mut() else { return Err(Erro::interno("processo indisponível")) };
    let principal = s.traducao.locked().modelo().map(str::to_owned);
    let por_modelo = uso_por_modelo(p, &resultado, principal.as_deref());
    if cancelado && let Some(p) = guarda.take() {
        // O programa pode sair depois do fim pedido: o próximo turno abre outro processo com --resume.
        encerrar(p).await;
    }
    let uso = ler_uso(&resultado["usage"]);
    {
        let mut t = s.traducao.locked();
        t.janelas(&resultado["modelUsage"]);
        let mut finais = t.fechar_abertas();
        if let Some(custo) = resultado["total_cost_usd"].as_f64().filter(|c| c.is_finite() && *c >= 0.0) {
            finais.push(t.uso(Some(("cost", json!({"amount": custo, "currency": "USD"}))), None));
        }
        t.encerrar_turno();
        drop(t);
        for u in finais {
            ponte.atualizar(&s.id, &u);
        }
    }
    let stop = if cancelado {
        "cancelled"
    } else if matches!(resultado["subtype"].as_str(), Some("error_max_turns" | "error_max_budget_usd")) {
        "max_turn_requests"
    } else if resultado["is_error"] == true || resultado["subtype"] != "success" {
        if login_recusado(&ponte.cfg).await {
            return Err(Erro::login());
        }
        let texto = resultado["result"].as_str().unwrap_or("o Claude Code informou um erro");
        let mut data = json!({});
        if let Some(c) = categoria
            .filter(|c| !c.is_empty() && c.len() <= 64 && c.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        {
            data["errorKind"] = c.into();
        }
        if let Some(info) = limite {
            data["_meta"] = json!({"_claude/rateLimit": info});
        }
        return Err(Erro { code: INTERNAL, message: crate::agent::acp::truncate(texto, 2000), data: Some(data) });
    } else {
        match resultado["stop_reason"].as_str() {
            Some("refusal") => "refusal",
            Some("max_tokens") => "max_tokens",
            _ => "end_turn",
        }
    };
    Ok(json!({"stopReason": stop,
        "usage": {"inputTokens": uso[0], "outputTokens": uso[1], "cachedReadTokens": uso[2], "cachedWriteTokens": uso[3],
            "totalTokens": uso.iter().sum::<u64>()},
        "_meta": {"quota": {"token_count": contagem(uso), "model_usage": por_modelo}}}))
}

fn fechar_turno(ponte: &Ponte, s: &Sessao) {
    let finais = {
        let mut t = s.traducao.locked();
        let f = t.fechar_abertas();
        t.encerrar_turno();
        f
    };
    for u in finais {
        ponte.atualizar(&s.id, &u);
    }
}

async fn abrir_sessao(ponte: &Arc<Ponte>, params: &Value, carregar: bool) -> Result<Value, Erro> {
    let opcoes = opcoes::ler(params)?;
    let id = if carregar {
        let id = params["sessionId"].as_str().unwrap_or_default();
        if !uuid_valido(id) {
            return Err(Erro::parametros("sessão inválida"));
        }
        if !transcricao_existe(ponte.cfg.pasta_config.as_deref(), id) {
            return Err(Erro::interno("Resource not found: sessão do Claude Code não encontrada"));
        }
        id.to_owned()
    } else {
        uuid::Uuid::new_v4().to_string()
    };
    exigir_login(&ponte.cfg).await?;
    let sessao = Arc::new(Sessao::nova(id.clone(), opcoes, carregar));
    let config = sessao.config();
    if let Some(antiga) = ponte.sessoes.locked().insert(id.clone(), sessao) {
        antiga.cancelar.send_modify(|v| *v += 1);
    }
    Ok(json!({"sessionId": id, "configOptions": config}))
}

fn configurar(s: &Sessao, id: &str, valor: &Value) -> Result<Value, Erro> {
    let v = valor.as_str().ok_or_else(|| Erro::parametros("valor inválido"))?;
    match id {
        "model" => {
            if !opcoes::modelo_valido(v) {
                return Err(Erro::parametros("modelo inválido"));
            }
            let mut m = s.modelo.locked();
            if *m != v {
                v.clone_into(&mut m);
                s.reabrir.store(true, Ordering::SeqCst);
            }
        }
        "effort" => {
            if !ESFORCOS.contains(&v) {
                return Err(Erro::parametros("esforço inválido"));
            }
            let mut e = s.esforco.locked();
            if *e != v {
                v.clone_into(&mut e);
                s.reabrir.store(true, Ordering::SeqCst);
            }
        }
        "mode" if v == "default" => {}
        "mode" => return Err(Erro::parametros("a ponte só usa o modo padrão")),
        _ => return Err(Erro::parametros("opção desconhecida")),
    }
    Ok(json!({"configOptions": s.config()}))
}

async fn atender(
    ponte: &Arc<Ponte>,
    metodo: &str,
    params: &Value,
    cancelar: Option<watch::Receiver<u64>>,
) -> Result<Value, Erro> {
    match metodo {
        "initialize" => Ok(json!({
            "protocolVersion": 1,
            "agentInfo": {"name": super::NOME, "title": "Claude Code", "version": env!("CARGO_PKG_VERSION")},
            "agentCapabilities": {
                "_meta": {"claudeCode": {"programaOficial": true}},
                "promptCapabilities": {"image": true, "embeddedContext": true},
                "mcpCapabilities": {"http": true, "sse": true},
                "loadSession": true,
                "sessionCapabilities": {"close": {}},
            },
            "authMethods": [],
        })),
        "session/new" => abrir_sessao(ponte, params, false).await,
        "session/load" => abrir_sessao(ponte, params, true).await,
        "session/set_config_option" => {
            let s = ponte.sessao(params)?;
            configurar(&s, params["configId"].as_str().unwrap_or_default(), &params["value"])
        }
        "session/set_model" => {
            let s = ponte.sessao(params)?;
            configurar(&s, "model", &params["modelId"]).map(|_| json!({}))
        }
        "session/set_mode" => {
            let s = ponte.sessao(params)?;
            configurar(&s, "mode", &params["modeId"]).map(|_| json!({}))
        }
        "session/prompt" => {
            let s = ponte.sessao(params)?;
            let cancelar = cancelar.unwrap_or_else(|| s.cancelar.subscribe());
            turno(ponte, &s, &params["prompt"], cancelar).await
        }
        "session/close" => {
            let id = params["sessionId"].as_str().unwrap_or_default();
            let sessao = ponte.sessoes.locked().remove(id);
            if let Some(s) = sessao {
                s.cancelar.send_modify(|v| *v += 1);
                let processo = s.processo.lock().await.take();
                if let Some(p) = processo {
                    encerrar(p).await;
                }
            }
            Ok(json!({}))
        }
        _ => Err(Erro { code: -32601, message: format!("{metodo} não é suportado"), data: None }),
    }
}

/// Atende o Hub até a entrada fechar. Ao sair, todos os processos do programa são encerrados.
pub(super) async fn servir<R, W>(entrada: R, mut saida: W, cfg: Config) -> anyhow::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let escritor = tokio::spawn(async move {
        while let Some(linha) = rx.recv().await {
            if saida.write_all(linha.as_bytes()).await.is_err()
                || saida.write_all(b"\n").await.is_err()
                || saida.flush().await.is_err()
            {
                break;
            }
        }
    });
    let ponte = Arc::new(Ponte {
        cfg,
        saida: tx,
        sessoes: Mutex::new(HashMap::new()),
        pendentes: Mutex::new(HashMap::new()),
        proximo: AtomicI64::new(1),
    });
    let mut tarefas = tokio::task::JoinSet::new();
    let mut linhas = BufReader::new(entrada).lines();
    loop {
        let linha = tokio::select! {
            Some(_) = tarefas.join_next(), if !tarefas.is_empty() => continue,
            l = linhas.next_line() => match l {
                Ok(Some(l)) => l,
                _ => break,
            },
        };
        let Ok(msg) = serde_json::from_str::<Value>(&linha) else { continue };
        let id = msg.get("id").filter(|v| !v.is_null()).cloned();
        match (msg["method"].as_str(), id) {
            (Some(metodo), Some(id)) => {
                let ponte = ponte.clone();
                let metodo = metodo.to_owned();
                let cancelar = (metodo == "session/prompt")
                    .then(|| ponte.sessao(&msg["params"]).ok())
                    .flatten()
                    .map(|s| s.cancelar.subscribe());
                tarefas.spawn(async move {
                    let resposta = match atender(&ponte, &metodo, &msg["params"], cancelar).await {
                        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                        Err(e) => {
                            let mut erro = json!({"code": e.code, "message": e.message});
                            if let Some(data) = e.data {
                                erro["data"] = data;
                            }
                            json!({"jsonrpc": "2.0", "id": id, "error": erro})
                        }
                    };
                    ponte.enviar(&resposta);
                });
            }
            (Some("session/cancel"), None) => {
                if let Ok(s) = ponte.sessao(&msg["params"]) {
                    s.cancelar.send_modify(|v| *v += 1);
                }
            }
            (None, Some(id)) => {
                if let Some(tx) = id.as_i64().and_then(|id| ponte.pendentes.locked().remove(&id)) {
                    let _ = tx.send(msg);
                }
            }
            _ => {}
        }
    }
    tarefas.abort_all();
    while tarefas.join_next().await.is_some() {}
    let sessoes: Vec<_> = ponte.sessoes.locked().drain().map(|(_, s)| s).collect();
    for s in sessoes {
        let processo = s.processo.lock().await.take();
        if let Some(p) = processo {
            encerrar(p).await;
        }
    }
    ponte.pendentes.locked().clear();
    drop(ponte);
    let _ = tokio::time::timeout(Duration::from_secs(2), escritor).await;
    Ok(())
}
