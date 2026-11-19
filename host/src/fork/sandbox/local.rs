//! Provedor de referência: processo comum no host, sem contenção de arquivos ou rede.
//! Serve para provar o contrato e para bots do próprio dono; agente desconhecido é
//! recusado na admissão. Home e tmp são privados por execução, e o ambiente do Hub
//! não é herdado, mas o processo enxerga o que o usuário do Hub enxerga.
use super::{
    contrato::{Contencao, Execucao, Indisponivel, Modo, Preparo, Provedor, Tipo, admitir},
    montagem::{Abertas, Areas, Lancador, SandboxMontagem, abrir_pasta, ambiente_base},
    processo::{Graca, Lancamento},
};
use anyhow::{Context as _, Result, ensure};
use std::{
    ffi::OsString,
    os::fd::AsRawFd as _,
    path::{Path, PathBuf},
};
use tokio::process::Command;

/// PATH fixo do toolchain; o agente não troca nem herda o do Hub.
pub(crate) const PATH_PADRAO: &str = "/usr/local/bin:/usr/bin:/bin";

pub(crate) struct ProcessoLocal {
    /// Pasta privada (0700) onde nascem home e tmp de cada sandbox.
    raiz: PathBuf,
    path: OsString,
}

impl ProcessoLocal {
    pub(crate) fn new(raiz: PathBuf, path: OsString) -> Result<Self> {
        ensure!(raiz.is_absolute(), "raiz_sandbox_relativa");
        validar_path(&path)?;
        Ok(Self { raiz, path })
    }
}

/// Só pastas absolutas, sem vazio nem `..`: PATH relativo executaria binário da cópia.
pub(crate) fn validar_path(path: &OsString) -> Result<()> {
    let texto = path.to_str().context("path_invalido")?;
    ensure!(
        !texto.is_empty()
            && texto.split(':').all(|p| {
                let p = Path::new(p);
                p.is_absolute() && p.components().all(|c| !matches!(c, std::path::Component::ParentDir))
            }),
        "path_invalido"
    );
    Ok(())
}

#[allow(clippy::unused_async_trait_impl, reason = "contrato async para provedores remotos; montagem local é síncrona")]
impl Provedor for ProcessoLocal {
    type Sandbox = SandboxMontagem;
    fn tipo(&self) -> Tipo {
        Tipo::ProcessoLocal
    }
    fn modo(&self) -> Modo {
        Modo::Montagem
    }
    fn contencao(&self) -> Contencao {
        Contencao::Nenhuma
    }
    fn verificar(&self) -> Result<(), Indisponivel> {
        Ok(())
    }
    async fn preparar(&self, preparo: &Preparo) -> Result<SandboxMontagem> {
        admitir(preparo.confianca, self.contencao())?;
        self.verificar()?;
        let areas = Areas::criar(&self.raiz, &preparo.copia)?;
        Ok(SandboxMontagem::new(areas, Lancador::Local { path: self.path.clone() }))
    }
}

/// Processo direto no host, na pasta pedida dentro da cópia. A pasta é aberta sem seguir links
/// e o filho entra nela por descritor (`fchdir`), sem reabrir o caminho: `pasta` que é link para
/// fora da cópia é recusada.
pub(crate) fn comando(abertas: &Abertas, areas: &Areas, path: &OsString, execucao: &Execucao) -> Result<Lancamento> {
    let pasta = abrir_pasta(&abertas.copia, &execucao.pasta)?;
    let fd = pasta.as_raw_fd();
    let mut command = Command::new(&execucao.argv[0]);
    command
        .args(&execucao.argv[1..])
        .env_clear()
        .envs(ambiente_base(&areas.home, &areas.tmp, path))
        .envs(execucao.env.iter().map(|(k, v)| (k, v)));
    // SAFETY: roda no filho entre fork e exec; só chama fchdir (async-signal-safe) em um
    // descritor que `Lancamento::manter` mantém vivo no pai até o spawn.
    unsafe {
        command.pre_exec(move || if libc::fchdir(fd) == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) });
    }
    Ok(Lancamento { command, manter: vec![pasta], graca: Graca::Grupo })
}
