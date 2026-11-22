pub const CODIGO: &str = "motor_macos_indisponivel";

pub fn indisponivel() -> anyhow::Result<()> {
    anyhow::bail!("{CODIGO}: Motor Mexerica desligado no macOS; execução local indisponível.")
}
