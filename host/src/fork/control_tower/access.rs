use crate::{
    api::devices::{self, Caller, Forbidden},
    hub::Hub,
    store::Scope,
};
use anyhow::Result;

/// O Caller capturado não conserva lease/concessão revogados enquanto a leitura aguardava.
pub(crate) fn require(hub: &Hub, caller: &Caller) -> Result<()> {
    crate::fork::autoridade::conferir_caller(hub, caller)?;
    super::super::bot_editor::authorization::require_owner(caller)?;
    devices::permit(caller, super::METHOD)?;
    match caller {
        Caller::Local => Ok(()),
        Caller::Device { key, .. } => {
            let device = devices::authorize(hub, key).map_err(|_| Forbidden)?;
            if !device.scopes.contains(&Scope::Control) {
                return Err(Forbidden.into());
            }
            Ok(())
        }
        _ => Err(Forbidden.into()),
    }
}

/// Conserva o mesmo retrato de transporte; não recaptura nem renova autoridade.
pub(crate) fn caller_copy(caller: &Caller) -> Caller {
    caller.clone()
}
