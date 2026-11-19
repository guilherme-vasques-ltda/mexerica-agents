//! D543: o Berga principal de um Space — nome e persona fixos, aparência por Space.
//! A ativação escreve bot + Space + conversa + recibo numa única Tx da autoridade,
//! idempotente por `clientNonce`; a lista de figuras do app é só conveniência de UI.
use super::{PERSONAL, Snapshot, store};
use crate::{
    LockExt,
    fork::{
        engine_accounts::{Registry, managed},
        engine_setup::access,
        native_catalog::workspace::PreparedWorkspace,
    },
    hub::Hub,
    store::BotConfig,
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fmt::Write as _, sync::Arc};

/// O nome do principal do Space é fixo: "Berga". Qualquer outro é recusado.
pub(crate) const FIXED_NAME: &str = "Berga";

/// Aparência provisória (estilo Simples, laranja, ponto): trocável pela arte final
/// sem mudar contrato nem dado gravado. Figurinos finais são escolha do app.
const DEFAULT_APPEARANCE: &str = concat!(
    r#"{"v":2,"estilo":"simples","aparencia":{"cor":"laranja","olhos":"ponto","#,
    r#""roupa":"nenhuma","acessorio":"nenhum","objeto":"nenhum"}}"#
);

/// Nome vindo do cliente: ausente ou exatamente "Berga"; qualquer outro valor
/// fornecido (inclusive vazio/branco) é recusado — 4.0 só abre essas duas portas.
pub(crate) fn checked_name(name: Option<&str>) -> Result<String> {
    match name {
        None => Ok(FIXED_NAME.into()),
        Some(raw) => {
            ensure!(raw.trim() == FIXED_NAME, "space_principal_name_fixed");
            Ok(FIXED_NAME.into())
        }
    }
}

/// Aparência do Berga do Space: valida o contrato Mexeriquinha v2 e exige o estilo
/// Simples sem `modeloId` — as figuras são as personalidades do desenho, não os
/// modelos nomeados. Enquanto não houver pelúcia 3D do Berga, `estilo:"3d"` é
/// recusado aqui para nenhum cliente contornar a regra pela API.
pub(crate) fn checked_appearance(value: Option<&Value>) -> Result<Value> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return serde_json::from_str(DEFAULT_APPEARANCE).map_err(Into::into);
    };
    let config: crate::fork_avatars::MexeriquinhaConfig =
        serde_json::from_value(value.clone()).map_err(|_| anyhow::anyhow!("space_principal_appearance_invalid"))?;
    let _ = config;
    ensure!(value["estilo"] == "simples", "space_principal_appearance_style");
    ensure!(value.get("modeloId").is_none_or(Value::is_null), "space_principal_appearance_model");
    Ok(value.clone())
}

/// O `BotConfig` do principal: mesma allowlist do Birth, nome fixo, persona BASE do
/// `donna_fixa` e a aparência escolhida. Nada é herdado do Pessoal.
pub(crate) fn config(space_id: &str, backend: &str, account: &str, appearance: &Value) -> Result<BotConfig> {
    serde_json::from_value(json!({
        "id":"", "spaceId":space_id, "name":FIXED_NAME,
        "description":crate::fork::donna_fixa::BASE_SPACE,
        "backend":backend,
        "forkEditor":{"engineAccountId":account},
        "ownerPreferences":"", "permission":"ask", "permissionChosenByUser":false,
        "sharedMemory":false, "manageTeam":false, "harnessMemory":false, "computer":false,
        "skills":[], "connectors":[], "logins":[], "createdBy":null,
        "mexeriquinha":appearance
    }))
    .map_err(Into::into)
}

/// Núcleo transacional comum de Birth e ativação. A Connection é a da
/// autoridade original: conta, vínculo, bot, conversa e revisão confirmam juntos.
/// O chamador acrescenta o recibo específico antes de deixar a mesma Tx.
pub(super) fn persist_on(db: &Connection, state: &mut Snapshot, cfg: &BotConfig, account: &str) -> Result<()> {
    let registry = Registry::load_db(db)?;
    managed::validate_selection_in(&registry, cfg)?;
    ensure!(
        registry.account(account)?.dto().state == crate::fork::engine_accounts::State::Connected,
        "account_unavailable"
    );
    db.execute("UPDATE kv SET v=?1 WHERE k=?2", params![serde_json::to_string(&state)?, store::KEY])?;
    crate::store::bots::save_bot(db, cfg)?;
    db.execute(
        "UPDATE donna_space_conversations SET engine_account_id=?2 WHERE id=?1 AND bot_id=?1",
        params![cfg.id, account],
    )?;
    state.revision = db.query_row("SELECT CAST(v AS INTEGER) FROM kv WHERE k='rev'", [], |r| r.get(0))?;
    let space =
        state.spaces.iter_mut().find(|s| s.id == cfg.space_id).ok_or_else(|| anyhow::anyhow!("space_not_found"))?;
    space.revision = state.revision;
    db.execute("UPDATE kv SET v=?1 WHERE k=?2", params![serde_json::to_string(&state)?, store::KEY])?;
    Ok(())
}

/// Campos que o principal do Space não pode mudar: persona, poderes, memória,
/// skills/conectores/logins e o próprio vínculo. Só aparência, motor e conta de IA
/// (e o nome, pela regra abaixo) passam — o resto é o mesmo em todo Space (dono, 4.7).
const FIXED_FIELDS: &[&str] = &[
    "spaceId",
    "context",
    "kind",
    "members",
    "leader",
    "description",
    "ownerPreferences",
    "title",
    "command",
    "cwd",
    "permission",
    "permissionChosenByUser",
    "model",
    "modelOptions",
    "fastMode",
    "computer",
    "browserPreference",
    "manageTeam",
    "sharedMemory",
    "harnessMemory",
    "skills",
    "connectors",
    "contaPadrao",
    "perguntarConta",
    "logins",
    "createdBy",
    "notify",
    "hidden",
    "autoName",
    "createdAt",
];

/// Guarda do `save_bot`: o principal do Space tem persona e poderes fixos. O nome é
/// "Berga"; um principal legado com outro nome pode ser renomeado para "Berga", nunca
/// para um terceiro nome. A comparação é por campo serializado: nenhum `PartialEq` novo.
pub(crate) fn check_principal_save(c: &Connection, cfg: &BotConfig) -> Result<()> {
    if !store::is_space_principal(c, &cfg.id)? {
        return Ok(());
    }
    let Some(stored) = crate::store::Store::bot_on(c, &cfg.id)? else {
        ensure!(cfg.name.trim() == FIXED_NAME, "space_principal_name_fixed");
        return Ok(());
    };
    let old = serde_json::to_value(&stored.config)?;
    let new = serde_json::to_value(cfg)?;
    ensure!(
        cfg.name.trim() == FIXED_NAME || cfg.name.trim() == stored.config.name.trim(),
        "space_principal_name_fixed"
    );
    for field in FIXED_FIELDS {
        if old[field] != new[field] {
            tracing::warn!(field, bot = %cfg.id, "space principal fixed field edit refused");
            anyhow::bail!("space_principal_fixed");
        }
    }
    if ["name", "backend", "forkEditor", "avatarColor", "avatarShape", "avatarConfig", "mexeriquinha"]
        .iter()
        .any(|field| old[*field] != new[*field])
    {
        let owner: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM donna_fixa_dono)", [], |r| r.get(0))?;
        ensure!(owner, "space_principal_fixed");
    }
    // A aparência pode mudar (figura/cor por Space), mas segue o mesmo contrato
    // da ativação: estilo Simples, sem modeloId — ninguém grava 3D pela edição.
    if let Some(mexeriquinha) = &cfg.mexeriquinha {
        let value = serde_json::to_value(mexeriquinha)?;
        checked_appearance(Some(&value))?;
    }
    Ok(())
}

// hubId é conferido e retirado pelo dispatch. spaceId é o Space alvo da ativação.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    space_id: String,
    expected_revision: i64,
    client_nonce: String,
    principal: Principal,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Principal {
    #[serde(default)]
    name: Option<String>,
    backend: String,
    engine_account_id: String,
    // Sem skip o `None` serializa `"appearance":null` e muda o digest de pedidos
    // gravados antes do campo existir (recibo do Birth precisa replayar igual).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    appearance: Option<Value>,
}

/// Recibo da ativação: mesma ideia do Birth — nonce + digest por Space.
pub(super) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_space_principal_receipts(
            hub_id TEXT NOT NULL, space_id TEXT NOT NULL, nonce TEXT NOT NULL,
            digest TEXT NOT NULL, bot_id TEXT NOT NULL,
            PRIMARY KEY(hub_id,space_id,nonce));",
    )?;
    Ok(())
}

fn replay(db: &Connection, hub_id: &str, request: &Request, digest: &str) -> Result<Option<Snapshot>> {
    let receipt: Option<(String, String)> = db
        .query_row(
            "SELECT digest,bot_id FROM donna_space_principal_receipts WHERE hub_id=?1 AND space_id=?2 AND nonce=?3",
            params![hub_id, request.space_id, request.client_nonce],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((saved_digest, bot)) = receipt else { return Ok(None) };
    ensure!(saved_digest == digest, "space_principal_nonce_conflict");
    let current = store::read(db)?;
    ensure!(
        current.spaces.iter().any(|s| s.id == request.space_id && s.principal_bot_id.as_deref() == Some(bot.as_str())),
        "space_principal_reference_unavailable"
    );
    Ok(Some(current))
}

fn device_key_label(authority: &access::Authority) -> &str {
    match authority {
        access::Authority::Local => "local",
        access::Authority::Owner { key, .. } => key.as_str(),
    }
}

/// `donnaSpacePrincipal`: ativa o Berga num Space antigo, só com a autoridade de
/// contas do dispatch (Local ou aparelho do dono). Nunca migra sozinho.
pub(crate) async fn call(hub: &Arc<Hub>, authority: &access::Authority, body: Value) -> Result<Value> {
    let authority = authority.clone();
    let request: Request = serde_json::from_value(body)?;
    ensure!(request.space_id != PERSONAL, "personal_space_immutable");
    ensure!(request.expected_revision >= 0, "invalid_space_revision");
    managed::request_id(&request.client_nonce)?;
    checked_name(request.principal.name.as_deref())?;
    let appearance = checked_appearance(request.principal.appearance.as_ref())?;
    ensure!(!request.principal.engine_account_id.is_empty(), "engine_account_required");
    let digest = Sha256::digest(serde_json::to_vec(&request)?).iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    });
    let guard = hub.store.team_lock.clone().lock_owned().await;
    if let Some(snapshot) = authority.with_db(hub, |db| replay(db, &hub.host_id, &request, &digest))? {
        return Ok(serde_json::to_value(snapshot)?);
    }
    let cfg = config(&request.space_id, &request.principal.backend, &request.principal.engine_account_id, &appearance)?;
    let space_id = request.space_id.clone();
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
            let index =
                state.spaces.iter().position(|s| s.id == space_id).ok_or_else(|| anyhow::anyhow!("space_not_found"))?;
            ensure!(state.spaces[index].kind == "space", "personal_space_immutable");
            ensure!(state.spaces[index].principal_bot_id.is_none(), "space_principal_exists");
            if state.revision != request.expected_revision {
                return Err(crate::remote::cloud::Conflict("spaces_revision_conflict").into());
            }
            // Cadastro deste Hub, relido na mesma Tx que cria todas as referências.
            let revision = crate::store::next_rev(db)?;
            state.spaces[index].principal_bot_id = Some(cfg.id.clone());
            state.spaces[index].revision = revision;
            // O validador canônico vê o vínculo dentro desta mesma Tx.
            persist_on(db, &mut state, &cfg, &request.principal.engine_account_id)?;
            db.execute(
                "INSERT INTO donna_space_principal_receipts(hub_id,space_id,nonce,digest,bot_id)
                VALUES(?1,?2,?3,?4,?5)",
                params![hub.host_id, space_id, request.client_nonce, digest, cfg.id],
            )?;
            Ok((state, true))
        })?;
        if created {
            workspace.keep();
            let _ = hub.events.send(json!({"type":"spaces","spaceId":PERSONAL,
                "rev":snapshot.revision,"snapshot":snapshot}));
        }
        drop(emit_guard);
        if created {
            hub.start_created_bot(&cfg);
        }
        // Quem pediu fica no log pela autoridade capturada (Local ou chave do aparelho).
        tracing::info!(space = %space_id, authority = device_key_label(&authority), "space principal activated");
        Ok(serde_json::to_value(snapshot)?)
    })
    .await?
}
