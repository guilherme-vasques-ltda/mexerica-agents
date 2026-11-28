//! Fechamento stdio como03a: prefixo incompleto não admite um frame MCP.
use anyhow::{Result, ensure};
use serde_json::Value;
use std::{
    future::{Future, poll_fn},
    pin::Pin,
    task::{Poll, ready},
    time::Duration,
};
use tokio::io::{AsyncWrite, AsyncWriteExt};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Begin,
    Commit,
}

/// `ChildStdin` usa pipe POSIX: os dois bytes finais cabem na escrita atômica.
/// Se o pipe esperar, repete a guarda do MESMO pedido antes de tentar fechar.
/// Nunca mantém trava síncrona durante I/O ou RPC; conserva os30s originais.
pub(crate) async fn write<W, G, Guard>(
    writer: &mut W,
    frame: &Value,
    id: &str,
    method: &str,
    guard: &mut G,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
    G: FnMut(String, String, Phase, Value) -> Guard,
    Guard: Future<Output = Result<()>>,
{
    let mut bytes = serde_json::to_vec(frame)?;
    bytes.push(b'\n');
    ensure!(bytes.ends_with(b"}\n"), "mcp_request_frame_invalid");
    let (prefix, commit) = bytes.split_at(bytes.len() - 2);
    tokio::time::timeout(Duration::from_secs(30), async {
        writer.write_all(prefix).await?;
        let mut admission = None;
        poll_fn(|cx| {
            let check = admission.get_or_insert_with(|| {
                Box::pin(guard(id.into(), method.into(), Phase::Commit, frame["params"].clone()))
            });
            ready!(check.as_mut().poll(cx))?;
            admission = None;
            poll_commit(writer, cx, commit)
        })
        .await?;
        writer.flush().await?;
        Ok::<(), anyhow::Error>(())
    })
    .await
    .map_err(|_| anyhow::anyhow!("Prazo da ponte MCP excedido"))?
}

pub(crate) fn poll_commit<W: AsyncWrite + Unpin>(
    writer: &mut W,
    cx: &mut std::task::Context<'_>,
    commit: &[u8],
) -> Poll<Result<()>> {
    let written = ready!(Pin::new(writer).poll_write(cx, commit))?;
    if written != commit.len() {
        return Poll::Ready(Err(anyhow::anyhow!("mcp_request_non_atomic_commit")));
    }
    Poll::Ready(Ok(()))
}
