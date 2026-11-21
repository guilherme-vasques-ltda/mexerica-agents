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
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{mpsc, watch};

/// ACP: "authentication required".
const AUTH_REQUIRED: i64 = -32000;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL: i64 = -32603;
/// Quanto o turno espera o fim pedido por SIGINT antes de encerrar o processo.
const PRAZO_CANCELAR: Duration = Duration::from_secs(10);

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
        let exe = std::env::var_os("CLAUDE_CODE_EXECUTABLE")
            .map(PathBuf::from)
            .and_then(|p| std::path::absolute(p).ok())
            .filter(|p| p.is_file())
            .or_else(|| {
                std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                    .map(|d| d.join(if cfg!(windows) { "claude.exe" } else { "claude" }))
                    // Item relativo no PATH devolve caminho relativo; o programa roda depois de trocar a pasta
                    // de trabalho, então o caminho vira absoluto uma vez, aqui, antes de qualquer uso.
                    .find_map(|p| std::path::absolute(&p).ok().filter(|p| p.is_file()))
            });
        let pasta_config = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| if cfg!(windows) { std::env::var_os("USERPROFILE") } else { None })
                .map(|h| PathBuf::from(h).join(".claude"))
        });
        Self { exe, pasta_config }
    }
}

#[derive(Debug)]
pub(super) struct Erro {
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

pub(super) struct Sessao {
    id: String,
    opcoes: Opcoes,
    modelo: Mutex<String>,
    esforco: Mutex<String>,
    /// A sessão já existe no disco do programa: o próximo processo usa `--resume`.
    iniciada: AtomicBool,
    /// Modelo ou esforço mudou: o processo atual precisa ser trocado no próximo turno.
    reabrir: AtomicBool,
    /// Algum processo desta sessão já atendeu: sem histórico salvo, trocar o processo recomeça a conversa.
    aberta: AtomicBool,
    processo: tokio::sync::Mutex<Option<Processo>>,
    /// Pedido de cancelamento do turno: avança e acorda quem espera o sinal.
    cancelar: watch::Sender<u64>,
    traducao: Mutex<Traducao>,
}

struct Ponte {
    cfg: Config,
    saida: mpsc::UnboundedSender<String>,
    sessoes: Mutex<HashMap<String, Arc<Sessao>>>,
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

/// Teto de uma imagem (a API do Claude aceita até 5 MB por imagem) e das imagens de um mesmo pedido.
const LIMITE_IMAGEM: usize = 5 * 1024 * 1024;
const LIMITE_IMAGENS: usize = 20 * 1024 * 1024;

/// Uma imagem só vai ao programa com tipo da lista documentada, base64 válido, bytes iniciais do tipo declarado e
/// tamanho dentro dos tetos; o que não bater é recusado antes de qualquer envio.
fn imagem(tipo: &str, dados: &str, total: &mut usize) -> Result<Value, Erro> {
    use base64::Engine as _;
    let recusar = |m: &str| Err(Erro::parametros(m));
    if dados.len() > LIMITE_IMAGEM / 3 * 4 + 4 {
        return recusar("imagem maior que o limite");
    }
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(dados) else {
        return recusar("imagem com dados que não são base64 válido");
    };
    let confere = match tipo {
        "image/png" => bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
        "image/jpeg" => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        _ => return recusar("só imagens JPEG, PNG, GIF e WebP são suportadas"),
    };
    if !confere {
        return recusar("os dados da imagem não correspondem ao tipo declarado");
    }
    *total += bytes.len();
    if bytes.len() > LIMITE_IMAGEM || *total > LIMITE_IMAGENS {
        return recusar("imagem maior que o limite");
    }
    Ok(json!({"type": "image", "source": {"type": "base64", "data": dados, "media_type": tipo}}))
}

/// Blob de texto anexado ao pedido: decodifica e devolve o conteúdo; o limite é o mesmo das imagens.
fn anexo_texto(dados: &str) -> Result<String, Erro> {
    use base64::Engine as _;
    if dados.len() > LIMITE_IMAGEM / 3 * 4 + 4 {
        return Err(Erro::parametros("anexo maior que o limite"));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(dados)
        .map_err(|_| Erro::parametros("anexo com dados que não são base64 válido"))?;
    if bytes.len() > LIMITE_IMAGEM {
        return Err(Erro::parametros("anexo maior que o limite"));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Blocos ACP do pedido → blocos de mensagem do Claude.
pub(super) fn blocos(prompt: &Value) -> Result<Vec<Value>, Erro> {
    let mut out = Vec::new();
    let mut contexto = Vec::new();
    let mut total_imagens = 0;
    for b in prompt.as_array().into_iter().flatten() {
        match b["type"].as_str().unwrap_or_default() {
            "text" => out.push(json!({"type": "text", "text": b["text"].as_str().unwrap_or_default()})),
            "image" => out.push(imagem(
                b["mimeType"].as_str().unwrap_or_default(),
                b["data"].as_str().unwrap_or_default(),
                &mut total_imagens,
            )?),
            "resource_link" => out.push(json!({"type": "text", "text": b["uri"].as_str().unwrap_or_default()})),
            "resource" => {
                let uri = b["resource"]["uri"].as_str().unwrap_or_default();
                out.push(json!({"type": "text", "text": uri}));
                if let Some(t) = b["resource"]["text"].as_str() {
                    contexto
                        .push(json!({"type": "text", "text": format!("\n<context ref=\"{uri}\">\n{t}\n</context>")}));
                } else if let Some(dados) = b["resource"]["blob"].as_str() {
                    let tipo = b["resource"]["mimeType"].as_str().unwrap_or_default();
                    if tipo.starts_with("text/") {
                        contexto.push(json!({"type": "text", "text": format!("\n<context ref=\"{uri}\">\n{}\n</context>", anexo_texto(dados)?) }));
                    } else {
                        contexto.push(imagem(tipo, dados, &mut total_imagens)?);
                    }
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
    /// Pedido de cancelamento: acorda quem espera o sinal. O turno confere o aviso logo ao começar; um
    /// cancelamento que chega fora dele não vale para o próximo turno, que assina o sinal na própria vez.
    pub(super) fn cancelar_turno(&self) {
        self.cancelar.send_modify(|v| *v += 1);
    }

    pub(super) fn nova(id: String, opcoes: Opcoes, iniciada: bool) -> Self {
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
            aberta: AtomicBool::new(false),
            processo: tokio::sync::Mutex::new(None),
            cancelar: watch::channel(0).0,
            traducao: Mutex::new(traducao),
        }
    }

    fn config(&self) -> Value {
        opcoes_config(&self.modelo.locked(), &self.esforco.locked())
    }
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
    // O servidor de permissão nega tudo nesta versão; segue no ar só para o programa ter quem perguntar.
    let servidor = permissao::iniciar().await.map_err(|e| Erro::interno(format!("{e:#}")))?;
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
    // Diagnóstico do programa segue para o Hub sem valor que seja segredo: o da ponte e os valores de
    // variáveis dos servidores MCP configurados para esta sessão. Os curtos ficam como estão (não são
    // distintivos e atrapalham a leitura); um valor de variável sempre é segredo em potencial.
    let segredos = mascaraveis(&servidor.segredo, &s.opcoes.mcp);
    tokio::spawn(async move {
        let mut leitor = BufReader::new(erros);
        let mut linha = Vec::new();
        while matches!(leitor.read_until(b'\n', &mut linha).await, Ok(n) if n > 0) {
            let texto = mascarar(String::from_utf8_lossy(&linha), &segredos);
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

/// Conduz um turno do programa. `cancelar` foi assinado antes de o pedido sair da fila: um `session/cancel`
/// logo depois não se perde.
async fn turno(
    ponte: &Arc<Ponte>,
    s: &Arc<Sessao>,
    prompt: &Value,
    mut cancelar: watch::Receiver<u64>,
) -> Result<Value, Erro> {
    let conteudo = blocos(prompt)?;
    let mut guarda = s.processo.lock().await;
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
                if !recebeu && !cancelado && tentativas == 0 {
                    // O processo tinha saído entre turnos: abre outro na mesma sessão e reenvia a mensagem.
                    if let Some(p) = guarda.take() {
                        encerrar(p).await;
                    }
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
                if let Some(p) = guarda.take() {
                    encerrar(p).await;
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
                    s.aberta.store(true, Ordering::SeqCst);
                    if s.opcoes.persistir {
                        s.iniciada.store(true, Ordering::SeqCst);
                    }
                    // O programa já leu a configuração MCP: o arquivo com os segredos sai do disco.
                    if let Some(arquivo) = guarda.as_mut().and_then(|p| p.mcp.take()) {
                        let _ = std::fs::remove_file(arquivo);
                    }
                }
                // A tradução das notificações do programa acontece sob a trava da sessão, na ordem do fluxo.
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
        antiga.cancelar_turno();
    }
    Ok(json!({"sessionId": id, "configOptions": config}))
}

/// Valores que não podem sair no diagnóstico: o segredo da ponte e os valores das variáveis de
/// ambiente dos servidores MCP desta sessão (mesmos que o arquivo de configuração leva e apaga).
/// Piso de 6 caracteres: credenciais curtas (códigos de 6 ou 7) entram, e palavras comuns de
/// 1 a 5 caracteres («1», «true») ficam de fora para não destruir a linha.
pub(super) fn mascaraveis(segredo: &str, servidores: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut out = vec![segredo.to_owned()];
    for servidor in servidores.values() {
        for valor in servidor["env"].as_object().into_iter().flat_map(|env| env.values()) {
            if let Some(v) = valor.as_str()
                && v.chars().count() >= 6
            {
                out.push(v.to_owned());
            }
        }
    }
    out
}

/// Aplica a máscara a uma linha de diagnóstico; valor comum de até 5 caracteres não entra na lista.
pub(super) fn mascarar(linha: std::borrow::Cow<'_, str>, segredos: &[String]) -> String {
    let mut texto = linha.into_owned();
    for s in segredos {
        if !s.is_empty() && texto.contains(s.as_str()) {
            texto = texto.replace(s.as_str(), "***");
        }
    }
    texto
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
                if !s.opcoes.persistir && s.aberta.load(Ordering::SeqCst) {
                    return Err(Erro::parametros(
                        "a troca de modelo recomeça a conversa, e esta sessão não salva histórico para retomar",
                    ));
                }
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
                if !s.opcoes.persistir && s.aberta.load(Ordering::SeqCst) {
                    return Err(Erro::parametros(
                        "a troca de esforço recomeça a conversa, e esta sessão não salva histórico para retomar",
                    ));
                }
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
                s.cancelar_turno();
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
    let ponte = Arc::new(Ponte { cfg, saida: tx, sessoes: Mutex::new(HashMap::new()) });
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
                    s.cancelar_turno();
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
    drop(ponte);
    let _ = tokio::time::timeout(Duration::from_secs(2), escritor).await;
    Ok(())
}
