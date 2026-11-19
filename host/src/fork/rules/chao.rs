//! Classificação pura por efeito estruturado, sem adivinhar semântica de comandos.
#[derive(Debug, Default)]
pub struct FloorContext {
    // Compatibilidade da borda administrativa. Não é prova de resolução de alvo.
    pub hub_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Categoria {
    Dinheiro,
    Destruicao,
    Elevacao,
    Segredos,
    PastaHubMotores,
    ApiHubMotores,
    ConfiguracaoHubMotores,
    ExecucaoFutura,
    ConfirmacaoLocal,
    Desconhecida,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Efeito {
    MoverDinheiro,
    ApagarOuSobrescreverIrreversivelmente,
    ForcePush,
    ElevarPrivilegios,
    AcessarSegredo,
    AlterarPastaHubMotor,
    AlterarApiHubMotor,
    AlterarConfiguracaoHubMotor,
    AlterarExecucaoFutura,
    EnviarNoComputadorDoDono,
    LoginComTouchIdEscolhido,
    ComumComprovado,
    Opaco,
}

pub const fn categoria(efeito: Efeito) -> Option<Categoria> {
    match efeito {
        Efeito::MoverDinheiro => Some(Categoria::Dinheiro),
        Efeito::ApagarOuSobrescreverIrreversivelmente | Efeito::ForcePush => Some(Categoria::Destruicao),
        Efeito::ElevarPrivilegios => Some(Categoria::Elevacao),
        Efeito::AcessarSegredo => Some(Categoria::Segredos),
        Efeito::AlterarPastaHubMotor => Some(Categoria::PastaHubMotores),
        Efeito::AlterarApiHubMotor => Some(Categoria::ApiHubMotores),
        Efeito::AlterarConfiguracaoHubMotor => Some(Categoria::ConfiguracaoHubMotores),
        Efeito::AlterarExecucaoFutura => Some(Categoria::ExecucaoFutura),
        Efeito::EnviarNoComputadorDoDono | Efeito::LoginComTouchIdEscolhido => Some(Categoria::ConfirmacaoLocal),
        Efeito::ComumComprovado => None,
        Efeito::Opaco => Some(Categoria::Desconhecida),
    }
}

/// D211: evidência lexical negativa; não resolve links, filesystem nem scripts.
/// Uma classificação ausente NÃO comprova que a mutação seja comum.
pub fn sinal_execucao_futura(caminho: &str) -> Option<Categoria> {
    let partes: Vec<_> = caminho.split(['/', '\\']).filter(|parte| !parte.is_empty()).collect();
    if partes.contains(&"..") || caminho.contains(['$', '`']) {
        return Some(Categoria::Desconhecida);
    }
    if partes.iter().any(|parte| [".githooks", ".claude", ".vscode"].contains(parte))
        || partes.windows(2).any(|par| par == [".git", "hooks"] || par == [".github", "workflows"])
        || partes.last().is_some_and(|parte| ["package.json", "CLAUDE.md"].contains(parte))
    {
        return Some(Categoria::ExecucaoFutura);
    }
    None
}

/// Criação/remoção usa um alvo; renomeação avalia origem E destino.
/// Indireção não resolvida impede concluir que o alvo escapou da D211.
pub fn categoria_mutacao(origem: Option<&str>, destino: Option<&str>, indireto: bool) -> Categoria {
    if indireto || origem.is_none() && destino.is_none() {
        return Categoria::Desconhecida;
    }
    let sinais: Vec<_> = origem.into_iter().chain(destino).filter_map(sinal_execucao_futura).collect();
    if sinais.contains(&Categoria::ExecucaoFutura) { Categoria::ExecucaoFutura } else { Categoria::Desconhecida }
}
