//! Etapa administrativa: executada por root, nunca pelo UID do Hub nem pelo
//! executável dele. Confere o pacote e a raiz TUF contra fingerprints obtidos
//! por canal independente, registra a baseline antes de alterar o sistema e
//! falha fechada diante de estado misto, ambíguo ou de outra instalação.
//! Instalações com arquivos administrativos de formato anterior são recusadas.
//! Revogue e reinstale pelo fluxo administrativo; não há migração automática.
use super::{
    Hex as _,
    armazenamento::Diretorio,
    baseline::{Baseline, Legado},
    journal::{Evento, Motivo, Operacao, Origem, Resultado},
    modo_motor::{self, CONFIG as MOTOR_CONF},
    politica::Politica,
    sudoers, tuf,
};
use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest as _, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use sudoers::{FORMAS_WRAPPER, WRAPPER};

pub const ESTADO: &str = "/var/lib/donnabot/atualizacao";
const SBIN: &str = "/opt/donnabot/sbin";
const RELEASES: &str = "/opt/donnabot/releases";
const SUDOERS: &str = "/etc/sudoers.d/donnabot";
const MOTOR_USER: &str = "donnabot-motor";
const MOTOR_HOME: &str = "/var/lib/donnabot/motor";
const TRABALHO: &str = "/var/lib/donnabot/trabalho";
const GRUPO_TRABALHO: &str = "donnabot-trabalho";
const CGROUP: &str = "/sys/fs/cgroup/donnabot-atualizacao";
const UNIDADE_GUARDA: &str = "/etc/systemd/system/donnabot-guarda-atualizacao@.service";
const UNIDADE_CGROUP: &str = "/etc/systemd/system/donnabot-cgroup-atualizacao.service";
const DROPIN_HUB: &str = "/etc/systemd/user/donnabot-host.service.d/donnabot-guarda.conf";
const SOCKET_COPIA: &str = "/etc/systemd/system/donnabot-motor-copia.socket";
const SOCKET_RUN: &str = "/run/donnabot-motor-copia.sock";
const SERVICO_COPIA: &str = "/etc/systemd/system/donnabot-motor-copia.service";
/// Hub gerido: binário do par ativo e status público, ambos publicados pelo wrapper.
const HUB_ATIVO: &str = "/opt/donnabot/releases/ativo/donnabot-host";
const STATUS_PUBLICO: &str = "/opt/donnabot/releases/status.json";
const CERTIDAO_PUBLICA: &str = "/opt/donnabot/releases/transacao.json";
/// Canal do motor: executa como `donnabot-motor`, só escreve na home do motor.
const TEXTO_SERVICO_COPIA: &str = "[Unit]\nDescription=Canal de cópia executado como o usuário motor\nRequires=donnabot-motor-copia.socket\n\n[Service]\nType=simple\nUser=donnabot-motor\nGroup=donnabot-motor\nUMask=0077\nExecStart=/opt/donnabot/sbin/donnabot-motor-copia\nNoNewPrivileges=yes\nProtectSystem=strict\nReadWritePaths=/var/lib/donnabot/motor\nProtectHome=yes\nPrivateTmp=yes\nPrivateDevices=yes\nCapabilityBoundingSet=\nRestrictAddressFamilies=AF_UNIX\nSystemCallArchitectures=native\n";

/// Socket acessível só ao motor (dono) e ao grupo primário do Hub; o serviço ainda
/// confere o UID do par pelo kernel.
fn texto_socket_copia(grupo_hub: &str) -> String {
    format!(
        "[Unit]\nDescription=Socket do canal de cópia do motor\n\n[Socket]\nListenStream=/run/donnabot-motor-copia.sock\nSocketUser=donnabot-motor\nSocketGroup={grupo_hub}\nSocketMode=0660\nRemoveOnStop=yes\nAccept=no\n\n[Install]\nWantedBy=sockets.target\n"
    )
}
/// Binários do pacote, instalados root:root 0755 em /opt/donnabot/sbin.
const BINARIOS: [(&str, &str); 4] = [
    ("atualizar-privilegiado", "atualizar-privilegiado"),
    ("donnabot-guarda-atualizacao", "donnabot-guarda-atualizacao"),
    ("donnabot-instalar-atualizacao", "donnabot-instalar-atualizacao"),
    ("donnabot-motor-copia", "donnabot-motor-copia"),
];
const ARQUIVOS_PACOTE: [&str; 7] = [
    "atualizar-privilegiado",
    "donnabot-guarda-atualizacao",
    "donnabot-instalar-atualizacao",
    "donnabot-motor-copia",
    "raiz.json",
    "politica.json",
    "SHA256SUMS",
];
const LIMITE_ARQUIVO: u64 = 512 * 1024 * 1024;

/// Guard root por UID do Hub; `ExecStopPost` bloqueia se o guard parar fora do desligamento.
const TEXTO_UNIDADE_GUARDA: &str = "[Unit]\nDescription=Guarda de expiração da atualização privilegiada do Hub UID %i\nAfter=local-fs.target\nBefore=user@%i.service\n\n[Service]\nType=simple\nUser=root\nGroup=root\nUMask=0077\nExecStart=/opt/donnabot/sbin/donnabot-guarda-atualizacao\nExecStopPost=/opt/donnabot/sbin/donnabot-guarda-atualizacao --bloquear\nRestart=on-failure\nRestartSec=1s\nTimeoutStopSec=30s\n\n[Install]\nWantedBy=multi-user.target\n";
/// Drop-in global do `systemd --user`: a parada alcança o Hub e todos os filhos.
const TEXTO_DROPIN_HUB: &str =
    "# Instalado pela etapa administrativa da atualização privilegiada.\n[Service]\nKillMode=control-group\n";
const TEXTO_UNIDADE_CGROUP: &str = "[Unit]\nDescription=Prepara o cgroup root-owned da atualização privilegiada\nDefaultDependencies=no\nAfter=local-fs.target\nBefore=donnabot-guarda-atualizacao@.service\n\n[Service]\nType=oneshot\nRemainAfterExit=yes\nUMask=0077\nExecStart=/opt/donnabot/sbin/donnabot-instalar-atualizacao preparar-cgroup\n\n[Install]\nWantedBy=multi-user.target\n";

/// Identidade local inequívoca do Hub (somente base `files`, sem NSS remoto).
pub struct Hub {
    pub login: String,
    pub uid: u32,
    pub gid: u32,
    pub home: PathBuf,
}

fn agora() -> Result<i64> {
    Ok(i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs())?)
}

fn exigir_root_direto(hub_uid: Option<u32>) -> Result<()> {
    // SAFETY: consultas de identidade sem efeitos colaterais.
    let (real, efetivo) = unsafe { (libc::getuid(), libc::geteuid()) };
    ensure!(real == 0 && efetivo == 0, "a etapa administrativa exige root");
    if let (Some(uid), Some(sudo)) = (hub_uid, std::env::var_os("SUDO_UID")) {
        ensure!(sudo.to_str() != Some(&uid.to_string()), "o UID do Hub não pode executar a etapa administrativa");
    }
    Ok(())
}

/// Lê /etc/passwd diretamente: exige exatamente uma entrada para o login e para o UID.
pub fn hub(login: &str) -> Result<Hub> {
    sudoers::fragmento(login, 1)?; // mesma gramática de login aceita na regra
    let texto = ler_limitado(Path::new("/etc/passwd"), 16 * 1024 * 1024)?;
    let texto = String::from_utf8(texto).context("/etc/passwd inválido")?;
    let linhas: Vec<Vec<&str>> =
        texto.lines().filter(|l| !l.starts_with('#')).map(|l| l.split(':').collect()).collect();
    let nossas: Vec<&Vec<&str>> = linhas.iter().filter(|c| c.first() == Some(&login)).collect();
    ensure!(nossas.len() == 1 && nossas[0].len() == 7, "login ausente ou ambíguo em /etc/passwd");
    let uid: u32 = nossas[0][2].parse().context("UID inválido")?;
    let gid: u32 = nossas[0][3].parse().context("GID inválido")?;
    ensure!(uid != 0 && gid != 0, "o Hub não pode ser root nem usar grupo root");
    ensure!(
        linhas.iter().filter(|c| c.get(2) == Some(&nossas[0][2])).count() == 1,
        "UID do Hub compartilhado por outra conta"
    );
    let home = PathBuf::from(nossas[0][5]);
    ensure!(home.is_absolute(), "home do Hub inválida");
    Ok(Hub { login: login.to_owned(), uid, gid, home })
}

fn comando(nome: &str) -> Result<PathBuf> {
    ensure!(!nome.contains('/'), "comando inválido");
    for base in ["/usr/sbin", "/usr/bin", "/sbin", "/bin"] {
        let caminho = Path::new(base).join(nome);
        if let Ok(meta) = fs::metadata(&caminho) {
            ensure!(
                meta.is_file() && meta.uid() == 0 && meta.mode() & 0o022 == 0,
                "comando inseguro: {}",
                caminho.display()
            );
            return Ok(caminho);
        }
    }
    bail!("requisito ausente: {nome}")
}

fn executar(nome: &str, args: &[&OsStr]) -> Result<String> {
    let saida = Command::new(comando(nome)?)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("executando {nome}"))?;
    ensure!(saida.status.success(), "{nome} falhou: {}", String::from_utf8_lossy(&saida.stderr).trim());
    Ok(String::from_utf8_lossy(&saida.stdout).into_owned())
}

fn os(texto: &str) -> &OsStr {
    OsStr::new(texto)
}

fn ler_limitado(caminho: &Path, limite: u64) -> Result<Vec<u8>> {
    let arquivo = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(caminho)
        .with_context(|| format!("abrindo {}", caminho.display()))?;
    let meta = arquivo.metadata()?;
    ensure!(meta.is_file() && meta.len() <= limite, "arquivo inválido: {}", caminho.display());
    let mut bytes = Vec::new();
    arquivo.take(limite + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limite, "arquivo grande demais: {}", caminho.display());
    Ok(bytes)
}

fn existe(caminho: &str) -> bool {
    fs::symlink_metadata(caminho).is_ok()
}

fn hash_hex(texto: &str) -> bool {
    texto.len() == 64 && texto.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Pacote lido inteiro para a memória após conferir SHA256SUMS contra o fingerprint.
pub struct Pacote {
    arquivos: BTreeMap<String, Vec<u8>>,
}

impl Pacote {
    pub fn ler(dir: &Path, fingerprint_pacote: &str, fingerprint_raiz: &str) -> Result<Self> {
        ensure!(
            hash_hex(fingerprint_pacote) && hash_hex(fingerprint_raiz),
            "fingerprints devem ser SHA-256 em hexadecimal minúsculo"
        );
        let mut arquivos = BTreeMap::new();
        for nome in ARQUIVOS_PACOTE {
            arquivos.insert(nome.to_owned(), ler_limitado(&dir.join(nome), LIMITE_ARQUIVO)?);
        }
        let somas = &arquivos["SHA256SUMS"];
        ensure!(
            Sha256::digest(somas).hex() == fingerprint_pacote,
            "SHA256SUMS não confere com o fingerprint do pacote"
        );
        let texto = std::str::from_utf8(somas).context("SHA256SUMS inválido")?;
        let mut vistos = BTreeMap::new();
        for linha in texto.lines().filter(|l| !l.is_empty()) {
            let (soma, nome) = linha.split_once("  ").context("linha de SHA256SUMS inválida")?;
            ensure!(hash_hex(soma) && vistos.insert(nome.to_owned(), soma.to_owned()).is_none(), "SHA256SUMS ambíguo");
        }
        ensure!(vistos.len() == ARQUIVOS_PACOTE.len() - 1, "SHA256SUMS deve listar exatamente os arquivos do pacote");
        for (nome, soma) in &vistos {
            let bytes = arquivos.get(nome).context("SHA256SUMS lista arquivo desconhecido")?;
            ensure!(&Sha256::digest(bytes).hex() == soma, "hash de {nome} não confere");
        }
        ensure!(
            Sha256::digest(&arquivos["raiz.json"]).hex() == fingerprint_raiz,
            "raiz TUF não confere com o fingerprint independente"
        );
        Ok(Self { arquivos })
    }

    fn bytes(&self, nome: &str) -> &[u8] {
        &self.arquivos[nome]
    }

    /// Política do pacote completada com a identidade local; validada antes de qualquer efeito.
    fn politica(&self, hub: &Hub) -> Result<Politica> {
        let mut valor: serde_json::Value =
            serde_json::from_slice(self.bytes("politica.json")).context("politica.json inválida")?;
        let objeto = valor.as_object_mut().context("politica.json deve ser objeto")?;
        for campo in ["uid_hub", "gid_hub", "arquitetura"] {
            ensure!(!objeto.contains_key(campo), "politica.json não pode definir {campo}");
        }
        objeto.insert("uid_hub".into(), hub.uid.into());
        objeto.insert("gid_hub".into(), hub.gid.into());
        objeto.insert("arquitetura".into(), std::env::consts::ARCH.into());
        let politica: Politica = serde_json::from_value(valor).context("politica.json incompatível")?;
        politica.validar()?;
        Ok(politica)
    }
}

fn preflight() -> Result<()> {
    let protegido = fs::read_to_string("/proc/sys/fs/protected_hardlinks")?;
    ensure!(protegido.trim() == "1", "ative fs.protected_hardlinks=1 antes de prosseguir (nada foi alterado)");
    // SAFETY: estrutura local preenchida por statfs/statvfs com caminhos fixos.
    let mut sistema: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: caminho fixo terminado em NUL e ponteiro para estrutura local.
    ensure!(unsafe { libc::statfs(c"/sys/fs/cgroup".as_ptr(), &raw mut sistema) } == 0, "cgroup indisponível");
    ensure!(sistema.f_type.cast_unsigned() == 0x6367_7270, "cgroup v2 exigido");
    ensure!(Path::new("/run/systemd/system").is_dir(), "systemd deve estar ativo");
    for raiz in [c"/opt", c"/var/lib"] {
        // SAFETY: estrutura local preenchida pelo kernel.
        let mut vfs: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: caminho fixo e ponteiro válido.
        ensure!(unsafe { libc::statvfs(raiz.as_ptr(), &raw mut vfs) } == 0, "statvfs falhou");
        ensure!(vfs.f_flag & libc::ST_NOSUID == 0, "{} não pode estar montado com nosuid", raiz.to_string_lossy());
    }
    for nome in ["groupadd", "useradd", "usermod", "visudo", "systemctl", "timedatectl", "nologin", "sudo"] {
        comando(nome)?;
    }
    let sincronizado = executar("timedatectl", &[os("show"), os("--property=NTPSynchronized"), os("--value")])?;
    ensure!(
        sincronizado.trim() == "yes",
        "relógio do sistema não sincronizado (NTPSynchronized != yes); nada foi alterado"
    );
    Ok(())
}

/// Objetos criados pela etapa; todos ausentes (nova) ou todos presentes (instalada).
fn objetos_da_etapa() -> [&'static str; 8] {
    [ESTADO, RELEASES, SBIN, SUDOERS, UNIDADE_GUARDA, DROPIN_HUB, SOCKET_COPIA, SERVICO_COPIA]
}

/// Nome do grupo primário do Hub em /etc/group (para o socket do canal do motor).
fn nome_grupo(gid: u32) -> Result<String> {
    let texto = String::from_utf8(ler_limitado(Path::new("/etc/group"), 16 * 1024 * 1024)?)?;
    let achados: Vec<&str> = texto
        .lines()
        .filter_map(|l| {
            let c: Vec<&str> = l.split(':').collect();
            (c.len() == 4 && c[2] == gid.to_string()).then_some(c[0])
        })
        .collect();
    ensure!(achados.len() == 1, "grupo primário do Hub ausente ou ambíguo");
    let nome = achados[0];
    ensure!(
        !nome.is_empty()
            && nome.len() <= 32
            && nome.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')),
        "nome do grupo do Hub incompatível com a unidade"
    );
    Ok(nome.to_owned())
}

fn conferir_canal_motor(hub: &Hub) -> Result<()> {
    conferir_arquivo(SOCKET_COPIA, texto_socket_copia(&nome_grupo(hub.gid)?).as_bytes(), 0o644)?;
    conferir_arquivo(SERVICO_COPIA, TEXTO_SERVICO_COPIA.as_bytes(), 0o644)?;
    ensure!(
        executar("systemctl", &[os("is-active"), os("donnabot-motor-copia.socket")])?.trim() == "active",
        "socket do canal do motor inativo"
    );
    let (uid_motor, ..) = conta(MOTOR_USER)?.context("conta do motor ausente")?;
    let meta = fs::symlink_metadata(SOCKET_RUN)?;
    ensure!(
        std::os::unix::fs::FileTypeExt::is_socket(&meta.file_type())
            && meta.uid() == uid_motor
            && meta.gid() == hub.gid
            && meta.mode() & 0o7777 == 0o660,
        "socket do canal do motor com dono ou modo inesperado"
    );
    Ok(())
}

fn sha_arquivo(caminho: &Path) -> Result<Option<String>> {
    match fs::symlink_metadata(caminho) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
        Ok(meta) => {
            ensure!(meta.is_file() && meta.len() <= LIMITE_ARQUIVO, "objeto legado inesperado: {}", caminho.display());
            let mut arquivo = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
                .open(caminho)?;
            let mut hash = Sha256::new();
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                let lidos = arquivo.read(&mut buffer)?;
                if lidos == 0 {
                    break;
                }
                hash.update(&buffer[..lidos]);
            }
            Ok(Some(hash.finalize().hex()))
        }
    }
}

/// Estado real do Hub legado, somente leitura e por hashes.
fn legado(hub: &Hub) -> Result<Legado> {
    let unidade = hub.home.join(".config/systemd/user/donnabot-host.service");
    let unidade_sha256 = sha_arquivo(&unidade)?;
    if unidade_sha256.is_some() {
        let texto = String::from_utf8(ler_limitado(&unidade, 1024 * 1024)?).context("unidade legada inválida")?;
        let execs: Vec<&str> = texto.lines().filter(|l| l.trim_start().starts_with("ExecStart=")).collect();
        ensure!(execs.len() == 1, "unidade legada ambígua: exatamente um ExecStart esperado");
        ensure!(
            !execs[0].contains("/opt/donnabot/bin/"),
            "instalação F4 de teste detectada; migração administrativa manual exigida"
        );
    }
    let mut dropins_sha256 = BTreeMap::new();
    let dir = hub.home.join(".config/systemd/user/donnabot-host.service.d");
    if let Ok(entradas) = fs::read_dir(&dir) {
        for entrada in entradas {
            let entrada = entrada?;
            let nome = entrada.file_name().to_str().context("drop-in com nome inválido")?.to_owned();
            let soma = sha_arquivo(&entrada.path())?.context("drop-in desapareceu")?;
            ensure!(dropins_sha256.len() < 256, "drop-ins demais");
            dropins_sha256.insert(nome, soma);
        }
    }
    let link = hub.home.join(".local/bin/donnabot-host");
    let link_app = match fs::symlink_metadata(&link) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
        Ok(meta) if meta.file_type().is_symlink() => Some(format!("link:{}", fs::read_link(&link)?.display())),
        Ok(meta) if meta.is_file() => Some(format!("arquivo:{}", sha_arquivo(&link)?.unwrap_or_default())),
        Ok(_) => bail!("link do app legado inesperado"),
    };
    Ok(Legado { unidade_sha256, dropins_sha256, link_app })
}

fn conta(nome: &str) -> Result<Option<(u32, u32, PathBuf, PathBuf)>> {
    let texto = String::from_utf8(ler_limitado(Path::new("/etc/passwd"), 16 * 1024 * 1024)?)?;
    let achadas: Vec<Vec<&str>> =
        texto.lines().map(|l| l.split(':').collect()).filter(|c: &Vec<&str>| c.first() == Some(&nome)).collect();
    ensure!(achadas.len() <= 1, "conta {nome} ambígua");
    achadas
        .first()
        .map(|c| -> Result<_> {
            ensure!(c.len() == 7, "entrada de {nome} inválida");
            Ok((c[2].parse()?, c[3].parse()?, PathBuf::from(c[5]), PathBuf::from(c[6])))
        })
        .transpose()
}

fn grupo(nome: &str) -> Result<Option<(u32, Vec<String>)>> {
    let texto = String::from_utf8(ler_limitado(Path::new("/etc/group"), 16 * 1024 * 1024)?)?;
    let achados: Vec<Vec<&str>> =
        texto.lines().map(|l| l.split(':').collect()).filter(|c: &Vec<&str>| c.first() == Some(&nome)).collect();
    ensure!(achados.len() <= 1, "grupo {nome} ambíguo");
    achados
        .first()
        .map(|c| -> Result<_> {
            ensure!(c.len() == 4, "entrada de {nome} inválida");
            Ok((c[2].parse()?, c[3].split(',').filter(|m| !m.is_empty()).map(str::to_owned).collect()))
        })
        .transpose()
}

fn diretorio(caminho: &str, uid: u32, gid: u32, modo: u32) -> Result<()> {
    let path = Path::new(caminho);
    match fs::symlink_metadata(path) {
        Ok(meta) => ensure!(meta.is_dir() && !meta.file_type().is_symlink(), "{caminho} existe e não é diretório"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::DirBuilder::new().mode(0o700).create(path)?,
        Err(e) => return Err(e.into()),
    }
    let arquivo = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    std::os::unix::fs::fchown(&arquivo, Some(uid), Some(gid))?;
    arquivo.set_permissions(fs::Permissions::from_mode(modo))?;
    arquivo.sync_all()?;
    Ok(())
}

/// Escrita atômica root-owned por nome temporário no mesmo diretório, sem seguir links.
fn gravar_arquivo(caminho: &str, bytes: &[u8], modo: u32) -> Result<()> {
    let path = Path::new(caminho);
    let pai = path.parent().context("caminho sem pai")?;
    let temporario = pai.join(format!(".donnabot-{}", uuid::Uuid::new_v4().simple()));
    let resultado = (|| -> Result<()> {
        let mut arquivo = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&temporario)?;
        arquivo.write_all(bytes)?;
        std::os::unix::fs::fchown(&arquivo, Some(0), Some(0))?;
        arquivo.set_permissions(fs::Permissions::from_mode(modo))?;
        arquivo.sync_all()?;
        if let Ok(meta) = fs::symlink_metadata(path) {
            ensure!(meta.is_file() && meta.uid() == 0, "{caminho} existente inseguro");
        }
        fs::rename(&temporario, path)?;
        File::open(pai)?.sync_all()?;
        Ok(())
    })();
    if resultado.is_err() {
        let _ = fs::remove_file(&temporario);
    }
    resultado
}

fn conferir_arquivo(caminho: &str, bytes: &[u8], modo: u32) -> Result<()> {
    let meta = fs::symlink_metadata(caminho).with_context(|| format!("{caminho} ausente"))?;
    ensure!(
        meta.is_file() && meta.uid() == 0 && meta.gid() == 0 && meta.mode() & 0o7777 == modo && meta.nlink() == 1,
        "{caminho}: dono ou modo divergente"
    );
    ensure!(ler_limitado(Path::new(caminho), LIMITE_ARQUIVO)? == bytes, "{caminho}: conteúdo divergente");
    Ok(())
}

fn ler_cgroup(nome: &str) -> Result<String> {
    Ok(fs::read_to_string(Path::new(CGROUP).join(nome))?.trim().to_owned())
}

/// Recria o cgroup root-owned com os limites da política (também a cada boot).
pub fn preparar_cgroup() -> Result<()> {
    exigir_root_direto(None)?;
    let estado = Diretorio::abrir(Path::new(ESTADO))?;
    let politica = Politica::ler(&estado)?;
    let controladores = fs::read_to_string("/sys/fs/cgroup/cgroup.subtree_control")?;
    let ativos: Vec<&str> = controladores.split_whitespace().collect();
    for controlador in ["memory", "pids", "cpu"] {
        if !ativos.contains(&controlador) {
            fs::write("/sys/fs/cgroup/cgroup.subtree_control", format!("+{controlador}"))?;
        }
    }
    match fs::symlink_metadata(CGROUP) {
        Ok(meta) => ensure!(meta.is_dir() && meta.uid() == 0, "cgroup existente inseguro"),
        Err(_) => fs::DirBuilder::new().mode(0o755).create(CGROUP)?,
    }
    let cpu = format!("{} {}", politica.cgroup_cpu_quota, politica.cgroup_cpu_periodo);
    for (nome, valor) in [
        ("memory.max", politica.cgroup_memoria.to_string()),
        ("pids.max", politica.cgroup_processos.to_string()),
        ("cpu.max", cpu),
    ] {
        // Uma única escrita por arquivo de controle.
        fs::write(Path::new(CGROUP).join(nome), format!("{valor}\n"))?;
        ensure!(ler_cgroup(nome)? == valor, "{nome} não confirmado");
    }
    Ok(())
}

fn auditar(estado: &Diretorio, resultado: Resultado, motivo: Motivo) -> Result<()> {
    let evento = Evento {
        horario_unix: agora()?,
        uid: 0,
        origem: Origem::AdminRoot,
        operacao: Operacao::Administrar,
        sequencia_atual: None,
        sequencia_anterior: None,
        resultado,
        motivo,
    };
    if matches!(resultado, Resultado::Inicio) { estado.inicio(&evento) } else { estado.auditar(&evento) }
}

fn criar_estado(pacote: &Pacote, politica: &Politica, baseline: &Baseline) -> Result<()> {
    diretorio("/var/lib/donnabot", 0, 0, 0o755)?;
    // Exclusivo: um estado preexistente é estado misto e já foi recusado antes.
    fs::DirBuilder::new().mode(0o700).create(ESTADO).context("estado da atualização já existe")?;
    diretorio(ESTADO, 0, 0, 0o700)?;
    diretorio(&format!("{ESTADO}/transacoes"), 0, 0, 0o700)?;
    for nome in ["lock", "auditoria"] {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(Path::new(ESTADO).join(nome))?;
    }
    let estado = Diretorio::abrir(Path::new(ESTADO))?;
    let trava = estado.travar()?;
    estado.gravar_baseline_inicial(baseline)?;
    estado.gravar("politica", politica)?;
    let raiz = pacote.bytes("raiz.json");
    let pinada = tuf::carregar_raiz_pinada(raiz, agora()?)?;
    estado.gravar(
        "historico-tuf",
        &serde_json::json!({"raiz": pinada.version, "raiz_base64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, raiz), "versoes": {}}),
    )?;
    estado.carregar_historico_tuf(&trava)?;
    estado.inicializar_relogio(&trava)?;
    estado.hora_conferida(&trava)?;
    Ok(())
}

fn contas(hub: &Hub) -> Result<()> {
    if grupo(GRUPO_TRABALHO)?.is_none() {
        executar("groupadd", &[os("--system"), os(GRUPO_TRABALHO)])?;
    }
    let (gid_trabalho, _) = grupo(GRUPO_TRABALHO)?.context("grupo de trabalho ausente")?;
    ensure!(gid_trabalho != 0 && gid_trabalho != hub.gid, "grupo de trabalho deve ser dedicado");
    let nologin = comando("nologin")?;
    if conta(MOTOR_USER)?.is_none() {
        executar(
            "useradd",
            &[
                os("--system"),
                os("--user-group"),
                os("--no-create-home"),
                os("--home-dir"),
                os(MOTOR_HOME),
                os("--shell"),
                nologin.as_os_str(),
                os(MOTOR_USER),
            ],
        )?;
    }
    let (uid, gid, home, shell) = conta(MOTOR_USER)?.context("conta do motor ausente")?;
    ensure!(
        uid != 0 && uid != hub.uid && gid != 0 && gid != hub.gid && home == Path::new(MOTOR_HOME) && shell == nologin,
        "conta {MOTOR_USER} existente incompatível; não será reciclada"
    );
    for membro in [hub.login.as_str(), MOTOR_USER] {
        executar("usermod", &[os("-a"), os("-G"), os(GRUPO_TRABALHO), os(membro)])?;
    }
    diretorio(MOTOR_HOME, uid, gid, 0o700)?;
    diretorio(TRABALHO, hub.uid, gid_trabalho, 0o2770)?;
    Ok(())
}

/// O guard para a unidade por `systemctl --user --machine=LOGIN@.host`: sem o
/// barramento de sistema e o do usuário (gerenciador ativo, ex. linger), ele não
/// conseguiria parar o Hub na expiração. Recusa antes de instalar.
fn guard_alcanca_gerenciador(hub: &Hub) -> Result<()> {
    ensure!(
        fs::symlink_metadata("/run/dbus/system_bus_socket")
            .is_ok_and(|m| std::os::unix::fs::FileTypeExt::is_socket(&m.file_type())),
        "barramento D-Bus de sistema ausente; o guard não conseguiria parar o Hub (instale dbus)"
    );
    let maquina = format!("--machine={}@.host", hub.login);
    executar(
        "systemctl",
        &[os("--user"), os(&maquina), os("--no-pager"), os("show"), os("-p"), os("Id"), os("donnabot-host.service")],
    )
    .context("o guard não alcança o gerenciador systemd do usuário do Hub (dbus-user-session e linger exigidos)")?;
    Ok(())
}

fn sudoers_instalar(hub: &Hub) -> Result<()> {
    let fragmento = sudoers::fragmento(&hub.login, hub.uid)?;
    // Nome com ponto: o sudo ignora o temporário em /etc/sudoers.d.
    let temporario = format!("/etc/sudoers.d/.donnabot-{}", uuid::Uuid::new_v4().simple());
    gravar_arquivo(&temporario, fragmento.as_bytes(), 0o440)?;
    let checagem = executar("visudo", &[os("-cqf"), os(&temporario)]);
    if checagem.is_err() {
        let _ = fs::remove_file(&temporario);
        checagem?;
    }
    fs::rename(&temporario, SUDOERS)?;
    File::open("/etc/sudoers.d")?.sync_all()?;
    if let Err(erro) = executar("visudo", &[os("-cq")]).and_then(|_| conferir_sudo(hub)) {
        fs::remove_file(SUDOERS).with_context(|| format!("não foi possível remover a regra recusada: {erro:#}"))?;
        File::open("/etc/sudoers.d")?.sync_all()?;
        return Err(erro.context("configuração sudoers recusada; regra removida"));
    }
    Ok(())
}

/// `sudo -l` do login: exatamente as três formas e nenhuma regra ampla adicional.
fn conferir_sudo(hub: &Hub) -> Result<()> {
    conferir_arquivo(SUDOERS, sudoers::fragmento(&hub.login, hub.uid)?.as_bytes(), 0o440).context(
        "fragmento sudoers divergente; para instalações antigas, revogue e reinstale pelo fluxo administrativo",
    )?;
    let lista = executar("sudo", &[os("-n"), os("-l"), os("-U"), os(&hub.login)])?;
    sudoers::conferir_lista(&lista, &hub.login)
}

pub fn verificar(login: &str) -> Result<()> {
    exigir_root_direto(None)?;
    modo_motor::exigir_local(modo_motor::configurado_em(Path::new(MOTOR_CONF)))?;
    let hub = hub(login)?;
    let estado = Diretorio::abrir(Path::new(ESTADO))?;
    let politica = Politica::ler(&estado)?;
    ensure!(politica.uid_hub == hub.uid && politica.gid_hub == hub.gid, "instalação pertence a outro Hub");
    let trava = estado.travar()?;
    estado.carregar_historico_tuf(&trava)?;
    estado.hora_conferida(&trava)?;
    let _: serde_json::Value = estado.ler("baseline")?;
    drop(trava);
    Diretorio::abrir_publicacao()?;
    Diretorio::abrir_com_modo(Path::new(SBIN), 0o755)?;
    for (_, destino) in BINARIOS {
        let meta = fs::symlink_metadata(Path::new(SBIN).join(destino))?;
        ensure!(
            meta.is_file() && meta.uid() == 0 && meta.gid() == 0 && meta.mode() & 0o7777 == 0o755 && meta.nlink() == 1,
            "{destino} inseguro"
        );
    }
    conferir_arquivo(UNIDADE_GUARDA, TEXTO_UNIDADE_GUARDA.as_bytes(), 0o644)?;
    conferir_arquivo(UNIDADE_CGROUP, TEXTO_UNIDADE_CGROUP.as_bytes(), 0o644)?;
    conferir_arquivo(DROPIN_HUB, TEXTO_DROPIN_HUB.as_bytes(), 0o644).context(
        "configuração administrativa divergente; para instalações antigas, revogue e reinstale pelo fluxo administrativo",
    )?;
    conferir_sudo(&hub)?;
    guard_alcanca_gerenciador(&hub)?;
    conferir_canal_motor(&hub)?;
    ensure!(ler_cgroup("memory.max")? == politica.cgroup_memoria.to_string(), "cgroup divergente da política");
    let guarda = format!("donnabot-guarda-atualizacao@{}.service", hub.uid);
    ensure!(executar("systemctl", &[os("is-active"), os(&guarda)])?.trim() == "active", "guard inativo");
    Ok(())
}

/// Instalação limpa: o login não pode ter Hub legado (unidade própria, drop-ins ou o
/// binário do app em `~/.local/bin`). Uma unidade que já aponta para o par gerido é a
/// retomada de uma instalação limpa anterior. Conferido antes de qualquer efeito.
fn exigir_sem_hub_legado(hub: &Hub) -> Result<()> {
    let legado = legado(hub)?;
    let unidade = hub.home.join(".config/systemd/user/donnabot-host.service");
    let gerida = legado.unidade_sha256.is_some()
        && String::from_utf8(ler_limitado(&unidade, 1024 * 1024)?)
            .context("unidade do Hub inválida")?
            .lines()
            .any(|l| l.trim_start().starts_with("ExecStart=") && l.contains(HUB_ATIVO));
    ensure!(
        (legado.unidade_sha256.is_none() || gerida) && legado.dropins_sha256.is_empty() && legado.link_app.is_none(),
        "Hub legado presente para {}: a instalação limpa (--hub-novo) não o substitui; migre com `donnabot-host coordenar-atualizacao`",
        hub.login
    );
    Ok(())
}

/// Fase do status público (`None` antes da primeira publicação).
fn fase_publica() -> Result<Option<String>> {
    match fs::symlink_metadata(STATUS_PUBLICO) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
        Ok(_) => (),
    }
    let status: serde_json::Value =
        serde_json::from_slice(&ler_limitado(Path::new(STATUS_PUBLICO), 4096)?).context("status público inválido")?;
    Ok(status["fase"].as_str().map(str::to_owned))
}

/// Instala e ativa o Hub no layout gerido (instalação limpa, #812). A primeira release
/// vem pelo caminho autenticado de sempre: o wrapper, executado aqui como administrador
/// root, verifica a cadeia TUF, publica o slot, troca o seletor e abre a janela pendente.
/// Depois, como o UID do Hub (sem grupos nem capacidades de root), `donnabot-host
/// instalar-gerido` cria a unidade apontando para o seletor, inicia, roda a F3 na janela
/// da primeira migração e aceita; em falha, desfaz a unidade e reverte a release.
fn instalar_hub_gerido(hub: &Hub, porta: u16) -> Result<()> {
    if !matches!(fase_publica()?.as_deref(), Some("pendente" | "liberado")) {
        let estado = Command::new(WRAPPER)
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .stdin(Stdio::null())
            .status()
            .context("executando o wrapper")?;
        ensure!(
            estado.success(),
            "o wrapper não publicou a primeira release do Hub (diagnóstico em {ESTADO}/diagnostico)"
        );
        ensure!(fase_publica()?.as_deref() == Some("pendente"), "primeira release publicada sem janela pendente");
    }
    let porta = porta.to_string();
    let tentativa = uuid::Uuid::new_v4().simple().to_string();
    let estado = Command::new(comando("setpriv")?)
        .arg(format!("--reuid={}", hub.uid))
        .arg(format!("--regid={}", hub.gid))
        .arg("--init-groups")
        .arg("--inh-caps=-all")
        .arg("--")
        .arg(HUB_ATIVO)
        .args(["instalar-gerido", "--porta", &porta])
        .env_clear()
        .env("HOME", &hub.home)
        .env("MEXERICA_INSTALACAO_ID", &tentativa)
        .env("USER", &hub.login)
        .env("LOGNAME", &hub.login)
        .env("PATH", "/usr/bin:/bin")
        .env("XDG_RUNTIME_DIR", format!("/run/user/{}", hub.uid))
        .env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path=/run/user/{}/bus", hub.uid))
        .current_dir("/")
        .stdin(Stdio::null())
        .status();
    let falha = match estado {
        Ok(estado) if estado.success() => return Ok(()),
        Ok(estado) => anyhow::anyhow!("instalação do Hub gerido recusada (motivo acima; {estado})"),
        Err(erro) => anyhow::Error::from(erro).context("executando a instalação do Hub como o usuário do Hub"),
    };
    // A release já foi publicada: qualquer falha do passo do Hub (inclusive `setpriv` que
    // não executa ou processo encerrado no meio) desfaz a publicação também aqui. O passo do
    // Hub desfaz a dele; esta volta cobre o que ele não alcançou e é idempotente.
    Err(match desfazer_primeira_publicacao(hub, &tentativa) {
        Ok(true) => falha.context("primeira release revertida pela etapa administrativa"),
        Ok(false) => falha,
        Err(erro) => falha.context(format!("e a volta administrativa também falhou: {erro:#}")),
    })
}

fn unidade_da_tentativa(texto: &str) -> bool {
    let execs: Vec<_> = texto.lines().filter_map(|l| l.trim().strip_prefix("ExecStart=")).collect();
    execs.len() == 1
        && shlex::split(execs[0]).is_some_and(|args| {
            args.first().is_some_and(|s| s == HUB_ATIVO) && args.get(1).is_some_and(|s| s == "serve")
        })
}

/// `true` quando uma marca de instalação gerida da home corresponde a esta tentativa.
/// Marca ausente ou de outra tentativa só desautoriza a remoção da unidade; erros de
/// leitura continuam fatais (fail-closed).
fn marca_da_tentativa(hub: &Hub, tentativa: &str) -> Result<bool> {
    let marcas =
        [".mexerica", ".mexerica-dev"].map(|dir| hub.home.join(dir).join("atualizacao-privilegiada/instalacao-gerida"));
    let mut corresponde = false;
    for marca in marcas {
        match fs::symlink_metadata(&marca) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
            Ok(_) => corresponde |= ler_limitado(&marca, 128)? == tentativa.as_bytes(),
        }
    }
    Ok(corresponde)
}

/// Reverte a janela da primeira publicação (`--reverter` como administrador root) depois de
/// parar o Hub da unidade desta instalação. `false` quando não há o que desfazer: o passo do
/// Hub já reverteu (status não está pendente nem em reversão). Uma janela que não é a da
/// primeira publicação nunca é revertida aqui.
fn desfazer_primeira_publicacao(hub: &Hub, tentativa: &str) -> Result<bool> {
    if !matches!(fase_publica()?.as_deref(), Some("pendente" | "revertendo")) {
        return Ok(false);
    }
    let certidao: serde_json::Value = serde_json::from_slice(&ler_limitado(Path::new(CERTIDAO_PUBLICA), 4096)?)
        .context("certidão pública inválida")?;
    ensure!(certidao["sequencia_anterior"].is_null(), "a janela aberta não é a da primeira publicação; nada revertido");
    // O wrapper recusa reverter com o Hub em execução; a unidade é desta instalação (o Hub
    // legado foi recusado antes de qualquer efeito).
    let maquina = format!("--machine={}@.host", hub.login);
    let unidade = hub.home.join(".config/systemd/user/donnabot-host.service");
    if unidade.exists() {
        // Marca de outra tentativa: a unidade não é desta instalação e a remoção é
        // pulada (fail-closed), mas a parada e o `--reverter` abaixo ainda ocorrem.
        if marca_da_tentativa(hub, tentativa)? {
            let texto = String::from_utf8(ler_limitado(&unidade, 1024 * 1024)?)?;
            ensure!(unidade_da_tentativa(&texto), "unidade não corresponde à instalação gerida; limpeza recusada");
            executar(
                "systemctl",
                &[os("--user"), os(&maquina), os("disable"), os("--now"), os("donnabot-host.service")],
            )?;
            let estado = Command::new(comando("setpriv")?)
                .arg(format!("--reuid={}", hub.uid))
                .arg(format!("--regid={}", hub.gid))
                .args(["--init-groups", "--inh-caps=-all", "--"])
                .arg(comando("rm")?)
                .arg("--")
                .arg(&unidade)
                .env_clear()
                .stdin(Stdio::null())
                .status()
                .context("removendo unidade como usuário do Hub")?;
            ensure!(estado.success(), "remoção da unidade recusada");
            executar("systemctl", &[os("--user"), os(&maquina), os("daemon-reload")])?;
        } else {
            eprintln!(
                "instalação: unidade sem a identidade desta tentativa; remoção ignorada, stop e --reverter seguem"
            );
        }
    }
    if let Err(erro) = executar("systemctl", &[os("--user"), os(&maquina), os("stop"), os("donnabot-host.service")]) {
        eprintln!("instalação: systemctl --user stop falhou ({erro:#}); conferindo parada pelo kernel");
    }
    super::servico_hub::exigir_parado(&Diretorio::abrir_publicacao()?, hub.uid, super::chamador::Identidade::AdminRoot)
        .context("o Hub continua em execução; pare-o e rode o wrapper com --reverter")?;
    let estado = Command::new(WRAPPER)
        .arg("--reverter")
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .stdin(Stdio::null())
        .status()
        .context("executando o wrapper com --reverter")?;
    ensure!(estado.success(), "--reverter recusado (diagnóstico em {ESTADO}/diagnostico)");
    Ok(true)
}

/// `hub_novo`: porta do Hub para a instalação limpa no layout gerido; `None` só prepara a
/// parte privilegiada (uma instalação antiga migra depois pelo coordenador).
pub fn instalar(
    login: &str,
    pacote_dir: &Path,
    fingerprint_pacote: &str,
    fingerprint_raiz: &str,
    hub_novo: Option<u16>,
) -> Result<()> {
    let hub = hub(login)?;
    exigir_root_direto(Some(hub.uid))?;
    modo_motor::exigir_local(modo_motor::configurado_em(Path::new(MOTOR_CONF)))?;
    preflight()?;
    let pacote = Pacote::ler(pacote_dir, fingerprint_pacote, fingerprint_raiz)?;
    guard_alcanca_gerenciador(&hub)?;
    let politica = pacote.politica(&hub)?;
    tuf::carregar_raiz_pinada(pacote.bytes("raiz.json"), agora()?)?;
    ensure!(
        !existe("/opt/donnabot/bin"),
        "instalação F4 de teste (/opt/donnabot/bin) detectada; migração administrativa manual exigida"
    );
    if hub_novo.is_some() {
        exigir_sem_hub_legado(&hub)?;
        comando("setpriv")?;
    }
    let presentes: Vec<&str> = objetos_da_etapa().into_iter().filter(|o| existe(o)).collect();
    if presentes.len() == objetos_da_etapa().len() {
        println!("Instalação já existente; conferindo.");
        verificar(login)?;
        if let Some(porta) = hub_novo {
            hub_gerido_auditado(&hub, porta)?;
        }
        return Ok(());
    }
    ensure!(
        presentes.is_empty(),
        "estado misto: já existem {presentes:?}; nada foi alterado, revisão administrativa exigida"
    );
    // Baseline antes de qualquer alteração (ausências inclusive).
    let mut objetos = BTreeMap::new();
    for objeto in [
        "/var/lib/donnabot",
        MOTOR_HOME,
        TRABALHO,
        "/opt/donnabot",
        "/etc/donnabot",
        MOTOR_CONF,
        CGROUP,
        UNIDADE_CGROUP,
    ] {
        objetos.insert(objeto.to_owned(), existe(objeto));
    }
    objetos.insert(format!("usuario:{MOTOR_USER}"), conta(MOTOR_USER)?.is_some());
    let membros = grupo(GRUPO_TRABALHO)?;
    objetos.insert(format!("grupo:{GRUPO_TRABALHO}"), membros.is_some());
    objetos.insert(format!("membro:{}", hub.login), membros.is_some_and(|(_, m)| m.contains(&hub.login)));
    let configuracao = if existe("/etc/donnabot") {
        Diretorio::abrir_configuracao()?.capturar_motor_conf()?
    } else {
        super::snapshot::Configuracao::Ausente
    };
    modo_motor::exigir_local(!matches!(configuracao, super::snapshot::Configuracao::Ausente))?;
    let baseline = Baseline {
        versao: 1,
        login: hub.login.clone(),
        uid_hub: hub.uid,
        gid_hub: hub.gid,
        motor_conf: configuracao,
        objetos,
        legado: legado(&hub)?,
    };
    criar_estado(&pacote, &politica, &baseline)?;
    let estado = Diretorio::abrir(Path::new(ESTADO))?;
    auditar(&estado, Resultado::Inicio, Motivo::Solicitacao)?;
    let resultado = (|| -> Result<()> {
        contas(&hub)?;
        diretorio("/etc/donnabot", 0, 0, 0o755)?;
        diretorio("/opt/donnabot", 0, 0, 0o755)?;
        diretorio(SBIN, 0, 0, 0o755)?;
        for (origem, destino) in BINARIOS {
            gravar_arquivo(&format!("{SBIN}/{destino}"), pacote.bytes(origem), 0o755)?;
        }
        diretorio(RELEASES, 0, 0, 0o755)?;
        gravar_arquivo(UNIDADE_CGROUP, TEXTO_UNIDADE_CGROUP.as_bytes(), 0o644)?;
        diretorio("/etc/systemd/user", 0, 0, 0o755)?;
        diretorio("/etc/systemd/user/donnabot-host.service.d", 0, 0, 0o755)?;
        gravar_arquivo(DROPIN_HUB, TEXTO_DROPIN_HUB.as_bytes(), 0o644)?;
        gravar_arquivo(UNIDADE_GUARDA, TEXTO_UNIDADE_GUARDA.as_bytes(), 0o644)?;
        executar("systemctl", &[os("daemon-reload")])?;
        executar("systemctl", &[os("enable"), os("--now"), os("donnabot-cgroup-atualizacao.service")])?;
        let guarda = format!("donnabot-guarda-atualizacao@{}.service", hub.uid);
        executar("systemctl", &[os("enable"), os("--now"), os(&guarda)])?;
        gravar_arquivo(SOCKET_COPIA, texto_socket_copia(&nome_grupo(hub.gid)?).as_bytes(), 0o644)?;
        gravar_arquivo(SERVICO_COPIA, TEXTO_SERVICO_COPIA.as_bytes(), 0o644)?;
        executar("systemctl", &[os("daemon-reload")])?;
        executar("systemctl", &[os("enable"), os("--now"), os("donnabot-motor-copia.socket")])?;
        // Por último: a regra só existe depois de tudo o que ela protege.
        sudoers_instalar(&hub)?;
        verificar(login)
    })();
    let (saida, motivo) =
        if resultado.is_ok() { (Resultado::Sucesso, Motivo::Solicitacao) } else { (Resultado::Falha, Motivo::Sistema) };
    auditar(&estado, saida, motivo)?;
    resultado?;
    println!("Etapa administrativa concluída para {} (UID {}).", hub.login, hub.uid);
    println!("Fingerprint da raiz TUF pinada: {fingerprint_raiz}");
    if let Some(porta) = hub_novo {
        hub_gerido_auditado(&hub, porta)?;
    } else {
        println!("Uma nova sessão do gerenciador do usuário pode ser necessária para o grupo {GRUPO_TRABALHO}.");
    }
    Ok(())
}

fn hub_gerido_auditado(hub: &Hub, porta: u16) -> Result<()> {
    let estado = Diretorio::abrir(Path::new(ESTADO))?;
    auditar(&estado, Resultado::Inicio, Motivo::Solicitacao)?;
    let resultado = instalar_hub_gerido(hub, porta);
    let (saida, motivo) =
        if resultado.is_ok() { (Resultado::Sucesso, Motivo::Solicitacao) } else { (Resultado::Falha, Motivo::Sistema) };
    auditar(&estado, saida, motivo)?;
    resultado?;
    println!("Hub instalado e ativo no layout gerido ({HUB_ATIVO}) para {}.", hub.login);
    Ok(())
}

/// Substitui a raiz TUF confiável por outra conferida por canal independente.
pub fn repinar_raiz(caminho: &Path, fingerprint: &str) -> Result<()> {
    exigir_root_direto(None)?;
    ensure!(hash_hex(fingerprint), "fingerprint inválido");
    let bytes = ler_limitado(caminho, 1024 * 1024)?;
    ensure!(Sha256::digest(&bytes).hex() == fingerprint, "raiz não confere com o fingerprint independente");
    let estado = Diretorio::abrir(Path::new(ESTADO))?;
    let trava = estado.travar()?;
    let agora = estado.hora_conferida(&trava)?;
    let evento = |resultado| Evento {
        horario_unix: agora,
        uid: 0,
        origem: Origem::AdminRoot,
        operacao: Operacao::Administrar,
        sequencia_atual: None,
        sequencia_anterior: None,
        resultado,
        motivo: Motivo::Solicitacao,
    };
    estado.inicio(&evento(Resultado::Inicio))?;
    let resultado = estado.repinar_raiz_tuf(&trava, &bytes, agora);
    estado.auditar(&evento(if resultado.is_ok() { Resultado::Sucesso } else { Resultado::Falha }))?;
    let versao = resultado?;
    println!("Raiz TUF repinada na versão {versao}; histórico de versões zerado.");
    Ok(())
}

/// Presença comprovada sem seguir links: ausência só com `ENOENT`; outros erros recusam.
fn presente(caminho: &str) -> Result<bool> {
    match fs::symlink_metadata(caminho) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => bail!("não foi possível verificar {caminho}: {e}"),
    }
}

/// Estado arquivado mais recente de uma revogação anterior (reexecução idempotente).
fn ultimo_arquivado() -> Result<Option<PathBuf>> {
    let prefixo = "atualizacao.revogada-";
    let mut ultimo: Option<(i64, PathBuf)> = None;
    for entrada in fs::read_dir("/var/lib/donnabot")? {
        let entrada = entrada?;
        let nome = entrada.file_name();
        let Some(sufixo) = nome.to_str().and_then(|n| n.strip_prefix(prefixo)) else { continue };
        let instante: i64 = sufixo.parse().context("estado arquivado com nome inválido")?;
        if ultimo.as_ref().is_none_or(|(atual, _)| instante > *atual) {
            ultimo = Some((instante, entrada.path()));
        }
    }
    Ok(ultimo.map(|(_, caminho)| caminho))
}

/// Consulta efetiva do sudo para uma forma exata do wrapper. `true` = autorizado;
/// `false` = negação comprovada (status 1 sem saída); qualquer outro resultado recusa.
fn sudo_autoriza(login: &str, args: &[&str]) -> Result<bool> {
    let saida = Command::new(comando("sudo")?)
        .args(["-n", "-l", "-U", login, WRAPPER])
        .args(args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .context("executando sudo -l")?;
    match saida.status.code() {
        Some(0) => Ok(true),
        Some(1) if saida.stdout.is_empty() && saida.stderr.is_empty() => Ok(false),
        _ => bail!(
            "consulta sudo -l inconclusiva (status {:?}): {}",
            saida.status.code(),
            String::from_utf8_lossy(&saida.stderr).trim()
        ),
    }
}

/// Para e desabilita uma unidade de sistema e prova que não está ativa. Falha ao
/// parar só é tolerada se a unidade comprovadamente não existe.
fn desativar_unidade(unidade: &str, arquivo: Option<&str>) -> Result<()> {
    let parada = executar("systemctl", &[os("disable"), os("--now"), os(unidade)]);
    if let Err(erro) = parada {
        let existe_arquivo = match arquivo {
            Some(arquivo) => presente(arquivo)?,
            None => true,
        };
        ensure!(!existe_arquivo, "não foi possível parar {unidade}: {erro:#}");
    }
    let estado = Command::new(comando("systemctl")?)
        .args(["is-active", unidade])
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()?;
    let texto = String::from_utf8_lossy(&estado.stdout).trim().to_owned();
    ensure!(matches!(texto.as_str(), "inactive" | "failed"), "{unidade} não está comprovadamente parada ({texto})");
    Ok(())
}

fn verificar_parada_hub(parada: Result<()>, prova: Result<()>) -> Result<()> {
    match (parada, prova) {
        (Ok(()), prova) => prova,
        (Err(_), Ok(())) => {
            eprintln!("revogar: solicitação de parada falhou; parada do Hub confirmada pelo kernel");
            Ok(())
        }
        (Err(_), Err(prova)) => Err(prova.context("solicitação de parada do Hub também falhou")),
    }
}

fn conter_revogacao<T>(
    bloqueio: Result<()>,
    desarme: Result<T>,
    parar_wrapper: impl FnOnce() -> Result<()>,
    parar_hub: impl FnOnce() -> Result<()>,
) -> Result<T> {
    let desarme = match (bloqueio, desarme) {
        (Err(bloqueio), Err(desarme)) => {
            Err(anyhow::anyhow!("bloqueio da atualização: {bloqueio:#}; desarme: {desarme:#}"))
        }
        (bloqueio, desarme) => bloqueio.and(desarme),
    };
    let wrapper = parar_wrapper();
    let hub = parar_hub();
    let falhas = [
        ("desarme", desarme.as_ref().err()),
        ("parada do wrapper", wrapper.as_ref().err()),
        ("parada do Hub", hub.as_ref().err()),
    ]
    .into_iter()
    .filter_map(|(etapa, erro)| erro.map(|erro| format!("{etapa}: {erro:#}")))
    .collect::<Vec<_>>();
    ensure!(falhas.is_empty(), "revogação incompleta: {}", falhas.join("; "));
    desarme
}

/// Revogação administrativa (root direto). Ordem do desenho, com cada etapa provada:
/// identidade conferida contra a política do estado, auditoria de início antes de
/// qualquer efeito, sudoers removido e negação efetiva comprovada por forma, trava
/// do wrapper mantida até o fim, Hub e unidades comprovadamente parados, ajudante
/// sem capabilities e sem processos, remoção dos objetos e estado arquivado.
/// Reexecutar após uma revogação concluída ou interrompida é idempotente.
pub fn revogar(login: &str) -> Result<()> {
    exigir_root_direto(None)?;
    let hub = hub(login)?;
    let horario = agora()?;
    ensure!(horario > 0, "relógio inválido");
    let (caminho_estado, arquivado) = if presente(ESTADO)? {
        (PathBuf::from(ESTADO), false)
    } else {
        let anterior = ultimo_arquivado()?.context("nenhuma instalação nem revogação anterior reconhecida")?;
        (anterior, true)
    };
    let estado = Diretorio::abrir(&caminho_estado)?;
    let politica = Politica::ler(&estado)?;
    ensure!(
        politica.uid_hub == hub.uid && politica.gid_hub == hub.gid,
        "o login informado não corresponde ao Hub desta instalação"
    );
    let evento = |resultado| Evento {
        horario_unix: horario,
        uid: 0,
        origem: Origem::AdminRoot,
        operacao: Operacao::Revogar,
        sequencia_atual: None,
        sequencia_anterior: None,
        resultado,
        motivo: Motivo::Solicitacao,
    };
    estado.inicio(&evento(Resultado::Inicio))?;
    // A trava do wrapper, uma vez obtida, só é liberada depois do arquivamento e da
    // auditoria final.
    let mut trava_mantida: Option<File> = None;
    let resultado = (|| -> Result<()> {
        // 1. Regra removida e negação efetiva comprovada para cada forma exata.
        if presente(SUDOERS)? {
            for forma in FORMAS_WRAPPER {
                ensure!(
                    sudo_autoriza(&hub.login, forma)?,
                    "controle positivo do sudo falhou antes da remoção; revise manualmente"
                );
            }
            fs::remove_file(SUDOERS)?;
            File::open("/etc/sudoers.d")?.sync_all()?;
        }
        executar("visudo", &[os("-cq")])?;
        if presente(WRAPPER)? {
            for forma in FORMAS_WRAPPER {
                ensure!(
                    !sudo_autoriza(&hub.login, forma)?,
                    "o UID do Hub ainda alcança o wrapper por outra regra sudoers (ALL, alias ou regra ampla); revise manualmente"
                );
            }
            println!("revogar: regra sudoers removida; negação comprovada nas três formas");
        } else {
            // Os binários só são removidos depois desta etapa: uma execução anterior já
            // comprovou a negação, e sem o arquivo o sudo não tem o que autorizar.
            println!(
                "revogar: wrapper já removido por revogação anterior; regra sudoers ausente e configuração válida"
            );
        }
        // 2. Nenhuma chamada em curso: a trava do wrapper fica conosco até o fim.
        let mut trava = None;
        for _ in 0..600 {
            trava = estado.tentar_travar()?;
            if trava.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        trava_mantida =
            Some(trava.context("operação do wrapper em andamento há mais de 10 minutos; revogação interrompida")?);
        let publicacao = presente(RELEASES)?.then(Diretorio::abrir_publicacao).transpose()?;
        let (bloqueio, ajudantes) = if let Some(publicacao) = &publicacao {
            let bloqueio = estado.bloquear_por_falha(trava_mantida.as_ref().context("trava ausente")?);
            let ajudantes = super::slots::em_todos([super::slots::Slot::A, super::slots::Slot::B], |slot| {
                publicacao.desarmar_slot(slot)
            });
            (bloqueio, ajudantes)
        } else {
            (Ok(()), Ok(Vec::new()))
        };
        // Chamada já iniciada e suspensa antes de obter a trava: encerrada e conferida (o
        // inode do wrapper não tem capabilities; a rotina mira quem o executa).
        let parar_wrapper = || -> Result<()> {
            if !presente(WRAPPER)? {
                return Ok(());
            }
            let wrapper = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
                .open(WRAPPER)?;
            super::processos::encerrar_wrapper(&wrapper)
                .context("chamada do wrapper em curso não pôde ser encerrada")?;
            println!("revogar: nenhuma chamada do wrapper em curso (suspensas encerradas)");
            Ok(())
        };
        // 3. Hub e filhos parados (provado por cgroup e /proc), depois guard, canal e cgroup.
        let parar_hub = || -> Result<()> {
            let maquina = format!("--machine={}@.host", hub.login);
            let parada =
                executar("systemctl", &[os("--user"), os(&maquina), os("stop"), os("donnabot-host.service")]).map(drop);
            let prova = if let Some(publicacao) = &publicacao {
                super::servico_hub::exigir_parado(publicacao, hub.uid, super::chamador::Identidade::AdminRoot)
            } else {
                super::servico_hub::exigir_parado_sem_slots(hub.uid)
            };
            verificar_parada_hub(parada, prova)
        };
        let ajudantes = conter_revogacao(bloqueio, ajudantes, parar_wrapper, parar_hub)?;
        let guarda = format!("donnabot-guarda-atualizacao@{}.service", hub.uid);
        desativar_unidade(&guarda, Some(UNIDADE_GUARDA))?;
        desativar_unidade("donnabot-motor-copia.socket", Some(SOCKET_COPIA))?;
        desativar_unidade("donnabot-motor-copia.service", Some(SERVICO_COPIA))?;
        desativar_unidade("donnabot-cgroup-atualizacao.service", Some(UNIDADE_CGROUP))?;
        println!("revogar: Hub, guard, canal do motor e cgroup comprovadamente parados");
        // 4. Só depois de desarmar todos os slots, conferir referências antes da remoção.
        if publicacao.is_some() {
            for ajudante in ajudantes.into_iter().flatten() {
                super::processos::verificar_ajudante_revogado(&ajudante, hub.uid)?;
            }
            drop(publicacao);
            fs::remove_dir_all(RELEASES)?;
            File::open("/opt/donnabot")?.sync_all()?;
            println!("revogar: capabilities zeradas e conferidas; slots e seletor removidos");
        }
        // 5. Binários, unidades, drop-in, socket e cgroup (que precisa estar vazio).
        for (_, destino) in BINARIOS {
            let caminho = format!("{SBIN}/{destino}");
            if presente(&caminho)? {
                fs::remove_file(&caminho)?;
            }
        }
        if presente(SBIN)? {
            fs::remove_dir(SBIN).context("diretório de binários com conteúdo inesperado")?;
        }
        for unidade in [UNIDADE_GUARDA, UNIDADE_CGROUP, SOCKET_COPIA, SERVICO_COPIA, DROPIN_HUB] {
            if presente(unidade)? {
                fs::remove_file(unidade)?;
            }
        }
        let dropins = "/etc/systemd/user/donnabot-host.service.d";
        if presente(dropins)? && fs::read_dir(dropins)?.next().is_none() {
            fs::remove_dir(dropins)?;
        }
        if presente(SOCKET_RUN)? {
            fs::remove_file(SOCKET_RUN)?;
        }
        if presente(CGROUP)? {
            fs::remove_dir(CGROUP).context("cgroup de atualização não está vazio")?;
        }
        executar("systemctl", &[os("daemon-reload")])?;
        Ok(())
    })();
    if let Err(erro) = resultado {
        estado.auditar(&evento(Resultado::Falha))?;
        return Err(erro);
    }
    // 6. Estado (política, chave, journal, auditoria) arquivado fora do uso; o sucesso
    // só é registrado depois do arquivamento durável.
    let destino = if arquivado {
        caminho_estado
    } else {
        let destino = PathBuf::from(format!("{ESTADO}.revogada-{horario}"));
        ensure!(!presente(destino.to_str().context("caminho inválido")?)?, "estado arquivado já existe");
        fs::rename(ESTADO, &destino)?;
        File::open("/var/lib/donnabot")?.sync_all()?;
        destino
    };
    Diretorio::abrir(&destino)?.auditar(&evento(Resultado::Sucesso))?;
    drop(trava_mantida);
    println!("revogar: estado arquivado em {} (root 0700, com a auditoria)", destino.display());
    println!(
        "Revogação concluída. Conta e dados do motor preservados; o serviço do Hub fica parado até intervenção administrativa."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revogacao_aceita_parada_comprovada_apesar_de_falha_na_solicitacao() {
        let parada = Err(anyhow::anyhow!("falha simulada"));
        assert!(verificar_parada_hub(parada, Ok(())).is_ok());
    }

    #[test]
    fn revogacao_tenta_as_duas_paradas_e_preserva_publicacao_se_desarme_falhar() {
        use std::cell::{Cell, RefCell};

        let eventos = RefCell::new(Vec::new());
        let publicacao = Cell::new(true);
        let desarme =
            super::super::slots::em_todos([super::super::slots::Slot::A, super::super::slots::Slot::B], |slot| {
                eventos.borrow_mut().push(slot.nome());
                ensure!(slot != super::super::slots::Slot::B, "falha de desarme simulada");
                Ok(())
            });
        let resultado = conter_revogacao(
            Ok(()),
            desarme,
            || {
                eventos.borrow_mut().push("wrapper");
                bail!("falha de parada simulada");
            },
            || {
                eventos.borrow_mut().push("Hub");
                Ok(())
            },
        )
        .map(|_| publicacao.set(false));
        assert_eq!(*eventos.borrow(), ["slot-a", "slot-b", "wrapper", "Hub"]);
        assert!(publicacao.get());
        assert_eq!(
            resultado.unwrap_err().to_string(),
            "revogação incompleta: desarme: slot-b: falha de desarme simulada; parada do wrapper: falha de parada simulada"
        );
    }

    #[test]
    fn revogacao_contem_falha_dupla_de_bloqueio_e_desarme() {
        use std::cell::{Cell, RefCell};

        let paradas = RefCell::new(Vec::new());
        let publicacao = Cell::new(true);
        let resultado = conter_revogacao(
            Err(anyhow::anyhow!("falha de bloqueio simulada")),
            Err::<(), _>(anyhow::anyhow!("falha de desarme simulada")),
            || {
                paradas.borrow_mut().push("wrapper");
                Ok(())
            },
            || {
                paradas.borrow_mut().push("Hub");
                Ok(())
            },
        )
        .map(|()| publicacao.set(false));
        assert_eq!(*paradas.borrow(), ["wrapper", "Hub"]);
        assert!(publicacao.get());
        assert_eq!(
            resultado.unwrap_err().to_string(),
            "revogação incompleta: desarme: bloqueio da atualização: falha de bloqueio simulada; desarme: falha de desarme simulada"
        );
    }

    fn pacote_em(dir: &Path, arquivos: &[(&str, &[u8])]) -> Result<(String, String)> {
        use std::fmt::Write as _;
        let mut somas = String::new();
        for (nome, bytes) in arquivos {
            fs::write(dir.join(nome), bytes)?;
            writeln!(somas, "{}  {nome}", Sha256::digest(bytes).hex())?;
        }
        fs::write(dir.join("SHA256SUMS"), &somas)?;
        let raiz =
            arquivos.iter().find(|(n, _)| *n == "raiz.json").map_or(String::new(), |(_, b)| Sha256::digest(b).hex());
        Ok((Sha256::digest(somas.as_bytes()).hex(), raiz))
    }

    #[test]
    fn limpeza_reconhece_somente_execucao_gerida() {
        assert!(unidade_da_tentativa(&format!("[Service]\nExecStart=\"{HUB_ATIVO}\" serve --port 1\n")));
        assert!(!unidade_da_tentativa("[Service]\nExecStart=/tmp/outro serve\n"));
        assert!(!unidade_da_tentativa(&format!("ExecStart={HUB_ATIVO} serve\nExecStart=/tmp/outro serve\n")));
    }

    /// SEG P3-4 / TEC P3-3: marca de outra tentativa desautoriza só a remoção da
    /// unidade, sem abortar o stop nem o `--reverter`; marca da tentativa autoriza.
    #[test]
    fn marca_de_outra_tentativa_nao_autoriza_remocao() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("mx365-marca-{}", uuid::Uuid::new_v4()));
        let hub = Hub { login: "h".into(), uid: 1, gid: 1, home: dir.clone() };
        assert!(!marca_da_tentativa(&hub, "t1")?, "sem marca nenhuma");
        let marcas = dir.join(".mexerica/atualizacao-privilegiada");
        fs::create_dir_all(&marcas)?;
        fs::write(marcas.join("instalacao-gerida"), b"outra")?;
        assert!(!marca_da_tentativa(&hub, "t1")?, "marca de outra tentativa");
        fs::write(marcas.join("instalacao-gerida"), b"t1")?;
        assert!(marca_da_tentativa(&hub, "t1")?);
        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn pacote_exige_fingerprints_e_somas_exatas() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("mx365-pacote-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir)?;
        let arquivos: [(&str, &[u8]); 6] = [
            ("atualizar-privilegiado", b"w"),
            ("donnabot-guarda-atualizacao", b"g"),
            ("donnabot-instalar-atualizacao", b"i"),
            ("donnabot-motor-copia", b"c"),
            ("raiz.json", b"{}"),
            ("politica.json", b"{}"),
        ];
        let (pacote, raiz) = pacote_em(&dir, &arquivos)?;
        assert!(Pacote::ler(&dir, &pacote, &raiz).is_ok());
        assert!(Pacote::ler(&dir, &"0".repeat(64), &raiz).is_err());
        assert!(Pacote::ler(&dir, &pacote, &"0".repeat(64)).is_err());
        // Binário trocado depois das somas: recusado.
        fs::write(dir.join("atualizar-privilegiado"), b"outro")?;
        assert!(Pacote::ler(&dir, &pacote, &raiz).is_err());
        // Arquivo extra listado nas somas: recusado.
        let mut com_extra = arquivos.to_vec();
        com_extra.push(("extra", b"x"));
        let (pacote, raiz) = pacote_em(&dir, &com_extra)?;
        assert!(Pacote::ler(&dir, &pacote, &raiz).is_err());
        // Pacote sem o canal do motor: recusado.
        let (pacote, raiz) = pacote_em(&dir, &[arquivos[0], arquivos[1], arquivos[2], arquivos[4], arquivos[5]])?;
        assert!(Pacote::ler(&dir, &pacote, &raiz).is_err());
        fs::remove_dir_all(dir)?;
        Ok(())
    }

    /// Instalação limpa (#812): só sem Hub legado; uma unidade que já aponta para o par
    /// gerido é a retomada de uma instalação limpa anterior.
    #[test]
    fn instalacao_limpa_recusa_hub_legado() -> Result<()> {
        let home = std::env::temp_dir().join(format!("mx365-home-{}", uuid::Uuid::new_v4()));
        let unidades = home.join(".config/systemd/user");
        fs::create_dir_all(&unidades)?;
        let hub = Hub { login: "hubteste".into(), uid: 4242, gid: 4242, home: home.clone() };
        assert!(exigir_sem_hub_legado(&hub).is_ok(), "home sem Hub deve aceitar");
        let unidade = unidades.join("donnabot-host.service");
        let binario_legado = home.join(".local/bin/donnabot-host");
        fs::write(&unidade, format!("[Service]\nExecStart={} serve --port 19422\n", binario_legado.display()))?;
        assert!(exigir_sem_hub_legado(&hub).is_err(), "unidade legada aceita");
        fs::write(&unidade, format!("[Service]\nExecStart=\"{HUB_ATIVO}\" serve --port 19422\n"))?;
        assert!(exigir_sem_hub_legado(&hub).is_ok(), "retomada da instalação limpa recusada");
        fs::create_dir_all(home.join(".local/bin"))?;
        fs::write(home.join(".local/bin/donnabot-host"), b"binario")?;
        assert!(exigir_sem_hub_legado(&hub).is_err(), "binário legado do app aceito");
        fs::remove_file(home.join(".local/bin/donnabot-host"))?;
        fs::create_dir_all(unidades.join("donnabot-host.service.d"))?;
        fs::write(unidades.join("donnabot-host.service.d/zz.conf"), b"[Service]\n")?;
        assert!(exigir_sem_hub_legado(&hub).is_err(), "drop-in legado aceito");
        fs::remove_dir_all(home)?;
        Ok(())
    }

    #[test]
    fn politica_do_pacote_nao_define_identidade_do_hub() -> Result<()> {
        let base = serde_json::json!({"endpoint":"https://exemplo.invalid/","canal":"estavel",
            "chave_manifesto":"11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=","tamanho_maximo":1024,"tempo_rede_segundos":10,
            "janela_minima":10,"janela_maxima":100,"cgroup_memoria":1024,"cgroup_processos":2,"cgroup_cpu_quota":1000,"cgroup_cpu_periodo":1000});
        let hub = Hub {
            login: "hub".into(),
            uid: 1000,
            gid: 1000,
            home: "/home/hub".into(), // vazamento-ok: conta fictícia de teste
        };
        let pacote = |valor: &serde_json::Value| Pacote {
            arquivos: BTreeMap::from([("politica.json".to_owned(), serde_json::to_vec(valor).unwrap_or_default())]),
        };
        let politica = pacote(&base).politica(&hub)?;
        assert_eq!((politica.uid_hub, politica.gid_hub), (1000, 1000));
        let mut com_uid = base.clone();
        com_uid["uid_hub"] = 0.into();
        assert!(pacote(&com_uid).politica(&hub).is_err());
        Ok(())
    }
}
