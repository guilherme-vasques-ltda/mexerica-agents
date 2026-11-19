//! Marketplace 1a: migração do Space das contas anteriores ao campo `space`.
//! - todos os bots que usam a conta estão no MESMO Space → a conta vai para esse Space;
//! - nenhum bot a usa → Space pessoal, com o mesmo aviso ("nenhum bot perdeu acesso");
//! - usada em Spaces diferentes → Space pessoal, e o dono recebe UM aviso (`add_entry_once`) com os nomes dos bots
//!   que perderam a conta, sem dado sensível (nenhum nome de conta, endereço, token ou identidade).
//!
//! Só o registro de contas no cofre muda; nenhum bot é regravado, então o Berga e os principais de Space (gatilho
//! `donna_fixa`) nem entram nesta migração. A lista `connectors` do bot que perdeu a conta fica como estava: a
//! execução já recusa conta de outro Space (`exigir_mesmo_space`). A conta antiga é a que tem `space` vazio no registro.
//! A migração é idempotente: roda uma vez, com marca (`migration.contas-space.v1`).
use super::Connector;
use crate::LockExt;
use crate::fork::spaces::PERSONAL;
use crate::hub::Hub;
use crate::store::{BotConfig, EntryKind, Lane, Store};
use anyhow::Result;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const MARCA: &str = "migration.contas-space.v1";
/// Texto do aviso já pronto, guardado antes de gravar o cofre para sobreviver a uma queda no meio.
const PENDENTE: &str = "contas.space.aviso.v1";
const AVISO_ID: &str = "contas-space-v1";
const LISTA_MAX: usize = 10;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Plano {
    /// Conta (`id`) → Space de destino, só das contas antigas.
    pub destinos: BTreeMap<String, String>,
    /// Bots (`id` → nome) que usavam uma conta que foi para um Space diferente do deles.
    pub perdas: BTreeMap<String, String>,
    /// Quantas contas antigas nenhum bot usava (vão para o Pessoal; ninguém perdeu acesso).
    pub sem_uso: usize,
}

pub(crate) fn planejar(contas: &[Connector], bots: &[BotConfig]) -> Plano {
    let mut plano = Plano::default();
    for conta in contas.iter().filter(|c| c.space.is_empty()) {
        let usuarios: Vec<&BotConfig> = bots.iter().filter(|b| b.connectors.contains(&conta.id)).collect();
        let spaces: BTreeSet<&str> = usuarios.iter().map(|b| b.space_id.as_str()).collect();
        let destino = match spaces.iter().next() {
            Some(unico) if spaces.len() == 1 => (*unico).to_owned(),
            _ => PERSONAL.to_owned(),
        };
        if usuarios.is_empty() {
            plano.sem_uso += 1;
        }
        for bot in usuarios.iter().filter(|b| b.space_id != destino) {
            plano.perdas.insert(bot.id.clone(), bot.name.clone());
        }
        plano.destinos.insert(conta.id.clone(), destino);
    }
    plano
}

fn texto_do_aviso(plano: &Plano) -> String {
    let mut partes = vec!["Contas conectadas antes dos Spaces foram movidas para o Space pessoal.".to_owned()];
    if !plano.perdas.is_empty() {
        let mut nomes: Vec<&str> = plano.perdas.values().map(String::as_str).collect();
        nomes.sort_unstable();
        nomes.dedup();
        let resto = nomes.len().saturating_sub(LISTA_MAX);
        let mut lista = nomes.into_iter().take(LISTA_MAX).collect::<Vec<_>>().join(", ");
        if resto > 0 {
            lista = format!("{lista} e mais {resto}");
        }
        partes.push(format!(
            "Bots de Spaces diferentes usavam a mesma conta. Estes bots perderam o acesso: {lista}. Conecte a conta de novo dentro do Space deles, se quiser."
        ));
    }
    if plano.sem_uso > 0 {
        partes.push(format!(
            "{} conta(s) que nenhum bot usava foram para o Space pessoal; nenhum bot perdeu acesso por isso.",
            plano.sem_uso
        ));
    }
    partes.join(" ")
}

fn bots_vivos(store: &Store) -> Result<Vec<BotConfig>> {
    Ok(store.bots()?.into_iter().filter(|b| !b.deleted).map(|b| b.config).collect())
}

thread_local! {
    /// Só a execução da migração (`run`) pode esperar o banco; qualquer outra leitura do cofre nunca bloqueia nele.
    static ESPERAR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Teste: finge banco ocupado na leitura sem espera, para provar que nenhuma escrita sela Space incerto.
    #[cfg(test)]
    pub(crate) static SIMULAR_OCUPADO: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Lê os bots sem esperar o banco: leitor que já está dentro de uma transação de escrita (ex.: o início do login) não
/// pode travar aqui. Tenta poucas vezes (contenção de outra thread passa logo); `None` = banco ocupado.
fn bots_sem_esperar(store: &Store) -> Result<Option<Vec<BotConfig>>> {
    #[cfg(test)]
    if SIMULAR_OCUPADO.with(std::cell::Cell::get) {
        return Ok(None);
    }
    let mut guard = None;
    for tentativa in 0..5 {
        match store.db.try_lock() {
            Ok(g) => guard = Some(g),
            Err(std::sync::TryLockError::Poisoned(erro)) => guard = Some(erro.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) if tentativa < 4 => {
                std::thread::sleep(std::time::Duration::from_millis(10));
                continue;
            }
            Err(std::sync::TryLockError::WouldBlock) => {}
        }
        break;
    }
    let Some(guard) = guard else { return Ok(None) };
    let ids: Vec<String> = {
        let mut st = guard.prepare("SELECT id FROM bots WHERE deleted=0")?;
        st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    let mut bots = Vec::new();
    for id in ids {
        if let Some(row) = Store::bot_on(&guard, &id)?.filter(|r| !r.deleted) {
            bots.push(row.config);
        }
    }
    Ok(Some(bots))
}

/// Dá Space às contas antigas, na memória, com a regra acima (determinística pelo estado atual dos bots). Antes da
/// marca, guarda o texto do aviso se alguém perdeu a conta. Sempre `false`: quem regrava o cofre é `run`.
///
/// Se o banco está ocupado (o chamador já está numa transação), não espera **e não chuta**: o Space da conta antiga
/// continua VAZIO ("incerto"). Conta de Space vazio não casa com nenhum bot, então a execução e o vínculo a recusam
/// (fail-closed), e qualquer escrita do cofre fora do `run` sela o vazio, nunca um destino adivinhado. Só o `run`, que
/// espera o banco, sela o destino calculado e grava o aviso.
pub(crate) fn normalizar(store: &Store, todas: &mut [Connector]) -> Result<bool> {
    if !todas.iter().any(|c| c.space.is_empty()) {
        return Ok(false);
    }
    let bots = if ESPERAR.with(std::cell::Cell::get) { Some(bots_vivos(store)?) } else { bots_sem_esperar(store)? };
    let Some(bots) = bots else { return Ok(false) };
    let plano = planejar(todas, &bots);
    for conta in todas.iter_mut().filter(|c| c.space.is_empty()) {
        conta.space = plano.destinos.get(&conta.id).cloned().unwrap_or_else(|| PERSONAL.to_owned());
    }
    if (!plano.perdas.is_empty() || plano.sem_uso > 0) && store.kv_read(MARCA)?.is_none() {
        store.kv_set(PENDENTE, &texto_do_aviso(&plano))?;
    }
    // Leitura nunca regrava o cofre (mudaria o texto guardado debaixo de quem já o capturou): só `run` grava.
    Ok(false)
}

/// Há conta antiga no cofre? Só lê; não normaliza nem grava.
fn ha_conta_antiga(store: &Store) -> Result<bool> {
    let _guard = store.connector_lock.locked();
    let (raw, _) = super::vault::read_unmigrated(store, "connectors")?;
    let contas: Vec<Connector> = raw.map_or_else(|| Ok(Vec::new()), |s| serde_json::from_str(&s))?;
    Ok(contas.iter().any(|c| c.space.is_empty()))
}

/// Quem recebe o aviso: o primeiro bot que não é grupo, preferindo o Space pessoal.
fn destinatario(store: &Store) -> Result<Option<String>> {
    let mut bots = bots_vivos(store)?;
    bots.retain(|b| !b.is_group());
    bots.sort_by(|a, b| (a.space_id != PERSONAL, &a.id).cmp(&(b.space_id != PERSONAL, &b.id)));
    Ok(bots.into_iter().next().map(|b| b.id))
}

/// Roda uma vez por instalação, com o cofre aberto (sem a chave, tenta de novo na próxima abertura). Chamada na
/// partida e logo depois de abrir o cofre.
pub(crate) fn run(hub: &Hub) -> Result<()> {
    let store = &hub.store;
    if store.kv_read(MARCA)?.is_some() || store.secret_key.locked().is_none() {
        return Ok(());
    }
    if ha_conta_antiga(store)? {
        // Normaliza (e guarda o texto do aviso) e grava o cofre com o Space de cada conta.
        ESPERAR.with(|e| e.set(true));
        let regravado = super::update_connectors(store, |_| Ok(()));
        ESPERAR.with(|e| e.set(false));
        regravado?;
    }
    if let Some(texto) = store.kv_read(PENDENTE)? {
        if let Some(bot) = destinatario(store)? {
            hub.add_entry_once(
                AVISO_ID,
                &Lane::main(&bot),
                EntryKind::Notice,
                store.max_turn(&bot),
                &json!({"text": texto, "style": "info"}),
            )?;
        }
        store.kv_delete(PENDENTE)?;
    }
    store.kv_set(MARCA, "1")?;
    Ok(())
}
