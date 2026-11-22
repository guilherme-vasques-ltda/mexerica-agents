// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CryptoKit
import Foundation
import os
import Testing
@testable import CodyncKit
@testable import CodyncUI

private let hostKey = Data(repeating: 7, count: 32)

/// Each arrival means the previous response has been fully processed on the main actor.
/// Buffered signals avoid timing assumptions and never depend on a wall-clock delay.
@MainActor private final class ApprovalClock {
    let arrivals = AsyncStream<Void>.makeStream()
    private let ticks = AsyncStream<Void>.makeStream()

    func wait() async throws {
        arrivals.continuation.yield(())
        var iterator = ticks.stream.makeAsyncIterator()
        guard await iterator.next() != nil else { throw CancellationError() }
    }

    func advance() { ticks.continuation.yield(()) }
    func finish() { ticks.continuation.finish(); arrivals.continuation.finish() }
}

/// One access request's cloud calls: the account lists the computer, the request is created,
/// the host's nonce is already there, the reveal is accepted; approval never comes.
private final class AccessCloudStub: URLProtocol, @unchecked Sendable {
    /// Access requests committed so far.
    static let commits = OSAllocatedUnfairLock(initialState: 0)
    static let started = AsyncStream<Void>.makeStream()
    static let release = AsyncStream<Void>.makeStream()

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}

    override func startLoading() {
        let path = request.url?.path ?? ""
        if path.hasSuffix("/access-requests") {
            Self.commits.withLock { $0 += 1 }
            // URLProtocol isn't Sendable; the stub only answers once, after the release signal.
            nonisolated(unsafe) let stub = self
            Task {
                Self.started.continuation.yield(())
                var iterator = Self.release.stream.makeAsyncIterator()
                _ = await iterator.next()
                stub.respond()
            }
        } else { respond() }
    }

    private func respond() {
        let path = request.url?.path ?? ""
        let json = if path.hasSuffix("/v1/computers") {
            #"{"computers":[{"computerId":"\#(RelayCrypto.computerId(signKey: hostKey))","name":"Desk","signKey":"\#(hostKey.base64URL)","online":true}]}"#
        } else if path.hasSuffix("/access-requests") {
            #"{"requestId":"r1","expiresAt":4102444800000}"#
        } else if path.hasSuffix("/v1/access-requests/r1") {
            #"{"requestId":"r1","computerId":"\#(RelayCrypto.computerId(signKey: hostKey))","status":"pending","hostNonce":"\#(Data(repeating: 9, count: 32).base64URL)"}"#
        } else {
            "{}"
        }
        let response = HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(json.utf8))
        client?.urlProtocolDidFinishLoading(self)
    }
}

/// Signed in with an online computer this device can't use, nothing is asked until the user taps.
/// Closing the access sheet before the code arrives and opening it again makes one request, which
/// goes on in the store and ends with the code in the banner, not pending on the computer unrevealed.
@MainActor @Test(.timeLimit(.minutes(1))) func accessIsAskedOnlyOnTapAndOutlivesItsSheet() async throws {
    AccessCloudStub.commits.withLock { $0 = 0 }
    let host = "access-\(UUID().uuidString.lowercased()).test"
    let suite = "AccessRequest.\(UUID().uuidString)"
    defer {
        UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite)
        AccessCloudStub.commits.withLock { $0 = 0 }
    }
    let config = URLSessionConfiguration.ephemeral
    config.protocolClasses = [AccessCloudStub.self]
    let identity = DeviceIdentity(signing: .init(), push: .init())
    let cloud = CloudClient(baseURL: URL(string: "https://\(host)")!, identity: identity,
                            session: URLSession(configuration: config)) { "token" }
    let storage = SharedStore.Context(accountID: "account", suite: suite)
    let accounts = AccountStore(storage: storage, cloud: cloud)
    let clock = ApprovalClock()
    accounts.waitForApprovalPoll = { try await clock.wait() }
    accounts.nowMs = { 1_000 }
    var arrivals = clock.arrivals.stream.makeAsyncIterator()
    defer { accounts.retire(); clock.finish() }
    await accounts.refreshCloud()
    let id = RelayCrypto.computerId(signKey: hostKey)
    #expect(accounts.cloudComputers.map(\.computerId) == [id])
    #expect(accounts.pendingAccess.isEmpty)
    #expect(AccessCloudStub.commits.withLock { $0 } == 0)

    let sheet = Task { try await accounts.requestAccess(id) }
    var starts = AccessCloudStub.started.stream.makeAsyncIterator()
    _ = await starts.next() // The first request exists, but cannot have returned its code.
    sheet.cancel()
    let reopened = Task {
        AccessCloudStub.release.continuation.yield(())
        // Join on the main actor before the first request can finish there.
        try await accounts.requestAccess(id)
    }

    _ = await arrivals.next()
    #expect(accounts.pendingAccess[id]?.requestId == "r1")
    #expect(accounts.pendingAccess[id]?.code.count == 6)
    try await reopened.value
    try await sheet.value
    #expect(AccessCloudStub.commits.withLock { $0 } == 1)
}

/// The cloud answers "pending" until the device has revealed its nonce (POST /reveal) and the test
/// lets the Mac approve; only then does it answer "approved". Both steps are explicit, no timers.
/// State is kept per host name (unique per test, removed when it ends), so tests can run in parallel
/// without sharing it.
private final class ApprovedCloudStub: URLProtocol {
    struct Phase {
        var revealed = false
        var macApproves = false
        /// The store's clock in these tests (epoch ms).
        var now: Int64 = 1_000
        /// Listing the computers moves the clock here: past the ticket's expiry, or past only the approval's.
        var nowWhenListed: Int64?
        /// When the cloud says the approval stops being valid (the ticket's own expiry is 4102444800000).
        var approvalExpiresAt: Int64 = 4_102_444_800_000
        /// Byte the host nonce in the status answer is made of; the ticket was created with 9.
        var statusNonce: UInt8 = 9
        /// Sign key the account list gives for the computer; nil means the real one.
        var listedSignKey: String?
        /// The id the cloud gives the new request.
        var requestId = "req_ok"
        /// A confirmation code the cloud slips into its "approved" answer: the iPhone must ignore it.
        var networkConfirmCode: String?
        /// DELETE /v1/access-requests/{id} calls (withdrawals).
        var deletes = 0
        /// Paths of DELETE /v1/computers/{id}/grants/{grantId} calls (owner revocations).
        var grantDeletes: [String] = []
        /// What the grant revocation answers.
        var grantRevokeStatus = 200
        /// What creating a request answers: 409 while this device still holds an active grant.
        var createStatus = 200
        /// Creates and revocations, in order.
        var calls: [String] = []
        /// Listing the computers is held until the test releases it (`ConfirmHarness.releaseList`).
        var holdList = false
        /// The account list names the computer twice.
        var listDuplicated = false
        /// What listing the computers answers.
        var listStatus = 200
        /// The grant revocation's answer is held until the test releases it (`ConfirmHarness.releaseRevoke`).
        var holdRevoke = false
        /// A revocation was received and its answer is still held.
        var revokeOutstanding = false
        /// A request was created while a revocation's answer was still held.
        var createdWhileRevokeHeld = false
        /// The "approved" answer moves the clock here (it arrives late).
        var nowWhenApproved: Int64?
        /// The state "req_ok" reports instead of pending/approved (e.g. "denied").
        var statusOverride: String?
        /// Reading a request fails (a network error as far as the iPhone can tell).
        var statusFails = false
        /// GET /v1/access-requests/{id} calls.
        var statusGets = 0
        /// Authorization of each request read, in order.
        var statusTokens: [String] = []
        /// Authorization of each grant revocation, in order.
        var revokeTokens: [String] = []
        /// Reading a request is held until the test releases it (`ConfirmHarness.releaseStatus`).
        var holdStatus = false
        /// Creating a request answers 409 while the Mac's approval left an active grant (not revoked).
        var conflictOnActiveGrant = false
    }
    /// Held answers, per host and kind ("host#list", "host#revoke"): calling one sends it.
    static let heldLists = OSAllocatedUnfairLock<[String: @Sendable () -> Void]>(initialState: [:])
    /// Signalled once an answer is held, with the same keys.
    static let listHeld = OSAllocatedUnfairLock<[String: AsyncStream<Void>.Continuation]>(initialState: [:])
    static let phases = OSAllocatedUnfairLock(initialState: [String: Phase]())
    static func phase(_ host: String) -> Phase { phases.withLock { $0[host] ?? Phase() } }
    static func remove(_ host: String) { phases.withLock { _ = $0.removeValue(forKey: host) } }
    static func update(_ host: String, _ change: (inout Phase) -> Void) {
        phases.withLockUnchecked { var phase = $0[host] ?? Phase(); change(&phase); $0[host] = phase }
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}
    override func startLoading() {
        let id = RelayCrypto.computerId(signKey: hostKey)
        let path = request.url!.path
        let host = request.url?.host() ?? ""
        let json: String
        var status = 200
        let auth = request.value(forHTTPHeaderField: "Authorization") ?? ""
        if request.httpMethod == "DELETE", path.contains("/grants/") {
            Self.update(host) { $0.grantDeletes.append(path); $0.calls.append("revoke"); $0.revokeTokens.append(auth) }
            status = Self.phase(host).grantRevokeStatus
            json = status == 200 ? #"{"revocations":[]}"# : #"{"error":{"code":"internal","message":"remote text"}}"#
            if Self.phase(host).holdRevoke {
                Self.update(host) { $0.revokeOutstanding = true }
                hold("\(host)#revoke", json, status: status)
                return
            }
        } else if request.httpMethod == "DELETE" {
            Self.update(host) { $0.deletes += 1 }
            json = "{}"
        } else if path.hasSuffix("/reveal") {
            Self.update(host) { $0.revealed = true }
            json = "{}"
        } else if path.hasSuffix("/v1/computers") {
            Self.update(host) { if let moved = $0.nowWhenListed { $0.now = moved } }
            let phase = Self.phase(host)
            let listed = phase.listedSignKey ?? hostKey.base64URL
            let entry = #"{"computerId":"\#(id)","name":"Mac","signKey":"\#(listed)","access":"granted"}"#
            let items = phase.listDuplicated ? entry + "," + entry : entry
            status = phase.listStatus
            json = status == 200
                ? #"{"computers":[\#(items)],"personalComputers":[],"revocations":[]}"#
                : #"{"error":{"code":"internal","message":"remote text"}}"#
            if phase.holdList {
                hold("\(host)#list", json, status: status)
                return
            }
        } else if path.hasSuffix("/access-requests") {
            Self.update(host) {
                $0.calls.append("create")
                if $0.revokeOutstanding { $0.createdWhileRevokeHeld = true }
            }
            let created = Self.phase(host)
            let granted = created.conflictOnActiveGrant && created.macApproves && created.grantDeletes.isEmpty
            status = granted ? 409 : created.createStatus
            json = status == 200 ? #"{"requestId":"\#(Self.phase(host).requestId)","expiresAt":4102444800000}"#
                : #"{"error":{"code":"conflict","message":"This device already has access"}}"#
        } else if path.hasPrefix("/v1/access-requests/") {
            // Only "req_ok" is ever approved; any later request (another id) stays pending.
            let requested = path.split(separator: "/").last.map(String.init) ?? ""
            Self.update(host) { $0.statusGets += 1; $0.statusTokens.append(auth) }
            let early = Self.phase(host)
            if requested == "req_ok", early.revealed, early.macApproves, let late = early.nowWhenApproved { Self.update(host) { $0.now = late } }
            let phase = Self.phase(host)
            let approved = requested == "req_ok" && phase.revealed && phase.macApproves
            let state = requested == "req_ok" ? (phase.statusOverride ?? (approved ? "approved" : "pending")) : "pending"
            let nonce = Data(repeating: phase.statusNonce, count: 32).base64URL
            let slipped = phase.networkConfirmCode.map { #","confirmCode":"\#($0)""# } ?? ""
            status = phase.statusFails ? 500 : 200
            json = status == 200
                ? #"{"requestId":"\#(requested)","computerId":"\#(id)","status":"\#(state)","hostNonce":"\#(nonce)","expiresAt":\#(phase.approvalExpiresAt),"grantId":"grant_fixture"\#(slipped)}"#
                : #"{"error":{"code":"internal","message":"remote text"}}"#
            if phase.holdStatus {
                hold("\(host)#status", json, status: status)
                return
            }
        } else { json = "{}" }
        reply(json, status: status)
    }

    /// Keeps this answer until the test releases it, and signals that it is held.
    private func hold(_ key: String, _ json: String, status: Int) {
        // URLProtocol isn't Sendable; the held answer is sent once, when the test releases it.
        nonisolated(unsafe) let stub = self
        Self.heldLists.withLock { $0[key] = { stub.reply(json, status: status) } }
        _ = Self.listHeld.withLock { $0[key] }?.yield(())
    }

    private func reply(_ json: String, status: Int) {
        client?.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil, headerFields: nil)!, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(json.utf8))
        client?.urlProtocolDidFinishLoading(self)
    }
}

/// One access request against `ApprovedCloudStub`, driven poll by poll, up to the Mac's approval.
@MainActor private final class ConfirmHarness {
    let host: String
    let suite: String
    let identity: DeviceIdentity
    let storage: SharedStore.Context
    let accounts: AccountStore
    let clock: ApprovalClock
    let id: ComputerID
    let cloud: CloudClient
    /// The account whose session is active ("A" is this harness's; "B" another; "" signed out). The
    /// cloud client only gets a token while A is active, like the app's (`AccountSession.cloudClient`).
    let activeAccount: OSAllocatedUnfairLock<String>
    private var arrivals: AsyncStream<Void>.Iterator
    private var listsHeld: AsyncStream<Void>.Iterator
    private var revokesHeld: AsyncStream<Void>.Iterator
    private var statusesHeld: AsyncStream<Void>.Iterator

    /// `account` names this harness's account; `sharing` is another harness's active-account lock, for
    /// two accounts on one iPhone (only one active at a time).
    init(_ name: String, account: String = "A", sharing: OSAllocatedUnfairLock<String>? = nil) {
        let host = "\(name)-\(UUID().uuidString.lowercased()).test"
        let suite = "\(name).\(UUID().uuidString)"
        let identity = DeviceIdentity(signing: .init(), push: .init())
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [ApprovedCloudStub.self]
        let activeAccount = sharing ?? OSAllocatedUnfairLock(initialState: account)
        let cloud = CloudClient(baseURL: URL(string: "https://\(host)")!, identity: identity,
                                session: URLSession(configuration: config)) {
            guard activeAccount.withLock({ $0 }) == account else { throw CloudError(status: 0, code: "signedOut") }
            return "token-\(account)"
        }
        let storage = SharedStore.Context(accountID: "account", suite: suite)
        let accounts = AccountStore(storage: storage, cloud: cloud) { computer in
            BotStore(computer: computer, storage: storage) { _ in FakeRemote(.connecting) }
        }
        let clock = ApprovalClock()
        accounts.waitForApprovalPoll = { try await clock.wait() }
        accounts.nowMs = { ApprovedCloudStub.phase(host).now }
        self.host = host
        self.suite = suite
        self.identity = identity
        self.storage = storage
        self.accounts = accounts
        self.clock = clock
        self.cloud = cloud
        self.activeAccount = activeAccount
        id = RelayCrypto.computerId(signKey: hostKey)
        arrivals = clock.arrivals.stream.makeAsyncIterator()
        let lists = AsyncStream<Void>.makeStream()
        let revokes = AsyncStream<Void>.makeStream()
        let statuses = AsyncStream<Void>.makeStream()
        ApprovedCloudStub.listHeld.withLock {
            $0["\(host)#list"] = lists.continuation
            $0["\(host)#revoke"] = revokes.continuation
            $0["\(host)#status"] = statuses.continuation
        }
        listsHeld = lists.stream.makeAsyncIterator()
        revokesHeld = revokes.stream.makeAsyncIterator()
        statusesHeld = statuses.stream.makeAsyncIterator()
    }

    func finish() {
        accounts.retire()
        clock.finish()
        for key in ["\(host)#list", "\(host)#revoke", "\(host)#status"] {
            ApprovedCloudStub.heldLists.withLock { _ = $0.removeValue(forKey: key) }
            ApprovedCloudStub.listHeld.withLock { $0.removeValue(forKey: key) }?.finish()
        }
        UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite)
        ApprovedCloudStub.remove(host)
    }

    /// Waits until the poll processed an answer and waits again.
    func nextPoll() async {
        var iterator = arrivals
        // Kept on the main actor: the iterator is this class's state, so it must not be sent to a
        // nonisolated `next()` (Swift 6 region isolation).
        _ = await iterator.next(isolation: #isolation)
        arrivals = iterator
    }

    func update(_ change: (inout ApprovedCloudStub.Phase) -> Void) { ApprovedCloudStub.update(host, change) }
    var phase: ApprovedCloudStub.Phase { ApprovedCloudStub.phase(host) }

    /// Request made and revealed; the cloud still says "pending".
    func request() async throws {
        await accounts.refreshCloud()
        try await accounts.requestAccess(id)
        await nextPoll()
    }

    /// The Mac approves and every check passes: the request now waits for the Mac's code.
    func approve() async {
        update { $0.macApproves = true }
        clock.advance()
        await nextPoll()
    }

    /// The Mac approves; the store takes the approval and is now waiting on the held account list.
    func approveHoldingList() async {
        update { $0.macApproves = true; $0.holdList = true }
        clock.advance()
        var iterator = listsHeld
        _ = await iterator.next(isolation: #isolation)
        listsHeld = iterator
    }

    /// Sends the held account list.
    func releaseList() {
        update { $0.holdList = false }
        ApprovedCloudStub.heldLists.withLock { $0.removeValue(forKey: "\(host)#list") }?()
    }

    /// Waits until a grant revocation reached the cloud and its answer is held.
    func revokeIsHeld() async {
        var iterator = revokesHeld
        _ = await iterator.next(isolation: #isolation)
        revokesHeld = iterator
    }

    /// Waits until a request read reached the cloud and its answer is held.
    func statusIsHeld() async {
        var iterator = statusesHeld
        _ = await iterator.next(isolation: #isolation)
        statusesHeld = iterator
    }

    /// Sends the held request read.
    func releaseStatus() {
        update { $0.holdStatus = false }
        ApprovedCloudStub.heldLists.withLock { $0.removeValue(forKey: "\(host)#status") }?()
    }

    /// The app's account change: our hook while A is still active (`AccountSession.beforeLeaving`), then
    /// the session changes (to B, or none on sign out), then A's context retires (`AppStore.switchAccount`).
    func leaveAccount(to next: String) async {
        await accounts.settleBeforeLeaving()
        activeAccount.withLock { $0 = next }
        accounts.retire()
    }

    /// Sends the held revocation answer.
    func releaseRevoke() {
        update { $0.holdRevoke = false; $0.revokeOutstanding = false }
        ApprovedCloudStub.heldLists.withLock { $0.removeValue(forKey: "\(host)#revoke") }?()
    }

    /// The code the real Mac would show for this request, computed apart from the store's check.
    var macCode: String? { accounts.confirmations[id]?.confirmation.expectedCode }

    /// Another six digits than the Mac's code.
    var wrongCode: String {
        let code = macCode ?? "000000"
        let last = (Int(String(code.last!))! + 1) % 10
        return String(code.dropLast()) + String(last)
    }

    var savedNothing: Bool { storage.computers.isEmpty && accounts.stores.isEmpty }

    /// The owner revocation of the grant the cloud made when the Mac approved.
    var grantPath: String { "/v1/computers/\(id)/grants/grant_fixture" }

    /// Waits for the grant revocation, if one was started.
    func revocationsSettled() async { await accounts.grantRevocations[id]?.value }
}

/// The Mac approves and every check passes: nothing is saved yet. The iPhone waits for the code the Mac
/// shows (a Mac without it never gets saved), and only that code, typed here, saves the computer over
/// the relay with the key the SAS was computed with.
@MainActor @Test(.timeLimit(.minutes(1))) func approvedWaitsForTheMacCodeAndOnlyTheRightCodeSaves() async throws {
    let h = ConfirmHarness("approved-ok")
    defer { h.finish() }
    try await h.request()
    #expect(h.phase.revealed) // requestAccess only returns after the reveal.
    let ticket = try #require(h.accounts.pendingAccess[h.id])
    #expect(h.accounts.accessConfirm.isEmpty)

    // What the code is bound to is this request's own: same nonce as the SAS shown, this iPhone's key.
    let material = try #require(h.accounts.confirmations[h.id]?.confirmation)
    #expect(material.requestId == "req_ok")
    #expect(material.deviceKey == h.identity.deviceKey)
    #expect(material.hostSignPub == hostKey)
    #expect(material.hostNonce == Data(repeating: 9, count: 32))
    #expect(RelayCrypto.sasCode(hostSignKey: hostKey, deviceKey: h.identity.deviceKey, deviceNonce: material.deviceNonce,
                                hostNonce: material.hostNonce) == ticket.code)

    await h.approve()
    #expect(h.accounts.accessConfirm[h.id] == AccessConfirmPrompt(requestId: "req_ok", attemptsLeft: 5))
    #expect(h.savedNothing)
    // An old Mac shows no code: the cloud keeps saying "approved" and nothing is ever saved.
    for _ in 0..<2 {
        h.clock.advance()
        await h.nextPoll()
        #expect(h.savedNothing)
        #expect(h.accounts.accessConfirm[h.id]?.attemptsLeft == 5)
    }

    let code = try #require(h.macCode)
    #expect(await h.accounts.confirmAccess(h.id, code: "\(code.prefix(3)) \(code.suffix(3))") == .saved)
    #expect(h.storage.computers.first?.id == h.id)
    #expect(h.storage.computers.first?.signKey == ticket.signKey)
    #expect(h.storage.computers.first?.route == .relayOnly)
    #expect(h.accounts.stores[h.id] != nil)
    #expect(h.accounts.pendingAccess.isEmpty)
    #expect(h.accounts.accessConfirm.isEmpty)
    #expect(h.accounts.confirmations.isEmpty)
    #expect(h.phase.deletes == 0)
    // Confirmed: the grant is the link, never revoked.
    #expect(h.accounts.grantRevocations.isEmpty)
    #expect(h.phase.grantDeletes.isEmpty)
}

/// A wrong code saves nothing and counts a try; fewer than six digits isn't a try. The fifth wrong code
/// withdraws the request in the cloud and drops everything; even the right code does nothing after.
@MainActor @Test(.timeLimit(.minutes(1))) func wrongCodesCountAndTheFifthWithdrawsTheRequest() async throws {
    let h = ConfirmHarness("wrong-code")
    defer { h.finish() }
    try await h.request()
    await h.approve()
    let right = try #require(h.macCode)
    let wrong = h.wrongCode

    #expect(await h.accounts.confirmAccess(h.id, code: "12345") == .incomplete)
    #expect(h.accounts.accessConfirm[h.id]?.attemptsLeft == 5)
    for left in (1...4).reversed() {
        #expect(await h.accounts.confirmAccess(h.id, code: wrong) == .wrong(attemptsLeft: left))
        #expect(h.accounts.accessConfirm[h.id]?.attemptsLeft == left)
        #expect(h.savedNothing)
        #expect(h.phase.deletes == 0)
    }
    #expect(h.phase.grantDeletes.isEmpty)
    #expect(await h.accounts.confirmAccess(h.id, code: wrong) == .tooManyAttempts)
    #expect(h.phase.deletes == 1)
    await h.revocationsSettled()
    #expect(h.phase.grantDeletes == [h.grantPath]) // The approval's grant goes too, once.
    #expect(h.accounts.pendingAccess.isEmpty)
    #expect(h.accounts.accessConfirm.isEmpty)
    #expect(h.accounts.confirmations.isEmpty)
    #expect(h.accounts.lastError == AccessConfirmText.text(.tooMany))
    #expect(await h.accounts.confirmAccess(h.id, code: right) == .ended)
    #expect(h.savedNothing)
}

/// The cloud slips a "confirmCode" into its approval, right or wrong: it is never used. Nothing is saved
/// until a code is typed here, and typing the cloud's wrong code is just a wrong code.
@MainActor @Test(.timeLimit(.minutes(1))) func confirmCodeFromTheNetworkIsIgnored() async throws {
    let h = ConfirmHarness("network-code")
    defer { h.finish() }
    try await h.request()
    let right = try #require(h.macCode)
    h.update { $0.networkConfirmCode = right }
    await h.approve()
    h.clock.advance()
    await h.nextPoll()
    #expect(h.savedNothing)
    #expect(h.accounts.accessConfirm[h.id]?.attemptsLeft == 5)

    let wrong = h.wrongCode
    h.update { $0.networkConfirmCode = wrong }
    h.clock.advance()
    await h.nextPoll()
    #expect(await h.accounts.confirmAccess(h.id, code: wrong) == .wrong(attemptsLeft: 4))
    #expect(h.savedNothing)
}

/// The request or the approval runs out while the person types: the right code saves nothing and the
/// request ends with a message.
@MainActor @Test(.timeLimit(.minutes(1)), arguments: ["ticket", "approval"])
func deadlinePassingWhileTypingSavesNothing(deadline: String) async throws {
    let h = ConfirmHarness("typing-\(deadline)")
    defer { h.finish() }
    h.update { $0.approvalExpiresAt = 5_000 } // Valid when approved (now 1000).
    try await h.request()
    await h.approve()
    #expect(h.accounts.accessConfirm[h.id] != nil)
    let code = try #require(h.macCode)
    h.update { $0.now = deadline == "ticket" ? 4_102_444_800_001 : 6_000 }

    #expect(await h.accounts.confirmAccess(h.id, code: code) == .ended)
    #expect(h.savedNothing)
    // Both: the request is withdrawn and the approval's grant revoked, once each.
    #expect(h.phase.deletes == 1)
    await h.revocationsSettled()
    #expect(h.phase.grantDeletes == [h.grantPath])
    #expect(h.accounts.pendingAccess.isEmpty)
    #expect(h.accounts.accessConfirm.isEmpty)
    #expect(h.accounts.confirmations.isEmpty)
    #expect(h.accounts.lastError == "O pedido expirou. Peça acesso novamente.")
}

/// Cancelling, expiring, switching accounts or asking again drops the waiting step and everything it
/// was bound to (the device nonce included): the old code confirms nothing.
@MainActor @Test(.timeLimit(.minutes(1)), arguments: ["cancel", "expire", "retire", "retry"])
func endingTheRequestDropsTheConfirmationStep(ending: String) async throws {
    let h = ConfirmHarness("drop-\(ending)")
    defer { h.finish() }
    try await h.request()
    await h.approve()
    let oldCode = try #require(h.macCode)
    let oldNonce = try #require(h.accounts.confirmations[h.id]?.confirmation.deviceNonce)

    switch ending {
    case "cancel":
        await h.accounts.cancelAccess(h.id)
        #expect(h.phase.deletes == 1)
    case "expire":
        h.update { $0.now = 4_102_444_800_001 }
        h.clock.advance()
        await h.accounts.accessPolls[h.id]?.value // The poll ends on its own once it sees the expiry.
        #expect(h.accounts.pendingAccess.isEmpty)
    case "retire":
        h.accounts.retire()
    default:
        h.update { $0.macApproves = false }
        try await h.accounts.requestAccess(h.id)
        await h.nextPoll()
        // The new request has its own nonce; the Mac hasn't approved it yet.
        let fresh = try #require(h.accounts.confirmations[h.id]?.confirmation.deviceNonce)
        #expect(fresh != oldNonce)
    }
    #expect(h.accounts.accessConfirm.isEmpty)
    if ending != "retry" { #expect(h.accounts.confirmations.isEmpty) }
    #expect(await h.accounts.confirmAccess(h.id, code: oldCode) == .ended)
    #expect(h.savedNothing)
    // The Mac had approved: its grant is revoked, once, with this request's ids. A new request is only
    // made after the revocation, or the cloud would refuse it (409).
    await h.revocationsSettled()
    #expect(h.phase.grantDeletes == [h.grantPath])
    if ending == "retry" { #expect(h.phase.calls == ["create", "revoke", "create"]) }
}

/// Ending a request before the Mac approved leaves no grant behind: nothing is revoked.
@MainActor @Test(.timeLimit(.minutes(1)), arguments: ["cancel", "retire", "retry", "refusedApproval"])
func endingBeforeApprovalRevokesNothing(ending: String) async throws {
    let h = ConfirmHarness("before-approval-\(ending)")
    defer { h.finish() }
    try await h.request()
    switch ending {
    case "cancel":
        await h.accounts.cancelAccess(h.id)
        #expect(h.phase.deletes == 1)
    case "retire":
        h.accounts.retire()
    case "retry":
        try await h.accounts.requestAccess(h.id)
        await h.nextPoll()
    default:
        // Approved with a host nonce that isn't this request's: the checks refuse it, no code step.
        h.update { $0.macApproves = true; $0.statusNonce = 8 }
        h.clock.advance()
        await h.nextPoll()
        await h.accounts.cancelAccess(h.id)
    }
    // The request is read back once (nothing approved for it there): no revocation.
    await h.revocationsSettled()
    #expect(h.phase.grantDeletes.isEmpty)
    #expect(h.savedNothing)
}

/// The Mac approved on the server (the person typed the code there), but this iPhone hadn't seen it yet
/// when the request ended: cancel, expiry, account switch with a valid session, or a new request. One
/// look at the request finds the approval and revokes its grant exactly once; nothing is saved from it.
@MainActor @Test(.timeLimit(.minutes(1)), arguments: ["cancel", "expire", "retire", "retry"])
func endingBeforeSeeingTheServerApprovalRevokesItsGrantOnce(ending: String) async throws {
    let h = ConfirmHarness("unseen-\(ending)")
    defer { h.finish() }
    try await h.request()
    let poll = try #require(h.accounts.accessPolls[h.id])
    h.update { $0.macApproves = true } // Approved on the server; the iPhone hasn't polled since.
    #expect(h.accounts.confirmations[h.id]?.grantId == nil)

    switch ending {
    case "cancel":
        await h.accounts.cancelAccess(h.id)
        #expect(h.phase.deletes == 1)
    case "expire":
        h.update { $0.now = 4_102_444_800_001 }
        h.clock.advance()
        await poll.value // Ends on the expiry, before reading the request.
    case "retire":
        h.accounts.retire()
    default:
        h.update { $0.requestId = "req_two" }
        try await h.accounts.requestAccess(h.id)
        await h.nextPoll()
    }
    await h.revocationsSettled()
    #expect(h.phase.grantDeletes == [h.grantPath])
    #expect(h.accounts.accessConfirm.isEmpty)
    #expect(h.savedNothing)
    if ending == "retry" {
        // The new request is made only after the revocation, and it is the new one that's pending.
        #expect(h.phase.calls == ["create", "revoke", "create"])
        #expect(h.accounts.pendingAccess[h.id]?.requestId == "req_two")
        #expect(h.accounts.confirmations[h.id]?.grantId == nil)
    } else {
        #expect(h.accounts.pendingAccess.isEmpty)
        #expect(h.accounts.confirmations.isEmpty)
    }
}

/// Switching to B or signing out while A's request showed its code and the Mac already approved it on
/// the server (unseen here): before the session changes, A's request is read and its grant revoked with
/// A's own session, exactly once; then the change goes on. Back on A, asking again isn't refused (409).
@MainActor @Test(.timeLimit(.minutes(1)), arguments: ["B", ""])
func leavingTheAccountRevokesAnUnseenApprovalWithItsOwnSession(next: String) async throws {
    let h = ConfirmHarness("leave-\(next.isEmpty ? "signout" : "switch")")
    defer { h.finish() }
    h.update { $0.conflictOnActiveGrant = true }
    try await h.request()
    h.update { $0.macApproves = true } // Approved on the server; the iPhone hasn't polled since.
    let reads = h.phase.statusTokens.count
    // A deadline that never passes: the change waits for the clean-up itself, no wall clock.
    let never = AsyncStream<Void>.makeStream()
    h.accounts.leaveDeadline = {
        var iterator = never.stream.makeAsyncIterator()
        _ = await iterator.next()
    }

    await h.leaveAccount(to: next)
    #expect(h.activeAccount.withLock { $0 } == next) // The change went on.
    await h.revocationsSettled()
    #expect(Array(h.phase.statusTokens.dropFirst(reads)) == ["Bearer token-A"]) // One read, with A's session.
    #expect(h.phase.revokeTokens == ["Bearer token-A"]) // One revocation, with A's session.
    #expect(h.phase.grantDeletes == [h.grantPath])
    #expect(h.accounts.pendingAccess.isEmpty)
    #expect(h.accounts.confirmations.isEmpty)
    #expect(h.savedNothing)

    // Back on A, in a new context like the app's: a new request goes through.
    h.activeAccount.withLock { $0 = "A" }
    h.update { $0.requestId = "req_two" }
    let clock = ApprovalClock()
    let back = AccountStore(storage: h.storage, cloud: h.cloud) { computer in
        Issue.record("Asking again must not open the computer")
        return BotStore(computer: computer, storage: h.storage) { _ in FakeRemote(.connecting) }
    }
    back.waitForApprovalPoll = { try await clock.wait() }
    back.nowMs = { 1_000 }
    defer { back.retire(); clock.finish() }
    await back.refreshCloud()
    try await back.requestAccess(h.id)
    #expect(h.phase.calls == ["create", "revoke", "create"])
}

/// The clean-up before an account change takes longer than its deadline (the read is held): the change
/// goes on anyway, and once A's session is gone nothing more is sent with it, nor with B's.
@MainActor @Test(.timeLimit(.minutes(1)), arguments: ["B", ""])
func leavingTheAccountDoesNotWaitPastItsDeadline(next: String) async throws {
    let h = ConfirmHarness("leave-late-\(next.isEmpty ? "signout" : "switch")")
    defer { h.finish() }
    try await h.request()
    h.update { $0.macApproves = true; $0.holdStatus = true }
    // The deadline passes as soon as the read is held.
    h.accounts.leaveDeadline = { await h.statusIsHeld() }

    await h.leaveAccount(to: next)
    #expect(h.activeAccount.withLock { $0 } == next) // Not held past the deadline.
    #expect(h.phase.grantDeletes.isEmpty)

    h.releaseStatus()
    await h.revocationsSettled()
    #expect(h.phase.grantDeletes.isEmpty) // A's session is gone: no revocation with it, none with B's.
    #expect(h.phase.revokeTokens.isEmpty)
    #expect(h.accounts.pendingAccess.isEmpty)
    #expect(h.savedNothing)
}

/// The person cancelled just before switching or signing out, after the Mac approved on the server: the
/// read of that request is still out when the account changes. The change waits for it (within its
/// deadline): the grant is revoked with A's session, once, and only then the session changes.
@MainActor @Test(.timeLimit(.minutes(1)), arguments: ["B", ""])
func leavingTheAccountWaitsForACleanUpAlreadyRunning(next: String) async throws {
    let h = ConfirmHarness("leave-running-\(next.isEmpty ? "signout" : "switch")")
    defer { h.finish() }
    try await h.request()
    h.update { $0.macApproves = true; $0.holdStatus = true }
    await h.accounts.cancelAccess(h.id)
    await h.statusIsHeld() // The cancelled request's read is out, unanswered.
    #expect(h.accounts.pendingAccess.isEmpty)

    // The deadline never passes; it signals once the change is waiting on the clean-up.
    let waiting = AsyncStream<Void>.makeStream()
    let never = AsyncStream<Void>.makeStream()
    h.accounts.leaveDeadline = {
        waiting.continuation.yield(())
        var iterator = never.stream.makeAsyncIterator()
        _ = await iterator.next()
    }
    let leaving = Task { await h.leaveAccount(to: next) }
    var waits = waiting.stream.makeAsyncIterator()
    _ = await waits.next()
    #expect(h.activeAccount.withLock { $0 } == "A") // Still A while the read is out.

    h.releaseStatus()
    await leaving.value
    #expect(h.phase.revokeTokens == ["Bearer token-A"]) // Revoked with A's session, before the change.
    #expect(h.phase.grantDeletes == [h.grantPath])
    #expect(h.activeAccount.withLock { $0 } == next)
    #expect(h.savedNothing)
}

/// Same, but the read outlasts the deadline: the change goes on, and nothing is sent with B's session.
@MainActor @Test(.timeLimit(.minutes(1)), arguments: ["B", ""])
func leavingTheAccountDoesNotWaitPastItsDeadlineForACleanUpAlreadyRunning(next: String) async throws {
    let h = ConfirmHarness("leave-running-late-\(next.isEmpty ? "signout" : "switch")")
    defer { h.finish() }
    try await h.request()
    h.update { $0.macApproves = true; $0.holdStatus = true }
    await h.accounts.cancelAccess(h.id)
    await h.statusIsHeld()
    h.accounts.leaveDeadline = {} // Passes at once.

    await h.leaveAccount(to: next)
    #expect(h.activeAccount.withLock { $0 } == next) // Not held past the deadline.
    h.releaseStatus()
    await h.revocationsSettled()
    #expect(h.phase.grantDeletes.isEmpty) // A's session is gone: no revocation with it, none with B's.
    #expect(h.phase.revokeTokens.isEmpty)
    #expect(h.savedNothing)
}

/// The app's single path for session changes (`AccountTransition`: `AccountSession` sends signing in,
/// switching and signing out through it), with the store's clean-up attached as the app attaches it.
/// With A active and A's request approved on the server (unseen here), the request is read and its grant
/// revoked with A's session before the change itself starts; then B, or nobody, is active.
@MainActor @Test(.timeLimit(.minutes(1)), arguments: ["switch", "signIn", "signOut"])
func sessionChangesCleanUpTheActiveAccountFirst(change: String) async throws {
    let h = ConfirmHarness("transition-\(change)")
    defer { h.finish() }
    try await h.request()
    h.update { $0.macApproves = true } // Approved on the server; the iPhone hasn't polled since.
    let reads = h.phase.statusTokens.count
    let never = AsyncStream<Void>.makeStream()
    h.accounts.leaveDeadline = { // Never passes: the change waits for the clean-up itself, no wall clock.
        var iterator = never.stream.makeAsyncIterator()
        _ = await iterator.next()
    }
    let transition = AccountTransition()
    transition.isAccountActive = { h.activeAccount.withLock { $0 } == "A" }
    transition.attach { h.accounts }
    let next = change == "signOut" ? "" : "B"

    var readsAtChange: [String] = []
    var revokesAtChange: [String] = []
    try await transition.run(change == "signIn" ? .signInWithCallback : .change) {
        // The Clerk call: B signs in or becomes active, or A signs out. What reached the cloud by now:
        readsAtChange = Array(h.phase.statusTokens.dropFirst(reads))
        revokesAtChange = h.phase.revokeTokens
        h.activeAccount.withLock { $0 = next }
    }
    h.accounts.retire() // What the app does once the active account changed.

    #expect(readsAtChange == ["Bearer token-A"]) // One read, with A's session, before the change.
    #expect(revokesAtChange == ["Bearer token-A"]) // One revocation, with A's session, before the change.
    await h.revocationsSettled()
    #expect(h.phase.grantDeletes == [h.grantPath])
    #expect(h.activeAccount.withLock { $0 } == next)
    #expect(h.accounts.pendingAccess.isEmpty)
    #expect(h.savedNothing)
}

/// Signing in with no account active: there is nothing to clean up, and the change doesn't wait.
@MainActor @Test func sessionChangeWithoutAnActiveAccountWaitsForNothing() async throws {
    let transition = AccountTransition()
    transition.isAccountActive = { false }
    let cleaned = OSAllocatedUnfairLock(initialState: false)
    transition.attach {
        cleaned.withLock { $0 = true }
        return nil
    }
    var changed = false
    try await transition.run { changed = true }
    #expect(changed)
    #expect(!cleaned.withLock { $0 })
}

/// Two account changes asked at once run one after the other. The first activates B and, before it
/// returns, B opens a request the Mac approves on the server (unseen here). The second starts only once
/// the first has ended, reads that B is now the active account and cleans up B's store: one read and one
/// revocation with B's session, before its own change.
@MainActor @Test(.timeLimit(.minutes(1))) func sessionChangesRunOneAtATimeAndCleanUpTheAccountActiveWhenEachStarts() async throws {
    let a = ConfirmHarness("queue-a")
    let b = ConfirmHarness("queue-b", account: "B", sharing: a.activeAccount)
    defer { a.finish(); b.finish() }
    let never = AsyncStream<Void>.makeStream()
    for h in [a, b] {
        h.accounts.leaveDeadline = { // Never passes: each change waits for its clean-up, no wall clock.
            var iterator = never.stream.makeAsyncIterator()
            _ = await iterator.next()
        }
    }
    let active = a.activeAccount
    let transition = AccountTransition()
    transition.isAccountActive = { !active.withLock { $0 }.isEmpty }
    // Whichever account is active when a change starts, its store: what the app's `attach` reads.
    transition.attach { active.withLock { $0 } == "B" ? b.accounts : a.accounts }

    let firstIn = AsyncStream<Void>.makeStream()
    let releaseFirst = AsyncStream<Void>.makeStream()
    let firstEnded = OSAllocatedUnfairLock(initialState: false)
    let first = Task {
        try await transition.run {
            active.withLock { $0 = "B" } // B becomes active, and the call hasn't returned yet.
            firstIn.continuation.yield(())
            var iterator = releaseFirst.stream.makeAsyncIterator()
            _ = await iterator.next()
            firstEnded.withLock { $0 = true }
        }
    }
    var entered = firstIn.stream.makeAsyncIterator()
    _ = await entered.next()

    let firstEndedAtSecond = OSAllocatedUnfairLock(initialState: false)
    let revokesAtSecond = OSAllocatedUnfairLock(initialState: [String]())
    let second = Task {
        try await transition.run {
            let ended = firstEnded.withLock { $0 }
            let revokes = b.phase.revokeTokens
            firstEndedAtSecond.withLock { $0 = ended }
            revokesAtSecond.withLock { $0 = revokes }
            active.withLock { $0 = "A" } // Back to A.
        }
    }

    // Meanwhile on B: a request the Mac approves on the server; this iPhone hasn't polled since.
    try await b.request()
    b.update { $0.macApproves = true }
    let reads = b.phase.statusTokens.count
    releaseFirst.continuation.yield(())
    try await first.value
    try await second.value

    #expect(firstEndedAtSecond.withLock { $0 }) // The second started only after the first ended...
    #expect(revokesAtSecond.withLock { $0 } == ["Bearer token-B"]) // ...and cleaned B up before its change.
    #expect(Array(b.phase.statusTokens.dropFirst(reads)) == ["Bearer token-B"])
    #expect(b.phase.grantDeletes == [b.grantPath])
    #expect(b.accounts.pendingAccess.isEmpty)
    #expect(a.phase.grantDeletes.isEmpty)
    #expect(active.withLock { $0 } == "A")
}

/// The browser comes back through the app's URL while its sign-in's Clerk call runs (its window, after
/// its clean-up): that callback is part of the sign-in, so it runs at once (queued behind it, it would
/// wait forever) and the sign-in ends. A callback on its own is a change of its own, with its clean-up.
@MainActor @Test(.timeLimit(.minutes(1))) func aURLCallbackDuringASignInIsPartOfItAndDoesNotWait() async throws {
    let transition = AccountTransition()
    transition.isAccountActive = { true }
    let cleanUps = OSAllocatedUnfairLock(initialState: 0)
    transition.attach {
        cleanUps.withLock { $0 += 1 }
        return nil
    }
    let started = AsyncStream<Void>.makeStream()
    let browserBack = AsyncStream<Void>.makeStream()
    let signIn = Task {
        try await transition.run(.signInWithCallback) { () -> String in
            started.continuation.yield(())
            var iterator = browserBack.stream.makeAsyncIterator()
            _ = await iterator.next() // Waits for the browser, which answers through the callback below.
            return "signed in"
        }
    }
    var running = started.stream.makeAsyncIterator()
    _ = await running.next()

    let handled = try await transition.run(.callback) { () -> String in "callback handled" }
    browserBack.continuation.yield(())
    #expect(handled == "callback handled")
    #expect(try await signIn.value == "signed in")
    #expect(cleanUps.withLock { $0 } == 1) // The sign-in's clean-up only.

    try await transition.run(.callback) {}
    #expect(cleanUps.withLock { $0 } == 2)
}

/// A callback arrives while a browser sign-in is still cleaning up (its read held) — after an earlier
/// sign-in ended, or with none before: the browser can't be answering this sign-in yet, so the callback
/// doesn't jump the queue. It waits for the sign-in to end, then runs as a change of its own.
@MainActor @Test(.timeLimit(.minutes(1)), arguments: ["first", "afterAnother"])
func aCallbackDuringASignInCleanUpWaitsItsTurn(signIn: String) async throws {
    let h = ConfirmHarness("callback-cleanup-\(signIn)")
    defer { h.finish() }
    let never = AsyncStream<Void>.makeStream()
    h.accounts.leaveDeadline = {
        var iterator = never.stream.makeAsyncIterator()
        _ = await iterator.next()
    }
    let transition = AccountTransition()
    transition.isAccountActive = { true }
    transition.attach { h.accounts }
    let order = OSAllocatedUnfairLock(initialState: [String]())
    if signIn == "afterAnother" {
        try await transition.run(.signInWithCallback) { order.withLock { $0.append("earlier sign-in") } }
    }

    // A's request approved on the server: this sign-in's clean-up reads it, and the read is held.
    try await h.request()
    h.update { $0.macApproves = true; $0.holdStatus = true }
    let signingIn = Task {
        try await transition.run(.signInWithCallback) { order.withLock { $0.append("sign-in") } }
    }
    await h.statusIsHeld()

    let asked = AsyncStream<Void>.makeStream()
    let callback = Task {
        asked.continuation.yield(())
        try await transition.run(.callback) { order.withLock { $0.append("callback") } }
    }
    var asking = asked.stream.makeAsyncIterator()
    _ = await asking.next() // The callback was asked for and is waiting: it didn't run.
    let before = signIn == "afterAnother" ? ["earlier sign-in"] : []
    #expect(order.withLock { $0 } == before)

    h.releaseStatus()
    try await signingIn.value
    try await callback.value
    #expect(order.withLock { $0 } == before + ["sign-in", "callback"])
    #expect(h.phase.grantDeletes == [h.grantPath]) // The sign-in's clean-up finished first.
}

/// A callback taken in by a browser sign-in is still running when that sign-in is cancelled (or fails):
/// the next change starts only after the callback has ended.
@MainActor @Test(.timeLimit(.minutes(1)), arguments: ["cancel", "error"])
func aSignInEndingWithACallbackStillRunningHoldsTheNextChange(ending: String) async throws {
    struct Failure: Error {}
    let transition = AccountTransition()
    let started = AsyncStream<Void>.makeStream()
    let release = AsyncStream<Void>.makeStream()
    let signInOut = AsyncStream<Void>.makeStream()
    let signIn = Task {
        try await transition.run(.signInWithCallback) {
            started.continuation.yield(())
            var iterator = release.stream.makeAsyncIterator()
            _ = await iterator.next()
            signInOut.continuation.yield(())
            if ending == "error" { throw Failure() }
            try Task.checkCancellation()
        }
    }
    var running = started.stream.makeAsyncIterator()
    _ = await running.next()

    let order = OSAllocatedUnfairLock(initialState: [String]())
    let callbackIn = AsyncStream<Void>.makeStream()
    let callbackRelease = AsyncStream<Void>.makeStream()
    let callback = Task {
        try await transition.run(.callback) {
            callbackIn.continuation.yield(())
            var iterator = callbackRelease.stream.makeAsyncIterator()
            _ = await iterator.next()
            order.withLock { $0.append("callback ended") }
        }
    }
    var admitted = callbackIn.stream.makeAsyncIterator()
    _ = await admitted.next() // Taken in by the sign-in, and still running.

    let next = Task {
        try await transition.run { order.withLock { $0.append("next change") } }
    }
    if ending == "cancel" { signIn.cancel() } else { release.continuation.yield(()) }
    var out = signInOut.stream.makeAsyncIterator()
    _ = await out.next() // The sign-in's Clerk call ended; it now waits for the callback.
    #expect(order.withLock { $0 }.isEmpty) // The next change hasn't started.

    callbackRelease.continuation.yield(())
    try await callback.value
    if case .success = await signIn.result { Issue.record("The sign-in must end with \(ending)") }
    try await next.value
    #expect(order.withLock { $0 } == ["callback ended", "next change"])
}

/// The first change fails or is cancelled while the second waits: the queue goes on and the second runs.
@MainActor @Test(.timeLimit(.minutes(1)), arguments: ["error", "cancel"])
func aFailedSessionChangeLetsTheNextOneRun(ending: String) async throws {
    struct Failure: Error {}
    let transition = AccountTransition()
    let started = AsyncStream<Void>.makeStream()
    let release = AsyncStream<Void>.makeStream()
    let first = Task {
        try await transition.run {
            started.continuation.yield(())
            var iterator = release.stream.makeAsyncIterator()
            _ = await iterator.next()
            if ending == "error" { throw Failure() }
            try Task.checkCancellation()
        }
    }
    var running = started.stream.makeAsyncIterator()
    _ = await running.next()
    let secondRan = OSAllocatedUnfairLock(initialState: false)
    let second = Task {
        try await transition.run { secondRan.withLock { $0 = true } }
    }
    if ending == "cancel" { first.cancel() }
    release.continuation.yield(())
    if case .success = await first.result { Issue.record("The first change must end with \(ending)") }
    try await second.value
    #expect(secondRan.withLock { $0 })
}

/// A change cancelled while it waits its turn never calls its change (nor Clerk), and doesn't hold the queue.
@MainActor @Test(.timeLimit(.minutes(1))) func aChangeCancelledBeforeItsTurnDoesNotRun() async throws {
    let transition = AccountTransition()
    let started = AsyncStream<Void>.makeStream()
    let release = AsyncStream<Void>.makeStream()
    let first = Task {
        try await transition.run {
            started.continuation.yield(())
            var iterator = release.stream.makeAsyncIterator()
            _ = await iterator.next()
        }
    }
    var running = started.stream.makeAsyncIterator()
    _ = await running.next()
    let ran = OSAllocatedUnfairLock(initialState: [String]())
    let cancelled = Task { try await transition.run { ran.withLock { $0.append("cancelled") } } }
    let third = Task { try await transition.run { ran.withLock { $0.append("third") } } }
    cancelled.cancel()
    release.continuation.yield(())
    try await first.value
    if case .success = await cancelled.result { Issue.record("A cancelled change must not run") }
    try await third.value
    #expect(ran.withLock { $0 } == ["third"])
}

/// Every Clerk call in `AccountSession` that can change the active session sits inside `changeSession`.
/// The app has no test target, so this reads its source; a new call outside the wrapper fails here.
@Test func accountSessionChangesTheSessionOnlyThroughTheTransition() throws {
    let url = URL(filePath: #filePath).deletingLastPathComponent()
        .appending(path: "../../../App/AccountSession.swift").standardized
    let lines = try String(contentsOf: url, encoding: .utf8).components(separatedBy: "\n")
    let calls = [".signInWith", ".signOut(sessionId", ".setActive(sessionId", "clerk.handle(", "verifyMfaCode("]
    var found = 0
    for (index, line) in lines.enumerated() where calls.contains(where: { line.contains($0) }) {
        found += 1
        let context = lines[max(0, index - 3)...index].joined(separator: "\n")
        #expect(context.contains("changeSession"), "AccountSession.swift:\(index + 1) calls Clerk outside changeSession")
    }
    #expect(found >= 8) // Password, Google, Apple, email code, URL, switch, sign out, sign out of all.
}

/// Reading the request back after it ended revokes only an approval of it: pending or denied leaves
/// nothing to revoke, and a failed read isn't retried. Either way the next request isn't held.
@MainActor @Test(.timeLimit(.minutes(1)), arguments: ["pending", "denied", "networkError"])
func reconciliationRevokesOnlyAnApprovalOfThisRequest(answer: String) async throws {
    let h = ConfirmHarness("reconcile-\(answer)")
    defer { h.finish() }
    try await h.request()
    switch answer {
    case "denied": h.update { $0.statusOverride = "denied" }
    case "networkError": h.update { $0.macApproves = true; $0.statusFails = true } // Approved, but unreadable.
    default: break
    }
    let gets = h.phase.statusGets
    await h.accounts.cancelAccess(h.id)
    await h.revocationsSettled()
    #expect(h.phase.statusGets == gets + 1) // Read once, never again.
    #expect(h.phase.grantDeletes.isEmpty)
    #expect(h.accounts.pendingAccess.isEmpty)
    #expect(h.savedNothing)

    h.update { $0.statusFails = false; $0.statusOverride = nil; $0.macApproves = false; $0.requestId = "req_two" }
    try await h.accounts.requestAccess(h.id)
    await h.nextPoll()
    #expect(h.accounts.pendingAccess[h.id]?.requestId == "req_two")
    #expect(h.phase.grantDeletes.isEmpty)
}

/// The revocation fails: nothing is saved, the screen isn't held, and it isn't tried again. The next
/// request is still made; the cloud refuses it with 409, which reads as "already approved".
@MainActor @Test(.timeLimit(.minutes(1))) func failedRevocationSavesNothingAndIsNotRetried() async throws {
    let h = ConfirmHarness("revoke-fails")
    defer { h.finish() }
    h.update { $0.grantRevokeStatus = 500 }
    try await h.request()
    await h.approve()
    await h.accounts.cancelAccess(h.id)
    #expect(h.accounts.pendingAccess.isEmpty)
    #expect(h.accounts.accessConfirm.isEmpty)
    await h.revocationsSettled()
    #expect(h.phase.grantDeletes == [h.grantPath])
    #expect(h.savedNothing)

    h.update { $0.createStatus = 409 }
    do {
        try await h.accounts.requestAccess(h.id)
        Issue.record("A 409 must end the new request")
    } catch let error as CloudError {
        #expect(error.status == 409)
        #expect(error.code == "deviceAlreadyApproved")
        #expect(error.message == nil) // The cloud's text never comes along.
    }
    #expect(h.phase.grantDeletes == [h.grantPath]) // Not retried.
    #expect(h.accounts.pendingAccess.isEmpty)
    #expect(h.savedNothing)
}

/// The request ends while the account list is still loading, right after the cloud approved: the grant
/// was already kept with the attempt, so it is revoked exactly once, with this request's ids. A new
/// request is only made after the revocation finished.
@MainActor @Test(.timeLimit(.minutes(1)), arguments: ["cancel", "expire", "retire", "retry"])
func endingWhileTheListLoadsRevokesTheGrantOnce(ending: String) async throws {
    let h = ConfirmHarness("held-\(ending)")
    defer { h.finish() }
    try await h.request()
    let poll = try #require(h.accounts.accessPolls[h.id])
    await h.approveHoldingList()
    #expect(h.accounts.confirmations[h.id]?.grantId == "grant_fixture")
    #expect(h.accounts.accessConfirm.isEmpty)
    #expect(h.phase.grantDeletes.isEmpty)

    switch ending {
    case "cancel":
        await h.accounts.cancelAccess(h.id)
        #expect(h.phase.deletes == 1)
    case "expire":
        h.update { $0.now = 4_102_444_800_001 }
        h.releaseList()
    case "retire":
        h.accounts.retire()
    default:
        h.update { $0.macApproves = false; $0.holdRevoke = true }
        let retry = Task { try await h.accounts.requestAccess(h.id) }
        await h.revokeIsHeld()
        // The revocation reached the cloud and hasn't answered: the new request waits for it.
        #expect(h.phase.calls == ["create", "revoke"])
        h.releaseRevoke()
        try await retry.value
        await h.nextPoll()
        #expect(!h.phase.createdWhileRevokeHeld) // No request was created before the revocation finished.
    }
    await poll.value // The old poll ends on its own, whatever the held list does.
    await h.revocationsSettled()
    #expect(h.phase.grantDeletes == [h.grantPath])
    #expect(h.accounts.accessConfirm.isEmpty)
    #expect(h.savedNothing)
    if ending == "retry" {
        #expect(h.phase.calls == ["create", "revoke", "create"])
    } else {
        #expect(h.accounts.pendingAccess.isEmpty)
        #expect(h.accounts.confirmations.isEmpty)
    }
    if ending == "expire" { #expect(h.accounts.lastError == "O pedido expirou. Peça acesso novamente.") }
}

/// An approval whose identity checks out but arrives already past its deadline (the approval's own, or
/// the ticket's): the grant is kept first, then the request ends and the grant is revoked exactly once.
@MainActor @Test(.timeLimit(.minutes(1)), arguments: ["approval", "ticket"])
func approvalArrivingPastItsDeadlineRevokesTheGrantOnce(deadline: String) async throws {
    let h = ConfirmHarness("late-\(deadline)")
    defer { h.finish() }
    try await h.request()
    let poll = try #require(h.accounts.accessPolls[h.id])
    h.update {
        $0.macApproves = true
        if deadline == "approval" { $0.approvalExpiresAt = 500 } else { $0.nowWhenApproved = 4_102_444_800_001 }
    }
    h.clock.advance()
    await poll.value // Ends on its own: a late approval is not waited on again.
    await h.revocationsSettled()
    #expect(h.phase.grantDeletes == [h.grantPath])
    #expect(h.accounts.pendingAccess.isEmpty)
    #expect(h.accounts.accessConfirm.isEmpty)
    #expect(h.accounts.confirmations.isEmpty)
    #expect(h.accounts.lastError == "O pedido expirou. Peça acesso novamente.")
    #expect(h.savedNothing)
}

/// The checks after the list fail (a list that failed, the computer listed twice, another sign key):
/// the approved request ends, nothing saved, and its grant is revoked exactly once.
@MainActor @Test(.timeLimit(.minutes(1)), arguments: ["listError", "duplicated", "signKey"])
func failedCheckAfterTheListEndsTheRequestAndRevokesOnce(check: String) async throws {
    let h = ConfirmHarness("after-list-\(check)")
    defer { h.finish() }
    try await h.request() // The list used to ask is the real one; only the one after approval differs.
    let poll = try #require(h.accounts.accessPolls[h.id])
    h.update {
        $0.macApproves = true
        switch check {
        case "listError": $0.listStatus = 500
        case "duplicated": $0.listDuplicated = true
        default: $0.listedSignKey = Data(repeating: 8, count: 32).base64URL
        }
    }
    h.clock.advance()
    await poll.value
    await h.revocationsSettled()
    #expect(h.phase.grantDeletes == [h.grantPath])
    #expect(h.accounts.pendingAccess.isEmpty)
    #expect(h.accounts.accessConfirm.isEmpty)
    #expect(h.accounts.confirmations.isEmpty)
    #expect(h.accounts.lastError == AccessConfirmText.text(.unusable))
    #expect(h.savedNothing)
}

/// The six languages of the "type the Mac's code here" line.
private let typeOnPhoneLocales = ["pt-BR", "en", "es", "fr", "zh-Hans-CN", "ar-AE"]

/// Home banner (RootView): the code to type on the Mac while the Mac hasn't approved, also while the
/// approval is being checked; once the Mac shows its own code, where to type it; gone with the request.
@MainActor @Test(.timeLimit(.minutes(1))) func homeBannerLineFollowsTheConfirmationStep() async throws {
    let h = ConfirmHarness("banner-line")
    defer { h.finish() }
    let pt = Locale(identifier: "pt-BR")
    try await h.request()
    let ticket = try #require(h.accounts.pendingAccess[h.id])
    let old = "Digite no Mac o código \(ticket.code.prefix(3)) \(ticket.code.suffix(3))"
    #expect(h.accounts.accessBannerDetail(ticket, locale: pt) == old)

    await h.approveHoldingList() // Approved, still being checked: no code step yet.
    #expect(h.accounts.accessConfirm.isEmpty)
    #expect(h.accounts.accessBannerDetail(ticket, locale: pt) == old)
    h.releaseList()
    await h.nextPoll()
    #expect(h.accounts.accessConfirm[h.id] != nil)
    #expect(h.accounts.accessBannerDetail(ticket, locale: pt) == "Digite no iPhone o código que apareceu no Mac")
    for locale in typeOnPhoneLocales {
        let line = h.accounts.accessBannerDetail(ticket, locale: Locale(identifier: locale))
        #expect(line == AccessConfirmText.text(.typeOnPhone, locale: Locale(identifier: locale)), "\(locale)")
        #expect(!line.contains(ticket.code))
    }

    await h.accounts.cancelAccess(h.id)
    #expect(h.accounts.pendingAccess.isEmpty) // The banner lists pending requests only: it's gone.
    #expect(h.accounts.accessBannerDetail(ticket, locale: pt) == old) // A stale ticket never reads as the code step.
}

/// Settings row (SettingsView): no line without a request; the code to type on the Mac while the Mac
/// hasn't approved; once the Mac shows its own code, where to type it; none after the request ends.
@MainActor @Test(.timeLimit(.minutes(1))) func settingsRowLineFollowsTheConfirmationStep() async throws {
    let h = ConfirmHarness("settings-line")
    defer { h.finish() }
    let pt = Locale(identifier: "pt-BR")
    #expect(h.accounts.accessRowDetail(h.id, locale: pt) == nil)
    try await h.request()
    let ticket = try #require(h.accounts.pendingAccess[h.id])
    #expect(h.accounts.accessRowDetail(h.id, locale: pt) == "Digite no Mac o código \(ticket.code.prefix(3)) \(ticket.code.suffix(3))")

    await h.approve()
    #expect(h.accounts.accessConfirm[h.id] != nil)
    #expect(h.accounts.accessRowDetail(h.id, locale: pt) == "Digite no iPhone o código que apareceu no Mac")
    for locale in typeOnPhoneLocales {
        #expect(h.accounts.accessRowDetail(h.id, locale: Locale(identifier: locale))
            == AccessConfirmText.text(.typeOnPhone, locale: Locale(identifier: locale)), "\(locale)")
    }

    await h.accounts.cancelAccess(h.id)
    #expect(h.accounts.accessRowDetail(h.id, locale: pt) == nil)
}

/// A 409 when asking again shows the "already approved" phrase in each of the six languages, decided
/// by the status alone: the cloud's own text never shows.
@MainActor @Test(.timeLimit(.minutes(1))) func conflictOnNewRequestShowsAlreadyApprovedInSixLanguages() async throws {
    let h = ConfirmHarness("conflict")
    defer { h.finish() }
    h.update { $0.createStatus = 409 }
    await h.accounts.refreshCloud()
    var caught: (any Error)?
    do {
        try await h.accounts.requestAccess(h.id)
    } catch {
        caught = error
    }
    let error = try #require(caught)
    #expect((error as? CloudError)?.code == "deviceAlreadyApproved")
    let expected = [
        "pt-BR": "Este iPhone já foi aprovado nesse Mac, mas não terminou a ligação. No Mac, em Ajustes › Computadores e dispositivos, remova este iPhone e peça de novo.",
        "en": "This iPhone was already approved on that Mac, but the connection wasn't finished. On the Mac, in Settings › Computers and devices, remove this iPhone and ask again.",
        "es": "Este iPhone ya se aprobó en ese Mac, pero la conexión no terminó. En el Mac, en Ajustes › Ordenadores y dispositivos, quita este iPhone y vuelve a pedir acceso.",
        "fr": "Cet iPhone a déjà été approuvé sur ce Mac, mais la connexion n’a pas abouti. Sur le Mac, dans Réglages › Ordinateurs et appareils, retirez cet iPhone et redemandez l’accès.",
        "zh-Hans-CN": "这台 iPhone 已在那台 Mac 上获得批准，但连接没有完成。请在 Mac 的“设置 › 电脑和设备”中移除这台 iPhone，然后重新申请。",
        "ar-AE": "تمت الموافقة على هذا الـ iPhone في جهاز Mac هذا، لكن الربط لم يكتمل. على جهاز Mac، من «الإعدادات › الحواسيب والأجهزة»، أزِل هذا الـ iPhone ثم اطلب من جديد.",
    ]
    for (locale, text) in expected {
        let shown = AccessConfirmText.requestErrorText(error, locale: Locale(identifier: locale))
        #expect(shown == text, "\(locale)")
        #expect(!shown.contains("already has access"))
    }
    // Another failure keeps its own fixed phrase.
    #expect(AccessConfirmText.requestErrorText(CloudError(status: 429, code: "rateLimited", message: "remote text"))
        == "Muitos pedidos em pouco tempo. Tente de novo daqui a pouco.")
    #expect(h.phase.grantDeletes.isEmpty)
}

/// A request id outside `^[A-Za-z0-9_-]{1,64}$` ends the request before the reveal: no code, nothing saved.
@MainActor @Test(.timeLimit(.minutes(1))) func invalidRequestIdFailsClosedBeforeReveal() async throws {
    let h = ConfirmHarness("bad-request-id")
    defer { h.finish() }
    h.update { $0.requestId = "req.bad" }
    await h.accounts.refreshCloud()
    do {
        try await h.accounts.requestAccess(h.id)
        Issue.record("An invalid request id must end the request")
    } catch let error as CloudError {
        #expect(error.code == "invalidAccessResponse")
    }
    #expect(!h.phase.revealed)
    #expect(h.accounts.pendingAccess.isEmpty)
    #expect(h.accounts.confirmations.isEmpty)
    #expect(h.savedNothing)
}

/// The confirmation step's words exist in the product's six languages; others fall back to Portuguese.
@Test func confirmTextsCoverTheSixLanguages() {
    let locales = ["pt-BR", "en", "es", "fr", "zh-Hans-CN", "ar-AE"]
    for key in AccessConfirmText.Key.allCases {
        let texts = locales.map { AccessConfirmText.text(key, attemptsLeft: 3, locale: Locale(identifier: $0)) }
        #expect(texts.allSatisfy { !$0.isEmpty && !$0.contains("%d") })
        // English, Chinese and Arabic are never the Portuguese fallback (Spanish may share a word).
        #expect(texts[1] != texts[0] && texts[4] != texts[0] && texts[5] != texts[0], "\(key)")
        #expect(Set(texts.dropFirst()).count == 5, "\(key)")
    }
    #expect(AccessConfirmText.text(.prompt, locale: Locale(identifier: "pt-BR")) == "O Mac mostrou um código. Digite-o aqui")
    #expect(AccessConfirmText.text(.oldMac, locale: Locale(identifier: "pt-BR")) == "Se o Mac não mostrou um código, atualize o Mexerica no Mac.")
    #expect(AccessConfirmText.text(.wrong, attemptsLeft: 2, locale: Locale(identifier: "ar-AE")).contains("2"))
    #expect(AccessConfirmText.text(.prompt, locale: Locale(identifier: "de-DE")) == AccessConfirmText.text(.prompt, locale: Locale(identifier: "pt-BR")))
}

/// "Approved" with a host nonce that isn't the one the code was computed with: the approval is refused
/// before anything is kept, nothing is saved or revoked, and the request stays pending (the checks run
/// again on the next answer). A sign key that differs in the list after approval ends the request:
/// `failedCheckAfterTheListEndsTheRequestAndRevokesOnce`.
@MainActor @Test(.timeLimit(.minutes(1)), arguments: ["hostNonce"])
func approvedWithMismatchedHostNonceOrSignKeySavesNothing(mismatch: String) async throws {
    let host = "mismatch-\(mismatch.lowercased())-\(UUID().uuidString.lowercased()).test"
    let suite = "Mismatch.\(mismatch).\(UUID().uuidString)"
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite); ApprovedCloudStub.remove(host) }
    let config = URLSessionConfiguration.ephemeral
    config.protocolClasses = [ApprovedCloudStub.self]
    let cloud = CloudClient(baseURL: URL(string: "https://\(host)")!, identity: DeviceIdentity(signing: .init(), push: .init()),
                            session: URLSession(configuration: config)) { "synthetic-token" }
    let storage = SharedStore.Context(accountID: "account", suite: suite)
    let accounts = AccountStore(storage: storage, cloud: cloud) { computer in
        Issue.record("A mismatched approval must not open the computer")
        return BotStore(computer: computer, storage: storage) { _ in FakeRemote(.connecting) }
    }
    let clock = ApprovalClock()
    accounts.waitForApprovalPoll = { try await clock.wait() }
    accounts.nowMs = { ApprovedCloudStub.phase(host).now }
    var arrivals = clock.arrivals.stream.makeAsyncIterator()
    defer { accounts.retire(); clock.finish() }
    await accounts.refreshCloud()
    let id = RelayCrypto.computerId(signKey: hostKey)
    try await accounts.requestAccess(id)
    _ = await arrivals.next()
    #expect(accounts.pendingAccess[id]?.requestId == "req_ok")

    ApprovedCloudStub.update(host) {
        $0.macApproves = true
        $0.statusNonce = 8
    }
    for _ in 0..<2 {
        clock.advance()
        _ = await arrivals.next() // The "approved" answer was processed and refused; the poll waits again.
        #expect(storage.computers.isEmpty)
        #expect(accounts.stores.isEmpty)
        #expect(accounts.pendingAccess[id]?.requestId == "req_ok")
        #expect(accounts.accessConfirm.isEmpty) // Never reaches the step that asks for the Mac's code.
        #expect(ApprovedCloudStub.phase(host).grantDeletes.isEmpty)
    }
}

/// The ticket runs out while the account list is loading, after the Mac approved:
/// the request ends with a message, nothing is saved, the screen doesn't stay on "approved", and the
/// grant the approval made is revoked once.
@MainActor @Test(.timeLimit(.minutes(1))) func ticketExpiringDuringListCheckEndsTheRequest() async throws {
    let host = "expiring-\(UUID().uuidString.lowercased()).test"
    let suite = "ExpiresWhileListing.\(UUID().uuidString)"
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite); ApprovedCloudStub.remove(host) }
    let config = URLSessionConfiguration.ephemeral
    config.protocolClasses = [ApprovedCloudStub.self]
    let cloud = CloudClient(baseURL: URL(string: "https://\(host)")!, identity: DeviceIdentity(signing: .init(), push: .init()),
                            session: URLSession(configuration: config)) { "synthetic-token" }
    let storage = SharedStore.Context(accountID: "account", suite: suite)
    let accounts = AccountStore(storage: storage, cloud: cloud) { computer in
        Issue.record("An expired request must not open the computer")
        return BotStore(computer: computer, storage: storage) { _ in FakeRemote(.connecting) }
    }
    let clock = ApprovalClock()
    accounts.waitForApprovalPoll = { try await clock.wait() }
    accounts.nowMs = { ApprovedCloudStub.phase(host).now }
    var arrivals = clock.arrivals.stream.makeAsyncIterator()
    defer { accounts.retire(); clock.finish() }
    await accounts.refreshCloud()
    let id = RelayCrypto.computerId(signKey: hostKey)
    try await accounts.requestAccess(id)
    _ = await arrivals.next()
    #expect(accounts.pendingAccess[id] != nil)
    ApprovedCloudStub.update(host) { $0.macApproves = true; $0.nowWhenListed = 4_102_444_800_001 }

    clock.advance()
    await accounts.accessPolls[id]?.value // The poll ends on its own once it sees the expiry.
    #expect(accounts.pendingAccess[id] == nil)
    #expect(accounts.lastError == "O pedido expirou. Peça acesso novamente.")
    #expect(storage.computers.isEmpty)
    #expect(accounts.stores.isEmpty)
    await accounts.grantRevocations[id]?.value
    #expect(ApprovedCloudStub.phase(host).grantDeletes == ["/v1/computers/\(id)/grants/grant_fixture"])
}

/// The approval (not the ticket) runs out while the account list is loading: nothing is saved, the
/// request ends with a message and the grant the approval made is revoked once.
@MainActor @Test(.timeLimit(.minutes(1))) func approvalExpiringDuringListCheckSavesNothing() async throws {
    let host = "approval-expiring-\(UUID().uuidString.lowercased()).test"
    let suite = "ApprovalExpires.\(UUID().uuidString)"
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite); ApprovedCloudStub.remove(host) }
    let config = URLSessionConfiguration.ephemeral
    config.protocolClasses = [ApprovedCloudStub.self]
    let cloud = CloudClient(baseURL: URL(string: "https://\(host)")!, identity: DeviceIdentity(signing: .init(), push: .init()),
                            session: URLSession(configuration: config)) { "synthetic-token" }
    let storage = SharedStore.Context(accountID: "account", suite: suite)
    let accounts = AccountStore(storage: storage, cloud: cloud) { computer in
        Issue.record("An expired approval must not open the computer")
        return BotStore(computer: computer, storage: storage) { _ in FakeRemote(.connecting) }
    }
    let clock = ApprovalClock()
    accounts.waitForApprovalPoll = { try await clock.wait() }
    accounts.nowMs = { ApprovedCloudStub.phase(host).now }
    var arrivals = clock.arrivals.stream.makeAsyncIterator()
    defer { accounts.retire(); clock.finish() }
    await accounts.refreshCloud()
    let id = RelayCrypto.computerId(signKey: hostKey)
    try await accounts.requestAccess(id)
    _ = await arrivals.next()
    #expect(accounts.pendingAccess[id]?.requestId == "req_ok")
    // Valid when the status arrives (now 1000 < 5000); the list moves the clock to 6000, past the
    // approval but far from the ticket's expiry.
    ApprovedCloudStub.update(host) { $0.macApproves = true; $0.approvalExpiresAt = 5_000; $0.nowWhenListed = 6_000 }

    clock.advance()
    await accounts.accessPolls[id]?.value // The poll ends on its own once it sees the approval ran out.
    #expect(ApprovedCloudStub.phase(host).now == 6_000) // The list was consulted.
    #expect(storage.computers.isEmpty)
    #expect(accounts.stores.isEmpty)
    #expect(accounts.pendingAccess[id] == nil)
    #expect(accounts.lastError == "O pedido expirou. Peça acesso novamente.")
    await accounts.grantRevocations[id]?.value
    #expect(ApprovedCloudStub.phase(host).grantDeletes == ["/v1/computers/\(id)/grants/grant_fixture"])
}

@MainActor @Test func signedOutConfiguredAccountDoesNotOpenSavedLocalComputers() {
    let suite = "LoginGate.\(UUID().uuidString)"
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let storage = SharedStore.Context(accountID: nil, suite: suite)
    let computer = Computer(id: RelayCrypto.computerId(signKey: hostKey), name: "Local", signKey: hostKey.base64URL)
    storage.computers = [computer]
    let accounts = AccountStore(storage: storage, cloud: nil, loadSavedComputers: false) { computer in
        Issue.record("Signed-out account must not open saved computers")
        return BotStore(computer: computer, storage: storage) { _ in FakeRemote(.connecting) }
    }
    accounts.nowMs = { 1_000 }
    defer { accounts.retire() }
    #expect(accounts.computers.isEmpty)
    #expect(accounts.roster.isEmpty)
    #expect(storage.computers == [computer])
}

@MainActor @Test(arguments: ["configured", "cloud", "account"])
func accountStoreRejectsQRBeforeNetworkOrStorage(mode: String) async throws {
    let suite = "QRGuard.\(UUID().uuidString)"
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let storage = SharedStore.Context(accountID: mode == "account" ? "account" : nil, suite: suite)
    let cloud = mode == "cloud" ? CloudClient(baseURL: URL(string: "https://qr-guard-\(UUID().uuidString.lowercased()).test")!, identity: nil) { "synthetic" } : nil
    let accounts = AccountStore(storage: storage, cloud: cloud, accountConfigured: mode == "configured")
    accounts.nowMs = { 1_000 }
    defer { accounts.retire() }
    var link = URLComponents(string: "donnabot://pair")!
    link.queryItems = [
        "v": "3", "id": RelayCrypto.computerId(signKey: hostKey), "name": "Mac",
        "sk": hostKey.base64URL, "bk": hostKey.base64URL,
        "code": Data(repeating: 1, count: 16).base64URL, "urls": "http://127.0.0.1:1"
    ].map { URLQueryItem(name: $0.key, value: $0.value) }
    let pairing = try Pairing.parse(link.url!, environment: .main)
    do {
        _ = try await accounts.pair(pairing, deviceName: "Test", platform: "ios")
        Issue.record("A configured account must reject QR pairing")
    } catch let error as CloudError {
        #expect(error.code == "accountRequired")
    }
    #expect(storage.computers.isEmpty)
    #expect(accounts.stores.isEmpty)
}
