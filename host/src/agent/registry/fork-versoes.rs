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
pub(super) struct Trava {
    /// Mantém a trava viva: fechar o descritor a libera.
    _arquivo: File,
}

impl Trava {
    pub(super) fn abrir(raiz: &Path, id: &str) -> Result<Self> {
        // A trava mora dentro da pasta do agente: um agente chamado `demo.lock` não pode
        // colidir com o arquivo de trava do agente `demo`.
        let pasta = criar_raiz(raiz)
            .map_err(|e| {
                erro_marcador(
                    "criar_raiz_trava",
                    id,
                    "-",
                    e.downcast_ref::<std::io::Error>().map_or(ErrorKind::Other, std::io::Error::kind),
                )
            })?
            .join(componente(id)?);
        criar_diretorio(&pasta).map_err(|e| erro_marcador("criar_pasta_trava", id, "-", e.kind()))?;
        let arquivo = abrir_lock(&pasta.join("trava")).map_err(|e| erro_marcador("abrir_trava", id, "-", e.kind()))?;
        arquivo.lock_exclusive().map_err(|e| erro_marcador("travar_agente", id, "-", e.kind()))?;
        Ok(Self { _arquivo: arquivo })
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
    pub(crate) fn em_uso(&self, versao: &str) -> bool {
        self.em_uso.iter().any(|v| v.eq_ignore_ascii_case(versao))
    }

    /// Versão comprovadamente publicada por este protocolo e sem uso agora.
    ///
    /// Serve tanto para apagar quanto para substituir: sem recibo e sem prova de desuso,
    /// a versão fica no disco.
    pub(crate) fn liberada(&self, versao: &str) -> bool {
        // O recibo é da pasta exata: em volume sensível a maiúsculas, `RC1` e `rc1` são
        // instalações distintas, e o recibo de uma não libera a outra.
        !self.em_uso(versao) && self.registradas.iter().any(|v| v == versao)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Uso(Arc<Estado>);

/// A raiz é gravável por este processo: no modo de motor separado, quem grava a raiz é o
/// próprio instalador, e a preservação de versões não se aplica (bloqueada em release).
#[derive(Debug)]
enum Estado {
    Local(Referencia),
}

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
    /// Reserva local: marca em arquivo desta raiz, gravada por este processo.
    pub(super) fn registrar(raiz: &Path, id: &str, versao: &str) -> Result<Self> {
        componente(id)?;
        componente(versao)?;
        // `trava` é o arquivo de trava dentro da pasta do agente: uma versão com esse
        // nome tentaria criar o diretório de marcadores sobre o arquivo da trava.
        ensure!(versao != "trava", "{}", erro_marcador("criar_pasta", id, versao, ErrorKind::InvalidInput));
        let pasta = criar_raiz(raiz)?.join(id).join(versao);
        let _trava = Trava::abrir(raiz, id)?;
        criar_diretorio(&pasta).map_err(|e| erro_marcador("criar_pasta", id, versao, e.kind()))?;
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
        Ok(Self(Arc::new(Estado::Local(Referencia { marcador, pasta, id: id.into(), versao: versao.into() }))))
    }

    /// Reserva local que deve ser ignorada ao decidir sobre a própria versão.
    pub(crate) fn marcador(&self) -> &Path {
        let Estado::Local(referencia) = &*self.0;
        &referencia.marcador
    }

    /// Grava a marca de lançamento antes de o comando ser entregue.
    ///
    /// A marca nunca é desfeita depois: outro lançamento pode estar usando a mesma marca,
    /// e sem prova de desuso a versão fica protegida até uma reinicialização comprovada.
    pub(crate) fn marcar_lancamento(&self, boot: Option<uuid::Uuid>) -> Result<()> {
        let Estado::Local(referencia) = &*self.0;
        let boot = boot.map_or_else(|| "-".into(), |id| id.to_string());
        let marcador = referencia.pasta.join(format!("boot.{boot}"));
        let raiz = referencia.pasta.parent().and_then(Path::parent).and_then(Path::parent);
        let _trava = raiz.map(|raiz| Trava::abrir(raiz, &referencia.id)).transpose()?;
        criar_marcador_boot(&marcador)
            .map_err(|e| erro_marcador("criar_lancamento", &referencia.id, &referencia.versao, e.kind()))?;
        Ok(())
    }
}

fn criar_diretorio(pasta: &Path) -> std::io::Result<()> {
    let mut diretorio = fs::DirBuilder::new();
    diretorio.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut diretorio, 0o700);
    diretorio.create(pasta)
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
            // Formato desconhecido preserva: sem prova, a marca continua valendo.
            if *inicio != "-" && inicio.parse::<u64>().is_err() {
                return true;
            }
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

pub(crate) fn boot() -> Option<uuid::Uuid> {
    identidade::boot()
}

pub(super) fn protegidas(raiz: &Path, id: &str) -> Option<Protecao> {
    protegidas_no_boot(raiz, id, identidade::boot())
}

fn protegidas_no_boot(raiz: &Path, id: &str, boot: Option<uuid::Uuid>) -> Option<Protecao> {
    let _trava = Trava::abrir(raiz, id).ok()?;
    listar_protegidas(raiz, id, boot, None)
}

/// Relê a proteção sob a trava do agente e publica dentro dela.
///
/// `ignorar` é a reserva de quem está instalando: ela não conta como uso da própria versão.
/// Quando não há reserva de quem chamou, a lista vale como está.
pub(super) fn com_protecao_atual<T>(
    raiz: &Path,
    id: &str,
    ignorar: Option<&Path>,
    f: impl FnOnce(&Trava, Option<&Protecao>) -> Result<T>,
) -> Result<T> {
    // A trava aberta aqui é entregue ao fechamento: readquiri-la por outro descritor
    // travaria a publicação de vez.
    let trava = Trava::abrir(raiz, id)?;
    let propria = match ignorar {
        Some(caminho) if !fs::symlink_metadata(caminho).is_ok_and(|meta| meta.is_file()) => None,
        caminho => caminho,
    };
    if ignorar.is_some() && propria.is_none() {
        // A reserva de quem instala desapareceu: sem prova, não publica.
        return f(&trava, None);
    }
    let protecao = listar_protegidas(raiz, id, identidade::boot(), propria);
    f(&trava, protecao.as_ref())
}

fn listar_protegidas(raiz: &Path, id: &str, boot: Option<uuid::Uuid>, ignorar: Option<&Path>) -> Option<Protecao> {
    // Versões candidatas são a união das instaladas com as que têm marcadores.
    let mut instaladas = std::collections::HashSet::new();
    match fs::read_dir(raiz.join("agents").join(id)) {
        Ok(entradas) => {
            for entrada in entradas {
                let entrada = entrada.ok()?;
                if !entrada.file_type().ok()?.is_dir() {
                    return None;
                }
                instaladas.insert(entrada.file_name());
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(_) => return None,
    }
    let mut marcadores = std::collections::HashSet::new();
    match fs::read_dir(raiz.join(MARCADORES).join(id)) {
        Ok(entradas) => {
            for entrada in entradas {
                let entrada = entrada.ok()?;
                if !entrada.file_type().ok()?.is_dir() {
                    // Só a trava do agente é arquivo esperado aqui; qualquer outra entrada
                    // inesperada é estado indeterminado e preserva tudo.
                    if entrada.file_name() == "trava" {
                        continue;
                    }
                    return None;
                }
                marcadores.insert(entrada.file_name());
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(_) => return None,
    }
    if instaladas.is_empty() && marcadores.is_empty() {
        return None;
    }
    let mut protecao = Protecao::default();
    for nome in instaladas.union(&marcadores) {
        let nome_texto = nome.to_str()?;
        if instaladas.contains(nome) {
            // O recibo mora dentro da árvore publicada (gravado antes do rename atômico):
            // transacional com a instalação e morto junto com ela. Só vale a prova nova,
            // com versão e vínculo ao marcador; um arquivo `.publicada` legado não prova
            // publicação por este protocolo e a versão fica fora da substituição e da
            // limpeza.
            let pasta = raiz.join("agents").join(id).join(nome);
            if super::arquivo::publicada_por_este_protocolo(&pasta) {
                protecao.registradas.push(nome_texto.to_owned());
            }
        }
        if !marcadores.contains(nome) {
            continue;
        }
        let pasta = raiz.join(MARCADORES).join(id).join(nome);
        let mut ativa = false;
        for marcador in fs::read_dir(&pasta).ok()? {
            let marcador = marcador.ok()?;
            ativa |= ignorar != Some(marcador.path().as_path()) && marcador_ativo(&marcador, boot);
        }
        if ativa {
            protecao.em_uso.push(nome_texto.to_owned());
            if protecao.em_uso.len() > 128 {
                return None;
            }
        }
    }
    Some(protecao)
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

    fn local(uso: &Uso) -> &Referencia {
        let Estado::Local(referencia) = &*uso.0;
        referencia
    }

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
        // Com recibo, a versão antiga é elegível: só a marca de lançamento a preserva.
        let bytes = b"fixture";
        let hash = super::super::sha256_hex(bytes);
        super::super::arquivo::publicar(&velha, "adapter", bytes, &hash, Path::new("adapter"), 1024).unwrap();
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
        uso.marcar_lancamento(Some(boot)).unwrap();
        let mut lider = crate::fork::motor_exec::spawn(&mut comando).unwrap();
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
        uso.marcar_lancamento(Some(boot)).unwrap();
        let mut child = pty.slave.spawn_command(portable_pty::CommandBuilder::new("/bin/true")).unwrap();
        assert!(child.wait().unwrap().success());
        drop(uso);

        let protegidas = |boot| protegidas_no_boot(raiz.path(), "agente", boot).unwrap().em_uso;
        assert_eq!(protegidas(Some(boot)), ["1"]);
        assert_eq!(protegidas(None), ["1"]);
        assert_eq!(protegidas(Some(uuid::Uuid::new_v4())), Vec::<String>::new());

        let uso = Uso::registrar(raiz.path(), "agente", "2").unwrap();
        uso.marcar_lancamento(None).unwrap();
        drop(uso);
        assert_eq!(protegidas(Some(boot)), ["2"]);
    }

    #[test]
    fn marcador_de_lancamento_unico_por_boot_sobrevive_a_falha_posterior() {
        let raiz = tempfile::tempdir().unwrap();
        let boot = Some(uuid::Uuid::new_v4());
        for (versao, boot) in [("1", boot), ("2", None)] {
            let uso = Uso::registrar(raiz.path(), "agente", versao).unwrap();
            let reserva = local(&uso).marcador.clone();
            uso.marcar_lancamento(boot).unwrap();
            assert_eq!(fs::read_dir(&local(&uso).pasta).unwrap().count(), 2);
            assert!(reserva.is_file());
            // Repetir o lançamento reutiliza a mesma marca, e uma falha não a desfaz.
            for _ in 0..3 {
                uso.marcar_lancamento(boot).unwrap();
            }
            assert!(uso.marcar_lancamento(boot).is_ok());
            let pasta = local(&uso).pasta.clone();
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
        fs::remove_dir_all(&local(&uso).pasta).unwrap();
        let error = uso.marcar_lancamento(None).unwrap_err();
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
        let marcador = local(&uso).marcador.clone();
        let (entrou, espera_entrada) = tokio::sync::oneshot::channel();
        let (liberar, espera_saida) = std::sync::mpsc::channel();
        let tarefa = tokio::spawn(async move {
            tokio::task::spawn_blocking(move || {
                uso.marcar_lancamento(None).unwrap();
                entrou.send(()).unwrap();
                espera_saida.recv_timeout(Duration::from_secs(5)).unwrap();
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
        com_protecao_atual(raiz.path(), "agente", Some(&local(&uso).marcador), |_, protegidas| {
            assert!(!protegidas.unwrap().em_uso("1"));
            Ok(())
        })
        .unwrap();
        let _outro = Uso::registrar(raiz.path(), "agente", "1").unwrap();
        com_protecao_atual(raiz.path(), "agente", Some(&local(&uso).marcador), |_, protegidas| {
            assert!(protegidas.unwrap().em_uso("1"));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn descarta_reserva_mesmo_com_runtime_encerrado() {
        let raiz = tempfile::tempdir().unwrap();
        let uso = Uso::registrar(raiz.path(), "agente", "1").unwrap();
        let marcador = local(&uso).marcador.clone();
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
        let viva = local(&uso).marcador.clone();
        let mut processo = std::process::Command::new("/bin/cat").stdin(std::process::Stdio::piped()).spawn().unwrap();
        let pid = processo.id();
        let identidade::Processo::Vivo(inicio) = identidade::processo(pid) else {
            panic!("início do processo não encontrado");
        };
        let orfa = criar_marcador(&local(&uso).pasta, &format!("reserva.{pid}.{inicio}"))
            .unwrap()
            .into_temp_path()
            .keep()
            .unwrap();
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
        let lease = criar_marcador(&local(&uso).pasta, "lease").unwrap().into_temp_path().keep().unwrap();
        let incerto = local(&uso).pasta.join("lease.diretorio");
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
        // Sem recibo nem marcadores não existe prova de desuso: nada é removido.
        let vazia = protegidas(raiz.path(), "agente").unwrap();
        assert!(vazia.em_uso.is_empty() && vazia.registradas.is_empty());
        limpar_versoes(&atual, protegidas(raiz.path(), "agente").as_ref());
        assert!(antiga.is_dir() && primeira.is_dir());

        // A versão instalada por este protocolo passa a ter recibo de publicação.
        // Versão publicada por este protocolo: a árvore carrega o recibo.
        let bytes = b"fixture";
        let hash = super::super::sha256_hex(bytes);
        super::super::arquivo::publicar(&primeira, "adapter", bytes, &hash, Path::new("adapter"), 1024).unwrap();
        super::super::arquivo::publicar(&atual, "adapter", bytes, &hash, Path::new("adapter"), 1024).unwrap();
        limpar_versoes(&atual, protegidas(raiz.path(), "agente").as_ref());
        assert!(!primeira.exists(), "versão registrada e livre deveria sair");
        assert!(antiga.is_dir(), "instalação anterior ao protocolo não tem prova de desuso");
        assert!(atual.is_dir());
    }

    #[test]
    fn recibo_legado_na_arvore_nao_vale_como_prova_nova() {
        let raiz = tempfile::tempdir().unwrap();
        let agente = raiz.path().join("agents").join("agente");
        let legada = agente.join("1.0.0");
        let atual = agente.join("2.0.0");
        for pasta in [&legada, &atual] {
            fs::create_dir_all(pasta).unwrap();
            fs::write(pasta.join("adapter"), b"fixture").unwrap();
        }
        // Pacote antigo com marcador de instalação e um arquivo `.publicada` qualquer:
        // nem vazio, nem com a versão nova e resumo errado, o recibo legado prova algo.
        let marcador = "a".repeat(128);
        fs::write(legada.join(".installed"), &marcador).unwrap();
        for conteudo in [
            Vec::new(),
            b"mexerica-publicada-v1\n".to_vec(),
            format!("mexerica-publicada-v1\n{}\n", "b".repeat(64)).into_bytes(),
        ] {
            fs::write(legada.join(super::super::arquivo::RECIBO), &conteudo).unwrap();
            let protegidas = protegidas(raiz.path(), "agente").unwrap();
            assert!(protegidas.registradas.is_empty(), "recibo legado não pode registrar a versão");
            limpar_versoes(&atual, Some(&protegidas));
            assert!(legada.is_dir(), "versão com recibo legado fica preservada");
        }
    }

    #[test]
    fn entrada_inesperada_nos_marcadores_preserva_a_versao() {
        let raiz = tempfile::tempdir().unwrap();
        let agente = raiz.path().join("agents").join("agente");
        let velha = agente.join("1");
        let atual = agente.join("2");
        for pasta in [&velha, &atual] {
            fs::create_dir_all(pasta).unwrap();
        }
        let bytes = b"fixture";
        let hash = super::super::sha256_hex(bytes);
        super::super::arquivo::publicar(&velha, "adapter", bytes, &hash, Path::new("adapter"), 1024).unwrap();
        super::super::arquivo::publicar(&atual, "adapter", bytes, &hash, Path::new("adapter"), 1024).unwrap();
        // Um arquivo onde deveria estar o diretório de marcadores da versão com recibo.
        let marcadores = raiz.path().join(MARCADORES).join("agente");
        fs::create_dir_all(&marcadores).unwrap();
        fs::write(marcadores.join("1"), "inesperado").unwrap();
        let protegidas = protegidas(raiz.path(), "agente");
        assert!(protegidas.is_none(), "entrada inesperada não pode liberar a versão");
        limpar_versoes(&atual, protegidas.as_ref());
        assert!(velha.is_dir() && atual.is_dir());
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
        // Erro de leitura determinístico, sem depender de permissão nem de UID: um arquivo
        // onde o coletor espera o diretório de marcadores do agente.
        let marcadores = raiz.path().join(MARCADORES).join("agente");
        fs::remove_dir_all(&marcadores).unwrap();
        fs::write(&marcadores, "não é diretório").unwrap();
        let protegidas = protegidas(raiz.path(), "agente");
        assert!(protegidas.is_none(), "erro de I/O não pode virar lista vazia");
        // Direto no coletor: é o ramo de erro de leitura dele que está sob prova,
        // não só a recusa anterior da trava.
        assert!(listar_protegidas(raiz.path(), "agente", None, None).is_none());
        limpar_versoes(&atual, protegidas.as_ref());
        assert!(velha.is_dir() && atual.is_dir());
    }

    #[test]
    fn versao_chamada_trava_nao_colide_com_o_arquivo_da_trava() {
        let raiz = tempfile::tempdir().unwrap();
        // A trava do agente abre (e fecha) e a reserva de uma versão comum nasce ao
        // lado do arquivo dela.
        drop(Trava::abrir(raiz.path(), "agente").unwrap());
        assert!(Uso::registrar(raiz.path(), "agente", "1").is_ok());
        // A versão com o nome do arquivo da trava é recusada antes de criar pasta.
        let erro = Uso::registrar(raiz.path(), "agente", "trava").err().map(|e| e.to_string()).unwrap_or_default();
        assert!(erro.contains("criar_pasta"), "{erro}");
        assert!(raiz.path().join(MARCADORES).join("agente").join("trava").is_file());
    }

    #[test]
    fn trava_do_so_serializa_usos_da_mesma_raiz() {
        use fs2::FileExt as _;
        use std::sync::mpsc;
        let raiz = tempfile::tempdir().unwrap();
        let (segurou, aviso) = mpsc::channel();
        let (liberar, espera) = mpsc::channel();
        let caminho = raiz.path().to_path_buf();
        let dono = std::thread::spawn(move || {
            let _trava = Trava::abrir(&caminho, "agente").unwrap();
            segurou.send(()).unwrap();
            espera.recv().unwrap();
        });
        aviso.recv().unwrap();
        // Sem medir tempo: enquanto o dono segura, a tentativa não bloqueante falha.
        let arquivo = abrir_lock(&raiz.path().join(MARCADORES).join("agente").join("trava")).unwrap();
        assert_eq!(arquivo.try_lock_exclusive().unwrap_err().kind(), ErrorKind::WouldBlock);
        liberar.send(()).unwrap();
        dono.join().unwrap();
        arquivo.try_lock_exclusive().unwrap();
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
            let trava = tokio::task::spawn_blocking(move || Trava::abrir(&caminho, "agente")).await.unwrap().unwrap();
            assert!(inicio.elapsed() >= Duration::from_millis(200), "a trava não excluiu o outro processo");
            drop(trava);
            assert!(filho.wait().unwrap().success());
            return;
        }
        let raiz = PathBuf::from(std::env::var_os(FIXTURE).unwrap());
        let aviso = PathBuf::from(std::env::var_os(AVISO).unwrap());
        let _trava = Trava::abrir(&raiz, "agente").unwrap();
        fs::write(&aviso, "").unwrap();
        std::thread::sleep(Duration::from_millis(600));
    }
}
