// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import AuthenticationServices
import CodyncKit
import SwiftUI

struct InstallConnectorSheet: View {
    let item: MarketConnector
    var requestId: String? = nil
    let done: () -> Void
    @Environment(BotStore.self) private var model
    @Environment(\.dismissModal) private var dismiss
    @Environment(\.webAuthenticationSession) private var webAuthenticationSession
    @Environment(\.openURL) private var openURL
    @State private var optionId = ""
    @State private var values: [String: String] = [:]
    @State private var installed: InstalledConnector?
    @State private var started = false
    @State private var saving = false
    @State private var error: String?

    private var option: MarketConnector.InstallOption? {
        item.options.first { $0.id == optionId } ?? item.options.first
    }

    var body: some View {
        VStack(spacing: 0) {
            ModalHeader(String(localized: "Adicionar conector", bundle: .module)) {
                if saving {
                    Spinner()
                } else {
                    IconButton(String(localized: "Adicionar", bundle: .module), systemImage: "plus") { install() }
                        .disabled(installed == nil && (option?.inputs.contains { $0.required && (values[$0.name] ?? $0.default ?? "").isEmpty } ?? true))
                }
            }
            form
        }
        .background(Palette.background)
        .task {
            guard !started else { return }
            started = true
            if item.options.count == 1 && item.options[0].inputs.isEmpty { install() }
        }
        .onDisappear { values.removeAll() }
    }

    private var form: some View {
        CardForm {
            CardSection {
                VStack(alignment: .leading, spacing: 6) {
                    Text(item.title).font(.title3.weight(.semibold))
                    if let d = item.description { Text(d).foregroundStyle(Palette.secondary) }
                    if let w = item.website, let url = URL(string: w) {
                        WebLink(w, url: url).font(.footnote).lineLimit(1)
                    }
                }
                .padding(.vertical, 4)
            }
            if item.options.count > 1 {
                CardSection(String(localized: "Executa", bundle: .module)) {
                    ChoiceList(selection: $optionId, options: item.options.map { o in
                        (id: o.id, label: o.kind == "remote" ? String(localized: "Hospedado por \(item.title)", bundle: .module) : String(localized: "Em \(model.hostName) (\(o.kind))", bundle: .module), detail: nil)
                    })
                }
            }
            if let option {
                CardSection(String(localized: "Configuração", bundle: .module), footer: String(localized: "As chaves são salvas somente em \(model.hostName).", bundle: .module)) {
                    if option.inputs.isEmpty {
                        Text(option.kind == "remote"
                            ? String(localized: "Não são necessárias chaves aqui. Se \(item.title) pedir entrada, a página de acesso será aberta em seguida.", bundle: .module)
                            : String(localized: "Nenhuma configuração necessária.", bundle: .module))
                            .foregroundStyle(Palette.secondary)
                    }
                    ForEach(option.inputs) { input in
                        VStack(alignment: .leading, spacing: 4) {
                            Group {
                                if input.secret {
                                    SecureField(input.name, text: binding(input))
                                } else {
                                    TextField(input.name, text: binding(input), prompt: Text(input.default ?? input.placeholder ?? input.name))
                                }
                            }
                            .plainTextInput()
                            if let d = input.description {
                                Text(input.required ? d : String(localized: "\(d) (opcional)", bundle: .module)).font(.caption).foregroundStyle(Palette.secondary)
                            }
                        }
                    }
                }
            }
            if let error {
                CardSection { Text(error).foregroundStyle(Palette.danger) }
            }
        }
        .textFieldStyle(.plain)
        .onAppear { optionId = item.options.first?.id ?? "" }
    }

    private func binding(_ input: MarketConnector.Input) -> Binding<String> {
        Binding(get: { values[input.name] ?? "" }, set: { values[input.name] = $0 })
    }

    private func install() {
        guard let option else { return }
        saving = true
        error = nil
        Task {
            do {
                guard let client = model.client else { return }
                let c: InstalledConnector
                if let installed { c = try await client.connectors().first(where: { $0.id == installed.id }) ?? installed }
                else {
                    c = try await model.installConnector(item, option: option.id, inputs: values)
                    installed = c
                    values.removeAll()
                }
                if c.needsSignIn {
                    try await ConnectorSignInFlow(model: model, webAuthenticationSession: webAuthenticationSession, openURL: openURL).signIn(c.id)
                }
                if let requestId {
                    try await client.finishConnectionRequest(requestId, connectorId: c.id)
                } else {
                    try await client.verifyConnector(c.id)
                }
                await model.refreshPlugins()
                done()
                dismiss()
            } catch {
                self.error = error.localizedDescription
            }
            saving = false
        }
    }
}
