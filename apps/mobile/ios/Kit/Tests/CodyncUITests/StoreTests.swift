// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CryptoKit
import Foundation
import Observation
import Testing
@testable import CodyncKit
@testable import CodyncUI

/// A scripted transport: tests set its link state and push events; it records what the store asked.
actor FakeRemote: RemoteTransport {
    private var state: LinkState
    private var stateSinks: [AsyncStream<LinkState>.Continuation] = []
    private var eventSinks: [AsyncThrowingStream<Data, Error>.Continuation] = []
    private var mailboxSinks: [AsyncStream<MailboxEvent>.Continuation] = []
    private(set) var sent: [String] = []
    private(set) var sendSessions: [String?] = []
    /// Each `send`'s `pasted` payload (empty when the key is absent).
    private(set) var sentPasted: [[String]] = []
    private(set) var historyBodies: [Data] = []
    private(set) var updateBodies: [Data] = []
    private(set) var queuedSessions: [String?] = []
    private var enqueueWaiters: [CheckedContinuation<Void, Never>] = []
    func waitForEnqueue() async {
        if !enqueued.isEmpty { return }
        await withCheckedContinuation { enqueueWaiters.append($0) }
    }
    /// Each `send`'s quoted message, in order.
    private(set) var replyTos: [String?] = []
    private(set) var readReceipts: [Data] = []
    private var failReadReceipts = false
    private(set) var readAttempts = 0
    private(set) var shutdownCount = 0
    private var isShutdown = false

    func setReadFailure(_ fail: Bool) { failReadReceipts = fail }
    private(set) var calls: [String] = []
    private var failures: [String: Int] = [:]
    /// The next `count` calls of `method` fail as if the link dropped under them.
    func fail(_ method: String, times count: Int) { failures[method] = count }
    private(set) var enqueued: [String] = []
    var cancelResult = MailboxCancel.cancelled
    /// The channel generation: a call bound to an older one fails without going out.
    private var generation = 1
    private var replaceAfter: String?
    /// Right after the next `method` call, the link is replaced by a new channel (a reconnect in between).
    func replaceLink(after method: String) { replaceAfter = method }
    func replaceLink() { generation += 1 }
    private var heldRefusal: (method: String, error: HostError)?
    private var refusalWaiter: CheckedContinuation<Void, Never>?
    var refusalWaiting: Bool { refusalWaiter != nil }
    func holdRefusal(_ method: String, _ error: HostError) { heldRefusal = (method, error) }
    func releaseRefusal() {
        refusalWaiter?.resume()
        refusalWaiter = nil
    }
    private func lateRefusal(_ method: String, onLink link: Int) async throws {
        guard !isShutdown, generation == link, let heldRefusal, heldRefusal.method == method else { return }
        self.heldRefusal = nil
        await withCheckedContinuation { refusalWaiter = $0 }
        throw heldRefusal.error
    }
    private var errors: [String: HostError] = [:]
    /// Every `method` call fails with `error` from now on (a real 401/403 from the host).
    func fail(_ method: String, with error: HostError) { errors[method] = error }
    private var helloHeld = false
    private var helloWaiters: [CheckedContinuation<Void, Never>] = []
    var helloWaiting: Int { helloWaiters.count }
    /// `hello` answers only after `releaseHello()` (the Hub not identified yet).
    func holdHello() { helloHeld = true }
    func releaseHello() {
        helloHeld = false
        let waiters = helloWaiters
        helloWaiters = []
        for w in waiters { w.resume() }
    }
    private func helloGate() async {
        guard helloHeld else { return }
        await withCheckedContinuation { helloWaiters.append($0) }
    }

    private var sessionResponse = #"{"sessions":[],"sessionActions":["create","send"]}"#
    private var historyResponse = #"{"entries":[]}"#
    func setSessionResponse(_ json: String) { sessionResponse = json }
    func setHistoryResponse(_ json: String) { historyResponse = json }

    private var hello = #"{"hostId":"h1","name":"Mac","version":"3.0.0","os":"macos","backends":[],"rev":0}"#
    /// What `hello` answers from now on (a host updated in place keeps its link).
    func setHello(_ json: String) { hello = json }

    /// One element per events subscription, as it happens: tests wait on it instead of polling.
    let inscricoes: AsyncStream<Void>
    private let avisarInscricao: AsyncStream<Void>.Continuation

    init(_ state: LinkState) {
        self.state = state
        let made = AsyncStream<Void>.makeStream()
        inscricoes = made.stream
        avisarInscricao = made.continuation
    }

    var subscribed: Bool { !eventSinks.isEmpty }
    var eventSubscriptionCount: Int { eventSinks.count }

    func set(_ new: LinkState) {
        state = new
        for s in stateSinks { s.yield(new) }
    }

    func emit(_ json: String) {
        for s in eventSinks { s.yield(Data(json.utf8)) }
    }

    private var linkAvailable = true
    func loseLinkWithoutStateUpdate() {
        linkAvailable = false
        for s in eventSinks { s.finish() }
    }

    func emit(_ event: MailboxEvent) {
        for s in mailboxSinks { s.yield(event) }
    }

    private func respond(_ method: String, _ body: Data, link: Int? = nil) throws -> Data {
        guard !isShutdown else { throw HostError.unreachable }
        if let link, link != generation { throw HostError.unreachable }
        calls.append(method)
        defer {
            if replaceAfter == method {
                replaceAfter = nil
                generation += 1
            }
        }
        if let error = errors[method] { throw error }
        if let left = failures[method], left > 0 {
            failures[method] = left - 1
            throw HostError.unreachable
        }
        switch method {
        case "hello":
            return Data(hello.utf8)
        case "markRead":
            readAttempts += 1
            if failReadReceipts { throw HostError.unreachable }
            readReceipts.append(body)
            return Data("{}".utf8)
        case "sessions":
            return Data(sessionResponse.utf8)
        case "history":
            historyBodies.append(body)
            return Data(historyResponse.utf8)
        case "updateBot":
            updateBodies.append(body)
            return Data(#"{"bot":{"id":"b1","name":"Bot","rev":2}}"#.utf8)
        case "send":
            let bruto = (try JSONSerialization.jsonObject(with: body) as? [String: Any]) ?? [:]
            let b = bruto.compactMapValues { $0 as? String }
            sentPasted.append(bruto["pasted"] as? [String] ?? [])
            sent.append(b["text"] ?? "")
            sendSessions.append(b["sessionId"])
            replyTos.append(b["replyTo"])
            let entry = #"{"id":"e\#(sent.count)","seq":\#(sent.count),"botId":"\#(b["botId"] ?? "")","rev":\#(sent.count),"kind":"user","turn":1,"data":{"text":"\#(b["text"] ?? "")","clientNonce":"\#(b["clientNonce"] ?? "")"},"createdAt":1,"updatedAt":1}"#
            let session = b["sessionId"].map { #", "sessionId":"\#($0)""# } ?? ""
            let withSession = String(entry.dropLast()) + session + "}"
            return Data(#"{"entry":\#(withSession)}"#.utf8)
        default:
            return Data("{}".utf8)
        }
    }

    nonisolated func call(_ method: String, body: Data, timeout: TimeInterval) async throws -> Data {
        if method == "hello" { await helloGate() }
        return try await respond(method, body)
    }

    nonisolated func call(_ method: String, body: Data, timeout: TimeInterval, onLink link: Int) async throws -> Data {
        if method == "hello" { await helloGate() }
        try await lateRefusal(method, onLink: link)
        return try await respond(method, body, link: link)
    }

    func currentLink() async -> Int? {
        guard !isShutdown, linkAvailable, case .ready = state else { return nil }
        return generation
    }

    private func addEvents(_ c: AsyncThrowingStream<Data, Error>.Continuation) {
        eventSinks.append(c)
        avisarInscricao.yield(())
    }
    private func addState(_ c: AsyncStream<LinkState>.Continuation) {
        isShutdown = false
        c.yield(state)
        stateSinks.append(c)
    }
    private func addMailbox(_ c: AsyncStream<MailboxEvent>.Continuation) { mailboxSinks.append(c) }

    nonisolated func stream(_ request: HostStreamRequest) -> AsyncThrowingStream<Data, Error> {
        AsyncThrowingStream { c in Task { await self.addEvents(c) } }
    }

    nonisolated func states() -> AsyncStream<LinkState> {
        AsyncStream { c in Task { await self.addState(c) } }
    }

    nonisolated func computerUpdates() -> AsyncStream<Computer> { AsyncStream { _ in } }

    func enqueue(botId: String, text: String, clientNonce: String, replyTo: String?, sessionId: String?) async throws {
        enqueued.append(clientNonce)
        queuedSessions.append(sessionId)
        let waiters = enqueueWaiters
        enqueueWaiters = []
        for waiter in waiters { waiter.resume() }
    }
    func cancelQueued(clientNonce: String) async -> MailboxCancel { cancelResult }
    func listQueued() async throws -> [QueuedItem] { enqueued.map { QueuedItem(nonce: $0, exp: nil, state: "queued") } }

    nonisolated func mailboxEvents() -> AsyncStream<MailboxEvent> {
        AsyncStream { c in Task { await self.addMailbox(c) } }
    }

    func shutdown() async {
        shutdownCount += 1
        isShutdown = true
        for s in eventSinks { s.finish() }
        for s in stateSinks { s.finish() }
        eventSinks = []
        stateSinks = []
    }
}

private func randomComputer(_ name: String) -> Computer {
    let sk = Curve25519.Signing.PrivateKey().publicKey.rawRepresentation
    return Computer(id: RelayCrypto.computerId(signKey: sk), name: name, signKey: sk.base64URLForTests,
                    boxKey: Curve25519.KeyAgreement.PrivateKey().publicKey.rawRepresentation.base64URLForTests,
                    cloud: URL(string: "https://cloud.example.dev"))
}

private extension Data {
    var base64URLForTests: String {
        base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    }
}

private func context() -> (SharedStore.Context, String) {
    let suite = "CodyncUITests.\(UUID().uuidString)"
    return (SharedStore.Context(accountID: "user_\(UUID().uuidString)", suite: suite), suite)
}

/// Polls until `condition` holds (2 s at most).
@MainActor
private func until(_ condition: @MainActor () async -> Bool) async -> Bool {
    for _ in 0..<200 {
        if await condition() { return true }
        try? await Task.sleep(for: .milliseconds(10))
    }
    return false
}

private func botEvent(_ id: String, name: String, rev: Int) -> String {
    #"{"type":"bot","bot":{"id":"\#(id)","name":"\#(name)","rev":\#(rev),"lastAt":\#(rev)}}"#
}

@MainActor @Test func voiceCallSendsAndReceivesInBackgroundWithoutViewUpdates() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.connection == .online })

    let call = UUID()
    var spoken: [String] = []
    store.beginVoiceCall(call, botId: "b1", speak: { spoken.append($0) }, end: {})
    store.setActive(false)
    // Allow shutdown to run if backgrounding incorrectly scheduled one.
    try await Task.sleep(for: .milliseconds(30))
    store.send("spoken while backgrounded", to: "b1")
    #expect(await until { store.chat("b1").first?.id == "e1" })
    #expect(await fake.sent == ["spoken while backgrounded"])
    #expect(await fake.shutdownCount == 0)

    let reply = #"{"type":"entry","entry":{"id":"reply","seq":2,"botId":"b1","rev":2,"kind":"agent","turn":1,"data":{"text":"The answer","final":true},"createdAt":1,"updatedAt":1}}"#
    await fake.emit(reply)
    #expect(await until { spoken == ["The answer"] })
    await fake.emit(reply) // Event replays must not read the same reply twice.
    await fake.emit(#"{"type":"entry","entry":{"id":"other","seq":4,"botId":"b2","rev":4,"kind":"agent","turn":1,"data":{"text":"Other bot","final":true},"createdAt":1,"updatedAt":1}}"#)
    #expect(await until { store.chat("b2").last?.id == "other" })
    #expect(spoken == ["The answer"])

    store.setActive(true)
    #expect(await fake.shutdownCount == 0)
    store.endVoiceCall(call)
    #expect(await fake.shutdownCount == 0) // Foreground keeps the ordinary connection.
    store.setActive(false)
    #expect(await until { await fake.shutdownCount == 1 })
}

@MainActor @Test func lastVoiceCallEndingDisconnectsBackgroundStore() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.relay))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.connection == .online })
    let first = UUID(), second = UUID()
    store.beginVoiceCall(first, botId: "b1", speak: { _ in }, end: {})
    store.beginVoiceCall(second, botId: "b1", speak: { _ in }, end: {})
    store.setActive(false)
    store.endVoiceCall(first)
    try await Task.sleep(for: .milliseconds(30))
    #expect(await fake.shutdownCount == 0)
    store.endVoiceCall(second)
    #expect(await until { await fake.shutdownCount == 1 })
}

@MainActor @Test func retiringStoreEndsVoiceCallAndPreventsLaterSends() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.connection == .online })
    let call = UUID()
    var ended = false
    store.beginVoiceCall(call, botId: "b1", speak: { _ in Issue.record("Retired call received speech") }, end: { ended = true })
    store.setActive(false)
    store.retire()
    #expect(ended)
    #expect(await until { await fake.shutdownCount == 1 })
    store.send("must not send", to: "b1")
    store.beginVoiceCall(UUID(), botId: "b1", speak: { _ in }, end: {})
    #expect(store.chat("b1").isEmpty)
    #expect(await fake.sent.isEmpty)
}

@MainActor @Test func offlineSendsWaitInTheMailbox() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.hostOffline(lastSeen: nil))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    store.setActive(true)
    #expect(await until { store.connection == .computerOffline(lastSeen: nil) })
    #expect(store.canQueue && store.isOffline)

    store.send("hi", to: "b1")
    #expect(await until { await fake.enqueued.count == 1 })
    let waiting = try #require(store.chat("b1").first)
    #expect(waiting.data.status == "waiting")
    #expect(await fake.sent.isEmpty)

    // Cancelled before the computer saw it: gone.
    store.cancelQueued(waiting)
    #expect(await until { store.chat("b1").isEmpty })

    // Already handed over: it stays, marked delivered.
    store.send("second", to: "b1")
    #expect(await until { await fake.enqueued.count == 2 })
    await fake.setCancelResult(.delivering)
    let second = try #require(store.chat("b1").first)
    store.cancelQueued(second)
    #expect(await until { store.chat("b1").first?.data.status == "delivering" })

    // Expired in the mailbox: failed, can be resent.
    store.send("third", to: "b2")
    #expect(await until { await fake.enqueued.count == 3 })
    let third = try #require(store.chat("b2").first?.data.clientNonce)
    await fake.emit(.expired(nonce: third))
    #expect(await until { store.chat("b2").first?.data.status == "failed" })

    // Back online: sends go straight to the computer, and its copy replaces the local one.
    await fake.set(.ready(.relay))
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Rex", rev: 1))
    #expect(await until { store.connection == .online })
    store.send("live", to: "b3")
    #expect(await until { store.chat("b3").first?.id == "e1" })
    #expect(await fake.sent == ["live"])
    store.retire()
}

@MainActor @Test func accountKeepsComputersApart() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let c1 = randomComputer("Mac")
    let c2 = randomComputer("Linux box")
    storage.computers = [c1, c2]
    let fakes = [c1.id: FakeRemote(.ready(.direct)), c2.id: FakeRemote(.ready(.relay))]
    let account = AccountStore(storage: storage, cloud: nil) { computer in
        BotStore(computer: computer, storage: storage) { _ in fakes[computer.id]! }
    }
    var updates: [BotReference] = []
    account.onBotUpdated = { ref, _ in updates.append(ref) }
    #expect(account.computers.map(\.id) == [c1.id, c2.id])
    for fake in fakes.values { #expect(await until { await fake.subscribed }) }

    // Same bot ID on both computers.
    await fakes[c1.id]!.emit(botEvent("b1", name: "Mac bot", rev: 1))
    await fakes[c2.id]!.emit(botEvent("b1", name: "Linux bot", rev: 2))
    #expect(await until { account.roster.count == 2 })
    let refs = account.roster.map(\.ref)
    #expect(Set(refs).count == 2)
    #expect(Set(refs.map(\.computerId)) == [c1.id, c2.id])
    #expect(account.roster.first { $0.ref.computerId == c2.id }?.bot.name == "Linux bot")
    #expect(Set(updates) == Set(refs))

    // Routing by reference reaches only that computer.
    let ref = try #require(refs.first { $0.computerId == c2.id })
    account.store(for: ref.computerId)?.send("hello", to: ref.botId)
    #expect(await until { await fakes[c2.id]!.sent == ["hello"] })
    #expect(await fakes[c1.id]!.sent.isEmpty)
    #expect(storage.lastComputerId == c2.id)

    // After switching accounts, late events write nothing.
    let old = account.store(for: c1.id)
    account.retire()
    await fakes[c1.id]!.emit(#"{"type":"usage","usage":{"providers":[{"id":"claude","name":"Claude","windows":[],"source":"x","updatedAt":1}]}}"#)
    try? await Task.sleep(for: .milliseconds(100))
    #expect(storage.usage[c1.id] == nil)
    #expect(old?.usage.providers.isEmpty == true)
    #expect(updates.count == 2)
}

@MainActor @Test func computersReorderAndStaySaved() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let a = randomComputer("A"), b = randomComputer("B"), c = randomComputer("C")
    storage.computers = [a, b, c]
    let account = AccountStore(storage: storage, cloud: nil) { computer in
        BotStore(computer: computer, storage: storage) { _ in FakeRemote(.ready(.direct)) }
    }
    account.move(a.id, to: c.id)
    #expect(account.computers.map(\.id) == [b.id, c.id, a.id])
    account.move(a.id, to: b.id)
    #expect(account.computers.map(\.id) == [a.id, b.id, c.id])
    account.move(c.id, to: b.id)
    #expect(storage.computers.map(\.id) == [a.id, c.id, b.id])
    account.retire()
}

@MainActor @Test func savedComputersKeepTheirColorAndCanBeForgotten() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let saved = randomComputer("Saved")
    storage.computers = [saved]
    let account = AccountStore(storage: storage, cloud: nil) { _ in
        BotStore(computer: saved, storage: storage) { _ in FakeRemote(.ready(.direct)) }
    }
    account.setColor(saved.id, "red")
    #expect(storage.computers.first?.color == "red")
    account.forget(saved.id)
    #expect(storage.computers.isEmpty && account.stores.isEmpty)
    account.retire()
}

extension FakeRemote {
    func setCancelResult(_ result: MailboxCancel) { cancelResult = result }
}


@MainActor @Test func visibleConversationAcknowledgesEntriesBeforeUnreadAndFinalUpdates() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    store.setActive(true)
    defer { store.setActive(false) }
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.connection == .online })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.bots["b1"] != nil })
    let token = UUID()
    store.setReading(token, botId: "b1", active: true)
    #expect(await until { await fake.readReceipts.count == 1 })
    // Streaming and final content have the same ID. The final update must be read
    // even though the roster has not yet reported any unread messages.
    await fake.emit(#"{"type":"entry","entry":{"id":"answer","seq":1,"botId":"b1","rev":2,"kind":"agent","turn":1,"data":{"text":"draft","final":false},"createdAt":1,"updatedAt":1}}"#)
    #expect(await until { store.chat("b1").last?.rev == 2 })
    await fake.emit(#"{"type":"entry","entry":{"id":"answer","seq":1,"botId":"b1","rev":3,"kind":"agent","turn":1,"data":{"text":"done","final":true},"createdAt":1,"updatedAt":2}}"#)
    #expect(await until { await fake.readReceipts.count == 2 })
    await fake.emit(#"{"type":"bot","bot":{"id":"b1","name":"Bot","rev":4,"unread":1}}"#)
    #expect(await until { await fake.readReceipts.count == 3 })
    store.setReading(token, botId: "b1", active: false)
    await fake.emit(#"{"type":"bot","bot":{"id":"b1","name":"Bot","rev":5,"unread":2}}"#)
    #expect(await until { store.bots["b1"]?.rev == 5 })
    try await Task.sleep(for: .milliseconds(50))
    #expect(await fake.readReceipts.count == 3)
    // Returning to the foreground acknowledges without needing navigation.
    store.setReading(token, botId: "b1", active: true)
    #expect(await until { await fake.readReceipts.count == 4 })
}

@MainActor @Test func readingOneChatDoesNotAcknowledgeAnother() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    store.setActive(true)
    defer { store.setActive(false) }
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.connection == .online })
    let token = UUID()
    store.setReading(token, botId: "b1", active: true)
    #expect(await until { await fake.readReceipts.count == 1 })
    await fake.emit(#"{"type":"entry","entry":{"id":"other","seq":1,"botId":"b2","rev":2,"kind":"agent","turn":1,"data":{"text":"done","final":true},"createdAt":1,"updatedAt":1}}"#)
    #expect(await until { store.chat("b2").last?.id == "other" })
    try await Task.sleep(for: .milliseconds(50))
    #expect(await fake.readReceipts.count == 1)
    await fake.emit(#"{"type":"entry","entry":{"id":"own","seq":2,"botId":"b1","rev":3,"kind":"agent","turn":1,"data":{"text":"done","final":true},"createdAt":1,"updatedAt":1}}"#)
    #expect(await until { await fake.readReceipts.count == 2 })
    for receipt in await fake.readReceipts {
        let body = try #require(JSONSerialization.jsonObject(with: receipt) as? [String: Any])
        #expect(body.keys.sorted() == ["botId"])
        #expect(body["botId"] as? String == "b1")
    }
}

@MainActor @Test func initialOfflineReportWaitsForReconnect() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.hostOffline(lastSeen: nil))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    try await Task.sleep(for: .milliseconds(200))
    #expect(store.connection == .connecting)
    #expect(store.lastError == nil)
    await fake.set(.ready(.relay))
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.connection == .online })
    try await Task.sleep(for: .seconds(1))
    #expect(store.connection == .online)
}

@MainActor @Test func initialUnreachableEventuallyShowsOffline() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.failed("Can't reach"))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    try await Task.sleep(for: .milliseconds(200))
    #expect(store.connection == .connecting)
    #expect(await until { store.connection == .offline("Can't reach") })
    #expect(store.lastError == nil)
}

@MainActor @Test func failedAutomaticReadDoesNotInterruptChatAndRecoversWithTheLink() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.connection == .online })
    await fake.setReadFailure(true)
    store.setReading(UUID(), botId: "b1", active: true)
    #expect(await until { await fake.readAttempts == 1 })
    try await Task.sleep(for: .milliseconds(50))
    #expect(store.lastError == nil)
    await fake.set(.hostOffline(lastSeen: nil))
    #expect(await until { store.hostRoute == nil })
    #expect(store.connection == .online)
    #expect(store.canQueue)
    await fake.setReadFailure(false)
    await fake.set(.ready(.relay))
    #expect(await until { await fake.eventSubscriptionCount == 2 })
    await fake.emit(botEvent("b2", name: "Other bot", rev: 2))
    #expect(await until { await fake.readReceipts.count == 1 })
    #expect(store.connection == .online)
    #expect(store.lastError == nil)
}

@MainActor @Test func actionDuringDropFailsAtOnceAndIsNotReplayedOnTheNewLink() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.connection == .online })
    // A drop still inside its grace: the header says connected, but there is no authority.
    await fake.set(.connecting)
    #expect(await until { store.hostRoute == nil })
    #expect(store.estadoConexao.estaReconectando)
    #expect(!store.temAutoridade)
    store.stop("b1")
    #expect(await until { store.lastError != nil })
    await fake.set(.ready(.relay))
    #expect(await until { await fake.eventSubscriptionCount == 2 })
    await fake.emit(botEvent("b1", name: "Bot", rev: 2))
    #expect(await until { store.temAutoridade })
    try await Task.sleep(for: .milliseconds(100))
    #expect(await !fake.calls.contains("stop"))
}

@MainActor @Test func droppedCallIsRetriedOnlyWhenSafeToRepeat() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.connection == .online })
    await fake.fail("stop", times: 1)
    store.stop("b1")
    #expect(await until { await fake.calls.filter { $0 == "stop" }.count == 2 })
    #expect(store.lastError == nil)
    // A new session can't be told apart from a second one, so it isn't sent twice.
    await fake.fail("newSession", times: 1)
    store.newSession("b1")
    #expect(await until { store.lastError != nil })
    #expect(await fake.calls.filter { $0 == "newSession" }.count == 1)
}

@MainActor @Test func offlineTapFailsAtOnceWithoutOpeningALink() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let down = FakeRemote(.failed("Can't reach"))
    var made = 0
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in
        made += 1
        return down
    }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { store.connection == .offline("Can't reach") })
    store.stop("b1")
    #expect(await until { store.lastError != nil })
    #expect(made == 1)
    #expect(await !down.calls.contains("stop"))
}

@MainActor @Test func permissionAnswerShowsOnTheCardAndIsSentOnce() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.connection == .online })
    let card = Entry(id: "p1", seq: 1, botId: "b1", rev: 1, kind: "permission", turn: 1,
                     data: EntryData(status: "pending"), createdAt: 1, updatedAt: 1)
    store.respond(card, option: "allow")
    store.respond(card, option: "deny")
    #expect(store.answering["p1"] == "allow")
    #expect(await until { await fake.calls.contains("respondPermission") })
    try await Task.sleep(for: .milliseconds(100))
    #expect(await fake.calls.filter { $0 == "respondPermission" }.count == 1)
}

// MARK: - Voice call operator

/// The realtime voice operator's tools run against the store, the same paths the chat uses.
@MainActor @Test func callOperatorToolsDriveTheBot() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(#"{"type":"bot","bot":{"id":"b1","name":"Ada","rev":1,"lastAt":1}}"#)
    #expect(await until { store.connection == .online })
    let op = CallOperator(model: store, botId: "b1")
    // The call's microphone started on this link and the user spoke.
    op.audioComecou()
    op.falaDetectada()

    func json(_ s: String) -> [String: Any] {
        (try? JSONSerialization.jsonObject(with: Data(s.utf8))) as? [String: Any] ?? [:]
    }

    #expect(json(op.call("send_to_bot", arguments: ["text": "  run the tests "]))["sent"] as? Bool == true)
    #expect(await until { await fake.sent == ["run the tests"] })
    #expect(json(op.call("send_to_bot", arguments: [:]))["error"] != nil)
    #expect(json(op.call("answer_approval", arguments: ["option": "yes"]))["error"] as? String == "Nothing is waiting for approval.")

    await fake.emit(#"{"type":"entry","entry":{"id":"p1","seq":5,"botId":"b1","rev":5,"kind":"permission","turn":1,"data":{"title":"Update TODOs","status":"pending","voiceApproval":"allow_once","options":[{"optionId":"a","name":"Allow once","kind":"allow_once"},{"optionId":"r","name":"Reject","kind":"reject_once"}]},"createdAt":1,"updatedAt":1}}"#)
    #expect(await until { store.chat("b1").contains { $0.id == "p1" } })
    let status = json(op.call("bot_status", arguments: [:]))
    #expect((status["approval"] as? [String: Any])?["options"] as? [String] == ["Allow once"])
    let unknown = json(op.call("answer_approval", arguments: ["option": "maybe"]))
    #expect(unknown["error"] as? String == "Opção não reconhecida")
    #expect((unknown["options"] as? [[String: String]])?.map { $0["optionId"] } == ["a"])
    #expect(json(op.call("answer_approval", arguments: ["option": "allow"]))["error"] as? String == "Opção não reconhecida")
    #expect(json(op.call("answer_approval", arguments: ["option": "r"]))["error"] as? String == "Aprove no app")
    #expect(store.answering["p1"] == nil)
    // The status and options above came from the computer: the user says it again.
    op.falaDetectada()
    #expect(json(op.call("answer_approval", arguments: ["option": "a"]))["answered"] as? String == "Allow once")
    #expect(store.answering["p1"] == "a")

    let recent = json(op.call("recent_messages", arguments: ["count": 1]))["messages"] as? [[String: String]]
    #expect(recent?.count == 1)
    #expect(recent?.first?["from"] == "approval request")
    #expect(op.reply("**Done**, see `x`").hasPrefix("[Ada replied] "))
}

/// Approvals reach a call as notices, replies as replies (a realtime operator words them differently).
@MainActor @Test func voiceCallGetsApprovalsAsNotices() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(#"{"type":"bot","bot":{"id":"b1","name":"Ada","rev":1,"lastAt":1}}"#)
    #expect(await until { store.connection == .online })
    var replies: [String] = []
    var notices: [String] = []
    store.beginVoiceCall(UUID(), botId: "b1", speak: { replies.append($0) }, announce: { notices.append($0) }, end: {})
    await fake.emit(#"{"type":"bot","bot":{"id":"b1","name":"Ada","rev":2,"lastAt":2,"status":"needsInput"}}"#)
    #expect(await until { notices == [String(localized: "\("Ada") precisa da sua aprovação na conversa.", bundle: ConnectionSummary.resourceBundle)] })
    #expect(replies.isEmpty)
}

// MARK: - Version compatibility (docs/reference/compatibility.md)

private func helloJSON(version: String, minApp: String? = nil, backends: String = "[]") -> String {
    let min = minApp.map { #","minApp":"\#($0)""# } ?? ""
    return #"{"hostId":"h1","name":"Mac","version":"\#(version)"\#(min),"os":"macos","backends":\#(backends),"rev":0}"#
}

@MainActor @Test func hostNeedingANewerAppStopsSyncAndAsksForTheUpdate() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.relay))
    await fake.setHello(helloJSON(version: "2.6.0", minApp: "2.5.0"))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage, appVersion: "2.4.0") { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { store.mismatch == .updateApp(minimum: "2.5.0") })
    #expect(await until { store.connection == .online })
    try await Task.sleep(for: .milliseconds(200))
    // Nothing this app can't read is synced.
    #expect(await !fake.subscribed)
}

@MainActor @Test func updatedHostIsNoticedOnReconnect() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.relay))
    await fake.setHello(helloJSON(version: "2.2.0"))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage, appVersion: VersionMismatch.minHost) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { store.mismatch == .updateHost(version: "2.2.0", minimum: VersionMismatch.minHost) })
    // The host updates and restarts: the link drops and comes back.
    await fake.setHello(helloJSON(version: VersionMismatch.minHost, minApp: "2.3.0"))
    await fake.set(.connecting)
    await fake.set(.ready(.relay))
    #expect(await until { store.mismatch == nil })
    #expect(await until { await fake.subscribed })
    #expect(store.hostVersion == HostVersion(version: VersionMismatch.minHost, minApp: "2.3.0"))
}

@MainActor @Test func unreadableHelloStillSaysWhichSideToUpdate() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    // A newer host whose hello this app can't decode, with or without a minApp that covers it.
    for minApp in ["9.1.0", "2.0.0"] {
        let fake = FakeRemote(.ready(.relay))
        await fake.setHello(helloJSON(version: "9.1.0", minApp: minApp, backends: #""changed shape""#))
        let store = BotStore(computer: randomComputer("Mac"), storage: storage, appVersion: "9.0.0") { _ in fake }
        store.setActive(true)
        #expect(await until { store.mismatch == .updateApp(minimum: "9.1.0") }, "minApp \(minApp)")
        #expect(store.hello == nil)
        store.retire()
    }
}

@MainActor @Test func unreadableEventsAskForAnAppUpdateOnlyFromANewerHost() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    for (hostVersion, appIsBehind) in [("9.1.0", true), ("9.0.0", false)] {
        let fake = FakeRemote(.ready(.relay))
        await fake.setHello(helloJSON(version: hostVersion, minApp: "2.3.0"))
        let store = BotStore(computer: randomComputer("Mac"), storage: storage, appVersion: "9.0.0") { _ in fake }
        store.setActive(true)
        let broken = #"{"type":"bot","bot":{"id":"b1","name":7}}"#
        #expect(await until { await fake.eventSubscriptionCount == 1 })
        await fake.emit(broken)
        // The first one rewinds and subscribes again; the second one is final.
        #expect(await until { await fake.eventSubscriptionCount == 2 })
        await fake.emit(broken)
        if appIsBehind {
            #expect(await until { store.mismatch == .updateApp(minimum: hostVersion) })
        } else {
            try await Task.sleep(for: .milliseconds(200))
            #expect(store.mismatch == nil)
        }
        store.retire()
    }
}

@MainActor @Test func draftsFromBeforeQuoteRepliesStayWithTheirChat() {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let computer = randomComputer("Mac")
    storage.composerDrafts[computer.id] = [#"["first",null]"#: "Unsent", #"["first","root"]"#: "Aside", "second": "New"]
    let store = BotStore(computer: computer, storage: storage) { _ in FakeRemote(.connecting) }
    defer { store.retire() }
    #expect(store.composerDraft(for: "first") == "Unsent")
    #expect(store.composerDraft(for: "second") == "New")
    store.setComposerDraft("Unsent!", for: "first")
    #expect(storage.composerDrafts[computer.id] == ["first": "Unsent!", "second": "New"])
}

@MainActor @Test func composerDraftsPersistAcrossNavigationAndReopening() {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let computer = randomComputer("Mac")
    let store = BotStore(computer: computer, storage: storage) { _ in FakeRemote(.connecting) }
    let text = "  中文\nunfinished 🐱\n"
    store.setComposerDraft(text, for: "first")
    store.setComposerDraft("Other draft", for: "second")
    store.selection = "second"
    #expect(store.composerDraft(for: "first") == text)
    store.retire()
    store.setComposerDraft("late edit", for: "first")
    let reopened = BotStore(computer: computer, storage: storage) { _ in FakeRemote(.connecting) }
    defer { reopened.retire() }
    #expect(reopened.composerDraft(for: "first") == text)
    reopened.setComposerDraft("", for: "first")
    #expect(reopened.composerDraft(for: "first").isEmpty)
    #expect(reopened.composerDraft(for: "second") == "Other draft")
    let other = BotStore(computer: randomComputer("Other"), storage: storage) { _ in FakeRemote(.connecting) }
    defer { other.retire() }
    #expect(other.composerDraft(for: "first").isEmpty)
    storage.erase()
    #expect(storage.composerDrafts.isEmpty)
}

@MainActor @Test func sendingDraftNeverClearsAnotherConversationOrLaterText() async {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("first", name: "First", rev: 1))
    #expect(await until { store.connection == .online })
    store.setComposerDraft("First message", for: "first")
    store.setComposerDraft("Other draft", for: "second")
    #expect(store.sendComposerDraft(to: "first"))
    #expect(store.composerDraft(for: "first").isEmpty)
    store.setComposerDraft("Next unfinished message", for: "first")
    #expect(await until { store.chat("first").last?.id == "e1" })
    #expect(store.composerDraft(for: "first") == "Next unfinished message")
    #expect(store.composerDraft(for: "second") == "Other draft")
    let anotherAccount = SharedStore.Context(accountID: "another-user", suite: suite)
    #expect(anotherAccount.composerDrafts.isEmpty)
}

@MainActor @Test func existingCachesKeepTheirMessagesAndLocalSends() throws {
    let (storage, suite) = context()
    defer { storage.erase(); UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let computer = randomComputer("Fixture")
    let build = Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "0"
    let url = URL.cachesDirectory.appending(path: "codync-mirror-\(Bundle.main.bundleIdentifier ?? "app")-\(storage.id)-\(computer.id).json")
    let cached = #"""
    {"stamp":"\#(build)/3.0.0","hostId":"h1","rev":77,"bots":[],"entries":[
      {"id":"notice","seq":1,"botId":"dex","rev":77,"kind":"notice","turn":1,"createdAt":1,"updatedAt":1,
       "data":{"text":"Message from Miles","status":"completed"}},
      {"id":"local-pending","seq":2,"botId":"dex","rev":0,"kind":"user","turn":1,"createdAt":2,"updatedAt":2,
       "data":{"text":"My unsent message","status":"failed"}}
    ]}
    """#
    try Data(cached.utf8).write(to: url)
    let store = BotStore(computer: computer, storage: storage) { _ in FakeRemote(.ready(.direct)) }
    defer { store.retire() }
    // Adding conversation metadata does not invalidate the existing base cache.
    #expect(store.chat("dex").map(\.id) == ["notice", "local-pending"])
    #expect(store.chat("dex").last?.data.text == "My unsent message")
    store.saveCache()
    let bytes = try Data(contentsOf: url)
    let cache = try #require(JSONSerialization.jsonObject(with: bytes) as? [String: Any])
    #expect(cache["rev"] as? Int == 77)
}

private func entryEvent(_ id: String, seq: Int, rev: Int, turn: Int = 5, text: String = "hi") -> String {
    #"{"type":"entry","entry":{"id":"\#(id)","seq":\#(seq),"botId":"b1","rev":\#(rev),"kind":"agent","turn":\#(turn),"data":{"text":"\#(text)","final":true},"createdAt":1,"updatedAt":1}}"#
}

private func windowEntry(_ id: String, seq: Int64, rev: Int64 = 1) -> Entry {
    var data = EntryData(text: id)
    data.final = true
    return Entry(id: id, seq: seq, botId: "b1", rev: rev, kind: "agent", turn: 0, data: data, createdAt: 1, updatedAt: 1)
}

@MainActor @Test func windowRuleDropsOnlyUnknownEntriesBelowTheFloor() {
    let below = windowEntry("a", seq: 5)
    #expect(BotStore.isOutsideLoadedWindow(below, floor: 100, held: false))
    #expect(!BotStore.isOutsideLoadedWindow(below, floor: 100, held: true))
    #expect(!BotStore.isOutsideLoadedWindow(windowEntry("a", seq: 100), floor: 100, held: false))
    #expect(!BotStore.isOutsideLoadedWindow(windowEntry("a", seq: 150), floor: 100, held: false))
    #expect(!BotStore.isOutsideLoadedWindow(below, floor: nil, held: false))
}

/// A connection that resumes from a rev > 0 leaves a rewritten old entry to history paging.
@MainActor @Test func resumedConnectionDropsRewrittenEntriesBelowTheMirrorsFloor() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    defer { store.retire() }
    store.upsert(windowEntry("e100", seq: 100))
    store.upsert(windowEntry("local-1", seq: 0)) // optimistic: not a floor
    store.rev = 10
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(#"{"type":"hello","hostId":"h1","rev":20}"#)
    await fake.emit(botEvent("b1", name: "Bot", rev: 11))
    // Rewritten old entry: dropped, though its rev is consumed.
    await fake.emit(entryEvent("old", seq: 5, rev: 12, turn: 0))
    // New entry above the floor and an update to a held one apply.
    await fake.emit(entryEvent("e101", seq: 101, rev: 13))
    await fake.emit(entryEvent("e100", seq: 100, rev: 14, text: "edited"))
    #expect(await until { store.chat("b1").first { $0.id == "e100" }?.data.text == "edited" && store.chat("b1").contains { $0.id == "e101" } })
    #expect(!store.chat("b1").contains { $0.id == "old" })
    #expect(store.rev == 14)
    // History results still insert older entries.
    store.upsert(windowEntry("old", seq: 5))
    #expect(store.chat("b1").contains { $0.id == "old" })
}

/// From rev 0 the catch-up is rev-ordered, not seq-ordered, and a sent message's RPC response may
/// land first: nothing is below a floor.
@MainActor @Test func freshCatchUpAcceptsEntriesInRevOrder() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    defer { store.retire() }
    store.upsert(windowEntry("sent", seq: 100)) // RPC response before the catch-up
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(#"{"type":"hello","hostId":"h1","rev":20}"#)
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    await fake.emit(entryEvent("e50", seq: 50, rev: 2, turn: 0))
    await fake.emit(entryEvent("e5", seq: 5, rev: 3, turn: 0))
    #expect(await until { store.chat("b1").map(\.id) == ["e5", "e50", "sent"] })
}

// MARK: - Short drop

@MainActor @Test func shortDropKeepsTheSameConversationBotsAndDraft() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.estadoConexao == .conectado })
    store.selection = "b1"
    store.setComposerDraft("rascunho", for: "b1")

    await fake.set(.connecting)
    #expect(await until { store.estadoConexao.estaReconectando })
    #expect(store.bots["b1"] != nil)
    #expect(store.selection == "b1")
    #expect(store.composerDraft(for: "b1") == "rascunho")
    #expect(store.mantemNaTela)

    await fake.set(.ready(.relay))
    #expect(await until { await fake.eventSubscriptionCount == 2 })
    await fake.emit(botEvent("b1", name: "Bot", rev: 2))
    #expect(await until { store.estadoConexao == .conectado })
    #expect(store.bots["b1"]?.rev == 2)
    #expect(store.selection == "b1")
    #expect(store.composerDraft(for: "b1") == "rascunho")
    #expect(!store.faixaDeQueda)
}

@MainActor @Test func shortDropBecomesDisconnectedOnlyAtFiveMinutes() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let start = Date(timeIntervalSince1970: 1_000_000)
    let base = ContinuousClock.now
    var passou: Duration = .zero
    // The deadline timer never wakes by itself here: the test moves the monotonic clock and checks.
    let store = BotStore(computer: randomComputer("Mac"), storage: storage, agora: { start },
                         monotonico: { base.advanced(by: passou) },
                         esperar: { _ in try await Task.sleep(for: .seconds(86_400)) }) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.estadoConexao == .conectado })

    await fake.set(.connecting)
    #expect(await until { store.estadoConexao == .reconectando(desde: start) })
    passou = .seconds(4)
    store.verificarPrazo()
    #expect(!store.faixaDeQueda)
    passou = .seconds(5)
    store.verificarPrazo()
    #expect(store.faixaDeQueda)
    passou = .seconds(4 * 60 + 59)
    store.verificarPrazo()
    #expect(store.estadoConexao == .reconectando(desde: start))
    passou = .seconds(5 * 60)
    store.verificarPrazo()
    #expect(store.estadoConexao == .desconectado(.prazoLongo))
    #expect(!store.faixaDeQueda)
    // Disconnected shows today's offline screen; what was loaded is still there for when it's back.
    #expect(store.bots["b1"] != nil)
}

@MainActor @Test(arguments: [
    "Este iPhone não tem mais acesso a este computador.",
    "This device isn't allowed on Mac anymore.",
    "This computer's identity changed. Pair it again to keep using it.",
])
func definitiveFailureLetsGoAtOnce(_ message: String) async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage,
                         esperar: { _ in try await Task.sleep(for: .seconds(86_400)) }) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.estadoConexao == .conectado })
    // No clock moves and no grace runs: revoked, 401/403 and a changed server key let go at once.
    await fake.set(.unauthorized(message))
    #expect(await until { store.estadoConexao == .desconectado(.acessoEncerrado(message)) })
    #expect(store.connection == .unauthorized(message))
    #expect(!store.temAutoridade)
}

@MainActor @Test func refusedLinkAndSignOutLetGoAtOnce() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let refused = BotStore(computer: randomComputer("Mac"), storage: storage,
                           esperar: { _ in try await Task.sleep(for: .seconds(86_400)) }) { _ -> any HostTransport in
        throw HostError.unauthorized("revoked")
    }
    refused.setActive(true)
    #expect(await until { refused.estadoConexao == .desconectado(.acessoEncerrado("revoked")) })
    refused.retire()

    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.estadoConexao == .conectado })
    // Signing out, switching accounts or removing the computer: no grace.
    store.retire()
    #expect(store.estadoConexao == .desconectado(.encerradoNoApp))
}

@MainActor @Test func anotherAccountOrComputerAfterADropStartsOver() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.estadoConexao == .conectado })
    store.selection = "b1"
    store.setComposerDraft("rascunho", for: "b1")
    // The mirror was loaded for someone else (another account here).
    store.donoDoEspelho = DonoDoEspelho(conta: "outra-conta", computador: store.computer.id, chave: store.computer.signKey)

    await fake.set(.connecting)
    #expect(await until { store.estadoConexao.estaReconectando })
    await fake.set(.ready(.relay))
    #expect(await until { await fake.eventSubscriptionCount == 2 })
    #expect(store.bots["b1"] == nil)
    #expect(store.selection == nil)
    #expect(store.composerDraft(for: "b1") == "")
    #expect(store.donoDoEspelho == DonoDoEspelho(conta: storage.accountID, computador: store.computer.id, chave: store.computer.signKey))
}

@MainActor @Test func messageSentDuringDropStaysNotSentAndIsNotResentByItself() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.estadoConexao == .conectado })

    await fake.set(.connecting)
    #expect(await until { store.estadoConexao.estaReconectando })
    store.send("oi", to: "b1")
    #expect(await until { store.chat("b1").last?.data.status == "failed" })

    await fake.set(.ready(.relay))
    #expect(await until { await fake.eventSubscriptionCount == 2 })
    await fake.emit(botEvent("b1", name: "Bot", rev: 2))
    #expect(await until { store.temAutoridade })
    try await Task.sleep(for: .milliseconds(100))
    #expect(await fake.sent.isEmpty)
    let failed = try #require(store.chat("b1").last)
    #expect(failed.data.status == "failed")
    // Resend is the person's choice, on the new link.
    store.retry(failed)
    #expect(await until { await fake.sent == ["oi"] })
}

// MARK: - Short drop: review fixes

/// The drop timer's sleep, woken by the test instead of by real time.
private actor Despertador {
    private var dormindo: [UUID: CheckedContinuation<Void, Error>] = [:]
    var quantos: Int { dormindo.count }
    /// How long each sleep asked for, in order.
    private(set) var pedidos: [Duration] = []

    func dormir(_ duracao: Duration = .zero) async throws {
        let id = UUID()
        pedidos.append(duracao)
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (c: CheckedContinuation<Void, Error>) in
                dormindo[id] = c
            }
        } onCancel: {
            Task { await self.cancelar(id) }
        }
    }

    private func cancelar(_ id: UUID) {
        dormindo.removeValue(forKey: id)?.resume(throwing: CancellationError())
    }

    func acordar() {
        let todos = Array(dormindo.values)
        dormindo = [:]
        for c in todos { c.resume() }
    }
}

/// A connected store and its transport, for the tests below.
@MainActor
private func lojaConectada(_ storage: SharedStore.Context, _ fake: FakeRemote,
                       monotonico: @escaping @MainActor () -> ContinuousClock.Instant = { ContinuousClock.now },
                       esperar: @escaping @Sendable (Duration) async throws -> Void = { _ in try await Task.sleep(for: .seconds(86_400)) })
    async -> BotStore {
    let store = BotStore(computer: randomComputer("Mac"), storage: storage, monotonico: monotonico, esperar: esperar) { _ in fake }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.temAutoridade })
    return store
}

// Finding 1: an action bound to a channel never goes out on a newer one, not even between upload chunks.
@MainActor @Test func uploadNeverContinuesOnANewerChannel() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaConectada(storage, fake)
    defer { store.retire() }
    // A reconnect lands right after the first chunk.
    await fake.replaceLink(after: "upload")
    store.send("com arquivo", to: "b1", files: [OutgoingFile(name: "a.bin", data: Data(count: 400 * 1024))])
    #expect(await until { store.chat("b1").last?.data.status == "failed" })
    #expect(await fake.calls.filter { $0 == "upload" }.count == 1)
    #expect(await !fake.calls.contains("send"))
}

// Finding 1: the channel is replaced between the store's check and the call.
@MainActor @Test func actionAfterTheChannelWasReplacedFailsAtOnce() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaConectada(storage, fake)
    defer { store.retire() }
    // The next call is the last one on this channel; the store hasn't heard of the new one yet.
    await fake.replaceLink(after: "markRead")
    store.markRead("b1")
    #expect(await until { await fake.calls.contains("markRead") })
    store.stop("b1")
    #expect(await until { store.lastError != nil })
    #expect(await !fake.calls.contains("stop"))
}

// Finding 2: back from the background, nothing has authority until the new Hub identifies itself.
@MainActor @Test func reopeningWhileOnlineWaitsForTheNewHelloBeforeActing() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let first = FakeRemote(.ready(.direct))
    let second = FakeRemote(.ready(.relay))
    var made = 0
    let store = BotStore(computer: randomComputer("Mac"), storage: storage,
                         esperar: { _ in try await Task.sleep(for: .seconds(86_400)) }) { _ in
        made += 1
        return made == 1 ? first : second
    }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { await first.subscribed })
    await first.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.temAutoridade })

    await second.holdHello()
    store.setActive(false)
    store.setActive(true)
    #expect(await until { await second.helloWaiting == 1 })
    #expect(!store.temAutoridade)
    #expect(store.estadoConexao.estaReconectando)
    store.stop("b1")
    #expect(await until { store.lastError != nil })
    #expect(await !second.calls.contains("stop"))
    #expect(await !first.calls.contains("stop"))

    await second.releaseHello()
    #expect(await until { await second.subscribed })
    await second.emit(botEvent("b1", name: "Bot", rev: 2))
    #expect(await until { store.temAutoridade })
}

// Finding 3: a Hub naming another computer is refused, not reused.
@MainActor @Test func helloFromAnotherComputerIsRefused() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaConectada(storage, fake)
    defer { store.retire() }
    await fake.setHello(#"{"hostId":"h1","name":"Mac","version":"3.0.0","os":"macos","backends":[],"rev":0,"computerId":"outro-computador"}"#)
    await fake.set(.connecting)
    #expect(await until { store.estadoConexao.estaReconectando })
    await fake.set(.ready(.relay))
    #expect(await until { store.estadoConexao == .desconectado(.acessoEncerrado(HostError.identidadeMudou)) })
    #expect(!store.temAutoridade)
    // Never subscribed again on the refused link.
    #expect(await fake.eventSubscriptionCount <= 1)
}

// Finding 3: another owner wipes everything, messages waiting to be sent and their files included.
@MainActor @Test func ownerChangeWipesPendingMessagesAndFiles() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaConectada(storage, fake)
    defer { store.retire() }
    await fake.set(.connecting)
    #expect(await until { store.estadoConexao.estaReconectando })
    store.send("pendente", to: "b1", files: [OutgoingFile(name: "a.txt", data: Data("x".utf8))])
    #expect(await until { store.chat("b1").last?.data.status == "failed" })
    #expect(!store.outgoingFiles.isEmpty)
    store.donoDoEspelho = DonoDoEspelho(conta: "outra-conta", computador: store.computer.id, chave: store.computer.signKey)

    await fake.set(.ready(.relay))
    #expect(await until { await fake.eventSubscriptionCount == 2 })
    #expect(store.chat("b1").isEmpty)
    #expect(store.outgoingFiles.isEmpty)
    await fake.emit(botEvent("b1", name: "Bot", rev: 2))
    #expect(await until { store.temAutoridade })
    try await Task.sleep(for: .milliseconds(100))
    #expect(await fake.sent.isEmpty)
    #expect(await !fake.calls.contains("upload"))
}

// Finding 4: a real 401/403 from the Hub while it identifies itself ends access at once.
@MainActor @Test(arguments: [401, 403])
func refusalFromHelloLetsGoAtOnce(_ status: Int) async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaConectada(storage, fake)
    defer { store.retire() }
    await fake.fail("hello", with: .http(status, "Revoked."))
    await fake.set(.connecting)
    #expect(await until { store.estadoConexao.estaReconectando })
    await fake.set(.ready(.relay))
    #expect(await until { store.estadoConexao.estaDesconectado })
    guard case .desconectado(.acessoEncerrado) = store.estadoConexao else {
        Issue.record("expected access ended, got \(store.estadoConexao)")
        return
    }
    #expect(await fake.eventSubscriptionCount <= 1)
}

// Finding 4: a 401 answering any action ends access too; a 403 answering one action refuses only it.
@MainActor @Test func unauthorizedActionLetsGoButARefusedActionDoesNot() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaConectada(storage, fake)
    defer { store.retire() }
    await fake.fail("newSession", with: .http(403, "Only on the computer."))
    store.newSession("b1")
    #expect(await until { store.lastError == "Only on the computer." })
    // The Hub identified itself again: authority is back.
    #expect(await until { store.temAutoridade })
    #expect(store.estadoConexao == .conectado)

    await fake.fail("stop", with: .http(401, ""))
    store.stop("b1")
    #expect(await until { store.estadoConexao.estaDesconectado })
    guard case .desconectado(.acessoEncerrado) = store.estadoConexao else {
        Issue.record("expected access ended, got \(store.estadoConexao)")
        return
    }
}

// Finding 5: the background doesn't restart a drop's clock; the deadline is checked on return.
@MainActor @Test func backgroundKeepsTheDropStartAndChecksTheDeadlineOnReturn() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let start = Date(timeIntervalSince1970: 1_000_000)
    let base = ContinuousClock.now
    var passou: Duration = .zero
    let store = BotStore(computer: randomComputer("Mac"), storage: storage,
                         agora: { start.addingTimeInterval(passou / .seconds(1)) },
                         monotonico: { base.advanced(by: passou) },
                         esperar: { _ in try await Task.sleep(for: .seconds(86_400)) }) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.estadoConexao == .conectado })
    await fake.set(.connecting)
    #expect(await until { store.estadoConexao == .reconectando(desde: start) })

    passou = .seconds(60)
    store.setActive(false)
    store.setActive(true)
    #expect(store.estadoConexao == .reconectando(desde: start))

    passou = .seconds(4 * 60)
    store.setActive(false)
    passou = .seconds(5 * 60 + 1)
    store.setActive(true)
    #expect(store.estadoConexao == .desconectado(.prazoLongo))
}

// The five-minute timer itself (not a manual check): it shows the strip, then lets go.
@MainActor @Test func dropTimerShowsTheStripAndLetsGoAtFiveMinutes() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let base = ContinuousClock.now
    var passou: Duration = .zero
    let despertador = Despertador()
    let store = await lojaConectada(storage, fake, monotonico: { base.advanced(by: passou) },
                                esperar: { _ in try await despertador.dormir() })
    defer { store.retire() }
    // The timer armed before the first connection was cancelled.
    #expect(await until { await despertador.quantos == 0 })

    await fake.set(.connecting)
    #expect(await until { store.estadoConexao.estaReconectando })
    #expect(await until { await despertador.quantos == 1 })
    passou = .seconds(5)
    await despertador.acordar()
    #expect(await until { store.faixaDeQueda })
    #expect(await until { await despertador.quantos == 1 })
    passou = .seconds(4 * 60 + 59)
    await despertador.acordar()
    #expect(await until { await despertador.quantos == 1 })
    #expect(store.estadoConexao.estaReconectando)
    passou = .seconds(5 * 60)
    await despertador.acordar()
    #expect(await until { store.estadoConexao == .desconectado(.prazoLongo) })
    #expect(store.bots["b1"] != nil)
}

// "Try now" is limited in the store, not only on screen.
@MainActor @Test func tryNowIsLimitedInTheStore() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let base = ContinuousClock.now
    var passou: Duration = .zero
    var made = 0
    let store = BotStore(computer: randomComputer("Mac"), storage: storage, monotonico: { base.advanced(by: passou) },
                         esperar: { _ in try await Task.sleep(for: .seconds(86_400)) }) { _ in
        made += 1
        return FakeRemote(.failed("Can't reach"))
    }
    defer { store.retire() }
    store.setActive(true)
    #expect(await until { made == 1 })
    store.tentarAgora()
    #expect(await until { made == 2 })
    store.tentarAgora()
    passou = .seconds(2)
    store.tentarAgora()
    try await Task.sleep(for: .milliseconds(100))
    #expect(made == 2)
    passou = .seconds(3)
    store.tentarAgora()
    #expect(await until { made == 3 })
}

// Review C: a 403 answering one action re-identifies the Hub on the same channel before deciding.
@MainActor @Test func actionRefusedOnlyOnTheComputerKeepsTheConnection() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaConectada(storage, fake)
    defer { store.retire() }
    let hellos = await fake.calls.filter { $0 == "hello" }.count
    await fake.fail("newSession", with: .http(403, "Only on the computer."))
    store.newSession("b1")
    #expect(await until { store.lastError == "Only on the computer." })
    #expect(await until { await fake.calls.filter { $0 == "hello" }.count == hellos + 1 })
    #expect(await until { store.temAutoridade })
    #expect(store.estadoConexao == .conectado)
}

@MainActor @Test func actionUnauthorizedAfterGoingOnlineEndsAccess() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaConectada(storage, fake)
    defer { store.retire() }
    let denied = HostError.http(401, "Refused")
    let message = try #require(denied.errorDescription)
    await fake.fail("newSession", with: denied)
    store.newSession("b1")
    #expect(await until { store.connection == .unauthorized(message) })
    #expect(!store.temAutoridade)
}

@MainActor @Test func eventsRetryWithoutALinkWithdrawsAuthority() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaConectada(storage, fake)
    defer { store.retire() }
    try #require(store.temAutoridade)
    await fake.loseLinkWithoutStateUpdate()
    #expect(await until { store.client == nil && store.refusalListener == nil })
    #expect(!store.temAutoridade)
    #expect(store.connection == .online)
}

@MainActor @Test(arguments: [401, 403]) func lateActionRefusalCannotAffectReopenedLink(_ status: Int) async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaConectada(storage, fake)
    defer { store.retire() }
    let old = try #require(store.client)
    await fake.holdRefusal("newSession", .http(status, "Old refusal"))
    let action = Task { try await old.newSession("b1") }
    let waiting = await until { await fake.refusalWaiting }
    #expect(waiting)
    guard waiting else { return }
    await fake.set(.connecting)
    let disconnected = await until { !store.temAutoridade }
    #expect(disconnected)
    await fake.replaceLink()
    await fake.set(.ready(.direct))
    let subscribed = await until { await fake.eventSubscriptionCount == 2 }
    #expect(subscribed)
    await fake.emit(botEvent("b1", name: "Bot", rev: 2))
    let reopened = await until { store.temAutoridade }
    #expect(reopened)
    let helloCount = await fake.calls.filter { $0 == "hello" }.count
    await fake.releaseRefusal()
    await #expect(throws: HostError.unreachable) { try await action.value }
    #expect(store.temAutoridade)
    #expect(await fake.calls.filter { $0 == "hello" }.count == helloCount)
}

@MainActor @Test func actionRefusedBecauseTheDeviceWasRevokedLetsGoAtOnce() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaConectada(storage, fake)
    defer { store.retire() }
    await fake.fail("stop", with: .http(403, "Revoked."))
    await fake.fail("hello", with: .http(403, "Revoked."))
    store.stop("b1")
    #expect(await until { store.estadoConexao == .desconectado(.acessoEncerrado("Revoked.")) })
    #expect(!store.temAutoridade)
}

@MainActor @Test func refusalWithinTheWindowSuspendsAndChecksOnceAtItsEnd() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let base = ContinuousClock.now
    var passou: Duration = .zero
    let despertador = Despertador()
    let store = await lojaConectada(storage, fake, monotonico: { base.advanced(by: passou) },
                                    esperar: { try await despertador.dormir($0) })
    defer { store.retire() }
    #expect(await until { await despertador.quantos == 0 })
    let hellos = await fake.calls.filter { $0 == "hello" }.count
    await fake.fail("newSession", with: .http(403, "Only on the computer."))
    store.newSession("b1")
    #expect(await until { await fake.calls.filter { $0 == "hello" }.count == hellos + 1 })
    #expect(await until { store.temAutoridade })

    // Refused again 3 s later: authority is suspended at once; one check waits for the window's end.
    passou = .seconds(3)
    store.newSession("b1")
    #expect(await until { !store.temAutoridade })
    #expect(await until { await despertador.quantos == 1 })
    #expect(await despertador.pedidos.last == .seconds(7))
    // A burst of refusals (calls already in flight) schedules nothing more.
    store.reverificarAcesso()
    store.reverificarAcesso()
    #expect(await despertador.quantos == 1)
    #expect(await fake.calls.filter { $0 == "hello" }.count == hellos + 1)
    #expect(!store.temAutoridade)

    passou = .seconds(10)
    await despertador.acordar()
    #expect(await until { await fake.calls.filter { $0 == "hello" }.count == hellos + 2 })
    #expect(await until { store.temAutoridade })
    #expect(store.estadoConexao == .conectado)
    try await Task.sleep(for: .milliseconds(100))
    #expect(await fake.calls.filter { $0 == "hello" }.count == hellos + 2)
}

@MainActor @Test func revokedWithinTheWindowLetsGoWhenTheDelayedCheckAnswers() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let base = ContinuousClock.now
    var passou: Duration = .zero
    let despertador = Despertador()
    let store = await lojaConectada(storage, fake, monotonico: { base.advanced(by: passou) },
                                    esperar: { try await despertador.dormir($0) })
    defer { store.retire() }
    #expect(await until { await despertador.quantos == 0 })
    await fake.fail("newSession", with: .http(403, "Only on the computer."))
    store.newSession("b1")
    #expect(await until { store.lastError == "Only on the computer." })
    #expect(await until { store.temAutoridade })

    // The device is revoked right after: the next 403 suspends at once, the delayed hello ends access.
    await fake.fail("stop", with: .http(403, "Revoked."))
    await fake.fail("hello", with: .http(403, "Revoked."))
    passou = .seconds(2)
    store.stop("b1")
    #expect(await until { !store.temAutoridade })
    #expect(await until { await despertador.quantos == 1 })
    #expect(store.estadoConexao == .conectado)
    passou = .seconds(10)
    await despertador.acordar()
    #expect(await until { store.estadoConexao == .desconectado(.acessoEncerrado("Revoked.")) })
    #expect(!store.temAutoridade)
}

// Review B: the early-403 tolerance of a fresh link is single use and short-lived.

/// A relay that answers every upgrade with 403 (this device was revoked), counting the dials.
private actor Recusas {
    private(set) var dials = 0

    func discar() -> any ChannelSocket {
        dials += 1
        return Recusado()
    }
}

private final class Recusado: ChannelSocket {
    func send(_ text: String) async throws { throw SocketRefused(status: 403) }
    func receive() async throws -> String { throw SocketRefused(status: 403) }
    func ping() async throws {}
    func close(code: Int) {}
}

/// The transports a store opened, tolerant (just linked) or ordinary.
@MainActor
private final class Criados {
    var tolerantes: [ChannelTransport] = []
    var comuns: [ChannelTransport] = []
}

/// A store whose transports are real channels to that relay; tolerant and ordinary ones dial through
/// separate counters.
@MainActor
private func lojaRecusada(_ storage: SharedStore.Context, monotonico: @escaping @MainActor () -> ContinuousClock.Instant,
                          comum: Recusas, vinculo: Recusas, criados: Criados) -> BotStore {
    var computer = randomComputer("Mac")
    computer.route = .relayOnly
    let identity = DeviceIdentity(signing: Curve25519.Signing.PrivateKey(), push: Curve25519.KeyAgreement.PrivateKey())
    let store = BotStore(computer: computer, storage: storage, monotonico: monotonico,
                         esperar: { _ in try await Task.sleep(for: .seconds(86_400)) }) { current in
        let t = ChannelTransport(computer: current, identity: identity, pairingCode: nil,
                                 dial: { _ in await comum.discar() }, watchesNetwork: false)
        criados.comuns.append(t)
        await t.start()
        return t
    }
    store.conectarRecemVinculado = { current in
        let t = ChannelTransport(computer: current, identity: identity, pairingCode: nil,
                                 dial: { _ in await vinculo.discar() }, watchesNetwork: false, recemVinculado: true)
        criados.tolerantes.append(t)
        await t.start()
        return t
    }
    return store
}

@MainActor @Test func revokedBeforeTheFirstIdentificationExhaustsTheToleranceOnceOnly() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let base = ContinuousClock.now
    let comum = Recusas()
    let vinculo = Recusas()
    let criados = Criados()
    let store = lojaRecusada(storage, monotonico: { base }, comum: comum, vinculo: vinculo, criados: criados)
    defer { store.retire() }
    store.vinculoGravadoEm = base
    store.setActive(true)

    // The just-linked transport retries the early 403s, once, then gives up for good.
    #expect(await until { await vinculo.dials >= 1 })
    let tolerante = try #require(criados.tolerantes.first)
    for n in 2...4 {
        await tolerante.reconnect() // skips the backoff
        #expect(await until { await vinculo.dials >= n })
    }
    #expect(await until { store.estadoConexao.estaDesconectado })
    #expect(await tolerante.isStopped)
    #expect(await vinculo.dials == 4)

    // Foreground: a new transport without tolerance ends on its first 403.
    store.setActive(false)
    store.setActive(true)
    #expect(await until { criados.comuns.count == 1 })
    #expect(await until { await criados.comuns[0].isStopped })
    #expect(await comum.dials == 1)

    // "Try now": the same, one dial.
    store.tentarAgora()
    #expect(await until { criados.comuns.count == 2 })
    #expect(await until { await criados.comuns[1].isStopped })
    #expect(await comum.dials == 2)

    #expect(criados.tolerantes.count == 1)
    #expect(await vinculo.dials == 4)
    #expect(store.estadoConexao.estaDesconectado)
}

@MainActor @Test func secondTransportBeforeTheFirstIdentificationHasNoTolerance() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let base = ContinuousClock.now
    let comum = Recusas()
    let vinculo = Recusas()
    let criados = Criados()
    let store = lojaRecusada(storage, monotonico: { base }, comum: comum, vinculo: vinculo, criados: criados)
    defer { store.retire() }
    store.vinculoGravadoEm = base
    store.setActive(true)
    #expect(await until { await vinculo.dials >= 1 })
    #expect(store.vinculoGravadoEm == nil)

    // Still retrying, not identified yet: "Try now" opens another transport, and it has no tolerance.
    store.tentarAgora()
    #expect(await until { criados.comuns.count == 1 })
    #expect(await until { await criados.comuns[0].isStopped })
    #expect(await comum.dials == 1)
    #expect(criados.tolerantes.count == 1)
    #expect(await until { store.estadoConexao.estaDesconectado })
}

@MainActor @Test func expiredLinkMarkGivesNoTolerance() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let base = ContinuousClock.now
    var passou: Duration = .zero
    let comum = Recusas()
    let vinculo = Recusas()
    let criados = Criados()
    let store = lojaRecusada(storage, monotonico: { base.advanced(by: passou) }, comum: comum, vinculo: vinculo, criados: criados)
    defer { store.retire() }
    store.vinculoGravadoEm = base
    // The relay has known a device linked two minutes ago for a while: no reason to tolerate a 403.
    passou = BotStore.prazoVinculoRecente
    store.setActive(true)
    #expect(await until { criados.comuns.count == 1 })
    #expect(await until { await criados.comuns[0].isStopped })
    #expect(await comum.dials == 1)
    #expect(criados.tolerantes.isEmpty)
    #expect(await vinculo.dials == 0)
    #expect(await until { store.estadoConexao.estaDesconectado })
}

@MainActor @Test func freshLinkConnectsThroughTheTolerantTransportOnce() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    var comum = 0
    var vinculo = 0
    let store = BotStore(computer: randomComputer("Mac"), storage: storage,
                         esperar: { _ in try await Task.sleep(for: .seconds(86_400)) }) { _ in
        comum += 1
        return fake
    }
    defer { store.retire() }
    store.vinculoGravadoEm = store.monotonico()
    store.conectarRecemVinculado = { _ in
        vinculo += 1
        return fake
    }
    store.setActive(true)
    #expect(await until { await fake.subscribed })
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    #expect(await until { store.temAutoridade })
    #expect(vinculo == 1 && comum == 0)
    #expect(store.vinculoGravadoEm == nil)
    // Any later transport (foreground, Try now) is an ordinary one.
    store.tentarAgora()
    #expect(await until { comum == 1 })
    #expect(vinculo == 1)
}

@MainActor @Test func savedComputerReopensWithoutTolerance() {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    storage.computers = [randomComputer("Mac")]
    var made: [BotStore] = []
    let accounts = AccountStore(storage: storage, cloud: nil) { computer in
        let store = BotStore(computer: computer, storage: storage) { _ in FakeRemote(.connecting) }
        made.append(store)
        return store
    }
    defer { accounts.retire() }
    #expect(made.count == 1)
    #expect(made.first?.vinculoGravadoEm == nil)
}

// MARK: - The link of the tap

// Each action carries the link of when the person tapped. Here the link
// changes after that moment and before the action's task runs (a drop and a new `hello` handled in
// between): nothing may go out on the new link, and the error lands where it always did. No clocks:
// the tests wait on the store's own changes and on the transport's subscriptions.

/// Waits until `condition` holds, waking on each change of what it reads (Observation), never on time.
@MainActor
private func quando(_ condition: @MainActor () -> Bool) async {
    while !condition() {
        await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in
            withObservationTracking {
                _ = condition()
            } onChange: {
                c.resume()
            }
        }
    }
}

/// A connected store, reached by waiting on events (the subscription, then the authority).
@MainActor
private func lojaPronta(_ storage: SharedStore.Context, _ fake: FakeRemote) async -> BotStore {
    let store = BotStore(computer: randomComputer("Mac"), storage: storage,
                         esperar: { _ in try await Task.sleep(for: .seconds(86_400)) }) { _ in fake }
    store.setActive(true)
    var inscricoes = fake.inscricoes.makeAsyncIterator()
    _ = await inscricoes.next()
    await fake.emit(botEvent("b1", name: "Bot", rev: 1))
    await quando { store.temAutoridade }
    return store
}

@MainActor @Test(.timeLimit(.minutes(1))) func performUsesTheLinkOfTheTap() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    store.stop("b1")
    store.linkId += 1 // still on the main actor: the action's task hasn't started yet
    await quando { store.lastError != nil }
    #expect(store.lastError == HostError.unreachable.localizedDescription)
    #expect(await !fake.calls.contains("stop"))
}

@MainActor @Test(.timeLimit(.minutes(1))) func respondUsesTheLinkOfTheTap() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    let card = Entry(id: "p1", seq: 1, botId: "b1", rev: 1, kind: "permission", turn: 1,
                     data: EntryData(status: "pending"), createdAt: 1, updatedAt: 1)
    store.respond(card, option: "allow")
    store.linkId += 1
    await quando { store.answering["p1"] == nil }
    #expect(store.lastError == HostError.unreachable.localizedDescription)
    #expect(await !fake.calls.contains("respondPermission"))
}

@MainActor @Test(.timeLimit(.minutes(1))) func deliverUsesTheLinkOfTheTap() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    store.send("oi", to: "b1")
    store.linkId += 1
    await quando { store.chat("b1").last?.data.status == "failed" }
    #expect(await fake.sent.isEmpty)
    #expect(await !fake.calls.contains("send"))
}

/// The public edit takes the mark read at the tap; a link that came after it carries nothing.
@MainActor @Test(.timeLimit(.minutes(1))) func editHandoffUsesTheMarkOfTheTap() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    let handoff = Entry(id: "h1", seq: 1, botId: "b1", rev: 1, kind: "notice", turn: 1,
                        data: EntryData(text: "Resumo"), createdAt: 1, updatedAt: 1)
    let marca = store.marcaDoLink
    store.linkId += 1
    #expect(await store.editHandoff(handoff, text: "Outro resumo", marca: marca) == false)
    #expect(store.lastError == HostError.unreachable.localizedDescription)
    #expect(await !fake.calls.contains("editHandoff"))
}

@MainActor @Test(.timeLimit(.minutes(1))) func unsupportedSessionSendAndRetryAreRefused() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    store.send("oi", to: "b1", sessionId: "s1")
    #expect(store.chat("b1", sessionId: "s1").isEmpty)
    #expect(store.lastError != nil)
    let failed = Entry(id: "local-n", seq: .max, botId: "b1", rev: 0, kind: "user", turn: 0,
                       data: EntryData(text: "oi", status: "failed", clientNonce: "n"), createdAt: 1, updatedAt: 0, sessionId: "s1")
    store.upsert(failed)
    store.retry(failed)
    #expect(store.chat("b1", sessionId: "s1") == [failed])
    #expect(await fake.sent.isEmpty)
}

@MainActor @Test(.timeLimit(.minutes(1))) func retryPreservesSession() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    _ = try await store.sessions("b1", marca: store.marcaDoLink)
    let failed = Entry(id: "local-n", seq: .max, botId: "b1", rev: 0, kind: "user", turn: 0,
                       data: EntryData(text: "oi", status: "failed", clientNonce: "n"), createdAt: 1, updatedAt: 0, sessionId: "s1")
    store.upsert(failed)
    store.retry(failed)
    await quando { store.chat("b1", sessionId: "s1").last?.id == "e1" }
    #expect(await fake.sendSessions == ["s1"])
    #expect(store.chat("b1").isEmpty)
}

@MainActor @Test(.timeLimit(.minutes(1))) func absentSessionActionsDoNotEnableSend() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    _ = try await store.sessions("b1", marca: store.marcaDoLink)
    await fake.setSessionResponse(#"{"sessions":[]}"#)
    let result = try await store.sessions("b1", marca: store.marcaDoLink)
    #expect(result.sessionActions == nil)
    store.send("oi", to: "b1", sessionId: "s1")
    #expect(store.chat("b1", sessionId: "s1").isEmpty)
    #expect(await fake.sent.isEmpty)
}

@MainActor @Test(.timeLimit(.minutes(1))) func historySkipsOnlyInvalidSessionEntry() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    await fake.setHistoryResponse(#"{"entries":[{"id":"a","seq":1,"botId":"b1","rev":1,"kind":"user","turn":1,"data":{},"createdAt":1,"updatedAt":1},{"id":"bad","seq":2,"botId":"b1","rev":2,"kind":"user","turn":1,"data":{},"createdAt":1,"updatedAt":1,"sessionId":""},{"id":"c","seq":3,"botId":"b1","rev":3,"kind":"user","turn":1,"data":{},"createdAt":1,"updatedAt":1,"sessionId":"b1"}]}"#)
    let rows = try await store.loadHistory("b1", marca: store.marcaDoLink)
    #expect(rows.map(\.id) == ["a", "c"])
}

@MainActor @Test(.timeLimit(.minutes(1))) func invalidSessionInFullPageDoesNotEndPagination() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    let entries: [[String: Any]] = (1...100).map { seq in
        var row: [String: Any] = ["id": "e\(seq)", "seq": seq, "botId": "b1", "rev": seq,
                                  "kind": "user", "turn": 1, "data": [String: String](), "createdAt": 1, "updatedAt": 1]
        if seq == 50 { row["sessionId"] = "" }
        return row
    }
    let firstPage = try JSONSerialization.data(withJSONObject: ["entries": entries])
    await fake.setHistoryResponse(String(decoding: firstPage, as: UTF8.self))
    await store.loadOlder("b1")
    #expect(store.chat("b1").count == 99)
    #expect(!store.historyComplete.contains("b1"))
    await fake.setHistoryResponse(#"{"entries":[]}"#)
    await store.loadOlder("b1")
    let bodies = await fake.historyBodies
    #expect(bodies.count == 2)
    let second = try #require(try JSONSerialization.jsonObject(with: bodies[1]) as? [String: Any])
    #expect(second["beforeSeq"] as? Int == 1)
    #expect(store.historyComplete.contains("b1"))
}

@MainActor @Test(.timeLimit(.minutes(1))) func loadOlderRequestsPrincipalSession() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    await store.loadOlder("b1")
    let bodies = await fake.historyBodies
    let primeiro = try #require(bodies.first)
    let body = try #require(try JSONSerialization.jsonObject(with: primeiro) as? [String: Any])
    #expect(body["sessionId"] as? String == "b1")
}

@MainActor @Test(.timeLimit(.minutes(1))) func sessionHistoryAndLiveEntriesStaySeparate() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    try await store.loadHistory("b1", sessionId: "s1", marca: store.marcaDoLink)
    try await store.loadHistory("b1", marca: store.marcaDoLink)
    let bodies = await fake.historyBodies
    let extra = try #require(try JSONSerialization.jsonObject(with: bodies[0]) as? [String: Any])
    let main = try #require(try JSONSerialization.jsonObject(with: bodies[1]) as? [String: Any])
    #expect(extra["sessionId"] as? String == "s1")
    #expect(main["sessionId"] as? String == "b1")
    await fake.emit(#"{"type":"entry","entry":{"id":"s","seq":2,"botId":"b1","rev":2,"kind":"user","turn":1,"data":{"text":"extra"},"createdAt":1,"updatedAt":1,"sessionId":"s1"}}"#)
    await fake.emit(#"{"type":"entry","entry":{"id":"m","seq":3,"botId":"b1","rev":3,"kind":"user","turn":1,"data":{"text":"principal"},"createdAt":1,"updatedAt":1}}"#)
    await quando { store.chat("b1").contains { $0.id == "m" } }
    #expect(store.chat("b1").map(\.id) == ["m"])
    #expect(store.chat("b1", sessionId: "s1").map(\.id) == ["s"])
}

@MainActor @Test(.timeLimit(.minutes(1))) func sessionSendUsesTheLinkOfTheTap() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    _ = try await store.sessions("b1", marca: store.marcaDoLink)
    store.send("oi", to: "b1", sessionId: "s1")
    store.linkId += 1
    await quando { store.chat("b1", sessionId: "s1").last?.data.status == "failed" }
    #expect(await fake.sent.isEmpty)
}

@MainActor @Test(.timeLimit(.minutes(1))) func sessionSendCarriesTheDestination() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    _ = try await store.sessions("b1", marca: store.marcaDoLink)
    store.send("oi", to: "b1", sessionId: "s1")
    await quando { store.chat("b1", sessionId: "s1").last?.id == "e1" }
    #expect(await fake.sendSessions == ["s1"])
    #expect(store.chat("b1").isEmpty)
}

/// Numa sessão extra, o que foi colado vai marcado no payload, como na Principal.
@MainActor @Test(.timeLimit(.minutes(1))) func sessionSendCarriesWhatWasPasted() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    let fonte = MXFonteReal(store: store)
    let extra = MXIdDaConversa(agente: "b1", sessao: "s1").texto
    await fonte.carregarSessoes(extra)
    #expect(fonte.conversa(extra)?.semEnvio == nil)
    let colado = "trecho colado de outra janela, bem comprido"
    fonte.guardarRascunho("antes ", em: extra)
    fonte.guardarRascunho("antes " + colado, em: extra)
    fonte.enviar("antes " + colado, para: extra)
    await quando { store.chat("b1", sessionId: "s1").last?.id == "e1" }
    #expect(await fake.sendSessions == ["s1"])
    #expect(await fake.sentPasted == [[colado]])
    // A marcação não vaza para o envio seguinte (digitado, sem colar).
    fonte.guardarRascunho("digitado", em: extra)
    fonte.enviar("digitado", para: extra)
    await quando { store.chat("b1", sessionId: "s1").last?.id == "e2" }
    #expect(await fake.sentPasted == [[colado], []])
}

@MainActor @Test(.timeLimit(.minutes(1))) func oldMarkWithSessionIsRejectedBeforeMailbox() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.hostOffline(lastSeen: nil))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage,
                         esperar: { _ in try await Task.sleep(for: .seconds(86_400)) }) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    await quando { store.canQueue }
    // The catalog previously confirmed support while this Hub was online.
    store.sessionSendSupported.insert("b1")
    let old = store.marcaDoLink
    store.linkId += 1
    store.send("antiga", to: "b1", sessionId: "s1", marca: old)
    #expect(store.chat("b1", sessionId: "s1").first?.data.status == "failed")
    #expect(await fake.enqueued.isEmpty)
    store.send("atual", to: "b1", sessionId: "s1", marca: store.marcaDoLink)
    #expect(store.chat("b1", sessionId: "s1").last?.data.status == "waiting")
    // The fake receives this call after the synchronous refusal above.
    await fake.waitForEnqueue()
    #expect(await fake.queuedSessions == ["s1"])
    #expect(await fake.enqueued.count == 1)
}

@MainActor @Test(.timeLimit(.minutes(1))) func oldMarkWithSessionIsRejectedOnLiveLink() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    _ = try await store.sessions("b1", marca: store.marcaDoLink)
    let old = store.marcaDoLink
    store.linkId += 1
    store.send("antiga", to: "b1", sessionId: "s1", marca: old)
    await quando { store.chat("b1", sessionId: "s1").last?.data.status == "failed" }
    #expect(await fake.sent.isEmpty)
}

// MARK: - The link of the speech

// What is said by voice goes out only on the link its audio was captured on. The engines stamp each
// buffer as the microphone delivers it; the tests drive the engine with stamped buffers and hold the
// transcription until the link has changed. No clocks: they wait on events.

/// A transcription that answers only when the test says so; each request is announced.
@MainActor
private final class Transcricao {
    let chegou = AsyncStream<Void>.makeStream()
    private var esperando: [CheckedContinuation<String, Never>] = []
    /// The size of each WAV asked to be transcribed, in order.
    private(set) var tamanhos: [Int] = []

    func transcrever(_ wav: Data) async -> String {
        tamanhos.append(wav.count)
        chegou.continuation.yield(())
        return await withCheckedContinuation { esperando.append($0) }
    }

    func responder(_ texto: String) {
        guard !esperando.isEmpty else { return }
        esperando.removeFirst().resume(returning: texto)
    }
}

/// The phone's speech engine on `store`, with `transcricao` in place of the provider, already listening.
@MainActor
private func vozDaNuvem(_ store: BotStore, _ transcricao: Transcricao) -> CloudSpeechEngine {
    let engine = CloudSpeechEngine(provider: .openAI, speech: .init(
        transcribe: { await transcricao.transcrever($0) },
        speak: { _ in (Data(), "audio/mpeg") },
        marcar: store.marcador,
        send: { store.send($0, to: "b1", marca: $1) }
    ))
    engine.phase = .listening
    return engine
}

/// A whole utterance at once (the engine's largest): submitted as soon as it is heard.
private let falaInteira = Data(count: 16000 * 2 * 20)

@MainActor @Test(.timeLimit(.minutes(1))) func reconnectionBeforeTheFirstResultSendsNothing() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    let transcricao = Transcricao()
    let engine = vozDaNuvem(store, transcricao)
    var chegadas = transcricao.chegou.stream.makeAsyncIterator()

    engine.heard(falaInteira, level: 1, marca: store.marcador())
    _ = await chegadas.next()
    store.linkId += 1 // the link drops and comes back before the text is ready
    transcricao.responder("oi")
    await quando { store.chat("b1").last?.data.status == "failed" }
    #expect(await fake.sent.isEmpty)
}

@MainActor @Test(.timeLimit(.minutes(1))) func speechThatCrossesAReconnectionIsDroppedWhole() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    let transcricao = Transcricao()
    let engine = vozDaNuvem(store, transcricao)
    var agora = Date(timeIntervalSince1970: 1_000_000)
    engine.agora = { agora }
    var chegadas = transcricao.chegou.stream.makeAsyncIterator()

    // Speech begins on one link…
    engine.heard(Data(count: 3200), level: 1, marca: store.marcador())
    store.linkId += 1
    // …and goes on after the reconnection: none of it counts, not even a whole utterance's worth.
    engine.heard(Data(count: 3200), level: 1, marca: store.marcador())
    engine.heard(falaInteira, level: 1, marca: store.marcador())
    // It ends (a pause)…
    engine.heard(Data(count: 3200), level: 0, marca: store.marcador())
    agora = agora.addingTimeInterval(60)
    engine.heard(Data(count: 3200), level: 0, marca: store.marcador())
    // …and the next utterance, begun on the new link, goes out.
    engine.heard(falaInteira, level: 1, marca: store.marcador())
    _ = await chegadas.next()
    // The first thing transcribed is that next utterance alone: nothing of the one that crossed.
    #expect(transcricao.tamanhos == [MicTap.wav(falaInteira).count])
    transcricao.responder("seguinte")
    await quando { store.chat("b1").contains { $0.id == "e1" } }
    #expect(await fake.sent == ["seguinte"])
}

@MainActor @Test(.timeLimit(.minutes(1))) func twoUtterancesOnDifferentLinksEachKeepTheirOwnMark() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    let transcricao = Transcricao()
    let engine = vozDaNuvem(store, transcricao)
    var chegadas = transcricao.chegou.stream.makeAsyncIterator()

    engine.heard(falaInteira, level: 1, marca: store.marcador())
    _ = await chegadas.next()
    store.linkId += 1
    // A second utterance on the new link while the first is still being transcribed.
    engine.heard(falaInteira, level: 1, marca: store.marcador())
    transcricao.responder("primeira")
    await quando { store.chat("b1").contains { $0.data.text == "primeira" && $0.data.status == "failed" } }
    _ = await chegadas.next()
    transcricao.responder("segunda")
    await quando { store.chat("b1").contains { $0.id == "e1" } }
    #expect(await fake.sent == ["segunda"])
}

/// A realtime call carries the link of when its microphone started; a later utterance doesn't move it.
@MainActor @Test(.timeLimit(.minutes(1))) func realtimeCallKeepsTheMarkOfItsAudio() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    await fake.emit(#"{"type":"entry","entry":{"id":"p1","seq":5,"botId":"b1","rev":5,"kind":"permission","turn":1,"data":{"title":"Update TODOs","status":"pending","voiceApproval":"allow_once","options":[{"optionId":"a","name":"Allow once","kind":"allow_once"},{"optionId":"r","name":"Reject","kind":"reject_once"}]},"createdAt":1,"updatedAt":1}}"#)
    await quando { store.chat("b1").contains { $0.id == "p1" } }
    let op = CallOperator(model: store, botId: "b1")
    // No audio yet: nothing to act on.
    #expect(op.call("send_to_bot", arguments: ["text": "oi"]).contains("error"))

    op.audioComecou()
    store.linkId += 1
    op.audioComecou() // speech on the new link doesn't replace the call's mark
    op.falaDetectada()
    _ = op.call("send_to_bot", arguments: ["text": "oi"])
    await quando { store.chat("b1").last?.data.status == "failed" }
    #expect(op.call("answer_approval", arguments: ["option": "a"]).contains("answered"))
    await quando { store.answering["p1"] == nil }
    #expect(store.lastError == HostError.unreachable.localizedDescription)
    #expect(await fake.sent.isEmpty)
    #expect(await !fake.calls.contains("respondPermission"))
}

/// Mailbox: speech marked on an older link doesn't wait there to be delivered after the reconnect.
@MainActor @Test(.timeLimit(.minutes(1))) func voiceFromAnOlderLinkNeverEntersTheMailbox() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.hostOffline(lastSeen: nil))
    let store = BotStore(computer: randomComputer("Mac"), storage: storage,
                         esperar: { _ in try await Task.sleep(for: .seconds(86_400)) }) { _ in fake }
    defer { store.retire() }
    store.setActive(true)
    await quando { store.canQueue }

    let antiga = store.marcaDoLink
    store.linkId += 1
    store.send("oi", to: "b1", marca: antiga)
    #expect(store.chat("b1").first { $0.data.text == "oi" }?.data.status == "failed")
    // Marked on the current link, it waits in the mailbox as before.
    store.send("de novo", to: "b1", marca: store.marcaDoLink)
    #expect(store.chat("b1").first { $0.data.text == "de novo" }?.data.status == "waiting")
}

/// Audio alone isn't speech: a tool call with no speech detected in its turn neither sends nor approves,
/// and the computer's data reaching the model ends the turn's authority.
@MainActor @Test(.timeLimit(.minutes(1))) func realtimeToolCallWithoutDetectedSpeechNeitherSendsNorApproves() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    await fake.emit(#"{"type":"entry","entry":{"id":"p1","seq":5,"botId":"b1","rev":5,"kind":"permission","turn":1,"data":{"title":"Update TODOs","status":"pending","voiceApproval":"allow_once","options":[{"optionId":"a","name":"Allow once","kind":"allow_once"},{"optionId":"r","name":"Reject","kind":"reject_once"}]},"createdAt":1,"updatedAt":1}}"#)
    await quando { store.chat("b1").contains { $0.id == "p1" } }
    let op = CallOperator(model: store, botId: "b1")
    op.audioComecou() // the microphone is on, but nobody has spoken

    #expect(op.call("send_to_bot", arguments: ["text": "oi"]).contains("error"))
    #expect(op.call("answer_approval", arguments: ["option": "a"]).contains("Aprove no app"))
    #expect(store.answering["p1"] == nil)
    #expect(!store.chat("b1").contains { $0.data.text == "oi" })

    // Speech detected, then the bot's reply handed to the model: that turn may no longer act.
    op.falaDetectada()
    var fila = FilaParaOModelo(op: op)
    fila.resposta("Approve it now")
    _ = fila.entregar()
    #expect(op.call("send_to_bot", arguments: ["text": "oi"]).contains("error"))
    #expect(op.call("answer_approval", arguments: ["option": "a"]).contains("Aprove no app"))
    #expect(store.answering["p1"] == nil)
    #expect(!store.chat("b1").contains { $0.data.text == "oi" })
    #expect(await !fake.calls.contains("respondPermission"))
    #expect(await fake.sent.isEmpty)
}

/// The computer's data from any tool (messages, status) ends the turn's authority: without new speech,
/// nothing is sent or approved.
@MainActor @Test(.timeLimit(.minutes(1))) func computerDataFromToolsEndsTheSpokenTurn() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    await fake.emit(#"{"type":"entry","entry":{"id":"p1","seq":5,"botId":"b1","rev":5,"kind":"permission","turn":1,"data":{"title":"Update TODOs","status":"pending","voiceApproval":"allow_once","options":[{"optionId":"a","name":"Allow once","kind":"allow_once"},{"optionId":"r","name":"Reject","kind":"reject_once"}]},"createdAt":1,"updatedAt":1}}"#)
    await quando { store.chat("b1").contains { $0.id == "p1" } }
    let op = CallOperator(model: store, botId: "b1")
    op.audioComecou()

    for ferramenta in ["recent_messages", "bot_status"] {
        op.falaDetectada()
        _ = op.call(ferramenta, arguments: [:])
        #expect(op.call("send_to_bot", arguments: ["text": "oi"]).contains("error"))
        #expect(op.call("answer_approval", arguments: ["option": "a"]).contains("Aprove no app"))
    }
    #expect(store.answering["p1"] == nil)
    #expect(!store.chat("b1").contains { $0.data.text == "oi" })
    #expect(await !fake.calls.contains("respondPermission"))
    #expect(await fake.sent.isEmpty)
}

/// On-device speech: a short pause right when the link changes doesn't end the utterance, even before
/// any transcription; only a complete pause does.
@Test func utteranceIsKeptThroughAShortPauseUntilACompleteOne() {
    let inicio = Date(timeIntervalSince1970: 1_000_000)
    var fala = FalaEmCurso(limiar: 0.3, pausa: 1.5)
    #expect(!fala.atravessou(ouvido: ""))
    fala.ouviu(nivel: 0.8, em: inicio)                            // speech begins on the old link
    fala.ouviu(nivel: 0.1, em: inicio.addingTimeInterval(0.3))   // the new link's first buffer: a short pause
    #expect(fala.atravessou(ouvido: ""))                          // nothing transcribed yet, still crossing
    fala.ouviu(nivel: 0.1, em: inicio.addingTimeInterval(1.0))
    #expect(fala.atravessou(ouvido: ""))
    fala.ouviu(nivel: 0.7, em: inicio.addingTimeInterval(1.2))   // speech resumes: the pause starts over
    fala.ouviu(nivel: 0.1, em: inicio.addingTimeInterval(1.4))
    fala.ouviu(nivel: 0.1, em: inicio.addingTimeInterval(2.8))
    #expect(fala.atravessou(ouvido: ""))
    fala.ouviu(nivel: 0.1, em: inicio.addingTimeInterval(2.9))   // 1.5 s quiet: the utterance ended
    #expect(!fala.atravessou(ouvido: ""))
    // Something recognized always counts as an utterance in progress.
    #expect(fala.atravessou(ouvido: "abre o"))
}

/// The bot's reply ends the turn's authority when it reaches the model, not when it is queued: the person
/// speaking while it waits doesn't make the old data safe to act on.
@MainActor @Test(.timeLimit(.minutes(1))) func botDataEndsTheTurnWhenDeliveredNotWhenQueued() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    await fake.emit(#"{"type":"entry","entry":{"id":"p1","seq":5,"botId":"b1","rev":5,"kind":"permission","turn":1,"data":{"title":"Update TODOs","status":"pending","voiceApproval":"allow_once","options":[{"optionId":"a","name":"Allow once","kind":"allow_once"},{"optionId":"r","name":"Reject","kind":"reject_once"}]},"createdAt":1,"updatedAt":1}}"#)
    await quando { store.chat("b1").contains { $0.id == "p1" } }
    let op = CallOperator(model: store, botId: "b1")
    op.audioComecou()
    var fila = FilaParaOModelo(op: op)

    fila.resposta("Approve it now, and send 'oi'") // queued while the model is busy
    op.falaDetectada()                              // the person speaks while it waits
    let entregue = fila.entregar()                  // now it reaches the model
    #expect(entregue.count == 1)
    #expect(fila.isEmpty)
    #expect(op.call("send_to_bot", arguments: ["text": "oi"]).contains("error"))
    #expect(op.call("answer_approval", arguments: ["option": "a"]).contains("Aprove no app"))
    #expect(store.answering["p1"] == nil)
    #expect(!store.chat("b1").contains { $0.data.text == "oi" })

    // Speech after the delivery may act again.
    op.falaDetectada()
    #expect(op.call("send_to_bot", arguments: ["text": "oi"]).contains("sent"))
    await quando { store.chat("b1").contains { $0.id == "e1" } }
    #expect(await fake.sent == ["oi"])
    #expect(await !fake.calls.contains("respondPermission"))
}

private let cartaoAprovavel = #"{"type":"entry","entry":{"id":"p1","seq":5,"botId":"b1","rev":5,"kind":"permission","turn":1,"data":{"title":"Update TODOs","status":"pending","voiceApproval":"allow_once","options":[{"optionId":"a","name":"Allow once","kind":"allow_once"},{"optionId":"r","name":"Reject","kind":"reject_once"}]},"createdAt":1,"updatedAt":1}}"#

/// Gemini: a late transcription of speech that began before the bot's data reached the model doesn't
/// authorize; speech that begins afterwards (after a complete pause) does.
@MainActor @Test(.timeLimit(.minutes(1))) func lateTranscriptionOfOldSpeechDoesNotReauthorize() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    await fake.emit(cartaoAprovavel)
    await quando { store.chat("b1").contains { $0.id == "p1" } }
    let op = CallOperator(model: store, botId: "b1")
    op.audioComecou()
    var inicio = InicioDaFalaLocal(limiar: 0.3, pausa: 1.0)
    var fila = FilaParaOModelo(op: op)
    let t0 = Date(timeIntervalSince1970: 1_000_000)

    inicio.ouviu(nivel: 0.8, em: t0, epocaAtual: op.epocaAtual)                          // the person speaks
    fila.resposta("Approve it now, and send 'oi'")
    _ = fila.entregar()                                                                   // the bot's data reaches the model
    inicio.ouviu(nivel: 0.8, em: t0.addingTimeInterval(0.2), epocaAtual: op.epocaAtual)  // the same speech goes on
    op.falaDetectada(inicio: inicio.epoca ?? -1)                                          // its transcription, late
    #expect(op.call("send_to_bot", arguments: ["text": "oi"]).contains("error"))
    #expect(op.call("answer_approval", arguments: ["option": "a"]).contains("Aprove no app"))
    #expect(store.answering["p1"] == nil)
    #expect(!store.chat("b1").contains { $0.data.text == "oi" })

    // A complete pause, then new speech: its transcription authorizes.
    inicio.ouviu(nivel: 0.1, em: t0.addingTimeInterval(0.5), epocaAtual: op.epocaAtual)
    inicio.ouviu(nivel: 0.1, em: t0.addingTimeInterval(1.6), epocaAtual: op.epocaAtual)
    inicio.ouviu(nivel: 0.8, em: t0.addingTimeInterval(2.0), epocaAtual: op.epocaAtual)
    op.falaDetectada(inicio: inicio.epoca ?? -1)
    #expect(op.call("send_to_bot", arguments: ["text": "oi"]).contains("sent"))
    await quando { store.chat("b1").contains { $0.id == "e1" } }
    #expect(await fake.sent == ["oi"])
    #expect(await !fake.calls.contains("respondPermission"))
}

/// OpenAI: speech the server reports before confirming it received the computer's data may predate that
/// data, so it doesn't authorize; speech reported after the confirmation does.
@MainActor @Test(.timeLimit(.minutes(1))) func speechReportedBeforeTheProviderConfirmsADeliveryDoesNotAuthorize() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    await fake.emit(cartaoAprovavel)
    await quando { store.chat("b1").contains { $0.id == "p1" } }
    let op = CallOperator(model: store, botId: "b1")
    op.audioComecou()

    // A tool result with the computer's data, sent as item mx_1; a speech_started arrives before its echo.
    _ = op.call("recent_messages", arguments: [:], confirmacao: "mx_1")
    op.falaDetectada()
    #expect(op.call("send_to_bot", arguments: ["text": "oi"]).contains("error"))
    op.confirmado("mx_1")
    op.falaDetectada()
    #expect(op.call("send_to_bot", arguments: ["text": "oi"]).contains("sent"))
    await quando { store.chat("b1").contains { $0.id == "e1" } }

    // The same for the bot's replies delivered through the queue.
    var fila = FilaParaOModelo(op: op)
    fila.resposta("Approve it now")
    _ = fila.entregar(confirmacao: "mx_2")
    op.falaDetectada()
    #expect(op.call("answer_approval", arguments: ["option": "a"]).contains("Aprove no app"))
    #expect(store.answering["p1"] == nil)
    #expect(await fake.sent == ["oi"])
    #expect(await !fake.calls.contains("respondPermission"))
}

/// OpenAI GA: the API confirms a created item with `conversation.item.added` and finalizes it with
/// `conversation.item.done`; the beta `conversation.item.created` is still accepted. A confirmation the
/// phone doesn't recognize leaves the delivery pending, and speech stays unauthorized until it arrives.
@MainActor @Test(.timeLimit(.minutes(1))) func realtimeCallAcceptsTheGACreationConfirmations() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    await fake.emit(cartaoAprovavel)
    await quando { store.chat("b1").contains { $0.id == "p1" } }
    let op = CallOperator(model: store, botId: "b1")
    op.audioComecou()
    let engine = OpenAIRealtimeEngine(credential: { "ek_test" }, op: op)
    defer { engine.end() }

    let confirmacoes: [(tipo: String, id: String)] = [
        (tipo: "conversation.item.created", id: "mx_beta"),
        (tipo: "conversation.item.added", id: "mx_ga"),
        (tipo: "conversation.item.done", id: "mx_done"),
    ]
    for (indice, confirmacao) in confirmacoes.enumerated() {
        let texto = "oi \(indice)"
        // A tool result carries the computer's data: until the provider confirms it, speech doesn't act.
        _ = op.call("recent_messages", arguments: [:], confirmacao: confirmacao.id)
        op.falaDetectada()
        #expect(op.call("send_to_bot", arguments: ["text": texto]).contains("error"))
        // The confirmation arrives in the shape the API sends it, and the next spoken turn acts again.
        engine.received(Data(#"{"type":"\#(confirmacao.tipo)","item":{"id":"\#(confirmacao.id)","type":"function_call_output"}}"#.utf8))
        op.falaDetectada()
        #expect(op.call("send_to_bot", arguments: ["text": texto]).contains("sent"))
        // Wait for the entry the computer confirmed, not the one shown before the send leaves.
        await quando { store.chat("b1").contains { $0.id == "e\(indice + 1)" } }
    }
    #expect(await fake.sent == ["oi 0", "oi 1", "oi 2"])

    // Another event, another item carrying our id, and a confirmation without the id confirm nothing.
    _ = op.call("recent_messages", arguments: [:], confirmacao: "mx_other")
    engine.received(Data(#"{"type":"conversation.item.input_audio_transcription.completed","item":{"id":"mx_other","type":"function_call_output"}}"#.utf8))
    engine.received(Data(#"{"type":"conversation.item.added","item":{"id":"mx_other","type":"message"}}"#.utf8))
    engine.received(Data(#"{"type":"conversation.item.added"}"#.utf8))
    op.falaDetectada()
    #expect(op.call("send_to_bot", arguments: ["text": "fora"]).contains("error"))
    // Our own function-call output with the id confirms it and speech acts again.
    engine.received(Data(#"{"type":"conversation.item.added","item":{"id":"mx_other","type":"function_call_output"}}"#.utf8))
    op.falaDetectada()
    #expect(op.call("send_to_bot", arguments: ["text": "fora"]).contains("sent"))
    await quando { store.chat("b1").contains { $0.id == "e4" } }
    #expect(await fake.sent == ["oi 0", "oi 1", "oi 2", "fora"])
}

/// Gemini: a buffer captured before the bot's data reached the model but processed after it keeps the
/// epoch it was captured at (stamped off the main actor, as the microphone's tap does), so its speech
/// doesn't authorize.
@MainActor @Test(.timeLimit(.minutes(1))) func bufferCapturedBeforeADeliveryKeepsItsEpoch() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    await fake.emit(cartaoAprovavel)
    await quando { store.chat("b1").contains { $0.id == "p1" } }
    let op = CallOperator(model: store, botId: "b1")
    op.audioComecou()
    var inicio = InicioDaFalaLocal(limiar: 0.3, pausa: 1.0)
    var fila = FilaParaOModelo(op: op)
    let t0 = Date(timeIntervalSince1970: 1_000_000)

    // The buffer is captured: stamped on another thread, as the tap does before hopping to the main actor.
    let epocas = op.epocas
    let carimbo = await Task.detached { epocas.ler() }.value
    // The bot's data reaches the model before the buffer is processed.
    fila.resposta("Approve it now, and send 'oi'")
    _ = fila.entregar()
    #expect(op.epocaAtual != carimbo)
    // The buffer is processed now, with the epoch it was captured at, and its speech is transcribed.
    inicio.ouviu(nivel: 0.8, em: t0, epocaAtual: carimbo)
    op.falaDetectada(inicio: inicio.epoca ?? -1)
    #expect(op.call("send_to_bot", arguments: ["text": "oi"]).contains("error"))
    #expect(op.call("answer_approval", arguments: ["option": "a"]).contains("Aprove no app"))
    #expect(store.answering["p1"] == nil)
    #expect(!store.chat("b1").contains { $0.data.text == "oi" })
    #expect(await fake.sent.isEmpty)
    #expect(await !fake.calls.contains("respondPermission"))
}

@MainActor @Test(.timeLimit(.minutes(1))) func deleteWaitsForHubSuccess() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    await quando { store.bots["b1"] != nil }
    let bot = try #require(store.bots["b1"])
    let entry = Entry(id: "e", seq: 1, botId: "b1", rev: 1, kind: "user", turn: 1, data: EntryData(text: "oi"), createdAt: 1, updatedAt: 1)
    store.upsert(entry)
    store.setComposerDraft("rascunho", for: "b1")
    store.delete(bot)
    #expect(store.bots["b1"] != nil && store.chat("b1") == [entry] && store.composerDraft(for: "b1") == "rascunho")
    await quando { store.bots["b1"] == nil }
    #expect(store.chat("b1").isEmpty && store.composerDraft(for: "b1").isEmpty)
    #expect(await fake.calls.filter { $0 == "deleteBot" }.count == 1)
}

@MainActor @Test(.timeLimit(.minutes(1))) func failedDeleteKeepsEverythingAndDoesNotRetry() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    await quando { store.bots["b1"] != nil }
    let bot = try #require(store.bots["b1"])
    let entry = Entry(id: "e", seq: 1, botId: "b1", rev: 1, kind: "user", turn: 1, data: EntryData(text: "oi"), createdAt: 1, updatedAt: 1)
    store.upsert(entry)
    store.setComposerDraft("rascunho", for: "b1")
    await fake.fail("deleteBot", times: 1)
    store.delete(bot)
    await quando { store.lastError != nil }
    #expect(store.bots["b1"] != nil && store.chat("b1") == [entry] && store.composerDraft(for: "b1") == "rascunho")
    #expect(await fake.calls.filter { $0 == "deleteBot" }.count == 1)
}

@MainActor @Test(.timeLimit(.minutes(1))) func hubRefusesDeleteWithoutLosingLocalData() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    await quando { store.bots["b1"] != nil }
    let bot = try #require(store.bots["b1"])
    let entry = Entry(id: "e", seq: 1, botId: "b1", rev: 1, kind: "user", turn: 1, data: EntryData(text: "oi"), createdAt: 1, updatedAt: 1)
    store.upsert(entry)
    store.setComposerDraft("rascunho", for: "b1")
    await fake.fail("deleteBot", with: .http(400, "recusado"))
    store.delete(bot)
    await quando { store.lastError != nil }
    #expect(store.lastError == "recusado")
    #expect(store.bots["b1"] != nil && store.chat("b1") == [entry] && store.composerDraft(for: "b1") == "rascunho")
    #expect(await fake.calls.filter { $0 == "deleteBot" }.count == 1)
}

@MainActor @Test(.timeLimit(.minutes(1))) func deleteCannotMoveToANewLink() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    await quando { store.bots["b1"] != nil }
    let bot = try #require(store.bots["b1"])
    let entry = Entry(id: "e", seq: 1, botId: "b1", rev: 1, kind: "user", turn: 1, data: EntryData(text: "oi"), createdAt: 1, updatedAt: 1)
    store.upsert(entry)
    store.setComposerDraft("rascunho", for: "b1")
    store.delete(bot)
    store.linkId += 1
    await quando { store.lastError != nil }
    #expect(store.bots["b1"] != nil && store.chat("b1") == [entry] && store.composerDraft(for: "b1") == "rascunho")
    #expect(await !fake.calls.contains("deleteBot"))
}

@MainActor @Test(.timeLimit(.minutes(1))) func editingSendsOnlyChangedFieldOnWire() async throws {
    let (storage, suite) = context()
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let fake = FakeRemote(.ready(.direct))
    let store = await lojaPronta(storage, fake)
    defer { store.retire() }
    await quando { store.bots["b1"] != nil }
    var draft = BotDraft(try #require(store.bots["b1"]))
    draft.name = "Novo"
    _ = try await store.save(draft)
    let bodies = await fake.updateBodies
    let primeiro = try #require(bodies.first)
    let body = try #require(try JSONSerialization.jsonObject(with: primeiro) as? [String: Any])
    #expect(Set(body.keys) == ["id", "name"])
    #expect(body["name"] as? String == "Novo")
}
