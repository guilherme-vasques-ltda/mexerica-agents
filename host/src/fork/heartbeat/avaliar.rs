//! A decisão de uma checagem, sem relógio nem estado global: a mesma fonte, o mesmo estado anterior e o mesmo
//! instante dão sempre a mesma saída.
use super::{Declaracao, Estado, Fonte, Item, Motivo, Resultado, UltimaChecagem};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt::Write as _};

/// Mais itens do que isto, a fonte é tratada como indisponível por limite em vez de lida pela metade.
pub(crate) const MAX_ITENS: usize = 1000;
const FALHAS_ATE_ALERTA: u32 = 3;
const RECUO_MAX_S: u64 = 6 * 3600;
const MAX_TEXTO: usize = 160;
const MAX_NO_AVISO: usize = 5;

/// O que o dono lê: o fato e a escolha de cada pendência nova, ou a falha que pede atenção.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Aviso {
    Decisao { itens: Vec<(String, String)>, alem: usize },
    Falha { motivo: Motivo },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Saida {
    pub resultado: Resultado,
    pub estado: Estado,
    /// `None` é silêncio: nenhuma mensagem ao dono.
    pub aviso: Option<Aviso>,
}

/// A fonte já foi consultada há pouco, ou está em recuo depois de falhar: não consulta de novo.
pub(crate) fn pode_checar(estado: &Estado, declaracao: &Declaracao, agora: i64) -> bool {
    let intervalo_ms = i64::try_from(declaracao.intervalo_minimo_s.saturating_mul(1000)).unwrap_or(i64::MAX);
    agora >= estado.nao_antes && estado.ultima.is_none_or(|ultima| agora.saturating_sub(ultima.em) >= intervalo_ms)
}

pub(crate) fn avaliar(fonte: &dyn Fonte, declaracao: &Declaracao, anterior: &Estado, agora: i64) -> Saida {
    let marcador = match fonte.marcador(&declaracao.escopo) {
        Ok(marcador) => marcador,
        Err(motivo) => return falha(declaracao, anterior, motivo, agora),
    };
    if marcador.is_some() && marcador == anterior.marcador {
        let mut estado = anterior.clone();
        estado.falhas_seguidas = 0;
        estado.nao_antes = 0;
        estado.ultima = Some(UltimaChecagem {
            em: agora,
            resultado: Resultado::SemMudanca,
            abertas: anterior.ultima.map_or(0, |u| u.abertas),
        });
        return Saida { resultado: Resultado::SemMudanca, estado, aviso: None };
    }
    let itens = match fonte.ler(&declaracao.escopo) {
        Ok(itens) if itens.len() <= MAX_ITENS => itens,
        Ok(_) => return falha(declaracao, anterior, Motivo::Limite, agora),
        Err(motivo) => return falha(declaracao, anterior, motivo, agora),
    };
    let assinatura = assinar(&itens);
    let mudou = anterior.assinatura.as_deref() != Some(assinatura.as_str());
    let abertas: BTreeMap<String, &Item> = itens
        .iter()
        .filter(|item| declaracao.criterio.estados.contains(&item.estado.trim().to_lowercase()))
        .map(|item| (impressao(item), item))
        .collect();
    let novas: Vec<&Item> =
        abertas.iter().filter(|(impressao, _)| !anterior.avisadas.contains(impressao)).map(|(_, item)| *item).collect();
    let resultado = if !novas.is_empty() {
        Resultado::DecisaoPendente
    } else if mudou {
        Resultado::MudancaSemDecisao
    } else {
        Resultado::SemMudanca
    };
    let estado = Estado {
        marcador,
        assinatura: Some(assinatura),
        avisadas: abertas.keys().cloned().collect(),
        ultima: Some(UltimaChecagem { em: agora, resultado, abertas: abertas.len() }),
        falhas_seguidas: 0,
        nao_antes: 0,
    };
    let aviso = (!novas.is_empty()).then(|| Aviso::Decisao {
        itens: novas.iter().take(MAX_NO_AVISO).map(|item| (limpar(&item.fato), limpar(&item.escolha))).collect(),
        alem: novas.len().saturating_sub(MAX_NO_AVISO),
    });
    Saida { resultado, estado, aviso }
}

/// A fonte falhou: nada do que se sabia muda (marcador, assinatura e pendências avisadas ficam como estavam), o
/// contador sobe e a próxima consulta recua. Só avisa quando a falha pede ação, ou quando persiste.
fn falha(declaracao: &Declaracao, anterior: &Estado, motivo: Motivo, agora: i64) -> Saida {
    let falhas = anterior.falhas_seguidas.saturating_add(1);
    let base = declaracao.intervalo_minimo_s;
    let segundos = base.saturating_mul(1 << falhas.saturating_sub(1).min(6)).min(RECUO_MAX_S).max(base);
    let milis = i64::try_from(segundos.saturating_mul(1000)).unwrap_or(i64::MAX);
    let mut estado = anterior.clone();
    estado.falhas_seguidas = falhas;
    estado.nao_antes = agora.saturating_add(milis);
    estado.ultima = Some(UltimaChecagem {
        em: agora,
        resultado: Resultado::FonteIndisponivel,
        abertas: anterior.ultima.map_or(0, |u| u.abertas),
    });
    let alertar = if motivo.exige_acao() { falhas == 1 } else { falhas == FALHAS_ATE_ALERTA };
    Saida { resultado: Resultado::FonteIndisponivel, estado, aviso: alertar.then_some(Aviso::Falha { motivo }) }
}

fn assinar(itens: &[Item]) -> String {
    let mut linhas: Vec<String> = itens.iter().map(|i| format!("{}\0{}\0{}", i.id, i.versao, i.estado)).collect();
    linhas.sort();
    hex(&Sha256::digest(linhas.join("\n").as_bytes()))
}

/// Identifica uma pendência pelo item e pelo estado, não pelo conteúdo: o mesmo pedido aberto não vira aviso novo
/// só porque o texto mudou; ele volta a ser novo se sair da fonte e reaparecer.
fn impressao(item: &Item) -> String {
    hex(&Sha256::digest(format!("{}\0{}", item.id, item.estado.trim().to_lowercase()).as_bytes()))[..32].to_owned()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut texto, byte| {
        let _ = write!(texto, "{byte:02x}");
        texto
    })
}

/// Uma linha curta, sem caracteres de controle: o texto vem de fora e só é mostrado, nunca executado.
fn limpar(texto: &str) -> String {
    let junto = texto
        .split(|c: char| c.is_control() || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if junto.chars().count() <= MAX_TEXTO {
        return junto;
    }
    let mut curto: String = junto.chars().take(MAX_TEXTO - 1).collect();
    curto.push('…');
    curto
}
