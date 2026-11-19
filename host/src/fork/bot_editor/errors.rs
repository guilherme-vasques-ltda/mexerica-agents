//! Traduz somente falhas conhecidas; nunca devolve texto/caminhos do upstream.
use anyhow::{Error, anyhow};

pub(super) fn save(error: &Error) -> Error {
    if let Some(message) = crate::fork::donna_fixa::wire(error) {
        return anyhow!(message);
    }
    for cause in error.chain() {
        let message = cause.to_string();
        // hub::validate antigo e prefixos do validador de pasta do PR #42.
        if [
            "workspace folder does not exist",
            "A pasta ",
            "O caminho da pasta ",
            "O caminho escolhido não é uma pasta",
            "Escolha um caminho absoluto para a pasta",
            "Informe a pasta de trabalho",
            "Esta pasta contém dados internos do Hub",
            "Pastas ocultas da pasta pessoal",
            "Pastas pessoais de outros usuários",
            "Diretórios do sistema não podem",
            "Não foi possível resolver a pasta",
            "Não foi possível resolver o nome da pasta",
            "Não foi possível verificar as pastas protegidas",
        ]
        .iter()
        .any(|prefix| message.starts_with(prefix))
        {
            return anyhow!(
                "folder_unavailable: A pasta de trabalho está indisponível. Revise a pasta nas configurações."
            );
        }
        if [
            "account_incompatible",
            "account_unavailable",
            "account_not_found",
            "model_unavailable",
            "model_option_unavailable",
        ]
        .iter()
        .any(|code| message.starts_with(code))
        {
            return anyhow!("model_unavailable: A conta ou o modelo selecionado está indisponível.");
        }
        if message.starts_with("remote_setup_unavailable") {
            return anyhow!("remote_setup_unavailable: autorização de contas de IA encerrada.");
        }
        if message.starts_with("unknown backend") {
            return anyhow!("model_unavailable: O motor configurado está indisponível.");
        }
        if message.starts_with("Use um comando literal do adaptador") {
            return anyhow!(
                "command_not_allowed: O comando configurado não é permitido. Revise o motor nas configurações."
            );
        }
    }
    anyhow!("save_failed: Não foi possível salvar.")
}
