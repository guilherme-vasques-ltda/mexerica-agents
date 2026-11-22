pub const CODIGO: &str = "motor_macos_indisponivel";

pub fn indisponivel() -> anyhow::Result<()> {
    tracing::warn!(codigo = CODIGO, "Execução local do Motor Mexerica indisponível no macOS");
    anyhow::bail!("{CODIGO}: Motor Mexerica desligado no macOS; execução local indisponível.")
}
