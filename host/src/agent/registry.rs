// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! The official ACP agent registry (<https://agentclientprotocol.com/registry>):
//! ~40 coding agents with a known-good way to launch each over ACP — an npx or
//! uvx package, or a per-platform binary archive we download on first use.
//!
//! Registry JSON is untrusted input: ids, versions and commands become paths on
//! disk, so they're checked before use.

use crate::LockExt;
use crate::fork::activity_code::ActivityKind;
use crate::fork::motor_install::{self, Reply, Request};
use crate::shell;
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{Mutex as TravaAsync, OwnedMutexGuard};

mod arquivo;
mod fixados;

const URL: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
const MAX_ARCHIVE_BYTES: usize = 128 * 1024 * 1024;
/// Upstream updates hourly; once a day is plenty for picking agents.
const REFRESH_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

static CACHE: Mutex<Option<Value>> = Mutex::new(None);

/// How this machine would launch a registry agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Launch {
    /// A per-platform archive, downloaded once into `~/.codync/agents`.
    Download,
    Npx,
    Uvx,
}

fn cache_file() -> PathBuf {
    crate::service::data_dir().join("registry.json")
}

/// Registry agents (from memory, else the on-disk copy).
pub fn agents() -> Vec<Value> {
    let mut cache = CACHE.locked();
    if cache.is_none() {
        *cache = std::fs::read_to_string(cache_file()).ok().and_then(|s| serde_json::from_str(&s).ok());
    }
    cache.as_ref().map(agentes_oferecidos).unwrap_or_default()
}

fn agentes_oferecidos(catalogo: &Value) -> Vec<Value> {
    catalogo["agents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| oferecido(a))
        .cloned()
        .map(|mut agente| {
            if let Some(id) = id_normalizado(&agente) {
                agente["id"] = Value::String(id);
            }
            agente
        })
        .collect()
}

fn id_normalizado(agente: &Value) -> Option<String> {
    agente["id"].as_str().map(str::to_ascii_lowercase)
}

fn oferecido(agente: &Value) -> bool {
    let id = id_normalizado(agente);
    !matches!(id.as_deref(), Some("junie" | "cortex-code" | "corust-agent" | "stakpak" | "vtcode"))
        && id.as_deref().is_none_or(|id| {
            !crate::agent::backends::is_antigravity(id) && !crate::fork::motores_retirados::retirado(id)
        })
}

pub fn agent(id: &str) -> Option<Value> {
    agents().into_iter().find(|a| a["id"].as_str().is_some_and(|origem| origem.eq_ignore_ascii_case(id)))
}

pub async fn refresh_loop() {
    // Gancho só de teste (MEX-1291): o teste de ponta a ponta traz o seu registro e não vai à rede.
    if crate::agent::backends::teste_isolado() {
        return;
    }
    loop {
        match fetch().await {
            Ok(v) => {
                if let Err(error) = tokio::fs::write(cache_file(), v.to_string()).await {
                    tracing::warn!(%error, "couldn't cache the ACP registry");
                }
                *CACHE.locked() = Some(v);
            }
            Err(error) => {
                tracing::info!(error = format!("{error:#}"), "ACP registry refresh failed; using the cached copy");
            }
        }
        tokio::time::sleep(REFRESH_EVERY).await;
    }
}

async fn fetch() -> Result<Value> {
    let v: Value =
        crate::http().get(URL).timeout(Duration::from_secs(20)).send().await?.error_for_status()?.json().await?;
    if !v["agents"].is_array() {
        bail!("unexpected registry format");
    }
    Ok(v)
}

pub fn platform() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "darwin-aarch64",
        ("macos", "x86_64") => "darwin-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("linux", "x86_64") => "linux-x86_64",
        ("windows", "aarch64") => "windows-aarch64",
        ("windows", "x86_64") => "windows-x86_64",
        _ => return None,
    })
}

pub fn launch_kind(agent: &Value) -> Option<Launch> {
    if !oferecido(agent) {
        return None;
    }
    if let Some(fixo) = fixados::agente(agent) {
        return fixo.selecionar(agent, platform()).ok().map(|_| Launch::Download);
    }
    if let Some(alvo) = alvo_binario(agent) {
        return (nomes_de_instalacao_validos(agent) && alvo_lancavel(alvo)).then_some(Launch::Download);
    }
    let d = &agent["distribution"];
    let on_path = crate::agent::backends::on_path;
    let pacote = |ferramenta: &str| d[ferramenta]["package"].as_str().unwrap_or_default();
    if d["npx"].is_object() && on_path("npx") {
        return pacote_npm_fixo(pacote("npx")).is_ok().then_some(Launch::Npx);
    }
    if d["uvx"].is_object() && on_path("uvx") {
        return pacote_uv_fixo(pacote("uvx")).is_ok().then_some(Launch::Uvx);
    }
    None
}

/// Pede Node só quando o pacote npx é lançável com ele; senão, o que falta é o uv.
fn pede_node(d: &Value) -> bool {
    d["npx"].is_object() && pacote_npm_fixo(d["npx"]["package"].as_str().unwrap_or_default()).is_ok()
}

/// A versão não fixa que impede o lançamento: instalar Node ou uv não a resolve, então ela é dita antes de pedir a
/// ferramenta. Como em `launch_kind`, a ferramenta presente decide sozinha; sem nenhuma, só a recusa em todas explica.
fn versao_que_impede(d: &Value) -> Option<anyhow::Error> {
    let on_path = crate::agent::backends::on_path;
    let npx = d["npx"]
        .is_object()
        .then(|| (on_path("npx"), pacote_npm_fixo(d["npx"]["package"].as_str().unwrap_or_default())));
    let uvx = d["uvx"]
        .is_object()
        .then(|| (on_path("uvx"), pacote_uv_fixo(d["uvx"]["package"].as_str().unwrap_or_default())));
    let mut candidatos: Vec<_> = [npx, uvx].into_iter().flatten().collect();
    let todas_recusadas = !candidatos.is_empty() && candidatos.iter().all(|(_, versao)| versao.is_err());
    let indice = candidatos.iter().position(|(presente, _)| *presente).or(todas_recusadas.then_some(0))?;
    candidatos.swap_remove(indice).1.err()
}

/// A pasta de instalação nasce do id e da versão; nomes que `pasta_instalacao` recusaria não aparecem como disponíveis.
fn nomes_de_instalacao_validos(agente: &Value) -> bool {
    let versao = agente["version"].as_str().and_then(safe_component).unwrap_or("latest");
    id_instalacao(agente)
        .is_ok_and(|id| [id.as_str(), versao].iter().all(|nome| arquivo::caminho_relativo(Path::new(nome)).is_ok()))
}

/// As mesmas conferências de metadados que `command` faz antes de baixar: o que não passa aqui não aparece como disponível.
fn alvo_lancavel(alvo: &Value) -> bool {
    hash_obrigatorio(alvo).is_ok()
        && alvo["archive"].as_str().is_some()
        && alvo["cmd"].as_str().and_then(contained).is_some_and(|cmd| arquivo::caminho_relativo(cmd).is_ok())
}

fn alvo_binario(agente: &Value) -> Option<&Value> {
    let alvo = &agente["distribution"]["binary"][platform()?];
    alvo.is_object().then_some(alvo)
}

fn id_instalacao(agente: &Value) -> Result<String> {
    id_normalizado(agente).filter(|id| safe_component(id).is_some()).context("Identificador de agente inválido.")
}

pub(crate) fn chave_instalacao_binaria(agente: &Value) -> Result<Option<String>> {
    if fixados::agente(agente).is_some() || alvo_binario(agente).is_some() {
        return id_instalacao(agente).map(Some);
    }
    Ok(None)
}

pub(crate) async fn reservar_instalacao_binaria(id: String) -> OwnedMutexGuard<()> {
    type Travas = Mutex<HashMap<String, Weak<TravaAsync<()>>>>;
    static TRAVAS: LazyLock<Travas> = LazyLock::new(Mutex::default);
    // Uma publicação pode remover versões antigas do mesmo agente. A trava cobre a pasta
    // pai dessas versões, sem bloquear outros agentes durante download ou verificação.
    let trava = {
        let mut travas = TRAVAS.locked();
        travas.retain(|_, trava| trava.strong_count() != 0);
        if let Some(trava) = travas.get(&id).and_then(Weak::upgrade) {
            trava
        } else {
            let trava = Arc::new(TravaAsync::new(()));
            travas.insert(id, Arc::downgrade(&trava));
            trava
        }
    };
    trava.lock_owned().await
}

/// The agent runs through `npx` here (no build of its own for this platform).
pub fn needs_node(agent: &Value) -> bool {
    let d = &agent["distribution"];
    oferecido(agent) && fixados::agente(agent).is_none() && d["npx"].is_object() && alvo_binario(agent).is_none()
}

fn args_of(v: &Value) -> String {
    v["args"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(shell::quote).collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

/// `KEY=value ` pairs from a registry `env` object.
pub fn env_prefix(env: &Value) -> String {
    shell::env_prefix(env.as_object().into_iter().flatten().filter_map(|(k, v)| Some((k.as_str(), v.as_str()?))))
}

/// One path component made only of `[A-Za-z0-9._-]` and not `.`/`..`.
fn safe_component(s: &str) -> Option<&str> {
    let ok = !s.is_empty()
        && s != "."
        && s != ".."
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    ok.then_some(s)
}

/// A relative path that stays inside its base directory (no `..`, no root).
fn contained(rel: &str) -> Option<&Path> {
    let p = Path::new(rel);
    p.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir)).then_some(p)
}

/// How to run a registry agent: `program` alone (what ACP `terminal` sign-in
/// methods add their own args to) and `args` for its ACP mode.
pub struct Cmd {
    pub program: String,
    pub args: String,
}

impl Cmd {
    /// The shell command that starts it over ACP stdio.
    pub fn acp(&self) -> String {
        format!("{} {}", self.program, self.args)
    }
}

/// Downloads the agent first if needed.
/// `progress` receives short status lines with their stable code ("Downloading Cursor…", `downloading`).
pub async fn command(agent: &Value, progress: impl Fn(&str, Option<ActivityKind>)) -> Result<Cmd> {
    ensure!(oferecido(agent), "A instalação pelo catálogo está desativada para este agente.");
    let fixado = fixados::agente(agent).map(|fixo| fixo.selecionar(agent, platform())).transpose()?;
    if motor_install::orchestrating() {
        let item = agent["name"].as_str().unwrap_or("agent").to_owned();
        progress("Instalando adaptador como motor…", Some(ActivityKind::Installing { item }));
        return match motor_install::invoke(Request::Registry { agent: agent.clone() }).await? {
            Reply::Command { program, args } => Ok(Cmd { program, args }),
            _ => bail!("resposta inválida ao instalar adaptador"),
        };
    }
    if let Some(pacote) = fixado {
        return comando_binario(agent, &pacote, "", "acp".into(), &progress).await;
    }
    if let Some(target) = alvo_binario(agent) {
        // Para os demais agentes, o catálogo externo ainda fornece o hash e o endereço.
        let pacote = PacoteBinario {
            hash: hash_obrigatorio(target)?,
            url: target["archive"].as_str().context("O registro não informou o arquivo do agente.")?,
            cmd: target["cmd"].as_str().context("Caminho inválido para o executável do agente.")?,
            limite_download: MAX_ARCHIVE_BYTES,
            limite_extraido: arquivo::LIMITE_EXTRAIDO,
        };
        return comando_binario(agent, &pacote, &env_prefix(&target["env"]), args_of(target), &progress).await;
    }
    let name = agent["name"].as_str().unwrap_or("agent");
    let d = &agent["distribution"];
    match launch_kind(agent) {
        Some(Launch::Npx) => {
            let package = d["npx"]["package"].as_str().unwrap_or_default();
            pacote_npm_fixo(package)?;
            prepare_npx(package, name, &progress).await?;
            Ok(npx_cmd(&d["npx"]))
        }
        Some(Launch::Uvx) => {
            let package = d["uvx"]["package"].as_str().unwrap_or_default();
            pacote_uv_fixo(package)?;
            Ok(Cmd {
                program: format!("{}uvx {}", env_prefix(&d["uvx"]["env"]), shell::quote(package)),
                args: args_of(&d["uvx"]),
            })
        }
        _ => {
            if let Some(erro) = versao_que_impede(d) {
                return Err(erro);
            }
            if pede_node(d) {
                bail!("{name} needs Node.js on this computer: add it in Extensions");
            }
            bail!("{name} has no build for this computer (needs uv)")
        }
    }
}

struct PacoteBinario<'a> {
    url: &'a str,
    hash: &'a str,
    cmd: &'a str,
    limite_download: usize,
    limite_extraido: u64,
}

async fn comando_binario(
    agente: &Value,
    pacote: &PacoteBinario<'_>,
    env: &str,
    args: String,
    progress: &impl Fn(&str, Option<ActivityKind>),
) -> Result<Cmd> {
    let cmd = contained(pacote.cmd).context("Caminho inválido para o executável do agente.")?;
    let cmd = arquivo::caminho_relativo(cmd)?;
    let executable = install_binary(agente, pacote, &cmd, progress).await?.join(cmd);
    let executable = if motor_install::worker() { motor_install::published_path(&executable)? } else { executable };
    Ok(Cmd { program: format!("{env}{}", shell::quote(&executable.to_string_lossy())), args })
}

// Fixar a versão evita resolver tags ou faixas móveis. Não fixa as dependências transitivas,
// não autentica o catálogo externo e não audita o pacote nem seus scripts de instalação.
fn pacote_npm_fixo(pacote: &str) -> Result<()> {
    static NOME: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"\A(?:@[a-z0-9][a-z0-9._-]*/)?[a-z0-9][a-z0-9._-]*\z").expect("regex constante")
    });
    if pacote.rsplit_once('@').is_some_and(|(nome, versao)| {
        NOME.is_match(nome) && semver::Version::parse(versao.strip_prefix('v').unwrap_or(versao)).is_ok()
    }) {
        return Ok(());
    }
    bail!("O registro deve indicar uma versão exata do pacote npm (por exemplo, pacote@1.2.3).")
}

fn pacote_uv_fixo(pacote: &str) -> Result<()> {
    static ESPECIFICACAO: LazyLock<regex::Regex> = LazyLock::new(|| {
        let nome = r"[a-z0-9](?:[a-z0-9._-]*[a-z0-9])?";
        regex::Regex::new(&format!(
            r"(?i)\A{nome}(?:\[{nome}(?:,{nome})*\])?(?:==|@)v?(?:[0-9]+!)?[0-9]+(?:\.[0-9]+)*(?:[-_.]?(?:a|b|c|rc|alpha|beta|pre|preview)[-_.]?[0-9]*)?(?:-[0-9]+|[-_.]?(?:post|rev|r)[-_.]?[0-9]*)?(?:[-_.]?dev[-_.]?[0-9]*)?(?:\+[a-z0-9]+(?:[-_.][a-z0-9]+)*)?\z"
        )).expect("regex constante")
    });
    if pacote.is_ascii() && ESPECIFICACAO.is_match(pacote) {
        return Ok(());
    }
    bail!("O registro deve indicar uma versão exata do pacote uv (por exemplo, pacote==1.2.3).")
}

fn npx_cmd(npx: &Value) -> Cmd {
    Cmd {
        program: format!(
            "{}{}npx -y {}",
            env_prefix(&npx["env"]),
            motor_install::npm_prefix(),
            shell::quote(npx["package"].as_str().unwrap_or_default())
        ),
        args: args_of(npx),
    }
}

/// npx installs a package into `<npm cache>/_npx/<hash>` and writes its `package.json` last.
/// An install cut short (the first Codex download takes minutes, longer than the ACP handshake
/// may) leaves the packages without their bin links, and npx then fails with "command not
/// found" on every later run. So the download happens here, without the handshake's timeout,
/// and a half-finished one is started over.
async fn prepare_npx(package: &str, name: &str, progress: &impl Fn(&str, Option<ActivityKind>)) -> Result<()> {
    static INSTALLING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    if motor_install::orchestrating() {
        crate::fork::motor_exec::refuse("npm em raiz do Hub")?;
    }
    let _one_at_a_time = INSTALLING.lock().await;
    let dir = npm_cache().await?.join("_npx").join(npx_hash(package));
    if tokio::fs::try_exists(dir.join("package.json")).await.unwrap_or(false) {
        motor_install::check_npx(&dir)?;
        return Ok(());
    }
    let _ = tokio::fs::remove_dir_all(&dir).await;
    progress(&format!("Downloading {name}…"), Some(ActivityKind::Downloading { item: name.to_owned() }));
    let mut command = tokio::process::Command::new(shell::program("npm"));
    command
        // `exit 0`, not `true`: on Windows npm runs `-c` in cmd.exe, which has no `true`.
        .args(["exec", "--yes", "--package", package, "-c", "exit 0"])
        // Away from any project, so npx doesn't settle for a local node_modules.
        .current_dir(motor_install::root())
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let install = crate::fork::engine_setup::discovery_output(command);
    let failure = match tokio::time::timeout(Duration::from_secs(20 * 60), install).await {
        Ok(Ok(out)) if out.status.success() => {
            motor_install::check_npx(&dir)?;
            return Ok(());
        }
        Ok(Ok(out)) => npm_error(&String::from_utf8_lossy(&out.stderr)).to_owned(),
        Ok(Err(e)) => format!("{e}"),
        Err(_) => "timed out".to_owned(),
    };
    let _ = tokio::fs::remove_dir_all(&dir).await;
    bail!("couldn't download {name} with npm: {failure}")
}

/// The line of npm's stderr that says what went wrong: its own `npm notice` lines (a newer npm
/// is out) come after the error.
fn npm_error(stderr: &str) -> &str {
    stderr.lines().map(str::trim).rfind(|l| !l.is_empty() && !l.starts_with("npm notice")).unwrap_or_default()
}

/// `npm config get cache`, asked once.
async fn npm_cache() -> Result<&'static Path> {
    static DIR: tokio::sync::OnceCell<PathBuf> = tokio::sync::OnceCell::const_new();
    if motor_install::orchestrating() {
        crate::fork::motor_exec::refuse("npm em raiz do Hub")?;
    }
    let dir = DIR
        .get_or_try_init(|| async {
            let mut command = tokio::process::Command::new(shell::program("npm"));
            command.args(["config", "get", "cache"]).stdin(std::process::Stdio::null());
            let out = crate::fork::engine_setup::discovery_output(command).await.context("running npm")?;
            let dir = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            if !out.status.success() || dir.is_empty() {
                bail!("npm didn't report its cache directory");
            }
            Ok(PathBuf::from(dir))
        })
        .await?;
    Ok(dir)
}

/// npx's cache key for one package spec (libnpmexec: the first 16 hex digits of its SHA-512).
fn npx_hash(package: &str) -> String {
    use std::fmt::Write as _;
    sha2::Sha512::digest(package.as_bytes())[..8].iter().fold(String::with_capacity(16), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    digest_hex(&Sha256::digest(bytes))
}

fn digest_hex(digest: &[u8]) -> String {
    use std::fmt::Write as _;
    digest.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn hash_obrigatorio(alvo: &Value) -> Result<&str> {
    alvo["sha256"]
        .as_str()
        .filter(|hash| sha256_valido(hash))
        .context("O catálogo não trouxe um SHA-256 válido (64 dígitos hexadecimais) para este agente. A instalação foi recusada.")
}

fn sha256_valido(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

fn cliente_download_builder() -> reqwest::ClientBuilder {
    let limite = reqwest::redirect::Policy::default();
    let politica = reqwest::redirect::Policy::custom(move |tentativa| {
        if tentativa.url().scheme() != "https" {
            return tentativa.error("O download do agente não segue redirecionamentos para fora de HTTPS.");
        }
        limite.redirect(tentativa)
    });
    reqwest::Client::builder().https_only(true).redirect(politica)
}

async fn baixar_binario(cliente: &reqwest::Client, url: reqwest::Url, limite: usize) -> Result<Vec<u8>> {
    let resposta = cliente.get(url).timeout(Duration::from_secs(600)).send().await?.error_for_status()?;
    ensure!(resposta.status().is_success(), "O servidor não retornou o binário solicitado.");
    ler_download(resposta, limite).await
}

async fn ler_download(mut resposta: reqwest::Response, limite: usize) -> Result<Vec<u8>> {
    const ERRO: &str = "O download do agente excede o limite de tamanho.";
    ensure!(resposta.content_length().is_none_or(|n| n <= limite as u64), ERRO);
    let mut bytes = Vec::new();
    while let Some(parte) = resposta.chunk().await? {
        ensure!(parte.len() <= limite - bytes.len(), ERRO);
        bytes.extend_from_slice(&parte);
    }
    Ok(bytes)
}

/// Downloads + extracts a binary distribution into `~/.codync/agents/<id>/<version>`.
async fn install_binary(
    agent: &Value,
    pacote: &PacoteBinario<'_>,
    cmd: &Path,
    progress: &impl Fn(&str, Option<ActivityKind>),
) -> Result<PathBuf> {
    static CLIENTE: LazyLock<reqwest::Client> =
        LazyLock::new(|| cliente_download_builder().build().expect("cliente HTTPS do catálogo ACP"));
    let id = id_instalacao(agent)?;
    let version = agent["version"].as_str().and_then(safe_component).unwrap_or("latest").to_owned();
    let hash = pacote.hash.to_owned();
    let cmd = cmd.to_owned();
    let reserva = reservar_instalacao_binaria(id.clone()).await;
    let (dir, cache_valido) = {
        let (id, hash, cmd) = (id.clone(), hash.clone(), cmd.clone());
        tokio::task::spawn_blocking(move || {
            let dir = arquivo::pasta_instalacao(&motor_install::root(), &id, &version)?;
            let cache_valido = arquivo::cache_verificado(&dir, &hash, &cmd)?;
            Ok::<_, anyhow::Error>((dir, cache_valido))
        })
        .await??
    };
    if cache_valido {
        return Ok(dir);
    }
    let url = reqwest::Url::parse(pacote.url).or_else(|_| bail!("Endereço de download do agente inválido."))?;
    if url.scheme() != "https" {
        bail!("O download do agente exige HTTPS.");
    }
    let file_name =
        url.path_segments().and_then(Iterator::last).and_then(safe_component).unwrap_or("download").to_owned();
    let item = agent["name"].as_str().unwrap_or(&id).to_owned();
    progress(&format!("Downloading {item}…"), Some(ActivityKind::Downloading { item: item.clone() }));
    let bytes = baixar_binario(&CLIENTE, url, pacote.limite_download).await?;
    progress("Installing…", Some(ActivityKind::Installing { item }));
    let dir2 = dir.clone();
    let limite_extraido = pacote.limite_extraido;
    tokio::task::spawn_blocking(move || -> Result<()> {
        let _reserva = reserva;
        arquivo::publicar(&dir2, &file_name, &bytes, &hash, &cmd, limite_extraido)?;
        remove_other_versions(&dir2);
        Ok(())
    })
    .await??;
    Ok(dir)
}

/// Registry builds update often; keep only the version just installed.
fn remove_other_versions(installed: &Path) {
    let Some(parent) = installed.parent() else { return };
    let Ok(entries) = std::fs::read_dir(parent) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p != installed && !e.file_name().to_string_lossy().starts_with(".agente-") && p.is_dir() {
            let _ = std::fs::remove_dir_all(p);
        }
    }
}

/// Extrator do instalador de Node.js, para arquivos com hash fixado no próprio Hub.
pub(crate) fn extract(archive: &Path, dir: &Path, cmd: &Path) -> Result<()> {
    if motor_install::orchestrating() {
        crate::fork::motor_exec::refuse("extração de adaptadores")?;
    }
    let name = archive.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    // Multi-part extensions (`.tar.gz`) rule out `Path::extension`; `name` is already lowercased.
    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    // Windows' bsdtar (`tar.exe`) reads zips too; it has no `unzip`.
    let status = if name.ends_with(".zip") && cfg!(unix) {
        std::process::Command::new("unzip").arg("-q").arg("-o").arg(archive).arg("-d").arg(dir).status()
    } else if [".zip", ".tar.gz", ".tgz", ".tar.bz2", ".tbz2", ".tar.xz", ".tar"].iter().any(|e| name.ends_with(e)) {
        // Both GNU tar and bsdtar refuse `..` members by default.
        std::process::Command::new("tar").arg("-xf").arg(archive).arg("-C").arg(dir).status()
    } else {
        // Raw binary: it *is* the command.
        let exe = dir.join(cmd);
        if let Some(parent) = exe.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(archive, &exe)?;
        #[cfg(unix)]
        chmod_x(&exe)?;
        return Ok(());
    }
    .context("running the extractor")?;
    if !status.success() {
        bail!("couldn't extract {}", archive.display());
    }
    let _ = std::fs::remove_file(archive);
    #[cfg(unix)]
    if dir.join(cmd).exists() {
        chmod_x(&dir.join(cmd))?;
    }
    Ok(())
}

#[cfg(unix)]
fn chmod_x(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut p = std::fs::metadata(path)?.permissions();
    p.set_mode(p.mode() | 0o755);
    std::fs::set_permissions(path, p)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn antigravity_registry_nao_oferece_nem_instala_adaptador() {
        for id in ["antigravity", "Antigravity-ACP", " agy "] {
            let agent = json!({"id":id, "distribution":{"npx":{"package":"fixture@1.0.0"}}});
            assert_eq!(agentes_oferecidos(&json!({"agents":[agent.clone()]})), [] as [Value; 0]);
            assert!(launch_kind(&agent).is_none());
            assert!(command(&agent, |_, _| panic!("download inesperado")).await.is_err());
        }
    }

    #[test]
    fn lista_do_catalogo_omite_os_cinco_bloqueados_e_preserva_os_motores_nativos() {
        let v = json!({"agents": [
            {"id":"cursor"}, {"id":"devin"}, {"id":"junie"}, {"id":"cortex-code"},
            {"id":"corust-agent"}, {"id":"stakpak"}, {"id":"vtcode"}, {"id":"outro"}
        ]});
        let ids: Vec<_> = agentes_oferecidos(&v).into_iter().map(|a| a["id"].as_str().unwrap().to_owned()).collect();
        assert_eq!(ids, ["cursor", "devin", "outro"]);
        for id in ["junie", "cortex"] {
            assert!(crate::agent::backends::harness(id).unwrap().local.is_some());
        }
        for id in ["corust-agent", "stakpak", "vtcode"] {
            assert!(crate::agent::backends::harness(id).is_none());
        }
    }

    #[tokio::test]
    async fn nao_instala_do_catalogo_bloqueado_nem_troca_um_binario_fixado_por_npm() {
        for id in ["junie", "cortex-code", "corust-agent", "stakpak", "vtcode"] {
            let agente = json!({"id": id, "version": "1.0.0", "distribution":{"npx":{"package":"pacote@1.0.0"}}});
            let erro = command(&agente, |_, _| panic!("não deve iniciar a instalação")).await.err().unwrap();
            assert!(erro.to_string().contains("A instalação pelo catálogo está desativada"));
        }
        let agente = json!({"id":"cursor", "version":"0.0.1", "distribution":{"npx":{"package":"pacote@1.0.0"}}});
        let erro = command(&agente, |_, _| panic!("não deve iniciar a instalação")).await.err().unwrap();
        assert!(erro.to_string().contains("versão verificada"));
    }

    #[test]
    fn ids_do_catalogo_em_maiusculas_nao_contornam_a_pinagem_nem_o_bloqueio() {
        let cursor = json!({"id":"Cursor"});
        let fixado = fixados::agente(&cursor).unwrap();
        let esperado = fixados::agente(&json!({"id":"cursor"})).unwrap();
        assert!(std::ptr::eq(fixado, esperado));
        let junie = json!({"id":"Junie"});
        assert!(!oferecido(&junie));
        let agentes = agentes_oferecidos(&json!({"agents": [cursor, junie]}));
        assert_eq!(agentes, [json!({"id":"cursor"})]);
    }

    #[test]
    fn agentes_bloqueados_nao_anunciam_binario_nem_node() {
        for id in ["junie", "cortex-code", "corust-agent", "stakpak", "vtcode"] {
            let mut agente = json!({"id": id, "distribution": {"npx": {"package": "pacote@1.2.3"}}});
            assert!(!needs_node(&agente), "{id}");
            assert!(launch_kind(&agente).is_none(), "{id}");
            if let Some(plataforma) = platform() {
                agente["distribution"]["binary"][plataforma] = json!({"sha256": "a".repeat(64)});
                assert!(launch_kind(&agente).is_none(), "{id}");
            }
        }
    }

    #[tokio::test]
    async fn devin_fixado_recusa_fallback_para_npx() {
        let agente =
            json!({"id":"devin", "version":"3000.14.30821", "distribution":{"npx":{"package":"pacote@1.2.3"}}});
        assert!(launch_kind(&agente).is_none());
        assert!(!needs_node(&agente));
        let erro = command(&agente, |_, _| panic!("não deve iniciar a instalação")).await.err().unwrap();
        assert!(erro.to_string().contains("A instalação foi recusada."));
    }

    #[tokio::test]
    async fn binario_sem_sha256_valido_nao_aparece_como_disponivel() {
        let Some(plataforma) = platform() else { return };
        let mut agente = json!({
            "id": "exemplo",
            "distribution": {"binary": {(plataforma): {"cmd": "agente", "archive": "https://example.test/agente"}}}
        });
        assert!(launch_kind(&agente).is_none());
        agente["distribution"]["binary"][plataforma]["sha256"] = json!("curto");
        assert!(launch_kind(&agente).is_none());
        let erro = command(&agente, |_, _| panic!("não deve iniciar a instalação")).await.err().unwrap();
        assert!(erro.to_string().contains("O catálogo não trouxe um SHA-256"));
        agente["distribution"]["binary"][plataforma]["sha256"] = json!("a".repeat(64));
        assert_eq!(launch_kind(&agente), Some(Launch::Download));
    }

    #[test]
    fn launch_kind_nao_oferece_o_que_command_recusa() {
        let on_path = crate::agent::backends::on_path;
        let npx = |pacote: &str| json!({"id": "exemplo", "distribution": {"npx": {"package": pacote}}});
        for pacote in ["pacote", "pacote@latest", "pacote@^1.2.3"] {
            assert!(launch_kind(&npx(pacote)).is_none(), "{pacote}");
        }
        assert_eq!(launch_kind(&npx("pacote@1.2.3")), on_path("npx").then_some(Launch::Npx));
        let uvx = |pacote: &str| json!({"id": "exemplo", "distribution": {"uvx": {"package": pacote}}});
        for pacote in ["pacote", "pacote@latest", "pacote>=1.2"] {
            assert!(launch_kind(&uvx(pacote)).is_none(), "{pacote}");
        }
        assert_eq!(launch_kind(&uvx("pacote==1.2.3")), on_path("uvx").then_some(Launch::Uvx));
        if on_path("npx") {
            let ambos = json!({"id": "exemplo", "distribution": {"npx": {"package": "pacote"}, "uvx": {"package": "pacote==1.2.3"}}});
            assert!(launch_kind(&ambos).is_none(), "npx sem versão não cai para o uvx");
        }
        let Some(plataforma) = platform() else { return };
        let mut agente = json!({
            "id": "exemplo",
            "distribution": {"binary": {(plataforma): {"sha256": "a".repeat(64), "archive": "https://example.test/agente"}}}
        });
        assert!(launch_kind(&agente).is_none(), "sem cmd");
        agente["distribution"]["binary"][plataforma]["cmd"] = json!("../fora");
        assert!(launch_kind(&agente).is_none(), "cmd fora da pasta");
        agente["distribution"]["binary"][plataforma]["cmd"] = json!("agente");
        assert_eq!(launch_kind(&agente), Some(Launch::Download));
        agente["id"] = json!("../fora");
        assert!(launch_kind(&agente).is_none(), "id que não é componente de caminho");
        agente["id"] = json!("exemplo.");
        assert!(launch_kind(&agente).is_none(), "id que a pasta de instalação recusa");
        agente["id"] = json!("exemplo");
        agente["version"] = json!("1.2.");
        assert!(launch_kind(&agente).is_none(), "versão que a pasta de instalação recusa");
        agente.as_object_mut().unwrap().remove("version");
        agente["distribution"]["binary"][plataforma].as_object_mut().unwrap().remove("archive");
        assert!(launch_kind(&agente).is_none(), "sem archive");
    }

    #[test]
    fn pede_node_so_quando_o_pacote_npx_e_lancavel() {
        let d = |npx: &str, uvx: &str| json!({"npx": {"package": npx}, "uvx": {"package": uvx}});
        assert!(pede_node(&d("pacote@1.2.3", "pacote")));
        assert!(!pede_node(&d("pacote@latest", "pacote==1.2.3")));
        assert!(!pede_node(&json!({"uvx": {"package": "pacote==1.2.3"}})));
    }

    #[tokio::test]
    async fn pedir_node_so_quando_ha_um_lancamento_possivel_com_a_ferramenta() {
        let on_path = crate::agent::backends::on_path;
        if on_path("npx") || on_path("uvx") {
            return;
        }
        let agente = json!({"id": "exemplo", "distribution": {"npx": {"package": "pacote@1.2.3"}, "uvx": {"package": "pacote"}}});
        let erro = command(&agente, |_, _| panic!("não deve iniciar a instalação")).await.err().unwrap();
        assert!(erro.to_string().contains("needs Node.js"), "{erro}");
        let invertido = json!({"id": "exemplo", "distribution": {"npx": {"package": "pacote@latest"}, "uvx": {"package": "pacote==1.2.3"}}});
        let erro = command(&invertido, |_, _| panic!("não deve iniciar a instalação")).await.err().unwrap();
        assert!(erro.to_string().contains("needs uv"), "{erro}");
    }

    #[tokio::test]
    async fn pacote_sem_versao_exata_explica_o_motivo_em_vez_de_pedir_node() {
        let agente = json!({"id": "exemplo", "distribution": {"npx": {"package": "pacote@latest"}}});
        let erro = command(&agente, |_, _| panic!("não deve iniciar a instalação")).await.err().unwrap();
        assert!(erro.to_string().contains("versão exata"), "{erro}");
    }

    #[tokio::test]
    async fn download_recusa_redirecionamento_para_http_sem_entregar_bytes() {
        use axum::{Router, response::Redirect, routing::get};
        use std::net::Ipv4Addr;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let destino = format!("{base}/destino");
        let acessou_destino = Arc::new(AtomicBool::new(false));
        let acessou = acessou_destino.clone();
        let app = Router::new().route("/origem", get(move || async move { Redirect::temporary(&destino) })).route(
            "/destino",
            get(move || async move {
                acessou.store(true, Ordering::Relaxed);
                "binário fictício"
            }),
        );
        let servidor = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let cliente = cliente_download_builder().https_only(false).no_proxy().build().unwrap();
        let erro =
            baixar_binario(&cliente, reqwest::Url::parse(&format!("{base}/origem")).unwrap(), 1024).await.unwrap_err();
        assert!(erro.downcast_ref::<reqwest::Error>().is_some_and(reqwest::Error::is_redirect));
        assert!(!acessou_destino.load(Ordering::Relaxed));
        servidor.abort();
    }

    #[tokio::test]
    async fn trava_do_agente_serializa_a_mesma_chave_sem_bloquear_outra() {
        let chave = "agente-teste".to_owned();
        let primeira = reservar_instalacao_binaria(chave.clone()).await;
        let mut mesma = Box::pin(reservar_instalacao_binaria(chave));
        assert!(futures::poll!(&mut mesma).is_pending());
        let mut outra = Box::pin(reservar_instalacao_binaria("outro-agente-teste".to_owned()));
        assert!(futures::poll!(&mut outra).is_ready());
        drop(primeira);
        assert!(futures::poll!(&mut mesma).is_ready());
    }

    #[test]
    fn exige_sha256_com_64_hexadecimais_antes_de_baixar() {
        let erro = hash_obrigatorio(&json!({"sha256": null})).unwrap_err();
        assert!(erro.to_string().contains("O catálogo não trouxe um SHA-256"));
        assert!(hash_obrigatorio(&json!({"sha256": "abc"})).is_err());
        assert!(hash_obrigatorio(&json!({"sha256": "g".repeat(64)})).is_err());
        let alvo = json!({"sha256": "A".repeat(64)});
        assert_eq!(hash_obrigatorio(&alvo).unwrap(), "A".repeat(64));
    }

    #[tokio::test]
    async fn limita_download_sem_precisar_do_tamanho_informado_pelo_servidor() {
        use axum::{Router, body::Body, routing::get};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/pacote", listener.local_addr().unwrap());
        let app = Router::new().route(
            "/pacote",
            get(|| async {
                let partes = [Ok::<_, std::io::Error>("abc"), Ok("def"), Ok("ghi")];
                Body::from_stream(futures::stream::iter(partes))
            }),
        );
        let servidor = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let cliente = reqwest::Client::builder().no_proxy().build().unwrap();
        let resposta = cliente.get(&url).send().await.unwrap();
        assert_eq!(resposta.content_length(), None);
        let erro = ler_download(resposta, 8).await.unwrap_err();
        assert!(erro.to_string().contains("excede o limite de tamanho"));
        let resposta = cliente.get(url).send().await.unwrap();
        assert_eq!(ler_download(resposta, 9).await.unwrap(), b"abcdefghi");
        servidor.abort();
    }

    #[test]
    fn recusa_pacotes_sem_versao_exata() {
        for pacote in ["pacote", "@escopo/pacote", "pacote@latest", "pacote@^1.2.3", "pacote@1.2", "pacote@1.x"] {
            assert!(pacote_npm_fixo(pacote).is_err(), "{pacote}");
        }
        for pacote in ["pacote", "pacote@latest", "pacote>=1.2", "pacote~=1.2", "pacote==1.*", "pacote==1.2,!=1.2.1"] {
            assert!(pacote_uv_fixo(pacote).is_err(), "{pacote}");
        }
        assert!(pacote_npm_fixo("@escopo/pacote@1.2.3-beta.1").is_ok());
        assert!(pacote_uv_fixo("pacote==1.2.3rc1").is_ok());
        assert!(pacote_uv_fixo("pacote@1.2.3").is_ok());
    }

    #[test]
    fn npx_command_is_quoted() {
        let npx = json!({"package": "@s/x@1.0.0", "args": ["--acp", "a b"], "env": {"K": "v", "BAD;rm": "x"}});
        let expected =
            if cfg!(windows) { "K=v npx -y @s/x@1.0.0 --acp \"a b\"" } else { "env K=v npx -y @s/x@1.0.0 --acp 'a b'" };
        assert_eq!(npx_cmd(&npx).acp(), expected);
    }

    #[test]
    fn npm_errors_skip_its_update_notice() {
        let stderr = "'true' is not recognized as an internal or external command\n\
                      npm notice New major version of npm available! 10.9.2 -> 11.6.2\n\
                      npm notice To update run: npm install -g npm@11.6.2\n";
        assert_eq!(npm_error(stderr), "'true' is not recognized as an internal or external command");
        assert_eq!(npm_error(""), "");
    }

    #[test]
    fn npx_hash_matches_npm() {
        // ~/.npm/_npx/317ff93dc6c5b519 after `npx -y @agentclientprotocol/codex-acp@2.0.0`.
        assert_eq!(npx_hash("@agentclientprotocol/codex-acp@2.0.0"), "317ff93dc6c5b519");
    }

    #[test]
    fn keeps_only_the_installed_version() {
        let root = std::env::temp_dir().join(format!("codync-ver-{}", uuid::Uuid::new_v4()));
        for v in ["1.0.0", "1.1.0"] {
            std::fs::create_dir_all(root.join(v)).unwrap();
        }
        remove_other_versions(&root.join("1.1.0"));
        assert!(!root.join("1.0.0").exists());
        assert!(root.join("1.1.0").exists());
    }

    #[cfg(unix)]
    #[test]
    fn raw_binary_extract_marks_executable() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("codync-reg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("tool-darwin");
        let contents = b"#!/bin/sh\necho hi\n";
        std::fs::write(&archive, contents).unwrap();
        extract(&archive, &dir, Path::new("./bin/tool")).unwrap();
        let executable = dir.join("bin/tool");
        assert_eq!(std::fs::read(&executable).unwrap(), contents);
        assert_eq!(std::fs::metadata(&executable).unwrap().permissions().mode() & 0o111, 0o111);
        assert!(!archive.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn registry_paths_cannot_escape_the_agents_dir() {
        assert_eq!(safe_component("cursor"), Some("cursor"));
        assert_eq!(safe_component("1.0.3-beta"), Some("1.0.3-beta"));
        for bad in ["", ".", "..", "a/b", "../x", "x y"] {
            assert_eq!(safe_component(bad), None, "{bad:?}");
        }
        assert!(contained("./dist-package/cursor-agent").is_some());
        assert!(contained("../../bin/sh").is_none());
        assert!(contained("/bin/sh").is_none());
    }
}
