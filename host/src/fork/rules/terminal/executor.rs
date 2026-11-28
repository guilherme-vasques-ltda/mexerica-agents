use super::Decisao;
use crate::fork::sandbox::{
    contrato::{Limites, Saida},
    processo::{Graca, Lancamento, rodar_guardado},
};
use anyhow::{Context as _, Result, ensure};
use std::{path::PathBuf, time::Duration};
use tokio::{process::Command, sync::oneshot, time::Instant};

const PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin";
const APPROVAL_TTL: Duration = Duration::from_secs(120);
const LIMITS: Limites =
    Limites { tempo: Duration::from_secs(120), saida_bytes: 40 * 1024, carencia: Duration::from_millis(100) };

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Policy {
    pub(crate) decision: Decisao,
    pub(crate) revision: i64,
    pub(crate) enabled_generation: u64,
}

pub(crate) struct Prepared {
    argv: Vec<String>,
    cwd: PathBuf,
    admitted: Policy,
    expires_at: Instant,
}

impl Prepared {
    pub(crate) fn opaque(argv: Vec<String>, cwd: PathBuf, admitted: Policy) -> Result<Self> {
        ensure!(admitted.enabled_generation != 0, "comandos locais desligados");
        ensure!(admitted.decision == Decisao::Perguntar, "comando sem autorização");
        ensure!(cwd.is_absolute() && cwd.is_dir(), "pasta de trabalho indisponível");
        ensure!(
            argv.first().is_some_and(|first| {
                !first.is_empty() && (!first.contains('/') || std::path::Path::new(first).is_absolute())
            }),
            "executável vazio ou caminho relativo"
        );
        ensure!(
            argv.len() <= 64
                && argv.iter().all(|part| !part.chars().any(char::is_control))
                && argv.iter().map(String::len).sum::<usize>() <= 4096,
            "argumentos de comando inválidos"
        );
        Ok(Self { argv, cwd, admitted, expires_at: Instant::now() + APPROVAL_TTL })
    }

    pub(crate) fn argv(&self) -> &[String] {
        &self.argv
    }

    pub(crate) fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }

    pub(crate) async fn execute(
        self,
        approval: oneshot::Receiver<bool>,
        start_fenced: impl FnOnce(&mut dyn FnMut(Policy) -> Result<tokio::process::Child>) -> Result<tokio::process::Child>,
    ) -> Result<Saida> {
        ensure!(
            matches!(tokio::time::timeout_at(self.expires_at, approval).await, Ok(Ok(true))),
            "comando não aprovado"
        );
        let (program, args) = self.argv.split_first().context("comando vazio")?;
        let mut command = Command::new(program);
        command.args(args).current_dir(&self.cwd).env_clear().env("PATH", PATH);
        let mut unused = true;
        rodar_guardado(Lancamento { command, manter: Vec::new(), graca: Graca::Grupo }, LIMITS, |command| {
            start_fenced(&mut |now| {
                ensure!(std::mem::take(&mut unused), "a autorização já foi usada");
                ensure!(now == self.admitted, "política ou autorização alterada");
                ensure!(Instant::now() < self.expires_at, "aprovação expirada");
                command.spawn().context("não foi possível iniciar o comando")
            })
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fork::sandbox::contrato::Fim;

    const ASK: Policy = Policy { decision: Decisao::Perguntar, revision: 1, enabled_generation: 1 };

    #[tokio::test]
    async fn sem_aprovacao_ou_com_regra_alterada_o_processo_nao_inicia() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let output = directory.path().join("prova");
        let job = || Prepared::opaque(vec!["/usr/bin/touch".into(), "prova".into()], directory.path().into(), ASK);
        let (send, receive) = oneshot::channel();
        drop(send);
        assert!(job()?.execute(receive, |_| unreachable!()).await.is_err());
        let (send, receive) = oneshot::channel();
        send.send(false).expect("receiver alive");
        assert!(job()?.execute(receive, |_| unreachable!()).await.is_err());
        for changed in [Policy { revision: 2, ..ASK }, Policy { enabled_generation: 0, ..ASK }] {
            let (send, receive) = oneshot::channel();
            send.send(true).expect("receiver alive");
            assert!(job()?.execute(receive, |start| start(changed)).await.is_err());
        }
        let (send, receive) = oneshot::channel();
        send.send(true).expect("receiver alive");
        assert!(job()?.execute(receive, |_| anyhow::bail!("turno cancelado")).await.is_err());
        assert!(!output.exists());
        let (send, receive) = oneshot::channel();
        send.send(true).expect("receiver alive");
        let result = job()?
            .execute(receive, |start| {
                let first = start(ASK)?;
                assert!(start(ASK).is_err());
                Ok(first)
            })
            .await?;
        assert!(matches!(result.fim, Fim::Saiu(0)));
        assert!(output.is_file());
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn aprovacao_vencida_nao_inicia_processo() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let job = Prepared::opaque(vec!["/usr/bin/touch".into(), "prova".into()], directory.path().into(), ASK)?;
        tokio::time::advance(APPROVAL_TTL).await;
        let (send, receive) = oneshot::channel();
        send.send(true).expect("receiver alive");
        assert!(job.execute(receive, |start| start(ASK)).await.is_err());
        assert!(!directory.path().join("prova").exists());
        Ok(())
    }
}
