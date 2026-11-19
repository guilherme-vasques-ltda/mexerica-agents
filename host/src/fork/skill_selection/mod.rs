//! c1/D288: a seleção canônica continua em `bots.config.skills`. Este produtor só
//! acrescenta a condição opaca `donna_skills_version`, a intenção tipada de uma célula
//! no MESMO `updateBot` e a recusa de caminhos antigos que trocariam a lista sem condição.
//! Não é catálogo, autoridade, grant ou executor: a autoridade é a original do transporte
//! (`fork::autoridade`) e a instalação é conferida pelo catálogo existente.
mod schema;

pub(crate) use schema::migrate;

use crate::{
    LockExt,
    api::devices::Caller,
    fork::autoridade::Autoridade,
    hub::Hub,
    store::{BotConfig, next_rev},
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) const UNSUPPORTED: &str = "skill_selection_unsupported";
pub(crate) const CONFLICT: &str = "skill_selection_conflict";

/// O token é opaco e não é credencial nem campo editável do perfil.
pub(crate) fn valid_version(value: Option<String>) -> Option<String> {
    value.filter(|value| value.len() == 32 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

/// Condição de uma célula, fixada no gesto do cliente.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cell {
    protocol: u8,
    expected_version: String,
    skill_id: String,
    enabled: bool,
    expected_installation_id: String,
}

/// Pedido protegido já capturado: autoridade ORIGINAL e condição antes de qualquer fila/await.
pub(crate) struct Selection {
    bot: String,
    cell: Cell,
    autoridade: Autoridade,
}

impl Selection {
    /// `None` quando o `updateBot` não traz `skillSelection`; o caminho antigo segue sua própria recusa.
    pub(crate) fn capture(hub: &Hub, caller: &Caller, body: &Value) -> Result<Option<Self>> {
        let Some(raw) = body.get("skillSelection") else { return Ok(None) };
        let fields = body.as_object().ok_or_else(|| anyhow!(UNSUPPORTED))?;
        // Uma célula por pedido: nenhum outro campo do perfil viaja junto.
        ensure!(fields.len() == 2, UNSUPPORTED);
        let bot = body["id"].as_str().filter(|id| !id.is_empty()).ok_or_else(|| anyhow!(UNSUPPORTED))?.to_owned();
        let cell: Cell = serde_json::from_value(raw.clone()).map_err(|_| anyhow!(UNSUPPORTED))?;
        ensure!(
            cell.protocol == 1
                && valid_version(Some(cell.expected_version.clone())).is_some()
                && !cell.skill_id.is_empty(),
            UNSUPPORTED
        );
        let autoridade = Autoridade::capturar(hub, caller)?;
        Ok(Some(Self { bot, cell, autoridade }))
    }

    /// Chamado com `team_lock` já obtido. Ordem: catálogo (MUTATION → filesystem) →
    /// `memory_boundary` → `emit_lock` → dono Cloud → DB; o callback só usa a Connection.
    pub(crate) fn commit(&self, hub: &Hub) -> Result<Value> {
        let expected = [(self.cell.skill_id.clone(), self.cell.expected_installation_id.clone())];
        let changed =
            crate::fork::skill_catalog::with_installations(&hub.store, &expected, self.cell.enabled, |vista| {
                let _boundary = hub.store.memory_boundary.locked();
                let _emit = hub.emit_lock.locked();
                self.autoridade.with_db(hub, None, |c| {
                    vista.conferir(c)?;
                    apply(c, &self.bot, &self.cell)
                })
            })?;
        let row = hub.store.bot(&self.bot)?.filter(|r| !r.deleted).ok_or_else(|| anyhow!("unknown bot"))?;
        if changed {
            if !row.config.is_group()
                && let Err(error) =
                    hub.send_cmd(&self.bot, crate::agent::bot::Cmd::Reconfigure(Box::new(row.config.clone())))
            {
                tracing::warn!(%error, bot = %self.bot, "seleção de skill salva; reconfiguração do ator pendente");
            }
            hub.emit_bot(&self.bot);
        }
        let row = hub.store.bot(&self.bot)?.ok_or_else(|| anyhow!("unknown bot"))?;
        Ok(hub.bot_json(&row))
    }
}

/// Lista canônica de um campo-array do perfil; entradas inválidas são recusadas
/// (CONFLICT), nunca descartadas em silêncio. Compartilhada pela seleção de uma
/// célula, pela criação e pelo lote de extensões (MX-264).
pub(crate) fn lista_estrita(valor: &Value) -> Result<Vec<String>> {
    match valor {
        Value::Null => Ok(Vec::new()),
        v => serde_json::from_value(v.clone()).map_err(|_| anyhow!(CONFLICT)),
    }
}

/// Grava uma célula se a versão observada no gesto ainda é a atual. Sem retentativa.
fn apply(c: &Connection, bot: &str, cell: &Cell) -> Result<bool> {
    let row: Option<(String, bool, Option<String>)> = c
        .query_row("SELECT config, deleted, donna_skills_version FROM bots WHERE id=?1", [bot], |r| {
            Ok((r.get(0)?, r.get::<_, i64>(1)? != 0, r.get(2).ok().flatten()))
        })
        .optional()?;
    let (config, deleted, version) = row.ok_or_else(|| anyhow!("unknown bot"))?;
    ensure!(!deleted, "unknown bot");
    ensure!(valid_version(version).as_deref() == Some(cell.expected_version.as_str()), CONFLICT);
    let profile: Value = serde_json::from_str(&config).map_err(|_| anyhow!(CONFLICT))?;
    ensure!(
        profile["spaceId"].as_str().is_none_or(|space| space == crate::fork::spaces::PERSONAL),
        "skills_personal_only"
    );
    let mut skills = lista_estrita(&profile["skills"])?;
    if skills.contains(&cell.skill_id) == cell.enabled {
        return Ok(false);
    }
    if cell.enabled {
        skills.push(cell.skill_id.clone());
    } else {
        skills.retain(|id| *id != cell.skill_id);
    }
    // O trigger rotaciona a versão na mesma transação. D368: intenção do dono pelo app.
    let rev = next_rev(c)?;
    crate::fork::donna_fixa::como_dono(c, |c| {
        Ok(c.execute(
            "UPDATE bots SET config=json_set(config,'$.skills',json(?1)), rev=?2 WHERE id=?3 AND deleted=0",
            params![serde_json::to_string(&skills)?, rev, bot],
        )?)
    })?;
    Ok(true)
}

/// Perfil completo antigo: só a lista idêntica à atual passa; mudar exige a intenção tipada.
pub(crate) fn refuse_unprotected(patch: &Value, current: &[String]) -> Result<()> {
    ensure!(patch.get("skillSelection").is_none(), UNSUPPORTED);
    if let Some(asked) = patch.get("skills") {
        let asked: Vec<String> = serde_json::from_value(asked.clone()).map_err(|_| anyhow!(UNSUPPORTED))?;
        ensure!(asked == current, UNSUPPORTED);
    }
    Ok(())
}

/// Escritores de perfil completo (ordem, grupos, gestão, avatar…) nunca restauram a
/// seleção de um snapshot: a lista gravada é a atual da própria Connection.
pub(crate) fn preserve(c: &Connection, cfg: &mut BotConfig) -> Result<()> {
    let current: Option<Option<String>> = c
        .query_row(
            "SELECT CASE WHEN json_valid(config) THEN json_extract(config,'$.skills') END FROM bots WHERE id=?1 AND deleted=0",
            [&cfg.id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(current) = current {
        cfg.skills = match current {
            Some(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            None => Vec::new(),
        };
    }
    Ok(())
}

/// Criação humana: seleção positiva só com a instalação vista no formulário.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Creation {
    protocol: u8,
    installations: BTreeMap<String, String>,
}

pub(crate) struct CreationSelection {
    expected: Vec<(String, String)>,
    autoridade: Autoridade,
}

impl CreationSelection {
    /// MX-264/C: seleção montada pelo padrão "novos bots já vêm com…" com a
    /// autoridade já capturada pelo caminho do dono; a gravação é a mesma da
    /// seleção explícita (`save`), nunca uma porta paralela.
    pub(crate) fn com_autoridade(expected: Vec<(String, String)>, autoridade: Autoridade) -> Self {
        Self { expected, autoridade }
    }

    /// `None` para lista vazia; positiva sem `skillSelection` exato é recusada.
    pub(crate) fn capture(hub: &Hub, caller: &Caller, body: &mut Value) -> Result<Option<Self>> {
        let raw = body.as_object_mut().and_then(|b| b.remove("skillSelection"));
        let skills: Vec<String> = match body.get("skills") {
            None | Some(Value::Null) => Vec::new(),
            Some(list) => serde_json::from_value(list.clone()).map_err(|_| anyhow!(UNSUPPORTED))?,
        };
        if skills.is_empty() {
            ensure!(raw.is_none(), UNSUPPORTED);
            return Ok(None);
        }
        ensure!(
            body["spaceId"].as_str().is_none_or(|space| space == crate::fork::spaces::PERSONAL),
            "skills_personal_only"
        );
        let creation: Creation =
            serde_json::from_value(raw.ok_or_else(|| anyhow!(UNSUPPORTED))?).map_err(|_| anyhow!(UNSUPPORTED))?;
        let mut unique = skills.clone();
        unique.sort();
        unique.dedup();
        ensure!(
            creation.protocol == 1 && unique.len() == skills.len() && creation.installations.keys().eq(unique.iter()),
            UNSUPPORTED
        );
        let autoridade = Autoridade::capturar(hub, caller)?;
        Ok(Some(Self { expected: creation.installations.into_iter().collect(), autoridade }))
    }

    /// Grava o bot novo sob os locks do catálogo e a autoridade original na mesma Connection.
    pub(crate) fn save(
        &self,
        hub: &Hub,
        cfg: &BotConfig,
        contas: Option<&crate::fork::engine_setup::access::Authority>,
    ) -> Result<i64> {
        let revision = crate::fork::skill_catalog::with_installations(&hub.store, &self.expected, true, |vista| {
            self.autoridade.with_db(hub, contas, |c| {
                vista.conferir(c)?;
                crate::fork::donna_fixa::como_dono(c, |c| crate::store::bots::save_bot(c, cfg))
            })
        })?;
        hub.store.guard_cleanup()?;
        Ok(revision)
    }
}

/// Herança legada de `manage.create_bot`: a condição do criador vista ANTES da aprovação,
/// inclusive lista vazia. Conferida na transação da criação; nunca recapturada.
pub(crate) fn creator_version(hub: &Hub, creator: &BotConfig) -> Result<String> {
    let row = hub.store.bot(&creator.id)?.filter(|r| !r.deleted).ok_or_else(|| anyhow!(CONFLICT))?;
    ensure!(row.config.skills == creator.skills, CONFLICT);
    valid_version(row.skills_version).ok_or_else(|| anyhow!(CONFLICT))
}

pub(crate) fn confirm_creator(c: &Connection, creator: &str, expected: &str) -> Result<()> {
    let current: Option<Option<String>> = c
        .query_row("SELECT donna_skills_version FROM bots WHERE id=?1 AND deleted=0", [creator], |r| {
            Ok(r.get(0).ok().flatten())
        })
        .optional()?;
    ensure!(valid_version(current.flatten()).as_deref() == Some(expected), CONFLICT);
    Ok(())
}

/// Fixtures: grava a lista diretamente, como o trigger veria qualquer escritor.
#[cfg(test)]
pub(crate) fn set_for_test(store: &crate::store::Store, bot: &str, skills: &[&str]) {
    // D368: fixture com a autoridade do dono pelo app (a principal só muda skills por ele).
    crate::fork::donna_fixa::como_dono(&store.db.locked(), |c| {
        Ok(c.execute(
            "UPDATE bots SET config=json_set(config,'$.skills',json(?1)) WHERE id=?2",
            params![serde_json::to_string(skills).unwrap(), bot],
        )?)
    })
    .unwrap();
}

#[cfg(test)]
mod tests;

/// Observação somente de leitura para o preparo interno do perfil; não concede autoridade.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Observed {
    pub(crate) skills: Vec<String>,
    version: String,
}
impl Observed {
    pub(crate) fn read(c: &Connection, bot: &str) -> Result<Self> {
        let (raw, version): (String, Option<String>) =
            c.query_row("SELECT config,donna_skills_version FROM bots WHERE id=?1 AND deleted=0", [bot], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        let profile: Value = serde_json::from_str(&raw)?;
        ensure!(profile["spaceId"].as_str().is_none_or(|s| s == crate::fork::spaces::PERSONAL), CONFLICT);
        let skills = match profile.get("skills") {
            None | Some(Value::Null) => Vec::new(),
            Some(v) => serde_json::from_value(v.clone())?,
        };
        Ok(Self { skills, version: valid_version(version).ok_or_else(|| anyhow!(CONFLICT))? })
    }
    pub(crate) fn check(&self, c: &Connection, bot: &str) -> Result<()> {
        ensure!(Self::read(c, bot)? == *self, CONFLICT);
        Ok(())
    }
}
