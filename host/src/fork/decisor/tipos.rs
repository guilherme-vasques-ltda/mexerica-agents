use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::Zeroizing;

/// Provedores com API oficial verificada em 13/11/2026 (ver `docs/features/decisor-ia.md`).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProvedorDecisor {
    /// Genérico: qualquer modelo de decisão que o `OpenRouter` liste. Na Cloud, com a chave da plataforma.
    OpenRouter,
    Google,
}

impl ProvedorDecisor {
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::OpenRouter => "openrouter",
            Self::Google => "google",
        }
    }
}

/// Provedor + modelo a chamar. Vem só de config do usuário ou da política server-side.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Alvo {
    pub provedor: ProvedorDecisor,
    pub modelo: String,
}

/// Chave de API em memória: zerada ao soltar e sem texto em `Debug`/`Display`.
pub(crate) struct Chave(Zeroizing<String>);

impl Chave {
    /// Recusa vazia, longa demais ou com espaço/controle; o erro nunca ecoa o valor.
    pub(crate) fn nova(valor: &str) -> Result<Self, ErroDecisao> {
        let valor = valor.trim();
        if valor.is_empty() || valor.len() > 512 || valor.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(ErroDecisao::ChaveInvalida);
        }
        Ok(Self(Zeroizing::new(valor.to_owned())))
    }

    pub(crate) fn expor(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Chave {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Chave(***)")
    }
}

/// Opção que o chamador já permite. `id` é opaco (nunca conta, motor ou modelo em claro além do
/// que o chamador decidir expor) e é o que o decisor devolve; `descricao` é contexto mínimo.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Opcao {
    pub id: String,
    pub descricao: String,
}

/// Uma pergunta de escolha única. Sem chave, sem identidade do usuário, sem histórico.
#[derive(Debug)]
pub(crate) struct Pedido<'a> {
    pub estado: &'a str,
    pub pergunta: &'a str,
    pub opcoes: &'a [Opcao],
}

/// Saída fechada do decisor: uma opção ou abstenção. Nenhum texto livre do provedor passa daqui.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Saida {
    Escolha(String),
    Abstencao,
}

/// Erros sem detalhe do provedor: corpo, URL e chave nunca entram na mensagem.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ErroDecisao {
    ChaveInvalida,
    Autenticacao,
    LimiteTaxa,
    Recusado,
    Indisponivel,
    RespostaInvalida,
}

impl fmt::Display for ErroDecisao {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ChaveInvalida => "chave do decisor inválida",
            Self::Autenticacao => "o provedor recusou a chave do decisor",
            Self::LimiteTaxa => "limite de uso do decisor atingido",
            Self::Recusado => "o provedor recusou o pedido do decisor",
            Self::Indisponivel => "decisor indisponível",
            Self::RespostaInvalida => "resposta inválida do decisor",
        })
    }
}

impl std::error::Error for ErroDecisao {}

/// Contrato único dos clientes Decision. Uma chamada, sem retry; cancela ao soltar o futuro.
pub(crate) trait DecisionProvider: Send + Sync {
    fn decidir<'a>(&'a self, chave: &'a Chave, pedido: &'a Pedido<'a>) -> BoxFuture<'a, Result<Saida, ErroDecisao>>;
}
