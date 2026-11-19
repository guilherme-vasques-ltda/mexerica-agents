// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Background services started alongside the host API.

use crate::{
    agent::{backends, registry},
    hub::Hub,
    remote::relay,
    screen, update, usage, voice,
};
use std::sync::Arc;

pub fn start(hub: &Arc<Hub>, scope: crate::service::mac_local::Scope) {
    tokio::spawn(crate::computers::run(hub.clone()));
    tokio::spawn(registry::refresh_loop());
    tokio::spawn(voice::refresh_loop(hub.clone()));
    let sign_in_hub = hub.clone();
    tokio::spawn(async move { backends::refresh_sign_in(&sign_in_hub.store).await });
    tokio::spawn(usage::poll(hub.clone()));
    tokio::spawn(update::automatic_loop(hub.clone()));
    tokio::spawn(screen::serve_helpers(hub.screen.clone()));
    // Hub local do Mac ("só neste Mac"): nenhuma conexão de saída ao relay.
    spawn_relay_if_allowed(scope, || {
        tokio::spawn(relay::run(hub.clone()));
    });
    #[cfg(target_os = "linux")]
    tokio::spawn(screen::supervise_linux_helper(hub.screen.clone()));
}

/// Único caminho que liga o relay de saída: o Hub "só neste Mac" nunca o inicia.
fn spawn_relay_if_allowed(scope: crate::service::mac_local::Scope, spawn: impl FnOnce()) -> bool {
    let allowed = scope.runs_outbound_relay();
    if allowed {
        spawn();
    }
    allowed
}

#[cfg(test)]
mod tests {
    use super::spawn_relay_if_allowed;
    use crate::service::mac_local::Scope;

    #[test]
    fn this_mac_scope_never_spawns_the_relay() {
        let mut spawned = 0;
        assert!(!spawn_relay_if_allowed(Scope::ThisMacOnly, || spawned += 1));
        assert_eq!(spawned, 0);
        assert!(spawn_relay_if_allowed(Scope::Network, || spawned += 1));
        assert_eq!(spawned, 1);
    }
}
