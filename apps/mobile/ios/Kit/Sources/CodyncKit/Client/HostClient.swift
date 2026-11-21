// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation
import os

private let historyLog = Logger(subsystem: "app.mexerica.ios", category: "History")

public enum HostEvent: Sendable {
    case hello(hostId: String, rev: Int64, usage: Usage, screen: ScreenState?)
    case bot(Bot)
    case botDeleted(id: String, rev: Int64)
    case entry(Entry)
    case usage(Usage)
    case screen(ScreenState)
    case resync
    #if DEBUG
    /// Mission Board v1 (só Debug): um evento do ciclo que o app antes descartava (`session`, `conversation`, `spaces`,
    /// `donnaControlTowerChanged`). Só o tipo; nada do corpo é lido nem guardado: serve só para invalidar o quadro.
    case cycleChanged(type: String)
    #endif
    /// An event this app version couldn't decode (host newer/older than the app).
    case undecodable(type: String)
}

/// A setup terminal's output stream.
public enum TermEvent: Sendable {
    case output(Data)
    case exit(Int)
}

public struct Empty: Codable, Sendable {
    public init() {}
}

/// Only fields changed since the editor opened are written to the Hub.
struct BotUpdate: Encodable {
    let draft: BotDraft
    private enum Keys: String, CodingKey {
        case id, name, description, avatarColor, avatarShape, backend, command, cwd, permission, permissionChosenByUser
        case model, modelOptions, fastMode, pinned, hidden, notify, connectors, skills, computer, manageTeam, sharedMemory, harnessMemory
    }
    func encode(to encoder: Encoder) throws {
        guard let id = draft.id, let source = draft.source.bot, source.id == id else {
            throw EncodingError.invalidValue(draft, .init(codingPath: encoder.codingPath, debugDescription: "Missing original bot"))
        }
        let base = BotDraft(source)
        var c = encoder.container(keyedBy: Keys.self)
        try c.encode(id, forKey: .id)
        if draft.name != base.name { try c.encode(draft.name, forKey: .name) }
        if draft.description != base.description { try c.encode(draft.description, forKey: .description) }
        if draft.avatarColor != base.avatarColor { try c.encode(draft.avatarColor, forKey: .avatarColor) }
        if draft.avatarShape != base.avatarShape { try c.encode(draft.avatarShape, forKey: .avatarShape) }
        if draft.backend != base.backend { try c.encode(draft.backend, forKey: .backend) }
        if draft.command != base.command && (draft.backend == "custom" || draft.backend != base.backend) { try c.encode(draft.command, forKey: .command) }
        if draft.cwd != base.cwd { try c.encode(draft.cwd, forKey: .cwd) }
        if draft.permissionChosenByUser == true {
            try c.encode(draft.permission, forKey: .permission)
            try c.encode(true, forKey: .permissionChosenByUser)
        }
        if draft.model != base.model { try c.encode(draft.model, forKey: .model) }
        if draft.modelOptions != base.modelOptions { try c.encode(draft.modelOptions, forKey: .modelOptions) }
        if draft.fastMode != base.fastMode { try c.encodeIfPresent(draft.fastMode, forKey: .fastMode) }
        if draft.pinned != base.pinned { try c.encodeIfPresent(draft.pinned, forKey: .pinned) }
        if draft.hidden != base.hidden { try c.encodeIfPresent(draft.hidden, forKey: .hidden) }
        if draft.notify != base.notify { try c.encode(draft.notify, forKey: .notify) }
        if draft.connectors != base.connectors { try c.encodeIfPresent(draft.connectors, forKey: .connectors) }
        if draft.skills != base.skills { try c.encodeIfPresent(draft.skills, forKey: .skills) }
        if draft.computer != base.computer { try c.encodeIfPresent(draft.computer, forKey: .computer) }
        if draft.manageTeam != base.manageTeam { try c.encodeIfPresent(draft.manageTeam, forKey: .manageTeam) }
        if draft.sharedMemory != base.sharedMemory { try c.encodeIfPresent(draft.sharedMemory, forKey: .sharedMemory) }
        if draft.harnessMemory != base.harnessMemory { try c.encodeIfPresent(draft.harnessMemory, forKey: .harnessMemory) }
    }
}

struct GroupUpdate: Encodable {
    let draft: GroupDraft
    private enum Keys: String, CodingKey { case id, name, description, members, pinned }
    func encode(to encoder: Encoder) throws {
        guard let id = draft.id, let source = draft.source.bot, source.id == id else {
            throw EncodingError.invalidValue(draft, .init(codingPath: encoder.codingPath, debugDescription: "Missing original group"))
        }
        let base = GroupDraft(source)
        var c = encoder.container(keyedBy: Keys.self)
        try c.encode(id, forKey: .id)
        if draft.name != base.name { try c.encode(draft.name, forKey: .name) }
        if draft.description != base.description { try c.encode(draft.description, forKey: .description) }
        if draft.members != base.members { try c.encode(draft.members, forKey: .members) }
        if draft.pinned != base.pinned { try c.encodeIfPresent(draft.pinned, forKey: .pinned) }
    }
}

public struct HostClient: Sendable {
    public let transport: any HostTransport

    public init(transport: any HostTransport) {
        self.transport = transport
    }

    static let decoder = JSONDecoder()
    static let encoder = JSONEncoder()

    public func call<T: Decodable>(_ method: String, _ body: some Encodable = Empty(), timeout: TimeInterval = 20) async throws -> T {
        let data = try await transport.call(method, body: try Self.encoder.encode(body), timeout: timeout)
        return try Self.decoder.decode(T.self, from: data)
    }

    /// Catch-up since `since`, then live updates.
    public func events(since: Int64, client: String) -> AsyncThrowingStream<HostEvent, Error> {
        map(transport.stream(.events(since: since, client: client)), Self.parseEvent)
    }

    /// A setup terminal's output: everything so far, then live, finishing after `.exit`.
    public func termOutput(_ term: String) -> AsyncThrowingStream<TermEvent, Error> {
        map(transport.stream(.term(term))) { data in
            struct Raw: Decodable { var type: String; var data: String?; var code: Int? }
            guard let raw = try? Self.decoder.decode(Raw.self, from: data) else { return nil }
            switch raw.type {
            case "output": return raw.data.flatMap { Data(base64Encoded: $0) }.map(TermEvent.output)
            case "exit": return .exit(raw.code ?? -1)
            default: return nil
            }
        }
    }

    private func map<T: Sendable>(_ source: AsyncThrowingStream<Data, Error>, _ parse: @escaping @Sendable (Data) -> T?) -> AsyncThrowingStream<T, Error> {
        AsyncThrowingStream { continuation in
            let task = Task {
                do {
                    for try await data in source {
                        if let event = parse(data) { continuation.yield(event) }
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }

    public static func parseEvent(_ data: Data) -> HostEvent? {
        struct Envelope: Decodable {
            var type: String
            var hostId: String?
            var rev: Int64?
        }
        struct BotWrap: Decodable { var bot: Bot }
        struct StubWrap: Decodable { var bot: BotStub }
        struct EntryWrap: Decodable { var entry: Entry }
        struct UsageWrap: Decodable { var usage: Usage }
        struct ScreenWrap: Decodable { var screen: ScreenState }
        let d = decoder
        guard let env = try? d.decode(Envelope.self, from: data) else { return nil }
        switch env.type {
        case "hello":
            let usage = (try? d.decode(UsageWrap.self, from: data))?.usage ?? Usage()
            let screen = (try? d.decode(ScreenWrap.self, from: data))?.screen
            return .hello(hostId: env.hostId ?? "", rev: env.rev ?? 0, usage: usage, screen: screen)
        case "bot":
            if let stub = try? d.decode(StubWrap.self, from: data), stub.bot.deleted == true {
                return .botDeleted(id: stub.bot.id, rev: stub.bot.rev)
            }
            return (try? d.decode(BotWrap.self, from: data)).map { .bot($0.bot) } ?? .undecodable(type: "bot")
        case "entry":
            return (try? d.decode(EntryWrap.self, from: data)).map { .entry($0.entry) } ?? .undecodable(type: "entry")
        case "usage":
            return (try? d.decode(UsageWrap.self, from: data)).map { .usage($0.usage) }
        case "screen":
            return (try? d.decode(ScreenWrap.self, from: data)).map { .screen($0.screen) }
        case "resync":
            return .resync
        #if DEBUG
        case "session", "conversation", "spaces", "donnaControlTowerChanged":
            return .cycleChanged(type: env.type)
        #endif
        default:
            return nil
        }
    }
}

/// History entries and the number of rows before invalid session IDs were discarded.
public struct HistoryPage: Sendable {
    public let entries: [Entry]
    public let rawCount: Int
}

// MARK: - Typed calls

public extension HostClient {
    func hello() async throws -> Hello { try await call("hello") }

    func history(botId: String, beforeSeq: Int64, limit: Int = 100, sessionId: String? = nil) async throws -> HistoryPage {
        struct Body: Encodable { var botId: String; var beforeSeq: Int64; var limit: Int; var sessionId: String? }
        struct Row: Decodable {
            let entry: Entry?
            private enum Keys: String, CodingKey { case id }
            init(from decoder: Decoder) throws {
                do {
                    entry = try Entry(from: decoder)
                } catch let error as DecodingError {
                    let path: [any CodingKey]
                    switch error {
                    case .dataCorrupted(let context), .typeMismatch(_, let context), .valueNotFound(_, let context):
                        path = context.codingPath
                    case .keyNotFound(_, let context):
                        path = context.codingPath
                    @unknown default:
                        throw error
                    }
                    guard path.count == decoder.codingPath.count + 1, path.last?.stringValue == "sessionId" else { throw error }
                    let container = try decoder.container(keyedBy: Keys.self)
                    let id = (try? container.decode(String.self, forKey: .id)) ?? "<unknown>"
                    historyLog.debug("Discarded history entry with invalid sessionId: \(id, privacy: .private)")
                    entry = nil
                }
            }
        }
        struct Res: Decodable { var entries: [Row] }
        let res: Res = try await call("history", Body(botId: botId, beforeSeq: beforeSeq, limit: limit, sessionId: sessionId))
        return HistoryPage(entries: res.entries.compactMap(\.entry), rawCount: res.entries.count)
    }

    func sessions(botId: String) async throws -> BotSessions {
        try await call("sessions", ["botId": botId])
    }

    /// The newest 200 bot-to-bot notices between a bot's chat and a peer, oldest first.
    func botConversation(botId: String, peerId: String) async throws -> [Entry] {
        struct Res: Decodable { var entries: [Entry] }
        let res: Res = try await call("botConversation", ["botId": botId, "peerId": peerId])
        return res.entries
    }

    /// Creates a group chat (or returns the one these bots already share).
    func createGroup(_ draft: GroupDraft) async throws -> Bot {
        struct Res: Decodable { var bot: Bot }
        let res: Res = try await call("createBot", draft)
        return res.bot
    }

    func updateGroup(_ draft: GroupDraft) async throws -> Bot {
        struct Res: Decodable { var bot: Bot }
        let res: Res = try await call("updateBot", GroupUpdate(draft: draft))
        return res.bot
    }

    func createBot(_ draft: BotDraft) async throws -> Bot {
        struct Res: Decodable { var bot: Bot }
        let res: Res = try await call("createBot", draft)
        return res.bot
    }

    func updateBot(_ draft: BotDraft) async throws -> Bot {
        struct Res: Decodable { var bot: Bot }
        let res: Res = try await call("updateBot", BotUpdate(draft: draft))
        return res.bot
    }

    func deleteBot(_ id: String) async throws {
        let _: Empty = try await call("deleteBot", ["botId": id])
    }

    /// The roster as the user dragged it, top first.
    func reorderBots(_ ids: [String]) async throws {
        let _: Empty = try await call("reorderBots", ["ids": ids])
    }

    /// The chat read; a no-op when nothing there is unread.
    func markRead(_ id: String) async throws {
        let _: Empty = try await call("markRead", ["botId": id])
    }

    /// Toggles the user's `emoji` reaction on a chat message.
    func react(entryId: String, emoji: String) async throws -> Entry {
        struct Res: Decodable { var entry: Entry }
        let res: Res = try await call("react", ["entryId": entryId, "emoji": emoji])
        return res.entry
    }

    /// `replyTo`: the chat message this one quotes.
    /// `pasted`: what the user pasted rather than typed, which memory never takes as their own words.
    func send(botId: String, text: String, clientNonce: String, replyTo: String? = nil, attachments: [String]? = nil,
              pasted: [String]? = nil, sessionId: String? = nil) async throws -> Entry {
        struct Body: Encodable {
            var botId: String; var text: String; var clientNonce: String; var replyTo: String?; var attachments: [String]?
            var pasted: [String]?; var sessionId: String?
        }
        struct Res: Decodable { var entry: Entry }
        let body = Body(botId: botId, text: text, clientNonce: clientNonce, replyTo: replyTo, attachments: attachments,
                        pasted: pasted?.isEmpty == false ? pasted : nil, sessionId: sessionId)
        let res: Res = try await call("send", body)
        return res.entry
    }

    /// Bytes per `upload` call: base64 of it stays well under the channel's 1 MiB message.
    static let uploadChunk = 384 * 1024

    /// Sends one file in chunks for a later `send`; `id` names the upload there.
    func upload(botId: String, id: String, name: String, data: Data) async throws {
        struct Body: Encodable { var botId, uploadId, name, data: String; var offset: Int; var done: Bool }
        var offset = 0
        repeat {
            let end = min(offset + Self.uploadChunk, data.count)
            let chunk = data.subdata(in: offset..<end).base64EncodedString()
            let _: Empty = try await call("upload", Body(botId: botId, uploadId: id, name: name, data: chunk, offset: offset, done: end == data.count), timeout: 60)
            offset = end
        } while offset < data.count
    }

    /// A sent file, fetched back in chunks.
    func readUpload(botId: String, id: String) async throws -> Data {
        struct Body: Encodable { var botId, uploadId: String; var offset: Int }
        struct Res: Decodable { var data: String; var size: Int }
        var out = Data()
        repeat {
            let res: Res = try await call("readUpload", Body(botId: botId, uploadId: id, offset: out.count), timeout: 60)
            guard let chunk = Data(base64Encoded: res.data), !chunk.isEmpty else { break }
            out.append(chunk)
            if out.count >= res.size { break }
        } while true
        return out
    }

    func logCall(_ botId: String, seconds: Int) async throws {
        struct Body: Encodable { var botId: String; var seconds: Int }
        let _: Empty = try await call("logCall", Body(botId: botId, seconds: seconds))
    }

    func stop(_ botId: String) async throws {
        let _: Empty = try await call("stop", ["botId": botId])
    }

    func newSession(_ botId: String) async throws {
        let _: Empty = try await call("newSession", ["botId": botId])
    }

    func respondPermission(botId: String, entryId: String, entryRev: Int64, optionId: String?, source: String? = nil) async throws {
        struct Body: Encodable { var botId: String; var entryId: String; var entryRev: Int64; var optionId: String?; var source: String? }
        let _: Empty = try await call("respondPermission", Body(botId: botId, entryId: entryId, entryRev: entryRev, optionId: optionId, source: source))
    }

    /// Push tickets are bound to this device's key; `pushKey` lets the host seal notification text (§6.7).
    func registerDevice(ticket: String, relay: String, name: String, pushKey: String?, ctx: String) async throws {
        struct Body: Encodable { var ticket: String; var relay: String; var name: String; var pushKey: String?; var ctx: String }
        let _: Empty = try await call("registerDevice", Body(ticket: ticket, relay: relay, name: name, pushKey: pushKey, ctx: ctx))
    }

    /// Stops this device's alerts; used when notifications are switched off in the app.
    func unregisterDevice() async throws {
        let _: Empty = try await call("unregisterDevice")
    }

    func registerActivity(botId: String, ticket: String) async throws {
        let _: Empty = try await call("registerActivity", ["botId": botId, "ticket": ticket])
    }

    func usage(refresh: Bool = false) async throws -> Usage {
        try await call("usage", ["refresh": refresh])
    }

    func listDirs(_ path: String?) async throws -> DirListing {
        struct Body: Encodable { var path: String? }
        return try await call("listDirs", Body(path: path))
    }

    // MARK: remote screen

    func screenStatus() async throws -> ScreenState { try await call("screenStatus") }

    func screenPrepare() async throws -> ScreenConnection {
        let relay = try await transport.screenRelayRequired()
        return try await call("screenPrepare", ["relay": relay])
    }

    /// Sends a WebRTC offer (with all its ICE candidates); returns the session id and the answer.
    /// Passing `session` restarts ICE on an existing session.
    func screenOffer(sdp: String, session: String?, display: UInt32?) async throws -> ScreenAnswer {
        struct Body: Encodable { var sdp: String; var session: String?; var display: UInt32? }
        return try await call("screenOffer", Body(sdp: sdp, session: session, display: display), timeout: 30)
    }

    func screenClose(session: String) async throws {
        let _: Empty = try await call("screenClose", ["session": session])
    }

    /// Takes control from bots (they can still look) or hands it back.
    func screenTakeover(_ on: Bool) async throws -> ScreenState {
        try await call("screenTakeover", ["on": on])
    }

}

// MARK: - Agent setup

public extension HostClient {
    func refreshBackends() async throws -> [Backend] {
        struct Res: Decodable { var backends: [Backend] }
        let res: Res = try await call("refreshBackends", timeout: 60)
        return res.backends
    }

    /// Starts (or rejoins) installing or signing in to `backend` in a terminal on the computer.
    /// `method`: a terminal sign-in method from `agentAuth`; nil runs Codync's own command.
    func agentSetup(backend: String, step: SetupStep, method: String?, cols: Int, rows: Int) async throws -> String {
        struct Body: Encodable { var backend: String; var step: SetupStep; var method: String?; var cols: Int; var rows: Int }
        struct Res: Decodable { var term: String }
        // The first sign-in through a registry build can download it.
        let res: Res = try await call("agentSetup", Body(backend: backend, step: step, method: method, cols: cols, rows: rows), timeout: 300)
        return res.term
    }

    /// Starts the agent to see whether it's signed in and how it can be (first run may download it).
    func agentModels(_ backend: String) async throws -> AgentModels {
        try await call("agentModels", ["backend": backend], timeout: 300)
    }

    func agentAuth(_ backend: String) async throws -> AgentAuth {
        try await call("agentAuth", ["backend": backend], timeout: 300)
    }

    /// Lets the agent sign itself in; returns once it's done (or gave up).
    func agentAuthenticate(_ backend: String, method: String) async throws -> AgentAuth {
        try await call("agentAuthenticate", ["backend": backend, "method": method], timeout: 660)
    }

    /// Saves keys for the agent on the computer (empty values remove them).
    func setAgentEnv(_ backend: String, vars: [String: String]) async throws -> AgentAuth {
        struct Body: Encodable { var backend: String; var vars: [String: String] }
        return try await call("setAgentEnv", Body(backend: backend, vars: vars), timeout: 300)
    }

    func termInput(_ term: String, _ bytes: Data) async throws {
        let _: Empty = try await call("termInput", ["term": term, "data": bytes.base64EncodedString()])
    }

    func termResize(_ term: String, cols: Int, rows: Int) async throws {
        struct Body: Encodable { var term: String; var cols: Int; var rows: Int }
        let _: Empty = try await call("termResize", Body(term: term, cols: cols, rows: rows))
    }

    func termClose(_ term: String) async throws {
        let _: Empty = try await call("termClose", ["term": term])
    }
}

public struct ScreenAnswer: Codable, Sendable {
    public var session: String
    public var sdp: String
}
