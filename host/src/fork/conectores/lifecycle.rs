//! Posse de handles por sessão ACP. Nenhum vínculo é autorização humana.
#[cfg(test)]
use super::proxy;
use crate::LockExt;
use crate::{hub::Hub, store::Lane};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Default)]
pub(crate) struct Pool {
    sessions: HashMap<String, Definitions>,
    failed: Arc<AtomicBool>,
}
impl Pool {
    pub(crate) fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
    pub(crate) fn clear_failed(&self) {
        self.failed.store(false, Ordering::Release);
    }
    pub(crate) fn check(&self, hub: &Hub, sid: &str) -> Result<()> {
        if let Some(definitions) = self.sessions.get(sid) {
            let channels: Vec<_> = {
                let map = hub.conectores.channels.locked();
                definitions.handles.iter().map(|id| (id, map.get(id).cloned())).collect()
            };
            // Apenas os IDs originais e o vínculo imutável; sem runtime/DB/fonte.
            if !channels
                .into_iter()
                .all(|(id, channel)| channel.is_some_and(|channel| channel.id == *id && channel.live(sid)))
            {
                self.failed.store(true, Ordering::Release);
                bail!("A conexão foi encerrada. Confira o recibo antes de outra ação.");
            }
        }
        Ok(())
    }
    pub(crate) fn insert(&mut self, sid: &str, definitions: Definitions) {
        if !definitions.handles.is_empty() {
            self.sessions.insert(sid.into(), definitions);
        }
    }
    pub(crate) fn release(&mut self, sid: &str) -> bool {
        self.sessions.remove(sid).is_some()
    }
}

#[derive(Default)]
pub(crate) struct Definitions {
    pub(crate) servers: Vec<Value>,
    hub: Weak<Hub>,
    handles: Vec<String>,
    failed: Arc<AtomicBool>,
    committed: bool,
}
impl Definitions {
    pub(super) fn new(hub: &Arc<Hub>, bot: &str, exe: &Path, lane: &Lane, pool: &Pool) -> Result<Self> {
        let mut this = Self {
            servers: Vec::new(),
            hub: Arc::downgrade(hub),
            handles: Vec::new(),
            failed: pool.failed.clone(),
            committed: false,
        };
        for instance in super::selected(hub, bot) {
            let id = super::register(hub, bot, &instance, lane)?;
            this.servers.push(json!({"name":format!("donna-curado-{instance}"),"command":exe,
                "args":["mcp","curated","--context",id,"--port",hub.bridge_port().to_string()]}));
            this.handles.push(id);
        }
        Ok(this)
    }
    /// O ator fornece o sid real; bind exige exposição ACP já persistida.
    pub(crate) fn bind(mut self, hub: &Hub, sid: &str) -> Result<Self> {
        for id in &self.handles {
            hub.conectores.channel(id)?.bind(hub, sid)?;
        }
        self.committed = true;
        Ok(self)
    }
}
impl Drop for Definitions {
    fn drop(&mut self) {
        if !self.committed && !self.handles.is_empty() {
            self.failed.store(true, Ordering::Release);
        }
        if let Some(hub) = self.hub.upgrade() {
            for id in &self.handles {
                if hub.conectores.release(&hub, id).is_err() {
                    tracing::error!("Falha ao encerrar canal curado; startup preserva o ledger");
                }
            }
        }
    }
}

/// Retorna somente handles vivos do dono; não consulta nem promove conteúdo de RPC.
#[cfg(test)]
pub(super) fn handles(hub: &Hub) -> Vec<Arc<proxy::Channel>> {
    hub.conectores.channels.locked().values().cloned().collect()
}
