use crate::market::Connector;

pub(crate) struct Artefato {
    id: &'static str,
    arquivo: &'static str,
    sha256: [u8; 32],
}

const ARTEFATOS: &[Artefato] = &[
    #[cfg(test)]
    Artefato {
        id: "fixture-local.local",
        arquivo: "fixture-local",
        sha256: [
            0xdc, 0x1f, 0x5c, 0xbe, 0x70, 0xc8, 0x62, 0x7d, 0xc7, 0x04, 0x1e, 0x14, 0x10, 0xce, 0x97, 0x93, 0x4d, 0xd0,
            0x0a, 0xe4, 0xff, 0xc2, 0xd0, 0xa6, 0xbe, 0xef, 0xaf, 0xbe, 0xf7, 0x05, 0x8b, 0x97,
        ],
    },
];

pub(crate) fn da_ficha(servico: &str, tipo: &str) -> Option<&'static Artefato> {
    ARTEFATOS.iter().find(|artefato| artefato.id == tipo && tipo.split_once('.').map(|p| p.0) == Some(servico))
}

pub(crate) fn da_conta(conta: &Connector) -> Option<&'static Artefato> {
    if conta.command.is_some()
        || conta.url.is_some()
        || !conta.args.is_empty()
        || !conta.env.is_empty()
        || !conta.headers.is_empty()
    {
        return None;
    }
    let servico = conta.servico.as_deref()?;
    let tipo = conta.tipo.as_deref()?;
    let ficha = super::catalogo_servicos::servico(servico)?.tipo(tipo)?;
    if ficha.tipo != super::catalogo_servicos::Tipo::Local {
        return None;
    }
    da_ficha(servico, tipo)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn catalog_local_executa_apenas_copia_verificada() -> anyhow::Result<()> {
        let raiz = tempfile::tempdir_in("/tmp")?;
        let origem = raiz.path().join("mcp-artifacts/fixture-local");
        tokio::fs::create_dir(raiz.path().join("mcp-artifacts")).await?;
        let fixado = da_ficha("fixture-local", "fixture-local.local").ok_or_else(|| anyhow::anyhow!("fixture"))?;
        assert!(fixado.preparar_em(raiz.path()).await.is_err());
        let fixture = b"#!/bin/sh\nprintf \"%s\\n\" fixture-local\n";
        tokio::fs::write(&origem, fixture).await?;
        let (dir, mut command) = fixado.preparar_em(raiz.path()).await?;
        tokio::fs::write(&origem, b"alterado").await?;
        assert!(fixado.preparar_em(raiz.path()).await.is_err());
        assert_eq!(command.as_std().get_envs().count(), 1);
        let saida = command.output().await?;
        assert!(saida.status.success());
        assert_eq!(saida.stdout, b"fixture-local\n");
        assert!(
            command.as_std().get_program().as_encoded_bytes().starts_with(dir.path().as_os_str().as_encoded_bytes())
        );
        tokio::fs::write(raiz.path().join("outro"), fixture).await?;
        tokio::fs::remove_file(&origem).await?;
        std::os::unix::fs::symlink(raiz.path().join("outro"), &origem)?;
        assert!(fixado.preparar_em(raiz.path()).await.is_err());
        Ok(())
    }
}

pub(crate) fn reservado(conta: &Connector) -> bool {
    conta.servico.as_deref().zip(conta.tipo.as_deref()).is_some_and(|(servico, tipo)| {
        super::catalogo_servicos::servico(servico)
            .and_then(|s| s.tipo(tipo))
            .is_some_and(|t| t.tipo == super::catalogo_servicos::Tipo::Local)
            || da_ficha(servico, tipo).is_some()
    })
}

impl Artefato {
    pub(crate) fn id(&self) -> &'static str {
        self.id
    }

    pub(crate) async fn preparar(id: &str) -> anyhow::Result<(tempfile::TempDir, tokio::process::Command)> {
        let artefato =
            ARTEFATOS.iter().find(|a| a.id == id).ok_or_else(|| anyhow::anyhow!("Artefato MCP não fixado"))?;
        let exe = std::env::current_exe()?;
        let raiz = exe.parent().ok_or_else(|| anyhow::anyhow!("Instalação MCP indisponível"))?;
        artefato.preparar_em(raiz).await
    }

    async fn preparar_em(
        &self,
        raiz: &std::path::Path,
    ) -> anyhow::Result<(tempfile::TempDir, tokio::process::Command)> {
        #[cfg(unix)]
        {
            use anyhow::{Context, ensure};
            use sha2::{Digest, Sha256};
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let origem = raiz.join("mcp-artifacts").join(self.arquivo);
            let metadata = tokio::fs::symlink_metadata(&origem).await.context("Artefato MCP não instalado")?;
            ensure!(metadata.is_file(), "Artefato MCP inválido");
            let mut origem = tokio::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(origem)
                .await
                .context("Não foi possível abrir o artefato MCP")?;
            ensure!(
                origem.metadata().await.context("Não foi possível verificar o arquivo MCP")?.is_file(),
                "Artefato MCP inválido"
            );
            let dir = tempfile::Builder::new()
                .permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700))
                .tempdir_in("/tmp")
                .context("Não foi possível criar o diretório privado do MCP")?;
            let destino = dir.path().join("mcp");
            let mut arquivo = tokio::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o500)
                .open(&destino)
                .await
                .context("Não foi possível criar a cópia privada do MCP")?;
            let mut hash = Sha256::new();
            let mut total = 0_usize;
            let mut bloco = vec![0; 64 * 1024];
            loop {
                let n = origem.read(&mut bloco).await.context("Não foi possível ler o artefato MCP")?;
                if n == 0 {
                    break;
                }
                total += n;
                ensure!(total <= 128 * 1024 * 1024, "Artefato MCP acima do limite");
                hash.update(&bloco[..n]);
                arquivo.write_all(&bloco[..n]).await.context("Não foi possível gravar a cópia privada do MCP")?;
            }
            ensure!(hash.finalize().as_slice() == self.sha256, "Integridade do artefato MCP inválida");
            arquivo.flush().await.context("Não foi possível finalizar a cópia privada do MCP")?;
            drop(arquivo);
            let mut command = tokio::process::Command::new(destino);
            command.env_clear().current_dir(dir.path()).env("HOME", dir.path());
            Ok((dir, command))
        }
        #[cfg(not(unix))]
        {
            let _ = (raiz, self.arquivo, self.sha256);
            anyhow::bail!("Artefato MCP local indisponível nesta plataforma")
        }
    }
}
