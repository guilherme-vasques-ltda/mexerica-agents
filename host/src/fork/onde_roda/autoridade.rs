use super::tipos::{METHODS, Operation, Phase, Place};
use crate::api::devices::Caller;
use anyhow::{Result, ensure};

#[derive(Clone)]
pub enum Authority {
    Local { hub_id: String },
}
/// Executado antes de permit/cofre/roteamento, inclusive prefixos desconhecidos.
pub fn require(caller: &Caller, method: &str) -> Result<()> {
    ensure!(matches!(caller, Caller::Local), "origin_not_admitted");
    ensure!(METHODS.contains(&method), "workplace_method_unknown");
    Ok(())
}
impl Authority {
    pub fn local(caller: &Caller, hub_id: &str) -> Result<Self> {
        ensure!(matches!(caller, Caller::Local), "origin_not_admitted");
        Ok(Self::Local { hub_id: hub_id.into() })
    }
    pub fn hub_id(&self) -> &str {
        match self {
            Self::Local { hub_id } => hub_id,
        }
    }
    pub fn target(&self, place: &Place) -> Result<()> {
        ensure!(place.hub_id == self.hub_id() && !place.removed, "target_invalidated");
        Ok(())
    }
    pub fn operation(&self, p: &Place, op: &Operation) -> Result<()> {
        self.target(p)?;
        ensure!(
            op.hub_id == self.hub_id()
                && op.place_id == p.id
                && op.authority == "local"
                && op.admission_revision == p.admission_revision
                && op.connection_revision == p.connection_revision,
            "operation_invalidated"
        );
        ensure!(op.phase == Phase::Verificando && op.deadline > crate::store::now_ms(), "operation_not_live");
        Ok(())
    }
}
