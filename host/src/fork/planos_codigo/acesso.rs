//! «Testar acesso» de um plano de código: só lê a lista de modelos que a chave enxerga.
//!
//! O endereço vem do catálogo (nunca do app) e a chave só segue para ele. Sem redirecionamento, com prazo e teto de
//! tamanho. O corpo de uma resposta de erro nunca é lido, porque o fornecedor pode repetir a chave nele.
use super::cerca::Cerca;
use serde::Serialize;
use std::time::Duration;

const PRAZO: Duration = Duration::from_secs(20);
const MAX_CORPO: usize = 1 << 20;
const MAX_MODELOS: usize = 500;

/// Onde o fornecedor espera a chave: cabeçalho `Authorization: Bearer`, `x-api-key` (com a versão da API da Anthropic)
/// ou `x-goog-api-key`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Autenticacao {
    Bearer,
    XApiKey,
    XGoogApiKey,
}

/// O formato da lista de modelos do fornecedor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Formato {
    /// `{"data":[{"id","name"?,"type"?}]}`; o id não tem barra.
    Dados,
    /// Como `Dados`, só com os modelos de conversa da `OpenAI` (a lista traz também voz, imagem e vetores).
    DadosOpenai,
    /// Como `Dados`, com o nome em `display_name`.
    DadosAnthropic,
    /// Como `Dados`, mas o id é `<fornecedor>/<modelo>` (`OpenRouter`).
    DadosAninhados,
    /// `{"models":[{"name":"models/<id>","displayName","supportedGenerationMethods"}]}`.
    Google,
}

const NAO_E_CONVERSA_OPENAI: [&str; 13] = [
    "embedding",
    "whisper",
    "tts",
    "dall-e",
    "moderation",
    "image",
    "audio",
    "realtime",
    "transcribe",
    "davinci",
    "babbage",
    "sora",
    "search-preview",
];

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
pub(crate) async fn listar_modelos(
    provedor: &str,
    url: &str,
    chave: &str,
    autenticacao: Autenticacao,
    formato: Formato,
) -> Result<Vec<Modelo>, Falha> {
    listar_modelos_com_prazo(provedor, url, chave, PRAZO, autenticacao, formato).await
}

async fn listar_modelos_com_prazo(
    provedor: &str,
    url: &str,
    chave: &str,
    prazo: Duration,
    autenticacao: Autenticacao,
    formato: Formato,
) -> Result<Vec<Modelo>, Falha> {
    let cliente = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(prazo)
        .user_agent(concat!("Mexerica/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| Falha::FornecedorFalhou)?;
    let mut pedido = cliente.get(url);
    pedido = match autenticacao {
        Autenticacao::Bearer => pedido.bearer_auth(chave),
        Autenticacao::XApiKey => pedido.header("x-api-key", chave).header("anthropic-version", "2023-06-01"),
        Autenticacao::XGoogApiKey => pedido.header("x-goog-api-key", chave),
    };
    let mut resposta = pedido
        .send()
        .await
        .map_err(|erro| if erro.is_timeout() { Falha::SemResposta } else { Falha::FornecedorFalhou })?;
    if !resposta.status().is_success() {
        let falha = por_status(resposta.status());
        tracing::info!(
            provedor,
            status = resposta.status().as_u16(),
            falha = falha.codigo(),
            "teste de acesso do plano sem lista"
        );
        return Err(falha);
    }
    let mut corpo = Vec::new();
    while let Some(parte) = resposta.chunk().await.map_err(|_| Falha::SemResposta)? {
        if corpo.len() + parte.len() > MAX_CORPO {
            return Err(Falha::RespostaInvalida);
        }
        corpo.extend_from_slice(&parte);
    }
    // Uma resposta que repete a chave (no corpo inteiro, em qualquer campo, inclusive disfarçada: ver `cerca.rs`) nunca é
    // devolvida nem guardada.
    let cerca = Cerca::nova(chave);
    if !chave.is_empty() && cerca.contem(&corpo) {
        return Err(Falha::RespostaInvalida);
    }
    let lista: serde_json::Value = serde_json::from_slice(&corpo).map_err(|_| Falha::RespostaInvalida)?;
    let (dados, campo_id, campo_nome) = match formato {
        Formato::Google => (lista["models"].as_array(), "name", "displayName"),
        Formato::DadosAnthropic => (lista["data"].as_array(), "id", "display_name"),
        _ => (lista["data"].as_array(), "id", "name"),
    };
    let dados = dados.ok_or(Falha::RespostaInvalida)?;
    let mut modelos: Vec<Modelo> = Vec::new();
    for dado in dados {
        let Some(bruto) = dado[campo_id].as_str() else { continue };
        let bruto = if formato == Formato::Google { bruto.strip_prefix("models/").unwrap_or_default() } else { bruto };
        let sem_barra = !bruto.contains('/') || formato == Formato::DadosAninhados;
        if bruto.is_empty() || bruto.len() > 200 || !sem_barra || !bruto.chars().all(|c| c.is_ascii_graphic()) {
            continue;
        }
        if dado["type"].as_str().is_some_and(|tipo| !matches!(tipo, "chat" | "model")) {
            continue;
        }
        if formato == Formato::Google
            && !dado["supportedGenerationMethods"]
                .as_array()
                .is_some_and(|metodos| metodos.iter().any(|metodo| metodo == "generateContent"))
        {
            continue;
        }
        if formato == Formato::DadosOpenai {
            let minusculo = bruto.to_ascii_lowercase();
            if NAO_E_CONVERSA_OPENAI.iter().any(|marca| minusculo.contains(marca)) {
                continue;
            }
        }
        let id = format!("{provedor}/{bruto}");
        if cerca.contem(id.as_bytes()) {
            return Err(Falha::RespostaInvalida);
        }
        if modelos.iter().any(|modelo| modelo.id == id) {
            continue;
        }
        let name: String =
            dado[campo_nome].as_str().unwrap_or(&id).chars().filter(|c| !c.is_control()).take(120).collect();
        if cerca.contem(name.as_bytes()) {
            return Err(Falha::RespostaInvalida);
        }
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
    listar_modelos_com_prazo(provedor, url, chave, prazo, Autenticacao::Bearer, Formato::Dados).await
}

#[cfg(test)]
pub(super) async fn listar_modelos_de_prova_com(
    provedor: &str,
    url: &str,
    chave: &str,
    autenticacao: Autenticacao,
    formato: Formato,
) -> Result<Vec<Modelo>, Falha> {
    listar_modelos_com_prazo(provedor, url, chave, Duration::from_secs(5), autenticacao, formato).await
}
