//! Só sugere: a resposta do decisor vale apenas se for uma das opções já permitidas. Tudo o mais
//! (sem config/chave, teto, prazo, erro, abstenção, ID desconhecido) mantém o Fixo.
use super::{
    Alvo, DecisionProvider, ErroDecisao, Opcao, Pedido, Saida,
    chave::{self, FontePlataforma},
    config,
    plataforma::{self, Reserva},
};
use crate::store::Store;
use std::time::Duration;

/// Prazo total da decisão; ao estourar, o futuro é solto (cancela a requisição) e vale o Fixo.
pub(crate) const PRAZO: Duration = Duration::from_secs(12);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MotivoFixo {
    Desligado,
    SemChave,
    SemOpcoes,
    TetoAtingido,
    Abstencao,
    FalhaDoProvedor,
    ForaDasOpcoes,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Decisao {
    /// `id` de uma das opções permitidas. Quem consome ainda aplica as regras e a autoridade do Hub.
    Sugestao(String),
    /// Mantém o comportamento atual (modo "Fixo"), com as mesmas autorizações.
    Fixo(MotivoFixo),
}

/// Fábrica do cliente para um alvo; injetável para testes (mock do trait).
pub(crate) type Fabrica<'a> = &'a dyn Fn(&Alvo) -> Box<dyn DecisionProvider>;

/// `periodo` é `plataforma::periodo_atual()`. Zero chamadas e zero reserva sem alvo, chave ou opções.
pub(crate) async fn decidir(
    store: &Store,
    plataforma_fonte: Option<&dyn FontePlataforma>,
    fabrica: Fabrica<'_>,
    periodo: &str,
    pedido: &Pedido<'_>,
) -> Decisao {
    let politica = plataforma::politica(store);
    let (alvo, chave) = if politica.ativa {
        // Modalidade Cloud vem da política persistida; a escolha do usuário é ignorada.
        (plataforma::alvo(&politica), plataforma_fonte.and_then(FontePlataforma::chave_openrouter))
    } else {
        let Some(alvo) = config::carregar(store).alvo() else { return Decisao::Fixo(MotivoFixo::Desligado) };
        let chave = chave::do_usuario(store, alvo.provedor);
        (alvo, chave)
    };
    let Some(chave) = chave else { return Decisao::Fixo(MotivoFixo::SemChave) };
    if pedido.opcoes.is_empty() {
        return Decisao::Fixo(MotivoFixo::SemOpcoes);
    }
    if politica.ativa && plataforma::reservar(store, &politica, periodo) == Reserva::TetoAtingido {
        tracing::warn!("teto do decisor atingido; usando o modo Fixo até virar o mês");
        return Decisao::Fixo(MotivoFixo::TetoAtingido);
    }
    let provedor = fabrica(&alvo);
    let saida = tokio::time::timeout(PRAZO, provedor.decidir(&chave, pedido)).await;
    validar(saida, pedido.opcoes)
}

fn validar(saida: Result<Result<Saida, ErroDecisao>, tokio::time::error::Elapsed>, opcoes: &[Opcao]) -> Decisao {
    match saida {
        Ok(Ok(Saida::Escolha(id))) if opcoes.iter().any(|o| o.id == id) => Decisao::Sugestao(id),
        Ok(Ok(Saida::Escolha(_))) => Decisao::Fixo(MotivoFixo::ForaDasOpcoes),
        Ok(Ok(Saida::Abstencao)) => Decisao::Fixo(MotivoFixo::Abstencao),
        Ok(Err(erro)) => {
            tracing::warn!(%erro, "decisor indisponível; usando o modo Fixo");
            Decisao::Fixo(MotivoFixo::FalhaDoProvedor)
        }
        Err(_) => {
            tracing::warn!("decisor estourou o prazo; usando o modo Fixo");
            Decisao::Fixo(MotivoFixo::FalhaDoProvedor)
        }
    }
}
