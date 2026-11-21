// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import CodyncUI
import SwiftUI

/// Every computer this iPhone can use or ask for — paired here and in the account, merged by
/// computer ID — plus settings for the selected account.
struct SettingsView: View {
    /// Pushed inside Accounts rather than opened as its own sheet.
    var pushed = false
    @Environment(AppStore.self) private var app
    @Environment(AccountStore.self) private var accounts
    @Environment(\.dismissModal) private var dismissModal
    @Environment(\.openURL) private var openURL
    @State private var notificationsAllowed: Bool?
    @AppStorage("notificationsEnabled") private var notificationsEnabled = true
    @AppStorage(Analytics.enabledKey) private var analyticsEnabled = true
    @State private var addingComputer = false
    @State private var confirmForget: Computer?
    @State private var confirmRevoke: CloudComputer?
    @State private var access: AccessTarget?
    @State private var confirmStartOver = false
    @State private var confirmRemoveCopy: CloudComputer?
    @State private var voice: VoiceTarget?
    @State private var aboutAnalytics = false
    #if DEBUG
    /// "Novo visual (teste)": só em Debug/DevDebug; em Release nada disto existe.
    @AppStorage(MXNovoVisual.chave) private var novoVisual = false
    #endif

    var body: some View {
        CardForm {
            CardSection(String(localized: "Computadores"), footer: app.account.isSignedIn
                        ? String(localized: "Os computadores da sua conta precisam da sua aprovação no computador antes de este iPhone usá-los.")
                        : String(localized: "Cada bot funciona em seu próprio computador. Entre para ver os computadores da sua conta.")) {
                ForEach(accounts.computers) { computer in
                    if let store = accounts.store(for: computer.id) {
                        ComputerRow(store: store, inAccount: cloudComputer(computer.id),
                                    openScreen: { openScreen(store) },
                                    openMarketplace: { closeSheets { app.marketplace = store.computer.id } },
                                    revoke: cloudComputer(computer.id).flatMap { c in c.isGranted ? { confirmRevoke = c } : nil },
                                    remove: { confirmForget = computer })
                    }
                }
                ForEach(accountOnly) { computer in
                    if accounts.isOlderCopy(computer) {
                        OlderCopyRow(computer: computer) { confirmRemoveCopy = computer }
                    } else {
                        AccountComputerRow(computer: computer, ticket: accounts.pendingAccess[computer.computerId],
                                           waiting: accounts.accessRowDetail(computer.computerId),
                                           ask: { access = AccessTarget(computer: computer) },
                                           revoke: { confirmRevoke = computer })
                    }
                }
                if !app.account.isConfigured {
                    Button {
                        addingComputer = true
                    } label: {
                        Label(String(localized: "Parear um computador"), systemImage: "qrcode.viewfinder")
                            .foregroundStyle(Palette.text)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
                if app.account.isConfigured {
                    Button("Atualizar computadores") { Task { await accounts.refreshCloud() } }
                }
            }

            // A short drop keeps the section; its calls fail at once until the Hub is back.
            let online = accounts.computers.compactMap { accounts.store(for: $0.id) }.filter { $0.mantemNaTela }
            if !online.isEmpty {
                CardSection(String(localized: "Conversa por voz"), footer: String(localized: "Use o reconhecimento de fala da Apple neste iPhone ou o OpenAI e o Gemini com sua própria chave de API, guardada no computador.")) {
                    ForEach(online, id: \.computer.id) { store in
                        Button { voice = VoiceTarget(store: store) } label: {
                            LinkRow {
                                Label(online.count > 1 ? String(localized: "Conversa por voz em \(store.hostName)") : String(localized: "Voz e chaves de API"), systemImage: "waveform")
                            }
                        }
                        .buttonStyle(.plain)
                    }
                }
            }

            if SharedStore.remotePushEnabled {
                CardSection(String(localized: "Notificações"), footer: String(localized: "Receba um resumo do resultado, um pedido de resposta ou um aviso de falha. As prévias seguem os ajustes do iOS.")) {
                    Toggle(String(localized: "Notificações push"), isOn: $notificationsEnabled)
                        .toggleStyle(.codync)
                        .onChange(of: notificationsEnabled) { _, on in
                            PushRegistrar.shared.resync()
                            Analytics.shared.capture(.settingChanged(.push, on: on))
                        }
                    notificationsRow
                }
            }

            #if DEBUG
            CardSection(footer: MXTextosDebug.ajuda()) {
                Toggle(MXTextosDebug.interruptor(), isOn: $novoVisual)
                    .toggleStyle(.codync)
                    // Ligado, o redesenho cobre o app: fecha esta folha para ele aparecer na hora.
                    .onChange(of: novoVisual) { _, on in if on { dismissModal() } }
            }
            #endif

            CardSection {
                Text(String(localized: "A análise de uso está desativada na Mexerica."))
                    .foregroundStyle(Palette.secondary)
            }

            if !hiddenBots.isEmpty {
                CardSection(String(localized: "Bots ocultos")) {
                    ForEach(hiddenBots) { item in
                        let store = item.store, bot = item.bot
                        HStack {
                            CharacterAvatar(bot: bot, size: 28, animated: false)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(bot.name)
                                Text(store.hostName).font(.caption).foregroundStyle(Palette.tertiary)
                            }
                            Spacer()
                            IconButton(String(localized: "Mostrar novamente"), systemImage: "eye") {
                                withAnimation(Motion.layout) { store.setHidden(bot, false) }
                            }
                        }
                    }
                }
            }

            CardSection {
                ValueRow(String(localized: "Versão do app"), value: Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "")
                Text(String(localized: "baseado no Codync")).foregroundStyle(Palette.secondary)
            }

            CardSection {
                Button { confirmStartOver = true } label: {
                    Label(String(localized: "Recomeçar"), systemImage: "arrow.counterclockwise")
                        .foregroundStyle(Palette.danger)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .codyncDialog(String(localized: "Remover a cópia antiga de \(confirmRemoveCopy?.name ?? String(localized: "computador"))?"), isPresented: Binding(
            get: { confirmRemoveCopy != nil }, set: { if !$0 { confirmRemoveCopy = nil } }
        ), message: String(localized: "Este computador usa uma identidade antiga e não pode mais ser acessado. Removê-lo apenas limpa sua conta.")) {
            guard let copy = confirmRemoveCopy else { return [] }
            return [DialogAction(String(localized: "Remover"), destructive: true) { Task { await accounts.removeFromAccount(copy.computerId) } }]
        }
        .codyncDialog(String(localized: "Recomeçar?"), isPresented: $confirmStartOver,
                      message: String(localized: "Sai de todas as contas e esquece todos os computadores neste iPhone, depois mostra as boas-vindas novamente. Seus computadores mantêm os bots e as conversas; pareie novamente para usá-los.")) {
            [DialogAction(String(localized: "Recomeçar"), destructive: true) { Task { await app.startOver() } }]
        }
        .refreshable { await accounts.refreshCloud() }
        .page(String(localized: "Computadores e ajustes"), pushed: pushed)
        // One screen, named for the way in (as on Android): Accounts → settings, the Computers sheet → computers.
        .tracked(.screenViewed(pushed ? .settings : .computers))
        .task { await accounts.refreshCloud() }
        .codyncSheet(isPresented: $addingComputer) {
            PairingView(inModal: true)
        }
        .codyncSheet(item: $voice) { target in VoiceChatSettingsView().environment(target.store) }
        .codyncSheet(isPresented: $aboutAnalytics) { AnalyticsSheet() }
        .codyncSheet(item: $access) { target in
            AccessRequestView(computer: target.computer, pending: accounts.pendingAccess[target.id] != nil)
        }
        .codyncDialog(String(localized: "Remover \(confirmForget?.name ?? String(localized: "computador"))?"),
                      isPresented: Binding(get: { confirmForget != nil }, set: { if !$0 { confirmForget = nil } }),
                      message: String(localized: "Os bots e as conversas continuam nesse computador. Você pode pareá-lo novamente quando quiser.")) {
            [DialogAction(String(localized: "Remover"), destructive: true) {
                if let c = confirmForget { Task { await app.removeComputer(c.id) } }
            }]
        }
        .codyncDialog(String(localized: "Revogar o acesso deste iPhone a \(confirmRevoke?.name ?? String(localized: "o computador"))?"),
                      isPresented: Binding(get: { confirmRevoke != nil }, set: { if !$0 { confirmRevoke = nil } }),
                      message: String(localized: "Você pode pedir acesso novamente; o computador mostrará um novo código de confirmação.")) {
            [DialogAction(String(localized: "Revogar"), destructive: true) {
                if let c = confirmRevoke { Task { await app.revokeAccess(c.computerId) } }
            }]
        }
        .task { notificationsAllowed = await PushRegistrar.shared.isAllowed() }
        // What failed here (removing, revoking, refreshing) shows over this sheet, not under it at the root,
        // where the same error clears with this one. In place: a failure right after a confirmation comes
        // while that dialog's cover is still fading out, and a second cover can't present over it.
        .codyncDialog(String(localized: "Algo deu errado"), isPresented: Binding(
            get: { accounts.lastError != nil }, set: { if !$0 { accounts.lastError = nil } }
        ), message: accounts.lastError, cancel: "OK", inPlace: true) { [] }
    }


    private struct VoiceTarget: Identifiable {
        let store: BotStore
        let id: String

        @MainActor init(store: BotStore) {
            self.store = store
            id = store.computer.id
        }
    }

    private struct AccessTarget: Identifiable {
        let computer: CloudComputer
        var id: ComputerID { computer.computerId }
    }

    private func cloudComputer(_ id: ComputerID) -> CloudComputer? {
        accounts.cloudComputers.first { $0.computerId == id }
    }

    /// Account computers this iPhone isn't set up for yet.
    private var accountOnly: [CloudComputer] {
        accounts.cloudComputers.filter { c in !accounts.computers.contains { $0.id == c.computerId } }
    }

    private struct HiddenBot: Identifiable {
        let store: BotStore
        let bot: Bot
        let id: String
    }

    private var hiddenBots: [HiddenBot] {
        accounts.computers.compactMap { accounts.store(for: $0.id) }.flatMap { store in
            store.hiddenBots.map { HiddenBot(store: store, bot: $0, id: "\(store.computer.id)/\($0.id)") }
        }
    }

    /// This screen is a sheet of its own or inside Accounts; the marketplace and screen open over the app,
    /// once that sheet has slid away.
    private func closeSheets(then open: @escaping @MainActor () -> Void) {
        dismissModal()
        Task {
            try? await Task.sleep(for: .milliseconds(450))
            open()
        }
    }

    private func openScreen(_ store: BotStore) {
        closeSheets { store.screenRequest = ScreenRequest() }
    }

    private var notificationsRow: some View {
        Button {
            if notificationsAllowed != nil {
                UIApplication.shared.open(URL(string: UIApplication.openNotificationSettingsURLString)!)
            } else {
                Task { notificationsAllowed = await PushRegistrar.shared.requestAuthorization(from: .settings) }
            }
        } label: {
            ValueRow(String(localized: "Notificações"), value: notificationsAllowed == true ? String(localized: "Ativadas") : notificationsAllowed == false ? String(localized: "Desativadas nos Ajustes") : String(localized: "Ativar"))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// A computer this iPhone can use: its connection, how it's reached, and whether it's in the account.
private struct ComputerRow: View {
    let store: BotStore
    let inAccount: CloudComputer?
    let openScreen: () -> Void
    /// Agents, connectors and skills installed on this computer.
    let openMarketplace: () -> Void
    /// Set when this iPhone's access to the computer can be revoked.
    let revoke: (() -> Void)?
    let remove: () -> Void
    @Environment(AccountStore.self) private var accounts
    @State private var coloring = false
    @State private var voiceSettings = false

    var body: some View {
        HStack(spacing: 12) {
            ComputerBadge(store.computer, size: 40)
            VStack(alignment: .leading, spacing: 2) {
                Text(store.hostName).font(.body.weight(.semibold)).foregroundStyle(Palette.text)
                HStack(spacing: 4) {
                    Text(detail).lineLimit(stale ? 2 : 1)
                    if store.connection == .online { RouteIcon(route: store.hostRoute) }
                }
                .font(.subheadline)
                .foregroundStyle(store.isOffline ? Palette.warning : Palette.secondary)
            }
            Spacer()
            if store.mantemNaTela {
                IconButton(String(localized: "Extensões"), systemImage: "square.grid.2x2", action: openMarketplace)
            }
            if store.screen != nil, store.mantemNaTela {
                IconButton(String(localized: "Tela"), systemImage: "display", action: openScreen)
            }
            if stale {
                // Revoked, or this computer under an earlier identity: nothing to do but remove it.
                IconButton(String(localized: "Remover"), systemImage: "trash", action: remove)
            } else {
                // Which route goes first, and whether Cloudflare is a fallback at all.
                DropdownMenu {
                    ConnectionRoute.allCases.map { route in
                        MenuItem(route.title, icon: route.icon, selected: route == preference) {
                            accounts.setRoute(store.computer.id, route)
                        }
                    }
                } label: {
                    Image(systemName: preference.icon)
                        .font(.system(size: 18, weight: .medium))
                        .foregroundStyle(Palette.secondary)
                        .frame(width: 36, height: 36)
                        .contentShape(Rectangle())
                }
                .accessibilityLabel(String(localized: "Conexão"))
            }
            if inAccount != nil {
                Image(systemName: "person.crop.circle.badge.checkmark")
                    .foregroundStyle(Palette.tertiary)
                    .accessibilityLabel(String(localized: "Na sua conta"))
            }
        }
        .padding(.vertical, 2)
        .contentShape(Rectangle())
        .contextActions {
            var items = [
                MenuItem(String(localized: "Cor"), icon: "paintpalette") { coloring = true },
            ]
            if store.mantemNaTela {
                items.append(MenuItem(String(localized: "Extensões"), icon: "square.grid.2x2", action: openMarketplace))
                items.append(MenuItem(String(localized: "Conversa por voz"), icon: "waveform") { voiceSettings = true })
            }
            if store.screen != nil, store.mantemNaTela {
                items.append(MenuItem(String(localized: "Tela"), icon: "display", action: openScreen))
            }
            if let revoke {
                items.append(MenuItem(String(localized: "Revogar acesso deste iPhone"), icon: "lock.slash", destructive: true, divider: true, action: revoke))
            }
            items.append(MenuItem(String(localized: "Remover"), icon: "trash", destructive: true, divider: revoke == nil, action: remove))
            return items
        }
        .codyncSheet(isPresented: $voiceSettings) { VoiceChatSettingsView().environment(store) }
        .codyncOverlay(isPresented: $coloring) { close in
            ZStack {
                Color.black.opacity(0.35).ignoresSafeArea().onTapGesture(perform: close)
                SwatchPanel(selected: store.computer.color) { id in
                    close()
                    accounts.setColor(store.computer.id, id)
                }
                .background(Palette.bubbleAgent, in: RoundedRectangle(cornerRadius: 22, style: .continuous))
                .shadow(color: .black.opacity(0.25), radius: 24, y: 10)
            }
            .accessibilityAddTraits(.isModal)
        }
    }

    private var preference: ConnectionRoute { store.computer.route ?? .automatic }

    private var stale: Bool { accounts.isStale(store.computer.id) }

    private var detail: String {
        guard store.connection == .online else { return accounts.statusText(store) }
        let count = store.roster.count
        let bots = String(localized: "\(count) bot")
        switch store.hostRoute {
        case .direct: return String(localized: "\(bots) · Wi-Fi/Tailscale")
        case .relay: return String(localized: "\(bots) · Nuvem")
        default: return bots
        }
    }
}

/// A computer in the account that this iPhone can't use yet.
private struct AccountComputerRow: View {
    let computer: CloudComputer
    /// The request in progress, if any: its code shows in the line under the name.
    let ticket: AccessTicket?
    /// The request's line: the code to type on the Mac, or, once the Mac approved, where to type its code.
    let waiting: String?
    let ask: () -> Void
    let revoke: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            ComputerBadge(Computer(id: computer.computerId, name: computer.name, signKey: computer.signKey, device: computer.device), size: 40)
                .opacity(0.5)
            VStack(alignment: .leading, spacing: 2) {
                Text(computer.name).font(.body.weight(.semibold)).foregroundStyle(Palette.text)
                Text(detail).font(.subheadline).foregroundStyle(Palette.secondary).lineLimit(2)
            }
            Spacer()
            if ticket != nil {
                Button("Ver código", action: ask).buttonStyle(.secondary)
            } else if computer.isGranted {
                Button("Revogar", role: .destructive, action: revoke).buttonStyle(.secondary)
            } else {
                Button("Pedir acesso", action: ask).buttonStyle(.secondary)
            }
        }
        .padding(.vertical, 2)
    }

    private var detail: String {
        let online = computer.isOnline ? "Ligado" : "Desligado"
        if let waiting { return waiting }
        return switch computer.access {
        case "granted": "Aprovado para uma instalação antiga do Mexerica. Revogue e peça acesso de novo."
        case "pending": "\(online) · Pedido aguardando"
        default: "\(online) · Na sua conta"
        }
    }
}

/// The same machine under an earlier identity: one tap cleans it out of the account.
private struct OlderCopyRow: View {
    let computer: CloudComputer
    let remove: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            ComputerBadge(Computer(id: computer.computerId, name: computer.name, signKey: computer.signKey, device: computer.device), size: 40)
                .opacity(0.35)
            VStack(alignment: .leading, spacing: 2) {
                Text(computer.name).font(.body.weight(.semibold)).foregroundStyle(Palette.secondary)
                Text(detail).font(.subheadline).foregroundStyle(Palette.tertiary).lineLimit(2)
            }
            Spacer()
            IconButton(String(localized: "Remover da conta"), systemImage: "trash", action: remove)
        }
        .padding(.vertical, 2)
    }

    private var detail: String {
        if let lastSeen = computer.lastSeenAt {
            return String(localized: "Cópia antiga deste computador · visto \(RelativeTime.day(Date(milliseconds: lastSeen)))")
        }
        return String(localized: "Cópia antiga deste computador")
    }
}

/// A navigation row label with a trailing chevron.
private struct LinkRow<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        HStack {
            content.foregroundStyle(Palette.text)
            Spacer(minLength: 8)
            Image(systemName: "chevron.right").font(.caption.weight(.semibold)).foregroundStyle(Palette.tertiary)
        }
        .contentShape(Rectangle())
    }
}

/// The computer color choices: a grid of colored dots, the current one checked.
private struct SwatchPanel: View {
    let selected: String?
    let pick: (String) -> Void

    var body: some View {
        LazyVGrid(columns: Array(repeating: GridItem(.fixed(36), spacing: 10), count: 6), spacing: 10) {
            ForEach(AvatarPalette.colors) { swatch in
                Button { pick(swatch.id) } label: {
                    Circle().fill(swatch.color)
                        .frame(width: 32, height: 32)
                        .overlay {
                            if selected == swatch.id {
                                Image(systemName: "checkmark").font(.caption.weight(.bold)).foregroundStyle(.white)
                            }
                        }
                }
                .buttonStyle(.plain)
                .accessibilityLabel(swatch.label)
                .accessibilityAddTraits(selected == swatch.id ? .isSelected : [])
            }
        }
        .padding(14)
    }
}

private extension ConnectionRoute {
    var title: String {
        switch self {
        case .automatic: "Padrão do acesso (conta: nuvem; QR: direto primeiro)"
        case .relayOnly: "Nuvem Mexerica"
        case .cloudflareFirst: String(localized: "Nuvem, depois Wi-Fi ou Tailscale")
        case .directOnly: String(localized: "Somente Wi-Fi ou Tailscale, nunca pela nuvem")
        }
    }

    var icon: String {
        switch self {
        case .automatic: "arrow.triangle.branch"
        case .relayOnly: "cloud.fill"
        case .cloudflareFirst: "cloud"
        case .directOnly: "wifi"
        }
    }
}
