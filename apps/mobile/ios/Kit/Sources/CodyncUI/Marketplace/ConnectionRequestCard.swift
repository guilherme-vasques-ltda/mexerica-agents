// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import AuthenticationServices
import CodyncKit
import SwiftUI

/// Credentials go through the setup API; only the request's state is a chat entry.
struct ConnectionRequestCard: View {
    let entry: Entry
    let request: ConnectionRequest
    @Environment(BotStore.self) private var model
    @Environment(\.webAuthenticationSession) private var webAuthenticationSession
    @Environment(\.openURL) private var openURL
    @State private var app: ComposioApp?
    @State private var item: MarketConnector?
    @State private var agent: Backend?
    @State private var value = ""
    @State private var username = ""
    @State private var busy = false
    @State private var error: String?

    private var pending: Bool { request.status == "pending" }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 14) {
                logo
                VStack(alignment: .leading, spacing: 2) {
                    Text(request.title).font(.body.weight(.semibold)).foregroundStyle(Palette.text).lineLimit(1)
                    if let text = entry.data.text {
                        Text(text).font(.subheadline).foregroundStyle(Palette.secondary).lineLimit(2)
                    }
                }
                Spacer(minLength: 8)
                trailing
            }
            // The card has no fill, so each field gets one.
            if pending, request.kind == "login" {
                TextField(String(localized: "Usuário ou e-mail", bundle: .module), text: $username)
                    .plainTextInput()
                    .pill(fill: Palette.bubbleAgent)
                SecureField(String(localized: "Senha ou referência op://", bundle: .module), text: $value)
                    .plainTextInput()
                    .pill(fill: Palette.bubbleAgent)
                Text(String(localized: "Salvo com segurança neste computador. O bot digita o valor na página de entrada, mas nunca o vê.", bundle: .module))
                    .font(.caption).foregroundStyle(Palette.secondary)
            } else if pending, request.kind == "secret" {
                Text("\(request.field ?? String(localized: "Credencial", bundle: .module)) · \(request.location ?? "")")
                    .font(.caption).foregroundStyle(Palette.secondary)
                SecureField(String(localized: "Credencial ou referência op://", bundle: .module), text: $value)
                    .plainTextInput()
                    .pill(fill: Palette.bubbleAgent)
                Text(String(localized: "Salvo com segurança neste computador. Nunca é adicionado à conversa.", bundle: .module))
                    .font(.caption).foregroundStyle(Palette.secondary)
            }
            if let error { Text(error).font(.footnote).foregroundStyle(Palette.danger) }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .overlay(RoundedRectangle(cornerRadius: 20, style: .continuous).strokeBorder(Palette.border))
        // A card, not a banner: full width on a phone, capped on wider screens.
        .frame(maxWidth: 400, alignment: .leading)
        .frame(maxWidth: .infinity, alignment: .leading)
        .textFieldStyle(.plain)
        .codyncSheet(item: $item) { item in
            InstallConnectorSheet(item: item, requestId: entry.id) { self.item = nil }
        }
        .codyncSheet(item: $app) { app in
            ComposioConnectSheet(app: app, requestId: entry.id) { self.app = nil }
        }
        .codyncSheet(item: $agent) { AgentSheet(initial: $0, recheck: true) }
        .onDisappear { value = ""; username = "" }
    }

    /// The agent, website or service the request is for.
    @ViewBuilder private var logo: some View {
        switch request.kind {
        case "agent":
            let backend = model.hello?.backends.first { $0.id == request.backend }
            AgentIcon(registry: backend?.registry ?? request.backend, size: 26)
                .frame(width: 46, height: 46)
                .background(Palette.bubbleAgent, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        case "app":
            AppLogo(url: request.logo, name: request.title)
        case "login":
            // A login's site is a domain, or an app's name (no logo to find for that).
            ServiceLogo(website: nil, site: request.site.flatMap { $0.contains(".") ? $0 : nil }, name: request.title)
        default:
            ServiceLogo(website: nil, name: request.title, registryName: request.registryName)
        }
    }

    @ViewBuilder private var trailing: some View {
        if pending {
            HStack(spacing: 6) {
                if busy { Spinner() }
                Button(connectTitle) { connect() }
                    .buttonStyle(PrimaryButtonStyle())
                    .disabled(busy || (["secret", "login"].contains(request.kind) && value.isEmpty))
            }
        } else {
            HStack(spacing: 6) {
                if request.status == "ready" { Image(systemName: "checkmark").fontWeight(.semibold) }
                Text(request.status == "ready" ? doneTitle : String(localized: "Cancelado", bundle: .module))
            }
            .font(.subheadline)
            .foregroundStyle(Palette.secondary)
        }
    }

    private var connectTitle: String {
        switch request.kind {
        case "agent": String(localized: "Entrar", bundle: .module)
        case "login": String(localized: "Salvar", bundle: .module)
        default: String(localized: "Conectar", bundle: .module)
        }
    }

    private var doneTitle: String {
        switch request.kind {
        case "agent": String(localized: "Sessão iniciada", bundle: .module)
        case "login": String(localized: "Salvo", bundle: .module)
        default: String(localized: "Conectado", bundle: .module)
        }
    }

    private func connect() {
        guard let client = model.client else { return }
        busy = true
        error = nil
        Task {
            defer { busy = false }
            do {
                if request.kind == "agent" {
                    // The host completes the card and resends the messages once the sheet finds it signed in.
                    if let backend = model.hello?.backends.first(where: { $0.id == request.backend }) {
                        withAnimation(Motion.layout) { agent = backend }
                    } else {
                        error = String(localized: "Abra \(request.title) em Extensões para entrar.", bundle: .module)
                    }
                } else if request.kind == "app", let toolkit = request.toolkit {
                    withAnimation(Motion.layout) { app = ComposioApp(slug: toolkit, name: request.title) }
                } else if request.kind == "login" {
                    try await client.finishConnectionRequest(entry.id, value: value, username: username)
                    value = ""
                } else if request.kind == "secret" {
                    try await client.finishConnectionRequest(entry.id, value: value)
                    value = ""
                } else if let id = request.connectorId {
                    let connectors = try await client.connectors()
                    if connectors.first(where: { $0.id == id })?.needsSignIn == true {
                        try await ConnectorSignInFlow(model: model, webAuthenticationSession: webAuthenticationSession, openURL: openURL).signIn(id)
                    }
                    try await client.finishConnectionRequest(entry.id, connectorId: id)
                } else if let name = request.registryName {
                    let loaded = try await client.connectorInfo(name)
                    withAnimation(Motion.layout) { item = loaded }
                }
            } catch { self.error = error.localizedDescription }
        }
    }
}

struct CredentialsView: View {
    @Environment(BotStore.self) private var model
    @State private var selected: InstalledConnector?
    @State private var token = ""
    @State private var status: CredentialStatus?
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        VStack(spacing: 0) {
            ModalHeader(String(localized: "Credenciais", bundle: .module)) {}
            CardForm {
                CardSection(String(localized: "Armazenamento", bundle: .module)) {
                    Text(status?.provider ?? String(localized: "Armazenamento de credenciais do sistema", bundle: .module))
                    Text(String(localized: "Criptografado no computador conectado.", bundle: .module))
                        .foregroundStyle(Palette.secondary)
                }
                CardSection(String(localized: "Conexões salvas", bundle: .module)) {
                    ForEach(model.installedConnectors.filter { !$0.keys.isEmpty }) { connector in
                        Button { withAnimation(Motion.layout) { selected = connector } } label: {
                            HStack { Text(connector.name); Spacer(); Image(systemName: "chevron.right") }
                        }
                        .buttonStyle(.plain)
                    }
                    if !model.installedConnectors.contains(where: { !$0.keys.isEmpty }) {
                        Text(String(localized: "Nenhuma credencial de conexão salva ainda.", bundle: .module))
                            .font(.caption).foregroundStyle(Palette.secondary)
                    }
                    Text(String(localized: "Os valores permanecem ocultos. Selecione uma conexão para substituir suas credenciais.", bundle: .module))
                        .font(.caption).foregroundStyle(Palette.secondary)
                }
                LoginsSection()
                CardSection("1Password", footer: String(localized: "Use uma conta de serviço com acesso de leitura apenas a um cofre compartilhado dedicado. A CLI do 1Password deve estar instalada no computador.", bundle: .module)) {
                    Text(status?.onePasswordConnected == true ? String(localized: "Conectado", bundle: .module) : String(localized: "Conectar um cofre compartilhado", bundle: .module))
                    CredentialFieldRow(String(localized: "Token da conta de serviço", bundle: .module)) {
                        SecureField(String(localized: "Cole seu token", bundle: .module), text: $token).plainTextInput()
                    }
                    Button(String(localized: "Conectar", bundle: .module)) { save(token) }.buttonStyle(PrimaryButtonStyle()).disabled(busy || token.isEmpty)
                    if status?.onePasswordConnected == true {
                        Button(String(localized: "Desconectar", bundle: .module)) { save("") }.buttonStyle(SecondaryButtonStyle()).disabled(busy)
                    }
                    Text(String(localized: "Use op://vault/item/field no campo de credencial de um conector. A Mexerica obtém o valor quando o conector é executado.", bundle: .module))
                        .font(.footnote).foregroundStyle(Palette.secondary)
                }
                if busy { Spinner() }
                if let error { Text(error).foregroundStyle(Palette.danger) }
            }
        }
        .background(Palette.background)
        .textFieldStyle(.plain)
        .codyncSheet(item: $selected) { ConnectorCredentialEditor(connector: $0) }
        .task {
            await model.refreshPlugins()
            do { status = try await model.client?.credentialStatus() }
            catch { self.error = error.localizedDescription }
        }
        .onDisappear { token = "" }
    }

    private func save(_ value: String) {
        guard let client = model.client else { return }
        busy = true
        error = nil
        Task {
            defer { busy = false }
            do { status = try await client.setOnePasswordToken(value); token = "" }
            catch { self.error = error.localizedDescription }
        }
    }
}

/// Website and app logins bots type through the computer tool; passwords stay hidden.
private struct LoginsSection: View {
    @Environment(BotStore.self) private var model
    @State private var logins: [SavedLogin] = []
    @State private var site = ""
    @State private var username = ""
    @State private var password = ""
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        CardSection(String(localized: "Entradas", bundle: .module), footer: String(localized: "Os bots digitam esses dados no site ou aplicativo correspondente e nunca veem a senha. Você pode usar uma referência op://.", bundle: .module)) {
            ForEach(logins) { login in
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(login.site)
                        if !login.username.isEmpty { Text(login.username).font(.caption).foregroundStyle(Palette.secondary) }
                    }
                    Spacer()
                    IconButton(String(localized: "Remover", bundle: .module), systemImage: "trash") { remove(login) }
                }
            }
            CredentialFieldRow(String(localized: "Site ou aplicativo", bundle: .module)) {
                TextField("github.com", text: $site).plainTextInput()
            }
            CredentialFieldRow(String(localized: "Usuário ou e-mail", bundle: .module)) {
                TextField("name@example.com", text: $username).plainTextInput()
            }
            CredentialFieldRow(String(localized: "Senha", bundle: .module)) {
                SecureField(String(localized: "Senha ou referência op://", bundle: .module), text: $password).plainTextInput()
            }
            Button(String(localized: "Salvar dados de entrada", bundle: .module)) { save() }
                .buttonStyle(PrimaryButtonStyle())
                .disabled(busy || site.isEmpty || password.isEmpty)
            if let error { Text(error).font(.footnote).foregroundStyle(Palette.danger) }
        }
        .task { await load() }
        .onDisappear { password = "" }
    }

    private func load() async {
        do { logins = try await model.client?.logins() ?? [] } catch { self.error = error.localizedDescription }
    }

    private func save() {
        guard let client = model.client else { return }
        busy = true
        error = nil
        Task {
            defer { busy = false }
            do {
                try await client.saveLogin(site: site, username: username, password: password)
                site = ""; username = ""; password = ""
                await load()
            } catch { self.error = error.localizedDescription }
        }
    }

    private func remove(_ login: SavedLogin) {
        guard let client = model.client else { return }
        Task {
            do {
                try await client.removeLogin(login.id)
                withAnimation(Motion.layout) { logins.removeAll { $0.id == login.id } }
            } catch { self.error = error.localizedDescription }
        }
    }
}

private struct ConnectorCredentialEditor: View {
    let connector: InstalledConnector
    @Environment(BotStore.self) private var model
    @Environment(\.dismissModal) private var dismiss
    @State private var fields: [String: String] = [:]
    @State private var saving = false
    @State private var error: String?

    var body: some View {
        VStack(spacing: 0) {
            ModalHeader(connector.name) {
                if saving { Spinner() }
                else { IconButton(String(localized: "Salvar credenciais", bundle: .module), systemImage: "checkmark") { save() }.disabled(fields.values.allSatisfy(\.isEmpty)) }
            }
            CardForm {
                CardSection(footer: String(localized: "Digite apenas os valores que deseja substituir. Você pode usar uma referência op://.", bundle: .module)) {
                    ForEach(connector.keys, id: \.self) { key in
                        SecureField(key, text: Binding(get: { fields[key] ?? "" }, set: { fields[key] = $0 })).plainTextInput()
                    }
                }
                if let error { Text(error).foregroundStyle(Palette.danger) }
            }
        }
        .background(Palette.background)
        .textFieldStyle(.plain)
        .onDisappear { fields.removeAll() }
    }
    private func save() {
        guard let client = model.client else { return }
        saving = true
        error = nil
        Task {
            defer { saving = false }
            do {
                try await client.updateConnectorCredentials(connector.id, fields: fields.filter { !$0.value.isEmpty })
                fields.removeAll()
                dismiss()
            } catch { self.error = error.localizedDescription }
        }
    }
}

private struct CredentialFieldRow<Content: View>: View {
    let title: String
    let content: Content

    init(_ title: String, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title).font(.caption).foregroundStyle(Palette.secondary)
            content.accessibilityLabel(title)
        }
    }
}
