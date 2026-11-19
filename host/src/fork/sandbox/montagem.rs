//! Sandbox de montagem: a cópia vive no host; home e tmp privados são descartáveis.
//! Ciclo de vida, cópias e limpeza são iguais para todo provedor de montagem;
//! só o lançamento do processo muda (direto, bwrap ou Seatbelt).
use super::{
    contrato::{Estado, Execucao, Passo, Saida, Sandbox, avancar, relativo},
    copia, processo,
};
use crate::fork::private_fs::Directory;
use anyhow::{Context as _, Result, bail, ensure};
use std::{
    ffi::{CStr, CString, OsString},
    fs::File,
    os::{
        fd::{AsRawFd as _, FromRawFd as _, IntoRawFd as _},
        unix::{ffi::OsStrExt as _, fs::MetadataExt as _},
    },
    path::{Path, PathBuf},
};

/// Identidade de um diretório: (dispositivo, inode).
type Identidade = (u64, u64);

/// Áreas de um sandbox de montagem: cópia do Hub e home/tmp privados descartáveis.
pub(crate) struct Areas {
    pub(crate) id: String,
    pub(crate) copia: PathBuf,
    pub(crate) privada: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) tmp: PathBuf,
    /// Identidades conferidas no preparo (cópia, home, tmp, pasta privada); cada uso reabre e
    /// confere de novo.
    identidades: [Identidade; 4],
}

/// Origens abertas sem seguir links no momento de montar o comando. Os descritores
/// seguram o objeto conferido: trocar o caminho depois não muda o que é montado.
pub(crate) struct Abertas {
    pub(crate) copia: File,
    pub(crate) home: File,
    pub(crate) tmp: File,
}

fn identidade(arquivo: &File) -> Result<Identidade> {
    let meta = arquivo.metadata()?;
    Ok((meta.dev(), meta.ino()))
}

impl Areas {
    pub(crate) fn criar(raiz: &Path, copia: &Path) -> Result<Self> {
        ensure!(copia.is_absolute(), "copia_relativa");
        let dir_copia = Directory::open(copia, false).context("copia_recusada")?;
        let id = format!("sb_{}", uuid::Uuid::new_v4().simple());
        let privada = raiz.join(&id);
        let (home, tmp) = (privada.join("home"), privada.join("tmp"));
        let dir_home = Directory::open(&home, true)?;
        let dir_tmp = Directory::open(&tmp, true)?;
        let dir_privada = Directory::open(&privada, false)?;
        let identidades =
            [identidade(&dir_copia.0)?, identidade(&dir_home.0)?, identidade(&dir_tmp.0)?, identidade(&dir_privada.0)?];
        Ok(Self { id, copia: copia.to_path_buf(), privada, home, tmp, identidades })
    }

    /// Reabre a cópia sem seguir links e confere que é o mesmo objeto do preparo.
    pub(crate) fn abrir_copia(&self) -> Result<File> {
        self.reabrir(&self.copia, 0, "copia_trocada")
    }

    /// Reabre cópia, home e tmp sem seguir links e confere que são os mesmos objetos do
    /// preparo. Link ou troca entre preparar e usar vira erro, nunca outro alvo.
    pub(crate) fn abrir(&self) -> Result<Abertas> {
        Ok(Abertas {
            copia: self.abrir_copia()?,
            home: self.reabrir(&self.home, 1, "home_trocada")?,
            tmp: self.reabrir(&self.tmp, 2, "tmp_trocada")?,
        })
    }

    fn reabrir(&self, caminho: &Path, indice: usize, erro: &'static str) -> Result<File> {
        let dir = Directory::open(caminho, false).map_err(|e| anyhow::anyhow!("{erro}: {e}"))?;
        ensure!(identidade(&dir.0)? == self.identidades[indice], "{erro}: trocada depois do preparo");
        Ok(dir.0)
    }

    /// Remove a pasta privada relativamente a descritores: abre sem seguir link, confere a
    /// identidade do preparo e apaga com `unlinkat`, nunca por caminho. Pasta substituída é
    /// erro e nada é apagado; qualquer falha deixa a limpeza repetível.
    pub(crate) fn apagar(&self) -> Result<()> {
        let (Some(pai), Some(nome)) = (self.privada.parent(), self.privada.file_name()) else {
            bail!("sandbox_limpeza_falhou: caminho sem pai");
        };
        let pai = match Directory::open(pai, false) {
            Ok(pai) => pai.0,
            // Só a ausência comprovada (ENOENT) conta como limpo; erro de acesso sobe.
            Err(_) if ausente(pai)? => return Ok(()),
            Err(e) => return Err(e.context("sandbox_limpeza_falhou")),
        };
        let nome = CString::new(nome.as_bytes())?;
        let Some(atual) = stat_sem_seguir(&pai, &nome).context("sandbox_limpeza_falhou")? else { return Ok(()) };
        ensure!(
            id_de(&atual) == self.identidades[3],
            "privada_trocada: a pasta privada não é a do preparo; nada foi apagado"
        );
        let privada = abrir_filho(&pai, &nome).context("sandbox_limpeza_falhou")?;
        ensure!(identidade(&privada)? == self.identidades[3], "privada_trocada: nada foi apagado");
        esvaziar(&privada, 0)?;
        // Só some se ainda for o mesmo objeto (e vazio): `AT_REMOVEDIR` nunca apaga conteúdo.
        ensure!(
            stat_sem_seguir(&pai, &nome)?.is_some_and(|st| id_de(&st) == self.identidades[3]),
            "privada_trocada: nada mais foi apagado"
        );
        // SAFETY: descritor do pai vivo e nome terminado em NUL.
        let rc = unsafe { libc::unlinkat(pai.as_raw_fd(), nome.as_ptr(), libc::AT_REMOVEDIR) };
        ensure!(rc == 0, "sandbox_limpeza_falhou: {}", std::io::Error::last_os_error());
        Ok(())
    }
}

#[allow(clippy::unnecessary_cast, reason = "st_dev/st_ino variam de tipo entre Linux e macOS")]
fn id_de(st: &libc::stat) -> Identidade {
    (st.st_dev as u64, st.st_ino as u64)
}

/// `Ok(None)` só para ENOENT; qualquer outro erro (ex.: EACCES) sobe com contexto.
fn stat_sem_seguir(dir: &File, nome: &CStr) -> Result<Option<libc::stat>> {
    // SAFETY: stat é dado simples; zerado e preenchido pelo fstatat.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: descritor vivo, nome terminado em NUL, sem seguir link.
    let ok = unsafe { libc::fstatat(dir.as_raw_fd(), nome.as_ptr(), &raw mut st, libc::AT_SYMLINK_NOFOLLOW) };
    if ok == 0 {
        return Ok(Some(st));
    }
    let erro = std::io::Error::last_os_error();
    if erro.kind() == std::io::ErrorKind::NotFound {
        return Ok(None);
    }
    Err(anyhow::Error::new(erro).context("consultando entrada sem seguir link"))
}

/// Ausência comprovada do caminho; erro de acesso não conta como ausente.
fn ausente(caminho: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(caminho) {
        Ok(_) => Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(anyhow::Error::new(e).context("sandbox_limpeza_falhou")),
    }
}

/// Mesma política de `private_fs` para cada pasta atravessada: dono do Hub (ou root com
/// sticky) e sem escrita para grupo ou terceiros.
fn conferir_pasta(pasta: &File) -> Result<()> {
    let meta = pasta.metadata()?;
    let dono = crate::fork::private_fs::uid();
    ensure!(meta.is_dir() && (meta.uid() == dono || meta.uid() == 0), "pasta_da_copia_de_outro_dono");
    ensure!(
        meta.mode() & 0o022 == 0 || (meta.uid() == 0 && meta.mode() & 0o1000 != 0),
        "pasta_da_copia_gravavel_por_terceiros"
    );
    Ok(())
}

/// Abre o filho como pasta, sem seguir link.
fn abrir_filho(dir: &File, nome: &CStr) -> Result<File> {
    // SAFETY: descritor vivo e nome terminado em NUL; O_NOFOLLOW|O_DIRECTORY recusam link e arquivo.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            nome.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    ensure!(fd >= 0, "{}", std::io::Error::last_os_error());
    // SAFETY: fd recém-aberto, transferido uma única vez.
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// Abre `pasta` (relativa à cópia) componente a componente com `O_NOFOLLOW`, ancorada no
/// descritor da cópia: link em qualquer componente é recusado. Com `criar`, pastas que faltam
/// nascem 0700 (`mkdirat` relativo ao descritor).
pub(crate) fn abrir_relativa(copia: &File, pasta: &Path, criar: bool) -> Result<File> {
    relativo(pasta).map_err(|_| anyhow::anyhow!("pasta_fora_da_copia"))?;
    let mut atual = copia.try_clone().context("pasta_fora_da_copia")?;
    for parte in pasta.components() {
        let nome = CString::new(parte.as_os_str().as_bytes()).context("pasta_fora_da_copia")?;
        let mut aberta = abrir_filho(&atual, &nome);
        if aberta.is_err() && criar && stat_sem_seguir(&atual, &nome)?.is_none() {
            // SAFETY: descritor vivo e nome terminado em NUL; modo privado desde o nascimento.
            let rc = unsafe { libc::mkdirat(atual.as_raw_fd(), nome.as_ptr(), 0o700) };
            ensure!(rc == 0, "criando pasta na cópia: {}", std::io::Error::last_os_error());
            aberta = abrir_filho(&atual, &nome);
        }
        atual = aberta.map_err(|e| anyhow::anyhow!("pasta_fora_da_copia: {e}"))?;
        conferir_pasta(&atual)?;
    }
    Ok(atual)
}

/// Abre `pasta` sem criar nada (execução e exportação).
pub(crate) fn abrir_pasta(copia: &File, pasta: &Path) -> Result<File> {
    abrir_relativa(copia, pasta, false)
}

/// Esvazia a pasta aberta, relativamente a ela e sem seguir links: pastas próprias sem
/// permissão ganham u+rwx (`fchmodat` com `AT_SYMLINK_NOFOLLOW`), cada filho é aberto com
/// `O_NOFOLLOW` e conferido contra o `fstatat`, e tudo sai por `unlinkat`.
fn esvaziar(dir: &File, nivel: u32) -> Result<()> {
    ensure!(nivel < 256, "sandbox_limpeza_profunda");
    let dono = crate::fork::private_fs::uid();
    let copia = dir.try_clone()?.into_raw_fd();
    // SAFETY: `copia` é um dup recém-criado; fdopendir assume a posse dele.
    let listagem = unsafe { libc::fdopendir(copia) };
    if listagem.is_null() {
        let erro = std::io::Error::last_os_error();
        // SAFETY: fdopendir falhou e não assumiu `copia`; fecha uma única vez.
        unsafe { libc::close(copia) };
        return Err(erro.into());
    }
    let mut nomes = Vec::new();
    loop {
        // SAFETY: `listagem` válida até o closedir abaixo.
        let entrada = unsafe { libc::readdir(listagem) };
        if entrada.is_null() {
            break;
        }
        // SAFETY: d_name é string C terminada em NUL dentro da entrada devolvida.
        let nome = unsafe { CStr::from_ptr((*entrada).d_name.as_ptr()) }.to_owned();
        if nome.to_bytes() != b"." && nome.to_bytes() != b".." {
            nomes.push(nome);
        }
    }
    // SAFETY: `listagem` aberta acima; fecha também o dup.
    unsafe { libc::closedir(listagem) };
    for nome in nomes {
        let Some(st) = stat_sem_seguir(dir, &nome).context("sandbox_limpeza_falhou")? else { continue };
        let mut flags = 0;
        if st.st_mode & libc::S_IFMT == libc::S_IFDIR {
            flags = libc::AT_REMOVEDIR;
            let filho = abrir_pasta_para_limpar(dir, &nome, &st, dono)?;
            esvaziar(&filho, nivel + 1)?;
        }
        // SAFETY: descritor vivo e nome terminado em NUL; nunca segue link.
        let rc = unsafe { libc::unlinkat(dir.as_raw_fd(), nome.as_ptr(), flags) };
        if rc != 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::NotFound {
            bail!("sandbox_limpeza_falhou: {}", std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Abre a subpasta a esvaziar e, se for nossa e faltar u+rwx, ajusta a permissão PELO
/// DESCRITOR do objeto já conferido (nunca pelo nome: entre conferir e mudar, o nome poderia
/// virar hardlink para um arquivo sensível). Pasta sem leitura (ex.: 000) só se abre por
/// `O_PATH` (Linux); em outros sistemas a limpeza falha com erro, sem tocar no nome.
fn abrir_pasta_para_limpar(dir: &File, nome: &CStr, st: &libc::stat, dono: u32) -> Result<File> {
    let falta = st.st_uid == dono && st.st_mode & 0o700 != 0o700;
    let modo = (st.st_mode & 0o7777) | 0o700;
    let filho = match abrir_filho(dir, nome) {
        Ok(filho) => {
            ensure!(identidade(&filho)? == id_de(st), "privada_trocada: nada mais foi apagado");
            if falta {
                // SAFETY: descritor vivo do objeto conferido; fchmod não resolve nome.
                let rc = unsafe { libc::fchmod(filho.as_raw_fd(), modo) };
                ensure!(rc == 0, "sandbox_limpeza_falhou: {}", std::io::Error::last_os_error());
            }
            filho
        }
        #[cfg(target_os = "linux")]
        Err(_) if falta => {
            liberar_por_o_path(dir, nome, st, modo)?;
            abrir_filho(dir, nome).context("sandbox_limpeza_falhou")?
        }
        Err(e) => return Err(e.context("sandbox_limpeza_falhou")),
    };
    ensure!(identidade(&filho)? == id_de(st), "privada_trocada: nada mais foi apagado");
    Ok(filho)
}

/// Linux: `O_PATH|O_NOFOLLOW|O_DIRECTORY` abre a pasta sem permissão de leitura e garante que
/// é diretório (diretório não aceita hardlink); confere a identidade e muda a permissão pelo
/// próprio descritor via `/proc/self/fd/N`.
#[cfg(target_os = "linux")]
fn liberar_por_o_path(dir: &File, nome: &CStr, st: &libc::stat, modo: libc::mode_t) -> Result<()> {
    // SAFETY: descritor vivo e nome terminado em NUL; O_PATH não lê conteúdo.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            nome.as_ptr(),
            libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    ensure!(fd >= 0, "sandbox_limpeza_falhou: {}", std::io::Error::last_os_error());
    // SAFETY: fd recém-aberto, transferido uma única vez.
    let caminho = unsafe { File::from_raw_fd(fd) };
    ensure!(identidade(&caminho)? == id_de(st), "privada_trocada: nada mais foi apagado");
    let alvo = CString::new(format!("/proc/self/fd/{}", caminho.as_raw_fd()))?;
    // SAFETY: caminho mágico do próprio descritor conferido; vivo até o fim desta função.
    let rc = unsafe { libc::chmod(alvo.as_ptr(), modo) };
    ensure!(rc == 0, "sandbox_limpeza_falhou: {}", std::io::Error::last_os_error());
    Ok(())
}

/// Ambiente mínimo do provedor; tudo o mais vem explícito de [`Execucao::env`].
pub(crate) fn ambiente_base(home: &Path, tmp: &Path, path: &OsString) -> Vec<(OsString, OsString)> {
    vec![
        ("HOME".into(), home.into()),
        ("TMPDIR".into(), tmp.into()),
        ("PATH".into(), path.clone()),
        ("LANG".into(), "C.UTF-8".into()),
    ]
}

/// Como o processo de cada execução é lançado.
pub(crate) enum Lancador {
    Local {
        path: OsString,
    },
    #[cfg(target_os = "linux")]
    Bwrap(super::bwrap::Plano),
}

pub(crate) struct SandboxMontagem {
    areas: Areas,
    lancador: Lancador,
    estado: Estado,
}

impl SandboxMontagem {
    pub(crate) fn new(areas: Areas, lancador: Lancador) -> Self {
        Self { areas, lancador, estado: Estado::Preparado }
    }
}

impl Drop for SandboxMontagem {
    fn drop(&mut self) {
        // Sandbox abandonado não deixa home/tmp privados para trás.
        if self.estado != Estado::Encerrado {
            let _ = self.areas.apagar();
        }
    }
}

#[allow(clippy::unused_async_trait_impl, reason = "contrato async para provedores remotos; montagem local é síncrona")]
impl Sandbox for SandboxMontagem {
    fn id(&self) -> &str {
        &self.areas.id
    }
    fn estado(&self) -> Estado {
        self.estado
    }
    fn copia(&self) -> &Path {
        &self.areas.copia
    }
    async fn executar(&mut self, execucao: &Execucao) -> Result<Saida> {
        execucao.validar()?;
        let executando = avancar(self.estado, Passo::Executar)?;
        // Origens abertas e conferidas só agora, sem seguir links; falha aqui não executa nada.
        let abertas = self.areas.abrir()?;
        let lancamento = match &self.lancador {
            Lancador::Local { path } => super::local::comando(&abertas, &self.areas, path, execucao)?,
            #[cfg(target_os = "linux")]
            Lancador::Bwrap(plano) => plano.comando(abertas, execucao)?,
        };
        // Pessimista: se a future for cancelada daqui em diante, o estado não fica Executando
        // (o guarda de `rodar` já matou o grupo) e o sandbox só admite encerrar.
        self.estado = avancar(executando, Passo::Falha)?;
        let saida = processo::rodar(lancamento, execucao.limites).await?;
        self.estado = avancar(executando, Passo::FimExecucao)?;
        Ok(saida)
    }
    async fn copiar_para_dentro(&mut self, origem: &Path, destino: &Path) -> Result<()> {
        self.estado = avancar(self.estado, Passo::Copiar)?;
        copia::para_dentro(&self.areas.abrir_copia()?, origem, destino)
    }
    async fn copiar_para_fora(&mut self, origem: &Path, destino: &Path) -> Result<()> {
        self.estado = avancar(self.estado, Passo::Copiar)?;
        copia::para_fora(&self.areas.abrir_copia()?, origem, destino)
    }
    async fn encerrar(&mut self) -> Result<()> {
        // Execuções são sequenciais e o guarda de cada uma já matou o grupo. Só fica Encerrado
        // com a limpeza concluída; se ela falhar, o estado permite repetir (e o Drop também).
        let encerrado = avancar(self.estado, Passo::Encerrar)?;
        if let Err(erro) = self.areas.apagar() {
            self.estado = avancar(self.estado, Passo::Falha)?;
            return Err(erro);
        }
        self.estado = encerrado;
        Ok(())
    }
}
