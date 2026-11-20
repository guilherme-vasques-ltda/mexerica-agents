//! «Testar acesso» de um plano de código: só lê a lista de modelos que a chave enxerga.
//!
//! O endereço vem do catálogo (nunca do app) e a chave só segue para ele. Sem redirecionamento, com prazo e teto de
//! tamanho. O corpo de uma resposta de erro nunca é lido, porque o fornecedor pode repetir a chave nele.
use serde::Serialize;
use std::time::Duration;

const PRAZO: Duration = Duration::from_secs(20);
const MAX_CORPO: usize = 1 << 20;
const MAX_MODELOS: usize = 500;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Modelo {
    pub(crate) id: String,
    pub(crate) name: String,
}

/// Por que o teste não deu uma lista. `chave_invalida` e `sem_permissao` são a recusa do fornecedor; o resto não diz
/// nada sobre a chave («não foi possível verificar»).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Falha {
    ChaveInvalida,
    SemPermissao,
    Limite,
    FornecedorFalhou,
    SemResposta,
    RespostaInvalida,
}

impl Falha {
    pub(crate) fn codigo(self) -> &'static str {
        match self {
            Self::ChaveInvalida => "chave_invalida",
            Self::SemPermissao => "sem_permissao",
            Self::Limite => "limite",
            Self::FornecedorFalhou => "fornecedor_falhou",
            Self::SemResposta => "sem_resposta",
            Self::RespostaInvalida => "resposta_invalida",
        }
    }

    pub(crate) fn estado(self) -> &'static str {
        match self {
            Self::ChaveInvalida | Self::SemPermissao => "recusada",
            _ => "nao_verificado",
        }
    }
}

fn por_status(status: reqwest::StatusCode) -> Falha {
    match status.as_u16() {
        401 => Falha::ChaveInvalida,
        403 => Falha::SemPermissao,
        402 | 429 => Falha::Limite,
        _ => Falha::FornecedorFalhou,
    }
}

/// `GET <url>` com a chave; os modelos de conversa, como `<provedor>/<modelo>`.
pub(crate) async fn listar_modelos(provedor: &str, url: &str, chave: &str) -> Result<Vec<Modelo>, Falha> {
    listar_modelos_com_prazo(provedor, url, chave, PRAZO).await
}

async fn listar_modelos_com_prazo(
    provedor: &str,
    url: &str,
    chave: &str,
    prazo: Duration,
) -> Result<Vec<Modelo>, Falha> {
    let cliente = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(prazo)
        .user_agent(concat!("Mexerica/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| Falha::FornecedorFalhou)?;
    let mut resposta = cliente
        .get(url)
        .bearer_auth(chave)
        .send()
        .await
        .map_err(|erro| if erro.is_timeout() { Falha::SemResposta } else { Falha::FornecedorFalhou })?;
    if !resposta.status().is_success() {
        return Err(por_status(resposta.status()));
    }
    let mut corpo = Vec::new();
    while let Some(parte) = resposta.chunk().await.map_err(|_| Falha::SemResposta)? {
        if corpo.len() + parte.len() > MAX_CORPO {
            return Err(Falha::RespostaInvalida);
        }
        corpo.extend_from_slice(&parte);
    }
    let lista: serde_json::Value = serde_json::from_slice(&corpo).map_err(|_| Falha::RespostaInvalida)?;
    let dados = lista["data"].as_array().ok_or(Falha::RespostaInvalida)?;
    let mut modelos: Vec<Modelo> = Vec::new();
    for dado in dados {
        let Some(id) = dado["id"].as_str().filter(|id| {
            !id.is_empty() && id.len() <= 200 && !id.contains('/') && id.chars().all(|c| c.is_ascii_graphic())
        }) else {
            continue;
        };
        if dado["type"].as_str().is_some_and(|tipo| tipo != "chat") {
            continue;
        }
        let id = format!("{provedor}/{id}");
        if modelos.iter().any(|modelo| modelo.id == id) {
            continue;
        }
        let name = dado["name"].as_str().unwrap_or(&id).chars().filter(|c| !c.is_control()).take(120).collect();
        modelos.push(Modelo { id, name });
        if modelos.len() == MAX_MODELOS {
            break;
        }
    }
    Ok(modelos)
}

#[cfg(test)]
pub(super) async fn listar_modelos_de_prova(
    provedor: &str,
    url: &str,
    chave: &str,
    prazo: Duration,
) -> Result<Vec<Modelo>, Falha> {
    listar_modelos_com_prazo(provedor, url, chave, prazo).await
}
