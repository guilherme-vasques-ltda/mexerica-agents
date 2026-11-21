// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

/// Above the agents while some of them run on Node.js and the computer has none: one place
/// to add Codync's copy for all of them.
struct NodeNotice: View {
    let add: () -> Void
    @Environment(BotStore.self) private var model

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: "shippingbox")
                .font(.body)
                .foregroundStyle(Palette.secondary)
                .padding(.top, 1)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(String(localized: "Node.js não está em \(model.hostName)", bundle: .module)).font(.subheadline.weight(.semibold)).foregroundStyle(Palette.text)
                    Text(String(localized: "Alguns motores precisam dele para funcionar. A Mexerica pode adicionar uma cópia só para eles.", bundle: .module))
                        .font(.caption)
                        .foregroundStyle(Palette.secondary)
                }
                // Compact on a phone: under the text, so the text keeps the row's width.
                Button(String(localized: "Adicionar Node.js", bundle: .module), action: add)
                    .buttonStyle(.plain)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(Palette.onAccent)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 7)
                    .background(Palette.accentFill, in: Capsule())
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .background(Palette.bubbleAgent.opacity(0.45), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        .transition(.opacity)
    }
}

/// Getting an agent ready on the computer: install it, add Node.js when it runs on Node and
/// the computer has none, then sign in. Sign-in options come from the agent itself (ACP),
/// plus Codync's own command for CLIs it knows. Bots are made from New bot, not here.
struct AgentSheet: View {
    let initial: Backend
    /// Opened from a chat's sign-in request, whose agent refused a turn: the agent is asked
    /// again however signed in it looked.
    var recheck = false
    @Environment(BotStore.self) private var model
    @State private var route: SetupRoute?
    @State private var auth: AgentAuth?
    @State private var checking = false
    @State private var authenticating: String?
    @State private var error: String?

    /// Live: an install or sign-in updates the host's list.
    private var backend: Backend { model.hello?.backends.first { $0.id == initial.id } ?? initial }
    private var installed: Bool { backend.installed == true }
    private var curated: Bool { backend.curated == true }
    private var signedIn: Bool? { auth?.signedIn ?? backend.signedIn }

    var body: some View {
        ZStack {
            if let route {
                routed(route).transition(.move(edge: .trailing).combined(with: .opacity))
            } else {
                VStack(spacing: 0) {
                    ModalHeader(backend.name)
                    overview
                }
                .transition(.move(edge: .leading).combined(with: .opacity))
            }
        }
        .frame(minWidth: 0, minHeight: 0)
        // A known "signed in" needs no agent start; everything else asks the agent (which
        // can't start without its Node.js).
        .task { if recheck || backend.signedIn != true, !backend.needsNode { await check() } }
        .onChange(of: route) { old, new in
            // Back from a terminal or key form: see what changed.
            if new == nil, old != nil { Task { await check() } }
        }
    }

    @ViewBuilder private func routed(_ route: SetupRoute) -> some View {
        let back = { withAnimation(Motion.layout) { self.route = nil } }
        switch route {
        case .install: SetupTerminalView(backend: backend, step: .install, back: back)
        case .node: SetupTerminalView(backend: backend, step: .node, back: back)
        case .terminal(let method): SetupTerminalView(backend: backend, step: .login, method: method, back: back)
        case .keys(let method): AgentKeysForm(backend: backend, method: method, saved: auth?.savedEnv ?? [], back: back) { auth = $0 }
        }
    }

    private func go(_ route: SetupRoute) {
        withAnimation(Motion.layout) { self.route = route }
    }

    private var overview: some View {
        CardForm {
            CardSection {
                HStack(spacing: 14) {
                    AgentIcon(registry: backend.registry ?? backend.id, size: 30)
                        .frame(width: 52, height: 52)
                        .background(Palette.bubbleAgent, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                    VStack(alignment: .leading, spacing: 2) {
                        Text(backend.name).font(.title3.weight(.semibold))
                        if let d = backend.description, !d.isEmpty {
                            Text(d).font(.subheadline).foregroundStyle(Palette.secondary).lineLimit(3)
                        }
                    }
                }
                .padding(.vertical, 4)
            }
            if curated {
                CardSection {
                    SetupStepRow(
                        number: 1,
                        title: String(localized: "Instalar", bundle: .module),
                        detail: installed
                            ? String(localized: "Instalado em \(model.hostName).", bundle: .module)
                            : backend.canInstall == true ? String(localized: "Executa o instalador oficial em \(model.hostName).", bundle: .module) : backend.installHint,
                        done: installed,
                        action: installed || backend.canInstall != true ? nil : (String(localized: "Instalar", bundle: .module), { go(.install) })
                    )
                }
            }
            if backend.node != nil {
                CardSection {
                    SetupStepRow(
                        number: curated ? 2 : 1,
                        title: "Node.js",
                        detail: backend.needsNode
                            ? String(localized: "\(backend.name) usa Node.js. A Mexerica adiciona sua própria cópia.", bundle: .module)
                            : String(localized: "Adicionado em \(model.hostName).", bundle: .module),
                        done: !backend.needsNode,
                        action: backend.needsNode ? (String(localized: "Adicionar", bundle: .module), { go(.node) }) : nil
                    )
                }
            }
            signInSection
            if let error {
                CardSection { Text(error).foregroundStyle(Palette.danger).textSelection(.enabled) }
            }
        }
    }

    private var signInSection: some View {
        CardSection(footer: signedIn != true && auth?.methods.contains(where: { $0.kind == .agent }) == true
            ? String(localized: "As entradas pelo navegador abrem em \(model.hostName). Está longe dele? Use a tela remota para concluir.", bundle: .module)
            : nil) {
            HStack(spacing: 0) {
                SetupStepRow(
                    number: 1 + (curated ? 1 : 0) + (backend.node != nil ? 1 : 0),
                    title: String(localized: "Entrar", bundle: .module),
                    detail: statusText,
                    done: signedIn == true,
                    action: nil
                )
                if checking {
                    ThinkingOrb(state: .connecting, size: 16, color: Palette.secondary)
                } else {
                    Button(String(localized: "Verificar novamente", bundle: .module), systemImage: "arrow.clockwise") { Task { await check() } }
                        .labelStyle(.iconOnly)
                        .buttonStyle(.plain)
                        .foregroundStyle(Palette.secondary)
                        .help(String(localized: "Verificar novamente", bundle: .module))
                }
            }
            if signedIn != true, let detail = auth?.detail, !detail.isEmpty {
                Text(detail).font(.footnote).foregroundStyle(Palette.secondary).textSelection(.enabled)
            }
            // Some CLIs drop the current sign-in the moment a new one starts: no options once signed in.
            if signedIn != true, !checking {
                if auth?.login == true {
                    optionRow(String(localized: "Entrar", bundle: .module), detail: String(localized: "Em um terminal em \(model.hostName). Os links abrem aqui.", bundle: .module), icon: "terminal") {
                        go(.terminal(nil))
                    }
                }
                ForEach(auth?.methods ?? []) { m in
                    switch m.kind {
                    case .terminal?:
                        optionRow(m.name, detail: m.description ?? String(localized: "Em um terminal em \(model.hostName).", bundle: .module), icon: "terminal") {
                            go(.terminal(m))
                        }
                    case .envVar?:
                        optionRow(m.name, detail: m.description ?? String(localized: "Salvo somente em \(model.hostName).", bundle: .module), icon: "key") {
                            go(.keys(m))
                        }
                    case .agent?, nil:
                        optionRow(
                            m.name,
                            detail: authenticating == m.id
                                ? String(localized: "Conclua a entrada no navegador em \(model.hostName)…", bundle: .module)
                                : m.description ?? String(localized: "Abre um navegador em \(model.hostName).", bundle: .module),
                            icon: "safari",
                            busy: authenticating == m.id
                        ) {
                            Task { await authenticate(m) }
                        }
                    }
                }
            }
        }
    }

    private var statusText: String {
        if checking { return auth == nil ? String(localized: "Verificando com \(backend.name)… A primeira verificação pode baixá-lo.", bundle: .module) : String(localized: "Verificando…", bundle: .module) }
        switch signedIn {
        case true?: return String(localized: "Sessão iniciada.", bundle: .module)
        case _ where backend.needsNode: return String(localized: "Adicione o Node.js primeiro.", bundle: .module)
        case false?: return String(localized: "Sessão ainda não iniciada.", bundle: .module)
        case nil: return String(localized: "Não foi possível verificar. Pule esta etapa se você já entrou em \(model.hostName).", bundle: .module)
        }
    }

    private func optionRow(_ title: String, detail: String, icon: String, busy: Bool = false, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 12) {
                Image(systemName: icon)
                    .font(.body)
                    .foregroundStyle(Palette.secondary)
                    .frame(width: 26)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.body.weight(.medium)).foregroundStyle(Palette.text)
                    Text(detail).font(.subheadline).foregroundStyle(Palette.secondary).lineLimit(3)
                }
                Spacer(minLength: 8)
                if busy {
                    ThinkingOrb(size: 16, color: Palette.secondary)
                } else {
                    Image(systemName: "chevron.right").font(.caption.weight(.semibold)).foregroundStyle(Palette.tertiary)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(authenticating != nil)
    }

    private func check() async {
        guard let client = model.client, !checking else { return }
        checking = true
        defer { checking = false }
        do {
            auth = try await client.agentAuth(backend.id)
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
        await model.refreshBackends()
    }

    private func authenticate(_ m: AuthMethod) async {
        guard let client = model.client else { return }
        authenticating = m.id
        defer { authenticating = nil }
        do {
            auth = try await client.agentAuthenticate(backend.id, method: m.id)
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
        await model.refreshBackends()
    }
}

private enum SetupRoute: Hashable {
    case install
    case node
    case terminal(AuthMethod?)
    case keys(AuthMethod)
}

/// Keys an agent reads from its environment, kept on the computer.
private struct AgentKeysForm: View {
    let backend: Backend
    let method: AuthMethod
    let saved: [String]
    let back: () -> Void
    let done: (AgentAuth) -> Void
    @Environment(BotStore.self) private var model
    @State private var values: [String: String] = [:]
    @State private var saving = false
    @State private var error: String?

    private var vars: [AuthMethod.Var] { method.vars ?? [] }

    var body: some View {
        VStack(spacing: 0) {
            ScreenHeader {
                BackButton(action: back).keyboardShortcut(.cancelAction)
            } title: {
                Text(method.name).font(.body.weight(.semibold)).foregroundStyle(Palette.text).lineLimit(1)
            } trailing: {
                if saving {
                    Spinner()
                } else {
                    IconButton(String(localized: "Salvar", bundle: .module), systemImage: "checkmark") { save(values.filter { !$0.value.isEmpty }) }
                        .disabled(vars.contains { !$0.optional && (values[$0.name] ?? "").isEmpty && !saved.contains($0.name) })
                }
            }
            form
        }
        .background(Palette.background)
    }

    private var form: some View {
        CardForm {
            VStack(alignment: .leading, spacing: 6) {
                CardSection(method.name) {
                    ForEach(vars, id: \.name) { v in
                        VStack(alignment: .leading, spacing: 4) {
                            let prompt = Text(saved.contains(v.name) ? String(localized: "Salvo (digite para substituir)", bundle: .module) : v.name)
                            Group {
                                if v.secret {
                                    SecureField(v.label, text: binding(v.name), prompt: prompt)
                                } else {
                                    TextField(v.label, text: binding(v.name), prompt: prompt)
                                }
                            }
                            .plainTextInput()
                            Text(v.optional ? String(localized: "\(v.label) (opcional)", bundle: .module) : v.label).font(.caption).foregroundStyle(Palette.secondary)
                        }
                    }
                }
                VStack(alignment: .leading, spacing: 6) {
                    if let d = method.description { Text(d) }
                    Text(String(localized: "Salvo somente em \(model.hostName) e entregue a \(backend.name) quando ele iniciar.", bundle: .module))
                    if let link = method.link, let url = URL(string: link) {
                        WebLink(String(localized: "Obter uma chave", bundle: .module), url: url)
                    }
                }
                .font(.caption)
                .foregroundStyle(Palette.tertiary)
                .padding(.horizontal, 4)
            }
            if !saved.isEmpty {
                CardSection {
                    Button(String(localized: "Remover chaves salvas", bundle: .module), role: .destructive) {
                        save(Dictionary(uniqueKeysWithValues: vars.map { ($0.name, "") }))
                    }
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

    private func binding(_ name: String) -> Binding<String> {
        Binding(get: { values[name] ?? "" }, set: { values[name] = $0 })
    }

    private func save(_ vars: [String: String]) {
        guard let client = model.client else { return }
        saving = true
        Task {
            defer { saving = false }
            do {
                done(try await client.setAgentEnv(backend.id, vars: vars))
                back()
            } catch {
                self.error = error.localizedDescription
            }
        }
    }
}

private struct SetupStepRow: View {
    let number: Int
    let title: String
    let detail: String
    let done: Bool
    let action: (label: String, run: () -> Void)?

    var body: some View {
        HStack(spacing: 12) {
            ZStack {
                Circle().fill(done ? Palette.accentFill : Palette.bubbleAgent)
                if done {
                    Image(systemName: "checkmark").font(.caption.weight(.bold)).foregroundStyle(Palette.onAccent)
                } else {
                    Text("\(number)").font(.caption.weight(.semibold)).foregroundStyle(Palette.secondary)
                }
            }
            .frame(width: 26, height: 26)
            .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.body.weight(.medium)).foregroundStyle(Palette.text)
                Text(detail).font(.subheadline).foregroundStyle(Palette.secondary).textSelection(.enabled)
            }
            Spacer(minLength: 8)
            if let action {
                Button(action.label, action: action.run)
                    .buttonStyle(.plain)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(Palette.text)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 7)
                    .background(Palette.bubbleAgent, in: Capsule())
                    .fixedSize()
            }
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
        .accessibilityValue(done ? String(localized: "Concluído", bundle: .module) : "")
    }
}
