//! Transporte da rota: uma única chamada HTTPS, destino fixo, sem proxy,
//! redirecionamento ou retentativa. A interface existe para que as provas usem
//! um transporte falso e nenhum teste toque a rede.
use super::{Falha, Pedido, Segredo};
use std::time::Duration;

/// Teto do corpo recebido, em bytes. Uma resposta de texto acima disso é
/// recusada antes de virar memória sem limite.
const MAX_CORPO: usize = 1_000_000;
const MENSAGENS: &str = "https://api.anthropic.com/v1/messages";
const VERSAO_API: &str = "2023-06-01";

/// Corpo recebido do provedor, já limitado. O adaptador interpreta os eventos
/// tipados; nada aqui é exposto ao Motor.
pub(crate) struct CorpoBruto(Vec<u8>);

impl CorpoBruto {
    pub(crate) fn novo(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Recusa fechada do transporte. `Provedor` carrega só o código HTTP; nenhum
/// variante carrega cabeçalho, corpo ou credencial.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FalhaTransporte {
    /// A chave recebida não serve para montar o cabeçalho.
    Credencial,
    /// Falha de conexão, TLS ou tempo esgotado.
    Indisponivel,
    /// O provedor respondeu com status de erro.
    Provedor { status: u16 },
    /// O corpo passou do teto.
    CorpoGrande,
}

impl FalhaTransporte {
    pub(crate) fn codigo(self) -> &'static str {
        match self {
            Self::Credencial => "transporte_credencial",
            Self::Indisponivel => "transporte_indisponivel",
            Self::Provedor { .. } => "transporte_provedor",
            Self::CorpoGrande => "transporte_corpo",
        }
    }
}

impl From<FalhaTransporte> for Falha {
    fn from(falha: FalhaTransporte) -> Self {
        match falha {
            FalhaTransporte::Credencial => Self::Cofre(super::RecusaCofre::FormatoInvalido),
            FalhaTransporte::Indisponivel => Self::Transporte,
            FalhaTransporte::Provedor { status } => Self::Provedor { status: Some(status) },
            FalhaTransporte::CorpoGrande => Self::Limite,
        }
    }
}

/// Envio de uma requisição. Uma implementação faz no máximo uma tentativa: não
/// há retentativa escondida nem segunda chamada por erro.
pub(crate) trait Transporte {
    async fn enviar(&self, pedido: &Pedido<'_>, chave: &Segredo) -> Result<CorpoBruto, FalhaTransporte>;
}

/// Transporte real. Inerte nesta fatia: nenhum caminho de produção o constrói.
pub(crate) struct TransporteHttps;

impl TransporteHttps {
    fn cliente() -> Result<reqwest::Client, FalhaTransporte> {
        reqwest::Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(90))
            .build()
            .map_err(|_| FalhaTransporte::Indisponivel)
    }
}

impl Transporte for TransporteHttps {
    async fn enviar(&self, pedido: &Pedido<'_>, chave: &Segredo) -> Result<CorpoBruto, FalhaTransporte> {
        let cabecalho =
            crate::fork::background_anthropic::api_key(chave.expor()).map_err(|_| FalhaTransporte::Credencial)?;
        let mut resposta = Self::cliente()?
            .post(MENSAGENS)
            .header("x-api-key", cabecalho)
            .header("anthropic-version", VERSAO_API)
            .json(&pedido.corpo())
            .send()
            .await
            .map_err(|_| FalhaTransporte::Indisponivel)?;
        let status = resposta.status();
        if !status.is_success() {
            return Err(FalhaTransporte::Provedor { status: status.as_u16() });
        }
        let mut corpo = Vec::new();
        while let Some(pedaco) = resposta.chunk().await.map_err(|_| FalhaTransporte::Indisponivel)? {
            if corpo.len().saturating_add(pedaco.len()) > MAX_CORPO {
                return Err(FalhaTransporte::CorpoGrande);
            }
            corpo.extend_from_slice(&pedaco);
        }
        Ok(CorpoBruto::novo(corpo))
    }
}
