// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CryptoKit
import Foundation
import Testing
@testable import CodyncKit

private let v = Vectors.shared

/// One direction of a fake WebSocket.
actor Pipe {
    private var items: [String] = []
    private var waiters: [CheckedContinuation<String, Error>] = []
    private var closedCode: Int?

    func push(_ s: String) {
        guard closedCode == nil else { return }
        if waiters.isEmpty { items.append(s) } else { waiters.removeFirst().resume(returning: s) }
    }

    func pop() async throws -> String {
        if !items.isEmpty { return items.removeFirst() }
        if let closedCode { throw SocketClosed(code: closedCode) }
        return try await withCheckedThrowingContinuation { waiters.append($0) }
    }

    func close(_ code: Int) {
        guard closedCode == nil else { return }
        closedCode = code
        for w in waiters { w.resume(throwing: SocketClosed(code: code)) }
        waiters = []
    }
}

final class FakeSocket: ChannelSocket {
    let endpoint: Endpoint
    let toDevice = Pipe()
    let toHost = Pipe()

    init(_ endpoint: Endpoint) { self.endpoint = endpoint }

    func send(_ text: String) async throws { await toHost.push(text) }
    func receive() async throws -> String { try await toDevice.pop() }
    func ping() async throws {}
    func close(code: Int) {
        Task {
            await toDevice.close(code)
            await toHost.close(code)
        }
    }
}

private func object(_ text: String) -> [String: Any] {
    (try? JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any]) ?? [:]
}

private func text(_ obj: [String: Any]) -> String {
    String(decoding: (try? JSONSerialization.data(withJSONObject: obj)) ?? Data(), as: UTF8.self)
}

/// The host (or, for DO messages, the relay) on the far end of a fake socket.
struct HostSide {
    let socket: FakeSocket
    var sealer: RelayCrypto.FrameSealer?
    var opener: RelayCrypto.FrameOpener?

    init(_ socket: FakeSocket) { self.socket = socket }

    /// Next message that isn't a relay ping.
    func raw() async throws -> [String: Any] {
        while true {
            let o = object(try await socket.toHost.pop())
            if o["t"] as? String != "ping" { return o }
        }
    }

    func push(_ obj: [String: Any]) async { await socket.toDevice.push(text(obj)) }

    /// Verifies the device's hello and answers with a welcome signed by `hostSigner`.
    @discardableResult
    mutating func accept(hostSigner: Curve25519.Signing.PrivateKey = v.hostSign) async throws -> [String: Any] {
        let hello = try await raw()
        #expect(hello["t"] as? String == "hello")
        #expect(hello["v"] as? Int == 1)
        let dk = Data(base64URL: hello["dk"] as? String ?? "") ?? Data()
        let ekD = Data(base64URL: hello["ek"] as? String ?? "") ?? Data()
        let n = Data(base64URL: hello["n"] as? String ?? "") ?? Data()
        let cid = RelayCrypto.computerIdRaw(signKey: v.b64("keys", "hostSignPub"))
        let sig = Data(base64URL: hello["sig"] as? String ?? "") ?? Data()
        #expect(try Curve25519.Signing.PublicKey(rawRepresentation: dk)
            .isValidSignature(sig, for: RelayCrypto.hs1Input(cid: cid, dk: dk, ekD: ekD, n: n)))
        let ekH = Curve25519.KeyAgreement.PrivateKey()
        let th = RelayCrypto.transcriptHash(cid: cid, dk: dk, ekD: ekD, n: n, ekH: ekH.publicKey.rawRepresentation)
        await push(["t": "welcome", "v": 1, "ek": ekH.publicKey.rawRepresentation.base64URL, "sig": try hostSigner.signature(for: th).base64URL])
        let keys = RelayCrypto.channelKeys(sharedSecret: try RelayCrypto.sharedSecret(ekH, ekD), transcriptHash: th)
        sealer = RelayCrypto.FrameSealer(key: keys.h2d)
        opener = RelayCrypto.FrameOpener(key: keys.d2h)
        return hello
    }

    /// Next inner message from the device.
    mutating func receive() async throws -> [String: Any] {
        while true {
            let f = try await raw()
            guard f["t"] as? String == "f", var o = opener else { continue }
            let c = (f["c"] as? NSNumber)?.uint64Value ?? .max
            let message = try o.open(c: c, d: Data(base64URL: f["d"] as? String ?? "") ?? Data())
            opener = o
            if let message { return (try JSONSerialization.jsonObject(with: message) as? [String: Any]) ?? [:] }
        }
    }

    mutating func send(_ inner: [String: Any]) async throws {
        guard var s = sealer else { return }
        for frame in try s.seal(try JSONSerialization.data(withJSONObject: inner)) {
            await push(["t": "f", "c": frame.c, "d": frame.d.base64URL])
        }
        sealer = s
    }
}

/// A dialer handing each new fake socket to the test.
private func fakeDialer() -> (Dialer, AsyncStream<FakeSocket>) {
    let (sockets, sink) = AsyncStream<FakeSocket>.makeStream()
    return ({ endpoint in
        let s = FakeSocket(endpoint)
        sink.yield(s)
        return s
    }, sockets)
}

private func helloBody() -> [String: Any] { [
    "hostId": "h1", "name": "Studio", "version": "3.0.0", "os": "macos", "backends": [], "rev": 0,
    "computerId": v.str("keys", "computerId"), "signKey": v.str("keys", "hostSignPub"), "boxKey": v.str("keys", "hostBoxPub"),
    "urls": ["http://192.0.2.2:19222"], "cloud": "https://cloud.example.dev", "device": "macstudio",
] }

private func waitFor(_ states: AsyncStream<LinkState>, _ match: (LinkState) -> Bool) async -> LinkState? {
    for await s in states where match(s) { return s }
    return nil
}

@Test(.timeLimit(.minutes(1))) func directChannelMultiplexesCallsAndStreams() async throws {
    let (dial, sockets) = fakeDialer()
    var computer = v.computer
    computer.boxKey = nil
    computer.urls = ["http://127.0.0.1:1"]
    let t = ChannelTransport(computer: computer, identity: v.identity, pairingCode: nil, dial: dial, watchesNetwork: false)
    let a = t.states()
    let b = t.states()
    let updates = t.computerUpdates()
    await t.start()
    var socketIterator = sockets.makeAsyncIterator()
    let socket = try #require(await socketIterator.next())
    guard case let .direct(url) = socket.endpoint else { Issue.record("not direct"); return }
    #expect(url.absoluteString == "ws://127.0.0.1:1/channel?v=1")
    var host = HostSide(socket)
    let h = try await host.accept()
    #expect(h["pair"] == nil)

    // Every consumer sees the same state.
    #expect(await waitFor(a) { $0 == .ready(.direct) } == .ready(.direct))
    #expect(await waitFor(b) { $0 == .ready(.direct) } == .ready(.direct))

    // The transport asks for hello by itself and merges what it learns.
    let first = try await host.receive()
    #expect(first["m"] as? String == "hello")
    try await host.send(["id": first["id"] ?? 0, "ok": helloBody()])
    var updated = updates.makeAsyncIterator()
    let merged = try #require(await updated.next())
    #expect(merged.boxKey == v.str("keys", "hostBoxPub"))
    #expect(merged.urls == ["http://192.0.2.2:19222"])
    #expect(merged.cloud?.absoluteString == "https://cloud.example.dev")
    #expect(merged.device == "macstudio" && merged.name == "Studio")

    // Two calls in flight, answered out of order.
    async let usage = t.call("usage", body: Data(#"{"refresh":false}"#.utf8), timeout: 10)
    async let sync = t.call("sync", body: Data(#"{"since":7}"#.utf8), timeout: 10)
    let r1 = try await host.receive()
    let r2 = try await host.receive()
    #expect(r1["id"] as? Int != r2["id"] as? Int)
    for r in [r2, r1] { try await host.send(["id": r["id"] ?? 0, "ok": ["method": r["m"] ?? ""]]) }
    #expect(object(String(decoding: try await usage, as: UTF8.self))["method"] as? String == "usage")
    #expect(object(String(decoding: try await sync, as: UTF8.self))["method"] as? String == "sync")
    let syncBody = [r1, r2].first { $0["m"] as? String == "sync" }?["b"] as? [String: Any]
    #expect(syncBody?["since"] as? Int == 7)

    // Errors keep their HTTP status.
    let denied = Task { try await t.call("setScreenEnabled", body: Data(#"{"enabled":true}"#.utf8), timeout: 10) }
    let r3 = try await host.receive()
    try await host.send(["id": r3["id"] ?? 0, "err": ["status": 403, "message": "Only on the computer."]])
    await #expect(throws: HostError.http(403, "O computador recusou este pedido.")) { _ = try await denied.value }

    // A 400 never shows the Hub's text (it can carry internal detail); routine calls read it to recognise known refusals.
    let secret = "starting `npx x`: No such file /Users/user/.env token=sk-123"
    let refused = Task { try await t.call("saveRoutine", body: Data(#"{"botId":"b"}"#.utf8), timeout: 10) }
    let r4 = try await host.receive()
    try await host.send(["id": r4["id"] ?? 0, "err": ["status": 400, "message": secret]])
    await #expect(throws: HostError.refused(HubRefusal(secret))) { _ = try await refused.value }
    #expect(HostError.refused(HubRefusal(secret)).localizedDescription == "Não foi possível concluir. Tente de novo.")
    #expect(!"\(HostError.refused(HubRefusal(secret)))".contains("npx"))

    // A stream yields events until its consumer stops; the host is then told to cancel.
    let reader = Task {
        var got: [Data] = []
        for try await d in t.stream(.events(since: 5, client: "ios")) {
            got.append(d)
            if got.count == 2 { break }
        }
        return got
    }
    let sub = try await host.receive()
    #expect(sub["sub"] as? String == "events")
    #expect((sub["b"] as? [String: Any])?["since"] as? Int == 5)
    #expect((sub["b"] as? [String: Any])?["client"] as? String == "ios")
    try await host.send(["id": sub["id"] ?? 0, "ev": ["type": "hello", "hostId": "h1", "rev": 9]])
    try await host.send(["id": sub["id"] ?? 0, "ev": ["type": "resync"]])
    let got = try await reader.value
    #expect(got.count == 2 && object(String(decoding: got[1], as: UTF8.self))["type"] as? String == "resync")
    let cancel = try await host.receive()
    #expect(cancel["id"] as? Int == sub["id"] as? Int && cancel["cancel"] as? Bool == true)

    // Revoked: no retry.
    await socket.toDevice.close(4003)
    guard case .unauthorized = await waitFor(t.states(), { if case .unauthorized = $0 { true } else { false } }) else {
        Issue.record("expected unauthorized"); return
    }
    await #expect(throws: HostError.self) { _ = try await t.call("hello", body: Data(), timeout: 1) }
    await t.shutdown()
}

@Test(.timeLimit(.minutes(1))) func relayChannelMultiplexesCallsAndStreams() async throws {
    let (dial, sockets) = fakeDialer()
    var computer = v.computer
    computer.boxKey = nil
    computer.urls = []
    computer.cloud = URL(string: "https://cloud.example.dev")
    computer.route = .relayOnly
    let t = ChannelTransport(computer: computer, identity: v.identity, pairingCode: nil, dial: dial, watchesNetwork: false)
    let a = t.states()
    let b = t.states()
    let updates = t.computerUpdates()
    await t.start()
    var socketIterator = sockets.makeAsyncIterator()
    let socket = try #require(await socketIterator.next())
    guard case let .relay(request) = socket.endpoint else { Issue.record("not relay"); return }
    #expect(request.url?.absoluteString == "wss://cloud.example.dev/v1/relay/device/\(computer.id)?v=1")
    #expect(request.value(forHTTPHeaderField: "Codync-Sig")?.hasPrefix("v=1,kid=\(v.identity.publicKey),ts=") == true)
    var host = HostSide(socket)
    await host.push(["t": "presence", "online": true, "lastSeenAt": NSNull()])
    let h = try await host.accept()
    #expect(h["pair"] == nil)

    // Every consumer sees the same state.
    #expect(await waitFor(a) { $0 == .ready(.relay) } == .ready(.relay))
    #expect(await waitFor(b) { $0 == .ready(.relay) } == .ready(.relay))

    // The transport asks for hello by itself and merges what it learns.
    let first = try await host.receive()
    #expect(first["m"] as? String == "hello")
    try await host.send(["id": first["id"] ?? 0, "ok": helloBody()])
    var updated = updates.makeAsyncIterator()
    let merged = try #require(await updated.next())
    #expect(merged.boxKey == v.str("keys", "hostBoxPub"))
    #expect(merged.urls == ["http://192.0.2.2:19222"])
    #expect(merged.cloud?.absoluteString == "https://cloud.example.dev")
    #expect(merged.device == "macstudio" && merged.name == "Studio")

    // Two calls in flight, answered out of order.
    async let usage = t.call("usage", body: Data(#"{"refresh":false}"#.utf8), timeout: 10)
    async let sync = t.call("sync", body: Data(#"{"since":7}"#.utf8), timeout: 10)
    let r1 = try await host.receive()
    let r2 = try await host.receive()
    #expect(r1["id"] as? Int != r2["id"] as? Int)
    for r in [r2, r1] { try await host.send(["id": r["id"] ?? 0, "ok": ["method": r["m"] ?? ""]]) }
    #expect(object(String(decoding: try await usage, as: UTF8.self))["method"] as? String == "usage")
    #expect(object(String(decoding: try await sync, as: UTF8.self))["method"] as? String == "sync")
    let syncBody = [r1, r2].first { $0["m"] as? String == "sync" }?["b"] as? [String: Any]
    #expect(syncBody?["since"] as? Int == 7)

    // Errors keep their HTTP status.
    let denied = Task { try await t.call("setScreenEnabled", body: Data(#"{"enabled":true}"#.utf8), timeout: 10) }
    let r3 = try await host.receive()
    try await host.send(["id": r3["id"] ?? 0, "err": ["status": 403, "message": "Only on the computer."]])
    await #expect(throws: HostError.http(403, "O computador recusou este pedido.")) { _ = try await denied.value }

    // A stream yields events until its consumer stops; the host is then told to cancel.
    let reader = Task {
        var got: [Data] = []
        for try await d in t.stream(.events(since: 5, client: "ios")) {
            got.append(d)
            if got.count == 2 { break }
        }
        return got
    }
    let sub = try await host.receive()
    #expect(sub["sub"] as? String == "events")
    #expect((sub["b"] as? [String: Any])?["since"] as? Int == 5)
    #expect((sub["b"] as? [String: Any])?["client"] as? String == "ios")
    try await host.send(["id": sub["id"] ?? 0, "ev": ["type": "hello", "hostId": "h1", "rev": 9]])
    try await host.send(["id": sub["id"] ?? 0, "ev": ["type": "resync"]])
    let got = try await reader.value
    #expect(got.count == 2 && object(String(decoding: got[1], as: UTF8.self))["type"] as? String == "resync")
    let cancel = try await host.receive()
    #expect(cancel["id"] as? Int == sub["id"] as? Int && cancel["cancel"] as? Bool == true)

    // Revoked: no retry.
    await socket.toDevice.close(4003)
    guard case .unauthorized = await waitFor(t.states(), { if case .unauthorized = $0 { true } else { false } }) else {
        Issue.record("expected unauthorized"); return
    }
    await #expect(throws: HostError.self) { _ = try await t.call("hello", body: Data(), timeout: 1) }
    await t.shutdown()
}

@Test(.timeLimit(.minutes(1))) func changedHostIdentityIsNotRetried() async throws {
    let (dial, sockets) = fakeDialer()
    var computer = v.computer
    computer.urls = ["http://127.0.0.1:1"]
    let t = ChannelTransport(computer: computer, identity: v.identity, pairingCode: nil, dial: dial, watchesNetwork: false)
    await t.start()
    var it = sockets.makeAsyncIterator()
    var host = HostSide(try #require(await it.next()))
    try await host.accept(hostSigner: Curve25519.Signing.PrivateKey())
    #expect(await t.settled(within: .seconds(5)) == .unauthorized(ChannelTransport.identityChanged))
    await t.shutdown()
}

@Test(.timeLimit(.minutes(1))) func directRejectFallsBackToTheRelay() async throws {
    let (dial, sockets) = fakeDialer()
    var computer = v.computer
    computer.urls = ["http://192.0.2.20:19222"]
    computer.cloud = URL(string: "https://cloud.example.dev")
    let t = ChannelTransport(computer: computer, identity: v.identity, pairingCode: nil, dial: dial, watchesNetwork: false)
    await t.start()
    let screenRoute = Task { try await t.screenRelayRequired() }
    var it = sockets.makeAsyncIterator()
    // Some other host now answers at the saved LAN address; its reject isn't signed.
    let stranger = HostSide(try #require(await it.next()))
    _ = try await stranger.raw()
    await stranger.push(["t": "reject", "code": "unauthorized"])
    let socket = try #require(await it.next())
    guard case .relay = socket.endpoint else { Issue.record("expected the relay"); return }
    var relay = HostSide(socket)
    await relay.push(["t": "presence", "online": true, "lastSeenAt": NSNull()])
    try await relay.accept()
    #expect(await t.settled(within: .seconds(5)) == .ready(.relay))
    #expect(try await screenRoute.value)
    #expect(await !t.isStopped)
    await t.shutdown()
}

@Test(.timeLimit(.minutes(1))) func routeOrderFollowsTheComputer() async throws {
    var computer = v.computer
    computer.urls = ["http://192.0.2.20:19222"]
    computer.cloud = URL(string: "https://cloud.example.dev")

    computer.route = .cloudflareFirst
    let (dial, sockets) = fakeDialer()
    let first = ChannelTransport(computer: computer, identity: v.identity, pairingCode: nil, dial: dial, watchesNetwork: false)
    await first.start()
    var it = sockets.makeAsyncIterator()
    let socket = try #require(await it.next())
    guard case .relay = socket.endpoint else { Issue.record("Cloudflare first dials the relay first"); return }
    await first.shutdown()

    computer.route = .directOnly
    let (dial2, sockets2) = fakeDialer()
    let second = ChannelTransport(computer: computer, identity: v.identity, pairingCode: nil, dial: dial2, watchesNetwork: false)
    await second.start()
    var it2 = sockets2.makeAsyncIterator()
    let stranger = HostSide(try #require(await it2.next()))
    _ = try await stranger.raw()
    await stranger.push(["t": "reject", "code": "unauthorized"])
    let retry = try #require(await it2.next())
    if case .relay = retry.endpoint { Issue.record("direct only never dials the relay") }
    await second.shutdown()
}

@Test(.timeLimit(.minutes(1))) func relayPresenceAndMailbox() async throws {
    let (dial, sockets) = fakeDialer()
    var computer = v.computer
    computer.urls = []
    computer.cloud = URL(string: "https://cloud.example.dev")
    let t = ChannelTransport(computer: computer, identity: v.identity, pairingCode: nil, dial: dial, watchesNetwork: false)
    let mailbox = t.mailboxEvents()
    await t.start()
    var it = sockets.makeAsyncIterator()
    let socket = try #require(await it.next())
    guard case let .relay(request) = socket.endpoint else { Issue.record("not relay"); return }
    #expect(request.url?.absoluteString == "wss://cloud.example.dev/v1/relay/device/\(computer.id)?v=1")
    let sig = request.value(forHTTPHeaderField: "Codync-Sig") ?? ""
    #expect(sig.hasPrefix("v=1,kid=\(v.identity.publicKey),ts="))
    var relay = HostSide(socket)

    await relay.push(["t": "presence", "online": false, "lastSeenAt": 1_790_000_000_000])
    let seen = Date(milliseconds: 1_790_000_000_000)
    #expect(await t.settled(within: .seconds(5)) == .hostOffline(lastSeen: seen))
    await #expect(throws: HostError.computerOffline(lastSeen: seen)) { _ = try await t.call("hello", body: Data(), timeout: 1) }

    // Queue a message: it goes out sealed for the host's box key.
    let nonce = "3F2504E0-4F89-11D3-9A0C-0305E82C3301"
    async let queued: Void = t.enqueue(botId: "b1", text: "hi", clientNonce: nonce, sessionId: "s1")
    let put = try await relay.raw()
    #expect(put["t"] as? String == "mbox.put" && put["nonce"] as? String == nonce)
    let blob = try #require(Data(base64URL: put["d"] as? String ?? ""))
    let epk = blob.prefix(32), mboxSig = blob.dropFirst(32).prefix(64), ct = blob.dropFirst(96)
    let cid = RelayCrypto.computerIdRaw(signKey: v.b64("keys", "hostSignPub"))
    #expect(v.deviceSign.publicKey.isValidSignature(mboxSig, for: RelayCrypto.label("codync/mbox/v1") + cid + epk + ct))
    let key = RelayCrypto.mailboxKey(sharedSecret: try RelayCrypto.sharedSecret(v.hostBox, Data(epk)), cid: cid,
                                     dk: v.b64("keys", "deviceSignPub"), epk: Data(epk))
    let box = try ChaChaPoly.SealedBox(nonce: ChaChaPoly.Nonce(data: Data(count: 12)), ciphertext: ct.dropLast(16), tag: ct.suffix(16))
    let inner = object(String(decoding: try ChaChaPoly.open(box, using: key, authenticating: v.b64("keys", "deviceSignPub") + Data(nonce.utf8)), as: UTF8.self))
    #expect(inner["m"] as? String == "send")
    #expect((inner["b"] as? [String: Any])?["clientNonce"] as? String == nonce)
    #expect((inner["b"] as? [String: Any])?["text"] as? String == "hi")
    #expect((inner["b"] as? [String: Any])?["sessionId"] as? String == "s1")
    await relay.push(["t": "mbox.ok", "nonce": nonce, "state": "queued"])
    try await queued

    // The relay refusing says why.
    let refused = Task { try await t.enqueue(botId: "b1", text: "again", clientNonce: "n2") }
    _ = try await relay.raw()
    await relay.push(["t": "mbox.err", "nonce": "n2", "code": "full"])
    await #expect(throws: MailboxError.rejected("full")) { try await refused.value }

    async let listed = t.listQueued()
    #expect(try await relay.raw()["t"] as? String == "mbox.list")
    await relay.push(["t": "mbox.items", "items": [["nonce": nonce, "exp": 1, "state": "queued"]]])
    #expect(try await listed.map(\.nonce) == [nonce])

    async let cancelled = t.cancelQueued(clientNonce: nonce)
    #expect(try await relay.raw()["t"] as? String == "mbox.cancel")
    await relay.push(["t": "mbox.cancelled", "nonce": nonce])
    #expect(await cancelled == .cancelled)

    async let gone = t.cancelQueued(clientNonce: "n3")
    _ = try await relay.raw()
    await relay.push(["t": "mbox.gone", "nonce": "n3", "state": "delivering"])
    #expect(await gone == .delivering)

    await relay.push(["t": "mbox.expired", "nonce": "n4"])
    var events = mailbox.makeAsyncIterator()
    #expect(await events.next() == .expired(nonce: "n4"))

    // The host comes online: a fresh handshake through the relay.
    await relay.push(["t": "presence", "online": true, "lastSeenAt": nil as Int? as Any])
    try await relay.accept()
    #expect(await waitFor(t.states()) { $0 == .ready(.relay) } == .ready(.relay))
    await #expect(throws: MailboxError.self) {
        async let online: Void = t.enqueue(botId: "b1", text: "x", clientNonce: "n5")
        _ = try await relay.raw()
        await relay.push(["t": "mbox.err", "nonce": "n5", "code": "hostOnline"])
        try await online
    }
    await t.shutdown()
}

@Test(.timeLimit(.minutes(1))) func pairingSendsOnlyPairThenStopsOn4100() async throws {
    let (dial, sockets) = fakeDialer()
    var computer = v.computer
    computer.urls = ["http://127.0.0.1:1"]
    let code = v.str("pairing", "code")
    let t = ChannelTransport(computer: computer, identity: v.identity, pairingCode: code, dial: dial, watchesNetwork: false)
    await t.start()
    var it = sockets.makeAsyncIterator()
    let socket = try #require(await it.next())
    var host = HostSide(socket)
    let h = try await host.accept()
    #expect(h["pair"] as? Bool == true)
    async let paired = t.call("pair", body: Data(#"{"code":"\#(code)","name":"iPhone","platform":"ios"}"#.utf8), timeout: 10)
    let first = try await host.receive()
    #expect(first["m"] as? String == "pair")
    #expect((first["b"] as? [String: Any])?["code"] as? String == code)
    try await host.send(["id": first["id"] ?? 0, "ok": ["computerId": computer.id, "name": "Studio"]])
    #expect(object(String(decoding: try await paired, as: UTF8.self))["name"] as? String == "Studio")
    await socket.toDevice.close(4100)
    #expect(await waitFor(t.states()) { if case .failed = $0 { true } else { false } } == .failed("Paired."))
    await t.shutdown()
}

@Test func lapsedLeaseIsRetriedNotARevocation() throws {
    let reject = { (code: String) in ChannelTransport.rejection(try ChannelTransport.decodeWire(#"{"t":"reject","code":"\#(code)"}"#)) }
    // The computer couldn't confirm the account's approval lately: the channel keeps trying.
    guard case .wait(.failed) = try reject("leaseExpired") else { Issue.record("expected a retrying failure"); return }
    guard case .stop(.unauthorized) = try reject("revoked") else { Issue.record("expected a revocation"); return }
}

@Test(.timeLimit(.minutes(1))) func relayOnlyNeverFallsBackToAdvertisedLAN() async throws {
    var computer = v.computer
    computer.urls = ["http://192.168.1.20:19222"] // vazamento-ok: teste-rede-local
    computer.cloud = URL(string: "https://cloud.example.dev")
    computer.route = .relayOnly
    let computerID = computer.id
    let attempts = Pipe()
    let transport = ChannelTransport(computer: computer, identity: v.identity, pairingCode: nil, dial: { endpoint in
        switch endpoint {
        case .relay(let request):
            #expect(request.url?.absoluteString == "wss://cloud.example.dev/v1/relay/device/\(computerID)?v=1")
            #expect(request.value(forHTTPHeaderField: "Codync-Sig")?.hasPrefix("v=1,kid=") == true)
            await attempts.push("relay")
        case .direct:
            Issue.record("Account route dialed LAN after relay failure")
            await attempts.push("direct")
        }
        throw HostError.unreachable
    }, watchesNetwork: false)
    await transport.start()
    #expect(try await attempts.pop() == "relay")
    #expect(try await attempts.pop() == "relay")
    await transport.shutdown()
}

@Test func relayOnlyKeepsConfiguredCloudWhenHelloOmitsIt() async throws {
    var computer = v.computer
    computer.route = .relayOnly
    computer.cloud = URL(string: "https://cloud.example.dev")
    let transport = ChannelTransport(computer: computer, identity: v.identity, pairingCode: nil, watchesNetwork: false)
    var body = helloBody()
    body.removeValue(forKey: "cloud")
    let hello = try JSONDecoder().decode(Hello.self, from: JSONSerialization.data(withJSONObject: body))
    await transport.merge(hello)
    #expect(await transport.computer.cloud == computer.cloud)
    #expect(await transport.computer.route == .relayOnly)
    await transport.shutdown()
}

@Test(.timeLimit(.minutes(1))) func plaintextRejectNeverShowsRemoteMessage() async throws {
    let (dial, sockets) = fakeDialer()
    var computer = v.computer
    computer.urls = ["http://192.0.2.20:19222"]
    computer.cloud = nil
    let t = ChannelTransport(computer: computer, identity: v.identity, pairingCode: nil, dial: dial, watchesNetwork: false)
    await t.start()
    var it = sockets.makeAsyncIterator()
    let stranger = HostSide(try #require(await it.next()))
    _ = try await stranger.raw()
    let hostile = String(repeating: "mensagem remota", count: 50_000)
    await stranger.push(["t": "reject", "code": "unauthorized", "message": hostile])
    let state = await t.settled(within: .seconds(5))
    #expect(state == .unauthorized("Este iPhone não tem mais acesso a este computador."))
    await t.shutdown()
}

@Test func rejectReasonsUseOnlyLocalText() throws {
    let hostile = String(repeating: "x", count: 1_000_000)
    for (code, expected) in [("revoked", "Este iPhone não tem mais acesso a este computador."),
                             ("leaseExpired", "Não foi possível confirmar o acesso a este computador. Tentando novamente."),
                             ("unsupportedVersion", "Versão do app incompatível com o computador. Atualize o app."),
                             ("pairingClosed", "Este código de pareamento expirou. Gere outro no computador.")] {
        let data = try JSONSerialization.data(withJSONObject: ["t": "reject", "code": code, "message": hostile])
        let wire = try ChannelTransport.decodeWire(String(decoding: data, as: UTF8.self))
        let outcome = try #require(ChannelTransport.rejection(wire))
        switch outcome {
        case let .stop(.unauthorized(message)), let .stop(.failed(message)), let .wait(.failed(message)):
            #expect(message == expected)
            #expect(!message.contains(hostile))
        default: Issue.record("unexpected rejection state")
        }
    }
}

/// A call bound to a channel goes out on that channel or fails at once; it never waits for the
/// reconnect and never moves to the newer channel.
@Test(.timeLimit(.minutes(1))) func boundCallNeverMovesToANewerChannel() async throws {
    let (dial, sockets) = fakeDialer()
    var computer = v.computer
    computer.boxKey = nil
    computer.urls = ["http://127.0.0.1:1"]
    let t = ChannelTransport(computer: computer, identity: v.identity, pairingCode: nil, dial: dial, watchesNetwork: false)
    let states = t.states()
    await t.start()
    var socketIterator = sockets.makeAsyncIterator()
    var host = HostSide(try #require(await socketIterator.next()))
    try await host.accept()
    #expect(await waitFor(states) { $0 == .ready(.direct) } == .ready(.direct))
    let hello = try await host.receive()
    try await host.send(["id": hello["id"] ?? 0, "ok": helloBody()])
    let first = try #require(await t.currentLink())

    // The link is replaced: a call bound to the old channel fails instead of waiting for the new one.
    await t.reconnect()
    await #expect(throws: HostError.unreachable) {
        _ = try await t.call("stop", body: Data(), timeout: 10, onLink: first)
    }

    var host2 = HostSide(try #require(await socketIterator.next()))
    try await host2.accept()
    let hello2 = try await host2.receive()
    #expect(hello2["m"] as? String == "hello")
    try await host2.send(["id": hello2["id"] ?? 0, "ok": helloBody()])
    var second: Int?
    for _ in 0..<200 {
        if let link = await t.currentLink(), link != first {
            second = link
            break
        }
        try await Task.sleep(for: .milliseconds(10))
    }
    let current = try #require(second)
    // Still refused on the new channel, and nothing of it reaches the host there.
    await #expect(throws: HostError.unreachable) {
        _ = try await t.call("stop", body: Data(), timeout: 10, onLink: first)
    }
    async let usage = t.call("usage", body: Data(), timeout: 10, onLink: current)
    let next = try await host2.receive()
    #expect(next["m"] as? String == "usage")
    try await host2.send(["id": next["id"] ?? 0, "ok": ["fine": true]])
    _ = try await usage
    await t.shutdown()
}

/// A socket whose upgrade was answered with an HTTP status instead of 101.
private final class RefusedSocket: ChannelSocket {
    let status: Int
    init(_ status: Int) { self.status = status }
    func send(_ text: String) async throws { throw SocketRefused(status: status) }
    func receive() async throws -> String { throw SocketRefused(status: status) }
    func ping() async throws {}
    func close(code: Int) {}
}

/// Hands out scripted sockets in order and counts the dials.
private actor Dials {
    private var queue: [any ChannelSocket]
    private(set) var count = 0
    init(_ sockets: [any ChannelSocket]) { queue = sockets }

    func next() -> any ChannelSocket {
        count += 1
        return queue.isEmpty ? RefusedSocket(503) : queue.removeFirst()
    }
}

private func relayTransport(_ dials: Dials, recemVinculado: Bool = false) -> ChannelTransport {
    var computer = v.computer
    computer.boxKey = nil
    computer.urls = []
    computer.cloud = URL(string: "https://cloud.example.dev")
    computer.route = .relayOnly
    return ChannelTransport(computer: computer, identity: v.identity, pairingCode: nil,
                            dial: { _ in await dials.next() }, watchesNetwork: false, recemVinculado: recemVinculado)
}

/// A new handshake on the same relay socket is a new channel; a call bound to the
/// previous one never goes out on it.
@Test(.timeLimit(.minutes(1))) func newHandshakeOnTheSameRelaySocketIsANewChannel() async throws {
    let socket = FakeSocket(.relay(URLRequest(url: URL(string: "wss://cloud.example.dev")!)))
    let dials = Dials([socket])
    let t = relayTransport(dials)
    let states = t.states()
    await t.start()
    var host = HostSide(socket)
    await host.push(["t": "presence", "online": true, "lastSeenAt": NSNull()])
    try await host.accept()
    #expect(await waitFor(states) { $0 == .ready(.relay) } == .ready(.relay))
    let hello = try await host.receive()
    try await host.send(["id": hello["id"] ?? 0, "ok": helloBody()])
    let first = try #require(await t.currentLink())

    // The host rejoins the relay: the device runs a new handshake without a new socket.
    await host.push(["t": "presence", "online": true, "lastSeenAt": NSNull()])
    try await host.accept()
    var second: Int?
    for _ in 0..<200 {
        if let link = await t.currentLink(), link != first {
            second = link
            break
        }
        try await Task.sleep(for: .milliseconds(10))
    }
    let current = try #require(second)
    await #expect(throws: HostError.unreachable) {
        _ = try await t.call("upload", body: Data(), timeout: 10, onLink: first)
    }
    let hello2 = try await host.receive()
    #expect(hello2["m"] as? String == "hello")
    try await host.send(["id": hello2["id"] ?? 0, "ok": helloBody()])
    async let usage = t.call("usage", body: Data(), timeout: 10, onLink: current)
    let next = try await host.receive()
    #expect(next["m"] as? String == "usage")
    try await host.send(["id": next["id"] ?? 0, "ok": ["fine": true]])
    _ = try await usage
    #expect(await dials.count == 1)
    await t.shutdown()
}

/// A 401 answering the socket upgrade ends access at once.
@Test(.timeLimit(.minutes(1))) func unauthorizedUpgradeStopsAtOnce() async throws {
    let dials = Dials([RefusedSocket(401)])
    let t = relayTransport(dials)
    await t.start()
    guard case .unauthorized = await t.settled(within: .seconds(10)) else {
        Issue.record("expected unauthorized")
        await t.shutdown()
        return
    }
    #expect(await dials.count == 1)
    await t.shutdown()
}

/// Once the device had a channel, a 403 on the next upgrade ends access at once.
@Test(.timeLimit(.minutes(1))) func forbiddenUpgradeAfterAChannelStopsAtOnce() async throws {
    let socket = FakeSocket(.relay(URLRequest(url: URL(string: "wss://cloud.example.dev")!)))
    let dials = Dials([socket, RefusedSocket(403)])
    let t = relayTransport(dials)
    let states = t.states()
    await t.start()
    var host = HostSide(socket)
    await host.push(["t": "presence", "online": true, "lastSeenAt": NSNull()])
    try await host.accept()
    #expect(await waitFor(states) { $0 == .ready(.relay) } == .ready(.relay))

    await t.reconnect()
    let next = await waitFor(states) { state in
        if case .unauthorized = state { return true }
        if case .failed = state { return true }
        return false
    }
    guard case .unauthorized = next else {
        Issue.record("expected unauthorized, got \(String(describing: next))")
        await t.shutdown()
        return
    }
    #expect(await dials.count == 2)
    await t.shutdown()
}

/// A saved device's transport (recreated on foreground, say) ends on the first 403.
@Test(.timeLimit(.minutes(1))) func savedDeviceStopsOnTheFirst403() async throws {
    let dials = Dials([RefusedSocket(403)])
    let t = relayTransport(dials)
    await t.start()
    guard case .unauthorized = await t.settled(within: .seconds(10)) else {
        Issue.record("expected unauthorized")
        await t.shutdown()
        return
    }
    #expect(await dials.count == 1)
    await t.shutdown()
}

/// Just paired or approved, early 403s are retried, but only until the first channel.
@Test(.timeLimit(.minutes(1))) func justLinkedDeviceRetriesEarly403sUntilItsFirstChannel() async throws {
    let socket = FakeSocket(.relay(URLRequest(url: URL(string: "wss://cloud.example.dev")!)))
    let dials = Dials([RefusedSocket(403), socket, RefusedSocket(403)])
    let t = relayTransport(dials, recemVinculado: true)
    let states = t.states()
    await t.start()
    let early = await waitFor(states) { state in
        if case .unauthorized = state { return true }
        if case .failed = state { return true }
        return false
    }
    guard case .failed = early else {
        Issue.record("expected a retry, got \(String(describing: early))")
        await t.shutdown()
        return
    }
    // Skip the backoff: the next attempt reaches the computer.
    await t.reconnect()
    var host = HostSide(socket)
    await host.push(["t": "presence", "online": true, "lastSeenAt": NSNull()])
    try await host.accept()
    #expect(await waitFor(states) { $0 == .ready(.relay) } == .ready(.relay))
    #expect(await dials.count == 2)

    // It had a channel: the tolerance is over and the next 403 ends access at once.
    await t.reconnect()
    let next = await waitFor(states) { state in
        if case .unauthorized = state { return true }
        if case .failed = state { return true }
        return false
    }
    guard case .unauthorized = next else {
        Issue.record("expected unauthorized, got \(String(describing: next))")
        await t.shutdown()
        return
    }
    #expect(await dials.count == 3)
    await t.shutdown()
}

@Test func innerHTTPMessagesAreFixedByStatus() {
    #expect(ChannelTransport.httpMessage(401) == "Este iPhone não tem mais acesso a este computador.")
    #expect(HostError.http(401, "texto remoto").localizedDescription == "Este iPhone não tem mais acesso a este computador.")
    #expect(ChannelTransport.httpMessage(403) == "O computador recusou este pedido.")
    #expect(ChannelTransport.httpMessage(500) == "Não foi possível concluir. Tente de novo.")
}

@Test func wireIgnoresUnneededMessage() throws {
    let wire = try ChannelTransport.decodeWire(#"{"t":"reject","code":"revoked","message":{"unexpected":true}}"#)
    #expect(wire.code == "revoked")
}

@Test(.timeLimit(.minutes(1))) func directPairingUnsupportedVersionRequiresUpgrade() async throws {
    let (dial, sockets) = fakeDialer()
    var computer = v.computer
    computer.urls = ["http://127.0.0.1:1"]
    computer.cloud = nil
    let t = ChannelTransport(computer: computer, identity: v.identity, pairingCode: v.str("pairing", "code"), dial: dial, watchesNetwork: false)
    await t.start()
    var it = sockets.makeAsyncIterator()
    let host = HostSide(try #require(await it.next()))
    _ = try await host.raw()
    await host.push(["t": "reject", "code": "unsupportedVersion"])
    #expect(await waitFor(t.states()) { if case .failed = $0 { true } else { false } } == .failed("Versão do app incompatível com o computador. Atualize o app."))
    #expect(await t.needsUpgrade)
    await t.shutdown()
}
