import CodyncKit
import CodyncUI
import SwiftUI

struct ForkWorkspacePicker: View {
    let selection: ForkWorkspaceSelection
    let unavailable: Bool
    let select: (ForkWorkspaceID) -> Void

    var body: some View {
        if !selection.choices.isEmpty, !unavailable {
            CardSection("Workspaces") {
                ForEach(selection.choices, id: \.self) { id in
                    let selected = selection.active == id
                    Button { select(id) } label: {
                        HStack {
                            switch id {
                            case .personal:
                                Label("Pessoal", systemImage: "person")
                            case .team:
                                Label(selection.memberships?.teams.first(where: { $0.id == id })?.name ?? "Teams",
                                      systemImage: "person.2")
                            }
                            Spacer()
                            if selected { Image(systemName: "checkmark").accessibilityHidden(true) }
                        }
                        .frame(minHeight: 44)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityAddTraits(selected ? .isSelected : [])
                }
            }
        }
    }
}

struct ForkWorkspaceStatus: View {
    let selection: ForkWorkspaceSelection?
    let unavailable: Bool

    var body: some View {
        VStack(spacing: 0) {
            HStack { Spacer(); AccountSwitcherButton() }.padding(.horizontal, 16)
            if unavailable {
                EmptyState(title: String(localized: "Workspaces indisponíveis"), systemImage: "wifi.slash",
                           message: String(localized: "Não foi possível conferir seu acesso. A conexão será verificada novamente."))
            } else if selection?.memberships == nil {
                Spacer()
                ProgressView("Verificando workspaces")
                Spacer()
            } else if selection?.active == nil {
                EmptyState(title: String(localized: "Sem acesso a workspaces"), systemImage: "lock",
                           message: String(localized: "Nenhum workspace está disponível para esta conta."))
            } else {
                EmptyState(title: String(localized: "Hub indisponível"), systemImage: "wifi.slash",
                           message: String(localized: "Este workspace ainda não tem uma conexão autorizada com um Hub."))
            }
        }
    }
}
