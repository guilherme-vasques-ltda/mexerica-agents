//! D517-A: configuração explícita, capturada antes de `GStreamer`, portal e conexão.

use anyhow::{Result, bail};
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use tokio::net::UnixStream;

#[derive(Clone, Debug)]
pub(crate) struct Config {
    socket: PathBuf,
    state_dir: PathBuf,
}

impl Config {
    pub(crate) fn from_args(args: impl IntoIterator<Item = OsString>) -> Result<Self> {
        let mut args = args.into_iter();
        if args.next().as_deref() != Some(OsStr::new("--socket")) {
            bail!("configuração inválida: obrigatório --socket <caminho absoluto>");
        }
        let Some(socket) = args.next() else {
            bail!("configuração inválida: --socket exige um caminho absoluto");
        };
        if args.next().is_some() {
            bail!("configuração ambígua: esperado somente --socket <caminho absoluto>");
        }
        let bytes = socket.as_bytes();
        let path = PathBuf::from(&socket);
        // Preservar a resolução do kernel, inclusive pais relativos e symlinks do override.
        if !path.is_absolute()
            || bytes.contains(&0)
            || bytes.ends_with(b"/")
            || bytes.ends_with(b"/.")
            || bytes.ends_with(b"/..")
            || path.file_name().is_none()
        {
            bail!("socket inválido: use caminho absoluto de arquivo, sem NUL ou barra final");
        }
        let Some(parent) = path.parent() else {
            bail!("socket inválido: falta pasta de estado");
        };
        Ok(Self {
            state_dir: parent.to_path_buf(),
            socket: path,
        })
    }

    pub(crate) fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub(crate) async fn connect(&self) -> std::io::Result<UnixStream> {
        UnixStream::connect(&self.socket).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn d517_rejects_missing_invalid_and_ambiguous_arguments() {
        for args in [
            vec![],
            vec!["--socket"],
            vec!["--socket", ""],
            vec!["--socket", "relative/screen.sock"],
            vec!["--socket", "/"],
            vec!["--socket", "/tmp/."],
            vec!["--socket", "/tmp/.."],
            vec!["--socket", "/tmp/screen.sock/"],
            vec!["--socket", "/tmp/screen\0.sock"],
            vec!["--unknown", "/tmp/screen.sock"],
            vec!["--socket", "/tmp/a", "--socket", "/tmp/b"],
            vec!["--socket", "/tmp/a", "extra"],
        ] {
            assert!(
                Config::from_args(args.iter().map(|arg| OsString::from(*arg))).is_err(),
                "{args:?}"
            );
        }
    }
}
