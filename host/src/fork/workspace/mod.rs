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
use std::sync::atomic::{AtomicU64, Ordering};
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

/// Retrato publicado em RAM: dono e epoch mudam juntos, num envio só, e só depois do commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Epoca {
    pub valor: u64,
    motivo: Option<Motivo>,
    dono: Option<String>,
    /// Commits de aumento em curso, cada um pela sua marca: enquanto houver um, quem confere o
    /// epoch espera o número sair.
    pendentes: Vec<u64>,
}

impl Epoca {
    /// O número publicado avançou desde `epoch`: o que foi aberto nele termina.
    pub(crate) fn avancou(&self, epoch: u64) -> bool {
        self.valor != epoch
    }

    /// Há um commit em curso: nada sai pelo epoch atual até ele publicar ou desfazer.
    pub(crate) fn pendente(&self) -> bool {
        !self.pendentes.is_empty()
    }

    fn motivo(&self) -> Motivo {
        self.motivo.unwrap_or(Motivo::DeviceRevoked)
    }
}

/// Epoch e dono atuais em RAM, espelho do SQLite. Sob o lock do banco: a marca pendente entra
/// antes do commit e o número só é publicado depois dele. Uma queda entre os dois nunca faz um
/// epoch anunciado voltar atrás, e nada sai pelo epoch antigo entre o commit e a publicação.
pub(crate) struct Sinal {
    retrato: watch::Sender<Epoca>,
    marcas: AtomicU64,
}

/// Marca pendente de um commit. Publicar a resolve; erro, rollback ou panic a desfazem ao sair
/// do escopo, e só ela: a marca de outro commit nunca é apagada.
pub(crate) struct Marca<'a> {
    sinal: &'a Sinal,
    id: u64,
}

impl Marca<'_> {
    fn publicar(self, valor: u64, motivo: Motivo, dono: Option<&str>) {
        let id = self.id;
        self.sinal.retrato.send_if_modified(|e| {
            let propria = soltar(e, id);
            let subiu = avancar(e, valor, motivo);
            let trocou = dono.is_some_and(|dono| e.dono.as_deref() != Some(dono));
            if trocou {
                e.dono = dono.map(str::to_owned);
            }
            propria || subiu || trocou
        });
        std::mem::forget(self);
    }
}

impl Drop for Marca<'_> {
    fn drop(&mut self) {
        let id = self.id;
        self.sinal.retrato.send_if_modified(|e| soltar(e, id));
    }
}

impl Sinal {
    pub(crate) fn carregar(store: &Store) -> Self {
        let valor = epoch_on(&store.db.locked()).unwrap_or_else(|error| {
            tracing::error!(error = format!("{error:#}"), "workspaceEpoch ilegível; pedidos com epoch divergem");
            u64::MAX
        });
        Self {
            retrato: watch::Sender::new(Epoca { valor, motivo: None, dono: None, pendentes: vec![] }),
            marcas: AtomicU64::new(0),
        }
    }

    pub(crate) fn atual(&self) -> u64 {
        self.retrato.borrow().valor
    }

    pub(crate) fn retrato(&self) -> Epoca {
        self.retrato.borrow().clone()
    }

    pub(crate) fn assinar(&self) -> watch::Receiver<Epoca> {
        self.retrato.subscribe()
    }

    fn marcar(&self) -> Marca<'_> {
        let id = self.marcas.fetch_add(1, Ordering::Relaxed);
        self.retrato.send_modify(|e| e.pendentes.push(id));
        Marca { sinal: self, id }
    }

    /// Marca pendente fora de um commit, para provas do transporte.
    #[cfg(test)]
    pub(crate) fn marcar_para_prova(&self) -> Marca<'_> {
        self.marcar()
    }

    /// Dono sem aumento de epoch (o primeiro dono conhecido, ou o mesmo).
    fn publicar_dono(&self, dono: Option<&str>) {
        self.retrato.send_if_modified(|e| {
            if e.dono.as_deref() == dono {
                return false;
            }
            e.dono = dono.map(str::to_owned);
            true
        });
    }
}

fn soltar(e: &mut Epoca, id: u64) -> bool {
    let antes = e.pendentes.len();
    e.pendentes.retain(|p| *p != id);
    e.pendentes.len() != antes
}

fn avancar(e: &mut Epoca, valor: u64, motivo: Motivo) -> bool {
    if valor <= e.valor {
        return false;
    }
    e.valor = valor;
    e.motivo = Some(motivo);
    true
}

fn epoch_on(c: &Connection) -> Result<u64> {
    let raw: Option<String> = c.query_row("SELECT v FROM kv WHERE k=?1", [EPOCH_KEY], |r| r.get(0)).optional()?;
    Ok(raw.map(|v| v.parse()).transpose()?.unwrap_or(0))
}

/// Sobe o epoch na Tx do chamador. Quem chama publica em RAM só depois do commit.
fn bump_on(c: &Connection) -> Result<u64> {
    let proximo = epoch_on(c)?
        .checked_add(1)
        .filter(|n| i64::try_from(*n).is_ok())
        .ok_or_else(|| anyhow::anyhow!("workspaceEpoch esgotado"))?;
    c.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        params![EPOCH_KEY, proximo.to_string()],
    )?;
    Ok(proximo)
}

fn anunciar(marca: Marca<'_>, epoch: u64, motivo: Motivo, dono: Option<&str>) {
    marca.publicar(epoch, motivo, dono);
    tracing::info!(epoch, ?motivo, "workspaceEpoch subiu");
}

/// Commit de um aumento, com o lock do banco ainda tomado pelo chamador: a marca entra antes, e
/// o número sai logo depois do commit. Se o commit falhar, a marca sai do escopo e é desfeita
/// ainda sob o mesmo lock; nada é anunciado.
fn confirmar(hub: &Hub, tx: rusqlite::Transaction<'_>, epoch: u64, motivo: Motivo, dono: Option<&str>) -> Result<()> {
    let marca = hub.workspace.marcar();
    tx.commit()?;
    anunciar(marca, epoch, motivo, dono);
    Ok(())
}

/// Gancho para remoção de membro e troca de papel (Hub team).
#[cfg_attr(not(test), expect(dead_code, reason = "membros e papéis chegam com o vínculo team"))]
pub(crate) fn bump(hub: &Hub, motivo: Motivo) -> Result<u64> {
    let mut db = hub.store.db.locked();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let proximo = bump_on(&tx)?;
    confirmar(hub, tx, proximo, motivo, None)?;
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
    let epoch = bloquear_on(&tx, key, grant.flatten().as_deref())?;
    confirmar(hub, tx, epoch, Motivo::DeviceRevoked, None)?;
    Ok(true)
}

/// Revogação pela cerca do bootstrap: o bloqueio e o epoch entram na Tx da própria cerca, uma
/// vez só (na passagem para tombstone). A marca é resolvida sob o mesmo lock: publicada depois
/// do commit, ou desfeita se a cerca falhar.
pub(crate) fn revogar_cercado(hub: &Hub, grant: &str, key: &str, now: i64) -> Result<bool> {
    let marcado = std::cell::RefCell::new(None);
    hub.store.bootstrap_revoke_fenced(
        &hub.identity.sign,
        grant,
        key,
        now,
        |tx| {
            let epoch = bloquear_on(tx, key, Some(grant))?;
            *marcado.borrow_mut() = Some((hub.workspace.marcar(), epoch));
            Ok(())
        },
        |confirmado| {
            if let Some((marca, epoch)) = marcado.borrow_mut().take()
                && confirmado
            {
                anunciar(marca, epoch, Motivo::DeviceRevoked, None);
            }
        },
    )
}

/// Bloqueio durável da chave (com a concessão) e epoch novo, na Tx do chamador.
pub(crate) fn bloquear_on(c: &Connection, key: &str, grant: Option<&str>) -> Result<u64> {
    let epoch = bump_on(c)?;
    c.execute(
        "INSERT OR REPLACE INTO fork_workspace_revoked_devices(key,grant_id,epoch,revoked_at) VALUES(?1,?2,?3,?4)",
        params![key, grant.unwrap_or_default(), i64::try_from(epoch)?, now_ms()],
    )?;
    Ok(epoch)
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

/// Revogado e sem linha: só depois de o transporte provar a posse da chave.
pub(crate) fn revogado_sem_linha(hub: &Hub, key: &str) -> bool {
    hub.store
        .db
        .locked()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM fork_workspace_revoked_devices WHERE key=?1)
               AND NOT EXISTS(SELECT 1 FROM devices WHERE key=?1)",
            [key],
            |r| r.get(0),
        )
        .unwrap_or(false)
}

/// Pareamento por código mostrado no próprio computador: a pessoa repareou esta chave. Na Tx
/// que grava o aparelho novo.
pub(crate) fn liberar_pareamento_on(c: &Connection, key: &str) -> Result<()> {
    c.execute("DELETE FROM fork_workspace_revoked_devices WHERE key=?1 AND grant_id=''", [key])?;
    Ok(())
}

/// Readmissão pelo código: libera o bloqueio e grava o aparelho na mesma Tx.
pub(crate) fn readmitir_pareado(store: &Store, device: &crate::store::Device) -> Result<()> {
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    liberar_pareamento_on(&tx, &device.key)?;
    crate::store::put_device_on(&tx, device)?;
    tx.commit()?;
    Ok(())
}

/// Registra o dono que a nuvem informa agora e publica dono e epoch juntos. O dono é lido sob o
/// lock do status Cloud, na ordem Cloud → banco: uma observação atrasada nunca sobrescreve o dono
/// atual. Dono que só ficou conhecido não sobe o epoch; outro `userId` depois de um já visto sobe.
pub(crate) fn observar_dono(hub: &Hub) {
    let visto = |c: &Connection| -> Result<Option<String>> {
        Ok(c.query_row("SELECT v FROM kv WHERE k=?1", [OWNER_SEEN_KEY], |r| r.get(0)).optional()?)
    };
    let resultado = hub.cloud.with_personal_status(|status| {
        let mut db = hub.store.db.locked();
        let Some(dono) = status.owner.as_ref().map(|o| o.user_id.as_str()) else {
            hub.workspace.publicar_dono(None);
            return Ok(());
        };
        let anterior = visto(&db)?;
        if anterior.as_deref() == Some(dono) {
            hub.workspace.publicar_dono(Some(dono));
            return Ok(());
        }
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            params![OWNER_SEEN_KEY, dono],
        )?;
        if anterior.is_none() {
            tx.commit()?;
            hub.workspace.publicar_dono(Some(dono));
            return Ok(());
        }
        let epoch = bump_on(&tx)?;
        confirmar(hub, tx, epoch, Motivo::OwnerChanged, Some(dono))
    });
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
pub(crate) fn contexto(hub: &Hub, caller: &Caller, retrato: &Epoca) -> Result<Contexto> {
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
    Ok(Contexto {
        schema: 1,
        hub_id: hub.host_id.clone(),
        kind: "personal",
        workspace_id: workspace_id(retrato),
        user_id: retrato.dono.clone(),
        role: "owner",
        workspace_epoch: retrato.valor,
        capabilities,
    })
}

fn workspace_id(retrato: &Epoca) -> String {
    retrato.dono.as_deref().map_or_else(|| UNBOUND.to_owned(), |u| format!("personal:{u}"))
}

pub(crate) fn call(hub: &Hub, caller: &Caller) -> Result<Value> {
    Ok(serde_json::to_value(contexto(hub, caller, &hub.workspace.retrato())?)?)
}

fn divergente() -> anyhow::Error {
    GroupError { code: EPOCH_MISMATCH, message: "workspace epoch changed; reload the workspace context" }.into()
}

pub(crate) fn is_mismatch(e: &anyhow::Error) -> bool {
    e.downcast_ref::<GroupError>().is_some_and(|g| g.code == EPOCH_MISMATCH)
}

/// Sem epoch: aceito por compatibilidade com clientes antigos. Com epoch: só o atual; valor
/// que não é u64 também diverge.
fn conferir(atual: u64, pedido: Option<&Value>) -> Result<()> {
    match pedido.map(Value::as_u64) {
        None => Ok(()),
        Some(Some(pedido)) if pedido == atual => Ok(()),
        Some(_) => Err(divergente()),
    }
}

/// Retrato de quem abre um stream: dono e epoch de uma vez, conferidos com o que o cliente conhece.
pub(crate) fn abrir(hub: &Hub, pedido: Option<u64>) -> Result<Epoca> {
    let retrato = hub.workspace.retrato();
    conferir(retrato.valor, pedido.map(Value::from).as_ref())?;
    Ok(retrato)
}

/// Confere e retira `workspaceEpoch` do corpo antes de qualquer método ver o pedido.
pub(crate) fn conferir_pedido(hub: &Hub, mut body: Value) -> Result<Value> {
    let pedido = body.as_object_mut().and_then(|b| b.remove(EPOCH_FIELD));
    conferir(hub.workspace.atual(), pedido.as_ref())?;
    Ok(body)
}

/// Encerra o stream quando o epoch muda: o evento terminal sai, e nada depois dele. Um
/// evento já produzido mas ainda não entregue (em voo) é descartado se o epoch mudou.
pub(crate) fn cortar<S: Stream<Item = Value> + Send + 'static>(
    hub: &Hub,
    retrato: &Epoca,
    eventos: S,
) -> impl Stream<Item = Value> + Send + 'static + use<S> {
    let (epoch, workspace_id) = (retrato.valor, workspace_id(retrato));
    let terminal = move |e: &Epoca| {
        json!({
            "type": e.motivo().evento(),
            "workspaceId": workspace_id,
            "workspaceEpoch": e.valor,
        })
    };
    stream::unfold(Some((eventos.boxed(), hub.workspace.assinar())), move |estado| {
        let terminal = terminal.clone();
        async move {
            let (mut eventos, mut aviso) = estado?;
            loop {
                let atual = resolvido(&mut aviso).await?;
                if atual.valor != epoch {
                    return Some((terminal(&atual), None));
                }
                tokio::select! {
                    biased;
                    mudou = aviso.changed() => {
                        mudou.ok()?;
                    }
                    evento = eventos.next() => {
                        let evento = evento?;
                        let atual = resolvido(&mut aviso).await?;
                        if atual.valor != epoch {
                            return Some((terminal(&atual), None));
                        }
                        return Some((evento, Some((eventos, aviso))));
                    }
                }
            }
        }
    })
}

/// O retrato sem aumento pendente: o evento terminal do SSE leva o número já gravado.
async fn resolvido(aviso: &mut watch::Receiver<Epoca>) -> Option<Epoca> {
    loop {
        let atual = aviso.borrow_and_update().clone();
        if !atual.pendente() {
            return Some(atual);
        }
        aviso.changed().await.ok()?;
    }
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
        "stableCode": e.motivo().evento(),
    })
}

#[cfg(test)]
mod tests;
