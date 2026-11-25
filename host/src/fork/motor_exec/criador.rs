//! Thread única e permanente que cria os processos do motor.
//!
//! `PR_SET_PDEATHSIG` do ajudante acompanha a thread que criou o filho, não o processo
//! (man 2 PR_SET_PDEATHSIG). Criar o agente numa thread do pool de bloqueio do Tokio,
//! que se aposenta ociosa, mataria a sessão com o Hub ainda vivo. Por isso todo processo
//! nasce nesta thread, que só termina junto com o Hub.
use anyhow::Result;
use std::sync::{OnceLock, mpsc};

type Tarefa = Box<dyn FnOnce() + Send + 'static>;

static FILA: OnceLock<std::result::Result<mpsc::Sender<Tarefa>, String>> = OnceLock::new();

fn fila() -> Result<&'static mpsc::Sender<Tarefa>> {
    match FILA.get_or_init(|| {
        let (enviar, receber) = mpsc::channel::<Tarefa>();
        std::thread::Builder::new()
            .name("motor-criador".into())
            .spawn(move || {
                while let Ok(tarefa) = receber.recv() {
                    // Um chamador que entra em pânico não pode derrubar a thread dos demais.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(tarefa));
                }
            })
            .map(|_| enviar)
            .map_err(|erro| erro.to_string())
    }) {
        Ok(fila) => Ok(fila),
        Err(erro) => anyhow::bail!("thread de criação de processos indisponível: {erro}"),
    }
}

/// Roda `f` na thread criadora e devolve o resultado ao chamador.
pub(crate) async fn criar<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T> {
    let (enviar, receber) = tokio::sync::oneshot::channel();
    let tarefa: Tarefa = Box::new(move || {
        let _ = enviar.send(f());
    });
    fila()?.send(tarefa).map_err(|_| anyhow::anyhow!("thread de criação de processos encerrada"))?;
    receber.await.map_err(|_| anyhow::anyhow!("criação de processo não concluída"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Duration;

    fn viva(pid: u32) -> bool {
        // SAFETY: sinal 0 só consulta a existência do PID.
        (unsafe { libc::kill(pid.cast_signed(), 0) }) == 0
    }

    #[tokio::test]
    async fn processos_nascem_na_thread_dedicada_e_nunca_na_do_chamador() {
        let chamador = std::thread::current().id();
        let primeira = criar(|| (std::thread::current().name().map(str::to_owned), std::thread::current().id())).await.unwrap();
        let segunda = criar(|| std::thread::current().id()).await.unwrap();
        assert_eq!(primeira.0.as_deref(), Some("motor-criador"));
        assert_ne!(primeira.1, chamador);
        assert_eq!(primeira.1, segunda);
    }

    #[tokio::test]
    async fn filho_sobrevive_ao_chamador_e_a_criacao_nao_derruba_o_processo() {
        let mut comando = tokio::process::Command::new("/bin/sh");
        comando.args(["-c", "exec sleep 30"]);
        let filho = criar(move || crate::fork::motor_exec::spawn(&mut comando)).await.unwrap().unwrap();
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
        assert!(criar(|| panic!("falha proposital")).await.is_err());
        assert_eq!(criar(|| 1u32 + 1).await.unwrap(), 2);
    }
}
