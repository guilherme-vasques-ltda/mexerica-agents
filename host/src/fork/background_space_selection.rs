//! D246: autorização explícita para uma seleção existente, sem catálogo ou credencial paralelos.
use crate::{
    LockExt,
    api::devices::Caller,
    fork::{
        engine_accounts::{self, Registry},
        engine_setup::access::{self, Authority},
        memory_auto::queue::Context,
        modelos_funcao::{
            catalogo::{FUNCTIONS, FunctionId},
            config::{self, Binding, Document, Selection},
        },
    },
    hub::Hub,
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    sync::{Arc, atomic::Ordering},
};

const SPACES: &str = "donna.spaces.v1";
const AUDIENCE: &str = "owner_private";

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_background_space_selections(
        space TEXT PRIMARY KEY, revision INTEGER NOT NULL, selection TEXT, authority TEXT,
        updated_at INTEGER NOT NULL);",
    )?;
    Ok(())
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Issuer {
    Local,
    Owner { key: String, binding: String },
}
impl From<&Authority> for Issuer {
    fn from(authority: &Authority) -> Self {
        match authority {
            Authority::Local => Self::Local,
            Authority::Owner { key, binding, .. } => Self::Owner { key: key.clone(), binding: binding.clone() },
        }
    }
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reference {
    function_id: FunctionId,
    function_revision: u64,
    binding_digest: String,
    account_id: String,
    account_revision: u64,
    account_digest: String,
    space_revision: i64,
    audience: String,
}

/// Só este módulo constrói o snapshot; nenhum campo do payload vira autoridade.
pub(crate) struct Prepared {
    space: String,
    reference: Reference,
    revision: Option<i64>,
    issuer: Issuer,
    pub(crate) binding: Binding,
}

fn read<T: serde::de::DeserializeOwned>(db: &Connection, key: &str) -> Result<T> {
    let raw: String = db.query_row("SELECT v FROM kv WHERE k=?1", [key], |r| r.get(0))?;
    serde_json::from_str(&raw).map_err(|_| anyhow!("background_configuration_invalid"))
}
fn digest(value: &impl Serialize) -> Result<String> {
    let mut encoded = String::with_capacity(64);
    for byte in Sha256::digest(serde_json::to_vec(value)?) {
        write!(&mut encoded, "{byte:02x}")?;
    }
    Ok(encoded)
}
fn binding(document: &Document) -> Result<&Binding> {
    match document.functions.get(&FunctionId::MemoryConsolidation) {
        Some(Selection::Custom { binding } | Selection::Automatic { binding: Some(binding), .. }) => Ok(binding),
        _ => Err(anyhow!("background_selection_unavailable")),
    }
}
fn space_revision(db: &Connection, space: &str) -> Result<i64> {
    let spaces: crate::fork::spaces::Snapshot = read(db, SPACES)?;
    spaces.spaces.iter().find(|s| s.id == space).map(|s| s.revision).ok_or_else(|| anyhow!("space_not_found"))
}
fn current_reference(db: &Connection, space: &str) -> Result<(Reference, Binding)> {
    let document: Document = read(db, config::SLOT)?;
    ensure!(
        document.schema_version == 1
            && document.functions.len() == FUNCTIONS.len()
            && FUNCTIONS.iter().all(|id| document.functions.contains_key(id)),
        "background_configuration_invalid"
    );
    let selected = binding(&document)?.clone();
    let registry: Registry = read(db, engine_accounts::SLOT)?;
    let account = registry
        .accounts
        .iter()
        .find(|a| a.id == selected.account_id)
        .ok_or_else(|| anyhow!("background_account_missing"))?;
    ensure!(
        registry.accounts.iter().filter(|a| a.id == selected.account_id).count() == 1,
        "background_account_ambiguous"
    );
    ensure!(account.state == engine_accounts::State::Connected, "background_account_unavailable");
    ensure!(
        account.backend == engine_accounts::Backend::Claude
            && (account.subscription_verified()
                || (space == crate::fork::spaces::PERSONAL
                    && matches!(
                        document.functions.get(&FunctionId::MemoryConsolidation),
                        Some(Selection::Custom { .. })
                    )
                    && super::background_anthropic::slot(account).is_some())),
        "background_account_incompatible"
    );
    let model =
        account.models.iter().find(|m| m.id == selected.model_id).ok_or_else(|| anyhow!("background_model_missing"))?;
    ensure!(
        selected
            .model_options
            .iter()
            .all(|(key, value)| model.options.iter().any(|option| option.id == *key && option.values.contains(value))),
        "background_option_unsupported"
    );
    if let Some(metadata) = &account.managed {
        ensure!(metadata.enabled && account.catalog_valid(), "background_account_unverified");
    }
    // A observação muda verifiedAt sem trocar a conta. O digest não é atestado de login:
    // só detecta alterações do cadastro; o executor ainda deve provar a identidade oficial.
    let mut account_value = serde_json::to_value(account)?;
    account_value.as_object_mut().ok_or_else(|| anyhow!("background_account_invalid"))?.remove("verifiedAt");
    let reference = Reference {
        function_id: FunctionId::MemoryConsolidation,
        function_revision: document.revision,
        binding_digest: digest(&selected)?,
        account_id: selected.account_id.clone(),
        account_revision: account.managed.as_ref().map_or(1, |m| m.revision),
        account_digest: digest(&account_value)?,
        space_revision: space_revision(db, space)?,
        audience: AUDIENCE.into(),
    };
    Ok((reference, selected))
}

type StoredSelection = (i64, Option<String>, Option<String>);

fn row(db: &Connection, space: &str) -> Result<Option<StoredSelection>> {
    Ok(db
        .query_row(
            "SELECT revision,selection,authority FROM donna_background_space_selections WHERE space=?1",
            [space],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?)
}

pub(crate) fn resolve(hub: &Hub, context: &Context) -> Result<Prepared> {
    let db = hub.store.db.locked();
    let (reference, selected) = current_reference(&db, &context.space_id)?;
    // A seleção global autoriza apenas o chat próprio Pessoal. Contexto explícito
    // exige vínculo mesmo quando o nome do Space é Pessoal.
    if context.space_id == crate::fork::spaces::PERSONAL && context.conversation_revision.is_none() {
        return Ok(Prepared {
            space: context.space_id.clone(),
            reference,
            revision: None,
            issuer: Issuer::Local,
            binding: selected,
        });
    }
    let (revision, raw, issuer) =
        row(&db, &context.space_id)?.ok_or_else(|| anyhow!("background_space_not_authorized"))?;
    let saved: Reference = serde_json::from_str(&raw.ok_or_else(|| anyhow!("background_space_not_authorized"))?)?;
    ensure!(saved == reference, "background_space_selection_stale");
    let issuer = serde_json::from_str(&issuer.ok_or_else(|| anyhow!("background_space_not_authorized"))?)?;
    Ok(Prepared { space: context.space_id.clone(), reference, revision: Some(revision), issuer, binding: selected })
}

fn verify(db: &Connection, prepared: &Prepared, context: &Context) -> Result<()> {
    ensure!(context.space_id == prepared.space, "background_space_changed");
    let (current, _) = current_reference(db, &prepared.space)?;
    ensure!(current == prepared.reference, "background_selection_changed");
    if let Some(expected) = prepared.revision {
        let (revision, raw, authority) =
            row(db, &prepared.space)?.ok_or_else(|| anyhow!("background_space_revoked"))?;
        ensure!(revision == expected, "background_space_revoked");
        let saved: Reference = serde_json::from_str(&raw.ok_or_else(|| anyhow!("background_space_revoked"))?)?;
        let issuer = serde_json::to_string(&prepared.issuer)?;
        ensure!(saved == current && authority.as_deref() == Some(issuer.as_str()), "background_space_revoked");
    }
    Ok(())
}

/// Guarda original por fora do worker; callback interno só recebe a Connection.
pub(crate) fn with_current<T>(
    hub: &Hub,
    prepared: &Prepared,
    action: impl FnOnce(&dyn Fn(&Connection, &Context) -> Result<()>) -> Result<T>,
) -> Result<T> {
    match &prepared.issuer {
        Issuer::Local => action(&|db, context| verify(db, prepared, context)),
        Issuer::Owner { key, binding } => hub.cloud.with_engine_owner(|owner| {
            let owner = owner.ok_or_else(|| anyhow!("background_authority_revoked"))?;
            action(&|db, context| {
                ensure!(hub.cloud_synced.load(Ordering::Acquire), "background_authority_unsynced");
                crate::store::bootstrap::assert_engine_authority_tx(db, key, owner, binding)?;
                verify(db, prepared, context)
            })
        }),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Save {
    space_id: String,
    function_id: FunctionId,
    expected_revision: i64,
    expected_space_revision: i64,
    #[serde(default)]
    function_revision: Option<u64>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    account_revision: Option<u64>,
    enabled: bool,
}

fn listing(db: &Connection) -> Result<Value> {
    let spaces: crate::fork::spaces::Snapshot = read(db, SPACES)?;
    let mut rows = Vec::new();
    for space in spaces.spaces {
        let saved = row(db, &space.id)?;
        let revision = saved.as_ref().map_or(0, |r| r.0);
        let saved_reference =
            saved.as_ref().and_then(|r| r.1.as_deref()).map(serde_json::from_str::<Reference>).transpose()?;
        let current = current_reference(db, &space.id);
        let state = match (&saved_reference, &current) {
            (None, _) => "not_authorized",
            (Some(saved), Ok((current, _))) if saved == current => "configured",
            (Some(_), _) => "stale",
        };
        // Projeção administrativa mínima: acesso a este vínculo não libera o
        // documento global Local-only nem o catálogo completo da conta.
        let projection = current
            .as_ref()
            .ok()
            .map(|(r, selected)| -> Result<Value> {
                let registry: Registry = read(db, engine_accounts::SLOT)?;
                let account = registry
                    .accounts
                    .iter()
                    .find(|a| a.id == r.account_id)
                    .ok_or_else(|| anyhow!("background_account_missing"))?;
                Ok(json!({"functionRevision":r.function_revision,"accountId":r.account_id,
                "accountRevision":r.account_revision,"accountLabel":account.dto().label,
                "modelId":selected.model_id}))
            })
            .transpose()?;
        rows.push(json!({"spaceId":space.id,"spaceName":space.name,"spaceRevision":space.revision,
            "functionId":FunctionId::MemoryConsolidation,"revision":revision,"state":state,
            "current":projection,
            "execution":"awaiting_executor", "audience":"owner_private"}));
    }
    Ok(json!({"selections":rows,"execution":"awaiting_executor"}))
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    let authority = access::resolve(hub, caller)?;
    let _team = hub.store.team_lock.lock().await;
    let _accounts = engine_accounts::MUTATION.lock().await;
    if method == "backgroundSpaceSelections" {
        ensure!(body.as_object().is_some_and(serde_json::Map::is_empty), "invalid_background_request");
        return authority.with_db(hub, listing);
    }
    ensure!(method == "saveBackgroundSpaceSelection", "unknown_background_method");
    let request: Save = serde_json::from_value(body).map_err(|_| anyhow!("invalid_background_request"))?;
    ensure!(request.function_id == FunctionId::MemoryConsolidation, "background_function_not_supported");
    ensure!(request.expected_revision >= 0, "invalid_background_revision");
    let issuer = serde_json::to_string(&Issuer::from(&authority))?;
    let result = authority.with_db(hub, |db| {
        ensure!(space_revision(db, &request.space_id)? == request.expected_space_revision, "background_space_changed");
        let previous = row(db, &request.space_id)?;
        ensure!(previous.as_ref().map_or(0, |r| r.0) == request.expected_revision, "background_revision_changed");
        let reference = if request.enabled {
            let (reference, _) = current_reference(db, &request.space_id)?;
            ensure!(
                Some(reference.function_revision) == request.function_revision
                    && Some(reference.account_id.as_str()) == request.account_id.as_deref()
                    && Some(reference.account_revision) == request.account_revision,
                "background_selection_changed"
            );
            Some(serde_json::to_string(&reference)?)
        } else {
            None
        };
        let revision = crate::store::next_rev(db)?;
        db.execute(
            "INSERT INTO donna_background_space_selections(space,revision,selection,authority,updated_at)
            VALUES(?1,?2,?3,?4,?5) ON CONFLICT(space) DO UPDATE SET revision=excluded.revision,
            selection=excluded.selection,authority=excluded.authority,updated_at=excluded.updated_at",
            params![
                request.space_id,
                revision,
                reference,
                if request.enabled { Some(&issuer) } else { None },
                crate::store::now_ms()
            ],
        )?;
        listing(db)
    })?;
    let _ = hub.events.send(json!({"type":"backgroundSpaceSelectionsChanged","spaceId":request.space_id}));
    Ok(result)
}

#[cfg(test)]
mod tests;
