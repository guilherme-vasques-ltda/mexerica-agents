//! Etapa administrativa da atualização privilegiada. Somente root, por canal
//! independente do Hub; nunca chamada pelo executável ou pelo UID do Hub.
#[cfg(target_os = "linux")]
#[path = "../atualizacao/mod.rs"]
mod atualizacao;

#[cfg(target_os = "linux")]
#[derive(clap::Parser)]
#[command(name = "donnabot-instalar-atualizacao")]
struct Cli {
    #[command(subcommand)]
    acao: Acao,
}

#[cfg(target_os = "linux")]
#[derive(clap::Subcommand)]
enum Acao {
    /// Instala contas, diretórios, wrapper, guard, cgroup, política, raiz TUF pinada e sudoers.
    Instalar {
        #[arg(long)]
        login: String,
        #[arg(long)]
        pacote: std::path::PathBuf,
        /// SHA-256 de SHA256SUMS, conferido por canal independente.
        #[arg(long)]
        fingerprint_pacote: String,
        /// SHA-256 de raiz.json, conferido por canal independente.
        #[arg(long)]
        fingerprint_raiz: String,
        /// Instalação limpa: também instala e ativa o Hub no layout gerido (primeira
        /// release autenticada pelo wrapper). Recusa se o login já tiver um Hub legado.
        #[arg(long)]
        hub_novo: bool,
        /// Porta do Hub na instalação limpa.
        #[arg(long, default_value_t = 19422, requires = "hub_novo")]
        porta: u16,
    },
    /// Confere a instalação inteira sem alterar nada.
    Verificar {
        #[arg(long)]
        login: String,
    },
    /// Revoga sudoers, guard, canal do motor, capabilities e binários; arquiva o estado.
    Revogar {
        #[arg(long)]
        login: String,
    },
    /// Recria o cgroup root-owned com os limites da política (unidade de boot).
    PrepararCgroup,
    /// Substitui a raiz TUF confiável por outra conferida por canal independente.
    RepinarRaiz {
        #[arg(long)]
        raiz: std::path::PathBuf,
        #[arg(long)]
        fingerprint_raiz: String,
    },
}

fn main() {
    #[cfg(target_os = "linux")]
    {
        use clap::Parser as _;
        let cli = Cli::parse();
        // SAFETY: ajusta só a máscara do próprio processo.
        unsafe { libc::umask(0o077) };
        let resultado = match cli.acao {
            Acao::Instalar { login, pacote, fingerprint_pacote, fingerprint_raiz, hub_novo, porta } => {
                atualizacao::instalacao::instalar(
                    &login,
                    &pacote,
                    &fingerprint_pacote,
                    &fingerprint_raiz,
                    hub_novo.then_some(porta),
                )
            }
            Acao::Verificar { login } => {
                atualizacao::instalacao::verificar(&login).map(|()| println!("Instalação conferida."))
            }
            Acao::Revogar { login } => atualizacao::instalacao::revogar(&login),
            Acao::PrepararCgroup => atualizacao::instalacao::preparar_cgroup(),
            Acao::RepinarRaiz { raiz, fingerprint_raiz } => {
                atualizacao::instalacao::repinar_raiz(&raiz, &fingerprint_raiz)
            }
        };
        if let Err(erro) = resultado {
            eprintln!("Recusado: {erro:#}");
            std::process::exit(1);
        }
    }
    #[cfg(not(target_os = "linux"))]
    std::process::exit(1);
}
