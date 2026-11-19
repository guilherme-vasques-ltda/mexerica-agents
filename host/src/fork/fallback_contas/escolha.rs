//! F6 fatia B: a decisão de qual conta recebe o próximo pedido (regras (m) e (o), com (f) e (k) por baixo).
//!
//! Percorre a fila `origem → reservas` e devolve a primeira conta livre. Livre quer dizer: assinatura comprovada
//! (nunca chave de API nem `unverified`), mesmo motor, modelo comprovado nela, sem bloqueio de 1 h, sem descanso e
//! sem leitura >= 90% da sessão ou da semana. Sem leitura de uso só valem esgotada e erro (regra (m)). A decisão
//! só escolhe; a prova da conta ((k)) e o teto ((f)) são feitos por quem envia, a cada tentativa.
use super::estado::{self, Descanso, Estado, Motivo};
use crate::fork::engine_accounts::{Billing, Registry};
use crate::hub::Hub;
use crate::store::BotConfig;
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;

pub(crate) struct Pedido<'a> {
    pub space: &'a str,
    pub bot: &'a str,
    /// A escolha do dono, sem sobreposição.
    pub origem: &'a str,
    /// A conta em que a conversa roda agora (a sobreposição, ou a origem).
    pub atual: &'a str,
    /// A seleção efetiva (motor, modelo, opções): a reserva precisa comprová-la.
    pub cfg: &'a BotConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Decisao {
    /// Sem lista de reserva, ou a origem não é assinatura comprovada: nada muda (regra (a)).
    SemFallback,
    /// A conta atual segue valendo.
    Manter,
    /// Passa para `para`; `de` é a conta que o pedido deixa.
    Trocar { para: String, motivo: Motivo, volta_em: Option<i64> },
    /// A origem voltou a ser a primeira conta livre.
    Voltar,
    /// Nenhuma conta da fila pode receber o pedido: para e avisa (regra (m).2).
    Parar { conta: String, motivo: Motivo, volta_em: Option<i64> },
}

enum Aval {
    Livre,
    Ocupada {
        motivo: Motivo,
        volta_em: Option<i64>,
    },
    /// Não pode ser reserva (não comprovada, outro motor, modelo ausente, bloqueada): nem aparece como "volta".
    Fora,
}

/// A fila `origem → reservas`: cada conta uma vez, até 3 reservas.
pub(crate) fn fila(estado: &Estado, p: &Pedido<'_>) -> Vec<String> {
    let mut fila = vec![p.origem.to_owned()];
    for conta in estado.ordem(p.space, p.bot, &p.cfg.backend) {
        if !fila.contains(&conta) {
            fila.push(conta);
        }
    }
    fila
}

fn sonda(cfg: &BotConfig, conta: &str) -> BotConfig {
    let mut sonda = cfg.clone();
    sonda.fork_editor.engine_account_id = Some(conta.to_owned());
    sonda
}

/// A conta pode receber trabalho por troca automática: assinatura comprovada (regra (o).1 e (f)), conectada, ligada,
/// do mesmo motor e com o modelo desta conversa comprovado.
pub(crate) fn elegivel(registry: &Registry, cfg: &BotConfig, conta: &str) -> bool {
    let Ok(account) = registry.account(conta) else { return false };
    account.billing == Billing::Subscription
        && account.subscription_verified()
        && account.backend.id() == cfg.backend
        && crate::fork::engine_accounts::managed::validate_selection_in(registry, &sonda(cfg, conta)).is_ok()
}

/// A leitura de uso (fatia A) resumida: o maior percentual entre as janelas (`None` = sem leitura comprovada) e o maior
/// horário de volta entre as que passaram do limiar.
struct Leitura {
    maximo: Option<f64>,
    volta: Option<i64>,
}

fn resumir(leitura: &Value) -> Leitura {
    let janelas = (leitura["state"] == "available").then(|| leitura["windows"].as_array()).flatten();
    let Some(janelas) = janelas else { return Leitura { maximo: None, volta: None } };
    let maximo = janelas
        .iter()
        .filter_map(|w| w["percent"].as_f64())
        .fold(None, |m: Option<f64>, p| Some(m.map_or(p, |m| m.max(p))));
    let volta = janelas
        .iter()
        .filter(|w| w["percent"].as_f64().is_some_and(passou))
        .filter_map(|w| w["resetsAt"].as_i64())
        .max();
    Leitura { maximo, volta }
}

/// Um descanso por "perto do limite" só acaba com leitura comprovada abaixo desta folga (histerese: sem ela a conta
/// bateria entre 89% e 90% a cada leitura) ou quando o horário de volta chega.
const FOLGA_PERCENT: f64 = crate::usage::NEAR_LIMIT_PERCENT - 5.0;

fn passou(percent: f64) -> bool {
    percent >= crate::usage::NEAR_LIMIT_PERCENT
}

async fn avaliar(
    hub: &Arc<Hub>,
    registry: &Registry,
    p: &Pedido<'_>,
    conta: &str,
    reserva: bool,
    agora: i64,
) -> Result<Aval> {
    let estado = estado::ler(&hub.store)?;
    if reserva && (estado.bloqueada(conta, agora) || !elegivel(registry, p.cfg, conta)) {
        return Ok(Aval::Fora);
    }
    let extra = estado.uso_extra_vigente(conta, agora);
    if let Some(d) = estado.descansando(conta, agora)
        && !extra
        && d.motivo == Motivo::Esgotada
    {
        return Ok(Aval::Ocupada { motivo: d.motivo, volta_em: Some(d.ate) });
    }
    // Leitura refeita (validade de 60 s por conta, fatia A): sem leitura comprovada, só esgotada e erro valem.
    let space = Value::String(p.space.to_owned());
    let leitura = crate::fork::engine_setup::usage(hub, conta.to_owned(), Some(&space)).await.ok();
    let lida = leitura.as_ref().map_or(Leitura { maximo: None, volta: None }, resumir);
    if lida.maximo.is_some_and(passou) && !extra {
        let ate = lida.volta.unwrap_or(agora + estado::DESCANSO_SEM_LEITURA_MS);
        estado::alterar(&hub.store, |e| {
            e.descanso.insert(conta.to_owned(), Descanso { ate, motivo: Motivo::PertoDoLimite });
            Ok(())
        })?;
        return Ok(Aval::Ocupada { motivo: Motivo::PertoDoLimite, volta_em: lida.volta });
    }
    // Descanso por "perto do limite" vigente: sem leitura comprovada abaixo da folga, a conta continua parada.
    if !extra && let Some(d) = estado.descansando(conta, agora).filter(|d| d.motivo == Motivo::PertoDoLimite) {
        if lida.maximo.is_some_and(|m| m < FOLGA_PERCENT) {
            estado::alterar(&hub.store, |e| {
                e.descanso.remove(conta);
                Ok(())
            })?;
        } else {
            return Ok(Aval::Ocupada { motivo: d.motivo, volta_em: Some(d.ate) });
        }
    }
    Ok(Aval::Livre)
}

pub(crate) async fn decidir(hub: &Arc<Hub>, p: &Pedido<'_>) -> Result<Decisao> {
    let estado = estado::ler(&hub.store)?;
    let registry = Registry::load(&hub.store)?;
    let reservas = fila(&estado, p);
    // Sem lista (ou com a origem fora de "assinatura → assinatura"): nada muda, e uma sobreposição velha se desfaz.
    let origem_ok = registry.account(p.origem).is_ok_and(|a| a.billing == Billing::Subscription)
        && matches!(p.cfg.backend.as_str(), "claude" | "codex");
    if reservas.len() < 2 || !origem_ok {
        return Ok(if p.atual == p.origem { Decisao::SemFallback } else { Decisao::Voltar });
    }
    let agora = crate::store::now_ms();
    let mut primeiro: Option<(Motivo, Option<i64>)> = None;
    let mut voltas: Vec<i64> = Vec::new();
    for (i, conta) in reservas.iter().enumerate() {
        match avaliar(hub, &registry, p, conta, i > 0, agora).await? {
            Aval::Livre => {
                return Ok(if conta == p.atual {
                    Decisao::Manter
                } else if conta == p.origem {
                    Decisao::Voltar
                } else {
                    let (motivo, volta_em) = primeiro.unwrap_or((Motivo::Esgotada, None));
                    Decisao::Trocar { para: conta.clone(), motivo, volta_em }
                });
            }
            Aval::Ocupada { motivo, volta_em } => {
                primeiro.get_or_insert((motivo, volta_em));
                voltas.extend(volta_em);
            }
            Aval::Fora => {}
        }
    }
    let (motivo, _) = primeiro.unwrap_or((Motivo::Esgotada, None));
    Ok(Decisao::Parar { conta: p.origem.to_owned(), motivo, volta_em: voltas.into_iter().min() })
}
