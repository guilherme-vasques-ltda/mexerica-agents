//! MB-3 "O que aconteceu" (D323, D329): resumo do dia por Time, de baixo para cima, só para o
//! Mission Board. Uma chamada isolada por bot que trabalhou, na assinatura Claude do próprio bot,
//! sem ferramentas nem memória. Não manda mensagem, não muda o quadro nem permissões e não
//! segura avisos urgentes.
pub(crate) mod api;
mod entrada;
mod plano;
mod store;
mod torre;

pub(crate) use store::{Config, fuso_local, migrate, on_bot_deleted};
pub(crate) use torre::{WhatHappened, torre};

use crate::fork::background_notice::Problem;
use crate::fork::cadeias::{
    admissao,
    modelo::{Escopo, Tipo},
};
use crate::fork::spaces::PERSONAL;
use crate::{
    LockExt,
    hub::Hub,
    store::{BotConfig, Store, now_ms},
};
use anyhow::Result;
use serde_json::json;
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use store::{Estado, Fim};

const DIA_MS: i64 = 24 * 60 * 60 * 1000;

/// Chamada isolada ao motor; nos testes, um motor simulado.
pub(super) type Motor = dyn Fn(BotConfig, String) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> + Send + Sync;

/// Por que o bot não faz resumo próprio. Sem caminho de fundo para Codex, API ou outro motor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fora {
    Codex,
    Api,
    OutroMotor,
    SemConta,
    /// Sem modelo escolhido e sem Haiku na conta.
    SemModelo,
}

impl Fora {
    pub(super) fn sql(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Api => "api",
            Self::OutroMotor => "other_engine",
            Self::SemConta => "no_account",
            Self::SemModelo => "no_model",
        }
    }
}

pub(super) fn fora(store: &Store, cfg: &BotConfig) -> Option<Fora> {
    if cfg.backend == "codex" {
        return Some(Fora::Codex);
    }
    if cfg.backend != "claude" || cfg.command.is_some() {
        return Some(Fora::OutroMotor);
    }
    let error = crate::fork::background_selection::resolve_own(store, cfg).err()?;
    Some(match error.downcast_ref::<Problem>() {
        Some(Problem::Api) => Fora::Api,
        Some(Problem::Codex) => Fora::Codex,
        Some(Problem::Model) => Fora::SemModelo,
        _ => Fora::SemConta,
    })
}

fn motivo(error: &anyhow::Error) -> &'static str {
    if error.downcast_ref::<crate::fork::background_executor::SubscriptionLimit>().is_some() {
        return "quota";
    }
    match error.downcast_ref::<Problem>() {
        Some(Problem::PersonalSubscription) => "personal_subscription_chat_only",
        Some(Problem::Timeout) => "timeout",
        Some(Problem::AccountChanged | Problem::NoClaude | Problem::Model) => "account",
        _ => "error",
    }
}

pub(crate) fn start(hub: &Arc<Hub>) {
    let hub = hub.clone();
    tokio::spawn(async move {
        let real = hub.clone();
        let motor: Box<Motor> = Box::new(move |cfg, user| {
            let hub = real.clone();
            Box::pin(async move {
                let mut response =
                    crate::fork::background_executor::one_shot_own(&hub, &cfg, entrada::SISTEMA, &user).await?;
                response.finish(crate::fork::background_history::Status::ResponseReceived);
                Ok(std::mem::take(&mut response.text))
            })
        });
        let mut every = tokio::time::interval(Duration::from_secs(60));
        loop {
            every.tick().await;
            if hub.routines.stopping() {
                break;
            }
            if let Err(error) = tick(&hub, now_ms(), &*motor).await {
                tracing::warn!(error = format!("{error:#}"), "resumo do dia não rodou");
            }
        }
    });
}

/// Uma passada: roda, de baixo para cima, os bots cujo horário chegou no dia local atual.
/// O de cima espera os de baixo do mesmo dia terminarem. Devolve se algo mudou.
pub(super) async fn tick(hub: &Arc<Hub>, agora: i64, motor: &Motor) -> Result<bool> {
    let cfg = store::config(&hub.store)?;
    let pausado = || hub.routines.pause_state()["paused"] == true;
    if !cfg.enabled || pausado() {
        return Ok(false);
    }
    let (dia, fecha) = plano::dia_atual(&cfg, agora);
    let quadro = {
        let c = hub.store.db.locked();
        let tx = c.unchecked_transaction()?;
        crate::fork::times::resumo::quadro(&tx, PERSONAL)?
    };
    let vezes = plano::vezes(&quadro);
    let mut mudou = false;
    for vez in &vezes {
        // "Pausar tudo" acionado durante uma chamada vale para os bots seguintes.
        if pausado() {
            break;
        }
        if plano::horario(fecha, vez.nivel) > agora {
            continue;
        }
        let (atual, abaixo_pendente) = {
            let c = hub.store.db.locked();
            let atual = store::linha_na(&c, &vez.bot_id, &dia)?;
            let mut pendente = false;
            for baixo in vezes.iter().filter(|v| v.up.as_deref() == Some(vez.bot_id.as_str())) {
                // Bot pausado não segura o de cima; ele roda em atraso quando voltar.
                pendente |= store::linha_na(&c, &baixo.bot_id, &dia)?.is_none_or(|l| !l.estado.terminal())
                    && !crate::fork::bot_admission::paused_in(&c, &baixo.bot_id)?;
            }
            (atual, pendente)
        };
        if atual.is_some_and(|l| l.estado != Estado::Interrompido) || abaixo_pendente {
            continue;
        }
        mudou |= rodar(hub, motor, vez, &dia, &quadro, agora).await?;
    }
    if mudou {
        let _ = hub
            .events
            .send(json!({"type": crate::fork::control_tower::EVENT, "spaceId": PERSONAL, "rosterChanged": true}));
    }
    Ok(mudou)
}

async fn rodar(
    hub: &Arc<Hub>,
    motor: &Motor,
    vez: &plano::Vez,
    dia: &str,
    quadro: &crate::fork::times::resumo::Quadro,
    agora: i64,
) -> Result<bool> {
    let id = vez.bot_id.as_str();
    let up = vez.up.as_deref();
    let pular = |reason: &str| -> Result<bool> {
        store::pular_na(&hub.store.db.locked(), id, dia, up, reason)?;
        Ok(true)
    };
    let Some(cfg) =
        hub.store.bot(id)?.filter(|r| !r.deleted).map(|r| r.config).filter(|c| !c.is_group() && c.space_id == PERSONAL)
    else {
        return pular("missing");
    };
    // Pausa não consome o dia: nada é gravado e o bot roda em atraso ao voltar (P3).
    if crate::fork::bot_admission::paused(&hub.store, id)? {
        return Ok(false);
    }
    let nome = |bot: &str| {
        quadro.elos.iter().find(|e| e.bot_id == bot).map_or_else(|| "outro bot".to_owned(), |e| e.name.clone())
    };
    let (desde, entrada) = {
        let c = hub.store.db.locked();
        let desde = store::ultimo_corte_na(&c, id)?.unwrap_or(0).max(agora - DIA_MS);
        let recebidos = store::recebidos_na(&c, id, dia)?;
        (desde, entrada::montar_na(&c, id, desde, recebidos, &nome)?)
    };
    // Zero chamada: nada próprio desde o último resumo e nada recebido.
    if entrada.vazia() {
        return pular("no_activity");
    }
    let juntado = entrada::juntar(&entrada.recebidos);
    if let Some(motivo) = fora(&hub.store, &cfg) {
        // Sem IA: repassa ao de cima, junto pelo computador, o que veio de baixo.
        return match &juntado {
            Some(team) => {
                if store::comecar_na(&hub.store.db.locked(), id, dia, up, desde)?.is_none() {
                    return pular("restart_limit");
                }
                let fim = Fim {
                    estado: Estado::Feito,
                    reason: Some(motivo.sql()),
                    bot_line: None,
                    team_line: Some(team),
                    consome: true,
                };
                store::terminar(&hub.store, id, dia, &fim).map(|()| true)
            }
            None => pular(motivo.sql()),
        };
    }
    let Some(tentativa) = store::comecar_na(&hub.store.db.locked(), id, dia, up, desde)? else {
        return pular("restart_limit");
    };
    let falhou = |reason: &str| {
        let fim = match &juntado {
            Some(team) => Fim {
                estado: Estado::Feito,
                reason: Some(reason),
                bot_line: None,
                team_line: Some(team),
                consome: true,
            },
            None => {
                Fim { estado: Estado::Falhou, reason: Some(reason), bot_line: None, team_line: None, consome: false }
            }
        };
        store::terminar(&hub.store, id, dia, &fim).map(|()| true)
    };
    // D220: cada execução é uma cadeia própria, nível 1 e ordinal 1; nunca atravessa níveis.
    let run = uuid::Uuid::new_v4().to_string();
    let mut pedido = admissao::pedido(
        Tipo::Raiz,
        format!("daily-summary:{id}:{dia}:{tentativa}"),
        admissao::digest(&("daily-summary", id, dia))?,
        id,
        &run,
        None,
    );
    pedido.escopo = Escopo::ConversaDoBot { bot_id: id.into(), space_id: Some(PERSONAL.into()) };
    // Sem Run de Rotina para vincular: admite e conclui; `despachar` é só das Rotinas.
    let Ok(recibo) = admissao::admitir(&hub.store, &pedido) else {
        return falhou("chain");
    };
    store::cadeia_na(&hub.store.db.locked(), id, dia, &recibo.chain_id)?;
    let resposta = motor(cfg, entrada::usuario(&nome(id), dia, &entrada)).await;
    if let Err(error) = admissao::concluir(&hub.store, &recibo) {
        tracing::warn!(error = format!("{error:#}"), "cadeia do resumo do dia não concluída");
    }
    match resposta {
        Ok(text) => match entrada::ler_resposta(&text) {
            None => store::terminar(
                &hub.store,
                id,
                dia,
                &Fim { estado: Estado::Silencio, reason: None, bot_line: None, team_line: None, consome: true },
            )
            .map(|()| true),
            Some((bot, team)) => {
                let team = team.filter(|_| !entrada.recebidos.is_empty());
                let fim = Fim {
                    estado: Estado::Feito,
                    reason: None,
                    bot_line: Some(&bot),
                    team_line: team.as_deref(),
                    consome: true,
                };
                store::terminar(&hub.store, id, dia, &fim).map(|()| true)
            }
        },
        Err(error) if error.downcast_ref::<crate::fork::background_executor::Busy>().is_some() => store::terminar(
            &hub.store,
            id,
            dia,
            &Fim { estado: Estado::Interrompido, reason: None, bot_line: None, team_line: None, consome: false },
        )
        .map(|()| true),
        Err(error) => falhou(motivo(&error)),
    }
}

#[cfg(test)]
mod testes;
