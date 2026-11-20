//! Heartbeat das Rotinas: uma checagem silenciosa de uma fonte que só chama o dono quando há uma decisão nova.
//!
//! A checagem reusa o agendador das Rotinas e o histórico de execuções; não há segundo agendador. Ela é
//! declarada pelo dono (fonte, escopo, critério e intervalo), lê a fonte por uma interface sem nenhum método de
//! escrita e nunca chama a IA nem executa efeito externo (enviar, excluir, publicar). Os quatro resultados são
//! `Resultado`: sem mudança, mudança sem decisão, decisão pendente e fonte indisponível.
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod avaliar;
#[cfg(test)]
mod testes;

pub(crate) const MIN_INTERVALO_S: u64 = 60;
const MAX_INTERVALO_S: u64 = 7 * 86_400;
const INTERVALO_PADRAO_S: u64 = 300;
const MAX_ESTADOS: usize = 20;
const MAX_ROTULO: usize = 64;
const MAX_ESCOPO: usize = 200;

/// Texto fixo da rotina de checagem: ela nunca carrega instrução para a IA, nem num Hub mais antigo que não
/// conheça o Heartbeat e leria a rotina como uma comum.
pub(crate) const INSTRUCAO: &str = "Checagem silenciosa da fonte declarada. Esta rotina não executa nenhuma ação.";
pub(crate) const SO_O_DONO: &str =
    "heartbeat: só o dono declara, liga ou altera uma checagem silenciosa, pelo app; nada foi alterado.";
pub(crate) const SO_NO_HORARIO: &str =
    "Esta rotina é uma checagem silenciosa: ela só roda no horário, nunca por webhook, evento ou teste.";

/// Quem recebe o aviso. Só o dono por enquanto; um tipo próprio para que outro destino seja uma escolha explícita.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Destinatario {
    #[default]
    Dono,
}

/// O critério de decisão, simples e determinístico: um item da fonte pede decisão quando seu estado está na lista.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Criterio {
    pub estados: Vec<String>,
}

/// O que a rotina observa, e quando vale chamar o dono. Campo desconhecido é recusado: nada de permissão
/// escondida (enviar, excluir) dentro da declaração.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Declaracao {
    /// Nome da fonte no registro do Hub.
    pub fonte: String,
    /// O trecho da fonte que vale observar (por exemplo, uma caixa ou uma lista); a fonte só entrega esse escopo.
    pub escopo: String,
    pub criterio: Criterio,
    #[serde(default)]
    pub destinatario: Destinatario,
    #[serde(default = "intervalo_padrao", rename = "intervaloMinimoS")]
    pub intervalo_minimo_s: u64,
}

const fn intervalo_padrao() -> u64 {
    INTERVALO_PADRAO_S
}

impl Declaracao {
    /// Lê e confere a declaração do corpo salvo pelo app; qualquer falha recusa o corpo inteiro.
    pub(crate) fn parse(value: &Value) -> Result<Self> {
        let Ok(mut declaracao) = serde_json::from_value::<Self>(value.clone()) else {
            bail!(
                "A checagem silenciosa precisa de fonte, escopo, critério (estados) e, se quiser, intervaloMinimoS; nenhum outro campo é aceito."
            );
        };
        declaracao.fonte = declaracao.fonte.trim().to_owned();
        declaracao.escopo = declaracao.escopo.trim().to_owned();
        ensure!(
            !declaracao.fonte.is_empty()
                && declaracao.fonte.len() <= MAX_ROTULO
                && declaracao
                    .fonte
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b)),
            "A fonte da checagem deve ter até {MAX_ROTULO} letras minúsculas, números, ponto, hífen ou sublinhado."
        );
        ensure!(
            !declaracao.escopo.is_empty() && declaracao.escopo.chars().count() <= MAX_ESCOPO,
            "Diga o que observar: o escopo da checagem é obrigatório e tem até {MAX_ESCOPO} caracteres."
        );
        let mut estados: Vec<String> = declaracao.criterio.estados.iter().map(|s| s.trim().to_lowercase()).collect();
        estados.sort();
        estados.dedup();
        ensure!(
            !estados.is_empty()
                && estados.len() <= MAX_ESTADOS
                && estados.iter().all(|s| !s.is_empty() && s.chars().count() <= MAX_ROTULO),
            "Diga quando chamar o dono: o critério precisa de 1 a {MAX_ESTADOS} estados que pedem decisão."
        );
        declaracao.criterio.estados = estados;
        ensure!(
            (MIN_INTERVALO_S..=MAX_INTERVALO_S).contains(&declaracao.intervalo_minimo_s),
            "O intervalo mínimo da checagem fica entre {MIN_INTERVALO_S} segundos e 7 dias."
        );
        Ok(declaracao)
    }
}

/// Uma pendência como a fonte a descreve. `fato` e `escolha` são texto de fora: só aparecem no aviso ao dono e
/// nunca são gravados no estado nem no histórico.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Item {
    pub id: String,
    /// Muda quando o conteúdo do item muda; entra só na detecção de mudança.
    pub versao: String,
    pub estado: String,
    pub fato: String,
    pub escolha: String,
}

/// Por que a fonte não respondeu. Nunca vira «sem mudança».
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Motivo {
    /// Conta, permissão ou vínculo recusados ou revogados: exige ação do dono.
    SemAcesso,
    /// Fora do ar ou sem resposta: costuma passar sozinho.
    Fora,
    /// A fonte devolveu mais do que a checagem aceita, ou limitou o uso.
    Limite,
}

impl Motivo {
    pub(crate) const fn exige_acao(self) -> bool {
        matches!(self, Self::SemAcesso | Self::Limite)
    }
}

/// A fonte só se lê. A interface não tem escrita, envio nem exclusão: observar e executar são coisas separadas,
/// e a checagem não herda do bot nenhuma permissão de enviar ou apagar.
pub(crate) trait Fonte: Send + Sync {
    /// Marcador barato e verificável do estado atual do escopo (versão, data da última alteração, contagem);
    /// `None` quando a fonte não tem. Marcador igual ao da última leitura dispensa ler o conteúdo.
    fn marcador(&self, escopo: &str) -> Result<Option<String>, Motivo>;
    /// O conteúdo do escopo, no máximo `avaliar::MAX_ITENS` itens.
    fn ler(&self, escopo: &str) -> Result<Vec<Item>, Motivo>;
}

/// Os quatro resultados de uma checagem.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Resultado {
    /// Nada mudou desde a última leitura: silêncio, sem registro de execução.
    SemMudanca,
    /// Mudou, mas nenhuma decisão nova: só histórico.
    MudancaSemDecisao,
    /// Há uma pendência nova que pede decisão: um aviso ao dono.
    DecisaoPendente,
    /// A fonte não respondeu: falha visível, nunca «sem mudança».
    FonteIndisponivel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UltimaChecagem {
    pub em: i64,
    pub resultado: Resultado,
    /// Quantas pendências estão em aberto (já avisadas ou não) na última leitura que funcionou.
    pub abertas: usize,
}

/// O que sobrevive entre checagens. Só marcadores e impressões digitais: nenhum conteúdo da fonte.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Estado {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marcador: Option<String>,
    /// Resumo do conteúdo lido (itens e versões), para detectar mudança também quando a fonte não tem marcador.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assinatura: Option<String>,
    /// Impressões das pendências já avisadas; some quando a pendência some da fonte.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub avisadas: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ultima: Option<UltimaChecagem>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub falhas_seguidas: u32,
    /// Enquanto a fonte falha, não consulta antes deste instante (recuo).
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub nao_antes: i64,
}

#[allow(clippy::trivially_copy_pass_by_ref, reason = "serde skip_serializing_if exige fn(&T) -> bool")]
const fn is_zero(v: &u32) -> bool {
    *v == 0
}

#[allow(clippy::trivially_copy_pass_by_ref, reason = "serde skip_serializing_if exige fn(&T) -> bool")]
const fn is_zero_i64(v: &i64) -> bool {
    *v == 0
}
