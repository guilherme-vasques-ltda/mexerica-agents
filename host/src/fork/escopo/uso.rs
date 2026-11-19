//! 05a: seam de leitura privada. Só `CalendarRead` → `PrivateView`.
//! D544 retirou o consumidor Agenda. Sem leitura de calendário, grant ou escrita;
//! o adaptador de escopo continua disponível para consumidores futuros.
use super::{ResolvedScope, fontes::ScopeSourceError, resolver};
use crate::{
    fork::{
        atividades::{
            escopo_store::{self, PrivateReadTx},
            modelo::CoreError,
        },
        plugins::private_content::{Destination, Operation},
    },
    hub::Hub,
};

/// Revalida a captura original (Caller, vínculos, sessão, barreira, política) na mesma
/// `StoreTx` da leitura e, para turno ACP, o turno de novo depois do commit, antes de
/// devolver o valor. Sem nova resolução: escopo antigo com fonte mudada é negado.
/// Não chamar dentro de outra Tx, lock do Store ou do runtime.
pub(crate) fn read_private_view<T: 'static>(
    hub: &Hub,
    scope: &ResolvedScope,
    operation: Operation,
    destination: Destination,
    read: impl FnOnce(&mut PrivateReadTx<'_, '_>) -> Result<T, CoreError>,
) -> Result<T, ScopeSourceError> {
    if operation != Operation::CalendarRead || !matches!(destination, Destination::PrivateView) {
        return Err(ScopeSourceError::UseDenied);
    }
    let binding = resolver::binding(&scope.expectation)?;
    let value = escopo_store::personal_read(hub, &scope.caller, &binding, &scope.origin, &scope.sources, read)?;
    resolver::turn_live(hub, &scope.expectation, &scope.origin)?;
    Ok(value)
}
