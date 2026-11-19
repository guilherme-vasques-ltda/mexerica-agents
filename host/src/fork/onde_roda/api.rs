use super::{
    autoridade::Authority,
    inspect, protocolo as p, store,
    tipos::{ConnectionPackage, Manifest, Observation, Operation, Phase, Place, RUNNER_PORT, label},
};
use crate::{api::devices::Caller, hub::Hub, market::vault};
use anyhow::{Result, anyhow, ensure};
use ed25519_dalek::SigningKey;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
fn require_empty_object(body: Value) -> Result<()> {
    ensure!(body.as_object().is_some_and(serde_json::Map::is_empty), "empty_object_required");
    serde_json::from_value::<Empty>(body)?;
    Ok(())
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Target {
    pub id: String,
    pub revision: u64,
    pub admission_revision: u64,
    pub connection_revision: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Request {
    #[serde(with = "p::uuid_wire")]
    client_request_id: uuid::Uuid,
    target: Option<Target>,
    intent: Intent,
}
#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields, rename_all = "snake_case")]
enum Intent {
    Create { name: String, resource_id: String },
    Rename { name: String },
    PrepareConnection { rotate: bool },
    PinRunner { pin: String, manifest: Manifest, rotate: bool },
    Refix { package: ConnectionPackage },
    InspectStart {},
    Remove {},
}
impl Intent {
    fn method(&self) -> &'static str {
        match self {
            Self::Create { .. } => "donnaWorkplaceCreate",
            Self::Rename { .. } => "donnaWorkplaceRename",
            Self::PrepareConnection { .. } => "donnaWorkplacePrepareConnection",
            Self::PinRunner { .. } => "donnaWorkplacePinRunner",
            Self::Refix { .. } => "donnaWorkplaceRefix",
            Self::InspectStart { .. } => "donnaWorkplaceInspectStart",
            Self::Remove { .. } => "donnaWorkplaceRemove",
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Query {
    #[serde(with = "p::uuid_wire")]
    operation_id: uuid::Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Cancel {
    #[serde(with = "p::uuid_wire")]
    operation_id: uuid::Uuid,
    #[serde(with = "p::uuid_wire")]
    client_request_id: uuid::Uuid,
}

pub fn slot(id: &str, kind: &str) -> Result<String> {
    uuid::Uuid::parse_str(id)?;
    ensure!(matches!(kind, "ssh" | "control"), "slot_invalid");
    Ok(format!("donna.workplace.{id}.{kind}"))
}
fn target(db: &rusqlite::Connection, a: &Authority, t: &Target) -> Result<Place> {
    let p = store::get(db, &t.id)?;
    a.target(&p)?;
    store::cas(&p, t.revision, t.admission_revision, t.connection_revision)?;
    Ok(p)
}
fn replay(db: &rusqlite::Connection, a: &Authority, id: &str, digest: &str) -> Result<Option<Value>> {
    let answer = store::receipt(db, a, id, digest)?;
    if let Some(v) = &answer {
        if let Some(op_id) = v["operation"]["id"].as_str() {
            let op = store::operation(db, op_id)?;
            let place = store::get(db, &op.place_id)?;
            a.target(&place)?;
            ensure!(
                place.admission_revision == op.admission_revision
                    && place.connection_revision == op.connection_revision,
                "receipt_invalidated"
            );
            return Ok(Some(json!({"operation":op,"place":place.summary()})));
        }
        if let Some(place_id) = v["place"]["id"].as_str() {
            let place = store::get(db, place_id)?;
            // Remoção tem receipt repetível, sem ressuscitar projeção privada.
            if v["removed"] != true {
                a.target(&place)?;
                ensure!(v["place"] == place.summary(), "receipt_invalidated");
            }
        }
    }
    Ok(answer)
}

pub async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    call_inner(hub, caller, method, body).await.map_err(|error| {
        let code = error.to_string();
        let safe = [
            "origin_not_admitted",
            "workplace_method_unknown",
            "target_invalidated",
            "revision_conflict",
            "receipt_invalidated",
            "operation_invalidated",
            "operation_not_found",
            "place_not_found",
            "request_conflict",
            "inspection_conflict",
            "prepare_required",
            "runner_pin_required",
            "runner_pin_reserved",
            "runner_pin_invalid",
            "intent_mismatch",
            "target_required",
            "target_not_allowed",
            "resource_invalid",
            "resource_mismatch",
            "name_invalid",
            "endpoint_invalid",
            "host_key_invalid",
            "artifact_incompatible",
            "artifact_invalid",
            "driver_invalid",
            "rotation_confirmation_required",
            "rotation_without_preparation",
            "vault_unavailable",
            "keygen_timeout",
            "keygen_unavailable",
            "runtime_root_required",
        ];
        anyhow!(if safe.contains(&code.as_str()) { code } else { "workplace_request_failed".into() })
    })
}
async fn call_inner(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    super::autoridade::require(caller, method)?;
    let a = Authority::local(caller, &hub.host_id)?;
    if method == "donnaWorkplaceGuide" {
        require_empty_object(body)?;
        return super::guia::response();
    }
    if matches!(method, "donnaWorkplaceAccess" | "donnaWorkplaceList") {
        require_empty_object(body)?;
        return store::transaction(&hub.store, |db| {
            if method == "donnaWorkplaceAccess" {
                Ok(json!({"available":true,"origin":"local","helpers":"indisponivel"}))
            } else {
                Ok(json!({"places":store::list(db,&a)?,"operations":store::latest_operations(db,&a)?}))
            }
        });
    }
    if method == "donnaWorkplaceInspectStatus" {
        let q: Query = serde_json::from_value(body)?;
        return store::transaction(&hub.store, |db| {
            let op = store::operation(db, &q.operation_id.to_string())?;
            let place = store::get(db, &op.place_id)?;
            a.target(&place)?;
            ensure!(
                op.authority == "local"
                    && op.hub_id == a.hub_id()
                    && place.admission_revision == op.admission_revision
                    && place.connection_revision == op.connection_revision,
                "operation_invalidated"
            );
            Ok(json!({"operation":op}))
        });
    }
    if method == "donnaWorkplaceInspectCancel" {
        let digest = p::b64(&p::hash(&serde_json::to_vec(&body)?));
        let q: Cancel = serde_json::from_value(body)?;
        return store::transaction(&hub.store, |db| {
            if let Some(v) = replay(db, &a, &q.client_request_id.to_string(), &digest)? {
                return Ok(v);
            }
            let mut op = store::operation(db, &q.operation_id.to_string())?;
            let place = store::get(db, &op.place_id)?;
            a.target(&place)?;
            ensure!(
                op.authority == "local"
                    && op.hub_id == a.hub_id()
                    && op.admission_revision == place.admission_revision
                    && op.connection_revision == place.connection_revision,
                "operation_invalidated"
            );
            if op.phase == Phase::Verificando {
                op.phase = Phase::CanceladaIncerta;
                op.reason = Some("cancelled_result_uncertain".into());
                store::save_operation(db, &op)?;
            }
            let answer = json!({"operation":op});
            store::remember(db, &a, &q.client_request_id.to_string(), &digest, &answer)?;
            Ok(answer)
        });
    }
    let digest = p::b64(&p::hash(&p::fields(&[method.as_bytes(), &serde_json::to_vec(&body)?])?));
    let req: Request = serde_json::from_value(body)?;
    ensure!(req.intent.method() == method, "intent_mismatch");
    let rid = req.client_request_id.to_string();
    if let Some(v) = store::transaction(&hub.store, |db| replay(db, &a, &rid, &digest))? {
        return Ok(v);
    }
    if let Intent::PrepareConnection { rotate } = &req.intent {
        let t = req.target.as_ref().ok_or_else(|| anyhow!("target_required"))?;
        return prepare(hub, &a, t, *rotate, &rid, &digest).await;
    }
    let mut started = None;
    let answer = store::transaction(&hub.store, |db| {
        if let Some(v) = replay(db, &a, &rid, &digest)? {
            return Ok(v);
        }
        let mut place = if let Intent::Create { name, resource_id } = &req.intent {
            ensure!(req.target.is_none(), "target_not_allowed");
            label(name)?;
            ensure!(super::tipos::resource_id(resource_id), "resource_invalid");
            Place {
                id: uuid::Uuid::new_v4().to_string(),
                hub_id: hub.host_id.clone(),
                resource_id: resource_id.clone(),
                name: name.trim().into(),
                revision: 1,
                admission_revision: 1,
                connection_revision: 0,
                capacity_revision: 0,
                removed: false,
                ssh_public: None,
                control_public: None,
                runner_pin: None,
                manifest: None,
                connection: None,
            }
        } else {
            target(db, &a, req.target.as_ref().ok_or_else(|| anyhow!("target_required"))?)?
        };
        let mut operation = None;
        match &req.intent {
            Intent::Create { .. } => {}
            Intent::Rename { name } => {
                label(name)?;
                place.name = name.trim().into();
                place.revision += 1;
            }
            Intent::PinRunner { pin, manifest, rotate } => {
                let key = p::fixed::<32>(pin)?;
                ed25519_dalek::VerifyingKey::from_bytes(&key).map_err(|_| anyhow!("runner_pin_invalid"))?;
                manifest.validate()?;
                ensure!(place.control_public.is_some() && place.ssh_public.is_some(), "prepare_required");
                ensure!(*rotate == place.runner_pin.is_some(), "rotation_confirmation_required");
                store::reserve(db, &place, pin)?;
                place.runner_pin = Some(pin.clone());
                place.manifest = Some(manifest.clone());
                place.admission_revision += 1;
                store::invalidate(db, &place.id)?;
            }
            Intent::Refix { package } => {
                package.validate(&place.resource_id)?;
                ensure!(place.runner_pin.is_some(), "runner_pin_required");
                place.connection = Some(package.clone());
                place.connection_revision += 1;
                store::invalidate(db, &place.id)?;
            }
            Intent::Remove {} => {
                place.removed = true;
                place.admission_revision += 1;
                store::invalidate(db, &place.id)?;
            }
            Intent::InspectStart {} => {
                ensure!(
                    place.control_public.is_some()
                        && place.runner_pin.is_some()
                        && place.connection.is_some()
                        && place.manifest.is_some(),
                    "prepare_required"
                );
                ensure!(!store::active(db, &place.id)?, "inspection_conflict");
                let op = Operation {
                    id: uuid::Uuid::new_v4().to_string(),
                    place_id: place.id.clone(),
                    hub_id: hub.host_id.clone(),
                    admission_revision: place.admission_revision,
                    connection_revision: place.connection_revision,
                    authority: "local".into(),
                    intent: rid.clone(),
                    phase: Phase::Verificando,
                    observed_at: None,
                    deadline: crate::store::now_ms() + 60_000,
                    observation: Observation::default(),
                    reason: None,
                };
                store::save_operation(db, &op)?;
                started = Some(op.id.clone());
                operation = Some(op);
            }
            Intent::PrepareConnection { .. } => anyhow::bail!("intent_mismatch"),
        }
        store::save(db, &place)?;
        let answer = if let Some(op) = operation {
            json!({"operation":op,"place":place.summary()})
        } else {
            json!({"place":place.summary(),"removed":place.removed})
        };
        store::remember(db, &a, &rid, &digest, &answer)?;
        Ok(answer)
    })?;
    if let Some(id) = started {
        inspect::spawn(hub.clone(), a, id);
    }
    Ok(answer)
}

async fn prepare(hub: &Arc<Hub>, a: &Authority, t: &Target, rotate: bool, rid: &str, digest: &str) -> Result<Value> {
    let existing = store::transaction(&hub.store, |db| {
        let place = target(db, a, t)?;
        if !rotate && place.control_public.is_some() {
            let answer = material(&place);
            store::remember(db, a, rid, digest, &answer)?;
            return Ok(Some(answer));
        }
        ensure!(!rotate || place.control_public.is_some(), "rotation_without_preparation");
        Ok(None)
    })?;
    if let Some(answer) = existing {
        return Ok(answer);
    }
    let runtime_root = store::runtime_root(&hub.store)?;
    vault::unlock(hub.clone()).await.map_err(|_| anyhow!("vault_unavailable"))?;
    store::transaction(&hub.store, |db| target(db, a, t).map(|_| ()))?;
    let dir = super::transporte::PrivateDir::hub(&runtime_root)?;
    let key_path = dir.path.join("bridge");
    let mut cmd = tokio::process::Command::new("/usr/bin/ssh-keygen");
    cmd.env_clear()
        .args(["-q", "-t", "ed25519", "-N", "", "-C", "donna-ponte", "-f"])
        .arg(&key_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(std::time::Duration::from_secs(10), cmd.status())
        .await
        .map_err(|_| anyhow!("keygen_timeout"))??;
    ensure!(status.success(), "keygen_unavailable");
    let private = Zeroizing::new(std::fs::read_to_string(&key_path)?);
    let public = std::fs::read_to_string(key_path.with_extension("pub"))?.trim().to_owned();
    let control_seed = Zeroizing::new(p::random::<32>()?);
    let control = SigningKey::from_bytes(&control_seed);
    let control_public = p::b64(control.verifying_key().as_bytes());
    let ssh_slot = slot(&t.id, "ssh")?;
    let control_slot = slot(&t.id, "control")?;
    let sealed_ssh = vault::seal(&hub.store, &ssh_slot, &private).map_err(|_| anyhow!("vault_unavailable"))?;
    let sealed_control = vault::seal(&hub.store, &control_slot, &p::b64(control_seed.as_ref()))
        .map_err(|_| anyhow!("vault_unavailable"))?;
    store::transaction(&hub.store, |db| {
        if let Some(v) = replay(db, a, rid, digest)? {
            return Ok(v);
        }
        let mut place = target(db, a, t)?;
        ensure!(rotate == place.control_public.is_some(), "rotation_confirmation_required");
        // Ciphertext e admissão são publicados atomicamente. Nenhum Store/cofre aninhado.
        for (slot, sealed) in [(&ssh_slot, &sealed_ssh), (&control_slot, &sealed_control)] {
            db.execute(
                "INSERT INTO kv(k,v) VALUES(?,?) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
                rusqlite::params![slot, sealed],
            )?;
        }
        place.ssh_public = Some(public.clone());
        place.control_public = Some(control_public.clone());
        place.admission_revision += 1;
        store::invalidate(db, &place.id)?;
        store::save(db, &place)?;
        let answer = material(&place);
        store::remember(db, a, rid, digest, &answer)?;
        Ok(answer)
    })
}

fn material(place: &Place) -> Value {
    json!({"place":place.summary(),"publicMaterial":{"sshPublic":place.ssh_public,"controlPublic":place.control_public,
        "hubId":place.hub_id,"placeId":place.id,"resourceId":place.resource_id,"runnerPort":RUNNER_PORT,
        "runnerPin":place.runner_pin,"manifest":place.manifest}})
}
