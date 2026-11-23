// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import CodyncUI
import SwiftUI

extension BotStore {
    /// One short line for a computer's connection, used next to its name.
    var statusText: String {
        switch shownConnection {
        case .online where mismatch != nil: String(localized: "Precisa atualizar")
        case .online: String(localized: "Online")
        case .connecting: String(localized: "Conectando…")
        case let .computerOffline(lastSeen): lastSeen.map { String(localized: "Hub indisponível · visto \(RelativeTime.day($0))") } ?? String(localized: "Hub indisponível")
        case .offline: String(localized: "Hub indisponível")
        case let .unauthorized(message): message
        case .unpaired: String(localized: "Não pareado")
        }
    }
}

extension AccountStore {
    /// A computer's status line; one that's offline while its new identity is around says so.
    func statusText(_ store: BotStore) -> String {
        if case .unauthorized = store.connection { return store.statusText }
        return isStale(store.computer.id) ? String(localized: "Cópia antiga deste computador · \(store.statusText)") : store.statusText
    }
}

/// How the phone reaches a computer right now: a subtle icon, direct Wi-Fi/LAN or the encrypted relay.
struct RouteIcon: View {
    let route: HostRoute?

    var body: some View {
        switch route {
        case .direct:
            Image(systemName: "wifi").accessibilityLabel("Wi-Fi ou Tailscale")
        case .relay:
            Image(systemName: "cloud").accessibilityLabel("Pela nuvem")
        case nil:
            EmptyView()
        }
    }
}
