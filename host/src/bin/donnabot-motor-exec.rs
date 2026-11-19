//! Ajudante com `CAP_SETUID/CAP_SETGID`; não aceita configuração por argv nem ambiente.
#[cfg(target_os = "linux")]
#[path = "../fork/motor_exec/helper.rs"]
mod helper;

fn main() {
    #[cfg(target_os = "linux")]
    let result = helper::run();
    #[cfg(not(target_os = "linux"))]
    let result: anyhow::Result<()> = Err(anyhow::anyhow!("ajudante disponível somente no Linux"));
    if let Err(error) = result {
        eprintln!("donnabot-motor-exec: {error:#}");
        std::process::exit(1);
    }
}
