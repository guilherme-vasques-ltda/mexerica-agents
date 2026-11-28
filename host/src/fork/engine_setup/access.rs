//! SAAS-04: gestão de contas de IA, sem promover Device a Local ou delegar owner.devices.
use crate::{LockExt, api::devices::Caller, hub::Hub, store::Scope};
use anyhow::{Result, bail};
use serde_json::{Value, json};

#[derive(Clone)]
pub(crate) enum Authority {
    Local,
    Owner { key: String, binding: String, transporte: std::sync::Arc<crate::fork::autoridade::Autoridade> },
}
// A identidade da operação SAAS04 continua sendo o vínculo assinado. Cada
// guarda usa seu transporte ORIGINAL; comparar operações não renova sua guarda.
impl PartialEq for Authority {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Local, Self::Local) => true,
            (Self::Owner { key, binding, .. }, Self::Owner { key: other_key, binding: other_binding, .. }) => {
                key == other_key && binding == other_binding
            }
            _ => false,
        }
    }
}
impl Eq for Authority {}
impl Authority {
    /// No await and no nested Store/Cloud calls inside `write`. The original
    /// binding survives queues; owner -> DB serializes it with revocation.
    pub(crate) fn with_db<T>(&self, hub: &Hub, write: impl FnOnce(&rusqlite::Connection) -> Result<T>) -> Result<T> {
        match self {
            Self::Local => {
                let mut db = hub.store.db.locked();
                let tx = db.transaction()?;
                let value = write(&tx)?;
                tx.commit()?;
                Ok(value)
            }
            Self::Owner { transporte, .. } => transporte.with_db(hub, Some(self), write).map_err(|error| {
                if error.is::<crate::fork::autoridade::AutorizacaoEncerrada>() {
                    anyhow::anyhow!("remote_setup_unavailable: vínculo original de contas encerrado.")
                } else {
                    error
                }
            }),
        }
    }

    pub(crate) fn commit_and_admit<T, R>(
        &self,
        hub: &Hub,
        write: impl FnOnce(&rusqlite::Connection) -> Result<T>,
        admit: impl FnOnce(T) -> R,
    ) -> Result<R> {
        match self {
            Self::Owner { transporte, .. } => transporte.commit_and_admit(hub, self, write, admit),
            Self::Local => {
                let mut db = hub.store.db.locked();
                let tx = db.transaction()?;
                let value = write(&tx)?;
                tx.commit()?;
                Ok(admit(value))
            }
        }
    }

    /// Composição c1 na Connection do produtor, conservando o binding SAAS04 original.
    pub(crate) fn conferir_saida(&self, db: &rusqlite::Connection, owner: Option<&str>, synced: bool) -> Result<()> {
        if let Self::Owner { key, binding, .. } = self {
            let owner = owner.ok_or_else(|| anyhow::anyhow!("remote_setup_unavailable: dono encerrado"))?;
            anyhow::ensure!(
                synced
                    && crate::store::bootstrap::engine_binding(db, key, owner, crate::store::now_ms())?.as_deref()
                        == Some(binding),
                "remote_setup_unavailable: vínculo original de contas encerrado"
            );
        }
        Ok(())
    }

    pub(crate) fn persist(&self, hub: &Hub, registry: &crate::fork::engine_accounts::Registry) -> Result<()> {
        self.persist_removing(hub, registry, None, None)
    }
    /// `unused`: a conta removida. Na mesma transação da escrita, a leitura FINAL de referências (padrão, Space e, D372,
    /// seleção efetiva ou próxima das conversas) recusa; só a `Connection` recebida. Com `sessionEngineSet` (que
    /// valida a conta no cadastro desta mesma Connection), uma das duas ordens de commit sempre recusa o outro lado.
    pub(super) fn persist_removing(
        &self,
        hub: &Hub,
        registry: &crate::fork::engine_accounts::Registry,
        slot: Option<&str>,
        unused: Option<&str>,
    ) -> Result<()> {
        let value = serde_json::to_string(registry)?;
        self.with_db(hub, |db| {
            if let Some(account) = unused {
                anyhow::ensure!(
                    crate::fork::engine_accounts::managed::bot_ids_in(db, account)?.is_empty(),
                    "account_in_use: remova os vínculos dos bots primeiro."
                );
                crate::fork::uso_historico::remover_conta(db, account)?;
                crate::fork::teto::preservar_uso_e_remover_historico(db, account)?;
            }
            if let Some(slot) = slot {
                db.execute("DELETE FROM kv WHERE k=?1", [slot])?;
            }
            db.execute(
                "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=?2",
                rusqlite::params![crate::fork::engine_accounts::SLOT, value],
            )?;
            Ok(())
        })?;
        if let Some(account) = unused {
            crate::fork::uso_historico::invalidar_conta(&hub.store, account);
        }
        // MEX-1355: mudança no cadastro tira a função de fundo da parada.
        crate::fork::background_selection::backoff::changed(&hub.store);
        Ok(())
    }
    pub(crate) fn save_bot(&self, hub: &Hub, cfg: &crate::store::BotConfig) -> Result<i64> {
        let revision =
            self.with_db(hub, |db| crate::fork::donna_fixa::como_dono(db, |db| crate::store::bots::save_bot(db, cfg)))?;
        hub.store.guard_cleanup()?;
        Ok(revision)
    }
    pub(crate) fn require_current(&self, hub: &Hub) -> Result<()> {
        self.with_db(hub, |_| Ok(()))
    }
    pub(crate) fn current(&self, hub: &Hub) -> bool {
        self.require_current(hub).is_ok()
    }
}

pub(crate) fn resolve(hub: &Hub, caller: &Caller) -> Result<Authority> {
    if matches!(caller, Caller::Local) {
        return Ok(Authority::Local);
    }
    if let Caller::Device { key, scopes, .. } = caller
        && scopes.contains(&Scope::Control)
        && let Ok(original) = crate::fork::autoridade::Autoridade::capturar(hub, caller)
        && let Some(owner) = original.dono()
        && let Ok(binding) = original.with_db(hub, None, |db| {
            crate::store::bootstrap::engine_binding(db, key, owner, crate::store::now_ms())?
                .ok_or_else(|| anyhow::anyhow!("binding_unavailable"))
        })
    {
        return Ok(Authority::Owner { key: key.clone(), binding, transporte: std::sync::Arc::new(original) });
    }
    bail!("remote_setup_unavailable: este aparelho não tem autorização vigente para gerenciar contas de IA.")
}

pub(crate) fn require(hub: &Hub, caller: &Caller) -> Result<()> {
    resolve(hub, caller).map(|_| ())
}

pub(crate) fn capability(hub: &Hub, caller: &Caller) -> Value {
    json!({"capability":"engine.accounts", "canManage": resolve(hub, caller).is_ok()})
}
