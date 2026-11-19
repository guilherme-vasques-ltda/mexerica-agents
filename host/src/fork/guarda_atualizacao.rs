//! Gate de inicialização para binários publicados em slots root-owned.
use anyhow::{Context, Result, ensure};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) fn verificar() -> Result<()> {
    let executavel = std::env::current_exe().context("executável atual indisponível")?;
    let root = Path::new("/opt/donnabot/releases");
    let slot = if executavel == root.join("slot-a/donnabot-host") {
        "slot-a"
    } else if executavel == root.join("slot-b/donnabot-host") {
        "slot-b"
    } else {
        ensure!(!executavel.starts_with(root), "executável publicado fora dos slots permitidos");
        return Ok(());
    };
    super::publicacao_atualizacao::exigir_sem_emergencia()?;
    ensure!(super::publicacao_atualizacao::seletor()? == slot, "slot executado não é o ativo");
    let bytes = super::publicacao_atualizacao::ler(super::publicacao_atualizacao::Nome::Status)?;
    let status: super::status_atualizacao::Status =
        serde_json::from_slice(&bytes).context("estado de atualização inválido")?;
    let instante = SystemTime::now().duration_since(UNIX_EPOCH).context("relógio anterior à época")?.as_secs();
    let agora = i64::try_from(instante).context("relógio inválido")?;
    super::status_atualizacao::verificar(&status, slot, agora)
}
