// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

/// The roster's only connection surface: a quiet summary, with details and filters on demand.
public struct ComputerFilterHeader: View {
    let accounts: AccountStore
    @Binding var hidden: String
    let manage: () -> Void
    let compact: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    public init(accounts: AccountStore, hidden: Binding<String>, compact: Bool = false, manage: @escaping () -> Void) {
        self.accounts = accounts
        _hidden = hidden
        self.manage = manage
        self.compact = compact
    }

    private var selection: ComputerSelection {
        ComputerSelection(all: accounts.computers.map(\.id), hidden: hidden)
    }

    private var stores: [BotStore] { selection.shown.compactMap { accounts.store(for: $0) } }
    private var summary: String { ConnectionSummary(connections: stores.map(\.shownConnection)).text }

    public var body: some View {
        Menu {
            Button(String(localized: "Todos os computadores", bundle: .module), systemImage: selection.shown.count == accounts.computers.count ? "checkmark" : "desktopcomputer") { update("") }
            ForEach(accounts.computers) { computer in
                Button {
                    update(selection.toggling(computer.id))
                } label: {
                    Label(computerTitle(computer), systemImage: selection.shown.contains(computer.id) ? "checkmark" : "desktopcomputer")
                }
                .menuActionDismissBehavior(.disabled)
            }
            if accounts.computers.count > 1 {
                Menu(String(localized: "Mostrar somente", bundle: .module)) {
                    ForEach(accounts.computers) { computer in
                        Button(computer.name) { update(selection.only(computer.id)) }
                    }
                }
            }
            Divider()
            Button(String(localized: "Reconectar", bundle: .module), systemImage: "arrow.clockwise", action: reconnect)
                .disabled(stores.isEmpty)
            Button(String(localized: "Gerenciar computadores", bundle: .module), systemImage: "desktopcomputer", action: manage)
        } label: { label }
        .accessibilityLabel(Text("Computadores, \(summary). Filtrar conversas", bundle: .module))
    }

    private var label: some View {
        HStack(spacing: 4) {
            if compact {
                Image(systemName: "desktopcomputer")
            } else {
                Text(summary).lineLimit(1).contentTransition(.opacity)
            }
            Image(systemName: "chevron.down").font(.system(size: 8, weight: .semibold))
        }
        .font(.system(size: 12, weight: .medium))
        .foregroundStyle(Palette.secondary)
        .help(summary)
        .padding(.vertical, 6)
        .contentShape(Rectangle())
        .animation(Motion.reduced(Motion.fade, reduceMotion), value: summary)
    }

    private func computerTitle(_ computer: Computer) -> String {
        guard let store = accounts.store(for: computer.id) else { return computer.name }
        return "\(store.hostName) · \(store.connectionLabel)"
    }

    private func update(_ value: String) {
        withAnimation(Motion.reduced(Motion.layout, reduceMotion)) { hidden = value }
    }

    private func reconnect() { for store in stores { store.restartStream() } }
}

public extension BotStore {
    var connectionLabel: String {
        switch shownConnection {
        case .online where mismatch != nil: String(localized: "Precisa atualizar", bundle: .module)
        case .online: String(localized: "Conectado", bundle: .module)
        case .connecting: String(localized: "Conectando…", bundle: .module)
        case .computerOffline, .offline: String(localized: "Hub indisponível", bundle: .module)
        case .unauthorized: String(localized: "Sem acesso", bundle: .module)
        case .unpaired: String(localized: "Não pareado", bundle: .module)
        }
    }
}
