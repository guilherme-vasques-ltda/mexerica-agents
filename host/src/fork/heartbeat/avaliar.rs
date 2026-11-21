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
/// Pendências novas mostradas por aviso. As que não couberem ficam como novas e saem no aviso seguinte.
pub(crate) const MAX_NO_AVISO: usize = 5;

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
    // Outra declaração não herda a espera nem o intervalo da anterior.
    if estado.config.as_deref() != Some(config(declaracao).as_str()) {
        return true;
    }
    let Some(ultima) = estado.ultima else { return true };
    // A espera sai da declaração de agora, não do que ficou gravado: mudar o intervalo vale já.
    let espera_ms = if estado.falhas_seguidas > 0 {
        espera_ms(declaracao.intervalo_minimo_s, estado.falhas_seguidas)
    } else {
        i64::try_from(declaracao.intervalo_minimo_s.saturating_mul(1000)).unwrap_or(i64::MAX)
    };
    agora.saturating_sub(ultima.em) >= espera_ms
}

/// Quanto esperar depois de `falhas` falhas seguidas: o intervalo dobra a cada falha, até o maior entre seis horas
/// e o próprio intervalo.
fn espera_ms(base: u64, falhas: u32) -> i64 {
    let segundos = base.saturating_mul(1 << falhas.saturating_sub(1).min(6)).min(RECUO_MAX_S.max(base));
    i64::try_from(segundos.saturating_mul(1000)).unwrap_or(i64::MAX)
}

/// Resumo da declaração (fonte, escopo e cada estado do critério, um a um): a identidade a que o estado pertence.
fn config(declaracao: &Declaracao) -> String {
    let mut campos = vec![declaracao.fonte.as_str(), declaracao.escopo.as_str()];
    campos.extend(declaracao.criterio.estados.iter().map(String::as_str));
    resumir(&campos)
}

pub(crate) fn avaliar(fonte: &dyn Fonte, declaracao: &Declaracao, anterior: &Estado, agora: i64) -> Saida {
    // O que se sabia vale só para esta mesma declaração (fonte, escopo e critério): com outra, recomeça do zero.
    let config = config(declaracao);
    let zerado;
    let anterior = if anterior.config.as_deref() == Some(config.as_str()) {
        anterior
    } else {
        zerado = Estado { config: Some(config.clone()), ..Estado::default() };
        &zerado
    };
    let marcador = match fonte.marcador(&declaracao.escopo) {
        Ok(marcador) => marcador.map(|bruto| resumir(&[&bruto])),
        Err(motivo) => return falha(declaracao, anterior, motivo, agora),
    };
    // Marcador igual dispensa a leitura, a menos que ainda haja pendência por avisar (aviso limitado a
    // `MAX_NO_AVISO` por vez): sem isso, o resto nunca chegaria ao dono.
    // Depois de uma falha, o marcador sozinho não prova que a leitura voltou: relê o conteúdo.
    if marcador.is_some() && marcador == anterior.marcador && anterior.restantes == 0 && anterior.falhas_seguidas == 0 {
        let mut estado = anterior.clone();
        estado.falhas_seguidas = 0;
        estado.ultimo_motivo = None;
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
    let novas: Vec<(&String, &Item)> = abertas
        .iter()
        .filter(|(impressao, _)| !anterior.avisadas.contains(*impressao))
        .map(|(i, item)| (i, *item))
        .collect();
    let mostradas = &novas[..novas.len().min(MAX_NO_AVISO)];
    let resultado = if !novas.is_empty() {
        Resultado::DecisaoPendente
    } else if mudou {
        Resultado::MudancaSemDecisao
    } else {
        Resultado::SemMudanca
    };
    // Só vira «avisada» a pendência que o dono de fato leu neste aviso.
    let mut avisadas: Vec<String> = anterior.avisadas.iter().filter(|i| abertas.contains_key(*i)).cloned().collect();
    avisadas.extend(mostradas.iter().map(|(impressao, _)| (*impressao).clone()));
    avisadas.sort();
    let estado = Estado {
        config: anterior.config.clone(),
        marcador,
        assinatura: Some(assinatura),
        avisadas,
        ultima: Some(UltimaChecagem { em: agora, resultado, abertas: abertas.len() }),
        falhas_seguidas: 0,
        ultimo_motivo: None,
        nao_antes: 0,
        restantes: novas.len() - mostradas.len(),
    };
    let aviso = (!novas.is_empty()).then(|| Aviso::Decisao {
        itens: mostradas.iter().map(|(_, item)| (limpar(&item.fato), limpar(&item.escolha))).collect(),
        alem: novas.len() - mostradas.len(),
    });
    Saida { resultado, estado, aviso }
}

/// A fonte falhou: nada do que se sabia muda (marcador, assinatura e pendências avisadas ficam como estavam), o
/// contador sobe e a próxima consulta recua. Avisa quando a falha pede ação e é nova (primeira da sequência ou
/// de outro motivo), ou quando uma falha passageira persiste por `FALHAS_ATE_ALERTA` checagens.
fn falha(declaracao: &Declaracao, anterior: &Estado, motivo: Motivo, agora: i64) -> Saida {
    let falhas = anterior.falhas_seguidas.saturating_add(1);
    let milis = espera_ms(declaracao.intervalo_minimo_s, falhas);
    let mut estado = anterior.clone();
    estado.falhas_seguidas = falhas;
    estado.ultimo_motivo = Some(motivo);
    estado.nao_antes = agora.saturating_add(milis);
    estado.ultima = Some(UltimaChecagem {
        em: agora,
        resultado: Resultado::FonteIndisponivel,
        abertas: anterior.ultima.map_or(0, |u| u.abertas),
    });
    let ja_avisada_por_acao = anterior.ultimo_motivo.is_some_and(Motivo::exige_acao);
    let alertar = if motivo.exige_acao() {
        anterior.ultimo_motivo != Some(motivo)
    } else {
        falhas == FALHAS_ATE_ALERTA && !ja_avisada_por_acao
    };
    Saida { resultado: Resultado::FonteIndisponivel, estado, aviso: alertar.then_some(Aviso::Falha { motivo }) }
}

fn assinar(itens: &[Item]) -> String {
    let mut linhas: Vec<String> = itens.iter().map(|i| resumir(&[&i.id, &i.versao, &i.estado])).collect();
    linhas.sort();
    resumir(&linhas.iter().map(String::as_str).collect::<Vec<_>>())
}

/// Identifica uma pendência pelo item e pelo estado, não pelo conteúdo: o mesmo pedido aberto não vira aviso novo
/// só porque o texto mudou; ele volta a ser novo se sair da fonte e reaparecer.
fn impressao(item: &Item) -> String {
    resumir(&[&item.id, &item.estado.trim().to_lowercase()])[..32].to_owned()
}

/// Resumo SHA-256 de uma lista de campos, cada um precedido do seu tamanho: campos diferentes nunca dão a mesma
/// sequência de bytes, qualquer que seja o conteúdo. A fonte não controla o que fica guardado.
pub(super) fn resumir(campos: &[&str]) -> String {
    let mut sha = Sha256::new();
    for campo in campos {
        sha.update((campo.len() as u64).to_le_bytes());
        sha.update(campo.as_bytes());
    }
    sha.finalize().iter().fold(String::new(), |mut texto, byte| {
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
