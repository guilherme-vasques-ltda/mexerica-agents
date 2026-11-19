//! Persistência do MESMO catálogo: v1 compatível, metadado v2 e barreira de escritor legado.
use super::{Receipt, STATE, State, installation, put};
use crate::{LockExt, market::Skill, store::Store};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const META: &str = "donna.passiveSkills.v2";
const GUARDS: [(&str, &str); 3] = [
    (
        "d166_skill_v2_insert",
        "CREATE TRIGGER d166_skill_v2_insert AFTER INSERT ON kv WHEN NEW.k IN ('skills','donna.passiveSkills.v1') BEGIN UPDATE kv SET v=json_set(v,'$.legacyWriteObserved',json('true')) WHERE k='donna.passiveSkills.v2'; END",
    ),
    (
        "d166_skill_v2_update",
        "CREATE TRIGGER d166_skill_v2_update AFTER UPDATE ON kv WHEN OLD.k IN ('skills','donna.passiveSkills.v1') OR NEW.k IN ('skills','donna.passiveSkills.v1') BEGIN UPDATE kv SET v=json_set(v,'$.legacyWriteObserved',json('true')) WHERE k='donna.passiveSkills.v2'; END",
    ),
    (
        "d166_skill_v2_delete",
        "CREATE TRIGGER d166_skill_v2_delete AFTER DELETE ON kv WHEN OLD.k IN ('skills','donna.passiveSkills.v1') BEGIN UPDATE kv SET v=json_set(v,'$.legacyWriteObserved',json('true')) WHERE k='donna.passiveSkills.v2'; END",
    ),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Snapshot {
    skills: Option<String>,
    legacy: Option<String>,
    metadata: Option<String>,
}
impl Snapshot {
    fn read(db: &Connection) -> Result<Self> {
        let get = |key| -> Result<Option<String>> {
            Ok(db.query_row("SELECT v FROM kv WHERE k=?1", [key], |r| r.get(0)).optional()?)
        };
        Ok(Self { skills: get("skills")?, legacy: get(STATE)?, metadata: get(META)? })
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Legacy {
    seed_done: bool,
    receipts: BTreeMap<String, Receipt>,
    installed: BTreeMap<String, String>,
}
impl Legacy {
    fn from_state(state: &State) -> Self {
        Self { seed_done: state.seed_done, receipts: state.receipts.clone(), installed: state.installed.clone() }
    }
    fn into_state(self) -> State {
        State { seed_done: self.seed_done, receipts: self.receipts, installed: self.installed, ..State::default() }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Metadata {
    schema: u8,
    installation_namespace: Option<String>,
    installation_sequence: u64,
    installations: BTreeMap<String, installation::Version>,
    legacy_write_observed: bool,
}
impl Metadata {
    fn from_state(state: &State) -> Self {
        Self {
            schema: 2,
            installation_namespace: state.installation_namespace.clone(),
            installation_sequence: state.installation_sequence,
            installations: state.installations.clone(),
            legacy_write_observed: false,
        }
    }
    fn apply(self, state: &mut State) {
        state.installation_namespace = self.installation_namespace;
        state.installation_sequence = self.installation_sequence;
        state.installations = self.installations;
    }
}

fn decode<T: for<'a> Deserialize<'a>>(raw: &str) -> Result<T> {
    serde_json::from_str(raw).map_err(|_| anyhow!(installation::UNAVAILABLE))
}

fn legacy(raw: Option<&str>) -> Result<(State, bool)> {
    let Some(raw) = raw else {
        return Ok((State::default(), false));
    };
    let value: serde_json::Value = decode(raw)?;
    let fields = value.as_object().ok_or_else(|| anyhow!(installation::UNAVAILABLE))?;
    let old = ["seedDone", "receipts", "installed"];
    let extended =
        ["seedDone", "receipts", "installed", "installationNamespace", "installationSequence", "installations"];
    let exact = |keys: &[&str]| fields.len() == keys.len() && keys.iter().all(|k| fields.contains_key(*k));
    if exact(&old) {
        Ok((decode::<Legacy>(raw)?.into_state(), false))
    } else {
        ensure!(exact(&extended), installation::UNAVAILABLE);
        Ok((decode(raw)?, true))
    }
}

/// Nenhuma definição desconhecida é substituída. Criação é só dos nomes próprios ausentes.
fn guards(tx: &Connection, create_missing: bool) -> Result<usize> {
    let mut missing = Vec::new();
    for (name, expected) in GUARDS {
        let actual: Option<String> = tx
            .query_row("SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?1", [name], |r| r.get(0))
            .optional()?;
        if let Some(actual) = actual {
            ensure!(actual.trim().trim_end_matches(';') == expected, installation::UNAVAILABLE);
        } else {
            missing.push(expected);
        }
    }
    if create_missing {
        for sql in &missing {
            tx.execute_batch(sql)?;
        }
    }
    Ok(missing.len())
}

fn write(db: &Connection, key: &str, value: &impl Serialize) -> Result<String> {
    put(db, key, value)?;
    Ok(serde_json::to_string(value)?)
}

/// Só sob MUTATION + filesystem; Callback de Caller/contexto já passou antes desta função.
pub(super) fn load_initialized(store: &Store) -> Result<(Vec<Skill>, State)> {
    // D368/MX-264: a inicialização pode gravar (travas de escritor legado,
    // migração, reemissão). Num pedido do dono, roda dentro da autoridade
    // ORIGINAL reconferida na mesma transação — mesmo mecanismo de `persist`,
    // nunca numa transação interna que ignoraria a revogação. Aparelho sem a
    // autoridade do dono (Recusada) nunca migra nem reemite: usa a leitura pura
    // `current`, que falha fechado quando o catálogo pede escrita. Local e
    // tarefas internas seguem a transação comum de sempre.
    match crate::fork::donna_fixa::pedido::origem_efetiva() {
        Some(crate::fork::donna_fixa::pedido::Origem::Dono(..)) => {
            crate::fork::donna_fixa::pedido::gravar(store, load_initialized_on)
        }
        Some(crate::fork::donna_fixa::pedido::Origem::Recusada) => current(store),
        _ => {
            let mut db = store.db.locked();
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let out = load_initialized_on(&tx)?;
            tx.commit()?;
            Ok(out)
        }
    }
}

fn load_initialized_on(db: &Connection) -> Result<(Vec<Skill>, State)> {
    let mut observed = Snapshot::read(db)?;
    let all: Vec<Skill> = observed.skills.as_deref().map(decode).transpose()?.unwrap_or_default();
    let (mut state, extended) = legacy(observed.legacy.as_deref())?;
    let ids: BTreeSet<_> = all.iter().map(|skill| skill.id.as_str()).collect();
    ensure!(ids.len() == all.len(), installation::UNAVAILABLE);
    ensure!(
        state.installed.iter().all(|(id, digest)| ids.contains(id.as_str()) && state.receipts.contains_key(digest)),
        installation::UNAVAILABLE
    );
    let incoming_counter = state.installation_sequence;
    if extended {
        installation::validate_metadata(&state)?;
    }
    let metadata: Option<Metadata> = observed.metadata.as_deref().map(decode).transpose()?;
    let has_metadata = metadata.is_some();
    let dirty = metadata.as_ref().is_some_and(|meta| meta.legacy_write_observed);
    if let Some(meta) = metadata {
        ensure!(meta.schema == 2, installation::UNAVAILABLE);
        ensure!(!extended || dirty, installation::UNAVAILABLE);
        meta.apply(&mut state);
        installation::validate_metadata(&state)?;
        // Perda do v1 com autoridade anterior não é um novo seed nem uma migração vazia.
        ensure!(
            observed.legacy.is_some() || (all.is_empty() && state.installation_sequence == 0 && !dirty),
            installation::UNAVAILABLE
        );
    }
    let missing = guards(db, false)?;
    ensure!(has_metadata || missing == GUARDS.len(), installation::UNAVAILABLE);
    let gap = missing != 0;
    guards(db, true)?;
    let reissue = has_metadata && (dirty || gap);
    let mut changed = !has_metadata;
    if reissue {
        state.installation_sequence = state.installation_sequence.max(incoming_counter);
        state.installation_namespace = Some(uuid::Uuid::new_v4().to_string());
        // Conservar prova anterior por registro; `issue` nunca relê o corpo manual.
        // IDs removidos pelo escritor antigo são aposentados normalmente.
        state.installations.retain(|id, _| ids.contains(id.as_str()));
        changed = true;
    }
    if has_metadata && !reissue {
        ensure!(
            state.installations.len() == all.len() && ids.iter().all(|id| state.installations.contains_key(*id)),
            installation::UNAVAILABLE
        );
    } else {
        ensure!(state.installations.keys().all(|id| ids.contains(id.as_str())), installation::UNAVAILABLE);
        for skill in &all {
            if reissue || !state.installations.contains_key(&skill.id) {
                installation::issue(skill, &mut state)?;
                changed = true;
            }
        }
    }
    installation::validate_metadata(&state)?;
    if extended || observed.legacy.is_none() {
        observed.legacy = Some(write(db, STATE, &Legacy::from_state(&state))?);
    }
    if changed || extended {
        observed.metadata = Some(write(db, META, &Metadata::from_state(&state))?);
    }
    state.observed = Some(observed);
    Ok((all, state))
}

/// D306/R1: leitura do caminho do motor, sem lock de arquivo, migração ou escrita.
/// O que pediria migrar, reemitir ou reconciliar falha fechado até uma leitura sob os locks.
pub(super) fn current(store: &Store) -> Result<(Vec<Skill>, State)> {
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    current_on(&tx)
}

/// d2a: leitura sem migração na Connection protegida pela autoridade original.
pub(super) fn current_on(db: &Connection) -> Result<(Vec<Skill>, State)> {
    let observed = Snapshot::read(db)?;
    let all: Vec<Skill> = observed.skills.as_deref().map(decode).transpose()?.unwrap_or_default();
    let (mut state, extended) = legacy(observed.legacy.as_deref())?;
    let meta: Metadata =
        observed.metadata.as_deref().map(decode).transpose()?.ok_or_else(|| anyhow!(installation::UNAVAILABLE))?;
    ensure!(
        meta.schema == 2 && !extended && !meta.legacy_write_observed && observed.legacy.is_some(),
        installation::UNAVAILABLE
    );
    ensure!(guards(db, false)? == 0, installation::UNAVAILABLE);
    meta.apply(&mut state);
    installation::validate_metadata(&state)?;
    let ids: BTreeSet<_> = all.iter().map(|skill| skill.id.as_str()).collect();
    ensure!(
        ids.len() == all.len()
            && state.installations.len() == all.len()
            && ids.iter().all(|id| state.installations.contains_key(*id))
            && state
                .installed
                .iter()
                .all(|(id, digest)| ids.contains(id.as_str()) && state.receipts.contains_key(digest)),
        installation::UNAVAILABLE
    );
    state.observed = Some(observed);
    Ok((all, state))
}

pub(super) fn persist(store: &Store, all: &[Skill], state: &State, removed: Option<&str>) -> Result<()> {
    // D368: num pedido do app pareado, grava sob a autoridade original reconferida na transação.
    crate::fork::donna_fixa::pedido::gravar(store, |tx| persist_on(tx, all, state, removed, false))
}

/// D368: a mesma gravação dentro da transação da autoridade ORIGINAL de quem pediu a remoção
/// (`Autoridade::with_db`, guarda Cloud → DB reconferida antes e depois). Só aqui a remoção também
/// tira a skill do Berga (campo só do dono).
pub(super) fn persist_as_owner(
    hub: &crate::hub::Hub,
    owner: &crate::fork::autoridade::Autoridade,
    all: &[Skill],
    state: &State,
    removed: Option<&str>,
) -> Result<()> {
    crate::fork::donna_fixa::pedido::antes_de_gravar(&hub.store);
    owner.with_db(hub, None, |tx| persist_on(tx, all, state, removed, true))
}

fn persist_on(tx: &Connection, all: &[Skill], state: &State, removed: Option<&str>, dono: bool) -> Result<()> {
    let expected = state.observed.as_ref().ok_or_else(|| anyhow!(installation::UNAVAILABLE))?;
    installation::validate_metadata(state)?;
    let ids: BTreeSet<_> = all.iter().map(|skill| skill.id.as_str()).collect();
    ensure!(
        ids.len() == all.len()
            && state.installations.len() == all.len()
            && ids.iter().all(|id| state.installations.contains_key(*id))
            && state
                .installed
                .iter()
                .all(|(id, digest)| ids.contains(id.as_str()) && state.receipts.contains_key(digest)),
        installation::UNAVAILABLE
    );
    ensure!(guards(tx, false)? == 0, installation::STALE);
    ensure!(Snapshot::read(tx)? == *expected, installation::STALE);
    // Até aqui nenhum put: cache velho nunca limpa marca tardia de outro escritor.
    put(tx, "skills", &all)?;
    put(tx, STATE, &Legacy::from_state(state))?;
    if let Some(id) = removed {
        let configs: Vec<(String, String)> = {
            let mut query = tx.prepare("SELECT id, config FROM bots WHERE deleted=0")?;
            query.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<rusqlite::Result<_>>()?
        };
        for (bot_id, config) in configs {
            let mut config: crate::store::BotConfig = serde_json::from_str(&config)?;
            if config.skills.iter().any(|skill| skill == id) {
                config.skills.retain(|skill| skill != id);
                let rev: i64 = tx.query_row("INSERT INTO kv(k,v) VALUES('rev','1') ON CONFLICT(k) DO UPDATE SET v=CAST(v AS INTEGER)+1 RETURNING CAST(v AS INTEGER)", [], |row| row.get(0))?;
                let write = |c: &Connection| -> Result<usize> {
                    Ok(c.execute(
                        "UPDATE bots SET config=?1,rev=?2 WHERE id=?3",
                        params![serde_json::to_string(&config)?, rev, bot_id],
                    )?)
                };
                if dono {
                    crate::fork::donna_fixa::como_dono(tx, write)?
                } else {
                    write(tx)?
                };
            }
        }
    }
    put(tx, META, &Metadata::from_state(state))?;
    Ok(())
}

/// c1: o catálogo não mudou desde a observação feita sob os locks.
pub(super) fn unchanged(db: &Connection, expected: &Snapshot) -> Result<()> {
    ensure!(Snapshot::read(db)? == *expected, installation::STALE);
    Ok(())
}

/// Inspeção sem migração só para asserções de fixtures; nunca é rota RPC.
#[cfg(test)]
pub(super) fn inspect(store: &Store) -> Result<(Vec<Skill>, State)> {
    let db = store.db.locked();
    let raw = Snapshot::read(&db)?;
    let all = raw.skills.as_deref().map(decode).transpose()?.unwrap_or_default();
    let (mut state, _) = legacy(raw.legacy.as_deref())?;
    if let Some(metadata) = raw.metadata.as_deref() {
        let meta: Metadata = decode(metadata)?;
        meta.apply(&mut state);
    }
    Ok((all, state))
}
