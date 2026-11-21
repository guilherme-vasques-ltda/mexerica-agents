// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import Foundation
import os

private let log = Logger(subsystem: "app.mexerica.ios", category: "BotStore")

extension BotStore {
    /// Events (catch-up since `rev`, then live) for as long as the link stays ready.
    func runEvents(_ client: HostClient) async {
        var backoff: Double = 1
        while !Task.isCancelled {
            do {
                // Authority belongs to the channel the Hub identifies itself on now; every action after
                // this goes out on that channel or fails.
                guard let link = await client.transport.currentLink() else { throw HostError.unreachable }
                guard !Task.isCancelled, !retired else { return }
                let vinculado = HostClient(transport: TransporteDoLink(
                    base: client.transport, link: link,
                    aoRecusar: { [weak self] motivo in
                        Task { @MainActor in self?.soltarPorRecusa(motivo) }
                    },
                    aoDuvidar: { [weak self] in
                        Task { @MainActor in self?.reverificarAcesso() }
                    }))
                try await refreshHello(vinculado)
                guard !Task.isCancelled, !retired else { return }
                try validarIdentidade()
                reaproveitarOuRecomecar()
                let identificouAgora = clienteAutorizado == nil
                clienteAutorizado = vinculado
                // Push registration and the like run only on a Hub that identified itself.
                if identificouAgora { onConnected?(self) }
                if mismatch != nil {
                    // Reached, but nothing to sync until one side updates. A host update restarts
                    // the link (and this loop) anyway; this catches one that doesn't.
                    setConnection(.online)
                    try await Task.sleep(for: Self.mismatchRecheck)
                    continue
                }
                requestedSince = rev
                windowFloors = [:]
                for try await event in client.events(since: requestedSince, client: "ios") {
                    guard !Task.isCancelled, !retired else { return }
                    #if DEBUG
                    // Mission Board v1: um sinal do ciclo não muda o espelho, então não mexe na conexão, no backoff nem
                    // no salvamento pendente (o app antigo fica igual ao da main); só avisa o observador.
                    if case .cycleChanged = event {
                        quadroSinais.registrar(MXQuadroSinal.de(event))
                        continue
                    }
                    #endif
                    setConnection(.online)
                    backoff = 1
                    apply(event)
                }
            } catch is CancellationError {
                return
            } catch {
                if Task.isCancelled || retired { return }
                clienteAutorizado = nil
                log.info("stream ended: \(error.localizedDescription)")
                // 401/403 from `hello` or the events stream end this device's access, like `.unauthorized`.
                if let motivo = (error as? HostError)?.recusaDefinitiva(identificando: true) {
                    soltarPorRecusa(motivo)
                    return
                }
            }
            try? await Task.sleep(for: .seconds(backoff))
            backoff = min(backoff * 2, 20)
        }
    }

    /// After a drop the mirror (bots, open chat, drafts) is kept only for the same account, computer and
    /// server key it was loaded for; anything else starts over from scratch (fail-closed).
    func reaproveitarOuRecomecar() {
        // What the Hub just said it is (checked against this computer by `validarIdentidade`).
        let atual = DonoDoEspelho(conta: storage.accountID, computador: hello?.computerId ?? computer.id,
                                  chave: hello?.signKey ?? computer.signKey)
        guard !DonoDoEspelho.podeReaproveitar(donoDoEspelho, atual) else { return }
        limparTudo()
        donoDoEspelho = atual
    }

    /// The Hub that answered must be this computer: a `hello` naming another computer or server key is a
    /// definitive refusal (the key changed), never a mirror to reuse.
    func validarIdentidade() throws {
        guard let h = hello else { return }
        if let id = h.computerId, id != computer.id { throw HostError.unauthorized(HostError.identidadeMudou) }
        if let key = h.signKey, key != computer.signKey { throw HostError.unauthorized(HostError.identidadeMudou) }
    }

    /// Another owner: nothing of the old one stays, not even messages waiting to be sent, their files or
    /// pasted text, so none can reappear or go out here. Unlike `resetMirror` (same owner, new host
    /// database), which keeps those for a retry.
    private func limparTudo() {
        resetMirror()
        entries = [:]
        outgoingFiles = [:]
        composerPasted = [:]
        answering = [:]
        saveCache()
    }

    /// Asked on every (re)connect: the host may have been updated while this app was away.
    private func refreshHello(_ client: HostClient) async throws {
        let h: Hello
        do {
            h = try await client.hello()
        } catch let error as DecodingError {
            // Its version and `minApp` still say which side to update.
            let version: HostVersion = try await client.call("hello")
            guard !Task.isCancelled, !retired else { return }
            log.error("undecodable hello from host \(version.version)")
            noteHostVersion(version)
            Motion.animate { unreadable = true }
            if mismatch == nil { throw error }
            return
        }
        guard !Task.isCancelled, !retired else { return }
        // hello first: whatever the version change shows (notices, reminders) reads it.
        hello = h
        noteHostVersion(HostVersion(version: h.version, minApp: h.minApp))
    }

    private func noteHostVersion(_ new: HostVersion) {
        let cached = cacheStamp.map { $0 != "\(Self.appBuild)/\(new.version)" } ?? false
        if cached || (hostVersion.map { $0.version != new.version } ?? false) {
            // A different host version or app build: data may carry new fields, so fetch it all again.
            refetchAll()
            sessionSendSupported = []
            rewound = false
            unreadable = false
        }
        cacheStamp = nil
        // An update notice may appear or go away with it.
        if new != hostVersion { Motion.animate { hostVersion = new } }
    }

    private func apply(_ event: HostEvent) {
        switch event {
        case let .hello(id, hostRev, newUsage, newScreen):
            if let hostId, hostId != id {
                // Different host database: start over.
                resetMirror()
            }
            hostId = id
            setUsage(newUsage)
            screen = newScreen
            let hostRewound = hostRev < rev
            if hostRewound { rev = 0 }
            windowFloors = requestedSince > 0 && !hostRewound ? loadedFloors() : [:]
            catchUpRev = hostRev
            bump(rev)
        case let .bot(bot):
            let neededInput = bots[bot.id]?.needsInput == true
            bots[bot.id] = bot
            bump(bot.rev)
            onBotUpdated?(bot)
            onRosterChanged?()
            if bot.unread > 0 { acknowledgeVisibleConversations(bot.id) }
            if bot.needsInput && !neededInput {
                for call in Array(voiceCalls.values) where call.botId == bot.id {
                    call.announce(String(localized: "\(bot.name) precisa da sua aprovação na conversa.", bundle: .module))
                }
            }
        case let .botDeleted(id, r):
            removeComposerDrafts(for: id)
            bots[id] = nil
            entries[id] = nil
            windowFloors[id] = nil
            bump(r)
            if selection == id { selection = nil }
            onRosterChanged?()
        case let .entry(e):
            if (e.sessionId == nil || e.sessionId == e.botId), Self.isOutsideLoadedWindow(e, floor: windowFloors[e.botId], held: entries[e.botId]?.contains { $0.id == e.id } == true) {
                bump(e.rev)
                break
            }
            let known = entries[e.botId]?.first(where: { $0.id == e.id })
            upsert(e)
            bump(e.rev)
            guard e.sessionId == nil || e.sessionId == e.botId else { break }
            acknowledgeVisibleConversations(e.botId, entry: e)
            let calls = Array(voiceCalls.values).filter { $0.botId == e.botId && e.rev > $0.startRev }
            if e.kind == "agent", e.data.final == true, known?.data.final != true, let text = e.data.text {
                calls.forEach { $0.speak(spokenReply(e, text)) }
            }
            // A group has no status of its own: its members' approvals arrive as cards.
            if e.kind == "permission", known == nil, e.data.status == "pending", bots[e.botId]?.isGroup == true {
                let name = e.data.author.flatMap { bots[$0]?.name } ?? String(localized: "Um bot", bundle: .module)
                calls.forEach { $0.announce(String(localized: "\(name) precisa da sua aprovação na conversa.", bundle: .module)) }
            }
        case let .usage(u):
            setUsage(u)
        case let .screen(s):
            screen = s
        case .resync:
            restartEvents()
        case let .undecodable(type):
            // Never skip past data we couldn't read: rewind once and fetch everything again.
            log.error("undecodable \(type) event")
            if !rewound {
                rewound = true
                refetchAll()
                restartEvents()
            } else if !unreadable {
                Motion.animate { unreadable = true }
                // A newer host this app can't follow: stop syncing and ask for an update.
                if mismatch != nil { restartEvents() }
            }
        #if DEBUG
        case .cycleChanged:
            // Já tratado em `runEvents`, sem chegar aqui; se chegar, não agenda salvamento nenhum.
            quadroSinais.registrar(MXQuadroSinal.de(event))
            return
        #endif
        }
        #if DEBUG
        quadroSinais.registrar(MXQuadroSinal.de(event))
        #endif
        scheduleSave()
    }

    /// In a group, a reply is said with its speaker's name.
    private func spokenReply(_ e: Entry, _ text: String) -> String {
        guard bots[e.botId]?.isGroup == true, let name = e.data.author.flatMap({ bots[$0]?.name }) else { return text }
        return "\(name): \(text)"
    }

    private func resetMirror() {
        composerDrafts = [:]
        persistComposerDrafts()
        bots = [:]
        entries = entries.mapValues { $0.filter { $0.id.hasPrefix("local-") } }.filter { !$0.value.isEmpty }
        selection = nil
        onRosterChanged?()
        rev = 0
        hostId = nil
        historyComplete = []
        sessionSendSupported = []
        windowFloors = [:]
        screen = nil
        saveCache()
    }

    /// Fetches everything again from rev 0. That catch-up brings only each chat's newest entries, so
    /// the ones held now go too: an older one kept would hide the gap from `loadOlder`, which
    /// pages from the oldest one held. Messages still sending stay.
    private func refetchAll() {
        rev = 0
        entries = entries.mapValues { $0.filter { $0.id.hasPrefix("local-") } }
            .filter { !$0.value.isEmpty }
        historyComplete = []
    }

    /// Resubscribes from the current `rev` on the same link.
    private func restartEvents() {
        guard let client = clienteBase, eventsTask != nil else { return }
        eventsTask?.cancel()
        eventsTask = Task { [weak self] in await self?.runEvents(client) }
    }

    private func bump(_ r: Int64) {
        rev = max(rev, r)
        if !caughtUp && rev >= catchUpRev { caughtUp = true }
    }

    /// Per bot, the lowest synced `seq` in the mirror (optimistic entries don't count).
    /// Only entries the mirror had at `requestedSince`: newer ones (a send's RPC response landing
    /// before this hello) come again in this catch-up and say nothing about the window.
    private func loadedFloors() -> [String: Int64] {
        entries.compactMapValues { list in
            list.filter { ($0.sessionId == nil || $0.sessionId == $0.botId) && $0.seq > 0 && $0.seq != .max && $0.rev <= requestedSince }.map(\.seq).min()
        }
    }

    /// An entry event the mirror doesn't hold, below the floor the mirror had
    /// when this connection started: it existed before the window and arrives only because its
    /// rev was bumped (a rewrite, a reaction). Inserting it would hide the gap from `loadOlder`,
    /// which pages from the oldest loaded `seq`; history paging brings it. `seq` follows insert
    /// order, so anything created since the mirror was last synced sits above the floor.
    static func isOutsideLoadedWindow(_ e: Entry, floor: Int64?, held: Bool) -> Bool {
        guard !held, let floor else { return false }
        return e.seq < floor
    }

    func upsert(_ e: Entry) {
        var list = entries[e.botId] ?? []
        if let i = list.firstIndex(where: { $0.id == e.id }) {
            // A late API response mustn't undo a newer SSE update.
            guard e.rev >= list[i].rev else { return }
            list[i] = e
        } else {
            // Replace the optimistic copy of a message we sent.
            if e.kind == "user", let nonce = e.data.clientNonce, !nonce.isEmpty {
                list.removeAll { $0.id == "local-\(nonce)" }
            }
            if let last = list.last, last.seq > e.seq, e.seq > 0 {
                let i = list.firstIndex { $0.seq > e.seq } ?? list.endIndex
                list.insert(e, at: i)
            } else {
                list.append(e)
            }
        }
        entries[e.botId] = list
    }

    func setUsage(_ u: Usage) {
        guard u != usage, !retired else { return }
        usage = u
        storage.usage[computer.id] = u
        onUsageChanged?(computer.id, u)
    }
}
