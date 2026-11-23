//! Leitura canônica e só leitura dos pedidos R1, reutilizável por Times (Mission Board,
//! VG-03b). Só metadados: nenhum texto, digest, aviso, cadeia, entrada ou proveniência.
//! Funções `*_na` usam somente a `Connection` recebida; sem Store, Cloud, await nem lock.
//! Linha de outra versão ou com estado/motivo desconhecido é descartada (falha fechada).
use super::consulta::{Estado, Linha, estado};
use super::privacidade::{self, MetadataProjection, ReadAuthority};
use super::{Desfecho, Modo};
use crate::fork::control_tower::Observation;
use crate::fork::spaces::PERSONAL;
use crate::store::now_ms;
use anyhow::Result;
use rusqlite::{Connection, params};
use serde::Serialize;

/// Janela do Mission Board (decisão de 05/11/2026, opção A).
pub(crate) const JANELA_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// Itens no Mission Board; o resto vira `omitted`.
pub(crate) const LIMITE: usize = 20;

/// Pedido tipado do R1, sem formato de tela.
pub(crate) struct Pedido {
    pub id: String,
    pub modo: Modo,
    /// Bot solicitante e destinatário, como gravados pela admissão.
    pub de: String,
    pub para: String,
    pub estado: Estado,
    pub atualizado_em: i64,
}

impl Pedido {
    /// Terminou sem sucesso: decidido sem conclusão ou concluído com falha/parada/incompleto.
    pub(crate) fn sem_sucesso(&self) -> bool {
        match self.estado {
            Estado::Aceito | Estado::EmExecucao => false,
            Estado::Encerrado(d, _) => d != Desfecho::Concluiu,
            Estado::Decidido(_) => true,
        }
    }
}

pub(crate) struct Filtro<'a> {
    /// `updated_at` mínimo (ms).
    pub desde: i64,
    pub limite: usize,
    pub manter: &'a dyn Fn(&Pedido) -> bool,
}

/// Linhas mantidas, da mais nova para a mais antiga, e o total antes do limite.
pub(crate) struct Leitura {
    pub linhas: Vec<Pedido>,
    pub total: usize,
}

fn modo(sql: &str) -> Option<Modo> {
    match sql {
        "wait" => Some(Modo::Espera),
        "async" => Some(Modo::Assincrono),
        _ => None,
    }
}

pub(crate) fn ler_na(c: &Connection, filtro: &Filtro<'_>) -> Result<Leitura> {
    ler_autorizado_na(c, filtro, None)
}

pub(super) fn ler_autorizado_na(
    c: &Connection,
    filtro: &Filtro<'_>,
    caller: Option<&ReadAuthority<'_>>,
) -> Result<Leitura> {
    let mut st = c.prepare(
        "SELECT r.id,r.mode,r.requester_bot_id,r.target_bot_id,r.updated_at,
           r.schema_version,r.kind,r.state,r.end_reason,r.attempt,
           (SELECT COUNT(*) FROM donna_session_results x WHERE x.request_id=r.id),
           s.schema_version,s.outcome,s.end_reason,
           EXISTS(SELECT 1 FROM donna_session_privacy p WHERE p.request_id=r.id)
         FROM donna_session_requests r
         LEFT JOIN donna_session_results s ON s.request_id=r.id AND s.attempt=r.attempt
         WHERE r.updated_at>=?1 ORDER BY r.updated_at DESC,r.id DESC",
    )?;
    let mut rows = st.query(params![filtro.desde])?;
    let (mut linhas, mut total) = (Vec::new(), 0);
    while let Some(r) = rows.next()? {
        // Tipo de coluna inesperado também descarta a linha, sem ecoar valores.
        let lida = (|| -> rusqlite::Result<_> {
            let linha = Linha {
                versao: r.get(5)?,
                kind: r.get(6)?,
                state: r.get(7)?,
                end_reason: r.get(8)?,
                attempt: r.get(9)?,
                resultados: r.get(10)?,
                res_versao: r.get(11)?,
                outcome: r.get(12)?,
                res_motivo: r.get(13)?,
            };
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                linha,
                r.get::<_, bool>(14)?,
            ))
        })();
        let Ok((id, modo_sql, de, para, atualizado_em, linha, qualified)) = lida else { continue };
        let (Some(modo), Ok(estado)) = (modo(&modo_sql), estado(&linha)) else { continue };
        let pedido = Pedido { id, modo, de, para, estado, atualizado_em };
        if !(filtro.manter)(&pedido)
            || (qualified
                && (caller.is_none()
                    || !privacidade::metadata_visible_na(c, &pedido.id, MetadataProjection::MissionBoard, caller)?))
        {
            continue;
        }
        total += 1;
        if linhas.len() < filtro.limite {
            linhas.push(pedido);
        }
    }
    Ok(Leitura { linhas, total })
}

/// Seção "Pedidos entre bots" do Mission Board. DTO de Times; nada de texto ou IDs internos.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InterBotRequests {
    pub items: Vec<InterBotItem>,
    pub omitted: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InterBotItem {
    /// ID opaco do pedido, só para a key da lista.
    pub key: String,
    pub requester_bot_id: String,
    pub target_bot_id: String,
    pub state: &'static str,
    pub reason: Option<&'static str>,
    pub outcome: Option<&'static str>,
    pub mode: &'static str,
    pub ended_at: i64,
}

/// Na Tx da projeção: só no Pessoal, só com os dois bots no roster já autorizado, 7 dias,
/// 20 itens. Outro space devolve `Unavailable` sem ler o R1.
pub(crate) fn torre_na(c: &Connection, space: &str, roster: &[&str]) -> Result<Observation<InterBotRequests>> {
    if space != PERSONAL {
        return Ok(Observation::Unavailable);
    }
    let manter = |p: &Pedido| p.sem_sucesso() && roster.contains(&p.de.as_str()) && roster.contains(&p.para.as_str());
    let lidos = ler_na(c, &Filtro { desde: now_ms() - JANELA_MS, limite: LIMITE, manter: &manter })?;
    let items = lidos
        .linhas
        .into_iter()
        .filter_map(|p| {
            let (state, reason, outcome) = match p.estado {
                Estado::Encerrado(d, _) => ("finished", None, Some(d.sql())),
                Estado::Decidido(m) => (m.estado(), Some(m.sql()), None),
                // `manter` já excluiu os abertos.
                Estado::Aceito | Estado::EmExecucao => return None,
            };
            Some(InterBotItem {
                key: p.id,
                requester_bot_id: p.de,
                target_bot_id: p.para,
                state,
                reason,
                outcome,
                mode: p.modo.sql(),
                ended_at: p.atualizado_em,
            })
        })
        .collect::<Vec<_>>();
    Ok(Observation::Available {
        source: "retornos.pendentes",
        coverage: "personalOwnConversationsLast7Days",
        value: InterBotRequests { omitted: lidos.total - items.len(), items },
    })
}
