//! Coordenador sem privilégio da atualização privilegiada (MX-365, passos 4–7 e volta).
//!
//! Executa como o UID do Hub: para a unidade `systemd --user` e confirma pelo
//! cgroup que nenhum processo restou, obtém `host.lock` e a mantém durante toda a
//! troca, faz backup consistente do SQLite com hash e schema e passa pelo gate de
//! compatibilidade F3, chama o wrapper por `sudo -n` com as formas fixas, aponta a
//! unidade para o par gerido, só então libera a trava e inicia o serviço, confere a
//! saúde e aceita, ou executa a volta completa (DB, drop-ins, `--reverter`).
use anyhow::{Context, Result, bail, ensure};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const WRAPPER: &str = "/opt/donnabot/sbin/atualizar-privilegiado";
pub(super) const ATIVO: &str = "/opt/donnabot/releases/ativo/donnabot-host";
const RELEASES: &str = "/opt/donnabot/releases/";
pub(super) const UNIDADE: &str = "donnabot-host.service";
const DROPIN: &str = "zz-donnabot-atualizacao.conf";
/// Endereço do Hub nas unidades do layout gerido (#812: instalação limpa e migração): só
/// loopback. O acesso remoto é pelo túnel SSH do app, nunca por escuta em todas as interfaces.
pub(super) const BIND_GERIDO: &str = "127.0.0.1";

#[derive(Clone, Copy)]
pub(crate) struct Opcoes {
    /// Deixa a janela pendente (prova do guard na expiração).
    pub sem_aceite: bool,
    /// Executa a volta completa mesmo com o serviço saudável (prova da reversão).
    pub forcar_reversao: bool,
    pub saude_segundos: u64,
    /// Confirma uso de prova: sem ela, as duas opções acima são recusadas.
    pub prova: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifesto {
    banco_sha256: String,
    schema_sha256: String,
    raizes_f3: usize,
    unidade_sha256: String,
    dropins_sha256: BTreeMap<String, String>,
    habilitado: bool,
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut texto, byte| {
        let _ = write!(texto, "{byte:02x}");
        texto
    })
}

fn sha_arquivo(path: &Path) -> Result<String> {
    let mut arquivo = File::open(path).with_context(|| format!("abrindo {}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let lidos =
            std::io::Read::read(&mut arquivo, &mut buffer).with_context(|| format!("lendo {}", path.display()))?;
        if lidos == 0 {
            break;
        }
        hash.update(&buffer[..lidos]);
    }
    Ok(hex(&hash.finalize()))
}

pub(super) fn uid() -> u32 {
    // SAFETY: consulta sem efeitos colaterais.
    unsafe { libc::getuid() }
}

pub(super) fn systemctl(args: &[&str]) -> Result<std::process::Output> {
    let uid = uid();
    Ok(Command::new("/usr/bin/systemctl")
        .args(["--user", "--no-pager"])
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("XDG_RUNTIME_DIR", format!("/run/user/{uid}"))
        .env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path=/run/user/{uid}/bus"))
        .stdin(Stdio::null())
        .output()?)
}

fn systemctl_ok(args: &[&str]) -> Result<String> {
    let saida = systemctl(args)?;
    ensure!(
        saida.status.success(),
        "systemctl --user {} falhou: {}",
        args.join(" "),
        String::from_utf8_lossy(&saida.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&saida.stdout).trim().to_owned())
}

fn ativo() -> Result<bool> {
    Ok(String::from_utf8_lossy(&systemctl(&["is-active", UNIDADE])?.stdout).trim() == "active")
}

/// Cgroup da unidade sem processos (ou inexistente): o Hub e todos os filhos pararam.
fn cgroup_vazio() -> Result<bool> {
    let uid = uid();
    let base = PathBuf::from(format!("/sys/fs/cgroup/user.slice/user-{uid}.slice/user@{uid}.service"));
    for relativo in [format!("app.slice/{UNIDADE}"), UNIDADE.to_owned()] {
        match fs::read_to_string(base.join(relativo).join("cgroup.events")) {
            Ok(texto) => {
                if !texto.lines().any(|l| l == "populated 0") {
                    return Ok(false);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(true)
}

/// Espera ao menos o `TimeoutStopUSec` efetivo da unidade (depois dele o systemd
/// envia SIGKILL) mais folga; nunca menos que o padrão de 90 s.
fn prazo_parada() -> Duration {
    let configurado = systemctl_ok(&["show", "-p", "TimeoutStopUSec", "--value", UNIDADE])
        .ok()
        .and_then(|texto| segundos_systemd(&texto))
        .unwrap_or(90);
    Duration::from_secs(configurado.clamp(90, 3600) + 30)
}

/// Interpreta durações do `systemctl show` como "1min 30s", "90s", "500ms" ou "infinity".
fn segundos_systemd(texto: &str) -> Option<u64> {
    let texto = texto.trim();
    if texto == "infinity" {
        return Some(3600);
    }
    let mut total = 0u64;
    for parte in texto.split_whitespace() {
        let fim = parte.find(|c: char| !c.is_ascii_digit())?;
        let (numero, unidade) = parte.split_at(fim);
        let numero: u64 = numero.parse().ok()?;
        total += match unidade {
            "ms" | "us" => 0,
            "s" => numero,
            "min" => numero * 60,
            "h" => numero * 3600,
            _ => return None,
        };
    }
    Some(total)
}

pub(super) fn parar() -> Result<()> {
    systemctl_ok(&["stop", UNIDADE])?;
    let limite = Instant::now() + prazo_parada();
    while ativo()? || !cgroup_vazio()? {
        ensure!(Instant::now() < limite, "serviço do Hub ou filhos não terminaram; nada foi trocado");
        std::thread::sleep(Duration::from_millis(200));
    }
    println!("coordenador: serviço parado e cgroup vazio");
    Ok(())
}

/// Saudável = ativo continuamente pelo período e executando o binário esperado.
pub(super) fn saudavel(segundos: u64, gerido: bool) -> Result<bool> {
    let limite = Instant::now() + Duration::from_secs(segundos.max(1));
    while Instant::now() < limite {
        if !ativo()? {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let pid = systemctl_ok(&["show", "-p", "MainPID", "--value", UNIDADE])?;
    let pid: u32 = pid.parse().context("MainPID inválido")?;
    ensure!(pid > 0, "serviço sem processo principal");
    let proc_exe = format!("/proc/{pid}/exe");
    let exe = fs::read_link(&proc_exe)?;
    if gerido {
        let processo = fs::metadata(proc_exe)?;
        let selecionado = fs::metadata(ATIVO)?;
        Ok(exe.starts_with(RELEASES) && processo.dev() == selecionado.dev() && processo.ino() == selecionado.ino())
    } else {
        Ok(!exe.starts_with(RELEASES))
    }
}

pub(super) fn iniciar() -> Result<()> {
    systemctl_ok(&["daemon-reload"])?;
    // Paradas deliberadas do coordenador não devem esgotar o limite de partidas.
    let _ = systemctl(&["reset-failed", UNIDADE]);
    systemctl_ok(&["start", UNIDADE])?;
    Ok(())
}

/// Recusa wrappers antigos antes da troca: eles ignorariam o pedido e aceitariam direto.
pub(super) fn exigir_protocolo_wrapper() -> Result<()> {
    let caminho = Path::new(WRAPPER);
    for parte in caminho.ancestors() {
        let meta = fs::symlink_metadata(parte)?;
        ensure!(meta.uid() == 0 && meta.gid() == 0 && meta.mode() & 0o7022 == 0, "wrapper ou ancestral inseguro");
        ensure!(!meta.file_type().is_symlink(), "wrapper ou ancestral é link");
        if parte == caminho {
            ensure!(meta.is_file() && meta.nlink() == 1, "wrapper inválido");
        } else {
            ensure!(meta.is_dir(), "ancestral do wrapper inválido");
        }
    }
    let saida = Command::new(caminho)
        .arg("--protocolo-verificacao")
        .env_clear()
        .stdin(Stdio::null())
        .output()
        .context("consultando protocolo do wrapper")?;
    ensure!(
        resposta_protocolo_confere(saida.status.success(), &saida.stdout),
        "wrapper anterior ao protocolo de verificação; atualize a etapa administrativa"
    );
    Ok(())
}

fn resposta_protocolo_confere(sucesso: bool, bytes: &[u8]) -> bool {
    sucesso && bytes == b"mexerica-verificacao-v1\n"
}

pub(super) fn sudo(args: &[&str]) -> Result<bool> {
    let estado = Command::new("/usr/bin/sudo")
        .arg("-n")
        .arg(WRAPPER)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .status()?;
    Ok(estado.success())
}

/// Torna duráveis as entradas criadas, removidas ou renomeadas num diretório.
fn sincronizar_dir(path: &Path) -> Result<()> {
    File::open(path).and_then(|dir| dir.sync_all()).with_context(|| format!("sincronizando {}", path.display()))
}

/// Cria (se preciso) um diretório 0700 do Hub; um diretório novo tem o vínculo no pai
/// sincronizado antes de qualquer marca ou arquivo que dependa dele.
pub(super) fn diretorio_privado(path: &Path) -> Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => sincronizar_dir(path.parent().context("diretório sem pai")?)?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    }
    let meta = fs::symlink_metadata(path)?;
    ensure!(meta.is_dir() && meta.uid() == uid() && meta.mode() & 0o7777 == 0o700, "diretório de backup inseguro");
    Ok(())
}

fn copiar_privado(origem: &Path, destino: &Path) -> Result<()> {
    let bytes = fs::read(origem)?;
    let mut arquivo =
        OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(destino)?;
    arquivo.write_all(&bytes)?;
    arquivo.sync_all()?;
    Ok(())
}

pub(super) fn dir_unidade() -> PathBuf {
    crate::service::home().join(".config/systemd/user")
}

fn dropins() -> Result<BTreeMap<String, PathBuf>> {
    let dir = dir_unidade().join(format!("{UNIDADE}.d"));
    let mut mapa = BTreeMap::new();
    match fs::read_dir(&dir) {
        Ok(entradas) => {
            for entrada in entradas {
                let entrada = entrada?;
                let nome = entrada.file_name().to_str().context("drop-in com nome inválido")?.to_owned();
                ensure!(entrada.file_type()?.is_file(), "drop-in não regular: {nome}");
                mapa.insert(nome, entrada.path());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    Ok(mapa)
}

/// Backup consistente (VACUUM INTO lê um snapshot transacional), hashes, schema,
/// gate F3, unidade e o conjunto exato de drop-ins.
fn backup(data: &Path, destino: &Path) -> Result<Manifesto> {
    diretorio_privado(destino)?;
    let banco = destino.join("codync.db");
    {
        let origem = Connection::open(data.join("codync.db"))?;
        origem.execute("VACUUM INTO ?1", [banco.to_str().context("caminho não UTF-8")?])?;
    }
    fs::set_permissions(&banco, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    let copia = Connection::open(&banco)?;
    // Cópia em modo rollback journal: sem -wal/-shm ao conferir mais tarde.
    copia.pragma_update(None, "journal_mode", "DELETE")?;
    let integridade: String = copia.query_row("PRAGMA integrity_check", [], |l| l.get(0))?;
    ensure!(integridade == "ok", "backup SQLite sem integridade");
    let schema_sha256 = super::backup_atualizacao::schema(&copia)?;
    let raizes_f3 = super::motor_migration::inventario_compativel(&copia, data)
        .context("gate F3: inventário de raízes incompatível")?;
    drop(copia);
    let unidade = dir_unidade().join(UNIDADE);
    copiar_privado(&unidade, &destino.join("unidade"))?;
    let dir_dropins = destino.join("dropins");
    diretorio_privado(&dir_dropins)?;
    let mut dropins_sha256 = BTreeMap::new();
    for (nome, path) in dropins()? {
        copiar_privado(&path, &dir_dropins.join(&nome))?;
        dropins_sha256.insert(nome, sha_arquivo(&path)?);
    }
    let habilitado = systemctl(&["is-enabled", UNIDADE])?.status.success();
    let manifesto = Manifesto {
        banco_sha256: sha_arquivo(&banco)?,
        schema_sha256,
        raizes_f3,
        unidade_sha256: sha_arquivo(&unidade)?,
        dropins_sha256,
        habilitado,
    };
    let mut arquivo =
        OpenOptions::new().write(true).create_new(true).mode(0o600).open(destino.join("manifesto.json"))?;
    arquivo.write_all(&serde_json::to_vec(&manifesto)?)?;
    arquivo.sync_all()?;
    // Entradas do backup duráveis antes do wrapper (a volta depende delas).
    sincronizar_dir(&dir_dropins)?;
    sincronizar_dir(destino)?;
    println!(
        "coordenador: backup consistente (schema {}, {} raízes F3)",
        &manifesto.schema_sha256[..12],
        manifesto.raizes_f3
    );
    Ok(manifesto)
}

/// `argv` efetivo do `ExecStart` atual, sem aspas (recusa o que não for inequívoco).
pub(super) fn exec_start() -> Result<Vec<String>> {
    let texto = systemctl_ok(&["show", "-p", "ExecStart", "--value", UNIDADE])?;
    let arquivos = systemctl_ok(&["cat", "--no-pager", UNIDADE])?;
    recusar_prefixo_arroba(&arquivos)?;
    interpretar_exec_start(&texto)
}

/// O D-Bus pode omitir o prefixo @ quando argv[0] coincide com path. Inspeciona
/// também as diretivas dos fragmentos e drop-ins, sem registrar o conteúdo da unidade.
fn recusar_prefixo_arroba(arquivos: &str) -> Result<()> {
    for linha in arquivos.lines() {
        let linha = linha.trim();
        if let Some((chave, valor)) = linha.split_once('=')
            && chave.trim() == "ExecStart"
        {
            ensure!(!valor.contains(['@', '\\']), "ExecStart com prefixo ou continuação ambígua");
        }
    }
    Ok(())
}

fn interpretar_exec_start(texto: &str) -> Result<Vec<String>> {
    let texto = texto.trim();
    ensure!(texto.starts_with("{ ") && texto.ends_with(" }"), "ExecStart ilegível");
    ensure!(
        texto.matches("{ path=").count() == 1 && texto.matches("argv[]=").count() == 1,
        "unidade com zero ou vários ExecStart; nada alterado"
    );
    let path = texto.strip_prefix("{ path=").and_then(|s| s.split_once(" ; argv[]=")).context("ExecStart ilegível")?;
    let argv = path.1.split_once(" ;").context("ExecStart ilegível")?.0;
    ensure!(
        !path.0.contains([' ', '"', '\'', '\\', '$', '%', '@']) && path.0.starts_with('/'),
        "caminho ExecStart ambíguo"
    );
    ensure!(!argv.contains(['"', '\'', '\\', '$', '%']), "ExecStart com aspas ou expansões; ajuste manual exigido");
    let argumentos: Vec<String> = argv.split_whitespace().map(str::to_owned).collect();
    ensure!(
        argumentos.first().map(String::as_str) == Some(path.0),
        "caminho e argv[0] do ExecStart diferem; ajuste manual exigido"
    );
    Ok(argumentos)
}

/// Porta explícita e inequívoca da unidade, ou a porta padrão do Hub.
pub(super) fn porta_exec_start(argv: &[String]) -> Result<u16> {
    let mut iterador = argv.iter().skip(1);
    let mut porta = None;
    while let Some(arg) = iterador.next() {
        let valor = if arg == "--port" {
            Some(iterador.next().context("ExecStart com --port sem valor")?.as_str())
        } else {
            arg.strip_prefix("--port=")
        };
        if let Some(valor) = valor {
            ensure!(porta.is_none(), "ExecStart com múltiplas portas");
            let numero: u16 = valor.parse().context("porta do Hub inválida")?;
            ensure!(numero > 0, "porta do Hub inválida");
            porta = Some(numero);
        }
    }
    Ok(porta.unwrap_or(crate::service::DEFAULT_PORT))
}

/// Testa ambos os protocolos antes da partida. O bind exclusivo também recusa
/// listeners em interfaces diferentes do endereço de loopback da unidade gerida.
pub(super) fn exigir_porta_livre(porta: u16) -> Result<()> {
    let ipv4 = std::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, porta))
        .context("porta IPv4 do Hub indisponível antes da partida")?;
    drop(ipv4);
    match std::net::TcpListener::bind((std::net::Ipv6Addr::UNSPECIFIED, porta)) {
        Ok(ipv6) => drop(ipv6),
        Err(e) if e.raw_os_error() == Some(libc::EAFNOSUPPORT) => (),
        Err(e) => return Err(e).context("porta IPv6 do Hub indisponível antes da partida"),
    }
    Ok(())
}

/// Identidade da release publicada pelo wrapper root a partir do manifesto assinado.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReleaseEsperada {
    transacao: String,
    pub(super) versao: String,
    hub_sha256: String,
}

pub(super) fn release_esperada(transacao: &str) -> Result<ReleaseEsperada> {
    let bytes = super::publicacao_atualizacao::ler(super::publicacao_atualizacao::Nome::Release)?;
    interpretar_release(&bytes, transacao)
}

fn interpretar_release(bytes: &[u8], transacao: &str) -> Result<ReleaseEsperada> {
    let release: ReleaseEsperada = serde_json::from_slice(bytes).context("identidade da release inválida")?;
    ensure!(release.transacao == transacao, "release não corresponde à transação");
    semver::Version::parse(&release.versao).context("versão da release inválida")?;
    ensure!(
        release.hub_sha256.len() == 64
            && release.hub_sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "hash da release inválido"
    );
    Ok(release)
}

fn versao_binario_confere(saida: &str, esperada: &str) -> bool {
    let mut partes = saida.split_whitespace();
    partes.next() == Some("donnabot-host") && partes.next() == Some(esperada) && partes.next().is_none()
}

pub(super) fn conferir_binario_novo(release: &ReleaseEsperada) -> Result<()> {
    ensure!(sha_arquivo(Path::new(ATIVO))? == release.hub_sha256, "binário selecionado diverge do manifesto");
    let saida = Command::new(ATIVO)
        .arg("--version")
        .env_clear()
        .stdin(Stdio::null())
        .output()
        .context("consultando versão do binário selecionado")?;
    ensure!(saida.status.success() && saida.stdout.len() <= 4096, "versão do binário indisponível");
    let texto = std::str::from_utf8(&saida.stdout).context("versão do binário inválida")?;
    ensure!(versao_binario_confere(texto, &release.versao), "versão do binário diverge da release");
    Ok(())
}

/// O Hub declara as conferências ao wrapper sem ampliar as formas sudoers.
pub(super) fn registrar_verificacao(release: &ReleaseEsperada, porta: u16) -> Result<()> {
    ensure!(porta != 0, "porta inválida");
    let pedido = serde_json::to_vec(&serde_json::json!({
        "transacao": release.transacao,
        "hub_sha256": release.hub_sha256,
        "versao": release.versao,
        "porta": porta,
    }))?;
    let mut filho = Command::new("/usr/bin/sudo")
        .args(["-n", WRAPPER, "--aceitar"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .spawn()
        .context("iniciando registro da verificação")?;
    let mut entrada = filho.stdin.take().context("entrada do wrapper indisponível")?;
    let escrita = entrada.write_all(&pedido);
    drop(entrada);
    let estado = filho.wait()?;
    escrita.context("enviando verificação ao wrapper")?;
    ensure!(estado.success(), "registro da verificação recusado");
    Ok(())
}

fn versao_hello_confere(hello: &serde_json::Value, esperada: &str) -> bool {
    hello["version"].as_str() == Some(esperada)
}

pub(super) fn conferir_hello(porta: u16, esperada: &str) -> Result<()> {
    std::thread::scope(|escopo| {
        escopo
            .spawn(|| -> Result<()> {
                let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
                let resposta = runtime.block_on(crate::local_call(porta, "hello", serde_json::json!({})))?;
                ensure!(versao_hello_confere(&resposta, esperada), "versão do hello diverge da release");
                Ok(())
            })
            .join()
            .map_err(|_| anyhow::anyhow!("verificação do hello interrompida"))?
    })
}

/// `argv` da unidade gerida a partir do `ExecStart` atual: o seletor no lugar do
/// executável, os argumentos de `serve` preservados e o endereço sempre
/// `--bind 127.0.0.1` (qualquer `--bind` anterior, inclusive `0.0.0.0`, é substituído).
fn argv_gerido(argv: &[String]) -> Result<Vec<String>> {
    let mut resto = argv.iter().skip(1);
    match resto.next().map(String::as_str) {
        // Sem subcomando, o Hub executa `serve` com os padrões.
        None | Some("serve") => (),
        Some(outro) => bail!("ExecStart do Hub executa `{outro}`, não `serve`; ajuste manual exigido"),
    }
    let mut gerido = vec![ATIVO.to_owned(), "serve".to_owned()];
    while let Some(argumento) = resto.next() {
        if argumento == "--bind" {
            ensure!(resto.next().is_some(), "ExecStart com `--bind` sem endereço; ajuste manual exigido");
        } else if !argumento.starts_with("--bind=") {
            gerido.push(argumento.clone());
        }
    }
    gerido.extend(["--bind".to_owned(), BIND_GERIDO.to_owned()]);
    Ok(gerido)
}

fn apontar_para_gerido() -> Result<()> {
    let argv = exec_start()?;
    let gerido = argv_gerido(&argv)?;
    if argv == gerido {
        return Ok(());
    }
    let dir = dir_unidade().join(format!("{UNIDADE}.d"));
    match fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => sincronizar_dir(&dir_unidade())?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    }
    let texto = format!(
        "# Gerado pelo coordenador da atualização privilegiada.\n[Service]\nExecStart=\nExecStart={}\n",
        gerido.join(" ")
    );
    let temporario = dir.join(format!(".{DROPIN}.tmp"));
    // Temporário de uma tentativa interrompida (só o Hub escreve aqui).
    match fs::remove_file(&temporario) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    let mut arquivo =
        OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&temporario)?;
    arquivo.write_all(texto.as_bytes())?;
    arquivo.sync_all()?;
    fs::rename(&temporario, dir.join(DROPIN))?;
    // O drop-in precisa estar durável antes da marca `apontado` (gravada em outro diretório).
    sincronizar_dir(&dir)?;
    systemctl_ok(&["daemon-reload"])?;
    ensure!(exec_start()? == gerido, "unidade não aponta para o par gerido em {BIND_GERIDO}");
    println!("coordenador: unidade aponta para {ATIVO} em {BIND_GERIDO}");
    Ok(())
}

/// Restaura exatamente o conjunto de drop-ins do backup (remove os que não existiam).
fn restaurar_dropins(backup: &Path, manifesto: &Manifesto) -> Result<()> {
    let dir = dir_unidade().join(format!("{UNIDADE}.d"));
    for (nome, path) in dropins()? {
        if !manifesto.dropins_sha256.contains_key(&nome) {
            fs::remove_file(path)?;
        }
    }
    for (nome, soma) in &manifesto.dropins_sha256 {
        let origem = backup.join("dropins").join(nome);
        ensure!(&sha_arquivo(&origem)? == soma, "drop-in do backup adulterado");
        let destino = dir.join(nome);
        if fs::symlink_metadata(&destino).is_ok() && &sha_arquivo(&destino)? == soma {
            continue;
        }
        let _ = fs::remove_file(&destino);
        copiar_privado(&origem, &destino)?;
    }
    // Remoções e criações duráveis antes de concluir a volta (e antes do `--reverter`).
    match fs::symlink_metadata(&dir) {
        Ok(_) => sincronizar_dir(&dir)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    ensure!(
        sha_arquivo(&dir_unidade().join(UNIDADE))? == manifesto.unidade_sha256,
        "unidade alterada fora do coordenador"
    );
    Ok(())
}

const TRABALHO: &str = "/var/lib/donnabot/trabalho";
const CONTAS: &str = "/var/lib/donnabot/motor/engine-setup/accounts";
/// Área de staging privada do Hub dentro de `trabalho/` (mesmo sistema de arquivos do
/// destino, para a publicação por rename). É 0700 do Hub: o motor, que escreve em
/// `trabalho/` pelo grupo, não cria, renomeia nem apaga entradas dentro dela.
const AREA_STAGING: &str = ".donnabot-staging-hub";
/// Identidade fixada da área (dispositivo, inode e criação), no diretório privado de
/// backups do Hub: uma pasta posta no lugar dela nunca tem o conteúdo apagado.
const IDENTIDADE_AREA: &str = "staging-trabalho.id";

/// Modos do contrato F3 por tipo de raiz de trabalho: (pasta, arquivo, preserva execução).
/// Workspace preserva o bit de execução do dono, estendido ao grupo; uploads e skills
/// nunca são executáveis.
fn modos_trabalho(tipo: &str) -> (u32, u32, bool) {
    if tipo == "workspace" { (0o2770, 0o660, true) } else { (0o2750, 0o640, false) }
}

/// Manifesto de uma árvore aberta (mesmo cálculo do canal do motor), por descritores e
/// sem seguir links. O bit de execução entra normalizado pela transformação do destino
/// (`preserva_execucao`), para origem e cópia serem comparáveis.
fn manifesto_local(raiz: &File, excluir: &[(u64, u64)], preserva_execucao: bool) -> Result<String> {
    use super::motor_copia::{Manifesto, abrir_em, nomes_ordenados};
    fn visitar(
        dir: &File,
        relativo: &str,
        excluir: &[(u64, u64)],
        preserva: bool,
        manifesto: &mut Manifesto,
    ) -> Result<()> {
        for nome in nomes_ordenados(dir)? {
            let objeto = abrir_em(dir, &nome)?;
            if excluir.contains(&(objeto.meta.dev(), objeto.meta.ino())) {
                continue;
            }
            let filho = if relativo.is_empty() { nome.clone() } else { format!("{relativo}/{nome}") };
            if objeto.pasta() {
                manifesto.pasta(&filho);
                visitar(&objeto.arquivo, &filho, excluir, preserva, manifesto)?;
            } else {
                let (tamanho, modo) = (objeto.meta.len(), objeto.meta.mode());
                let conteudo = sha_de(objeto.arquivo, tamanho)?;
                manifesto.arquivo(&filho, tamanho, preserva && modo & 0o100 != 0, &conteudo);
            }
        }
        Ok(())
    }
    let mut manifesto = Manifesto::default();
    visitar(raiz, "", excluir, preserva_execucao, &mut manifesto)?;
    Ok(manifesto.fim())
}

/// SHA-256 de exatamente `tamanho` bytes de um arquivo aberto.
fn sha_de(mut arquivo: File, tamanho: u64) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut restante = tamanho;
    while restante > 0 {
        let parte = usize::try_from(restante.min(buffer.len() as u64))?;
        arquivo.read_exact(&mut buffer[..parte]).context("origem mudou durante a cópia")?;
        hash.update(&buffer[..parte]);
        restante -= parte as u64;
    }
    ensure!(arquivo.metadata()?.len() == tamanho, "origem mudou durante a cópia");
    Ok(hash.finalize().to_vec())
}

fn fchmod(arquivo: &File, modo: u32) -> Result<()> {
    // SAFETY: descritor válido; modo explícito do contrato.
    ensure!(unsafe { libc::fchmod(arquivo.as_raw_fd(), modo) } == 0, "{}", std::io::Error::last_os_error());
    Ok(())
}

/// Grupo de trabalho por descritor e só então o modo do contrato (`fchown` de quem não é
/// root limpa o setgid; o `fchmod` seguinte o devolve). Não depende do grupo herdado.
fn fchown_fchmod(arquivo: &File, gid: u32, modo: u32) -> Result<()> {
    // SAFETY: descritor válido; dono inalterado (-1) e grupo de trabalho explícito.
    let rc = unsafe { libc::fchown(arquivo.as_raw_fd(), libc::uid_t::MAX, gid) };
    ensure!(rc == 0, "grupo de trabalho {gid} recusado: {}", std::io::Error::last_os_error());
    fchmod(arquivo, modo)
}

/// `work_gid` único do `motor.conf` root.
fn work_gid(texto: &str) -> Result<u32> {
    let mut valores = texto.lines().filter_map(|linha| linha.strip_prefix("work_gid="));
    let gid = valores.next().context("motor.conf sem work_gid")?.parse().context("work_gid inválido")?;
    ensure!(valores.next().is_none(), "motor.conf com work_gid repetido");
    Ok(gid)
}

/// GID de trabalho do contrato F3: o `work_gid` do `motor.conf`, que precisa ser o grupo
/// da raiz `trabalho/` (do Hub, 2770) e diferente do grupo primário do Hub, o que o motor
/// não tem. Toda árvore publicada recebe esse grupo explicitamente.
fn gid_trabalho(trabalho: &File) -> Result<u32> {
    let texto = fs::read_to_string(super::motor_exec::CONFIG).context("lendo motor.conf")?;
    let gid = work_gid(&texto)?;
    // SAFETY: consulta sem efeitos colaterais.
    let primario = unsafe { libc::getgid() };
    ensure!(gid != primario, "grupo de trabalho igual ao grupo primário do Hub; nada copiado");
    let meta = trabalho.metadata()?;
    ensure!(
        meta.uid() == uid() && meta.gid() == gid && meta.mode() & 0o7777 == 0o2770,
        "raiz de trabalho fora do contrato (dono Hub, grupo {gid}, 2770); nada copiado"
    );
    Ok(gid)
}

fn criar_pasta_em(pai: &File, nome: &str) -> Result<File> {
    use super::motor_copia::{arquivo, c, componente};
    componente(nome)?;
    let nome_c = c(nome)?;
    // SAFETY: componente validado relativo ao descritor; mkdirat não segue links.
    let rc = unsafe { libc::mkdirat(pai.as_raw_fd(), nome_c.as_ptr(), 0o700) };
    ensure!(rc == 0, "{}", std::io::Error::last_os_error());
    // SAFETY: mesmo componente recém-criado, sem seguir links.
    arquivo(unsafe {
        libc::openat(
            pai.as_raw_fd(),
            nome_c.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    })
}

/// Copia a árvore aberta `origem` para a pasta nova `nome` em `destino_pai`, por
/// descritores, com o grupo de trabalho `gid` e os modos do contrato em cada objeto;
/// arquivos e pastas sincronizados de baixo para cima.
fn copiar_arvore(
    origem: &File,
    destino_pai: &File,
    nome: &str,
    excluir: &[(u64, u64)],
    tipo: &str,
    gid: u32,
) -> Result<()> {
    use super::motor_copia::{abrir_em, arquivo, c, nomes_ordenados};
    let (modo_pasta, modo_arquivo, preserva) = modos_trabalho(tipo);
    let destino = criar_pasta_em(destino_pai, nome)?;
    for filho in nomes_ordenados(origem)? {
        let objeto = abrir_em(origem, &filho)?;
        if excluir.contains(&(objeto.meta.dev(), objeto.meta.ino())) {
            continue;
        }
        if objeto.pasta() {
            copiar_arvore(&objeto.arquivo, &destino, &filho, excluir, tipo, gid)?;
            continue;
        }
        let nome_c = c(&filho)?;
        // SAFETY: componente validado por abrir_em; criação exclusiva sem seguir links.
        let mut saida = arquivo(unsafe {
            libc::openat(
                destino.as_raw_fd(),
                nome_c.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        })?;
        let (tamanho, modo) = (objeto.meta.len(), objeto.meta.mode());
        let copiados = std::io::copy(&mut std::io::Read::take(&objeto.arquivo, tamanho), &mut saida)?;
        ensure!(copiados == tamanho && objeto.arquivo.metadata()?.len() == tamanho, "origem mudou durante a cópia");
        let execucao = if preserva { (modo & 0o100) | ((modo & 0o100) >> 3) } else { 0 };
        fchown_fchmod(&saida, gid, modo_arquivo | execucao)?;
        saida.sync_all()?;
    }
    fchown_fchmod(&destino, gid, modo_pasta)?;
    destino.sync_all()?;
    Ok(())
}

/// Confere dono (Hub), grupo de trabalho `gid`, tipos e modos do contrato em toda a árvore
/// de trabalho e a torna durável (arquivos e pastas, de baixo para cima). Usado na cópia
/// nova e na repetição: uma árvore com outro grupo deixaria o motor sem acesso.
fn conferir_modos(dir: &File, tipo: &str, gid: u32) -> Result<()> {
    use super::motor_copia::{abrir_em, nomes_ordenados};
    let (modo_pasta, modo_arquivo, preserva) = modos_trabalho(tipo);
    for nome in nomes_ordenados(dir)? {
        let objeto = abrir_em(dir, &nome)?;
        let modo = objeto.meta.mode() & 0o7777;
        ensure!(
            objeto.meta.uid() == uid() && objeto.meta.gid() == gid,
            "dono ou grupo fora do contrato F3 em {nome}: {}:{} (esperado {}:{gid})",
            objeto.meta.uid(),
            objeto.meta.gid(),
            uid()
        );
        if objeto.pasta() {
            conferir_modos(&objeto.arquivo, tipo, gid)?;
        } else {
            let execucao = modo & 0o110;
            ensure!(
                modo & !0o110 == modo_arquivo && (execucao == 0 || (preserva && execucao == 0o110)),
                "modo fora do contrato F3 em {nome}: {modo:o}"
            );
            objeto.arquivo.sync_all()?;
        }
    }
    let meta = dir.metadata()?;
    ensure!(meta.mode() & 0o7777 == modo_pasta, "modo de pasta fora do contrato F3");
    ensure!(meta.uid() == uid() && meta.gid() == gid, "dono ou grupo de pasta fora do contrato F3");
    dir.sync_all()?;
    Ok(())
}

/// Publica `origem` (na área de staging) como `destino` em `pai`, sem nunca substituir.
fn renomear_sem_substituir(area: &File, origem: &str, pai: &File, destino: &str) -> Result<()> {
    use super::motor_copia::c;
    let (a, b) = (c(origem)?, c(destino)?);
    // SAFETY: nomes validados relativos a diretórios abertos; RENAME_NOREPLACE nunca substitui.
    let rc =
        unsafe { libc::renameat2(area.as_raw_fd(), a.as_ptr(), pai.as_raw_fd(), b.as_ptr(), libc::RENAME_NOREPLACE) };
    ensure!(rc == 0, "publicação da cópia recusada: {}", std::io::Error::last_os_error());
    Ok(())
}

/// Identidade estável de um diretório aberto: dispositivo, inode e instante de criação
/// (`statx`), para um inode reaproveitado não passar pela área original.
fn identidade(dir: &File) -> Result<String> {
    // SAFETY: estrutura local zerada; statx preenche no máximo o próprio tamanho.
    let mut meta: libc::statx = unsafe { std::mem::zeroed() };
    // SAFETY: descritor válido, caminho vazio com AT_EMPTY_PATH e ponteiro para a estrutura local.
    let rc = unsafe {
        libc::statx(
            dir.as_raw_fd(),
            c"".as_ptr(),
            libc::AT_EMPTY_PATH,
            libc::STATX_INO | libc::STATX_BTIME,
            &raw mut meta,
        )
    };
    ensure!(rc == 0, "statx da área de staging: {}", std::io::Error::last_os_error());
    let criacao = if meta.stx_mask & libc::STATX_BTIME != 0 {
        format!("{}.{}", meta.stx_btime.tv_sec, meta.stx_btime.tv_nsec)
    } else {
        "-".to_owned()
    };
    Ok(format!("{}:{}:{}:{criacao}", meta.stx_dev_major, meta.stx_dev_minor, meta.stx_ino))
}

/// Abre (ou cria) a área de staging privada do Hub em `trabalho/` e coleta o que restou
/// de tentativas interrompidas. Só o Hub escreve dentro dela, então tudo o que contém é
/// staging dele. Uma área vazia, do Hub e 0700, pode ser (re)adotada sem risco, pois não
/// há nada a apagar; uma área com conteúdo precisa ser exatamente a registrada. Qualquer
/// outra coisa no lugar recusa sem remover nada.
fn area_staging(trabalho: &File, lista_dir: &File) -> Result<File> {
    use super::motor_copia::{abrir_em, acrescentar, c, estado, ler_linhas, nomes, remover_em};
    if estado(trabalho, AREA_STAGING)?.is_none() {
        let nova = criar_pasta_em(trabalho, AREA_STAGING)?;
        drop(nova);
        trabalho.sync_all()?;
    }
    let area = abrir_em(trabalho, AREA_STAGING)?;
    ensure!(area.pasta() && area.meta.uid() == uid(), "área de staging de trabalho com outro dono; nada removido");
    let atual = identidade(&area.arquivo)?;
    let registro = ler_linhas(lista_dir, IDENTIDADE_AREA)?;
    if nomes(&area.arquivo)?.is_empty() {
        if registro.as_slice() != [atual.clone()] {
            if !registro.is_empty() {
                let nome = c(IDENTIDADE_AREA)?;
                // SAFETY: nome fixo no diretório privado aberto; troca o registro da área vazia.
                ensure!(unsafe { libc::unlinkat(lista_dir.as_raw_fd(), nome.as_ptr(), 0) } == 0, "registro da área");
            }
            acrescentar(lista_dir, IDENTIDADE_AREA, &atual)?;
        }
        fchmod(&area.arquivo, 0o700)?;
    } else {
        ensure!(
            registro.as_slice() == [atual] && area.meta.mode() & 0o7777 == 0o700,
            "área de staging de trabalho substituída ou adulterada; nada removido, revisão manual"
        );
        for nome in nomes(&area.arquivo)? {
            println!("coordenador: staging de trabalho de tentativa interrompida removido: {nome}");
            remover_em(&area.arquivo, &nome, 0)?;
        }
    }
    area.arquivo.sync_all()?;
    Ok(area.arquivo)
}

/// Raiz de trabalho F3 e o lugar dela.
struct Trabalho<'a> {
    /// Raiz `trabalho/` (`TRABALHO` fora dos testes).
    raiz: &'a Path,
    /// Área privada de staging do Hub dentro da raiz.
    area: &'a File,
    /// Grupo de trabalho do contrato, aplicado por descritor a cada objeto publicado.
    gid: u32,
}

/// Raízes de trabalho pertencem ao Hub (contrato F3): o próprio Hub copia, por
/// descritores, com o grupo de trabalho atribuído explicitamente (a área de staging é
/// 0700 sem setgid, então nada é herdado) e os modos do contrato.
fn copiar_trabalho(origem: &Path, destino: &Path, tipo: &str, excluir: &[PathBuf], t: &Trabalho) -> Result<()> {
    use super::motor_copia::{abrir_em, abrir_raiz, estado, remover_em};
    let (area, gid) = (t.area, t.gid);
    ensure!(destino.starts_with(t.raiz) && destino != t.raiz, "destino F3 fora da raiz de trabalho");
    let (_, _, preserva) = modos_trabalho(tipo);
    let raiz = abrir_raiz(origem)?;
    ensure!(raiz.pasta(), "raiz de trabalho F3 deve ser pasta");
    // Raízes F3 aninhadas nesta origem são copiadas à parte (identificadas por inode).
    let excluir: Vec<(u64, u64)> = excluir
        .iter()
        .map(|caminho| abrir_raiz(caminho).map(|o| (o.meta.dev(), o.meta.ino())))
        .collect::<Result<_>>()?;
    let esperado = manifesto_local(&raiz.arquivo, &excluir, preserva)?;
    // Ancestrais do destino abertos (ou criados 2770 com o grupo de trabalho) componente a
    // componente, sem links; um ancestral existente precisa ser do Hub com esse grupo.
    let mut ancestrais = vec![abrir_raiz(t.raiz)?.arquivo];
    let relativo: Vec<String> = destino
        .strip_prefix(t.raiz)?
        .components()
        .map(|p| p.as_os_str().to_str().map(str::to_owned).context("caminho não UTF-8"))
        .collect::<Result<_>>()?;
    let (final_, pais) = relativo.split_last().context("destino sem nome")?;
    for parte in pais {
        let atual = ancestrais.last().context("sem ancestral")?;
        let proximo = if estado(atual, parte)?.is_some() {
            let objeto = abrir_em(atual, parte)?;
            ensure!(
                objeto.pasta() && objeto.meta.uid() == uid() && objeto.meta.gid() == gid,
                "ancestral F3 inseguro ou fora do grupo de trabalho"
            );
            objeto.arquivo
        } else {
            let nova = criar_pasta_em(atual, parte)?;
            fchown_fchmod(&nova, gid, 0o2770)?;
            nova
        };
        ancestrais.push(proximo);
    }
    let pai = ancestrais.last().context("sem pai")?.try_clone()?;
    if estado(&pai, final_)?.is_some() {
        // Repetição após volta ou queda: só aceita o mesmo conteúdo e modos do contrato.
        let atual = abrir_em(&pai, final_)?;
        ensure!(
            atual.pasta() && manifesto_local(&atual.arquivo, &[], preserva)? == esperado,
            "destino F3 já existe com conteúdo diferente"
        );
        conferir_modos(&atual.arquivo, tipo, gid)?;
    } else {
        // Staging dentro da área privada do Hub: o motor não alcança o nome nem o conteúdo
        // até o rename que publica a cópia, e uma queda antes dele deixa só lixo coletável
        // ali. Depois do rename, nada mais aponta para o staging.
        let temporario = format!(".copia-{}", uuid::Uuid::new_v4().simple());
        let resultado = (|| -> Result<()> {
            copiar_arvore(&raiz.arquivo, area, &temporario, &excluir, tipo, gid)?;
            let copia = abrir_em(area, &temporario)?;
            ensure!(manifesto_local(&copia.arquivo, &[], preserva)? == esperado, "cópia F3 divergente da origem");
            conferir_modos(&copia.arquivo, tipo, gid)?;
            renomear_sem_substituir(area, &temporario, &pai, final_)?;
            area.sync_all()?;
            Ok(())
        })();
        if let Err(erro) = resultado {
            let _ = remover_em(area, &temporario, 0);
            return Err(erro);
        }
    }
    // Ligações duráveis de baixo para cima, inclusive de ancestrais recém-criados.
    for dir in ancestrais.iter().rev() {
        dir.sync_all()?;
    }
    Ok(())
}

/// Executa a F3 completa: cópias conferidas (trabalho pelo Hub, perfis pelo canal
/// do motor) e então a reescrita das referências com a `host.lock` herdada.
pub(super) fn migrar_raizes(data: &Path, trava: &File) -> Result<()> {
    if !super::motor_exec::configured() {
        return Ok(());
    }
    let itens = {
        let db = Connection::open_with_flags(data.join("codync.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        super::motor_migration::inventario_copias(&db, data)?
    };
    if itens.is_empty() {
        return Ok(());
    }
    // Área privada de staging e coleta de uma coordenação interrompida, sob a host.lock
    // (nenhum staging em uso coexiste).
    let lista_dir = super::motor_copia::abrir_raiz(&data.join("atualizacao-privilegiada"))?.arquivo;
    let trabalho = super::motor_copia::abrir_raiz(Path::new(TRABALHO))?.arquivo;
    let gid = gid_trabalho(&trabalho)?;
    let area = area_staging(&trabalho, &lista_dir)?;
    let lugar = Trabalho { raiz: Path::new(TRABALHO), area: &area, gid };
    let mut perfis = 0usize;
    for (origem, destino, tipo) in &itens {
        if *tipo == "profile" {
            let relativo = destino.strip_prefix(CONTAS).context("perfil fora da raiz de contas do motor")?;
            let mut partes = relativo.iter().map(|p| p.to_str().context("caminho não UTF-8"));
            let conta = partes.next().context("perfil sem conta")??;
            let resto = partes.collect::<Result<Vec<_>>>()?.join("/");
            let resposta = super::motor_copia::enviar(Path::new(super::motor_copia::SOCKET), conta, &resto, origem)?;
            perfis += 1;
            println!(
                "coordenador: perfil copiado pelo canal do motor ({} arquivos{})",
                resposta.arquivos,
                if resposta.ja_existia { ", já existente e idêntico" } else { "" }
            );
        } else {
            let excluir: Vec<PathBuf> = itens
                .iter()
                .filter(|(outra, _, _)| outra != origem && outra.starts_with(origem))
                .map(|(outra, _, _)| outra.clone())
                .collect();
            copiar_trabalho(origem, destino, tipo, &excluir, &lugar)?;
        }
    }
    super::motor_migration::migrar_com_trava(data, trava)?;
    println!("coordenador: F3 concluída ({} raízes, {perfis} perfis pelo canal do motor)", itens.len());
    Ok(())
}

/// Volta completa: DB do backup vinculado à transação, posterior preservado, drop-ins, `--reverter`.
fn voltar(data: &Path, backup_dir: &Path, manifesto: &Manifesto, trava: &File) -> Result<()> {
    voltar_com(data, backup_dir, manifesto, trava, true)
}

/// Volta completa; `restaurar_banco = false` só quando o DB atual comprovadamente não
/// recebeu a F3 de uma primeira migração (continua compatível com o par legado).
fn voltar_com(
    data: &Path,
    backup_dir: &Path,
    manifesto: &Manifesto,
    trava: &File,
    restaurar_banco: bool,
) -> Result<()> {
    let certidao = super::backup_atualizacao::ler_certidao(uid())?;
    let comprovante: super::backup_atualizacao::Comprovante =
        serde_json::from_slice(&fs::read(backup_dir.join("comprovante.json"))?).context("comprovante ausente")?;
    let banco = backup_dir.join("codync.db");
    {
        let db = Connection::open(&banco)?;
        let mut arquivo = File::open(&banco)?;
        super::backup_atualizacao::conferir(&db, &mut arquivo, trava, data, uid(), &certidao, &comprovante)?;
    }
    if restaurar_banco {
        restaurar_banco_do_backup(data, backup_dir, manifesto)?;
    } else {
        println!("coordenador: DB atual preservado (F3 não confirmada; compatível com o par legado)");
    }
    restaurar_dropins(backup_dir, manifesto)?;
    // Melhor esforço: adianta o reload dos drop-ins restaurados para a janela entre o
    // `--reverter` e o reload final; falhar aqui não compromete a reversão (o reload
    // posterior continua obrigatório e fatal).
    if let Err(e) = systemctl_ok(&["daemon-reload"]) {
        eprintln!("coordenador: daemon-reload inicial falhou ({e:#}); o reload após --reverter continua obrigatório");
    }
    ensure!(
        sudo(&["--reverter"])?,
        "--reverter recusado; serviço mantido parado, recuperação administrativa necessária"
    );
    systemctl_ok(&["daemon-reload"])?;
    println!("coordenador: DB, drop-ins e par anterior restaurados");
    Ok(())
}

fn auxiliares_do_banco(data: &Path) -> [PathBuf; 2] {
    ["-wal", "-shm"].map(|sufixo| {
        let mut nome = data.join("codync.db").into_os_string();
        nome.push(sufixo);
        PathBuf::from(nome)
    })
}

/// Guarda o estado atual do DB numa cópia privada nova do backup, sem nunca sobrescrever
/// uma cópia posterior de tentativa anterior (`posterior.db`, `posterior-2.db`, …). O
/// `VACUUM INTO` vai para um nome temporário e só então ganha o nome final por vínculo
/// exclusivo, então uma queda no meio não deixa nome final parcial nem bloqueia a retomada.
fn preservar_posterior(data: &Path, backup_dir: &Path) -> Result<PathBuf> {
    for entrada in fs::read_dir(backup_dir)? {
        let entrada = entrada?;
        if entrada.file_name().to_string_lossy().starts_with(".posterior-") {
            fs::remove_file(entrada.path())?;
        }
    }
    let temporario = backup_dir.join(format!(".posterior-{}.db", uuid::Uuid::new_v4().simple()));
    Connection::open(data.join("codync.db"))?
        .execute("VACUUM INTO ?1", [temporario.to_str().context("caminho não UTF-8")?])?;
    File::open(&temporario)?.sync_all()?;
    let mut indice = 1u32;
    let destino = loop {
        let nome = if indice == 1 { "posterior.db".to_owned() } else { format!("posterior-{indice}.db") };
        match fs::hard_link(&temporario, backup_dir.join(&nome)) {
            Ok(()) => break backup_dir.join(nome),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && indice < 1000 => indice += 1,
            Err(e) => return Err(anyhow::Error::from(e).context("cópia posterior do DB")),
        }
    };
    fs::remove_file(&temporario)?;
    File::open(backup_dir)?.sync_all()?;
    Ok(destino)
}

/// Restaura o DB do backup. Retomável: se o DB já é o do backup (queda depois do rename),
/// nada é refeito; senão o estado atual vai antes para uma cópia posterior nova.
fn restaurar_banco_do_backup(data: &Path, backup_dir: &Path, manifesto: &Manifesto) -> Result<()> {
    let banco = backup_dir.join("codync.db");
    let ja_restaurado = sha_arquivo(&data.join("codync.db")).ok().as_ref() == Some(&manifesto.banco_sha256)
        && auxiliares_do_banco(data).iter().all(|p| fs::symlink_metadata(p).is_err());
    if ja_restaurado {
        println!("coordenador: DB já restaurado do backup (retomada da volta)");
        return Ok(());
    }
    // Estado posterior em cópia privada separada; não entra no banco restaurado.
    let posterior = preservar_posterior(data, backup_dir)?;
    println!("coordenador: estado posterior do DB preservado em {}", posterior.display());
    for auxiliar in auxiliares_do_banco(data) {
        match fs::remove_file(auxiliar) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    let temporario = data.join(".codync.db.restaurando");
    let _ = fs::remove_file(&temporario);
    copiar_privado(&banco, &temporario)?;
    fs::rename(&temporario, data.join("codync.db"))?;
    File::open(data)?.sync_all()?;
    ensure!(sha_arquivo(&data.join("codync.db"))? == manifesto.banco_sha256, "DB restaurado diverge do backup");
    Ok(())
}

/// Marca durável de uma etapa da coordenação no diretório do backup.
fn marcar(backup_dir: &Path, marca: &str) -> Result<()> {
    File::create(backup_dir.join(marca))?.sync_all()?;
    File::open(backup_dir)?.sync_all()?;
    Ok(())
}

/// Presença de um caminho: só a ausência comprovada (ENOENT) é `false`; falha de acesso
/// ou de I/O é erro (fail-closed), nunca "ausente".
fn existe(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(anyhow::Error::from(e).context(format!("consultando {}", path.display()))),
    }
}

/// Coordenação interrompida em `backup_dir`, decidida só pelas marcas: comprovante da
/// transação pendente e (a) sem `apontado` — queda entre a publicação e o apontamento da
/// unidade — ou (b) com `voltando` — queda no meio da volta.
fn interrompida_em(dir: &Path) -> Result<bool> {
    Ok(existe(&dir.join("comprovante.json"))? && (!existe(&dir.join("apontado"))? || existe(&dir.join("voltando"))?))
}

/// Status público em que uma coordenação interrompida ainda é retomável pelo wrapper:
/// janela pendente, ou reversão iniciada e não concluída (queda dentro do `--reverter`)
/// ainda dentro do prazo. `bloqueado` (inclusive o fim normal de uma primeira migração
/// revertida), `liberado` e a reversão vencida não são retomáveis automaticamente:
/// vencida, vale a recuperação administrativa.
fn retomavel(status: &crate::fork::status_atualizacao::Status, agora: i64) -> bool {
    use crate::fork::status_atualizacao::Status;
    match status {
        Status::Pendente { .. } => true,
        Status::Revertendo { prazo, .. } => agora < *prazo,
        Status::Bloqueado | Status::Liberado { .. } => false,
    }
}

fn agora_unix() -> Result<i64> {
    let instante =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).context("relógio anterior à época")?;
    i64::try_from(instante.as_secs()).context("relógio inválido")
}

/// Coordenação interrompida: janela retomável (pendente, ou reversão interrompida no
/// prazo) cuja transação tem comprovante sem a marca `apontado`, ou com a volta iniciada
/// (`voltando`). Só a ausência comprovada dispensa a conferência (nenhum backup, nenhuma
/// publicação); erro de acesso, integridade ou I/O propaga (fail-closed).
fn coordenacao_interrompida(raiz_backups: &Path) -> Result<Option<(PathBuf, Manifesto)>> {
    use crate::fork::publicacao_atualizacao::{Nome, ler_opcional};
    match fs::symlink_metadata(raiz_backups) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(anyhow::Error::from(e).context("diretório de backups da atualização inacessível")),
        Ok(meta) => ensure!(meta.is_dir(), "diretório de backups da atualização inválido"),
    }
    let Some(status) = ler_opcional(Nome::Status)? else { return Ok(None) };
    let status: crate::fork::status_atualizacao::Status =
        serde_json::from_slice(&status).context("status público da atualização inválido")?;
    if !retomavel(&status, agora_unix()?) {
        return Ok(None);
    }
    let transacao = super::backup_atualizacao::ler_certidao(uid())?.transacao();
    for entrada in fs::read_dir(raiz_backups)? {
        let entrada = entrada?;
        // Arquivos do diretório privado (identidade da área de staging, marca da instalação
        // limpa) não são backups.
        if !entrada.file_type()?.is_dir() {
            continue;
        }
        let dir = entrada.path();
        let bytes = match fs::read(dir.join("comprovante.json")) {
            Ok(bytes) => bytes,
            // Backup de tentativa que o wrapper não publicou: sem comprovante.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(anyhow::Error::from(e).context(format!("lendo comprovante em {}", dir.display()))),
        };
        let comprovante: super::backup_atualizacao::Comprovante =
            serde_json::from_slice(&bytes).with_context(|| format!("comprovante inválido em {}", dir.display()))?;
        if comprovante.transacao == transacao && interrompida_em(&dir)? {
            let manifesto: Manifesto = serde_json::from_slice(&fs::read(dir.join("manifesto.json"))?)
                .with_context(|| format!("manifesto inválido em {}", dir.display()))?;
            return Ok(Some((dir, manifesto)));
        }
    }
    Ok(None)
}

/// Unidade transitória que conclui uma coordenação interrompida fora do cgroup do Hub.
const UNIDADE_RETOMADA: &str = "donnabot-retomar-atualizacao";

/// Gate de início do Hub (legado ou gerido), antes de abrir o DB: com uma coordenação
/// interrompida, nenhum par processa nem grava, porque a retomada pode restaurar o DB do
/// backup e o que fosse gravado sairia do banco ativo. O Hub agenda a retomada numa
/// unidade transitória (que para este serviço, conclui pela volta e o reinicia no par
/// anterior) e recusa iniciar. Estado ilegível também recusa (fail-closed).
pub(crate) fn exigir_sem_coordenacao_interrompida(data: &Path) -> Result<()> {
    let raiz = data.join("atualizacao-privilegiada");
    let interrompida =
        coordenacao_interrompida(&raiz).context("estado da coordenação da atualização ilegível; Hub não inicia")?;
    let Some((dir, _)) = interrompida else { return Ok(()) };
    agendar_retomada(data);
    bail!(
        "coordenação da atualização interrompida ({}): o Hub só inicia depois da retomada, agendada em {UNIDADE_RETOMADA}",
        dir.display()
    )
}

fn agendar_retomada(data: &Path) {
    let Ok(exe) = std::env::current_exe() else {
        tracing::error!("retomada da atualização não agendada: executável atual indisponível");
        return;
    };
    let uid = uid();
    let mut comando = Command::new("/usr/bin/systemd-run");
    comando
        .args(["--user", "--collect", "--no-block", "--quiet"])
        .arg(format!("--unit={UNIDADE_RETOMADA}"))
        .arg(format!("--setenv=MEXERICA_HOME={}", data.display()))
        .arg("--")
        .arg(exe)
        .arg("coordenar-atualizacao")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("XDG_RUNTIME_DIR", format!("/run/user/{uid}"))
        .env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path=/run/user/{uid}/bus"))
        .stdin(Stdio::null());
    match comando.output() {
        Ok(saida) if saida.status.success() => tracing::warn!("coordenação interrompida: retomada agendada"),
        // Já agendada por uma partida anterior (mesmo nome) ou gerenciador indisponível.
        Ok(saida) => tracing::warn!(
            "retomada da atualização não agendada agora: {}",
            String::from_utf8_lossy(&saida.stderr).trim()
        ),
        Err(erro) => tracing::error!("retomada da atualização não agendada: {erro}"),
    }
}

/// Conclui pela volta uma coordenação interrompida. Na primeira migração, o DB só é
/// restaurado se a F3 chegou a ser confirmada; senão o banco atual (o do backup, pois o
/// gate de início impede qualquer par de gravar antes desta retomada) é preservado.
fn retomar_interrompida(data: &Path, backup_dir: &Path, manifesto: &Manifesto, trava: &File) -> Result<()> {
    let primeira = super::backup_atualizacao::ler_certidao(uid())?.transacao().sequencia_anterior.is_none();
    let f3_confirmada: bool = Connection::open_with_flags(
        data.join("codync.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?
    .query_row("SELECT EXISTS(SELECT 1 FROM kv WHERE k='d330.raizes.v1')", [], |r| r.get(0))?;
    voltar_com(data, backup_dir, manifesto, trava, !primeira || f3_confirmada)?;
    marcar(backup_dir, "retomada")
}

/// Atualização preparatória comprovada (TEC/SEG P2, gate legado). O gate de partida que
/// impede qualquer par de gravar antes da retomada só protege se o binário da unidade o
/// tiver, e a F3 confirma o DB antes de a unidade ser apontada (e a volta devolve a
/// unidade ao binário legado antes do `--reverter`). Por isso, fora do layout gerido, a
/// unidade precisa executar exatamente este binário (mesmo SHA-256), que contém o gate;
/// um Hub legado anterior a ele é recusado antes de qualquer efeito, pedindo antes a
/// atualização do próprio Hub legado. No layout gerido, todo par publicado já tem o gate.
fn exigir_unidade_com_gate(executavel: &str, proprio: &Path) -> Result<()> {
    if executavel == ATIVO {
        return Ok(());
    }
    let unidade = sha_arquivo(Path::new(executavel)).context("binário do Hub da unidade ilegível; nada alterado")?;
    ensure!(
        unidade == sha_arquivo(proprio)?,
        "a unidade executa {executavel}, que não é este binário (pode ser um Hub anterior ao gate de partida): \
         atualize antes o Hub legado para esta versão e rode o coordenador por ele; nada foi alterado"
    );
    Ok(())
}

/// Evidência pública usada para não religar o serviço após uma publicação parcial.
#[derive(PartialEq, Eq)]
struct PublicacaoObservada {
    certidao: Option<Vec<u8>>,
    status: Option<Vec<u8>>,
    seletor: Option<PathBuf>,
}

fn observar_publicacao() -> Result<PublicacaoObservada> {
    use super::publicacao_atualizacao::{Nome, ler_opcional};
    let seletor = match fs::read_link(Path::new(ATIVO).parent().context("seletor sem caminho")?) {
        Ok(alvo) => Some(alvo),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).context("lendo seletor da publicação"),
    };
    Ok(PublicacaoObservada { certidao: ler_opcional(Nome::Certidao)?, status: ler_opcional(Nome::Status)?, seletor })
}

pub(crate) fn executar(opcoes: Opcoes) -> Result<()> {
    ensure!(uid() != 0, "o coordenador executa como o usuário do Hub, nunca root");
    ensure!(
        opcoes.prova || !(opcoes.sem_aceite || opcoes.forcar_reversao),
        "--sem-aceite e --forcar-reversao existem só para prova em VM descartável e exigem --prova"
    );
    exigir_protocolo_wrapper()?;
    let data = crate::service::data_dir();
    ensure!(dir_unidade().join(UNIDADE).is_file(), "unidade {UNIDADE} do usuário ausente");
    let executavel = exec_start()?.first().cloned().context("unidade sem executável")?;
    exigir_unidade_com_gate(&executavel, Path::new("/proc/self/exe"))?;
    let publicacao_antes = observar_publicacao()?;
    let estava_ativo = ativo()?;
    parar()?;
    let trava = crate::service::lock_host_at(&data.join("host.lock"))?;
    let raiz_backups_existente = data.join("atualizacao-privilegiada");
    if let Some((dir, manifesto)) = coordenacao_interrompida(&raiz_backups_existente)? {
        println!("coordenador: coordenação anterior interrompida antes do apontamento; concluindo pela volta");
        retomar_interrompida(&data, &dir, &manifesto, &trava)?;
        drop(trava);
        iniciar()?;
        ensure!(
            saudavel_legado_ou_anterior(opcoes.saude_segundos)?,
            "serviço anterior não ficou saudável após a volta"
        );
        bail!(
            "atualização interrompida foi revertida e o serviço anterior restabelecido; execute novamente para atualizar"
        );
    }
    let id = format!("{}-{}", chrono::Utc::now().format("%Y%m%dT%H%M%SZ"), uuid::Uuid::new_v4().simple());
    let raiz_backups = data.join("atualizacao-privilegiada");
    diretorio_privado(&raiz_backups)?;
    let backup_dir = raiz_backups.join(id);
    let manifesto = match backup(&data, &backup_dir) {
        Ok(m) => m,
        Err(erro) => {
            drop(trava);
            if estava_ativo {
                iniciar()?;
            }
            return Err(erro.context("backup ou gate F3 recusado; serviço restabelecido sem troca"));
        }
    };
    println!("coordenador: chamando o wrapper (sudo -n, sem argumentos)");
    let publicado = sudo(&[])?;
    if !publicado {
        // Certidão sozinha pode estar antiga após falha entre journal, status e certidão.
        ensure!(
            observar_publicacao()? == publicacao_antes,
            "wrapper falhou com publicação alterada; serviço mantido parado, recuperação necessária"
        );
        drop(trava);
        if estava_ativo {
            iniciar()?;
        }
        bail!("wrapper recusou a atualização; estado anterior mantido e serviço restabelecido");
    }
    let certidao = super::backup_atualizacao::ler_certidao(uid())?;
    let comprovante = super::backup_atualizacao::Comprovante {
        transacao: certidao.transacao(),
        banco_sha256: manifesto.banco_sha256.clone(),
        schema_sha256: manifesto.schema_sha256.clone(),
    };
    let mut arquivo =
        OpenOptions::new().write(true).create_new(true).mode(0o600).open(backup_dir.join("comprovante.json"))?;
    arquivo.write_all(&serde_json::to_vec(&comprovante)?)?;
    arquivo.sync_all()?;
    // Comprovante durável antes da F3: é ele que permite retomar uma coordenação interrompida.
    File::open(&backup_dir)?.sync_all()?;
    // Passo 6: raízes F3 copiadas e conferidas antes de reescrever referências.
    let f3 = migrar_raizes(&data, &trava);
    let mut trava = Some(trava);
    let resultado =
        f3.and_then(|()| apontar_para_gerido()).and_then(|()| marcar(&backup_dir, "apontado")).and_then(|()| {
            let argv = exec_start()?;
            ensure!(argv.first().map(String::as_str) == Some(ATIVO), "unidade não executa o seletor gerido");
            let porta = porta_exec_start(&argv)?;
            let release = release_esperada(&certidao.transacao().id)?;
            exigir_porta_livre(porta)?;
            conferir_binario_novo(&release)?;
            // A trava só é liberada para o Hub novo poder obtê-la ao iniciar.
            trava = None;
            iniciar()?;
            if !saudavel(opcoes.saude_segundos, true)? {
                return Ok(false);
            }
            conferir_hello(porta, &release.versao)?;
            registrar_verificacao(&release, porta)?;
            Ok(true)
        });
    let saudavel = matches!(resultado, Ok(true));
    if saudavel && !opcoes.forcar_reversao {
        if opcoes.sem_aceite {
            println!("coordenador: par novo ativo; janela pendente sem aceite");
            return Ok(());
        }
        ensure!(sudo(&["--aceitar"])?, "aceite recusado");
        println!("coordenador: atualização aceita");
        return Ok(());
    }
    println!("coordenador: executando a volta ({})", if saudavel { "forçada" } else { "saúde reprovada" });
    parar()?;
    // Volta iniciada: uma queda daqui até o `--reverter` é retomada (e o Hub não inicia antes).
    marcar(&backup_dir, "voltando")?;
    let trava = match trava {
        Some(trava) => trava,
        None => crate::service::lock_host_at(&data.join("host.lock"))?,
    };
    voltar(&data, &backup_dir, &manifesto, &trava)?;
    drop(trava);
    iniciar()?;
    ensure!(saudavel_legado_ou_anterior(opcoes.saude_segundos)?, "serviço anterior não ficou saudável após a volta");
    if opcoes.forcar_reversao && saudavel {
        println!("coordenador: volta forçada concluída; serviço anterior ativo");
        return Ok(());
    }
    bail!("atualização revertida: {}", resultado.err().map_or("saúde reprovada".into(), |e| format!("{e:#}")))
}

fn saudavel_legado_ou_anterior(segundos: u64) -> Result<bool> {
    let gerido = exec_start()?.first().map(String::as_str) == Some(ATIVO);
    saudavel(segundos, gerido)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arvore_com_executavel() -> Result<PathBuf> {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("mx365-trab-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&dir)?;
        fs::create_dir(dir.join("ferramenta"))?;
        fs::write(dir.join("ferramenta/run.sh"), b"#!/bin/sh\necho oi\n")?;
        fs::set_permissions(dir.join("ferramenta/run.sh"), fs::Permissions::from_mode(0o755))?;
        fs::write(dir.join("SKILL.md"), b"# skill\n")?;
        Ok(dir)
    }

    #[test]
    fn executavel_em_skills_nao_diverge_e_workspace_preserva_execucao() -> Result<()> {
        let origem = arvore_com_executavel()?;
        let destino = std::env::temp_dir().join(format!("mx365-dest-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&destino)?;
        let (raiz, pai) = (File::open(&origem)?, File::open(&destino)?);
        // SAFETY: consulta sem efeitos colaterais.
        let gid = unsafe { libc::getgid() };
        for (tipo, preserva, modo_script) in
            [("skills", false, 0o640), ("uploads", false, 0o640), ("workspace", true, 0o770)]
        {
            copiar_arvore(&raiz, &pai, tipo, &[], tipo, gid)?;
            let copia = File::open(destino.join(tipo))?;
            assert_eq!(manifesto_local(&raiz, &[], preserva)?, manifesto_local(&copia, &[], preserva)?, "{tipo}");
            conferir_modos(&copia, tipo, gid)?;
            let modo = fs::metadata(destino.join(tipo).join("ferramenta/run.sh"))?.mode() & 0o7777;
            assert_eq!(modo, modo_script, "{tipo}");
            assert_eq!(fs::read(destino.join(tipo).join("ferramenta/run.sh"))?, b"#!/bin/sh\necho oi\n");
        }
        // Execução indevida em skills é recusada na conferência de modos.
        fs::set_permissions(
            destino.join("skills/SKILL.md"),
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o750),
        )?;
        assert!(conferir_modos(&File::open(destino.join("skills"))?, "skills", gid).is_err());
        // Hard link na origem é recusado.
        fs::hard_link(origem.join("SKILL.md"), origem.join("vinculo"))?;
        assert!(manifesto_local(&raiz, &[], false).is_err());
        fs::remove_dir_all(origem)?;
        fs::remove_dir_all(destino)?;
        Ok(())
    }

    fn pasta(nome: &str) -> Result<PathBuf> {
        let dir = std::env::temp_dir().join(format!("mx365-{nome}-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&dir)?;
        Ok(dir)
    }

    /// SEG (residual de staging): só o conteúdo da área privada registrada é coletado.
    /// Uma pasta do Hub com dados posta no lugar da área (o motor pode renomear entradas
    /// em `trabalho/`) é recusada sem perder nada, nos modos 2750 e 0700.
    #[test]
    fn area_de_staging_coleta_so_a_propria_e_preserva_substituta() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let (trabalho, privado) = (pasta("trabalho")?, pasta("backups")?);
        let (dir_trabalho, dir_privado) = (File::open(&trabalho)?, File::open(&privado)?);
        let area = trabalho.join(AREA_STAGING);
        drop(area_staging(&dir_trabalho, &dir_privado)?);
        assert_eq!(fs::metadata(&area)?.mode() & 0o7777, 0o700);
        // Staging de tentativa interrompida dentro da área: coletado na chamada seguinte.
        fs::create_dir_all(area.join(".copia-0123/sub"))?;
        fs::write(area.join(".copia-0123/sub/x"), b"lixo")?;
        drop(area_staging(&dir_trabalho, &dir_privado)?);
        assert!(fs::read_dir(&area)?.next().is_none(), "staging órfão não coletado");
        for modo in [0o2750, 0o700] {
            // Área original com um staging; uma pasta alheia do Hub, com dados, entra no lugar.
            fs::create_dir_all(area.join(".copia-4567"))?;
            fs::rename(&area, trabalho.join("area-movida"))?;
            fs::create_dir_all(area.join(".copia-4567"))?;
            fs::write(area.join(".copia-4567/sentinela"), b"dados do Hub")?;
            fs::set_permissions(&area, fs::Permissions::from_mode(modo))?;
            let erro = area_staging(&dir_trabalho, &dir_privado).err().map(|e| e.to_string()).unwrap_or_default();
            assert!(erro.contains("substituída"), "{erro}");
            assert_eq!(fs::read(area.join(".copia-4567/sentinela"))?, b"dados do Hub", "substituta apagada");
            fs::remove_dir_all(&area)?;
            fs::rename(trabalho.join("area-movida"), &area)?;
            drop(area_staging(&dir_trabalho, &dir_privado)?);
        }
        // Área vazia no lugar (ex.: a original removida pelo grupo): readotada sem risco.
        fs::remove_dir(&area)?;
        fs::create_dir(&area)?;
        drop(area_staging(&dir_trabalho, &dir_privado)?);
        fs::create_dir(area.join(".copia-89ab"))?;
        drop(area_staging(&dir_trabalho, &dir_privado)?);
        assert!(fs::read_dir(&area)?.next().is_none());
        fs::remove_dir_all(trabalho)?;
        fs::remove_dir_all(privado)?;
        Ok(())
    }

    fn banco_com(caminho: &Path, valor: &str) -> Result<()> {
        let db = Connection::open(caminho)?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS t(v TEXT); DELETE FROM t;")?;
        db.execute("INSERT INTO t VALUES(?1)", [valor])?;
        Ok(())
    }

    fn valor(caminho: &Path) -> Result<String> {
        Ok(Connection::open(caminho)?.query_row("SELECT v FROM t", [], |r| r.get(0))?)
    }

    /// TEC: a volta é retomável depois de uma segunda queda. Um `posterior.db` de tentativa
    /// anterior nunca é sobrescrito nem impede a restauração, e repetir não refaz nada.
    #[test]
    fn restauracao_do_banco_retoma_sem_sobrescrever_posterior() -> Result<()> {
        let (data, backup_dir) = (pasta("data")?, pasta("backup")?);
        banco_com(&backup_dir.join("codync.db"), "backup")?;
        banco_com(&data.join("codync.db"), "posterior")?;
        // Queda depois de criar o posterior da tentativa anterior e antes do rename.
        fs::write(backup_dir.join("posterior.db"), b"posterior da tentativa anterior")?;
        fs::write(backup_dir.join(".posterior-morto.db"), b"temporario")?;
        let manifesto = Manifesto {
            banco_sha256: sha_arquivo(&backup_dir.join("codync.db"))?,
            schema_sha256: String::new(),
            raizes_f3: 0,
            unidade_sha256: String::new(),
            dropins_sha256: BTreeMap::new(),
            habilitado: true,
        };
        restaurar_banco_do_backup(&data, &backup_dir, &manifesto)?;
        assert_eq!(valor(&data.join("codync.db"))?, "backup");
        assert_eq!(fs::read(backup_dir.join("posterior.db"))?, b"posterior da tentativa anterior");
        assert_eq!(valor(&backup_dir.join("posterior-2.db"))?, "posterior");
        assert!(!backup_dir.join(".posterior-morto.db").exists());
        // Nova queda depois do rename: a retomada reconhece o DB restaurado e não duplica.
        restaurar_banco_do_backup(&data, &backup_dir, &manifesto)?;
        assert!(!backup_dir.join("posterior-3.db").exists());
        assert_eq!(valor(&data.join("codync.db"))?, "backup");
        fs::remove_dir_all(data)?;
        fs::remove_dir_all(backup_dir)?;
        Ok(())
    }

    /// TEC: as marcas decidem a retomada (e o gate de início do Hub).
    #[test]
    fn marcas_definem_coordenacao_interrompida() -> Result<()> {
        let dir = pasta("marcas")?;
        assert!(!interrompida_em(&dir)?, "sem comprovante não há coordenação");
        fs::write(dir.join("comprovante.json"), b"{}")?;
        assert!(interrompida_em(&dir)?, "publicada e não apontada: queda antes do apontamento");
        marcar(&dir, "apontado")?;
        assert!(!interrompida_em(&dir)?, "apontada: segue aceite ou volta normal");
        marcar(&dir, "voltando")?;
        assert!(interrompida_em(&dir)?, "volta iniciada e não concluída");
        fs::remove_dir_all(dir)?;
        Ok(())
    }

    /// TEC P2-3: uma segunda queda dentro do `--reverter` (status `revertendo`) é retomável
    /// até o prazo, ao contrário do bloqueio definitivo, do par liberado e da reversão vencida.
    #[test]
    fn reversao_interrompida_e_retomavel_e_bloqueio_nao() -> Result<()> {
        use crate::fork::status_atualizacao::Status;
        let ler = |json: &str| -> Result<Status> { Ok(serde_json::from_str(json)?) };
        let pendente = ler(r#"{"fase":"pendente","slot":"slot-a","sequencia":2,"inicio":100,"prazo":200}"#)?;
        let revertendo = ler(r#"{"fase":"revertendo","slot":"slot-b","sequencia":2,"inicio":100,"prazo":200}"#)?;
        assert!(retomavel(&pendente, 150));
        assert!(retomavel(&revertendo, 150), "queda dentro do --reverter não agenda a retomada");
        assert!(!retomavel(&revertendo, 200), "reversão vencida é recuperação administrativa");
        assert!(!retomavel(&ler(r#"{"fase":"bloqueado"}"#)?, 150), "bloqueio definitivo retomado");
        assert!(!retomavel(&ler(r#"{"fase":"liberado","slot":"slot-a","sequencia":1}"#)?, 150));
        // Status adulterado ou desconhecido não vira "nenhuma coordenação": é erro.
        assert!(ler(r#"{"fase":"revertendo"}"#).is_err());
        assert!(ler(r#"{"fase":"outra"}"#).is_err());
        Ok(())
    }

    /// SEG P3: só a ausência comprovada dispensa a conferência; falha de acesso propaga.
    #[test]
    fn ausencia_comprovada_dispensa_e_falha_de_acesso_propaga() -> Result<()> {
        let raiz = pasta("existe")?;
        assert!(!existe(&raiz.join("nada"))?);
        assert!(coordenacao_interrompida(&raiz.join("nada"))?.is_none(), "sem backups não há coordenação");
        fs::write(raiz.join("arquivo"), b"x")?;
        // ENOTDIR (componente que não é pasta) não é ausência.
        assert!(existe(&raiz.join("arquivo/comprovante.json")).is_err(), "ENOTDIR tratado como ausência");
        assert!(coordenacao_interrompida(&raiz.join("arquivo")).is_err(), "backups não-pasta aceitos");
        fs::remove_dir_all(raiz)?;
        Ok(())
    }

    /// SEG Other (#833): a unidade gerida da migração escuta só em loopback, mesmo que a
    /// legada escutasse em todas as interfaces ou não tivesse `--bind`.
    #[test]
    fn unidade_gerida_da_migracao_fixa_loopback() -> Result<()> {
        let argv = |texto: &str| texto.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
        let esperado = argv(&format!("{ATIVO} serve --port 7777 --bind 127.0.0.1"));
        for legado in [
            "/x/donnabot-host serve --port 7777",
            "/x/donnabot-host serve --port 7777 --bind 0.0.0.0",
            "/x/donnabot-host serve --bind=0.0.0.0 --port 7777",
            "/x/donnabot-host serve --bind 192.0.2.1 --port 7777 --bind 0.0.0.0",
        ] {
            let gerido = argv_gerido(&argv(legado))?;
            let mut ordenado = gerido.clone();
            ordenado.sort();
            let mut alvo = esperado.clone();
            alvo.sort();
            assert_eq!(ordenado, alvo, "{legado}");
            assert_eq!(gerido.first().map(String::as_str), Some(ATIVO));
            assert_eq!(gerido.iter().filter(|a| a.starts_with("--bind")).count(), 1, "{legado}");
            assert_eq!(gerido[gerido.len() - 2..], ["--bind".to_owned(), BIND_GERIDO.to_owned()], "{legado}");
            assert!(!gerido.iter().any(|a| a.contains("0.0.0.0")), "{legado}");
            // Já gerida em loopback: nada a reescrever.
            assert_eq!(argv_gerido(&gerido)?, gerido);
        }
        // Sem subcomando, o Hub executa `serve` (padrão 0.0.0.0): a gerida fixa loopback.
        assert_eq!(argv_gerido(&argv("/x/donnabot-host"))?, argv(&format!("{ATIVO} serve --bind 127.0.0.1")));
        assert!(argv_gerido(&argv("/x/donnabot-host mcp")).is_err());
        assert!(argv_gerido(&argv("/x/donnabot-host serve --bind")).is_err());
        Ok(())
    }

    #[test]
    fn falha_na_publicacao_detecta_status_ou_seletor_alterado() {
        let original = PublicacaoObservada {
            certidao: Some(b"antiga".to_vec()),
            status: Some(b"livre".to_vec()),
            seletor: Some(PathBuf::from("slot-a")),
        };
        let status_novo = PublicacaoObservada {
            certidao: original.certidao.clone(),
            status: Some(b"pendente".to_vec()),
            seletor: original.seletor.clone(),
        };
        let seletor_novo = PublicacaoObservada {
            certidao: original.certidao.clone(),
            status: original.status.clone(),
            seletor: Some(PathBuf::from("slot-b")),
        };
        assert!(original != status_novo);
        assert!(original != seletor_novo);
    }

    #[test]
    fn wrapper_antigo_nao_e_confundido_com_protocolo_atual() {
        assert!(resposta_protocolo_confere(true, b"mexerica-verificacao-v1\n"));
        assert!(!resposta_protocolo_confere(false, b"mexerica-verificacao-v1\n"));
        assert!(!resposta_protocolo_confere(true, b""));
        assert!(!resposta_protocolo_confere(true, b"mexerica-verificacao-v1 extra\n"));
    }

    #[test]
    fn identidade_da_release_exige_vinculo_versao_e_hash_validos() -> Result<()> {
        let hash = "a".repeat(64);
        let documento = |id: &str, versao: &str, digest: &str| {
            serde_json::to_vec(&serde_json::json!({"transacao":id,"versao":versao,"hub_sha256":digest}))
        };
        assert!(interpretar_release(&documento("transacao", "1.2.3", &hash)?, "transacao").is_ok());
        assert!(interpretar_release(&documento("outra", "1.2.3", &hash)?, "transacao").is_err());
        assert!(interpretar_release(&documento("transacao", "invalida", &hash)?, "transacao").is_err());
        assert!(interpretar_release(&documento("transacao", "1.2.3", "xyz")?, "transacao").is_err());
        assert!(interpretar_release(br#"{"transacao":"transacao","transacao":"outra"}"#, "transacao").is_err());
        Ok(())
    }

    #[test]
    fn hello_exige_versao_exata() {
        assert!(versao_hello_confere(&serde_json::json!({"version":"1.2.3"}), "1.2.3"));
        assert!(!versao_hello_confere(&serde_json::json!({"version":"1.2.4"}), "1.2.3"));
        assert!(!versao_hello_confere(&serde_json::json!({}), "1.2.3"));
    }

    #[test]
    fn versao_do_binario_deve_coincidir_exatamente() {
        assert!(versao_binario_confere("donnabot-host 1.2.3\n", "1.2.3"));
        assert!(!versao_binario_confere("donnabot-host 1.2.4", "1.2.3"));
        assert!(!versao_binario_confere("outro 1.2.3", "1.2.3"));
        assert!(!versao_binario_confere("donnabot-host 1.2.3 extra", "1.2.3"));
    }

    #[test]
    fn porta_da_unidade_exige_valor_unico_e_valido() -> Result<()> {
        let args = |texto: &str| texto.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(porta_exec_start(&args("/hub serve --port 7777"))?, 7777);
        assert_eq!(porta_exec_start(&args("/hub serve --port=7777"))?, 7777);
        assert_eq!(porta_exec_start(&args("/hub serve"))?, crate::service::DEFAULT_PORT);
        assert!(porta_exec_start(&args("/hub serve --port 7 --port=8")).is_err());
        assert!(porta_exec_start(&args("/hub serve --port 0")).is_err());
        assert!(porta_exec_start(&args("/hub serve --port")).is_err());
        Ok(())
    }

    #[test]
    fn porta_ocupada_em_loopback_e_recusada() -> Result<()> {
        let ocupado = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let porta = ocupado.local_addr()?.port();
        assert!(exigir_porta_livre(porta).is_err(), "listener existente não foi detectado");
        Ok(())
    }

    #[test]
    fn exec_start_exige_caminho_efetivo_igual_ao_argv() -> Result<()> {
        let linha = |path: &str, argv: &str| format!("{{ path={path} ; argv[]={argv} ; ignore_errors=no }}");
        assert_eq!(interpretar_exec_start(&linha(ATIVO, &format!("{ATIVO} serve")))?[0], ATIVO);
        assert!(interpretar_exec_start(&linha("/legado/donnabot-host", &format!("{ATIVO} serve"))).is_err());
        assert!(interpretar_exec_start(&linha("@/legado/donnabot-host", &format!("{ATIVO} serve"))).is_err());
        assert!(recusar_prefixo_arroba(&format!("[Service]\nExecStart=@{ATIVO} {ATIVO} serve\n")).is_err());
        assert!(recusar_prefixo_arroba(&format!("[Service]\nExecStart={ATIVO} serve\n")).is_ok());
        assert!(interpretar_exec_start(&format!("{} {}", linha(ATIVO, ATIVO), linha(ATIVO, ATIVO))).is_err());
        Ok(())
    }

    /// TEC/SEG P2 (gate legado): a coordenação só começa se a unidade executa este binário
    /// (que tem o gate de partida) ou o seletor gerido; um Hub legado diferente é recusado.
    #[test]
    fn coordenacao_exige_unidade_com_o_gate() -> Result<()> {
        let dir = pasta("gate")?;
        let (proprio, igual, anterior) = (dir.join("novo"), dir.join("copia"), dir.join("anterior"));
        fs::write(&proprio, b"binario com gate")?;
        fs::write(&igual, b"binario com gate")?;
        fs::write(&anterior, b"binario anterior ao gate")?;
        let texto = |p: &Path| p.to_string_lossy().into_owned();
        assert!(exigir_unidade_com_gate(&texto(&igual), &proprio).is_ok());
        assert!(exigir_unidade_com_gate(ATIVO, &proprio).is_ok(), "seletor gerido recusado");
        let erro = exigir_unidade_com_gate(&texto(&anterior), &proprio).err().map(|e| format!("{e:#}"));
        assert!(erro.unwrap_or_default().contains("nada foi alterado"));
        assert!(exigir_unidade_com_gate(&texto(&dir.join("ausente")), &proprio).is_err());
        fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn work_gid_unico_do_motor_conf() {
        assert_eq!(work_gid("motor_user=m\nhub_uid=1000\nwork_gid=995\n").ok(), Some(995));
        assert!(work_gid("motor_user=m\nhub_uid=1000\n").is_err());
        assert!(work_gid("work_gid=995\nwork_gid=996\n").is_err());
        assert!(work_gid("work_gid=x\n").is_err());
    }

    /// Credenciais só da thread atual (a syscall direta não propaga às outras threads):
    /// grupos suplementares, GID e UID, nessa ordem.
    fn assumir_identidade(uid: u32, gid: u32, grupos: &[u32]) -> Result<()> {
        // SAFETY: syscalls de credencial da própria thread com argumentos válidos.
        unsafe {
            ensure!(libc::syscall(libc::SYS_setgroups, grupos.len(), grupos.as_ptr()) == 0, "setgroups");
            ensure!(libc::syscall(libc::SYS_setresgid, gid, gid, gid) == 0, "setresgid");
            ensure!(libc::syscall(libc::SYS_setresuid, uid, uid, uid) == 0, "setresuid");
        }
        Ok(())
    }

    fn como<T: Send + 'static>(
        uid: u32,
        gid: u32,
        grupos: Vec<u32>,
        f: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> Result<T> {
        std::thread::spawn(move || {
            assumir_identidade(uid, gid, &grupos)?;
            f()
        })
        .join()
        .map_err(|_| anyhow::anyhow!("thread com outra identidade falhou"))?
    }

    /// TEC P2-1 / SEG P2 (GID): com identidades e grupos distintos (Hub com grupo primário
    /// próprio e membro do grupo de trabalho; motor com outro UID e outro primário, membro
    /// só do grupo de trabalho), a F3 do Hub publica workspace, uploads e skills com o grupo
    /// de trabalho em toda a árvore, mesmo montando a cópia na área 0700 sem setgid; o motor
    /// lê as três e escreve no workspace; uma árvore com o grupo primário do Hub é recusada.
    /// Exige root (só cria identidades numéricas na thread de teste); fora dele, ignorado.
    #[test]
    fn f3_publica_com_grupo_de_trabalho_e_motor_acessa() -> Result<()> {
        use std::os::unix::fs::{PermissionsExt, chown};
        // SAFETY: consulta sem efeitos colaterais.
        if unsafe { libc::geteuid() } != 0 {
            eprintln!("teste de identidades distintas exige root; ignorado");
            return Ok(());
        }
        let (uid_hub, gid_hub, uid_motor, gid_motor, gid_trab) = (61_401, 61_402, 61_403, 61_404, 61_405);
        let base = std::env::temp_dir().join(format!("mx365-gid-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&base)?;
        fs::set_permissions(&base, fs::Permissions::from_mode(0o755))?;
        let trabalho = base.join("trabalho");
        fs::create_dir(&trabalho)?;
        chown(&trabalho, Some(uid_hub), Some(gid_trab))?;
        fs::set_permissions(&trabalho, fs::Permissions::from_mode(0o2770))?;
        for nome in ["privado", "origem"] {
            fs::create_dir(base.join(nome))?;
            chown(base.join(nome), Some(uid_hub), Some(gid_hub))?;
            fs::set_permissions(base.join(nome), fs::Permissions::from_mode(0o700))?;
        }
        let hub = move || vec![gid_trab];
        let b = base.clone();
        let resultado = como(uid_hub, gid_hub, hub(), move || -> Result<()> {
            let origem = b.join("origem");
            for tipo in ["workspace", "uploads", "skills"] {
                fs::create_dir_all(origem.join(tipo).join("sub"))?;
                fs::write(origem.join(tipo).join("sub/dado.txt"), tipo.as_bytes())?;
            }
            let (trabalho, privado) = (File::open(b.join("trabalho"))?, File::open(b.join("privado"))?);
            let area = area_staging(&trabalho, &privado)?;
            assert_eq!(area.metadata()?.mode() & 0o7777, 0o700, "área de staging deixou de ser privada");
            let lugar = Trabalho { raiz: &b.join("trabalho"), area: &area, gid: gid_trab };
            for tipo in ["workspace", "uploads", "skills"] {
                let destino = b.join("trabalho/bots/bot1").join(tipo);
                copiar_trabalho(&origem.join(tipo), &destino, tipo, &[], &lugar)?;
                // Repetição: mesmo conteúdo, dono e grupo aceitos.
                copiar_trabalho(&origem.join(tipo), &destino, tipo, &[], &lugar)?;
            }
            Ok(())
        });
        let conferencia = resultado.and_then(|()| {
            // Toda a árvore publicada: dono Hub, grupo de trabalho (inclusive ancestrais novos).
            let mut pilha = vec![trabalho.join("bots")];
            while let Some(caminho) = pilha.pop() {
                let meta = fs::symlink_metadata(&caminho)?;
                ensure!(
                    meta.uid() == uid_hub && meta.gid() == gid_trab,
                    "{} com {}:{}",
                    caminho.display(),
                    meta.uid(),
                    meta.gid()
                );
                if meta.is_dir() {
                    for entrada in fs::read_dir(&caminho)? {
                        pilha.push(entrada?.path());
                    }
                }
            }
            let t = trabalho.clone();
            como(uid_motor, gid_motor, vec![gid_trab], move || -> Result<()> {
                for tipo in ["workspace", "uploads", "skills"] {
                    let dado = t.join("bots/bot1").join(tipo).join("sub/dado.txt");
                    ensure!(fs::read(&dado)? == tipo.as_bytes(), "motor não leu {tipo}");
                    let novo = t.join("bots/bot1").join(tipo).join("sub/motor.txt");
                    let escrita = fs::write(&novo, b"motor");
                    // Contrato F3: o motor escreve no workspace; uploads e skills são só leitura.
                    ensure!(escrita.is_ok() == (tipo == "workspace"), "escrita do motor em {tipo}: {escrita:?}");
                    if tipo == "workspace" {
                        fs::write(t.join("bots/bot1/workspace/sub/dado.txt"), b"alterado pelo motor")?;
                    }
                }
                Ok(())
            })?;
            // Árvore com o grupo primário do Hub (o defeito do ciclo 3): conferência recusa.
            let skills = trabalho.join("bots/bot1/skills/sub/dado.txt");
            chown(&skills, None, Some(gid_hub))?;
            let t = trabalho.clone();
            let erro = como(uid_hub, gid_hub, vec![gid_trab], move || {
                Ok(conferir_modos(&File::open(t.join("bots/bot1/skills"))?, "skills", gid_trab).err())
            })?;
            ensure!(
                erro.is_some_and(|e| e.to_string().contains("grupo fora do contrato")),
                "grupo primário do Hub aceito na conferência"
            );
            Ok(())
        });
        fs::remove_dir_all(&base)?;
        conferencia
    }

    #[test]
    fn duracoes_do_systemd_e_prazo_minimo() {
        assert_eq!(segundos_systemd("1min 30s"), Some(90));
        assert_eq!(segundos_systemd("5min"), Some(300));
        assert_eq!(segundos_systemd("45s"), Some(45));
        assert_eq!(segundos_systemd("infinity"), Some(3600));
        assert_eq!(segundos_systemd("2x"), None);
    }

    #[test]
    fn opcoes_de_prova_exigem_confirmacao() {
        let opcoes = Opcoes { sem_aceite: true, forcar_reversao: false, saude_segundos: 1, prova: false };
        // Recusa antes de qualquer efeito (UID do teste nunca é root no CI).
        // SAFETY: consulta sem efeitos colaterais.
        if unsafe { libc::getuid() } != 0 {
            let erro = executar(opcoes).err().map(|e| e.to_string()).unwrap_or_default();
            assert!(erro.contains("exigem --prova"), "{erro}");
        }
    }
}
