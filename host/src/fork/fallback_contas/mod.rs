//! F6 fatia B (MEX-1391, plano `docs/plans/fallback-entre-contas.md`): troca de conta perto do limite.
//!
//! Quando a conta da conversa chega perto do limite (>= 90% da sessão ou da semana) ou acaba, o Hub passa a conversa
//! para a próxima conta do MESMO motor e do MESMO Space, na ordem que o dono montou, e avisa. A troca é uma
//! sobreposição da escolha do dono por conversa (`estado::Sobreposicao`), lida onde a seleção efetiva é lida
//! (`engine_choice::read`): permit, recibo do Roteamento, carimbo e lançamento enxergam a conta nova, e a escolha do
//! dono não é reescrita — quando a conta dela volta, a sobreposição sai e a conversa retorna.
//!
//! - `estado`: ordens, descanso, bloqueio de 1 h, uso extra e diário, mais a sobreposição.
//! - `escolha`: regras (m) e (o) sobre a fila `origem → reservas`.
//! - `prova`: regra (k), refeita a cada troca e antes do envio.
//! - `api`: `donnaEngineFallback*` (capacidade `engine.accounts`).
//!
//! Teto de gasto (f), efeito incerto e aprovações (l) ficam com quem já os guarda: toda tentativa, de qualquer
//! conta, passa pelo mesmo permit.
pub(crate) mod api;
pub(crate) mod escolha;
pub(crate) mod estado;
pub(crate) mod prova;
pub(crate) mod torre;

use anyhow::Result;
use rusqlite::Connection;

pub(crate) use estado::{Motivo, Sobreposicao};

/// A conta consta de uma ordem de reserva do Space (a do Space ou a de um bot dele)?
pub(crate) fn reserva_do_space(c: &Connection, conta: &str, space: &str) -> Result<bool> {
    let estado = estado::Estado::carregar(c)?;
    if estado.ordens.iter().any(|(chave, contas)| {
        chave.split_once('|').is_some_and(|(s, _)| s == space) && contas.iter().any(|a| a == conta)
    }) {
        return Ok(true);
    }
    for (chave, contas) in &estado.ordens_bot {
        if !contas.iter().any(|a| a == conta) {
            continue;
        }
        let Some((bot, _)) = chave.split_once('|') else { continue };
        if crate::store::Store::bot_on(c, bot)?.is_some_and(|b| !b.deleted && b.config.space_id == space) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// A conta saiu do cadastro: some das ordens e dos descansos (a remoção já recusou se uma conversa a usa).
pub(crate) fn esquecer_conta(store: &crate::store::Store, conta: &str) -> Result<()> {
    estado::alterar(store, |e| {
        for lista in e.ordens.values_mut().chain(e.ordens_bot.values_mut()) {
            lista.retain(|a| a != conta);
        }
        e.ordens.retain(|_, l| !l.is_empty());
        e.ordens_bot.retain(|_, l| !l.is_empty());
        e.descanso.remove(conta);
        e.bloqueio.remove(conta);
        e.uso_extra.remove(conta);
        Ok(())
    })
}

#[cfg(test)]
mod tests;
