// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import Foundation
import os

private let log = Logger(subsystem: "app.mexerica.ios", category: "BotStore")

extension BotStore {
    /// The client while the link is really up: not during a reconnect the grace period keeps
    /// quiet, nor right after coming back to the foreground, when the old link is already gone.
    var live: HostClient? {
        connection == .online && heldDrop == nil && eventsTask != nil && !retired ? client : nil
    }

    /// The client while this store has authority: the link is up and the Hub identified itself on
    /// it. During a drop, its grace included, this throws at once: nothing waits for the reconnect, and
    /// nothing goes out over the old channel or, later, by itself over the new one.
    func ready() async throws -> HostClient {
        // The client of the channel the Hub identified itself on, and only while that channel is still up.
        if !retired, estadoConexao == .conectado, live != nil, let cliente = clienteAutorizado,
           await cliente.transport.currentLink() != nil {
            return cliente
        }
        switch connection {
        case let .computerOffline(lastSeen): throw HostError.computerOffline(lastSeen: lastSeen)
        case let .unauthorized(message): throw HostError.unauthorized(message)
        default: throw HostError.unreachable
        }
    }

    /// Waits (up to `deadline`) until the store has authority again, for pull to refresh. Sends nothing.
    func aguardarReconexao(until deadline: ContinuousClock.Instant = .now + BotStore.actionPatience) async {
        while !retired, !temAutoridade, ContinuousClock.now < deadline, !Task.isCancelled {
            try? await Task.sleep(for: .milliseconds(100))
        }
    }

    /// Runs `op` while the store has authority. With `replay`, only for calls the computer can safely get
    /// twice, a call that failed on the link is tried again, but only on that same link: if the link
    /// dropped meanwhile, it fails and stays with the person (a message shows "Not sent" with Resend).
    /// `link` is the `linkId` read synchronously when the person acted, before any `Task` hop:
    /// a link that dropped and came back before the task ran is another link, and nothing goes out on it.
    func withLink<T>(link: Int, replay: Bool = false, _ op: @MainActor (HostClient) async throws -> T) async throws -> T {
        let deadline = ContinuousClock.now + Self.actionPatience
        while true {
            let client = try await ready()
            guard !retired, !Task.isCancelled, linkId == link else { throw HostError.unreachable }
            do {
                return try await op(client)
            } catch let error as HostError where replay && error.isTransient && ContinuousClock.now < deadline {
                // Only on the same channel: once it is gone the action stays with the person.
                guard linkId == link, await client.transport.currentLink() != nil else { throw error }
                log.info("retrying after: \(error.localizedDescription)")
                try await Task.sleep(for: .milliseconds(500))
            }
        }
    }

    /// Fire-and-forget actions; only what waiting and retrying couldn't settle reaches the dialog.
    private func perform(replay: Bool = false, _ op: @escaping @MainActor (HostClient) async throws -> Void) {
        let link = linkId
        Task {
            do {
                try await withLink(link: link, replay: replay, op)
            } catch {
                if link == linkId { lastError = error.localizedDescription }
            }
        }
    }

    public func stop(_ botId: String) { perform(replay: true) { try await $0.stop(botId) } }
    public func logCall(_ botId: String, seconds: Int) { perform { try await $0.logCall(botId, seconds: seconds) } }
    public func newSession(_ botId: String) { perform { try await $0.newSession(botId) } }

    // MARK: remote screen

    /// Takes control from bots (they can still look) or hands it back.
    public func screenTakeover(_ on: Bool) {
        let link = linkId
        perform(replay: true) { [weak self] client in
            let screen = try await client.screenTakeover(on)
            guard self?.linkId == link else { return }
            self?.screen = screen
        }
    }

    /// Quick reactions offered on every message (Slack's hover bar).
    public static let quickReactions = ["👍", "❤️", "😂", "🎉", "👀", "✅"]

    /// Toggles the user's reaction; shown at once, then replaced by the host's copy.
    public func react(_ entry: Entry, _ emoji: String) {
        guard let i = entries[entry.botId]?.firstIndex(where: { $0.id == entry.id }) else { return }
        var reactions = entries[entry.botId]?[i].data.reactions ?? []
        if let at = reactions.firstIndex(of: emoji) { reactions.remove(at: at) } else { reactions.append(emoji) }
        entries[entry.botId]?[i].data.reactions = reactions
        let link = linkId
        perform { client in
            let e = try await client.react(entryId: entry.id, emoji: emoji)
            guard self.linkId == link else { return }
            self.upsert(e)
        }
    }

    public func respond(_ entry: Entry, option: String?, source: String? = nil) {
        respond(entry, option: option, source: source, marca: marcaDoLink)
    }

    /// Voice: `marca` is the link when the call's audio began, not when the model asked.
    func respond(_ entry: Entry, option: String?, source: String? = nil, marca: MarcaDoLink) {
        guard answering[entry.id] == nil else { return }
        Motion.animate { answering[entry.id] = option ?? "" }
        Task {
            do {
                try await withLink(link: marca.valor, replay: true) { try await $0.respondPermission(botId: entry.botId, entryId: entry.id, entryRev: entry.rev, optionId: option, source: source) }
                // The card's own update follows on the events stream; hold the spinner until then.
                try? await Task.sleep(for: .seconds(2))
            } catch {
                if marca.valor == linkId { lastError = error.localizedDescription }
            }
            guard marca.valor == linkId else { return }
            Motion.animate { answering[entry.id] = nil }
        }
    }

    /// Views register only while visible in an active scene. Multiple windows may
    /// read the same conversation without unregistering each other.
    func setReading(_ token: UUID, botId: String, active: Bool) {
        if active {
            readingViews[token] = botId
            if connection == .online { markRead(botId) }
        } else {
            readingViews[token] = nil
        }
    }

    func acknowledgeVisibleConversations(_ botId: String, entry: Entry? = nil) {
        guard connection == .online else { return }
        guard readingViews.values.contains(botId), entry?.isChat != false else { return }
        markRead(botId)
    }

    /// The chat on screen is read; the host ignores it when nothing there is unread.
    public func markRead(_ botId: String) {
        // Entry events may arrive before the roster's unread count. The host is
        // authoritative and treats a redundant scoped acknowledgement as a no-op.
        Task {
            // Opening a cached conversation can race the foreground reconnect.
            // This background acknowledgement must never interrupt the conversation.
            if live == nil {
                try? await Task.sleep(for: Self.initialConnectionGrace)
            }
            guard !Task.isCancelled, isActive, let client = live else { return }
            do {
                try await client.markRead(botId)
            } catch {
                // The visible scope is acknowledged again when the link recovers.
                log.debug("read acknowledgement deferred: \(error.localizedDescription)")
            }
        }
    }

    /// The roster's "Mark as read": everything in the chat.
    public func markAllRead(_ botId: String) {
        guard (bots[botId]?.unread ?? 0) > 0 else { return }
        bots[botId]?.unread = 0
        perform(replay: true) { try await $0.markRead(botId) }
    }

    public func setPinned(_ bot: Bot, _ pinned: Bool) {
        var d = BotDraft(bot)
        d.pinned = pinned
        bots[bot.id]?.pinned = pinned
        perform(replay: true) { _ = try await $0.updateBot(d) }
    }

    public func setHidden(_ bot: Bot, _ hidden: Bool) {
        var d = BotDraft(bot)
        d.hidden = hidden
        bots[bot.id]?.hidden = hidden
        perform(replay: true) { _ = try await $0.updateBot(d) }
    }

    /// Whether the bot may see and use the computer's screen.
    public func setComputer(_ bot: Bot, _ computer: Bool) {
        var d = BotDraft(bot)
        d.computer = computer
        bots[bot.id]?.computer = computer
        perform(replay: true) { _ = try await $0.updateBot(d) }
    }

    /// Whether the bot uses the memory all bots share.
    public func setSharedMemory(_ bot: Bot, _ shared: Bool) {
        var d = BotDraft(bot)
        d.sharedMemory = shared
        bots[bot.id]?.sharedMemory = shared
        perform(replay: true) { _ = try await $0.updateBot(d) }
    }

    /// Whether the bot's harness keeps its own memory on beside Codync's.
    public func setHarnessMemory(_ bot: Bot, _ on: Bool) {
        var d = BotDraft(bot)
        d.harnessMemory = on
        bots[bot.id]?.harnessMemory = on
        perform(replay: true) { _ = try await $0.updateBot(d) }
    }

    /// Saves the user's version of a handoff that hasn't been sent, only on the link of `marca`
    /// (read with `marcaDoLink` when the person tapped Save). Whether it did.
    public func editHandoff(_ entry: Entry, text: String, marca: MarcaDoLink) async -> Bool {
        do {
            try await withLink(link: marca.valor, replay: true) { try await $0.editHandoff(entryId: entry.id, text: text) }
            return true
        } catch {
            lastError = error.localizedDescription
            return false
        }
    }

    /// Carries on with the bot's current conversation instead of the handoff.
    public func keepConversation(_ entry: Entry) {
        guard answering[entry.id] == nil else { return }
        Motion.animate { answering[entry.id] = "handoff" }
        let link = linkId
        Task {
            do {
                try await withLink(link: link, replay: true) { try await $0.keepConversation(entryId: entry.id) }
            } catch {
                lastError = error.localizedDescription
            }
            Motion.animate { answering[entry.id] = nil }
        }
    }

    public func setFastMode(_ bot: Bot, _ fastMode: Bool) {
        var d = BotDraft(bot)
        d.fastMode = fastMode
        bots[bot.id]?.fastMode = fastMode
        perform(replay: true) { _ = try await $0.updateBot(d) }
    }

    /// The composer's `/fast`: the bot's Fast mode flips; in a group, every member that shows it
    /// goes to the opposite of the group's state.
    public func toggleFastMode(_ bot: Bot) {
        guard bot.isGroup else { return setFastMode(bot, !bot.fastMode) }
        let group = GroupFastMode(members(of: bot))
        for member in group.members { setFastMode(member, group.state == .off) }
    }

    // MARK: roster order

    /// Puts the roster in `ids` order (pinned bots still sort first) and sends it; the host
    /// syncs it to every device.
    public func reorder(_ ids: [String]) {
        for (position, id) in ids.enumerated() { bots[id]?.position = position }
        perform(replay: true) { try await $0.reorderBots(ids) }
    }

    /// One place up (-1) or down (+1) among bots with the same pin state (VoiceOver).
    public func move(_ botId: String, by offset: Int) {
        var ids = roster.map(\.id)
        guard let i = ids.firstIndex(of: botId), ids.indices.contains(i + offset),
              bots[ids[i + offset]]?.pinned == bots[botId]?.pinned else { return }
        ids.swapAt(i, i + offset)
        reorder(ids)
    }

    public func delete(_ bot: Bot) {
        let link = linkId
        Task {
            do {
                try await withLink(link: link) { try await $0.deleteBot(bot.id) }
                removeComposerDrafts(for: bot.id)
                bots[bot.id] = nil
                entries[bot.id] = nil
                scheduleSave()
            } catch {
                lastError = error.localizedDescription
            }
        }
    }

    /// Creates a group chat (or opens the one these bots already share) and selects it.
    public func createGroup(name: String, description: String = "", members: [String]) async throws -> Bot {
        let link = linkId
        let group = try await ready().createGroup(GroupDraft(name: name, description: description, members: members))
        guard !retired, link == linkId else { throw HostError.unreachable }
        bots[group.id] = group
        selection = group.id
        return group
    }

    public func updateGroup(_ draft: GroupDraft) async throws {
        let link = linkId
        let group = try await ready().updateGroup(draft)
        guard !retired, link == linkId else { throw HostError.unreachable }
        bots[group.id] = group
    }

    /// The bot-to-bot notices between a chat and a peer. Never `upsert`ed: old notices in the
    /// mirror would break `loadOlder`, which pages from the oldest loaded `seq`.
    public func botConversation(_ botId: String, peer: String) async throws -> [Entry] {
        try await ready().botConversation(botId: botId, peerId: peer)
    }

    public func save(_ draft: BotDraft) async throws -> Bot {
        let link = linkId
        let client = try await ready()
        let bot = draft.id == nil ? try await client.createBot(draft) : try await client.updateBot(draft)
        guard !retired, link == linkId else { throw HostError.unreachable }
        bots[bot.id] = bot
        return bot
    }

    public func listDirs(_ path: String?) async throws -> DirListing {
        try await ready().listDirs(path)
    }

    public func refreshUsage() async {
        guard !retired, let client else { return }
        let link = linkId
        if let u = try? await client.usage(refresh: true), !Task.isCancelled, !retired, link == linkId { setUsage(u) }
    }

    /// A ordem de uma leitura de sessões, tirada ANTES de qualquer espera: a resposta de uma leitura mais velha que a
    /// já aplicada não vale (identidade da operação, não só a geração do link).
    func novaLeituraDeSessoes(_ botId: String) -> Int {
        let ordem = (leiturasDeSessoesIniciadas[botId] ?? 0) + 1
        leiturasDeSessoesIniciadas[botId] = ordem
        return ordem
    }

    /// Aplica o resultado de uma leitura se nenhuma mais nova já foi aplicada (a mesma ordem pode confirmar de novo).
    @discardableResult
    func aplicarLeituraDeSessoes(_ botId: String, ordem: Int) -> Bool {
        guard ordem >= (leiturasDeSessoesAplicadas[botId] ?? 0) else { return false }
        leiturasDeSessoesAplicadas[botId] = ordem
        return true
    }

    /// Capture `marcaDoLink` at the tap, before creating a Task. `ordem`: a da leitura (`novaLeituraDeSessoes`), tirada
    /// por quem chama antes de esperar; sem ela, esta chamada tira a sua. Uma resposta mais velha que a aplicada devolve
    /// o resultado, mas não mexe no que o store sabe do envio por sessão.
    public func sessions(_ botId: String, marca: MarcaDoLink) async throws -> BotSessions {
        try await sessions(botId, marca: marca, ordem: novaLeituraDeSessoes(botId))
    }

    func sessions(_ botId: String, marca: MarcaDoLink, ordem: Int) async throws -> BotSessions {
        let result = try await withLink(link: marca.valor, replay: true) { try await $0.sessions(botId: botId) }
        guard linkId == marca.valor else { throw HostError.unreachable }
        guard aplicarLeituraDeSessoes(botId, ordem: ordem) else { return result }
        if result.sessionActions?.contains("send") == true { sessionSendSupported.insert(botId) }
        else { sessionSendSupported.remove(botId) }
        return result
    }

    /// Loads one page into the mirror; nil selects the principal conversation. Same behavior as always: it returns only
    /// the entries kept for this conversation. Callers that must know whether the history ended use `loadHistoryPage`.
    @discardableResult
    public func loadHistory(_ botId: String, sessionId: String? = nil, beforeSeq: Int64 = .max,
                            marca: MarcaDoLink) async throws -> [Entry] {
        try await loadHistoryPage(botId, sessionId: sessionId, beforeSeq: beforeSeq, marca: marca).entries
    }

    /// One page of history for a conversation: the entries kept (this session only, valid session ID) and the RAW row
    /// count the host sent, before any entry was discarded or filtered out. Only the raw count says whether the history
    /// ended (`rawCount < 100`, as `loadOlder` does): a full page that loses an entry here still has older pages.
    public struct PaginaDeHistorico: Sendable {
        public let entries: [Entry]
        public let rawCount: Int

        public init(entries: [Entry], rawCount: Int) {
            self.entries = entries
            self.rawCount = rawCount
        }
    }

    /// Loads one page into the mirror (same filtering and checks as `loadHistory`) and returns it with its raw count.
    @discardableResult
    public func loadHistoryPage(_ botId: String, sessionId: String? = nil, beforeSeq: Int64 = .max,
                                marca: MarcaDoLink) async throws -> PaginaDeHistorico {
        let rows = try await withLink(link: marca.valor, replay: true) {
            try await $0.history(botId: botId, beforeSeq: beforeSeq, sessionId: sessionId ?? botId)
        }
        guard linkId == marca.valor else { throw HostError.unreachable }
        let selected = rows.entries.filter { ($0.sessionId ?? botId) == (sessionId ?? botId) }
        for row in selected { upsert(row) }
        scheduleSave()
        return PaginaDeHistorico(entries: selected, rawCount: rows.rawCount)
    }

    public func loadOlder(_ botId: String) async {
        guard !historyComplete.contains(botId) else { return }
        let link = linkId
        let first = chat(botId).first { $0.seq > 0 && $0.seq != Int64.max }?.seq ?? Int64.max
        guard let older = try? await withLink(link: link, replay: true, {
            try await $0.history(botId: botId, beforeSeq: first, limit: 100, sessionId: botId)
        }), !retired, linkId == link else { return }
        if older.rawCount < 100 { historyComplete.insert(botId) }
        for e in older.entries { upsert(e) }
    }
}
