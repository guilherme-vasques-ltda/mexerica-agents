//! Admissão do par Team: contador, marcadores e avisos pertencem ao mesmo commit.
use super::{Entry, EntryKind, Lane, Store, bots, entries, turn_alloc};
use crate::LockExt;
use crate::fork::cadeias::{equipe, modelo::Recibo};
use crate::fork::retornos;
use crate::fork::sessoes::{ExecutionRef, bindings};
use anyhow::{Result, anyhow, ensure};
use rusqlite::Connection;
use serde_json::Value;

fn check(db: &Connection, lane: &Lane, notices: &[Value; 2], execution: Option<&ExecutionRef>) -> Result<String> {
    let from = notices[0]["sourceBotId"].as_str().ok_or_else(|| anyhow!("team_origin_required"))?;
    let to = notices[0]["targetBotId"].as_str().ok_or_else(|| anyhow!("team_target_changed"))?;
    ensure!(from != to && notices[0]["delegationId"].is_string(), "team_target_changed");
    for notice in notices {
        ensure!(
            notice["sourceBotId"] == from
                && notice["targetBotId"] == to
                && notice["delegationId"] == notices[0]["delegationId"],
            "team_target_changed"
        );
    }
    let visible = |id| {
        bots::read_on(db, id)?
            .filter(|b| !b.deleted && !b.config.hidden && !b.config.is_group())
            .ok_or_else(|| anyhow!("unknown or hidden bot"))
    };
    let source = visible(from)?;
    let target = visible(to)?;
    ensure!(source.config.space_id == target.config.space_id, "cross_space_reference");
    if let Some(execution) = execution {
        super::check_execution_on(db, lane, Some(execution))?;
        bindings::resolve(db, execution.acp_binding().ok_or_else(|| anyhow!("chamada vencida"))?, from)?;
    } else {
        // Produtor interno legado; nenhuma falha de `ExecutionRef` tenta este ramo.
        ensure!(lane == &Lane::main(from), "team_origin_required");
    }
    Ok(source.config.space_id)
}

/// Par gravado (marcador opcional + aviso) e o recibo D220 do repasse, se houver.
type AdmittedNotices = ([(Option<Entry>, Entry); 2], Option<Recibo>);

impl Store {
    pub(crate) fn insert_team_notices(
        &self,
        source: &Lane,
        turn: Option<i64>,
        notices: &[Value; 2],
        execution: Option<&ExecutionRef>,
        chain: &equipe::Entrada,
    ) -> Result<AdmittedNotices> {
        let mut db = self.db.locked();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let space = check(&tx, source, notices, execution)?;
        ensure!(
            notices[0]["sourceBotId"] == chain.de()
                && notices[0]["targetBotId"] == chain.para()
                && notices[0]["delegationId"] == chain.pedido(),
            "team_target_changed"
        );
        // D220 antes de qualquer aviso/turno; recusa ou falha posterior desfaz a reserva.
        let receipt = equipe::admitir_na(&tx, chain, Some(space.as_str()), execution)?;
        let target = Lane::main(notices[0]["targetBotId"].as_str().ok_or_else(|| anyhow!("team_target_changed"))?);
        let source_turn = match turn {
            Some(turn) => turn,
            None => {
                tx.query_row("SELECT COALESCE(MAX(turn), 0) FROM entries WHERE bot_id=?1", [&source.chat], |r| {
                    r.get(0)
                })?
            }
        };
        let target_turn = turn_alloc::alloc_on(&tx, &target.chat)?;
        let first = entries::insert_visible_on(
            &tx,
            &uuid::Uuid::new_v4().to_string(),
            source,
            EntryKind::Notice,
            source_turn,
            &notices[0],
        )?;
        let second = entries::insert_visible_on(
            &tx,
            &uuid::Uuid::new_v4().to_string(),
            &target,
            EntryKind::Notice,
            target_turn,
            &notices[1],
        )?;
        // R1: pedido durável no mesmo commit; só metadados e IDs dos avisos acima.
        if let Some(execution) = execution {
            let recibo = receipt.as_ref().ok_or_else(|| anyhow!("retorno_cadeia_invalida"))?;
            let pedido = retornos::NovoPedido {
                id: chain.pedido(),
                modo: if chain.espera() { retornos::Modo::Espera } else { retornos::Modo::Assincrono },
                de: chain.de(),
                para: chain.para(),
                execucao: execution,
                recibo,
                aviso_origem: &first.1.id,
                aviso_destino: &second.1.id,
            };
            retornos::aceitar_na(&tx, &pedido)?;
        }
        check(&tx, source, notices, execution)?;
        tx.commit()?;
        Ok(([first, second], receipt))
    }
}
