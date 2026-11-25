//! Thread única e permanente que cria os processos do motor.
//!
//! `PR_SET_PDEATHSIG` do ajudante acompanha a thread que criou o filho, não o processo
//! (ver `PR_SET_PDEATHSIG(2)`). Criar o agente numa thread do pool de bloqueio do Tokio,
//! que se aposenta ociosa, mataria a sessão com o Hub ainda vivo. Por isso todo processo
//! nasce nesta thread, que só termina junto com o Hub.
use anyhow::Result;
use std::sync::mpsc;

type Tarefa = Box<dyn FnOnce() + Send + 'static>;

/// Fila da thread criadora. Uma falha transitória na criação da thread não é permanente:
/// a próxima tentativa cria de novo, em vez de condenar todos os lançamentos até reiniciar.
static FILA: std::sync::Mutex<Option<mpsc::Sender<Tarefa>>> = std::sync::Mutex::new(None);

fn fila() -> Result<mpsc::Sender<Tarefa>> {
    use crate::LockExt as _;
    let mut fila = FILA.locked();
    if let Some(enviar) = fila.as_ref() {
        return Ok(enviar.clone());
    }
    let (enviar, receber) = mpsc::channel::<Tarefa>();
    std::thread::Builder::new()
        .name("motor-criador".into())
        .spawn(move || {
            while let Ok(tarefa) = receber.recv() {
                // Um chamador que entra em pânico não pode derrubar a thread dos demais.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(tarefa));
            }
        })
        .map_err(|erro| anyhow::anyhow!("thread de criação de processos indisponível: {erro}"))?;
    *fila = Some(enviar.clone());
    Ok(enviar)
}

/// Sinaliza que o chamador desistiu antes de a tarefa começar.
struct Desistencia(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Drop for Desistencia {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
}

/// Roda `f` na thread criadora e devolve o resultado ao chamador.
///
/// Um chamador cancelado antes da vez da sua tarefa não cria processo: a tarefa recusa o
/// trabalho não iniciado. Se o chamador sumir depois de a tarefa começar, `recolher`
/// recebe o resultado para encerrar o processo criado; nada fica sem dono.
pub(crate) async fn criar<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
    recolher: impl FnOnce(T) + Send + 'static,
) -> Result<T> {
    // O `tokio::process` precisa do reator do runtime do Hub, mesmo nesta thread.
    let runtime = tokio::runtime::Handle::current();
    let desistiu = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sinal = desistiu.clone();
    let (enviar, receber) = tokio::sync::oneshot::channel();
    let tarefa: Tarefa = Box::new(move || {
        if desistiu.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        let _ativa = runtime.enter();
        // A entrega viaja com o recolhimento: se o chamador sumir a qualquer momento —
        // inclusive depois de o envio entrar no canal —, descartar a entrega recolhe.
        let _ = enviar.send(Entrega { resultado: Some(f()), recolher: Some(recolher) });
    });
    if fila()?.send(tarefa).is_err() {
        use crate::LockExt as _;
        // A thread morreu depois de criada: a próxima chamada recria a fila.
        *FILA.locked() = None;
        anyhow::bail!("thread de criação de processos encerrada");
    }
    // Cancelar o chamador derruba este futuro: a desistência marca a tarefa enfileirada.
    let _desistencia = Desistencia(sinal);
    let mut entrega = receber.await.map_err(|_| anyhow::anyhow!("criação de processo não concluída"))?;
    entrega.resultado.take().ok_or_else(|| anyhow::anyhow!("criação de processo não concluída"))
}

/// Resultado em trânsito com seu recolhimento: descartar sem consumir recolhe o processo.
struct Entrega<T, R: FnOnce(T)> {
    resultado: Option<T>,
    recolher: Option<R>,
}

impl<T, R: FnOnce(T)> Drop for Entrega<T, R> {
    fn drop(&mut self) {
        if let (Some(resultado), Some(recolher)) = (self.resultado.take(), self.recolher.take()) {
            recolher(resultado);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn viva(pid: u32) -> bool {
        // SAFETY: sinal 0 só consulta a existência do PID.
        (unsafe { libc::kill(pid.cast_signed(), 0) }) == 0
    }

    #[tokio::test]
    async fn processos_nascem_na_thread_dedicada_e_nunca_na_do_chamador() {
        let chamador = std::thread::current().id();
        let primeira =
            criar(|| (std::thread::current().name().map(str::to_owned), std::thread::current().id()), |_| {})
                .await
                .unwrap();
        let segunda = criar(|| std::thread::current().id(), |_| {}).await.unwrap();
        assert_eq!(primeira.0.as_deref(), Some("motor-criador"));
        assert_ne!(primeira.1, chamador);
        assert_eq!(primeira.1, segunda);
    }

    #[tokio::test]
    async fn filho_sobrevive_ao_chamador_e_a_criacao_nao_derruba_o_processo() {
        let mut comando = tokio::process::Command::new("/bin/sh");
        comando.args(["-c", "exec sleep 30"]);
        let filho = criar(move || crate::fork::motor_exec::spawn(&mut comando), |_| {}).await.unwrap().unwrap();
        let pid = filho.id().unwrap();
        drop(filho);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(viva(pid), "agente morreu junto com o chamador");
        // SAFETY: o PID pertence ao filho ainda vivo deste teste.
        assert_eq!(unsafe { libc::kill(pid.cast_signed(), libc::SIGKILL) }, 0);
    }

    #[tokio::test]
    async fn panico_na_tarefa_nao_derruba_a_thread_criadora() {
        // O pânico fica contido na thread criadora; o chamador recebe erro e a fila continua.
        assert!(criar(|| panic!("falha proposital"), |_| {}).await.is_err());
        assert_eq!(criar(|| 1u32 + 1, |_| {}).await.unwrap(), 2);
    }

    /// Segura a thread criadora até o teste mandar, para enfileirar tarefas atrás dela.
    async fn segurar_a_fila() -> (tokio::task::JoinHandle<Result<()>>, mpsc::Sender<()>) {
        let (liberar, esperando) = mpsc::channel();
        let fila = criar(
            move || {
                let _ = esperando.recv();
            },
            |()| {},
        );
        let tarefa = tokio::spawn(fila);
        tokio::time::sleep(Duration::from_millis(100)).await;
        (tarefa, liberar)
    }

    #[test]
    fn entrega_descartada_sem_consumo_recolhe_e_consumida_nao() {
        let raiz = tempfile::tempdir().unwrap();
        let recolhido = raiz.path().join("recolhido");
        let caminho = recolhido.clone();
        drop(Entrega {
            resultado: Some(1u32),
            recolher: Some(move |_: u32| {
                std::fs::write(&caminho, "").unwrap();
            }),
        });
        assert!(recolhido.exists(), "descartar a entrega deveria recolher");
        let mut entrega = Entrega { resultado: Some(2u32), recolher: Some(|_: u32| panic!("não recolhe")) };
        assert_eq!(entrega.resultado.take(), Some(2));
        drop(entrega);
    }

    #[tokio::test]
    async fn tarefa_cancelada_antes_de_comecar_nao_roda() {
        let (_, liberar) = segurar_a_fila().await;
        let raiz = tempfile::tempdir().unwrap();
        let marcado = raiz.path().join("criado");
        let caminho = marcado.clone();
        let mut pedido = Box::pin(criar(
            move || {
                std::fs::write(&caminho, "").unwrap();
                1u32
            },
            |_| {},
        ));
        // Cancela antes de a tarefa ter a vez: o trabalho não pode ser executado.
        assert!(futures::poll!(&mut pedido).is_pending());
        drop(pedido);
        liberar.send(()).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!marcado.exists(), "tarefa cancelada criou o arquivo mesmo assim");
    }

    #[tokio::test]
    async fn entrega_falhada_recolhe_o_resultado() {
        let (_, liberar) = segurar_a_fila().await;
        let raiz = tempfile::tempdir().unwrap();
        let recolhido = raiz.path().join("recolhido");
        let (fim_do_f, entrar_f) = mpsc::channel();
        let (f_pronto, f_comecou) = mpsc::channel();
        // `f` segura a thread criadora até o chamador sumir: a entrega falha com o
        // trabalho já em andamento, e o resultado tem de ser recolhido.
        let caminho = recolhido.clone();
        let mut pedido = Box::pin(criar(
            move || {
                let _ = f_pronto.send(());
                let _ = entrar_f.recv();
                1u32
            },
            move |_| {
                std::fs::write(&caminho, "").unwrap();
            },
        ));
        assert!(futures::poll!(&mut pedido).is_pending());
        liberar.send(()).unwrap();
        f_comecou.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(pedido);
        fim_do_f.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !recolhido.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
}
