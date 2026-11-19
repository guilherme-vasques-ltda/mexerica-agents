//! Serviço do canal de cópia, executado como `donnabot-motor` e ativado pelo
//! socket do systemd. Nunca executa como root; aceita só o UID do Hub e só durante
//! a janela pendente da primeira migração publicada pelo wrapper root.
#[cfg(target_os = "linux")]
#[path = "../motor_copia/mod.rs"]
mod motor_copia;

#[cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "../fork/publicacao_atualizacao.rs"]
mod publicacao_atualizacao;

#[cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "../atualizacao/status.rs"]
mod status;

/// Autorização verificável do ciclo previsto (desenho, passo 6): o status público
/// root está `pendente`, dentro do prazo, e a certidão root da mesma transação é de
/// uma primeira migração (sem sequência anterior), a única que copia raízes F3.
/// Fora disso (antes, depois do aceite, após volta ou bloqueio) o canal recusa.
#[cfg(target_os = "linux")]
fn janela_da_primeira_migracao() -> anyhow::Result<()> {
    use anyhow::{Context, bail, ensure};
    use publicacao_atualizacao::{Nome, ler};
    let status: status::Status = serde_json::from_slice(&ler(Nome::Status)?).context("status público inválido")?;
    let certidao: serde_json::Value = serde_json::from_slice(&ler(Nome::Certidao)?).context("certidão inválida")?;
    let agora = i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs())?;
    let status::Status::Pendente { sequencia, inicio, prazo, .. } = status else {
        bail!("nenhuma atualização pendente")
    };
    ensure!(inicio <= agora && agora < prazo, "janela pendente fora do prazo");
    ensure!(certidao["sequencia_nova"].as_u64() == Some(sequencia), "certidão não corresponde à janela pendente");
    ensure!(certidao["sequencia_anterior"].is_null(), "só a primeira migração copia raízes F3");
    Ok(())
}

#[cfg(target_os = "linux")]
fn executar() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use std::os::fd::FromRawFd;
    // SAFETY: consultas de identidade sem efeitos colaterais.
    let (euid, pid) = unsafe { (libc::geteuid(), libc::getpid()) };
    ensure!(euid != 0, "o canal do motor nunca executa como root");
    let pid_ativacao: i32 = std::env::var("LISTEN_PID").context("sem ativação por socket")?.parse()?;
    let quantidade: i32 = std::env::var("LISTEN_FDS").context("sem ativação por socket")?.parse()?;
    ensure!(pid_ativacao == pid && quantidade == 1, "ativação por socket inesperada");
    // SAFETY: protocolo sd_listen_fds: o descritor 3 é o socket entregue pelo systemd a este PID.
    let ouvinte = unsafe { std::os::unix::net::UnixListener::from_raw_fd(3) };
    let uid_hub = motor_copia::ler_hub_uid(std::path::Path::new("/etc/donnabot/motor.conf"))?;
    motor_copia::servir(&ouvinte, uid_hub, &janela_da_primeira_migracao)
}

fn main() {
    #[cfg(target_os = "linux")]
    if let Err(erro) = executar() {
        eprintln!("canal do motor: {erro:#}");
        std::process::exit(1);
    }
    #[cfg(not(target_os = "linux"))]
    std::process::exit(1);
}
