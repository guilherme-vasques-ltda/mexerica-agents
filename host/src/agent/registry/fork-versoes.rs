use anyhow::{Result, ensure};
use fs2::FileExt as _;
use std::fs::{self, File};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[path = "fork-versoes-identidade.rs"]
mod identidade;

/// Pasta dos marcadores, ao lado da raiz física da instalação de cada agente.
const MARCADORES: &str = "registry-in-use";

/// Trava do SO por agente, liberada quando o descritor fecha.
///
/// Vale entre processos (dois Hubs, o instalador separado e a limpeza), não só dentro
/// deste processo: seleção, reserva, publicação e limpeza do mesmo agente se serializam.
pub(super) struct Trava(File);

impl Trava {
    pub(super) fn abrir(raiz: &Path, id: &str) -> Result<Self> {
        let pasta = criar_raiz(raiz)?;
        let arquivo = abrir_lock(&pasta.join(format!("{}.lock", componente(id)?)))?;
        arquivo.lock_exclusive().map_err(|e| erro_marcador("travar_agente", id, "-", e.kind()))?;
        Ok(Self(arquivo))
    }
}

fn componente(id: &str) -> Result<&str> {
    ensure!(!id.is_empty() && id != "." && id != ".." && !id.contains(['/', '\\', '\0']), "identificador inválido");
    Ok(id)
}

fn criar_raiz(raiz: &Path) -> Result<PathBuf> {
    let pasta = raiz.join(MARCADORES);
    let mut construtor = fs::DirBuilder::new();
    construtor.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut construtor, 0o700);
    construtor.create(&pasta)?;
    Ok(pasta)
}

fn abrir_lock(caminho: &Path) -> std::io::Result<File> {
    let mut opcoes = fs::OpenOptions::new();
    opcoes.read(true).write(true).create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opcoes, 0o600);
    opcoes.open(caminho)
}

/// O que se sabe sobre o uso das versões de um agente.
///
/// `None`, onde ele aparece, quer dizer "não há prova": a instalação não pode ser
/// substituída nem limpa. Ausência de sinal nunca autoriza apagar.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Protecao {
    /// Versões com uso comprovado agora.
    pub(crate) em_uso: Vec<String>,
    /// Versões que já passaram por este protocolo; as demais são anteriores a ele.
    pub(crate) registradas: Vec<String>,
}

impl Protecao {
    pub(super) fn em_uso(&self, versao: &str) -> bool {
        self.em_uso.iter().any(|v| v.eq_ignore_ascii_case(versao))
    }

    /// Só remove com recibo de que a versão foi instalada por este protocolo e está livre.
    pub(super) fn pode_remover(&self, versao: &str) -> bool {
        !self.em_uso(versao) && self.registradas.iter().any(|v| v.eq_ignore_ascii_case(versao))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Uso(Arc<Referencia>);

#[derive(Debug)]
struct Referencia {
    marcador: PathBuf,
    pasta: PathBuf,
    id: String,
    versao: String,
}

impl Drop for Referencia {
    fn drop(&mut self) {
        let marcador = std::mem::take(&mut self.marcador);
        // Um descritor sem nome não precisa de runtime: a remoção é uma chamada só.
        let _ = fs::remove_file(marcador);
    }
}

impl Uso {
    pub(super) fn registrar(raiz: &Path, id: &str, versao: &str) -> Result<Self> {
        componente(id)?;
        componente(versao)?;
        let pasta = criar_raiz(raiz)?.join(id).join(versao);
        let _trava = Trava::abrir(raiz, id)?;
        let mut diretorio = fs::DirBuilder::new();
        diretorio.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut diretorio, 0o700);
        diretorio.create(&pasta).map_err(|e| erro_marcador("criar_pasta", id, versao, e.kind()))?;
        let pid = std::process::id();
        let inicio = match identidade::processo(pid) {
            identidade::Processo::Vivo(inicio) => inicio.to_string(),
            _ => "-".into(),
        };
        let marcador = criar_marcador(&pasta, &format!("reserva.{pid}.{inicio}"))
            .map_err(|e| erro_marcador("criar_reserva", id, versao, e.kind()))?
            .into_temp_path()
            .keep()
            .map_err(|e| erro_marcador("manter_reserva", id, versao, e.error.kind()))?;
        Ok(Self(Arc::new(Referencia { marcador, pasta, id: id.into(), versao: versao.into() })))
    }

    pub(super) fn marcador(&self) -> &Path {
        &self.0.marcador
    }

    pub(crate) fn lancar<T>(&self, spawn: impl FnOnce() -> Result<T>) -> Result<T> {
        self.lancar_no_boot(identidade::boot(), spawn)
    }

    /// Grava a marca de lançamento e só então entrega o comando.
    ///
    /// A trava do SO cobre apenas a gravação: criar o processo fora dela mantém outros
    /// lançamentos andando enquanto um agente grande inicializa.
    fn lancar_no_boot<T>(&self, boot: Option<uuid::Uuid>, spawn: impl FnOnce() -> Result<T>) -> Result<T> {
        let boot = boot.map_or_else(|| "-".into(), |id| id.to_string());
        let marcador = self.0.pasta.join(format!("boot.{boot}"));
        let novo = {
            let raiz = self.0.pasta.parent().and_then(Path::parent).and_then(Path::parent);
            let _trava = raiz.map(|raiz| Trava::abrir(raiz, &self.0.id)).transpose()?;
            criar_marcador_boot(&marcador)
                .map_err(|e| erro_marcador("criar_lancamento", &self.0.id, &self.0.versao, e.kind()))?
        };
        if !novo {
            return spawn();
        }
        // Só o primeiro lançamento pode desfazer a criação, antes de outro reutilizar o marcador.
        let filho = spawn();
        if filho.is_err() {
            let _ = fs::remove_file(marcador);
        }
        filho
    }
}

fn erro_marcador(operacao: &str, id: &str, versao: &str, categoria: ErrorKind) -> anyhow::Error {
    anyhow::anyhow!("Marcador do agente: operação={operacao}, agente={id}, versão={versao}, categoria={categoria:?}")
}

fn criar_marcador(pasta: &Path, prefixo: &str) -> std::io::Result<tempfile::NamedTempFile> {
    tempfile::Builder::new().prefix(&format!("{prefixo}.")).tempfile_in(pasta)
}

fn criar_marcador_boot(caminho: &Path) -> std::io::Result<bool> {
    let mut opcoes = fs::OpenOptions::new();
    opcoes.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opcoes, 0o600);
    match opcoes.open(caminho) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == ErrorKind::AlreadyExists && fs::symlink_metadata(caminho)?.is_file() => Ok(false),
        Err(e) => Err(e),
    }
}

fn marcador_ativo(entrada: &fs::DirEntry, boot: Option<uuid::Uuid>) -> bool {
    if !entrada.file_type().is_ok_and(|t| t.is_file()) {
        return true;
    }
    let nome = entrada.file_name();
    let Some(nome) = nome.to_str() else { return true };
    let partes: Vec<_> = nome.split('.').collect();
    let caminho = entrada.path();
    let obsoleto = match partes.as_slice() {
        ["boot", anterior] | ["boot", anterior, _] => {
            matches!((uuid::Uuid::parse_str(anterior), boot), (Ok(anterior), Some(atual)) if anterior != atual)
        }
        ["reserva", pid, inicio, _] => {
            let Ok(pid) = pid.parse::<u32>() else { return true };
            match identidade::processo(pid) {
                identidade::Processo::Ausente => true,
                identidade::Processo::Vivo(atual) => inicio.parse::<u64>().is_ok_and(|anterior| anterior != atual),
                identidade::Processo::Incerto => false,
            }
        }
        _ => false,
    };
    !obsoleto || !remover_marcador(&caminho)
}

fn remover_marcador(caminho: &Path) -> bool {
    match fs::remove_file(caminho) {
        Ok(()) => true,
        Err(e) => e.kind() == ErrorKind::NotFound,
    }
}

pub(super) fn protegidas(raiz: &Path, id: &str) -> Option<Protecao> {
    protegidas_no_boot(raiz, id, identidade::boot())
}

fn protegidas_no_boot(raiz: &Path, id: &str, boot: Option<uuid::Uuid>) -> Option<Protecao> {
    let _trava = Trava::abrir(raiz, id).ok()?;
    listar_protegidas(raiz, id, boot, None)
}

pub(super) fn com_protecao_atual<T>(
    raiz: &Path,
    id: &str,
    propria_reserva: &Path,
    f: impl FnOnce(Option<&Protecao>) -> Result<T>,
) -> Result<T> {
    let _trava = Trava::abrir(raiz, id)?;
    let protecao = if fs::symlink_metadata(propria_reserva).is_ok_and(|meta| meta.is_file()) {
        listar_protegidas(raiz, id, identidade::boot(), Some(propria_reserva))
    } else {
        None
    };
    f(protecao.as_ref())
}

fn listar_protegidas(raiz: &Path, id: &str, boot: Option<uuid::Uuid>, ignorar: Option<&Path>) -> Option<Protecao> {
    let versoes = match fs::read_dir(raiz.join(MARCADORES).join(id)) {
        Ok(versoes) => versoes,
        // Sem marcadores não existe prova de desuso: instalação anterior ao protocolo.
        Err(_) => return None,
    };
    let mut protecao = Protecao::default();
    for versao in versoes {
        let versao = versao.ok()?;
        let nome = versao.file_name();
        let nome = nome.to_str()?;
        if nome.ends_with(".lock") {
            continue;
        }
        if !versao.file_type().ok()?.is_dir() {
            return None;
        }
        protecao.registradas.push(nome.to_owned());
        let mut ativa = false;
        for marcador in fs::read_dir(versao.path()).ok()? {
            let marcador = marcador.ok()?;
            ativa |= ignorar != Some(marcador.path().as_path()) && marcador_ativo(&marcador, boot);
        }
        if ativa {
            protecao.em_uso.push(nome.to_owned());
            if protecao.em_uso.len() > 128 {
                return None;
            }
        }
    }
    Some(protecao)
}

pub(super) fn protegida(versao: &str, protecao: Option<&Protecao>) -> bool {
    protecao.is_none_or(|protecao| protecao.em_uso(versao))
}

#[cfg(unix)]
pub(super) fn limpar_versoes(instalada: &Path, protecao: Option<&Protecao>) {
    limpeza::limpar(instalada, protecao);
}

#[cfg(not(unix))]
pub(super) fn limpar_versoes(_instalada: &Path, _protecao: Option<&Protecao>) {}

#[cfg(unix)]
#[path = "fork-versoes-limpeza.rs"]
mod limpeza;

#[derive(Clone, Debug)]
pub(crate) struct Comando {
    pub(crate) linha: String,
    pub(crate) uso: Option<Uso>,
}

impl From<String> for Comando {
    fn from(linha: String) -> Self {
        Self { linha, uso: None }
    }
}

impl std::ops::Deref for Comando {
    type Target = str;

    fn deref(&self) -> &str {
        &self.linha
    }
}


#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn cria_diretorios_de_marcadores_privados() {
        let raiz = tempfile::tempdir().unwrap();
        let _uso = Uso::registrar(raiz.path(), "agente", "1").unwrap();
        let mut pasta = raiz.path().to_path_buf();
        for componente in ["registry-in-use", "agente", "1"] {
            pasta.push(componente);
            assert_eq!(fs::metadata(&pasta).unwrap().permissions().mode() & 0o077, 0);
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn descendente_sem_descritores_preserva_versao_apos_saida_do_lider() {
        use std::process::Stdio;
        use std::time::Duration;

        let raiz = tempfile::tempdir().unwrap();
        let uso = Uso::registrar(raiz.path(), "agente", "1").unwrap();
        let pai = raiz.path().join("agents").join("agente");
        let velha = pai.join("1");
        let atual = pai.join("2");
        for pasta in [&velha, &atual] {
            fs::create_dir_all(pasta).unwrap();
        }
        let boot = uuid::Uuid::new_v4();
        let mut comando = tokio::process::Command::new("python3");
        comando
            .args([
                "-c",
                r#"import subprocess
subprocess.Popen(
    ["/bin/sh", "-c", 'touch pronto; i=0; while [ ! -f sair ] && [ "$i" -lt 300 ]; do sleep 0.1; i=$((i + 1)); done'],
    close_fds=True, start_new_session=True,
    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
)"#,
            ])
            .current_dir(raiz.path())
            .process_group(0)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut lider = uso.lancar_no_boot(Some(boot), || Ok(crate::fork::motor_exec::spawn(&mut comando)?)).unwrap();
        let pid = libc::pid_t::try_from(lider.id().unwrap()).unwrap();
        drop(uso);
        assert!(lider.wait().await.unwrap().success());
        tokio::time::timeout(Duration::from_secs(5), async {
            while !raiz.path().join("pronto").is_file() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        // SAFETY: sinal 0 só consulta o grupo do líder.
        assert_eq!(unsafe { libc::killpg(pid, 0) }, -1);
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
        let protegidas = protegidas_no_boot(raiz.path(), "agente", Some(boot));
        limpar_versoes(&atual, protegidas.as_ref());
        assert!(velha.is_dir());

        fs::write(raiz.path().join("sair"), "").unwrap();
        assert!(atual.is_dir());
    }

    #[test]
    fn marcador_pty_so_expira_com_boot_diferente_comprovado() {
        let raiz = tempfile::tempdir().unwrap();
        let uso = Uso::registrar(raiz.path(), "agente", "1").unwrap();
        let boot = uuid::Uuid::new_v4();
        let pty = portable_pty::native_pty_system().openpty(portable_pty::PtySize::default()).unwrap();
        let mut child = uso
            .lancar_no_boot(Some(boot), || pty.slave.spawn_command(portable_pty::CommandBuilder::new("/bin/true")))
            .unwrap();
        assert!(child.wait().unwrap().success());
        drop(uso);

        let protegidas = |boot| protegidas_no_boot(raiz.path(), "agente", boot).unwrap().em_uso;
        assert_eq!(protegidas(Some(boot)), ["1"]);
        assert_eq!(protegidas(None), ["1"]);
        assert_eq!(protegidas(Some(uuid::Uuid::new_v4())), Vec::<String>::new());

        let uso = Uso::registrar(raiz.path(), "agente", "2").unwrap();
        uso.lancar_no_boot(None, || Ok(())).unwrap();
        drop(uso);
        assert_eq!(protegidas(Some(boot)), ["2"]);
    }

    #[test]
    fn marcador_de_lancamento_unico_por_boot_sobrevive_a_falha_posterior() {
        let raiz = tempfile::tempdir().unwrap();
        let boot = Some(uuid::Uuid::new_v4());
        for (versao, boot) in [("1", boot), ("2", None)] {
            let uso = Uso::registrar(raiz.path(), "agente", versao).unwrap();
            let falhar = || anyhow::bail!("falha no processo de teste");
            let reserva = &uso.0.marcador;
            assert!(uso.lancar_no_boot::<()>(boot, falhar).is_err());
            assert_eq!(fs::read_dir(&uso.0.pasta).unwrap().count(), 1);
            assert!(reserva.is_file());
            for _ in 0..3 {
                uso.lancar_no_boot(boot, || Ok(())).unwrap();
            }
            assert!(uso.lancar_no_boot::<()>(boot, falhar).is_err());
            let pasta = uso.0.pasta.clone();
            drop(uso);
            assert_eq!(fs::read_dir(&pasta).unwrap().count(), 1);
        }
        let mut ativas = protegidas_no_boot(raiz.path(), "agente", boot).unwrap().em_uso;
        ativas.sort();
        assert_eq!(ativas, ["1", "2"]);
    }

    #[test]
    fn erro_ao_criar_marcador_expoe_apenas_identificadores() {
        let raiz = tempfile::tempdir().unwrap();
        let uso = Uso::registrar(raiz.path(), "agente", "1").unwrap();
        fs::remove_dir_all(&uso.0.pasta).unwrap();
        let error = uso.lancar::<()>(|| panic!("não deve lançar sem proteção")).unwrap_err();
        assert_eq!(
            format!("{error:#}"),
            "Marcador do agente: operação=criar_lancamento, agente=agente, versão=1, categoria=NotFound"
        );
    }

    #[tokio::test]
    async fn cancelar_espera_pelo_lancamento_nao_libera_reserva_antes_da_saida_da_tarefa() {
        use std::time::Duration;
        let raiz = tempfile::tempdir().unwrap();
        let uso = Uso::registrar(raiz.path(), "agente", "1").unwrap();
        let marcador = uso.marcador().to_owned();
        let (entrou, espera_entrada) = tokio::sync::oneshot::channel();
        let (liberar, espera_saida) = std::sync::mpsc::channel();
        let tarefa = tokio::spawn(async move {
            tokio::task::spawn_blocking(move || {
                uso.lancar(|| {
                    entrou.send(()).unwrap();
                    espera_saida.recv_timeout(Duration::from_secs(5)).unwrap();
                    Ok(())
                })
                .unwrap();
            })
            .await
            .unwrap();
        });
        tokio::time::timeout(Duration::from_secs(2), espera_entrada).await.unwrap().unwrap();
        tarefa.abort();
        assert!(tarefa.await.unwrap_err().is_cancelled());
        assert!(marcador.is_file());
        liberar.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while marcador.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(protegidas(raiz.path(), "agente").unwrap().em_uso, ["1"]);
    }

    #[test]
    fn revalidacao_ignora_apenas_a_reserva_da_propria_instalacao() {
        let raiz = tempfile::tempdir().unwrap();
        let uso = Uso::registrar(raiz.path(), "agente", "1").unwrap();
        com_protecao_atual(raiz.path(), "agente", uso.marcador(), |protegidas| {
            assert!(!protegida("1", protegidas));
            Ok(())
        })
        .unwrap();
        let _outro = Uso::registrar(raiz.path(), "agente", "1").unwrap();
        com_protecao_atual(raiz.path(), "agente", uso.marcador(), |protegidas| {
            assert!(protegida("1", protegidas));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn descarta_reserva_mesmo_com_runtime_encerrado() {
        let raiz = tempfile::tempdir().unwrap();
        let uso = Uso::registrar(raiz.path(), "agente", "1").unwrap();
        let marcador = uso.marcador().to_owned();
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let handle = runtime.handle().clone();
        drop(runtime);
        let _entra = handle.enter();
        drop(uso);
        assert!(!marcador.exists());
    }

    #[test]
    fn recupera_reserva_do_hub_morto_sem_remover_reserva_viva() {
        let raiz = tempfile::tempdir().unwrap();
        let uso = Uso::registrar(raiz.path(), "agente", "1").unwrap();
        let viva = uso.0.marcador.clone();
        let mut processo = std::process::Command::new("/bin/cat").stdin(std::process::Stdio::piped()).spawn().unwrap();
        let pid = processo.id();
        let identidade::Processo::Vivo(inicio) = identidade::processo(pid) else {
            panic!("início do processo não encontrado");
        };
        let orfa =
            criar_marcador(&uso.0.pasta, &format!("reserva.{pid}.{inicio}")).unwrap().into_temp_path().keep().unwrap();
        drop(processo.stdin.take());
        assert!(processo.wait().unwrap().success());
        assert_eq!(protegidas(raiz.path(), "agente").unwrap().em_uso, ["1"]);
        assert!(viva.exists());
        assert!(!orfa.exists());
    }

    #[test]
    fn preserva_marcadores_legados_sem_prova_de_reinicio() {
        let raiz = tempfile::tempdir().unwrap();
        let uso = Uso::registrar(raiz.path(), "agente", "1").unwrap();
        let lease = criar_marcador(&uso.0.pasta, "lease").unwrap().into_temp_path().keep().unwrap();
        let incerto = uso.0.pasta.join("lease.diretorio");
        fs::create_dir(&incerto).unwrap();
        drop(uso);
        assert_eq!(protegidas(raiz.path(), "agente").unwrap().em_uso, ["1"]);
        assert!(lease.is_file());
        assert!(incerto.is_dir());
    }

    #[test]
    fn preserva_instalacao_anterior_ao_protocolo_e_limpa_apenas_as_registradas() {
        let raiz = tempfile::tempdir().unwrap();
        let agente = raiz.path().join("agents").join("agente");
        let antiga = agente.join("1.0.0");
        let primeira = agente.join("1.1.0");
        let atual = agente.join("1.2.0");
        for pasta in [&antiga, &primeira, &atual] {
            fs::create_dir_all(pasta).unwrap();
        }
        // Sem marcadores não existe prova de desuso: nada é removido.
        assert!(protegidas(raiz.path(), "agente").is_none());
        limpar_versoes(&atual, protegidas(raiz.path(), "agente").as_ref());
        assert!(antiga.is_dir() && primeira.is_dir());

        // A versão instalada por este protocolo passa a ter recibo.
        drop(Uso::registrar(raiz.path(), "agente", "1.1.0").unwrap());
        drop(Uso::registrar(raiz.path(), "agente", "1.2.0").unwrap());
        limpar_versoes(&atual, protegidas(raiz.path(), "agente").as_ref());
        assert!(!primeira.exists(), "versão registrada e livre deveria sair");
        assert!(antiga.is_dir(), "instalação anterior ao protocolo não tem prova de desuso");
        assert!(atual.is_dir());
    }

    #[test]
    fn erro_ao_ler_marcadores_preserva_versoes() {
        let raiz = tempfile::tempdir().unwrap();
        let agente = raiz.path().join("agents").join("agente");
        let velha = agente.join("1");
        let atual = agente.join("2");
        for pasta in [&velha, &atual] {
            fs::create_dir_all(pasta).unwrap();
        }
        drop(Uso::registrar(raiz.path(), "agente", "1").unwrap());
        let marcadores = raiz.path().join(MARCADORES).join("agente");
        fs::set_permissions(&marcadores, fs::Permissions::from_mode(0o000)).unwrap();
        let protegidas = protegidas(raiz.path(), "agente");
        assert!(protegidas.is_none(), "erro de I/O não pode virar lista vazia");
        limpar_versoes(&atual, protegidas.as_ref());
        fs::set_permissions(&marcadores, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(velha.is_dir() && atual.is_dir());
    }

    #[test]
    fn trava_do_so_serializa_usos_da_mesma_raiz() {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};
        let raiz = tempfile::tempdir().unwrap();
        let (liberar, espera) = mpsc::channel();
        let (segurou, aviso) = mpsc::channel();
        let caminho = raiz.path().to_path_buf();
        let dono = std::thread::spawn(move || {
            let _trava = Trava::abrir(&caminho, "agente").unwrap();
            segurou.send(()).unwrap();
            espera.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        aviso.recv_timeout(Duration::from_secs(5)).unwrap();
        let inicio = Instant::now();
        let segunda = Trava::abrir(raiz.path(), "agente").unwrap();
        assert!(inicio.elapsed() >= Duration::from_millis(50), "a segunda trava não esperou a primeira");
        drop(segunda);
        liberar.send(()).unwrap();
        dono.join().unwrap();
    }

    const FIXTURE: &str = "DONNABOT_TEST_TRAVA_AGENTE";
    const AVISO: &str = "DONNABOT_TEST_TRAVA_AVISO";

    #[tokio::test]
    async fn trava_exclui_outro_processo_na_mesma_raiz() {
        use std::time::{Duration, Instant};
        if std::env::var_os(FIXTURE).is_none() {
            let raiz = tempfile::tempdir().unwrap();
            let aviso = raiz.path().join("segurou");
            let mut filho = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "agent::registry::versoes::tests::trava_exclui_outro_processo_na_mesma_raiz"])
                .env(FIXTURE, raiz.path())
                .env(AVISO, &aviso)
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let prazo = Instant::now() + Duration::from_secs(20);
            while !aviso.is_file() {
                assert!(Instant::now() < prazo, "o outro processo não segurou a trava");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let caminho = raiz.path().to_path_buf();
            let inicio = Instant::now();
            let trava = tokio::task::spawn_blocking(move || Trava::abrir(&caminho, "agente"))
                .await
                .unwrap()
                .unwrap();
            assert!(inicio.elapsed() >= Duration::from_millis(200), "a trava não excluiu o outro processo");
            drop(trava);
            assert!(filho.wait().unwrap().success());
            return;
        }
        let raiz = PathBuf::from(std::env::var_os(FIXTURE).unwrap());
        let aviso = PathBuf::from(std::env::var_os(AVISO).unwrap());
        let _trava = Trava::abrir(&raiz, "agente").unwrap();
        fs::write(&aviso, "").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(600));
    }
}
