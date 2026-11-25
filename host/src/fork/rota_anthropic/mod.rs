//! Rota Anthropic por chave de API, privada do Hub e atrás de interface com
//! transporte falso.
//!
//! Esta fatia é inerte: nenhum caminho de produção constrói [`TransporteHttps`]
//! nem liga a rota. A recusa da automação por chave continua valendo até a
//! ligação autorizada (cerca de efeito própria e cofre novo).
//!
//! Fronteira garantida aqui: o Hub reserva uma estimativa de gasto ANTES do
//! envio, lê a chave só do registro cifrado do próprio vínculo, faz uma única
//! chamada, aceita apenas texto e devolve texto com uso. Não existe fallback
//! para outro provedor, retentativa, ferramenta, catálogo ou sessão de chat. A
//! credencial não entra no corpo, no resultado, no log nem no erro.
//!
//! Ativação: a reserva é uma estimativa conservadora, não um limite superior
//! provado. Tokenizador, enquadramento real da requisição, preços vigentes e
//! identificadores de modelo só se comprovam contra o contrato do provedor, com
//! teto imposto por ele, no artefato exato — é o roteiro da chave de teste. Nesta
//! fatia [`ATIVACAO_ABERTA`] é `false`: [`executar`] recusa sempre, antes de
//! banco, cofre ou rede, e não existe parâmetro nem caminho que a abra. A fatia
//! de ativação substitui esta constante por uma prova verificável, ligada a
//! origem, artefato em execução, modelo e preços.
#![allow(dead_code)] // Fatia inerte: entrega o ponto de ligação sem ligar chamada real.

pub(crate) mod cofre;
mod eventos;
mod limite;
#[cfg(test)]
mod testes;
mod transporte;

pub(crate) use cofre::{Cofre, RecusaCofre, Segredo, Vinculo};
pub(crate) use limite::migrate;
pub(crate) use transporte::{CorpoBruto, Transporte};

use crate::store::Store;

/// Teto de entrada em caracteres: o mesmo da tarefa de fundo.
const MAX_ENTRADA: usize = 128_000;
/// Teto de saída pedido ao provedor.
const MAX_SAIDA_TOKENS: u32 = 4096;
/// Envelope JSON do pedido (papéis, chaves, delimitadores) contado como token.
const MARGEM_ENVELOPE: usize = 4096;

/// Rota fechada por construção nesta fatia: sem prova verificável de ativação
/// não há como ligá-la. Trocar por uma verificação de prova é a fatia seguinte.
const ATIVACAO_ABERTA: bool = false;

/// Pedido tipado: sem ferramenta, sem histórico e sem escolha de provedor.
pub(crate) struct Pedido<'a> {
    pub(crate) modelo: &'a str,
    pub(crate) sistema: &'a str,
    pub(crate) usuario: &'a str,
}

impl Pedido<'_> {
    fn validar(&self) -> Result<(), Falha> {
        let modelo = self.modelo.starts_with("claude-")
            && self.modelo.len() <= 128
            && self.modelo.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
        let tamanho =
            !self.usuario.trim().is_empty() && self.sistema.len().saturating_add(self.usuario.len()) <= MAX_ENTRADA;
        if modelo && tamanho { Ok(()) } else { Err(Falha::PedidoInvalido) }
    }

    /// Corpo do pedido: só texto, sem ferramenta declarada e com stream ligado.
    fn corpo(&self) -> serde_json::Value {
        serde_json::json!({
            "model": self.modelo,
            "max_tokens": MAX_SAIDA_TOKENS,
            "system": self.sistema,
            "messages": [{"role": "user", "content": self.usuario}],
            "stream": true
        })
    }
}

/// Uso medido do provedor, em tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Uso {
    pub(crate) entrada: u64,
    pub(crate) saida: u64,
    pub(crate) cache_lido: u64,
    pub(crate) cache_escrito: u64,
}

/// Resultado entregue ao Motor: só texto e uso. Sem credencial, cabeçalho,
/// cliente HTTP ou autoridade.
pub(crate) struct Resposta {
    pub(crate) modelo: String,
    pub(crate) texto: String,
    pub(crate) uso: Uso,
}

/// `Debug` sem o texto: o conteúdo do provedor não entra em log de diagnóstico.
impl std::fmt::Debug for Resposta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resposta")
            .field("modelo", &self.modelo)
            .field("uso", &self.uso)
            .field("texto_bytes", &self.texto.len())
            .finish()
    }
}

/// Recusa fechada da rota. Nenhum variante carrega chave, cabeçalho, corpo ou
/// texto do pedido.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Falha {
    VinculoInvalido,
    PedidoInvalido,
    /// Sem orçamento definido para o escopo.
    SemTeto,
    /// Orçamento ilegível ou banco indisponível: não dá para conferir.
    TetoIndisponivel,
    TetoEsgotado,
    /// Há envio anterior sem uso provado no período.
    UsoIncerto,
    /// Modelo sem preço verificado: o teto não é conferível.
    SemPreco,
    /// O uso medido passou da reserva: a estimativa não cobriu o turno.
    TetoExcedido,
    /// A prova verificável de ativação ainda não existe; a rota não roda.
    AtivacaoPendente,
    Cofre(RecusaCofre),
    Transporte,
    Provedor {
        status: Option<u16>,
    },
    RespostaInvalida,
    ConteudoNaoTexto,
    ParadaAnormal,
    CredencialRepetida,
    UsoNaoConfirmado,
    Limite,
}

impl Falha {
    /// Código fechado para log: nenhum conteúdo variável entra na linha.
    pub(crate) fn codigo(self) -> &'static str {
        match self {
            Self::VinculoInvalido => "vinculo_invalido",
            Self::PedidoInvalido => "pedido_invalido",
            Self::SemTeto => "teto_ausente",
            Self::TetoIndisponivel => "teto_indisponivel",
            Self::TetoEsgotado => "teto_esgotado",
            Self::UsoIncerto => "uso_incerto",
            Self::SemPreco => "sem_preco",
            Self::TetoExcedido => "teto_excedido",
            Self::AtivacaoPendente => "ativacao_pendente",
            Self::Cofre(recusa) => recusa.codigo(),
            Self::Transporte => "transporte",
            Self::Provedor { .. } => "provedor",
            Self::RespostaInvalida => "resposta_invalida",
            Self::ConteudoNaoTexto => "conteudo_nao_texto",
            Self::ParadaAnormal => "parada_anormal",
            Self::CredencialRepetida => "credencial_repetida",
            Self::UsoNaoConfirmado => "uso_nao_confirmado",
            Self::Limite => "limite",
        }
    }
}

impl std::fmt::Display for Falha {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::VinculoInvalido => "O vínculo da chamada não confere.",
            Self::PedidoInvalido => "O pedido da rota não confere.",
            Self::SemTeto => "A rota não tem teto de gasto definido para este bot.",
            Self::TetoIndisponivel => "O teto de gasto da rota não pôde ser conferido.",
            Self::TetoEsgotado => "O teto de gasto da rota foi atingido.",
            Self::UsoIncerto => "Há envio anterior sem uso confirmado; a rota fica fechada até a prova.",
            Self::SemPreco => "O modelo não tem preço verificado; o teto não é conferível.",
            Self::TetoExcedido => "O uso medido passou da reserva; a chamada não vira sucesso.",
            Self::AtivacaoPendente => "A rota ainda não tem a ativação comprovada; ela não roda.",
            Self::Cofre(recusa) => return recusa.fmt(f),
            Self::Transporte => "Não foi possível enviar a chamada.",
            Self::Provedor { .. } => "O provedor recusou a chamada.",
            Self::RespostaInvalida => "A resposta do provedor não segue o contrato.",
            Self::ConteudoNaoTexto => "O provedor devolveu conteúdo que não é texto.",
            Self::ParadaAnormal => "A resposta do provedor não terminou normalmente.",
            Self::CredencialRepetida => "A resposta repetiu a credencial.",
            Self::UsoNaoConfirmado => "O uso da chamada não foi confirmado.",
            Self::Limite => "A chamada passou do limite de tamanho.",
        })
    }
}

impl std::error::Error for Falha {}

fn escopo(bot: &str) -> String {
    format!("bot:{bot}")
}

/// Estimativa conservadora em micro-USD: cada byte do texto conta como um
/// token, mais a margem do envelope; uma gravação de cache relatada pelo
/// provedor sem `cache_control` no pedido entra limitada ao mesmo tamanho; a
/// saída é o teto pedido. Não é prova de limite superior — isso é a fatia de
/// ativação. Modelo sem preço verificado não tem estimativa.
fn reserva_conservadora_micros(modelo: &str) -> Option<i64> {
    let entrada = i64::try_from(MAX_ENTRADA.checked_add(MARGEM_ENVELOPE)?).ok()?;
    let tokens = crate::fork::custo::Tokens {
        input: Some(entrada),
        output: Some(i64::from(MAX_SAIDA_TOKENS)),
        read: Some(0),
        write: Some(entrada),
        invalid: false,
    };
    micros(&crate::fork::custo::estimar(modelo, &tokens, crate::fork::custo::PoliticaCache::default())?)
}

fn uso_micros(modelo: &str, uso: Uso) -> Option<i64> {
    let tokens = crate::fork::custo::Tokens {
        input: i64::try_from(uso.entrada).ok(),
        output: i64::try_from(uso.saida).ok(),
        read: i64::try_from(uso.cache_lido).ok(),
        write: i64::try_from(uso.cache_escrito).ok(),
        invalid: false,
    };
    micros(&crate::fork::custo::estimar(modelo, &tokens, crate::fork::custo::PoliticaCache::default())?)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn micros(custo: &crate::fork::custo::Cost) -> Option<i64> {
    let valor = custo.amount * 1_000_000.0;
    (0.0..=9_007_199_254_740_992.0).contains(&valor).then(|| valor.round() as i64)
}

/// Executa um turno da rota. Nesta fatia sempre recusa: a rota é fechada por
/// construção e não há parâmetro que a abra.
pub(crate) async fn executar<T, C>(
    store: &Store,
    transporte: &T,
    cofre: &C,
    vinculo: &Vinculo<'_>,
    pedido: &Pedido<'_>,
    agora: i64,
) -> Result<Resposta, Falha>
where
    T: Transporte,
    C: Cofre + ?Sized,
{
    if !ATIVACAO_ABERTA {
        return Err(Falha::AtivacaoPendente);
    }
    let resultado = turno(store, transporte, cofre, vinculo, pedido, agora).await;
    if let Err(falha) = &resultado {
        tracing::warn!(motivo = falha.codigo(), "rota Anthropic recusada");
    }
    resultado
}

/// Caminho aberto do turno: teto reservado, cofre do vínculo, uma chamada,
/// texto tipado e liquidação pelo uso medido. Só o teste o alcança, porque
/// [`executar`] fecha a porta antes.
async fn turno<T, C>(
    store: &Store,
    transporte: &T,
    cofre: &C,
    vinculo: &Vinculo<'_>,
    pedido: &Pedido<'_>,
    agora: i64,
) -> Result<Resposta, Falha>
where
    T: Transporte,
    C: Cofre + ?Sized,
{
    vinculo.validar().map_err(|_| Falha::VinculoInvalido)?;
    pedido.validar()?;
    let estimativa = reserva_conservadora_micros(pedido.modelo).ok_or(Falha::SemPreco)?;
    let reserva = limite::reservar(store, &escopo(vinculo.bot), estimativa, agora)?;
    let chave = match cofre.abrir(vinculo) {
        Ok(chave) => chave,
        Err(recusa) => {
            limite::liberar_sem_envio(store, reserva)?;
            return Err(Falha::Cofre(recusa));
        }
    };
    if let Err(falha) = limite::revalidar(store, &escopo(vinculo.bot), agora) {
        // Dúvida sobre o período: a reserva fica cheia e incerta, sem devolver
        // margem. Só teto rebaixado ou ilegível (nada saiu) devolve a margem.
        if falha == Falha::UsoIncerto {
            limite::incerta(store, reserva, agora)?;
        } else {
            limite::liberar_sem_envio(store, reserva)?;
        }
        return Err(falha);
    }
    let corpo = match transporte.enviar(pedido, &chave).await {
        Ok(corpo) => corpo,
        Err(falha) => {
            limite::incerta(store, reserva, agora)?;
            return Err(falha.into());
        }
    };
    let resposta = match eventos::interpretar(&corpo, &chave) {
        Ok(resposta) => resposta,
        Err(falha) => {
            limite::incerta(store, reserva, agora)?;
            return Err(falha);
        }
    };
    let Some(uso) = uso_micros(&resposta.modelo, resposta.uso) else {
        limite::incerta(store, reserva, agora)?;
        return Err(Falha::UsoNaoConfirmado);
    };
    if limite::liquidar(store, reserva, uso, agora)? {
        return Err(Falha::TetoExcedido);
    }
    Ok(resposta)
}
