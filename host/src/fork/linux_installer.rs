//! Instalador Linux do fork. O próprio binário executa a etapa root; sem shell.
use anyhow::Result;
use clap::Subcommand;
use std::path::PathBuf;

#[cfg(target_os = "linux")]
mod identity;
#[cfg(target_os = "linux")]
mod migration;
#[cfg(target_os = "linux")]
mod rollback_data;
#[cfg(target_os = "linux")]
mod root;

#[derive(Subcommand)]
pub(crate) enum Migration {
    /// Copia, instala e chama migrar-raizes da F3. Somente instalação de teste do dono.
    Aplicar {
        #[arg(long)]
        instalacao_de_teste: bool,
        #[arg(long)]
        ajudante: PathBuf,
        #[arg(long)]
        backup: PathBuf,
        #[arg(long, default_value_t = crate::service::DEFAULT_PORT)]
        port: u16,
    },
    /// Restaura a cópia anterior e preserva os dados novos para conferência.
    Voltar {
        #[arg(long)]
        instalacao_de_teste: bool,
        #[arg(long)]
        backup: PathBuf,
    },
    /// Mostra os critérios operacionais de aceite e de volta.
    Conferir,
}

#[derive(Subcommand)]
pub(crate) enum RootAction {
    /// Etapa administrativa chamada por install, sem download.
    Instalar {
        #[arg(long)]
        hub_uid: u32,
        #[arg(long)]
        host: PathBuf,
        #[arg(long)]
        ajudante: PathBuf,
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long, default_value_t = crate::service::DEFAULT_PORT)]
        port: u16,
        #[arg(long)]
        sem_iniciar: bool,
    },
    Desinstalar {
        #[arg(long)]
        hub_uid: u32,
        #[arg(long)]
        remover_usuario_motor: bool,
    },
    Migrar {
        #[arg(long)]
        hub_uid: u32,
        #[arg(long)]
        host: PathBuf,
        #[arg(long)]
        ajudante: PathBuf,
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long)]
        backup: PathBuf,
        #[arg(long)]
        port: u16,
    },
    Voltar {
        #[arg(long)]
        hub_uid: u32,
        #[arg(long)]
        backup: PathBuf,
    },
}

pub(crate) fn install(port: u16, helper: Option<PathBuf>) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        root::request_install(port, helper)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (port, helper);
        anyhow::bail!("instalador separado exige Linux")
    }
}

pub(crate) fn uninstall(remove_user: bool) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        root::request_uninstall(remove_user)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = remove_user;
        anyhow::bail!("instalador separado exige Linux")
    }
}

pub(crate) fn migrate(action: Migration) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        root::request_migrate(action)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = action;
        anyhow::bail!("migração separada exige Linux")
    }
}

pub(crate) async fn privileged(action: RootAction) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        root::execute(action).await
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = action;
        anyhow::bail!("etapa administrativa exige Linux")
    }
}
