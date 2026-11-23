// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI
import UIKit

/// One endless conversation with a bot or a group chat. Only deliberate messages show
/// here; tool calls and thinking live in the "Full conversation" sheet. Any message can be
/// quoted in a reply.
public struct ThreadView: View {
    let botId: String

    public init(botId: String) { self.botId = botId }
    @Environment(BotStore.self) var model
    @Environment(\.dismiss) var dismiss
    @State var showTrace = false
    /// The message the composer's next send quotes.
    @State var replyingTo: Entry?
    @State var botChat: BotChatTarget?
    @State var editingGroup = false
    @State var details: DetailsRequest?
    @State var confirmNewSession = false
    @State var confirmDelete: Bot?
    @State var calling = false
    @State var callSpeaking = false
    @State var interruptCall: (() -> Void)?
    /// The chat stays on the newest message until the reader scrolls away.
    @State var following = true
    @State var loadingEarlier = false
    @Environment(\.accessibilityReduceMotion) var reduceMotion

    var bot: Bot? { model.bots[botId] }

    /// The messages (each platform's own `transcript`, in its `ThreadView+` file) and the
    /// composer, without the platform's top chrome.
    private var chat: some View {
        transcript
        .background(Palette.background)
        // A short drop: the same conversation stays, with this strip on top.
        .safeAreaInset(edge: .top, spacing: 0) {
            FaixaDeQueda(stores: [model])
                .animation(Motion.reduced(Motion.layout, reduceMotion), value: model.faixaDeQueda)
        }
        .safeAreaInset(edge: .bottom) {
            VStack(spacing: 6) {
                if let mismatch = model.mismatch {
                    // Nothing sent now could be read on the other side.
                    UpdateNeededCard(store: model, mismatch: mismatch)
                        .padding(.horizontal, 16)
                        .padding(.bottom, 8)
                        .transition(.opacity)
                } else {
                    Composer(botId: botId, replyingTo: $replyingTo,
                             onCall: !calling && bot != nil ? { calling = true } : nil,
                             onInterrupt: callSpeaking ? interruptCall : nil)
                }
            }
            .animation(Motion.reduced(Motion.layout, reduceMotion), value: model.mismatch)
        }
        // Grok Bot's call: a bar floating over the chat, which stays readable and usable.
        .overlay(alignment: .top) {
            if calling {
                CallView(botId: botId, isSpeaking: $callSpeaking, interrupt: $interruptCall) {
                    withAnimation(Motion.reduced(Motion.layout, reduceMotion)) { calling = false }
                }
                    .padding(.top, 4)
                    .transition(.move(edge: .top).combined(with: .opacity))
            }
        }
        .animation(Motion.reduced(Motion.layout, reduceMotion), value: calling)
    }

    var conversation: some View {
        chrome(chat)
            .readingConversation(botId)
            .codyncSheet(item: Binding<FileDownloads.Export?>(get: {
                guard let item = model.fileDownloads.export, item.botId == botId else { return nil }
                return item
            }, set: { if $0 == nil { model.fileDownloads.dismissExport() } })) { item in
                FileExportSheet(url: item.url) { model.fileDownloads.dismissExport() }
            }
            .codyncSheet(isPresented: $showTrace) {
                TraceView(botId: botId)
            }
            .codyncSheet(item: $botChat) { BotConversationView(botId: botId, peerId: $0.id) }
            .codyncSheet(item: $details) { request in
                BotDetailsView(botId: botId, request: request)
            }
            .codyncSheet(isPresented: $editingGroup) {
                if let bot { GroupEditorView(group: bot) }
            }
            .codyncDialog(String(localized: "Iniciar uma nova sessão?", bundle: .module), isPresented: $confirmNewSession,
                          message: String(localized: "A conversa continua aqui, mas o bot começa com um contexto novo.", bundle: .module)) {
                [DialogAction(String(localized: "Nova sessão", bundle: .module)) { model.newSession(botId) }]
            }
            .deleteBotConfirmation($confirmDelete) { dismiss() }
    }

    // MARK: rows

    @ViewBuilder func row(_ item: ChatItem) -> some View {
        switch item.kind {
        case .separator(let date):
            TimeSeparator(date: date)
        case .exchanges(let group):
            BotMessageRow(group: group) { botChat = BotChatTarget(id: group.peerId) }
        case .entry(let e, let groupStart):
            if e.kind == "notice", let id = e.data.routineId {
                Button {
                    openRoutine(id)
                } label: {
                    Label(e.data.text ?? String(localized: "Rotina", bundle: .module), systemImage: "clock.arrow.circlepath")
                        .font(.footnote).foregroundStyle(Palette.secondary).padding(.vertical, 10)
                }.buttonStyle(.plain)
            } else {
                ChatRow(entry: e, groupStart: groupStart, chat: bot, openTrace: { showTrace = true },
                        // An update card takes the composer's place, so there's nothing to reply with.
                        reply: e.isReplyable && model.mismatch == nil ? { reply(to: e) } : nil)
                    .equatable()
            }
        }
    }

    private func reply(to entry: Entry) {
        withAnimation(Motion.reduced(Motion.layout, reduceMotion)) { replyingTo = entry }
    }

    // MARK: chrome

    var connectionSubtitle: some View {
        HStack(spacing: 6) {
            Group {
                if model.shownConnection == .connecting {
                    Spinner(size: 8)
                } else {
                    Image(systemName: connectionSymbol).font(.system(size: 8, weight: .medium))
                }
            }
            .frame(width: 12)
            .accessibilityHidden(true)
            Text(model.connectionLabel)
                .lineLimit(1)
                .truncationMode(.middle)
        }
        .font(.system(size: 10, weight: .medium))
        .foregroundStyle(Palette.secondary)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(model.hostName), \(connectionDescription)")
        .help("\(model.hostName) · \(connectionDescription)")
    }

    private var connectionSymbol: String {
        guard model.connection == .online else { return "wifi.slash" }
        switch model.hostRoute {
        case .relay: return "cloud"
        case .direct: return "wifi"
        case nil: return "network"
        }
    }

    private var connectionDescription: String {
        switch model.shownConnection {
        case .connecting: return String(localized: "Conectando", bundle: .module)
        case .computerOffline, .offline: return String(localized: "Hub indisponível", bundle: .module)
        case .unauthorized: return String(localized: "Sem acesso", bundle: .module)
        case .unpaired: return String(localized: "Não pareado", bundle: .module)
        case .online: break
        }
        if model.mismatch != nil { return String(localized: "Precisa atualizar", bundle: .module) }
        switch model.hostRoute {
        case .relay: return String(localized: "Conectado pela nuvem", bundle: .module)
        case .direct: return String(localized: "Conectado por Wi-Fi ou Tailscale", bundle: .module)
        case nil: return String(localized: "Conectado", bundle: .module)
        }
    }

    var title: some View {
        HStack(spacing: 7) {
            if let bot {
                let size: CGFloat = 22
                if bot.isGroup { GroupAvatar(members: model.members(of: bot), size: size) } else { CharacterAvatar(bot: bot, size: size) }
            }
            Text(bot?.name ?? "")
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(Palette.text)
                .lineLimit(1)
        }
    }

    func openRoutine(_ id: String?) {
        withAnimation(Motion.reduced(Motion.layout, reduceMotion)) {
            details = DetailsRequest(tab: .details, routineId: id)
        }
    }

    var menuItems: [MenuItem] {
        var items = [MenuItem(String(localized: "Conversa completa", bundle: .module), icon: "list.bullet.rectangle") { showTrace = true }]
        if let bot, bot.isGroup {
            items.append(MenuItem(String(localized: "Editar grupo", bundle: .module), icon: "person.2") { editingGroup = true })
            items.append(MenuItem(bot.pinned ? String(localized: "Desafixar", bundle: .module) : String(localized: "Fixar", bundle: .module), icon: "pin") { model.setPinned(bot, !bot.pinned) })
            items.append(MenuItem(String(localized: "Excluir grupo", bundle: .module), icon: "trash", destructive: true, divider: true) { confirmDelete = bot })
            return items
        }
        if let bot {
            items.append(MenuItem(String(localized: "Ajustes", bundle: .module), icon: "gearshape") { openSettings() })
            items.append(MenuItem(bot.pinned ? String(localized: "Desafixar", bundle: .module) : String(localized: "Fixar", bundle: .module), icon: "pin") { model.setPinned(bot, !bot.pinned) })
        }
        items.append(MenuItem(String(localized: "Rotinas", bundle: .module), icon: "clock.arrow.circlepath") {
            openRoutine(nil)
        })
        items.append(MenuItem(String(localized: "Nova sessão", bundle: .module), icon: "arrow.counterclockwise") { confirmNewSession = true })
        items.append(MenuItem(String(localized: "Excluir bot", bundle: .module), icon: "trash", destructive: true, divider: true) { confirmDelete = bot })
        return items
    }
}

// MARK: - Chat model

struct ChatItem: Identifiable {
    enum Kind {
        case separator(Date)
        case entry(Entry, groupStart: Bool)
        /// Main chat only: consecutive bot-to-bot exchanges with one peer.
        case exchanges(BotExchangeGroup)
    }

    let id: String
    let kind: Kind

    /// Chat-visible entries plus time separators (gaps > 1 h) and author grouping. A bot's
    /// messages arrive whole (Grok Bot's `send_message`); what it writes along the way is trace. Runs of
    /// bot exchanges with one peer fold into one item.
    static func build(_ entries: [Entry]) -> [ChatItem] {
        var out: [ChatItem] = []
        var lastDate: Date?
        var lastAuthor: String?
        for slot in BotExchangeGroup.collapse(entries) {
            let date = slot.date
            // Bot-originated messages carry an empty nonce. Their entry IDs
            // distinguish them; only real nonces identify optimistic echoes.
            let id = if case .entry(let e) = slot, e.kind == "user", let nonce = e.data.clientNonce, !nonce.isEmpty { "user-\(nonce)" } else { slot.id }
            if lastDate.map({ date.timeIntervalSince($0) > 3600 }) ?? true {
                out.append(ChatItem(id: "sep-\(id)", kind: .separator(date)))
                lastAuthor = nil
            }
            lastDate = date
            switch slot {
            case .exchanges(let group):
                out.append(ChatItem(id: id, kind: .exchanges(group)))
                lastAuthor = nil
            case .entry(let e):
                // In a group each bot is its own author.
                let author = e.kind == "user" ? "user" : e.kind == "agent" ? "agent:\(e.data.author ?? "")" : e.kind
                out.append(ChatItem(id: id, kind: .entry(e, groupStart: author != lastAuthor)))
                lastAuthor = (author == "user" || e.kind == "agent") ? author : nil
            }
        }
        return out
    }

    var isUserMessage: Bool {
        if case let .entry(entry, _) = kind { entry.kind == "user" } else { false }
    }

}

/// The start of an empty group chat: who's in it and how the room works.
struct GroupIntroCard: View {
    let group: Bot
    @Environment(BotStore.self) private var model

    var body: some View {
        VStack(spacing: 12) {
            GroupAvatar(members: model.members(of: group), size: 72)
            Text(group.name).font(.title2.weight(.semibold)).foregroundStyle(Palette.text)
            let names = model.members(of: group).map(\.name)
            if group.name != names.joined(separator: ", ") {
                Text(names.joined(separator: " · "))
                    .font(.subheadline)
                    .foregroundStyle(Palette.secondary)
                    .multilineTextAlignment(.center)
            }
            if !group.description.isEmpty {
                Text(group.description)
                    .font(.subheadline)
                    .foregroundStyle(Palette.secondary)
                    .multilineTextAlignment(.center)
            }
            Text(String(localized: "Todos respondem por vez. Use @ para perguntar a apenas um bot.", bundle: .module))
                .font(.subheadline)
                .foregroundStyle(Palette.secondary)
                .multilineTextAlignment(.center)
                .padding(.top, 4)
        }
        .frame(maxWidth: .infinity)
        .padding(.horizontal, 24)
    }
}

struct IntroCard: View {
    let bot: Bot
    @Environment(BotStore.self) private var model

    var body: some View {
        VStack(spacing: 12) {
            CharacterAvatar(bot: bot, size: 72)
            Text(bot.name).font(.title2.weight(.semibold)).foregroundStyle(Palette.text)
            Text(bot.managedWorkspace ? String(localized: "\(model.backendName(bot.backend)) · Espaço de trabalho pessoal", bundle: .module) : String(localized: "\(model.backendName(bot.backend)) em \(bot.cwd)", bundle: .module))
                .font(.subheadline)
                .foregroundStyle(Palette.secondary)
                .multilineTextAlignment(.center)
            if !bot.description.isEmpty {
                Text(bot.description)
                    .font(.subheadline)
                    .foregroundStyle(Palette.secondary)
                    .multilineTextAlignment(.center)
            }
            Text(String(localized: "Diga o que precisa. Você receberá uma notificação quando ele terminar ou precisar de você.", bundle: .module))
                .font(.subheadline)
                .foregroundStyle(Palette.secondary)
                .multilineTextAlignment(.center)
                .padding(.top, 4)
        }
        .frame(maxWidth: .infinity)
        .padding(.horizontal, 24)
    }
}

extension ThreadView {
    public var body: some View {
        conversation
            .tracked(.screenViewed(.chat))
    }

    /// A short drop keeps it; only a computer let go hides it.
    private var working: Bool { bot?.isWorking(in: botId) == true && model.temAutoridade }

    /// iPhone's messages: all this device has, earlier ones from the computer as the reader
    /// scrolls up; the chat follows the newest message until they scroll away, and a reply is
    /// revealed steadily as it's written.
    var transcript: some View {
        let items = ChatItem.build(model.chat(botId))
        var rows: [ConversationRow] = []
        if moreOnComputer, !model.hubUnavailable {
            rows.append(ConversationRow("earlier") {
                Spinner(size: 14)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 14)
                    .accessibilityLabel(String(localized: "Carregando mensagens anteriores", bundle: .module))
                    // A chat shorter than the screen never scrolls, so showing the row is the trigger.
                    .onAppear { showEarlier() }
            })
        }
        if items.isEmpty && model.hubUnavailable {
            rows.append(ConversationRow("hub-offline") {
                Label(String(localized: "Hub indisponível", bundle: .module), systemImage: "wifi.slash")
                    .foregroundStyle(Palette.secondary)
                    .frame(maxWidth: .infinity)
                    .padding(.top, 40)
            })
        } else if items.isEmpty, let bot {
            rows.append(ConversationRow("intro") {
                if bot.isGroup { GroupIntroCard(group: bot).padding(.top, 40) } else { IntroCard(bot: bot).padding(.top, 40) }
            })
        }
        rows += items.map { item in ConversationRow(item.id, isUserMessage: item.isUserMessage) { row(item) } }
        // Offline, "working" is only what the computer last said; don't show it as live.
        if let bot, working {
            rows.append(ConversationRow("working") {
                WorkingIndicator(bot: bot, thinking: model.currentThinking(botId))
                    .padding(.top, 6)
            })
        }
        // Pull to refresh reconnects, like Reconnect in the computer menu, until the link is back.
        return ConversationList(rows: rows, following: $following, nearTop: { showEarlier() }, refresh: {
            model.restartStream()
            await model.aguardarReconexao()
        })
    }

    private var moreOnComputer: Bool {
        model.canLoadOlder(botId)
    }

    /// The reader scrolled up to the top: fetch earlier messages from the computer.
    private func showEarlier() {
        guard moreOnComputer, !loadingEarlier else { return }
        loadingEarlier = true
        Task {
            await model.loadOlder(botId)
            loadingEarlier = false
        }
    }

    /// The system navigation bar: the title pill holds the bot's actions; the corner is the computer.
    func chrome(_ content: some View) -> some View {
        content
            .navigationBarTitleDisplayMode(.inline)
            .toolbar(.visible, for: .navigationBar)
            .toolbar {
                ToolbarItem(placement: .principal) {
                    VStack(spacing: 0) {
                        header
                        connectionSubtitle
                    }
                }
                if model.screen != nil {
                    ToolbarItem(placement: .topBarTrailing) { computerButton }
                }
            }
    }

    var header: some View {
        // Like Grok Bot: the title pill holds the bot's actions; the corner is the computer.
        Menu {
            Text("\(model.hostName) · \(model.connectionLabel)")
            Button(String(localized: "Reconectar", bundle: .module), systemImage: "arrow.clockwise") { model.restartStream() }
            Divider()
            Button(String(localized: "Detalhes", bundle: .module), systemImage: "info.circle", action: openDetails)
            ForEach(menuItems) { item in
                if item.divider { Divider() }
                Button(role: item.destructive ? .destructive : nil, action: item.action) {
                    if let icon = item.icon { Label(item.title, systemImage: icon) } else { Text(item.title) }
                }
            }
        } label: {
            title
        }
        .accessibilityLabel(String(localized: "Ações de \(bot?.name ?? String(localized: "Conversa", bundle: .module))", bundle: .module))
    }

    private func openDetails() {
        if bot?.isGroup == true { editingGroup = true } else { details = DetailsRequest() }
    }

    /// Opens the computer's screen; pulses while this bot is operating it (watch, then take over).
    private var computerButton: some View {
        let operating = model.screen?.agentBot == botId
        return Button(String(localized: "Computador", bundle: .module), systemImage: "desktopcomputer") {
            model.screenRequest = operating ? ScreenRequest(watching: botId) : ScreenRequest()
        }
        .symbolEffect(.pulse, options: .repeating, isActive: operating)
    }

    func openSettings() {
        details = DetailsRequest(tab: .settings)
    }
}
