//! D543: escrita indivisível, sem herança pessoal; workspace novo e especulativo.
use super::{Appearance, PERSONAL, Snapshot, Space, Tile, assets, label, store};
use crate::{
    LockExt,
    fork::{engine_accounts::managed, engine_setup::access, native_catalog::workspace::PreparedWorkspace},
    hub::Hub,
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fmt::Write as _, sync::Arc};

// hubId é conferido e retirado pelo dispatch. spaceId é só o contexto de origem,
// sempre Pessoal; nunca é o ID do Space que será emitido pelo Host.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    #[serde(default = "super::personal_id")]
    space_id: String,
    expected_revision: i64,
    client_nonce: String,
    name: String,
    tile: Tile,
    principal: Principal,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Principal {
    #[serde(default)]
    name: Option<String>,
    backend: String,
    engine_account_id: String,
    // `skip_serializing_if`: recibos gravados antes do campo existir precisam
    // reproduzir o mesmo digest no replay (o `None` não pode entrar no JSON).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    appearance: Option<Value>,
}

pub(super) fn migrate(db: &Connection) -> Result<()> {
    // O chamador spaces::store::migrate já mantém a Tx para DROP + CREATE.
    super::principal::migrate(db)?;
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_space_birth_receipts(
            hub_id TEXT NOT NULL, nonce TEXT NOT NULL, digest TEXT NOT NULL,
            space_id TEXT NOT NULL, bot_id TEXT NOT NULL, PRIMARY KEY(hub_id,nonce));
         CREATE TABLE IF NOT EXISTS donna_space_delete_spaces(space_id TEXT PRIMARY KEY);
         CREATE TABLE IF NOT EXISTS donna_space_delete_bots(bot_id TEXT PRIMARY KEY);
         DROP TRIGGER IF EXISTS donna_space_principal_delete;
         CREATE TRIGGER donna_space_principal_delete BEFORE DELETE ON bots
         WHEN EXISTS(SELECT 1 FROM kv,json_each(kv.v,'$.spaces') s
             WHERE kv.k='donna.spaces.v1' AND json_extract(s.value,'$.principalBotId')=OLD.id)
           AND OLD.id NOT IN (SELECT bot_id FROM donna_space_delete_bots)
         BEGIN SELECT RAISE(ABORT,'space_principal_immutable'); END;
         DROP TRIGGER IF EXISTS donna_space_principal_update;
         CREATE TRIGGER donna_space_principal_update BEFORE UPDATE OF id,config,deleted ON bots
         WHEN EXISTS(SELECT 1 FROM kv,json_each(kv.v,'$.spaces') s
             WHERE kv.k='donna.spaces.v1' AND json_extract(s.value,'$.principalBotId')=OLD.id)
           AND (NEW.id IS NOT OLD.id OR NOT json_valid(NEW.config)
             OR json_extract(NEW.config,'$.spaceId') IS NOT json_extract(OLD.config,'$.spaceId')
             OR COALESCE(json_extract(NEW.config,'$.kind'),'agent')<>'agent'
             OR (NEW.deleted<>0 AND OLD.id NOT IN (SELECT bot_id FROM donna_space_delete_bots)))
         BEGIN SELECT RAISE(ABORT,'space_principal_immutable'); END;
         DROP TRIGGER IF EXISTS donna_space_principal_reference;
         CREATE TRIGGER donna_space_principal_reference BEFORE UPDATE ON kv
         WHEN OLD.k='donna.spaces.v1' AND EXISTS(
             SELECT 1 FROM json_each(OLD.v,'$.spaces') old_space
             WHERE json_extract(old_space.value,'$.principalBotId') IS NOT NULL
               AND json_extract(old_space.value,'$.id') NOT IN (SELECT space_id FROM donna_space_delete_spaces)
               AND (NEW.k<>OLD.k OR NOT EXISTS(
                   SELECT 1 FROM json_each(NEW.v,'$.spaces') new_space
                   WHERE json_extract(new_space.value,'$.id')=json_extract(old_space.value,'$.id')
                     AND json_extract(new_space.value,'$.principalBotId')=json_extract(old_space.value,'$.principalBotId'))))
         BEGIN SELECT RAISE(ABORT,'space_principal_immutable'); END;
         CREATE TRIGGER IF NOT EXISTS donna_space_principal_snapshot_delete BEFORE DELETE ON kv
         WHEN OLD.k='donna.spaces.v1' AND EXISTS(
             SELECT 1 FROM json_each(OLD.v,'$.spaces') s
             WHERE json_extract(s.value,'$.principalBotId') IS NOT NULL)
         BEGIN SELECT RAISE(ABORT,'space_principal_immutable'); END;
         DELETE FROM donna_space_delete_bots;
         DELETE FROM donna_space_delete_spaces;",
    )?;
    Ok(())
}

/// A mesma Connection da autoridade/Tx, inclusive no replay: nenhuma guarda é renovada.
fn replay(db: &Connection, hub_id: &str, request: &Request, digest: &str) -> Result<Option<Snapshot>> {
    let receipt: Option<(String, String, String)> = db
        .query_row(
            "SELECT digest,space_id,bot_id FROM donna_space_birth_receipts WHERE hub_id=?1 AND nonce=?2",
            params![hub_id, request.client_nonce],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((saved_digest, space, bot)) = receipt else { return Ok(None) };
    ensure!(saved_digest == digest, "space_birth_nonce_conflict");
    let current = store::read(db)?;
    ensure!(
        current.spaces.iter().any(|s| s.id == space && s.principal_bot_id.as_deref() == Some(bot.as_str())),
        "space_birth_reference_unavailable"
    );
    // A resposta acompanha avanços de outros Spaces; nunca reproduz uma lista antiga.
    Ok(Some(current))
}

pub(crate) async fn call(hub: &Arc<Hub>, authority: &access::Authority, body: Value) -> Result<Value> {
    // A mesma autoridade de contas capturada pelo dispatch, antes de filas/awaits.
    let authority = authority.clone();
    let request: Request = serde_json::from_value(body)?;
    ensure!(request.space_id == PERSONAL, "space_birth_source_must_be_personal");
    ensure!(request.expected_revision >= 0, "invalid_space_revision");
    managed::request_id(&request.client_nonce)?;
    let digest = Sha256::digest(serde_json::to_vec(&request)?).iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    });
    let guard = hub.store.team_lock.clone().lock_owned().await;
    if let Some(snapshot) = authority.with_db(hub, |db| replay(db, &hub.host_id, &request, &digest))? {
        return Ok(serde_json::to_value(snapshot)?);
    }
    let name = label(&json!(request.name))?;
    super::principal::checked_name(request.principal.name.as_deref())?;
    let appearance = super::principal::checked_appearance(request.principal.appearance.as_ref())?;
    assets::tile(&request.tile)?;
    ensure!(!request.principal.engine_account_id.is_empty(), "engine_account_required");
    let space_id = uuid::Uuid::new_v4().to_string();
    // Allowlist própria. Não clona o bot pessoal, nem passa pelos defaults globais.
    let cfg = super::principal::config(
        &space_id,
        &request.principal.backend,
        &request.principal.engine_account_id,
        &appearance,
    )?;
    let hub = hub.clone();
    tokio::task::spawn_blocking(move || -> Result<Value> {
        let _guard = guard;
        authority.require_current(&hub)?;
        let mut workspace = PreparedWorkspace::default();
        let cfg = hub.prepare_new_bot_with_workspace(cfg, |cfg| workspace.prepare(cfg))?;
        let emit_guard = hub.emit_lock.locked();
        let (snapshot, created) = authority.with_db(&hub, |db| {
            if let Some(snapshot) = replay(db, &hub.host_id, &request, &digest)? {
                return Ok((snapshot, false));
            }
            let mut state = store::read(db)?;
            if state.revision != request.expected_revision {
                return Err(crate::remote::cloud::Conflict("spaces_revision_conflict").into());
            }
            // Cadastro deste Hub, relido na mesma Tx que cria todas as referências.
            let revision = crate::store::next_rev(db)?;
            state.spaces.push(Space {
                id: space_id,
                principal_bot_id: Some(cfg.id.clone()),
                kind: "space".into(),
                name,
                position: i64::try_from(state.spaces.len())?,
                revision,
                tile: Some(request.tile),
                appearance: Appearance::default(),
                appearance_configured: true,
                owner: None,
            });
            state.revision = revision;
            // O validador canônico vê o Space e seu vínculo dentro desta mesma Tx.
            super::principal::persist_on(db, &mut state, &cfg, &request.principal.engine_account_id)?;
            db.execute(
                "INSERT INTO donna_space_birth_receipts(hub_id,nonce,digest,space_id,bot_id)
                VALUES(?1,?2,?3,?4,?5)",
                params![hub.host_id, request.client_nonce, digest, cfg.space_id, cfg.id],
            )?;
            Ok((state, true))
        })?;
        if created {
            // A partir daqui o commit venceu. Nenhum erro de publicação limpa o workspace.
            workspace.keep();
            let _ = hub.events.send(json!({"type":"spaces","spaceId":PERSONAL,
                "rev":snapshot.revision,"snapshot":snapshot}));
        }
        drop(emit_guard);
        if created {
            hub.start_created_bot(&cfg);
        }
        authority.require_current(&hub)?;
        Ok(serde_json::to_value(snapshot)?)
    })
    .await?
}

#[cfg(test)]
mod receipt_compatibility {
    use super::*;

    #[test]
    fn d543_digest_sem_aparencia_preserva_serializer_anterior() {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct OldPrincipal {
            name: String,
            backend: String,
            engine_account_id: String,
        }
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct OldRequest {
            space_id: String,
            expected_revision: i64,
            client_nonce: String,
            name: String,
            tile: Tile,
            principal: OldPrincipal,
        }
        let request: Request = serde_json::from_value(json!({"expectedRevision":3,"clientNonce":"historico",
            "name":"Cliente","tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"CL"}},
            "principal":{"name":"Berga","backend":"codex","engineAccountId":"conta"}}))
        .unwrap();
        let old = OldRequest {
            space_id: request.space_id.clone(),
            expected_revision: request.expected_revision,
            client_nonce: request.client_nonce.clone(),
            name: request.name.clone(),
            tile: request.tile.clone(),
            principal: OldPrincipal {
                name: "Berga".into(),
                backend: "codex".into(),
                engine_account_id: "conta".into(),
            },
        };
        assert_eq!(
            serde_json::to_vec(&request).unwrap(),
            serde_json::to_vec(&old).unwrap(),
            "recibo histórico reconhece o mesmo corpo"
        );
    }
}
