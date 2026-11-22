// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

// Apps through Composio (host/src/market/composio.rs): set up a key once, connect
// apps with Composio's hosted sign-in, then turn them on per bot like any
// other connector.

/// The Marketplace's "Apps" section.
struct ComposioSection: View {
    let query: String
    /// Bumped by the Marketplace when the user submits a search.
    let searchToken: Int
    @Environment(BotStore.self) private var model
    @State private var status: ComposioStatus?
    @State private var apps = MarketplacePage<ComposioApp>()
    @State private var loadRequest = UUID()
    @State private var loading = true
    @State private var error: String?
    @State private var settingUp = false
    @State private var connecting: ComposioApp?

    var body: some View {
        MarketSection(title: String(localized: "Aplicativos", bundle: .module)) {
            VStack(alignment: .leading, spacing: 10) {
                if status?.configured != true {
                    ItemGrid {
                        MarketRow(
                            title: "Composio",
                            subtitle: String(localized: "Gmail, Slack, GitHub, Notion e mais de 1.000 aplicativos", bundle: .module),
                            added: false,
                            busy: status == nil && loading,
                            addLabel: String(localized: "Configurar", bundle: .module)
                        ) {
                            TileIcon(systemName: "square.grid.3x3")
                        } add: {
                            withAnimation(Motion.layout) { settingUp = true }
                        }
                    }
                } else if loading {
                    SkeletonGrid()
                } else if apps.isEmpty {
                    Text(query.isEmpty ? String(localized: "Não foi possível carregar os aplicativos.", bundle: .module) : String(localized: "Nenhum aplicativo corresponde a “\(query)”.", bundle: .module))
                        .foregroundStyle(Palette.secondary)
                } else {
                    ItemGrid {
                        ForEach(apps.visible) { app in
                            MarketRow(
                                title: app.name,
                                subtitle: app.description ?? app.slug,
                                added: false,
                                addLabel: app.connected ? "Conectar outra" : "Conectar"
                            ) {
                                AppLogo(url: app.logo, name: app.name)
                            } add: {
                                withAnimation(Motion.layout) { connecting = app }
                            }
                        }
                    }
                }
                if !loading, apps.hasHiddenItems, status?.configured == true {
                    Button(String(localized: "Carregar mais aplicativos", bundle: .module)) {
                        withAnimation(Motion.fade) { apps.revealMore() }
                    }
                    .buttonStyle(SecondaryButtonStyle())
                    .frame(maxWidth: .infinity)
                }
                if let error {
                    Text(error).font(.footnote).foregroundStyle(Palette.danger)
                }
                if status?.configured == true {
                    Button(String(localized: "Ajustes do Composio", bundle: .module), systemImage: "key") { withAnimation(Motion.layout) { settingUp = true } }
                        .labelStyle(.titleAndIcon)
                        .buttonStyle(.plain)
                        .font(.footnote)
                        .foregroundStyle(Palette.tertiary)
                }
            }
        }
        .task(id: searchToken) { await load() }
        .codyncSheet(isPresented: $settingUp) {
            ComposioKeySheet(status: status) { new in
                status = new
                Task { await load() }
            }
        }
        .codyncSheet(item: $connecting) { app in
            ComposioConnectSheet(app: app) { Task { await load(); await model.refreshPlugins() } }
        }
    }

    private func load() async {
        guard let client = model.client else { return }
        let request = UUID()
        loadRequest = request
        let search = query
        loading = true
        defer { if loadRequest == request { loading = false } }
        do {
            let s = try await client.composioStatus()
            let page = s.configured ? try await client.composioApps(search: search) : []
            guard !Task.isCancelled, loadRequest == request else { return }
            status = s
            apps.replace(with: page)
            error = nil
        } catch {
            guard !Task.isCancelled, loadRequest == request else { return }
            self.error = error.localizedDescription
        }
    }

}

/// An app's logo from Composio, a letter tile until it loads.
struct AppLogo: View {
    let url: String?
    let name: String
    var size: CGFloat = 46

    var body: some View {
        AsyncImage(url: url.flatMap(URL.init(string:))) { image in
            image.resizable().scaledToFit().padding(size * 0.18)
        } placeholder: {
            Text(name.prefix(1).uppercased())
                .font(.system(size: size * 0.4, weight: .semibold))
                .foregroundStyle(Palette.secondary)
        }
        .frame(width: size, height: size)
        .background(Palette.bubbleAgent, in: RoundedRectangle(cornerRadius: size * 0.28, style: .continuous))
        .accessibilityHidden(true)
    }
}

/// Paste (or remove) the Composio API key; the host checks it with Composio first.
struct ComposioKeySheet: View {
    let status: ComposioStatus?
    let done: (ComposioStatus) -> Void
    @Environment(BotStore.self) private var model
    @Environment(\.dismissModal) private var dismiss
    @State private var key = ""
    @State private var saving = false
    @State private var error: String?

    var body: some View {
        VStack(spacing: 0) {
            ModalHeader("Composio") {
                if saving {
                    Spinner()
                } else {
                    IconButton(String(localized: "Salvar", bundle: .module), systemImage: "checkmark") { save(key) }.disabled(key.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            form
        }
        .background(Palette.background)
    }

    private var form: some View {
        CardForm {
            CardSection(
                String(localized: "Chave de API do Composio", bundle: .module),
                footer: String(localized: "O Composio conecta aplicativos como Gmail e Slack e executa suas ferramentas para seus bots. A chave fica em \(model.hostName).", bundle: .module)
            ) {
                SecureField(String(localized: "Chave de API", bundle: .module), text: $key, prompt: Text(status?.configured == true ? String(localized: "Salva (cole para substituir)", bundle: .module) : "ak_…"))
                    .plainTextInput()
                if let url = URL(string: status?.keyUrl ?? "https://platform.composio.dev") {
                    WebLink(String(localized: "Obter uma chave do Composio", bundle: .module), url: url).font(.footnote)
                }
            }
            if status?.configured == true {
                CardSection(footer: String(localized: "Os bots perdem os aplicativos conectados até você adicionar uma chave novamente.", bundle: .module)) {
                    Button(String(localized: "Remover chave", bundle: .module), role: .destructive) { save("") }
                        .buttonStyle(.plain)
                        .foregroundStyle(Palette.danger)
                }
            }
            if let error {
                CardSection { Text(error).foregroundStyle(Palette.danger) }
            }
        }
        .textFieldStyle(.plain)
    }

    private func save(_ key: String) {
        guard let client = model.client else { return }
        saving = true
        Task {
            defer { saving = false }
            do {
                done(try await client.setComposioKey(key))
                dismiss()
            } catch {
                self.error = error.localizedDescription
            }
        }
    }
}

/// Connects one app: Composio's sign-in page (opened here, finished anywhere),
/// or a form for apps that use a key.
struct ComposioConnectSheet: View {
    let app: ComposioApp
    var requestId: String? = nil
    let done: () -> Void
    @Environment(BotStore.self) private var model
    @Environment(\.dismissModal) private var dismiss
    @Environment(\.openURL) private var openURL
    @State private var plan: ComposioConnect?
    @State private var values: [String: String] = [:]
    @State private var connected = false
    @State private var waiting = false
    @State private var saving = false
    @State private var error: String?

    var body: some View {
        VStack(spacing: 0) {
            ModalHeader(String(localized: "Conectar \(app.name)", bundle: .module)) {
                if connected {
                    IconButton(String(localized: "Concluído", bundle: .module), systemImage: "checkmark") { dismiss() }
                } else if plan?.status == "needsFields" {
                    if saving {
                        Spinner()
                    } else {
                        IconButton(String(localized: "Conectar", bundle: .module), systemImage: "checkmark") { submit() }
                            .disabled(plan?.fields?.contains { $0.required && (values[$0.name] ?? "").isEmpty } ?? true)
                    }
                }
            }
            form
        }
        .background(Palette.background)
    }

    private var form: some View {
        CardForm {
            CardSection {
                HStack(spacing: 14) {
                    AppLogo(url: app.logo, name: app.name, size: 52)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(app.name).font(.title3.weight(.semibold))
                        if let d = app.description {
                            Text(d).font(.subheadline).foregroundStyle(Palette.secondary).lineLimit(3)
                        }
                    }
                }
                .padding(.vertical, 4)
            }
            if connected {
                CardSection(footer: String(localized: "Ative \(app.name) para um bot nos ajustes dele, em Conectores.", bundle: .module)) {
                    Label(String(localized: "Conectado", bundle: .module), systemImage: "checkmark.circle.fill").foregroundStyle(Palette.added)
                }
            } else if let plan, plan.status == "needsFields" {
                CardSection(footer: String(localized: "Enviado ao Composio, que o guarda para \(app.name).", bundle: .module)) {
                    ForEach(plan.fields ?? [], id: \.name) { f in
                        VStack(alignment: .leading, spacing: 4) {
                            Group {
                                if f.secret {
                                    SecureField(f.label, text: binding(f.name))
                                } else {
                                    TextField(f.label, text: binding(f.name))
                                }
                            }
                            .plainTextInput()
                            if let d = f.description {
                                Text(f.required ? d : String(localized: "\(d) (opcional)", bundle: .module)).font(.caption).foregroundStyle(Palette.secondary)
                            }
                        }
                    }
                }
            } else if let plan, plan.status == "redirect" {
                CardSection(footer: String(localized: "Entre na página que foi aberta. Isto será atualizado automaticamente quando você terminar.", bundle: .module)) {
                    if let url = plan.url.flatMap(URL.init(string:)) {
                        Button(String(localized: "Abrir a página de entrada", bundle: .module), systemImage: "safari") { openURL(url) }
                            .buttonStyle(.secondary)
                    }
                    HStack(spacing: 8) {
                        Spinner()
                        Text(String(localized: "Aguardando você terminar de entrar…", bundle: .module)).foregroundStyle(Palette.secondary)
                    }
                }
            } else if error == nil {
                CardSection {
                    HStack(spacing: 8) {
                        Spinner()
                        Text(String(localized: "Consultando Composio…", bundle: .module)).foregroundStyle(Palette.secondary)
                    }
                }
            }
            if let error {
                CardSection { Text(error).foregroundStyle(Palette.danger).textSelection(.enabled) }
            }
        }
        .textFieldStyle(.plain)
        .task { await start() }
        .onDisappear { values.removeAll(); if connected { done() } }
    }

    private func binding(_ name: String) -> Binding<String> {
        Binding(get: { values[name] ?? "" }, set: { values[name] = $0 })
    }

    private func start() async {
        guard let client = model.client else { return }
        do {
            let plan = try await client.composioConnect(app.slug)
            self.plan = plan
            guard plan.status == "redirect", let id = plan.connection else { return }
            if let url = plan.url.flatMap(URL.init(string:)) { openURL(url) }
            await wait(for: id, client: client)
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// Polls until the sign-in finishes (or the sheet goes away).
    private func wait(for id: String, client: HostClient) async {
        waiting = true
        defer { waiting = false }
        for _ in 0..<300 {
            try? await Task.sleep(for: .seconds(2))
            if Task.isCancelled { return }
            guard let state = try? await client.composioConnection(id) else { continue }
            switch state.status {
            case "active":
                if let requestId {
                    do { try await client.finishConnectionRequest(requestId, connectorId: state.connectorId) }
                    catch { self.error = error.localizedDescription; return }
                }
                values.removeAll()
                connected = true
                return
            case "failed", "expired":
                error = String(localized: "Não foi possível entrar em \(app.name) (\(state.status)). Feche e tente novamente.", bundle: .module)
                return
            default:
                continue
            }
        }
        error = String(localized: "O tempo de espera para entrar terminou.", bundle: .module)
    }

    private func submit() {
        guard let client = model.client, let mode = plan?.mode else { return }
        saving = true
        Task {
            defer { saving = false }
            do {
                let state = try await client.composioConnect(app.slug, mode: mode, fields: values.filter { !$0.value.isEmpty })
                if state.status == "active" {
                    if let requestId { try await client.finishConnectionRequest(requestId, connectorId: state.connectorId) }
                    values.removeAll()
                    connected = true
                } else {
                    error = String(localized: "\(app.name) informa que a conexão está \(state.status).", bundle: .module)
                }
            } catch {
                self.error = error.localizedDescription
            }
        }
    }
}
