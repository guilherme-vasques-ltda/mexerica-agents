//! RPC do dono (D371): Local ou Device com Control e autoridade original, capturada antes da trava. `spaceId`
//! seleciona o contexto das regras. Bots, MCP, rotinas e webhooks não têm caminho.
use super::{limpa, regra, versao};
use crate::{api::devices::Caller, fork::spaces::PERSONAL, hub::Hub};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{fmt::Write as _, sync::Arc};

pub(crate) const METHODS: &[&str] = &["donnaDelegationRules", "donnaDelegationRulesSave"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Read {
    space_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Save {
    pub space_id: Option<String>,
    /// `null`: regra geral; senão, o id de uma área do quadro de Equipes do Space.
    pub team_id: Option<String>,
    pub text: String,
    pub expected_version: i64,
    pub client_request_id: String,
}

fn parse<T: serde::de::DeserializeOwned>(body: Value) -> Result<T> {
    serde_json::from_value(body).map_err(|_| anyhow!("invalid_field: pedido de regras de delegação inválido."))
}

fn space(id: Option<&str>) -> &str {
    id.unwrap_or(PERSONAL)
}

/// Só as áreas que existem no quadro agora; regra de área apagada não aparece.
pub(super) fn snapshot(c: &Connection, space: &str) -> Result<Value> {
    let general = regra(c, space, "")?.unwrap_or_default();
    let mut teams = Map::new();
    let mut st = c.prepare(
        "SELECT r.team_id,r.body FROM donna_delegation_rules r JOIN donna_team_nodes n
           ON n.space_id=r.space_id AND n.id=r.team_id WHERE r.space_id=?1 ORDER BY r.team_id",
    )?;
    for row in st.query_map([space], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (id, body) = row?;
        teams.insert(id, body.into());
    }
    Ok(json!({"spaceId":space,"version":versao(c, space)?,"limit":super::LIMITE,"general":general,"teams":teams}))
}

pub(super) fn save(c: &Connection, r: &Save) -> Result<(Value, bool)> {
    let space = space(r.space_id.as_deref());
    crate::fork::spaces::require_space(c, space)?;
    uuid::Uuid::parse_str(&r.client_request_id).map_err(|_| anyhow!("invalid_field: clientRequestId inválido."))?;
    ensure!(r.expected_version >= 0, "invalid_field: versão inválida.");
    let text = limpa(&r.text)?;
    let team = r.team_id.as_deref().unwrap_or("");
    let digest = Sha256::digest(serde_json::to_vec(&json!([space, team, text, r.expected_version]))?).iter().fold(
        String::new(),
        |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        },
    );
    // `c` já é a transação de `Autoridade::with_db`: commit e reconferência ficam com ela.
    let tx = c;
    let receipt: Option<(String, i64)> = tx
        .query_row(
            "SELECT digest,version FROM donna_delegation_receipts WHERE space_id=?1 AND client_request_id=?2",
            params![space, r.client_request_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((before, version)) = receipt {
        ensure!(before == digest, "request_conflict: identificador já usado para outra alteração.");
        return Ok((
            json!({"spaceId":space,"snapshot":snapshot(tx, space)?,"receipt":{"clientRequestId":r.client_request_id,"version":version}}),
            false,
        ));
    }
    let current = versao(tx, space)?;
    ensure!(current == r.expected_version, "delegation_conflict: as regras mudaram; releia antes de salvar.");
    if !team.is_empty() {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM donna_team_nodes WHERE space_id=?1 AND id=?2)",
            params![space, team],
            |row| row.get(0),
        )?;
        ensure!(exists, "delegation_team: esta equipe não existe mais no quadro; releia Equipes.");
    }
    let changed = regra(tx, space, team)?.unwrap_or_default() != text;
    let mut version = current;
    if changed {
        if text.is_empty() {
            tx.execute("DELETE FROM donna_delegation_rules WHERE space_id=?1 AND team_id=?2", params![space, team])?;
        } else {
            tx.execute(
                "INSERT INTO donna_delegation_rules(space_id,team_id,body) VALUES(?1,?2,?3)
                 ON CONFLICT(space_id,team_id) DO UPDATE SET body=excluded.body",
                params![space, team, text],
            )?;
        }
        version += 1;
        tx.execute(
            "INSERT INTO donna_delegation_versions(space_id,version) VALUES(?1,?2)
             ON CONFLICT(space_id) DO UPDATE SET version=excluded.version",
            params![space, version],
        )?;
    }
    tx.execute(
        "INSERT INTO donna_delegation_receipts(space_id,client_request_id,digest,version) VALUES(?1,?2,?3,?4)",
        params![space, r.client_request_id, digest, version],
    )?;
    let answer = json!({"spaceId":space,"snapshot":snapshot(tx, space)?,"receipt":{"clientRequestId":r.client_request_id,"version":version}});
    Ok((answer, changed))
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    ensure!(METHODS.contains(&method), "invalid_field: operação de regras de delegação inválida.");
    crate::fork::bot_editor::authorization::require_owner(caller)?;
    // Segurança C1: a autoridade original é capturada antes de qualquer espera e reconferida na transação.
    let autoridade = crate::fork::autoridade::Autoridade::capturar(hub, caller)?;
    ensure!(serde_json::to_vec(&body)?.len() <= 65_536, "delegation_limit: pedido grande demais.");
    // A trava do quadro: a área conferida não some antes do commit.
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let save = method == "donnaDelegationRulesSave";
    let hub = hub.clone();
    tokio::task::spawn_blocking(move || -> Result<Value> {
        let _guard = guard;
        let (answer, changed) = autoridade.with_db(&hub, None, |tx| {
            if save {
                super::api::save(tx, &parse(body)?)
            } else {
                let r: Read = parse(body)?;
                let space = space(r.space_id.as_deref());
                crate::fork::spaces::require_space(tx, space)?;
                Ok((snapshot(tx, space)?, false))
            }
        })?;
        let space = answer["spaceId"].as_str().ok_or_else(|| anyhow!("space_not_found"))?;
        hub.store.require_space(space)?;
        if changed {
            let _ = hub.events.send(
                json!({"type":"delegationRules","spaceId":answer["spaceId"],"version":answer["receipt"]["version"]}),
            );
        }
        Ok(answer)
    })
    .await?
}
