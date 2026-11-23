//! `donnaEngineFallback` (lê), `donnaEngineFallbackSave` (ordem de reserva) e `donnaEngineFallbackAction`
//! (Voltar agora, Permitir uso extra). Mesma capacidade das contas de IA (`engine.accounts`); só o dono, com a
//! autoridade original reconferida na transação.
use super::estado::{self, Estado, MAX_RESERVAS};
use crate::LockExt;
use crate::fork::engine_accounts::Registry;
use crate::fork::engine_setup::access::Authority;
use crate::hub::Hub;
use anyhow::{Result, anyhow, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ler {
    #[serde(default)]
    space_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Salvar {
    #[serde(default)]
    space_id: Option<String>,
    backend: String,
    #[serde(default)]
    bot_id: Option<String>,
    /// `null` com `botId` tira a ordem só daquele bot (volta a valer a do Space).
    accounts: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Acao {
    #[serde(default)]
    space_id: Option<String>,
    action: String,
    engine_account_id: String,
}

fn parse<T: serde::de::DeserializeOwned>(body: Value) -> Result<T> {
    serde_json::from_value(body).map_err(|_| anyhow!("invalid_field: solicitação do fallback inválida."))
}

fn space(hub: &Hub, id: Option<String>) -> Result<String> {
    let space = id.unwrap_or_else(|| crate::fork::spaces::PERSONAL.to_owned());
    ensure!(!space.is_empty() && space.len() <= 100, "invalid_space");
    crate::fork::spaces::require_space(&hub.store.db.locked(), &space).map_err(|_| anyhow!("invalid_space"))?;
    // Esta fatia só troca a conversa do dono no Pessoal; ordem salva em outro Space seria promessa sem efeito.
    ensure!(
        space == crate::fork::spaces::PERSONAL,
        "fallback_space_unsupported: o fallback só vale no Pessoal nesta versão."
    );
    Ok(space)
}

pub(crate) fn call(hub: &Arc<Hub>, authority: &Authority, method: &str, body: Value) -> Result<Value> {
    match method {
        "donnaEngineFallback" => {
            let request: Ler = parse(body)?;
            let space = space(hub, request.space_id)?;
            leitura(hub, &space)
        }
        "donnaEngineFallbackSave" => {
            let request: Salvar = parse(body)?;
            let space = space(hub, request.space_id.clone())?;
            salvar(hub, authority, &space, &request)?;
            leitura(hub, &space)
        }
        "donnaEngineFallbackAction" => {
            let request: Acao = parse(body)?;
            let space = space(hub, request.space_id.clone())?;
            acao(hub, authority, &space, &request)?;
            leitura(hub, &space)
        }
        _ => bail!("invalid_field: método do fallback desconhecido."),
    }
}

fn motor(backend: &str) -> Result<&str> {
    ensure!(!crate::fork::assinatura_pessoal::claude(backend), crate::fork::assinatura_pessoal::Refusal::Fallback);
    ensure!(backend == "codex", "invalid_field: motor sem troca automática.");
    Ok(backend)
}

/// Só assinatura comprovada e conectada entra como reserva; chave de API nunca (regra (o).1).
fn validar_contas(registry: &Registry, backend: &str, contas: &[String]) -> Result<()> {
    ensure!(contas.len() <= MAX_RESERVAS, "fallback_too_many: até {MAX_RESERVAS} reservas.");
    for (i, conta) in contas.iter().enumerate() {
        ensure!(!contas[..i].contains(conta), "fallback_duplicate: cada conta aparece uma vez.");
        let account = registry.account(conta).map_err(|_| anyhow!("fallback_account_unknown"))?;
        ensure!(account.backend.id() == backend, "account_incompatible: outro motor não entra como reserva.");
        ensure!(
            account.subscription_verified() && account.managed.as_ref().is_none_or(|m| m.enabled),
            "fallback_not_subscription: só assinatura comprovada entra como reserva."
        );
    }
    Ok(())
}

fn salvar(hub: &Arc<Hub>, authority: &Authority, space: &str, request: &Salvar) -> Result<()> {
    let backend = motor(&request.backend)?.to_owned();
    let registry = Registry::load(&hub.store)?;
    if let Some(contas) = &request.accounts {
        validar_contas(&registry, &backend, contas)?;
    }
    authority.with_db(hub, |db| {
        // A reserva precisa de vínculo de verdade com o Space (um bot dele a usa), além de constar da lista.
        for conta in request.accounts.iter().flatten() {
            ensure!(
                crate::fork::engine_accounts::managed::linked_to_space(db, conta, space)?,
                "account_wrong_space: use a conta em um bot deste Space antes de torná-la reserva."
            );
        }
        if let Some(bot) = &request.bot_id {
            let row = crate::store::Store::bot_on(db, bot)?.filter(|b| !b.deleted);
            ensure!(row.is_some_and(|b| b.config.space_id == space), "bot_not_found");
        } else {
            ensure!(request.accounts.is_some(), "invalid_field: informe a ordem de reserva.");
        }
        estado::alterar_em(db, |e| {
            let (mapa, chave) = match &request.bot_id {
                Some(bot) => (&mut e.ordens_bot, estado::chave_ordem_bot(bot, &backend)),
                None => (&mut e.ordens, estado::chave_ordem(space, &backend)),
            };
            match &request.accounts {
                Some(contas) if !contas.is_empty() => {
                    mapa.insert(chave, contas.clone());
                }
                _ => {
                    mapa.remove(&chave);
                }
            }
            Ok(())
        })
    })
}

fn acao(hub: &Arc<Hub>, authority: &Authority, space: &str, request: &Acao) -> Result<()> {
    let conta = request.engine_account_id.as_str();
    authority.with_db(hub, |db| {
        ensure!(
            super::reserva_do_space(db, conta, space)?
                || crate::fork::engine_accounts::managed::linked_to_space(db, conta, space)?,
            "account_wrong_space"
        );
        let registry = Registry::load_db(db)?;
        motor(registry.account(conta)?.backend.id())?;
        estado::alterar_em(db, |e| {
            let agora = crate::store::now_ms();
            match request.action.as_str() {
                // Voltar agora encerra o descanso e o bloqueio; a próxima leitura decide se a conta já pode.
                "backNow" => {
                    e.descanso.remove(conta);
                    e.bloqueio.remove(conta);
                    e.uso_extra.remove(conta);
                }
                // Uso extra só até o reset informado, e só quando a conta está parada de fato.
                "allowExtra" => {
                    // O aviso de parada cita a conta em que a conversa roda agora; se ela é a reserva, o uso extra
                    // vale para a conta de origem que está parada.
                    let alvo = if e.descansando(conta, agora).is_some() {
                        conta.to_owned()
                    } else {
                        estado::todas_sobreposicoes(db)?
                            .into_iter()
                            .find(|(_, s)| {
                                s.space == space && s.atual == conta && e.descansando(&s.origem, agora).is_some()
                            })
                            .map(|(_, s)| s.origem)
                            .ok_or_else(|| anyhow!("fallback_extra_not_needed"))?
                    };
                    let ate = e
                        .descansando(&alvo, agora)
                        .map(|d| d.ate)
                        .ok_or_else(|| anyhow!("fallback_extra_not_needed"))?;
                    e.uso_extra.insert(alvo, ate);
                }
                _ => bail!("invalid_field: ação do fallback desconhecida."),
            }
            Ok(())
        })
    })
}

fn leitura(hub: &Arc<Hub>, space: &str) -> Result<Value> {
    let db = hub.store.db.locked();
    let estado = Estado::carregar(&db)?;
    let agora = crate::store::now_ms();
    let ordens: Vec<Value> = estado
        .ordens
        .iter()
        .filter_map(|(chave, contas)| {
            let (s, backend) = chave.split_once('|')?;
            (s == space).then(|| json!({"backend": backend, "accounts": contas}))
        })
        .collect();
    let mut bots = Vec::new();
    for (chave, contas) in &estado.ordens_bot {
        let Some((bot, backend)) = chave.split_once('|') else { continue };
        let vivo = crate::store::Store::bot_on(&db, bot)?.is_some_and(|b| !b.deleted && b.config.space_id == space);
        if vivo {
            bots.push(json!({"botId": bot, "backend": backend, "accounts": contas}));
        }
    }
    // Contas com descanso, bloqueio de 1 h ou uso extra permitido: o app mostra a hora e os botões.
    let mut ids: Vec<&String> =
        estado.descanso.keys().chain(estado.bloqueio.keys()).chain(estado.uso_extra.keys()).collect();
    ids.sort();
    ids.dedup();
    // Só contas deste Space (na ordem dele ou vinculadas a um bot dele): a resposta não mistura Spaces.
    let ids: Vec<&String> = ids
        .into_iter()
        .filter(|conta| {
            super::reserva_do_space(&db, conta, space).unwrap_or(false)
                || crate::fork::engine_accounts::managed::linked_to_space(&db, conta, space).unwrap_or(false)
        })
        .collect();
    let contas: Vec<Value> = ids
        .into_iter()
        .map(|conta| {
            let descanso = estado.descansando(conta, agora);
            json!({"engineAccountId": conta, "restUntil": descanso.map(|d| d.ate), "reason": descanso.map(|d| d.motivo),
                "extraUntil": estado.uso_extra.get(conta).filter(|ate| **ate > agora),
                "blockedUntil": estado.bloqueio.get(conta).filter(|ate| **ate > agora)})
        })
        .collect();
    let sessoes: Vec<Value> = estado::todas_sobreposicoes(&db)?
        .into_iter()
        .filter(|(_, s)| s.space == space)
        .map(|(id, s)| {
            json!({"sessionId": id, "botId": s.bot, "originAccountId": s.origem, "currentAccountId": s.atual,
                "since": s.desde, "reason": s.motivo,
                "originBackAt": estado.descansando(&s.origem, agora).map(|d| d.ate)})
        })
        .collect();
    let trocas: Vec<&estado::Troca> = estado.trocas.iter().filter(|t| t.space == space).rev().take(20).collect();
    Ok(json!({"spaceId": space, "orders": ordens, "botOrders": bots, "resting": contas, "sessions": sessoes,
        "switches": trocas}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fork::fallback_contas::estado::{Descanso, Motivo, Sobreposicao};
    use crate::remote::identity::Identity;
    use crate::store::Store;

    #[test]
    fn leitura_mostra_apenas_descansos_e_sessoes_do_space() {
        let dir = std::env::temp_dir().join(format!("fallback-isolamento-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("test.db")).unwrap();
        let hub = Hub::new(
            store,
            "00000000-0000-4000-8000-0000000000f6".into(),
            Identity::load_or_create(&dir).unwrap(),
            "test".into(),
            0,
        );
        let ate = crate::store::now_ms() + 60_000;
        estado::alterar(&hub.store, |e| {
            for (space, conta) in [("pessoal", "conta-a"), ("outro", "conta-b")] {
                e.ordens.insert(estado::chave_ordem(space, "claude"), vec![conta.into()]);
                e.descanso.insert(conta.into(), Descanso { ate, motivo: Motivo::Esgotada });
            }
            Ok(())
        })
        .unwrap();
        {
            let db = hub.store.db.locked();
            for (space, sessao, conta) in [("pessoal", "sessao-a", "conta-a"), ("outro", "sessao-b", "conta-b")] {
                estado::gravar_sobreposicao(
                    &db,
                    sessao,
                    Some(&Sobreposicao {
                        space: space.into(),
                        bot: "bot".into(),
                        origem: "origem".into(),
                        atual: conta.into(),
                        desde: 1,
                        motivo: Motivo::Esgotada,
                    }),
                )
                .unwrap();
            }
        }
        for (space, conta, sessao) in [("pessoal", "conta-a", "sessao-a"), ("outro", "conta-b", "sessao-b")] {
            let result = leitura(&hub, space).unwrap();
            assert_eq!(result["resting"].as_array().unwrap().len(), 1);
            assert_eq!(result["resting"][0]["engineAccountId"], conta);
            assert_eq!(result["sessions"].as_array().unwrap().len(), 1);
            assert_eq!(result["sessions"][0]["sessionId"], sessao);
        }
    }
}
