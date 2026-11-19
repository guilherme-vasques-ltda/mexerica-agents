//! Fundação pessoal do Hub. Não é estado ativo nem autoriza Devices/canais.
use super::{
    clock::Clock,
    provisioning::{AdmittedConfig, RootJournal},
};
use crate::{LockExt, remote::identity::Identity};
use anyhow::{Result, ensure};
use std::sync::Mutex;
#[cfg(test)]
pub(super) type CutSchedule = std::sync::Arc<dyn Fn() + Send + Sync>;
pub(super) struct Foundation {
    pub(super) config: AdmittedConfig,
    pub(super) journal: Option<RootJournal>,
    pub(super) clock: Clock,
    pub(super) applied: Option<Applied>,
    pub(super) socket_epoch: u64,
    pub(super) socket_live: bool,
}
pub(super) struct Applied {
    pub(super) record: super::live::Record,
    pub(super) lease: super::clock::Lease,
    pub(super) accepted: bool,
}
#[derive(Default)]
pub(crate) struct Runtime {
    state: Mutex<Option<Foundation>>,
    pub(super) prepare: Mutex<super::prepare::Gate>,
    pub(super) possession: Mutex<super::channel::Admission>,
    #[cfg(test)]
    pub(super) test_transport: Mutex<Option<super::transport::TestTransport>>,
    #[cfg(test)]
    pub(super) test_journal: Mutex<Option<std::path::PathBuf>>,
    #[cfg(test)]
    pub(super) test_cut_after_false: Mutex<Option<CutSchedule>>,
}
impl Runtime {
    pub(super) fn with<T>(&self, action: impl FnOnce(&mut Foundation) -> Result<T>) -> Result<T> {
        let mut state = self.state.locked();
        action(state.as_mut().ok_or_else(|| anyhow::anyhow!("personal foundation unavailable"))?)
    }
    pub(super) fn refresh(&self) -> Result<()> {
        let cfg = self.with(|s| Ok(s.config.clone()))?;
        // FS fora das guardas/Store. Revalidação monotônica sob trava após a leitura.
        #[cfg(test)]
        let test_path = self.test_journal.locked().clone();
        #[cfg(test)]
        let next = if let Some(path) = test_path {
            RootJournal::read(&cfg, &super::secure_fs::fixture_root(&path, 192 * 1024)?)?
        } else {
            RootJournal::refresh(&cfg, None)?
        };
        #[cfg(not(test))]
        let next = RootJournal::refresh(&cfg, None)?;
        self.with(|s| {
            if let Some(old) = &s.journal {
                next.follows(old)?;
            }
            s.journal = Some(next);
            Ok(())
        })
    }

    pub(super) fn install(&self, config: AdmittedConfig, journal: Option<RootJournal>, clock: Clock) -> Result<()> {
        let mut state = self.state.locked();
        ensure!(state.is_none(), "personal foundation already loaded");
        *state = Some(Foundation { config, journal, clock, applied: None, socket_epoch: 0, socket_live: false });
        Ok(())
    }
    pub(crate) fn configured(&self) -> bool {
        self.state.locked().is_some()
    }
    pub(super) fn time_request(&self, identity: &Identity) -> Result<Vec<u8>> {
        let mut state = self.state.locked();
        let s = state.as_mut().ok_or_else(|| anyhow::anyhow!("personal foundation unavailable"))?;
        s.clock.time_request(&s.config, identity)
    }
    pub(super) fn accept_time(&self, bytes: &[u8]) -> Result<()> {
        let mut state = self.state.locked();
        let s = state.as_mut().ok_or_else(|| anyhow::anyhow!("personal foundation unavailable"))?;
        s.clock.accept_time(&s.config, bytes)
    }
}
