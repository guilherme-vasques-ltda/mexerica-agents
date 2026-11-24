//! Contexto de workspace do Hub (`WorkspaceContext`, schema 1), `workspaceEpoch` persistido,
//! bloqueio durável de aparelho revogado e corte dos streams quando o epoch sobe.
//!
//! Um Hub é um workspace. Sem vínculo gravado, ele é derivado do dono que o Hub já conhece
//! (`CloudStatus.owner`). Com vínculo (só leitura aqui), `kind` e `workspaceId` vêm dele e a sessão
//! Clerk do acesso remoto fica em `fork::sessao_clerk`. Nada aqui grava ou troca o vínculo.
use crate::{
    LockExt,
    api::{GroupError, devices::Caller},
    hub::Hub,
    remote::cloud::CloudStatus,
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
    /// Troca de papel registrada pela sessão do vínculo.
    RoleChanged,
    /// Token mais novo de um `sub` sem acesso ao workspace.
    AcessoRetirado,
}

impl Motivo {
    fn evento(self) -> &'static str {
        match self {
            Self::RoleChanged => "role_changed",
            Self::DeviceRevoked | Self::OwnerChanged | Self::AcessoRetirado => "workspace_revoked",
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
    /// Retenção do dono recebido e ainda não gravado: armada sob o lock do status Cloud antes de a
    /// troca ficar visível, e solta só depois de gravar.
    trava_dono: std::sync::Mutex<Option<TravaDono>>,
}

struct TravaDono {
    id: u64,
    /// Uma gravação já falhou com a troca visível no status recebido: só um epoch novo gravado solta.
    sem_gravar: bool,
}

/// Marca pendente de um commit. Ao sair do escopo (fim normal, erro ou panic), decide pela
/// verdade durável: commit feito publica o número gravado; sem commit, desfaz a marca; commit
/// sem desfecho conhecido deixa a marca pendente. Só mexe na própria marca, nunca na de outro.
pub(crate) struct Marca<'a> {
    sinal: &'a Sinal,
    id: u64,
    epoch: u64,
    motivo: Motivo,
    dono: Dono,
    fase: Fase,
}

/// O que o commit faz com o dono publicado, no mesmo envio do número.
enum Dono {
    Mantem,
    Passa(Option<String>),
}

#[derive(Clone, Copy)]
enum Fase {
    SemCommit,
    Duvida,
    Commitado,
}

impl Marca<'_> {
    /// Commit da Tx que gravou `epoch`. O desfecho fica registrado antes de qualquer outra coisa.
    fn commit(&mut self, tx: rusqlite::Transaction<'_>) -> Result<()> {
        self.fase = Fase::Duvida;
        let feito = tx.commit();
        self.fase = if feito.is_ok() { Fase::Commitado } else { Fase::SemCommit };
        Ok(feito?)
    }

    /// O commit corre fora daqui (a cerca do bootstrap): até `resolver`, o desfecho é incerto.
    fn em_duvida(&mut self) {
        self.fase = Fase::Duvida;
    }

    fn resolver(mut self, commitado: bool) {
        self.fase = if commitado { Fase::Commitado } else { Fase::SemCommit };
    }
}

impl Drop for Marca<'_> {
    fn drop(&mut self) {
        let (id, epoch, motivo) = (self.id, self.epoch, self.motivo);
        match self.fase {
            Fase::SemCommit => {
                self.sinal.retrato.send_if_modified(|e| soltar(e, id));
            }
            Fase::Duvida => {
                tracing::error!(epoch, "commit do workspaceEpoch sem desfecho conhecido; a saída segue retida");
            }
            Fase::Commitado => {
                let dono = std::mem::replace(&mut self.dono, Dono::Mantem);
                self.sinal.retrato.send_if_modified(|e| {
                    let propria = soltar(e, id);
                    let subiu = avancar(e, epoch, motivo);
                    let trocou = match dono {
                        Dono::Passa(novo) if e.dono != novo => {
                            e.dono = novo;
                            true
                        }
                        _ => false,
                    };
                    propria || subiu || trocou
                });
                tracing::info!(epoch, ?motivo, "workspaceEpoch subiu");
            }
        }
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
            trava_dono: std::sync::Mutex::new(None),
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

    /// Marca pendente do commit que vai gravar `epoch` (e, se houver, o dono novo).
    fn marcar(&self, epoch: u64, motivo: Motivo, dono: Dono) -> Marca<'_> {
        let id = self.marcas.fetch_add(1, Ordering::Relaxed);
        self.retrato.send_modify(|e| e.pendentes.push(id));
        Marca { sinal: self, id, epoch, motivo, dono, fase: Fase::SemCommit }
    }

    /// Marca pendente fora de um commit, para provas do transporte.
    #[cfg(test)]
    pub(crate) fn marcar_para_prova(&self) -> Marca<'_> {
        self.marcar(self.atual(), Motivo::DeviceRevoked, Dono::Mantem)
    }

    /// Nada sai pelo epoch atual até o dono recebido ser gravado. Uma retenção já armada é mantida.
    fn travar_dono(&self) {
        let mut trava = self.trava_dono.locked();
        if trava.is_none() {
            let id = self.marcas.fetch_add(1, Ordering::Relaxed);
            self.retrato.send_modify(|e| e.pendentes.push(id));
            *trava = Some(TravaDono { id, sem_gravar: false });
        }
    }

    fn falha_ao_gravar_dono(&self) {
        self.travar_dono();
        if let Some(trava) = self.trava_dono.locked().as_mut() {
            trava.sem_gravar = true;
        }
    }

    /// Há um dono recebido ainda não gravado: o status Cloud não sai para os apps.
    pub(crate) fn dono_retido(&self) -> bool {
        self.trava_dono.locked().is_some()
    }

    fn troca_sem_gravar(&self) -> bool {
        self.trava_dono.locked().as_ref().is_some_and(|t| t.sem_gravar)
    }

    fn destravar_dono(&self) {
        if let Some(trava) = self.trava_dono.locked().take() {
            self.retrato.send_if_modified(|e| soltar(e, trava.id));
        }
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

pub(crate) fn epoch_on(c: &Connection) -> Result<u64> {
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

/// Commit de um aumento, com o lock do banco ainda tomado pelo chamador: a marca entra antes, e
/// sai do escopo ainda sob o mesmo lock, publicando o número se o commit foi feito ou desfazendo
/// a marca se não foi.
fn confirmar(hub: &Hub, tx: rusqlite::Transaction<'_>, epoch: u64, motivo: Motivo, dono: Dono) -> Result<()> {
    hub.workspace.marcar(epoch, motivo, dono).commit(tx)
}

/// Sobe o epoch na Tx do chamador e a confirma: aumento e publicação no mesmo commit.
pub(crate) fn subir_na_tx(hub: &Hub, tx: rusqlite::Transaction<'_>, motivo: Motivo) -> Result<u64> {
    let proximo = bump_on(&tx)?;
    confirmar(hub, tx, proximo, motivo, Dono::Mantem)?;
    Ok(proximo)
}

/// Gancho para remoção de membro e troca de papel (Hub team).
#[cfg_attr(not(test), expect(dead_code, reason = "membros e papéis chegam com o vínculo team"))]
pub(crate) fn bump(hub: &Hub, motivo: Motivo) -> Result<u64> {
    let mut db = hub.store.db.locked();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let proximo = bump_on(&tx)?;
    confirmar(hub, tx, proximo, motivo, Dono::Mantem)?;
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
    confirmar(hub, tx, epoch, Motivo::DeviceRevoked, Dono::Mantem)?;
    Ok(true)
}

/// Revogação pela cerca do bootstrap: o bloqueio e o epoch entram na Tx da própria cerca, uma
/// vez só (na passagem para tombstone). A marca fica em dúvida até a cerca informar o desfecho
/// do commit, ainda sob o mesmo lock: publicada se confirmou, desfeita se não.
pub(crate) fn revogar_cercado(hub: &Hub, grant: &str, key: &str, now: i64) -> Result<bool> {
    let marcado = std::cell::RefCell::new(None);
    hub.store.bootstrap_revoke_fenced(
        &hub.identity.sign,
        grant,
        key,
        now,
        |tx| {
            let epoch = bloquear_on(tx, key, Some(grant))?;
            let mut marca = hub.workspace.marcar(epoch, Motivo::DeviceRevoked, Dono::Mantem);
            marca.em_duvida();
            *marcado.borrow_mut() = Some(marca);
            Ok(())
        },
        |confirmado| {
            if let Some(marca) = marcado.borrow_mut().take() {
                marca.resolver(confirmado);
            }
        },
    )
}

/// Revogação feita por outra cerca, na Tx dela, depois de apagar o aparelho: grava o bloqueio
/// durável e sobe o epoch na mesma Tx, e faz o commit com a marca pendente, ainda sob o lock.
pub(crate) fn revogar_na_tx(hub: &Hub, tx: rusqlite::Transaction<'_>, key: &str, grant: &str) -> Result<()> {
    let epoch = bloquear_on(&tx, key, Some(grant))?;
    confirmar(hub, tx, epoch, Motivo::DeviceRevoked, Dono::Mantem)
}

/// Bloqueio durável da chave (com a concessão) e epoch novo, na Tx do chamador.
pub(crate) fn bloquear_on(c: &Connection, key: &str, grant: Option<&str>) -> Result<u64> {
    let epoch = bump_on(c)?;
    crate::fork::sessao_clerk::desligar_on(c, key)?;
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

/// Readmissão pelo código: libera o bloqueio e grava o aparelho na mesma Tx. Sem vínculo, a chave
/// não fica ligada a nenhum `userId` de um pareamento anterior.
pub(crate) fn readmitir_pareado(store: &Store, device: &crate::store::Device) -> Result<()> {
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    liberar_pareamento_on(&tx, &device.key)?;
    crate::fork::sessao_clerk::desligar_on(&tx, &device.key)?;
    crate::store::put_device_on(&tx, device)?;
    tx.commit()?;
    Ok(())
}

/// Sob o lock do status Cloud, antes de aplicar o dono recebido: se ele muda, a saída fica retida
/// antes de a troca ficar visível. Quem aplica chama `registrar_sob_status` sob o mesmo lock.
pub(crate) fn reter_troca(hub: &Hub, status: &CloudStatus, novo: Option<&str>) {
    if status.owner.as_ref().map(|o| o.user_id.as_str()) != novo {
        hub.workspace.travar_dono();
    }
}

/// Grava o dono recebido e publica dono e epoch juntos, sob o lock do status Cloud (ordem Cloud →
/// banco): uma observação atrasada nunca sobrescreve o dono atual. Dono que só ficou conhecido não
/// sobe o epoch; outro `userId` depois de um já visto sobe. Se a gravação falhar, a retenção fica
/// e `start` repete. Depois de uma falha, a retenção só cai com um epoch novo gravado: nem a
/// ausência de dono nem a volta do dono anterior provam que a troca não circulou.
/// Devolve se a saída ficou livre.
pub(crate) fn registrar_sob_status(hub: &Hub, status: &CloudStatus) -> bool {
    match registrar_dono(hub, status, hub.workspace.troca_sem_gravar()) {
        Ok(()) => {
            hub.workspace.destravar_dono();
            true
        }
        Err(error) => {
            hub.workspace.falha_ao_gravar_dono();
            tracing::error!(error = format!("{error:#}"), "não foi possível registrar o dono do workspace");
            false
        }
    }
}

/// `registrar_sob_status` com o status atual. Devolve se a saída ficou livre.
pub(crate) fn observar_dono(hub: &Hub) -> bool {
    hub.cloud.with_personal_status(|status| Ok(registrar_sob_status(hub, status))).unwrap_or(false)
}

/// Intervalo em que uma retenção do dono não gravado é tentada de novo, sem esperar outra
/// mudança de status da nuvem.
const REPETIR_DONO: std::time::Duration = std::time::Duration::from_secs(5);

/// Repete a gravação do dono enquanto a saída estiver retida; ao soltar, os apps recebem o status.
pub(crate) fn start(hub: &std::sync::Arc<Hub>) {
    let hub = std::sync::Arc::downgrade(hub);
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(REPETIR_DONO);
        loop {
            tick.tick().await;
            let Some(hub) = hub.upgrade() else { return };
            if hub.workspace.dono_retido() && observar_dono(&hub) {
                crate::remote::cloud::avisar_status(&hub);
            }
        }
    });
}

fn registrar_dono(hub: &Hub, status: &CloudStatus, cortar: bool) -> Result<()> {
    let mut db = hub.store.db.locked();
    let dono = status.owner.as_ref().map(|o| o.user_id.as_str());
    let anterior: Option<String> =
        db.query_row("SELECT v FROM kv WHERE k=?1", [OWNER_SEEN_KEY], |r| r.get(0)).optional()?;
    let novo = dono.filter(|d| anterior.as_deref() != Some(*d));
    let sobe = cortar || (anterior.is_some() && novo.is_some());
    if novo.is_none() && !sobe {
        hub.workspace.publicar_dono(dono);
        return Ok(());
    }
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(novo) = novo {
        tx.execute(
            "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            params![OWNER_SEEN_KEY, novo],
        )?;
    }
    if !sobe {
        tx.commit()?;
        hub.workspace.publicar_dono(dono);
        return Ok(());
    }
    let epoch = bump_on(&tx)?;
    confirmar(hub, tx, epoch, Motivo::OwnerChanged, Dono::Passa(dono.map(str::to_owned)))
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
    #[serde(skip_serializing_if = "Option::is_none")]
    grant_generation: Option<i64>,
    capabilities: Vec<&'static str>,
}

/// O Hub decide; `capabilities` só orienta a interface. Sem vínculo, o papel é `owner`. Com
/// vínculo, `kind` e `workspaceId` vêm dele, e `userId` e `role` do aparelho vêm só da sessão
/// verificada do pedido em curso.
pub(crate) fn contexto(hub: &Hub, caller: &Caller, retrato: &Epoca) -> Result<Contexto> {
    use crate::fork::sessao_clerk::vinculo::{self, Papel, Tipo};
    let estado = vinculo::ler(hub);
    let identidade = match (&estado, caller) {
        (vinculo::Estado::Vinculado(_), Caller::Device { autoridade, .. }) => {
            let guarda = autoridade.as_deref().and_then(|a| a.sessao());
            Some(guarda.map(|g| (g.sub().to_owned(), g.papel())).ok_or(crate::api::devices::Forbidden)?)
        }
        (vinculo::Estado::Ilegivel, Caller::Device { .. }) => return Err(crate::api::devices::Forbidden.into()),
        _ => None,
    };
    let (mut capabilities, grant_generation) = match caller {
        Caller::Local => (vec!["chat", "bots.manage", "devices.manage"], None),
        Caller::Device { scopes, autoridade, .. } => {
            let mut lista = vec![];
            if scopes.contains(&Scope::Control) {
                lista.extend(["chat", "bots.manage"]);
            }
            if scopes.contains(&Scope::OwnerDevices) {
                lista.push("devices.manage");
            }
            (lista, autoridade.as_ref().and_then(|a| a.geracao_capturada()))
        }
        _ => return Err(crate::api::devices::Forbidden.into()),
    };
    if identidade.as_ref().is_some_and(|(_, papel)| *papel == Papel::Membro) {
        capabilities.retain(|c| *c == "chat");
    }
    let (kind, workspace_id, user_id, role) = match &estado {
        vinculo::Estado::Vinculado(v) => {
            let user_id = match (&identidade, &v.tipo) {
                (Some((sub, _)), _) => Some(sub.clone()),
                (None, Tipo::Pessoal { dono }) => Some(dono.clone()),
                (None, Tipo::Time { .. }) => None,
            };
            (v.kind(), v.workspace_id(), user_id, identidade.as_ref().map_or("owner", |(_, papel)| papel.nome()))
        }
        _ => ("personal", workspace_id(hub, retrato), retrato.dono.clone(), "owner"),
    };
    Ok(Contexto {
        schema: 1,
        hub_id: hub.host_id.clone(),
        kind,
        workspace_id,
        user_id,
        role,
        workspace_epoch: retrato.valor,
        grant_generation,
        capabilities,
    })
}

/// O vínculo decide; sem ele, o dono que a nuvem informou (ou `personal:unbound`).
fn workspace_id(hub: &Hub, retrato: &Epoca) -> String {
    if let crate::fork::sessao_clerk::vinculo::Estado::Vinculado(v) = crate::fork::sessao_clerk::vinculo::ler(hub) {
        return v.workspace_id();
    }
    workspace_id_legado(retrato.dono.as_deref())
}

pub(super) fn workspace_id_legado(dono: Option<&str>) -> String {
    dono.map_or_else(|| UNBOUND.to_owned(), |u| format!("personal:{u}"))
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
/// que não é u64 também diverge. A recusa vai para o log com motivo fixo, a origem e o epoch
/// atual; nunca com o valor que veio do cliente.
fn conferir(atual: u64, origem: &'static str, pedido: Option<&Value>) -> Result<()> {
    match pedido.map(Value::as_u64) {
        None => Ok(()),
        Some(Some(pedido)) if pedido == atual => Ok(()),
        Some(_) => {
            tracing::warn!(reason = EPOCH_MISMATCH, origin = origem, epoch = atual, "workspace epoch refused");
            Err(divergente())
        }
    }
}

/// `workspaceEpoch` de uma query string: ausente é aceito; presente e fora de u64 em dígitos
/// diverge (409), como no corpo.
pub(crate) fn epoch_da_query(hub: &Hub, origem: &'static str, bruto: Option<&str>) -> Result<Option<u64>> {
    let Some(bruto) = bruto else { return Ok(None) };
    let valor = bruto.parse::<u64>().ok().filter(|_| bruto.bytes().all(|b| b.is_ascii_digit()));
    conferir(hub.workspace.atual(), origem, Some(&valor.map_or(Value::Null, Value::from)))?;
    Ok(valor)
}

/// Retrato de quem abre um stream: dono e epoch de uma vez, conferidos com o que o cliente conhece.
pub(crate) fn abrir(hub: &Hub, origem: &'static str, pedido: Option<u64>) -> Result<Epoca> {
    let retrato = hub.workspace.retrato();
    conferir(retrato.valor, origem, pedido.map(Value::from).as_ref())?;
    Ok(retrato)
}

/// Confere e retira `workspaceEpoch` do corpo antes de qualquer método ver o pedido.
pub(crate) fn conferir_pedido(hub: &Hub, origem: &'static str, mut body: Value) -> Result<Value> {
    let pedido = body.as_object_mut().and_then(|b| b.remove(EPOCH_FIELD));
    conferir(hub.workspace.atual(), origem, pedido.as_ref())?;
    Ok(body)
}

/// Encerra o stream quando o epoch muda: o evento terminal sai, e nada depois dele. Um
/// evento já produzido mas ainda não entregue (em voo) é descartado se o epoch mudou.
pub(crate) fn cortar<S: Stream<Item = Value> + Send + 'static>(
    hub: &Hub,
    retrato: &Epoca,
    eventos: S,
) -> impl Stream<Item = Value> + Send + 'static + use<S> {
    let (epoch, workspace_id) = (retrato.valor, workspace_id(hub, retrato));
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
                let atual = resolvido(&mut aviso, epoch).await?;
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
                        let atual = resolvido(&mut aviso, epoch).await?;
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

/// O retrato sem aumento pendente, ou já avançado desde `epoch`: o evento terminal leva um número
/// já gravado e não espera outra marca (a retenção do dono, por exemplo).
async fn resolvido(aviso: &mut watch::Receiver<Epoca>, epoch: u64) -> Option<Epoca> {
    loop {
        let atual = aviso.borrow_and_update().clone();
        if !atual.pendente() || atual.avancou(epoch) {
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
