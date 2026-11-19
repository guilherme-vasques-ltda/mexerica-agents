//! MX-177: executor local com bubblewrap (Linux), só para bots do próprio dono (MX-188).
//!
//! Contém arquivos, ambiente e processos: o agente vê `/usr`, as pastas de biblioteca da raiz
//! e uma lista fixa de arquivos de `/etc`, tudo somente leitura, mais a cópia em `/trabalho`,
//! home e tmp privados e as ferramentas declaradas; não vê a home do Hub, logins, cofre nem
//! dados do Hub. PID namespace próprio: ao fim, nada sobrevive, nem `setsid`.
//! Limite: no prazo não há término gracioso; o monitor `bwrap` recebe SIGKILL na hora e o PID
//! namespace leva tudo (a carência só vale no `ProcessoLocal`). Sinalizar o comando interno
//! exigiria PID lido de `/proc`, sujeito a reuso; por isso não é feito.
//! Cópia, home, tmp e ferramentas entram por descritor aberto sem seguir links no momento de
//! montar o comando (`--bind-fd`/`--ro-bind-fd`): trocar o caminho por link não muda o que monta.
//! Limites: rede aberta e mesmo kernel; não é isolamento de kernel.
use super::{
    contrato::{Contencao, Execucao, Indisponivel, Modo, Preparo, Provedor, Tipo, admitir},
    local::validar_path,
    montagem::{Abertas, Areas, Lancador, SandboxMontagem, ambiente_base},
    processo::{Graca, Lancamento},
};
use crate::fork::private_fs::Directory;
use anyhow::{Result, anyhow, bail, ensure};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    os::{
        fd::{AsRawFd as _, RawFd},
        unix::fs::MetadataExt as _,
    },
    path::{Component, Path, PathBuf},
    process::Stdio,
};
use tokio::process::Command;

/// Última operação de montagem: deixa a raiz do sandbox somente leitura.
const REMOUNT_RO_RAIZ: [&str; 2] = ["--remount-ro", "/"];
pub(crate) const BINARIO_PADRAO: &str = "/usr/bin/bwrap";
pub(crate) const COPIA_DENTRO: &str = "/trabalho";
pub(crate) const HOME_DENTRO: &str = "/home/user";
pub(crate) const TMP_DENTRO: &str = "/tmp";
/// Entradas da raiz que espelham o host (link vira link, pasta vira montagem somente leitura).
const RAIZ_SISTEMA: [&str; 5] = ["bin", "sbin", "lib", "lib64", "lib32"];
/// Itens de `/etc` montados (somente leitura, só se existirem): o mínimo para resolver nomes,
/// confiar em certificados públicos e achar bibliotecas. Nunca `/etc/ssl`, `/etc/pki` nem
/// `/etc` inteiros: podem guardar chaves privadas e credenciais.
const ETC_ITENS: [&str; 15] = [
    "/etc/ssl/certs",
    "/etc/pki/tls/certs",
    "/etc/pki/ca-trust/extracted",
    "/etc/ca-certificates.conf",
    "/etc/resolv.conf",
    "/etc/hosts",
    "/etc/nsswitch.conf",
    "/etc/passwd",
    "/etc/group",
    "/etc/localtime",
    "/etc/ld.so.cache",
    "/etc/ld.so.conf",
    "/etc/ld.so.conf.d",
    "/etc/alternatives",
    "/etc/gitconfig",
];
/// Raízes onde uma ferramenta pode morar fora da home do Hub. Nada de `/run`, `/var`, `/srv`,
/// `/mnt`, `/media`, `/root`, `/sys`, `/proc`, `/dev`, `/tmp`, `/etc` nem `/home` de outros.
const PREFIXOS_FERRAMENTA: [&str; 4] = ["/usr", "/opt", "/nix/store", "/snap"];
/// Nomes de pasta que nunca entram como ferramenta: guardam logins e chaves.
const NOMES_PROIBIDOS: [&str; 10] = [
    ".ssh",
    ".gnupg",
    ".aws",
    ".config",
    ".docker",
    ".kube",
    ".netrc",
    ".password-store",
    ".git-credentials",
    "Library",
];
/// Destinos internos que uma ferramenta não pode cobrir.
const DESTINOS_RESERVADOS: [&str; 5] = ["/proc", "/dev", TMP_DENTRO, COPIA_DENTRO, HOME_DENTRO];

pub(crate) struct Bwrap {
    raiz: PathBuf,
    plano: Plano,
}

/// O que cada execução monta. Escolhido pela configuração do Hub, nunca pelo texto do bot.
#[derive(Clone, Debug)]
pub(crate) struct Plano {
    binario: PathBuf,
    path: OsString,
    /// Raiz das áreas privadas (a sonda cria sua pasta temporária aqui).
    raiz: PathBuf,
    /// Pastas extras do toolchain montadas somente leitura no mesmo caminho.
    ferramentas: Vec<PathBuf>,
}

impl Bwrap {
    pub(crate) fn new(raiz: PathBuf, binario: PathBuf, path: OsString, ferramentas: Vec<PathBuf>) -> Result<Self> {
        ensure!(raiz.is_absolute() && binario.is_absolute(), "caminho_relativo");
        validar_path(&path)?;
        let home = dirs::home_dir();
        let dados = crate::service::data_dir();
        for ferramenta in &ferramentas {
            validar_ferramenta(ferramenta, home.as_deref(), &dados, &raiz)?;
        }
        conferir_montagens(&montagens_fixas(&raiz_do_host()), home.as_deref(), &dados, &raiz)?;
        Ok(Self { plano: Plano { binario, path, raiz: raiz.clone(), ferramentas }, raiz })
    }
    pub(crate) fn plano(&self) -> &Plano {
        &self.plano
    }

    /// Sonda real com as mesmas capacidades do lançamento: `--bind-fd` e `--ro-bind-fd` sobre
    /// um descritor real de pasta privada (CLOEXEC limpo só no filho), mais os isolamentos.
    /// Exige bubblewrap 0.10 ou mais novo (0.9 ou menos não têm as opções por descritor).
    fn sondar(&self, host: &[(&str, Entrada)], fixas: &Fixas) -> Result<(), Indisponivel> {
        let caminho = self.raiz.join(format!("sonda_{}", uuid::Uuid::new_v4().simple()));
        let pasta = Directory::open(&caminho, true).map_err(|e| {
            indisponivel(format!("raiz dos sandboxes recusada: {e}"), Some("crie a raiz com modo 0700 e dono do Hub"))
        })?;
        // O bwrap fecha cada descritor após usá-lo: cada opção recebe o seu.
        let segunda = Directory::open(&caminho, false).map_err(|e| {
            indisponivel(format!("raiz dos sandboxes recusada: {e}"), Some("crie a raiz com modo 0700 e dono do Hub"))
        })?;
        let fds = [pasta.0.as_raw_fd(), segunda.0.as_raw_fd()];
        let resultado = if fds.iter().all(|fd| *fd > 2) && fds[0] != fds[1] {
            let mut sonda = base(host, fixas);
            for ((opcao, destino), fd) in
                [("--bind-fd", "/trabalho"), ("--ro-bind-fd", "/sonda-ro")].into_iter().zip(fds)
            {
                sonda.extend([opcao.into(), fd.to_string().into(), destino.into()]);
            }
            sonda.extend(REMOUNT_RO_RAIZ.map(OsString::from));
            sonda.extend(["--", "/usr/bin/true"].map(OsString::from));
            let mut command = std::process::Command::new(&self.plano.binario);
            command.args(sonda).env_clear().current_dir("/").stdin(Stdio::null());
            // SAFETY: instalado uma única vez; `pasta` e `segunda` vivem até o fim da sonda.
            unsafe { herdar_no_filho(&mut command, fds.to_vec()) };
            command
                .output()
                .map_err(|e| indisponivel(format!("bubblewrap não executou: {e}"), Some("apt install bubblewrap")))
        } else {
            Err(indisponivel("descritor reservado na sonda", None))
        };
        drop((pasta, segunda));
        let _ = std::fs::remove_dir(&caminho);
        let saida = resultado?;
        if saida.status.success() { Ok(()) } else { Err(diagnosticar(&String::from_utf8_lossy(&saida.stderr))) }
    }
}

/// Cada caminho em forma textual e resolvida (links na configuração do Hub).
fn resolvidos<'a>(caminhos: impl IntoIterator<Item = &'a Path>) -> Vec<PathBuf> {
    let mut lista = Vec::new();
    for caminho in caminhos {
        lista.push(caminho.to_path_buf());
        if let Ok(resolvido) = caminho.canonicalize() {
            lista.push(resolvido);
        }
    }
    lista
}

/// Montagens fixas (sistema): nenhuma pode sobrepor dados do Hub, home do Hub nem áreas
/// privadas, em nenhum dos dois sentidos. Sobreposição é erro, nunca é pulada.
pub(crate) fn conferir_montagens(montagens: &[PathBuf], home: Option<&Path>, dados: &Path, raiz: &Path) -> Result<()> {
    for protegido in resolvidos(home.into_iter().chain([dados, raiz])) {
        for montagem in montagens {
            ensure!(
                !(montagem.starts_with(&protegido) || protegido.starts_with(montagem)),
                "montagem_expoe_dados: {} sobrepõe {}",
                montagem.display(),
                protegido.display()
            );
        }
    }
    Ok(())
}

/// Origens das montagens fixas de [`base`].
pub(crate) fn montagens_fixas(raiz: &[(&str, Entrada)]) -> Vec<PathBuf> {
    let mut lista = vec![PathBuf::from("/usr")];
    lista.extend(raiz.iter().filter(|(_, e)| *e == Entrada::Pasta).map(|(nome, _)| Path::new("/").join(nome)));
    lista.extend(ETC_ITENS.map(PathBuf::from));
    lista
}

/// Origem fixa já resolvida (sem links) e o destino dentro do sandbox.
pub(crate) type Fixas = Vec<(PathBuf, PathBuf)>;

/// O bwrap segue links na origem. Cada fonte existente é resolvida, conferida contra dados,
/// home e raiz (também resolvidos) e montada pelo caminho RESOLVIDO. A origem e todos os seus
/// ancestrais não podem ser do usuário do Hub nem gravável por terceiros sem sticky: assim
/// nenhum concorrente com o mesmo UID troca o objeto entre a conferência e a montagem. Fonte
/// que existe e não passa é erro (não é pulada); só a ausente é ignorada.
pub(crate) fn resolver_fixas(fontes: &[PathBuf], home: Option<&Path>, dados: &Path, raiz: &Path) -> Result<Fixas> {
    let dono = crate::fork::private_fs::uid();
    let mut fixas = Vec::new();
    for fonte in fontes {
        let resolvida = match fonte.canonicalize() {
            Ok(resolvida) => resolvida,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => bail!("montagem_ilegivel: {}: {e}", fonte.display()),
        };
        conferir_montagens(std::slice::from_ref(&resolvida), home, dados, raiz)?;
        for ancestral in resolvida.ancestors() {
            let meta = std::fs::symlink_metadata(ancestral)
                .map_err(|e| anyhow!("montagem_ilegivel: {}: {e}", ancestral.display()))?;
            let (modo, sticky) = (meta.mode(), meta.mode() & 0o1000 != 0);
            ensure!(
                meta.uid() != dono && (modo & 0o022 == 0 || sticky),
                "montagem_insegura: {} (origem de {}) é do usuário do Hub ou gravável por terceiros",
                ancestral.display(),
                fonte.display()
            );
        }
        fixas.push((resolvida, fonte.clone()));
    }
    ensure!(
        !fontes.iter().any(|f| f == Path::new("/usr")) || fixas.iter().any(|(_, d)| d == Path::new("/usr")),
        "montagem_ausente: /usr"
    );
    Ok(fixas)
}

/// Ferramenta só mora em raiz da lista (ou ≥2 níveis dentro da home do Hub) e não pode expor
/// home, dados do Hub, áreas privadas nem pastas de login. Validação de configuração: o
/// que é montado de fato é aberto sem seguir links em cada execução ([`abrir_ferramenta`]).
pub(crate) fn validar_ferramenta(f: &Path, home: Option<&Path>, dados: &Path, raiz: &Path) -> Result<()> {
    ensure!(
        f.is_absolute() && f.components().skip(1).all(|c| matches!(c, Component::Normal(_))),
        "ferramenta_invalida"
    );
    ensure!(f.components().count() > 1, "ferramenta_invalida");
    // Link no caminho muda o que o bwrap montaria; a execução também recusa (O_NOFOLLOW).
    if let Ok(resolvido) = f.canonicalize() {
        ensure!(resolvido == f, "ferramenta_com_link");
    }
    let dentro_da_home = home.and_then(|h| f.strip_prefix(h).ok());
    // Dentro da home, só subpastas (ex.: ~/.cargo/bin), nunca a pasta inteira de um app
    // (~/.cargo guarda credentials.toml).
    if let Some(dentro) = dentro_da_home {
        ensure!(dentro.components().count() >= 2, "ferramenta_expoe_home");
    } else {
        ensure!(PREFIXOS_FERRAMENTA.iter().any(|p| f.starts_with(p) && f != Path::new(p)), "ferramenta_fora_da_lista");
    }
    for protegido in resolvidos([dados, raiz]) {
        ensure!(!(f.starts_with(&protegido) || protegido.starts_with(f)), "ferramenta_expoe_dados");
    }
    // Dentro da home é permitido (regra acima); cobrir a home por cima, não.
    ensure!(!resolvidos(home).iter().any(|h| h.starts_with(f)), "ferramenta_expoe_home");
    ensure!(
        !DESTINOS_RESERVADOS.iter().any(|d| f.starts_with(d) || Path::new(d).starts_with(f)),
        "ferramenta_destino_reservado"
    );
    ensure!(
        !f.components().any(|c| NOMES_PROIBIDOS.iter().any(|n| c.as_os_str() == OsStr::new(n))),
        "ferramenta_expoe_login"
    );
    Ok(())
}

/// Abre a pasta da ferramenta sem seguir links (nenhum componente pode ser link, nenhum
/// ancestral gravável por terceiros): dona o usuário do Hub (ex.: `~/.cargo/bin`) ou root.
pub(crate) fn abrir_ferramenta(f: &Path) -> Result<File> {
    let dir = Directory::open(f, false).or_else(|_| Directory::for_owner(f, false, 0, None));
    Ok(dir.map_err(|e| anyhow!("ferramenta_recusada: {}: {e}", f.display()))?.0)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Entrada {
    Link(PathBuf),
    Pasta,
    Ausente,
}

pub(crate) fn raiz_do_host() -> Vec<(&'static str, Entrada)> {
    RAIZ_SISTEMA
        .iter()
        .map(|nome| {
            let caminho = Path::new("/").join(nome);
            let entrada = match std::fs::symlink_metadata(&caminho) {
                Ok(m) if m.file_type().is_symlink() => {
                    std::fs::read_link(&caminho).map_or(Entrada::Ausente, Entrada::Link)
                }
                Ok(m) if m.is_dir() => Entrada::Pasta,
                _ => Entrada::Ausente,
            };
            (*nome, entrada)
        })
        .collect()
}

/// Montagens comuns à sonda e às execuções: `/usr`, pastas da raiz e itens de `/etc` já
/// resolvidos ([`resolver_fixas`]); links da raiz do host viram links.
fn base(raiz: &[(&str, Entrada)], fixas: &Fixas) -> Vec<OsString> {
    let mut a: Vec<OsString> = [
        "--die-with-parent",
        "--new-session",
        "--unshare-user",
        "--disable-userns",
        "--unshare-pid",
        "--unshare-ipc",
        "--unshare-uts",
        "--unshare-cgroup-try",
        "--cap-drop",
        "ALL",
    ]
    .map(OsString::from)
    .into();
    for (nome, entrada) in raiz {
        if let Entrada::Link(alvo) = entrada {
            a.extend(["--symlink".into(), alvo.into(), Path::new("/").join(nome).into()]);
        }
    }
    for (origem, destino) in fixas {
        a.extend(["--ro-bind".into(), origem.into(), destino.into()]);
    }
    a.extend(["--proc", "/proc", "--dev", "/dev"].map(OsString::from));
    a
}

/// Descritores que o bwrap herda, já abertos e conferidos (valores só para montar o argv).
pub(crate) struct Fds {
    pub(crate) copia: RawFd,
    pub(crate) home: RawFd,
    pub(crate) tmp: RawFd,
    pub(crate) ferramentas: Vec<(PathBuf, RawFd)>,
}
impl Fds {
    fn todos(&self) -> Vec<RawFd> {
        [self.copia, self.home, self.tmp].into_iter().chain(self.ferramentas.iter().map(|(_, fd)| *fd)).collect()
    }
}

/// Limpa o `FD_CLOEXEC` dos descritores no filho (entre fork e exec); o pai os mantém com
/// CLOEXEC, então nenhum outro spawn do Hub os herda.
///
/// # Safety
/// Só pode ser instalado em `pre_exec`, com descritores vivos até o spawn.
unsafe fn herdar_no_filho<C: std::os::unix::process::CommandExt>(command: &mut C, fds: Vec<RawFd>) {
    // SAFETY: o closure só chama fcntl (async-signal-safe) e não aloca; os descritores são
    // mantidos vivos pelo chamador até o spawn.
    unsafe {
        command.pre_exec(move || {
            for fd in &fds {
                if libc::fcntl(*fd, libc::F_SETFD, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
}

impl Plano {
    pub(crate) fn argumentos(fds: &Fds, execucao: &Execucao, raiz: &[(&str, Entrada)], fixas: &Fixas) -> Vec<OsString> {
        let mut a = base(raiz, fixas);
        for (fd, destino) in [(fds.tmp, TMP_DENTRO), (fds.home, HOME_DENTRO), (fds.copia, COPIA_DENTRO)] {
            a.extend(["--bind-fd".into(), fd.to_string().into(), destino.into()]);
        }
        for (f, fd) in &fds.ferramentas {
            a.extend(["--ro-bind-fd".into(), fd.to_string().into(), f.into()]);
        }
        // Raiz inteira só leitura depois de TODAS as montagens (a raiz tmpfs do bwrap é gravável
        // e `/etc`, `/opt` etc. nascem nela). Cópia, home e tmp seguem graváveis: são binds próprios.
        a.extend(REMOUNT_RO_RAIZ.map(OsString::from));
        a.extend(["--chdir".into(), Path::new(COPIA_DENTRO).join(&execucao.pasta).into(), "--".into()]);
        a.extend(execucao.argv.iter().cloned());
        a
    }

    /// Abre agora, sem seguir links, cada origem (cópia, home, tmp e ferramentas) e passa os
    /// descritores ao bwrap. O CLOEXEC só é limpo no filho (`pre_exec`): o pai não cria
    /// descritor herdável que outros spawns do Hub levariam.
    pub(crate) fn comando(&self, abertas: Abertas, execucao: &Execucao) -> Result<Lancamento> {
        // `pasta` relativa que é link (ou some) é recusada antes de lançar.
        super::montagem::abrir_pasta(&abertas.copia, &execucao.pasta)?;
        let mut manter = vec![abertas.copia, abertas.home, abertas.tmp];
        let mut ferramentas = Vec::new();
        for f in &self.ferramentas {
            let dir = abrir_ferramenta(f)?;
            ferramentas.push((f.clone(), dir.as_raw_fd()));
            manter.push(dir);
        }
        let fds =
            Fds { copia: manter[0].as_raw_fd(), home: manter[1].as_raw_fd(), tmp: manter[2].as_raw_fd(), ferramentas };
        let herdar = fds.todos();
        // fd ≤ 2 colidiria com stdio do filho.
        ensure!(herdar.iter().all(|fd| *fd > 2), "fd_reservado");
        // Origens fixas resolvidas e conferidas agora, a cada execução.
        let (home, dados) = (dirs::home_dir(), crate::service::data_dir());
        let host = raiz_do_host();
        let fixas = resolver_fixas(&montagens_fixas(&host), home.as_deref(), &dados, &self.raiz)?;
        let mut command = Command::new(&self.binario);
        command
            .args(Self::argumentos(&fds, execucao, &host, &fixas))
            .env_clear()
            .envs(ambiente_base(Path::new(HOME_DENTRO), Path::new(TMP_DENTRO), &self.path))
            .envs(execucao.env.iter().map(|(k, v)| (k, v)))
            .current_dir("/");
        // SAFETY: instalado uma única vez; `manter` mantém os descritores vivos até o spawn.
        unsafe { herdar_no_filho(command.as_std_mut(), herdar) };
        Ok(Lancamento { command, manter, graca: Graca::Nenhuma })
    }
}

fn indisponivel(motivo: impl Into<String>, correcao: Option<&str>) -> Indisponivel {
    Indisponivel { tipo: Tipo::Bwrap, motivo: motivo.into(), correcao: correcao.map(Into::into) }
}

/// Traduz a falha da sonda em passo de correção; nunca troca de executor.
pub(crate) fn diagnosticar(stderr: &str) -> Indisponivel {
    let linha: String = stderr.lines().next().unwrap_or("falha sem mensagem").chars().take(200).collect();
    let correcao = if stderr.contains("overflowuid") || stderr.contains("/proc/sys") {
        "rode o Hub num usuário com /proc completo (sem hidepid/subset=pid)"
    } else if stderr.contains("Unknown option") || stderr.contains("Unrecognized") {
        "atualize o bubblewrap para 0.10 ou mais novo"
    } else if stderr.contains("uid map") || stderr.contains("Operation not permitted") || stderr.contains("namespace") {
        "habilite user namespaces sem privilégio (sysctl kernel.unprivileged_userns_clone=1 ou perfil AppArmor do bwrap)"
    } else {
        "confira a instalação do bubblewrap (apt install bubblewrap)"
    };
    indisponivel(linha, Some(correcao))
}

#[allow(clippy::unused_async_trait_impl, reason = "contrato async para provedores remotos; montagem local é síncrona")]
impl Provedor for Bwrap {
    type Sandbox = SandboxMontagem;
    fn tipo(&self) -> Tipo {
        Tipo::Bwrap
    }
    fn modo(&self) -> Modo {
        Modo::Montagem
    }
    fn contencao(&self) -> Contencao {
        Contencao::ArquivosEAmbiente
    }
    fn verificar(&self) -> Result<(), Indisponivel> {
        if crate::fork::private_fs::uid() == 0 {
            return Err(indisponivel("Hub como root não é admitido", Some("rode o Hub como usuário comum")));
        }
        if !self.plano.binario.is_file() {
            return Err(indisponivel("bubblewrap ausente", Some("apt install bubblewrap")));
        }
        for f in &self.plano.ferramentas {
            if !f.is_dir() {
                return Err(indisponivel(format!("ferramenta ausente: {}", f.display()), None));
            }
        }
        let host = raiz_do_host();
        let (home, dados) = (dirs::home_dir(), crate::service::data_dir());
        let fixas = resolver_fixas(&montagens_fixas(&host), home.as_deref(), &dados, &self.raiz).map_err(|e| {
            indisponivel(
                e.to_string(),
                Some("mova MEXERICA_HOME e a raiz dos sandboxes para fora das pastas de sistema; origens montadas devem ser de root"),
            )
        })?;
        self.sondar(&host, &fixas)
    }
    async fn preparar(&self, preparo: &Preparo) -> Result<SandboxMontagem> {
        admitir(preparo.confianca, self.contencao())?;
        self.verificar()?;
        let areas = Areas::criar(&self.raiz, &preparo.copia)?;
        Ok(SandboxMontagem::new(areas, Lancador::Bwrap(self.plano.clone())))
    }
}
