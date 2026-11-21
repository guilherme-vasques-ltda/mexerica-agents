// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import AuthenticationServices
import CodyncKit
import SwiftUI

struct CustomConnectorSheet: View {
    private enum Mode: Hashable { case command, url, config }

    @Environment(BotStore.self) private var model
    @Environment(\.dismissModal) private var dismiss
    @Environment(\.webAuthenticationSession) private var webAuthenticationSession
    @Environment(\.openURL) private var openURL
    @State private var name = ""
    @State private var mode = Mode.command
    @State private var target = ""
    @State private var envText = ""
    @State private var config = ""
    @State private var saving = false
    @State private var error: String?

    private var ready: Bool {
        mode == .config
            ? !config.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            : !name.trimmingCharacters(in: .whitespaces).isEmpty && !target.trimmingCharacters(in: .whitespaces).isEmpty
    }

    var body: some View {
        VStack(spacing: 0) {
            ModalHeader(String(localized: "Conector personalizado", bundle: .module)) {
                if saving {
                    Spinner()
                } else {
                    IconButton(String(localized: "Adicionar", bundle: .module), systemImage: "plus") { save() }
                        .disabled(!ready)
                }
            }
            form
        }
        .background(Palette.background)
    }

    private var footer: String {
        switch mode {
        case .command: String(localized: "Executa em \(model.hostName), na pasta de projeto do bot. Aspas funcionam como em um terminal.", bundle: .module)
        case .url: String(localized: "Um servidor MCP remoto, com HTTP por streaming ou SSE.", bundle: .module)
        case .config: String(localized: "Cole a configuração MCP de um README ou outro aplicativo (Claude, Cursor, VS Code). Todos os servidores nela serão adicionados. Salvo somente em \(model.hostName).", bundle: .module)
        }
    }

    private var form: some View {
        CardForm {
            CardSection(footer: footer) {
                SegmentedChoice(selection: $mode, options: [(id: .command, label: String(localized: "Comando", bundle: .module)), (id: .url, label: String(localized: "URL", bundle: .module)), (id: .config, label: String(localized: "Config.", bundle: .module))])
                if mode == .config {
                    TextField("{ \"mcpServers\": { … } }", text: $config, axis: .vertical)
                        .lineLimit(6...16)
                        .font(.callout.monospaced())
                        .plainTextInput()
                } else {
                    TextField(String(localized: "Nome", bundle: .module), text: $name)
                    TextField(mode == .url ? "https://example.com/mcp" : "npx -y @scope/server", text: $target)
                        .font(.callout.monospaced())
                        .plainTextInput()
                }
            }
            if mode == .url {
                CardSection(String(localized: "Cabeçalhos", bundle: .module), footer: String(localized: "Opcional, um Nome: valor por linha. Deixe vazio se o serviço pedir entrada. Salvo somente em \(model.hostName).", bundle: .module)) {
                    TextField("Authorization: Bearer …", text: $envText, axis: .vertical)
                        .lineLimit(2...6)
                        .font(.callout.monospaced())
                        .plainTextInput()
                }
            } else if mode == .command {
                CardSection(String(localized: "Ambiente", bundle: .module), footer: String(localized: "Um CHAVE=valor por linha. Salvo somente em \(model.hostName).", bundle: .module)) {
                    TextField("API_KEY=…", text: $envText, axis: .vertical)
                        .lineLimit(2...6)
                        .font(.callout.monospaced())
                        .plainTextInput()
                }
            }
            if let error {
                CardSection { Text(error).foregroundStyle(Palette.danger) }
            }
        }
        .textFieldStyle(.plain)
        .animation(Motion.layout, value: mode)
    }

    private func save() {
        saving = true
        error = nil
        let remote = mode == .url
        var pairs: [String: String] = [:]
        for line in envText.split(separator: "\n") {
            let parts = line.split(separator: remote ? ":" : "=", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
            if parts.count == 2, !parts[0].isEmpty { pairs[parts[0]] = parts[1] }
        }
        Task {
            do {
                let added = mode == .config
                    ? try await model.importConnectors(config: config)
                    : [try await model.addConnector(
                        name: name, command: remote ? nil : target, url: remote ? target : nil,
                        env: remote ? [:] : pairs, headers: remote ? pairs : [:]
                    )]
                let signIn = ConnectorSignInFlow(model: model, webAuthenticationSession: webAuthenticationSession, openURL: openURL)
                for c in added where c.needsSignIn {
                    try await signIn.signIn(c.id)
                }
                dismiss()
            } catch {
                self.error = error.localizedDescription
            }
            saving = false
        }
    }
}
