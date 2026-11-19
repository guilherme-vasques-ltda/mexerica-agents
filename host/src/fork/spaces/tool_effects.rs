//! Commit de uma resposta ACP sem limite adicional para ID ou `optionId`.
use crate::LockExt;
use anyhow::{Result, ensure};
use std::{future::poll_fn, pin::Pin, sync::Mutex, task::Poll};
use tokio::io::AsyncWrite;

/// O chamador usa `ChildStdin`: os dois bytes finais cabem na escrita atômica
/// mínima do pipe POSIX. Retemos a chave obrigatória de fechamento, não só a
/// newline: nenhum prefixo anterior é JSON válido, nem no EOF. O corpo pode ter
/// qualquer tamanho. Cada poll revalida e libera a barreira antes de `Pending`.
pub(crate) async fn write_checked<W: AsyncWrite + Unpin>(
    writer: &mut W,
    line: &[u8],
    boundary: &Mutex<()>,
    check: impl Fn() -> Result<()>,
) -> Result<()> {
    write_scoped(writer, line, |effect| {
        let _boundary = boundary.locked();
        if let Err(error) = check() {
            return Poll::Ready(Err(error));
        }
        effect()
    })
    .await
}

/// O escopo conserva as fontes canônicas até o `poll_write`, soltando-as em Pending.
/// Não basta validar e soltar a Connection antes da escrita concorrente ao commit.
pub(crate) async fn write_scoped<W: AsyncWrite + Unpin>(
    writer: &mut W,
    line: &[u8],
    scope: impl Fn(&mut dyn FnMut() -> Poll<Result<()>>) -> Poll<Result<()>>,
) -> Result<()> {
    let mut started = false;
    write_scoped_progress(writer, line, scope, &mut started).await
}

/// Conserva progresso real para o consumer decidir se pode emitir erro no mesmo pipe.
pub(crate) async fn write_scoped_progress<W: AsyncWrite + Unpin>(
    writer: &mut W,
    line: &[u8],
    scope: impl Fn(&mut dyn FnMut() -> Poll<Result<()>>) -> Poll<Result<()>>,
    started: &mut bool,
) -> Result<()> {
    ensure!(line.ends_with(b"}\n"), "acp_permission_frame_invalid");
    let (prefix, commit) = line.split_at(line.len() - 2);
    let mut offset = 0;
    poll_fn(|cx| {
        scope(&mut || {
            while offset < prefix.len() {
                match Pin::new(&mut *writer).poll_write(cx, &prefix[offset..]) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(error)) => return Poll::Ready(Err(error.into())),
                    Poll::Ready(Ok(0)) => return Poll::Ready(Err(anyhow::anyhow!("acp_permission_write_zero"))),
                    Poll::Ready(Ok(written)) => {
                        *started = true;
                        offset += written;
                    }
                }
            }
            // O fechamento e a newline são admitidos juntos, sob a validação acima.
            Pin::new(&mut *writer).poll_write(cx, commit).map(|result| {
                let written = result?;
                *started |= written > 0;
                ensure!(written == commit.len(), "acp_permission_non_atomic_commit");
                Ok(())
            })
        })
    })
    .await
}
