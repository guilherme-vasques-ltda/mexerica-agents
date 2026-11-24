use crate::{
    api::{
        self,
        devices::{self, Caller},
    },
    fork::{autoridade::Autoridade, workspace},
    hub::Hub,
    remote::crypto,
    store::Store,
};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::sync::Arc;

#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Failure {
    Unauthorized,
    UnknownBot,
    Invalid,
    EpochRequired,
    EpochChanged,
    GrantChanged,
    Unknown,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "mailbox: {self:?}")
    }
}

impl std::error::Error for Failure {}

pub(crate) fn migrate(tx: &Connection) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS fork_mailbox_receipts(
            nonce TEXT PRIMARY KEY NOT NULL,
            hub_id TEXT NOT NULL,
            workspace_id TEXT NOT NULL,
            user_id TEXT,
            dk TEXT NOT NULL,
            generation INTEGER NOT NULL,
            digest BLOB NOT NULL);",
    )?;
    Ok(())
}

pub(crate) struct Delivery {
    authority: Autoridade,
    key: String,
    nonce: String,
    epoch: u64,
    generation: i64,
    digest: [u8; 32],
}

tokio::task_local! {
    static DELIVERY: Arc<Delivery>;
}

pub(crate) fn current() -> Option<Arc<Delivery>> {
    DELIVERY.try_with(Arc::clone).ok()
}

pub(crate) async fn deliver(hub: &Arc<Hub>, caller: &Caller, mut body: Value, plain: &[u8], fresh: bool) -> Result<()> {
    let Caller::Device { key, .. } = caller else { return Err(Failure::Unauthorized.into()) };
    devices::permit(caller, "send")?;
    let authority = Autoridade::capturar(hub, caller)?;
    let epoch = body[workspace::EPOCH_FIELD].as_u64().ok_or(Failure::EpochRequired)?;
    let generation = body["grantGeneration"].as_i64().ok_or(Failure::EpochRequired)?;
    if !fresh {
        return Err(Failure::Invalid.into());
    }
    let nonce = body["clientNonce"].as_str().filter(|n| !n.is_empty()).ok_or(Failure::Invalid)?.to_owned();
    let bot = body["botId"].as_str().ok_or(Failure::Invalid)?;
    let delivery = Delivery { authority, key: key.clone(), nonce, epoch, generation, digest: crypto::sha256(&[plain]) };
    if delivery.with_db(hub, bot, |_, _| Ok(()))?.is_none() {
        return Ok(());
    }
    if let Some(body) = body.as_object_mut() {
        body.remove(workspace::EPOCH_FIELD);
        body.remove("grantGeneration");
    }
    DELIVERY.scope(Arc::new(delivery), api::dispatch(hub, caller, "send", body)).await?;
    Ok(())
}

impl Delivery {
    pub(crate) fn with_db<T>(
        &self,
        hub: &Hub,
        bot: &str,
        write: impl FnOnce(&Connection, Option<&str>) -> Result<T>,
    ) -> Result<Option<T>> {
        self.authority.with_db_owner(hub, None, |tx, owner| {
            if hub.workspace.dono_retido()
                || !matches!(
                    crate::fork::sessao_clerk::vinculo::ler_on(tx),
                    crate::fork::sessao_clerk::vinculo::Estado::SemVinculo
                )
            {
                return Err(Failure::Unauthorized.into());
            }
            let workspace = workspace::workspace_id_legado(owner);
            let generation = self.authority.geracao_capturada().ok_or(Failure::Unauthorized)?;
            let receipt: Option<bool> = tx
                .query_row(
                    "SELECT hub_id=?2 AND workspace_id=?3 AND user_id IS ?4 AND dk=?5 AND generation=?6
                        AND digest=?7 FROM fork_mailbox_receipts WHERE nonce=?1",
                    params![self.nonce, hub.host_id, workspace, owner, self.key, generation, self.digest],
                    |row| row.get(0),
                )
                .optional()?;
            if receipt == Some(true) && self.generation == generation {
                return Ok(None);
            }
            if receipt.is_some() || Store::entry_by_nonce_on(tx, bot, &self.nonce)?.is_some() {
                return Err(Failure::Unknown.into());
            }
            if workspace::epoch_on(tx)? != self.epoch {
                return Err(Failure::EpochChanged.into());
            }
            if generation != self.generation {
                return Err(Failure::GrantChanged.into());
            }
            if Store::bot_on(tx, bot)?.is_none_or(|r| r.deleted) {
                return Err(Failure::UnknownBot.into());
            }
            Ok(Some(write(tx, owner)?))
        })
    }

    pub(crate) fn record(&self, tx: &Connection, hub: &Hub, owner: Option<&str>) -> Result<()> {
        tx.execute(
            "INSERT INTO fork_mailbox_receipts(nonce, hub_id, workspace_id, user_id, dk, generation, digest)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                self.nonce,
                hub.host_id,
                workspace::workspace_id_legado(owner),
                owner,
                self.key,
                self.generation,
                self.digest
            ],
        )?;
        Ok(())
    }
}
