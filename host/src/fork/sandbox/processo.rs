//! Execução comum aos provedores locais: grupo de processos próprio, prazo, teto de saída
//! e watchdog que mata o grupo inteiro ao fim. Processo que sai do grupo (`setsid`)
//! escapa deste watchdog; provedores com PID namespace (bwrap) cobrem esse caso.
//!
//! Garantias:
//! - o líder do grupo nunca é recolhido antes de a sinalização terminar (o zumbi mantém o
//!   PGID reservado; `killpg` não atinge grupo alheio);
//! - qualquer saída de [`rodar`] (fim, erro ou cancelamento da future) mata o grupo e aborta
//!   os leitores;
//! - no prazo, o grupo recebe SIGTERM e a carência corre inteira antes do SIGKILL (só no
//!   `ProcessoLocal`; no bwrap o prazo é SIGKILL imediato do monitor, que derruba o namespace).
use super::contrato::{Fim, Limites, Saida};
use anyhow::{Context as _, Result};
use std::{
    fs::File,
    os::unix::process::ExitStatusExt as _,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt as _},
    process::Command,
    task::{AbortHandle, JoinHandle},
    time::Instant,
};

/// Intervalo de observação da saída do líder (sem recolhê-lo).
const INTERVALO: Duration = Duration::from_millis(15);
/// Piso da drenagem: tempo para os leitores esvaziarem pipes já fechados quando a carência é 0.
const DRENAGEM_MIN: Duration = Duration::from_millis(50);

/// Como o prazo vira término.
pub(crate) enum Graca {
    /// O grupo do líder recebe SIGTERM e a carência corre inteira.
    Grupo,
    /// SIGKILL imediato (bwrap): o comando efetivo vive em outro PID namespace e sinalizá-lo
    /// exigiria um PID lido de `/proc`, sujeito a reuso. O PID namespace leva tudo.
    Nenhuma,
}

/// Comando pronto para lançar, mais os descritores que precisam viver até o spawn.
pub(crate) struct Lancamento {
    pub(crate) command: Command,
    /// Descritores herdados pelo filho (montagens ancoradas); fechados no pai após o spawn.
    pub(crate) manter: Vec<File>,
    pub(crate) graca: Graca,
}

/// Guarda de saída: em qualquer caminho (inclusive cancelamento ou `?`) mata o grupo e aborta
/// os leitores. Só é desarmada depois de o líder ser recolhido, ou se o PGID deixou de ser nosso.
struct Guarda {
    grupo: i32,
    armada: bool,
    tarefas: Vec<AbortHandle>,
}
impl Guarda {
    fn desarmar(&mut self) {
        self.armada = false;
    }
}
impl Drop for Guarda {
    fn drop(&mut self) {
        if self.armada {
            sinalizar(self.grupo, libc::SIGKILL);
        }
        for tarefa in &self.tarefas {
            tarefa.abort();
        }
    }
}

/// Roda o comando até o fim ou o prazo.
pub(crate) async fn rodar(lancamento: Lancamento, limites: Limites) -> Result<Saida> {
    rodar_guardado(lancamento, limites, |command| command.spawn().context("sandbox_spawn_falhou")).await
}

pub(crate) async fn rodar_guardado(
    lancamento: Lancamento,
    limites: Limites,
    spawn: impl FnOnce(&mut Command) -> Result<tokio::process::Child>,
) -> Result<Saida> {
    let Lancamento { mut command, manter, graca } = lancamento;
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0).kill_on_drop(true);
    let inicio = std::time::Instant::now();
    let mut child = spawn(&mut command)?;
    drop(manter);
    let pid = child.id().and_then(|p| i32::try_from(p).ok()).context("sandbox_sem_pid")?;
    // Declarada depois de `child`: cai antes dele, com o líder ainda sem recolher.
    let mut guarda = Guarda { grupo: pid, armada: true, tarefas: Vec::new() };
    let orcamento = Arc::new(AtomicUsize::new(limites.saida_bytes));
    let stdout = ler(child.stdout.take().context("sandbox_sem_stdout")?, orcamento.clone());
    guarda.tarefas.push(stdout.tarefa.abort_handle());
    let stderr = ler(child.stderr.take().context("sandbox_sem_stderr")?, orcamento.clone());
    guarda.tarefas.push(stderr.tarefa.abort_handle());

    let no_prazo = match aguardar_saida(pid, limites.tempo).await {
        Ok(saiu) => saiu,
        Err(erro) => {
            // Só a prova de que o líder já foi recolhido (ECHILD) tira o PGID de nós. Qualquer
            // outra falha de observação mantém a guarda armada: o Drop mata o grupo.
            if erro.downcast_ref::<LiderRecolhido>().is_some() {
                guarda.desarmar();
            }
            return Err(erro);
        }
    };
    if !no_prazo {
        parar(&graca, pid, limites.carencia).await;
    }
    // Watchdog: filhos deixados no grupo não sobrevivem à execução nem seguram a saída.
    // O líder (vivo ou zumbi) ainda não foi recolhido: o PGID segue nosso.
    sinalizar(pid, libc::SIGKILL);
    let status = child.wait().await;
    guarda.desarmar();
    let status = status.context("sandbox_wait_falhou")?;

    let (stdout, stderr, aberta) = drenar(stdout, stderr, limites.carencia).await;
    let fim = if no_prazo {
        status.code().map_or_else(|| Fim::Sinal(status.signal().unwrap_or(0)), Fim::Saiu)
    } else {
        Fim::TempoEsgotado
    };
    // O orçamento é compartilhado durante a captura: a soma nunca passa do teto.
    let truncada = stdout.1 || stderr.1;
    Ok(Saida { fim, stdout: stdout.0, stderr: stderr.0, truncada, incompleta: aberta, duracao: inicio.elapsed() })
}

/// Prazo vencido: término gracioso quando o provedor permite, depois a carência inteira.
async fn parar(graca: &Graca, pid: i32, carencia: Duration) {
    match graca {
        Graca::Grupo => {
            sinalizar(pid, libc::SIGTERM);
            // Sem recolher não há como saber se membros seguem vivos: a carência corre inteira.
            tokio::time::sleep(carencia).await;
        }
        Graca::Nenhuma => {}
    }
}

fn sinalizar(grupo: i32, sinal: i32) {
    // SAFETY: grupo criado por `process_group(0)` neste spawn e líder ainda não recolhido (o
    // zumbi mantém o PGID reservado); ESRCH (grupo vazio) é ignorado.
    unsafe { libc::killpg(grupo, sinal) };
}

/// Espera o líder sair SEM recolhê-lo (`WNOWAIT`): o zumbi segura o PGID até a sinalização
/// terminar. `true` se saiu até `prazo`.
pub(crate) async fn aguardar_saida(pid: i32, prazo: Duration) -> Result<bool> {
    let limite = Instant::now() + prazo;
    loop {
        if saiu_sem_recolher(pid)? {
            return Ok(true);
        }
        let agora = Instant::now();
        if agora >= limite {
            return Ok(false);
        }
        tokio::time::sleep((limite - agora).min(INTERVALO)).await;
    }
}

/// O líder já foi recolhido por outro (ECHILD): o PGID deixou de ser reservado por nós.
#[derive(Debug)]
struct LiderRecolhido;
impl std::fmt::Display for LiderRecolhido {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("sandbox_lider_ja_recolhido")
    }
}
impl std::error::Error for LiderRecolhido {}

/// Resultado de uma chamada a `waitid`, classificado.
#[derive(Debug, PartialEq, Eq)]
enum Observacao {
    Saiu,
    Vivo,
    /// EINTR: tentar de novo.
    Repetir,
    /// ECHILD: líder comprovadamente recolhido.
    Recolhido,
    /// Qualquer outro errno: não prova nada sobre o líder.
    Falha(i32),
}

/// `erro` é o errno quando `waitid` devolveu -1; `si_pid` vale quando devolveu 0.
fn classificar_waitid(erro: Option<i32>, si_pid: i32) -> Observacao {
    match erro {
        None if si_pid != 0 => Observacao::Saiu,
        None => Observacao::Vivo,
        Some(libc::EINTR) => Observacao::Repetir,
        Some(libc::ECHILD) => Observacao::Recolhido,
        Some(outro) => Observacao::Falha(outro),
    }
}

fn saiu_sem_recolher(pid: i32) -> Result<bool> {
    loop {
        // SAFETY: siginfo_t é dado simples; zerado, `si_pid == 0` significa "ninguém mudou de estado".
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: `info` válido para escrita; WNOWAIT deixa o filho como zumbi (sem recolher),
        // WNOHANG não bloqueia.
        let r = unsafe {
            libc::waitid(libc::P_PID, pid.cast_unsigned(), &raw mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT)
        };
        let erro = (r != 0).then(|| std::io::Error::last_os_error().raw_os_error().unwrap_or(0));
        match classificar_waitid(erro, if r == 0 { si_pid(&info) } else { 0 }) {
            Observacao::Saiu => return Ok(true),
            Observacao::Vivo => return Ok(false),
            Observacao::Repetir => {}
            Observacao::Recolhido => return Err(LiderRecolhido.into()),
            Observacao::Falha(codigo) => {
                return Err(
                    anyhow::Error::new(std::io::Error::from_raw_os_error(codigo)).context("sandbox_waitid_falhou")
                );
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn si_pid(info: &libc::siginfo_t) -> i32 {
    // SAFETY: leitura da união preenchida pelo kernel em waitid (SIGCHLD).
    unsafe { info.si_pid() }
}
#[cfg(not(target_os = "linux"))]
fn si_pid(info: &libc::siginfo_t) -> i32 {
    info.si_pid
}

struct Leitura {
    buffer: Arc<Mutex<(Vec<u8>, bool)>>,
    tarefa: JoinHandle<()>,
}

/// Lê até o fim guardando só o que cabe no `orcamento` compartilhado (stdout e stderr somados);
/// drena o resto para o filho não travar.
fn ler(mut fonte: impl AsyncRead + Unpin + Send + 'static, orcamento: Arc<AtomicUsize>) -> Leitura {
    let buffer: Arc<Mutex<(Vec<u8>, bool)>> = Arc::default();
    let destino = buffer.clone();
    let tarefa = tokio::spawn(async move {
        let mut bloco = vec![0_u8; 8192];
        while let Ok(n @ 1..) = fonte.read(&mut bloco).await {
            let cabe = reservar(&orcamento, n);
            let Ok(mut guardado) = destino.lock() else { return };
            guardado.0.extend_from_slice(&bloco[..cabe]);
            guardado.1 |= cabe < n;
        }
    });
    Leitura { buffer, tarefa }
}

/// Reserva até `n` bytes do orçamento; devolve quantos couberam.
fn reservar(orcamento: &AtomicUsize, n: usize) -> usize {
    let mut atual = orcamento.load(Ordering::Acquire);
    loop {
        let cabe = atual.min(n);
        match orcamento.compare_exchange_weak(atual, atual - cabe, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return cabe,
            Err(visto) => atual = visto,
        }
    }
}

type Parcial = (Vec<u8>, bool);

/// Drena os dois pipes concorrentemente sob um único prazo (a carência, com piso curto para
/// os leitores esvaziarem pipes já fechados). Se um escapado segurar um pipe, devolve o parcial
/// e marca aberto. Terceira posição: algum pipe ainda aberto.
async fn drenar(stdout: Leitura, stderr: Leitura, carencia: Duration) -> (Parcial, Parcial, bool) {
    let abortar = [stdout.tarefa.abort_handle(), stderr.tarefa.abort_handle()];
    let (buffer_out, buffer_err) = (stdout.buffer, stderr.buffer);
    let espera = carencia.max(DRENAGEM_MIN);
    let ambas = async { tokio::join!(stdout.tarefa, stderr.tarefa) };
    let aberta = tokio::time::timeout(espera, ambas).await.is_err();
    for tarefa in abortar {
        tarefa.abort();
    }
    let tomar = |buffer: &Mutex<Parcial>| buffer.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
    (tomar(&buffer_out), tomar(&buffer_err), aberta)
}

#[cfg(test)]
mod testes {
    use super::*;
    use tokio::io::AsyncWriteExt as _;

    #[tokio::test]
    async fn orcamento_e_compartilhado_durante_a_captura() {
        let orcamento = Arc::new(AtomicUsize::new(1024));
        let ((mut a_in, a_out), (mut b_in, b_out)) = (tokio::io::duplex(4096), tokio::io::duplex(4096));
        let (a, b) = (ler(a_out, orcamento.clone()), ler(b_out, orcamento.clone()));
        let escrever = async {
            for _ in 0..50 {
                tokio::join!(async { a_in.write_all(&[1; 1000]).await.unwrap() }, async {
                    b_in.write_all(&[2; 1000]).await.unwrap();
                });
            }
            drop((a_in, b_in));
        };
        escrever.await;
        let ((bytes_a, cheio_a), (bytes_b, cheio_b), aberta) = drenar(a, b, Duration::from_secs(2)).await;
        assert!(!aberta);
        // 100 KB produzidos; o que foi GUARDADO (antes de qualquer corte final) cabe no teto.
        assert_eq!(bytes_a.len() + bytes_b.len(), 1024);
        assert!(cheio_a || cheio_b);
        assert_eq!(orcamento.load(Ordering::Acquire), 0);
    }

    #[test]
    fn reserva_nunca_passa_do_restante() {
        let orcamento = AtomicUsize::new(10);
        assert_eq!(reservar(&orcamento, 6), 6);
        assert_eq!(reservar(&orcamento, 6), 4);
        assert_eq!(reservar(&orcamento, 6), 0);
    }

    #[test]
    fn classifica_errno_do_waitid_sem_desarmar_por_engano() {
        assert_eq!(classificar_waitid(None, 42), Observacao::Saiu);
        assert_eq!(classificar_waitid(None, 0), Observacao::Vivo);
        assert_eq!(classificar_waitid(Some(libc::EINTR), 0), Observacao::Repetir);
        assert_eq!(classificar_waitid(Some(libc::ECHILD), 0), Observacao::Recolhido);
        for errno in [libc::EPERM, libc::EINVAL, libc::ENOSYS, libc::EFAULT] {
            assert_eq!(classificar_waitid(Some(errno), 0), Observacao::Falha(errno));
        }
    }

    /// Ordenação: o líder só é recolhido depois de a saída ser observada e o grupo sinalizado.
    #[tokio::test]
    async fn observa_a_saida_sem_recolher_o_lider() {
        let mut filho = Command::new("/bin/sh").args(["-c", "exit 3"]).process_group(0).spawn().unwrap();
        let pid = i32::try_from(filho.id().unwrap()).unwrap();
        assert!(aguardar_saida(pid, Duration::from_secs(5)).await.unwrap());
        // Ainda zumbi: o PGID segue reservado e observável repetidamente.
        assert!(aguardar_saida(pid, Duration::ZERO).await.unwrap());
        // SAFETY: sinal 0 só consulta; o zumbi não recolhido ainda existe.
        assert_eq!(unsafe { libc::kill(pid, 0) }, 0, "líder foi recolhido antes da hora");
        assert_eq!(filho.wait().await.unwrap().code(), Some(3));
        // Recolhido: waitid passa a falhar (ECHILD), nunca a reportar saída falsa; é essa a
        // prova que autoriza desarmar a guarda.
        let erro = aguardar_saida(pid, Duration::ZERO).await.unwrap_err();
        assert!(erro.downcast_ref::<LiderRecolhido>().is_some());
    }

    #[tokio::test]
    async fn aguardar_saida_respeita_o_prazo_com_lider_vivo() {
        let mut filho = Command::new("/bin/sleep").arg("30").kill_on_drop(true).spawn().unwrap();
        let pid = i32::try_from(filho.id().unwrap()).unwrap();
        let inicio = std::time::Instant::now();
        assert!(!aguardar_saida(pid, Duration::from_millis(120)).await.unwrap());
        assert!(inicio.elapsed() >= Duration::from_millis(120) && inicio.elapsed() < Duration::from_secs(3));
        filho.kill().await.unwrap();
    }
}
