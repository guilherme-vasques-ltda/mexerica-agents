//! Contexto de workspace do Hub (`WorkspaceContext`, schema 1), `workspaceEpoch` persistido,
//! bloqueio durável de aparelho revogado e corte dos streams quando o epoch sobe.
//!
//! Um Hub é um workspace. Nesta fase o vínculo é só derivado do dono que o Hub já conhece
//! (`CloudStatus.owner`): nada aqui grava, troca ou exige vínculo. As únicas recusas novas são
//! o aparelho revogado (bloqueio persistente) e o epoch divergente (`409`).
use crate::{
    LockExt,
    api::{GroupError, devices::Caller},
    hub::Hub,
    store::{Scope, Store, now_ms},
};
use anyhow::Result;
use futures::{Stream, StreamExt, stream};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::watch;

pub(crate) const METHOD: &str = "workspaceContext";
/// Campo do corpo (POST), da query (`/events`) e do `b` da assinatura (`/channel`).
pub(crate) const EPOCH_FIELD: &str = "workspaceEpoch";
pub(crate) const EPOCH_MISMATCH: &str = "workspace_epoch_mismatch";
const EPOCH_KEY: &str = "fork.workspace.epoch";
/// Último dono observado: só detecta troca real de dono, não é vínculo.
const OWNER_SEEN_KEY: &str = "fork.workspace.owner_seen";
const UNBOUND: &str = "personal:unbound";

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS fork_workspace_revoked_devices(
            key TEXT NOT NULL,
            grant_id TEXT NOT NULL,
            epoch INTEGER NOT NULL,
            revoked_at INTEGER NOT NULL,
            PRIMARY KEY(key, grant_id));",
    )?;
    Ok(())
}

/// Por que o epoch subiu; decide o evento terminal dos streams abertos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Motivo {
    DeviceRevoked,
    OwnerChanged,
    /// Troca de papel de membro (Hub team).
    #[cfg_attr(not(test), expect(dead_code, reason = "papéis de membro chegam com o vínculo team"))]
    RoleChanged,
}

impl Motivo {
    fn evento(self) -> &'static str {
        match self {
            Self::RoleChanged => "role_changed",
            Self::DeviceRevoked | Self::OwnerChanged => "workspace_revoked",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Epoca {
    pub valor: u64,
    motivo: Option<Motivo>,
}

/// Epoch atual em RAM, espelho do SQLite; publicado antes do commit de cada aumento.
pub(crate) struct Sinal(watch::Sender<Epoca>);

impl Sinal {
    pub(crate) fn carregar(store: &Store) -> Self {
        let valor = epoch_on(&store.db.locked()).unwrap_or_else(|error| {
            tracing::error!(error = format!("{error:#}"), "workspaceEpoch ilegível; pedidos com epoch divergem");
            u64::MAX
        });
        Self(watch::Sender::new(Epoca { valor, motivo: None }))
    }

    pub(crate) fn atual(&self) -> u64 {
        self.0.borrow().valor
    }

    pub(crate) fn assinar(&self) -> watch::Receiver<Epoca> {
        self.0.subscribe()
    }

    fn publicar(&self, valor: u64, motivo: Motivo) {
        self.0.send_replace(Epoca { valor, motivo: Some(motivo) });
    }
}

fn epoch_on(c: &Connection) -> Result<u64> {
    let raw: Option<String> = c.query_row("SELECT v FROM kv WHERE k=?1", [EPOCH_KEY], |r| r.get(0)).optional()?;
    Ok(raw.map(|v| v.parse()).transpose()?.unwrap_or(0))
}

/// Sobe o epoch na Tx do chamador e publica em RAM antes do commit: nenhum evento sai
/// entre o commit e o aviso. Se o commit falhar, a RAM fica à frente (fecha, não abre).
fn bump_on(hub: &Hub, c: &Connection, motivo: Motivo) -> Result<u64> {
    let atual = epoch_on(c)?.max(hub.workspace.atual());
    let proximo = atual
        .checked_add(1)
        .filter(|n| i64::try_from(*n).is_ok())
        .ok_or_else(|| anyhow::anyhow!("workspaceEpoch esgotado"))?;
    c.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        params![EPOCH_KEY, proximo.to_string()],
    )?;
    hub.workspace.publicar(proximo, motivo);
    tracing::info!(epoch = proximo, ?motivo, "workspaceEpoch subiu");
    Ok(proximo)
}

/// Gancho para remoção de membro e troca de papel (Hub team).
#[cfg_attr(not(test), expect(dead_code, reason = "membros e papéis chegam com o vínculo team"))]
pub(crate) fn bump(hub: &Hub, motivo: Motivo) -> Result<u64> {
    let mut db = hub.store.db.locked();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let proximo = bump_on(hub, &tx, motivo)?;
    tx.commit()?;
    Ok(proximo)
}

/// Revoga no Hub: apaga o aparelho, grava o bloqueio durável e sobe o epoch na mesma Tx.
pub(crate) fn revogar(hub: &Hub, key: &str) -> Result<bool> {
    let mut db = hub.store.db.locked();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let grant: Option<Option<String>> =
        tx.query_row("SELECT grant_id FROM devices WHERE key=?1", [key], |r| r.get(0)).optional()?;
    if !crate::store::remove_device_on(&tx, key)? {
        return Ok(false);
    }
    let epoch = bump_on(hub, &tx, Motivo::DeviceRevoked)?;
    tx.execute(
        "INSERT OR REPLACE INTO fork_workspace_revoked_devices(key,grant_id,epoch,revoked_at) VALUES(?1,?2,?3,?4)",
        params![key, grant.flatten().unwrap_or_default(), i64::try_from(epoch)?, now_ms()],
    )?;
    tx.commit()?;
    Ok(true)
}

/// A mesma chave com a mesma concessão (ou sem concessão) nunca volta sozinha. Só um
/// novo pareamento por código mostrado no computador, ou uma concessão nova, readmite.
pub(crate) fn bloqueado_on(c: &Connection, key: &str, grant: Option<&str>) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM fork_workspace_revoked_devices WHERE key=?1 AND grant_id=?2)",
        params![key, grant.unwrap_or_default()],
        |r| r.get(0),
    )?)
}

/// Leitura fora de Tx; erro de leitura conta como bloqueado.
pub(crate) fn bloqueado(hub: &Hub, device: &crate::store::Device) -> bool {
    bloqueado_on(&hub.store.db.locked(), &device.key, device.grant_id.as_deref()).unwrap_or(true)
}

/// Erro ao ler aparelho bloqueado: o transporte responde `revoked`, não `unauthorized`.
#[derive(Debug)]
pub(crate) struct Revogado;
impl std::fmt::Display for Revogado {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("not_allowed: aparelho revogado neste computador")
    }
}
impl std::error::Error for Revogado {}

/// Pareamento por código emitido no próprio computador: a pessoa repareou o aparelho.
pub(crate) fn liberar_pareamento_local(store: &Store, key: &str) -> Result<()> {
    store.db.locked().execute("DELETE FROM fork_workspace_revoked_devices WHERE key=?1 AND grant_id=''", [key])?;
    Ok(())
}

/// Registra o dono conhecido. Dono que só ficou conhecido não sobe o epoch; outro `userId`
/// depois de um dono já visto sobe.
pub(crate) fn observar_dono(hub: &Hub, dono: Option<&str>) {
    let Some(dono) = dono else { return };
    let visto = |c: &Connection| -> Result<Option<String>> {
        Ok(c.query_row("SELECT v FROM kv WHERE k=?1", [OWNER_SEEN_KEY], |r| r.get(0)).optional()?)
    };
    let resultado = (|| -> Result<()> {
        let mut db = hub.store.db.locked();
        if visto(&db)?.as_deref() == Some(dono) {
            return Ok(());
        }
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let visto = visto(&tx)?;
        if visto.as_deref() == Some(dono) {
            return Ok(());
        }
        tx.execute(
            "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            params![OWNER_SEEN_KEY, dono],
        )?;
        if visto.is_some() {
            bump_on(hub, &tx, Motivo::OwnerChanged)?;
        }
        tx.commit()?;
        Ok(())
    })();
    if let Err(error) = resultado {
        tracing::error!(error = format!("{error:#}"), "não foi possível registrar o dono do workspace");
    }
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Contexto {
    schema: u32,
    hub_id: String,
    kind: &'static str,
    pub(crate) workspace_id: String,
    user_id: Option<String>,
    role: &'static str,
    pub(crate) workspace_epoch: u64,
    capabilities: Vec<&'static str>,
}

/// O Hub decide; `capabilities` só orienta a interface. Papel único nesta fase: `owner`.
pub(crate) fn contexto(hub: &Hub, caller: &Caller, epoch: u64) -> Result<Contexto> {
    let capabilities = match caller {
        Caller::Local => vec!["chat", "bots.manage", "devices.manage"],
        Caller::Device { scopes, .. } => {
            let mut lista = vec![];
            if scopes.contains(&Scope::Control) {
                lista.extend(["chat", "bots.manage"]);
            }
            if scopes.contains(&Scope::OwnerDevices) {
                lista.push("devices.manage");
            }
            lista
        }
        _ => return Err(crate::api::devices::Forbidden.into()),
    };
    let dono = hub.cloud.status().owner.map(|o| o.user_id);
    Ok(Contexto {
        schema: 1,
        hub_id: hub.host_id.clone(),
        kind: "personal",
        workspace_id: dono.as_deref().map_or_else(|| UNBOUND.to_owned(), |u| format!("personal:{u}")),
        user_id: dono,
        role: "owner",
        workspace_epoch: epoch,
        capabilities,
    })
}

pub(crate) fn call(hub: &Hub, caller: &Caller) -> Result<Value> {
    Ok(serde_json::to_value(contexto(hub, caller, hub.workspace.atual())?)?)
}

fn divergente() -> anyhow::Error {
    GroupError { code: EPOCH_MISMATCH, message: "workspace epoch changed; reload the workspace context" }.into()
}

pub(crate) fn is_mismatch(e: &anyhow::Error) -> bool {
    e.downcast_ref::<GroupError>().is_some_and(|g| g.code == EPOCH_MISMATCH)
}

/// Sem epoch: aceito por compatibilidade com clientes antigos. Com epoch: só o atual.
pub(crate) fn conferir(atual: u64, pedido: Option<u64>) -> Result<()> {
    match pedido {
        Some(pedido) if pedido != atual => Err(divergente()),
        _ => Ok(()),
    }
}

/// Confere e retira `workspaceEpoch` do corpo antes de qualquer método ver o pedido.
pub(crate) fn conferir_pedido(hub: &Hub, mut body: Value) -> Result<Value> {
    let Some(valor) = body.as_object_mut().and_then(|b| b.remove(EPOCH_FIELD)) else {
        return Ok(body);
    };
    let pedido = valor.as_u64().ok_or_else(divergente)?;
    conferir(hub.workspace.atual(), Some(pedido))?;
    Ok(body)
}

/// Encerra o stream quando o epoch muda: o evento terminal sai, e nada depois dele. Um
/// evento já produzido mas ainda não entregue (em voo) é descartado se o epoch mudou.
pub(crate) fn cortar<S: Stream<Item = Value> + Send + 'static>(
    hub: &Hub,
    epoch: u64,
    workspace_id: String,
    eventos: S,
) -> impl Stream<Item = Value> + Send + 'static + use<S> {
    let terminal = move |e: Epoca| {
        json!({
            "type": e.motivo.unwrap_or(Motivo::DeviceRevoked).evento(),
            "workspaceId": workspace_id,
            "workspaceEpoch": e.valor,
        })
    };
    stream::unfold(Some((eventos.boxed(), hub.workspace.assinar())), move |estado| {
        let terminal = terminal.clone();
        async move {
            let (mut eventos, mut aviso) = estado?;
            loop {
                let atual = *aviso.borrow_and_update();
                if atual.valor != epoch {
                    return Some((terminal(atual), None));
                }
                tokio::select! {
                    biased;
                    mudou = aviso.changed() => {
                        mudou.ok()?;
                    }
                    evento = eventos.next() => {
                        let evento = evento?;
                        let atual = *aviso.borrow();
                        if atual.valor != epoch {
                            return Some((terminal(atual), None));
                        }
                        return Some((evento, Some((eventos, aviso))));
                    }
                }
            }
        }
    })
}

fn terminal(evento: &Value) -> bool {
    matches!(evento["type"].as_str(), Some("workspace_revoked" | "role_changed")) && evento.get(EPOCH_FIELD).is_some()
}

/// No `/channel` quem encerra é o canal (`reject` e close 4003); a assinatura só deixa de entregar.
pub(crate) fn sem_terminal(
    eventos: impl Stream<Item = Value> + Send + 'static,
) -> impl Stream<Item = Value> + Send + 'static {
    stream::unfold(eventos.boxed(), |mut eventos| async move {
        let evento = eventos.next().await?;
        if terminal(&evento) {
            std::future::pending::<()>().await;
        }
        Some((evento, eventos))
    })
}

/// Código estável do `reject` do `/channel`, ao lado do `code` legado.
pub(crate) fn codigo_estavel(code: crate::api::devices::RejectCode) -> Option<&'static str> {
    use crate::api::devices::RejectCode;
    match code {
        RejectCode::Revoked => Some("device_revoked"),
        RejectCode::Unauthorized | RejectCode::BadSignature => Some("device_unpaired"),
        RejectCode::LeaseExpired => Some("lease_expired"),
        RejectCode::UnsupportedVersion | RejectCode::PairingClosed | RejectCode::RateLimited => None,
    }
}

/// `reject` do `/channel` quando o epoch subiu com o canal aberto.
pub(crate) fn reject_canal(e: &Epoca) -> Value {
    use crate::api::devices::RejectCode;
    json!({
        "t": "reject",
        "code": RejectCode::Revoked,
        "message": RejectCode::Revoked.message(),
        "stableCode": e.motivo.unwrap_or(Motivo::DeviceRevoked).evento(),
    })
}

#[cfg(test)]
mod tests;
